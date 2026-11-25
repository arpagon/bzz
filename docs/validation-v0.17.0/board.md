# v0.17.0 — task `board`: F1, F2, F3, F4, F5

Linux x86_64, 2026-11-25, on branch `t3/implement-board` cut from
`v0.17.0` at `2ff8301`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md).
`<scratch>` is a folder outside the repository; its files are local.
Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.

The five defects were seen while filming v0.16.0 with several clients on
one relay (Buzz desktop 1.0.1 with a plugin, bzz, a third client in
Python), recorded in the experiments `buzz1/apps-interop-2026-11-24/` and
`promo-video/feature-apps/` (local, untracked).

"Red" below is the same test run on the code of `2ff8301`: the fix's
source files put back (`git stash push -- <files>`), the test kept.

## F1 — The Board keeps a `null` field and an empty object

**Where the loss was.** In the bridge, not in the script. A record's JSON
becomes a Lua table (`value::to_lua`): a `null` is nil, so the field is
simply absent, and `{}` is an empty table, which `value::to_json` sends
back as `[]` (`docs/app-api.md` says so). The Board copies the content
with `pairs` and changes `column`; nothing it can write tells those values
apart. The same holds for a list with a `null` inside (a table with a hole,
sent as an object `{"1":…,"3":…}`) and a whole number past a Lua integer (a
float, `1.2345678901234567e+19`).

**What changed, and what a script sees.** Nothing a script sees: a null
still arrives as nil, an empty object as an empty table, a whole number
past 2^63 as a float, exactly as `api = 1` promises. What changed is what
an update writes. The session holds the record at the revision the update
names, and `value::as_read` puts back, against it, only what the script
could not tell apart and left as it was handed: a `null` it left out, an
empty table where the record held `{}` (or an object of nulls), the `null`
places of a list, a number equal to the one read. What the script changed
is written as it wrote it; two whole numbers are compared as whole
numbers, so `2^53` and `2^53 + 1` stay two. `docs/app-api.md` has a new
paragraph, "An update keeps what Lua cannot show". `docs/app-api.d.lua`
needed nothing: what it says of a null arriving as nil is still true.

| Claim | Proof | Red | Result |
|---|---|---|---|
| A card another client wrote with `null`, `{}`, nested nulls, lists with nulls, `[]`, `1.0`, 2^64-ish and seventeen digits comes back from the Board as written, but its `column`; and back again | `apps::session::tests::a_card_another_client_wrote_comes_back_from_the_board_as_it_was_read` (the Board fixture, in-memory store) | `"gone"` and `"nested.b"` dropped, `"meta": []`, `"list": {"1":1,"3":3}`, `"big": 1.2345678901234567e+19` | pass |
| What a script changes is written as it wrote it (a null field it sets, `{}` it makes a list, 2^53+1, a float) | `a_change_of_the_script_is_written_as_it_wrote_it` | `"keep": null` dropped | pass |
| The function alone, both ways | `apps::runtime::value::tests::an_update_keeps_what_lua_could_not_show`, `an_update_writes_what_the_script_changed` | (new function) | pass |
| On the pinned relay, a card a second client wrote with a null field, an empty object, a list with a null, a 20-digit whole number and a 17-digit `order` comes back from the Board byte for byte, but `column` | `real_relay_v017_the_board_keeps_fields_its_own_burst_deletions_and_new_names`, part F1: the head event's content after Ana's move equals Ben's with `todo` → `doing` | not run red on the relay | pass |

The six fixtures and the three templates pass their cases unchanged:
`cargo test --lib apps::` (274 passed) and the apps bench below.

## F2 — The Board's own writes do not conflict

**Where it belongs: the session.** A script cannot know the revision its
own write will make before the reply, and every app would have to queue
its own writes to stay out of this. The service is shared with the
agents' tools, which answer per call. The session already holds the set
and the budget, so the rule is there: an app's writes to one record are
sent one after the other; a write built on a revision that the app's own
earlier write replaced is sent on the app's own new revision. A revision
of someone else is never put in its place, so a real conflict is still a
conflict. Each write is spent from the budget of 30 a minute when it is
sent; past it the script gets `rate_limited`, before anything is sent.

The Board also leaves a card as shown while a later move of its own to
that card is on its way, so the card does not flick back to an earlier
column between the answers, and says `Saving...` until the last of them
is answered (it said `Saved.` at the first). `docs/app-api.md` has a new
point under the record forms: an app's own writes to one record do not
conflict.

