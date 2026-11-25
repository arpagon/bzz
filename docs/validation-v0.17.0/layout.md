# v0.17.0 — task `layout` (L10, L11, L12, F7)

Branch `t3/layout`, cut from `v0.17.0` at `9902c74`. Linux x86_64,
2026-11-25, on a machine shared with six other builds. Buzz pin as in
`runs/buzz-pin`. `<scratch>` is a folder outside the repository.

## The table (L10)

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

## Claims and proofs

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

## Relay suite

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

## The automated scenarios

`PYTHONDONTWRITEBYTECODE=1 BZZ_BIN=<scratch>/bzz BZZ_TMUX_SCENARIOS=automated python3 scripts/test-tui-tmux.py`
(debug build of the last commit): 10 of 11. `t17-demo-first-minute` waits
for `reaction · Enter toggle`, a title the base no longer draws either (task
`look` moved the keys to the footer: `┌ reaction ─`, `└ Enter toggle · :
custom · Esc close ─`); not this task's, left for its owner.

## Captures

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

## Gates

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
