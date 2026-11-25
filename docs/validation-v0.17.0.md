# v0.17.0 validation

What was proven for v0.17.0, how, and what was not. Plan:
[`planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md). Release
notes: [`release-v0.17.0.md`](release-v0.17.0.md). Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

Each task that built part of v0.17.0 recorded its proofs in a file of its
own, [`validation-v0.17.0/`](validation-v0.17.0/). This document joins them,
one section for each track of the plan in the plan's order, each task's text
as it wrote it (its headings two levels down, its links pointing from here).
Before the tracks: what the principal ran and saw himself, and what went
wrong on the way that a reader of this record should know.
`<scratch>` is a folder outside the repository on the machine the work was
done on. Linux x86_64 unless a section says otherwise.

## Where each item is proven

| Item | Proof | Note |
|---|---|---|
| A1 | [demo](#v0170-validation--task-demo-a1-a4-a8) |  |
| A2 | [demo2](#v0170-validation--task-demo2-a2-and-the-demos-loose-ends) |  |
| A3 | [demo](#v0170-validation--task-demo-a1-a4-a8) |  |
| A4 | [demo](#v0170-validation--task-demo-a1-a4-a8) |  |
| A5 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) |  |
| A6 | [install](#v0170--task-install-a6-a7-f6) |  |
| A7 | [install](#v0170--task-install-a6-a7-f6) |  |
| A8 | [demo](#v0170-validation--task-demo-a1-a4-a8) |  |
| A9 | [join](#v0170--join-joining-a-community-without-leaving-bzz-a9) | Pairing with Buzz desktop and the browser sign-in were not run against the real thing; an invite was claimed on a scripted relay only |
| B1 | [viz](#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4) |  |
| B2 | [viz](#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4) | Proven on the engine on the pinned relay with generated frames, not with a recorded call in the interface |
| B3 | [viz](#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4) | As B2 |
| B4 | [viz](#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4) |  |
| B5 | [viz2](#v0170--viz2-the-zoomed-huddle-its-styles-the-microphone-check-b5-to-b7) |  |
| B6 | [viz2](#v0170--viz2-the-zoomed-huddle-its-styles-the-microphone-check-b5-to-b7) |  |
| B7 | [viz2](#v0170--viz2-the-zoomed-huddle-its-styles-the-microphone-check-b5-to-b7) | With a fake device only |
| B8 | not built; `viz2` measured the frame instead (its section, "B8"). Moved to v0.17.1 |  |
| C1 | [palette](#v0170-validation-the-palette-c1) |  |
| C2 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| C3 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) |  |
| C4 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) |  |
| C5 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) |  |
| C6 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) |  |
| C7 | [find](#v0170-validation--task-find-c3-to-c7-a5-rule-19) | `u` marks the channel unread, not "from here" (the plan's D5 wording) |
| C8 | [registry](#v0170-validation--task-registry-c8-c10-c11-rules-19-20-and-23) |  |
| C9 | [palette2](#v0170-validation-arguments-as-steps-and-more-to-find-c9-c12) |  |
| C10 | [registry](#v0170-validation--task-registry-c8-c10-c11-rules-19-20-and-23) |  |
| C11 | [registry](#v0170-validation--task-registry-c8-c10-c11-rules-19-20-and-23) |  |
| C12 | [palette2](#v0170-validation-arguments-as-steps-and-more-to-find-c9-c12) |  |
| L1 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L2 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L3 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L4 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L5 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L6 | [look](#v0170--task-look-l1-to-l6-c2) |  |
| L7 | [motion](#v0170--task-motion-l7-l8-l9-one-look) | Toasts carry bzz's own confirmations; an app's `Saved.` is the app's own text and stays (D10) |
| L8 | [motion](#v0170--task-motion-l7-l8-l9-one-look) |  |
| L9 | [motion](#v0170--task-motion-l7-l8-l9-one-look) | Measured on debug builds on a loaded machine |
| L10 | [layout](#v0170--task-layout-l10-l11-l12-f7) |  |
| L11 | [layout](#v0170--task-layout-l10-l11-l12-f7) |  |
| L12 | [layout](#v0170--task-layout-l10-l11-l12-f7) |  |
| E1 | [sound](#v0170-validation-sound-e1-decision-d3) | PipeWire in containers, PulseAudio in CI; no real card |
| F1 | [board](#v0170--task-board-f1-f2-f3-f4-f5) |  |
| F2 | [board](#v0170--task-board-f1-f2-f3-f4-f5) |  |
| F3 | [board](#v0170--task-board-f1-f2-f3-f4-f5) |  |
| F4 | [board](#v0170--task-board-f1-f2-f3-f4-f5) |  |
| F5 | [board](#v0170--task-board-f1-f2-f3-f4-f5) |  |
| F6 | [install](#v0170--task-install-a6-a7-f6) · [ci](#v0170--task-ci-ci-green-and-quick) | Two consecutive green runs on `main`: not yet, nothing is merged to `main` |
| F7 | [layout](#v0170--task-layout-l10-l11-l12-f7) |  |
| F8 | [release](#v0170--task-release-the-upgrade-from-v0160-the-version-f8) · [release2](#v0170--task-release2-the-documents-brought-level-with-the-code) | The version commit is held, to be placed last; the branch head still says 0.16.0 |
| W1 | [windows](#v0170-validation-windows-task-windows-plan-w1-and-w2) |  |
| W2 | [windows](#v0170-validation-windows-task-windows-plan-w1-and-w2) | Also walked by the principal with CI's release binary ([the principal's own checks](#windows-with-cis-release-binary)); a paste still arrives as keys |
| W3 | Done by the principal on 2026-11-26 over SSH in a macOS 14 virtual machine, with CI's Intel release binary ([the principal's own checks](#the-principals-own-checks)); not at a terminal program, not Apple Silicon |  |

## The principal's own checks

What the principal ran and saw himself, apart from the tasks, taken from his
log of the build. Commits are of the branch `v0.17.0`. Generated identities
and data only; no real account.

### Before the build (2026-11-24)

- **v0.16.0 as published, in clean containers.** It installs in 3 to 4 s
  on `debian:stable-slim` and `ubuntu:22.04` and then does not start:
  `libasound.so.2` is missing. `ubuntu:20.04` and Alpine get no download.
  `bzz app new` and `bzz app try` work there. This is the plan's "Why now".
- **The prototype of D3** (the research's copy of bzz on the `pulseaudio`
  crate): `readelf -d` names no `libasound`, and `bzz --version` answers in
  `debian:stable-slim` with `--network none`.
- **v0.16.0 as published, in a Windows 11 virtual machine**, over SSH:
  installed with the README's PowerShell line (78 s); the command line
  works; the interface starts in the console; `?` does nothing, `q` works.
  This became task `windows` (W1).

### Driving the merged build (2026-11-25)

Each time on the merged debug build, in `bzz demo`, through tmux, with a
scratch home and no network.

- **At `9902c74`** (look, viz2 and the wave-1 tasks): the status line reads
  `NORMAL #team … DEMO · generated, offline … Ctrl-K actions · ? keys`; the
  sidebar shows unread counts; `Ctrl-K` lists the actions for the selected
  message; the help scrolls, with the keys at its frame's foot. Seen then:
  the palette's row said "Mark unread from here" while `u` marks the
  channel. It now reads "Mark this channel unread".
- **At `8a95a33`** (sound merged): `scripts/check-linux-linkage.sh` on the
  merged binary: `NEEDED` is `libgcc_s`, `libm`, `libc` and the loader.
- **At `5dd3d4c`** (motion merged): at rest in `#team`, 70 ms of CPU in
  10 s (task `motion` measured 80 to 86 ms after and 633 ms before, its
  section "At rest"). The palette's step "Set my status › status" shows its
  usage line.
- **At `006d8d1`** (registry merged): the footer follows the focus,
  `Enter thread · r react · i reply · Ctrl-K actions` on a message and
  `Enter send · Ctrl-J newline · Esc close` in the composer; `:hu`, Tab
  gives `:huddle`, Tab again `start`, the usage under the line. Seen: the
  hint said `i reply` on a message, where `i` writes in the channel (fixed
  on 2026-11-26: it says `i write`); a space typed before Tab leaves two
  (`:huddle  start`), not fixed.
- **At `6fcd428`** (layout merged), at 80×24: opening a thread hides the
  sidebar and the frame says `hidden · channels 2`; `z` shows the
  conversation alone (`hidden · the other panes z`); `Esc` with nothing open
  says `nothing to close · q quits`.

### Windows with CI's release binary

On 2026-11-25, in the Windows 11 virtual machine, over SSH in a console
(ConPTY), with the MSVC release binary CI built at `0d96e80` (artifact
`bzz-x86_64-pc-windows-msvc` of run 45609541408): `bzz demo` opens; `?`
shows the help; `Ctrl-K` and `thr` put "Open thread" first; `:hu`, Tab
gives `:huddle` and its usage; in `#launch` the Board: `5`, `n`, a card,
`]` moves it to Doing, `Ctrl-c` leaves the Board; `:huddle zoom`, `v`, the
bars previewed; `q`, `y` quits and gives the console back; no `bzz-demo-*`
folder is left. Run with `BZZ_NO_MOTION=1`, so the lanes were still. Not
done there: a relay community, a paste, a person at Windows Terminal.

### Gate runs

In a checkout of their own, and from `m11` on with a target directory of
their own too ([what went wrong](#what-went-wrong-on-the-way), 1). "All
green": `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D
warnings`, the example, `cargo test --locked --all-targets`, `cargo test
--locked --no-default-features --lib`, `cargo deny check`, `cargo audit`;
from `m14` also `cargo clippy --all-targets --no-default-features`
([what went wrong](#what-went-wrong-on-the-way), 4).

| Run | Commit | Result |
|---|---|---|
| `m1-viz` | `bfd340f` | all green |
| `m3-windows-demo` | `64156d3` | all green |
| `m5-palette-find` | the palette and find merges | failed: find's usage test read the first screen behind the prompt (a test's fault, fixed) and `agent_runtime_test` under load (passes alone) |
| `m6-look`, `m7-viz2` | `18c4eb9`, `9902c74` | all green |
| `m8-demo2` | the demo2 merge | failed only `demo_offline_test`, which read the Board's old title (fixed in `8a95a33`) |
| `m9-sound` | `e144e52` | reported green; **not counted** ([what went wrong](#what-went-wrong-on-the-way), 1) |
| `m10-palette2` | `8a95a33` | failed only `an_endless_loop_is_stopped_by_time`, under load (task `tidy` made it wait for the event) |
| `m11-board` | `60f6a8b` | died in its test stage with no output; cause unknown ([what went wrong](#what-went-wrong-on-the-way), 5) |
| `m13-motion` | `5dd3d4c` | all green: the first full run of the merged work with a target directory of its own |
| `m14-join` | `0d96e80` | all green, with the no-default-features clippy |
| `m15-tidy` | `12b355b` | all green |
| `m16-layout` | `6fcd428` | **all green, with every planned piece merged** |
| `m17-ci` | `18ff30a` | running when this was written |

### Scenario runs

`m13-motion`, at `5dd3d4c`: every harness scenario of
`scripts/tui-herdr-scenarios.toml` on the pinned relay through tmux, at
200×50, in the gate's checkout: **43 of 49**. The six that failed are the
ones the tasks had named: scenarios that pass alone and fail after the
v0.16 scenarios leave the Board open or post to `#t9-general` (the order of
the suite), one stale string (`t17-palette2-find`), and
`t15-coding-harnesses-in-panel`, which depends on the programs installed
on the machine. No product defect was seen. The tasks' own runs are in
their sections (`motion` 48 of 54, `registry` 42 of 50, `layout` 42 of 44 at
200×50 and 37 of 44 at 80×24). Task `suite`, which makes the whole suite
pass in any order and at 80×24, had not landed when this was written.

### CI on the pull request

The draft pull request of D14, read on GitHub:

- **`e144e52`** (through sound): green on Linux and macOS, among them the
  macOS builds of the sound change, the upgrade and the agent host on
  `macos-15-intel`, and the audio job on macOS. On Windows the `build` job
  failed five tests: four goldens checked out with CRLF and the orb's
  slow-writer test (`sleep(0)`). Fixed in `5859f29`.
- **`24f7486`**: Windows `build` green. Job `test` failed: `cargo clippy
  --all-targets --no-default-features` on the v0.16.0 upgrade test. Fixed
  in `006d8d1`.
- **`006d8d1`**: 21 jobs passed, none failed, one pending: the Linux audio
  job, which never ended ([what went wrong](#what-went-wrong-on-the-way), 3).
- **`agent-host` on `macos-15-intel`** failed twice in twelve runs on a
  30 s `initialize` timeout (`64156d3`, `0d96e80`); the agent host is
  v0.16.0's, unchanged. Task `ci` gave the client's waits a margin there.
- **`18ff30a`** (ci merged): queued when this was written. Its audio job's
  time, the Windows and macOS builds and the agent host on the Intel Mac
  are still to be read.
- **At `18ff30a`, CI on the pull request (2026-11-26):** four jobs failed,
  from two causes, neither a defect of bzz. The layout's goldens held
  message times and date lines as this machine draws them (it is not in
  UTC; CI is), and failed on Linux and Windows; the audio test passed on
  GitHub's runner in 8.14 s (round trip 20 ms, no holes) and the job then
  failed removing its folder while the sound server was still leaving.
  Fixed in `2137ff5`: the fixture's messages fall inside five minutes that
  no time zone splits across midnight and times and dates are masked (run
  under five time zones); the script's cleanup cannot fail a passed run.
  The principal's gates now run in UTC.
- **At `2137ff5`:** the gates were green except the lint of the build
  without apps or audio, which named a helper the principal had just added
  (used only with apps); CI's `test` job failed on the same line. Fixed with
  the merge of the scenario suite. The macOS build on Apple Silicon was
  cancelled by GitHub for lack of a runner ("not acquired by Runner of type
  hosted"), as on `main` before this version; not a result.
- **The candidate's own Linux binary** (built by CI at `c79f827`, version
  0.17.0, 2026-11-26), with nothing installed and no network:
  `bzz --version`, `bzz paths` and `bzz demo --help` answer on
  `debian:stable-slim`, `ubuntu:22.04`, `ubuntu:24.04`,
  `almalinux:9-minimal` and `fedora:latest`; on `ubuntu:20.04` the loader
  refuses it (glibc 2.31) and on Alpine it does not start (musl), as the
  release notes say. Its `NEEDED` names no sound library.
- **A defect found there and fixed.** In that Debian container `bzz demo`
  ended with "network error: builder error": the image has no certificate
  store, and reqwest cannot build a client without one. Every earlier proof
  of the demo had run where a store exists. Fixed with
  `protocol::http::build_client` (a client with no trusted certificate when
  the store cannot be loaded), used by the relay's HTTP client, media,
  avatars, the join form's probe, the agents' tool server and the sign-in
  service; `tests/demo_offline_test.rs` now runs the demo with no store,
  seen failing without the fix and passing with it.
- **Windows again, with the candidate's binary** (CI's MSVC release build
  at `c79f827`, `bzz 0.17.0`, 2026-11-26), in the console over SSH: the
  demo; the Board (a card added and moved); `z` shows the conversation
  alone; Esc with nothing open says "nothing to close · q quits"; `:join`
  in the demo says it needs a real community; the first screen without a
  community; leaving leaves no folder and no process.
- **macOS, for the first time** (W3, 2026-11-26): CI's Intel release binary
  at `c79f827` on macOS 14.8 in a virtual machine (an emulated Intel
  processor), over SSH through tmux, with a scratch home. `bzz --version`
  and `bzz paths`; the demo's first screen; `?`; `Ctrl-K` with "thr" puts
  Open thread first; `:hu`, Tab; a message with an accented letter and
  Scout's answer in its thread; the Board (a card added and moved); the
  huddle zoomed, its lanes moving between two captures two seconds apart,
  and the orb; `q`, `y` leaves the shell clean, no process and no demo
  folder; the first screen without a community and `j` opening the join
  form. Not done there: a keychain, a relay, sound, a terminal program at
  the machine, Apple Silicon.
- **The candidate, whole** (`c79f827`, the version commit, 2026-11-26), in
  a checkout and a build directory of its own, in UTC: format, both lints,
  the tests, the build without default features, licences and the security
  audit; the apps bench in release; the upgrade from v0.16.0 and from
  v0.15.0; the release build and its linkage; the pinned relay's suite (30
  tests) and the federated-identity suite; every scenario at 200×50 (52 of
  52) and at 80×24 (48 of 48, four skipped for their size); the own-quota
  test. All green, the relay down after.
- **Two things the site's preview showed, fixed on the candidate**
  (2026-11-26, the owner chose both). In the capture of `bzz demo`'s first
  screen the keys Ada's welcome asks for were the hardest text to read:
  code in a sentence was muted and dim. It now takes the theme's accent
  (`MarkdownInlineCode`; a theme that sets only `MarkdownCode` keeps both
  alike), with tests of the group in every built-in theme and of the
  rendered spans. And Ada's and Sam's badges drew `◆`, the agent's mark:
  the badge's four shapes no longer include it (`▼`), with a test over 256
  keys; sixteen layout goldens changed in that one character and nothing
  else.
- **The site's section, looked at before it was placed** (2026-11-26). A
  task made the three captures from `bzz demo` and a preview of the page;
  its review found the first proposal's palette and huddle captures too
  small to read and its loop without a way to play, and cards that said
  more than these notes prove. The owner approved the corrected section.
  The principal made the captures again on the release build with the two
  fixes above, searched them (the tapes' bytes, every distinct screen, an
  OCR of 65 images: nothing of the machine or its owner), placed them, and
  looked at the page served from `site/public` at 1280 and 390 wide in a
  headless browser: no request failed, the terminal's text reads at both.
  Not looked at in a real browser or on a phone; nothing deployed.
- **The candidate again, after those two fixes** (`fdfa004`, 2026-11-26):
  the same pass as above, green in everything but one scenario at 80×24:
  `t15-agent-answers-mention` saw no reply from the scripted agent within
  90 s (47 of 48). The same scenario passed at 200×50 in that pass, and at
  80×24 in the two passes before. The principal ran it with its two
  neighbours eight times and the whole suite at 80×24 six times more: all
  passed. The failing run's profile and journal had been removed with the
  run, so the cause is unknown; `scripts/test-relay.sh` now keeps the
  fixture's profiles under `BZZ_TMUX_KEEP`, for the next time. One of the
  principal's own repeat runs failed for a reason of its own making: a
  longer scratch path wrapped a line of the agents panel at 80×24 and
  pushed the text the scenario waits for off the screen.

## What went wrong on the way

What would mislead a reader of this record if it were left out. Each was
found by the principal or by a task, and each is fixed or named.

1. **A gate run that proved nothing.** Until `m10`, the principal's gate
   checkout and his own cargo runs in the main checkout built into one
   target directory. Both write `debug/bzz`, and the tests that start the
   binary (`demo_offline_test`, the agent tests) could run the other
   checkout's build. Evidence: `m9-sound` at `e144e52` reported
   `demo_offline_test` green although that test's expectations cannot match
   that commit's binary, and two local failures that were never explained.
   `m9`'s green is not counted. `e144e52` had been pushed on it (to the
   draft pull request, whose CI builds on its own). From `m11` every gate
   run has a target directory of its own; one target directory per
   checkout, as the v0.16 rules already said.
2. **A merge that mixed scenarios.** A helper that resolved the
   append-against-append conflicts of `scripts/tui-herdr-scenarios.toml`
   interleaved the two sides' hunks and corrupted five `t17` scenarios in
   the palette merge. It was never pushed. The file was rebuilt from each
   branch's own scenarios in the find merge and checked scenario by
   scenario. Every later merge took each scenario whole, by its id. Task
   `palette2` was cut from the corrupted commit, so only its own scenarios
   were taken from it.
3. **An audio test in CI that hung.** T7 (a tone through PulseAudio in the
   CI image, `audio (ubuntu-latest)`) took 731 s at `e144e52` and never
   ended at `24f7486`, `006d8d1` and `0d96e80` (over two hours; the
   principal cancelled two superseded runs). Cause, found by task `ci`: the
   `pulseaudio` crate built without optimisation, as a debug test build
   builds it, spends about 4 ms growing a 1 MiB buffer before every read of
   the socket, and writes playback only when a read finds nothing waiting;
   capture keeps arriving, so playback starves. Not bzz's device code:
   optimised, the same read takes 0.018 ms, and release builds are not
   affected. Fixed by building that crate optimised in every debug build
   (`[profile.dev.package.pulseaudio] opt-level = 3`, `18ff30a`) and by a
   bound on every wait of T7, which now ends in about 8 s in the CI image
   (task `ci`). The "21 passed, none failed" at `006d8d1` had this job still
   pending.
4. **The principal's gates lacked one of CI's checks**, `cargo clippy
   --all-targets --no-default-features`. CI found the failure at `24f7486`;
   the check was added to the gates.
5. **A gate run died, and a gate script was edited while it ran.** `m11`
   stopped in its test stage with no output (cause unknown) and recorded
   nothing. `m13`'s status shows "test ok" twice because its script was
   edited while it ran (bash reads a script as it goes); its results stand.
6. **Tests that depend on time failed under load** (seven or more builds
   at once, load 20 to 50): `agent_runtime_test`, the apps' time budgets, a
   join's wait, the layout goldens. Each passed alone. Task `tidy` made five
   of them wait for the event instead of a fixed time, and the gates set
   `BZZ_APPS_STOP_MARGIN_MS=3000` as CI does.
7. **Text pasted into two task files.** In `registry`'s "Commands run" and
   in `layout`'s row for `bzz demo` and its "Relay suite" commands, the
   variable `BZZ_TMUX_CAPTURES=<scratch>/…` was replaced by the text of the
   file's own "Captures" section. Read those commands as
   `BZZ_TMUX_CAPTURES=<scratch>/…` followed by the rest of the line. The
   task files are left as their tasks wrote them.

## A — The first minute

### v0.17.0 validation — task `demo` (A1, A4, A8)

Plan: [`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
track A, D2, rule 22. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
Linux x86_64, 2026-11-24, branch `t3/demo` cut from `v0.17.0` at `543dbd1`;
`v0.17.0` merged in at `bfd340f` (viz) for the huddle's lanes. The rows
above "The huddle of `#infra`" were measured before that merge.
The machine was shared with six other agents' builds throughout (load
average 16 to 28), which is in every time below.

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every bzz run by hand had a scratch home (`HOME` and the four
`XDG_*_HOME` in `<scratch>/home`, `DBUS_SESSION_BUS_ADDRESS` unset, `TMPDIR`
in `<scratch>/tmp`); `bzz paths` named only `<scratch>/home`.

#### What was built

- `bzz demo` (`src/demo/`): the real client against a relay held in memory
  on `127.0.0.1:<port the system picks>` (`relay.rs`), the generated world
  (`world.rs`) written first to a store held in memory, a private
  `bzz-demo-<pid>-<random>` folder (mode 0700) for the rest. Dispatched in
  `main` before `Paths::discover`.
- The first screen with no community (`src/ui/first_screen.rs`); `d` there
  opens the demo in the same process (`App::run_to_exit` → `Exit::OpenDemo`).
- Scout: a verified agent (Ada's NIP-OA proof on its profile, `bot`
  membership, a kind 10100 declaration, Ada's 30177 policy `respond_to =
  anyone`), so bzz marks it `◆` and shows it typing.

#### Proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| The demo fills its first screen offline in under 1 s | PTY in a network namespace with loopback only | `BZZ_DEMO_BIN=<scratch>/bzz-after cargo test --locked --test demo_offline_test -- --nocapture`, ten times | Two series of ten. First series: 1.137 s (the first run after the binary was rebuilt on a hard disk, load 20), then 0.497, 0.125, 0.159, 0.160, 0.135, 0.161, 0.180, 0.300, 0.200 s. Second series: 0.267, 0.187, 0.226, 0.227, 0.165, 0.135, 0.142, 0.165, 0.129, 0.133 s. 19 of 20 under 1 s; see "Not verified" |
| The same, with the debug build of `cargo test` | same | `cargo test --locked --test demo_offline_test -- --nocapture` | 0.289 s (limit 10 s for debug) |
| "Full" means: `#team` open, `DEMO · generated, offline`, Ada's welcome with its three things to try, `@Lin` (the DM's name, which needs the seeded store), `#infra` | read | `tests/demo_offline_test.rs` `full()` | — |
| On a machine that is not this one | clean `debian:stable-slim` (13.8, glibc 2.41) with `libasound2t64` 1.2.14 and tmux, `docker run --network none` | `<scratch>/container/try.sh` with the release binary mounted | Eight runs: 237, 165, 224, 171, 245, 164, 169, 143 ms to the full screen. A message written stays and Ada answers (`after-write-80x24.txt`). `demo folders left: 0`, `home changed: no` |
| Before the store moved into memory, the same container gave 0.3 to 2.3 s | same | same, binary of `9df4396` | 655, 343, 316, 1033, 608, 1370, 2346, 1161 ms: the fresh database's migrations waited on the container's disk (`bzz check` on a fresh home there: 185 to 1051 ms). Fixed in `d53cf5e` |
| No address outside loopback in the generated world | unit | `cargo test --locked --lib demo::` (`the_world_names_no_address_outside_loopback`) | Every `://` in every event is `<origin>/media/…` (five pictures); no dotted address but `127.0.0.1`; no `.com`/`.dev`/`.xyz`/`.org`/`.net`/`.io`/`.local`; not `$USER`, `$LOGNAME`, `$HOSTNAME` or `/etc/hostname` |
| The relay answers only under its token, media only to the viewer's signature, writes only from the viewer | unit, real sockets on loopback | `the_relay_serves_only_the_token_and_the_viewer` | `GET /` and `POST /query` → 404; under `/<token>/` → 200; `/media/…` without authorization → 401, with a kind-24242 event of another key → 401, of the viewer → 200; a WebSocket on `/` is refused; on `/<token>/` the AUTH challenge comes, a viewer's message is stored, sent back live and answered; a stranger's event gets `OK false` |
| Scout is a verified, eligible agent of `#team` in the seeded store | unit | `scout_is_a_verified_agent_in_the_seeded_store` | one agent, `Scout`, not stale, in `#team`, `Eligible` |
| Scout types, then answers in the thread of its mention; the channel's typing line ends as the answer comes | unit; tmux | `scout_types_then_answers_in_the_thread_of_its_mention`; scenario `t17-demo-first-minute` steps 10–12 | kinds `[20002, 20002, 9]`, the typing lines in channel and thread scope, the answer tagged with the mention's id. In tmux: `◆ Scout ⠋` in the status line 0.99 s after Enter, `↳ 1 reply` 2.1 s after, then `◆ Scout · owned by Ada` with its answer in the thread (`<scratch>/captures/t17-demo-first-minute/10.txt` to `12.txt`). By hand, the typing line was gone one second after the answer |
| Teammates answer a few times, then go quiet; others' messages are never answered | unit | `teammates_answer_a_few_times_then_go_quiet` | 4 answers for 8 messages; 0 for a stranger |
| The demo's first minute works | tmux, release binary, 80×24 | `BZZ_BIN=<scratch>/bzz-after BZZ_TMUX_SCENARIOS=t17-demo-first-screen,t17-demo-first-minute BZZ_TMUX_CAPTURES=<scratch>/captures python3 scripts/test-tui-tmux.py` | `2 of 2 scenarios passed`: move to the checklist, its thread (`context · 4 replies`), react (`👍 1` under the welcome), search `rollback` (`rollback plan is step 7`), write (`Welcome aboard` 1.7 s later), mention Scout and read its answer, quit with exit 0 |
| The folder is removed on quitting and on each signal bzz handles; a killed demo's folder by the next demo | release binary by hand, tmux | `<scratch>/cleanup.txt` | `q y`: exit 0, 0 folders. SIGINT, SIGTERM (`kill`), SIGHUP: exit 0, 0 folders. `kill -9`: exit 137, its folder stays; the next `bzz demo` removed it on start (`bzz-demo-3813465-…` gone, `bzz-demo-3814271-…` its own) and left 0 when it quit |
| The folder goes after a panic, and again once the runtime has stopped | unit | `the_folder_goes_on_a_panic_and_after_the_runtime_stops`, `a_folder_left_by_a_killed_demo_is_removed_and_a_running_one_kept` | removed by the unwinding guard; recreated by a late write and removed by `clean_up_after_exit`; a dead process's folder is left behind, a live one's and a name without a pid are kept |
| The real configuration is byte-identical | PTY test; by hand | `demo_offline_test` (a home with a `config.toml` of its own, every file and its bytes before and after); by hand a sha256 of the scratch home's tree, before and after a demo where a theme was chosen and saved | identical; the theme was saved in the demo's own `config/config.toml`, which went with its folder |
| The size of the release binary | measured | `cargo build --locked --release --bin bzz` at `543dbd1` and at the branch | 31 605 344 → 31 883 232 bytes: **+277 888** (limit +350 KB). The five 96-px avatars are 13 473 bytes of it |
| What needs a real community says so | unit; tmux | `what_needs_a_real_community_says_so`; `:huddle`, `j`, `y` in `#infra`, `:apps`, `:community add` by hand | `Demo: the huddle is a recording to look at.` (the panel's `j` answers before any confirmation), `Demo: the Board comes in a later version.`, `Demo only: quit and run bzz to join a real community.` |
| The first screen at 80×24 and 200×50, with and without a choice's commands | goldens | `cargo test --locked --lib first_screen` (`tests/fixtures/first-screen/*.txt`, `BZZ_BLESS=1` writes them) | four goldens; `d` gives `OpenDemo`, `j`/`p`/`k` show their commands, Esc hides them; Esc with nothing shown, `q`, `?`, `:` and Ctrl-d stay bzz's |
| `d` on the first screen opens the demo in the same process | tmux, release binary | scenario `t17-demo-first-screen` | passed: `j` shows `bzz identity new --label me`, Esc, `d` → `DEMO · generated, offline`, quit exit 0 |
| The welcome screen still shows command feedback | unit (existing) | `empty_profile_welcome_screen_shows_command_feedback` | passes unchanged |

##### The huddle of `#infra`, with viz's lanes (after the merge)

| Claim | Rung | Command | Seen |
|---|---|---|---|
| Ada, Lin and Sam speak in generated voices in `#infra`'s huddle; You listen muted, your lane still (the script's lines for you are left out) | unit | `cargo test --locked --lib demo::` (`what_needs_a_real_community_says_so`: peers `[1, 2, 3]`) | passes |
| The recording plays only where its huddle runs, and the panel does not contradict its lanes | unit, 80×24 render | `cargo test --locked --lib the_demo_huddle_is_a_recording_only_where_it_runs` | in `#team` no lanes and no strip; in `#infra` the lanes, `a generated recording`, `4 in the call` (the panel's roster: `4 in it`), `nothing is sent`; none of `No huddle is running here`, `you are not in the call`, `j join`, `m unmutes`, `s start` |
| The same on screen | tmux, debug build, 160×45 | scenario `t17-demo-huddle` | passed: the strip `🎧 ⣀…⣶ Ada · mic muted` in `#infra`'s status line, the panel's title `a generated recording · Esc`, the microphone line `a recording · no microphone is open, nothing is sent`, `j` → `Demo: the huddle is a recording to look at.` |
| No sound device is opened | strace | `strace -f -e trace=openat,connect bzz demo`, then `#infra`, `:huddle`, `j`, `m`, quit | the only sound file opened is `libasound.so.2` by the loader at start (the link E1 removes); no `/dev/snd`, PulseAudio or PipeWire socket. Every `inet` connect is to 127.0.0.1 |

| After the merge, on the release binary | tmux; PTY in a namespace | the three `t17-demo-*` scenarios, three times; `demo_offline_test` with `BZZ_DEMO_BIN`, five times | `3 of 3 scenarios passed` each time (once the mention waits a second after Ada's answer: two messages of one second sort by id); full first screen in 0.169, 0.144, 0.130, 0.127, 0.126 s. The merged binary is 31 989 216 bytes, viz's work included; this task's own delta was measured before the merge (+277 888) |
| The gates, on the merged branch | — | `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo build --locked --example scripted_harness`, `cargo test --locked --all-targets`, `cargo test --locked --no-default-features --lib` | all green; 1 662 tests passed, 0 failed |

Captures, read as a newcomer: `<scratch>/first-screen-80x24.txt`,
`first-screen-join-80x24.txt`, `first-screen-160x45.txt`,
`first-screen-import-160x45.txt`, `demo-160x45-first.txt`,
`demo-160x45-thread.txt`, `demo-160x45-dm.txt`,
`demo-80x24-theme-preview.txt`, `demo-80x24-huddle.txt`,
`demo-80x24-infra-strip.txt`, the scenario steps in `<scratch>/captures/`,
the container's `first-80x24.txt` and `after-write-80x24.txt`.

#### Not verified

- **One cold start of twenty took 1.14 s**: the first after the binary was
  rebuilt, read from a hard disk shared with six builds (load 20). The
  other nineteen took 0.13 to 0.50 s. Not measured on an idle machine.
- **Ctrl-C as a key** does not leave the demo: in bzz's NORMAL mode Ctrl-C
  is not quit (v0.16 behaviour, rule 19), and in raw mode the terminal
  sends no SIGINT. SIGINT, SIGTERM and SIGHUP from outside do end it and
  clean up.
- **`kill -9`** leaves the folder until the next demo starts; nothing can
  run at SIGKILL. On Windows and macOS, the next demo removes only folders
  older than a day (no process check there); neither was run.
- **A namespace whose loopback is down** (a bare `unshare -n`): no socket
  reaches 127.0.0.1 there, so the demo shows its seeded world but cannot
  write. The test brings loopback up, as `docker --network none` does.
- **Avatars**: the relay serves the five pictures to the viewer's signed
  request (unit test), but in tmux (halfblocks) bzz asked for none; not seen
  in a terminal with an image protocol.
- **Scout after ten minutes**: bzz marks an agent stale ten minutes after it
  was verified; a demo left open that long was not watched.
- macOS and Windows: not built or run here (W2 is the principal's).

### v0.17.0 validation — task `demo2` (A2, and the demo's loose ends)

Plan: [`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
track A, D2, rules 19 to 22. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
Linux x86_64, 2026-11-25, branch `t3/implement-demo2-task` cut from
`v0.17.0` at `64156d3` (the merge of `demo`). The machine was shared with
other agents' builds throughout, which is in every time below.

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every bzz run by hand had a scratch home (`HOME` and the four
`XDG_*_HOME` in `<scratch>/home`, `DBUS_SESSION_BUS_ADDRESS` unset, `TMPDIR`
in `<scratch>/tmp`). `bzz-before` is the release build of `64156d3`,
`bzz-after` the release build of this branch.

#### What was built

- **The Board in `#launch`** (`src/demo/board.rs`). The shipped `board`
  template is proposed as the owner's, loaded apart, and approved by its
  hash through `Apps` in the demo's own store (in memory; its three files
  in the demo's temporary folder), then opened in `#launch`. bzz's pane
  takes it from `Apps::runnable`, as any app: there is no other door. The
  records are a `SharedStore` held in memory (`live::Where::Memory`, which
  was test-only), seeded with eight cards of Harbor 2.4's last days.
- **A teammate moves a card** while the Board is on screen: 15 s after it
  is first shown, then at most once a minute, never within 10 s of a key or
  a paste in the Board, three moves in all; each is said in `#launch`
  (`Lin: Moved "Blog post review" to Done on the Board.`), so the newcomer
  sees who changed the shared records.
- **Scout uses the Board.** `@Scout add <title>` and `@Scout move <title>
  [to <column>]` (the verb first, after the mention) add or move a card
  when Scout answers, and Scout says what it did in the thread. Anything
  else about the Board gets one sentence of what it can do there; any other
  mention gets its usual answer, which now names the Board too.
- **Ada's welcome** names the Board, its keys (`5`, `n`, `]`) and the words
  for Scout; Ada says the same in `#launch`.
- **`:app`** in the demo: `open`, `close`, `zoom`, `brief` and `:apps` work
  as anywhere; every other verb (add, new, dev, share, unshare, pending,
  approve, refuse, rollback, enable, disable, remove) answers
  `Demo: only the Board.` The `:apps` panel's note says the Board is
  installed and approved.
- **Loose ends**: a huddle person who has not spoken says "quiet"; the
  demo's status line has no `HALFBLOCKS`; an answer is dated after what it
  answers; every notice of the demo fits the status line at 80 columns.

#### Proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| The Board is the shipped template, approved in the demo's store, handed out by `Apps::runnable`; its files are in the demo's folder | unit | `cargo test --locked --lib demo::` (`the_board_is_the_approved_template_from_the_one_door`) | the runnable's bytes are the template's script, title `Board`, types `["card"]`, open in `#launch`; the app's folder is under the given paths |
| On the seeded cards, `n`, a title, Enter, `]`: the card is in Doing, written by the viewer | unit, headless session | `a_card_added_and_moved_is_in_the_next_column` | `To do (3)` → `To do (4)` → `Doing (3)`; the record's `column` is `doing` |
| **The same in the demo, in a PTY with no network**; leaving leaves nothing, the home byte-identical | PTY in a network namespace with loopback only (`unshare -rn`) | `cargo test --locked --test demo_offline_test` (`the_board_takes_a_card_offline_and_the_demo_leaves_nothing`), debug build | passes: `Space Space`, `launch`, Enter, `Board · 5 focus`, `5`, `n`, `Press kit`, Enter, `]`; `> Press kit` between `Doing (3)` and `Done (3)`; `q y`, `DEMO-ENDED 0`, 0 entries in the demo's `TMPDIR`, the home's tree (with a `config.toml` of its own) equal byte for byte |
| A teammate moves a card: the first 15 s after the Board is first shown, then not within a minute, not within 10 s of a key, not while the Board is not on screen; a move whose card the person moved is skipped; it is said in the channel | unit, a clock stepped by hand | `a_teammate_moves_a_card_calmly_and_not_while_the_person_types` | no move off screen for 600 s; at 614 s none, at 615 s `Blog post review` → Done and `Lin: Moved "Blog post review" to Done on the Board.`; none from 616 to 674 s; a key at 670 s: none at 675 and 679, one at 680; the third move skipped; none after |
| The teammate's move appears with no key pressed | tmux, release binary, 80×24 | scenario `t17-demo2-first-minute` steps 19–20 | `Moved "Blog post review" to` 14.9, 14.7 and 14.9 s after the step began, in the three runs below; `Done (4)` with `Blog post review` under it |
| …and not while the person types in the Board | tmux by hand, release binary, 80×24 | `<scratch>/run.sh 80x24 <scratch>/bzz-after`, `#launch`, `5`, then `j`/`k` every 3 s for 33 s, the screen read after each key | no `Moved` while typing (33.2 s, well past the first 15 s); it came 10.26 s after the last key (`typing-then-move.txt`) |
| Scout reads `add` and `move` as the first word, and nothing else as an order | unit | `scout_reads_add_and_move_and_nothing_else` | `@Scout add Ship party`, `please add a card: Order cake.`, `move launch tweet to Done`, `move the blog post`; "what is on the board?" → what it can do; "what do you do?" and "can you add more tests?" → its usual answer |
| Scout adds and moves cards and says exactly what it did, and what it can do when it cannot | unit | `scout_adds_and_moves_cards_and_says_what_it_did` | `Done: I added "Ship party" to To do on the Board in #launch.` (the card signed by Scout); `Done: I moved "Launch tweet" from To do to Doing …`; a card in Done: "in Done already"; an unknown title: "I found no card called …", the cards, what it can do; no title: "Which card?" |
| Through the relay's script, the card is written when Scout answers, not before | unit | `a_mention_that_asks_scout_writes_on_the_board_when_it_answers` | three answers (two typing lines, the message); no card before the message is signed; then the card in To do |
| The same on screen | tmux, release binary, 80×24 | `t17-demo2-first-minute` steps 9–14; by hand (`scout-add-thread.txt`, `scout-move.txt`, `launch-after-scout.txt`) | `↳ 1 reply` under `@Scout add Ship party`, `Done: I added "Ship party" to To do` in the thread, `To do (4)` with `Ship party` on the Board in `#launch` |
| The welcome names the Board, its keys and the words for Scout | unit; capture | `the_welcome_names_the_board_its_keys_and_the_words_for_scout`; `first-80x24.txt` | `Then our Board in #launch: 5 opens it, n adds a card, ] moves it on. Or ask: @Scout add Ship party` on the first screen at 80×24, below the three things |
| `:apps`, `:app open board`, `5`, `Space 5` work; the rest of `:app` says it is the demo | unit; tmux by hand, 160×45 | `what_needs_a_real_community_says_so`; `apps-160x45.txt`, `space5-160x45.txt` | `board (Board) · version 1 · open in #launch`; Enter opens it; `Space 5` → `Board · zoomed`, three columns side by side; 13 other verbs and a bare `:app` → `Demo: only the Board.` |
| A huddle person who has not spoken since the lanes appeared is "quiet", not "2s ago" | unit, 80×24 render; seen it fail | `cargo test --locked --lib in_the_demo_huddle_who_has_not_spoken_yet_is_quiet` | 30 s of the recording in `#team`, then `#infra` in the loop's silent first 0.35 s: no track has spoken, the panel says `quiet` three times and `ago` only in its scale; then Ada is live. **With the fix taken out it fails** (peer 1 had "spoken") |
| The same on screen | tmux by hand, debug build, 80×24 | `huddle-later-80x24.txt` | `Ada · ● live`, `Lin · quiet`, `Sam · quiet` on opening `:huddle` a minute in |
| No `HALFBLOCKS` in the demo's status line | capture | every capture | ` NORMAL … DEMO · generated, offline    ? help · q quit`; agent `look`'s L3 removes the word everywhere, so the line in `src/app.rs` is a one-line seam that its merge replaces |
| An answer is dated after what it answers, whatever the seconds; the teammate's wait lets it be dated a second before it is sent | unit; seen it fail | `an_answer_is_dated_after_what_it_answers` | a message dated now, now + 1 and now + 5: every answer later; teammates and Scout wait at least 2 s. The scenario's mention no longer needs a pause after Ada's answer: `t17-demo2-first-minute` writes `hello team`, reads `Welcome aboard`, and mentions Scout at once; its answer lands under the mention every run. **Signed at `now` as before, it fails** at the first case (an answer of the same second) |
| Every notice of the demo fits the status line at 80 columns | unit; tmux | `every_notice_of_the_demo_fits_an_80_column_status_line`; `t17-demo2-first-minute` step 18 | each at most 24 columns (`Demo: needs a community.`, `Demo: a recorded huddle.`, `Demo: only the Board.`, `Demo: apps not built in.`); mode + notice + mark + hint ≤ 80; `Demo: only the Board.` whole on screen at 80×24 |
| Nothing is drawn at rest with the Board open (rule 20) | tmux `pipe-pane`, release binaries, 80×24, 10 s windows, twice each | `bzz-before`/`bzz-after`, `#team` and `#launch` | before: `#team` 50 and 25 bytes, `#launch` 0 and 25; after: `#team` 50 and 50, `#launch` 0 and 0. The bytes are a cursor-hide sequence of an empty frame (`ESC[39m ESC[49m ESC[59m ESC[0m ESC[?25l`), in both builds and with no app shown; agent `look`/L9's work, not this task's. The Board asks for a frame only when a card changes |
| The demo's scenarios, three times in a row | tmux, release binary | `BZZ_BIN=<scratch>/bzz-after BZZ_TMUX_SCENARIOS=t17-demo-first-screen,t17-demo-first-minute,t17-demo-huddle,t17-demo2-first-minute python3 scripts/test-tui-tmux.py`, three times | `4 of 4 scenarios passed` each time: 2.6/9.6/2.0/25.9 s, 2.5/9.5/2.7/25.6 s, 2.1/9.8/3.3/26.0 s. `t17-demo-first-minute` takes the welcome's new last line in its reaction step, and `t17-demo-huddle` the new notice |
| The size of the release binary | measured | `cargo build --locked --release --bin bzz` at `64156d3` (from `git archive`) and at this branch | 31 995 232 → 32 043 872 bytes: **+48 640** |

##### What is left of the plan's size budget

The plan's limit is +350 KB over v0.16.0 for the whole demo. Agent `demo`
measured +277 888 bytes for its part (before the viz merge); this task adds
+48 640. Together 326 528 bytes: **23 472 bytes left** of 350 000 (31 872
of 350 KiB).

#### Captures, read as a newcomer

In `<scratch>/caps/`: `first-80x24.txt` (the welcome with the Board),
`launch-80x24.txt`, `board-added.txt`, `board-moved.txt`,
`board-teammate.txt`, `launch-teammate.txt` (Lin's line beside the Board),
`scout-add.txt`, `scout-add-thread.txt`, `scout-move.txt`,
`launch-after-scout.txt`, `huddle-later-80x24.txt`, `notice-*.txt`,
`typing-then-move.txt`; at 160×45 `first-160x45.txt`, `launch-160x45.txt`,
`apps-160x45.txt`, `space5-160x45.txt`. Every step of the scenarios in
`<scratch>/scen1/` to `scen3/`.

Read as a newcomer: the welcome's fourth line is the Board, and in
`#launch` the Board stands beside Ada's message that names its keys, at
80×24 and 160×45. A card added is selected in To do; `]` puts it in
Doing at once. Lin's move comes with a line in the channel saying so.

#### Not verified

- **The gates**: `cargo fmt --check`, `cargo build --locked --example
  scripted_harness`, `cargo test --locked --all-targets` (1 157 passed, 0
  failed) and `cargo test --locked --no-default-features --lib` passed on
  `acf5070`; `cargo clippy --locked --all-targets -- -D warnings` found one
  complex type there, fixed in the last commit, and passed on it, with
  `fmt --check`, `demo::` and `demo_offline_test` again. The whole test
  suite was not run again for that change (a type alias). `cargo deny` and
  `cargo audit` were not run: `Cargo.toml` and `Cargo.lock` are unchanged.
- **macOS and Windows**: not built or run here.
- **The second and third teammate moves on screen**: the one-minute gap is
  proven by the unit test with a stepped clock; on screen only the first
  move was watched.
- **Scout on a slow machine**: the Board is written when Scout's answer is
  signed (2.5 s after the mention); not measured under load beyond this
  machine's.
- **The Board's `q`** closes the Board in `#launch`, as it does anywhere
  (`:app open board` opens it again). A newcomer who presses `q` in the
  Board to leave it loses it from the channel until then; this is the
  template's key, unchanged here.

### v0.17.0 — task `install`: A6, A7, F6

Linux x86_64, 2026-11-24/25, on branch `t3/install-onboarding` cut from
`v0.17.0` at `543dbd1`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md).
`<scratch>` is a folder outside the repository; its files are local. Every
container is a stock image from Docker Hub (`debian:stable-slim`,
`ubuntu:22.04`, `ubuntu:24.04`, `ubuntu:20.04`, `almalinux:9-minimal`,
`fedora:latest`, `alpine:3`) as pulled on that day. No bzz binary ran on
this machine; every bzz that ran is the published v0.16.0, inside a
container.

#### A6 — glibc 2.34 in the generated installer

**Claim.** With `[dist.min-glibc-version] "*" = "2.34"` in
`dist-workspace.toml`, the shell installer cargo-dist generates accepts
glibc 2.34, so AlmaLinux 9 installs; the release workflow is unchanged.

**Rung.** The generated installer itself, run in a clean AlmaLinux 9
container against the published v0.16.0 archives (the version in
`Cargo.toml` is still 0.16.0, so the generated installer downloads them).

```sh
dist generate --check                        # cargo-dist 0.30.2: prints nothing, exit 0
dist build --artifacts=global --output-format=json
grep -o 'check_glibc "2" "3[0-9]"' target/distrib/bzz-installer.sh | sort -u
docker run --rm -v <scratch>/distrib-2.34:/g:ro almalinux:9-minimal sh -c '
  microdnf install -y tar xz
  sh <the published v0.16.0 bzz-installer.sh>     # for contrast
  sh /g/bzz-installer.sh; ~/.cargo/bin/bzz --version
  microdnf install -y alsa-lib; ~/.cargo/bin/bzz --version'
```

**Seen.** `dist generate --check`: no output, exit 0 (the workflow does not
change). The generated installer has `check_glibc "2" "34"` for both Linux
targets; the published v0.16.0 one has `"2" "35"`. In AlmaLinux 9
(`glibc 2.34`):

```
=== the published v0.16.0 installer (min glibc 2.35)
System glibc version (`2.34') is too old; checking alternatives
ERROR: no compatible downloads were found for your platform x86_64-unknown-linux-gnu
[exit 1]
=== the installer generated with [dist.min-glibc-version] 2.34
downloading bzz 0.16.0 x86_64-unknown-linux-gnu
no checksums to verify
installing to /root/.cargo/bin
everything's installed!
[exit 0]
/root/.cargo/bin/bzz: error while loading shared libraries: libasound.so.2 ...
--- after microdnf install alsa-lib
bzz 0.16.0
```

"no checksums to verify": a global-only build has no local archives to
hash; the release workflow's installer carries them. v0.16.0 needs ALSA to
start (E1 removes that); the point here is that the installer no longer
refuses glibc 2.34.

#### A6 — `getbzz.dev/install.sh`

**Claim.** `site/public/install.sh` gives each machine the answer it should
in one line: glibc too old and musl are refused with what to do; a missing
`tar` or `xz` (which the archive needs) is refused with the one command
that installs it; the sound library is reported in the wording that the
variable `BZZ_SOUND` picks. It is POSIX `sh` and `shellcheck` clean.

**Rung.** The script in the seven clean images with `BZZ_CHECK_ONLY=1`,
twice: as the image comes (no network) and with `tar` and `xz` added (what a
desktop has); each in both wordings (`BZZ_SOUND=server`, the default, and
`alsa`, made with `sed` on a copy).

```sh
docker run --rm koalaman/shellcheck:stable --shell=sh --severity=style install.sh   # 0.11.0
docker run --rm --network none -v <dir>:/i:ro -e BZZ_CHECK_ONLY=1 <image> sh /i/install-<mode>.sh
dash -n, bash --posix -n, busybox sh -n                                            # parse
```

**Seen.** `shellcheck`: nothing, exit 0. All three shells parse it. The
answers (full output in `<scratch>/check-images.txt`):

| Image | As it comes | With tar and xz, `server` | With tar and xz, `alsa` |
|---|---|---|---|
| debian:stable-slim | `no xz … sudo apt install xz-utils` (1) | note: no PipeWire or PulseAudio found (0) | `… sudo apt install libasound2t64` (0) |
| ubuntu:22.04 | `no xz … sudo apt install xz-utils` (1) | note (0) | `… sudo apt install libasound2` (0) |
| ubuntu:24.04 | `no xz … sudo apt install xz-utils` (1) | note (0) | `… sudo apt install libasound2t64` (0) |
| ubuntu:20.04 | `this system has glibc 2.31 and bzz needs 2.34 or newer: use Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9 or newer, or a container of one.` (1) | same (1) | same (1) |
| almalinux:9-minimal | `no tar and xz … sudo dnf install tar xz` (1) | note (0) | `… sudo dnf install alsa-lib` (0) |
| fedora:latest | note (0) | note (0) | `… sudo dnf install alsa-lib` (0) |
| alpine:3 | `there is no download for musl systems such as Alpine yet: run bzz on a glibc system (glibc 2.34 or newer) or in a container such as debian:stable-slim.` (1) | same (1) | same (1) |

In the `alsa` wording, an image without `xz` gets one command for both:
`sudo apt install xz-utils libasound2t64, then run this installer again
(libasound2t64 is the sound library bzz needs to start)`.

The `server` note is skipped when a sound server is there (fedora, with
`BZZ_CHECK_ONLY=1`; `<scratch>/sound-server-detection.txt`): a listening
socket at `$XDG_RUNTIME_DIR/pulse/native`, `PULSE_SERVER` set (as WSLg
does), or `pipewire` on `PATH` each give only `checks done`. With a fake
`uname` that answers `Darwin`, the script goes straight to the checks'
end (exit 0): macOS takes the release's installer as before.

**A full install** of the published v0.16.0 through the script, in
`ubuntu:24.04` with `curl` and `xz-utils`, as a new user, the file piped to
`sh` as `curl … | sh` does (`<scratch>/full-install-ubuntu24-final.txt`):

```
downloading bzz 0.16.0 x86_64-unknown-linux-gnu
installing to /home/newcomer/.cargo/bin
everything's installed!
...
Note: huddle audio needs PipeWire or PulseAudio, and neither was found here; everything else in bzz works without them.

Try it now, offline, with nothing to set up:   bzz demo
Then join your team's community:               bzz
(bzz is in /home/newcomer/.cargo/bin, not on this shell's PATH yet: open a new terminal, or run /home/newcomer/.cargo/bin/bzz demo)
[installer exit 0]
bzz: error while loading shared libraries: libasound.so.2 ...   [exit 127]
--- after apt install libasound2t64
bzz 0.16.0
```

The `server` wording is for v0.17.0 and is wrong for v0.16.0, which still
needs ALSA: this run shows exactly that. Until v0.17.0 is published (and if
E1 does not ship), the site must serve `BZZ_SOUND=alsa`
(`site/README.md`, "The installer"). Also seen in the same container:
`BZZ_VERSION=0.16.0 sh -s -- --no-modify-path` installs that release and
passes the argument on (no `~/.cargo/env` written); `BZZ_VERSION=9.9.9`
stops with `bzz: could not download …/v9.9.9/bzz-installer.sh`, exit 1.

**Not verified.** The script on a real macOS or Windows shell (Git Bash);
on arm64 Linux; the full chain through the script on AlmaLinux 9 (the
latest release's installer still asks for 2.35 until v0.17.0 is
published); Cloudflare Pages serving `install.sh` with `_headers`
(nothing was deployed).

#### A7 — README and site

**Claim.** The README's hero shows the v0.16 film and its image, a "Try it
in 60 seconds" block, a requirements line and a compatibility line, and the
"not affiliated with Block" sentence near the top; the site has the same
block and requirements and a current social card description.

**Rung.** Links resolved from the file; the page drawn by headless Chrome.

```sh
python3 <check every relative link and #anchor in README.md, and that each
         getbzz.dev/assets/… link has its file in site/public/assets/>
curl -sI https://getbzz.dev/assets/promo-v016.mp4         # HTTP/2 200, video/mp4
google-chrome --headless=new --window-size=1280,900 --screenshot … http://127.0.0.1:<port>/
```

**Seen.** 20 relative links, none missing; every `#anchor` matches a
heading; `promo-v016.mp4` exists in `site/public/assets/` and is served.
The compatibility line names what `docs/protocol-compatibility.md` says:
relay `block/buzz` `bf3bdd0e…`, pairing proven with Buzz desktop 1.0.1.
Screenshots at 1280 and 390 wide (`<scratch>/shots/site-{1280,390}-top.png`,
`site-{1280,390}-install.png`): the hero line "then `bzz demo`: …" under the
install command, and the install section "Try it in 60 seconds." with the
requirements, the two-line block and the installer's note; nothing clipped
at 390. `styles.css` gained one rule (`.hero p.then`), so its `?v=` went
from 7 to 8.

**Not verified.** The block pasted into a clean Debian reaching the demo's
first screen: `bzz demo` is built by another task and is not in this
branch.

#### F6 — CI on `main`

**Claim.** The two jobs that kept `main` red are fixed in the workflow.

**Rung.** The logs of the runs on `main` (read with `gh run list` and
`gh run view`; nothing started or cancelled), and `actionlint` on the
changed workflow. The proof proper is CI itself: it is not run from here.

**Seen in the logs.**

- **`agent-host (macos-15)`, cancelled.** Runs 45462401762, 45427321605
  and 45391983168 (2026-11-24) failed only through cancelled `macos-15`
  jobs: `agent-host`, and in two of them `upgrade (macos-15, 0150)`,
  `audio (macos-15, false)` and `build (macos-15, …)` too. Each has zero
  steps and ends 15 minutes after it was queued with the annotation `The
  job was not acquired by Runner of type hosted even after multiple
  attempts`, and three times `Due to capacity constraints, jobs targeting
  macOS arm64 runners may experience longer queue times`. The label exists;
  hosted Apple Silicon capacity is what is missing. `macos-15-intel` jobs
  of the same runs (`build (macos-15-intel, …)`) started and ran.
  **Change:** `agent-host` (Node only) and `upgrade` (a profile opened by
  this build) run on `macos-15-intel`. That halves the Apple Silicon jobs
  of a run (left: `build` for `aarch64-apple-darwin`, and `audio`).
- **`audio (windows-latest, true)`, failed.** It fails in every run since
  at least 2026-11-15: the loopback test panics with `the virtual devices
  open: Unsupported("microphone unavailable: …")`; hosted Windows has no
  audio endpoint, as the workflow's comment says. The job already had
  `continue-on-error: ${{ matrix.experimental }}`, and that works for the
  run: run 45322414306 (2026-11-24) concluded `success` with this job
  failed. But the job and the pull request's check still show a red ✗.
  **Change:** the same `continue-on-error` on the test step, so an
  experimental row passes its job and records the failure in the step.
  The step's command is untouched (agent `sound` owns it).

`actionlint` (rhysd/actionlint, latest): no findings, before and after.
`main` has no branch protection and no rulesets (`gh api`), so no required
check carries the old job names.

**Not verified.** Two consecutive green runs on `main`: only CI can show
them, after the principal pushes. `build (macos-15, aarch64-apple-darwin)`
and `audio (macos-15, false)` still ask for Apple Silicon and can still be
cancelled the same way; neither is moved here (the first must build and
run on that processor; the second belongs to agent `sound`).

### v0.17.0 — join: joining a community without leaving bzz (A9)

Linux x86_64, 2026-11-25, branch `t3/implement-join-command` cut from
`v0.17.0` at `69fdec2`. Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
item A9, rules 19, 20 and 22, and the privacy and key rules of v0.13 to
v0.15 (4 and 10). Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository. Every identity below was generated for the run; no keychain,
Buzz desktop or account of this machine was used.

#### What was built

| Item | Where |
|---|---|
| The flow's logic, shared by the CLI and the form: the address and invite links read, NIP-11 probed and told apart, the AUTH answer told apart, an invite claimed, a community written with its relay key pinned | `src/join.rs` |
| Pairing with Buzz desktop, lifted from the CLI | `src/auth/pairing.rs` |
| `IdentityManager::save_new` (write an identity, take it back on failure), `forget` (take back one this join made), `keychain::available` (asked by reading an entry that does not exist) | `src/auth/mod.rs`, `src/auth/keychain.rs` |
| The CLI's `community add`, `identity new`, `import`, `import-backup`, `pair-desktop` call them | `src/main.rs` |
| The form: its state, keys and screen; secrets in `Zeroizing` buffers drawn as dots | `src/ui/join.rs` |
| The App's half: the seam `open_join(JoinStart)`, background steps, the wait and its pace, the landing, the demo seam | `src/app/join.rs` |
| `:join [address]`, `:community add [address]` | `src/ui/command.rs`, `src/app.rs` (`execute_command`) |
| The switch to a runtime, shared by switching communities and joining | `src/app.rs` (`adopt_runtime`) |
| Three tmux scenarios, and the runner's `empty` and `isolated` | `scripts/tui-herdr-scenarios.toml` (end), `scripts/test-tui-tmux.py`, `scripts/test-tui-herdr.sh` |

#### Claims

```sh
cargo test --locked --lib join            # 27 tests: src/join.rs, src/ui/join.rs, src/app/join.rs
cargo test --locked --test join_test      # 5 tests on a scripted relay
BZZ_BIN=<release bzz> BZZ_TMUX_SCENARIOS=t17-join-not-there ./scripts/test-tui-tmux.py
BZZ_TMUX_SCENARIOS=t17-join-open ./scripts/test-relay.sh --tmux
BZZ_FIXTURE_AGENTS=0 BUZZ_REQUIRE_RELAY_MEMBERSHIP=true RELAY_OWNER_PUBKEY=<64 hex> \
  BZZ_TMUX_SCENARIOS=t17-join-members ./scripts/test-relay.sh --tmux
```

| Claim | Lowest rung | Proof | Result |
|---|---|---|---|
| From an empty home with no keychain, a person joins the pinned relay's open community, lands, opens a channel and sends a message, with nothing typed in a shell but `bzz` | the release binary on the pinned relay, through tmux | `t17-join-open`: `:join localhost:3030` → `✓ Buzz Relay · a Buzz community at localhost:3030` / `open: anyone who signs in may join` → Enter → `A new identity, made here · no keychain: key in a file with a passphrase` → the passphrase twice, drawn as dots → `choose a channel to open` (the finder on the open channels) → `t9-gen` Enter → `i`, the message, Enter → the message in `#t9-general`, and no longer `[pending]` (acknowledged by the relay) | passed, 20 steps, 3.3 s |
| Against a relay that lets in members only, the npub is shown; the screen is left and found again; the person is let in from the harness side; bzz gets in by itself without a restart | the same, relay with `BUZZ_REQUIRE_RELAY_MEMBERSHIP=true` (NIP-43 advertised) | `t17-join-members`: `it lets in its members only` → `Your id, to send to an admin` and a full `npub1…` (58 characters checked) → `waiting for an admin · asked 1 time` → Esc: `the join goes on; :join shows it again` → `:join`: the same wait → a row in `relay_members` for the pubkey bzz wrote in the profile's settings (`docker compose … psql`, as the fixture seeds members) → with no key pressed, `choose a channel to open` 14.7 s later (the second ask, 15 s after the first) → a message sent and acknowledged | passed, 11 steps, 18.2 s |
| The relay's AUTH answer is told apart: in, not a member, refused (a ban), sign-in first, no answer | scripted relay | `join_test::the_relays_answer_to_auth_decides_what_comes_next`; `join::tests::a_refused_auth_is_told_apart` | pass |
| No answer at the address; a relay with no community there (404); something that is not a Buzz relay (a plain relay with no signing key, a web page): each says what happened and what to try | scripted relay, real closed port | `join_test::each_failure_at_the_address_is_told_apart`; in the form `app::join::tests::an_address_where_nothing_answers_says_so_in_the_form`; through tmux `t17-join-not-there` (`✗ nothing answers at localhost:9` / `check the address with the person who gave it to you…`; `ws://far.example` → `ws:// and http:// work only on this machine; use wss://`) | pass |
| An identity refused: said in the form, and the identity this join made is taken back (`k` keeps it) | App on a scripted relay that answers `blocked: you are banned` | `app::join::tests::a_refused_identity_is_said_and_nothing_half_made_stays`: the form says `My team refused this identity (blocked: you are banned)`; after Enter the settings hold no community and no identity | pass |
| Nothing half-made: no community entry is written before the relay lets the identity in; stopping a wait asks whether to keep the identity whose id was given out, and `n` removes it and its key file | App, scripted relay | `a_members_only_community_shows_the_npub_and_lets_in_without_a_restart` (no community while waiting; one identity); `stopping_a_wait_keeps_or_drops_the_identity_as_the_person_says` (both answers; the key files counted) | pass |
| The wait asks again on its own at a calm pace (15 s, 30 s, then every minute) and on `r`, and asks nothing before its time | App, scripted relay | `the_wait_asks_again_on_its_own`; `retry_after`; `the_members_only…` (`r`); on the pinned relay, the 14.7 s above | pass |
| An invite link of the pinned Buzz (`https://<relay>/invite/<code>`, `buzz://join?relay=…&code=…`) is read, claimed with NIP-98 by the joining key with the code in the body only, and lets the identity in; its refusals are Buzz desktop's words | scripted relay | `join_test::an_invite_is_claimed_by_the_joining_key_with_the_code_in_the_body_only`; `app::join::tests::an_invite_link_is_claimed_and_lets_in_a_members_only_community`; `join::tests::invite_links_of_the_pinned_buzz_are_read` | pass. Not run against the pinned relay's `/api/invites/claim` (an invite needs an admin's mint) |
| The CLI and the form write a community the same way: probe, then `add_community` (configuration, store row, relay key pinned); a relay already configured is refused and nothing changes | integration | `join_test::a_community_is_written_with_its_relay_key_pinned_and_taken_back_on_failure`; the CLI's own tests in the gates | pass |
| `:community add` opens the form (it answered "use the CLI"); `:join` and `:join <address>`, `:community add <address>` too; a paste goes into the field as one piece of text | App | `the_community_add_command_opens_the_form_and_a_bad_address_is_said`; `ui::command` tests | pass |
| After joining, bzz opens the first channel the person is in, or the finder on the open ones; nothing is joined for them | App; pinned relay | `after_joining_bzz_lands_in_the_first_channel_or_offers_the_open_ones`; both relay scenarios | pass |

##### Secrets

A passphrase and an nsec are typed only into the form's hidden fields, as
`bzz identity new` and `import` read them only from the controlling
terminal. They are held in `Zeroizing` buffers, wiped when the form moves
on or goes back, passed to `IdentityManager` as a `SecretString` or as
`Keys`, never put in a notice, a `Debug` (the form's `Debug` is written by
hand) or a diagnostic. An invite's code is a bearer of an admin's
permission: drawn as dots, out of `Debug`, sent only in the claim's body to
its relay.

How it was checked:

- On screen: `ui::join::tests::without_a_keychain_a_new_identity_asks_a_passphrase_twice_hidden`
  and `…an_import_takes_a_valid_secret_only_and_never_shows_it` render the
  form at 80×24 and 160×45 and find dots and not the secret, and not in
  `Debug`; `an_invite_code_is_drawn_as_dots`.
- In every file bzz wrote: `app::join::tests` read every file under the
  test's home after a join (settings, key file, database) and find neither
  the passphrase nor the invite's code.
- In the process and the terminal, on the pinned relay: `t17-join-open`
  step 8 reads `/proc/<pid>/cmdline` and `/proc/<pid>/environ` of the
  profile's bzz (found by its `BZZ_CONFIG_DIR`) while the passphrase is
  typed; step 14 greps the profile's whole home (settings, the key file,
  the database, diagnostics, cache) and the bytes bzz wrote to the
  terminal (the pane's pipe). The passphrase is in none of them.
- What is not bzz: the scenario types the generated passphrase with
  `tmux send-keys`, one key per argument, as a person types it, and the
  check's own `grep` has it as its pattern. Both are the harness, with a
  passphrase generated for the run.

##### The demo (rule 22)

Under `bzz demo` the form opens and says that joining needs a real
community's address, that the demo writes nothing to the person's
settings, and to leave with `q` and start `bzz`. It writes nothing. The
demo is agent `demo`'s branch (`t3/demo`), not this one: `open_join` asks
`join_in_demo()`, which returns `false` here and is to return `self.demo`
when the two are joined (`JoinForm::demo` is the screen).

##### Pairing with Buzz desktop

Not run with a desktop: no Buzz desktop may be started on this machine.
The form's choice calls `auth::pairing::find_desktop` and `pair`, the
same two functions `bzz identity pair-desktop` now calls, and the CLI's
own tests go as far as the provider (`tests/credential_provider_test.rs`,
`pairing_needs_no_token_and_returns_one`, a fake provider). Shown by
tests: without a running desktop the choice says `Buzz desktop is not
running here: start it, then come back` and stays; without a keychain it
says pairing needs one (the paired token lives only there).

What a person must check, with Buzz desktop running on the machine and a
keychain: `:join <address>`, choose "Pair with Buzz desktop", approve the
request in Buzz, and see bzz come in with the desktop's npub.

##### Sign-in through the browser

Built, not run on a NIP-FI relay: where NIP-11 says the community signs in
through an identity provider, the form offers it, asks the address of the
sign-in service (https, as `identity_adapter` is configured; ADR v0.13:
bzz never derives it from relay data), makes a new identity, writes the
community with that adapter and runs `:signin`. Tested at the form's rung
only (`the_browser_is_offered_where_the_community_signs_in_through_it`).

#### The first minute, counted

The study (`r4-first-run/REPORT.md` §3) counted five commands after the
installer and ten ideas to know before: identity, label, keychain,
backend, passphrase, identity id, community, relay, `wss://` address,
pubkey/npub, membership.

| | Commands in a shell | Ideas the person needs |
|---|---|---|
| Before | 5 (`identity new`, `--backend`, `identity list`, `community add`, `bzz`) | 10 |
| Now, an open community, with a keychain | 1 (`bzz`) | 1: the community's address |
| Now, no keychain | 1 | 2: and a passphrase |
| Now, members only | 1 | 3: and "your id, to send to an admin" |

Keys after `bzz`: `:join` and the address (or `j` on the first screen once
it is joined with agent `demo`'s), Enter, Enter for a new identity, the
passphrase twice when there is no keychain, Enter on a channel.

#### Captures

Text captures of every step from tmux, at 80×24 and 160×45, in
`<scratch>/captures-open/t17-join-open/`,
`<scratch>/captures-members/t17-join-members/` and
`<scratch>/captures-notthere/t17-join-not-there/` (one file per step; its
first line says the keys and the size).

#### Gates

On `525f5a1` and after, with the machine shared (load 13 to 16):

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets --no-fail-fast` | 4 failures in one run under that load: `apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time`, `apps::runtime::tests::hostile::every_hostile_script_is_stopped_in_budget` (time budgets), `agent_runtime_test::at_startup_leftovers_are_stopped_before_recovery_and_recovery_before_any_start`, and this task's `an_invite_link_is_claimed…`, whose wait of 30 s for a join step was too short for an Argon2 key file in a debug build under that load (now 120 s). Each passed alone; then `cargo test --locked --lib` (841 passed), `--test agent_runtime_test` (54) and `--test join_test` (5) passed whole. In an earlier run `app::tests::a_command_waits_for_the_owner_on_every_screen_and_runs_only_on_his_yes` failed once and passed three times alone. None of these touch the join's code but the one named |
| `cargo test --locked --no-default-features --lib` | pass |
| `cargo deny check`, `cargo audit` | not run: `Cargo.toml` and `Cargo.lock` unchanged |

## B — The huddle visualizer

### v0.17.0 — viz: the huddle shows who is speaking (B1 to B4)

Linux x86_64, 2026-11-24, branch `t3/visualization-task` cut from
`v0.17.0` at `543dbd1`. Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
track B, D7, D8, D9, rules 20 and 21. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository.

No sound device of this machine was opened, played through or recorded
from. Every level in a test or a golden comes from generated audio: pure
tones (`huddle::synth::tone`) or the generated voices of
`huddle::synth::Synthetic` (D8).

#### What was built

| Item | Where |
|---|---|
| Levels, lock-free and allocation-free on the audio path | `src/huddle/levels.rs` |
| The engine publishes each person's level after decoding and before the mix, the microphone's before the mute check; it reads `joined`, `left`, `roster` | `src/huddle/audio.rs` (`PeerMixer`, `apply_control`, `start`); device opening untouched |
| Gate, smoothing, 12 s history ring; the pacer that asks for frames | `src/huddle/meter.rs` |
| Generated voices and tones, for the demo and the tests | `src/huddle/synth.rs` |
| Lanes, the status strip, colour depth and glyph fallbacks | `src/ui/huddle_lanes.rs` |
| The lanes in the panel | `src/ui/huddle.rs` (`render_with_lanes`) |
| Wiring: frame tick, panel, status line, generated feed | `src/app.rs` (`HuddleViz`, `tick_huddle_viz`, `render_status`, the `Overlay::Huddle` arm) |
| `[ui] motion = full \| calm \| off`, `BZZ_NO_MOTION` | `src/config.rs` (`Motion`, `UiConfig::motion`) |
| D8's sentence | `docs/adr-v0.13-huddle-audio.md` §6 |

#### Claims

```sh
cargo test --locked --lib huddle
cargo test --locked --lib app::tests::the_huddle_visualizer_asks_for_no_frame_without_a_call
cargo test --locked --lib app::tests::a_generated_huddle_shows_its_strip_and_its_lanes
cargo test --locked --test config_test motion
```

| Claim | Lowest rung | Proof | Red without the change | Result |
|---|---|---|---|---|
| B1. A person's level is taken after decoding and before the mix, and the gate opens and closes at the expected instants | unit, real decode path | `huddle::audio::tests::a_tone_through_the_decode_path_opens_and_closes_the_gate`: 1 kHz at −20 dBFS through `AudioEncoder`, relay framing, NetEQ and Opus, `PeerMixer::tick`, the meter at 30 fps | with `publish_peer` removed from `PeerMixer::tick`: fails (the gate never opens) | opens 0.167 s after the tone starts (the jitter buffer's delay), closes 0.400 s after it stops playing (0.3 s hang, a frame, a chunk) |
| B1. The gate at 30 and 60 fps alike; a −55 dBFS hiss never opens it | unit | `huddle::meter::tests::the_gate_opens_and_closes_at_the_expected_instants` | — | opens within one frame, closes 0.3 to 0.4 s after the tone, at both rates |
| B1. History scrolls by time, not by frame | unit | `history_scrolls_by_time_not_by_frame` at 20, 30, 60 fps | — | pass |
| B1. Two at once light both lanes | unit | `two_voices_at_once_light_both_lanes`, and the lane goldens at 5.5 s of the passage | — | pass |
| B1. Writer and reader: no lock, no loss | unit, threads | `huddle::levels::tests::writer_and_reader_stress`: 2,000 rounds, a writer of six chunks around one loud one, a second writer racing every slot; the reader swaps | — | the loudest chunk of every round is read back, to 0.02 dB |
| B1. The `Debug` of every type that holds a level prints no number | unit | `levels::debug_prints_no_number`, `meter::debug_of_every_type_that_holds_a_level_prints_no_number` (Meter, Track, Pacer), `huddle_lanes::debug_prints_no_number` (Lanes, Person), `synth` (Synthetic); `HuddleViz` in `app.rs` prints `HuddleViz { .. }` | — | pass |
| B1. The relay's `joined`, `left`, `roster` keep every name; a seat given to someone new gets a fresh decoder; malformed messages change nothing | unit | `huddle::audio::tests::the_relays_control_messages_keep_every_name` | — (the engine ignored these messages: `Some(Ok(_)) => {}`) | pass |
| B1. On the pinned relay, a person who joins after you has a name and their level reaches your meter while their tone plays, not before; your muted microphone still reaches your own lane; a person who leaves is forgotten | relay | `tests/relay_integration.rs::real_relay_huddle_names_a_late_joiner_and_shows_their_level`, `BZZ_RELAY_TEST_FILTER=real_relay_huddle ./scripts/test-relay.sh` | see "Relay run" | see "Relay run" |
| B2. The strip in an 80×1 status line: who speaks, "you're muted" in amber | golden | `huddle_lanes::tests::strip_goldens` (`tests/fixtures/huddle_lanes/strip-*`): 4 moments × bzz, blocks only, `NO_COLOR` | — | pass; at 7.1 s "you're muted" carries the theme's Warning colour |
| B2. The strip only while a call is on; one frame with no huddle | unit, App | `app::tests::the_huddle_visualizer_asks_for_no_frame_without_a_call`: 300 frame ticks, the redraw gate admits only the first frame; `redraw_gate::tests::idle_ticks_do_not_admit_extra_frames` unchanged | — | pass |
| B2. The strip shortens to fit: moving cells go before the name | App screen | `app::tests::a_generated_huddle_shows_its_strip_and_its_lanes` at 80×24 and 160×45 | — | 80×24: `🎧 Ada and Lin · mic muted`; 160×45: twelve braille cells, then the same |
| B3. Lanes at 42×12 and 80×24 in the default theme, a light theme, 256 colours, blocks only and `NO_COLOR` | golden | `huddle_lanes::tests::lanes_goldens`: 3 moments (Ada and Lin at once, you talking muted, Sam) × 2 sizes × 5 looks, text and style of every cell (`tests/fixtures/huddle_lanes/`) | — | 30 goldens pass |
| B3. Both lanes move and the header says so | unit | `two_at_once_both_lanes_move_and_the_header_says_so` | — | "4 in the call · Ada and Lin at once", both right edges lit, both names bold |
| B3. Your microphone moves while muted, says nobody hears you, turns amber and says "muted!" when you speak | unit | `talking_muted_says_so_in_words_and_in_amber`, `muted_and_quiet_says_nobody_hears_you` | — | pass, with and without colour |
| B3. Rule 21: under `NO_COLOR` no cell of the lanes carries a colour; in 256 colours none is true colour; blocks only uses Block Elements and words | unit | `no_color_draws_no_colour`, `colours_are_256_or_none_when_asked`, `blocks_only_uses_block_elements_and_words` | — | pass |
| B3. More people than rows: whoever spoke last, and you, stay | unit | `many_people_keep_the_last_speakers_and_you` | — | "+4 more" |
| B3. The panel shows the lanes above its text, at 80×24 and 42×12; without a call it is as before | unit | `ui::huddle::tests::the_panel_shows_the_lanes_above_its_text` | — | pass |
| B4. Frames: none hidden, one in silence, ≤30 a second while moving, none 2 s after silence, a still meter with motion off | unit, counted | `meter::tests::frames_only_while_seen_and_moving`: the redraw gate and the pacer over 10 s at bzz's 33 ms frame tick, speech from 1 to 4 s | with the `visible` check removed from `Pacer::wants_frame`: fails | hidden 0; silence 1; full 176 (about 5.8 s of frames); calm 28; off 3 (start, speaking, quiet); hidden at 2 s 31 |
| B4. Silence after a call's passage stops frames (App) | unit, App | `a_generated_huddle_shows_its_strip_and_its_lanes`: 8 s after the passage | — | at most one frame |
| B4. Motion off is still: same people speaking, same picture | unit | `motion_off_is_a_still_meter` | — | pass |
| B4. `motion` parses strictly; `BZZ_NO_MOTION` turns it off | integration | `tests/config_test.rs::motion_parses_strictly_and_bzz_no_motion_turns_it_off` | — | pass |

#### Relay run

```sh
ln -s <scratch>/target target        # this worktree's target directory
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin BZZ_RELAY_TEST_FILTER=real_relay_huddle \
  BZZ_RELAY_NOCAPTURE=1 ./scripts/test-relay.sh     # CARGO_TARGET_DIR unset
```

Both huddle tests passed (`2 passed; 0 failed`), the existing
`real_relay_huddle_lifecycle_and_audio_round_trip` with them. Alice joined
first and was alone; her muted microphone's level reached her own lane.
Bob joined after her: Alice's call named him by his public key at his relay
index, the relay's `joined` message being the only way it could. Bob sent
1 s of silence then 1.5 s of 1 kHz at −20 dBFS, unmuted; Alice's meter,
read every 33 ms, had him speaking from 1.06 s to 2.91 s (tone 1.00 to
2.50 s; the rest is the jitter buffer, the hang and the relay). When he
left, Alice's call dropped him. The script tore the harness down (its log
shows the containers, volumes and network removed); the `buzz-harness`
containers running a minute later were another run's, started after the
lock was released.

Red: the engine before this change ignored every text message
(`Some(Ok(_)) => {}`), so `others()` stayed at the roster of the join and
the test's "names Bob" step cannot pass; this was not run as a separate
relay run.

#### Captures

From `app::tests::a_generated_huddle_shows_its_strip_and_its_lanes` with
`BZZ_VIZ_CAPTURES=<scratch>/captures`: `strip-80x24.txt`,
`strip-160x45.txt` (the status line while the panel is closed),
`panel-80x24.txt`, `panel-160x45.txt` (`:huddle` open). They are the real
`App::render` with a generated huddle, drawn into a test backend. Read as a
newcomer: at 80×24 the status line says `🎧 Ada and Lin · mic muted`; at
160×45 it has the moving cells too. In the panel, the header says who
speaks and the scale ("10s ago … now"), each person has a ringed badge
while speaking and "● live", and the last line says "mic muted · nobody
hears you · m unmutes".

#### Not proven

- **A call through the virtual-audio path.** The brief asks for two clients
  in a huddle through the virtual-audio test path. The relay test above
  runs the real engine on the pinned relay with generated capture frames,
  not through a sound server: sound servers here may only run inside
  containers, and `scripts/test-audio-loopback.sh` drives the session's
  own server. The CI's loopback job is unchanged.
- **The real TUI in a call through tmux.** Joining needs a sound device.
  The captures are the App's own render with a generated feed; no `t17-viz`
  scenario was added, for the same reason.
- **Terminals.** Block Elements and braille were read in a test backend
  only; no terminal emulator, no `TERM=linux` console.
- **NetEQ's `vad_active`** is not used: whether it is true for ordinary
  decoded speech was not measured, and the gate works without it.

### v0.17.0 — viz2: the zoomed huddle, its styles, the microphone check (B5 to B7)

Linux x86_64, 2026-11-25, branch `t3/70f252c4` cut from `v0.17.0` at
`bfd340f` (the merge of `viz`, B1 to B4). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
track B, D7, D8, D9, D11, rules 19 to 21. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository.

No sound device of this machine was opened, played through or recorded
from. Every level, sample and spectrum in a test, a golden, a capture or
the film comes from generated audio: pure tones (`huddle::synth::tone`) or
the generated voices of `huddle::synth::Synthetic` (D8). The microphone
check is proven with a fake device (generated frames written where the
device's frames go).

#### What was built

| Item | Where |
|---|---|
| The mix handed over to the interface: `MixTap`, a fixed ring of 4,096 atomics inside `Levels`, written only while a style asks for it | `src/huddle/spectrum.rs`, `src/huddle/levels.rs` (`publish_mix`, `mix`), one line in `PeerMixer::tick` (`src/huddle/audio.rs`); device opening untouched |
| The band analysis: Hann window, a 1024-point radix-2 FFT written here (D11), 48 log bands 90 Hz to 7 kHz, fixed range, rise, gravity, thrown peak marks | `src/huddle/spectrum.rs` (`Spectrum`) |
| The orb's rate that follows the terminal; a pacer interval of the caller's | `src/huddle/meter.rs` (`Throttle`, `Pacer::wants_frame_every`) |
| The microphone check | `src/huddle/mic_check.rs`, `src/ui/huddle_mic.rs`, `App::start_mic_check` / `tick_mic_check` / `stop_mic_check` |
| The zoomed view: lanes (tall), bars, orb; names in every style; the picker | `src/ui/huddle_zoom.rs` |
| `[ui] huddle_style = "lanes" \| "bars" \| "orb"` | `src/config.rs` (`HuddleStyle`, `UiConfig::huddle_style`) |
| Keys and commands; a zoomed frame draws only the view and the status line; the draw is timed for the orb | `src/app.rs` (`huddle_key`, `huddle_view_command`, `render_app`, the `Overlay::Huddle` arm, the frame tick) |
| Generated voices with vowel formants up to 6 kHz, at the energy they had | `src/huddle/synth.rs` (`Timbre`) |
| A generated huddle without a call, for development and the film | `BZZ_DEV_GENERATED_HUDDLE=1` (read in `App::run`) |

##### Keys and commands (B5)

| Where | Key or command | Does |
|---|---|---|
| Huddle panel, a call on | `z` | zooms the visualizer to the window; `z`, `Esc` or `q` come back to the panel |
| Huddle panel or zoomed view | `v` | opens the style picker (zooming first) |
| Picker | `↑`/`k`, `↓`/`j`/`Tab` | selects a style and draws it live |
| Picker | `Enter` | keeps it and saves `[ui] huddle_style` |
| Picker | `Esc`, `q`, `v` | closes it; the saved style is drawn again |
| `:` line | `:huddle zoom` | zooms (or comes back); needs a call, not a channel |
| `:` line | `:huddle style [lanes\|bars\|orb]` | with a name: saves it and zooms; alone: opens the picker (with no call it says which style is saved) |

`z` was chosen because it is free in the panel (`s j l e m`, Space,
`Esc`, `q` are taken), says "zoom" as plan L10's conversation-only `z`
will, and does not touch the app pane's `Space 5`, which is a leader
sequence (in the panel, Space is push-to-talk). `v` ("visual") is free
too. Every v0.16 key of the panel does what it did; `m`, Space and `l`
work in the zoomed view as in the panel.

#### Claims

```sh
cargo test --locked --lib huddle
cargo test --locked --lib -- the_zoomed_view zoom_without_a_call the_join_confirmation zoomed_view_captures
cargo test --locked --release --lib -- measure_the_zoomed_styles --ignored --nocapture
```

| Claim | Lowest rung | Proof | Red without the change | Result |
|---|---|---|---|---|
| B6. 300 Hz, 1 kHz and 3 kHz land in the right bars | unit | `huddle::spectrum::tests::known_tones_light_the_right_bar`: each tone at −20 dBFS, the bars as drawn at 21 bars (42 columns) and 40 (120 columns); the tallest bar is the one the frequency falls in on the log scale; bars four away are under 70 % of it. `known_tones_land_in_their_bands`: the loudest band is the tone's (±1 band), above 0.6, far bands low, silence empty | first run: with whole-bin bands (the prototype's) every band below 1.6 kHz was one FFT bin, so the scale was linear there and 1 kHz missed its band; then, with neighbouring bars sharing a band at their edge, 300 Hz tied two bars | pass |
| B6. Bars rise within a few frames of a tone, fall back to rest, and leave a peak mark above the falling bar, at 30 and 60 frames a second alike | unit | `bars_rise_fall_and_leave_a_peak_mark` | first run: at 60 fps a frame with no new chunk read as silence and the bars flickered; a window now counts as silence only after 60 ms with nothing new | pass |
| B6. No allocation and no lock on the audio path | unit, threads; read | `MixTap::push` is relaxed stores into a `Box<[AtomicU32]>` allocated when the call's `Levels` is made, and one release store of the index; it returns at once unless a style asked (`want`). Checked by reading `push` and its one caller (`PeerMixer::tick`, after the existing clamp); `writer_and_reader_race_safely` races 20,000 pushes against 2,000 reads (every value read is one written or zero); `the_tap_writes_nothing_unless_wanted` | — | pass. The engine's own `Vec` for each 10 ms mix predates this and is unchanged |
| B6. The mix is asked of the audio side, and analysed, only while bars or the orb are on screen | unit, App | `app::tests::the_zoomed_view_counts_its_frames`: zoomed bars want the mix; after `z` (zoom hidden) the tap is no longer wanted | with `mix.want(spectral)` replaced by `want(true)`: fails | pass |
| B5. `z` and `:huddle zoom` zoom to the whole window and back; the picker previews each style live; Esc puts the saved one back; Enter keeps and saves it; `:huddle style <name>`; a wrong name changes nothing | unit, App | `app::tests::the_zoomed_view_and_its_picker_preview_keep_and_restore` (the real `App::render` at 120×30, the saved `config.toml` read back) | with Esc saving the previewed style: fails | pass |
| B5. With no call, zoom says what it needs | unit, App | `zoom_without_a_call_says_so` | — | "the zoomed view shows a call: join the huddle first" |
| B5, B6. Goldens of each style at 42×12 and 120×30 in true colour, 256 colours, block characters only, `NO_COLOR` and the light theme; the picker over the bars | golden | `ui::huddle_zoom::tests::zoom_goldens` (`tests/fixtures/huddle_zoom/`, 31 files: text and the style of every cell) at 5.5 s of the generated passage, Ada and Lin at once | — | pass |
| B5. Names of who speaks stay on screen in every style, in words | unit | `every_style_names_who_speaks`: "Ada and Lin at once" in each style, size and look; `● Ada`, `● Lin`, `· Sam`, `You · muted` under bars and the orb; "You · you're muted" when you talk muted | — | pass |
| Rule 21 in every style: `NO_COLOR` has no colour in any cell; 256 colours no true colour; blocks only uses Block Elements and words | unit | `no_color_and_256_colours_and_blocks_hold_in_every_style` | — | pass |
| B6. Bars are the call's spectrum: lit while people speak, a flat baseline in silence | unit | `the_bars_rise_with_speech_and_rest_in_silence` | — | pass |
| B6. The orb takes the colour of who speaks, two colours for two, grey in silence, and is never more than 18 rows | unit | `the_orb_is_in_the_speakers_colour_and_capped` | — | pass |
| B6. Motion off is a still picture that changes with the speaker | unit | `motion_off_holds_still_until_the_speaker_changes` (lanes and orb identical while Ada speaks; every style changes when Lin takes over); with motion off the spectrum is read only when the speaker changes | — | pass |
| Rule 20. Frames counted: none in silence, none with the view hidden, a still picture with motion off | unit, App, counted | `the_zoomed_view_counts_its_frames` over the 12 s passage at bzz's 33 ms tick: bars zoomed, while people speak 250 to 370 (about 30 a second); from 13 s to 20 s none; a call that never speaks, zoomed: none after the first frame; motion off, each style: 5 to 14 (one a change of speaker). The view hidden: the existing `meter::tests::frames_only_while_seen_and_moving` (pacer `visible = false`: 0) still holds, and the zoom no longer asks for the mix | — | pass |
| B6. The orb drops its rate under a slow writer | unit, real backend | `the_orb_drops_its_rate_on_a_slow_writer`: the orb at 120×30 through `CrosstermBackend` to a writer whose flush sleeps 30 ms: the interval goes 0.05 → 0.1 s after three frames, 0.2 s after six, and holds; the writer quick again, it climbs back. App: `the_zoomed_view_counts_its_frames`: three 40 ms draws take the orb from 20 to 10 frames a second, and the frame tick then admits 8 to 11 a second | first run under the full suite: the whole draw was timed and a debug build on a busy machine made the quick writer look slow; the test now times the writer | pass |
| B7. Your lane is live in the join confirmation before anything is sent; three seconds of nothing names the device; the check ends with the confirmation | unit, App, fake device | `huddle::mic_check::tests::*` (a voice says "your microphone works"; digital silence, no frames at all, or a −75 dBFS hiss for 3 s say "no sound from “USB Mic” · pick another with :huddle input <name>", and not at 2.9 s); `ui::huddle_mic::tests::your_lane_moves_before_you_join_and_silence_names_the_device`; `app::tests::the_join_confirmation_checks_the_microphone`: more than 20 frames while the fake device speaks, the confirmation shows "● heard", "your microphone works", "y join", and no call exists; closed another way, the check ends | — | pass |

##### Cost of each style, in bzz

`app::tests::measure_the_zoomed_styles`: the real `App` with the generated
huddle looping, the frame tick every 33 ms, a frame drawn through
`CrosstermBackend` to a counting writer whenever the visualizer asks; 12 s
after a 1 s warm-up; true colour, the `bzz` theme. "CPU" is the time of the
tick (generated voices and analysis included) and the draw, over 12 s, on
one core of this machine (Ryzen 7 3700X, shared with other builds).

Release build (`cargo test --release`):

| style | size | frames/s | KiB/s | ms a frame | CPU % of a core |
|---|---|---|---|---|---|
| lanes | 120×30 | 30.2 | 20.5 | 0.16 | 1.42 |
| bars | 120×30 | 30.2 | 35.0 | 0.15 | 1.44 |
| orb | 120×30 | 15.2 | 50.6 | 0.48 | 1.82 |
| lanes | 50×12 | 30.2 | 5.0 | 0.03 | 0.86 |
| bars | 50×12 | 30.2 | 10.9 | 0.05 | 1.41 |
| orb | 50×12 | 15.2 | 11.1 | 0.10 | 1.19 |
| lanes (panel, not zoomed) | 120×30 | 30.2 | 9.9 | 0.24 | 1.69 |

- bzz does not draw below 50×12 ("bzz needs at least 50×12"), so the
  smallest size measured in bzz is 50×12. At 42×12 the styles are proven
  by the goldens, drawn by the same function.
- The orb was measured at 397 KiB/s at 120×30 and 30 frames in the study.
  Here it is capped at 18 rows, at 20 frames a second (15 with the 33 ms
  tick), its colours stepped by 8, and the liquid shaded only near its
  surface, so fewer cells change from one frame to the next.
- The generated voices are most of the CPU figure (a frame costs 0.03 to
  0.5 ms; 30 of them a second are 0.1 to 1.4 % of the 0.9 to 1.8 %); a
  real call has none, and decodes audio instead.

##### B8: frames that redraw only the visualizer

Not built as asked. What was done and measured instead:

- A zoomed frame lays out nothing under the view: `render_app` draws the
  zoomed view and the status line and returns. Measured above: a frame of
  the zoomed lanes at 120×30 against the same lanes in the panel over the
  workspace (the "panel" row): 0.16 ms against 0.24 ms a frame in
  release (1.42 against 3.14 ms in a debug build).
- The panel's lanes (and the status strip) still run `App::render` in
  full. Keeping a copy of the last full buffer and drawing only the lanes'
  region into `current_buffer_mut()` before `flush()` and
  `swap_buffers()` needs the frame loop to know which frames were asked
  for by the visualizer alone. The loop and the redraw gate are being
  changed by the motion work (plan L7, L9); doing it here would collide
  with that. Left for after L9, with the numbers above as the "before".

#### Gates

At `d3db394`, all green: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets` (lib: 826 passed, 3
ignored), `cargo test --locked --no-default-features --lib`. No dependency
and no migration was added, so `cargo deny` and `cargo audit` were not run.

#### Captures

- `<scratch>/captures/`: from the tests with `BZZ_VIZ_CAPTURES` set: the
  zoomed view in each style at 80×24 and 160×45 (`zoom-{lanes,bars,orb}-*.txt`)
  and the join confirmation with the check at 80×24 (`mic-check-80x24.txt`),
  all the real `App::render`.
- `<scratch>/tmux-captures/`: the real binary (debug), a scratch home
  (`bzz paths` checked), `BZZ_DEV_GENERATED_HUDDLE=1`, no community, in tmux
  at 80×24 and 160×45: `:huddle zoom` (lanes), `v` `j` (bars previewed),
  `j` `Enter` (orb kept, "huddle style: orb" in the status line), `Esc`
  (back to the panel).
- `<scratch>/viz2-styles.mp4` (35.6 s, 1920×1080, 30 fps, H.264, no
  sound): the real bzz recorded as a tape with
  `.agents/skills/bzz-promo-video/scripts/record-tape.py` (120×34 cells),
  replayed by HyperFrames 0.9.13 (`HYPERFRAMES_NO_TELEMETRY=1
  DO_NOT_TRACK=1`): the lanes, the picker, bars and the orb previewed
  live, Esc restoring the lanes, Enter keeping the orb, then the bars.
  Stills in `<scratch>/stills/`: `lanes`, `picker`, `bars-preview`,
  `bars-two-speakers`, `orb-preview`, `orb-two-speakers` (cyan and magenta
  in one sphere), `orb-quiet` (grey), `esc-restored`, `bars-kept`.

Read as a newcomer: the header says who is in the call and who speaks; the
style's name and what it shows are at the top right; the keys are at the
bottom right; under bars and the orb the names carry `●` when speaking.
The picker's caption says what each style shows. With no channel (the
development huddle) the header reads "huddle · 4 in the call".

#### Not proven

- **A real call.** No relay run: the zoomed view, the picker and the
  spectrum were driven by generated audio only. The mix tap sits after the
  existing mix in `PeerMixer::tick`, which `viz`'s relay test exercises,
  but no relay test reads the spectrum.
- **The microphone check on a real device.** The device task
  (`open_devices`, then the capture frames into the check's levels) is
  compiled but was not run: opening this machine's microphone is not
  allowed. It opens the speaker too, because `open_devices` opens both;
  nothing is played. The join then waits up to 2 s for the check to let
  the devices go before it opens them again.
- **The default microphone's name.** With no `huddle_input_device` set,
  the check says "the default microphone": the device layer does not say
  which device it chose. SEAM for agent `sound`: a name for the opened
  input would go into `MicCheck::new`.
- **Terminals.** xterm.js (the film) and tmux only; no kitty, Ghostty,
  WezTerm, Terminal.app or Windows Terminal; no real slow SSH link (the
  slow writer is a test double).
- **B8**, as above.
- No `t17-viz2-*` scenario: the views need a call or the development
  huddle, which no scenario starts.

## C — The palette, and finding things

### v0.17.0 validation: the palette (C1)

Task `palette`, branch `t3/palette`, cut from `v0.17.0` at `543dbd1`. Linux
x86_64, 2026-11-24, on a machine shared with six other builds and relay runs
(load 15 to 24). Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
item C1 and D4. `<scratch>` is a folder outside the repository.

#### What was built

- `src/ui/action_table.rs`: one static table, 159 rows. Every interface
  action of the R2 inventory (`actions.tsv`, 114 rows: checked by script,
  `artifact.edit` and `section.manage` are split into one row per verb),
  113 listed in the palette, the composer's 16 editing keys (in the table,
  not listed: the palette does not open over the composer), and 30 shell
  commands worth knowing (two of them, `community add` and `remove`, are
  the `:` commands bzz parses and refuses today). Each row: id, title, extra words, exact aliases,
  group, what it runs (`UiAction`, a `:` command run at once, a `:` command
  opened filled in, the quit question, or a shell command), the `:` form
  shown dim, what it needs, and where it runs (workspace, Inbox, anywhere).
  The key beside a row is read from the live key map.
- `src/ui/palette.rs`: ranking (`nucleo-matcher`), availability, the empty
  state, keys, drawing. `src/app/palette.rs`: opening it with the facts of
  the moment, and running the choice through the paths a key or a typed `:`
  command take.
- Keys (D4): `Ctrl+K` and `Space k` in the global scope, `UiAction::OpenPalette`
  (`open-palette` in `keymap.toml`). Text-owning scopes (composer, filter) do
  not inherit global bindings, so `Ctrl+K` in the composer stays "delete to
  line end", as in v0.16.0.
- `benches/palette.rs`.

#### Proofs

| Claim | Rung | Command | What was seen |
|---|---|---|---|
| A `UiAction` or a `Command` that no row mentions fails a test; every `:` text in the table parses to a known command; every verb of `:huddle`, `:app`, `:artifact`, `:section` has a row | unit | `cargo test --locked --lib action_table::` | 5 passed. Red below |
| A variant added to `UiAction` or `Command` and not named in the test does not compile | unit | the two `macro_rules!` in `action_table::tests` write an exhaustive `match` | read; not run red |
| "thr" puts "Open thread" first; "q" puts "Quit bzz" first and Enter asks to quit; "theme" → "Change theme…"; "status" → "Set my status…"; "delete" → "Delete my message"; from three letters a match scattered over a row's extra words is dropped | unit | `cargo test --locked --lib palette::` | 23 passed (19 in `ui::palette`, 4 in `app::palette`) |
| Enter on "Delete my message" over another person's message runs nothing, keeps the palette open, and shows the reason `actions::derive` gives (`only your own message can be deleted`); a signer's refusal is the reason when the signer refuses | unit + App | `palette::tests::delete_on_another_persons_message_says_why_and_stays`, `the_signers_refusal_is_the_reason_when_it_refuses`, `app::palette::tests::delete_over_another_persons_message_runs_nothing_and_stays_open` | pass; no confirmation opened |
| `>` narrows to actions, `#` to channels, `@` to DMs; channels and actions are in one list; Enter on a channel row opens it | unit + App | `prefixes_narrow_the_kinds`, `channels_and_actions_share_one_list`, `a_channel_row_is_chosen_with_the_actions` | pass |
| An action with an argument opens the `:` line filled in; a shell-only row asks to copy its command | unit | `an_argument_row_runs_as_a_prefill`, `a_shell_row_asks_to_copy_its_command` | pass |
| The empty state offers what the selection allows, the session's recent actions, then unread channels, and a line with the counts | unit | `the_empty_state_offers_the_selection_recent_and_unread` | pass |
| Channels that arrive while the palette is open are listed, the query kept (seen failing on the pinned relay: opened while bzz still synced, `#t9-general` found nothing) | unit | `channels_that_arrive_while_it_is_open_are_listed`; the App refreshes before a key and before a frame | pass |
| Esc closes and the focus is where it was | App | `esc_closes_and_puts_the_focus_back` (opened from the channels pane) | pass |
| ↑ ↓ Ctrl-n Ctrl-p PageUp PageDown move; one step wraps | unit | `keys_move_and_wrap_and_esc_closes` | pass |
| The key column comes from the live key map (`Ctrl+K`, `G`, `Space p`; "Open thread" shows `Enter`, "Quit bzz" `q`) | unit | `the_shortcut_comes_from_the_live_key_map` | pass |
| Inbox rows wait for the Inbox, workspace rows for the workspace, with a reason | unit | `inbox_rows_wait_for_the_inbox_and_workspace_rows_for_the_workspace` | pass |
| The panel fits 50×12 to 200×50; 100 columns and 26 rows at most | buffer | `the_panel_fits_every_size`, `it_reads_at_80x24_and_200x50`, `the_palette_reads_at_80x24_and_160x45` | pass |
| The filter's p95 over 3,500 rows is at or under 4 ms in release | bench | `cargo bench --locked --bench palette`, at `f3aa969` and later (with the noise cut) | p95 **0.70 ms** over 7 queries × 50 runs (median 0.28 ms, max 1.9 ms), load average 15 to 20 from other builds; an earlier run before the cut: p95 0.41 ms. Building the 3,500 candidates at open: 2.8 ms median, 5.9 ms p95. A 200×50 frame with the palette open: 0.30 ms median, 0.34 ms p95 (`TestBackend`, not a terminal) |
| On the release binary and the pinned relay: Ctrl-K and Space k open it, Esc closes it; "thr" → "Open thread" first; "q" → "Quit bzz", Enter → "quit bzz?" | tmux | `BZZ_TMUX_SCENARIOS=t17-palette-open-thr-quit ./scripts/test-relay.sh --tmux` | passed, 7 steps |
| Same: over another person's message, Enter on "Delete my message" leaves the row on screen with "can't now: only your own message can be deleted" under it; Esc closes | tmux | `t17-palette-unavailable-stays` | passed, 7 steps |
| Same: "theme" opens the theme picker; "status" opens the `:` line (`COMMAND`); `#t14-names` opens that channel | tmux | `t17-palette-theme-status-channel` | passed, 8 steps |
| Same at 80×24, 160×45 and 200×50: empty, "thr", "delete" (the reason in the footer at 80 columns, beside the row at 160), "community add" (the shell command and "Enter copies it") | tmux | `t17-palette-sizes` | passed, 14 steps |

The four `t17-palette-*` scenarios passed together in one run at `f61be2a`
(`<scratch>/relay6.log`, 36 steps): `BZZ_BUZZ_SOURCE=<buzz-pin>
BZZ_TMUX_SCENARIOS=t17-palette-open-thr-quit,t17-palette-unavailable-stays,t17-palette-theme-status-channel,t17-palette-sizes
./scripts/test-relay.sh --tmux`, `./target` linked to this task's target
folder and `CARGO_TARGET_DIR` unset for the command; the link removed after.
The harness was torn down by the script (its log ends with the volumes and
network removed). Two earlier runs of `t17-palette-sizes` failed and led to
changes: at 80×24 "Generated custom emoji" wraps (the step now waits for the
pane title), and the palette opened before the channels had synced found
no `#t9-general` (fixed in `f61be2a`, above). The same race seems to be
behind a run where the channel finder's Enter opened nothing at 80×24; the
finder is not this task's, and it was not looked into.

##### Red runs

One build with four temporary edits, then `git checkout` of both files:
the row `msg.delete` removed, the row `tasks.panel` removed, the alias `q`
of "Quit bzz" removed, and the noise cut in `palette::rank` removed.
`cargo test --locked --lib -- action_table palette::`: 7 failed, 20 passed.

| Test | Failure |
|---|---|
| `every_ui_action_is_in_the_table` | `Delete is in no row of src/ui/action_table.rs` |
| `every_command_is_in_the_table` | `Command::Tasks is in no row of src/ui/action_table.rs` |
| `delete_on_another_persons_message_says_why_and_stays` | first row `left: "Delete sidebar section"` |
| `the_signers_refusal_is_the_reason_when_it_refuses` | no reason: `left: None` |
| `app::…::delete_over_another_persons_message_runs_nothing_and_stays_open` | Enter ran a row: `left: Some(Action(49))` |
| `it_reads_at_80x24_and_200x50` | the reason not on screen |
| `a_match_scattered_over_words_is_not_listed` | "Widen side pane" listed for "thr" |

`q_puts_quit_first_and_enter_runs_it` stayed green without the alias: a
single `q` already ranks "Quit bzz" first on its title's first letter. The
alias is kept so that it stays first when another row's title starts with
Q; that case was not made red.

##### Captures

Text captures of every scenario step are in `<scratch>/captures6/t17-palette-*/NN.txt`:
80×24 in `t17-palette-sizes/03.txt` (empty), `04.txt` ("thr"), `05.txt`
("delete"); 160×45 in `06.txt` to `08.txt`; 200×50 in `09.txt` to `11.txt`;
160×45 also in the other three scenarios. Beside the prototype's images
(`runs/experiments/bzz/v017-tui-2026-11-24/r2-palette/shots/`), the layout is
the same: a centred panel, the search line, rows with a kind glyph, the `:`
form dim, the key or the reason at the right, a footer with the keys and the
count. Differences: no group headers while typing (the prototype had none
either), and the panel is as tall as its rows, at least 12.
`NO_COLOR` itself is item L1 (agent `look`); the palette does not rely on
colour: the selected row carries `▌`, a row that cannot run carries `✕` and
its reason (or `✕` alone under 90 columns, the reason then in the footer), a
row's kind is a glyph (`·` action, `#` channel, `@` DM, `$` shell), and the
captures above are plain text without colour.

#### Not verified

- `NO_COLOR` on the screen: bzz does not read it before L1 lands.
- The copy of a shell command reaching a real clipboard: OSC 52 is written
  (as `y` writes it); tmux on a private socket does not show the clipboard.
- VS Code's terminal and Zellij, where `Ctrl+K` may not reach bzz: `Space k`
  and `open-palette` in `keymap.toml` are the ways round, read from R2, not
  run.
- Windows and macOS.

#### Gates

At `f61be2a`, `<scratch>/gates2.log`:

| Gate | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --locked --all-targets -- -D warnings` | ok |
| `cargo build --locked --example scripted_harness` | ok |
| `cargo test --locked --all-targets` | ok; the library 804 passed, 2 ignored |
| `cargo test --locked --no-default-features --lib` | ok; 514 passed |
| `cargo deny check` (Cargo.toml gained a `[[bench]]`, no dependency) | ok |
| `cargo audit` | ok; the five allowed warnings of v0.16.0 |

### v0.17.0 validation — task `find` (C3 to C7, A5, rule 19)

Linux x86_64, 2026-11-24/25. Branch `t3/implement-find-task`, cut from
`v0.17.0` at `543dbd1`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md).
Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository on the machine the work was
done on.

`U:` a unit test (`cargo test --locked --lib`, or `--bin bzz`, or one
`--test`). `T:` a tmux scenario of `scripts/tui-herdr-scenarios.toml` on the
release binary through `scripts/test-tui-tmux.py`: `automated` ones on an
empty isolated profile, `harness` ones on the pinned relay
(`scripts/test-relay.sh --tmux`). `C:` a clean container.

#### Rule 19: nothing that worked stops working

`tests/v016_pins_test.rs` holds, as data, every effective binding of every
scope (Global, Workspace, Inbox, Composer, Filter, Overlay: 161 rows) and
what `command::parse` made of 53 inputs covering every v0.16.0 `:` command
and alias. Both were printed from the code of `543dbd1` by a throwaway test
run on a clean copy of the three files (the printout is
`<scratch>/pins-v0160-clean.txt`; a first printout taken while my edits
were being compiled was identical).

| Claim | Proof | Red without | Result |
|---|---|---|---|
| Every v0.16.0 key does what it did, in every scope | `U: v016_pins_test::every_v016_key_does_what_it_did` | `y` rebound to `Refresh` (temporary): `Global y no longer does CopyMessages` | pass |
| Nothing is bound that is neither v0.16.0's nor named with its decision; no addition replaces a v0.16.0 key | `U: every_binding_is_v016s_or_named` (`ADDED_KEYS`: Workspace `d` → `Delete`, `u` → `MarkUnread`, plan D5) | the same change: `("Global", "y", "Refresh") is new: name it in ADDED_KEYS with its decision` | pass |
| Every v0.16.0 command parses as it did; `:q`, `:quit`, `:help`, `:theme [name]` are new and were not v0.16.0 commands | `U: every_v016_command_parses_as_it_did` | — | pass |
| The three deliberate changes are named in the test: `d` (D5), `u` (D5), a bare `:status` (D6) | `DELIBERATE_CHANGES` in the test | — | listed |

Two changes that are not keys or commands, so the pin does not hold them;
both are named here and in the report:

- `:theme <word>` other than `reload` was "unknown command" in v0.16.0 and
  now sets that theme or names the themes (C4). The unit test of the parser
  that asserted the old answer was changed and says why.
- The leader panel no longer drops the sequence after 750 ms once it is
  shown (C6, below).

#### C3 — the help scrolls and lists the `:` commands

| Claim | Proof | Result |
|---|---|---|
| The help lists every `:` command of `command::COMMANDS` with its arguments, after the keys; wrapped to 78 columns no row is wider | `U: ui::help::tests::help_lists_commands_after_the_keys_and_scrolls_to_the_end` | pass |
| At 80×24 the frame's title says which rows show; `j` goes row by row to the last row (`:purge-cache … local cache`) and stops; PageDown reaches it a page at a time; `g` and `G` go to the top and the bottom | `U: app::find_tests::the_help_scrolls_to_its_last_row_at_80x24` (renders the App on a 80×24 test backend after every key) | pass |
| The same on the release binary at 80×24: `?` shows `rows 1-22 of 114`, `j` `rows 2-23`, PageDown `rows 23-44`, `G` the last command, `g` the top | `T: t17-find-help-80x24` | pass |
| The table and the parser agree: every name and alias the help lists is a command `parse` knows | `U: ui::command::tests::the_table_and_the_parser_agree` | pass |
| The v0.16 help scenarios still pass (`?` and Esc on the empty profile; the Inbox help names `mark unread` with a custom keymap) | `T: startup-help-quit`, `custom-keymap-inbox` | pass |

Seen: `<scratch>/captures/t17-find-help-80x24/01.txt` (top) and `04.txt`
(the end, rows 93-114 of 114); at 160×45 `<scratch>/captures-160/…` (rows
1-43 of 110).

#### C4 — the `:` line answers a mistake

| Claim | Proof | Result |
|---|---|---|
| `:hudle` answers `unknown command :hudle · did you mean :huddle? · :help lists them all`; `:stauts`, `:serach`, `:hud` name the right one; a word near nothing points to `:help`; a right first word with wrong arguments gets its usage; no answer starts with v0.16's bare `unknown command:` | `U: ui::command::tests::a_mistyped_command_is_answered_with_the_nearest` | pass |
| The prompt stays open with the text as typed and the answer under it; typing clears the answer; Esc leaves | `U: app::find_tests::a_mistyped_command_names_the_nearest_under_its_prompt` | pass |
| A usage longer than the window (`:agents nowhere`, 86 characters) is wrapped whole under the prompt at 80×24; `:community add` names the shell command, whole | `U: app::find_tests::a_long_usage_wraps_and_is_not_cut` | pass |
| `:q` and `:quit` ask as `q` does; `:theme` opens the picker; `:theme nord` sets and saves it; `:theme plaid` names the themes and changes nothing | `U: app::find_tests::quit_and_theme_commands`, `ui::command::tests::quit_help_and_theme_are_commands` | pass |
| On the release binary at 80×24: `:hudle`, `:agents nowhere` (wrapped on two rows), `:community add`, `:help`, `:theme`, `:q` then `y` | `T: t17-find-commands` | pass |
| The v0.13 commands on an empty profile answer as before | `T: v013-panels-empty-profile` | pass |

Seen: `<scratch>/captures/t17-find-commands/03.txt` (the usage wrapped
under `:agents nowhere`).

#### C5 — the channel finder

| Claim | Proof | Result |
|---|---|---|
| ↓, Tab and Ctrl-n move the selection; ↑, Shift-Tab and Ctrl-p move it back, wrapping; a new letter starts at the top; the selected row has `›` | `U: app::find_tests::the_finder_selection_moves`, `ui::finder::tests::the_selection_moves_wraps_and_restarts_with_the_query` | pass |
| A DM is found by the person's name and drawn `@Scout`; the last row starts a DM with the query as the search | `U: ui::finder::tests::dms_are_found_by_name_and_a_new_dm_comes_last`; the same App test | pass |
| On the pinned relay: `Space Space`, `gen` selects `+general`, ↓ selects `#t9-general`, Enter opens it (`message #t9-general`) | `T: t17-find-finder` | pass |
| The finder as v0.16 scenarios use it (`t9-gen` Enter) still opens the first row | `T: t14-edit-own-message`, `t9-system-group-read-only`, `t14-sections-and-solo` | pass |

#### C6 — the leader panel

| Claim | Proof | Result |
|---|---|---|
| Every continuation of `Space` is in the panel at 80×23, 120×31 and 40×12 (two columns when one does not fit); every row is inside the frame | `U: ui::help::tests::leader_panel_shows_every_key_at_80x24` | pass |
| The panel appears 300 ms after the key, on Tokio's clock: not at 150 ms; at 300 ms it opens, and five ticks ask for one frame between them; five seconds later it is still open and the sequence still waits | `U: app::find_tests::the_leader_panel_waits_300_ms_and_asks_for_one_frame` | pass |
| A sequence whose panel cannot show (another overlay holds the screen) ends after 750 ms, as in v0.16 | `U: app::find_tests::a_sequence_behind_another_overlay_still_expires` | pass |
| On the release binary, at 80×24 and at 120×32, the panel lists all eleven keys, `s cycle channel sort` included, appears 364 to 438 ms after `Space` (300 ms and the scenario's 50 ms poll), is still there 1.5 s later, and Esc closes it | `T: t17-find-leader` | pass |

Seen: `<scratch>/captures/t17-find-leader/01.txt` (80×24) and `04.txt`
(120×32).

#### C7 — `d`, `u`, `:status` (D5, D6)

| Claim | Proof | Result |
|---|---|---|
| `d` and `u` are bound in the workspace only; the Inbox keeps `U` and does not get `u` | `U: app::find_tests::d_on_another_persons_message_says_why_not`; the pin test | pass |
| `d` on another person's message says `delete own message: only your own message can be deleted`, the menu's words, from the same function; with nothing selected `select a message first`; outside the conversation it says to select a message there | the same App test (`context_refusal`) | pass |
| A bare `:status` shows the status with when it ends, or that there is none; `:status clear` still clears | `U: app::find_tests::bare_status_shows`, `bare_status_does_not_clear` | pass |
| On the pinned relay: `d` on the fixture's first message (another person's) says why not; `G`, `d`, `y` delete my own, and its text leaves the screen; `u` says `marked #t9-general unread`; `:status t17 lunch`, then `:status` shows `your status: t17 lunch · :status clear clears it`; `:status clear`, then `:status` shows `you have no status` | `T: t17-find-delete-unread-status` | pass |

#### A5 — the first commands in the shell

| Claim | Proof | Result |
|---|---|---|
| Without a keychain `bzz identity new` says so and names `--backend encrypted-file`; another backend's error is unchanged; a label with a space is quoted in the line it prints | `U: tests::a_missing_keychain_names_the_encrypted_file` (`cargo test --bin bzz`) | pass |
| `bzz community add --help` describes the command and each argument | `U: tests::community_add_help_describes_itself` | pass |
| In a clean container (`bzz-r4-firstrun`: Debian stable slim with `libasound2`, `--network none`, a scratch home, no D-Bus), the release binary: `identity new --label personal` prints `no keychain to keep the key in: … Keep the key in a file protected by a passphrase instead: bzz identity new --label personal --backend encrypted-file` (v0.16.0 printed `identity is locked: the OS credential service is unavailable; unlock it and restart bzz`, the research's `capture/get-started.txt`); with `--backend encrypted-file` it prints `identity:`, `npub: npub1…` and `pubkey:`; `community add --help` describes itself | `C: <scratch>/a5/get-started.sh`, output `<scratch>/a5/get-started.txt` | pass |
| `:community add` inside bzz names the exact shell command | C4 above | pass |

#### Pinned relay

`scripts/test-relay.sh --tmux` with `BZZ_BUZZ_SOURCE` the main checkout's
`runs/buzz-pin` (Buzz `bf3bdd0`), `./target` linked to this worktree's
target folder and `CARGO_TARGET_DIR` unset for the run (the link removed
after), `BZZ_TMUX_SCENARIOS` naming the scenarios. Three runs, each after
waiting for the harness lock:

| Run | Scenarios | Result |
|---|---|---|
| 1 | `t9-system-group-read-only`, `t14-edit-own-message`, `t14-sections-and-solo`, `t17-find-finder`, `t17-find-delete-unread-status` | the three v0.16 scenarios passed. Both of mine failed on mistakes of the scenarios, not of bzz: a regular expression that did not allow the popup's borders between two rows, and an `Esc` after sending (sending had closed the composer, so `Esc` asked to quit, as it should). Both corrected |
| 2 | my two | not run: the relay was rebuilt and did not come up within the script's 30 s (`Relay did not come up on :3030`; load average 25). Torn down by the script |
| 3 | my two | **both passed** (3.7 s and 8.6 s). `relay run exit 0`; the script removed its containers, network and volumes |

Captures: `<scratch>/relay-captures/t17-find-finder/02.txt` (the second row
selected), `<scratch>/relay-captures/t17-find-delete-unread-status/03.txt`
and `08.txt` (the refusal and the status in the status line).

#### Gates

On the final commit of the branch, with the build environment of the rules:

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | pass (lib 792, every integration test) |
| `cargo test --locked --no-default-features --lib` | pass (502) |
| `cargo deny check`, `cargo audit` | not run: `Cargo.toml` and `Cargo.lock` unchanged |

### v0.17.0 validation: arguments as steps, and more to find (C9, C12)

Task `palette2`, branch `t3/palette2`, cut from `v0.17.0` at `59e3505`
(the merge of the palette, C1). Linux x86_64, 2026-11-25, on a machine
shared with other builds and relay runs (load 17 to 22). Buzz pin
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
items C9 and C12, rules 19 and 23. `<scratch>` is a folder outside the
repository.

#### What was built

- **Steps declared in the table** (`src/ui/action_table.rs`). A row that
  needs something more says what, in order: `Run::Steps(Then, &[Arg])`.
  An `Arg` asks for a person, a channel, an emoji, a duration, free text, a
  choice among a few words, a theme or an installed app; it has a label for
  the breadcrumb, an optional form written before the answer (`--expires `),
  whether it is written before the answers asked earlier, and, when it can
  be left out, what leaving it out means ("no expiry"). `command_text`
  builds the `:` text from the answers. 30 rows have steps: status (text,
  then expiry), report (what, reason, note), start a DM and add to a DM
  (person), a person's profile (person, a new row), react to a message and
  react in a huddle (emoji), sort (alpha, recent, default), presence (on,
  off), huddle (start, join, leave, mute, end, devices; a new row), section
  new, rename and move, Solo, artifact rename (text) and move (channel), the
  huddle's microphone and speaker (text), and every `:app` verb that takes a
  name (app), a folder or a reason. `:dm` bare keeps its row ("Start a group
  DM", the picker).
- **The palette asks** (`src/ui/palette.rs`). Enter on a row with steps opens
  its first step inside the palette: the title is a breadcrumb ("Start a DM ›
  person", "Set my status › In a meeting › expires"), the list holds the
  step's answers, and the footer shows the `:` line being built
  (`:status --expires 2h In a meeting`), the selected or typed answer
  included. Backspace on an empty field goes back a step, with what was
  typed there; from the first step back to the list, with the query. A
  free-text step shows the command's usage under the field and does not go
  on with nothing typed unless the step can be left out. A duration step
  offers 30m to 1w and checks what is typed with the parser `:status` uses
  (`protocol::presence::parse_expiry`), and says why a number with no unit
  or `45x` is not one. An emoji step lists the emoji I reacted with (newest
  first) and then the reaction picker's eight and eight more, with words
  that find them; an emoji typed in full is offered as typed. The theme
  step starts on the theme in use, previews the selected one behind the
  palette as it moves, Enter keeps it, Esc (or Backspace out of the step)
  puts the old one back. A step lists this session's choices first.
- **One executor** (`src/app/palette.rs`). What a row's steps build runs
  through `App::execute_command`, as if typed on the `:` line. Two rows have
  no `:` form to build: "React to message" runs the reaction picker's path
  (`send_reaction` now calls `send_reaction_emoji`, which it shares), and
  the theme row runs the theme picker's own Enter (`confirm_theme_selection`
  on a picker opened at that theme).
- **`:dm <npub|hex>…` and `:dm add <npub|hex>`** (`src/ui/command.rs`,
  `src/app.rs`, `src/app/palette.rs::dm_command`), so the DM step has a
  command to teach and to run. They do what the DM picker's Enter does
  (`start_dm_open`, `start_dm_add`, the same refusal by the signer, at most
  eight people, one at a time to add). The bare `:dm`, `:dm add` and
  `:dm hide` are unchanged.
- **More to find** (C12). Beside actions, channels and DMs the palette lists
  people and agents (`@`, `◆`: Enter opens the profile through
  `:profile <npub>`, Ctrl+D opens the DM through `:dm <npub>`), installed
  apps (`▣ Open board here`: `:app open board`), the other communities
  (`⌂ Switch to …`: the rail's switch), the 60 themes (`◑`: kept as the
  theme picker keeps one), and, last whenever something is typed,
  `/ Search messages for «…»`, which opens the search with the text. An
  agent is one the relay verified (`App::agents`) or one of my local agents.
  Prefixes: `>` actions, `#` channels, `@` people, agents and DMs, and two
  new ones, `$` shell commands and `/` the search row; the footer names
  them. Choices of this session rank first (after an exact alias) and fill
  the empty palette's "Recent" group, whatever their kind.
- `benches/palette.rs`: 2,000 channels, 1,000 DMs, 1,000 people, 500
  actions, the 60 themes and 50 apps (4,611 rows).

#### Proofs

| Claim | Rung | Command | What was seen |
|---|---|---|---|
| Every `:` command that takes an argument a person must give has a row with steps; a command added to bzz does not compile in the test until someone says whether it takes one; no row with a required `<argument>` is left to a bare prefill; what each row's steps build parses to a command of the row's kind; the report's reasons are the relay's | unit | `cargo test --locked --lib action_table::` | 10 passed. Red below |
| "Start a DM" asks for a person, lists people, finds Scout by "sco", shows `:dm npub1…` in the footer, and Enter runs `dm npub1…`, which parses to `Command::DmWith` | unit | `palette::tests::start_a_dm_asks_for_a_person_and_builds_the_dm`, `command::tests::dm_takes_people_and_its_bare_forms_stay` | pass |
| "status": text, then expiry; the usage under the field; nothing typed does not go on; `45x` is refused with the reason; the footer shows `:status In a meeting`, then `:status --expires 2h In a meeting`; Enter runs that text | unit | `status_asks_text_then_expiry_and_the_footer_teaches_the_command` | pass |
| Backspace on an empty field goes back a step with what was typed there, then out of the steps with the query | unit | `backspace_on_an_empty_field_goes_back_a_step` | pass |
| The theme step starts on the theme in use, previews as it moves (the App's theme changes), Esc and Backspace out of it put it back, Enter keeps it and saves it as the picker does | unit + App | `the_theme_step_previews_as_it_moves_and_backspace_puts_back`, `app::palette::tests::the_theme_step_previews_and_esc_puts_the_theme_back`, `the_theme_step_keeps_on_enter` | pass |
| React asks for an emoji, recent ones first, the picker's eight offered, an emoji typed in full offered as typed, "thumbs" finds 👍; react in a huddle builds `huddle react 🎉`; moving an artifact asks for a joined channel and builds `artifact move #t14-names`; sort is a choice; report's note can be left out | unit | `react_asks_for_an_emoji_with_recent_ones_first`, `a_huddle_reaction_and_an_artifact_move_build_their_commands`, `a_choice_and_an_optional_step` | pass |
| People, agents, apps, communities, themes and the search row are found; Ctrl+D on a person builds the DM; without an identity Ctrl+D says why and runs nothing; the last row opens the search with the query | unit + App | `people_apps_communities_themes_and_search_are_found`, `a_person_is_found_by_name_and_opens_their_profile_or_a_dm`, `the_last_row_searches_messages_for_the_query`, `prefixes_narrow_the_kinds` | pass |
| This session's choices rank first, fill "Recent", and come first in a step | unit | `recent_choices_rank_first`, `a_step_lists_this_sessions_choices_first` | pass |
| `:dm notakey` without an identity gives the read-only answer `:dm` gives | App | `the_dm_command_checks_its_keys` | pass |
| A step reads at 80×24 and 160×45 (breadcrumb, header, the selected agent, the `:dm` line) | buffer | `a_step_reads_at_80x24_and_160x45` | pass |
| The filter's p95 stays at or under 4 ms with 2,000 channels, 1,000 people and 500 actions, plus 1,000 DMs, 60 themes and 50 apps | bench | `cargo bench --locked --bench palette` at `c34d8c3` | 4,611 rows: p95 **0.60 ms** over 9 queries × 50 runs (median 0.35 ms, max 0.71 ms), load average about 21. Building the 4,611 candidates at open: 1.4 ms median, 2.1 ms p95. A 200×50 frame with the palette open: 0.28 ms median, 0.37 ms p95 (`TestBackend`) |
| On the release binary and the pinned relay: "Start a DM", "Scout" (the footer shows `:dm npub1…`), Enter opens the DM | tmux | `t17-palette2-dm` | passed, 6 steps |
| Same: "status", the usage under the field, "In a meeting" (`:status In a meeting`), Enter, Backspace back to the text as typed, `2x` refused, `2h` (`:status --expires 2h In a meeting`), Enter: "status updated" | tmux | `t17-palette2-status` | passed, 9 steps |
| Same: "theme" opens on `bzz`, "nord" selects Nord, Esc closes, and the step opens again on `bzz` (the theme was put back); a message selected, "react to", "party", Enter: `🎉 1` under it | tmux | `t17-palette2-theme-react` | passed, 14 steps |
| Same: "Honey" finds the person (`↵ profile · Ctrl+D DM`), Enter opens the profile; "fixture" ends with `/ Search messages for «fixture»`, Up reaches it, Enter opens the search (`local + relay`) | tmux | `t17-palette2-find` | passed, 9 steps |
| Same, after the Board is installed (`t16-cli-app-new`, `-add`, `-approve`): "board" finds `▣ Open board here`, Enter opens it beside `#t9-general` | tmux | `t17-palette2-app` | passed, 7 steps |
| Same at 80×24 and 160×45: a person step with its `:dm` line, a text step with its usage, an expiry step | tmux | `t17-palette2-sizes` | passed, 10 steps |
| C1's four scenarios still pass; `t17-palette-theme-status-channel` now expects the steps where C1 opened the theme picker and the `:` line | tmux | `t17-palette-*` | passed |

The scenarios ran on `./scripts/test-relay.sh --tmux` with
`BZZ_BUZZ_SOURCE=<buzz-pin>`, `./target` linked to this task's target
folder and `CARGO_TARGET_DIR` unset for the command, the link removed after.
The harness was torn down by the script each time (each log ends with the
volumes and network removed).

- `<scratch>/relay1.log`: `t17-palette2-dm`, `-status`, `-app`, the three
  `t16-cli-app-*`, and C1's four passed; `-theme-react`, `-find` and
  `-sizes` failed in the scenarios themselves: an `esc` sent together with
  the next key reads as Alt and that key, so Esc now has a step of its own.
- `<scratch>/relay2.log`: `-find` and `-sizes` passed; `-theme-react`
  reacted to the newest row, a person joining, which takes no reaction; it
  now selects the custom emoji's message three rows up.
- `<scratch>/relay3.log`: `-theme-react` passed.
- `<scratch>/relay4.log`, at `6c3c3ff`: the six `t17-palette2-*`, the three
  `t16-cli-app-*` and C1's four `t17-palette-*` passed together in one run
  (96 steps): `BZZ_TMUX_SCENARIOS=t17-palette2-dm,t17-palette2-status,t17-palette2-theme-react,t17-palette2-find,t16-cli-app-new,t16-cli-app-add,t16-cli-app-approve,t17-palette2-app,t17-palette2-sizes,t17-palette-open-thr-quit,t17-palette-unavailable-stays,t17-palette-theme-status-channel,t17-palette-sizes`.

##### Red runs

One build with three temporary edits, then `git checkout` of both files:
"Set channel group sort" made a bare prefill again (no steps), the bonus of
this session's choices removed from `palette::rank`, and Backspace on an
empty field no longer going back. `cargo test --locked --lib -- palette
action_table command::`: 5 failed, 45 passed.

| Test | Failure |
|---|---|
| `every_command_with_an_argument_has_its_steps` | `:sort takes an argument and no row of src/ui/action_table.rs asks for it in steps` |
| `a_choice_and_an_optional_step` | Enter on the row ran it: `["Set channel group sort", "Search messages for"]` listed instead of `["alpha", "recent", "default"]` |
| `recent_choices_rank_first` | first row `"Open thread"`, not the recent `"t14-names"` |
| `backspace_on_an_empty_field_goes_back_a_step` | stayed at `Set my status › lunch › expires` |
| `the_theme_step_previews_as_it_moves_and_backspace_puts_back` | `Stay` instead of `Restore` |

`no_row_leaves_a_required_argument_to_be_typed` stayed green in that run:
`:sort alpha|recent|default` has no `<`. The exhaustive `match` of
`forms_with_an_argument` (a command added and not named does not compile)
was read, not run red.

##### Captures

Text captures of every step: `<scratch>/captures4/<scenario>/NN.txt`.
80×24: `t17-palette2-sizes/01.txt` (the person step and its `:dm` line),
`02.txt` (the status step and its usage), `03.txt` (the expiry step and
`:status --expires 2h In a meeting`); 160×45: `05.txt` to `07.txt` there,
and every step of the other scenarios. Beside the approved images
(`r2-palette/shots/3-two-step-dm.png`, `4-two-step-theme.png`,
`11-two-step-dm-80x24.png`): the same panel, breadcrumb title, the step's
header ("People", "Themes · moving previews, Enter keeps, Esc puts back"),
rows of `@Name  short key · agent`. Differences: the footer shows the `:`
line being built rather than the keys (the keys of a step are in its list:
"⌫ on an empty field goes back a step"); the selected row carries `▌` and
its kind's glyph (`◆` for an agent) rather than `@`.

#### Decisions taken here

- **The palette does not open with the app pane focused.** `docs/app-api.md`
  promises the script every key but Ctrl-c ("bzz keeps one key: Ctrl-c ...
  Every other key is the app's while the app has the focus"). Taking
  Ctrl+K would break that contract (`api = 1`, D10). Ctrl-c leaves the app,
  then Ctrl+K opens the palette.
- **`:dm <npub|hex>…` and `:dm add <npub|hex>`** were added: without them the
  DM step had nothing to teach, and two executors. Additive: every v0.16
  form of `:dm` does what it did (rule 19), pinned by
  `dm_takes_people_and_its_bare_forms_stay`.
- **React and theme run through their pickers' paths**, not a `:` command:
  bzz has no `:react`, and `:theme <name>` is agent `find`'s (not merged
  here). Seam: `Then::Theme` and `App::keep_palette_theme`; once `:theme
  <name>` lands, the theme row becomes `Then::Command("theme")` and the
  coverage test will ask for it.
- **Motion** (`full | calm | off`) has no `:` command in this branch, so it
  has no step; when one exists the coverage test's exhaustive match asks
  for it.
- **The answer of a person step is the npub**, the form `:profile` documents;
  the footer shows it whole, so it is long at 80 columns.
- A theme row is listed under its name ("Nord", `◑`, detail `theme · nord`),
  not "Theme: Nord": the word "Theme" at the start of 60 titles made "thr"
  list them.
- The order of status's steps is text first, then expiry, and `:status
  --expires 2h text` is still built: `Arg::lead` writes an answer before the
  ones asked earlier.
- `/` and `$` became prefixes (the search row; shell commands).

#### Not verified

- The theme preview on screen (colour): the text captures cannot show it;
  the App tests check the App's theme changes and comes back, and the
  scenario checks the step reopens on the theme in use.
- The DM opened by `:dm` was not opened twice to see the relay give back the
  same conversation; it is the DM picker's call.
- Communities: only one in the harness profile, so "Switch to …" was run in
  a unit test, not on the relay.
- `NO_COLOR`, Windows, macOS.

#### Gates

At `d6b80e9` (the code of `6c3c3ff`: only scenarios and this file changed after),
`<scratch>/gates1.log` and `<scratch>/gates2.log`:

| Gate | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --locked --all-targets -- -D warnings` | ok |
| `cargo build --locked --example scripted_harness` | ok |
| `cargo test --locked --all-targets` | first run: the library 877 passed, 1 failed, `apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time` (a Lua loop of 100,000 steps took over its 50 ms budget, load about 21; the test is not this task's and no code it runs changed); run again with `--no-fail-fast`: every target ok, the library 878 passed, 2 ignored |
| `cargo test --locked --no-default-features --lib` | ok; 586 passed |
| `cargo deny check`, `cargo audit` | not run: `Cargo.toml` and `Cargo.lock` unchanged |

### v0.17.0 validation — task `registry` (C8, C10, C11, rules 19, 20 and 23)

Linux x86_64, 2026-11-25. Branch `t3/440a7815`, cut from `v0.17.0` at
`8a95a33` ("demo2 and look together"). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
items C8, C10 and C11. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository.

`U:` a unit or integration test (`cargo test --locked`). `T:` a tmux
scenario of `scripts/tui-herdr-scenarios.toml` on the release binary through
`scripts/test-tui-tmux.py`: `automated` ones on an empty isolated profile,
`harness` ones on the pinned relay (`scripts/test-relay.sh --tmux`).

#### What was built

- **The registry** is `src/ui/action_table.rs`'s `TABLE`. Each row now also
  carries its **default keys per scope** (`keys`), the **`:` command it
  stands for** with aliases, arguments and summary (`head`, one row per
  first word, 32 of them) and **where it is a hint** in the status line and
  with what rank (`hints`). Executors did not move: a row still points at a
  `UiAction`, a `:` text, steps or a shell command.
- `KeyMap::builtin()` is `KeyMap::generated()`, built from the rows' keys
  (`src/ui/keymap.rs`). The hand-written list is gone. `keymap.toml` is read
  on top as before; its validation is unchanged.
- `command::COMMANDS` is gone: `command::commands()` lists the `head` of the
  rows. The help, "did you mean", the usage answers and Tab on the `:` line
  read it.
- **The help** lists the rows that have a key, grouped as the palette groups
  them (Message, Go, View, Inbox, Channel, …, Writing), each with every key
  the live map gives it; a key no row lists goes under "Other keys"; then
  disabled keys, the actions of the focused item and the commands.
  **The leader panel** names each next key by its row's title, in the same
  group order. **The actions menu** (`Space a`) takes its labels from the
  rows and shows each action's live key first.
- **`docs/keys.md`** is written by `cargo run --example keys_doc >
  docs/keys.md` (`examples/keys_doc.rs`, `ui::help::keys_markdown`):
  every key by group with where it works and its `:` form, every command,
  and what only the shell does. `tests/keys_doc_test.rs` fails when the
  file is stale. The README's key section points to it.
- **C10, the footer** (`ui::status::footer`, `src/app/footer.rs`): where
  the focus is (a message selected, the empty timeline, the thread pane,
  the channels, the Inbox, the composer, the huddle panel, any other
  overlay), up to three rows that are a hint there and can run now (judged
  as the palette judges them), each with its key from the live map, then the
  palette's key. Narrow, it takes fewer keys, down to `Ctrl-K actions · ?
  keys`. The app pane keeps its own `Ctrl-c leaves the app`.
- **C11, Tab on the `:` line** (`ui::command::{offer, Completion}`,
  `src/app/command_line.rs`): Tab completes a command's name, then its
  sub-commands and fixed words, then what its steps take (themes, installed
  apps, channels, the words of `:sort`, `:presence`, `:report`, the huddle
  verbs), all read from the rows. Repeated Tab cycles, Shift-Tab goes back;
  a single candidate is written with a space so the next Tab goes a level
  down. Under the line: the candidates (the one written in brackets) and the
  usage of the command, or for a wrong word the nearest command. Enter runs
  exactly what is typed.

#### C8 — one registry generates the key map

| Claim | Proof | Red without | Result |
|---|---|---|---|
| The map generated from the rows equals the hand-written one, binding for binding in all six scopes, each action's keys in the same order | `U: ui::keymap::tests::the_generated_map_is_the_hand_written_one` at commit `dcdb2e6`, before the hand-written list was removed. It printed `generated == hand-written: 169 effective bindings in 6 scopes` | `Tab` and `l` swapped on "Next pane" (temporary): `Global NextFocus left: ["l", "Tab"] right: ["Tab", "l"]` | pass |
| After the swap, the actions with several keys keep v0.16.0's order (the first is what the palette and the footer show) | `U: ui::keymap::tests::several_keys_of_one_action_keep_their_order` | — | pass |
| Every v0.16.0 key and command still does what it did; nothing is added | `U: tests/v016_pins_test.rs`, unchanged | — | pass (3 of 3) |
| A row's keys always bind an action | `U: ui::action_table::tests::ids_are_unique_and_rows_well_formed` | — | pass |
| One row stands for each first word, and its own usage starts with it; the parser knows every name and alias the rows list | `U: ui::command::tests::each_first_word_has_one_row_of_the_registry`, `the_table_and_the_parser_agree` | — | pass |
| The help lists the rows grouped (Message, Go, View, …) with the live keys; an override (`x` → open-inbox, `?` disabled) shows on the row and the disabled key goes away | `U: ui::help::tests::help_is_derived_from_the_effective_scoped_map`, `help_lists_commands_after_the_keys_and_scrolls_to_the_end` | — | pass |
| The leader panel names keys by the rows' titles, grouped, every key at 80×24 and 120×32 | `U: ui::help::tests::which_key_labels_only_valid_continuations`, `leader_panel_shows_every_key_at_80x24`, `app::find_tests::the_leader_panel_waits_300_ms_and_asks_for_one_frame` | — | pass |
| The actions menu says what the palette says (`Delete my message: only your own message can be deleted`) | `U: app::find_tests::d_on_another_persons_message_says_why_not`, `app::tests::empty_state_renders_contextual_actions_overlay` | — | pass |
| **A user's binding shows in the help, the leader panel, the palette and the footer**: `r` disabled, `x` and `Space x` bound to react; the help's "React to message" row reads `x, Space x`, the leader panel `x  React to message`, the palette's key column `x`, the footer `x react` | `U: app::registry_tests::a_users_binding_shows_in_the_help_the_leader_panel_the_palette_and_the_footer`; footer alone `ui::status::tests::the_hint_shows_a_users_binding` | — | pass |
| `docs/keys.md` is what bzz writes | `U: tests/keys_doc_test.rs` | the file with one line removed (temporary): `docs/keys.md is stale: run cargo run --example keys_doc > docs/keys.md` | pass |
| The help scenarios on the release binary: `?` on an empty profile; a custom `z i` sequence shows on "Open Inbox"; the help at 80×24; the leader panel at 80×24 and 120×32 | `T: startup-help-quit`, `custom-keymap-inbox`, `t17-find-help-80x24`, `t17-find-leader` | — | pass |

#### C10 — the footer follows the focus

| Claim | Proof | Result |
|---|---|---|
| At 160 columns: a message `Enter thread · r react · i reply · Ctrl-K actions`; the empty timeline `i write · Space Space find · Ctrl-K actions`; the thread pane `i reply · r react · Space 4 hide · Ctrl-K actions`; the channels `Enter open · Space Space find · i write · Ctrl-K actions`; the Inbox `o open · i reply · m read · Ctrl-K actions`; the composer `Enter send · Ctrl-J newline · Esc close`; the huddle panel `s start · j join · m mute` (the App offers only those that apply); an overlay `Esc close`. At 80 the message hint is whole; at 60 it takes fewer keys, and the palette's key is always the last | `U: ui::status::tests::the_hint_follows_the_focus_at_60_80_and_160_columns` | pass |
| Only what can run is offered | `U: ui::status::tests::the_hint_offers_only_what_can_run` | pass |
| In the App the hint changes with the focus (channels, timeline, Inbox, the `:` line) | `U: app::registry_tests::the_status_line_hint_follows_the_focus` | pass |
| On the release binary on the pinned relay: a message, the channels, the composer, the huddle panel, then 80×24 | `T: t17-registry-app-footer` | see "Relay run" |

Rule 20: the hint is computed while the status line is drawn, from state
the frame already reads; it requests no frame and arms no timer. The
existing frame-count tests (`app::tests::idle_tick_does_not_request_a_redraw_without_visible_work`
and the at-rest tests) pass unchanged.

#### C11 — the `:` line completes

| Claim | Proof | Result |
|---|---|---|
| `:hu`, Tab → `:huddle `; Tab → `:huddle start`; Tab → `:huddle join`; Shift-Tab back; the verbs start, join, leave, mute, end, devices, react, input, output are offered; under the line `Tab: [start]  join …` and `usage: :huddle [start | …]`; typing after a completion parses as typed | `U: ui::command::tests::tab_completes_a_command_then_its_verbs_and_cycles` | pass |
| `:theme n`, Tab → a theme id bzz has; `:theme ` offers the themes then `reload`; `:app open `, Tab, Tab cycles the installed apps with `usage: :app open <name>`; `:sort ` → alpha, recent, default; `:presence ` → on, off; `:app ` lists its verbs | `U: ui::command::tests::tab_completes_themes_apps_and_fixed_words` | pass |
| A wrong word completes nothing and says `no command starts with hudle · did you mean :huddle?`; `:huddle xyz` says nothing after `:huddle` starts with `xyz`, with the usage | `U: ui::command::tests::a_wrong_word_completes_nothing_and_says_so` | pass |
| The same through the App at 80×24, and Esc forgets the cycle | `U: app::registry_tests::tab_completes_the_command_line_and_shows_what_it_offers`, `tab_completes_theme_names_and_a_wrong_word_says_so` | pass |
| On the release binary at 80×24: `:hu`, Tab, Tab, Tab; `:theme n`, Tab; `:hudle`, Tab; `:sort `, Tab | `T: t17-registry-complete` | pass |
| On the pinned relay: `:app open `, Tab completes an installed app | `T: t17-registry-app-footer` | see "Relay run" |

#### Commands run

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --example scripted_harness
cargo test --locked --all-targets
cargo test --locked --no-default-features --lib
cargo run --locked --example keys_doc > docs/keys.md
BZZ_BIN=<target>/release/bzz BZZ_TMUX_SCENARIOS=automated BZZ_TMUX_Text captures from tmux, read as a newcomer would:

- 80×24, `:hu`, Tab, Tab: `<scratch>/captures-complete/t17-registry-complete/03.txt`
  — the line `:huddle start`, under it `Tab: [start]  join  leave  mute
  end  devices  react  input  output` and the usage whole on three rows
  with its summary. A first capture cut the usage at two rows; the usage
  now takes up to four.
- 80×24, a wrong word: `…/t17-registry-complete/08.txt` — `no command
  starts with hudle · did you mean :huddle?`.
- 160×45 on the relay: `<scratch>/captures-harness/t17-registry-app-footer/`
  `02.txt` (a message: `Enter thread · r react · i reply · Ctrl-K
  actions`), `03.txt` (`:app open `, Tab), `05.txt` (the channels),
  `06.txt` (the composer: `Enter send · Ctrl-J newline · Esc close`, under
  the composer's own foot), `08.txt` (the huddle panel with no huddle:
  `s start · Esc close`), `10.txt` (80×24: `Enter thread · r react · Ctrl-K
  actions`).
- 160×45, the help with a custom sequence:
  `<scratch>/captures-complete/custom-keymap-inbox/02.txt`.

What a newcomer may trip on: in the composer the hint repeats two keys the
composer's frame already shows in its foot.=<scratch>/captures-auto3 python3 scripts/test-tui-tmux.py
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin BZZ_TMUX_Text captures from tmux, read as a newcomer would:

- 80×24, `:hu`, Tab, Tab: `<scratch>/captures-complete/t17-registry-complete/03.txt`
  — the line `:huddle start`, under it `Tab: [start]  join  leave  mute
  end  devices  react  input  output` and the usage whole on three rows
  with its summary. A first capture cut the usage at two rows; the usage
  now takes up to four.
- 80×24, a wrong word: `…/t17-registry-complete/08.txt` — `no command
  starts with hudle · did you mean :huddle?`.
- 160×45 on the relay: `<scratch>/captures-harness/t17-registry-app-footer/`
  `02.txt` (a message: `Enter thread · r react · i reply · Ctrl-K
  actions`), `03.txt` (`:app open `, Tab), `05.txt` (the channels),
  `06.txt` (the composer: `Enter send · Ctrl-J newline · Esc close`, under
  the composer's own foot), `08.txt` (the huddle panel with no huddle:
  `s start · Esc close`), `10.txt` (80×24: `Enter thread · r react · Ctrl-K
  actions`).
- 160×45, the help with a custom sequence:
  `<scratch>/captures-complete/custom-keymap-inbox/02.txt`.

What a newcomer may trip on: in the composer the hint repeats two keys the
composer's frame already shows in its foot.=<scratch>/captures-harness ./scripts/test-relay.sh --tmux
```

`cargo test --locked --all-targets`: 1288 passed, 0 failed (3 ignored).
`--no-default-features --lib`: 656 passed. fmt and clippy clean.
`automated`: 11 of 11 passed (`<scratch>/captures-auto3`); after the last
change `t17-registry-complete`, `custom-keymap-inbox` and
`startup-help-quit` again, 3 of 3.

#### Relay run

`./scripts/test-relay.sh --tmux` (every harness scenario, v0.13 to v0.17,
in manifest order, one fixture, pinned relay): **42 of 50 passed**, among
them `t17-registry-app-footer` (`:app open `, Tab wrote
`:app open approvals` with `Tab: [approvals]  board  looper  poll  tasks`
and `usage: :app open <name>` under it; the hint in each context).

The 8 that failed do not reach anything this branch changed:

- `t17-palette-unavailable-stays`, `t17-find-delete-unread-status`,
  `t17-sound-call`, `t17-palette2-theme-react`, `t17-palette2-app` stop at
  their second step, `type:t9-gen, enter` → `Generated custom emoji`: the
  channel opens, but in the whole suite the t16 scenarios have posted the
  Board's long brief to `#t9-general` and the fixture's message is above the
  fold. `t17-palette-sizes` expects `can't now` on the last message, which
  in the whole suite is my own (the brief). Run apart on a fresh fixture
  (`BZZ_TMUX_SCENARIOS=` those, `<scratch>/subset.log`), the first four
  pass; `t17-palette2-theme-react` then counts three rows up to a message
  that another scenario's rows moved, and `t17-palette2-app` and
  `t17-registry-app-footer` need the apps the t16 scenarios install.
- `t17-palette2-find` waits for `profile · r refresh · Esc close`; since
  `8a95a33` (look) the profile's frame has `profile` in its title and
  `r refresh · Esc close` in its foot, so the string never shows. Not
  changed here: it is palette2's scenario.
- `t15-coding-harnesses-in-panel` waits for a sentence about `BZZ_GROK`
  that depends on the coding programs installed on the machine.

Two scenarios that were stale in the same way and touch what this branch
changed were brought up to date: `custom-keymap-inbox` (the help's Inbox
rows are no longer on the first page; it now looks for its `z i` sequence
on "Open Inbox"), and `t17-demo-first-minute` (`reaction · Enter toggle`,
the reaction picker's title before look moved its keys to the foot; it now
looks for `Enter toggle · : custom`). `t17-find-leader` and
`t17-find-delete-unread-status` read the registry's titles
(`Cycle channel sort`, `Delete my message: …`).

Harness log: `<scratch>/harness.log`; captures `<scratch>/captures-harness`.

#### Captures

Text captures from tmux, read as a newcomer would:

- 80×24, `:hu`, Tab, Tab: `<scratch>/captures-complete/t17-registry-complete/03.txt`
  — the line `:huddle start`, under it `Tab: [start]  join  leave  mute
  end  devices  react  input  output` and the usage whole on three rows
  with its summary. A first capture cut the usage at two rows; the usage
  now takes up to four.
- 80×24, a wrong word: `…/t17-registry-complete/08.txt` — `no command
  starts with hudle · did you mean :huddle?`.
- 160×45 on the relay: `<scratch>/captures-harness/t17-registry-app-footer/`
  `02.txt` (a message: `Enter thread · r react · i reply · Ctrl-K
  actions`), `03.txt` (`:app open `, Tab), `05.txt` (the channels),
  `06.txt` (the composer: `Enter send · Ctrl-J newline · Esc close`, under
  the composer's own foot), `08.txt` (the huddle panel with no huddle:
  `s start · Esc close`), `10.txt` (80×24: `Enter thread · r react · Ctrl-K
  actions`).
- 160×45, the help with a custom sequence:
  `<scratch>/captures-complete/custom-keymap-inbox/02.txt`.

What a newcomer may trip on: in the composer the hint repeats two keys the
composer's frame already shows in its foot.

#### Changed on purpose, not keys or commands

Rule 19 holds for every key and command (the pins pass unchanged). What a
person reads changed:

- The help is grouped by the palette's groups and names actions by the
  registry's titles ("Open Inbox", not "open Inbox"; "Go to channel or DM",
  not "channel / DM switcher"); the leader panel and the actions menu too.
  The actions menu's "reply" is "Write a message", "delete own message" is
  "Delete my message", "open context" is "Open thread".
- The commands in the help are in the registry's order, not find's
  hand-picked one.
- The `:` prompt is taller: under the line it lists what Tab offers and
  the usage.
- The status line's hint names the keys of the focus instead of always
  `Ctrl-K actions · ? keys`.

## L — The look

### v0.17.0 — task `look` (L1 to L6, C2)

Branch `t3/implement-look-feature`, cut from `v0.17.0` at `543dbd1`, with
`v0.17.0` at `bfd340f` (viz) merged in. Linux x86_64, 2026-11-24/25, on a
machine shared with six other builds (load 18 to 35). Buzz pin
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. `<scratch>` is a folder outside
the repository.

#### Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **L1.** With `NO_COLOR` (any non-empty value) no cell carries a colour, and the focused pane has a heavy frame and a reversed title; an unfocused one stays thin and plain | buffer test | `cargo test --lib ui::frame::tests::under_no_color_no_cell_has_a_colour_and_focus_is_a_heavy_frame` | pass: every cell `fg`/`bg` `Reset`, top row `┏ design ━`, label `REVERSED`; unfocused `┌ design ─` |
| L1. The colour depth follows `NO_COLOR`, `COLORTERM`, `TERM` (`dumb` none; `*256*` 256; other `TERM` 16; `truecolor`/`24bit`/`*direct*` true; Windows without `TERM` true) | unit | `ui::frame::tests::colour_mode_follows_no_color_colorterm_and_term` | pass |
| L1. Without true colour every RGB value goes to the nearest 256 or 16 colour in the frame's last pass | unit | `ui::frame::tests::without_true_colour_rgb_values_are_reduced` | pass |
| L1. The huddle visualizer asks the same decision (`Look::from_env` → `ColorMode::detected`) | read + lib tests | `cargo test --lib huddle_lanes` | pass (the lanes' own `look_reads_the_environment_it_is_given` still covers `from_vars`) |
| **L2.** A focused pane is in the primary colour with its title as a label; an unfocused one is muted | buffer test | `ui::frame::tests::a_focused_pane_is_in_the_primary_colour_with_its_title_as_a_label`, `…an_unfocused_pane_is_quiet` | pass |
| L2. All 60 themes keep focus apart from the quiet frames, and still draw | theme tests | `ui::frame::tests::every_builtin_theme_keeps_focus_apart_from_the_quiet_frames`; `ui::theme::tests::*`; `cargo test --test theme_test` | pass |
| L2. Instructions left the titles: `Board · Ctrl-c leave` is `┌ Board ─` with `Ctrl-c leave` at the foot; overlays' keys are a coloured footer | buffer tests + scenarios | `ui::app_pane::tests::the_border_says_how_to_leave_and_the_notice_costs_no_row`, `app::pane::tests::*`, `ui::frame::tests::hints_colour_the_keys_and_mute_what_they_do`; the relay suite (below) | pass |
| **L3.** At 60, 80 and 160 columns the status line says ` NORMAL `, ` #general `, ` ● online ` and `Ctrl-K actions · ? keys`, and no `HALFBLOCKS`, `SIXEL`, `KITTY` or `q quit` | buffer test | `ui::status::tests::at_60_80_and_160_columns_the_line_says_mode_channel_and_online` | pass |
| L3. The mode is a label, the dot is coloured, `● offline` when offline | buffer test | `ui::status::tests::the_mode_is_a_label_and_the_dot_is_coloured` | pass |
| L3. Narrow: the lowest priority drops first; the two slots take a line and a priority; the huddle slot takes its shorter forms before it drops | buffer tests | `ui::status::tests::when_narrow_the_lowest_priority_drops_first`, `…the_two_slots_take_a_line_and_a_priority`, `…the_huddle_slot_shortens_before_it_drops` | pass |
| L3. A notice is shown whole before the channel and the hint drop | buffer test + scenario | `ui::status::tests::a_notice_is_shown_whole_before_the_channel_and_the_hint`; `t16-m4-dev-remove-reinstall` | red first on the relay suite (the 69-character notice was cut at 120 columns), green after `89acba5` |
| **C2.** The hint reads `Ctrl-K actions · ? keys`; a notice expires 5 s after it appears and asks for exactly one frame in 10 s of rest | paused-time test | `ui::status::tests::a_notice_expires_after_five_seconds_and_asks_for_one_frame` | pass: 1 frame, the notice gone at 5 s, a replaced notice starts its 5 s again |
| **L4.** Behind an overlay every cell is dim (`DIM`, so it holds under `NO_COLOR`), the overlay's own cells are not, and its footer has its keys | app buffer test | `app::tests::behind_an_overlay_every_cell_is_dim`; `ui::frame::tests::the_backdrop_dims_every_cell_even_without_colour` | pass |
| **L5.** An empty conversation says `No messages here yet` and what to do; offline, the first row says `offline · showing what is saved here · … :reconnect tries now` | buffer tests | `ui::timeline::tests::an_empty_conversation_and_an_offline_one_say_what_to_do_next`; `app::tests::rendered_core_and_mention_rows_have_semantic_hit_targets` (offline line, first message one row lower) | pass |
| L5. The approval banner has its own row: the panes' titles are whole under it, and the help starts under it | app test | `app::tests::a_command_waits_for_the_owner_on_every_screen_and_runs_only_on_his_yes`; scenario `t15-approval-approve-refuse` | pass |
| **L6.** A `── new ──` line before the first unread message, nowhere without one | buffer test | `ui::timeline::tests::a_new_line_is_drawn_before_the_first_unread_message` | pass |
| L6. The unread count at the right of the channel, in `UnreadCount`; none for a read channel; in every theme | buffer tests | `ui::sidebar::tests::the_unread_count_is_at_the_right_of_its_channel`; `cargo test --test theme_test` | pass |
| L6. The count is what the unread set counts, and reading clears it (an event in another channel shows its count) | store test | `cargo test --test store_events_test unread_counts_agree_with_the_unread_channels` | pass |
| Rule 19: no key or `:` command moved | read | no change to `src/ui/keymap.rs` or `src/ui/command.rs` | — |
| The scenario suite on the pinned relay, through tmux | scenarios | `BZZ_TMUX_SIZE=200x50 BZZ_TMUX_SCENARIOS=harness BZZ_TMUX_CAPTURES=<scratch>/captures-after BZZ_TMUX_CAPTURES_ANSI=1 ./scripts/test-relay.sh --tmux` | see "Relay suite" |

Not seen red against the base for every test: most of the new tests
test what did not exist (a module, a field) and would not compile there.
Seen red during the work, for the claimed reason: the notice truncation
(relay suite), the pane titles (`app::pane::tests::*`, `ui::approval`,
`ui::local_agents` after L2 until updated), the hit row under the offline
line.

#### Scenario checks that read a title or the status line, changed

In `scripts/tui-herdr-scenarios.toml`:

| Was | Now |
|---|---|
| `agent broker · not serving` | `not serving ┐` |
| `edit message · Enter save · Esc cancel` | `Enter save · Esc cancel` |
| `a real command waits for your answer · a allow · r refuse` | `a allow · r refuse` |
| `new local agent · Tab/↑/↓ move · …` / `new local agent · Enter sign and create · …` | the same keys without `new local agent · ` |
| `Board · Ctrl-c leave`, `Looper · Ctrl-c leave` | `Ctrl-c leave ┘` |
| `Board · zoomed · Ctrl-c leave` | `zoomed ┐` |
| `Board · 5 focus` (11 checks, one `absent`) | `5 focus ┘` |
| `message #t9-general · Enter send` | `Enter send · Ctrl-v paste` |

Added at the end: `t17-look-captures` (80×24 then 160×45: the workspace,
the finder over it, the focus moved). `scripts/test-tui-tmux.py` gained
`BZZ_TMUX_CAPTURES_ANSI` (each capture also with its colours).

#### Relay suite

- Run 1 (`4215053`): 33 of 36. `t16-m4-dev-remove-reinstall` (the notice cut,
  fixed in `89acba5`), `t16-m6-share-offer-approve` (follows from it: no
  board left), `t17-look-captures` (its own check wrapped at 80 columns).
- Run 2 (`89acba5`): 35 of 36; `t17-look-captures` quit with the focus moved
  (fixed in the scenario, `eb4d27a`).
- Run 3 (`9f8ac04`, after the merge of viz): 35 of 36; every v0.13 to v0.16
  scenario passed; `t17-look-captures` again, its `Tab` went into the Board
  open in `#t9-general` and `q` was the app's. Its focus steps were removed
  (`255bed9`) and it was run alone, with a pause before typing in the finder
  (a fresh fixture typed into it too early): 1 of 1, after and before.

#### Captures

`<scratch>/captures-after2/t17-look-captures/NN.{txt,ansi}` (after, at
`255bed9`+pause) and `<scratch>/captures-before/t17-look-captures/` (before).
Steps 03–05 are 80×24 (workspace, finder, workspace), 06–08 160×45. Every
scenario of run 3 is captured in `<scratch>/captures-after/`, at 200×50
unless the scenario names a size, with colours (`.ansi`).

"Before" is the v0.16.0 release binary (`bzz 0.16.0`, built 2026-11-20),
not a build of `543dbd1`: the look did not change between them. It ran on
the same fixture through a wrapper that removes `motion = …` from the
fixture's `config.toml` (v0.16.0 refuses the key viz added), and the
runner's `cargo` was a no-op so nothing was rebuilt. Read as a newcomer:
before, the status line says `online  HALFBLOCKS  ? help · q quit` and
the finder's title carries its keys; after, `NORMAL  #t9-general  ● online
Ctrl-K actions · ? keys`, the unread counts `1` stand at the right of
`#t14-names` and `#t9-announcements`, the finder's keys are its footer, and
the `.ansi` captures show the focused title as a bold reversed label
(`1;7m`) in a cyan frame, the other frames in grey, and everything behind
the finder dim (`2m`).

#### Theme tokens added (the site mirrors `builtin.rs`)

No palette field was added to `builtin.rs`. New highlight groups in
`src/ui/theme/mod.rs`, each a link with a default, overridable in a theme
file: `FocusedPaneTitle` (FocusBorder, reversed, bold), `HintKey` (Info,
bold), `HintText` (Muted), `StatusChannel` (Muted), `StatusOnline` (Success),
`StatusOffline` (Warning), `StateBanner` (warning background; palettes:
`background` on `warning`), `NewMessageMarker` (Warning), `UnreadCount`
(Warning, bold). Changed defaults: `PaneBorder` and `PaneTitle` link to
`Muted` (were `Border`, `Title`); `ComposerBorder` links to `PaneBorder`;
bzz's own `FocusBorder` is cyan (was the terminal's colour); bzz's
`StatusBar` has no background and its mode labels are cyan, yellow, yellow.

### v0.17.0 — task `motion` (L7, L8, L9, one look)

Branch `t3/motion`, cut from `v0.17.0` at `9902c74` ("viz2 and demo
together"). Linux x86_64, 2026-11-25, on a machine shared with six other
builds (load 20 to 25). Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
`<scratch>` is a folder outside the repository. Debug builds unless said.

#### What was built

- `src/ui/motion.rs`: the timing scale (`FAST` 80 ms, `BASE` 150 ms, `SLOW`
  250 ms, `HOLD` 3 s, `LOOP` 1.2 s, `LOADING` 10 s, `FRAME` 33 ms), `Anim`
  (a value from 0 to 1 between two instants of `tokio::time::Instant`, with
  `wake(now)`), `Role` (`Informs` / `Decorates`: `calm` keeps the first),
  `blend` (only between two true colours), and the cell passes: the focus
  `sweep`, `unfolded`, `changed_rect`, `copy_cells`/`paste_cells`,
  `restore_outside`, `tint`, `flash_count`, `shimmer`.
- `src/ui/toast.rs`: `Toasts`, one shown and three waiting.
- `src/app/stage.rs`: what moves in the App (noticed while a frame is drawn,
  laid over it after the panes), the loop's `Pacing`, and when the loop's
  housekeeping is busy.
- The run loop in `src/app.rs`: no fixed tick. It sleeps until the earliest
  of a frame asked for (at once, at most one per 33 ms), an animation's next
  frame, the huddle's levels (every 33 ms while a call, the generated huddle
  or the microphone check is on), a notice's end, and the housekeeping.
- One look: the palette (frame, title, keys), the first screen (frame and
  keys), the zoomed huddle's picker (frame) and keys, and the microphone
  check (frame and keys) draw through `frame::block` and `frame::hints`. The
  leader panel already did (`frame::modal`).

#### Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **L7.** An animation asks for a frame every 33 ms while it runs and its end, then none; the scale's lengths give 3 (80 ms), 5 (150 ms) and 8 (250 ms) frames | paused-time unit | `cargo test --lib ui::motion` | pass: `[33, 66, 80]`, `[33, 66, 99, 132, 150]`, 8 ending at 250 |
| L7. With motion off every animation is at its end at once and asks for no frame; `calm` drops what decorates and keeps what informs | paused-time unit | `ui::motion::tests::with_motion_off_…`, `…calm_keeps_what_informs…` | pass |
| **L7. A toast asks for frames for 150 ms, none for 3 s, then for 250 ms**; with motion off it is there at once and gone at 3 s, with no frame between | paused-time unit | `ui::toast::tests::a_toast_asks_for_frames_for_150_ms_none_for_3_s_then_for_250_ms`, `…with_motion_off_a_toast_is_there_and_gone_with_no_frame_between` | pass: frames at 33, 66, 99, 132, 150, then 3150 … 3400 (9); off: one frame, at 3000 (the one that removes it) |
| L7. The same at the level of the App, and the toast sits above the composer, never over it | paused-time App test | `app::stage::tests::a_toast_moves_150_ms_rests_3_s_moves_250_ms_and_never_covers_the_composer` | pass: 5 frames; the next wake exactly 3 s later; 9 frames out; the toast's row is above `message #general` |
| L7. One toast at a time; three wait, the oldest waiting gives way; the same text is not said twice | unit | `ui::toast::tests::one_toast_at_a_time_with_a_short_queue` | pass |
| **L7. An arriving message is tinted 250 ms**: blended towards the theme's own background where it has one (8 frames, `dark`), one step where it has none (1 frame, `bzz`, whose background is the terminal's); `calm` keeps it, `off` none; my own message and one written over 60 s ago take none | paused-time App test | `app::stage::tests::an_arriving_message_takes_a_tint_for_250_ms` | pass |
| **L7. A rising unread count flashes once** (reversed, 250 ms, one step: 1 frame); a count seen first or that falls does not | paused-time App test | `app::stage::tests::a_rising_unread_count_flashes_once` | pass |
| **L8. The focus colour runs round the pane** that takes the focus in 150 ms (5 frames); none with `calm` or `off` | paused-time App test | `app::stage::tests::the_focus_sweep_takes_150_ms_and_only_with_full_motion` | pass |
| **L8. An overlay unfolds in 150 ms and folds in 80 ms**; while it unfolds only its own cells change (the screen behind is dimmed once); folded away the screen is as before it opened; with motion off it is whole at once and asks for nothing | paused-time App tests | `app::stage::tests::an_overlay_unfolds_in_150_ms_and_folds_in_80_ms`, `…with_motion_off_an_overlay_shows_whole_at_once` | pass: `[33, 66, 99, 132, 150]` and `[33, 66, 80]`; every cell outside the finder equal between its first and last opening frame |
| **L8. Placeholder rows shimmer while history loads**, from 80 ms after the fetch starts (no flash on a quick one), for 10 s at most, then the plain `loading history…` line and no frame; with motion off the plain line at once and no frame | paused-time App test | `app::stage::tests::a_loading_shimmer_asks_for_frames_for_10_s_at_most` | pass: first frame at 80, last at 10 000, ≤ 304 frames; off: none |
| Rule 20: at rest nothing asks for a frame, under `full`, `calm` and `off` | App test | `app::stage::tests::at_rest_nothing_asks_for_a_frame` | pass |
| **Rule 20: a waiting key is handled before a frame of an animation** | model of the loop | `app::stage::tests::a_waiting_key_is_handled_before_a_frame_of_an_animation` | pass: two keys, then the frame. The test models the loop's `select!` (biased, the input branch before the frame branch); that the real loop has this order is read, `src/app.rs` `run_to_exit` |
| **L9. The loop sleeps until its next deadline**: housekeeping once a second at rest, within 100 ms after any event or while something it polls is busy; frames asked for in a burst one interval apart; the huddle's levels every 33 ms only while polled | paused-time unit | `app::stage::tests::the_loop_sleeps_until_its_next_deadline` | pass |
| **L9. Wake-ups at rest, before and after**, `bzz demo` in `#team`, 160×45, 10 s after 4 s of start | real binary, `/proc/<pid>/task/*/schedstat` (switch-ins, summed over threads) | `<scratch>/rest.sh <binary> <label>` and `<scratch>/threads.sh` | see "At rest" |
| Rule 20 over SSH: the bytes of each transition through a real backend at 160×45 | crossterm backend into a counting writer, paused time | `app::stage::tests::the_bytes_of_each_transition_are_bounded_and_none_at_rest -- --nocapture` | see "Bytes" |
| The same, from a recording of the real bzz | `record-tape.py` (PTY, byte-for-byte with times) of `bzz demo` | `<scratch>/film/timeline.json`, tape analysed by mark | see "Bytes" |
| **One look**: the palette's keys are in the hint colour and its title a label; the first screen's keys in the hint colour | App buffer test | `app::stage::tests::the_palette_and_the_first_screen_look_like_the_rest` | pass |
| One look: the zoom's picker has the frame every picker has; its keys and the zoom's keys in the hint colour; without colour (`Depth::None`) no colour | goldens | `cargo test --lib huddle_zoom` (13 goldens rewritten with `BZZ_UPDATE_GOLDENS=1`: only the keys' colour, and the picker's frame) | pass; the diff read: text unchanged but the picker's frame `╭─ huddle style ─╮` → `┌ huddle style ─┐` |
| One look: the microphone check's frame and keys | buffer test | `ui::huddle_mic::tests::*` (no colour inside the frame under `Depth::None` still holds) | pass |
| Rule 19: no key or `:` command moved | read + scenarios | no change to `src/ui/keymap.rs`, `src/ui/command.rs`; the relay suite | — |
| Scenarios run with motion off unless they test motion | read + scenarios | `scripts/test-tui-tmux.py` adds `BZZ_NO_MOTION=1` unless the scenario has `motion = true`; `scripts/test-tui-herdr.sh` adds it always | `t17-motion-toast`, `t17-motion-overlay-and-focus`, `t17-motion-off-toast` pass on the debug binary |
| The whole scenario suite through tmux on the pinned relay | scenarios | `BZZ_TMUX_SCENARIOS=all BZZ_TMUX_CAPTURES=<scratch>/captures-relay ./scripts/test-relay.sh --tmux` | see "Relay suite" |

The tests of `motion`, `toast` and `stage` test what did not exist and do
not compile against the base. Seen red during the work: twelve App tests
that read a frame right after opening an overlay (the overlay was mid-
unfold), until App tests see motion off unless they set it
(`stage::TEST_MOTION`); the overlay test's own first form (a timeline cell
is `DIM` by design); the bytes test's first bound (a frame with nothing
changed still writes the cursor's 25 bytes; the loop draws none at rest).

#### At rest (L9)

`bzz demo`, 160×45, `TERM=xterm-256color COLORTERM=truecolor`, a scratch
home, 10 s of nothing after 4 s of start. Switch-ins of every thread of the
process (`/proc/<pid>/task/*/schedstat`, third field); CPU from the first
field; bytes from `tmux pipe-pane`.

| Binary | Wake-ups / s (process) | main thread / s | CPU in 10 s | Bytes in 10 s |
|---|---|---|---|---|
| before (`9902c74`) | 226.6 | 104 | 633 ms | 50 |
| after (`71645bb`) | 45.8 | 12.4 | 80 ms | 25 |
| before, again | 192.3 | | 638 ms | 50 |
| after, final (the rest deadline is the store refresh's own) | 54.9 | 17.7 | 86 ms | 50 |
| before, in `#infra` with the generated huddle speaking (strip on) | 268.8 | | 1611 ms | 24 116 |
| after, the same | 274.8 | | 1579 ms | 21 405 |

The loop itself: a debug counter (not committed) showed 10 loop wakes in
10 s at rest, all the once-a-second housekeeping, none busy. What is left
on the main thread (12 to 18/s) is that housekeeping's store round trips
(`hydrate_cache` and the subscriptions each second; `bzz-sqlite` 12 to
16/s), and the rest is tokio's workers and the demo's in-process relay.
The first measurement's 11.7/s on `bzz-sqlite` was low for a reason found
after it: a rest tick exactly 1 s after the last could miss the store's
refresh ("more than a second" since a refresh that ended after the tick
began) and put it off to 2 s; the rest deadline is now the refresh's own
(`Pacing::tick_at`, `App::refresh_due`). With a
huddle the visualizer keeps its own pace (plan B4), as before.

The bytes in 10 s at rest are the 25 to 50 bytes the terminal answers or
the cursor, before and after; no frame is drawn.

#### Every user of the old ticks

The 33 ms tick and the 100 ms tick are gone. Each user and what wakes it now:

| Old tick | User | Now |
|---|---|---|
| 33 ms | `tick_huddle_viz` (the call's meter, the generated huddle, the microphone check) | every 33 ms while `huddle_polled()`: a call, the demo's huddle where it plays, or the check is on |
| 33 ms | draw a frame asked for (`RedrawGate`), apps polled first | at once when asked, at most one per 33 ms |
| 33 ms | `huddle_frame_drawn` (the orb's throttle) | after every frame, as before |
| 100 ms | `media.poll` (images decoded) | 100 ms while an image is in flight (`MediaRuntime::busy`) |
| 100 ms | rate-limit countdown | 100 ms while limited |
| 100 ms | agent typing expiry and spinner (200 ms) | 100 ms while anyone is typing |
| 100 ms | leader panel (300 ms) and its 750 ms timeout | 100 ms while a sequence is on |
| 100 ms | search and DM search debounce (300 ms) | 100 ms while one waits |
| 100 ms | approvals of local agents and their countdown | 100 ms while local agents run or one waits |
| 100 ms | `:agents` panel refresh (1 s) | 100 ms while it is open |
| 100 ms | read-state publish 5 s after reading | 100 ms in the last second before it |
| 100 ms | app sessions: timers, records, offers (`tick_apps`) | 100 ms while an app session runs; once a second otherwise |
| 100 ms | store refresh `hydrate_cache` (when dirty, else each 1 s) and subscriptions | 100 ms while dirty; once a second |
| 100 ms | mark the visible conversation read, the agent-typing subscription | within 100 ms of any event (input, relay, task), as before; once a second otherwise |
| 100 ms | presence snapshot (60 s), own presence (away at 300 s, heartbeat 60 s), inbox refresh (30 s) | the once-a-second housekeeping: at most 1 s later than the deadline |
| — | a notice's end (5 s) | its own deadline, unchanged |

#### Bytes (rule 20)

Through a crossterm backend into a counting writer, 160×45, `dark`-less
default theme, paused time (`--nocapture` output):

| Transition | Bytes | Frames |
|---|---|---|
| ten frames drawn with nothing changed | 250 (the cursor's codes; the loop draws none) | 10 |
| toast in, held 3 s, out | 1 555 | 15 |
| focus sweep | 2 791 | 6 |
| an arriving message's tint (one step) | 624 | 2 |
| overlay (finder) opens, with the dimming | 12 036 | 6 |
| overlay closes, the screen back | 12 291 | 4 |
| the same overlay with motion off | 9 448 and 12 027 | 1 and 1 |

The unfolding adds about 2.6 KB to the dimming's 9.4 KB, and the folding
about 0.3 KB: the backdrop is dimmed once and only the overlay's cells
change between frames.

From the recording of the real `bzz demo` (`record-tape.py`, 160×45, the
tape's output chunks summed between its marks):

| Window | Bytes | Writes |
|---|---|---|
| focus to the channels / back (each) | 2 850 to 2 876 | 5 to 7 |
| toast in (0.5 s from `y`) | 990 | 6 |
| **toast holding, 2.55 s** | **0** | **0** |
| toast out | 1 099 | 8 |
| palette opens (with the dimming and its rows) | 13 366 | 16 |
| palette closes | 12 782 | 13 |
| typing, sending and the teammate's answer with its tint | 5 467 | 41 |
| the last 2 s, nothing happening | 0 | 0 |

#### The film

`<scratch>/film/motion.mp4`: 30 s, 1920×1080, 30 fps, H.264, from the tape
above through the promo skill's tape player (`mountTape`, xterm.js) and
`hyperframes@0.9.13 render` (`HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1`):
the focus moving between the channels and the conversation, a copy's toast
in, held, out, the palette unfolding and folding, and a message written and
answered. Stills read at 5.55 s, 10.72 s, 12 s, 15.36 s, 16.5 s and 24 s.
Not shown: the shimmer (the demo's history loads in under 80 ms, so it never
shows) and the unread flash (no message arrives in another channel in this
take).

#### Captures

`<scratch>/look/`: at 80×24 and 160×45, the toast settled (`toast-*.txt`),
the palette mid-unfold (`palette-opening-*.txt`) and open (`palette-*`), the
focus sweep mid-way (`sweep-mid-*.ansi`). Read as a newcomer: the toast sits
at the bottom right of the conversation, inside its frame, above the
composer, and says what happened; the palette's keys are coloured like every
other footer. The palette's mid-unfold frame shows the dimmed conversation
around its middle for one or two frames of 150 ms.

#### Relay suite

Release binary, `BZZ_TMUX_SCENARIOS=all` (harness and automated), 160×45,
every scenario but the three `t17-motion-*` with `BZZ_NO_MOTION=1`.

- **Run 1** (`11cdc3d`; `ad3caa3` was committed while its release build ran, so its build may hold that small change or not; all of L7 to L9 in
  it): **48 of 54**. Failed:
  - `t17-demo-first-minute`: waits for `reaction · Enter toggle`, but the
    reaction picker's keys are now at its foot (task `look`). It fails the
    same with the base binary (`9902c74`, debug, alone): not this task's.
  - `t17-palette-unavailable-stays`, `t17-palette-sizes`,
    `t17-find-delete-unread-status`: in the whole run the Board that the
    v0.16 scenarios open in `#t9-general` is still open and covers what
    they look for. Run 2 below: all three pass alone.
  - `t15-coding-harnesses-in-panel`: its sentence wraps at 160 columns; the
    v0.16 record says it passes at 200×50 (run 3).
  - `t14-search-older`: `Ctrl-o` before the debounced search has answered,
    whose answer then clears the "older results" notice; the v0.16 record
    names it a flake, once in two runs.
- **Run 2** (`714d026`): the five above that are not the
  demo's, and the three `t17-motion-*`: **6 of 8**; the three order-bound
  palette and find scenarios and the three motion scenarios pass;
  `t14-search-older` and `t15-coding-harnesses-in-panel` as above.
- **Run 3** (`714d026`, 200×50): `t15-coding-harnesses-in-panel` passes;
  `t14-search-older` fails a third time. Read: the search answers twice,
  the local answer with "searching the relay…" and then the relay's; the
  scenario presses `Ctrl-o` when it sees `local + relay`, which the local
  answer already shows, and the relay's answer, coming after the older
  page, replaces that page's notice. In v0.16 it lost this race once in
  two; frames now come as soon as asked (before: up to 33 ms later), so
  the local answer is on screen sooner and the race is lost every time.
  Inferred, not measured against the base on the relay. The scenario now
  waits for `searching the relay` to be gone before `Ctrl-o` (the only
  change to an existing scenario). The product race (a later search
  answer clears the older page's notice) is left as it was: not this
  task's file.
- **Run 4** (`714d026` and the scenario's wait, 200×50):
  `t14-search-older` passes (Ctrl-o's notice 80 ms after the key);
  `t17-demo-first-minute` fails as on the base, for the reason above.

Every v0.13 to v0.16 scenario and every `t17-*` scenario of the other
tasks passed in run 1 or alone in runs 2 to 4, except
`t17-demo-first-minute`, which fails the same on the base binary.

### v0.17.0 — task `layout` (L10, L11, L12, F7)

Branch `t3/layout`, cut from `v0.17.0` at `9902c74`. Linux x86_64,
2026-11-25, on a machine shared with six other builds. Buzz pin as in
`runs/buzz-pin`. `<scratch>` is a folder outside the repository.

#### The table (L10)

Pinned by `ui::layout::tests::the_table_of_breakpoints` (24 rows, a writing
dock of 3, the default widths: sidebar 28, thread 44, app 42, rail 22). `R`
rail, `S` sidebar, `C` conversation, `T` thread, `A` app, each with its
width; after `|` the keys of what is hidden, which the conversation's frame
names (`hidden · channels 2 · thread 4`). "focus back" is the focus on the
conversation; "thread" has the focus on the thread; "thread and app" has
the app asked for last, "app and thread" the thread.

| Open panes | 60 | 72 | 80 | 100 | 120 | 160 | 200 |
|---|---|---|---|---|---|---|---|
| sidebar (2 communities) | C60 \| 12 | S28 C44 \| 1 | S28 C52 \| 1 | R22 S28 C50 | R22 S28 C70 | R22 S28 C110 | R22 S28 C150 |
| sidebar, one community | C60 \| 2 | S28 C44 | S28 C52 | R22 S28 C50 | R22 S28 C70 | R22 S28 C110 | R22 S28 C150 |
| thread (focused) | T60 \| 123 | C38 T34 \| 12 | C38 T42 \| 12 | C56 T44 \| 12 | S28 C48 T44 \| 1 | R22 S28 C66 T44 | R22 S28 C106 T44 |
| thread, focus back | C60 \| 124 | C38 T34 \| 12 | C38 T42 \| 12 | C56 T44 \| 12 | S28 C48 T44 \| 1 | R22 S28 C66 T44 | R22 S28 C106 T44 |
| thread, focus on channels | S22 C38 \| 14 | S28 C44 \| 14 | S28 C52 \| 14 | S28 C72 \| 14 | S28 C48 T44 \| 1 | R22 S28 C66 T44 | R22 S28 C106 T44 |
| app (not focused) | C60 \| 12 | S28 C44 \| 1 | C38 A42 \| 12 | C58 A42 \| 12 | S28 C50 A42 \| 1 | R22 S28 C68 A42 | R22 S28 C108 A42 |
| app focused | zoomed | zoomed | C38 A42 \| 12 | C58 A42 \| 12 | S28 C50 A42 \| 1 | R22 S28 C68 A42 | R22 S28 C108 A42 |
| thread and app | C60 \| 124 | C38 T34 \| 12 | C38 A42 \| 124 | C58 A42 \| 124 | S28 C50 A42 \| 14 | S28 C46 T44 A42 \| 1 | R22 S28 C64 T44 A42 |
| app and thread | T60 \| 123 | C38 T34 \| 12 | C38 T42 \| 12 | C56 T44 \| 12 | S28 C48 T44 \| 1 | S28 C46 T44 A42 \| 1 | R22 S28 C64 T44 A42 |
| `z` (thread and app open) | C60 \| z | C72 \| z | C80 \| z | C100 \| z | C120 \| z | C160 \| z | C200 \| z |

The rules behind it (`src/ui/layout.rs`, module documentation):

- While anything is beside it the conversation keeps **38** columns
  (`MIN_CONVERSATION`; v0.16: 30). 38 and not 40 so that v0.16's trial at
  80×24 holds: the conversation 38 and the app 40×18 inside.
- Panes are admitted in order: the pane with the focus, the right pane asked
  for last, the sidebar, the other right pane, the rail. The rail only when
  every pane before it that is asked for is drawn.
- The focused pane and the right pane asked for last narrow to their least
  width (sidebar 18, thread 30) before they drop; the others keep theirs.
- A focused pane that cannot sit beside the conversation takes its place
  (`T60` above); `3` gives the conversation back. A focused app that cannot
  dock still takes the whole window, as in v0.16.
- With one community the rail shows from 100 columns, for everyone (the demo
  had its own rule of never; it is gone). `1` shows it at any width.

**What changed from v0.16.** The app docks from 80 columns (v0.16: 72; at
72 a focused app is now zoomed, an unfocused one hidden with `app hidden ·
5`); app and sidebar from 108 (100); thread and app together from 124 (116),
with the sidebar from 152 (144), with the rail from 174 (166); a thread is
drawn from 68 columns at 30 wide (v0.16: from 74 at its own width) and beside
the sidebar from 110 (102). Below 70 columns v0.16 drew the conversation
alone whatever was asked; now the table applies at every width. v0.16 drew
the rail where a thread did not fit or where the sidebar would not fit even
alone; it no longer does. `scripts/tui-herdr-scenarios.toml` `t16-app-pane`
follows these widths (its 72-column step is now 80, and 72 says `app hidden ·
5`).

#### Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **L10.** The table above, at 60, 72, 80, 100, 120, 160 and 200 columns | unit | `cargo test --locked --lib ui::layout::tests::the_table_of_breakpoints` | pass |
| L10. At every width from 50 to 220, for 32 sets of open panes × 5 focuses × 1 or 3 communities × 3 width settings: the columns are adjacent and fill the body; the conversation keeps 38 while anything is beside it; the focused pane is drawn; what is asked and not drawn is in `Hidden` (the rail of a single community under 100 is the one exception, by rule); no rail beside a sidebar that gave way; a docked app is 42 wide | unit | `…::at_every_width_the_panes_fill_the_body_and_the_conversation_keeps_its_columns` | pass |
| L10. What drops comes back as the window widens: widening never draws fewer panes, a pane drops only for one before it in the order, and at 220 nothing is hidden | unit | `…::what_drops_comes_back_when_the_window_widens` | pass; red first (the rail came in where a thread did not fit, and left when it did): the rule "the rail only when every pane before it is drawn" came from it |
| L10. The thread from 68 columns, narrowed; at its width from 82; with the sidebar from 110; in the conversation's place below 68 while focused | unit | `…::a_thread_asked_for_is_drawn_from_68_columns_and_takes_the_place_below` | pass |
| L10. The rail of one community from 100, and at any width on `1` | unit | `…::the_rail_of_one_community_waits_for_100_columns_and_for_its_key` | pass |
| L10. v0.16's app rules at their new widths; zoom while focused where it cannot dock; 16 rows | unit | `…::the_thresholds_of_the_default_widths`, `…::a_focused_app_that_cannot_dock_takes_the_window_and_leaves_it_with_the_focus`, `…::the_widths_of_the_exit_gate`; `app::pane::tests::*` | pass (`the_thread_and_the_app_share_the_right_side` moved from 144 to 152) |
| L10. The frame names what is hidden with its key and shortens: `hidden · channels 2 · thread 4`, then without `hidden ·`, then keys only, then fewer keys | unit | `ui::shell::tests::what_is_hidden_is_named_at_the_right_of_the_frame_and_shortens` | pass. A first wording (`2 channels`) read as a count; the name now goes first |
| L10. `z`: only the conversation and the writing dock, the frame says `hidden · the other panes z`, `z` again restores; the focus goes to the conversation | unit, app buffer | `ui::layout::tests::conversation_only_draws_the_conversation_and_the_dock`; `app::shell::tests::the_frame_names_what_is_hidden_and_z_draws_the_conversation_alone` | pass |
| L10. Text wraps to the pane's real width and loses no character: every kind of timeline line (a system notice about a 64-character name, a header with an agent mark, an edited body with a long word, a grouped line, a thread reply also sent to the channel with an unknown delivery, a message sent from a thread, a deleted one, a pending one, a thread summary, reactions, the offline banner), every message marked, at every width from 20 to 200, shows the same characters as at 400 | buffer test | `cargo test --locked --lib ui::timeline::tests::every_kind_of_line_wraps_at_every_width_and_loses_no_character` | **red without the fix**: at 20 columns `…3333oined·…`, `lin17:15deliveryunknown]`, `sam17:28deleted]`, `me17:30pending]` (the wrapped row went back to column 0, under the selection mark). Pass after `4a0cd1d`; the banner (cut at narrow widths) wraps since `f9d02a9` |
| L10. Goldens of the main screens: the sidebar, a thread, an app and both, at 60×24, 80×24, 120×32 and 160×45 | golden | `cargo test --locked --lib app::shell::tests::goldens_of_the_main_screens_at_four_widths` (`tests/fixtures/layout/*.txt`, `BZZ_BLESS=1` writes them) | pass; 16 goldens, read one by one |
| L10. On `bzz demo`, one window from 60 to 200 columns: `hidden · channels 2` at 60; Enter on the checklist puts its thread in the conversation's place with `conversation 3`; `3` brings the conversation back with `thread 4`; at 80 the thread beside it; `z` and `z`; at 160 `┌ communities`; at 200 nothing hidden | tmux | `BZZ_BIN=<scratch>/bzz BZZ_TMUX_SCENARIOS=t17-layout-mouse,t17-layout-widths BZZ_TMUX_Text from tmux, in `<scratch>`, read as a newcomer would:

- `captures/t17-layout-widths/` (`bzz demo`): `02.txt` 60×20, the
  checklist's thread in the conversation's place, `channels 2 ·
  conversation 3` on its frame; `04.txt` 60×20, the conversation back with
  `hidden · channels 2 · thread 4`; `06.txt` 80×24, the thread at 42 beside a
  conversation of 38; `08.txt` `z`; `10.txt` 160×45, rail, sidebar,
  conversation and thread.
- `captures/t17-layout-mouse/` (`bzz demo`, 80×24): each gesture's screen.
- `mine-200x50/t17-layout-board-captures/` (`#t9-general` on the relay):
  `05.txt` 160×45 with a thread and the Board, `07.txt` 120×32 (the Board
  behind the thread, `app behind thread · 5`), `08.txt` 80×24, `09.txt`
  60×20 (the thread in the conversation's place), `10.txt` 60×20 the Board
  zoomed, `11.txt` 80×24 the Board docked beside 38 columns, `13.txt` 160×45
  sidebar, conversation, thread and Board. `12.txt`, named 120×32, was taken
  before the resize reached the pane and shows 80 columns.
- `mine-80x24/…`: the same at 80×24.

What a newcomer reads: at 80×24 and 60 columns the conversation is never
narrower than 38, and its top edge says what is hidden and which key brings
it (`hidden · channels 2`); at 160×45 everything fits and nothing is said.
The thread of the demo's code block wraps its long command outside the
code fence (`--version 2.4.0` under `│ harbor deploy`): markdown's own
wrapping, no character lost, left as it is.=<scratch>/captures python3 scripts/test-tui-tmux.py` | `2 of 2 scenarios passed` |
| **L11.** Each gesture on the hit map: the borders of the sidebar and the thread are `Splitter` targets (the two columns where the frames meet); a drag sets the width inside 18–60 / 30–80 and the conversation's 38, and the release saves it and says so; a press elsewhere is not a drag | unit, app buffer | `ui::shell::tests::the_borders_between_panes_are_where_the_frames_meet`; `app::shell::tests::a_border_dragged_resizes_its_pane_within_the_limits_and_is_saved` | pass |
| L11. The wheel scrolls the pane under the pointer and leaves the focus where it is, also while writing; over the composer it does nothing | app buffer | `app::shell::tests::the_wheel_scrolls_the_pane_under_it_and_leaves_the_focus` | pass |
| L11. A double click on a message opens that message's thread, and keeps it open if it is the one open (v0.16 toggled it, and closed another message's thread) | app + tmux | `app::shell::tests::a_double_click_opens_the_thread_of_the_message_clicked`; `t17-layout-mouse` step 2 | pass |
| L11. A click on the status line's `Ctrl-K actions · ? keys` opens the palette (the hint's place is read back from the frame) | app buffer + tmux | `ui::shell::tests::the_hint_is_found_where_the_status_line_drew_it`; `app::shell::tests::a_click_on_the_status_lines_hint_opens_the_palette`; `t17-layout-mouse` step 9 | pass |
| L11. Through a terminal, SGR mouse reports sent by tmux at 80×24: two wheel-ups show the checklist, a double click opens its thread, a drag from column 27 to 35 says `channel pane width saved as 36`, a click on `#design` opens it, a click on the hint opens the palette | tmux | `t17-layout-mouse` (the step field `mouse` of `scripts/test-tui-tmux.py`) | pass |
| L11. Nothing the keyboard does needs the mouse | read | every gesture above has its key: Tab/1–5, Enter, J/K and Ctrl-d/u, Alt-h/Alt-l (resize), Space Space, Ctrl-K | — |
| **L12.** `Esc` in the workspace and the Inbox is `Back`: it closes an overlay, the composer, the thread, the Inbox as before, and with nothing open says `nothing to close · q quits`; `q` still asks; overlays and the composer keep their own Esc | unit + tmux | `app::shell::tests::esc_with_nothing_open_does_not_ask_to_quit_and_q_does`; `t17-layout-mouse` steps 4–5 (`absent = "quit bzz?"`) | pass |
| L12. Rule 19: an owner's v0.16 key map that binds a sequence starting with a key v0.17 added (`z i` in `custom-keymap-inbox`) still loads, and its binding wins in that scope; with v0.16's own keys an ambiguity is still refused | unit + tmux | `app::shell::tests::an_owners_map_that_binds_z_something_still_loads`; `ui::keymap::tests::action_prefix_ambiguity_is_rejected`; scenario `custom-keymap-inbox` | **red first**: with `z` added the scenario's bzz refused to start (`keymap has an action/prefix sequence ambiguity`); pass after the fix in `KeyMap::load_from` |
| L12. Rule 19: the two moved bindings (`Workspace Esc`, `Inbox Esc`: `BackOrQuit` → `Back`) and the new `Workspace z` are named | integration | `cargo test --locked --test v016_pins_test` (`MOVED_KEYS`, `ADDED_KEYS`, a fourth `DELIBERATE_CHANGES`) | pass |
| **F7.** A huddle's backing channel (a private stream `huddle-` and eight hex digits) is not in the sidebar, the finder or the palette; `:system` shows it in the sidebar | app buffer | `app::shell::tests::a_dm_composer_names_the_person_and_the_backing_channel_is_not_listed` | pass |
| F7. A DM's composer says `message @Scout` (it said `message #Scout`) | app buffer + tmux | same test; `t17-layout-mouse` capture of `@Lin` | pass |
| F7. `:tochannel` says `sent to #x as a new message · the thread's draft is cleared` and clears the draft (in memory at once, in the store once the relay took it; on a failure the stored draft stays and the notice says so) | relay scenario | `t17-layout-f7` | see "Relay suite" |
| F7. `:huddle end` with no huddle says `no huddle is running here`, not "only the huddle's creator can end it" | relay scenario | `t17-layout-f7` | see "Relay suite" |
| Rule 20: nothing is drawn while nothing changes | read | no new timer or frame request: the notices are the status line's (C2 expires them); a drag redraws on its input events only | — |

#### Relay suite

The whole `harness` set on the pinned relay, through tmux, with the release
binary of this branch (checked: it holds `nothing to close`), then the same
on the release binary of the base `9902c74` (an export of the commit built in
`<scratch>`; its run lacks `t17-layout-*` and fails `t16-m5-…` at its first
step only because the export has no `.git`).

```sh
BZZ_BUZZ_SOURCE=<pin> BZZ_TMUX_SIZE=200x50 BZZ_TMUX_Text from tmux, in `<scratch>`, read as a newcomer would:

- `captures/t17-layout-widths/` (`bzz demo`): `02.txt` 60×20, the
  checklist's thread in the conversation's place, `channels 2 ·
  conversation 3` on its frame; `04.txt` 60×20, the conversation back with
  `hidden · channels 2 · thread 4`; `06.txt` 80×24, the thread at 42 beside a
  conversation of 38; `08.txt` `z`; `10.txt` 160×45, rail, sidebar,
  conversation and thread.
- `captures/t17-layout-mouse/` (`bzz demo`, 80×24): each gesture's screen.
- `mine-200x50/t17-layout-board-captures/` (`#t9-general` on the relay):
  `05.txt` 160×45 with a thread and the Board, `07.txt` 120×32 (the Board
  behind the thread, `app behind thread · 5`), `08.txt` 80×24, `09.txt`
  60×20 (the thread in the conversation's place), `10.txt` 60×20 the Board
  zoomed, `11.txt` 80×24 the Board docked beside 38 columns, `13.txt` 160×45
  sidebar, conversation, thread and Board. `12.txt`, named 120×32, was taken
  before the resize reached the pane and shows 80 columns.
- `mine-80x24/…`: the same at 80×24.

What a newcomer reads: at 80×24 and 60 columns the conversation is never
narrower than 38, and its top edge says what is hidden and which key brings
it (`hidden · channels 2`); at 160×45 everything fits and nothing is said.
The thread of the demo's code block wraps its long command outside the
code fence (`--version 2.4.0` under `│ harbor deploy`): markdown's own
wrapping, no character lost, left as it is.=<scratch>/mine-200x50 ./scripts/test-relay.sh --tmux
BZZ_BUZZ_SOURCE=<pin> BZZ_TMUX_SIZE=80x24  BZZ_TMUX_Text from tmux, in `<scratch>`, read as a newcomer would:

- `captures/t17-layout-widths/` (`bzz demo`): `02.txt` 60×20, the
  checklist's thread in the conversation's place, `channels 2 ·
  conversation 3` on its frame; `04.txt` 60×20, the conversation back with
  `hidden · channels 2 · thread 4`; `06.txt` 80×24, the thread at 42 beside a
  conversation of 38; `08.txt` `z`; `10.txt` 160×45, rail, sidebar,
  conversation and thread.
- `captures/t17-layout-mouse/` (`bzz demo`, 80×24): each gesture's screen.
- `mine-200x50/t17-layout-board-captures/` (`#t9-general` on the relay):
  `05.txt` 160×45 with a thread and the Board, `07.txt` 120×32 (the Board
  behind the thread, `app behind thread · 5`), `08.txt` 80×24, `09.txt`
  60×20 (the thread in the conversation's place), `10.txt` 60×20 the Board
  zoomed, `11.txt` 80×24 the Board docked beside 38 columns, `13.txt` 160×45
  sidebar, conversation, thread and Board. `12.txt`, named 120×32, was taken
  before the resize reached the pane and shows 80 columns.
- `mine-80x24/…`: the same at 80×24.

What a newcomer reads: at 80×24 and 60 columns the conversation is never
narrower than 38, and its top edge says what is hidden and which key brings
it (`hidden · channels 2`); at 160×45 everything fits and nothing is said.
The thread of the demo's code block wraps its long command outside the
code fence (`--version 2.4.0` under `│ harbor deploy`): markdown's own
wrapping, no character lost, left as it is.=<scratch>/mine-80x24  ./scripts/test-relay.sh --tmux
```

| Size | This branch | The base | Failing on this branch |
|---|---|---|---|
| 200×50 | **42 of 44** | 39 of 42 | `t17-palette-unavailable-stays`, `t17-palette-sizes`: both fail on the base too, the same way (the last message of `#t9-general` is by then the viewer's own share of the Board, so "Delete my message" is enabled) |
| 80×24 | **37 of 44** | 27 of 42 | `t15-approval-approve-refuse`, `t15-cli-agent-activity`, `t15-agent-new-form`, `t15-coding-harnesses-in-panel`, `t17-palette-unavailable-stays`, `t17-palette-sizes`, `t17-find-delete-unread-status`: every one fails on the base at 80×24 too (lines they look for are wrapped or cut at 80 columns, or the message they look for is off screen) |

So the suite is not green at 80×24 on the base, and this branch fails no
scenario the base passes, at either size. Seven that fail on the base at
80×24 pass here (`t9-artifacts-emoji-report`, `t9-presence-broker-huddle`,
`t14-edit-own-message`, `t14-mention-namesakes`, `t14-sections-and-solo`,
`t15-agent-answers-mention`, `t15-agents-panel-turn-stop-pause`): the base
drew rail 22 + sidebar 28 + a conversation of 30 at 80 columns (its capture:
`Generated fixture message 024:` wrapped over three rows of 28) and the line
they wait for was off screen; this branch draws sidebar 28 and a
conversation of 52 and names the rail as hidden.

This branch's own: `t17-layout-f7` (`sent to #t9-general as a new message ·
the thread's draft is cleared`; the message `↪ sent from thread` at the
bottom; `no huddle is running here`, never "only the huddle's creator"; no
`huddle-` in the sidebar after `t9-presence-broker-huddle` started and ended
one) and `t17-layout-board-captures` pass at both sizes; `t16-app-pane`
passes with its new widths. Two earlier runs of the 200×50 suite stopped
before any scenario: the harness fixture was refused (`rate-limited: shared
admission unavailable`, Redis timing out under the machine's load). One run
named "mine" ran the base's binary (cargo found this branch's release
"fresh" after the base was built into the same target); it is not counted.
Every run tore its harness down (`docker compose … down -v` in the log, no
`buzz-harness` container after).

#### The automated scenarios

`PYTHONDONTWRITEBYTECODE=1 BZZ_BIN=<scratch>/bzz BZZ_TMUX_SCENARIOS=automated python3 scripts/test-tui-tmux.py`
(debug build of the last commit): 10 of 11. `t17-demo-first-minute` waits
for `reaction · Enter toggle`, a title the base no longer draws either (task
`look` moved the keys to the footer: `┌ reaction ─`, `└ Enter toggle · :
custom · Esc close ─`); not this task's, left for its owner.

#### Captures

Text from tmux, in `<scratch>`, read as a newcomer would:

- `captures/t17-layout-widths/` (`bzz demo`): `02.txt` 60×20, the
  checklist's thread in the conversation's place, `channels 2 ·
  conversation 3` on its frame; `04.txt` 60×20, the conversation back with
  `hidden · channels 2 · thread 4`; `06.txt` 80×24, the thread at 42 beside a
  conversation of 38; `08.txt` `z`; `10.txt` 160×45, rail, sidebar,
  conversation and thread.
- `captures/t17-layout-mouse/` (`bzz demo`, 80×24): each gesture's screen.
- `mine-200x50/t17-layout-board-captures/` (`#t9-general` on the relay):
  `05.txt` 160×45 with a thread and the Board, `07.txt` 120×32 (the Board
  behind the thread, `app behind thread · 5`), `08.txt` 80×24, `09.txt`
  60×20 (the thread in the conversation's place), `10.txt` 60×20 the Board
  zoomed, `11.txt` 80×24 the Board docked beside 38 columns, `13.txt` 160×45
  sidebar, conversation, thread and Board. `12.txt`, named 120×32, was taken
  before the resize reached the pane and shows 80 columns.
- `mine-80x24/…`: the same at 80×24.

What a newcomer reads: at 80×24 and 60 columns the conversation is never
narrower than 38, and its top edge says what is hidden and which key brings
it (`hidden · channels 2`); at 160×45 everything fits and nothing is said.
The thread of the demo's code block wraps its long command outside the
code fence (`--version 2.4.0` under `│ harbor deploy`): markdown's own
wrapping, no character lost, left as it is.

#### Gates

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets --no-fail-fast` | pass on the last commit (every target). Two earlier runs, at load 17–24, failed `apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time` and `apps::runtime::tests::drawing::the_largest_screen_a_script_can_return_is_drawn_in_good_time` on their time limits; both pass alone and neither touches what this task changed |
| `cargo test --locked --no-default-features --lib` | pass |
| `cargo deny check`, `cargo audit` | not run: `Cargo.toml` and `Cargo.lock` unchanged |

The first full run also failed `app::shell::tests::goldens_of_the_main_screens_at_four_widths`:
under load the comparison caught the app's view of the previous size
(`room 40x16` in a pane of 40×18). The test now waits for the view of the
pane's own size, and the goldens were written again from it (`room 40x18`,
`40x26`, `40x39`); three runs in a row pass.

## E — Sound without a library (D3)

### v0.17.0 validation: sound (E1, decision D3)

Linux huddle devices through the PulseAudio protocol in pure Rust, so that
bzz names no sound library and starts on every Linux machine. Branch
`t3/sound`, cut from `v0.17.0` at `543dbd1`. Linux x86_64, 2026-11-24/25,
on a machine shared with six other builds. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every sound server below ran inside a disposable container with
`--network none`, as user 1000, with no host sound socket and no `/dev/snd`;
this machine's own sound was never played through, recorded from or
reconfigured.

**Builds.** "CI image" means bzz's release image (`bzz-build:smoke`, the
local build of `ci/linux-build/Dockerfile`: Ubuntu 22.04, glibc 2.35), with
the host's Rust 1.98.1 mounted: binaries built on the host need glibc 2.43
and do not run in the containers. The release binary is the `release`
profile as shipped (thin LTO, one codegen unit). The T7 test binary is the
`release` profile with LTO off and 16 codegen units (to rebuild it in about
three minutes while the buffers were tuned); a debug build of T7 was too
slow to measure underruns and its numbers were thrown away
(`<scratch>/tune2-debug-build.txt`).

#### The release binary names no sound library

**Claim.** The Linux release binary's `NEEDED` has no sound library, and
CI fails one that does. **Rung:** the artifact itself, and the check run
against a binary that has the defect.

```sh
scripts/check-linux-linkage.sh <scratch>/bin/bzz            # this branch, CI image
scripts/check-linux-linkage.sh <scratch>/v017-r6/bin/bzz-base/bzz   # 543dbd1's code, CI image (study R6)
```

| Binary | `NEEDED` | Exit |
|---|---|---|
| this branch | libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2 | 0 |
| before (same code as `543dbd1`) | **libasound.so.2** libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2 | 1, "bzz must not name a sound library; it names: libasound.so.2" |

The `build` job of `.github/workflows/ci.yml` runs the check on the Linux
release binary. Size: 31,701,352 bytes against 31,366,072, **+335 KB**
(+1.1 %).

`cargo tree --target <t> -i <crate> -e normal`: `pulseaudio` is in the graph
for x86_64 and aarch64 Linux only; `cpal` for aarch64-apple-darwin and
x86_64-pc-windows-msvc only; `alsa-sys` for none of the four.

#### bzz starts where there is no sound library

**Claim.** `bzz --version` and `bzz paths` work on clean images with no
sound library. **Rung:** the release binary in clean containers.

```sh
scripts/test-audio-containers.sh start <scratch>/bin/bzz
```

| Image | Sound libraries in it | This branch | Before |
|---|---|---|---|
| `debian:stable-slim` | 0 | `bzz 0.16.0`, paths all under the scratch home | `error while loading shared libraries: libasound.so.2` |
| `ubuntu:22.04` | 0 | the same | the same error |
| `ubuntu:24.04` | 0 | the same | the same error |
| `almalinux:9-minimal` | 0 | the same | the same error |

(The version still reads 0.16.0: the version bump is F8's.)

#### T7 on PipeWire and on PulseAudio, and the message with no server

**Claim.** `tests/huddle_device_loopback_test.rs` passes on PipeWire on
Debian, PipeWire on EL9 and PulseAudio, with the streams released on drop;
with no sound server, opening the devices answers at once with the one
line. **Rung:** the real device layer against real sound servers (null
sinks), in containers.

```sh
scripts/test-audio-containers.sh t7 <scratch>/bin/t7     # three passes
```

Three passes, all green (`<scratch>/t7-final.txt`):

| Server | Round trip (first tone frame) | Holes in 3 s of paced playout | Streams before / open / after drop |
|---|---|---|---|
| PipeWire 1.4.2, Debian (named null sink and monitor) | 16, 21, 15 ms | 1, 1, 1 | 0 / 2 / 0 |
| PipeWire 1.4.11, AlmaLinux 9 (named) | 21, 15, 20 ms | 0, 4, 2 | 0 / 2 / 0 |
| PulseAudio, private server as CI runs it (defaults) | 1362, 1008, 1088 ms | 0, 1, 0 | 0 / 2 / 0 |
| none (`debian:stable-slim`) | `Unsupported("huddle audio on Linux needs PipeWire or PulseAudio running in your session")` in 0.01 s | | |

The PulseAudio round trip is the container's null sink, whose clock runs
at about 36,700 samples a second instead of 48,000; study R6 measured the
same 1.35 s for v0.16.0's ALSA path there. It is not a bzz number.

A "hole" is a run of 1 ms or more of near silence inside the captured tone
while 10 ms chunks are fed on a 10 ms clock with `try_send`, from an empty
queue, as the engine plays out: an underrun. T7 prints it and does not
assert it.

#### Buffer tuning

**Claim.** The chosen buffers give the lowest round trip that does not
underrun at the engine's pace. **Rung:** T7 in the containers, three runs
per setting; `BZZ_PULSE_TUNE` and `BZZ_PULSE_CUSHION` were a temporary
override for this table only and are not in the code.

Playback as the study had it (silence handed to the server when the engine
was late), paced phase started with a full queue (`<scratch>/tune.txt`):

| target / request / fragment | PipeWire Debian | PipeWire EL9 |
|---|---|---|
| 20 / 10 / 10 ms (the study's) | 37–47 ms, holes 0, 2, 0 | 32–42 ms, holes 0, 1, 0 |
| 10 / 5 / 5 ms (the study's "18 ms tuned") | 17–21 ms, holes 5, 12, 33 | 18–21 ms, holes 24, 26, 7 |

Playback that waits for the engine (the source is pending until the next
chunk; this branch), paced phase from an empty queue (`<scratch>/tune5.txt`):

| target / request / fragment, cushion | PipeWire Debian | PipeWire EL9 | PulseAudio holes |
|---|---|---|---|
| **20 / 10 / 10 ms, 10 ms (chosen)** | **13–16 ms, holes 1, 0, 0** | **15–26 ms, holes 0, 0, 0** | 0, 1, 0 |
| 20 / 10 / 10 ms, none | 15–21 ms, holes 0, 1, 0 | 15–21 ms, holes 1, 1, 0 | 0, 2, 2 |
| 10 / 5 / 5 ms, none | 18–23 ms, holes 9, 6, 2 | 20–24 ms, holes 5, 9, 20 | 1, 2, 2 |
| 30 / 10 / 10 ms, none | 21–23 ms, holes 9, 4, 5 | 22–26 ms, holes 12, 11, 9 | 0, 0, 0 |

So: **13 to 26 ms** round trip through PipeWire, against 37 ms untuned in
the study, without the underruns that 10 / 5 / 5 ms costs. A larger target
(30 ms) underran more on PipeWire, not less; not investigated.

#### Unit tests

`cargo test --lib pulse_devices` (9 tests): the socket is looked for where
libpulse looks (`PULSE_RUNTIME_PATH/native` as CI sets it, `XDG_RUNTIME_DIR`,
`/run/user/<uid>`, the system socket); `PULSE_SERVER` is the only place when
set, with lists, machine-id prefixes and network entries skipped; names
match a server name or a label; **names saved by v0.16.0** (`default`,
`pipewire`, `pulse`, `sysdefault` → the default; `hw:CARD=Headset,DEV=0`,
`plughw:1,0`, `sysdefault:CARD=Headset`, `front:CARD=PCH,DEV=0` → the source
on that card, never its monitor; a card that is gone, a device it lacks or
an unknown card id → the default); the notice is told once a run; the
playout waits instead of playing silence and holds its cushion; the byte
framer cuts 960-sample frames from bytes of any size; with nowhere to
connect the one line comes back in under a second.

Not verified: the card mapping against a real card's properties
(`alsa.card`, `api.alsa.pcm.card`): the containers have no card, and this
machine's own server was not to be asked.

#### A call between two clients on the pinned relay

**Claim.** Two bzz clients on the pinned relay hold a huddle and the sound
crosses: what one microphone takes, the other plays. A v0.16.0 device name
in the configuration opens the default device and the join says so; with no
sound server, a join answers in one line and bzz keeps working. **Rung:**
the release binary (host build) in tmux, `t17-sound-call`, on the pinned
relay (`scripts/test-relay.sh --tmux`), each client with its own sound
server in a container.

```sh
# A: the viewer; its microphone is a PipeWire session playing 440 Hz.
# B: another person (profile other), a PipeWire session with a null sink.
scripts/test-audio-containers.sh serve <scratch>/a tone       # -> PULSE_SERVER for A
scripts/test-audio-containers.sh serve <scratch>/b pipewire   # -> PULSE_SERVER for B
scripts/test-audio-containers.sh record <scratch>/b 900 &     # what B plays
BZZ_TEST_PULSE_SERVER=unix:<scratch>/a/pulse/native \
BZZ_TEST_PULSE_SERVER_OTHER=unix:<scratch>/b/pulse/native \
BZZ_TMUX_SCENARIOS=t17-sound-call ./scripts/test-relay.sh --tmux
```

(`<scratch>/call.sh` does this and analyses the recording.)

- `t17-sound-call` passed in 13.4 s, 24 steps. The viewer saved
  `hw:CARD=PCH,DEV=0` with `:huddle input`, as v0.16.0 saved ALSA's names;
  the join opened the default microphone and the status line said
  `saved input "hw:CARD=PCH,DEV=0" not found; using the default · joined…`
  (capture `<scratch>/call-captures/t17-sound-call/07.txt`, 160×45).
- **The sound crossed.** B's sink played the tone from 0.1 s after B joined
  until it left: 6.0 s, 588 of the 598 10 ms windows with sound, 10 silent;
  the frequency by zero crossings in the 100 ms windows was 440 Hz in most
  and 260 to 510 Hz in all. Both clients' streams were on their servers
  while the call held and gone after (polled every half second with
  `pactl` in each container).
- Before this, three things were found and fixed in the scenario and the rig
  (runs kept in `<scratch>/call-run-{1..7}.log`): the viewer's "second"
  profile is the viewer again, so its leaving ended the huddle (the call is
  now with another person); after a join the huddle panel is open, so the
  leave is `l`; and PulseAudio's sine source in a container works in fixed
  2 s blocks (`pactl list sources`: "configured 2000000 usec"), which the
  call heard as bursts of 0.2 to 0.6 s every 2 s. The same 2 s fixed latency
  of PulseAudio's null sink there is the likelier cause of T7's 1.3 s round
  trip on PulseAudio than the slow clock study R6 supposed; not checked
  further.
- **No sound server** (`<scratch>/no-server-scenario.toml`, run with
  `BZZ_HERDR_MANIFEST` and `BZZ_TEST_PULSE_SERVER_OTHER` pointing at nothing;
  it is not in the shared manifest because the normal runs give every
  profile a server): the other person's `:huddle join` and `y` showed
  `huddle audio on Linux needs PipeWire or PulseAudio running in your
  session` in the status line at 160×45, Esc went back to the channel,
  `online`, and bzz quit cleanly. Passed in 6.4 s. **At 80×24 the line is
  cut** after "needs PipeWire or" (`<scratch>/call-captures-5/t17-sound-no-server/11.txt`):
  the status line gives the left part to the mode and the huddle.
- **The harness as anyone runs it** (no `BZZ_TEST_PULSE_SERVER`): the tmux
  runner served its own PulseAudio with a null sink in a container
  (`sound server: a container with a null sink (unix:/tmp/bzz-tmux.…/sound/native)`),
  `t9-presence-broker-huddle` (start, join muted, unmute, mute, leave) and
  `t17-sound-call` passed, 2 of 2, and the container was removed. No sound
  or relay container was left after any run.

#### Gates

On the host, at `23b2cc7` (before the status-line commit `184ac6e`; rerun
after it: GATES-AFTER):

| Gate | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --locked --all-targets -- -D warnings` | ok |
| the same with `--no-default-features` | ok |
| `cargo build --locked --example scripted_harness` | ok |
| `cargo test --locked --all-targets` | 64 test binaries, 1,585 passed, 0 failed |
| `cargo test --locked --no-default-features --lib` | ok |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo audit` | 5 allowed warnings (unmaintained: chrono, instant, zbus_names, zvariant, zvariant_derive), the same 5 as `543dbd1`'s lock file; none new |

Lock file: three crates added (`pulseaudio` 0.3.2, `enum-primitive-derive`
0.3.0, `futures` 0.3.33, pinned to the `futures-*` already locked). Adding
them made cargo re-pick `windows-sys` 0.61.2 for thirteen crates that
accept a range (`errno`, `rustix`, `tempfile`, `socket2`…); Windows only,
and deny and audit pass.

#### Not verified here

- macOS and Windows builds: the targets are not installed in the shared
  toolchain and were not added. The non-Linux code is today's, behind
  `cfg(not(target_os = "linux"))`, with one field added; CI on the pull
  request will tell.
- PipeWire in CI: the CI image has PulseAudio only, and product workflows
  install no packages (`tests/build_environment_test.rs`). The Linux audio
  job runs T7 on PulseAudio; PipeWire is proven here in containers.
- A machine with a real card and a desktop session, WSLg
  (`PULSE_SERVER=unix:/mnt/wslg/PulseServer`, covered by a unit test only),
  and a sound server restarted in the middle of a call.

## F — Defects and the release

### v0.17.0 — task `board`: F1, F2, F3, F4, F5

Linux x86_64, 2026-11-25, on branch `t3/implement-board` cut from
`v0.17.0` at `2ff8301`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md).
`<scratch>` is a folder outside the repository; its files are local.
Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.

The five defects were seen while filming v0.16.0 with several clients on
one relay (Buzz desktop 1.0.1 with a plugin, bzz, a third client in
Python), recorded in the experiments `buzz1/apps-interop-2026-11-24/` and
`promo-video/feature-apps/` (local, untracked).

"Red" below is the same test run on the code of `2ff8301`: the fix's
source files put back (`git stash push -- <files>`), the test kept.

#### F1 — The Board keeps a `null` field and an empty object

**Where the loss was.** In the bridge, not in the script. A record's JSON
becomes a Lua table (`value::to_lua`): a `null` is nil, so the field is
simply absent, and `{}` is an empty table, which `value::to_json` sends
back as `[]` (`docs/app-api.md` says so). The Board copies the content
with `pairs` and changes `column`; nothing it can write tells those values
apart. The same holds for a list with a `null` inside (a table with a hole,
sent as an object `{"1":…,"3":…}`) and a whole number past a Lua integer (a
float, `1.2345678901234567e+19`).

**What changed, and what a script sees.** Nothing a script sees: a null
still arrives as nil, an empty object as an empty table, a whole number
past 2^63 as a float, exactly as `api = 1` promises. What changed is what
an update writes. The session holds the record at the revision the update
names, and `value::as_read` puts back, against it, only what the script
could not tell apart and left as it was handed: a `null` it left out, an
empty table where the record held `{}` (or an object of nulls), the `null`
places of a list, a number equal to the one read. What the script changed
is written as it wrote it; two whole numbers are compared as whole
numbers, so `2^53` and `2^53 + 1` stay two. `docs/app-api.md` has a new
paragraph, "An update keeps what Lua cannot show". `docs/app-api.d.lua`
needed nothing: what it says of a null arriving as nil is still true.

| Claim | Proof | Red | Result |
|---|---|---|---|
| A card another client wrote with `null`, `{}`, nested nulls, lists with nulls, `[]`, `1.0`, 2^64-ish and seventeen digits comes back from the Board as written, but its `column`; and back again | `apps::session::tests::a_card_another_client_wrote_comes_back_from_the_board_as_it_was_read` (the Board fixture, in-memory store) | `"gone"` and `"nested.b"` dropped, `"meta": []`, `"list": {"1":1,"3":3}`, `"big": 1.2345678901234567e+19` | pass |
| What a script changes is written as it wrote it (a null field it sets, `{}` it makes a list, 2^53+1, a float) | `a_change_of_the_script_is_written_as_it_wrote_it` | `"keep": null` dropped | pass |
| The function alone, both ways | `apps::runtime::value::tests::an_update_keeps_what_lua_could_not_show`, `an_update_writes_what_the_script_changed` | (new function) | pass |
| On the pinned relay, a card a second client wrote with a null field, an empty object, a list with a null, a 20-digit whole number and a 17-digit `order` comes back from the Board byte for byte, but `column` | `real_relay_v017_the_board_keeps_fields_its_own_burst_deletions_and_new_names`, part F1: the head event's content after Ana's move equals Ben's with `todo` → `doing` | not run red on the relay | pass |

The six fixtures and the three templates pass their cases unchanged:
`cargo test --lib apps::` (274 passed) and the apps bench below.

#### F2 — The Board's own writes do not conflict

**Where it belongs: the session.** A script cannot know the revision its
own write will make before the reply, and every app would have to queue
its own writes to stay out of this. The service is shared with the
agents' tools, which answer per call. The session already holds the set
and the budget, so the rule is there: an app's writes to one record are
sent one after the other; a write built on a revision that the app's own
earlier write replaced is sent on the app's own new revision. A revision
of someone else is never put in its place, so a real conflict is still a
conflict. Each write is spent from the budget of 30 a minute when it is
sent; past it the script gets `rate_limited`, before anything is sent.

The Board also leaves a card as shown while a later move of its own to
that card is on its way, so the card does not flick back to an earlier
column between the answers, and says `Saving...` until the last of them
is answered (it said `Saved.` at the first). `docs/app-api.md` has a new
point under the record forms: an app's own writes to one record do not
conflict.

**Found on the way, in the records service.** The service took the
sync's echo of the caller's own revision as a new head when it arrived two
or more revisions late (it caught one revision late only), and the
session would then tell the app of its own old revision as a change. In a
burst this can happen. `State::apply` now leaves alone a revision that the
caller's own writes, followed back from the head it holds, replaced;
another client's older revision is told as before
(`service::records::tests::an_echo_of_a_revision_the_callers_own_writes_replaced_is_no_change`,
red without the three lines). This was found by reading while a relay run
failed for another reason (the test's own wait, see below); it was not
seen to happen on the relay.

| Claim | Proof | Red | Result |
|---|---|---|---|
| 25 moves of one card before any reply: one write on its way at a time, 25 writes, none refused, none sent again, the card where the last key put it all along | `the_boards_own_moves_do_not_conflict_when_keys_come_faster_than_replies` | 25 writes on their way at once | pass |
| A real conflict with another client is still refused and merged; nothing lost | `a_real_conflict_with_another_client_is_still_refused_in_a_burst` | 30 writes: the stale moves conflicted until the budget ran out | pass |
| Queued moves still spend the budget: 31 moves, 30 sent, the 31st "Too many changes. Retry in 60 s." | `queued_moves_still_spend_the_apps_budget` | more than one write on its way | pass |
| On the pinned relay: 25 moves while no reply has come; no status but `Saving...` and `Saved.`; the app's budget spent by 27 writes in all (2 before, the 25: a refused move would have cost its write and its merge another); 27 revisions of Ana's in the relay's history; the head byte for byte as Ben wrote it but `column` | relay test, part F2 | not run red on the relay | pass |
| A real conflict on the relay between two profiles still merges | `real_relay_app_session_two_profiles_share_a_board` (v0.16 M2, unchanged): five simultaneous moves, merged by ana, ana, ben, ana, ben | — | pass |

#### F3 — "Save failed." and "Loaded N cards."

| Claim | Proof | Red | Result |
|---|---|---|---|
| A move of a card deleted meanwhile says `This card was deleted by cy.` and drops the card, whether the deletion or the refusal reaches the Board first (until bzz says who: `This card was deleted meanwhile.`) | `a_move_of_a_card_deleted_meanwhile_says_who_deleted_it` | `Save failed.` | pass |
| `Loaded N cards.` counts the cards shown: a list asked for before a deletion, and a card of a column the Board does not show, are not counted | `the_board_counts_the_cards_it_shows` | `Loaded 5 cards.` with four shown | pass |
| On the pinned relay: Ben deletes a card as Ana moves it: `This card was deleted by ben.`, the card gone; `r` then says `Loaded 1 cards.` with one shown | relay test, part F3 | — | pass |

The bench's specs (`tests/apps-bench/spec/board.md`,
`board-priority.md`) say the new texts. The Board with priority counts
the cards on the board whatever its priority filter.

#### F4 — A number reaches the agent as the relay holds it

**Where the loss was.** `serde_json` parses floats with a fast algorithm
that is off by one unit in the last place for some numbers of seventeen
significant digits, unless its feature `float_roundtrip` is on: measured
on serde_json 1.0.151, `1795537221.1991239` parses to the double printed
`1795537221.199124`; with the feature, to the double printed as the text
it came from. bzz parsed the relay's content and printed the parsed
double again, in the records tool and in the app runtime alike (the app
path is the same parse, then Lua's exact double, then serde_json's
shortest printing). The feature is turned on in `Cargo.toml`. No new
crate: `Cargo.lock` did not change (`--locked` builds pass).

| Claim | Proof | Red | Result |
|---|---|---|---|
| An `order` of seventeen digits reaches the agent through `records` as the relay holds it, and goes back unchanged through `update_record` | `tests/agent_records_test.rs` `a_number_reaches_the_agent_and_the_relay_as_the_relay_holds_it` (the scripted harness, bzz's own host, a scripted relay of records) | the tool answered `1795537221.199124` | pass |
| The app runtime's path: relay text → Lua → update keeps `1795537221.1991239`, a 20-digit whole number and `5e-324` as text | `apps::runtime::value::tests::a_number_crosses_the_script_as_the_relay_holds_it` | not run red | pass |
| On the pinned relay, Ben's 17-digit `order` is back byte for byte after Ana's Board moved the card | relay test, part F1 | — | pass |

What it does not do: a number with more digits than a double holds is
still rounded to the nearest double. A text that is not the shortest form
of its double (`1.50`) comes back as the shortest (`1.5`).

#### F5 — bzz refreshes a profile name it holds

**Why the newer profile did not replace the held one.** Not the timestamp
comparison: the store keeps the newest by `(created_at, event_id)`. bzz
asked the relay for a profile only for a person it held none of (the
timeline's authors, an app's signers), and none of its subscriptions
carried kind 0. So the first profile it got was the one it kept, also
after a restart, unless that person happened to write in the channel bzz
opened on. An open app was given only the names it had none of, so a new
name never reached it either.

What changed: the global stream carries kind 0 (a profile changed while
bzz runs arrives as any event and replaces the held one); each person
shown is asked for once a run, held or not (a profile changed while bzz
was closed); an open app is given the names of everyone it shows
(`AppSession::people`), and `set_names` passes on only what changed.

| Claim | Proof | Red | Result |
|---|---|---|---|
| The global stream asks for kind 0 | `tests/session_test.rs` `global_aux_and_membership_subscriptions_match_buzz_routing` | fails at the new assertion | pass |
| A renamed profile reaches an open app (`ctx.user.name` from Watcher to Cy) | `app::pane::tests::a_renamed_profile_reaches_an_open_app` | `me Watcher` never shown: the app was never given a name for someone it had one for | pass |
| On the pinned relay: Ana holds Ben's profile "Watcher"; Ben renames himself "Cy"; the global stream brings it (118 ms after it was sent) and the store then shows "Cy"; a profile renamed again is fetched for a held person ("Cy 2") | relay test, part F5 | — | pass |

Not shown at this rung: the timeline of a running bzz redrawing the new
name. The live event takes the path every event of the global stream
takes (applied to the store, `cache_dirty`, profiles read again); no
scenario of the TUI was added for it.

#### Relay

```sh
ln -sfn "$CARGO_TARGET_DIR" target; unset CARGO_TARGET_DIR
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin BZZ_RELAY_NOCAPTURE=1 \
  BZZ_RELAY_TEST_FILTER=real_relay_v017 ./scripts/test-relay.sh
BZZ_BUZZ_SOURCE=… BZZ_RELAY_TEST_FILTER=real_relay_app_session_two_profiles_share_a_board \
  ./scripts/test-relay.sh
```

Both green, each on a harness of its own (the lock taken, the containers
and volumes removed at the end, in the logs):

```
F1: Ben's card after Ana's Board moved it: {"big":12345678901234567890,"column":"doing","gone":null,"list":[1,null,3],"meta":{},"nested":{"a":1,"b":null},"order":1795537221.1991239,"title":"Odd"}
F2: 25 moves answered in 4340 ms; status lines seen: {"Saved.", "Saving..."}
F2: Ana's Board sent 27 writes in all; the relay's history holds 28 revisions, 27 of Ana's
F5: Ben's new profile reached Ana's global stream 118 ms after it was sent
test result: ok. 1 passed; 0 failed
the five simultaneous moves were merged by ["ana", "ana", "ben", "ana", "ben"]
test result: ok. 1 passed; 0 failed
```

The 4.3 s of the burst include one second the test waits after it; one
write at a time means 25 moves take 25 round trips (about 130 ms each on
this machine, at a load of about 20). The first runs of the test failed
on the test itself: it waited for `Saved.`, which the screen said already
before the keys were handled, and read the relay while the burst was
going on.

#### The apps bench and the hostile suite

```sh
APPS_BENCH_OUT=<scratch>/bench-out ./scripts/test-apps-bench.sh
```

Debug build: every case of the six fixtures at 80×24 and 40×16 (board
12/12, poll 12/12, rota 14/14, approvals 15/15, panel 15/15,
board-priority 17/17), reload 20/20 at both sizes, pushes 8/8, the
hostile script 29/29 (rule 15). `PASS: everything passed.`

#### Gates

All green on the last commit: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets --no-fail-fast` (64
test binaries ok), `cargo test --locked --no-default-features --lib`,
`cargo deny check` (advisories, bans, licenses, sources ok) and `cargo
audit` (exit 0, the allowed warnings it had before). `Cargo.lock` is
unchanged. One earlier run of the whole suite failed once on
`apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time` (a loop of
100 000 steps stopped by its 250 ms at a load of 23); it passed three
times alone and in the last whole run.

#### Captures

`bzz app try <scratch>/home/tasks --seed <scratch>/home/seed.json --size
80x24` and `--size 160x45`, the Board template on a scratch home, with
three cards in the columns, one with `"gone": null` and `"meta": {}`, and
one in a column the Board does not show: both say `Loaded 3 cards.`
(`<scratch>/capture-board-80x24.txt`, `capture-board-160x45.txt`).

### v0.17.0 — task `tidy` (loose ends of the reviews)

Branch `t3/v017-tidy`, cut from `v0.17.0` at `24f7486`. Linux x86_64,
2026-11-25, on a machine shared with other builds (16 cores, load 9 to 32
during the runs). `<scratch>` is a folder outside the repository.

#### Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **1.** In `bzz demo`, when the Board closes on its own `q` (`host.quit()`), the status line says `Demo: :app open board`; outside the demo an app of that name closes with no such word | app buffer test | `cargo test --lib app::pane::tests::the_demo_says_how_to_bring_back_a_board_that_closed_itself` | red without the notice (`left: None`), green with it |
| 1. The notice fits the demo's 24 columns beside the mark and the hint at 80 columns | unit | `cargo test --lib demo::tests::every_notice_of_the_demo_fits_an_80_column_status_line` | pass |
| 1. Ada's line beside the Board says `Ctrl-c` steps out of it and `q` closes it | unit | `cargo test --lib demo::tests::board::the_welcome_names_the_board_its_keys_and_the_words_for_scout` | pass |
| 1. On the real binary: `#launch`, `5`, `q`: the Board closes, the status line reads ` NORMAL  #launch  Demo: :app open board … DEMO · generated, offline`; `:app open board` brings it back; `q`, `y` leaves the demo and no `bzz-demo-*` folder is left | tmux, debug build | by hand on a private tmux socket, 80×24 then 160×45 | `<scratch>/captures/demo-80x24-launch.txt`, `…-board-closed.txt`, `…-board-reopened.txt`, `demo-160x45-board-closed.txt` |
| **2.** `:huddle devices` with two empty lists says why: on Linux the join's line, elsewhere "no microphone or speaker found"; with devices it lists them and says nothing of the kind | buffer test | `cargo test --lib ui::huddle::tests::devices_with_none_found_say_why` | red without the line, green with it |
| 2. Two empty lists mean no sound server answered: a server with no card still lists an output | container | `pactl list short sinks` in `bzz-audio-pipewire-debian`, `…-pipewire-el9` (PipeWire 1.4.2, 1.4.11) and `bzz-audio-pulseaudio` | `auto_null` in all three, and its monitor as the only source |
| **3.** The join's answer with no sound server is shown whole in the status line at 80 columns, with the mode, `#general`, a huddle strip of 12 people, and the connection as `online`, `authenticating` or `identity missing` | buffer test | `cargo test --lib huddle::audio::pulse_devices::tests::no_sound_server_fits_an_80_column_status_line` | red with the old 76-column sentence, green with `huddle audio needs PipeWire or PulseAudio running` (49) |
| **4.** Five tests no longer fail under load for reasons outside bzz, and prove what they proved | 30 runs in a row each, before and after, while a `cargo build --release` of bzz (own target directory, 8 jobs) loads the machine | `<scratch>/runs.sh` | see "Item 4" |
| **5.** `docs/themes.md` names every highlight group and no other; a group added to the code and not to the document fails | unit | `cargo test --lib ui::theme::tests::the_themes_document_names_every_group` | red on the document as it was (lacks the 9 groups of v0.17.0 and the 5 `Media*` groups), green after |
| **6.** What `site/public/styles.css` would need | read | `src/ui/theme/builtin.rs`, `src/ui/theme/mod.rs`, `site/public/styles.css` | see "Item 6"; nothing under `site/` changed |
| **7.** `bzz help` and `bzz --help` list `demo` first; `bzz demo --help` says in two lines, each under 80 columns, what it is, that nothing is read or saved, and how to leave (`q`, then `y`) | binary test | `cargo test --test newcomer_help_test` | red at the base (demo last, no way to leave), green after; `q`, `y` seen to leave the demo (above) |
| **8.** The completion scripts of bash, elvish, fish, PowerShell and zsh offer `demo` | binary test + script | `cargo test --test newcomer_help_test every_shells_completion_offers_the_demo`; `rustc scripts/generate-completions.rs` then `BZZ_BIN=…/debug/bzz <scratch>/gen-completions <scratch>/completions` | pass (was already true: a check, not a fix); `demo` in all five files |

#### Item 4

Each test was run thirty times in a row as its own process, the binary of
the base and the changed one taking turns, while a release build of bzz in
a target directory of its own (8 jobs, rebuilt from scratch in a loop) and
the other agents' builds loaded the machine. The load average at each run
was 9.4 to 32.0, 15 to 16 on average.

| Test | Why it was timing-sensitive | Change | Failures before | After |
|---|---|---|---|---|
| `apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time` | The script that must end in time (`for i = 1, 100000`) had the 50 ms budget meant for the endless ones; its thread can wait past 50 ms for a core | It runs on an interpreter given the budget and `hostile::margin()` (`BZZ_APPS_STOP_MARGIN_MS`, 800 ms by default). The stop and the endless scripts are unchanged | 0 of 30; **9 of 30** with 32 busy loops beside it (load 18), the failure the gates saw: `Err(Stopped { at: Some("test.lua:1") })` | 0 of 30; 0 of 30 with the busy loops (load 25) |
| `agent_runtime_test::the_owner_stops_a_running_turn_and_it_ends_once` | A harness that honours the cancel had the test's 400 ms `cancel_grace` to answer; a loaded one did not, and bzz stopped it as deaf (`its process stays: Idle`) | `cancel_grace` 3 s for this test, and the test now looks once that grace is over that the process is still there (before, it looked at once, before the grace could end). The deaf harness is still stopped after the grace | **1 of 30** (`left: Idle`, the failure the gates saw); 0 of 8 with busy loops | 0 of 30; 0 of 8 with busy loops (load 49) |
| `agent_runtime_test::a_pause_in_another_process_stops_a_running_agent_and_a_resume_runs_it_again` | bzz writes the reason (`paused by its owner, with bzz agent pause`) after the process is stopped; the test saw the process gone and read the status before the reason was there | The test waits for the reason | 0 of 30; **2 of 8** with busy loops (`Scout · pi-durable · paused`, no reason) | 0 of 30; 0 of 8 with busy loops (load 51) |
| `agent_host_test::the_real_host_is_installed_started_and_answers_once_across_kills_and_restarts` | The host was killed 1.5 s after the turn was listed, inside a 4 s sleep of the stand-in model: a loaded machine could stretch the wait past the sleep | The turn runs `echo before the kill` first, and the kill comes when the status shows that tool call of this turn. (A first try that waited for any tool call of a listed turn failed 2 of 3 with `(Replied, 1)`: the kill came before this request was handed over. The likeliest reading, not traced: the turn before was still listed a moment, with its own tool call) | 0 of 30 | 0 of 30 |
| `agent_host_test::the_real_host_checkpoints_its_files_when_it_has_been_idle` | "Not before the time": looked at after a fixed 2 s, which a loaded machine can make later than the 4 s idle time. "Nothing while a turn runs": looked at once after 6 s of an 8 s turn, which a loaded machine can stretch past the turn's end | The first check counts from when the test looked (with the 1 s the test already allows); the second is looked at every 50 ms for as long as the turn runs, and the turn must have run past the idle time | 0 of 30 | 0 of 30 |

The two `agent_host_test` tests that failed once on 2026-11-25 are not
named in the gates' logs; these two are the ones with fixed waits. The
other five of that file wait for events already.

Seen in the gates of this branch, not changed: `apps::runtime::tests::
hostile::every_hostile_script_is_stopped_in_budget` failed once at load 24
(`scale-rep` ended after 1.31 s against 1 s, three tries). It already reads
`BZZ_APPS_STOP_MARGIN_MS`; a loaded machine needs it set, as CI's build job
does (3000).

#### Item 6

`site/public/styles.css` mirrors the palettes of `src/ui/theme/builtin.rs`
as eleven tokens per theme. Agent `look` did not change `builtin.rs` (its
last change is `15f7aca`); it changed the default groups in
`src/ui/theme/mod.rs` (`0fff6c1`), which the `bzz` theme uses as they are
(`from_palette` applies no palette to `bzz`):

- `FocusBorder` went from no colour to ANSI cyan; `FocusedPaneTitle` is
  that colour reversed and bold. `PaneBorder` and `PaneTitle` link to
  `Muted` (dark grey) instead of `Border` and `Title`.
- `StatusBar` lost its dark grey background and white text (terminal
  default now); `StatusMode` is black on cyan, `StatusModeInsert` and
  `StatusModeCommand` black on yellow (all three were white on dark grey).
- New: `HintKey` cyan bold, `HintText` dark grey, `StatusChannel` dark grey,
  `StatusOnline` green, `StatusOffline` yellow, `StateBanner` black on
  yellow bold, `NewMessageMarker` yellow, `UnreadCount` yellow bold.

So the site's eleven tokens need no change: none of them is a highlight
group, and the palettes they copy did not move. If the site is to show the
new look too, it would need, for `[data-theme="bzz"]` (and the same rule for
the others, from each palette): a focus token equal to `--primary`
(`#5ec8d8`), used where the site now marks the active or hovered element
with `--accent` (`.install-line button:hover`, `.hero .cta a.primary`,
`.hero .cta a:hover`, `.card:hover`, `.card.feature`); frames at rest in
`--muted` (`#8a8272`) rather than `--border` where they stand for panes;
and a status-bar look of mode label `--bg` on `--primary`. None of this is
a token the TUI defines; it is a choice for the owner.

Found while comparing, older than v0.17.0: three site themes use a yellow
`--accent` where the palette's `accent` is green: `gruvbox-dark` (`#fabd2f`
against `#B8BB26`), `catppuccin-mocha` (`#f9e2af` against `#A6E3A1`) and
`tokyo-night` (`#e0af68` against `#9ECE6A`). Every `--selection` is a
colour where the palette leaves `selection_background` empty (the TUI then
uses `primary`). The `bzz` tokens are hex values for ANSI names
(`cyan` → `#5ec8d8`, `yellow` → `#ffb000`), as before.

#### Gates

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | first run: 1 failure, `hostile::every_hostile_script_is_stopped_in_budget` at load 24 (see "Item 4"); second run (`--no-fail-fast`): pass, 0 failed in 68 targets |
| `cargo test --locked --no-default-features --lib` | pass, 646 tests |

`Cargo.toml` and `Cargo.lock` did not change: deny and audit were not run.

### v0.17.0 — task `ci` (CI green and quick)

Branch `t3/ci-pipeline`, cut from `v0.17.0` at `12b355b`. Linux x86_64,
2026-11-25, on a machine shared with other builds (16 cores). `<scratch>` is
a folder outside the repository. Every sound server below ran inside a
container (`--network none`, no host sound socket, no `/dev/snd`); this
machine's own sound was never used. CI was only read (`gh run view`,
`gh api …/logs`); nothing was started, re-run or cancelled.

"The CI image" is `ghcr.io/arpagon/bzz-build@sha256:e708b63b…`, the digest
in `.github/workflows/ci.yml`, pulled and run here. Test binaries were built
inside it (Rust 1.98.1 of this machine mounted read-only, `cargo test`'s
debug profile as CI builds it) and the T7 step was run in it as the job
runs it: as root, `HOME=/github/home`, `scripts/test-audio-loopback.sh`
starting its private PulseAudio (15.99.1).

#### Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **1.** The defect reproduces in the CI image: T7 prints its round trip, then does not end | the CI image, T7 as CI runs it | `<scratch>/run-t7-ci.sh <scratch>/t7-debug-before` (the test binary of `12b355b`) | `T7 round trip: first tone frame after 1342 ms`, then nothing; still running when killed at 960 s (`<scratch>/repro-before-1.txt`). In CI: 731 s at `e144e52`, over two hours at `24f7486`, `006d8d1`, `0d96e80` |
| 1. Where it waits: the playback device stops asking for audio because the `pulseaudio` crate's reader never lets its writer run | stack samples | the same binary under `gdb` on this machine, as its parent, against a PulseAudio in a container (`scripts/test-audio-containers.sh serve`), 8 interrupts 2 s apart | the reactor thread in `Vec<u8>::resize` / `extend_with` (from `Reactor::recv`) in 7 of 8 samples; the test's feeder parked on the full playback channel (`<scratch>/gdb2.txt`) |
| 1. Why that is slow: before every read of the socket the crate grows its buffer by 1 MiB of zeros; unoptimised that is a loop of a million steps | measured | `<scratch>/resize.rs` built at `-C opt-level=0` and `3` | **3.924 ms** a read at opt-level 0, **0.018 ms** at 3. Its `run` loop reads until a read finds nothing waiting and only then writes playback; capture arrives every few milliseconds, so that moment almost never comes |
| 1. The crate optimised alone ends the hang | the CI image | test binary of `12b355b` built with `--config 'profile.dev.package.pulseaudio.opt-level=3'` | 7.48 s, `0 holes in 2999 ms of tone`, streams 0 / 2 / 0 (round trip still 1381 ms: claim 3) |
| **2.** Every wait in T7 has a bound and says what it waited for; the old defect now fails in seconds | the CI image | this branch's test, built *without* the optimised crate | fails after 10.0 s: `T7 waited more than 8s for the playback device to take 2 s of tone (it stopped asking for audio)` |
| **3.** The 1.4 s round trip in CI (2 s is its bound) was PulseAudio's idle null sink, not bzz: the sink sleeps up to 2 s, its latency while no stream asks for less, and gives its monitor nothing until it wakes | the CI image | this branch's settle step prints what the capture gave while it waited | `capture live after 2226 ms (260 ms of audio in that time)`; PipeWire: `live after 303 ms (300 ms of audio)`. Also tried: a `parec` kept on the monitor 2.5 s before T7 gave 20 ms (`<scratch>/scripts-keep/`); a minimal server config (`-n`) and a suspended sink did not (1383 ms, 1404 ms) |
| 3. With the settle step the round trip reads what bzz does | the CI image | as below | 19 to 20 ms |
| **4.** T7 takes well under a minute in the CI image, ten times in a row | the CI image | `for i in $(seq 10); do <scratch>/run-t7-ci.sh <scratch>/t7-after-paO3; done` | 10 of 10 pass; see "T7, ten runs" |
| 4. The script's own path (cargo inside the image, the `--config` line) builds that same binary and passes | the CI image | `./scripts/test-audio-loopback.sh` in the image as uid 1000, toolchain mounted | `T7 took 6878 ms`, same test binary hash as the runs above |
| 4. With two cores, as a hosted runner has about | the CI image, `docker run --cpus 2` | the same, three times | 8.09 to 8.21 s, round trip 20 ms, 0 holes |
| **5.** T7 on PipeWire (Debian, EL9) and PulseAudio in containers, and the one line with no server | containers | `scripts/test-audio-containers.sh t7 <scratch>/t7-after-paO3`, three times | see "T7 in containers" |
| **6.** The audio job and its T7 step have bounds | read + lint | `.github/workflows/ci.yml`; `actionlint` (`rhysd/actionlint` image, with shellcheck) | `timeout-minutes: 30` on the job, `15` on the step; actionlint: no findings |
| **7.** The agent-host test client gives a slow runner room, from an environment variable CI sets, and a hung host still fails | node, this machine (Node 26.5) | `BZZ_AGENT_HOST_WAIT_MARGIN_MS=60000 node --test test/packages.test.mjs`; a scratch test whose host logs a line and never answers, with the margin at 2000 | 7 of 7 pass; the hung host fails after **32003 ms** with `timed out after 32000 ms: initialize (the host's log so far follows)\nhost started, then hangs` |
| 7. A timeout shows the host's log at the time it ran out; before, the log as it was when the request was sent | read | `agent-host/test/support/client.mjs` at `12b355b`: the message was a template string built when `rpc` was called | in both CI failures the message ends right after `initialize`: the log was read at 0 s, empty |
| 7. The whole suite still passes with no margin | node, this machine | `npm test` in `agent-host/` (the main checkout's `node_modules`, same lock file, linked for the run and removed) | 100 of 100 pass, 66 s |
| **8.** A pull request's superseded run is cancelled; a run on `main`, a tag or by hand is not | read + lint | `concurrency` in `ci.yml` and `integration.yml`; `actionlint`; `cargo test --test build_environment_test` (in the gates) | the group is `ci-pr-<number>` / `relay-pr-<number>` on a pull request, with `cancel-in-progress` true only there; any other event's group is its own run id, so it is neither cancelled nor queued behind another. Not proven on GitHub (see "Not verified") |
| **9.** The gates | gates | the five commands of the rules | see "Gates" |

#### T7, ten runs in the CI image

This branch, `<scratch>/t7-ci-image-10x.txt`:

| Run | Capture live after | Round trip | Holes in 3 s paced | Streams | T7 took |
|---|---|---|---|---|---|
| 1 | 2226 ms | 20 ms | 0 | 0 / 2 / 0 | 8.07 s |
| 2 | 2225 ms | 20 ms | 0 | 0 / 2 / 0 | 8.08 s |
| 3 | 2225 ms | 19 ms | 0 | 0 / 2 / 0 | 8.08 s |
| 4 | 2326 ms | 20 ms | 0 | 0 / 2 / 0 | 8.19 s |
| 5 | 2327 ms | 20 ms | 0 | 0 / 2 / 0 | 8.16 s |
| 6 | 2326 ms | 20 ms | 0 | 0 / 2 / 0 | 8.17 s |
| 7 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.18 s |
| 8 | 2225 ms | 20 ms | 0 | 0 / 2 / 0 | 8.06 s |
| 9 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.19 s |
| 10 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.15 s |

Before and after, the same step in the same image:

| | T7 | Round trip | Paced holes |
|---|---|---|---|
| `12b355b` (CI, `e144e52`) | 731 s, or never ends | 1342 to 1400 ms | 3 |
| `12b355b`, crate optimised only | 7.5 s | 1381 ms | 0 |
| this branch | 8.1 s | 20 ms | 0 |

#### T7 in containers

`scripts/test-audio-containers.sh t7`, three times (`<scratch>/t7-containers-3x.txt`):

| Server | Live after | Round trip | Holes | Streams | T7 took |
|---|---|---|---|---|---|
| PipeWire 1.4.2, Debian | 302–303 ms | 21, 21, 21 ms | 0, 1, 0 | 0 / 2 / 0 | 6.18–6.22 s |
| PipeWire 1.4.11, EL9 | 302–303 ms | 21, 26, 21 ms | 0, 0, 0 | 0 / 2 / 0 | 6.18–6.22 s |
| PulseAudio, Debian (as CI starts it) | 2224–2328 ms | 20, 20, 20 ms | 0, 0, 0 | 0 / 2 / 0 | 8.06–8.18 s |
| none | `huddle audio needs PipeWire or PulseAudio running`, 0.00 s | | | | |

#### The agent host on the Intel Macs

CI keeps no time for `initialize` alone. The failing test ("what a package
leaves uncaught…", test 54) is one `initialize` that loads two packages,
then a wait of at most 5 s for a line of the log, so its time is an upper
bound of that `initialize`. Every run of the job on this branch:

| Commit (run) | macos-15-intel: test 54 | its `--check-model` test | suite | ubuntu: test 54 | windows: test 54 |
|---|---|---|---|---|---|
| `543dbd1` (45530273900), macos-15 Apple | 0.8 s | 2.8 s | 80.8 s | 0.9 s | 1.1 s |
| `2ff8301` (45540182641) | 2.5 s | 9.5 s | 101.6 s | 0.9 s | 1.7 s |
| `bfd340f` (45555775783) | 2.2 s | 12.8 s | 111.1 s | 1.5 s | 1.9 s |
| `64156d3` (45560292857) | 31.9 s **failed** | 8.1 s | 110.1 s | 1.1 s | 1.7 s |
| `18c4eb9` (45571201773) | 2.3 s | 8.6 s | 88.9 s | 1.1 s | 1.5 s |
| `9902c74` (45571866191) | 2.1 s | 9.8 s | 97.6 s | 1.0 s | 1.9 s |
| `e144e52` (45577698807) | 5.9 s | 16.6 s | 133.3 s | 1.4 s | 1.8 s |
| `5859f29` (45593677292) | 3.2 s | 10.9 s | 102.4 s | 1.5 s | 1.7 s |
| `24f7486` (45593885608) | 3.9 s | 16.7 s | 119.8 s | 1.3 s | 2.1 s |
| `006d8d1` (45601454766) | 1.4 s | 6.8 s | 80.7 s | 1.3 s | 2.0 s |
| `0d96e80` (45609541408) | 32.7 s **failed** | 14.9 s | 139.8 s | 1.4 s | 1.8 s |
| `12b355b` (45614530167) | 6.5 s | 31.0 s | 170.0 s | 1.0 s | 1.9 s |
| `6fcd428` (45617462153) | 2.6 s | 10.5 s | 104.6 s | 1.1 s | 1.5 s |

So on the Intel Macs the same test takes 2 to 6 times as long as on Linux,
the slowest passing runs are several times the usual (`--check-model`, about
a dozen starts of the host, took 31 s once against 5 s on Linux), and twice in twelve
`initialize` passed 30 s. Whether those two would have answered at 35 s or
never cannot be read from the logs: the message carried the log as it was
at 0 s. From now on it carries the log at the timeout, and an `initialize`
of 5 s or more is written to the test's output.

The bound chosen: `BZZ_AGENT_HOST_WAIT_MARGIN_MS`, added to the client's own
waits (a request's answer, 30 s; the host's exit, 5 s; `--check-model`,
30 s). CI sets 60000 on `macos-15-intel` (so 90 s for `initialize`, three
times the Linux bound, against a worst seen pass of about 6 s for this
test), 30000 on Windows (its one failure, `6fcd428`, was the 5 s wait for
the host's exit in a run where another test took 21 s), 0 on Linux. Bounds
a test states as claims (`stop(2_000)`: "the host leaves within 2 s") get
no margin.

#### Gates

| Command | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | pass (562 s), `build_environment_test` 2 of 2 |
| `cargo test --locked --no-default-features --lib` | pass (710 tests) |

`Cargo.toml` and `Cargo.lock` did not change: `cargo deny` and `cargo audit`
were not needed.

#### Not verified

- The fixes on GitHub's runners: nothing here pushes. The audio job's time
  in CI after the push, the agent-host job on `macos-15-intel`, and the
  `concurrency` groups (a pull request's earlier run cancelled, `main`
  untouched) are shown only once the principal pushes.
- T7 on macOS (BlackHole) and Windows with the settle step: the step only
  waits for audio at the pace of the clock, which BlackHole and WASAPI give
  from the start; not run here.
- Whether the two `initialize` timeouts on the Intel Macs were slowness or
  a hang: the logs of those runs cannot tell (above).
- `cargo run` of bzz itself in a debug build still uses the unoptimised
  crate: a developer's debug huddle on PulseAudio or PipeWire can starve
  its playback as T7 did. Only `Cargo.toml` (`[profile.dev.package.pulseaudio]
  opt-level = 3`) fixes that for every debug build; it is not this task's
  file. Release builds, which people install, read in 0.018 ms and are not
  affected.

### v0.17.0 — task `release`: the upgrade from v0.16.0, the version (F8)

Linux x86_64, 2026-11-25, branch `t3/release` cut from `v0.17.0` at
`60f6a8b` (the merge of `board`). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
D11, rule 19. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository on the machine the work was
done on. No sound server of this machine was asked for anything: the one
that opened the saved devices ran in a container, with no network and a null
sink.

#### The upgrade from v0.16.0

**Claim.** v0.17.0 opens a whole v0.16.0 profile, written by v0.16.0's own
code, without a migration and without losing anything; the settings it saves
again are the bytes v0.16.0 wrote; a theme of the user's own that sets only
`[highlight.Border]` loads, with the one change the release notes name; a
key map of the user's own binds as it did; the huddle devices v0.16.0 saved
under their ALSA names open the default devices, with one notice in a run.

**Rung.** The real fixture: `tests/upgrade/seed_v0160.rs` compiled inside a
checkout of the tag `v0.16.0` (`650a648`) and run there, then
`tests/upgrade_from_v0160_test.rs` on a copy of what it wrote.

What the seed writes, with v0.16.0's own functions: an identity in an
encrypted file and a community; the local agent Scout (`manage::create`, the
code of `bzz agent new`; its key sealed to the owner); `ui.theme = "nord"`;
`huddle_input_device = "hw:CARD=PCH,DEV=0"` and `huddle_output_device =
"front:CARD=PCH,DEV=0"`, as v0.16.0's device list gave ALSA's names;
`theme.toml` with only `[highlight.Border] foreground = "#ff8800"`;
`keymap.toml` binding `u` (free in v0.16.0's workspace, bound by v0.17.0,
D5) to `open-inbox`, `!` to `open-help`, `ctrl-g` to `channel-switcher`, and
turning `Space o` off; messages, an applied edit, a mention, a DM, a draft;
the Board template proposed, approved by its hash and open in `#general`
(`Apps::propose`, `approve`, `open_in`, the code of `bzz app add`, `approve`
and `:app open`), a second version proposed by Scout at a person's request
and waiting; two of the Board's cards as the relay's sync stores them, one
with a `null`, an empty object and seventeen digits. It records what v0.16.0
reads back (`expected.json`): the settings' bytes, the theme's colours, what
each key does, the apps, the cards, the history, and the row count of every
table.

```sh
./scripts/upgrade-test.sh                              # BZZ_UPGRADE_FROM_VERSION=0160 is the default
# and, for the devices, with a sound server of a test:
scripts/test-audio-containers.sh serve <tmp>/snd       # PulseAudio, null sink, in a container
BZZ_UPGRADE_AUDIO=1 PULSE_SERVER=unix:<tmp>/snd/native BZZ_UPGRADE_FIXTURE=<scratch>/fixture \
  cargo test --locked --test upgrade_from_v0160_test -- --ignored --nocapture
scripts/test-audio-containers.sh stop <tmp>/snd
```

| Claim | Seen |
|---|---|
| No migration (D11): schema 23 before and after; every table's row count equal; no `bzz.pre-migration-*.bak` made; `PRAGMA integrity_check` ok; `search_integrity` ok | pass |
| The history, the edit, the Inbox, search (`upgradeneedle`, `editedneedle`), the draft are what v0.16.0 showed | pass |
| The local agent is what v0.16.0 saved, field for field; its key, sealed by v0.16.0, opens with the owner's identity | pass |
| The Board: installed, enabled, version 1 runs with the hash v0.16.0 approved (`Apps::runnable` checks the files' bytes), open in `#general`, version 2 from the agent, asked by the person, waiting and its agent not told; its two cards byte for byte | pass |
| Loaded and saved again by v0.17.0, `config.toml` is the bytes v0.16.0 wrote | **red first**, then pass (below) |
| The theme loads without a warning; `Border` and `FocusBorder` are the user's and Nord's as in v0.16.0; an unfocused frame (`PaneBorder`) is now `Muted` (`Rgb(123, 136, 161)` in Nord), where v0.16.0 drew it in the user's `#ff8800`; the focused frame is `FocusBorder` as before | pass |
| The user's key map loads; `u`, `!`, `ctrl-g`, `?`, `Space o` (off) and the Inbox's `u` do what v0.16.0 did, `u` included (`OpenInbox`, not v0.17.0's `MarkUnread`); `!` as Windows sends it (with Shift) is the same key; `d` is v0.17.0's `Delete` where the user bound nothing | pass |
| `bzz check` says nothing is wrong; `bzz app list` shows the Board at version 1; `bzz agent list` shows Scout; none of them changes the settings | pass |
| With a test sound server: the saved ALSA names open the default devices, and the one notice is `saved input "hw:CARD=PCH,DEV=0" not found; using the default; saved output "front:CARD=PCH,DEV=0" not found; using the default`; opened again in the same run, no notice | pass; red when the seed's input is `default` (then only the output is named) |

**Red, and what it found.** The first run failed at the settings:

```
assertion `left == right` failed: saved again by v0.17.0, the settings are the bytes v0.16.0 wrote: v0.16.0 can read them
  left: "…huddle_output_device = \"front:CARD=PCH,DEV=0\"\nmotion = \"full\"\nhuddle_style = \"lanes\"\n\n[media]…"
```

v0.17.0 wrote `motion` and `huddle_style` on every save of the settings
(choosing a theme, resizing a pane, choosing a device). v0.16.0's own
binary, built from the tag, on a copy of the fixture with those two lines:
`bzz: configuration error: …/config.toml contains invalid or unknown
settings`; on the fixture as v0.16.0 wrote it: `configuration, theme, media,
diagnostics, telemetry, and database are valid`. So whoever tried v0.17.0
could not go back to v0.16.0. Fixed in `src/config.rs` (both settings are
written only when not the default); the test then passes. A person who sets
`motion` or a huddle style still writes a line v0.16.0 refuses; the release
notes say so.

**What it cost.** The script builds v0.16.0 from its tag in a worktree of
its own. Cold, into a target folder on the hard disk
(`BZZ_UPGRADE_TARGET_DIR`, `CARGO_BUILD_JOBS=4`, other agents building at
once): 4 min 22 s and 2.7 GB for v0.16.0 and the seed, about 1 min to write
the fixture; with that folder warm, the whole script took 4 min 54 s (v0.16.0
is compiled again for the new worktree's path, then the test, 5 s). The
default, a target folder inside the temporary folder, is removed after.

#### The last release before: v0.15.0

The CI job keeps the one before the last release on Linux, now v0.15.0
(`BZZ_UPGRADE_FROM_VERSION=0150 ./scripts/upgrade-test.sh`). On this
branch, with the settings change above in it: `test
v0150_profile_upgrades_with_its_agents_ledger_and_artifacts_and_no_app ...
ok` (migrations 21 to 23, the ledger, no reply sent twice). Cold, v0.15.0
built into a target folder of its own on the hard disk: 5 min 4 s in all.

#### Not verified

- The upgrade on macOS and Windows: the CI job runs it there
  (`macos-15-intel`, `windows-latest`) when the branch is pushed.
- The devices part on PipeWire (the test server here is PulseAudio) and on
  a machine with a real card: on a real card, a saved `hw:CARD=…` name of a
  card that is there opens that card's device, not the default
  (`huddle::pulse_devices` unit tests); not seen on hardware.
- The TUI drawn on the upgraded profile: the test reads what the TUI reads
  (store, apps, theme, key map), and `bzz check`; it does not start the
  interface.

### v0.17.0 — task `release2`: the documents brought level with the code

Linux x86_64, 2026-11-25, branch `t3/02fbdc8c` cut from `v0.17.0` at
`18ff30a` ("dev builds: the pulseaudio crate is built optimised…"). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md),
F8. `<scratch>` is a folder outside the repository. Documents only: no file
under `src/`, `tests/`, `scripts/` or `site/` changed, nor `Cargo.toml`.

#### What was written

- `docs/release-v0.17.0.md`: what landed after task `release` wrote it
  (A9, C8, C10, C11, L7 to L12, F7, and tidy's and ci's changes), every
  change of behaviour in three tables, the known limits, "Not in this
  version" brought up to date.
- `docs/validation-v0.17.0.md`, regenerated by `<scratch>/join-validation.py`
  from every task file that landed, with an index that maps every plan item
  to its proof or says why there is none, a note where the proof departs
  from the plan's, and two sections of the principal's
  (`<scratch>/principal-checks.md`, `<scratch>/went-wrong.md`, which the
  script puts after the index; task `suite` stays a commented line in it).
- The plan's status line and its last section, "How it closed".
- `README.md`: the "New in 0.17" block, "Get started" (`bzz demo`, `:join`,
  `Ctrl-K`, `docs/keys.md`), two rows of the keys table (`u`, `q`/`Esc`,
  `z`). `docs/README.md`: `keys.md` in the index, the plan's closing.
  `docs/e2e-manual.md` section 15: joining against the real thing, the
  mouse in a real terminal, motion by eye, the look under `NO_COLOR`.
- Out of the repository: `<scratch>/site-new-in-v0.17.0.patch` and
  `<scratch>/SITE-PROPOSAL.md`, the site's section as a proposal.

#### Claims checked against the code

Each change of behaviour the release notes name was read in the code at
`18ff30a`, not only in a task's notes:

| Claim | Where it was read |
|---|---|
| `Esc` with nothing open says `nothing to close · q quits`; `q` is still `BackOrQuit` | `src/app/shell.rs`, `src/ui/action_table.rs` (`nav.back`), `tests/v016_pins_test.rs` (`MOVED_KEYS`, `DELIBERATE_CHANGES`) |
| `u` is "Mark this channel unread" | `src/ui/action_table.rs` (`msg.mark_unread`), `docs/keys.md` |
| `z` is new in the workspace | `tests/v016_pins_test.rs` (`ADDED_KEYS`) |
| v0.16.0's `:community add` answered `use the CLI to add/remove communities` | `git show v0.16.0:src/app.rs` |
| v0.16.0's `:tochannel` kept the thread's draft | `git show v0.16.0:src/app.rs` (`send_draft_to_channel`) |
| v0.16.0 had `ui.sidebar_width` and `ui.thread_width` (a dragged width is the old setting) | `git show v0.16.0:src/config.rs` |
| The widths (38, 80, 108, 124, 152, 174, 68, 110, 100) | `src/ui/layout.rs` (constants, module documentation, tests) |
| Toasts: attachment saved and removed, messages copied, sidebar updated | `grep toast( src` |
| The no-sound-server sentence | `src/huddle/audio.rs` (`NO_SOUND_SERVER`) |
| The first screen's `j`, `p`, `k` open the join form | `src/app.rs` (`take_shown`), `src/ui/first_screen.rs` |
| The footer's `i reply` on a message | `src/ui/action_table.rs` (hints of the "Write a message" row) |
| What CI runs on macOS | `.github/workflows/ci.yml` (`build`, `upgrade`, `audio`, `agent-host`) |
| `install.sh` already assumes v0.17.0 (`BZZ_SOUND=server`) | `site/public/install.sh` |
| The version is still 0.16.0 at the head; the version commit is `8184d9c` on another branch | `Cargo.toml`, `git show --stat 8184d9c` |

#### Proofs

| Claim | Command | Seen |
|---|---|---|
| Every relative link and anchor of the documents touched, and of every task file, resolves | `python3 <scratch>/linkcheck.py docs/release-v0.17.0.md docs/validation-v0.17.0.md README.md docs/README.md docs/e2e-manual.md docs/planning/2026-11-24/v0.17.0.md docs/validation-v0.17.0/*.md` | `299 links, 0 broken` |
| The validation document is the join of every task file that landed | `python3 <scratch>/join-validation.py` | `joined: [demo, demo2, install, join, viz, viz2, palette, find, palette2, registry, look, motion, layout, sound, board, tidy, ci, release, release2, windows] not joined: []` |
| Nothing in the diff names a machine path, an address, the owner's user names, the virtual machines or another project | `<scratch>/scan.sh`: every added line of `git diff --cached 18ff30a` against 22 patterns in `<scratch>/scan-patterns.txt` (the five top folders of this machine's paths, any IPv4 address, the owner's user names, the two virtual machines' names, Windows paths, the folders of the scratch and build caches, the principal's notes, the other projects on the machine), less the repository's own GitHub and image names | empty |
| The site patch applies to the head and changes only `site/public/index.html` | `git apply --check -p1 --directory=site <scratch>/site-new-in-v0.17.0.patch` | applies; not applied |
| The documents some tests read are still right | `cargo fmt --check`; `cargo test --locked --test keys_doc_test --test build_environment_test` | fmt clean; 1 and 2 passed |

The full gates were not run: nothing but documents changed.

#### Not verified

- The CI run at `18ff30a`: queued when this was written. The release
  notes and the plan say so.
- The numbers the documents quote were not measured again here; each names
  the task or the principal who measured it.
- The site section was not drawn in a browser.

### v0.17.0 — task suite: the scenario suite green, in any order

Every run below: the release binary of this branch, the pinned relay
(`./scripts/test-relay.sh --tmux`, `BZZ_BUZZ_SOURCE=<scratch>/buzz-pin`,
`BZZ_TMUX_SIZE` as given, `BZZ_TMUX_CAPTURES=<scratch>/runs/<run>/captures`),
on this machine, through tmux on a private socket. After each run the
`buzz-harness` containers were counted (0) and the relay lock tried
(`flock -n /tmp/bzz-test-relay-$(id -u).lock true`: free). Logs and captures
are in `<scratch>/runs/`.

#### Claim: before this task the suite does not pass as a whole

Rung: the suite itself, on the head the task started from (`0d96e80`).

| Run | Result |
|---|---|
| `base-200`, 200x50 | 44 of 50 passed. Failed: `t17-palette-unavailable-stays`, `t17-palette-sizes`, `t17-sound-call`, `t17-palette2-theme-react`, `t17-palette2-find`, `t17-palette2-app` |
| `base-80`, 80x24 (with the new fixture channels already in) | 32 of 50 passed |

Why each failed, from the capture at the failing step:

| Scenario | What the screen showed |
|---|---|
| `t17-palette-unavailable-stays` | `#t9-general`'s newest message was the viewer's own (the Board's brief `t16-m6` posts), so "Delete my message" could run and asked `delete message?` instead of saying why not |
| `t17-palette-sizes` | the same at 80x24: no `can't now` |
| `t17-sound-call`, `t17-palette2-app` | `Generated custom emoji`, used to check that `#t9-general` opened, had scrolled away under the brief |
| `t17-palette2-theme-react` | `3 G k k k` no longer reached the custom emoji's message |
| `t17-palette2-find` | the profile's `r refresh · Esc close` is now on its frame's foot, not after `profile ·` |
| at 80x24, t9 to t15 | the messages used as "the channel is open" (`Generated custom emoji`, `Generated agents channel`, `Generated namesakes channel`) wrap in a narrow timeline |

The Herdr runner refused the manifest: `./scripts/test-tui-herdr.sh --list`
printed `Herdr scenario has an invalid profile, or args outside a harness
scenario` (the automated `bzz demo` scenarios), and would then have refused
`motion` and, after the merge of `layout`, the `mouse` step field.

#### Claim: each scenario stands alone, in any order

Rung: the suite run in other orders (`BZZ_TMUX_ORDER`, new; each scenario
still after what it `needs`). Each run found a dependency, fixed before the
next:

| Run | Order | Result | What it found |
|---|---|---|---|
| `rev-200` | reverse | 42 of 47, 3 skipped | the Board left open in `#t17-app`; `:apps` read as "open nowhere"; `↳ 1 reply` matched an older thread; `agent move` before `move-mismatch`; Scout not yet known to the palette |
| `rev2-200` (after the merge of v0.17.0) | reverse | 46 of 49, 3 skipped | the first start of a fresh profile is `online` before its channels; `t17-layout-f7` opened the thread of a join row; `t16-app-pane` counts cards that `t16-m6` made |
| `shuf1-200` | `shuffle:1` | 50 of 52 | the approval banner names the Board's waiting version and hides the newer one |
| `shuf2-200` | `shuffle:2` | **52 of 52**, 208 s | — |

#### Claim: the suite is green at 200x50, three times in a row, and at 160x45 and 80x24

Rung: the suite in the manifest's order, on `e3367b6` (the branch with
`v0.17.0` at `ecff610` merged in; the one commit after it only removes the
runner's temporary folder at the end), release binary built from it.

| Run | Size | Result | Time |
|---|---|---|---|
| `fin3-200-a` | 200x50 | 52 of 52 passed | 210 s |
| `fin3-200-b` | 200x50 | 52 of 52 passed | 207 s |
| `fin3-200-c` | 200x50 | 52 of 52 passed | 207 s |
| `fin3-160` | 160x45 | 52 of 52 passed | 207 s |
| `fin3-80` | 80x24 | 48 of 48 passed, 4 skipped | 183 s |
| `fin3-join2` | `t17-join-open` (kind `join`) | 1 of 1 | 72 s |
| `fin3-members` | `t17-join-members`, relay closed (`BUZZ_REQUIRE_RELAY_MEMBERSHIP=true`, a random `RELAY_OWNER_PUBKEY`, `BZZ_FIXTURE_AGENTS=0`) | 1 of 1 | 71 s |

Skipped at 80x24, and why: `t15-approval-approve-refuse` (`min_size =
"120x30"`: the waiting line is one row of 108 characters, cut under 120
columns), `t15-agent-new-form` (`120x30`: the form's sentences wrap inside
its frame between the words a step reads), `t15-coding-harnesses-in-panel`
(`120x36`: the agent's record is longer than the panel under 36 rows),
`t15-cli-agent-activity` (it needs the first, which was skipped). Each size
was measured, below.

With agents on, the fixture fails on a closed relay: `the relay did not
confirm the agent's profile` (`examples/harness_fixture.rs`, the local
agent; unchanged by this task). The members-only scenario needs no agent.

The automated scenarios (no relay), `BZZ_TMUX_SCENARIOS=automated`:

| Size | Result | Time |
|---|---|---|
| 200x50, three times | 17 of 17 each | 66, 66, 67 s |
| 160x45 | 17 of 17 | 67 s |
| 80x24 | 17 of 17 | 66 s |

Every automated scenario names its size but `startup-help-quit`,
`custom-keymap-inbox` and `v013-panels-empty-profile`, which hold at all
three.

#### Claim: `needs` and `min_size` are checked and reported

- `BZZ_TMUX_SCENARIOS=t15-approval-approve-refuse,t15-agent-new-form,t15-coding-harnesses-in-panel`
  printed `scenario t15-cli-agent-move added: t15-agent-new-form needs it`
  (and the rest of the chain) and ran them first (`probe-*` runs).
- At 80x24 (`fin2-80`): `SKIPPED: holds at 120x30 or larger; this run is at
  80x24` and `SKIPPED: needs t15-approval-approve-refuse, which skipped`;
  the run's exit is 0 and its last line names the skipped ones.
- `rev-200`: `t16-app-looper-composer SKIPPED: needs t16-app-pane, which
  failed`, instead of a failure at its second step.

The smallest sizes were measured: `t15-approval-approve-refuse` and
`t15-agent-new-form` fail at 100x30 and pass at 120x30;
`t15-coding-harnesses-in-panel` fails at 120x30 and 100x40 and passes at
120x36.

#### Claim: the Herdr runner reads the manifest

Rung: its manifest reader only. `bash -n scripts/test-tui-herdr.sh`;
`./scripts/test-tui-herdr.sh --list` lists 69 rows and says that
`t17-join-not-there` (isolated profile) and `t17-layout-mouse` (mouse
steps) are run by the tmux runner only; `--scenario t17-palette2-app` says
`needs t16-cli-app-approve, which is not selected`. Not run: Herdr is not
for this task, so the bash that drives a pane (the new `args` and `motion`
of automated scenarios) was read, not run.

#### What bzz does that a scenario had to work around

- **Your own local agent cannot be found in the palette on a fresh
  profile.** `palette_people` (`src/app/palette.rs`) lists profiles bzz
  holds and verified agents; a hosted agent of `config.hosted_agents`
  without a profile yet is not listed, so on a profile's first starts
  "Start a DM", `sc` showed "No people yet: bzz knows a person once it has
  seen them" for 15 s (capture: `rev-200`, `t17-palette2-sizes`). The
  scenarios now wait for Scout's join row first.
- **`online` comes before the channel list on a profile's first start.**
  The finder, typed into at once, held only the selected channel and "start
  a DM", and Enter opened the DM picker with the typed name (`rev2-200`,
  `t17-layout-board-captures`, step 2). The runner now starts the profile
  once before the suite.

#### What was wrong, and what changed

| Scenario | What was wrong | What changed |
|---|---|---|
| `t17-palette-unavailable-stays`, `t17-palette-sizes` | `#t9-general`'s newest message was the viewer's own | `#t17-palette` (fixture), newest message another person's; the row selected checked first |
| `t17-palette2-theme-react` | `3 G k k k` reached another row | `#t17-react`: one message, `3 G` |
| `t17-find-delete-unread-status` | wrote and deleted in `#t9-general` | `#t17-find` |
| `t17-sound-call` | its check of `#t9-general` scrolled away | `#t17-sound`, both people members |
| `t17-palette2-app` | needs the Board; left it open in `#t9-general`'s view of `:apps` | `needs = ["t16-cli-app-approve"]`, `#t17-app`, `:app close` at the end |
| `t17-registry-app-footer` | needs an installed app | `needs`, `#t17-palette` |
| `t17-palette2-find` | the profile's keys moved to its frame's foot | a pattern over the frame, the name and the keys |
| `t17-palette2-dm`, `t17-palette2-sizes` | Scout not yet known on a fresh profile | wait for `Scout joined` at the top of `#t15-agents` |
| `t17-layout-f7` | `k Enter` opened a join row's thread | `#t17-thread` |
| `t17-layout-board-captures` | needs the Board | `needs` |
| `t17-layout-mouse` | the demo's longer notices; the channels' order; the hint now follows the focus | three turns of the wheel, `#design` on row 4, the palette's key at column 70 |
| `t17-demo2-first-minute` | Scout's answer wraps beside the conversation | a pattern across the row beside it |
| `t17-find-commands`, `t17-join-not-there` | "Join a community" is also a row of the first screen | the form's own words |
| `t17-join-*` | the join form is 100 columns wide, centred at 160 | the frame of that size, edge to edge or centred |
| `t17-join-open` | the six new channels push `#t9-general` off the finder at 80x24 | narrowed first |
| t9, t14, t15 that open a channel | a message used as "the channel is open" wraps or scrolls away | the composer's title or the status line |
| `t15-agent-answers-mention` | `↳ 1 reply` matched an older thread | this mention's row |
| `t15-coding-harnesses-in-panel` | the `BZZ_GROK` sentence wraps where the fixture's path puts it | any space may be a break; `min_size` |
| `t15-approval-approve-refuse`, `t15-agent-new-form` | do not hold at 80x24 | `min_size` |
| t15 and t16 chains | dependencies silent | `needs` (credential, show, move, receive, activity, the form; add, approve, the M3 and M4 chain, M6 after M4, M5 and the Poll after the Board is approved, since the banner names the oldest version waiting) |
| `t16-app-pane`, `t16-m6-share-offer-approve` | `:apps` read as one exact line | the Board's row with where it is open |
| the runner | nothing for order, needs, sizes; first starts; ~9 MB left in `/tmp` per run | `needs`, `min_size`, `BZZ_TMUX_ORDER`, a warm-up of each profile, the folder removed |
| the Herdr runner | refused the manifest | reads it (above) |

#### The final head, and the gates

`fin4-200`, on `74cbba6` (the runner now removes its folder): 52 of 52 at
200x50 in 216 s; relay down, lock free, the run's folder gone.

Gates on `74cbba6`: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets` (2,106 passed, 0
failed, 44 ignored, in 72 test binaries) and `cargo test --locked
--no-default-features --lib`: all exit 0. Cargo.toml and Cargo.lock are
unchanged, so deny and audit were not run.

## W — Windows and macOS (D15)

### v0.17.0 validation: Windows (task `windows`, plan W1 and W2)

Windows 11 (build 26100), PowerShell 5, Windows OpenSSH: every session is a
ConPTY. Driven from Linux through tmux, as
[`docs/e2e-windows.md`](e2e-windows.md) says. 2026-11-24 and 25.
`<scratch>` is a folder outside the repository on the Linux machine; the
captures named below are there.

The build under test is this branch, built on Linux for
`x86_64-pc-windows-gnu` with Zig as C compiler and linker (the machine has
no `clang-cl`, so `cargo xwin` could not build bzz's C dependencies; a
crossterm-only probe built with `cargo xwin` for MSVC). Its terminal code
is the release's: the same crossterm Windows backend. `bzz --version` of it
says `bzz 0.16.0` (the branch has not changed the version). The installed
v0.16.0 (`bzz --version`: `bzz 0.16.0`, from the README's installer) was
used for the "before" and left in place.

#### What works on Windows now, what does not

| | |
|---|---|
| Works (seen) | `?` opens the help and `:` the command line, on the first screen and in a community; channels, a thread, the composer with accents, `¿ ¡ € °`, CJK and emoji, sending; `:search`, `:inbox`, the theme picker (`Space o`), `:apps`, the approval of an app version with `Ctrl+Y` (scroll to the end, `a`), the Board in the channel (`5`, `n`, text, Enter, `]`, `Ctrl+C` leaves) and in `bzz app dev`; Tab and Shift+Tab; a click on a channel and the wheel; resizing to 80×24 and back; `q` then `y` restores the terminal (main screen, cursor, mouse off) |
| Works by tests only | keys a physical keyboard gives that SSH cannot: AltGr text, Caps Lock with letters and with Ctrl |
| Does not work | a paste is not a paste: it arrives as keys. Its line breaks no longer send (on this branch), but it is still typed key by key, so outside the composer its characters are keys of the key map, as on a terminal without bracketed paste. After a kill (`Stop-Process`), the console keeps bzz's last screen and stays in the alternate screen until `clear` (no program can restore what it is killed in) |
| For a person at Windows Terminal | the list at the end of [`e2e-windows.md`](e2e-windows.md): AltGr and Caps Lock on a real keyboard, a paste with `Ctrl+V` and with a right click (one line and three lines), the look (frame lines, colours, emoji width), clicks |

#### The dead keys (W1)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| On v0.16.0, `?` and `:` do nothing on Windows | the published binary on the machine | `bzz` on a scratch profile in the ConPTY, `?`, then `:` | the first screen unchanged after each (`v016-after-question.txt`, `v016-after-colon.txt`) |
| The cause: Windows reports Shift with every shifted symbol, and the key map matches modifiers exactly | the key probe on both sides | `examples/key_probe.rs` (crossterm-only copy built with `cargo xwin`) on Windows; the same example in tmux on Linux; the same keys sent to both (`<scratch>/keys.sh`) | `? : @ ! # $ % ^ & * ( ) _ + { } \| < > " ~` with `SHIFT` on Windows, none on Linux; `Ctrl+J` as Enter with Ctrl on Windows; a Release after every Press on Windows; everything else the same (`probe-windows-conpty.txt`, `probe-linux-tmux.txt`; the table in `e2e-windows.md`) |
| Read from crossterm 0.29's Windows parser (`event/sys/windows/parse.rs`), not seen over SSH | read | | a capital under Caps Lock has no Shift; Ctrl+letter under Caps Lock is the capital; AltGr text carries Ctrl+Alt |
| Fixed at the door, for every platform, Linux and macOS keys unchanged | unit | `cargo test --lib keymap:: keys_enter_in_one_form` | 19 pass. Red without the change (normalisation and the parse of `shift-` made identities): 7 fail, `windows_shifted_symbols_reach_their_bindings`, `windows_capitals_and_ctrl_keys_under_caps_lock`, `windows_altgr_characters_are_text`, `windows_ctrl_enter_is_the_composer_newline`, `shift_tab_reaches_previous_focus`, `user_maps_match_with_or_without_shift`, `keys_enter_in_one_form_for_every_platform`. `keys_from_a_linux_terminal_are_unchanged` passes on both: every key of the Linux probe comes out as it went in |
| A user key map written with or without `shift-` works on both | unit | `user_maps_match_with_or_without_shift` | `shift-!` and `!`, `X`, `shift-x`, `shift-X`, `shift-tab` |
| On the machine with this branch: `?` opens the help, `:` the command line | the build on the machine | as the first row | `w1-welcome-question.txt` (the help), `w1-welcome-colon.txt` (the command line) |

#### Found on the walk, and fixed (W1)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| A character the layout cannot type (`¿ ¡ € °` on a US layout) was lost: the console sends it as an Alt code, which crossterm gives as a release with no press | probe, then the build | the probe on Windows; `¿qué?` typed in the composer | the probe: `Char('¿') … kind=Release` and no Press; the message sent read `qué?` (`r1-composer.txt`), a card `sí?` (`r1-app-card.txt`) |
| Fixed: such a release is the press (Windows, non-ASCII only) | unit, then the build | `windows_alt_codes_are_presses_and_releases_stay_releases`; on the machine, `:` then `¿¡€° café ñ 😀`, and a card `¿Funciona? ¡Sí! 20 °C, 5 €` in `bzz app dev` | the command line and the card show every character (`w1b-colon-altcodes.txt`, `w1b-appdev-altcodes.txt`) |
| A paste of two lines into the composer sent the first line, and the next characters ran as keys until an `i` | the build, with the relay | `i`, then a tmux paste of `paste line one⏎paste line two` | `paste line one` sent; the composer held `ne two`: `paste li` were keys (`r1-paste.txt`) |
| Fixed: in the composer on Windows an Enter with key presses already waiting behind it is a line break | unit, then the build | `windows_paste_line_break_is_a_composer_newline_only_with_keys_behind`, `a_paste_on_windows_is_seen_behind_its_line_break` | see the section W2 below |
| Shift+Tab never moved the focus back, on any platform: crossterm reports BackTab with Shift and the builtin `backtab` has none | unit, then the build | `shift_tab_reaches_previous_focus`; on the machine, Shift+Tab twice then Tab | the focused frame went communities → timeline → channels → timeline |

#### The walk (W2), on this branch

Against the pinned test relay (`ws://localhost:3030`, reached through `ssh
-R`), the harness fixture's viewer profile (its configuration and its key
only; the store synced fresh).

| Step | Seen | Capture |
|---|---|---|
| Start | the passphrase prompt, then the workspace in about ten seconds | `r1-first.txt` |
| A channel | `2`, `k`, Enter opens `#t9-general` with its history | `r1-general.txt` |
| The composer | `café, ñandú, über, 日本語, 😀👍` typed and sent; acknowledged | `r1-composer.txt`, `r1-sent.txt` |
| A thread | Enter on the message with replies opens them | `r1-thread.txt` |
| `:` commands | `:search` then `Windows` finds the message just sent; `:inbox` | `r1-search2.txt`, `r1-inbox.txt` |
| Help in the Inbox | `?` | `r1-inbox-help.txt` |
| Theme picker | `Space o`, `j j` Enter: Light, the colours change (truecolor) | `r1-options.txt`, `r1-apps.txt` (status line) |
| An app | `bzz app add`, `Ctrl+Y`, PgDn to the end, `a`; `:apps`, Enter; `5`, `n`, a card, `]` moves it to Doing, `Ctrl+C` leaves | `r1-ctrly.txt`, `r1-approved.txt`, `r1-app-open.txt`, `r1-app-card.txt`, `r1-app-left.txt` |
| The mouse | a click on a channel opens it; the wheel | `r1-click.txt`, `r1-click2.txt`, `r1-wheel.txt` |
| Resize | 160×45 to 80×24 and back | `r1-80x24.txt`, `w1-welcome-80x24.txt`, `w1-welcome-160x45.txt` |
| Quit | `q`, `y`: main screen, cursor shown, mouse off (tmux's own state) | `w1-after-quit.txt` |
| Kill | `Stop-Process`: the console keeps the alternate screen and bzz's last frame under the prompt; the cursor and the mouse are back; `clear` gives a clean prompt | `r1-after-kill.txt`, `r1-after-kill-clear.txt` |
| `bzz app dev` | the Board offline: a card with accents and emoji, `]`, `Ctrl+C` | `appdev-*.txt` |

Seen in passing, not fixed (not Windows, or not small):

- On the first screen `:theme` is "unknown command" (C4 adds it).
- At 80×24, a wrapped system line under the selection bar loses its first
  character (`▌d8c869d…` for `6d8c869d…`, `r1-80x24.txt`): the same code on
  every platform; L10's layout work is where it belongs.
- The approval banner covers the pane titles (`r1-banner.txt`; L5).
- `q` in a thread closes the thread; a `y` typed right after it copies the
  selected message (`r1-oops.txt` shows what the driver typed into bzz when
  it took a closed thread for a quit): no defect, a reason the driver now
  refuses to type unless the pane shows a prompt.

#### Paste (W2)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| crossterm on Windows never produces a paste event | read | `crossterm 0.29` `event/sys/windows/parse.rs`, `event.rs` (`EnableBracketedPaste` writes `CSI ?2004h`; the Windows reader has no `Paste`) | |
| Over SSH the console swallows the bracketed-paste markers | probe | `ESC [ 200 ~ ab LF c ESC [ 201 ~` sent to the probe | `a`, `b`, Ctrl+Enter, `c`: no marker, no paste |
| A tmux paste (bracketed when the pane asks) arrives as keys | probe | `paste-buffer -p` of two lines | one key per character, Enter for the line break |
| With this branch, a paste of three lines stays in the composer, and a typed Enter still sends | the build, with the relay (a second run) | `i`, a tmux paste of `paste line one ¿sí?⏎paste line two⏎qy line three`; then Enter; then `i`, `typed line`, Enter | the three lines in the composer and nothing sent (`w1b-paste.txt`); Enter sent them as one message (`w1b-paste-sent.txt`); the typed line was sent at once (`w1b-typed-sent.txt`). The `q` and `y` of the third line stayed text |

What this is: a guess from what waits in the input, which the brief allows
only where it is the only way and safe. It is the only way (no marker
reaches bzz), and it fails safe: a person who types Enter and the next key
before bzz reads the Enter gets a line break instead of a send, and sends
with the next Enter. Over SSH, the console puts each key's release right
behind its press, so the release alone is not "more waiting". A paste that
ends in a line break still sends at its last Enter, and a paste outside the
composer is still keys. A real paste event on Windows needs the console's
VT input mode and a reader that parses it, which crossterm 0.29 does not
have: a plan question, not this task's.

#### Windows Terminal

Not installed on the machine, and nothing was installed. What was proven
in a ConPTY over SSH is the console side, which Windows Terminal shares.
What remains for a person is listed at the end of `e2e-windows.md`.

#### On the machine at the end

The last build under test: `w1-bzz.exe`, SHA-256 `fa5ed6664dd7c6af…`,
`bzz --version` `bzz 0.16.0` (the branch's version). Then, in one
PowerShell script over SSH: no process of the build or the probe left
(none was running; `Stop-Process` on the list anyway), the scratch folder
removed (`Test-Path` false), the installed binary in its place with
`bzz 0.16.0` and its date from the installer, no `bzz` process, nothing
listening on port 3030. The tmux server of the driver was stopped, which
closed the SSH session and its port forward. The relay run of this task
reported `relay down` and its process is gone; the lock was then held by
another task's run.

The scratch folder also held a `tasks` app folder from the principal's own
v0.16.0 check, made before this task began; it went with the folder, as
the brief asks. `bzz app new tasks` makes it again.

#### Gates

On this branch at its last code commit, Linux x86_64:

```
cargo fmt --check                                   ok
cargo clippy --locked --all-targets -- -D warnings  ok
cargo build --locked --example scripted_harness     ok
cargo test --locked --all-targets                   ok (1,598 passed over all targets with the next line, 0 failed)
cargo test --locked --no-default-features --lib     ok
```

`Cargo.toml` and `Cargo.lock` are unchanged: no `cargo deny`, no
`cargo audit`. The Windows build was made with `--ignore-rust-version` on
Rust 1.95.0, the only toolchain of the machine with a Windows target; it
compiled with the two dead-code warnings the Windows build already had
(`agents/runtime/process.rs` `boot_id`, `auth/provider.rs` `Owner`).