**Found on the way, in the records service.** The service took the
sync's echo of the caller's own revision as a new head when it arrived two
or more revisions late (it caught one revision late only), and the
session would then tell the app of its own old revision as a change. In a
burst this can happen. `State::apply` now leaves alone a revision that the
caller's own writes, followed back from the head it holds, replaced;
another client's older revision is told as before
(`service::records::tests::an_echo_of_a_revision_the_callers_own_writes_replaced_is_no_change`,
red without the three lines). This was found by reading while a relay run
failed for another reason (the test's own wait, see below); it was not
seen to happen on the relay.

| Claim | Proof | Red | Result |
|---|---|---|---|
| 25 moves of one card before any reply: one write on its way at a time, 25 writes, none refused, none sent again, the card where the last key put it all along | `the_boards_own_moves_do_not_conflict_when_keys_come_faster_than_replies` | 25 writes on their way at once | pass |
| A real conflict with another client is still refused and merged; nothing lost | `a_real_conflict_with_another_client_is_still_refused_in_a_burst` | 30 writes: the stale moves conflicted until the budget ran out | pass |
| Queued moves still spend the budget: 31 moves, 30 sent, the 31st "Too many changes. Retry in 60 s." | `queued_moves_still_spend_the_apps_budget` | more than one write on its way | pass |
| On the pinned relay: 25 moves while no reply has come; no status but `Saving...` and `Saved.`; the app's budget spent by 27 writes in all (2 before, the 25: a refused move would have cost its write and its merge another); 27 revisions of Ana's in the relay's history; the head byte for byte as Ben wrote it but `column` | relay test, part F2 | not run red on the relay | pass |
| A real conflict on the relay between two profiles still merges | `real_relay_app_session_two_profiles_share_a_board` (v0.16 M2, unchanged): five simultaneous moves, merged by ana, ana, ben, ana, ben | — | pass |

## F3 — "Save failed." and "Loaded N cards."

| Claim | Proof | Red | Result |
|---|---|---|---|
| A move of a card deleted meanwhile says `This card was deleted by cy.` and drops the card, whether the deletion or the refusal reaches the Board first (until bzz says who: `This card was deleted meanwhile.`) | `a_move_of_a_card_deleted_meanwhile_says_who_deleted_it` | `Save failed.` | pass |
| `Loaded N cards.` counts the cards shown: a list asked for before a deletion, and a card of a column the Board does not show, are not counted | `the_board_counts_the_cards_it_shows` | `Loaded 5 cards.` with four shown | pass |
| On the pinned relay: Ben deletes a card as Ana moves it: `This card was deleted by ben.`, the card gone; `r` then says `Loaded 1 cards.` with one shown | relay test, part F3 | — | pass |

The bench's specs (`tests/apps-bench/spec/board.md`,
`board-priority.md`) say the new texts. The Board with priority counts
the cards on the board whatever its priority filter.

## F4 — A number reaches the agent as the relay holds it

**Where the loss was.** `serde_json` parses floats with a fast algorithm
that is off by one unit in the last place for some numbers of seventeen
significant digits, unless its feature `float_roundtrip` is on: measured
on serde_json 1.0.151, `1795537221.1991239` parses to the double printed
`1795537221.199124`; with the feature, to the double printed as the text
it came from. bzz parsed the relay's content and printed the parsed
double again, in the records tool and in the app runtime alike (the app
path is the same parse, then Lua's exact double, then serde_json's
shortest printing). The feature is turned on in `Cargo.toml`. No new
crate: `Cargo.lock` did not change (`--locked` builds pass).

