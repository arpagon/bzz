# v0.17.0 validation — task `demo` (A1, A4, A8)

Plan: [`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
track A, D2, rule 22. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
Linux x86_64, 2026-11-24, branch `t3/demo` cut from `v0.17.0` at `543dbd1`;
`v0.17.0` merged in at `bfd340f` (viz) for the huddle's lanes. The rows
above "The huddle of `#infra`" were measured before that merge.
The machine was shared with six other agents' builds throughout (load
average 16 to 28), which is in every time below.

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every bzz run by hand had a scratch home (`HOME` and the four
`XDG_*_HOME` in `<scratch>/home`, `DBUS_SESSION_BUS_ADDRESS` unset, `TMPDIR`
in `<scratch>/tmp`); `bzz paths` named only `<scratch>/home`.

## What was built

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

## Proofs

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

### The huddle of `#infra`, with viz's lanes (after the merge)

| Claim | Rung | Command | Seen |
|---|---|---|---|
| Ada, Lin and Sam speak in generated voices in `#infra`'s huddle; You listen muted, your lane still (the script's lines for you are left out) | unit | `cargo test --locked --lib demo::` (`what_needs_a_real_community_says_so`: peers `[1, 2, 3]`) | passes |
| The recording plays only where its huddle runs, and the panel does not contradict its lanes | unit, 80×24 render | `cargo test --locked --lib the_demo_huddle_is_a_recording_only_where_it_runs` | in `#team` no lanes and no strip; in `#infra` the lanes, `a generated recording`, `4 in the call` (the panel's roster: `4 in it`), `nothing is sent`; none of `No huddle is running here`, `you are not in the call`, `j join`, `m unmutes`, `s start` |
| The same on screen | tmux, debug build, 160×45 | scenario `t17-demo-huddle` | passed: the strip `🎧 ⣀…⣶ Ada · mic muted` in `#infra`'s status line, the panel's title `a generated recording · Esc`, the microphone line `a recording · no microphone is open, nothing is sent`, `j` → `Demo: the huddle is a recording to look at.` |
| No sound device is opened | strace | `strace -f -e trace=openat,connect bzz demo`, then `#infra`, `:huddle`, `j`, `m`, quit | the only sound file opened is `libasound.so.2` by the loader at start (the link E1 removes); no `/dev/snd`, PulseAudio or PipeWire socket. Every `inet` connect is to 127.0.0.1 |

Captures, read as a newcomer: `<scratch>/first-screen-80x24.txt`,
`first-screen-join-80x24.txt`, `first-screen-160x45.txt`,
`first-screen-import-160x45.txt`, `demo-160x45-first.txt`,
`demo-160x45-thread.txt`, `demo-160x45-dm.txt`,
`demo-80x24-theme-preview.txt`, `demo-80x24-huddle.txt`,
`demo-80x24-infra-strip.txt`, the scenario steps in `<scratch>/captures/`,
the container's `first-80x24.txt` and `after-write-80x24.txt`.

## Not verified

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
