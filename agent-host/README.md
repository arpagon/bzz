# bzz agent host

The process bzz starts for an agent that runs on the Pi Durable harness: one
Node process per agent. It holds the agent's memory and its disk, and speaks
the [agent host protocol](../docs/agent-host-protocol.md) (ACP over stdio) to
bzz. It holds no key and no relay connection: it acts on the chat only by
asking bzz. The reasons are in
[`adr-v0.15-local-agents.md`](../docs/adr-v0.15-local-agents.md).

By default the agent's world is its own disk, and no program of the machine
is started for it. Two things the owner can turn on reach beyond that, and
each is described below with what it costs: [Pi packages](#pi-packages),
whose code runs inside this process, and [`run`](#hands-on-this-machine-run),
which starts real commands in a folder he grants for it.

Nothing here is Rust, and bzz without agents does not need it.

## Run and test

Node 22.19 or newer. No package runs an install script.

```sh
npm ci --ignore-scripts          # from package-lock.json
npm test                         # about 45 s; no network, model, relay or Buzz
node main.mjs <agent folder>     # what bzz runs; the protocol is on stdin and stdout
```

`--omit=optional` also works and is the smaller install: it leaves out 28
packages that only matter with install scripts (native `xz` and `zstd`
bindings, one of them LGPL-3.0). The tests pass either way.

The git and credential tests need `git` with `http-backend` on the machine
and say so when they are skipped. The `run` tests start real commands
(`sh`, `sleep`, `seq`, `env`) in temporary folders and are skipped on
Windows. Everything else needs only Node. Tests write in temporary folders
and never read `~/.pi`, Buzz's data or a keychain.

## What is in an agent's folder

| File | What |
|---|---|
| `memory.sqlite` | Pi Durable's storage: conversations, unfinished work, and which conversation belongs to which place |
| `files.sqlite` | the agent's disk |
| `agent.json` | only in a moved copy: name, model, instructions, the hosts it may reach, and whether Pi packages are off |

Both SQLite files hold virtual paths only (`/home/agent/…`, `/mnt/<name>/…`),
so a move is a copy of the files. The one exception is the output of a real
command (`run`), which is kept as it was printed. A write-ahead file (`-wal`)
sits beside each while the host works. `_bzz/checkpoint` and a clean exit
leave none.

## Source

| File | What |
|---|---|
| `main.mjs` | arguments, the log, leaving when the parent is gone |
| `src/host.mjs` | the ACP face: sessions, places, turns, tools, checkpoint |
| `src/rpc.mjs` | JSON-RPC, one message per line |
| `src/model.mjs` | Pi's login file and settings; the stand-in model switch |
| `src/packages.mjs` | the Pi Durable entries of Pi packages: providers and extensions |
| `src/run.mjs` | the `run` tool: a real command in a granted folder, after bzz said yes |
| `src/run-child.mjs` | the keeper of one such command, a process of its own that ends the command's group |
| `src/remote.mjs` | the remote copy: `agent.json`, buzz-acp's prompt, the `buzz` command |
| `src/disk.mjs` | the disk in SQLite |
| `src/granted.mjs` | one granted folder of the machine, and `/mnt` |
| `src/vfs.mjs` | the disk and the granted folders as one tree |
| `src/adapters.mjs` | that tree as Node's `fs` (for git) and as the shell's filesystem |
| `src/env.mjs` | the tree and the shell as a Pi Durable environment |
| `src/git.mjs` | the `git` command, and how it asks bzz for a private remote's credentials |
| `src/network.mjs` | the allow-list and the one way out to the network |

## What the model gets

- **Chat tools**, served by bzz over the pipe (`_bzz/tool`): `send`, `read`
  and `react`. The model sees them as `buzz_send`, `buzz_read` and
  `buzz_react`, because Pi's file tools already use the name `read`.
- **Pi's own tools** `read`, `write`, `edit` and `bash`, on the agent's disk.
- **`run`**, only when the owner granted a folder for it
  ([below](#hands-on-this-machine-run)). Otherwise the tool does not exist
  and nothing tells the model of it.
- What the owner's Pi packages bring: tools, prompt sections, hooks.
- A short section in its system prompt that says what it has: its own disk,
  the granted folders and which of them it may write, the hosts it may
  reach, and, when it has `run`, where, and that the owner may be asked.

## The sandbox

The agent's world is `files.sqlite`. Its shell is
[just-bash](https://github.com/vercel-labs/just-bash), an interpreter inside
this process, and its `git` is
[isomorphic-git](https://isomorphic-git.org): **no program of the machine is
started for the agent** by `read`, `write`, `edit`, `bash` or `git`, and the
shell's environment is its own, not this process's. `run` is the one
exception, and it exists only by the owner's grant.

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
- **Private git remotes**: when a listed host asks who is calling (HTTP 401),
  `git` asks bzz (`_bzz/credential`) and uses the answer for that one
  command. Nothing is kept: the next command asks again. The answer is never
  written to either SQLite file, to the repository's configuration, to what
  the command prints, or to the log. A host that is not listed is never
  asked about. A turn that is cancelled does not wait for the answer.

What it does not give:

- **No other programs.** No compiler, no package manager, no Python, no
  `node`. Only the built-in commands exist.
- **git is a subset.** No `rebase`, no `stash`, no `cherry-pick`, no
  submodules, no merge with conflicts (a conflict changes nothing and says
  so), and **no SSH remotes**: HTTPS only. A missing command says what to do
  instead.
- **Credentials are for `git` only, and for the host in the remote's URL.**
  `curl` gets none. A remote that sends git to another host to sign in is not
  served: credentials do not follow a redirect to another host. A URL with
  credentials in it (`https://user:token@host/…`) still works as in any git,
  and is then stored in the repository's configuration by the agent's own
  doing: bzz's credentials never take that path.
- **A remote that hides a private repository behind "not found"** (HTTP 404
  without credentials, where git expects 401) never asks, so bzz is never
  asked: git reports what the remote said.
- **A remote copy has no credentials.** Under `buzz-acp` there is nobody to
  ask, and git says so.
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
`error.data.kind` names it: `provider_unavailable` (neither one of Pi's
built-in providers nor one of a loaded package), `model_unavailable`,
`sign_in_needed` or `no_model`.

Not supported: `!command` or `$NAME` keys in `auth.json` (this host starts no
program for a key and has no such environment).

## Pi packages

Extensions written for the `pi` command, which export `(pi) => { … }`, do not
load here: this host runs Pi Durable, not `pi`. A Pi package can also hold
parts written for Pi Durable, listed as `"pi": {"durable": ["./durable.ts"]}`
in its `package.json`, and those are loaded, as Buzz 1.0 loads them
(`src/bundled/pi/EXTENSIONS.md` in `block/buzz-app` shows how to write one).
Each entry's default export returns `{ extensions?, providers? }`:

- **Providers** join Pi's own, and a model of theirs is chosen like any
  other. A provider takes the place of one with the same id.
- **Extensions** (tools, prompt sections, hooks) are installed for the agent,
  after Pi's file tools and before bzz's own.

The packages are the ones listed under `packages` in `settings.json` of Pi's
folder, the list `pi install` writes to. The rules:

- **A package's code runs inside this process, with this process's rights.**
  It is not in the agent's sandbox: its tools can read and change what the
  owner's user can, and start programs. A tool it names `read`, `write`,
  `edit` or `bash` replaces Pi's. In a remote copy, under `buzz-acp`, the
  agent's key is in this process's environment and a package can read it.
  Install only packages you trust.
- **bzz can switch them off for an agent**: `_meta.bzz.packages: false` in
  `initialize`, and `"packages": false` in `agent.json` for a moved copy.
  Then no code of any package runs. The default is on.
- Only a package in a local folder is loaded: a path that is absolute, starts
  with `~/`, or is relative to Pi's folder. What `pi install` fetched from
  npm or git is left out.
- Each package has 10 seconds to load, and they load at the same time, so a
  package that hangs delays the agent's start by 10 seconds, once. A setup
  that never gives the thread back, such as an endless loop, cannot be timed
  out: it holds the whole host.
- A package that fails is left out, and the others still load. It fails when
  its setup throws or is too slow, when an extension cannot be installed or
  takes a name that is taken (`bzz`, `coding-tools`, another package's), when
  a tool has the name of one of bzz's own (`run`, `buzz_…`), or when a
  provider lacks an id, a name, `auth` or `getModels`, or lists no models.
- What loaded and what did not is in the answer to `initialize` and in the
  log. When a turn fails because its provider is not there, the error names
  each package that did not load and why, and says whether a package without
  a Pi Durable entry names that provider. That last part is a search of the
  package's files for the provider's id as a quoted string. Nothing in them
  is run.
- A package gets this host's copies of the `@earendil-works` packages, and
  may be TypeScript in the subset Node runs as is.
- What a package prints goes to the log. The pipe carries only the protocol.
- A change to a package or to the list takes effect when the host starts
  again.

## Hands on this machine: `run`

A granted folder may carry `"exec": true`. An agent with at least one such
grant gets one more tool, `run`, that starts a **real** command of the
machine in that folder. This is the one place where the host starts a
program for the agent, on purpose, and only by the owner's grant.

- The model gives the folder's name, the command as one line for `/bin/sh`,
  and optionally a time limit in seconds (default 120, at most 3600).
- **bzz is asked before anything starts** (`_bzz/approve`, with the folder
  and the command). Only `{"allow": true}` starts it. A refusal, an error or
  a cancelled turn starts nothing, and the reason goes back to the model as
  the tool's error. Whether the owner sees a question is bzz's to decide.
- **What was approved is what runs.** The command is one string. The host
  does not read it, quote it or add to it: the string bzz was asked about is
  the string the shell receives (`/bin/sh -c -- <command>`).
- It starts with the granted folder as its working directory, in an
  environment built from nothing: `PATH`, `HOME`, `LANG` and `TMPDIR` as this
  process has them, and nothing else of this process's environment. It has
  no terminal and its standard input is empty: a command that asks a
  question gets no answer.
- It runs in a process group of its own, watched by a small keeper process
  (`src/run-child.mjs`). The group is ended, TERM and then KILL after 2
  seconds, when the turn is cancelled, at the time limit, and when the host
  goes, however it goes: a host that is killed outright can do nothing, but
  the keeper sees its end of the pipe close.
- Nothing outlives the call: what the command left running in its group is
  killed when it returns.
- Its output, standard output and standard error together, comes back as the
  last 2000 lines or 50 KB. A longer output is saved on the agent's disk
  (`/tmp/run-<id>.log`, the first 8 MiB of it) and the result says where.
- A command cut off by a restart of the host is not started again. The model
  is told that it was interrupted.
- Two commands asked for in one answer of the model are put to bzz one after
  the other.
- Files go between the agent's disk and the folder with the agent's own
  tools (`cp`, `git`), which need the folder granted for writing to put
  something into it.

What it does not give, said plainly:

- **The folder is where a command starts, not a wall around it.** A real
  command has the rights of the owner's user: it can read and change
  whatever he can, anywhere, and reach the network without the allow-list.
  A grant with `exec` and without `write` holds the agent's file tools to
  reading, not its commands. The owner's approval of each command is the
  control.
- **A process that leaves the group is not ended**: one that starts a new
  session (`setsid`, a daemon). The call does not wait for it either.
- **If the host and the keeper are both killed at the same moment**, the
  command goes on. bzz's sweep of leftovers does not see it: it is not in the
  host's process group.
- A command's output is stored as printed, so paths of the machine can enter
  `memory.sqlite`, which a move carries.
- Output arrives when the command ends, as in the sandbox.
- A remote copy has no granted folders, so it never has `run`.

## As a remote copy

Started without bzz's fields in `initialize`, by Buzz's `buzz-acp` on another
machine, the host reads `agent.json`, takes the place and the request from the
text of buzz-acp's prompt, and posts with the `buzz` command. Buzz puts the
agent's key in this process's environment there; the host passes its
environment to `buzz` and to nothing else, and never logs it. A remote copy
has no granted folders, so no `run`, and no credentials for a private remote.
Its allow-list and the switch for Pi packages are the ones in `agent.json`;
Pi packages are read from Pi's folder on that machine.

## Environment

bzz builds the host's environment from nothing. These are read when present:

| Variable | Use |
|---|---|
| `PI_CODING_AGENT_DIR` | Pi's folder, for a remote copy. Default `~/.pi/agent` |
| `BZZ_AGENT_HOST_BUZZ` | remote copy: the `buzz` command, as a path or a JSON array. Default `buzz` on the `PATH` |
| `BZZ_AGENT_HOST_RECORD` | remote copy: a folder where each prompt of buzz-acp is saved, to record `tests/fixtures/agents/buzz-acp-prompts` again |
| `BZZ_AGENT_HOST_SCRIPTED_MODEL` | tests only: a module that plays the model, as the provider `scripted`. Never set by bzz |

## Windows

CI runs the tests on Windows. These things differ there:

- **`run` is not offered.** Windows has no process groups, and without them
  the host cannot promise that a command's whole tree is ended. A folder
  granted with `exec` gives no tool there; `_meta.bzz.run` is empty and the
  log says why. The `run` tests are skipped, so nothing about it is proven
  there.
- bzz stops the host by closing its input. There is no TERM signal to catch.
- Whether the parent is gone is found by probing its process id, which
  Windows can give to a new process. The test of that path is skipped on
  Windows, so it is unproven there.
- File modes (`0600` on the two SQLite files) change little; Windows keeps
  its own access rules.
