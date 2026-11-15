# bzz v0.15.0 — Local agents

**Status:** Release candidate (2026-11-14). Not published: publishing is the
owner's decision. See [`validation-v0.15.0.md`](validation-v0.15.0.md) for
what was proven, how, and what is still open.

bzz has shown agents and talked to them since v0.11, and since v0.13 it has
held an agent's key for a process you run yourself. It has never run an
agent. v0.15.0 makes bzz **create and run agents on your machine**, and lets
you **move** one to another machine.

An agent is three things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **key** | in bzz, sealed to your own key | the model and its tools never see it |
| Its **memory and disk** | two SQLite files in the agent's own folder | nothing of your machine is in them; a move is a copy |
| Its **hands** | nowhere, until you grant a folder, a host or a folder for commands | never by default |

The protocol baseline is unchanged: Buzz
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Rust stays at 1.98.1. The agent
host is built on `@earendil-works/pi-durable` 1.5.0. The plan is
[`planning/2026-11-13/v0.15.0.md`](planning/2026-11-13/v0.15.0.md), the
owner's guide is [`agents.md`](agents.md), and the reasons are in
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md).

## Highlights

- **Create an agent with one command (M4).** `bzz agent new`, or `:agent new`
  in the TUI, asks for a name, a model, instructions, who may wake it and
  what it may touch, and then makes the agent all or nothing: its key, its
  folder, its profile marked as a bot and your record of it, which is what
  Buzz lists an agent from. `bzz agent add Scout general` puts it in a
  channel. If any step fails, nothing is left behind.
- **bzz runs it and signs for it (M1, M2).** The agent answers while bzz is
  open, or under `bzz agent run` without the TUI. bzz decides who may wake it
  (you only, listed people, or anyone) and signs what it says. **A mention
  gets one final reply, whatever is killed and whenever:** the reply is
  signed and stored before it is published, and published again unchanged
  after a crash. The agent holds no key and no relay connection.
- **Its own disk, with git (M3).** A `pi-durable` agent works on a disk of
  its own, one SQLite file, with a shell and a `git` that start no program of
  your machine. It sees a folder of yours only when you grant it
  (`--folder site=/path`, read-only unless `:rw`), reaches only the hosts you
  allow, and clones a private repository with a token you stored for that
  agent and that host (`bzz agent credential set`).
- **Hands on your machine, by your yes (M7).** Grant a folder with `:exec`
  and the agent may run real commands there. Each one waits for you: a line
  at the top of every screen, `Ctrl-Y` to read the whole command exactly as
  the shell will get it, `a` to allow, `r` to refuse with a reason.
- **Move it (M5).** `bzz agent move` seals the key, the memory, the disk and
  the settings in one file with a passphrase; `bzz agent receive` takes it in
  another bzz of yours, where the conversation goes on. `bzz agent
  launch-kit` writes a folder from which Buzz's own `buzz-acp` runs the agent
  on a machine without bzz, and `bzz agent shutdown` stops that copy. The bzz
  an agent left refuses to run it.
- **Four harnesses (M6).** `pi-durable` is bzz's own and the default. An
  agent can also run on a coding program you have installed and signed in
  to: Grok (supported), Codex and Claude Code (both **experimental**). bzz
  sets each one's guard itself, answers every permission request by rule and
  never with "always", and prints what each can and cannot touch before you
  confirm.
- **The agents panel.** `:agents` shows each agent's state, what it is doing
  now, how its last turn ended, its settings and its capability record.
  `Space` pauses, `x` stops the running turn, `i` adds it to the open
  channel, `l` shows its log, `n` creates another.
- **The agent directory.** It no longer requires the `bot` role: a member
  of a channel whose profile carries its owner's valid authorization is
  listed as an agent and can be mentioned as one.

## What each harness can and cannot do

