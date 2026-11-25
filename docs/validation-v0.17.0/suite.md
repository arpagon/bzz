# v0.17.0 — task suite: the scenario suite green, in any order

Every run below: the release binary of this branch, the pinned relay
(`./scripts/test-relay.sh --tmux`, `BZZ_BUZZ_SOURCE=<scratch>/buzz-pin`,
`BZZ_TMUX_SIZE` as given, `BZZ_TMUX_CAPTURES=<scratch>/runs/<run>/captures`),
on this machine, through tmux on a private socket. After each run the
`buzz-harness` containers were counted (0) and the relay lock tried
(`flock -n /tmp/bzz-test-relay-$(id -u).lock true`: free). Logs and captures
are in `<scratch>/runs/`.

## Claim: before this task the suite does not pass as a whole

Rung: the suite itself, on the head the task started from (`0d96e80`).

| Run | Result |
|---|---|
| `base-200`, 200x50 | 44 of 50 passed. Failed: `t17-palette-unavailable-stays`, `t17-palette-sizes`, `t17-sound-call`, `t17-palette2-theme-react`, `t17-palette2-find`, `t17-palette2-app` |
| `base-80`, 80x24 (with the new fixture channels already in) | 32 of 50 passed |

Why each failed, from the capture at the failing step:

| Scenario | What the screen showed |
|---|---|
| `t17-palette-unavailable-stays` | `#t9-general`'s newest message was the viewer's own (the Board's brief `t16-m6` posts), so "Delete my message" could run and asked `delete message?` instead of saying why not |
| `t17-palette-sizes` | the same at 80x24: no `can't now` |
| `t17-sound-call`, `t17-palette2-app` | `Generated custom emoji`, used to check that `#t9-general` opened, had scrolled away under the brief |
| `t17-palette2-theme-react` | `3 G k k k` no longer reached the custom emoji's message |
| `t17-palette2-find` | the profile's `r refresh · Esc close` is now on its frame's foot, not after `profile ·` |
| at 80x24, t9 to t15 | the messages used as "the channel is open" (`Generated custom emoji`, `Generated agents channel`, `Generated namesakes channel`) wrap in a narrow timeline |

The Herdr runner refused the manifest: `./scripts/test-tui-herdr.sh --list`
printed `Herdr scenario has an invalid profile, or args outside a harness
scenario` (the automated `bzz demo` scenarios), and would then have refused
`motion` and, after the merge of `layout`, the `mouse` step field.

## Claim: each scenario stands alone, in any order

Rung: the suite run in other orders (`BZZ_TMUX_ORDER`, new; each scenario
still after what it `needs`). Each run found a dependency, fixed before the
next:

| Run | Order | Result | What it found |
|---|---|---|---|
| `rev-200` | reverse | 42 of 47, 3 skipped | the Board left open in `#t17-app`; `:apps` read as "open nowhere"; `↳ 1 reply` matched an older thread; `agent move` before `move-mismatch`; Scout not yet known to the palette |
| `rev2-200` (after the merge of v0.17.0) | reverse | 46 of 49, 3 skipped | the first start of a fresh profile is `online` before its channels; `t17-layout-f7` opened the thread of a join row; `t16-app-pane` counts cards that `t16-m6` made |
| `shuf1-200` | `shuffle:1` | 50 of 52 | the approval banner names the Board's waiting version and hides the newer one |
| `shuf2-200` | `shuffle:2` | **52 of 52**, 208 s | — |

## Claim: the suite is green at 200x50, three times in a row, and at 160x45 and 80x24

Rung: the suite in the manifest's order, on `e3367b6` (the branch with
`v0.17.0` at `ecff610` merged in; the one commit after it only removes the
runner's temporary folder at the end), release binary built from it.

| Run | Size | Result | Time |
|---|---|---|---|
| `fin3-200-a` | 200x50 | 52 of 52 passed | 210 s |
| `fin3-200-b` | 200x50 | 52 of 52 passed | 207 s |
| `fin3-200-c` | 200x50 | 52 of 52 passed | 207 s |
| `fin3-160` | 160x45 | 52 of 52 passed | 207 s |
| `fin3-80` | 80x24 | 48 of 48 passed, 4 skipped | 183 s |
| `fin3-join2` | `t17-join-open` (kind `join`) | 1 of 1 | 72 s |
| `fin3-members` | `t17-join-members`, relay closed (`BUZZ_REQUIRE_RELAY_MEMBERSHIP=true`, a random `RELAY_OWNER_PUBKEY`, `BZZ_FIXTURE_AGENTS=0`) | 1 of 1 | 71 s |

Skipped at 80x24, and why: `t15-approval-approve-refuse` (`min_size =
"120x30"`: the waiting line is one row of 108 characters, cut under 120
columns), `t15-agent-new-form` (`120x30`: the form's sentences wrap inside
its frame between the words a step reads), `t15-coding-harnesses-in-panel`
(`120x36`: the agent's record is longer than the panel under 36 rows),
`t15-cli-agent-activity` (it needs the first, which was skipped). Each size
was measured, below.

With agents on, the fixture fails on a closed relay: `the relay did not
confirm the agent's profile` (`examples/harness_fixture.rs`, the local
agent; unchanged by this task). The members-only scenario needs no agent.

The automated scenarios (no relay), `BZZ_TMUX_SCENARIOS=automated`:

| Size | Result | Time |
|---|---|---|
| 200x50, three times | 17 of 17 each | 66, 66, 67 s |
| 160x45 | 17 of 17 | 67 s |
| 80x24 | 17 of 17 | 66 s |

Every automated scenario names its size but `startup-help-quit`,
`custom-keymap-inbox` and `v013-panels-empty-profile`, which hold at all
three.

## Claim: `needs` and `min_size` are checked and reported

- `BZZ_TMUX_SCENARIOS=t15-approval-approve-refuse,t15-agent-new-form,t15-coding-harnesses-in-panel`
  printed `scenario t15-cli-agent-move added: t15-agent-new-form needs it`
  (and the rest of the chain) and ran them first (`probe-*` runs).
- At 80x24 (`fin2-80`): `SKIPPED: holds at 120x30 or larger; this run is at
  80x24` and `SKIPPED: needs t15-approval-approve-refuse, which skipped`;
  the run's exit is 0 and its last line names the skipped ones.
- `rev-200`: `t16-app-looper-composer SKIPPED: needs t16-app-pane, which
  failed`, instead of a failure at its second step.

The smallest sizes were measured: `t15-approval-approve-refuse` and
`t15-agent-new-form` fail at 100x30 and pass at 120x30;
`t15-coding-harnesses-in-panel` fails at 120x30 and 100x40 and passes at
120x36.

## Claim: the Herdr runner reads the manifest

Rung: its manifest reader only. `bash -n scripts/test-tui-herdr.sh`;
`./scripts/test-tui-herdr.sh --list` lists 69 rows and says that
`t17-join-not-there` (isolated profile) and `t17-layout-mouse` (mouse
steps) are run by the tmux runner only; `--scenario t17-palette2-app` says
`needs t16-cli-app-approve, which is not selected`. Not run: Herdr is not
for this task, so the bash that drives a pane (the new `args` and `motion`
of automated scenarios) was read, not run.

## What bzz does that a scenario had to work around

- **Your own local agent cannot be found in the palette on a fresh
  profile.** `palette_people` (`src/app/palette.rs`) lists profiles bzz
  holds and verified agents; a hosted agent of `config.hosted_agents`
  without a profile yet is not listed, so on a profile's first starts
  "Start a DM", `sc` showed "No people yet: bzz knows a person once it has
  seen them" for 15 s (capture: `rev-200`, `t17-palette2-sizes`). The
  scenarios now wait for Scout's join row first.
- **`online` comes before the channel list on a profile's first start.**
  The finder, typed into at once, held only the selected channel and "start
  a DM", and Enter opened the DM picker with the typed name (`rev2-200`,
  `t17-layout-board-captures`, step 2). The runner now starts the profile
  once before the suite.

## What was wrong, and what changed

| Scenario | What was wrong | What changed |
|---|---|---|
| `t17-palette-unavailable-stays`, `t17-palette-sizes` | `#t9-general`'s newest message was the viewer's own | `#t17-palette` (fixture), newest message another person's; the row selected checked first |
| `t17-palette2-theme-react` | `3 G k k k` reached another row | `#t17-react`: one message, `3 G` |
| `t17-find-delete-unread-status` | wrote and deleted in `#t9-general` | `#t17-find` |
| `t17-sound-call` | its check of `#t9-general` scrolled away | `#t17-sound`, both people members |
| `t17-palette2-app` | needs the Board; left it open in `#t9-general`'s view of `:apps` | `needs = ["t16-cli-app-approve"]`, `#t17-app`, `:app close` at the end |
| `t17-registry-app-footer` | needs an installed app | `needs`, `#t17-palette` |
| `t17-palette2-find` | the profile's keys moved to its frame's foot | a pattern over the frame, the name and the keys |
| `t17-palette2-dm`, `t17-palette2-sizes` | Scout not yet known on a fresh profile | wait for `Scout joined` at the top of `#t15-agents` |
| `t17-layout-f7` | `k Enter` opened a join row's thread | `#t17-thread` |
| `t17-layout-board-captures` | needs the Board | `needs` |
| `t17-layout-mouse` | the demo's longer notices; the channels' order; the hint now follows the focus | three turns of the wheel, `#design` on row 4, the palette's key at column 70 |
| `t17-demo2-first-minute` | Scout's answer wraps beside the conversation | a pattern across the row beside it |
| `t17-find-commands`, `t17-join-not-there` | "Join a community" is also a row of the first screen | the form's own words |
| `t17-join-*` | the join form is 100 columns wide, centred at 160 | the frame of that size, edge to edge or centred |
| `t17-join-open` | the six new channels push `#t9-general` off the finder at 80x24 | narrowed first |
| t9, t14, t15 that open a channel | a message used as "the channel is open" wraps or scrolls away | the composer's title or the status line |
| `t15-agent-answers-mention` | `↳ 1 reply` matched an older thread | this mention's row |
| `t15-coding-harnesses-in-panel` | the `BZZ_GROK` sentence wraps where the fixture's path puts it | any space may be a break; `min_size` |
| `t15-approval-approve-refuse`, `t15-agent-new-form` | do not hold at 80x24 | `min_size` |
| t15 and t16 chains | dependencies silent | `needs` (credential, show, move, receive, activity, the form; add, approve, the M3 and M4 chain, M6 after M4, M5 and the Poll after the Board is approved, since the banner names the oldest version waiting) |
| `t16-app-pane`, `t16-m6-share-offer-approve` | `:apps` read as one exact line | the Board's row with where it is open |
| the runner | nothing for order, needs, sizes; first starts; ~9 MB left in `/tmp` per run | `needs`, `min_size`, `BZZ_TMUX_ORDER`, a warm-up of each profile, the folder removed |
| the Herdr runner | refused the manifest | reads it (above) |

## The final head, and the gates

`fin4-200`, on `74cbba6` (the runner now removes its folder): 52 of 52 at
200x50 in 216 s; relay down, lock free, the run's folder gone.

Gates on `74cbba6`: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets` (2,106 passed, 0
failed, 44 ignored, in 72 test binaries) and `cargo test --locked
--no-default-features --lib`: all exit 0. Cargo.toml and Cargo.lock are
unchanged, so deny and audit were not run.
