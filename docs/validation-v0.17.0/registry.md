# v0.17.0 validation — task `registry` (C8, C10, C11, rules 19, 20 and 23)

Linux x86_64, 2026-11-25. Branch `t3/440a7815`, cut from `v0.17.0` at
`8a95a33` ("demo2 and look together"). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
items C8, C10 and C11. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository.

`U:` a unit or integration test (`cargo test --locked`). `T:` a tmux
scenario of `scripts/tui-herdr-scenarios.toml` on the release binary through
`scripts/test-tui-tmux.py`: `automated` ones on an empty isolated profile,
`harness` ones on the pinned relay (`scripts/test-relay.sh --tmux`).

## What was built

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

## C8 — one registry generates the key map

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

## C10 — the footer follows the focus

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

## C11 — the `:` line completes

| Claim | Proof | Result |
|---|---|---|
| `:hu`, Tab → `:huddle `; Tab → `:huddle start`; Tab → `:huddle join`; Shift-Tab back; the verbs start, join, leave, mute, end, devices, react, input, output are offered; under the line `Tab: [start]  join …` and `usage: :huddle [start | …]`; typing after a completion parses as typed | `U: ui::command::tests::tab_completes_a_command_then_its_verbs_and_cycles` | pass |
| `:theme n`, Tab → a theme id bzz has; `:theme ` offers the themes then `reload`; `:app open `, Tab, Tab cycles the installed apps with `usage: :app open <name>`; `:sort ` → alpha, recent, default; `:presence ` → on, off; `:app ` lists its verbs | `U: ui::command::tests::tab_completes_themes_apps_and_fixed_words` | pass |
| A wrong word completes nothing and says `no command starts with hudle · did you mean :huddle?`; `:huddle xyz` says nothing after `:huddle` starts with `xyz`, with the usage | `U: ui::command::tests::a_wrong_word_completes_nothing_and_says_so` | pass |
| The same through the App at 80×24, and Esc forgets the cycle | `U: app::registry_tests::tab_completes_the_command_line_and_shows_what_it_offers`, `tab_completes_theme_names_and_a_wrong_word_says_so` | pass |
| On the release binary at 80×24: `:hu`, Tab, Tab, Tab; `:theme n`, Tab; `:hudle`, Tab; `:sort `, Tab | `T: t17-registry-complete` | pass |
| On the pinned relay: `:app open `, Tab completes an installed app | `T: t17-registry-app-footer` | see "Relay run" |

## Commands run

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

## Relay run

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

## Captures

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

## Changed on purpose, not keys or commands

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
