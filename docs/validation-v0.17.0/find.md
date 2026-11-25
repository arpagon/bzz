# v0.17.0 validation — task `find` (C3 to C7, A5, rule 19)

Linux x86_64, 2026-11-24/25. Branch `t3/implement-find-task`, cut from
`v0.17.0` at `543dbd1`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md).
Method: [`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository on the machine the work was
done on.

`U:` a unit test (`cargo test --locked --lib`, or `--bin bzz`, or one
`--test`). `T:` a tmux scenario of `scripts/tui-herdr-scenarios.toml` on the
release binary through `scripts/test-tui-tmux.py`: `automated` ones on an
empty isolated profile, `harness` ones on the pinned relay
(`scripts/test-relay.sh --tmux`). `C:` a clean container.

## Rule 19: nothing that worked stops working

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

## C3 — the help scrolls and lists the `:` commands

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

## C4 — the `:` line answers a mistake

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

## C5 — the channel finder

| Claim | Proof | Result |
|---|---|---|
| ↓, Tab and Ctrl-n move the selection; ↑, Shift-Tab and Ctrl-p move it back, wrapping; a new letter starts at the top; the selected row has `›` | `U: app::find_tests::the_finder_selection_moves`, `ui::finder::tests::the_selection_moves_wraps_and_restarts_with_the_query` | pass |
| A DM is found by the person's name and drawn `@Scout`; the last row starts a DM with the query as the search | `U: ui::finder::tests::dms_are_found_by_name_and_a_new_dm_comes_last`; the same App test | pass |
| On the pinned relay: `Space Space`, `gen` selects `+general`, ↓ selects `#t9-general`, Enter opens it (`message #t9-general`) | `T: t17-find-finder` | pass |
| The finder as v0.16 scenarios use it (`t9-gen` Enter) still opens the first row | `T: t14-edit-own-message`, `t9-system-group-read-only`, `t14-sections-and-solo` | pass |

## C6 — the leader panel

| Claim | Proof | Result |
|---|---|---|
| Every continuation of `Space` is in the panel at 80×23, 120×31 and 40×12 (two columns when one does not fit); every row is inside the frame | `U: ui::help::tests::leader_panel_shows_every_key_at_80x24` | pass |
| The panel appears 300 ms after the key, on Tokio's clock: not at 150 ms; at 300 ms it opens, and five ticks ask for one frame between them; five seconds later it is still open and the sequence still waits | `U: app::find_tests::the_leader_panel_waits_300_ms_and_asks_for_one_frame` | pass |
| A sequence whose panel cannot show (another overlay holds the screen) ends after 750 ms, as in v0.16 | `U: app::find_tests::a_sequence_behind_another_overlay_still_expires` | pass |
| On the release binary, at 80×24 and at 120×32, the panel lists all eleven keys, `s cycle channel sort` included, appears 364 to 438 ms after `Space` (300 ms and the scenario's 50 ms poll), is still there 1.5 s later, and Esc closes it | `T: t17-find-leader` | pass |

Seen: `<scratch>/captures/t17-find-leader/01.txt` (80×24) and `04.txt`
(120×32).

## C7 — `d`, `u`, `:status` (D5, D6)

| Claim | Proof | Result |
|---|---|---|
| `d` and `u` are bound in the workspace only; the Inbox keeps `U` and does not get `u` | `U: app::find_tests::d_on_another_persons_message_says_why_not`; the pin test | pass |
| `d` on another person's message says `delete own message: only your own message can be deleted`, the menu's words, from the same function; with nothing selected `select a message first`; outside the conversation it says to select a message there | the same App test (`context_refusal`) | pass |
| A bare `:status` shows the status with when it ends, or that there is none; `:status clear` still clears | `U: app::find_tests::bare_status_shows`, `bare_status_does_not_clear` | pass |
| On the pinned relay: `d` on the fixture's first message (another person's) says why not; `G`, `d`, `y` delete my own, and its text leaves the screen; `u` says `marked #t9-general unread`; `:status t17 lunch`, then `:status` shows `your status: t17 lunch · :status clear clears it`; `:status clear`, then `:status` shows `you have no status` | `T: t17-find-delete-unread-status` | pass |

## A5 — the first commands in the shell

| Claim | Proof | Result |
|---|---|---|
| Without a keychain `bzz identity new` says so and names `--backend encrypted-file`; another backend's error is unchanged; a label with a space is quoted in the line it prints | `U: tests::a_missing_keychain_names_the_encrypted_file` (`cargo test --bin bzz`) | pass |
| `bzz community add --help` describes the command and each argument | `U: tests::community_add_help_describes_itself` | pass |
| In a clean container (`bzz-r4-firstrun`: Debian stable slim with `libasound2`, `--network none`, a scratch home, no D-Bus), the release binary: `identity new --label personal` prints `no keychain to keep the key in: … Keep the key in a file protected by a passphrase instead: bzz identity new --label personal --backend encrypted-file` (v0.16.0 printed `identity is locked: the OS credential service is unavailable; unlock it and restart bzz`, the research's `capture/get-started.txt`); with `--backend encrypted-file` it prints `identity:`, `npub: npub1…` and `pubkey:`; `community add --help` describes itself | `C: <scratch>/a5/get-started.sh`, output `<scratch>/a5/get-started.txt` | pass |
| `:community add` inside bzz names the exact shell command | C4 above | pass |

## Pinned relay

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

## Gates

On the final commit of the branch, with the build environment of the rules:

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | pass (lib 792, every integration test) |
| `cargo test --locked --no-default-features --lib` | pass (502) |
| `cargo deny check`, `cargo audit` | not run: `Cargo.toml` and `Cargo.lock` unchanged |