| | `pi-durable` | `grok` | `codex` (experimental) | `claude-code` (experimental) |
|---|---|---|---|---|
| What v0.15.0 promises | everything in this release | supported | experimental | experimental |
| Runs | bzz's own host on Node.js | your `grok` | your `codex`, through an adapter | your `claude`, through an adapter |
| Versions bzz starts | the host inside bzz | 1.0.77 and later below 1.1.0 | 0.166.0 and later below 0.167.0 | 2.1.322 and later below 2.2.0 |
| Its own disk; a shell and git that start no program | yes | no: a real folder and your real shell | no | no |
| Reads | its disk; folders you grant | everything you can read, except bzz's key folder | everything you can read | its folders; with a folder for commands, what a command reads |
| Writes | its disk; folders granted `:rw` | its folders, `~/.grok`, temporary folders | its folders, temporary folders | its folders |
| Who holds that | bzz's host | Grok's own sandbox | Codex's own sandbox, and bzz's answers | **only bzz's answers**: it has no sandbox |
| Real commands | only in a folder granted `:exec`, each after your yes | without asking, inside its sandbox | without asking inside its sandbox; to leave it, by `:exec` and your yes | none, unless `:exec`; then by your yes |
| A list of hosts, git credentials from bzz, Pi packages | yes | no | no | no |
| Survives a restart | its conversations and its disk | the program's own saved sessions | the program's own saved sessions | the program's own saved sessions |
| A move with its memory | yes | no | no | no |
| One final reply per mention, who may wake it | bzz | bzz | bzz | bzz |

A sandbox that holds an agent's writes does not keep it from reading. A Grok
or a Codex agent can read your other files and bzz's own database of your
messages, and whoever may wake it can ask it to. For every coding harness
bzz says: *it runs a real shell as your user; bzz never gives it the key;
bzz cannot promise the key is out of reach.* `bzz agent show <name>` prints
the record of one agent.

## Requirements

- **bzz without agents is unchanged: one binary.** Nothing below is needed
  unless you create an agent.
- **Node.js 22.19 or newer, with npm,** for a `pi-durable` agent and for the
  adapters of Codex and Claude Code. bzz uses the first `node` on your `PATH`
  that is new enough, or the one `BZZ_NODE` names.
- **The npm registry, once.** The agent host is inside the bzz binary. The
  first time an agent needs it, bzz writes it to its data folder and runs
  `npm ci --ignore-scripts --omit=optional --omit=dev` from a lock file that
  is also inside the binary (176 packages). The adapters are installed the
  same way when a Codex or Claude Code agent first needs one.
- **A model.** `pi-durable` uses Pi's own sign-in (`pi`, then `/login`): bzz
  stores no provider key. A coding harness uses the sign-in you made in that
  program.
- **Grok on Linux** needs bubblewrap (`bwrap`).
- `bzz agent doctor` reports all of this and installs nothing.

## Fixes

Found while the release was closed:

- `bzz broker list`, `allow`, `rotate`, `pause`, `resume` and `delete` acted
  on an agent bzz runs as well. `broker delete` would have removed its key
  and left its record on the relay, its folder and its credentials behind.
  They now leave such an agent alone and say to use `bzz agent`.
- Codex writes its own warnings as message text. A Codex turn that ended
  with one of them and nothing else would have published the warning as the
  agent's answer. It is no longer taken for the model's text.
- When an agent's process died in the middle of a turn, the process bzz
  started in its place could be left without bzz's record of it, and a
  later bzz would not have found it. Each process now removes only its own
  record.
- The agent directory read every membership of the community for each
  agent it listed. An index (migration 0019) answers that now.

## Compatibility and upgrade

- Database migrations 0018 (the agent runtime: requests, stored replies,
  sessions) and 0019 (an index of memberships by member, for the agent
  directory) run automatically, with the usual backup made before them. A real
  v0.14.0 profile upgrades with its history, Inbox, search, drafts and
  sidebar sections intact (`scripts/upgrade-test.sh`).
- **A hosted agent of v0.13 is untouched.** It loads as it was saved, bzz
  never starts a process for it, and the agent broker serves it with the
  credential and the permissions it had. `bzz broker` is unchanged for an
  agent whose process you run.
- A local agent is a `[[hosted_agents]]` entry with a `[hosted_agents.local]`
  table ([`configuration.md`](configuration.md)). No existing setting
  changes.
- The agent directory looks at plain members too. A refresh now asks the
  relay for the profiles of channel members whose profile is not stored yet,
  at most 2000 a refresh, 100 to a request. With the new index, listing
  fifty agents among 5,000 members takes 1 ms where v0.14.0 took 25 ms, and
  a mention of an agent is checked in 0.1 ms where it took 24 ms. Bringing
  the directory up to date when a member list arrives costs about 4 ms more
  than in v0.14.0 in a community of that size (30 ms against 27 ms).
