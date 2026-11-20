# bzz v0.16.0 — Apps in the channel

**Status:** Release candidate, not published. Nothing was pushed, tagged
or released. See [`validation-v0.16.0.md`](validation-v0.16.0.md) for what
was proven, how, and what is still pending; M8 there is the release
candidate.

Since v0.15 bzz runs agents, and they could only talk. v0.16.0 lets **an
agent write a small app for a channel, which bzz draws beside the
conversation, and whose records the people and agents of the channel
share**. You read and approve every version's exact bytes before it runs.

An app is four things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **script** | one Lua file in bzz's data folder, on the machine of whoever installed it | it runs only after its owner approved its bytes, with only what bzz lends it |
| Its **brief** | a short text kept with the script | it says what the app does and what its records hold |
| Its **records** | artifacts of the channel, on the relay | bzz signs them; the script never sees a key, and reaches only the record types it declared |
| Its **pane** | a region of bzz's window, at the right of the conversation | bzz draws it; the app has no terminal of its own |

The protocol baseline is unchanged: Buzz
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Rust stays at 1.98.1. The
interpreter is Lua 5.4.9, compiled into bzz through `mlua` 0.12.2. The plan
is [`planning/2026-11-17/v0.16.0.md`](planning/2026-11-17/v0.16.0.md), the
guide is [`apps.md`](apps.md), the contract a script is written against is
[`app-api.md`](app-api.md), and the reasons are in
[`adr-v0.16-apps.md`](adr-v0.16-apps.md).

## Highlights

- **An app beside the conversation (M1, M3).** `:app open <name>` draws an
  app at the right of the selected channel, 40 columns by 16 rows inside a
  border. `5` gives it the keyboard; while it has it, every key is the
  app's but `Ctrl-c`, which gives the keyboard back. `Space 5` zooms it.
  Below 72 columns it takes the window while it has the keyboard, and with
  a thread open both show from 144 columns. A paste reaches it as one piece
  of text.
- **Nothing runs that you did not approve (M4).** Every version, also one
  you add yourself, waits for your answer. You are shown who it comes from
  (you, your agent and who asked it, or a person of the channel), its
  hash, its brief, its record types with any change marked `CHANGED`, its
  size and its difference from the version that runs. Approval is of the
  exact bytes, by their SHA-256; a file changed behind bzz's back is not
  run. At the shell: `bzz app add`, `pending`, `approve`, `refuse`; in the
  TUI, a line at the top of every screen and `Ctrl-y`, as for an agent's
  command.
- **A new version changes the open app in place.** An approval, at the
  shell or in the TUI, reaches the open app without a restart, and the app
  keeps its state: the selection, a search, an open form. A version that
  fails as it is put to run leaves the one before running. `bzz app
  rollback` goes back to the exact bytes you approved before.
