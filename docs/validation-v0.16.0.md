# bzz v0.16.0 validation

**Status:** in progress. Recorded below, each under its own heading: M0;
M1 (the runtime and its contract; the example host, the bench and the six
apps); M2 (the records service; the app session); M3 (the app pane); M4
(the store and command line; approval and managing inside bzz, and the
templates); M7 (agents use an app's records); and M5 (agents write and
change apps; its real-model gate prepared and not run). Each has its
milestone's exit gate, on Linux. Not here yet: M6 and M8. Nothing was pushed: what only CI
can show on macOS and Windows is listed as not seen. Plan:
[`docs/planning/2026-11-17/v0.16.0.md`](planning/2026-11-17/v0.16.0.md).
Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`, Rust 1.98.1. The
experiment named below is
`runs/experiments/bzz/v016-m0/q2-unstoppable/` (local and untracked); its
`RESULT.md` has the commands to repeat every number, and `out/` their
output.

## M0 — Baseline and the unknowns that can change the plan

Linux x86_64, 2026-11-18. Code at `a42bfac` on `t3/v016-baseline`, cut from
`v0.16.0` at `385b6c6`. The machine was shared throughout with other builds
and with another relay run (load 3 to 23).

### Baselines

| What | Fixed at |
|---|---|
| Buzz | `block/buzz` `bf3bdd0eaefe39289bf9620594f254c7f68897f5`, unchanged since v0.14.0 |
| Interpreter | Lua 5.4.9, from the C source in `lua-src` 551.0.2, through `mlua` 0.12.2 with `mlua-sys` 0.13.0. `mlua` is pinned exactly in `Cargo.toml`; the others by the lock file |
| Ratatui, crossterm | 0.30.2 and 0.29, unchanged, with bzz's features (`crossterm_0_29`, `layout-cache`, `unstable-rendered-line-info`) |
| Toolchain | Rust 1.98.1 (`rust-toolchain.toml`), unchanged |
| Node | agents only, as in v0.15. Apps need nothing installed |
| Schema | 20, unchanged |

### Question 1: the interpreter in the build

**How it is added.** `mlua = { version = "=0.12.2", default-features =
false, features = ["lua54", "vendored"], optional = true }` behind a cargo
feature `apps`, on by default (D12).

| Feature of `mlua` | Why |
|---|---|
| `lua54` | the language of the contract (D1) |
| `vendored` | Lua's C is compiled from the locked `lua-src` crate into the binary; nothing is installed, and nothing is looked up with `pkg-config` |
| not `send` | an interpreter lives and dies on its app's thread; without it `mlua` lets a host function hold an `Rc` |
| not `async`, `serde`, `macros`, `luau`, `luajit` | nothing needs them; each adds crates |

**Crates added to the lock file: six.** `mlua`, `mlua-sys`, `lua-src`, `bstr`
(`mlua`'s byte strings; its own dependency `memchr` was locked already), and
two that only the build script of `mlua-sys` uses: `luajit-src` and `which`.
`mlua-sys` asks for `luajit-src` whenever `vendored` is on; its Rust is
compiled, LuaJIT's C is not.

| Claim | Proof | Result |
|---|---|---|
| bzz with `apps` builds and passes its tests | the gates below | pass |
| bzz without `apps` builds and passes its tests, with and without huddle audio | `cargo test --locked --no-default-features --lib` (452 tests, none of `apps::`); `cargo test --locked --no-default-features --features huddles-audio --lib` (453); `cargo build --locked --no-default-features`; Clippy on both | pass |
| `apps` alone, without huddle audio | `cargo test --locked --no-default-features --features apps --lib` (459) | pass |
| The supply chain accepts the six crates | `cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok`. `cargo audit`: no vulnerability, 5 allowed warnings, the same five on the lock file of `385b6c6` (`chrono`, `instant`, `zbus_names`, `zvariant`, `zvariant_derive`); none is in a crate added here | pass |
| The lock file only adds | `git diff 385b6c6 a42bfac -- Cargo.lock`: 68 lines added, none removed; `cargo metadata --locked` accepts it | pass |
| The release binary still links what it linked | `ldd target/release/bzz`: `libasound.so.2`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`, as before. Lua is linked statically | pass |

**Size of the release binary**, `cargo build --release --locked`, stripped by
the release profile as the published one is:

| Build | Bytes |
|---|---|
| without `apps` (`--no-default-features --features huddles-audio`) | 28,383,712 |
| with `apps`, as committed: nothing calls the interpreter, and the linker leaves it out | 28,383,712 |
| with `apps` and the interpreter reachable (a temporary call from `main` that evaluates a script; not committed) | 28,800,864 |

So the interpreter costs **417,152 bytes (0.40 MiB)** once something uses
it: Lua's C and the part of `mlua` one evaluation needs. The plan's D12 says
"about 0.6 MiB", from a smaller probe; the runtime of M1 will add its own
code to this.

**The five release targets.** Read, not run, except Linux x86_64:

- Lua's build needs a C compiler for the target and nothing else: `lua-src`
  hands the 32 C files of `lua-5.4.9/` to the `cc` crate with one define for
  each system (`LUA_USE_LINUX`, `LUA_USE_MACOSX`, `LUA_USE_WINDOWS`). No
  CMake, no assembler, no system library, no network.
- bzz already compiles C through `cc` on all five (`libsqlite3-sys`,
  `aws-lc-sys`, `ring`, `secp256k1-sys`, `libdbus-sys`). The Linux build
  image has `build-essential`; the macOS and Windows runners have their
  system compilers.
- So `.github/workflows/ci.yml`, `release.yml`, `integration.yml`,
  `dist-workspace.toml` and `ci/linux-build/` are **unchanged**. CI's `build`
  job already runs `cargo test --lib` on the four platforms, which now
  includes the seven tests of `apps::`. `tests/build_environment_test.rs`
  passes.

**Not seen, and what CI will show when the branch is pushed** (the owner is
asked first):

| Where | What has never run |
|---|---|
| `build` job, `macos-15-intel` and `macos-15` | Lua's C compiled by Apple's clang; the seven `apps::` tests |
| `build` job, `windows-latest` | Lua's C compiled by MSVC (the experiment could not check this target at all); the `apps::` tests, of which the stop by time and the memory error rest on Lua's `longjmp` crossing Rust frames |
| `release.yml`, `aarch64-unknown-linux-gnu` | Lua's C compiled by GCC on the arm64 runner. It runs only for a tag or a release pull request |
| every platform | `bzz --version` of a release binary built there. Here: `bzz 0.15.0` on Linux x86_64 |

### The interpreter's limits inside bzz

`src/apps/mod.rs`, compiled only with the feature. Nothing in bzz calls it.
An `Interpreter` has `table`, `string`, `math`, `utf8` and the base
functions without `collectgarbage`, `dofile`, `load`, `loadfile`, `print`,
`require`, `warn` and `string.dump`; it loads text only; it has a memory
cap; and each `eval` has a time budget. `U:` is a unit test of that module.

```sh
cargo test --locked --lib apps::
```

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| A script is evaluated: a value comes back; the four libraries work; an error carries the script's file and line | `U: a_script_is_evaluated` | — (it is the feature) | pass |
| A script has exactly 23 globals, and none of `io`, `os`, `package`, `debug`, `coroutine`, `require`, `dofile`, `loadfile`, `load`, `collectgarbage`, `print`, `warn`, `string.dump`; eight attempts to use them fail | `U: a_script_has_no_library_that_reaches_outside` | names not removed: `left: "_G _VERSION assert collectgarbage dofile error … load loadfile … print … warn xpcall"`. `string.dump` kept: `left: "function" right: "nil"` | pass |
| Bytecode is refused | `U: bytecode_is_refused`, which first shows that `mlua` runs the same bytes when it is not told the mode | without `.set_mode(ChunkMode::Text)`: `bytecode ran: Ok(Integer(42))` | pass |
| An allocation past the cap is an error in the script: one of 1 GiB, many that add up, a doubling string. The interpreter answers the next call, and a script may catch the error | `U: the_memory_cap_is_an_error_in_the_script` | without `set_memory_limit`: `left: Ok(Integer(1073741824)) right: Err(Memory)` | pass |
| `while true do end` is stopped by time, also inside `pcall`, in a tail call and in a metamethod; not before the budget; the interpreter answers the next call | `U: an_endless_loop_is_stopped_by_time` | the hook raises an error and asks for no yield: `while true do pcall(…) end` never ends, the test was killed after 240 s | pass |
| Where script code cannot yield (called by C's `table.sort`) the hook's error stops it | `U: a_stop_inside_a_c_function_falls_back_to_an_error` | no fallback: never ends, killed after 240 s | pass |

The whole red run is `out/m0a/red-runs.log` in the experiment.

### Question 2: the call that cannot be stopped

**Answer: Lua 5.4 stays (D1 holds), with a yield instead of an error and a
bounded library.** The full report is the experiment's `RESULT.md`.

What was found in Lua 5.4.9 as `mlua` 0.12.2 runs it:

| Finding | Hostile script | Kind |
|---|---|---|
| The pattern matcher backtracks without end | `("a"):rep(300000):find(".-.-.-.-x")`; sixty characters are enough with thirty `a*` | one C call |
| A plain search is quadratic | 4 MiB of `a`, a needle of 2 MiB of `a` and a `b` | one C call |
| `string.rep` runs `n` empty copies | `string.rep("", math.maxinteger)` | one C call |
| `table.insert` and `table.remove` shift up to the length, and the length can lie, with a metatable **or without one** | `__len` returning 2^63-2; or 62 keys `1, 2, 4 … 2^61` in a plain table | one C call |
| `table.move` takes any range | `table.move({}, 1, math.maxinteger, 1)` | one C call |
| `table.concat` stops only at a value that is no string | `__index = table.concat`, range to `math.maxinteger` | one C call |
| `table.sort` reads and writes through metamethods that can be C functions | `__len` of 2^31-2 | one C call |
| A finalizer runs with hooks off, also when the interpreter is dropped | `__gc = function() while true do end end` | script code, unhooked |
| A message handler of `xpcall` gets the hook's own error inside the hook | `xpcall(loop, loop)` | script code, unhooked |
| `mlua` closes the to-be-closed variables of the hooked function inside the hook when the hook raises an error | `local a <close> = …` with a looping `__close`; also the fourth value of a generic `for` | script code, unhooked |
| `mlua` turns a script's error value into text by calling its `__tostring`, outside any coroutine | `error(setmetatable({}, { __tostring = … }))` | script code that cannot yield |
| `mlua` runs bytecode unless told the chunk is text | the test above | not a stop, but found here |

**The recommendation, three parts:**

1. The hook makes the call's coroutine **yield**; bzz never resumes it and
   never closes it. In `src/apps/mod.rs` already.
2. **The bounded library** in place of 16 functions and of string
   arithmetic: `string.find`, `match`, `gmatch`, `gsub`, `rep`, `format`;
   `table.insert`, `remove`, `move`, `concat`, `unpack`, `sort`; `tostring`,
   `ipairs`, `xpcall`, `setmetatable`. Patterns are matched in safe Rust
   with the deadline; the rest is Lua, which the hook sees. `pcall` is Lua's
   own again.
3. **Every call goes through one entry function** inside the coroutine,
   which hands an error back as a string; and the host reads a script's
   tables raw.

Parts 2 and 3 are in the experiment (`src/prelude.lua`, `src/pattern.rs`,
`src/bounded.rs`: about 370 lines of Lua and 730 of Rust, no `unsafe`), ready for
M1 to bring in. They add no crate: `memchr` is locked already.

| Claim | Proof | Red | Result |
|---|---|---|---|
| Every hostile script ends within the budget (200 ms, plus 100 ms) and the interpreter then answers, or is replaced | `q2-lua54 suite bounded hostile.lua`: 80 scripts, each in a process of its own | `q2-lua54 suite plain hostile.lua` (the prototype's sandbox): **32 of 80 fail**, 31 killed after 5 s and one 2.2 s long | pass: 80 of 80 |
| No function a script can reach has a call that runs past the budget | `q2-lua54 sweep bounded`: 77 functions, 10,271 argument lists each, 790,867 calls | `sweep plain`: 9 functions never return for some list | pass: 0 |
| An allocation past the cap is an error in the script, not the end of the process | the 15 `memory-*` scripts of the suite, cap 64 MiB: every one an error or stopped at the budget, the process alive, its peak at most 89 MiB | — (Lua and `mlua` do this already: the same with `plain`) | pass |
| The replaced functions answer as Lua's own | `q2-lua54 diff 1000000 <seed>` for three seeds: 3,000,000 random calls of both, results and error messages compared | it found two faults while it was written (one error's wording; `gsub` with a table read captures it did not need) | pass: 0 differences |
| The apps agents wrote behave the same | the experiment's bench on nine apps, prototype host against the same host with the library: Board 12/12, Rota 14/14 ×2, Panel 15/15 ×2, Board with priority 17/17 ×2, Approvals 14/15 ×2 | — (the one failing case fails with both hosts: those two scripts predate JSON `null`) | same |
| The stop never needed its second line in the suite | no outcome of `suite bounded` is the hook's error | before part 3 one was: `error-object-tostring`, and with a to-be-closed variable in it the script never ended | pass |

**What it costs.** 133 KB in the prototype host (2,051,672 to 2,185,056
bytes, both built here). On five apps with 200 records the median time from
a key to the screen (0.83 to 2.16 ms with the prototype host) is 0.1 to
0.4 ms longer for four of them and 0.1 ms shorter for one. Single calls of
the replaced functions are 1.5 to 10 times slower (a plain `find` 98 ns
against 588 ns), and `pcall` five times faster than the prototype's. Nothing
was tuned.

**What an author loses:** `__gc`; a message handler that runs at the point
of the error; "invalid order function" from `table.sort`, which is now
stable; `unpack` of a million values. Nothing else was found to differ.

**The other candidates.** Luau, as `mlua` gives it: 64 of its 66 scripts
pass. A plain `find` of 4 MiB never ends and `table.sort(t, rawequal)` of
two million numbers runs 1.4 s, so it needs two of the same replacements;
its binary links `libstdc++.so.6`, which bzz does not link; the probe is
1.2 MB larger; and it changes D1. All 12 app files compile as Luau; two of
the nine apps call `math.tointeger`, which Luau has not. They were not run
on the bench. A process for each app stops anything (0.4 ms to kill one,
3.6 ms to start one and run a script) and changes the design; it is the way
out if a hole appears that cannot be closed.

**Not proven for question 2:** any platform but Linux x86_64 (the C
library's own `strtod` and `memcmp` differ there; here a number of four
million digits converts in 25 ms); that no other such call exists, beyond
what the source and the sweep show; Luau on the bench; a thousand stops in a
row.

### Question 6: the widgets

**Answer: everything is available; nothing needs enabling.**
`src/apps/widgets_test.rs` uses, and draws to Ratatui's test backend,
everything the prototype's drawing uses (`f1/s-embedded/host/src/draw.rs`
and `ui.rs`):

- widgets: `Paragraph` with `Wrap`, `Block` with `Borders`, `BorderType`
  (plain, rounded, double, thick), `Padding` and both titles, `List` with
  `ListState`, `ListItem`, `ListDirection` and `HighlightSpacing`, `Table`
  with `TableState` and `Row`, `Tabs`, `Gauge`, `LineGauge`, `Sparkline` with
  `RenderDirection`, `BarChart` with `Bar` and `BarGroup`, `Scrollbar` with
  `ScrollbarState` and the four `ScrollbarOrientation`s, `Clear`;
- layout: `Layout`, `Direction`, the six `Constraint`s, the seven `Flex`
  modes, `Margin`, `Rect`, `Position`, `Alignment`, and the cursor position
  of a frame;
- style and text: `Style`, the six `Modifier`s, `Color` from a name, from
  `#rrggbb` and by index, `Span`, `Line`, `Text`.

| Claim | Proof | Red | Result |
|---|---|---|---|
| Each is available under `default-features = false` with bzz's three features | `U: apps::widgets_test::every_widget_of_the_app_contract_is_available`: compiles, draws, and 17 things it drew are found on the screen | an import of `ratatui::widgets::calendar::Monthly`, which is behind a feature bzz does not enable: `error[E0432] … the item is gated behind the 'widget-calendar' feature` | pass |

The prototype itself was built with fewer features than bzz (`crossterm_0_29`
only), so this was expected; the test keeps it true.

### Questions 3, 4 and 5

