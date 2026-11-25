# v0.17.0 validation — task `demo2` (A2, and the demo's loose ends)

Plan: [`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
track A, D2, rules 19 to 22. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
Linux x86_64, 2026-11-25, branch `t3/implement-demo2-task` cut from
`v0.17.0` at `64156d3` (the merge of `demo`). The machine was shared with
other agents' builds throughout, which is in every time below.

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every bzz run by hand had a scratch home (`HOME` and the four
`XDG_*_HOME` in `<scratch>/home`, `DBUS_SESSION_BUS_ADDRESS` unset, `TMPDIR`
in `<scratch>/tmp`). `bzz-before` is the release build of `64156d3`,
`bzz-after` the release build of this branch.

## What was built

- **The Board in `#launch`** (`src/demo/board.rs`). The shipped `board`
  template is proposed as the owner's, loaded apart, and approved by its
  hash through `Apps` in the demo's own store (in memory; its three files
  in the demo's temporary folder), then opened in `#launch`. bzz's pane
  takes it from `Apps::runnable`, as any app: there is no other door. The
  records are a `SharedStore` held in memory (`live::Where::Memory`, which
  was test-only), seeded with eight cards of Harbor 2.4's last days.
- **A teammate moves a card** while the Board is on screen: 15 s after it
  is first shown, then at most once a minute, never within 10 s of a key or
  a paste in the Board, three moves in all; each is said in `#launch`
  (`Lin: Moved "Blog post review" to Done on the Board.`), so the newcomer
  sees who changed the shared records.
- **Scout uses the Board.** `@Scout add <title>` and `@Scout move <title>
  [to <column>]` (the verb first, after the mention) add or move a card
  when Scout answers, and Scout says what it did in the thread. Anything
  else about the Board gets one sentence of what it can do there; any other
  mention gets its usual answer, which now names the Board too.
- **Ada's welcome** names the Board, its keys (`5`, `n`, `]`) and the words
  for Scout; Ada says the same in `#launch`.
- **`:app`** in the demo: `open`, `close`, `zoom`, `brief` and `:apps` work
  as anywhere; every other verb (add, new, dev, share, unshare, pending,
  approve, refuse, rollback, enable, disable, remove) answers
  `Demo: only the Board.` The `:apps` panel's note says the Board is
  installed and approved.
- **Loose ends**: a huddle person who has not spoken says "quiet"; the
  demo's status line has no `HALFBLOCKS`; an answer is dated after what it
  answers; every notice of the demo fits the status line at 80 columns.

## Proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| The Board is the shipped template, approved in the demo's store, handed out by `Apps::runnable`; its files are in the demo's folder | unit | `cargo test --locked --lib demo::` (`the_board_is_the_approved_template_from_the_one_door`) | the runnable's bytes are the template's script, title `Board`, types `["card"]`, open in `#launch`; the app's folder is under the given paths |
| On the seeded cards, `n`, a title, Enter, `]`: the card is in Doing, written by the viewer | unit, headless session | `a_card_added_and_moved_is_in_the_next_column` | `To do (3)` → `To do (4)` → `Doing (3)`; the record's `column` is `doing` |
| **The same in the demo, in a PTY with no network**; leaving leaves nothing, the home byte-identical | PTY in a network namespace with loopback only (`unshare -rn`) | `cargo test --locked --test demo_offline_test` (`the_board_takes_a_card_offline_and_the_demo_leaves_nothing`), debug build | passes: `Space Space`, `launch`, Enter, `Board · 5 focus`, `5`, `n`, `Press kit`, Enter, `]`; `> Press kit` between `Doing (3)` and `Done (3)`; `q y`, `DEMO-ENDED 0`, 0 entries in the demo's `TMPDIR`, the home's tree (with a `config.toml` of its own) equal byte for byte |
| A teammate moves a card: the first 15 s after the Board is first shown, then not within a minute, not within 10 s of a key, not while the Board is not on screen; a move whose card the person moved is skipped; it is said in the channel | unit, a clock stepped by hand | `a_teammate_moves_a_card_calmly_and_not_while_the_person_types` | no move off screen for 600 s; at 614 s none, at 615 s `Blog post review` → Done and `Lin: Moved "Blog post review" to Done on the Board.`; none from 616 to 674 s; a key at 670 s: none at 675 and 679, one at 680; the third move skipped; none after |
| The teammate's move appears with no key pressed | tmux, release binary, 80×24 | scenario `t17-demo2-first-minute` steps 19–20 | `Moved "Blog post review" to` 14.9, 14.7 and 14.9 s after the step began, in the three runs below; `Done (4)` with `Blog post review` under it |
| …and not while the person types in the Board | tmux by hand, release binary, 80×24 | `<scratch>/run.sh 80x24 <scratch>/bzz-after`, `#launch`, `5`, then `j`/`k` every 3 s for 33 s, the screen read after each key | no `Moved` while typing (33.2 s, well past the first 15 s); it came 10.26 s after the last key (`typing-then-move.txt`) |
| Scout reads `add` and `move` as the first word, and nothing else as an order | unit | `scout_reads_add_and_move_and_nothing_else` | `@Scout add Ship party`, `please add a card: Order cake.`, `move launch tweet to Done`, `move the blog post`; "what is on the board?" → what it can do; "what do you do?" and "can you add more tests?" → its usual answer |
| Scout adds and moves cards and says exactly what it did, and what it can do when it cannot | unit | `scout_adds_and_moves_cards_and_says_what_it_did` | `Done: I added "Ship party" to To do on the Board in #launch.` (the card signed by Scout); `Done: I moved "Launch tweet" from To do to Doing …`; a card in Done: "in Done already"; an unknown title: "I found no card called …", the cards, what it can do; no title: "Which card?" |
| Through the relay's script, the card is written when Scout answers, not before | unit | `a_mention_that_asks_scout_writes_on_the_board_when_it_answers` | three answers (two typing lines, the message); no card before the message is signed; then the card in To do |
| The same on screen | tmux, release binary, 80×24 | `t17-demo2-first-minute` steps 9–14; by hand (`scout-add-thread.txt`, `scout-move.txt`, `launch-after-scout.txt`) | `↳ 1 reply` under `@Scout add Ship party`, `Done: I added "Ship party" to To do` in the thread, `To do (4)` with `Ship party` on the Board in `#launch` |
| The welcome names the Board, its keys and the words for Scout | unit; capture | `the_welcome_names_the_board_its_keys_and_the_words_for_scout`; `first-80x24.txt` | `Then our Board in #launch: 5 opens it, n adds a card, ] moves it on. Or ask: @Scout add Ship party` on the first screen at 80×24, below the three things |
| `:apps`, `:app open board`, `5`, `Space 5` work; the rest of `:app` says it is the demo | unit; tmux by hand, 160×45 | `what_needs_a_real_community_says_so`; `apps-160x45.txt`, `space5-160x45.txt` | `board (Board) · version 1 · open in #launch`; Enter opens it; `Space 5` → `Board · zoomed`, three columns side by side; 13 other verbs and a bare `:app` → `Demo: only the Board.` |
| A huddle person who has not spoken since the lanes appeared is "quiet", not "2s ago" | unit, 80×24 render; seen it fail | `cargo test --locked --lib in_the_demo_huddle_who_has_not_spoken_yet_is_quiet` | 30 s of the recording in `#team`, then `#infra` in the loop's silent first 0.35 s: no track has spoken, the panel says `quiet` three times and `ago` only in its scale; then Ada is live. **With the fix taken out it fails** (peer 1 had "spoken") |
| The same on screen | tmux by hand, debug build, 80×24 | `huddle-later-80x24.txt` | `Ada · ● live`, `Lin · quiet`, `Sam · quiet` on opening `:huddle` a minute in |
| No `HALFBLOCKS` in the demo's status line | capture | every capture | ` NORMAL … DEMO · generated, offline    ? help · q quit`; agent `look`'s L3 removes the word everywhere, so the line in `src/app.rs` is a one-line seam that its merge replaces |
| An answer is dated after what it answers, whatever the seconds; the teammate's wait lets it be dated a second before it is sent | unit | `an_answer_is_dated_after_what_it_answers` | a message dated now, now + 1 and now + 5: every answer later; teammates and Scout wait at least 2 s. The scenario's mention no longer needs a pause after Ada's answer: `t17-demo2-first-minute` writes `hello team`, reads `Welcome aboard`, and mentions Scout at once; its answer lands under the mention every run |
| Every notice of the demo fits the status line at 80 columns | unit; tmux | `every_notice_of_the_demo_fits_an_80_column_status_line`; `t17-demo2-first-minute` step 18 | each at most 24 columns (`Demo: needs a community.`, `Demo: a recorded huddle.`, `Demo: only the Board.`, `Demo: apps not built in.`); mode + notice + mark + hint ≤ 80; `Demo: only the Board.` whole on screen at 80×24 |
| Nothing is drawn at rest with the Board open (rule 20) | tmux `pipe-pane`, release binaries, 80×24, 10 s windows, twice each | `bzz-before`/`bzz-after`, `#team` and `#launch` | before: `#team` 50 and 25 bytes, `#launch` 0 and 25; after: `#team` 50 and 50, `#launch` 0 and 0. The bytes are a cursor-hide sequence of an empty frame (`ESC[39m ESC[49m ESC[59m ESC[0m ESC[?25l`), in both builds and with no app shown; agent `look`/L9's work, not this task's. The Board asks for a frame only when a card changes |
| The demo's scenarios, three times in a row | tmux, release binary | `BZZ_BIN=<scratch>/bzz-after BZZ_TMUX_SCENARIOS=t17-demo-first-screen,t17-demo-first-minute,t17-demo-huddle,t17-demo2-first-minute python3 scripts/test-tui-tmux.py`, three times | `4 of 4 scenarios passed` each time: 2.6/9.6/2.0/25.9 s, 2.5/9.5/2.7/25.6 s, 2.1/9.8/3.3/26.0 s. `t17-demo-first-minute` takes the welcome's new last line in its reaction step, and `t17-demo-huddle` the new notice |
| The size of the release binary | measured | `cargo build --locked --release --bin bzz` at `64156d3` (from `git archive`) and at this branch | 31 995 232 → 32 043 872 bytes: **+48 640** |

### What is left of the plan's size budget

The plan's limit is +350 KB over v0.16.0 for the whole demo. Agent `demo`
measured +277 888 bytes for its part (before the viz merge); this task adds
+48 640. Together 326 528 bytes: **23 472 bytes left** of 350 000 (31 872
of 350 KiB).

## Captures, read as a newcomer

In `<scratch>/caps/`: `first-80x24.txt` (the welcome with the Board),
`launch-80x24.txt`, `board-added.txt`, `board-moved.txt`,
`board-teammate.txt`, `launch-teammate.txt` (Lin's line beside the Board),
`scout-add.txt`, `scout-add-thread.txt`, `scout-move.txt`,
`launch-after-scout.txt`, `huddle-later-80x24.txt`, `notice-*.txt`,
`typing-then-move.txt`; at 160×45 `first-160x45.txt`, `launch-160x45.txt`,
`apps-160x45.txt`, `space5-160x45.txt`. Every step of the scenarios in
`<scratch>/scen1/` to `scen3/`.

Read as a newcomer: the welcome's fourth line is the Board, and in
`#launch` the Board stands beside Ada's message that names its keys, at
80×24 and 160×45. A card added is selected in To do; `]` puts it in
Doing at once. Lin's move comes with a line in the channel saying so.

## Not verified

- **The gates were run once, on the final branch** (see the report);
  `cargo deny` and `cargo audit` were not run: `Cargo.toml` and
  `Cargo.lock` are unchanged.
- **macOS and Windows**: not built or run here.
- **The second and third teammate moves on screen**: the one-minute gap is
  proven by the unit test with a stepped clock; on screen only the first
  move was watched.
- **Scout on a slow machine**: the Board is written when Scout's answer is
  signed (2.5 s after the mention); not measured under load beyond this
  machine's.
- **The Board's `q`** closes the Board in `#launch`, as it does anywhere
  (`:app open board` opens it again). A newcomer who presses `q` in the
  Board to leave it loses it from the channel until then; this is the
  template's key, unchanged here.
