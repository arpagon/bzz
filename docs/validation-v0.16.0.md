# bzz v0.16.0 validation

**Status:** in progress. M0 is recorded below for the parts that touch the
repository (the baseline, the interpreter in the build, questions 1, 2 and
6). Questions 3, 4 and 5 are answered by other work and are marked pending
here until their answers are brought in. Nothing was pushed: what only CI
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
