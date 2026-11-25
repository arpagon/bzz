# v0.17.0 validation: the palette (C1)

Task `palette`, branch `t3/palette`, cut from `v0.17.0` at `543dbd1`. Linux
x86_64, 2026-11-24, on a machine shared with six other builds and relay runs
(load 15 to 24). Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
item C1 and D4. `<scratch>` is a folder outside the repository.

## What was built

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

## Proofs

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

### Red runs

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

### Captures

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

## Not verified

- `NO_COLOR` on the screen: bzz does not read it before L1 lands.
- The copy of a shell command reaching a real clipboard: OSC 52 is written
  (as `y` writes it); tmux on a private socket does not show the clipboard.
- VS Code's terminal and Zellij, where `Ctrl+K` may not reach bzz: `Space k`
  and `open-palette` in `keymap.toml` are the ways round, read from R2, not
  run.
- Windows and macOS.

## Gates

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
