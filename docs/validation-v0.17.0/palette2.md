# v0.17.0 validation: arguments as steps, and more to find (C9, C12)

Task `palette2`, branch `t3/palette2`, cut from `v0.17.0` at `59e3505`
(the merge of the palette, C1). Linux x86_64, 2026-11-25, on a machine
shared with other builds and relay runs (load 17 to 22). Buzz pin
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
items C9 and C12, rules 19 and 23. `<scratch>` is a folder outside the
repository.

## What was built

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

## Proofs

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
| Same: "Honey" finds the person (`↵ profile · Ctrl+D DM`), Enter opens the profile; "fixture" ends with `/ Search messages for «fixture»`, Up reaches it, Enter opens the search (`local + relay`) | tmux | `t17-palette2-find` | passed |
| Same, after the Board is installed (`t16-cli-app-new`, `-add`, `-approve`): "board" finds `▣ Open board here`, Enter opens it beside `#t9-general` | tmux | `t17-palette2-app` | passed |
| Same at 80×24 and 160×45: a person step with its `:dm` line, a text step with its usage, an expiry step | tmux | `t17-palette2-sizes` | passed |
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
- `<scratch>/relay4.log`: all of them together, see "Final run" below.

### Red runs

See below, "Red runs (made)".

### Captures

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

## Decisions taken here

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

## Not verified

- The theme preview on screen (colour): the text captures cannot show it;
  the App tests check the App's theme changes and comes back, and the
  scenario checks the step reopens on the theme in use.
- The DM opened by `:dm` was not opened twice to see the relay give back the
  same conversation; it is the DM picker's call.
- Communities: only one in the harness profile, so "Switch to …" was run in
  a unit test, not on the relay.
- `NO_COLOR`, Windows, macOS.

## Gates

See below.
