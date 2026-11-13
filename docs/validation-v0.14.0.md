# bzz v0.14.0 validation

**Status:** In progress on branch `v0.14.0`. Plan:
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

## Pending

| Item | Why |
|---|---|
| M2–M5 | not started |
