# v0.17.0 — task `look` (L1 to L6, C2)

Branch `t3/implement-look-feature`, cut from `v0.17.0` at `543dbd1`, with
`v0.17.0` at `bfd340f` (viz) merged in. Linux x86_64, 2026-11-24/25, on a
machine shared with six other builds (load 18 to 35). Buzz pin
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. `<scratch>` is a folder outside
the repository.

## Claims and proofs

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

## Scenario checks that read a title or the status line, changed

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

## Relay suite

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

## Captures

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

## Theme tokens added (the site mirrors `builtin.rs`)

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