Each was answered by an agent in `runs/experiments/bzz/v016-m0/` (ignored by
git, kept on the principal's machine) and reviewed by the principal. The ADR
carries the answers.

| Question | Answer | How it was seen |
|---|---|---|
| 3. Records on the pinned relay | `bzz.app.<app>.<record>` is accepted; one sync takes an exact list of types; 4096 is per sync filter; a 128 KiB script fits one artifact; any channel writer may revise any record | `q3-relay/run.sh all`: eight measures on the pinned relay, all ok. The principal ran `names`, `shapes`, `roles` and `closed` again: all ok |
| 4. What Buzz desktop does with these artifacts | Buzz desktop 1.0.1 shows nothing of them, stores nothing, writes nothing, and is no slower with 4,101 | `q4-buzz-desktop/`: 54 screenshots, 4,708 artifact events, `verify.py final` with 0 missing, extra or changed. Run once, by the agent; the principal checked the teardown |
| 5. The pane and the keys | 42 columns from a window of 72; thread and app together from 144; 22 rows docked; `Ctrl+C` leaves the app | `q5-pane-keys/`: a throwaway pane (branch `t3/research-pane-keys`, not merged) on the pinned relay through tmux, 207 captures. Herdr was not running |

Not seen: a Buzz desktop newer than 1.0.1; an agent run by Buzz desktop in
the channel; the Linux console, Windows Terminal and macOS Terminal; a real
script in the pane.

### The ADR

[`adr-v0.16-apps.md`](adr-v0.16-apps.md): bzz runs code it did not ship.
Written from the plan's decisions and from what the six questions found.

### Gates after M0

On `a42bfac`, one after the other, 2026-11-18 from 14:26 (UTC−5). Only
documents changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features`, and with `--no-default-features --features huddles-audio` | clean, all three |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | 742 passed, 0 failed, 30 ignored, in 56 test programs; 7 of them are new, the tests of `apps::`. The ignored ones need the relay or an older release's profile |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 452 passed |
| Supply chain | `cargo deny check`; `cargo audit` | ok; ok, 5 allowed warnings as on `385b6c6` |
| Release binary | `cargo build --release --locked`; `target/release/bzz --version` | `bzz 0.15.0`, 28,383,712 bytes |
| Pinned relay | `BZZ_TEST_REQUIRE_NODE=1 BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh` | 22 passed in 858 s: 21 ran, and the one with the owner's own programs skipped itself and said so. Then `agent_host_test`, 7 passed, and `agent_move_test`, 8 passed. 15 min 17 s in all, Node 26.5.0 |
| Pinned relay, federated identity | the same with `--nip-fi` | 4 passed in 24 s |

The relay runs took the harness lock (`/tmp/bzz-test-relay-<uid>.lock`),
which another run held first. Afterwards no `buzz-harness` container and no
tmux session of the harness was left (`docker ps`, `tmux ls`). In a worktree
the script's `./target` is a link to the main checkout's, made for the run
and removed after it, and `CARGO_TARGET_DIR` is not exported, so that Buzz
at the pin is built in its own folder. No owner's program, sign-in or model
was used.

The same gates ran once before on the tree as it was before `cargo fmt`
(two hunks in the widgets test); `cargo fmt --check` failed there and
everything else passed. The logs of both passes, of the relay runs and of
the red runs are in `out/m0a/` of the experiment.

The principal ran the gates again on `e8e0f57` after merging the branch
(format, lint, the scripted harness, all tests: 742 passed, the tests
without the features, `cargo deny check`, `cargo audit`): all green. He ran
the hostile suite of question 2 twice. The bounded library stopped 80 of 80
on a quiet machine, and 79 of 80 while the lint was compiling beside it:
the mark of 300 ms is tight on a loaded machine, and M1's tests give it
room. The prototype's sandbox failed 32 of 80, as reported.

### Not proven in M0

| Item | State |
|---|---|
| macOS and Windows, and Linux on arm64 | not compiled or run here; the table under question 1 says what CI will show |
| The release TUI through Herdr | not run: nothing a user sees changed, and the brief of this work gave it no Herdr pane |
| The upgrade from v0.15.0, the benchmarks, the agent host's own tests | not run: no migration, no change to what a benchmark runs, no change under `agent-host/` |
| The bounded library inside bzz | it is in the experiment; M1 brings it in with its suite |

### Found on the way

- **Cargo wanted to change thirteen locked packages when it added `mlua`**:
  it pointed each at `windows-sys` 0.61.2 in place of the older version the
  lock file had (`anstyle-query`, `rustix`, `tempfile`, `socket2` and nine
  more), without saying so. None of the six new crates uses `windows-sys`.
  The lock file committed here keeps those thirteen as they were and only
  adds; `cargo metadata --locked` accepts it and cargo does not rewrite it.
- **`cargo audit` counts five allowed warnings**, where v0.15's validation
  says four. The fifth is `chrono` (RUSTSEC-2026-0543), there on the base
  commit too: the advisory is newer than v0.15.0.
- **Lua's C for `io`, `os` and `package` is in the binary** once the
  interpreter is used: `mlua` refers to every library's opening function.
  They are never opened, and a script has no way to open them.
- **An error raised by a Rust function of `mlua` reaches a script as a
  userdata**, not as a string: `"failed: " .. err` then fails. The functions
  of the app contract will need the same wrapper the bounded library uses.

## M1 — The app runtime, first half: the runtime and its contract

Linux x86_64, 2026-11-18 and 19. Code at `bc0f875` on
`t3/m1-runtime-implementation`, cut from `v0.16.0` at `5eabee9`. The
machine was shared with another agent's builds (load 1 to 13). Nothing in
bzz calls the runtime yet: no pane, no command, no relay.

What is there:

| Path | What |
|---|---|
| `src/apps/runtime/vm.rs`, `bounded.rs`, `pattern.rs`, `prelude.lua` | the interpreter and the three things that stop a script (M0, question 2) |
| `worker.rs`, `mod.rs` | one script on a thread of its own; the owner's end: events in, reports out, a waker |
| `api.rs`, `api.lua` | the closed API: `ui`, `data`, `host`, `print` |
| `ui.rs`, `draw.rs`, `layout.rs` | the eighteen elements, their drawing, and how rows and columns divide |
| `records.rs`, `memory.rs` | the boundary to records, and a store in memory behind it |
| `headless.rs` | a script without a window: steps in, the screen as text out |
| `limits.rs` | every limit, each with its reason |
| `docs/app-api.md`, `docs/app-api.d.lua` | the contract `api = 1`, and its twin for an editor |
| `tests/fixtures/apps/hello`, `board` | two apps written against the contract |

9,026 lines under `src/apps/runtime/`, 5,170 of tests under
`src/apps/runtime/tests/`. No `unsafe`. One dependency became direct:
`memchr`, locked already (the lock file gains one line).

### The limits, and why each number

`src/apps/runtime/limits.rs` holds them with their reasons, and
`docs/app-api.md` publishes them; a test compares the two tables.

| Limit | Value | Why |
|---|---|---|
| Time for one call | 250 ms | a hundred times the slowest key of the experiment (2.6 ms at the 95th percentile); three stops, which give an app up, cost under a second of one core |
| Memory | 64 MiB | ten times the experiment's peak (6.3 MiB); holds the fullest set of text records the limits allow, twice; the cap M0's hostile scripts ran under |
| Script | 128 KiB | compiling is outside the time budget and linear in the source; what travels as one artifact in M6 |
| Elements in a screen | 10,000 | laying out and drawing are one pass over them, on the owner's thread |
| Text in a screen | 128 KiB | a wrapped text is the one element whose drawing reads every byte: 8.6 ms at this size, 17.6 ms at the 256 KiB first chosen |
| Nesting of a screen | 32 | the experiment's apps nest six deep |
| Timers | 32 waiting, 100 ms to a day | the plan gives no timer finer than a tick of bzz's loop |
| Requests | 64 waiting, 600 a minute | a search sends one for each key; the rate bounds how often an app can make bzz's loop turn |
| Log | 120 lines a minute, 1 KiB each | lines past it are dropped and counted |
| Content of a record | 16,384 bytes of JSON | a thousand such records are 16 MiB, inside the memory that has to hold them; the relay takes 262,144 |
| Live records, writes | 1000; 60 a minute | the plan's; the service behind the boundary holds an app to them |

### Claims

`U:` is a unit test of the library, named under `apps::runtime::`. All of
them run with `cargo test --lib apps::`, which CI's build job runs on the
four platforms. Each red was produced by the change named, run, and
reverted; where a script exists for it, that is said.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| Every hostile script ends within the budget (200 ms, and a margin of 800 ms; a late end is tried again twice, a script that never ends is not), and the interpreter then answers or is replaced. The yield was enough everywhere: none was ended by the hook's error | `U: tests::hostile::every_hostile_script_is_stopped_in_budget`: M0's 80 scripts, each on a thread of its own. 36 stopped, 14 ended with an error, 12 with a memory error, 18 returned in time; the slowest call 239 ms | `U: tests::hostile::the_prototype_sandbox_fails_the_suite` (ignored; run with `--ignored`): **31 of 80 "NOT STOPPED: still running after 8s"**. M0 counted 32: its thirty-second, a sort of two million numbers, is ended here while it still builds its table | pass: 80 of 80, in 5 runs in a row, in a release build, and in every run of the whole library's tests (11 seen, one of them with clippy compiling beside it) |
| No function a script can reach has a call that runs past the budget (100 ms, same margin) | `U: tests::sweep::no_function_a_script_can_reach_runs_past_the_budget`: 107 functions, the 76 of Lua and the 31 of the API, with 5,029 argument lists each from a pool of 20 awkward values: 538,103 calls. With `BZZ_APPS_SWEEP=full`, M0's 12,517 lists: **1,339,319 calls, the slowest 125 ms** | `U: tests::sweep::the_prototype_sandbox_fails_the_sweep` (ignored): `NOT STOPPED: string.find(BIG(1 MiB of a), ".-.-.-.-.-x") has been running for 10.46 s` | pass: 0 |
| The replaced functions answer as Lua's own | `U: tests::differential::the_bounded_functions_answer_as_lua_does`: 180,000 random calls of both; with `BZZ_APPS_DIFF=1000000`, **3,000,000 comparisons, 0 differences** | `string.rep` made to drop its separator: 649 of 180,000 differ | pass |
| A thousand stopped calls in a row, each with a to-be-closed variable, leave the interpreter's memory flat and the interpreter answering (M0 had left it open) | `U: tests::vm::a_thousand_stops_in_a_row_leave_the_interpreter_as_it_was` | — (a measurement) | pass |
| A script reaches no file, network, process, environment, native code or bytecode | `U: tests::reach::a_script_reaches_no_file_network_process_environment_native_code_or_bytecode`: the prototype's `tests/hostile/app.lua` at `api = 1`, 19 attempts, each blocked; and the script's 27 globals and every member of `ui`, `data`, `host` by name. `U: tests::vm::a_script_has_no_library_that_reaches_outside`, `bytecode_is_refused` (M0's, kept) | the base functions not removed: `read file loadfile: REACHED nil` | pass |
| The fence: a type that was not declared cannot be listed, created, pushed, updated or deleted, also with the record's real id | `U: tests::reach::the_fence_holds_a_script_to_its_declared_types` | the check of the type off: the create of a `card` went through with no error | pass |
| bzz never runs a metamethod of a script | `U: tests::reach::bzz_never_runs_a_metamethod_of_a_script`: a metatable that counts its calls on everything a script hands over (the app, the state, every element, request, tag, field and key): 0 calls, also across a reload | one `raw_get` of `ui.rs` made a `get`: `calls 188` | pass |
| A broken reload leaves the old version running, with the error and its line; seven ways a version can fail to load | `U: tests::running::a_broken_reload_leaves_the_old_version_running`, `a_version_that_never_finishes_loading_is_stopped_and_refused` | the old version dropped on a failed load: the error lacks `(reload failed: the previous version keeps running)` | pass |
| State survives a reload: records, the selection, an open form with its text; a request that was waiting is answered to the new version; `init` is not called again | `U: tests::running::state_survives_a_reload`, `a_reload_drops_what_cannot_move_and_lets_the_script_migrate`, `a_state_too_deep_to_carry_starts_the_app_again` | the state not handed to the new version: the form is gone and the status is `Loaded 2 notes.` | pass |
| Memory past the cap is an error in the script, not an abort; the app answers the next key | `U: tests::vm::the_memory_cap_is_an_error_in_the_script`, `tests::reach::memory_past_the_cap_is_an_error_in_the_script_not_an_abort`, `tests::limits::a_record_set_too_big_for_the_memory_is_an_error_in_the_app` | without `set_memory_limit`: `left: Ok(Integer(1073741824)) right: Err(Memory)` | pass |
| The Board fixture passes the twelve cases of the bench, at 40×16 and at 80×24 | `U: tests::board::case_01` to `case_12`, on the in-memory store, without a window | the sort by the order field removed: `todo: ["Bravo", "Charlie", "Alpha"]` in every case | pass |
| The Board works in 40 columns: a title of 16 characters is shown in full | `U: tests::board::a_title_of_sixteen_characters_is_shown_in_full_at_every_size` | the stacked layout off: `todo: ["Sixteen let"]`. **The twelve cases passed without it**: their titles are short. This test was written because of that | pass |
| A stuck script does not delay its owner | `U: tests::running::a_stuck_script_does_not_delay_its_owner`: with the script in a loop for 2 s, the worst of some 460 rounds of `send`, `poll` and `draw`: **54 µs, against 53 µs idle** (release); 354 µs against 351 µs (debug). Starting a runtime returns in 0.1 ms, dropping one does not wait | `poll` made to wait for the script: the test finds the script at rest where it must still be in its loop, 2.3 s later | pass |
| `api` other than 1 is refused with a message that names 1 | `U: tests::running::an_api_other_than_1_is_refused_with_a_message_that_names_1`: 2, 0, 1.5, `"1"` and none | the check off: `left: None right: Some("app.lua: the script is written for api 2, and this bzz knows only api 1")` | pass |
| bzz without the `apps` feature builds and passes | `cargo build --locked --no-default-features`; `cargo test --locked --no-default-features --lib`: 458 passed, none of `apps::`; Clippy without the features | — | pass |
| An error carries the script's file and line, also when the call is the last of a function (`return ui.column { … }`), and a wrong value in a screen is told with the line of its element | `U: tests::running::an_error_carries_the_file_and_the_line_and_the_app_goes_on` | the API's functions wrapped in Lua functions: `in app.key: ui.text: unknown property 'colour'…`, the line lost | pass |
| Every error a script catches is a string: 29 ways to fail a function of the API and of the library | `U: tests::running::every_error_of_the_api_is_a_string_a_script_can_catch`, `tests::vm::an_error_a_script_catches_is_a_string` | the script's `pcall` Lua's own: `1 false userdata` | pass |
| A stopped call leaves done what it had done, and what it had asked for is done; the app goes on | `U: tests::running::a_stopped_call_leaves_its_work_half_done_and_the_app_goes_on` | the hook raising an error instead of a yield: the call ends as an error of the script | pass |
| Three stops of one handler in a row give the app up; stops that are not in a row do not; a reload starts it fresh | `U: tests::running::an_app_is_given_up_after_three_stops_of_one_handler_in_a_row`, `a_view_that_never_returns_gives_the_app_up_too` | never given up: `right: GivenUp` | pass |
| An error covers the last row or two of the app's region, and goes with the next key | the two tests of errors and of a broken reload above | — | pass |
| `Ctrl+C` never reaches a script, however its owner sends it; `host.quit()` closes the app | `U: tests::running::ctrl_c_never_reaches_a_script_and_host_quit_closes_the_app` | the check off: `keys: ctrl+c ctrl+C ctrl+c ctrl+C ctrl+d` | pass |
| The top level of a script can only compute: a version that is then refused has asked for nothing | `U: tests::running::the_top_level_of_a_script_can_only_compute` | the rule off: the request is made | pass |
| Each limit, reached and passed, is told to the script and the app goes on | `U: tests::limits::` (9 tests) | the count of elements off: the script's thread panicked on its own arithmetic and the app showed `the app's runtime failed: a fault in bzz, not in the script`. The only time the path for a fault of bzz itself was seen | pass |
| The published limits fit together: a thousand records of 16 KiB of text are listed, held, and listed twice more | `U: tests::limits::the_published_limits_fit_in_the_memory_of_one_app` | — | pass |
| A resync is not a change: nothing when nothing changed, else only the real differences | `U: tests::records::a_resync_is_not_a_change`, `records::tests::a_resync_gives_only_the_real_differences` | every record deleted and created again: `assertion failed: pushes.is_empty()` | pass |
| Every error code reaches the script, `rate_limited` with its seconds, `conflict` with the record; who signed a record; no order; null and numbers | `U: tests::records::` (6 tests) | — (the contract's new parts) | pass |
| Nothing is drawn outside the area: two apps, three pages, 106 areas each, in a window whose every other cell is checked | `U: tests::drawing::nothing_is_drawn_outside_the_area` | the screen drawn straight into the owner's buffer: `the cell at 22,13 was drawn on` (Ratatui's `List`) | pass |
| An app follows the theme: the eight names of meaning under three themes, fixed colours unchanged | `U: tests::drawing::an_app_follows_the_theme` | — | pass |
| No number a script gives an element breaks the drawing: 21 elements with 15 numbers each, at five sizes | `U: tests::drawing::no_value_a_script_can_give_an_element_breaks_the_drawing` | before the bars were bounded: `ratatui-widgets-0.3.2/src/barchart.rs:444: attempt to multiply with overflow` | pass |
| Rows and columns divide as Ratatui divides them wherever there is room for the parts, and ten thousand parts cost their number | `U: layout::tests::` (3 tests): 18,805 of 18,805 divisions the same as Ratatui's; without room, 1,501 of 1,765 | while it was written: a centred part one cell off for an odd remainder | pass |
| The document, its twin and the runtime name the same functions, globals, elements, properties, handlers, record calls, codes, fields and limits | `U: tests::contract::` (6 tests); the runtime's side is asked of a running interpreter | `ui.truncate` renamed in the document; the time of a call changed in `limits.rs` | pass |
| A script is tried without a window | `U: tests::running::a_script_is_tried_without_a_window` | — (the driver itself) | pass |

What the twelve cases leave out, because the in-memory store and a buffer
cannot express it: that the process is still running half a second after
its first draw (case 1); that it exits within 2 s with status 0, leaves no
child, and restores the terminal (case 9: here the app asks to close, at
once, with a request unanswered). Those are bzz's own and M3's.

### How long the largest screens take to draw

`U: tests::drawing::the_largest_screen_a_script_can_return_is_drawn_in_good_time`,
in a window of 200 by 60, the median of five draws. The owner draws on its
own thread, so this is what a script can make that thread do.

| Screen, at the limits | Release | Debug |
|---|---|---|
| 131,070 bytes of text, wrapped | 8.6 ms | 77.8 ms |
| a list of 9,990 items | 2.0 ms | 11.0 ms |
| blocks nested 31 deep | 1.1 ms | 11.7 ms |
| a grid of 384 rows of 8 bordered cells | 0.8 ms | 8.6 ms |
| an input of 60,000 characters | 0.7 ms | 6.3 ms |
| a row, a column, of 4,990 parts | 0.5 ms | 4.8 ms, 4.4 ms |
| a table of 1,240 rows of 7 cells | 0.4 ms | 3.3 ms |

### Found on the way

- **Ratatui's `Layout` cannot be given a script's sizes.** Its solver gets
  one constraint for every pair of parts that fill. Measured in a release
  build of Ratatui 0.30.2 alone: 16 parts 5 ms, 24 parts 25 ms, 32 parts
  102 ms, 48 parts not done after two minutes; and with 64 parts in 40
  columns it ended in a panic, `failed to split:
  InternalSolverError(ObjectiveUnbounded)`. Twelve columns of fixed width
  take 1 ms. So `layout.rs` divides rows and columns itself, by the order
  of strengths Ratatui gives its constraints, and a table is drawn in
  `draw.rs`, because Ratatui's `Table` uses the same solver for its
  columns. Where the parts do not fit, the result is the rule of
  `layout.rs` (the last parts give way first), not what the solver happens
  to settle on.
- **Ratatui's `List` writes one cell past its area** when the area is
  narrower than the selection symbol. An app's screen is therefore drawn
  on a buffer of its own and copied into the owner's.
- **Ratatui's `BarChart` multiplies the number of bars by their width in
  sixteen bits** and panics on overflow in a debug build. It gets only the
  bars that can be seen.
- **A Lua wrapper around a function of the API loses the script's line**
  when the call is the last of a function, which the last line of every
  view is: Lua drops the frame. The API's functions are therefore Rust
  functions called directly, and the script's `pcall` turns the error they
  raise into the string it stands for. That `pcall` also raises the hook's
  own error again, so no `pcall` of a script swallows the second line.
- **A panic on a script's thread is caught**, and the app says so. But
  bzz's panic hook (`src/ui/terminal.rs:110`) restores the terminal for a
  panic on any thread. M3 has to decide what it does for this one.
- **`df -h /home` reads another file system.** The shared `target/` is on
  `/home/arpagon`, which had 8.5 GB free when `/` had 26. `cargo test
  --locked --all-targets` failed twice with "No space left on device"; it
  passed the next morning with 15 GB free.

### Gates

On `bc0f875`, 2026-11-18 from 18:35 (UTC−5), one after the other; the
all-targets run on 2026-11-19 at 08:35, on the same commit. Only this
document changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features`, with `--no-default-features --features huddles-audio`, and with `--no-default-features --features apps` | clean, all four |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets -j 6` | 836 passed, 0 failed, 32 ignored, in 56 test programs. 94 more than M0's 742: 101 tests of `apps::` in place of 7. Two of the ignored are the red runs of the suite and the sweep |
| Tests without the features | `cargo test --locked --no-default-features --lib`; `cargo build --locked --no-default-features` | 458 passed; builds |
| `apps` alone | `cargo test --locked --no-default-features --features apps --lib` | 553 passed, 2 ignored |
| Supply chain | `cargo deny check`; `cargo audit` | `advisories ok, bans ok, licenses ok, sources ok`; no vulnerability, the same 5 allowed warnings as in M0 |
| The long runs | `BZZ_APPS_SWEEP=full …sweep::no_function`; `BZZ_APPS_DIFF=1000000 …differential` | 1,339,319 calls, 0 past the budget, in 93 s; 3,000,000 comparisons, 0 differences, in 120 s |
| Release | `cargo test --locked --release --lib apps::`; `cargo build --release --locked` | 101 passed, 2 ignored; `bzz 0.15.0`, 28,392,672 bytes: 8,960 more than M0's build, and nothing calls the runtime yet |

### Not proven in M1-A

| Item | State |
|---|---|
| macOS, Windows, Linux on arm64 | never run. Every test here is a unit test of the library, which CI's build job runs on the four platforms when the branch is pushed. The timing of the C library there is what M0 named |
| The six apps on the bench, the example host | M1-B |
| Records on the relay; the pane in bzz's window | M2; M3. The runtime has run on the in-memory store only, and has been drawn into a buffer only |
| An app that does not respond | no call is known that the budget does not end. What its owner then sees was shown by telling the owner's end a shorter budget than the script's thread has, and by a backlog of events |
| The hook's second line | never taken: the hostile suite fails if it is |
| A fault of bzz on a script's thread | seen once, in a red run; no test makes one |
| An agent writing an app against `docs/app-api.md` | M5. The contract was read by nobody but its author |
| The size of the runtime in the binary | nothing calls it, so the linker leaves it out |

## M1 (bench and apps) — The example host, the bench, and the six apps

Linux x86_64, Rust 1.98.1, 2026-11-19. Branch `t3/9c67a42d`, cut from
`v0.16.0` at `d0dbb36` (after M2 (session) was merged). Another agent
began the work on `t3/benchmark-apps` (six commits, the last an unfinished
script), and it was merged here at `4335e29`. The code proven below is at
`846b89b`. Every result in this section was run on this branch; none is
taken from the earlier agent's notes. The machine was shared with another
agent's builds (load 4 to 6).

**Where it is.**

| Path | What |
|---|---|
| `examples/app_host.rs` | the runtime alone in a terminal, on the bench's simulated data service; reload on save. The only place that knows the bench's protocol; the table at its top lists where that protocol and the contract differ |
| `tests/apps-bench/` | the bench, from the experiment's `bench/` and `f2/bench/`: `run`, `reload`, `pushes`, `hostile`, `measure`, `drive`, `data-service`, `spec/`, `lib/`. Python 3.10+, standard library only, Linux only |
| `scripts/test-apps-bench.sh` | runs all of it and prints one table |
| `tests/fixtures/apps/{board,poll,rota,approvals,panel,board-priority}/` | the six apps at `api = 1`, each an `app.lua` and a `brief.md` (D16). `hello` has a brief too |
| `src/apps/runtime/tests/{board,poll,rota,approvals,panel,board_priority}.rs` | the cases of each spec on the in-memory store, through an `AppSession`: 96 tests, each case at 40×16 and at 80×24 |

Nothing was changed in `src/apps/runtime/` (outside its tests) or in
`src/apps/session/`. No dependency was added; `Cargo.toml` gains the
`[[example]]` entry of `app_host`, which needs the `apps` feature.

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
scripts/test-apps-bench.sh                        # debug host, ~7 min
scripts/test-apps-bench.sh --release --measure    # release host, and the numbers
cargo test --locked --lib apps::runtime::tests::  # the same cases, any platform
```

### The exit gate of M1

| Claim | Rung | Result |
|---|---|---|
| On the example host, the six apps pass their cases | the bench: `scripts/test-apps-bench.sh`, every app at 80×24 and at 40×16 | pass: 85 of 85 cases at each size, in a debug build and in a release build |
| The hostile script reaches nothing | the bench: `tests/apps-bench/hostile` | pass: 29 of 29 checks |
| A broken save leaves the old version running | the bench: `tests/apps-bench/reload`, part `edit`, once for each app | pass, six apps at each size |
| State survives a reload | the same: data, selection, filter or search, an open form with its text, and the connection, over 25 saves of each app; and the owner's example, the Board swapped for the Board with priority | pass: 20 of 20 at each size, 150 saves each |

### The six apps on the example host

`scripts/test-apps-bench.sh --release --measure`, the last full run. The
debug run of the inherited code, before the change to pushes below, gave
the same counts.

| | 80×24 | 40×16 |
|---|---|---|
| Board, cases passed | 12/12 | 12/12 |
| Poll | 12/12 | 12/12 |
| Rota | 14/14 | 14/14 |
| Approvals | 15/15 | 15/15 |
| Panel | 15/15 | 15/15 |
| Board with priority (the Board's 12, then 13 to 17) | 17/17 | 17/17 |
| Reload: edit (six apps), swap (two seeds), a bad moment | 20/20 | 20/20 |
| When pushes begin | 8/8, at 80×24 | |
| The hostile script | 29/29, at 120×40 | |

**Every case passed, so no failing case had to be blamed on the app, the
runtime, the contract or the case.** Where a case of the experiment
depended on something `api = 1` removed, the earlier agent changed the
case before it ran here. I read each change against the experiment's
`f2/bench/lib/`:

| Case | In the experiment | Here | Why |
|---|---|---|---|
| Every case | starts at 80×24; 06 shrinks to 40×16 | starts at `--size` and resizes to the other of 80×24 and 40×16; the script runs both | every app works in the pane |
| Board, Poll 01 | first draw at 80×24 and 120×40 | at 40×16 too | the same |
| Board 06 | `Terminal too small. Need 60x16.` at 40×12 | `Need 40x16.` | M0 brought the Board down to 40 columns |
| Board 03, Approvals 03 | the created record's content exactly | the same, plus an `order` greater than the seeded ones | the contract: order is a field, ids do not sort |
| Rota 03 | the stored day | and every day keeps its `order` across the app's writes | an update replaces the whole content |
| Board with priority 13, 17 | the stored card exactly | the card as before, with only the priority or the column changed | the same |
| Poll 05 | one slow list on reload | one slow list of each type | a `data.list` names one type, and the Poll has two |
| The data service | records in creation order, `seq` | the same, and seeded cards, days and requests carry `order` | the host hands the script no order and no `seq` |

### The same cases on every platform

`cargo test --locked --lib apps::runtime::tests::` runs each fixture
through the cases of its spec that a store in memory can express. It uses
the same `AppSession` that bzz's window will use, at 40×16 and at 80×24.

| File | Tests | Cases |
|---|---|---|
| `board.rs` | 15 | 01 to 12, and three more (M1-A) |
| `poll.rs` | 14 | 01 to 12, and two of the spec's rules |
| `rota.rs` | 15 | 01 to 14, and one |
| `approvals.rs` | 16 | 01 to 15, and one |
| `panel.rs` | 15 | 01 to 15 |
| `board_priority.rs` | 21 | the Board's 01 to 12 on cards with priorities, 13 to 17, and four: a merge, the selection under the filter, the running Board becoming the Board with priority, the Board keeping a priority it does not know |

They leave out what a case checks of the process and its terminal: still
running after the first draw; exit within 2 s with status 0; the terminal
restored; no process left. Each file says so, case by case.

**Red.** One rule was broken in each new fixture, all five at once, and
`cargo test --lib` ran the five files: 76 passed, 5 failed. Each failure
was the test of the rule broken, and no other test failed:

| Broken | Failed |
|---|---|
| Rota: a list asked for before a take overwrites it | `rota::case_05_late_reload` |
| Approvals: a conflict on a decided request decides it again | `approvals::case_12_conflict` |
| Panel: a conflict on an acknowledged metric acknowledges it again | `panel::case_15_what_can_be_acknowledged` |
| Board with priority: a move writes the card without its priority | `board_priority::case_17_a_move_keeps_the_priority` |
| Poll: a vote for an option not in the poll counts | `poll::a_changed_vote_is_one_record_and_votes_for_no_option_count_nowhere` |

The first four also ran on the bench (release host, 40×16, a copy of the
fixtures outside the repository) and fail the same cases: Rota 05,
Approvals 12, Panel 15, Board with priority 17, each against the screen the
spec expects. With the fixtures restored, all of them pass (the full runs
above).

### Reload with real apps

`tests/apps-bench/reload` (`lib/reload.py`) runs the scripts of the
experiment's `g1/reload/`, brought to the fixtures. It has three parts:

- **edit**, for each of the six apps. The app is brought into a user's
  state: records, a selection that is not the first, a filter or a search,
  and a form open with text. Then 25 saves each change a visible label.
  After each save, the data, the selection, the filter and the form with
  its text must still be there. Then comes a **broken save**: the error
  names the file and line 4 on the screen, the log says the previous
  version keeps running, and a change by another participant still shows.
  The fix clears the error. Last, one write of the app's own and one
  change by bo show that the connection survived.
- **swap**, the owner's example. The Board runs with a search and a
  selection, and its file is replaced by the Board with priority. The
  indicator appears; the selection, the search and the data stay; `2`, `p`
  and `1 3` work at once and are stored. It runs twice: on the Board's
  cards, and on cards that already have priorities.
- **moment**: a save while a write waits for its reply (the reply reaches
  the new version, and there is one write), and a save while the first
  list waits.

On the release host, 150 saves at each size, from the write of the file to
the new screen: **p50 26.9 ms, p95 48.9 ms** at 80×24, and p50 28.6 ms,
p95 49.2 ms at 40×16. The host checks the file every 50 ms, and that wait
is most of this time.

**Red.** The host was made to start the app anew on every save instead of
reloading it in place (`examples/app_host.rs`, `Msg::File`). Then `reload
--saves 3 --only edit,swap board` at 40×16 passed **0 of 3**: edit 11 of
22 checks (`data survived every save (lost after 3)`, and the same for the
selection, the search, the form, and the old version after a broken save);
swap 11 and 10 of 16 (`the selection is kept`, `the search is kept`, …).

### When pushes begin

M2 (session) changed the contract: pushes of a type begin with the app's
first list of it. The simulated service pushes every change to whoever
subscribed, so the host now applies the rule itself. A push of a type the
script has not listed is dropped. A push that arrives while the first
list of its type is on its way is held, and given after the reply if its
`seq` is newer than the list's. The hostile app now lists its own type
first, as the contract tells every app to.

`tests/apps-bench/pushes` (`lib/pushes.py`) checks that an app that has
listed nothing hears of no change, that its list holds the change, and
that it hears of the next one. Then the Board runs with `slow-load`: the
first list is answered after 3 s, from a snapshot taken before the wait. A
card made during the wait is shown once, and counted in its column. **8 of
8.**

**Red**, on the debug host. With every push passed on, as before:
`FAIL before its first list the app hears of no change (pushes: 1)`, 7 of
8. With held pushes dropped instead: `FAIL a change made while the first
list was on its way is shown`, `and shown once (0 times)`, `the column
counts it`, 5 of 8.

### The hostile script

`tests/apps-bench/hostile` runs `tests/apps-bench/hostile-app/app.lua` on
the release host at 120×40. The script is the prototype's
`tests/hostile/app.lua` at `api = 1`. All 29 checks passed:

- The 19 ways out are blocked: file, module, bytecode, native code,
  environment, process, exit, debug, coroutine and the terminal. The
  script's globals are the contract's.
- Each of these is ended, and the app then answers the next key: an
  endless loop (255 ms), a loop that catches its stop (254 ms), a pattern
  that never ends (255 ms), endless recursion, and endless memory (host
  peak 72 MiB, against the cap of 64 MiB).
- Records outside the fence are refused. None of those requests reaches
  the service, and its records are untouched. Of two pushes, only the one
  of the declared type arrives.
- Three stops in a row give the app up. The host goes on drawing, and a
  save starts the app again.
- Ctrl+C leaves while the script loops: in 121 ms, with status 0, the
  terminal restored and no process left.

These are the runtime's claims, and they were not made red here. M1-A made
them red with the unit tests it names (`tests::reach`, `tests::hostile`,
`tests::running`); this run shows the same behaviour in a real terminal.

### Numbers

`tests/apps-bench/measure` on the release host, with the `big` data set
(200 records). The prototype's column is `f1/s-embedded` from
`RESULTS-2.md`, "Medidas": the principal's runs, at 80×24.

| | Prototype, 80×24 | bzz, 80×24 | bzz, 40×16 |
|---|---|---|---|
| Board: first complete draw | 9.5 ms | 11.3 ms | 12.0 ms |
| Board: peak memory of the process | 5.5 MiB | 6.0 MiB | 5.8 MiB |
| Board: key to screen, p95 (120 keys) | 0.96 ms | 1.0 ms | 0.92 ms |
| Panel: first complete draw | 13.2 ms | 16.9 ms | 14.3 ms |
| Panel: peak memory | 6.2 MiB | 6.4 MiB | 6.1 MiB |
| Panel: key to screen, p95 | 3.0 ms | 4.37 ms | 4.37 ms |
| Reload, save to screen, median | 29.9 ms | 26.9 ms (p95 48.9) | 28.6 ms (p95 49.2) |

Each figure is from one run, on a machine running another agent's builds.
bzz's runtime does more than the prototype's on each call: the hook that
stops a call, the bounded library, raw reads of the script's tables, and
its own layout. The first draw is 2 to 4 ms slower and a Panel key 1.4 ms
slower at the 95th percentile. These gaps were not looked into.

### The six apps: whose script

| App | From | Why this one |
|---|---|---|
| Board | M1-A's fixture, from `f1/s-embedded/apps/board` | the only one |
| Poll | written for this task from `spec/poll.md` | nobody had written it in Lua |
| Rota | `f3/S-rota-1` (about half of its lines remain) | both deliveries passed 14 of 14 on their first run, and both draw the confirmation inside the column. S-rota-1 leans less on `seq`, which `api = 1` removed (7 uses against 10; S-rota-2 holds pushes that come before a slow list and replays them by `seq`), so less of it had to change; it is also the shorter (248 lines against 263) |
| Approvals | `f3/S-approvals-3` | the only one of three written after `data.null` existed. S-approvals-1 and -2 failed case 03 on the missing null; S-approvals-3 passed 15 of 15 on its first run. It keeps the selection by record id, and a decision travels in the request's tag, so a conflict that finds the request still pending decides it again |
| Panel | `f3/S-panel-2` | both passed 15 of 15. S-panel-2 chooses its layout from `ctx.width` in three places, where S-panel-1 does in one |
| Board with priority | the Board fixture, with the priority added; the change follows both deliveries | the owner's example is a change to the running Board: the swap must find the same state, keys and layout. Both deliveries changed the experiment's Board, which this fixture no longer is |

Rota, Approvals and Panel were rewritten for `api = 1`. With no `seq`, a
list that predates a change must not undo it, so the apps track the ids
they changed since they asked. Order comes from a field, and the apps read
`ctx.user.name` and `p.by.name`. Each keeps about half of its source's
lines. The earlier agent made this choice and recorded no reason; the
reasons above are mine, from the deliveries and their result notes.

### Gates

On `846b89b`, run one after the other. Only this document changed
afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings` | clean |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | 966 passed, 0 failed, 35 ignored, in 59 test programs |
| Without the features | `cargo test --locked --no-default-features --lib` | 458 passed |
| Supply chain (`Cargo.toml` changed, no dependency) | `cargo deny check`; `cargo audit` | `advisories ok, bans ok, licenses ok, sources ok`; no vulnerability, 5 allowed warnings |
| The bench | `scripts/test-apps-bench.sh --release --measure` | everything passed (the tables above) |

### Not proven in M1 (bench and apps)

| Item | State |
|---|---|
| The bench on macOS, Windows | it reads `/proc` and drives a Linux pseudo-terminal. The cargo tests of the six apps are what CI runs there; they were not run on those platforms here |
| The example host on macOS, Windows | built and run on Linux only. It uses nothing specific to Linux |
| The red of the hostile script on the host | the runtime's reds are M1-A's unit tests; they were not repeated on the host |
| The numbers | one run of each, on a shared machine. The prototype's were measured on the same machine two days earlier, not beside these |

### Found on the way

- **The fixtures store people by name** (`holder = "ana"`, and `author`,
  `decided_by`, `acked_by`, `voter`), because the bench's data and cases
  do. The contract says to compare and store the id. A record can name a
  person only in a field, and the contract gives an app no way to show the
  name of an id that has not signed a record it holds. The templates of D9
  (M4) need an answer: store `{ id, name }`, or the id and a name the app
  learned. The briefs say so.
- **The earlier agent's unfinished script worked as it was.** Its only
  fault was that `--help` also printed a line of the script itself.
- **A host must apply "pushes begin with the first list" itself** when its
  records come from something that pushes from the start; for bzz, the
  session does it. The table at the top of `examples/app_host.rs` now says
  so.

## M2 (service) — The records of apps, in bzz's artifact service

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-18. Branch
`t3/m2-a-records`, cut from `v0.16.0` at `5eabee9`. This is the half of M2
that knows the relay: no Lua, no pane, no command. A later task (M2-B) joins
it to the runtime. The interface is written down in
[`app-records.md`](app-records.md).

**Where it is.**

| Path | What |
|---|---|
| `src/protocol/record.rs` (feature `apps`) | names, the fence, the mapping between a record and its revisions, the title rule, the checks before sending, what a refusal means, the difference between two sets |
| `src/service/records.rs` (feature `apps`) | one typed sync for each open app; create, revise, delete; the `Writer` |
| `src/store/artifacts.rs` | `replace_artifact_types`, `typed_artifacts`; the channel listing leaves app records out. No migration: the schema stays at 20 |
| `src/realtime/admission.rs`, `supervisor.rs` | why a sync was closed; the publication class `App` |
| `src/app.rs` | the artifact panel past the relay's bound; an app's subscription is not the status line's |
| `tests/support/scripted_relay.rs` | `push`: a frame the relay was not asked for |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo test --locked --lib -- protocol::record service::records store::artifacts realtime::   # unit
cargo test --locked --test app_records_test                                                 # scripted relay
cargo test --locked --test session_test a_closed_sync
cargo test --locked --lib app::tests::artifact_sync_commits                                 # the panel
# pinned relay, Docker; only the M2 test, then the whole suite:
BZZ_RELAY_TEST_FILTER=real_relay_app_records BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh
```

**Red runs.** Each "red" below is the named test run against the code with
one thing taken away (it is in the cell), seen failing with the quoted
message, then restored with `git checkout`. `U:` is a unit test in the named
module, `T:` a test of `tests/app_records_test.rs`, which drives the real
service, supervisor and store against the scripted relay.

### Unit: a record and its events

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| A record maps to a create, a whole-snapshot update over the head and an envelope-only delete, in the shape the pinned relay accepted in M0; integers, floats and `null` survive; the author is whoever signed the revision | `U: protocol::record::a_record_and_its_revisions_map_both_ways` | `prev` naming the record instead of the head: the relay's own validator refuses it (`artifact envelope: event ID must be 64 lowercase hex characters`) | pass |
| Tags another client put on a record, and its thread anchor, are carried by a revision; content that is not a JSON object is no record | `U: …::what_another_client_wrote_is_carried_or_left_alone` | tags not carried: `left: [] right: [("assignee", "p1"), ("k", "v")]` | pass |
| Names are `[a-z][a-z0-9_-]{0,31}`; an app declares 1 to 16 types, none twice; the longest names give a type the relay's validator takes | `U: …::names_follow_the_rule_of_the_contract` | no check of the first letter: `2048` is a name | pass |
| Content is at most 65,536 bytes of JSON, counted after escaping | `U: …::size_is_checked_before_anything_is_sent`; `T: nothing_is_sent_that_bzz_knows_would_be_refused` | no content limit: both fail (``called `Result::unwrap_err()` on an `Ok` value``; in the scripted test the oversized record is sent and stored) | pass |
| A revision whose escaped frame would pass 524,288 bytes is stopped, also when its content and tags each pass the relay's validator | the same `U:` (65,536 control characters and sixteen long tags: a frame of about 778,000 bytes) | no frame check: ``called `Result::unwrap_err()` on an `Ok` value: ()`` | pass |
| **The fence.** A type that was not declared, the long form, another app's type: refused. A head of another type or channel is not a record, cannot be revised or deleted, and a signed revision that names another channel or type is stopped | `U: …::a_fence_gives_only_its_types_in_its_channel`, `the_fence_holds_for_heads_and_for_signed_revisions` | the fence taken out of `Fence` (every type and channel admitted): both fail | pass |
| **The title rule.** The record's `title` field when it is a string with something to show, control characters to spaces, trimmed, cut at 512 bytes on a character; otherwise `<app> <record>`. The relay's validator takes every result | `U: …::the_title_is_the_title_field_or_the_names` | no cut: the check of the length fails on a title of 600 bytes; no fallback for a blank title: `left: "" right: "board card"` | pass |
| **The difference on a resync.** The same set again is no change; a revised record is one upsert, a missing one is one removal, a new one is one upsert, an unchanged one is not mentioned | `U: …::a_resync_shows_only_what_really_differs` | every record of the new set told: `assertion failed: difference(&held, &held.clone()).is_empty()` | pass |
| A refusal is read for what it means: the wait of a rate limit; a stale head; the two other `conflict:` refusals, which are not one; an archived channel, a guest, a non-member; the relay's words cleaned and cut | `U: …::a_refusal_is_read_for_what_it_means` | every `conflict:` taken for a stale head: `left: HeadChanged right: Gone`; no rate limit: `left: Refused { … "rate-limited: quota exceeded; retry in 40s" } right: RateLimited { seconds: 40 }` | pass |
| An unacknowledged write says whether it was on the wire | `U: service::records::a_write_without_an_acknowledgement_says_whether_it_was_sent` | a dropped connection reported as "not sent": `assertion failed: reason(error).ends_with("it may still have been stored")` | pass |

### The store

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| A typed set replaces only its types in its channel: another type of the same app, another app, a task and another channel are as they were; a head outside the set's own types or channel is not stored; no deletion row is kept | `U: store::artifacts::a_typed_set_replaces_only_its_types_in_its_channel` | the type left out of the delete: another type's row is gone (`left: [] right: [d913fcee-…]`) | pass |
| An app's records are read whole, past the listing's 500 | `U: store::artifacts::an_apps_records_are_read_whole_and_stay_out_of_the_channel_listing` (620 records) | `LIMIT 500`: `left: 500 right: 620` | pass |
| The channel listing, which the artifact panel shows, leaves out `bzz.app.<app>.<record>` and keeps `buzz.task`, `bzz.app` and `bzz.apple.note` | the same | no such condition: the listing is 500 cards and no task | pass |

### The supervisor

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| `resync:`, `error: sync set exceeds max_events` and `restricted:` are told apart; what reaches the owner of the subscription is a fixed text, never the relay's; each is final for the supervisor | `U: realtime::admission::a_closed_sync_says_which_of_three_things_happened`; `session_test::a_closed_sync_reaches_its_owner_with_its_reason_and_not_the_relays_words` (scripted relay) | as in v0.15: `left: TerminalProtocol right: Resync`, and on the wire `left: "invalid: relay closed the subscription" right: "resync: relay asked for a new sync"` | pass |
| An app's write goes after a message, recovery, maintenance and the selected channel's subscription; it waits for the person's unacknowledged publication, and the person's never waits for an app's | `U: realtime::supervisor::an_app_write_goes_after_everything_of_the_person_and_holds_none_of_it_back` | next table | pass |
| App writes have a queue of their own (32) that the person's queue does not count; a closed quota gate answers every waiting one with the wait, rounded up; one that waited the queue's 25 s is a wait too, not a local error | `U: realtime::supervisor::app_writes_have_their_own_queue_and_become_a_wait_instead_of_expiring`, `realtime::admission::a_deferred_app_write_carries_whole_seconds_rounded_up` | app writes in the person's queue: `left: 32 right: 0` | pass |

### On the scripted relay

`tests/app_records_test.rs`, nine tests, real sockets and real time. Run 60
times, ten at once, with no failure.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **A conflict returns the current record.** The relay refuses a stale head without the head. The app's own sync delivers the winner: the answer is `Conflict` with it, no HTTP request is made, and the retry on it is stored with `prev` naming the winner | `T: a_conflict_returns_the_current_record_from_the_sync_or_over_http`, part 1 | the head not looked up: `left: "refused" right: "conflict"` | pass |
| When the sync does not deliver it, bzz waits 300 ms and reads the head over HTTP (one `POST /query`, `{"artifact":"head","#d":[id]}`); a deletion found there is `NotFound` and a removal told; a revision the caller no longer holds is a conflict without a frame; `artifact head unavailable` and `artifact identity is taken` cause no read | the same, parts 2 to 4 | no HTTP read: `Unavailable { reason: "the record changed, and its current state could not be read" }` where `Conflict` was expected; every `conflict:` taken for a stale head: one HTTP read too many (`left: 3 right: 2`) | pass |
| **`rate-limited: quota exceeded; retry in 40s` is a wait of 40.** The next three writes are answered within 2 s with 35 to 40, and the relay receives one frame in all | `T: the_relays_quota_becomes_a_wait_in_seconds` | the refusal not read: `left: Refused { … } right: RateLimited { seconds: 40 }`; the gate not consulted for app writes: the next write is held in bzz's queue for 25 s and answered with a wait of `1` (the test took 25.33 s) | pass |
| **A resync gives no false deletion.** After `CLOSED resync:` the caller's set is unchanged while bzz asks again; the request is sent again no sooner than 250 ms, with the same filter; an identical set is told as no change; a set the relay ends before `EOSE` is not used; a set that differs is told as exactly its differences; a watcher that read the set every 2 ms never saw fewer records than the smallest real set | `T: a_resync_tells_only_real_differences_and_never_empties_the_set` | the set cleared on `resync:`: `nothing is emptied while bzz asks again left: []`; no delay: `asked again only after a short delay`; no difference computed: `unexpected Changes { changes: [Upsert(…"b"…` for an identical set | pass |
| A reconnect is the same to the caller, also one the person asked for | the same, last part | `Connecting` ignored: the state never leaves `Live` (`an event of the records in time`) | pass |
| **`sync set exceeds max_events` and `restricted:` are states.** `TooMany { max: 4096 }`: no second request in 1.5 s, a create is `Limit` without a frame, the part of the set that arrived is not shown, `retry()` asks again. `Restricted`: no second request, a write is refused without a frame. A relay without the sync: `Unavailable`, and no request at all | `T: too_many_records_and_no_access_reach_the_caller_as_states` | the overflow retried like a resync: `TooMany` never arrives; with the supervisor as in v0.15 the same test fails the same way | pass |
| **An app's writes do not delay a message.** With twenty app writes waiting, a message is sent after at most three of them and all twenty are stored after it. With an app write on the wire that the relay never acknowledges and four more waiting, a message is sent and acknowledged within 2 s | `T: an_apps_writes_do_not_delay_a_persons_message` | app writes in the person's queue: the message is the twenty-first (`20 of the app's 20 writes went out before the person's message`); one in-flight slot for both: `the message did not wait for the app's write: Elapsed(())` after 3 s | pass |
| **The fence at the service.** An undeclared type is `fenced` for the owner and for another writer, before anything is signed. A task, another app's vote and the same app's card in another channel, all in the local store and all sent by the relay unasked (in the set and live), never reach the caller and cannot be revised or deleted by either writer. No frame is sent; those rows are untouched. A kind 45011 is believed only from the pinned relay key. One instance of an app per channel | `T: the_fence_refuses_at_the_service_whoever_asks` | the fence taken out: the caller is told four records (`left: 4 right: 1`) | pass |
| Nothing is sent that bzz knows would be refused: over 1000 live records (an unreadable one counts), content too large for a create and for a revision, an unknown record, a stale revision. Six requests, no frame. Then a delete is sent as the relay wants it, and `channel is archived`, a guest and a non-member are told as such | `T: nothing_is_sent_that_bzz_knows_would_be_refused` | no limit: the create is sent and stored (``called `Result::unwrap_err()` on an `Ok` value: Record { … "one too many"``); no local conflict: the delete is sent on the real head and stored (the same, `"card 0"`) | pass |
| The caller's own writes are answered by their call and told nothing more, whether the relay echoes them after or before its `OK`; a write by another `Writer` is signed by that key and told once; the store has each at once and keeps no row of a deleted record; a record created while bzz fetches the set again does not vanish and come back | `T: the_callers_own_writes_are_answered_once_and_another_writers_are_told` | the echo not recognised: `unexpected Some(Changes { changes: [Upsert(… "Echoed first"`; the cut not corrected: `unexpected Changes { changes: [Removed { … "During the resync"` | pass |
| After a restart the same records, revisions and authors are there as `open` returns, with a relay that does not answer; when it does, only what changed meanwhile is told | `T: a_restart_shows_the_stored_records_before_the_relay_answers` (a store on disk, opened twice) | the store not read at `open`: `the same records, revisions and authors left: []` | pass |

### The artifact panel past 4096

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| When the relay ends the channel-wide sync with `error: sync set exceeds max_events`, the panel keeps the last complete set, shows `too many artifacts here to sync · list may be stale`, and schedules no retry; a dropped connection does not turn that back into "syncing" | `app::tests::artifact_sync_commits_only_at_eose_and_never_touches_chat` | as in v0.15 (a retry scheduled): `assertion failed: over.overflowed && !over.complete && !over.blocked` | pass |
| The closing of an app's records subscription does not reach the status line | the same | no such rule: `assertion failed: !changed && app.status_error.is_none()` | pass |

Before, the panel asked again every 60 to 90 seconds for ever and received
4096 heads each time. Now one attempt is made each time the channel is
selected.

### On the pinned relay

`real_relay_app_records_are_shared_by_two_profiles`, in
`tests/relay_integration.rs`, so `scripts/test-relay.sh` runs it with the
suite. Two generated identities, each with its own store on disk, connection
and records service, in a private channel one of them made; a guest and a
stranger; an agent of the first. Relay members with the role `member` (the
suite's `seed_member` makes relay owners).

| Claim | Observed | Result |
|---|---|---|
| A change by one arrives live at the other, with who signed it; the writer is answered by its call and told nothing | `Upsert` of the same record, author the creator; 500 ms of silence at the writer | pass |
| Both revise the same record at once, five rounds: one is stored, the other gets `Conflict` with the winner's record, its retry on that record is stored, and both profiles end on a record holding both edits | five rounds, each `(Ok, Err(Conflict))` one way or the other | pass |
| A deleted chat message ends both syncs; both ask again, are told of no change and never hold fewer records | `Resyncing` then `Live`, two records throughout | pass |
| A type that was not declared is refused and the relay never sees it | `fenced` three times; the relay's count of the channel is 2 before and after | pass |
| After closing and opening bzz again the same records are there before the relay answers; the relay's set then causes no change | the set read from the store equals the other profile's, by id, revision, content and author | pass |
| A deletion arrives live with who deleted | `Removed { by: the other profile }` | pass |
| A guest reads and is refused as a guest; a stranger to the private channel gets `Restricted` and its write is refused without a frame | `Refused { Guest }`; `Restricted`; nothing told to the others, the count unchanged | pass |
| **At the relay's own quota the caller gets the wait.** An agent of the first profile (the role `bot`, its own key, its own connection) writes through her open app | 120 writes stored, the 121st `RateLimited { seconds }` with 1 to 60; the next answered within 2 s with no more than that; her own write stored at that moment; all 122 records in both profiles | pass |
| An archived channel refuses a write and still answers the sync; archiving and unarchiving each end the sync, with no change told | `Refused { Archived }`; `Resyncing`, `Live`, twice; a write stored after | pass |

The suite raises a person's quota to 5000 a minute for its other tests and
leaves an agent's at the relay's default, which is why the quota is met as
an agent. That write path is the one M7 will use.

Four runs on this branch, each under the harness lock, each torn down by
the script (no `buzz-harness` container, no listener on 3030, no
`buzz-relay` process and no tmux server afterwards):

- 15:41 to 15:43, the M2 test alone: passed in 27 s.
- 16:11 to 16:13, **red**: with the lookup of the current record taken out,
  `round 0: Refused { refusal: Other, reason: "the relay reported a conflict
  for a new record" }`.
- 16:15 to 16:31, the whole suite, on `ad6009e`: 23 passed in 870 s (the 22
  of M0 and this one; the one with the owner's own programs skipped itself),
  then `agent_host_test` 7 and `agent_move_test` 8.
- 17:01 to 17:02, the M2 test alone on `60079e4`: passed in 27 s.

### The v0.15 behaviour of `:artifacts` and `:tasks`

| Claim | Proof | Result |
|---|---|---|
| The panel's sync, rename, conflict, delete, restore and move on the pinned relay | `real_relay_channel_artifacts`, unchanged, in the whole-suite run above | pass |
| Commit only at `EOSE`, the set over the bound never committed in part, `resync:` keeps the set and schedules a retry, the rendered panel | `app::tests::artifact_sync_commits_only_at_eose_and_never_touches_chat`: its v0.15 assertions are unchanged, the two rows above were added to it | pass |
| The store's `EOSE` replace, restore snapshot and move removal; "My tasks" | `store::artifacts::eose_replaces_the_set_and_deletions_keep_the_restore_snapshot`, unchanged | pass |
| Envelope edits and their reconciliation | `protocol::artifact::tests` (7), unchanged | pass |

One thing a user of the panel can see differently: an artifact of a type
`bzz.app.<app>.<record>` is no longer listed. None existed before v0.16.

### Gates

On `60079e4`, 2026-11-18, from 16:52 (UTC−5). Only this section changed
afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | 768 passed, 0 failed, 31 ignored, in 57 test programs. 26 passing tests are new (9 of `protocol::record`, 1 of `service::records`, 2 of the store, 2 of admission, 2 of the supervisor, 1 of `session_test`, 9 scripted); the new ignored one is the relay test |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 458 passed (6 new: the store, admission and the supervisor are not behind the feature) |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |
| Pinned relay | above | the whole suite on `ad6009e`: 23 passed. The M2 test alone again on `60079e4`, 17:01 to 17:02: passed in 27 s. Between the two, one message of `service::records` changed (the first table's last row) |

In this worktree `scripts/test-relay.sh` was run with a `cargo` on the
`PATH` that sets `CARGO_TARGET_DIR` to the main checkout's `target` only
inside the worktree, so that Buzz at the pin builds in its own folder.

### Not proven in M2 (service)

| Item | State |
|---|---|
| **The person's quota.** The relay has one budget for an identity's chat and its apps' writes | not solved. The class `App` keeps an app's writes out of the way inside bzz. It does not keep them from spending the relay's 60 a minute: an app that does leaves the person's next message refused by the relay until the minute passes. bzz does not know the relay's number (NIP-11 does not say it). See the report for what to do |
| A person's own quota of 60 on the pinned relay, through the service | not run: the suite raises it. The agent's 120 was met; a person's 60 was measured in M0 with the bare client |
| `TooMany` and the limit of 1000 on the pinned relay | scripted only. M0 measured the relay's side at 4097 |
| A kind 45011 removal, and a record moved to another channel, on the pinned relay | scripted only |
| Frames missed inside bzz (the broadcast receiver lagging) | written (ask for the whole set again), not tested |
| A real drop of the connection on the pinned relay | the scripted relay only, through `reconnect` |
| An identity whose key Buzz Desktop holds | not run. Kind 45010 is in the provider's table of kinds it signs unasked |
| A community with federated identity | not run |
| macOS and Windows | not compiled or run here |
| The time and memory of a sync of 1000 records through the service | not measured. The scripted test commits 999 heads; M0 measured 265 ms for the relay's part |
| An artifact of an app's type whose content is no JSON object | it is counted and hidden, in the app and in the panel: nothing in bzz can delete it |

### Found on the way

- **A reconnect the person asks for (`:reconnect`) sends no `Disconnected`.**
  The supervisor says only `Connecting`. The artifact panel's sync therefore
  stays "complete" across it and takes the set the relay sends again as live
  heads: an artifact deleted during the gap stays listed until the next
  resync. v0.15 behaviour, left as it is; the records service takes
  `Connecting` as the end of the old sync.
- **`rate-limited: local admission …`, bzz's own error for a publication it
  never sent, parses as a relay rate limit of 10 s** in
  `admission::rate_limit_retry_after`. The records service asks
  `is_local_admission_error` first.
- **The scripted relay could not send a frame unasked.** It can now
  (`ScriptedRelay::push`), to WebSockets only.

## M2 (session) — An app on real records

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-19. Branch
`t3/m2-b-session`, cut from `v0.16.0` at `f3ab248`. This is the second
half of M2: the runtime of M1 and the records service of M2 (service),
joined into **an app session**, and milestone M2's exit gate with two bzz
profiles on the pinned relay. No pane, no command, no store of scripts:
what owns a session is the next task (M3).
[`app-records.md`](app-records.md), "The session", is the interface in
words; [`app-api.md`](app-api.md) is the contract a script sees.

**Where it is.**

| Path | What |
|---|---|
| `src/apps/session/mod.rs` | `AppSession`: one open app in one channel. The owner's calls, the rules between a script and its records, the status |
| `src/apps/session/backend.rs` | the boundary to the records, in the service's types |
| `src/apps/session/relay.rs` | that boundary on `service::records`, from a thread that must not wait |
| `src/apps/session/memory.rs`, `headless.rs` | the store in memory and the headless driver, moved here from `apps::runtime` and put behind the session. `apps::runtime::{memory, headless}` still name them |
| `src/protocol/record.rs` | content at 16,384 bytes; `WriteBudget`, 30 a minute; a record with no fields |
| `src/service/records.rs` | `budget()`, `is_open()`, `owner_pubkey()`; no "unreadable" records |
| `src/apps/runtime/limits.rs`, `records.rs`, `mod.rs` | the limits that are the service's are its constants; `Runtime::waiting()` |
| `tests/app_session_test.rs`, `tests/support/app_pane.rs` | a session on the scripted relay, read as screens |
| `tests/support/records_world.rs` | the scripted world of `tests/app_records_test.rs`, shared |
| `tests/relay_integration.rs`, `scripts/test-relay.sh` | the two tests on the pinned relay; `BZZ_RELAY_OWN_QUOTA`, `BZZ_RELAY_NOCAPTURE` |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo test --locked --lib apps::                          # the session in memory, the runtime, the Board
cargo test --locked --lib protocol::record
cargo test --locked --test app_session_test               # a session on the scripted relay
cargo test --locked --test app_session_test a_sync_of_a_thousand -- --nocapture
cargo test --locked --test app_records_test frames_missed
# pinned relay, Docker:
BZZ_RELAY_NOCAPTURE=1 BZZ_RELAY_TEST_FILTER=real_relay_app_session BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh
BZZ_RELAY_OWN_QUOTA=1 BZZ_RELAY_NOCAPTURE=1 BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh
```

### What the join settled

Where the runtime and the service disagreed the contract decided; where the
contract was silent or wrong it was changed, with its twin
(`app-api.d.lua`), and the test that holds the three together
(`apps::runtime::tests::contract`) was extended.

| Question | Settled as | Contract |
|---|---|---|
| Content of a record: 65,536 (service) or 16,384 (contract) | 16,384 for every writer of this bzz: `protocol::record::MAX_CONTENT_BYTES`, which `runtime::limits` now names | unchanged |
| Types, names, live records, title bytes | one constant each, the service's | unchanged |
| The title rule: `title_of` (runtime) or `title` (service) | the service's, which the relay's validator was tried on: control characters to spaces, trimmed, cut at 512 bytes, else `<app> <type>`. `match` looks at it | changed: it said "the type's name" |
| The resync rule: `differences` (runtime) or `difference` (service) | the service's, also in the store in memory; the runtime's is gone. A deletion found in a resync is pushed as by the record's last signer | said |
| `list` | from the session's set, with `match`, at once, also offline. With no record held it waits for the first complete set, 5 s at most | changed: it said every request is `unavailable` without a connection |
| When pushes begin | with the app's first list of a type. Otherwise the records an app starts with arrive as one push each and then as the list | new |
| Identities | `{ id, name }`: the public key; the name its owner's profiles give, a short form of the key until one is known. When a name is learned or changes, that person's records are pushed again as updates with the same `rev` | new |
| An app's writes | 30 a minute for the apps of one person, half of the relay's 60; past it `rate_limited` with the wait and no frame. One budget for a person's open apps, not one each: two apps with 30 each would be the whole 60 | changed: it said 60 |
| `fenced` (service) has no code in the contract | `bad_request` | unchanged |
| An artifact of an app's type whose content is no JSON object | a record with no fields: listed, counted, replaceable, deletable. It was hidden, counted, and nothing in bzz could delete it | new |
| What the pane says by itself | one sentence in `status().notice`: the wait, too many records, an archived channel, no access, records not available | new |
| A version with other types | the records are opened again for them | new |

### Unit: the session in memory

`U:` is a unit test under `apps::session::tests::`, on the store in memory:
the session is the one the relay path uses, and these run on every
platform. Each red is the named test with one thing taken out of
`src/apps/session/mod.rs` (it is in the cell), seen failing with the quoted
message, and restored.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **An undeclared type is refused at the session.** Asked as if the runtime had let it through: a create and a list of another type, the long form of a declared one, an update and a delete that name another type or a record of another type. Each gets exactly one reply at once, `bad_request` or `not_found`; nothing reaches the records | `U: an_undeclared_type_is_refused_at_the_session` | the check of the type off: the request goes on to the store (`request 0: left: 0 right: 1` replies at once) | pass |
| A list is answered from the session's set, with `match` over the title bzz gives the relay: case ignored, control characters as spaces, a record without a title found by `ledger note`, nothing past the cut at 512 bytes, another type not searched | `U: a_list_is_answered_from_the_sessions_set_with_match` | `match` over the raw `content.title`: `left: ["match 0:"] right: ["match 1:   brief_notes_"]` | pass |
| A script hears of a type from its first list of it; the records an app starts with are no pushes | `U: a_script_hears_of_a_type_from_its_first_list_of_it` | every change pushed: four `push create` before `list 3` | pass |
| Who is the public key and the name bzz knows: a short form (`c0c0c0c0…c0c0`) until then; a name learned or changed hands that person's records over again with the same revision, and nobody else's; the local participant's new name is in `ctx.user` | `U: who_is_an_id_and_the_name_bzz_knows` | no record handed over again: `left: "cy's by c0c0c0c0…c0c0" right: "cy's by cy"` | pass |
| **An app's writes stop at 30 a minute.** The thirty-first is `rate_limited` with 60 s and is not sent; an update and a delete count; a list does not; the pane's sentence counts down with `tick()`; a minute after the first writes it writes again | `U: an_apps_writes_stop_at_half_the_persons_budget`, `the_board_shows_the_wait_and_the_pane_shows_it_too` (the Board fixture: `Too many changes. Retry in 60 s.` on its own status line, the card back in its column) | the budget not asked: `left: ["create ok mine 31"]` | pass |
| The apps of one person share that budget: two apps, forty writes in turn, thirty sent | `U: the_apps_of_one_person_share_one_budget` | the same red: forty sent | pass |
| The relay's own `rate_limited` is a wait the pane shows, and nothing is sent while it lasts | `U: the_relays_quota_is_a_wait_the_pane_shows` | the answer's wait not kept: `left: None right: Some("Too many changes: this app can write again in 40 s.")` | pass |
| An app that holds no record waits for its first set and gets it as one reply, with no push; after 5 s without one the list is `unavailable` | `U: an_app_that_holds_nothing_waits_for_its_first_set` | the list answered at once: `the list waits: left: ["list 0"]` | pass |
| After a restart the list has what bzz kept before the relay answers, a kept record can be written, and the relay's set then gives only what differs; with nothing differing the screen is the same | `U: a_restart_shows_what_bzz_kept_and_then_only_the_differences` | the stored set not taken at opening: `left: [] right: ["list 2"]` | pass |
| `TooMany`, `Restricted`, `Unavailable` and `Resyncing`: the sentence of each, what a list and a write are answered, and that nothing bzz knows will fail is sent | `U: each_state_of_the_sync_has_its_sentence_and_its_answers` | `TooMany` not looked at before a create: `no frame for the create: left: 1 right: 0` | pass |
| An archived channel: told by its owner, a write is `refused` without a frame; told by the relay's refusal, the pane says so until the relay takes a write or the sync starts anew | `U: an_archived_channel_says_so_and_refuses_before_a_frame` | the owner's word not looked at: `left: ["create ok mine 1", "update ok edited", "deleted edited", "list 3"]` | pass |
| A change by someone else that is told before the answer to the app's own earlier write is not lost: the reply carries the app's revision, and the record as it is now follows it; the same for a deletion; in the usual order nothing is told twice | `U: a_change_told_before_the_answer_to_an_own_write_is_not_lost` | nothing after the reply: the script ends on its own, older revision (`left: [.., "update ok edited"]`) | pass |
| A new version that declares other types gets their records, keeps the ones it had, and no longer hears of a type it dropped | `U: a_new_version_with_other_types_gets_other_records` | the records not opened again: `left: ["tags 0:"] right: ["tags 1: a card"]` | pass |
| 500 changes found in one resync never fill the queue of a script that may be owed 64 events | `U: a_set_fetched_again_never_fills_the_scripts_queue` | all sent at once: `the queue was never filled` (the app is "not responding") | pass |
| A session can be owned by a task (`Send`) | `U: a_session_can_be_owned_by_a_task` | — (it would not compile) | pass |
| The 99 tests of M1-A, the Board's twelve cases among them, pass through the session: the headless driver has no rules of its own any more | `cargo test --lib apps::runtime` | while it was written: `case_08` selected `Echo` (the stored set arrived as pushes after a failed list); `every_error_code` listed a record the session had never been told was deleted | pass: 99 |
| The budget itself: thirty at once, the wait until the oldest leaves the minute, never more than its number in any minute, none at all | `U: protocol::record::an_apps_writes_have_a_budget_of_half_the_persons` | — (new) | pass |
| An artifact that is no JSON object is a record with no fields, which can be replaced and deleted; on the scripted relay it counts as the thousandth record and its deletion is sent | `U: protocol::record::what_another_client_wrote_is_carried_or_left_alone`; `T: nothing_is_sent_that_bzz_knows_would_be_refused` (`tests/app_records_test.rs`) | as it was in M2 (service): no record, `app.unreadable() == 1`, and no call that could name it | pass |
| The content limit of 16,384: the largest revision the relay's validator passes is one frame (about 491,000 bytes), so the frame check cannot fire at the pin | `U: protocol::record::size_is_checked_before_anything_is_sent` | — (a measurement; with one tag more the relay's own validator refuses first: `total tag bytes exceeded`) | pass |

### On the scripted relay

`T:` is a test of `tests/app_session_test.rs`: a script, the runtime, the
session, the real records service, supervisor and store, real sockets, and a
relay that says what the pinned relay was measured to say. The app is drawn
into a buffer of 40 by 16, the size of a docked app, and read as text
(`tests/support/app_pane.rs`). These run on every platform.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **A resync changes nothing on the screen.** The Board with three cards; the relay ends the sync (`CLOSED resync: redaction`) and sends the same set: every screen drawn meanwhile is the screen from before, the pane says nothing, and the state goes `live`, `syncing again`, `live` with one new request. A set that differs shows exactly its differences, and the script was not made to load again | `T: a_resync_changes_nothing_on_the_screen` | a join that empties its set when the sync ends: `the screen during a resync left: "To do (0)\nDoing (0)\nDone (0)…"` | pass |
| **`TooMany`, `Restricted` and an archived channel each show their sentence.** `error: sync set exceeds max_events`: the sentence, the part of the set that arrived is not shown, a new card is `The board is full.` without a frame, no second request. `restricted:`: the sentence, `Load failed.`, a new card refused without a frame. `invalid: channel is archived` to a move: the app's `Refused: this channel is archived…`, the card back in its column, the sentence; once its owner says so, the next move costs no frame | `T: too_many_restricted_and_archived_each_show_their_sentence` | no sentence: `left: None right: Some("This app has more than 4096 records here, too many to load. None can be added.")` | pass |
| **A message of the person is not delayed by an app's writes.** A script asks for forty writes at once: a message sent meanwhile goes out after at most three of them; thirty frames of the app reach the relay and ten requests are answered `rate_limited` (1 to 60 s) by bzz; five more asked are five more refused; the person's next five messages are each acknowledged within a second. With a relay that never acknowledges the app's write, a message is acknowledged within 2 s and the pane still draws | `T: a_message_of_the_person_is_not_delayed_by_an_apps_writes` | the budget not asked: `stored 32, told to wait 8` (the supervisor's own queue of 32 is then the only bound, and the relay's 60 the next) | pass |
| **Frames missed inside bzz** (left untested in M2 (service)). With bzz's store busy and the task of the app's sync waiting for it, the relay sends a head and 2,600 more frames, more than a listener is kept (2,048). The head in hand is told live; then `Resyncing`, one new request, the missed head as the one difference, `Live` | `T: frames_missed_inside_bzz_are_made_up_by_a_whole_new_set` (`tests/app_records_test.rs`) | the lag ignored: `an event of the records in time: Elapsed(())`, the missed record never told | pass |
| A new version that declares other types, on the relay: the sync of the old types is closed, one request follows for both under the same subscription, the script's list is answered, the record of the new type arrives, and a write of the new type is stored | `T: a_new_version_with_other_types_opens_its_records_again` | — (the red of the unit test above is the same change) | pass |
| A second instance of an app in a channel: its script is answered `unavailable`, the pane says `the app board is already open in this channel`, the relay is asked nothing; when the first is gone, `retry()` opens it | `T: a_second_instance_in_the_channel_is_told_and_opens_when_the_first_is_gone` | — (new) | pass |

The first four of these, the ones that existed then, were run 40 times, ten
at once; the scripted tests of the service 30 times, ten at once; and the
session's unit tests 20 times. None failed.

### On the pinned relay

`real_relay_app_session_two_profiles_share_a_board`, in
`tests/relay_integration.rs`, so `scripts/test-relay.sh` runs it with the
suite. It is milestone M2's exit gate to the letter. Each profile is a
generated identity with its own store on disk, connection, records service
and app session; relay members with the role `member`; a private channel one
of them made. The app is the Board fixture
(`tests/fixtures/apps/board/app.lua`), driven by keys through the session
and read as screens of 40 by 16.

| Claim | Observed | Result |
|---|---|---|
| **A change by one arrives live on the other's screen** | Ana presses `n`, types a title, Enter: her screen has the card under `To do (1)` and `Saved.`; Ben's screen has it 110 to 154 ms after her keys. Ben makes a card and moves it with `]`: Ana's screen has it under `Doing (1)`. Both screens list the same cards in the same columns | pass |
| **A simultaneous change loses nothing** | Five rounds. The card is in the middle column on both screens; Ana presses `]` and Ben `[` before either request is carried, so both moves are made from the same revision. Each round exactly one status line says `Saved.` and the other `Conflict: merged with a newer change.`; both screens then show the card in the column of the one that merged; both sessions hold one record at one revision, signed by the one that merged; the relay still counts two artifacts. In three rounds both requests are carried at once: Ana's was the one stored, every time, in every run. In two rounds Ben's is carried 60 ms sooner, and Ana is the one told. Merged by `ben, ana, ben, ana, ben` | pass |
| **A restart shows the same records** | Ana's session and store are closed. Opened again with nothing to reach (the relay's address replaced by a closed port): the screen has both cards in their columns and `Loaded 2 cards.`, and the sync stays `loading` for the half second watched. Opened again with the relay: every screen that says `Loaded 2 cards.` has the same cards, seen while the state was `loading` and then `live`, and for half a second after; it is what Ben's screen shows | pass |
| **An undeclared type is refused, at the session and at the service** | At the session, asked past the runtime: a create of `vote`, a create of the long form `bzz.app.board.card`, a delete of a real card named as a `vote`. At the service, asked past the session with its own handle: `vote`, the long form and `buzz.task` are each `fenced`. The relay's count of the channel is 2 before and after, and the session holds 2 records | pass |
| **At the quota the pane shows the wait: the app's own budget** | Dan has written nothing. His app asks for forty records at once: `stored 30`, `told to wait 10 (1 to 60 s)`, the pane's sentence `Too many changes: this app can write again in 57 s.`; the relay counts 32 artifacts, so it never saw the ten; his chat message is accepted at that moment | pass |
| **At the quota the pane shows the wait: the relay's own quota** | A profile whose identity is an agent of Ana's (an owner attestation, the role `bot`), for which the suite does not raise the relay's quota. Its chat sends 120 messages, all accepted (15.0 s). The next write of its app is refused by the relay: `stored 0`, `told to wait 1 (1 to 60 s)`, the sentence `…write again in 45 s.`; a second write is answered within 2 s without a frame; the relay's count is unchanged; two seconds later the wait shown is smaller | pass |

`real_relay_app_session_meets_a_persons_own_quota` is the same for a
**person**, whose quota the suite raises to 5000 for its other tests. It
runs only under `BZZ_RELAY_OWN_QUOTA=1`, which leaves the relay's quota as
it ships and runs this test alone; in the suite it prints that it was
skipped.

| Claim | Observed | Result |
|---|---|---|
| A person's own sixty writes a minute, shared by chat and apps | Eva makes a channel and, in the Board, a card: two writes. Her chat then sends messages eight a second: the relay takes sixty writes in all, in 14 s, and answers the next `rate-limited: retry in 45s`. She moves the card: the Board's own line says `Too many changes. Retry in 45 s.`, the card is back under `To do (1)`, and the pane's sentence is `Too many changes: this app can write again in 45 s.`; the relay still counts one artifact | pass |

Runs on this branch, each under the harness lock and torn down by the
script (no `buzz-harness` container and no listener on 3030 afterwards, as
the wrapper recorded after each):

- 09:50 to 09:53, the gate alone, first run: passed in 31 s. Its binary
  was built while a red edit was in the tree ("Found on the way"); it is
  not counted.
- 09:55 to 09:57, `BZZ_RELAY_OWN_QUOTA=1`: passed in 19 s.
- 13:30 to 13:35, three **red** runs of the gate, each with one thing taken
  out of the session and restored after:
  - a change not pushed: `timed out waiting for ana's card on ben's
    screen`, with `To do (0)`;
  - the stored set not taken at opening: at the restart with no relay,
    `timed out waiting for Loaded 2 cards.`, the Board saying `Load failed.
    Press r to retry.` and the sync `loading`;
  - the budget not asked: `timed out waiting for forty answers`, with
    `stored 32`, `told to wait 8 (1 to 60 s)`.
- 14:12 to 14:28, the whole suite on `96eda85`: 25 passed in 904 s (the 23
  of M2 (service), the gate, and the person's quota, which skipped itself,
  as the one with the owner's own programs does), then `agent_host_test` 7
  and `agent_move_test` 8. 14:28, `BZZ_RELAY_OWN_QUOTA=1`: passed in 18 s.
- 14:36 to 14:37, the gate alone with Ben made sooner in two rounds: passed
  in 31 s.
- 14:44 to 15:00, **the whole suite on `c1f3a0e`**: 25 passed in 900 s,
  then 7 and 8. 15:00 to 15:01, `BZZ_RELAY_OWN_QUOTA=1` on `c1f3a0e`:
  passed in 18 s. The numbers in the two tables are these runs'.

In this worktree the script was run with a `cargo` first on the `PATH`
that sets `CARGO_TARGET_DIR` to the main checkout's `target` only inside
the worktree, as in M2 (service), and `BZZ_BUZZ_SOURCE` at the main
checkout's `runs/buzz-pin`.

### A sync of 1000 records through a session

`T: a_sync_of_a_thousand_records_through_a_session`, on the scripted relay:
the Board at 80 by 24, a store on disk, 1000 cards signed by another
participant. Each number is the time until the screen shows it, with the
test's loop looking every 3 ms. Three runs in a release build, five in a
debug build; the spread was under 3 %.

| | Release | Debug |
|---|---|---|
| The first open: nothing kept, one list reply with 1000 records, no push | 253 ms | 780 ms |
| A resync in which all 1000 records differ: 1000 pushes, sent as the script takes them | 2.15 s | 4.2 s |
| A restart with a relay that says nothing: the screen from the store | 80 ms | 420 ms |
| Resident memory of the test process, before | 17.2 MiB | 24.5 MiB |
| after the first sync (the service's set, the store, the session's set, the interpreter) | 28.6 MiB (+11.4) | 36.3 MiB (+11.8) |
| after the thousand pushes | 35.5 MiB (+18.3) | 43.2 MiB (+18.7) |

The two seconds of the resync are the Board's own: it sorts its thousand
cards once for each push, on the script's thread. The owner's calls did not
wait for it: the app stayed `responding`, and bzz never has more waiting
for a script than half of what the script may be owed.

### Gates

On `c1f3a0e`, 2026-11-19 from 14:37 (UTC−5), one after the other. Only
this document changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | 884 passed, 0 failed, 35 ignored, in 58 test programs. New here: 16 of `apps::session`, 1 of `protocol::record`, 6 of `tests/app_session_test.rs`, 1 of `tests/app_records_test.rs`; 2 of `apps::runtime::records` went with the functions they tested; the 2 new ignored are the relay tests |
| Tests without the features | `cargo test --locked --no-default-features --lib`; `cargo build --locked --no-default-features` | 458 passed; builds |
| `apps` alone | `cargo test --locked --no-default-features --features apps --lib` | 584 passed, 2 ignored |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |
| Pinned relay | above | the whole suite on `c1f3a0e`: 25 passed, then 7 and 8; the person's quota alone: passed |
| Repeats | the scripted tests of the session 40 times and of the service 30 times, ten at once; `apps::session` 20 times | no failure (on `ef89cd1`, before the test of a second instance was added) |
| Release | `cargo test --release --locked --test app_session_test a_sync_of_a_thousand -- --nocapture`, three times | passed; the numbers above |

### Not proven in M2 (session)

| Item | State |
|---|---|
| A session inside bzz's own loop | M3. The session was driven by a test's loop and by the headless driver; its waker was a test's. `app-records.md` says how it fits `App::run`; nothing in `src/app.rs` was changed |
| Names from bzz's profiles | the session takes names from its owner (`set_names`) and says whom it lacks (`unnamed()`); that the pane feeds it from the profile store and looks the others up is M3. On the pinned relay the names were given; the Board shows none |
| Where the pane's sentence is drawn | M3. It is `status().notice`, asserted as text; the app's region is not made smaller for it |
| The two simultaneous moves on the wire | each round one move was stored and the other was told of the conflict and merged. Whether the relay refused the second frame or bzz answered it from the head it already held was not told apart: both are a conflict to the script, and M2 (service) proved each |
| "Loses nothing" for two different fields | the Board changes one field, the column: the merged record has the loser's column on top of the winner's revision. Two fields both kept were shown at the service (M2 (service), five rounds) |
| The relay's own quota through a session, for a person, inside the suite | only under `BZZ_RELAY_OWN_QUOTA=1`, a second start of the relay. In the suite it is met as an agent's identity |
| `TooMany`, `Restricted` and an archived channel on the pinned relay through a session | scripted. M2 (service) saw `Restricted` and `Archived` at the pinned relay's service |
| A write whose task ends without an answer | written (`unavailable`, so that the request has its one reply); no test ends a task |
| A list that waits more than five seconds for a slow relay and gets its set later | the unit test only: the script is then told nothing of the set until it lists again |
| A drop of the connection under a session | the service's own tests (M2 (service)); under a session only `CLOSED resync:` |
| A record larger than 16,384 bytes written by another client | delivered as it is; a thousand of them would not fit the interpreter's memory, which the contract says |
| macOS, Windows, Linux on arm64 | not compiled or run here. The unit tests and the scripted tests run on every platform in CI's build job; the relay tests on Linux |
| The six apps on the bench, the example host | M1-B, in another worktree. Its host should run an app through `AppSession::in_memory`, as the headless driver now does |

### Found on the way

- **The rules of the store in memory were a second implementation.** It
  answered lists itself, by its own title rule, and computed a resync by
  its own function. Behind the session it is a relay and nothing more:
  two of M1-A's tests showed the difference at once. `case_08` of the Board
  expected no card after a failed list and got six pushes; a test of error
  codes listed a record deleted "while the app was not told", which a
  session only learns at the next resync, as on the relay.
- **Without the app's budget the next bound is the supervisor's queue of
  32**, not the relay's 60: forty writes at once gave `stored 32, told to
  wait 8`, on the scripted and on the pinned relay.
- **The check of the frame's size cannot fire at the pin** once content is
  16,384 bytes: the relay's validator refuses more than about 65,000 bytes
  of tags, and the largest revision it passes is about 491,000 bytes of
  the 524,288 a frame may be. It stays, for a relay that takes more.
- **Which card the Board selects after a resync depends on the order of
  the pushes** when the selected card is among the deleted: the
  differences come in the order of the records' ids. The first version of
  the scripted test asserted the selection and failed two times in three.
- **`scripts/test-relay.sh` gives a test no way to print.** The times
  above are printed by the tests; `BZZ_RELAY_NOCAPTURE=1` shows them.
- **A record with no fields is invisible in the Board fixture**, which
  shows a card only in a column it knows. An app that means to clean up
  after other clients has to list what it does not understand.
- **A red run must not overlap a relay run of the same checkout.** The
  relay script builds from the working tree: the first relay run of this
  branch was compiled while a red edit of `service/records.rs` was in
  place for another test. It was run again on the final commit.

## M4 (store and command line) — Where apps are kept, and `bzz app`

Linux x86_64, Rust 1.98.1, 2026-11-19. Branch `t3/m4-store-cli-1`, cut
from `v0.16.0` at `d0dbb36` (after M2 (session)), with the six commits of
an earlier agent's branch `t3/m4-store-cli` (cut before M2 (session))
merged in. This is the half of M4 that needs no window: the store of
installed apps, the rule that nothing runs unapproved, and the command
line. The approval overlay and `:app` in the TUI are M4-B, after the pane
(M3). [`apps.md`](apps.md) is the user's guide.

**Where it is.**

| Path | What |
|---|---|
| `migrations/0021_apps.sql`, `src/store/apps.rs` | migration 21: `apps`, `app_versions`, `app_channels`, and their queries |
| `src/apps/store/mod.rs` | `Apps`: propose (`examine`, `record`), `review`, `approve`, `refuse`, `failed_to_run`, `rollback`, `remove`, and `runnable`, the one function that hands out a script to run |
| `src/apps/store/manifest.rs` | the manifest (`app.toml`) and the brief: formats, limits, `visible` |
| `src/apps/store/review.rs`, `diff.rs` | what the owner is shown; the line difference |
| `src/apps/store/folder.rs`, `templates.rs`, `src/apps/templates/board/brief.md` | a folder of the owner's; the templates (the Board) |
| `src/apps/dev.rs` | `try` and `dev`: a script on records in memory, through an `AppSession` |
| `src/cli_app.rs`, `src/main.rs`, `src/paths.rs` | `bzz app …`; `<data>/apps`; `community remove --purge` removes the community's apps |
| `tests/app_cli_test.rs`, `tests/secret_scan_test.rs`, `tests/store_migration_test.rs` | the command line end to end, the secret scan, migration 21 |
| `docs/apps.md`, `docs/security.md` | the guide; "v0.16.0 trust boundaries" |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.

**What was decided here** (the plan and the brief left these open):

| Question | Decided |
|---|---|
| Files or database | **Files hold bytes, the database holds decisions.** A version's three files are written once under `<data>/apps/<community>/<name>/versions/<nnnn>/` and never changed; which version runs, the approved hash, who proposed each and what became of it are rows. Every read of a version compares the file with the hash recorded when it was proposed, so an edit on disk is found, and a copy of the folder elsewhere approves nothing |
| Manifest or script | **The script is the authority** for `api` and `types`: bzz loads it and takes both from there, refuses a folder whose `app.toml` says otherwise, and writes the manifest it keeps itself. `name` and `title` are only in the manifest. An unknown field is an error |
| Types a script computes | the approved types are kept with the version; `Runnable::check_loaded` holds what the runtime reports at load to them, for the pane (M4-B) to call |
| `approve`, `refuse`, `pending`, `rollback` at the shell | they exist: the owner at his shell is the owner. Without a terminal an approval must name the script's whole hash; `add` then prints the command |
| Versions kept | the one that runs, the one before it, the ten newest approved, every waiting one; lines without files: the fifty newest. A refused or failed version keeps no files |
| Proposers | one waiting version per proposer (a later one supersedes it); at most eight waiting for one app |
| A version that fails as it is put to run | `failed_to_run` records it and makes the version that ran before current again |
| `dev --channel` | refused in this version, with the reason; what M4-B must add is in `docs/apps.md` |

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo test --locked --lib apps::store            # the store, manifest, review, difference
cargo test --locked --lib store::apps            # migration 21's queries
cargo test --locked --lib apps::dev              # try and dev
cargo build --locked --example scripted_harness  # the secret scan needs it
cargo test --locked --test app_cli_test --test store_migration_test --test secret_scan_test
```

### Claims

`U:` is a unit test under `apps::store::tests::` (on a store on disk in a
temporary folder), `apps::dev::tests::` or `store::apps::tests::`; `C:` is
`tests/app_cli_test.rs`, which runs the `bzz` binary with a temporary data
directory and no terminal, and asks `Apps::runnable` of the same directory
after each command. Each red is the named test run against the one change
in its cell, seen failing with the quoted message, and the file restored
(`git checkout`); the runs are logged in the worktree's `/tmp`, not kept.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **A version that was not approved is never handed to a runtime**, tested at `Apps::runnable`: not installed, waiting (also the owner's own), refused, failed to load, rolled back to while unapproved, and disabled. Approved, it hands out exactly the approved bytes, version, hash and types | `U: a_version_that_was_not_approved_is_never_handed_to_a_runtime` | `runnable` taking the newest version and not looking at its state: `unwrap_err() on an Ok value: Runnable { … version: 1 … }` for the owner's waiting version | pass |
| **Bytes changed on disk after approval do not run, and bzz says why**: `tasks: app.lua of version 1 is not what was approved: the recorded hash is …, and the file on disk has …. It was changed outside bzz, and it is not used`; one byte more is enough; a file removed is `Missing`; the approved bytes put back run again; an edited brief or manifest is found where it is shown, and the approved types are the database's, not the file's. A waiting version whose script is swapped on disk cannot be approved and is recorded as failed | `U: bytes_changed_on_disk_after_approval_do_not_run_and_bzz_says_why`, `approval_is_of_exact_bytes`; `C:` (`show` says `it does not run: …`) | the comparison with the recorded hash off: `unwrap_err() on an Ok value: Runnable { … script: … "edited behind bzz" … }` | pass |
| **A proposal that fails to load is not offered and keeps the old version**: recorded as `failed` with the error and its file and line, no files kept, not in `waiting`, not approvable, and the approved version still the one `runnable` gives; the same for an empty brief, a brief over 16 KiB, a script over 128 KiB, `api = 2`, an error in the first `view`, and an empty script. A folder whose manifest disagrees with its script fails the same way (`a_folders_manifest_must_say_what_its_script_says`) | `U: a_proposal_that_fails_to_load_is_not_offered_and_keeps_the_old_version`; `C:` (`add` of a broken folder: `it does not load, and nothing was changed (recorded as version 3, failed)`) | a load error ignored: `Waiting { version: 2, … }` where `Failed` was expected | pass |
| **A change of the declared types is marked in what the owner is shown**: a first version says all are new; the same types (in any order) say so; other types show `CHANGED from the version that runs:` with `+ vote   new: …` and `- tag   no longer declared: …` | `U: a_change_of_the_declared_types_is_marked_in_what_the_owner_is_shown`; `C:` (the same lines from `bzz app add`) | `TypesChange::between` comparing only the number of types: `left: Unchanged right: Changed { added: ["vote"], removed: ["tag"] }`. A first red, the review comparing the new types with themselves, failed earlier: `left: Unchanged right: First` | pass |
| What the owner is shown besides: who (you, an agent, a person, with the key), the brief and whether it changed, sizes, the line difference from the version that runs; nothing a proposer wrote can draw on the terminal | `U: the_owner_is_shown_who_the_brief_the_sizes_and_the_difference`; `review::tests`, `diff::tests`, `manifest::tests::what_is_not_text_is_written_out` | — (inherited, run green; no red taken) | pass |
| **Rollback restores the earlier version's exact bytes**, and its approved types; back twice, then forward to a named version; nothing earlier is said; an earlier version whose file changed is not gone back to | `U: rollback_restores_the_earlier_versions_exact_bytes`; `C:` (`rollback`, then `runnable` gives version 1's bytes) | every version written to one folder: `rollback … unwrap() on an Err value: Changed { name: "board", version: 2, … }` | pass |
| An approved version that fails as it is put to run leaves the previous one current | `U: an_approved_version_that_fails_as_it_is_put_to_run_leaves_the_previous_one`; `store::apps::tests::a_version_that_fails_as_it_is_put_to_run_gives_way_to_the_one_before` | — (inherited, run green; no red taken). No runtime calls it yet: M4-B | pass |
| **`remove` leaves the channel's records alone, and says so**: the app, its versions, its channels and its folder are gone; every artifact is still there, the app's own among them; the count it reports is the app's types only (`bzz.app.board.*`, not `bzz.app.board-2.*`); an app of the same name installed again finds them | `U: remove_leaves_the_channels_records_alone_and_says_so`; `C:` (`2 records of types bzz.app.tasks.* are stored by this bzz, in 1 channel`, and `typed_artifacts` still 2) | `remove` also deleting artifacts of `bzz.app.<name>.`: `left: 3 right: 6` artifacts | pass |
| **Migration 21 on a v0.15 database: old data intact.** A database at schema 20 with an identity, a community, an agent's request and stored reply and an artifact opens at 21; a backup at 20 is made first; every row of every table v0.15 had is as it was; the three tables are empty; an app can be recorded | `T: store_migration_test::schema_twenty_of_v015_gains_the_apps_tables_and_keeps_every_row`; `fresh_database_has_expected_pragmas_and_schema` (21) | migration 21 taken out of `src/store/migrate.rs`: `no such table: app_channels` | pass |
| Migration 21 on a profile a real old release wrote | `scripts/upgrade-test.sh` (v0.14.0 writes a profile; `upgrade_from_v0140_test` opens it, migrations 18 to 21) | migration 21 taken out: `migrations 18 to 21 ran (17 -> 20)` | pass; see "The upgrade from a real profile" |
| **The command line end to end**, with a temporary data directory: `new` (and `--templates`, a folder that exists, a bad name), `check` (an error with `app.lua:11:`, a manifest that disagrees), `try` (keys, typing, a seed, a size), `add` (it waits: `runnable` is `NothingApproved`), `pending`, `approve` (refused without a terminal and without a hash, refused with another hash, done with the shown hash), `list`, `show`, `show --script` (the exact bytes), `add` of the same folder (nothing to approve), an update with its difference, a failing update, a change of types, `refuse` with a reason, `rollback`, an edit on disk, `disable`, `enable`, `dev` without a terminal and with `--channel`, `remove` | `C: the_command_line_from_a_new_folder_to_an_uninstalled_app`; `C: a_command_that_needs_a_community_says_so_and_the_others_need_none` | `add` approving by itself where there is no terminal: `` `tasks: version 1 waits for your approval. Nothing of it runs until then.` in: … `` | pass |
| **No key or credential appears in anything a script or these commands print.** A profile with a real owner key sealed in its file, its passphrase, and a broker credential; every `bzz app` command run on it; a script that writes down every global, its `ctx`, and tries `os.getenv`, `io.open("/proc/self/environ")` and `require("os")`. None of the secrets and no environment variable of bzz is in what was printed or in any file of the profile; the three ways out gave nothing | `T: secret_scan_test::no_key_reaches_a_script_or_anything_the_app_commands_print` | `try` printing bzz's environment: `an environment variable of bzz leaked into try` | pass |
| **`app dev` reloads on save**: a save draws the new version with the state of the old (keys, records); a broken save leaves the old version running and shows the error; a file gone for a moment is said once; the fix runs | `U: apps::dev::tests::a_save_reloads_the_running_app_and_a_broken_save_leaves_it_running` | the saved bytes not given to the session: `left: "first keys=2 notes=2" right: "second keys=2 notes=2"` | pass |
| The same in a real terminal | below | — | pass |
| The store's folders and files are private (`0700`, `0600`) | `U: an_apps_folder_and_files_are_private` | — (inherited, run green) | pass |
| Every template loads apart and declares what its entry says | `U: every_template_loads_and_declares_what_its_entry_says` | — (inherited, run green) | pass |

The other inherited tests of the store, the manifest, the difference and
the queries (42 in `apps::store` and `store::apps`, 4 in `apps::dev`) were
run green; for those without a red in the table, no red was taken in this
task.

### `bzz app dev` in a terminal

The debug binary of `31fda03`, in tmux on a private socket (100 by 30), with
throwaway `BZZ_*_DIR`s: `bzz app new tasks`, then `bzz app dev tasks`.

| Step | Seen |
|---|---|
| start | the Board in a border of 42 by 18, `To do (0)`, `Loaded 0 cards.`; under it `…/app.lua · running · not reloaded yet · 0 records in memory` and `Ctrl+C leaves. Every other key is the app's. Saving the file reloads it.` |
| `n`, `Buy milk`, Enter | `To do (1)`, `> Buy milk`, `Saved.`; `1 record in memory` |
| `"To do"` replaced by `"Backlog"` in the script, saved | `Backlog (1)`, `> Buy milk` still there; `reloaded once` |
| `return app(` appended, saved | the screen as it was, the error `app.lua:379: <eof> expected near 'return'` in the app's region and under it; `reloaded 2 times`, still `running` |
| the good script saved again, then `Ctrl+C` | the terminal is the shell's again; printed: the log (`reloaded; state kept`, the error, `(reload failed: the previous version keeps running)`, `reloaded; state kept`) and `left …/app.lua. Its records were kept in memory and are gone. To install it: bzz app add …`; exit status 0 |

The tmux server was killed afterwards; no `bzz app dev` process was left.

### The upgrade from a real profile

`scripts/upgrade-test.sh` builds v0.14.0 (`80e1b07`) in a temporary
worktree, lets it write a whole profile (settings with a hosted agent of
v0.13, its sealed key, a database with messages, at schema 17), and runs
`tests/upgrade_from_v0140_test.rs` against it with this branch. The test
now expects migrations 18 to 21 (`user_version` 21); the rest of what it
checks (the backup at the old version, nothing v0.14.0 showed lost or
changed, the hosted agent served and never started) is as in v0.15.0.

| Run | Result |
|---|---|
| `31fda03`, 16:14 to 16:17, the old release's target under `/media/arpagon/archive-data/…/m4a-upgrade/` | `v0140_profile_upgrades_with_its_hosted_agent_served_and_nothing_lost ... ok` |
| red: migration 21 taken out of `src/store/migrate.rs` | `migrations 18 to 21 ran (17 -> 20)`, `left: 20 right: 21` |

No profile written by v0.15.0 itself was opened: the script has no seed
for v0.15.0 (`tests/upgrade/seed_v0150.rs` does not exist), and writing one
is M8's upgrade work. Migration 21 only adds tables, and the test above
compares every row of every table of a schema-20 database before and after
it.

### Gates

On `31fda03`, 2026-11-19 from 16:05 (UTC−5), one after the other. Only
this document changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | 934 passed, 0 failed, 35 ignored. New here against `v0.16.0`: 16 of `apps::store::tests`, 19 of `apps::store::{manifest, review, diff, folder}`, 7 of `store::apps`, 4 of `apps::dev`, 2 of `tests/app_cli_test.rs`, 1 of `tests/secret_scan_test.rs`, 1 of `tests/store_migration_test.rs` |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 465 passed |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |
| Pinned relay | not run: nothing here reaches a relay | |
| Upgrade | `scripts/upgrade-test.sh` (from v0.14.0) | passed; above |

### Not proven in M4 (store and command line)

| Item | State |
|---|---|
| A script added from a folder runs **in the pane** only after approval; an approval reaches an open app without a restart; a failing update keeps the old version **running** | M4-B. Proven at `Apps::runnable`, the only door the pane is to use; nothing calls `failed_to_run` or `Runnable::check_loaded` yet |
| The approval overlay, `:app` commands | M4-B |
| `bzz app dev --channel` | refused; `docs/apps.md` says what M4-B must add |
| Poll and Approvals as templates (D9) | the mechanism and the Board ship; the two fixtures are on the bench's branch (`tests/fixtures/apps/{poll,approvals}/app.lua`), not on `v0.16.0` yet. Where they go is in `templates.rs` and `docs/apps.md` |
| Herdr on the release binary (install, approve, roll back) | M4-B's row. `dev` was seen in tmux on a debug binary |
| macOS, Windows | not compiled or run here. File permissions are `0700`/`0600` on Unix only, as bzz's other data |
| A profile written by v0.15.0 itself | no seed for v0.15.0 in `scripts/upgrade-test.sh`; M8 |
| Two bzz processes changing the store at once | each change is one SQLite transaction; not tested with two processes |

### Found on the way

- **The shared `target/` builds the same `libbzz` for every worktree.** The
  hash of the artifacts does not depend on the checkout's path, so a
  `cargo test` here once linked another worktree's newer `libbzz` and
  failed on `could not find store in apps`: cargo judged it fresh because
  the sources here were older than its output. Touching `src/lib.rs` and
  `src/main.rs` before each cargo command made every run build this
  checkout's own. Every run counted above was made that way.
- **`AppSession` replaced `MemoryStore::serve`.** The earlier branch's
  `dev` served the script's requests from the store itself; after M2
  (session) it runs through `AppSession::in_memory`, as the headless driver
  does, and shows the session's notice under the app.
- The earlier branch's test of migration 21 makes its schema-20 database by
  dropping the new tables from a new one. The upgrade from a profile a real
  release wrote was run with `scripts/upgrade-test.sh` (v0.14.0); there is
  no seed for v0.15.0 yet.

## M3 — The app pane

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-19. Branch
`t3/implement-m3-pane`, cut from `v0.16.0` at `4a765df` (before M1-B was
merged). An app inside bzz's window, beside the conversation: the layout
of M0's trial (q5), focus and keys, bracketed paste, `:apps` and `:app`,
one session per open app in the loop, and M3's exit gate on the release
binary with the pinned relay. The approval overlay is M4-B's.

**Where it is.**

| Path | What |
|---|---|
| `src/ui/layout.rs` | `panes_with_app`: the app pane, zoom, and the order of admission (the right-side pane asked for last, then the sidebar, the other one, the rail) |
| `src/ui/app_pane.rs` | the rules that need no app: `key_owner`, `paste_target`, `app_paste_text`, the pane's border, the "too small" sentence |
| `src/app/pane.rs` | the sessions in bzz's loop: start from `Apps::runnable`, waker, poll, tick, names, `failed_to_run`, `:apps`, `:app open/close/zoom`, keys, paste, drawing |
| `src/app.rs` | the loop's arms (`Background::AppWake`, the tick, poll before a frame), `handle_key`, paste, mouse, render, the status line, the composer's target |
| `src/ui/{state,action,keymap,hit_map,command,status,terminal}.rs` | `FocusSurface::App`, the focus cycle, `5` and `Space 5`, `HitTarget::App`, the commands, `APP` and the leave hint, bracketed paste and the panic hook |
| `src/app/pane/tests.rs` | the pane inside `App`, on Ratatui's test backend, on records in memory |
| `scripts/tui-herdr-scenarios.toml`, `scripts/test-tui-herdr.sh`, `scripts/test-tui-tmux.py`, `scripts/test-relay.sh` | the `t16-*` scenarios; the Herdr runner reads their fields; the same manifest through tmux (`--tmux`) |
| `examples/harness_fixture.rs` | `looper/`, an app that keeps its thread busy |
| `docs/apps.md`, `docs/app-api.md`, `docs/e2e-herdr.md` | the pane; what `host.quit()` is in bzz; the app scenarios |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.

**What was decided here** (the plan and the brief left these open):

| Question | Decided |
|---|---|
| A session whose channel is not selected | kept alive, so the app's state is there on return and its records keep syncing; at most four, the one shown least recently is closed and opened again from the store's records. All end on a switch of community, `:lock`, quit, and when bzz has no connection |
| The focus when another channel is selected | back to the conversation if that channel has no app (render checks it) |
| Several apps in one channel | the store can hold several; the pane shows one: `:app open` of another closes the first there |
| `host.quit()` | `:app close`: the pane closes and the app is no longer open in that channel (`docs/app-api.md` says so) |
| Under 40×16 | bzz's sentence, and `view` is not called; zoom when the docked pane is short and zoom gives 16 rows |
| Zoom | `:app zoom`, and `Space 5`; it applies only while the app has the focus; `Ctrl+C` leaves it |
| The thread between 74 and 101 columns | for everyone, app or not: the pane asked for last goes before the sidebar, so a thread asked for is drawn from 74 columns and the sidebar gives way; no rail beside a sidebar that gave way. The composer replies into a thread only while it is drawn (`reply_root`) |
| A paste with no composer and no app focused | dropped, as before |
| A paste into an app | the composer's rule (`sanitize_pasted_text`): CR and CRLF to LF, controls dropped, its size bounded |
| A panic on an app's thread | the panic hook leaves the terminal alone and prints nothing for a thread named `bzz-app …`; the runtime already catches it and the pane says so |
| Windows | crossterm 0.29 gives no paste event there: a paste arrives as keys, in the app as in the composer (`docs/apps.md`). Not run |
| Names | from bzz's profiles: all at start, and on every tick those `unnamed()` lists that bzz has. Profiles bzz does not have are not fetched for the app |
| Command completion | bzz has none for any command; `:apps`/`:app` are in `docs/apps.md`, `5` and `Space 5` in the generated help |

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo test --locked --lib ui::layout ui::app_pane ui::status ui::terminal ui::action app::pane
cargo build --locked --release
ln -sfn "$CARGO_TARGET_DIR" target            # test-relay.sh uses ./target
BZZ_TMUX_SCENARIOS=t16-cli-app-new,t16-cli-app-add,t16-cli-app-approve,t16-cli-app-add-second,t16-cli-app-add-looper,t16-app-pane,t16-app-live-second-profile,t16-app-looper-composer \
  BZZ_TMUX_CAPTURES=/tmp/captures BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-relay.sh --tmux
BZZ_TMUX_SIZE=200x50 BZZ_TMUX_SCENARIOS=harness BZZ_BUZZ_SOURCE=… ./scripts/test-relay.sh --tmux   # every harness scenario
rm target
```

### Unit

`U:` is a unit test; each red is the named test run with the one change in
its cell, seen failing with the quoted message, and the file restored
(`git checkout`). The runs are `/tmp/m3/reds.sh` and `reds2.sh` of the
worktree, not kept.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **With no app open the layout is v0.15's**, compared with a copy of v0.15's function over 65,160 cases (widths 40 to 220, five heights, every combination of rail, sidebar and thread, three pairs of widths, three docks), except where v0.15 left an open thread undrawn: there the thread is drawn at its width and the conversation keeps 30 | `U: ui::layout::tests::with_no_app_open_the_workspace_is_laid_out_as_in_v015` | the narrow body at 72 instead of 70: `left: (Some(Rect { … width: 22 …}), None, Rect { x: 22, … width: 48 …` `right: (None, None, Rect { x: 0, … width: 70 …` | pass |
| A thread asked for is drawn from 74 columns, the sidebar giving way, and from 102 both | `U: a_thread_asked_for_is_drawn_from_74_columns` | v0.15's order (the sidebar before the thread): panic at `layout.rs:493` (`thread drawn`) | pass |
| The trial's thresholds: 72 docks, 100 the sidebar too, 122 the rail; thread and app 116 and 144, everything 166; the one asked for last in front, the focused app always in front; 80×24 gives the conversation 38 and the app 40×18 | `U: the_thresholds_of_the_default_widths_are_the_trials` | the focused app not always in front: `left: HiddenBehindThread right: Docked` | pass |
| A focused app that cannot dock takes the window; docked under 16 rows it is zoomed when that gives 16 (19 rows); zoom on request only while focused | `U: a_focused_app_that_cannot_dock_takes_the_window_and_leaves_it_with_the_focus` | no zoom for a short dock: `left: Docked right: Zoomed` | pass |
| The panes never overlap, fill the body in order, keep the conversation's 30; the focused app is never undrawn; no rail beside a sidebar that gave way (171 widths × 6 heights × 64 requests) | `U: the_app_pane_never_overlaps_and_the_conversation_keeps_its_thirty_columns` | the rail shown beside a sidebar that gave way: panic at `layout.rs:584` | pass |
| The exit gate's widths: 144, 100, 72, 60 | `U: the_widths_of_the_exit_gate` | — (written after the rule; no red taken) | pass |
| **Who owns a key**: with the app focused every key but `Ctrl+C` is the app's (53 keys, among them `q ? : / i j k h l 1 2 3 4 5 [ ]`, Space, Enter, Esc, Tab, Shift+Tab, arrows, F1, F6, `Ctrl+U/Y/N/D/G/V/O/J`, `Alt+x`); `Ctrl+C` (and Ctrl+Shift+C) leaves | `U: ui::app_pane::tests::with_the_app_focused_every_key_but_ctrl_c_is_the_apps` | Ctrl+C given to the app: `left: App right: LeaveApp` | pass |
| An overlay, then the composer, own every key before the app | `U: overlays_and_the_composer_come_before_the_app` | the app before the composer: `left: App right: Composer` | pass |
| `Ctrl+Y` opens a waiting command first, and is the app's when nothing waits | `U: ctrl_y_opens_a_waiting_command_before_everything_and_is_the_apps_otherwise` | — (no red taken) | pass |
| **Where a paste goes**: the agent form, a refusal's reason, the composer, the app only when nothing else takes text, else nowhere | `U: a_paste_goes_to_the_app_only_when_nothing_else_takes_text` | the app before the composer: `left: App right: Composer` | pass |
| A pasted line break reaches an app as LF; an escape is dropped | `U: a_pasted_line_break_reaches_an_app_as_lf` | — (no red taken) | pass |
| The border says how to leave, the notice is in the bottom border and costs no row (inside 40×18 of 42×20); under 40×16 bzz says so | `U: the_border_says_how_to_leave_and_the_notice_costs_no_row`, `under_forty_by_sixteen_bzz_says_so` | — (no red taken) | pass |
| The status line names `Ctrl-c` and never offers `q quit` while an app has the keyboard, from 40 to 160 columns | `U: ui::status::tests::with_an_app_focused_the_status_line_says_how_to_leave_it` | the app ignored: panic at `status.rs:230` | pass |
| The app pane is in the Tab cycle only while drawn; `5` and `Space 5` are effects | `U: ui::action::tests::the_app_pane_is_in_the_focus_cycle_only_while_it_is_drawn` | the pane left out of the cycle: `left: Timeline right: App` | pass |
| A panic on an app's thread leaves the terminal alone | `U: ui::terminal::tests::a_panic_on_an_apps_thread_leaves_the_terminal_alone` | the name compared whole: panic at `terminal.rs:157` | pass |
| `:apps`, `:app …` parse | `U: ui::command::tests::theme_reload_is_an_explicit_command` | — | pass |
| **Keys and a paste reach the focused app and nothing else**, in bzz's own `App`: `q : 1 5 i ? Esc Tab Enter` reach the app, no overlay, composer or quit; a paste of two lines arrives as one, with LF; `Ctrl+C` leaves and the status line shows `? help · q quit` again; then the composer and an overlay own the keys and a paste, and the app gets nothing more | `U: app::pane::tests::keys_and_a_paste_reach_the_focused_app_and_nothing_else` | keys not sent to the session: `no "keys q : 1 5 i ? esc tab enter" on:`; the paste not given to the app: `no "pasted line one/line two" on:` | pass |
| The pane follows the window and its channel: 80×24 docked 40×18; 60 columns zoomed while focused, hidden (`app hidden · 5`) otherwise; zoom at 160×40; under 40×16 bzz's sentence and no `view`; another channel shows no app and gives the focus back; coming back finds the session | `U: the_pane_follows_the_window_and_its_channel` | `view` called under 40×16: panic at `tests.rs:256` | pass |
| The thread and the app share the right side; the composer replies into the thread only while it is drawn | `U: the_thread_and_the_app_share_the_right_side` | `reply_root` ignoring whether the thread is drawn: `left: Some("eeee…") right: None` | pass |
| The hit map has the pane at the far right; the wheel elsewhere keeps the app's focus; the wheel over the app does nothing | `U: the_mouse_focuses_the_pane_and_the_wheel_keeps_the_apps_focus` | the pane not in the hit map: `left: None right: Some(App)`; the wheel taking the focus: `left: Timeline right: App` | pass |
| A script stuck in a call costs the window nothing: twenty keys, polls and frames under 1.5 s with the script looping; the composer types twelve characters meanwhile | `U: a_looping_script_costs_the_window_nothing` | — (no red taken) | pass |

### The exit gate, on the release binary

**Run through tmux, not Herdr.** `herdr --session t3code pane list`
answered `server_not_running`; the session was not started. The scenarios
are written in the manifest's form, and `scripts/test-tui-herdr.sh` reads
their new fields, but that runner has not run them: its `size` (by
`stty` on the pane's terminal), `paste` (by `send-text` with the bracket
sequences) and second pane are untried. `scripts/test-tui-tmux.py` ran the
same manifest on a private tmux socket (tmux 3.6), on the release binary of
this branch, with a disposable profile from `examples/harness_fixture.rs`
on the pinned relay (`scripts/test-relay.sh --tmux`: the lock taken, the
harness torn down after; `docker ps` showed no `buzz-harness` container
after each run). The Board was written by `bzz app new`, proposed with
`bzz app add` (answered no), approved with `bzz app approve`.

Captures of every step, and the logs with the relay's lines removed, are in
`runs/v016/m3-pane-captures/` of the main checkout (untracked):
`t16-run/` (the eight `t16-*` scenarios), `all-harness/` (every harness
scenario), `red-no-bracketed-paste/`.

| Claim | Scenario | Seen | Result |
|---|---|---|---|
| Open an app | `t16-app-pane` | `:apps` lists `board (Board) · version 1 · open nowhere`; `:app open board` draws `Board · Ctrl-c leave` beside `#t9-general` with `Loaded 0 cards.`, and the status line `APP` and `Ctrl-c leaves the app` | pass |
| Type in it and in the composer without one reaching the other | `t16-app-pane` | `n`, then `q:1 ?5 gg`, Tab, Enter: the card `q:1 ?5 gg` is saved, no quit question; Esc stays in `APP`; a paste of two lines shows `Title: pasted one pasted two` in the form; after `Ctrl+C`, `i` and `typed in the composer q:1 n` go to the composer and the Board still has one card; a paste of two lines stays in the draft, both lines, with the composer open | pass |
| Narrow the window through 144, 100, 72 and 60 columns and back | `t16-app-pane` | 144: communities, channels, conversation, Board; 100: no communities, Board; 72: Board; 60: `app hidden · 5`, `5` zooms it, `Ctrl+C` hides it; 120: `Board · 5 focus` | pass |
| Open a thread beside it | `t16-app-pane` | at 120 a thread takes the right side (`app behind thread · 5`), `5` brings the Board back and the dock says `message #t9-general`; at 160 both, and the dock says `reply in #t9-general` | pass |
| Quit with the terminal restored | `t16-app-pane`, `t16-app-looper-composer` | exit 0; after it tmux reports `alternate_on=0 mouse_any_flag=0 mouse_sgr_flag=0 cursor_flag=1`, and in what bzz wrote the last `?1049l`, `?2004l`, `?1000l`, `?25h` come after the last switch on | pass |
| A second profile changes a card and it shows in the pane | `t16-app-live-second-profile` | both TUIs open the Board in `#t9-general`; the second adds `from the second profile` (`To do (2)` in it after 972 ms); the first pane, untouched, shows it 3 ms after that | pass |
| A looping script in the pane, and the composer still typing | `t16-app-looper-composer` | the looper spins 150 ms of every 250 on its thread; 98 characters sent to the composer at once were all on screen in 67 ms beside the idle Board and 65 ms beside the looper (`t16-run.log`; the full run at 200×50: 66 and 65 ms); `x` loops forever: `looper.lua:13: app.key ran longer than 250 ms and was stopped` in the pane and `app looper: looper.lua:13…` in the status line; `:app close` | pass |
| Red: the same scenario with bracketed paste not switched on | `t16-app-pane` on a release binary with `EnableBracketedPaste` commented out | the paste into the Board's form arrived as keys (`pasted onepasted two`, then saved by its Enter); the paste into the composer sent its first line and ran the second as bzz's keys, which opened a profile card: `'second pasted line' did not show` | fail, as expected |

The v0.15 behaviour while no app is open: every harness scenario of the
manifest, v0.13 to v0.16, 29 of 29, through tmux at 200×50
(`all-harness.log`). At 160×45 one v0.15 scenario failed only because a
line wrapped at another column (`t15-coding-harnesses-in-panel`, the text
was on screen). The first runs found two faults of the tmux runner (a `;`
was taken by tmux as a separator; Esc was sent apart from the key after
it, where Herdr sends a step's keys together); both fixed in the runner.

### Gates

On `4043a30`, 2026-11-19 at 19:10 (UTC−5), one after the other. Only
this document changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | exit 0: 955 passed, 0 failed, 35 ignored, in 59 test programs. New here: 6 of `ui::layout` (2 of v0.15 kept), 7 of `ui::app_pane`, 5 of `app::pane`, 1 each of `ui::status`, `ui::terminal`, `ui::action` |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 481 passed |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |
| Release binary | `cargo build --locked --release`, the scenarios above | 8 of 8 `t16-*`; 29 of 29 harness scenarios |

### Not proven in M3

| Item | State |
|---|---|
| Herdr | the session `t3code` was not running. The scenarios ran through tmux; the Herdr runner's handling of `size`, `paste` and a second pane is written and untried |
| A click on the pane, on the binary | the hit map and the focus are unit-tested; no mouse event was sent to the binary (tmux cannot synthesise one) |
| Windows (no paste event), macOS, the Linux console | not compiled or run here |
| A version that fails as the pane starts it (`failed_to_run`) | written; no test makes approved bytes fail to load in the pane. M4-B's approval brings new versions to an open pane |
| An approval at the shell reaching an open app without a restart | M4-B |
| Names of people bzz has no profile of | not fetched for the app; they show as the short key |
| The session's notice in the border on the relay (quota, archived) | unit-tested on the frame; not seen on the binary |
| A panic on an app's thread on the binary | the hook's rule is unit-tested; no panic was caused |

### Found on the way

- **tmux 3.6 exits when a window is created with `window-size manual`
  set globally** (`new-window` answers `server exited unexpectedly`). The
  runner sets sizes with `resize-window`, which makes that window's size
  manual alone.
- **Bracketed paste also fixes the composer** for everyone: before it, a
  pasted line break sent the message and the rest ran as bzz's keys (the
  red above opened a profile card).
- **The Board saves a pasted line break as a space** (`ui.paste` into a
  single-line input).


## M7 — Agents use an app's records

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-19. Branch
`t3/implement-m7-agent-records`, cut from `v0.16.0` at `2c05263` (after
M3 was merged). An agent takes part in an app without a screen and without
the app's script: five tools, served as `send`, `read` and `react` are, to
all four harnesses; the fence of rule 17 for agents where the tool is
served; signed by bzz with the agent's key; the agent's own quota; its
writes in its activity.

**Where it is.**

| Path | What |
|---|---|
| `src/agents/runtime/app_tools.rs` | `apps`, `records`, `create_record`, `update_record`, `delete_record`: names, checked arguments, the MCP definitions. M5's tools go beside these |
| `src/agents/runtime/tools.rs` | `ToolCall::App`, `publishes` and `is_chat_effect`, `names()`; the unknown-tool sentence lists every tool |
| `src/agents/runtime/agent/apps.rs` | the fence for agents (installed, enabled, approved, open in the turn's channel, the approved types), the agent's open records, its budget, the answers, the activity line |
| `src/agents/runtime/agent.rs` | the dispatch; a record write is refused after the final reply, is no chat effect, counts as an effect of the request in the ledger (so a crashed coding turn is not handed over again); `initialize` says `apps: true`; a stopped agent closes its apps' records |
| `src/agents/runtime/relay.rs` | `AgentRelay::records`; `RelayPort` makes the agent's own `RecordsService` (its key, connection, HTTP client, the owner's store) |
| `src/agents/runtime/permission.rs` | a coding harness's request for `mcp__buzz__<record tool>` is bzz's own tool, allowed as `send` |
| `src/agents/activity.rs` | `What::Record`: app, change, type, id; never content |
| `agent-host/src/host.mjs` | the Pi host offers `buzz_apps` … `buzz_delete_record` when bzz says `apps`; a remote copy does not |
| `examples/scripted_harness.rs` | `keep` and `"$name/pointer"`: a step uses what an earlier tool answered, as a model reads a record's id and rev |
| `tests/support/artifact_relay.rs`, `tests/support/scripted_relay.rs` | a scripted relay that keeps heads for several identities, checks `prev`, words its refusals as the pin, and sends each stored revision to every connection (`WsAction::Broadcast`) |
| `tests/agent_records_test.rs`, `tests/secret_scan_test.rs`, `tests/relay_integration.rs`, `agent-host/test/acp.test.mjs` | the proofs below |
| `scripts/test-relay.sh` | passes `BUZZ_REQUIRE_RELAY_MEMBERSHIP` and `RELAY_OWNER_PUBKEY` through to the relay when the caller sets them |
| `docs/agent-host-protocol.md` (sections 3, 5, 6), `docs/agents.md`, `docs/apps.md` ("Agents"), `docs/app-records.md` ("Who signs") | the tools and their definitions |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged. No
migration; the schema is unchanged.

**What was decided here** (the plan and the brief left these open):

| Question | Decided |
|---|---|
| Names | `apps`, `records`, `create_record`, `update_record`, `delete_record`; Pi sees `buzz_…`. `record_id` and `revision` are taken for `id` and `rev` (a script's names); `content` may come as JSON text |
| Who signs, and how | not `create_as` on the owner's open app: the agent has a `RecordsService` of its own on its own connection (see `docs/app-records.md`, "Who signs"). It needs no pane of the owner's open (`bzz agent run` has none), reads as the relay lets the agent read, and never takes the one instance of an app in a channel that the owner's pane needs. `create_as`, `update_as`, `delete_as` stay unused |
| "Open in the channel" | the store's `open_in` of M4 (the channel D4 opens an app in), not whether a pane is drawn now |
| What an agent sees of a record | a script's view: `{id, type, rev, content, by: {id, name}}`; names from the relay's profiles, the agent's own name for itself, a short key otherwise |
| `records` | `match` over the title rule, as `data.list`; `limit` 1 to 200, default 50; at most 64 KiB of answer, with `count`, `more` and a note; waits up to 10 s for the relay's first set, then answers from what bzz holds |
| Failures | first line `<code>: <reason>. <what to do>`, last line the JSON a script gets (`error.code`, `message`, `retry_after`, `current`) |
| The local cap | **60 record writes a minute per agent**, all apps and turns together: half of the 120 the pin gives an agent, as an owner's apps get half of his 60, so that a turn filling a board cannot leave the agent unable to post its answer. Past it `rate_limited` with the wait, no frame |
| The relay's own wait | not a gate of bzz's: the agent's connection already holds every frame while it lasts (`realtime::admission`), and the service answers `rate_limited` with what is left. A gate here was written, found to change nothing in the test of the quota (green with and without it), and removed |
| Owner's permission | **none, as for `send`**: the channel already lets the agent write there and the fence is narrow. Before each write bzz asks the relay's records whether the agent may still write in the channel, as before a message |
| The owner sees it | the tool's title in the agents panel as for any tool call, and one line per write in the agent's activity (`bzz agent show`): `update a day of the app rota (1a2b3c4d) · updated` |
| Turn rules | a record write is refused after the turn's final reply; it is no chat message (it does not count against `effects_per_turn`, and a turn's plain text can still be its reply); it is counted as an effect of the request in the ledger, before it is sent, so a coding harness whose turn crashed after a write is not handed the request again |
| Records kept open | an agent keeps at most 8 apps' syncs open, closes one unused for 10 minutes, reopens one whose approved types changed, and closes all when it stops |
| D17 | nothing about apps is added to a prompt; the tools' own descriptions say what they do |

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo build --locked --example scripted_harness
cargo test --locked --lib agents::runtime
cargo test --locked --test agent_records_test
cargo test --locked --test secret_scan_test no_key_reaches_a_harness
cargo test --locked --test agent_runtime_test
(cd agent-host && npm ci && npm test)
ln -sfn "$CARGO_TARGET_DIR" target
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin BZZ_RELAY_TEST_FILTER=real_relay_agent_takes_a_day \
  BZZ_RELAY_NOCAPTURE=1 ./scripts/test-relay.sh
RELAY_OWNER_PUBKEY=79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798 \
  BUZZ_REQUIRE_RELAY_MEMBERSHIP=true BZZ_BUZZ_SOURCE=… BZZ_RELAY_TEST_FILTER=real_relay_agent_takes_a_day \
  BZZ_RELAY_NOCAPTURE=1 ./scripts/test-relay.sh
rm target
```

### Claims

`U:` a unit test, `S:` the scripted rung (the scripted harness, a recorded
relay for chat, the scripted relay of records on real sockets), `R:` the
pinned relay, `N:` the Pi host's own tests under Node. Each red is the
named test run with the one change in its cell, seen failing with the
quoted message, and the file restored.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **Exit gate, scripted**: an agent mentioned in a channel where the Rota is open lists the apps (the Rota, its brief, `day`), lists the seven days, finds Tuesday and updates it with its rev; the relay holds one revision, signed by the agent and none by the owner; another profile's Rota and the owner's own open Rota show `Tue  Scout` and `Days: Scout 1`; the mention has one reply; the activity has `update a day of the app rota (…) · updated`. Through the pipe (bzz's host) and through `bzz agent-tools` (a coding harness) | `S: agent_records_test::an_agent_takes_a_day_in_the_rota_and_another_profile_sees_it` | the record tools not served (`app_tools::parse` skipped in `tools::parse`): `not JSON: There is no tool \`apps\`. …` | pass |
| **Exit gate, pinned relay**: the same with the agent added to a private channel with the role `bot`, run by `agents::runtime::service::serve` as the TUI runs it (its own key unsealed from the profile `bzz broker create` made); another profile's Rota shows `Tue  Scout` 502 ms after the mention; the relay's head of Tuesday is signed by the agent; one reply, `Tuesday is mine.` | `R: real_relay_agent_takes_a_day_in_the_rota_with_its_bot_role`, relay with `BUZZ_REQUIRE_RELAY_MEMBERSHIP` unset (its default, off) | the same red: `timed out waiting for the agent's day on bo's screen` with `Tue  free` on it | pass (20.1 s) |
| **The other membership mode**: with `BUZZ_REQUIRE_RELAY_MEMBERSHIP=true` an agent that is no relay member is refused at authentication (`Access("session is not authenticated")` for its first write); made a relay member, the same gate passes (534 ms) | the same test, relay with `BUZZ_REQUIRE_RELAY_MEMBERSHIP=true` and `RELAY_OWNER_PUBKEY` set | — (the refusal is the relay's own, as M0 saw it; no red taken) | pass (22.2 s) |
| **Fence: a type the app did not declare** (`card` in the Rota) is `fenced: the app rota did not declare the record type card … The app's record types are day.`; the long form `bzz.app.rota.day` is no name an agent can give | `S: the_fence_holds_whatever_the_harness_sends` | — (the service's fence, M2; this shows it holds through the tool) | pass |
| **Fence: an app open in another channel** (the Board, installed and approved) is `The app board is not open in this channel. The apps open here: rota.`, for a write and for a list | `S: the_fence_holds_whatever_the_harness_sends` | the store's apps listed whatever the channel: the Board card was created (`(false, "{\"record\":…\"type\":\"card\"}")`) | pass |
| **Fence: another channel**: woken where the Rota is also open, the agent names a record of the first channel by id and rev: `not_found`; a `channel` argument is `no argument \`channel\`` | `S: the_fence_holds_whatever_the_harness_sends` | — | pass |
| **Fence: a disabled app**: `apps` lists it under `unusable` (`its owner disabled it`), a write is `The app rota is open in this channel but cannot be used now: its owner disabled it.` | `S: the_fence_holds_whatever_the_harness_sends` | — (`Apps::runnable`, M4) | pass |
| None of the refused calls above sent a frame; the week is unchanged | `S: the_fence_holds_whatever_the_harness_sends` (`writes_by(agent) == []`) | — | pass |
| **A conflict gives the agent the current record**: another profile takes Tuesday after the agent listed it; the agent's update is `conflict: …` with `current.content.holder == "bo"`, `current.by.id` bo's key and a new rev | `S: a_conflict_gives_the_current_record_and_the_quota_gives_the_wait` | — (the service's conflict, M2) | pass |
| **At the relay's quota the agent gets the wait**: the relay answers `rate-limited: quota exceeded; retry in 19s`; the agent is told `rate_limited`, `retry_after` 1 to 19, `Try again in N s`; its second write is answered the same at once and sends no frame | `S: a_conflict_gives_the_current_record_and_the_quota_gives_the_wait` | — (see "The relay's own wait" above: green with and without a gate of the agent's) | pass |
| **The agent's own budget**: 61 creates in one turn: 60 stored, the 61st `rate_limited` with a wait of 1 to 60 s; the relay received 60; the turn still had its one reply | `S: an_agents_record_writes_stop_at_its_own_budget`; `U: agent::apps::tests::an_agent_writes_at_most_its_budget_in_a_minute` | the budget not spent: the 61st was stored (`{"record":{…"day":"Extra 60"…}}`) | pass |
| Every argument is checked and a wrong one named; no argument names a channel, place, session or community; the definitions list each tool once | `U: app_tools::tests::every_argument_is_checked_and_a_wrong_one_is_named`, `the_definitions_name_every_tool_once`; `U: tools::tests::*` | — | pass |
| A failure says its code, what to do, and the same as JSON; an unknown author is a short key; an app not open here is named with those that are | `U: agent::apps::tests::*` (3 more) | — | pass |
| `bzz agent-tools` lists all eight tools; a coding harness's request for a record tool is allowed as `send`, and not one whose kind deletes, nor a name bzz has not | `U: mcp::tests::it_lists_every_tool_and_passes_calls_on`; `U: permission::tests::each_kind_of_request_is_read_as_the_three_programs_send_it`, `a_request_that_names_only_its_call_is_read_from_what_was_reported` | — | pass |
| The Pi host offers the five tools only when bzz says `apps`, sends them to bzz as `_bzz/tool` under the name on the wire with the model's arguments, and adds nothing about apps to the prompt | `N: acp.test.mjs` "the tools of apps' records are offered only when bzz serves them, and go to bzz over the pipe" | `host.mjs` of `2c05263`: `actual: ['buzz_send', 'buzz_read', 'buzz_react']` | pass |
| **No key in the harness's environment, arguments, files or tool results**: each of the four harnesses (Pi Durable over the pipe; Grok, Codex and Claude Code through `bzz agent-tools`, as their own launch) lists the Rota and its days and writes a day; the harness logs with every answer, the four records on the relay, the database, the activity and every file bzz wrote hold neither key, nor the broker credential, nor a key of bzz's environment | `S: secret_scan_test::no_key_reaches_a_harness_environment_arguments_or_logs` | the sentinel of bzz's environment put into the answer of `apps`: `a key of bzz's environment leaked into pi-durable harness log` | pass |
| **One reply per mention still holds**: v0.15's ledger tests, unchanged but for the two that read what `initialize` and `tools/list` say | `S: agent_runtime_test` (54 tests), `agent_host_test`, `agent_coding_test` in the full suite | — | pass |
| A second host for the same agent can open the Rota again | `S: secret_scan_test` (four hosts in turn) | before `Agent::stop` closed the agent's records: `bad_request: the app rota is already open in this channel` for the second harness | pass |

The pinned-relay runs took the lock (`scripts/test-relay.sh`), each built
the relay at the pin, ran the one test, and tore the harness down:
`docker ps -a` listed no `buzz-harness` container after each of the three
runs (open, required membership, red). Logs:
`/media/arpagon/archive-data/experiment-artifacts/arpagon-01-med/bzz/m7/relay-{open,closed,red}.log`
(the first `closed` run stopped before the test: a relay that requires
membership needs `RELAY_OWNER_PUBKEY`, hence the second variable in the
script).

### Gates

On `9648403`, 2026-11-19 from 20:44 to 20:50 (UTC−5), one after the other.
Only this document changed afterwards.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | exit 0: 1047 passed, 0 failed, 36 ignored, in 61 test programs; `agent_host_test` ran the real Pi host (Node 26.5), not its skip. New here: 4 of `agent_records_test`, 1 ignored of `relay_integration` (the pinned relay's), 2 of `app_tools`, 4 of `agent::apps` |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 481 passed |
| The Pi host | `cd agent-host && npm ci && npm test` | 100 passed |
| Pinned relay | `scripts/test-relay.sh` with `BZZ_RELAY_TEST_FILTER=real_relay_agent_takes_a_day`, both membership modes | 1 passed each, with the source of `9648403` (`src/` has not changed since `693386f`); the open run had the test before its closed-membership block got a time limit, a block that runs only when membership is required |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |

### Not proven in M7

| Item | State |
|---|---|
| A real model using the tools | not run (no real model in this task). The scripted harness calls them as a model would; whether a model reads a brief and picks the right record is M5's acceptance with real models, or later |
| The TUI's pane showing an agent's change | shown with an `AppSession` on the owner's own records service and store beside the agent, drawn to a buffer (scripted); not on the release binary through Herdr or tmux |
| A guest agent | the relay's refusal (`restricted: only members can post in this channel`) is mapped by the service (M2) and passed through as `refused`; not run with an agent whose role is `guest` |
| The relay's 120 a minute for an agent through the tools | M0 measured it on the pin; bzz stops at 60 before it. The scripted relay stands in for the relay's wait |
| An approved version whose types change while the agent holds the app open | written (the agent reopens the records for the new types); not tested |
| `bzz agent run`, without the TUI | the same `serve` as the pinned test; not run as the command |
| Windows and macOS | not compiled or run here. The scripted test is `cfg(unix)` because the harness tests are |
| The capability records (`bzz agent show`) | they still say "it acts on the chat only through bzz's tools", which is true of records too; no rule of the record tools was added to them |

### Found on the way

- **A relay that requires membership needs `RELAY_OWNER_PUBKEY`.** Without
  it the pinned relay does not start (`BUZZ_REQUIRE_RELAY_MEMBERSHIP=true
  but RELAY_OWNER_PUBKEY is not set or invalid`).
- **The Rota's days have no `title`**, so `match` (over the title rule)
  finds nothing in them: every day's title is `rota day`. An agent finds a
  day by its content. That is the contract (`docs/app-api.md`); a brief
  could say so.
- **`test-relay.sh` builds Buzz into `CARGO_TARGET_DIR`** when it is set,
  for `buzz-cli` and `buzz-acp`, while it runs them from the pin's own
  `target/`: a first run in a worktree spends minutes building what it then
  does not use.

## M4 (inside bzz) — Approving and managing apps in the TUI, `:app dev`, `host.name`, the templates

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-19. Branch
`t3/approval-tui`, cut from `v0.16.0` at `2c05263` (after M3). The second
half of M4: the approval of a version on v0.15's overlay, managing apps
from `:app`, an approval or a rollback reaching an open app in place,
`:app dev` on a channel's records, `host.name(id)` in `api = 1`, the three
templates of D9, and M4's exit gate on the release binary.
[`apps.md`](apps.md) is the user's guide.

**Where it is.**

| Path | What |
|---|---|
| `src/ui/app_approval.rs` | the overlay: the head (who, hash, sizes, types with `CHANGED`) always on screen, the brief and the difference scrolling, the end to be seen before a yes; the banner |
| `src/app/manage.rs` | `Ctrl+Y`, `:approvals`, `:app pending|approve|refuse|rollback|enable|disable|remove|add|new|dev`; every answer a call of `apps::store::Apps` |
| `src/app/pane.rs` | `sync_apps` (the store asked every second: what runs, what waits, where apps are open); `Live::update` and the trial of a new version in the running session; `Pending::Failed { kept }` and `Updated`; `:app dev`; profiles looked up for ids an app names |
| `src/app/pane/dev.rs` | a folder in development: its name, its title, its saves |
| `src/app/pane/tests.rs` (`managed`) | the above inside bzz's `App`, with a store of apps on disk and records in memory |
| `src/apps/runtime/{api,worker,mod}.rs`, `src/apps/session/mod.rs` | `host.name(id)`: the names the owner knows, a short form and a lookup for the others, a redraw when one is learnt |
| `src/apps/store/mod.rs` | `Apps::outcome`, for the proposers of M5 and M6 |
| `src/apps/store/templates.rs`, `tests/fixtures/apps/{board,poll,approvals}/` | the three templates, one source with their fixtures (script and brief) |
| `examples/app_host.rs`, `tests/apps-bench/` | `host.name` on the example host (`MINIAPP_PEOPLE`); the Poll and the Approvals store ids, and the bench writes the id of a name |
| `docs/app-api.md`, `docs/app-api.d.lua` | `host.name` in the contract and its twin |
| `scripts/tui-herdr-scenarios.toml`, `scripts/test-tui-{tmux.py,herdr.sh}` | the `t16-m4-*` scenarios; a step's `run` field in both runners |
| `docs/apps.md`, `docs/e2e-herdr.md` | approval and managing inside bzz, `:app dev`, the templates; the M4 scenarios |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.
Migration 21 is unchanged; the schema stays at 21.

**What was decided here** (the plan and the brief left these open):

| Question | Decided |
|---|---|
| How a version is announced, and how it is opened | as an agent's command: a line at the top of every screen; `Ctrl+Y` (also while an app has the keyboard), `:approvals`, `:app pending [name]`. An agent's command comes first, because it is refused when its time is up |
| Approve from a command | never: `:app approve <name>` opens the overlay, and the approval is given there, of the hash its head shows, after its last row was on screen. `:app refuse <name> [reason]` refuses at once |
| How an answer reaches an open app | the pane asks the store every second (`STORE_CHECK`) and at once after an answer given here: one rule for an answer in the TUI, at the shell, a rollback, and a version that failed. A disabled app is open nowhere; a removed one closes |
| What "fails when it is put to run" is | the new bytes are given to the running session as a reload (state kept). They replace the old version only if they load, declare the approved types (`Runnable::check_loaded`) and run `app.reload` and the first `app.view` without an error. Otherwise the old bytes, approved and held in memory, are given back as a reload (state kept), `Apps::failed_to_run` records the version, and the status line says which goes on |
| `dev` (D5) | `:app dev <folder>` in the selected channel. Its records are those of the name in its `app.toml` (the folder's name without one): a folder named as an installed app meets that app's records, which is how a change is tried on real data. Marked `not approved · dev <folder>` in the border; the script is looked at every 300 ms; a broken save leaves the old version running. Ended by `:app close`, `:app open` of another app there, `host.quit()`, a switch of community, `:lock`, quitting. Never remembered in the store |
| `bzz app dev --channel` | still refused, and says to type `:app dev <folder>` in that channel: the command line cannot reach a running bzz |
| `host.name(id)` | answers from the names bzz has (its profiles), else `c0c0c0c0…c0c0`, cut on characters whatever a script gives. An id not named is told to the owner once (at most 4096); bzz looks up only what can be a key (64 hex), 64 at a time, as the timeline looks up authors. A name learnt later calls `app.view` again, and no handler: no record changed. On the example host the names are `MINIAPP_PEOPLE` and whoever it sees write |
| Templates and fixtures | one source: `templates.rs` takes script and brief from `tests/fixtures/apps/<name>/`, the files the runtime's tests and the bench run. The Board's brief of `src/apps/templates/board/` moved there. The Board stores no person; the Poll's `voter` and the Approvals' `author` and `decided_by` are ids, shown with `host.name`; the Poll shows who voted for the selected option (`By: bo, you`) |
| The bench's specs | `spec/poll.md` and `spec/approvals.md` say id where they said name; the service seeds, and the cases write, `sha256("participant:<name>")`, the id the example host makes of a name |
| `:app new` | writes `./<name>` from a template, as `bzz app new` does without `--to` |
| For M5 and M6 | a proposer calls `examine`, then `Apps::record` (with `Proposer::agent(…)` or `Proposer::member(…)`), and asks `Apps::outcome`: waiting, applied, refused with the reason, failed with the error, superseded. The approval and its announcement need nothing else |
| Long answers on the status line | written short, the fact first (`board uninstalled; 2 records of bzz.app.board.* stay in the channel …`): the status line cuts what does not fit |

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo test --locked --lib app::pane ui::app_approval apps::session::tests::host_name apps::store::tests::the_templates
scripts/test-apps-bench.sh poll approvals board        # the templates' specs, at 80x24 and 40x16
cargo build --locked --release
ln -sfn "$CARGO_TARGET_DIR" target                      # test-relay.sh uses ./target
BZZ_TMUX_SIZE=120x30 BZZ_TMUX_CAPTURES=/tmp/captures BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin \
BZZ_TMUX_SCENARIOS=t16-cli-app-new,t16-cli-app-add,t16-cli-app-approve,t16-cli-app-add-second,t16-cli-app-add-looper,t16-app-pane,t16-app-live-second-profile,t16-app-looper-composer,t16-m4-poll-approve-update-rollback,t16-m4-approvals-template,t16-m4-dev-remove-reinstall \
  ./scripts/test-relay.sh --tmux
rm target
```

### Unit and scripted

`U:` is a unit test; `P:` is `app::pane::tests::managed::…`, bzz's own
`App` drawn on Ratatui's test backend, with a store of apps on disk in a
temporary folder and records in memory (`AppPanes::memory`, a test seam in
place of the relay). Each red is the named test run with the one change in
its cell, seen failing with the quoted message, and the file restored; the
runs are `/tmp/m4b/reds.sh` and `reds2.sh` of the worktree, not kept.

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **Rule 16 end to end in bzz's window**: a version added runs only after its approval: before it `:app open` says `counter has no approved version (one waits for your answer)` and nothing is open or running; the banner says `counter: version 1 from you waits for your approval · Ctrl-Y to see and answer it`; the overlay shows who, the hash, the types, the brief, the script; `a` approves; then it opens and runs those bytes | `P: a_version_runs_only_after_its_approval_and_an_approval_changes_the_open_app_in_place` | — (the door is `Apps::runnable`, proven in M4-A; no red taken here) | pass |
| **An approval of a version of an open app changes it in place, keeping its state**: count 3 under version 1 is count 3 under version 2; the status line says `counter: version 2 runs in place of 1, its state kept` | the same | the new version given to a new session instead of the running one (`if true {` for `if !live.update(…)`): `no "v2 count 3" on: … v2 count 0` | pass |
| **A change of the declared types is shown as a change** in the overlay: `CHANGED from the version that runs:`, `+ tag   new: the app could not read or write these records before`, from your agent Scout with its key | the same; `U: ui::app_approval::tests::the_head_stays_and_the_end_must_be_seen` | — (the words are `Review::types_lines`, M4-A's red) | pass |
| `Ctrl+Y` reaches bzz while the app has the keyboard, when a version waits | the same | `approval_waiting` counting agents' commands only: `left: App right: Approval` | pass |
| **The head stays and the end must be seen**: a long difference scrolls under the head, `scroll to the end before you approve`; the end seen, it stays seen; a short version is seen whole; a version no longer waiting takes no answer | `U: the_head_stays_and_the_end_must_be_seen`; `P: a_version_that_fails…` (`a` before the end: `scroll to the end first`, and the version still waits) | the end counted as seen at once: `assertion failed: !state.seen_end`; no such check in the key: `None` where the notice was expected | pass |
| **A version that fails as it is put to run leaves the one before running** (M3 left this): approved bytes that load apart and raise in `app.reload` in the open app; version 1 goes on with its count; version 2 is recorded `failed` with `counter.lua:…: in app.reload: …`, `Apps::outcome` says so, the store's current is 1 again, and the same session goes on counting | `P: a_version_that_fails_as_it_is_put_to_run_leaves_the_one_before_running` | the old bytes not given back: `no "v1 count 2" on: … v2 count 2`; the trial blind to an error after the load: `version 2 is recorded as failed` timed out | pass |
| **Rollback, refuse, disable, enable, remove from the TUI** with the app open: rollback in place with the count; a refusal's reason kept (`Outcome::Refused`); disabled closes; enabled opens again; removed closes and says the records stay, and `:app open` then says it is not installed | `P: rollback_refuse_disable_enable_and_remove_from_the_tui` | a disabled app kept in the map of open apps: `assertion failed: app.shown_app().is_none()` | pass |
| **`:app dev` runs the owner's folder here, on the channel's records, and reloads it on save**: marked `Tasks · not approved · dev …`; a card, then a save (`"To do"` → `"Backlog"`) shows `Backlog (1)` with the card; a broken save leaves it running with `reload failed: the previous version keeps running`; no app is installed by it; `:app close` ends it | `P: app_dev_runs_the_owners_folder_here_and_reloads_it_on_save` | the folder not looked at on a tick: the screen stays `To do (1)` | pass |
| **`host.name(id)`**: the name bzz knows, the short form for an id it does not (`c0c0c0c0…c0c0`), the short form cut on characters (`éééééééé…éééé`), an error with file and line for what is not a string; the unnamed id is asked of the owner; a name learnt later draws the app again, with no push | `U: apps::session::tests::host_name_is_the_name_bzz_knows_and_a_name_learnt_later_draws_again` | a learnt name not redrawing (`learn_names(…) && false`): panic at `tests/mod.rs:457` (`voter: cy`); the asked ids not given to the owner: `[]` where cy's id was expected | pass |
| **The templates store ids and show names at 40×16**: the Board fits; the Poll's vote stores the voter's id and shows `By: bo, c7c7c7c7…c7c7, you`, then `By: bo, cy, you` once cy is named; the Approvals' author and decider are ids, shown `Author: bo`, `Decided by: ana`; "not your own request" compares ids | `U: apps::store::tests::the_templates_store_ids_and_show_names_at_forty_by_sixteen`; `every_template_loads_and_declares_what_its_entry_says` | the Approvals storing `ctx.user.name`: panic at `tests.rs:1106`; the Poll showing the stored id: panic at `tests.rs:1064` | pass |
| **The templates' specs on the bench**: the Poll, the Approvals and the Board, at 80×24 and 40×16, with reload, pushes and the hostile script | `scripts/test-apps-bench.sh poll approvals board` | — | 12/12, 15/15, 12/12 at each size; reload 20/20 at each size; pushes 8/8; hostile 29/29 |
| The fixtures' cases in cargo with ids (Poll, Approvals) | `apps::runtime::tests::{poll,approvals}` | — | pass |
| The v0.15 approvals (agents' commands) behave as before | the v0.15 tests of `app.rs` (`Ctrl+Y`, `:approvals`, the dialog), all green in the gate below | — | pass |

### The exit gate, on the release binary

**Run through tmux, not Herdr.** `herdr --session t3code pane list`
answered `server_not_running`; the session was not started.
`scripts/test-tui-tmux.py` ran the manifest on a private tmux socket, on
the release binary of this branch, with a disposable profile from
`examples/harness_fixture.rs` on the pinned relay (`scripts/test-relay.sh
--tmux`, the lock taken; `docker ps` showed no `buzz-harness` container
after each run). The M3 scenarios run first and leave the Board installed,
approved and open in both profiles with a card each. Captures of every step
are in `runs/v016/m4b-approval-captures/` of the main checkout
(untracked).

| Exit gate | Scenario | Seen | Result |
|---|---|---|---|
| 1. A script added from a folder runs only after approval | `t16-m4-poll-approve-update-rollback` | `:app add {fixture}/poll-app` shows `poll (Poll), version 1`, `Record types: poll, vote`; Esc: the banner `poll: version 1 from you waits for your approval · Ctrl-Y …`; `:app open poll`: `poll has no approved version (one waits for your answer)`, and no poll is drawn; `Ctrl+Y`, `a`: `scroll to the end first`; `G`: `rows 487–501 of 501: the end`; `a`: `poll: version 1 approved, the script of hash …`; `:app open poll`: `No poll yet. Press a to ask a question.` | pass |
| 2a. A new version that fails to load is never offered | the same | the script with `return app(` appended, `:app add`: `poll: version 2 does not load, nothing changed: …`, no banner, the pane as it was | pass |
| 2b. One that loads and fails when put to run leaves the old one running in the pane | the same | version 3 raises in `app.reload`; approved in the overlay: `poll: version 3 failed to run; version 1 goes on`; the pane still `> [ ] Sushi`, `Total: 1 vote` | pass |
| 3. Rollback works with the app open, the state kept | the same | version 4 (`Total:` → `In all:`) approved: `poll: version 4 runs in place of 1, its state kept`, `In all: 1 vote`, `> [ ] Sushi`; `:app rollback poll`: `poll: back to version 1 from 4`, `Total: 1 vote`, `> [ ] Sushi` | pass |
| 4. `app dev` reloads on save, on the channel's records, with a card a second profile made | `t16-m4-dev-remove-reinstall` | `:app dev {fixture}/board-app` in `#t9-general`: `Board · not approved · dev …`, `> q:1 ?5 gg` and `from the second profile` (the second profile's card, M3's scenario); `sed` of `"To do"`: `Backlog (2)` in 376 ms; `return app(` appended: `app board: app.lua:409: <eof> exp…` and still `Backlog (2)`; `:app close`: the installed Board again, `To do (2)` | pass |
| 5. Uninstalling leaves the channel's records alone | the same | `:app remove board`: `board uninstalled; 2 records of bzz.app.board.* stay in the channel`, the pane gone; a new folder added and approved, `:app open board`: `q:1 ?5 gg` and `from the second profile` | pass |
| 6. A change of the declared types is shown as a change | the same | the folder with `types = { "card", "tag" }`, `:app add`: `CHANGED from the version that runs:`, `+ tag   new: …`; `r`, a reason, Enter: `board: version 2 refused ("no tags on this board"); nothing of it runs`, the Board as it was | pass |
| 7. The three templates: `new`, `add`, approve, open | the Board in M3's `t16-cli-app-*` (at the shell), the Poll above, `t16-m4-approvals-template` | the Approvals in `#t14-names`: added, approved, `Filter: all`; `n`, `Gym pass`: `Request filed.`; its detail `Author: …`; `a`: `You cannot decide your own request.` | pass |
| M3's scenarios with this branch | `t16-cli-app-*`, `t16-app-pane`, `t16-app-live-second-profile`, `t16-app-looper-composer` | as in M3 | 8 of 8 |
| Red: the old bytes not given back after a failed trial | `t16-m4-poll-approve-update-rollback` on a release binary with `session.reload(runnable.script())` taken out of the trial's failure | the pane goes on running version 3, with `in app.reload: poll.lua:448: version 3 cannot take this state` in its region, although the store made version 1 current again; `poll: version 3 failed to run; version 1 goes on` did not show whole (the app's error took the status line's room) | fail, as expected |

What was not run on the binary: the session's notices (the wait at the
quota, an archived channel) and a panic on an app's thread, which M3 left
unit-tested only. The fixture has no archived channel an app is open in,
and reaching the quota of writes in a scenario takes thirty writes in a
minute; neither was added. Names of other people through `host.name` on the
binary: the fixture's viewer has no profile, and no other person writes in
the scenarios' channels, so the Approvals' `Author:` shows the viewer's
short id; the lookup and the redraw are proven on the session and on the
templates (above).

### Gates

On `2806f53`, 2026-11-19 from 21:17 (UTC−5), one after the other
(`/tmp/m4b/gates.sh` of the worktree). Only this document changed
afterwards. The release binary of the exit gate was built from the same
sources (the run that ended at 21:07; the commits after it changed documents only). The release binary left in the worktree's target is the red one of the table above.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | clean, both |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | exit 0: 1045 passed, 0 failed, 35 ignored, in 60 test programs. New here: 4 of `app::pane::tests::managed`, 2 of `ui::app_approval`, 1 of `apps::session::tests` (`host.name`), 1 of `apps::store::tests` (the templates) |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 481 passed |
| The apps bench | `scripts/test-apps-bench.sh poll approvals board` (debug host) | everything passed; above |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |
| Release binary | `cargo build --locked --release`, the scenarios above | 11 of 11 |

### Not proven in M4 (inside bzz)

| Item | State |
|---|---|
| Herdr | the session `t3code` was not running; the scenarios ran through tmux. The Herdr runner reads the new `run` field (checked with `bash -n` and by reading), and has not run it |
| macOS, Windows | not compiled or run here |
| The session's notices on the binary (quota, archived), a panic on an app's thread on the binary | unit-tested only, as M3 left them; no scenario added |
| `host.name` of another person on the binary | proven on the session and the templates in memory, and on the bench's host; on the binary only the viewer's own short id was seen |
| The whole harness suite, v0.13 to v0.16 | not run: only M3's eight `t16-*` and M4's three. Nothing outside the app pane, `:app` and the approval banner changed in the TUI |
| `:app new` on the binary | unit-tested through `templates::write`; the scenarios write folders with `bzz app new` |
| Two bzz processes answering the same version at once | the store answers each in one SQLite transaction, and the TUI asks again after every answer; not tested with two processes |
| A version whose computed types differ only in the open app | `Runnable::check_loaded` is called in the trial; no script that declares other types at run time than apart was written for a test |

### Found on the way

- **A `G` and an `a` sent together approve nothing,** and that is right:
  the end counts as seen only when it was drawn. The first run of the
  scenarios sent them in one step and the overlay said `scroll to the end
  first`; the scenarios now send `G`, wait for the end, then `a`. v0.15's
  overlay has the same rule.
- **The status line cuts a long answer.** `board is uninstalled: every
  version bzz kept of it is deleted. 2 records …` lost its point at 120
  columns; every answer of `:app` is now written with the fact first.
- **The looper of M3's scenarios closes the Board in `#t9-general`** (one
  app in a channel), so M4's scenario opens the Board there again before
  it tries `:app dev` beside it.
- **`crate::domain::abbreviated_pubkey` slices bytes,** and would panic on
  an id a script gives that is not ASCII; `host.name` cuts on characters
  (`runtime::api::short_id`). Record authors are always hex, so the
  session's own use is safe.

## M5 — Agents write and change apps

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-19 and 20.
Branch `t3/implement-m5-agent-apps`, cut from `v0.16.0` at `429bbd5`
(after M4 inside bzz was merged). A person asks an agent in a channel to
make or change an app; the agent reads the guide and the app, tries its
script apart, and proposes it; the owner approves it in the overlay; the
open app changes in place; the agent is told what became of it. Four tools,
served as M7's are, to all four harnesses; no real model was run.

**Where it is.**

| Path | What |
|---|---|
| `docs/app-authoring.md` | the guide an agent writes an app from: the experiment's `AUTHOR.md` brought to `api = 1` and bzz (215 lines, with the contract `app_guide` returns about 590) |
| `src/apps/authoring_test.rs` | every Lua block of the guide loads apart and draws; its example app does what the guide says at 40×16, 20×8 and 100×30 |
| `src/agents/runtime/app_tools.rs` | `app_guide`, `try_app`, `propose_app`, and `apps` with an optional `app`: names, checked arguments, MCP definitions (the descriptions are how a model learns of apps, D17) |
| `src/agents/runtime/agent/apps/authoring.rs` | serving them: the guide and the contract (`include_str!`), one app whole, a try on a thread of its own on a store in memory (optionally a copy of the channel's records), a proposal loaded apart, checked for its brief, recorded with the agent and the person who asked; the budgets; `app_notices`, what the next turn is told |
| `src/agents/runtime/agent.rs`, `prompt.rs` | the prompt's `Since your last turn in this channel, of what you proposed here:` section, only when there is something to tell |
| `migrations/0022_app_agent_proposals.sql`, `src/store/apps.rs` | `app_versions.asked_by_id`, `asked_by_name`, `told_at`; `app_versions_untold`, `app_version_told`, `app_versions_waiting_from` |
| `src/apps/store/mod.rs`, `review.rs`, `manifest.rs` | `Apps::untold`, `Apps::told`, `AGENT_WAITING_AT_ONCE` (3) in `Apps::record`, `AppError::TooManyFromAgent`; `Asked by:` on a line of its own in what the owner is shown; `check_brief_names_types` |
| `src/ui/app_approval.rs` | the banner says `from your agent Scout, asked by Ana` |
| `src/app/manage.rs` | `:app brief [name]` |
| `src/agents/capability.rs` | `writes apps:` in every capability record |
| `src/agents/activity.rs` | `What::Proposal`: `version 2 of the app board proposed, asked by Ana · waiting for the owner` |
| `agent-host/src/host.mjs` | the Pi host offers `buzz_app_guide`, `buzz_try_app`, `buzz_propose_app` and `buzz_apps` with `app` when bzz says `apps` |
| `tests/agent_authors_app_test.rs`, `src/app/pane/tests.rs` (`managed`), `tests/secret_scan_test.rs`, `tests/relay_integration.rs`, `tests/store_migration_test.rs`, `agent-host/test/acp.test.mjs` | the proofs below |
| `scripts/test-agent-authors-app.sh` | the real-model gate, prepared: `<harness>` on the pinned relay, then the bench's `board-priority` cases on what was approved |
| `docs/apps.md` ("Agents write and change apps"), `docs/agents.md`, `docs/agent-host-protocol.md` (sections 5 and 6) | the user's and the protocol's account |

No dependency was added; `Cargo.toml` and `Cargo.lock` are unchanged.
**Migration 22** is new (the schema goes from 21 to 22); 21 is untouched.

**What was decided here** (the plan and the brief left these open):

| Question | Decided |
|---|---|
| Names | `app_guide`, `try_app`, `propose_app`; reading one app is `apps` with `app` (the brief: extend M7's `apps`, do not double it). Pi sees `buzz_…` |
| What `app_guide` returns | `docs/app-authoring.md`, then `docs/app-api.md`, as one text (about 32 KiB). `app-api.d.lua` is left out: it says the same as declarations |
| How the outcome reaches the agent | **told at its next turn in the same channel, once**: the prompt holds a section after the message, one line per version answered since. Not woken: a turn started by bzz would be a reply to no mention. "The same channel", not the same thread: a top-level mention starts a new place, and the owner asks again where he likes. A refusal's reason is the stored one, word for word (`clean_reason` of M4 made it one bounded line). `apps` with `app` shows `your_versions` with the same sentence any time. A version the agent itself replaced is not told; rows from before migration 22 count as told |
| Where it is kept | three columns of `app_versions` (migration 22), not a table of the agent runtime: the proposer's key and channel are already there, and the untold versions of an agent in a channel are one indexed query |
| Who asked | the request's author, with the name the relay's profiles give (a short key otherwise), recorded as `Proposer::asked_by`; shown in the approval on its own line, `Asked by: Ana (<key>), whose message asked the agent for it`, because at 120 columns the `From:` line already reaches the edge |
| Which apps an agent may propose a version of | a new name; an app open in the turn's channel; or one of which no version was approved yet (its own new app, before the owner answered). An installed app open elsewhere is refused with `:app open` |
| A version that does not load | answered at once, **not recorded**: M4 records the owner's failed folder, but an agent's failures would create an app row per name it tries and bury the store. Same for a brief that does not name its types |
| "The brief's declared types must be the script's" | each record type the script declared as it loaded is a word of the brief (`card`, or `` `card` ``), checked by the tool after loading; the store does not check it, so M4's own proposals are unchanged |
| Bounds | at most **3 versions waiting per agent, of all apps** (`Apps::record`, so no path gets around it); a later version of an app replaces its own waiting one; 6 proposals and 20 tries a minute per agent; one try at a time; a try ends after 30 s of the machine's time, 200 steps, 10 minutes of `wait:` |
| A try | `bzz app try`'s rules: a manual clock, the local participant `you` with a made-up id, the script named `app.lua` (or `<from_app>.lua`); `from_app` plants a copy of the records the agent may read (M7's fence), with their authors; the screen's lines are trimmed at the right; the last 20 log lines |
| A proposal and the ledger | no effect counted: a crashed turn handed over again proposes the same bytes, which the store answers with the same waiting version |
| Owner's yes for the tool itself | none: a proposal is only a proposal; the yes is the approval of the bytes |
| `:app brief` | the brief of the version that runs (read through `Apps::bundle`, so of the approved bytes), with one sentence before it naming the app and version, as an ordinary message of the owner's (`MessageService::send`) |
| Capability records | one line for all four, `writes apps: yes: through bzz's tools only …`, saying it was seen with the scripted harness and not a real model |

**How to repeat.**

```sh
# the build environment of runs/v016/RULES.md, then:
cargo build --locked --example scripted_harness
cargo test --locked --lib -- agents::runtime apps:: app::pane::tests::managed store::apps
cargo test --locked --test agent_authors_app_test
cargo test --locked --test secret_scan_test no_key_reaches_a_harness
cargo test --locked --test store_migration_test
(cd agent-host && npm ci && node --test test/acp.test.mjs)
ln -sfn "$CARGO_TARGET_DIR" target; unset CARGO_TARGET_DIR
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin ./scripts/test-agent-authors-app.sh scripted
rm target
# the reds: /media/arpagon/archive-data/experiment-artifacts/arpagon-01-med/bzz/m5/reds.py
```

### Claims

`U:` a unit test (bzz's `App` on Ratatui's test backend for the window),
`S:` the scripted rung (the scripted harness through the pipe and through
`bzz agent-tools`, a recorded relay for chat, the scripted relay of
records on real sockets), `R:` the pinned relay, `N:` the Pi host's own
tests under Node. Each red is the named test run with the one change in
its cell, seen failing with the quoted message, and the file restored
(`reds.py`, logs beside it in `…/m5/reds/`).

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| **Exit gate 1, the agent's side**: Ana (who may wake the agent) asks for the priority and the filter; the agent lists the apps (the Board, version 1), reads the guide and the contract, reads the Board whole (its script, brief and manifest as approved), tries the Board with priority on a copy of the channel's cards (`j`, `2`: `> Bravo P2`, `Priority: all`, nothing sent to the relay), and proposes it: `waiting`, version 2; the Board that runs is still version 1's bytes; the review names the agent, the channel and Ana; one reply to the mention. The owner approves; bzz hands out version 2's bytes; the agent's next turn in the channel holds `- your version 2 of the app board: applied.`, and the one after it holds nothing. The activity has the proposal. Through the pipe and through `bzz agent-tools` | `S: agent_authors_app_test::an_agent_proposes_the_board_with_priority_and_is_told_it_was_applied` | `app_notices` not called (`since` empty): the second prompt has no such line (`panicked at tests/agent_authors_app_test.rs:496`) | pass |
| **Exit gate 1, the window**: the Board open with three cards and `Bravo` selected; the agent's version (as `propose_app` records it) is announced `board: version 2 from your agent Scout, asked by Ana waits`; Ctrl-Y shows `From: your agent Scout (<key>), in channel …`, `Asked by: Ana (<key>)…`, the types unchanged, the brief CHANGED and the difference; `G` then `a`; the pane shows `Priority: all` with `> Bravo` still selected and `To do (3)`; `2` on it gives `> Bravo P2`; the outcome is `Applied` | `U: app::pane::tests::managed::an_agents_board_with_priority_changes_the_open_board_in_place` | the `Asked by:` line not rendered: ``Asked by: Ana (cccc…), whose message asked the agent for it` in: …`` | pass |
| **Exit gate 2, refuse and nothing changes**: refused with `Not now: "P1" means something else here; ask me first.`; the Board that runs is version 1's bytes; the versions are `2 refused (files not kept)`, `1 approved`; the relay's heads are as before and the agent wrote none; the next turn holds `- your version 2 of the app board: refused by the owner, who said: "Not now: "P1" means something else here; ask me first.". Nothing of it runs.`; `Apps::outcome` says the same. In the window: a third version refused with `keep the priority` in the overlay leaves `> Bravo P2` and `Priority: all` on the pane | `S: a_refused_version_changes_nothing_and_the_agent_gets_the_reason`; `U: an_agents_board_with_priority_changes_the_open_board_in_place` | `app_notices` not called: `panicked at tests/agent_authors_app_test.rs:567` | pass |
| **Exit gate 3, an error comes back with file and line and the owner is never asked**: a syntax error at line 14 of the Board with priority is answered `failed: board.lua:14: unexpected symbol near '='` and `Nothing was proposed, and the owner was not asked.`, with the JSON `{"error": {"code": "failed", …}}`; `try_app` gives the same at `app.lua:14:`; a brief that does not name `card` is refused; none at all is `needs \`brief\``; an `approve` argument is no argument. Nothing waits, the Board has one version, it runs as it was. Both ways in | `S: a_script_with_an_error_comes_back_with_file_and_line_and_the_owner_is_never_asked` | the load error not checked before recording: `not JSON: failed: board.lua:14: …` (the store's own answer, without the JSON, and a version row); the brief not checked: the second proposal `waiting` | pass |
| **Exit gate 4, a try draws what the pane draws**: the Board at 40×16 with three cards and `j`: the screen `try_app` answers is, line for line, what `bzz app try` draws; and what `bzz app try` draws is what the pane draws inside its border in bzz's window (120×22, docked, 40×16 inside) | `S: a_try_draws_the_boards_screen_and_a_hostile_script_reaches_nothing`; `U: an_agents_board_with_priority_changes_the_open_board_in_place` | the try's default width 42: `left: String("42x16")` | pass |
| **Exit gate 4, a hostile try reaches nothing and costs nothing**: the bench's hostile app at 200×80 with every key that attacks (`x y p m s c l u d`): every line `blocked (…)`, none `REACHED`; the turn's next step and `apps` are answered; the whole turn of three tries ended in under a minute (1.4 s for the five tests); a try on a copy of the channel's cards that creates `Delta` shows it, and no frame of the agent's reached the relay | `S: a_try_draws_the_boards_screen_and_a_hostile_script_reaches_nothing` | — (the runtime's confinement, M1; the store in memory, M4) | pass |
| **Exit gate 5, nothing of an agent's runs unapproved, by any path**: five proposals in one turn: the Board, `notes`, `poll` wait; `tally` is `3 versions proposed by this agent already wait …`; a fifth Board replaces its own version 2 (`replaces: [2]`); `apps` with `notes` (not open here) gives only `your_versions`, no script; `approve` and `open_app` are no tools; afterwards `Apps::runnable` gives the Board's version 1 and nothing of `notes`; the only app open anywhere is the Board | `S: an_agents_proposals_never_run_unapproved_and_are_bounded`; `U: apps::store::tests::an_agent_has_at_most_three_versions_waiting_of_all_apps` | the bound lifted (`+ 100`): the fourth `Waiting` (`the fourth app: Ok(Waiting { version: 1, … })`), and the scripted test's list of waiting versions differs | pass |
| **Exit gate 5, the secret scan covers the new tools**: on each of the four harnesses the guide, the Rota read whole, a try of the Rota on a copy of its days, a try of a script that looks for a key everywhere a script can reach (`os.getenv`, `/proc/self/environ`, `require`, `debug`, every global, `ctx.user`), and a proposal: the snooping script draws only `blocked`; the harness logs with every answer, the database and every file hold no key, no broker credential and no key of bzz's environment | `S: secret_scan_test::no_key_reaches_a_harness_environment_arguments_or_logs` | — (no red taken for the new steps; M7's red of the same test, a sentinel in a tool's answer, stands) | pass |
| **Exit gate 6, one reply per mention on the pinned relay; and the whole gate there**: a disposable profile; Scout with the `bot` role in a new private channel; the Board installed, approved, open, three cards; the owner's mention carries the change request of `board-priority.md`; the scripted harness, run by `agents::runtime::service::serve` as the TUI runs it, reads, tries and proposes; one reply (`Proposed: the Board with priority and filter.`), 3 s after the mention; the version waits `From: your agent Scout (<key>), in channel …` `Asked by: <owner's short key> (<key>)`; approved; the agent's next turn says `applied`; one reply to the second mention; no record of the agent's on the relay. Then the bench: the approved script passes **17 of 17** `board-priority` cases at 80×24 and **17 of 17** at 40×16 | `R: real_relay_an_agent_changes_the_open_board_and_is_told_it_was_applied`, through `scripts/test-agent-authors-app.sh scripted` | — (the same code as the scripted claims; no separate red on the relay) | pass (23.6 s for the test) |
| The outcome is told once, in the channel it was proposed in, only after an answer; a superseded version and the owner's own are not told; approved, refused with the reason word for word, failed with the error are said as such | `U: apps::store::tests::an_agent_is_told_once_what_became_of_its_versions_in_that_channel`; `U: agent::apps::authoring::tests::an_outcome_is_said_with_the_owners_reason_word_for_word` | `told` not recorded: `assertion failed: fixture.apps().told(…)` | pass |
| The prompt has nothing about apps beforehand (D17): no section without an outcome; the Pi host's system prompt names no app or record with the new tools offered | `U: prompt::tests::the_prompt_says_who_where_what_and_what_came_before`; `S:` the first prompt of the exit-gate test; `N: acp.test.mjs` "the tools of apps' records are offered only when bzz serves them…" (now with the three new names) | — | pass |
| Migration 22: a database at schema 21 with an agent's answered version opens at 22, reads back as it was, nobody named as having asked, and counts as told; v0.15's schema 20 still migrates with every row kept | `U: store_migration_test::schema_twenty_one_gains_who_asked_and_counts_every_version_as_told`, `schema_twenty_of_v015_gains_the_apps_tables_and_keeps_every_row` and the others with the version 22 | the `UPDATE … told_at` taken out: `told as of when it was answered`, `left: None` | pass |
| Every argument of the new tools is checked and a wrong one named; a try's keys are `bzz app try`'s; sizes, steps, waits, records and the script and brief sizes are bounded; `apps` takes only `app` | `U: app_tools::tests::the_tools_of_a_script_check_every_argument_and_say_what_was_not_done`, `every_argument_is_checked_and_a_wrong_one_is_named`, `the_definitions_name_every_tool_once`; `U: tools::tests::read_and_react_are_checked_the_same_way` | — | pass |
| The guide's code loads on the runtime, and its example does what it says | `U: apps::authoring_test::every_lua_block_of_the_guide_loads_and_draws`, `the_guides_example_counts_and_fits_the_pane` | `return app(` in the guide: `block 1 of the guide: guide-1.lua:102: unexpected symbol near <eof>` | pass |
| `:app brief` reads the brief of the approved version, says what is missing (no app here, nothing approved, no connection); the message names the app and version and ends with the brief | `U: app::pane::tests::managed::app_brief_posts_the_approved_brief_and_says_what_it_needs` | — | pass |
| Each harness's capability record says whether it writes apps, and how | `U: capability::tests::each_record_says_whether_the_harness_writes_apps`, `every_harness_states_each_capability_and_who_enforces_each_rule` | the line not printed: `PiDurable: capability record: …` | pass |

The pinned-relay run took the lock, built nothing of Buzz (the pin's own
`target/` was used: `CARGO_TARGET_DIR` unset and `./target` linked to this
worktree's directory, then removed), ran the one test, and tore the
harness down: `docker ps -a` listed no `buzz-harness` container after it.
Log: `/media/arpagon/archive-data/experiment-artifacts/arpagon-01-med/bzz/m5/author-scripted.log`,
output `…/m5/author-scripted/`.

### Gates

On `2bb6d21`, 2026-11-19 from 23:58 to 00:07 (UTC−5), one after the
other. Afterwards only two lines changed for clippy (a `format!` in a test,
and `brief_message` behind the feature), checked as shown, and the
scenario of the release binary and this document were added.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets -- -D warnings`; the same with `--no-default-features` | two findings on `2bb6d21` (above), fixed in `5c6997e`; clean, both, on `5c6997e` |
| Tests | `cargo build --locked --example scripted_harness`, then `cargo test --locked --all-targets` | exit 0: 1071 passed, 0 failed, 37 ignored, in 62 test programs; `agent_host_test` ran the real Pi host (Node 26.5). New here: 5 of `agent_authors_app_test`, 1 ignored of `relay_integration` (the pinned relay's), 1 of `store_migration_test`, 2 of `authoring_test`, 1 of `app_tools`, 2 of `agent::apps::authoring`, 2 of the store, 2 of the pane, 1 of the capability records |
| Tests without the features | `cargo test --locked --no-default-features --lib` | 482 passed, on `2bb6d21` and again on `5c6997e` |
| After the lint fix | `cargo test --locked --test agent_authors_app_test`; `--lib app::pane::tests::managed` | 5 and 6 passed |
| The Pi host | `node --test test/*.test.mjs` in `agent-host/`, with the main checkout's `node_modules` linked for the run and removed after (no `npm ci` here) | 100 passed |
| Pinned relay | `scripts/test-agent-authors-app.sh scripted` (the relay test, then the bench) | 1 passed; bench 17/17 at 80×24 and 17/17 at 40×16 |
| Supply chain | not run: `Cargo.toml` and `Cargo.lock` are unchanged | |

### Not proven in M5

| Item | State |
|---|---|
| **A real model on each harness** (the plan's last gate of M5, D13) | not run: no real model or account in this task. Prepared: `scripts/test-agent-authors-app.sh pi-durable|grok|codex|claude-code` with `BZZ_AUTHOR_ALLOWED=yes`, proven end to end with `scripted`. Whether a model, from the tools' descriptions alone, finds `app_guide`, writes a version that passes the bench's 17 cases, and keeps the brief true is what that run will say; the brief is printed for a person to judge |
| `:app brief` reaching the relay | the message is built and sent with `MessageService::send`, the composer's own path; the unit test has no relay and checks the text and what is refused |
| An app made new by an agent, approved and opened by the owner | the store and the tool allow it (`notes`, `poll` wait in the bounds test); not approved and opened in a test |
| A version approved, then failing as it is put to run, told to the agent | `Outcome::Failed` is said (`outcome_words`, unit) and M4's test shows the pane records it; not run with an agent's version end to end |
| Waking the agent when the owner answers | not done, by decision (above): the agent is told at its next turn in the channel |
| Two bzz processes, one answering in the TUI while the agent's turn reads the outcome | `app_version_told` marks only an untold row, in one statement; not tested with two processes |
| Windows and macOS | not compiled or run here; the scripted tests are `cfg(unix)` as the harness tests are |
| A coding harness's own sandbox and the store's files | an agent has no tool that writes them; a coding harness writes only where its guard lets it (v0.15), and a file changed on disk is found out by its hash (M4). Not tested with a coding harness trying to write `<data>/apps` |

### Found on the way

- **The `From:` line of the approval reaches the edge at 120 columns**
  with an agent's key and the channel's id: the person who asked was cut
  off. It has a line of its own now (`Asked by:`); the `From:` line itself
  is as M4 left it, and still loses the end of the channel's id there.
- **An agent's failed proposals are not recorded,** unlike the owner's
  folder in M4: recording them would make an app row for every name an
  agent tries.
- **Ana must be allowed to wake the agent** for "a person who may wake it
  asks": the default rule is the owner only, so the scripted test sets an
  allow-list, as an owner would.
- **The scripted harness's `fill` does not touch a script** whose text
  holds `$`: only a whole string that starts with `$` is replaced, so a
  Lua script passes through a step's arguments unchanged.
- **`test-relay.sh` with `CARGO_TARGET_DIR` unset** and `./target` linked
  to the worktree's directory builds nothing of Buzz and runs the pin's
  own `target/` (M7 found the opposite with the variable set).
