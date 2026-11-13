# bzz v0.14.0 validation

**Status:** M0–M5 implemented and proven on branch `v0.14.0` (PR #8); release acceptance recorded below (2026-11-13). Plan:
[`docs/planning/2026-11-12/v0.14.0.md`](planning/2026-11-12/v0.14.0.md).
Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5` (2026-11-13), Rust 1.98.1.
`block/buzz-app` reference revision `ce63b93d`.

## M0 — Baseline

All checks ran locally on Linux x86_64 on 2026-11-12. On PR #8, `ci.yml`
(run 43356913999 for M0, 43360812858 after M1) and `integration.yml` (runs
43356913974 and 43360812906) passed on Linux, macOS Intel, macOS Apple
Silicon and Windows. `audio (windows-latest)` fails as in v0.13.0 and is
experimental: hosted Windows runners have no audio endpoint.

| Gate | Command | Result |
|---|---|---|
| Lock change | `Cargo.lock` diff | 3 `source` lines (`buzz-core`, `buzz-sdk`, `buzz-audio-client`); no other package moved |
| Format | `cargo fmt --check` | clean |
| Lint (audio) | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| Lint (view-only) | `cargo clippy --locked --all-targets --no-default-features -- -D warnings` | clean |
| Tests (audio) | `cargo test --locked --all-features` | 456 passed, 0 failed, 19 ignored (same count as v0.13.0) |
| Tests (view-only) | `cargo test --locked --no-default-features` | 455 passed, 0 failed |
| Supply chain | `cargo deny check`; `cargo audit` | ok; the 4 allowed warnings recorded for v0.13.0, no new advisory |
| Pinned relay | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh` | 13 passed |
| FI-enforcing relay (T4) | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh --nip-fi` | 4 passed |

The pin checkout is a worktree of the third-party cache at `runs/buzz-pin`.

**Note:** `target/debug` with full debuginfo had grown to 58 GB and filled
`/home`. `runs/env.sh` now sets `CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`;
a full test build then takes 6.8 GB.

## M1 — Message edits

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| The latest live edit governs text; ties break on the greatest id; any hex case; arrival order does not matter; an edit before its original still applies; threads too | `tests/message_edits_test.rs` (6), `protocol::edit` unit tests (5) | 6 of 6 failed with edit application disabled | pass |
| An edit replaces the attachment set; an empty set removes them | `an_edit_replaces_the_attachment_set_and_an_empty_set_removes_them` | failed (above) | pass |
| Foreign, cross-channel and deleted edits do not apply; a verified agent owner may edit the agent's message | message_edits_test, `store::edits` unit tests | failed (above) | pass |
| Migration 0016 indexes edits stored by v0.13 (first valid `e`, lowercased) | `migration_indexes_edits_stored_by_older_versions`; `store_migration_test` at schema 16 | n/a | pass |
| Search and Inbox previews use the edited text; deleting the edit restores the original | `search_indexes_the_governing_edit_and_falls_back_when_it_is_deleted`, `search_and_inbox_previews_follow_the_governing_edit` | n/a | pass |
| `effective_edit` v1 responses are checked; old relays (array) are refused; same-second edits stamp after the head; a head over 60 s ahead is refused | `protocol::edit::effective_responses_are_checked_and_old_relays_refused` | n/a | pass |
| Against the pinned relay, an edit from bzz keeps the attachment (exact `imeta` and markdown line), a second same-second edit wins, `effective_edit` returns it, the media stays readable, another author is refused | `real_relay_message_edits` via `./scripts/test-relay.sh` (14 passed) | with `build_edit_with_media(.., &[])` the test failed at "attachments kept" (13 passed, 1 failed) | pass |
| The release TUI edits an own message with `e` and shows "(edited)" | Herdr `t14-edit-own-message` via `./scripts/test-relay.sh --herdr` (4 harness scenarios passed) | n/a | pass |
| "(edited)" follows the body and is absent on unedited or deleted messages | `an_edited_message_shows_the_marker_after_its_body` | n/a | pass |

Gates after M1: fmt clean; clippy clean (audio and view-only); 473 tests
passed (audio build).

Bzz keeps the governing attachments on an edit; adding or removing
attachments while editing is not supported (the composer says so).

## M2 — Buzz 1.0 as identity provider

Real proof with **Buzz 1.0.1** (`block/buzz-app` `v1.0.1`, source commit
`dfdb5582e6810e76681e24a33c3489ba2aa77e5c`, Linux x86_64 AppImage checked
against the release's `SHA256SUMS`), 2026-11-12, in experiment
`runs/experiments/buzz1/pairing` (local, not tracked):

- **Sandbox.** Bubblewrap with an empty tmpfs over `/home` and the host
  runtime directory, so the real home, session bus and keyring are not
  reachable; a disposable home and a private D-Bus session with its own
  unlocked keyring are bound back; Xvfb `:77`. Verified from
  `/proc/<pid>/root` and `environ` of the running Buzz. Firejail, as in the
  host's `buzz-firejail`, could not be used: this shell has NoNewPrivs, and
  under a systemd user unit Buzz's WebKit process crashed (SIGSEGV, seen with
  strace) when the welcome screen changed. Driven through AT-SPI and XTest.
- **Relay.** The pinned relay with `RELAY_URL=wss://<host>`, reached through
  a Cloudflare quick tunnel, because Buzz 1.0 accepts only `wss`/`https`
  communities. Generated identity (key typed into Buzz, never shown); its
  pubkey seeded as a relay member; data generated. Tunnel and relay were torn
  down after.

| Claim | Proof | Result |
|---|---|---|
| bzz pairs with Buzz 1.0; the approval names bzz and the relay; the paired pubkey is the generated one | `bzz identity pair-desktop` → Buzz dialog "bzz wants to use Buzz as you on wss://…", Allow → `paired … pubkey: 7fccc5bd…` (matches) | pass |
| `describe`: role owner, the bound relay, methods incl. `sign_event`, `nip44_*`, `federated_identity`, `auth_tag` | probe (`_buzz1_probe.rs`) | pass |
| What Buzz 1.0.1 signs for bzz: 9, 40003, 7, 30315, 20001, 20002, 30078, 41010, 41012, 45010, 30030, 22242, 27235, 24242; refuses 5, 1984, 0, 41011, 9007, 9021, 9022, 9002, 48100, 48103, 24810, 30180 | probe, every kind bzz signs as the human | matches `operation_policy` exactly (`the_tables_match_the_provider_contract`) |
| The release TUI works end to end through Buzz 1.0: AUTH, history, send, edit, react; Buzz shows the message, "(edited)" and the 👍 | Herdr pane + Buzz screenshot | pass |
| Red: before M2, delete was offered, confirmed, then failed with "authentication failed: Buzz Desktop refused the request" | Herdr capture at the v0.14 M1 build | observed |
| After M2: report and huddle start are refused at once with "Buzz does not let other apps do this; do it in Buzz"; the first delete says so after confirming, and the actions menu then shows delete unavailable with that reason | Herdr capture at the M2 build | pass |
| Signer: refused kinds never reach Buzz; a refused sensitive kind is not asked again; `approval_required` is not a refusal; keys bzz holds are never refused | `buzz_1_0_refusals_are_known_before_the_person_acts`, `approval_required_keeps_the_kind_available` (fake provider in Buzz 1.0 mode) | pass |
| Delete is shown unavailable with the signer's reason; edit stays available | `a_signer_refusal_disables_delete_but_not_edit` | pass |
| Missing socket-ownership proof is retried within one 5 s deadline; a wrong owner is final | `missing_ownership_proof_is_retried_but_another_user_is_final`, `proc_net_tcp_rows_match_the_exact_connection` | pass |
| v0.13 provider behavior unchanged | `credential_provider_test` (7), `real_relay_buzz_desktop_identity_signs_through_the_provider` | pass |

Gates after M2: fmt clean; clippy clean (audio and view-only); 480 tests
passed; pinned relay 14 passed; FI-enforcing relay 4 passed.

A sensitive kind is offered until Buzz refuses it once per session: the
contract lets a provider ask a person, and bzz cannot tell from `describe`
whether it will. Pairing with no `--relay` (upstream #9417) is not adopted:
bzz always pairs for the community it is configured with.

## M3 — Identity names and mention rules

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| Identity names v1: all 31 fixture cases | `buzz_app_contracts_test::identity_names_v1_fixtures` | with owner qualification disabled: 10 cases failed | pass |
| Mention rules v3: matching 23, ranking 35, space 23, query 12, admission 9, tags.write 7, tags.read 6 | `buzz_app_contracts_test::mention_rules_v3_fixtures` | with NFC instead of NFKD folding: 6 matching cases failed | pass |
| Mention choices include the viewer; a person, the viewer's agent and another owner's agent named alike get `Honey`, `Honey (agent)`, `Wes’s Honey`; `honey` selects none of them, `Wes’s Honey` its own; the viewer's agent ranks first | `store::queries::mention_choice_tests` | n/a | pass |
| In the composer, Space after `@Honey` selects the one Honey; `Wes ` with `Wes Smith` known stays prose and closes the chooser, `Smith,` then selects Wes Smith; `@Honey bee` closes the chooser; a draft mentioning a non-member is refused and kept | `app::tests::space_selects_only_an_exact_unique_mention` | with Space selection disabled: failed at the first mention; with the member check disabled: the stale draft was sent | pass |
| Timeline uses the contextual label and drops "owned by" when the label names the owner | `ui::timeline::tests::contextual_labels_name_authors_without_repeating_the_owner` | n/a | pass |
| The release TUI shows two members both named Honey as `Honey · <suffix>`, Space after `@Honey` does not pick either, Tab does and the message is sent | Herdr `t14-mention-namesakes` (5 harness scenarios passed) | n/a | pass |

Fixtures are vendored unchanged from `block/buzz-app` `dfdb5582e` (v1.0.1);
see `tests/fixtures/buzz-app/README.md`.

Gates after M3: fmt clean; clippy clean (audio and view-only); 486 tests
passed.

Deviations, by design for a terminal client without a directory search: no
outside people, other-community agents, outside-channel prompt or pending
mentions; the member cache is the "known" set, so a member without a cached
profile counts as having no name; the NIP-FI username stays a searchable
alias, as in v0.13.

## M4 — Solo chats and sidebar sections

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| The register-tree codec decodes desktop and mobile documents, refuses unknown versions, round-trips and stamps edits after the newest register | `protocol::sidebar_doc` tests (8, ported with the codec from `buzz-cli`) | n/a | pass |
| A mobile head decodes; an edit publishes one second after it, encrypted to yourself; an unchanged edit publishes nothing; foreign or newer-format heads are refused | `protocol::sidebar` tests | n/a | pass |
| `:solo` sends exactly Buzz 1.0's captured creation command | `solo_creation_matches_the_buzz_1_0_command` against upstream `test-fixtures/solo-creation.json` | n/a | pass |
| The newest head wins (same second: smaller id); the layout projects sections, icons, assignments and sort modes | `store::sidebar::tests::newest_sidebar_head_wins_and_projects_the_layout` | n/a | pass |
| Sidebar groups: Solo, sections in order, Channels, Forums, DMs; synced sort per group; headings only with more than one group | `ui::sidebar::tests::channels_group_like_buzz_with_synced_sections_and_sort` | n/a | pass |
| Against the pinned relay: a section bzz writes is listed by the `buzz` CLI; a section the CLI adds is read by bzz; a bzz reorder keeps the CLI's section; a sort mode bzz sets is listed by the CLI; a Solo chat bzz creates comes back as a private stream labeled `buzz.solo` with bzz's key as creator | `real_relay_sidebar_documents_and_solo_chats` (15 passed) | when the write ignored the relay's head, the reorder found no CLI section and the test failed (14 passed, 1 failed) | pass |
| NIP-29's bare `["private"]` tag is read; stored channels are repaired on re-ingest | `bare_private_tag_and_solo_label_are_read_from_group_metadata`, `current_metadata_repairs_a_misread_private_channel` | the first test failed before the fix (`Public`) | pass |
| The release TUI: `:section new Ops Team` shows an `Ops Team` group holding the channel; `:solo Plans` shows `🔒Plans` under Solo | Herdr `t14-sections-and-solo` (6 harness scenarios passed) | the first run showed `#Plans`, which exposed the private-tag bug | pass |

Gates after M4: fmt clean; clippy clean (audio and view-only); 501 tests
passed; pinned relay 15 passed. `scripts/test-relay.sh` now builds the
`buzz` CLI at the pin for the interop test.

Not covered: starred, muted and project documents; section icons from the
TUI; inviting agents into a Solo chat; Buzz Desktop identities cannot create
Solo chats (kind `9007` is refused by its provider, and bzz says so). When a
group has no synced sort mode bzz keeps its own sort; desktop's default is
alphabetical.

## M5 — Small parity

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| WebSocket upgrades send `User-Agent: bzz/<version>` | `upgrades_send_user_agent_and_a_404_names_the_wrong_community` (scripted relay records the header); the huddle audio socket uses the same request builder | without the header: `left: None` | pass |
| A 404 upgrade is "this relay serves no community at this address", not a retried network error; NIP-11 404 likewise | same test | before: `Network("relay refused the WebSocket upgrade (HTTP 404)")` | pass |
| Filters with an empty `ids`/`authors`/`kinds`/tag set are never sent: HTTP skips them (no request at all when none remain), a REQ drops them, and a subscription of only such filters gets EOSE at once without a REQ | `filters_with_empty_sets_are_never_sent` | with the HTTP guard off: "no HTTP request was made" failed; with the session guard off the REQ builder refused the filter (second line of defence) | pass |
| Against the pinned relay, an agent writing in a DM with a non-owner gets `restricted: agents cannot post in direct messages…`, which the broker maps to `unauthorized` | `real_relay_local_agent_broker` (extended), `restricted_relay_refusals_are_unauthorized` | n/a | pass |
| Search loads older pages: each page is strictly older than the oldest shown, and repeated pages reach all 45 generated messages | `real_relay_search_loads_older_pages` (16 passed) | with the bound not moved: "every older result is older" failed (15 passed, 1 failed) | pass |
| `older` keeps an earlier explicit `before:`; appending keeps rows and selection in place; a new query cancels a pending page | `older_pages_tighten_the_before_bound`, `older_pages_append_below_without_moving_rows` | n/a | pass |
| The release TUI: `/fixture`, then `Ctrl-o` adds older results below | Herdr `t14-search-older` (7 harness scenarios passed; "5 older result(s) added below" observed) | n/a | pass |

Gates after M5: fmt clean; clippy clean (audio and view-only); 506 tests
passed (audio), 505 (view-only); pinned relay 16 passed; FI-enforcing relay 4
passed; `cargo audit` and `cargo deny` ok.

## Supply chain during v0.14

RUSTSEC-2026-0538 (`quinn-udp` 0.5.15, receive loop can spin after a
truncated datagram) appeared in the advisory database during M4 and failed
`cargo audit` in CI. `quinn-udp` moved to 0.5.16 (its own `windows-sys`
requirement moved to the 0.59 already in the graph; no other package
changed). `cargo audit`: the 4 recorded allowed warnings, no vulnerability;
`cargo deny check`: ok.

## Release acceptance (2026-11-13)

Version 0.14.0 (`Cargo.toml`, `Cargo.lock`). Release notes:
[`release-v0.14.0.md`](release-v0.14.0.md).

| Gate | Command / evidence | Result |
|---|---|---|
| Tests (audio) | `cargo test --locked --all-features` | 506 passed, 0 failed |
| Lint, format | `cargo fmt --check`; `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| Upgrade from v0.13.0 | `./scripts/upgrade-test.sh` (new default: `tests/upgrade/seed_v0130.rs`, `tests/upgrade_from_v0130_test.rs`) | pass locally and in CI (below) |
| Release TUI (Herdr) | `BZZ_HERDR_PANE=w5:p5 ./scripts/test-relay.sh --herdr`, release build of the final code | 8 harness scenarios passed, including the four `t14-*` |
| Release TUI (film capture) | `.agents/skills/bzz-promo-video/capture/shoot-all.sh` with the release build on the pinned relay | 19 of 19 on-screen expectations, including the new `edit` and `sync` shots |
| Benchmarks | `cargo bench --bench timeline --bench store --bench media` with `--save-baseline v0130` on `v0.13.0` and `--baseline v0130` here | two accepted regressions (below) |

### Upgrade from v0.13.0

`scripts/upgrade-test.sh` now builds the `v0.13.0` tag (`2b3429f`) in a
worktree, lets it write a database with disposable keys (schema 15) and
record what it reads back, then opens a copy with v0.14.0. Besides what the
v0.12.1 test checked (migrations, a readable pre-migration backup at the old
version, `PRAGMA integrity_check`, identical Inbox, FTS hits, history and
draft), it now checks what v0.13.0 held but misread:

- an edit (kind 40003) that v0.13.0 stored but showed as the stale
  `plain history row`: after the upgrade the row shows the edited text with
  `edited`, and search finds the original message by a word only the edit
  contains (search projection v2);
- a channel with the bare `private` tag that v0.13.0 listed as `Public`: the
  next metadata event from the relay repairs it to `Private`.

**Red:** with the backfill in migration 0016 removed, the test fails
(`left: "plain history row"`, `right: "plain history row, editedneedle"`).
`BZZ_UPGRADE_FROM_VERSION=0121` still selects the v0.12.1 seed; it was not
re-run here because building v0.12.1 needs a system `libdbus-1-dev` this
machine lacks (v0.13.0 vendors DBus).

### Benchmarks against v0.13.0

Same machine, sequential, nothing else building. Improved: FTS search
−13.7 %, sanitize −15.0 %, hit map −12.5 %, composer −9.4 %, timeline render
−6.8 %, `parse bounded imeta` −6.1 %, Inbox rebuild −5.1 %. No change: PNG
decode, Inbox first page, timeline cursor, redraw gate.

The first run found **query latest 500 of 100k: +62 %** (1.33 → 2.15 ms).
Cause: M1 ran edit selection over every loaded message. Fixed in this
release: each row now carries an indexed `EXISTS` probe of `edit_targets`,
and only rows with a stored edit go through selection. Measured: +62 % →
+17.5 % (≈1.56 ms); with the probe replaced by a constant the query is at
+10 %, so the probe costs ≈0.14 ms per 500 rows. Proven needed: with the
probe forced to "no edit", `tests/message_edits_test.rs` fails 6 of 6.

Accepted regressions, both well under one frame (16 ms):

| Benchmark | v0.13.0 | v0.14.0 | Why |
|---|---|---|---|
| query latest 500 of 100k | 1.33 ms | 1.56 ms (+17.5 %) | message edits (M1) |
| mention candidates over 1k members | 1.23 ms | 1.62 ms (+31 %) | Buzz 1.0 identity names and mention rules (M3), per `@` keystroke |
| Inbox cursor page | 0.17 ms | 0.20 ms (+17 %) | edit-aware previews (M1) |

### Manual E2E

`docs/e2e-manual.md` has a new section 12 for v0.14.0 (edits, namesakes and
mentions, Solo and sections, Buzz 1.0 as key holder, search paging, wrong
community address). On the release build its steps are covered by the Herdr
scenarios above (edit, namesakes, sections and Solo, older search results),
the film capture (edit and sidebar on the pinned relay), the Buzz 1.0.1
pairing proof (M2) and the scripted-relay 404 test (M5). The multi-client
and Buzz-desktop comparison steps are left to production acceptance.

## Still pending after release

The owner approved publishing v0.14.0 on 2026-11-13 with these items open:

| Item | Owner |
|---|---|
| Production check on the real community: an edited message, mentions, a Solo chat and sections as Buzz desktop and mobile show them | owner |
| E2E section 12 multi-client steps (a second client, Buzz desktop showing the same sections) | owner |
| Upgrade from v0.12.1 straight to v0.14.0 (needs `libdbus-1-dev` to build v0.12.1) | operator |
| Carried from v0.13: instrumented 66-channel frame count, FI community, Windows audio, E2E 4.1/4.2/7/9 | owner / operator |
| A huddle's backing channel (`🔒huddle-…`) stays in bzz's sidebar after the huddle; Buzz desktop hides it locally | next release |