- Benchmarks against v0.14.0 for a profile without agents are within 5 %
  but one: cleaning 64 KiB of text for display takes 0.20 ms where it took
  0.17 ms, with the same source
  ([`validation-v0.15.0.md`](validation-v0.15.0.md), "Benchmarks against
  v0.14.0").
- New Rust dependency: `close_fds` 0.3.2 (MIT), so that a harness is handed
  no open descriptor but its three streams. bzz's own memory is kept from
  other processes of your user on Linux while it runs agents.
- Not part of the binary, installed on first use from committed lock files,
  with install scripts off and optional packages left out: the agent host's
  packages, and for the experimental harnesses
  `@agentclientprotocol/codex-acp` 2.5.0 (24 packages) and
  `@agentclientprotocol/claude-agent-acp` 0.101.0 (107 packages).
  [`THIRD_PARTY_LICENSES.md`](../THIRD_PARTY_LICENSES.md) lists them and
  their licenses. One of them is not open source: the Claude Code adapter
  needs `@anthropic-ai/claude-agent-sdk`, which is under Anthropic's own
  terms. It is installed only when you create a `claude-code` agent.

## Security notes

- **The key stays in bzz.** No agent key, owner key or relay credential is
  placed in a harness's environment, arguments, files or tool results. A
  harness is started with an environment built from nothing. The secret scan
  covers every launch, the sealed file of a move and the launch kit.
- **A real command runs as you.** The folder is where it starts, not a wall.
  It is approved in the TUI of the bzz that runs the agent, and nowhere else:
  not in the chat, where whoever may wake the agent could answer for you.
  Under `bzz agent run` there is nobody to ask and the answer is no.
- **A coding harness is your own program at work on your own machine.**
  Read the table above, and choose who may wake such an agent with care.
- **A launch kit holds the agent's key in the clear,** in one file, because
  under `buzz-acp` the key sits beside the agent. bzz says so and asks before
  it writes one. A file made by `agent move` is sealed with your passphrase
  (Argon2id, XChaCha20-Poly1305); the passphrase is all that protects it.
- **A Pi package runs inside the agent's host with your rights,** outside
  the agent's sandbox. `--no-packages` switches them off for one agent.
- **Git credentials** are sealed to your key like the agent's key, given to
  the agent's host only for a host on that agent's list, and kept nowhere by
  the host.

Details: [`security.md`](security.md), "v0.15.0 trust boundaries".

## Known limits

What v0.15.0 does not do, and what was not seen. Every item is in
[`validation-v0.15.0.md`](validation-v0.15.0.md) with its reason.

**Where it ran**

- Agents were run on **Linux x86_64 only**. On macOS and Windows the code
  compiles and its unit tests pass in CI, and so do the agent host's own
  tests (on Windows without the eleven that need what Windows lacks). No
  agent was run there: not a turn, not a move, not a coding harness. On
  Windows the host offers no real commands, a launch kit does not run (it
  is a shell script and a systemd unit), and Grok has no sandbox.
- No test ran a **real model**: the tests may not read your Pi sign-in. The
  host answered through a real model once, in an experiment before the plan.
  A token refresh, a provider from a real Pi package, a usage limit and a
  missing sign-in were not seen with a real provider.
- The three coding programs were run at **one version each**, with short
  prompts, on Linux. Later versions inside the ranges were not run.
- Communities with **federated identity**: an agent enrolls with the adapter
  by the code the broker's agents use; no local agent ran against one.

**What Buzz 1.0 shows of an agent made by bzz** (seen with Buzz 1.0.1)

- Buzz 1.0 lists the agent under "Your agents", as "Harness unavailable":
  it cannot run it, edit it or delete it. It offers it for a mention, and a
  mention from Buzz 1.0 is answered once while bzz runs the agent.
- **The list is read when Buzz 1.0 starts.** An agent you create, or delete,
  shows there only after Buzz 1.0 has been started again.
- **Buzz 1.0 offers no direct message to a bzz agent.** Talk to it in a
  channel or a Solo chat.
- Buzz 1.0 is told nothing when you pause or move an agent: it shows the
  same card, and a mention gets no answer and no reason.
- **An identity paired with Buzz** (`bzz identity pair-desktop`) **cannot
  have agents.** `bzz agent new` refuses, and says so only after it has
  installed the agent host. An agent needs an authorization signed by its
  owner's key (NIP-OA), and bzz does not ask Buzz to sign one. Use an
  identity whose key bzz holds.

**The agent's disk**

- Its `git` has no `rebase`, no `stash`, no merge with conflicts and no SSH.
  Credentials are HTTP Basic over HTTPS only, and there is none for `curl`.
  No real forge was used in a test: the private remote was on the test
  machine.
- It has no compiler, no package manager and no Python. A command's output
  arrives when the command ends.
- Cloning a 40 MB repository took about 400 MB of memory. Repositories and
  files of hundreds of MB were not tried.
- A Pi package that never gives the thread back holds the agent's host. bzz
  stops a host that does not start within 30 s; the agent stays unavailable
  until the package is switched off. A package that ends the process ends
  the host.
- A folder you grant is checked, then used: another program of your machine
  that swaps a link in between can win. The agent itself cannot.
- The install of first use was tested for its failures with a stand-in for
  `npm`: not behind a proxy, not offline, not with a full disk.

**Real commands**

- A command is not confined to its folder, and there is no list of commands
  that need no approval: a folder asks each time, or never.
- Approval is in the TUI only. A question waits five minutes.
- A program that detaches itself (`setsid`, a daemon) outlives its command.
- A terminal too small to show the head of the dialog cannot allow a
  command there. A letter that looks like another is not marked; the dialog
  only says when a command holds characters outside ASCII.
- Long work (a build of minutes, a test suite) was not run.
- If the agent's host and the process that keeps its command are killed at
  the same moment, the command goes on.

**Moving**

- A move is cold. Nothing keeps two places in step, and **nothing stops two
  copies of one key from running**: bzz only refuses to run what it moved
  away. An older file of the same agent opens, and puts an older memory back.
- Under `buzz-acp`: a reply can be repeated if the machine dies between the
  reply and its record; a conversation in a Solo chat is not found again; an
  edited mention is answered at the edit; and a message that imitates Buzz's
  own prompt can make the copy answer another message of the same channel.
- The launch kit was run on one machine, with a folder standing for the
  other one. Its service unit was never started by systemd.
- A copy started again within about five seconds of `bzz agent shutdown`
  hears that command again and leaves: `buzz-acp` reads a few seconds back
  when it starts. The kit's service unit restarts only after a failure, so
  it does not do that by itself.
- Under `buzz-acp` a real model is given the whole of Buzz's prompt, which
  tells it to reply with a `buzz` command it does not have. The host posts
  through its own tool; what a real model makes of that text was not seen.
- The largest file sealed in a test is 3 MiB. An agent of hundreds of MB
  was not moved.

**Coding harnesses**

- Codex and Claude Code are experimental. They have no disk of their own, no
  list of hosts, no git credential from bzz and no move.
- Your own settings of Grok and of Codex still apply: MCP servers, hooks,
  rules and plugins. Whether an allow rule in your own Claude settings could
  get past bzz was not tried; bzz starts Claude Code with none of them.
- A key that your operating system keeps for you is reachable by any program
  of your user, and so by a coding harness.
- With the real programs these were not seen: a turn stopped half way, a
  usage limit, a missing sign-in, two conversations at once, a command of
  Codex or Claude Code that you allowed, and whether Codex asks leave for
  the tools of every MCP server of your own.
- After the last run with the real programs, bzz stopped taking Codex's own
  warnings for the model's text (see "Fixes"). That change was tested with
  Codex's recorded messages and against the adapter's source, not with
  Codex itself.

**Running**

- A bzz that is running keeps the settings it started with, but for what you
  take away. `bzz agent pause` and `resume` reach a running bzz at its next
  start; `Space` in `:agents` is immediate.
- A harness process keeps every conversation it opened until it restarts.
- Nothing stops you from running one agent from two data folders.
- A paused agent, and an agent nobody runs, do not answer later what was
  said meanwhile.
- `bzz agent delete --force` with the relay out of reach is written and was
  never run. Switching community in the TUI while agents run was not tried.
- An agent you add to a channel while bzz is open may not be offered by
  the mention chooser until the directory is refreshed: open the directory
  (`:agents directory`, or `Tab` in `:agents`) and press `r`, or start bzz
  again. A name the chooser did not complete is plain text and wakes nobody.
- An answer made in the same second as its mention can be listed above it
  in the thread pane: a fast stand-in model showed it.
- The line that says a command waits takes the first row of the screen,
  over the title of whatever is open. `Ctrl-Y` is not in the `?` help: it
  is bzz's only while a command waits, and the line names it.
