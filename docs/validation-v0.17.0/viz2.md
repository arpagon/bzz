# v0.17.0 — viz2: the zoomed huddle, its styles, the microphone check (B5 to B7)

Linux x86_64, 2026-11-25, branch `t3/70f252c4` cut from `v0.17.0` at
`bfd340f` (the merge of `viz`, B1 to B4). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
track B, D7, D8, D9, D11, rules 19 to 21. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository.

No sound device of this machine was opened, played through or recorded
from. Every level, sample and spectrum in a test, a golden, a capture or
the film comes from generated audio: pure tones (`huddle::synth::tone`) or
the generated voices of `huddle::synth::Synthetic` (D8). The microphone
check is proven with a fake device (generated frames written where the
device's frames go).

## What was built

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

### Keys and commands (B5)

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

## Claims

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

### Cost of each style, in bzz

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

### B8: frames that redraw only the visualizer

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

## Gates

At `d3db394`, all green: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets` (lib: 826 passed, 3
ignored), `cargo test --locked --no-default-features --lib`. No dependency
and no migration was added, so `cargo deny` and `cargo audit` were not run.

## Captures

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

## Not proven

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