- **Your agents write and change apps (M5).** Ask an agent in a channel
  for an app, or for a change to the one open there ("add a priority and a
  filter to the board"). It reads the guide and the contract, tries its
  script apart and sees the screen it draws, and proposes it; you approve
  it, and it is told at its next turn what became of it. It never writes a
  file, and nothing it wrote runs before you approve. Every harness has the
  tools: Pi Durable, Grok, Codex and Claude Code. Nothing about apps is in
  an agent's prompt until it asks.
- **Agents use an app's records (M7).** An agent woken in a channel can
  list, create, change and delete the records of the apps open there,
  without a screen: it reads each app's brief to know what they mean. bzz
  signs with the agent's own key and holds it to the app's record types.
- **Share an app in the channel (M6).** `:app share` offers the app open
  in a channel as one artifact, signed by you, and posts its brief as a
  message. Another bzz in the channel shows the offer with who signed it;
  nothing of it runs there before that person approves its exact bytes. A
  later version is a new offer. `:app unshare` withdraws it; installed
  copies stay.
- **Shared records (M2).** Two people with the app open see each other's
  changes within a second. A change made at the same moment by two people
  is not lost: the second is told of the conflict with the record as it is
  now, and the app decides what to do (the Board merges). A resync, which
  the relay asks for on ordinary chat events, never shows as records
  deleted and made again. The records are there after a restart before
  the relay answers.
- **Three templates (D9).** `bzz app new` writes a Board, a Poll or an
  Approvals app to start from, each fitting the pane and each with its
  brief. `bzz app check` loads a script and reports its errors with file
  and line, `bzz app try` prints the screen it draws, and `bzz app dev`
  runs your own folder in the terminal and reloads it on every save
  (`:app dev` does the same in a channel's pane, on its real records).
- **Paste works in the composer.** bzz now enables bracketed paste. Before,
  a pasted line break sent the message and the rest of the paste ran as
  bzz's keys.

## What a script can and cannot do

| A script can | A script cannot |
|---|---|
| compute, with Lua 5.4 and its `table`, `string`, `math` and `utf8` libraries | read or write a file, open a connection, start a process, read the environment, load code or bytecode |
| return elements for bzz to draw in its pane | write to the terminal, or draw outside its pane |
| react to keys, a paste, a resize and a timer | see a key while its pane has no focus, or `Ctrl-c` |
| list, create, change and delete records of the types it declared, in its channel | touch another type, another channel, a message, or another app |
| ask bzz for the name of a person (`host.name`) | sign anything, or see a key or a credential |
| use 64 MiB and 250 ms a call | hold bzz: a call that runs too long is stopped, memory past the cap is an error in the script, and an app stopped three times in a row is given up, in its own pane |

The whole list is one document, [`app-api.md`](app-api.md), marked
`api = 1`; bzz refuses a script written for another number and says which
it knows.

## Requirements

- **Nothing to install.** The interpreter is inside the binary. Apps need
  no Node, no network and no account beyond your community.
- **Your agents** need what they needed in v0.15.0 to write apps: Node.js
  22.19 or newer for a `pi-durable` agent.
- **A build without apps:** the cargo feature `apps` is on by default;
  `--no-default-features` builds bzz without the interpreter and without
  Lua's C.

## Fixes and changes beyond apps

- **Bracketed paste** (above): a paste into the composer is one piece of
  text, line breaks included, in every terminal that supports it. Windows
  has no paste event in the terminal library: there a paste still arrives
  as keys.
- **A thread asked for is drawn from 74 columns**, and the channel list
  gives way; before, between 74 and 101 columns an open thread could be
  left undrawn. The composer replies into a thread only while it is drawn.
- **The artifact panel** keeps the last complete list and says `too many
  artifacts here to sync · list may be stale` when the relay ends a
  channel's sync over its 4096 bound. Before, it asked again every minute
  or so for ever. It no longer lists an app's records
  (`bzz.app.<app>.<record>`); it lists an offered app (`bzz.app`) as `an
  app offered here` and says what a rename or a delete there does.
- **A name bzz cuts to the ends of a key** is cut on characters: an id
  that is not hex can no longer stop bzz.
- **The capability record of a `pi-durable` agent** names the release it
  is built from (`everything in v0.16.0`).

## Compatibility and upgrade

- **The schema goes from 20 to 23.** Migrations 0021 (the installed apps,
  their versions and where each is open), 0022 (the apps this bzz shared
  and the offers it took in) and 0023 (who asked an agent for a version,
  and whether the agent was told what became of it) run automatically,
  with the usual backup made first, next to the database:
  `bzz.pre-migration-<time>.bak`. They only add tables and columns. A real
  v0.15.0 profile, with local agents, their requests and replies, what
  they asked of their owner and the channel's artifacts, opens with all of
  it as v0.15.0 left it (`scripts/upgrade-test.sh`), and a reply v0.15.0
  stored and never saw accepted is published once, unchanged.
- **Going back to v0.15.0** means restoring that backup: v0.15.0 refuses a
  database at schema 23. Quit bzz, copy `bzz.pre-migration-<time>.bak`
  over `bzz.db` (`bzz paths` prints where), and start v0.15.0. What
  happened since the upgrade is not in it: messages come back from the
  relay, but the record of installed apps does not, and `<data>/apps/` is
  left as it was. An app's records stay on the relay either way.
- **The agent host changed** (the tools of apps), so every owner of a
  `pi-durable` agent installs it again on first use, from the lock file
  inside the binary, as with any change to `agent-host/`.
- **New Rust dependencies:** `mlua` 0.12.2 (MIT), pinned exactly, with
  `mlua-sys`, `lua-src` (Lua 5.4.9, MIT) and `bstr`; `luajit-src` and
  `which` are used by `mlua-sys`'s build only. `memchr`, locked before,
  is now a direct dependency. `cargo deny check` and `cargo audit` pass.
  The interpreter adds about 0.4 MiB to the binary.
- **An app's records cost relay quota.** The relay gives a person 60
  writes a minute, chat included. bzz gives the apps of one person 30 of
  them, all apps together, and answers the rest itself with the seconds to
  wait, so that an app cannot keep you from sending a message. An agent's
  record writes count against the agent, up to 60 a minute.
- **Benchmarks** against v0.15.0 for a profile with no app:
  [`validation-v0.16.0.md`](validation-v0.16.0.md), M8, "Benchmarks". One
  open Board on 200 cards is drawn in about 56 µs a frame at the pane's
  size, a key reaches its screen in about 0.4 ms, and it adds about 2 MiB
  of memory (one run, Linux x86_64).

## Security notes

- **bzz runs code it did not ship.** A script is interpreted inside bzz's
  process, by Lua's C. The script's reach is the interpreter's and bzz's
  library's, not the operating system's: it has no file, network, process,
  environment, native code or key, on every platform. M0 and M1 ran 80
  hostile scripts and called every function a script can reach with
  thousands of argument lists; every one ended within its budget. A memory
  bug in Lua 5.4.9 or in `mlua` would now be a bug in bzz.
- **Approval is of exact bytes, whoever wrote them.** Read the record types
  a version declares and their change: what it can do with your identity is
  create, change and delete records of those types in its channel.
- **An app offered in a channel is another person's script.** Anyone who
  can write in the channel can publish a new revision of an offer, which
  bzz then shows as coming from them, and the approval says so plainly.
- **An app's rules are its script's, not the relay's.** Any member of the
  channel, in any client, can write any record of an app. A rule such as
  "you cannot decide your own request" holds only among people who run
  the app.
- **No key reaches a script or an agent.** The secret scan runs every
  `bzz app` command, a script that looks for a key everywhere it can
  reach, and each of the four harnesses through the tools of apps.

Details: [`security.md`](security.md), "v0.16.0 trust boundaries".

## Known limits

What v0.16.0 does not do, and what was not seen. Every item is in
[`validation-v0.16.0.md`](validation-v0.16.0.md) with its reason.

**Where it ran**

- **Apps were run on Linux x86_64 only.** The branch was not pushed before
  this candidate, so macOS, Windows and Linux on arm64 have never compiled
  this code; CI runs the unit and scripted tests of apps there when it is
  pushed. The confinement is the interpreter's and does not depend on the
  system, but the time a call is given and Lua's C library were measured
  here only.
- **On Windows a paste arrives as keys**, in an app as in the composer: the
  terminal library (crossterm 0.29) gives no paste event there.
- **An agent's tests run on Unix only**, as in v0.15.0. On macOS and
  Windows no agent has written an app or used a record.
- The apps bench (the six apps in a real terminal) runs on Linux only; the
  same cases run everywhere as unit tests.
- The release binary was driven through tmux, not Herdr. The Linux
  console, Windows Terminal and macOS Terminal were not tried.

**Models**

- **No real model wrote an app.** The tools, the guide and the gate are
  proven with the scripted harness, on the pinned relay and on the release
  binary. Whether a real model, from the tools' descriptions alone, finds
  the guide, writes a version that passes the bench's cases and keeps its
  brief true is the gate `scripts/test-agent-authors-app.sh <harness>`,
  which runs only with the owner's own sign-ins (plan D13). It has not run.

**Buzz desktop** (seen with 1.0.1)

- It shows nothing of an app, an offer or a record, and is not harmed by
  them. People there see only the brief, posted as a message. A newer
  Buzz desktop, and an agent run by Buzz desktop in a channel with an app,
  were not tried. bzz ships no Buzz desktop plugin.

**The pane**

- One app is shown in a channel at a time. Four stay running while you
  look elsewhere; the one shown least recently is closed.
- A click gives the app the keyboard and nothing else: an app has no mouse.
  The wheel over it does nothing.
- An app shows the name of a person bzz has a profile of; others show as
  the ends of their key. bzz does not fetch a profile for an app beyond
  what it looks up for the timeline.
- What the pane says by itself at the quota of writes or in an archived
  channel, and a fault on an app's thread, were seen in unit tests, not on
  the release binary.

**Records**

- At most 1000 live records an app, 16,384 bytes of content a record, and
  30 writes a minute for all of a person's apps. A relay that does not
  serve the artifact sync, or a channel past 4096 records of one app,
  leaves the app saying so.
- Two people changing two fields of one record at the same moment: the
  service keeps both (seen on the pinned relay); what the Board does was
  seen for one field.
- A real drop of the connection under an open app was seen on the scripted
  relay only; on the pinned relay, a resync.
- A guest agent, a relay that requires membership with sharing, and two
  bzz processes of one owner changing the store of apps at once were not
  tried.

**Sharing**

- An offer is the revision and its signer: whoever can write in the
  channel can publish over it. In an open channel every relay member can
  offer.
- The owner who shared an app is not told who installed it.
- `bzz app share` does not exist: the command line does not connect to the
  relay. Use `:app share`.

**Agents**

- An agent is told what became of its version at its next turn in the
  channel, not at once: a turn bzz starts by itself would be a reply to no
  mention.
- An agent's version that does not load is not recorded: the agent is told
  the error and the owner is not asked.
- An agent proposes only a new app, an app open in the channel it was
  asked in, or one of which no version was approved yet.

**Pending acceptance**

- **The owner's production acceptance** (release acceptance 4): in one of
  his channels, an agent changes an app at his request and he approves it.
- The real-model gate of M5, above.
- CI on the four platforms and both relay jobs, which run when the branch
  is pushed.

## Not in this release

As the plan says (its "Explicit non-goals"), v0.16.0 does not:

- run Buzz desktop plugins, React, TypeScript or WebAssembly;
- install a script from Git or a URL, offer a catalogue, or apply a shared
  version without the receiver's approval;
- ship a Buzz desktop plugin that draws the same records;
- offer a declarative app format;
- give an app the mouse, images, sound, the network, files, timers finer
  than a tick, or calls into another app;
- open an app in a direct message;
- merge concurrent edits by itself: what a conflict means is the script's
  rule;
- make other clients obey an app's rules;
- show who else has the app open, or where their cursor is;
- change agents, harnesses or the broker beyond the new tools.

The follow-ups carried from v0.15.0 stay where they were.
