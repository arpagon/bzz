# v0.17.0 — viz: the huddle shows who is speaking (B1 to B4)

Linux x86_64, 2026-11-24, branch `t3/visualization-task` cut from
`v0.17.0` at `543dbd1`. Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
track B, D7, D8, D9, rules 20 and 21. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository.

No sound device of this machine was opened, played through or recorded
from. Every level in a test or a golden comes from generated audio: pure
tones (`huddle::synth::tone`) or the generated voices of
`huddle::synth::Synthetic` (D8).

## What was built

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

## Claims

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

## Relay run

RELAY_RESULT_PLACEHOLDER

## Captures

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

## Not proven

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
