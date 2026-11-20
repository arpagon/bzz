# ADR: v0.16 apps

**Status:** Accepted and built, in the release candidate of v0.16.0
(2026-11-20; not published). Every decision below holds as written; what
the milestones settled that it left open, and the one place where the
build went another way than it says, are in "What was decided while it
was built", at the end. Its decisions are those of the plan the owner approved
on 2026-11-17 ([`planning/2026-11-17/v0.16.0.md`](planning/2026-11-17/v0.16.0.md),
D1 to D17, rules 14 to 18), written out here with what milestone M0 found.
Narrows one line of
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md): "a JavaScript
engine inside bzz" was rejected there, and bzz now embeds an interpreter. The
section "Why this is not the engine v0.15 ruled out" says what differs and
what does not.

**Date:** 2026-11-18.

## Context

Since v0.15 bzz runs agents, and they can only talk. The owner wants an agent
to write a small **app** for a channel: bzz draws it beside the conversation,
and the people and agents of the channel share its records. The owner's
example is a task board to which an agent adds a priority and a filter while
the board is open.

For the first time **bzz runs code it did not ship**. An agent writes it, or
a person of the channel offers it. Three rounds of experiments chose the form
(`runs/experiments/bzz/react-terminal-plugins-2026-11-16/`, local and
untracked; the plan's table "What was tried first" is their summary), and M0
measured what the chosen form can and cannot be held to
([`validation-v0.16.0.md`](validation-v0.16.0.md), M0).

## Decision

1. **What is run.** One Lua 5.4 file per app, as source text. It is
   interpreted inside bzz by Lua 5.4.9, compiled from C into the binary
   through `mlua` 0.12.2 (cargo feature `apps`, on by default). bzz loads
   text only: bytecode is refused, because Lua does not check it. Nothing is
   compiled, installed or downloaded to run an app.
2. **Where it runs.** One interpreter for each open app, on a thread of its
   own. bzz's own thread never waits for a script: it sends events and takes
   back a tree of widgets, which bzz itself lays out and draws. An app has no
   terminal.
3. **With what reach: only what bzz lends** (rule 14). A script gets Lua's
   `table`, `string`, `math` and `utf8` and the base functions, without the
   ones that load code or write to the terminal (`load`, `loadfile`,
   `dofile`, `require`, `string.dump`, `print`, `warn`, `collectgarbage`),
   and the functions of the contract, `docs/app-api.md`. `io`, `os`,
   `package`, `debug` and `coroutine` are never loaded. So a script has no
   file, no network, no process, no environment variable, no clock finer than
   the contract gives, no native code and no other app to reach. Everything
   it can do is a function bzz wrote.
4. **A script never sees a key, and reaches only its own records** (rule 17).
   It asks bzz to create or change a record; bzz checks that the record's
   type is one the script declared and that the channel is the one the app
   is open in, and signs. Who may read and write is the relay's rule for the
   channel.
5. **Nothing runs that the owner has not approved** (rule 16). Approval is of
   exact bytes. The owner is shown where a version comes from (an agent, or a
   person of the channel), its brief, the record types it declares and its
   difference from the running version. A version that fails to load leaves
   the previous one running. Only the owner's own folder, run with `bzz app
   dev`, needs no approval (D5).
6. **A script cannot stop bzz** (rule 15). Every call of a script has a time
   budget and every interpreter a memory cap.
   - **Memory.** An allocation past the cap is an error in the script. The
     interpreter stays usable.
   - **Time.** A count hook looks at the clock every 1000 instructions. Past
     the deadline it makes the coroutine of the call **yield**, and bzz never
     resumes it. A yield is not an error: no `pcall` of the script can
     swallow it, and no handler of the script runs because of it.
   - **The bounded library.** A hook runs between instructions of the script.
     It does not run inside one call of a C function, and script code that a
     C function calls cannot yield. In Lua's own library M0 found ten
     functions in which one call never returns, and three places where
     script code runs with the hook switched off (a finalizer, a message
     handler, the handler of a to-be-closed variable). So a script does not
     get sixteen functions as Lua ships them: pattern matching is done in
     Rust with the deadline, and the others are written in Lua, where the
     hook sees every step; `__gc` is refused. After that **no function a
     script can call loops longer than its arguments are large, and none
     calls script code from C**. The list, with the test of each, is in the
     validation document.
   - **One way in.** Every call of bzz into a script goes through one
     function, inside the call's coroutine, that hands an error back as a
     string. bzz reads what a script returns, and a script's tables, without
     calling the script's metamethods. Otherwise bzz itself would run script
     code where it cannot yield: `mlua` calls the `__tostring` of an error
     value to make its message.
   - **The second and third line.** Where a yield changes nothing the hook
     raises an error instead. And because the interpreter is on its own
     thread, an app that still does not answer is shown as not responding in
     its own pane while bzz goes on; it can be closed and replaced. A thread
     cannot be killed: such an app would hold one processor core until bzz
     quits. No such call is known after the bounded library.
7. **The contract has a number** (rule 18). A script says `api = 1`; bzz
   refuses a number it does not know and says which it knows.
8. **An agent proposes, it does not write.** An agent gets tools to read an
   app, to try a script and see the screen it draws, and to propose a
   version. The proposal opens the owner's approval. No agent has a folder to
   write a script to.
9. **A script travels in the channel only as an offer** (D3, M6). Its bytes,
   manifest and brief are one artifact signed by whoever offers it. Another
   bzz shows the offer and runs nothing until that person approves the exact
   bytes. Nothing is fetched from anywhere else.
10. **The same on four platforms** (D10). The confinement is the
    interpreter's and the library's, not the operating system's, so it does
    not differ between Linux, macOS and Windows.

## What a script can and cannot do

| A script can | A script cannot |
|---|---|
| compute, with Lua 5.4 and four of its libraries | read or write a file, open a connection, start a process, read the environment |
| return widgets for bzz to draw in its pane | write to the terminal, or draw outside its pane |
| react to keys, pasted text, a resize, a timer tick | see keys while its pane has no focus, or the key bzz keeps to leave it |
| list, create, change and delete records of the types it declared, in its channel | touch another type, another channel, a message, or anything that is not a record |
| be wrong: its error is shown in its pane, with file and line | sign anything, or see a key, a credential or another app |
| use memory up to its cap and time up to its budget | hold bzz, or go on after it was stopped |

What an author loses against stock Lua 5.4, all found or decided in M0:
`load` and bytecode; coroutines; `__gc`; the message handler of `xpcall`
runs after the error has unwound, not at the point it was raised (a script
has no `debug` library to look at the stack with); `table.sort` is stable
and never reports an "invalid order function"; `table.unpack` gives fewer
than a million values; an error message of a library function names the
function without the adjustment Lua makes for a method call (`s:rep()`).
Everything else of the four libraries behaves as in Lua: three million
random calls of the replaced functions gave the same results as Lua's own.

## Why this is not the engine v0.15 ruled out

The v0.15 ADR rejected "a JavaScript engine inside bzz, or a port of Pi
Durable to Rust". That was about how to run an **agent**: Pi Durable is a
Node program with packages, a model sign-in, a disk and the network. Running
it inside bzz would have meant building Node's world inside bzz, and putting
the owner's packages, which run with his rights, in the process that holds
his keys.

An app differs in what it is given, not in what language it is written in:

- **No ambient authority.** A JavaScript engine that runs a harness must
  hand it files, sockets and processes, or the harness does not work. A
  script is handed nothing: its whole world is the functions of one document.
- **A library small enough to read.** M0 read the 77 functions a script
  gets, one by one, and called each with ten thousand argument lists. That
  cannot be done for a JavaScript runtime and its packages.
- **It decides nothing by itself.** A script draws and asks. bzz signs,
  fences and approves. A harness holds a model's sign-in and runs for
  minutes; a script runs for milliseconds per key.
- **It is small.** About 30,000 lines of C and no JIT, no foreign-function
  interface, no bytecode from outside.

What does **not** differ, and stays true: it is an interpreter written in C,
in bzz's own address space, parsing text an agent or a stranger wrote. A
memory-safety bug in Lua 5.4.9 or in `mlua` is now a bug in bzz. `unsafe`
stays forbidden in bzz's own code, but bzz's safety now rests on those two as
it already rests on SQLite and on the TLS library.

If apps are ever written in JavaScript, or an agent's harness is ever to run
inside bzz, the v0.15 line applies unchanged and must be reopened by name.

## Consequences

- bzz without the `apps` feature has no interpreter and none of Lua's C. It
  builds and passes its tests.
- Six crates are added (`mlua`, `mlua-sys`, `lua-src`, `bstr`, and, for the
  build only, `luajit-src` and `which`). bzz compiles C on its five release
  targets already (SQLite, the TLS library, libdbus); Lua is one more.
- `mlua` and Lua are pinned to exact versions. A new version of either is a
  change to review, with the hostile scripts run again: two of the things M0
  found are `mlua`'s (it runs bytecode unless told the chunk is text; and an
  error raised from a hook closes the to-be-closed variables of the hooked
  function while hooks are off), and bzz depends on going around both.
- The bounded library is code bzz must keep true to Lua: about 370 lines of
  Lua and 730 of Rust. M1 brings it into the repository with its
  differential test against Lua's own functions and its hostile scripts.
- A stopped call is abandoned in the middle. What it had changed of the app's
  own state stays changed. The contract has to say so.
- Rule 15 is kept for everything M0 found, and by a second and a third line
  for what it did not find. It is not proven that nothing else exists, and
  nothing of it has run on macOS or Windows yet.

## Records on the relay

From M0 question 3, run against the pinned relay
(`runs/experiments/bzz/v016-m0/q3-relay/RESULT.md`).

- **Names.** A record type is `bzz.app.<app>.<record>`; a shared script is an
  artifact of type `bzz.app`. The relay's rule is two or more parts
  separated by dots, each `[a-z][a-z0-9_-]*`, 128 bytes in all. bzz holds
  both names to `[a-z][a-z0-9_-]{0,31}` and checks them before sending,
  because a refused write costs quota as an accepted one does. bzz stays out
  of `buzz.*`.
- **Sync.** One subscription per open app, for its channel and the exact
  list of its types. The relay has no prefix filter. Heads arrive, then
  `EOSE`, then live heads; a deletion arrives as a head.
- **The limit.** One sync returns at most 4096 live artifacts and then
  closes with an error. It counts what the filter matches, so an app is
  limited by its own records, not by the channel's. bzz sets 1000 live
  records for an app and says so in the pane before the relay does.
- **A sync is closed by ordinary chat events** (a deleted message, a removed
  member) and must be asked again. The pane keeps what it shows until the
  new set has arrived; a script never sees a resync as records deleted and
  created.
- **Writes.** Every revision is a whole snapshot. The revision a script holds
  is the event id of the head. A stale revision is refused without the
  head; bzz takes the head from its sync, or reads it over HTTP.
- **Who writes.** Any member of the channel, an agent included, may revise
  or delete any record. The relay records only who signed the last
  revision. An app's rules are the script's, not the relay's.
- **Size.** Content up to 262,144 bytes and a frame up to 524,288. A script
  of 128 KiB with its manifest and brief travelled as one artifact. bzz
  refuses a script over 128 KiB and a brief over 16 KiB, on both sides.
- **The budget of writes.** The relay has one budget for a person, 60
  writes a minute, and that person's chat spends it too. bzz gives the
  apps of one person half of it, 30 a minute for all of them together,
  and answers the rest itself with the seconds to wait: an app cannot
  keep its owner from sending a message. bzz lets a record be 16,384
  bytes of content, so that the thousand an app may have fit the memory
  of its interpreter (M2, the session).

## What Buzz desktop does with these records

From M0 question 4 (`…/v016-m0/q4-buzz-desktop/RESULT.md`): Buzz desktop
1.0.1, in a disposable sandbox, in a channel with 1, 201 and 4,101 records
and a 36 KB script artifact.

It shows nothing of them: no list, tab, search result, unread mark,
activity entry or notification. It keeps nothing of them on disk and wrote
no artifact event. It did not get slower. So the names stay, and a person
on Buzz desktop cannot tell that a channel has an app: the brief, posted as
a message, is all they see. Archiving the channel refuses every record
write until it is unarchived. A newer Buzz desktop and an agent run by Buzz
desktop were not tried.

## The pane and its keys

From M0 question 5 (`…/v016-m0/q5-pane-keys/RESULT.md`), tried with a
stand-in pane on the pinned relay, through tmux.

- The pane is 42 columns with its border, 40 inside, at the far right. The
  conversation keeps 30.
- It docks from a window of 72 columns. Below that the app takes the whole
  window while it has the focus and is hidden otherwise.
- Thread and app show together from 144 columns (116 with the sidebar
  off). Below that, the one asked for last shows.
- An app of 16 rows needs a window of 22 rows docked, or 19 zoomed.
- `Ctrl+C` leaves the app. bzz does not quit on it, and the contract never
  gives it to a script. Every other key is the app's while it has the
  focus.
- A paste reaches the app as one piece of text. bzz did not enable
  bracketed paste before v0.16; it does now, for the composer too.

## Rejected

- **React in a pane of its own (Ink).** It worked, with 128 MiB and 356 ms to
  the first draw of a board, and it needs a terminal bzz does not own. React
  drawn by bzz through Ratatui did better on every measure.
- **React drawn by bzz.** It worked. Dropped with all of React: a Buzz
  desktop plugin does not travel to another client anyway, and agents wrote
  every option correctly at the first try, so the shape of Buzz's plugins
  bought nothing. It needs Node, and confines only in part.
- **A WebAssembly widget.** The only strict sandbox of the first round. Its
  author writes Rust and compiles it, and the runtime adds 76 crates and
  11.2 MB.
- **A declarative file, no code.** The cheapest for bzz, and enough for a
  board and a poll. It carried none of four new tasks completely, the owner's
  example among them.
- **TypeScript in Node, drawn by bzz.** It carried every task. It costs
  94 MiB for each app and needs Node, and its confinement is a list of what
  Node is told to refuse: with Node's own permissions alone a script could
  signal another process of the user.
- **Another embedded engine.** Measured beside Lua: `rhai` has no memory cap
  for the whole interpreter and an unmaintained dependency; `boa` (JavaScript)
  cannot be stopped by time or capped, and adds 12 MiB; QuickJS can be
  stopped everywhere, weighs more than twice Lua, and is the engine of the
  section above.
- **Lua's library as it ships, with only a hook** (the prototype). Of 80
  hostile scripts 32 were not stopped in time, 31 of them never.
- **Luau.** It stops a pattern by itself and has neither finalizers nor
  to-be-closed variables. It still needed two of the same replacements (a
  plain search of 4 MiB ran until it was killed; a sort with a C comparison
  ran 1.4 s against a budget of 200 ms), it adds a C++ runtime that bzz's
  Linux binary does not link today, and it changes the language the owner
  chose (D1) and the apps agents have written. It is the way out if Lua 5.4
  ever shows a call that cannot be closed.
- **A process for each app.** Killing a process stops anything, and a crash
  of the interpreter would not be bzz's. It costs a process for every open
  app and a pipe for every frame and record, and it is another design than
  the one approved. Kept as the last resort: the runtime's two ends already
  talk through messages.
- **Raising an error from the hook to stop a script** (the prototype). A
  `pcall` catches it, a message handler of `xpcall` then runs inside the
  hook where nothing stops it, and `mlua` closes the script's to-be-closed
  variables there too.
- **Removing what cannot be bounded instead of replacing it.** A script
  without `string.find`, `gsub` and `table.sort` is not one an agent can
  write a board in.

## What was decided while it was built

Each milestone's section of [`validation-v0.16.0.md`](validation-v0.16.0.md)
lists, under "What was decided here", what the plan and its brief left
open. The ones that shape the design:

| Where | Decided |
|---|---|
| Decision 4, for agents (M7) | **An agent's record is signed with the agent's key**, by bzz, on the agent's own connection and records service, not on the owner's open app. The fence is the same and is checked where bzz serves the tool: installed, enabled, approved, open in the turn's channel, of the approved types. An agent holds to 60 record writes a minute; the owner is not asked, as for a message |
| Decision 8 (M5) | An agent learns what became of its version **at its next turn in the channel**, once, in its prompt; bzz starts no turn for it. A version that does not load is answered to the agent at once and **not recorded**. An agent has at most three versions waiting, of all apps. `app_guide` returns `app-authoring.md` and `app-api.md` as one text (plan D17) |
| Decision 9 (M6) | **An offer is a revision and its signer.** Any writer of the channel can publish a new revision of someone's offer, which bzz then shows as coming from that person, with `Source: CHANGED` against the installed version's. A revision signed by this bzz's own identity is never offered to it. The brief is posted as a message once for each version shared. At most 3 offers of one person and 8 in one channel wait; bytes refused do not come back. The artifact panel lists an offer, as Buzz lets any member rename or delete it, and says what it is (M8) |
| Records (M2) | Content of 16,384 bytes, so that 1000 records fit an interpreter's memory; **30 writes a minute for all of a person's apps**, half of the relay's 60, answered by bzz itself past it; a list answered at once from what bzz holds; pushes of a type begin with the script's first list of it; a person is `{ id, name }`, and `host.name(id)` names an id (M4) |
| Storage and approval (M4) | **Files hold bytes, the database holds decisions**: a version's files are written once, and every read compares them with the hash recorded when they were proposed. The script, not the manifest, says `api` and `types`. A new version replaces the running one in place only if it loads, declares the approved types and runs `app.reload` and its first `app.view`; otherwise the version before goes on and the new one is recorded as failed. `Ctrl+Y` opens a waiting version as it opens an agent's command; no command approves by itself |
| The pane (M3) | An app's session lives on while its channel is not shown, four at most; `host.quit()` is `:app close`; a panic on an app's thread leaves the terminal alone. bzz enables bracketed paste for everyone; on Windows a paste is still keys. A thread asked for is drawn from 74 columns, the channel list giving way, with or without an app |
| `dev` (M4) | `bzz app dev <folder>` on records in memory; `:app dev <folder>` in a channel's pane on its real records, never remembered; `bzz app dev --channel` refused, since the command line cannot reach a running bzz |

None of these changed a decision of the plan or a rule of it. A later
change to any of them is a change to this record.
