# bzz agent host

The process bzz starts for an agent that runs on the Pi Durable harness: one
Node process per agent. It holds the agent's memory and its disk, and speaks
the [agent host protocol](../docs/agent-host-protocol.md) (ACP over stdio) to
bzz. It holds no key and no relay connection: it acts on the chat only by
asking bzz. The reasons are in
[`adr-v0.15-local-agents.md`](../docs/adr-v0.15-local-agents.md).

Nothing here is Rust, and bzz without agents does not need it.

## Run and test

Node 22.19 or newer. No package runs an install script.

```sh
npm ci --ignore-scripts          # from package-lock.json
npm test                         # about 40 s; no network, model, relay or Buzz
node main.mjs <agent folder>     # what bzz runs; the protocol is on stdin and stdout
```

`--omit=optional` also works and is the smaller install: it leaves out 28
packages that only matter with install scripts (native `xz` and `zstd`
bindings, one of them LGPL-3.0). The tests pass either way.

The git tests need `git` with `http-backend` on the machine and say so when
they are skipped. Everything else needs only Node. Tests write in temporary
folders and never read `~/.pi`, Buzz's data or a keychain.

## What is in an agent's folder

| File | What |
|---|---|
| `memory.sqlite` | Pi Durable's storage: conversations, unfinished work, and which conversation belongs to which place |
| `files.sqlite` | the agent's disk |
| `agent.json` | only in a moved copy: name, model and instructions |

Both SQLite files hold virtual paths only (`/home/agent/…`, `/mnt/<name>/…`),
so a move is a copy of the files. A write-ahead file (`-wal`) sits beside each
while the host works. `_bzz/checkpoint` and a clean exit leave none.

## Source

| File | What |
|---|---|
| `main.mjs` | arguments, the log, leaving when the parent is gone |
| `src/host.mjs` | the ACP face: sessions, places, turns, tools, checkpoint |
| `src/rpc.mjs` | JSON-RPC, one message per line |
| `src/model.mjs` | Pi's login file and settings; the stand-in model switch |
| `src/remote.mjs` | the remote copy: `agent.json`, buzz-acp's prompt, the `buzz` command |
| `src/disk.mjs` | the disk in SQLite |
| `src/granted.mjs` | one granted folder of the machine, and `/mnt` |
| `src/vfs.mjs` | the disk and the granted folders as one tree |
| `src/adapters.mjs` | that tree as Node's `fs` (for git) and as the shell's filesystem |
| `src/env.mjs` | the tree and the shell as a Pi Durable environment |
| `src/git.mjs` | the `git` command |
| `src/network.mjs` | the allow-list and the one way out to the network |

## What the model gets

- **Chat tools**, served by bzz over the pipe (`_bzz/tool`): `send`, `read`
  and `react`. The model sees them as `buzz_send`, `buzz_read` and
  `buzz_react`, because Pi's file tools already use the name `read`.
- **Pi's own tools** `read`, `write`, `edit` and `bash`, on the agent's disk.
- A short section in its system prompt that names its granted folders and the
  hosts it may reach.

## The sandbox

