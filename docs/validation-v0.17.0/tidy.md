# v0.17.0 — task `tidy` (loose ends of the reviews)

Branch `t3/v017-tidy`, cut from `v0.17.0` at `24f7486`. Linux x86_64,
2026-11-25, on a machine shared with other builds (16 cores, load 9 to 32
during the runs). `<scratch>` is a folder outside the repository.

## Claims and proofs

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

## Item 4

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

## Item 6

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

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | first run: 1 failure, `hostile::every_hostile_script_is_stopped_in_budget` at load 24 (see "Item 4"); second run (`--no-fail-fast`): pass, 0 failed in 68 targets |
| `cargo test --locked --no-default-features --lib` | pass, 646 tests |

`Cargo.toml` and `Cargo.lock` did not change: deny and audit were not run.
