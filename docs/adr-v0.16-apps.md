# ADR: v0.16 apps

**Status:** Proposed. Its decisions are those of the plan the owner approved
on 2026-11-17 ([`planning/2026-11-17/v0.16.0.md`](planning/2026-11-17/v0.16.0.md),
D1 to D17, rules 14 to 18), written out here with what milestone M0 found.
Three sections are open until M0's questions 3, 4 and 5 are answered; they
are marked **TBD**. Narrows one line of
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

## Records on the relay: TBD (M0 question 3)

To be written from the answer to question 3: the rule for a record type's name on the pinned
relay; how bzz syncs only the types of one app; what bzz does near the 4096
live artifacts a channel can have in one sync; whether a script with its
manifest and brief fits one artifact, and the limit bzz sets on it.

## What Buzz desktop does with these records: TBD (M0 question 4)

To be written from the answer to question 4: whether the artifacts of an app show, clutter or
break anything in a Buzz desktop that shares the channel, and what bzz does
about it.

## The pane and its keys: TBD (M0 question 5)

To be written from the answer to question 5: at which window widths conversation, thread and app
fit side by side, which of them gives way below that, and which key leaves
the app.

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