| Claim | Proof | Red | Result |
|---|---|---|---|
| An `order` of seventeen digits reaches the agent through `records` as the relay holds it, and goes back unchanged through `update_record` | `tests/agent_records_test.rs` `a_number_reaches_the_agent_and_the_relay_as_the_relay_holds_it` (the scripted harness, bzz's own host, a scripted relay of records) | the tool answered `1795537221.199124` | pass |
| The app runtime's path: relay text → Lua → update keeps `1795537221.1991239`, a 20-digit whole number and `5e-324` as text | `apps::runtime::value::tests::a_number_crosses_the_script_as_the_relay_holds_it` | not run red | pass |
| On the pinned relay, Ben's 17-digit `order` is back byte for byte after Ana's Board moved the card | relay test, part F1 | — | pass |

What it does not do: a number with more digits than a double holds is
still rounded to the nearest double. A text that is not the shortest form
of its double (`1.50`) comes back as the shortest (`1.5`).

## F5 — bzz refreshes a profile name it holds

**Why the newer profile did not replace the held one.** Not the timestamp
comparison: the store keeps the newest by `(created_at, event_id)`. bzz
asked the relay for a profile only for a person it held none of (the
timeline's authors, an app's signers), and none of its subscriptions
carried kind 0. So the first profile it got was the one it kept, also
after a restart, unless that person happened to write in the channel bzz
opened on. An open app was given only the names it had none of, so a new
name never reached it either.

What changed: the global stream carries kind 0 (a profile changed while
bzz runs arrives as any event and replaces the held one); each person
shown is asked for once a run, held or not (a profile changed while bzz
was closed); an open app is given the names of everyone it shows
(`AppSession::people`), and `set_names` passes on only what changed.

| Claim | Proof | Red | Result |
|---|---|---|---|
| The global stream asks for kind 0 | `tests/session_test.rs` `global_aux_and_membership_subscriptions_match_buzz_routing` | fails at the new assertion | pass |
| A renamed profile reaches an open app (`ctx.user.name` from Watcher to Cy) | `app::pane::tests::a_renamed_profile_reaches_an_open_app` | `me Watcher` never shown: the app was never given a name for someone it had one for | pass |
| On the pinned relay: Ana holds Ben's profile "Watcher"; Ben renames himself "Cy"; the global stream brings it (118 ms after it was sent) and the store then shows "Cy"; a profile renamed again is fetched for a held person ("Cy 2") | relay test, part F5 | — | pass |

Not shown at this rung: the timeline of a running bzz redrawing the new
name. The live event takes the path every event of the global stream
takes (applied to the store, `cache_dirty`, profiles read again); no
scenario of the TUI was added for it.

## Relay

```sh
ln -sfn "$CARGO_TARGET_DIR" target; unset CARGO_TARGET_DIR
BZZ_BUZZ_SOURCE=<main checkout>/runs/buzz-pin BZZ_RELAY_NOCAPTURE=1 \
  BZZ_RELAY_TEST_FILTER=real_relay_v017 ./scripts/test-relay.sh
BZZ_BUZZ_SOURCE=… BZZ_RELAY_TEST_FILTER=real_relay_app_session_two_profiles_share_a_board \
  ./scripts/test-relay.sh
```

Both green, each on a harness of its own (the lock taken, the containers
and volumes removed at the end, in the logs):

```
F1: Ben's card after Ana's Board moved it: {"big":12345678901234567890,"column":"doing","gone":null,"list":[1,null,3],"meta":{},"nested":{"a":1,"b":null},"order":1795537221.1991239,"title":"Odd"}
F2: 25 moves answered in 4340 ms; status lines seen: {"Saved.", "Saving..."}
F2: Ana's Board sent 27 writes in all; the relay's history holds 28 revisions, 27 of Ana's
F5: Ben's new profile reached Ana's global stream 118 ms after it was sent
test result: ok. 1 passed; 0 failed
the five simultaneous moves were merged by ["ana", "ana", "ben", "ana", "ben"]
test result: ok. 1 passed; 0 failed
```

The 4.3 s of the burst include one second the test waits after it; one
write at a time means 25 moves take 25 round trips (about 130 ms each on
this machine, at a load of about 20). The first runs of the test failed
on the test itself: it waited for `Saved.`, which the screen said already
before the keys were handled, and read the relay while the burst was
going on.

## The apps bench and the hostile suite

```sh
APPS_BENCH_OUT=<scratch>/bench-out ./scripts/test-apps-bench.sh
```

Debug build: every case of the six fixtures at 80×24 and 40×16 (board
12/12, poll 12/12, rota 14/14, approvals 15/15, panel 15/15,
board-priority 17/17), reload 20/20 at both sizes, pushes 8/8, the
hostile script 29/29 (rule 15). `PASS: everything passed.`

## Gates

All green on the last commit: `cargo fmt --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo build --locked --example
scripted_harness`, `cargo test --locked --all-targets --no-fail-fast` (64
test binaries ok), `cargo test --locked --no-default-features --lib`,
`cargo deny check` (advisories, bans, licenses, sources ok) and `cargo
audit` (exit 0, the allowed warnings it had before). `Cargo.lock` is
unchanged. One earlier run of the whole suite failed once on
`apps::runtime::tests::vm::an_endless_loop_is_stopped_by_time` (a loop of
100 000 steps stopped by its 250 ms at a load of 23); it passed three
times alone and in the last whole run.

## Captures

`bzz app try <scratch>/home/tasks --seed <scratch>/home/seed.json --size
80x24` and `--size 160x45`, the Board template on a scratch home, with
three cards in the columns, one with `"gone": null` and `"meta": {}`, and
one in a column the Board does not show: both say `Loaded 3 cards.`
(`<scratch>/capture-board-80x24.txt`, `capture-board-160x45.txt`).