The agent's world is `files.sqlite`. Its shell is
[just-bash](https://github.com/vercel-labs/just-bash), an interpreter inside
this process, and its `git` is
[isomorphic-git](https://isomorphic-git.org): **no program of the machine is
started for the agent**, and the shell's environment is its own, not this
process's.

What it gives:

- A filesystem with directories, symbolic and hard links, and content stored
  in 256 KiB chunks named by SHA-256, so equal content is kept once. Each
  call is one transaction: a kill leaves the last whole call. `check()` in
  `disk.mjs` tells whether the file agrees with itself.
- A shell with more than 80 built-in commands (`cat`, `grep`, `sed`, `awk`, `jq`,
  `find`, `tar`, `curl`, …), pipes, redirections, loops and functions.
- `git`: `init`, `clone`, `status`, `add`, `rm`, `commit`, `log`, `diff`,
  `branch`, `checkout`, `remote`, `config`, `fetch`, `merge`, `push`, `pull`,
  `rev-parse`. What real git reads back from it is tested with real git.
- **Granted folders**: a folder of the machine appears at `/mnt/<name>` when
  the owner grants it, read-only unless he allows writes. The shell, git and
  Pi's tools see the same thing, because all three go through `vfs.mjs`.
  Every path is resolved to where it really leads, links included, and
  refused unless that is inside the granted folder. Errors name the virtual
  path, never the real one.
- **A network allow-list**: `git` and `curl` send through one function that
  lets a request through only to a listed host, at every hop of a redirect.
  An entry is a host (`github.com`) or `host:port`; it covers that host only,
  over HTTPS. Plain HTTP is accepted only to this machine itself.

What it does not give:

- **No other programs.** No compiler, no package manager, no Python, no
  `node`. Only the built-in commands exist.
- **git is a subset.** No `rebase`, no `stash`, no `cherry-pick`, no
  submodules, no merge with conflicts (a conflict changes nothing and says
  so), and **no SSH remotes**: HTTPS only. A missing command says what to do
  instead.
- **No credentials for private remotes.** The protocol has no message yet by
  which bzz hands one over at the moment of use.
- **Output arrives when a command ends**, not while it runs. A long command
  shows nothing until then.
- **A clone is held in memory.** Cloning bzz's own repository (39 MB to
  download, 465 files) took 7.8 s and 396 MB of memory at its peak; the disk
  file was 80 MB afterwards. A repository of several hundred MB needs
  `--depth` or will not fit.
- **No quota.** Nothing limits how large `files.sqlite` grows.
- **The shell is an interpreter.** A loop of thousands of steps takes
  noticeable time.
- In a granted folder: a change made by another program is not reported to
  watchers; a file hard-linked from outside the folder is written through
  that link; and a program of the machine that swaps a link between the
  host's check and its use can win that race. The agent itself cannot.
- A redirection into a path that refuses the write (`echo x > /mnt/site/f` on
  a read-only grant) ends the whole command line, not only that command.

## Models

The model comes from `_meta.bzz.model` and from Pi's own folder
(`_meta.bzz.piDir`): `auth.json` for the sign-in, read and refreshed under the
lock the `pi` command takes, and `settings.json` for the default model when
bzz names none. bzz stores no provider key.

A turn whose model cannot be used fails with a message that says why, and
`error.data.kind` names it: `provider_unavailable` (not one of Pi's built-in
providers), `model_unavailable`, `sign_in_needed` or `no_model`.

Not supported: providers that come from a Pi extension (Buzz's host loads the
ones that have a Pi Durable entry; this host loads none yet), and `!command`
or `$NAME` keys in `auth.json` (this host starts no program and has no such
environment).

## As a remote copy

Started without bzz's fields in `initialize`, by Buzz's `buzz-acp` on another
machine, the host reads `agent.json`, takes the place and the request from the
text of buzz-acp's prompt, and posts with the `buzz` command. Buzz puts the
agent's key in this process's environment there; the host passes its
environment to `buzz` and to nothing else, and never logs it. A remote copy
has no granted folders and an empty allow-list: `agent.json` carries neither.

## Environment

bzz builds the host's environment from nothing. These are read when present:

| Variable | Use |
|---|---|
| `PI_CODING_AGENT_DIR` | Pi's folder, for a remote copy. Default `~/.pi/agent` |
| `BZZ_AGENT_HOST_BUZZ` | remote copy: the `buzz` command, as a path or a JSON array. Default `buzz` on the `PATH` |
| `BZZ_AGENT_HOST_RECORD` | remote copy: a folder where each prompt of buzz-acp is saved, to record `tests/fixtures/agents/buzz-acp-prompts` again |
| `BZZ_AGENT_HOST_SCRIPTED_MODEL` | tests only: a module that plays the model, as the provider `scripted`. Never set by bzz |

## Windows

The code uses no POSIX-only call, and CI runs the tests on Windows. Three
things differ there:

- bzz stops the host by closing its input. There is no TERM signal to catch.
- Whether the parent is gone is found by probing its process id, which
  Windows can give to a new process. The test of that path is skipped on
  Windows, so it is unproven there.
- File modes (`0600` on the two SQLite files) change little; Windows keeps
  its own access rules.
