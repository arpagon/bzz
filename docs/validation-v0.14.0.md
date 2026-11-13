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

## Pending

| Item | Why |
|---|---|
| M3–M5 | not started |
