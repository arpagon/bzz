# bzz v0.16.0 validation

**Status:** in progress. M0 is recorded below, and the first half of M1:
the app runtime and its contract (M1-A). The bench, the example host and
the remaining apps of M1 (M1-B) are not here yet. Nothing was pushed: what only CI
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
