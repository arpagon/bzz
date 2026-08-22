# Draft acknowledgement and stale-composer recovery

**Status:** proposed
**Owner:** bzz maintainers
**Scope:** durable composer drafts and acknowledged message submission

## Problem

Opening the ordinary composer with `i` must restore only an unsent local draft. It
must never refill the composer with a message that the relay already accepted.

The current path has an acknowledgement-ordering bug:

1. `open_composer_target()` loads the draft row for the selected channel/thread.
2. `Submit` calls `Composer::take_message()`, which clears only the in-memory
   composer.
3. It queues the send, immediately removes `composer_target`, then calls
   `persist_draft()`.
4. `persist_draft()` becomes a no-op because there is no longer a target, so the
   pre-send draft row remains in SQLite after a successful relay acknowledgement.
5. A later `i` loads that row and displays the prior sent content as though it
   were an unsent draft.

Read-only inspection of the affected local database confirmed the signature
without reading or recording message content: five non-empty draft rows existed,
and all five exactly matched an event authored by the local identity.

A disposable bzz process was also opened in the current Herdr session. It was
not used to send anything; the available channel did not have a non-empty stored
draft, so it correctly opened empty. The source path and database correlation
above provide the deterministic reproduction without publishing or exposing a
message.

## Invariants

- A draft is removed only after the relay has acknowledged its exact event as
  accepted.
- A rejected or transport-uncertain send keeps its draft recoverable. No retry is
  implicit and no message content is logged.
- A late acknowledgement for an older send cannot delete a newer draft for the
  same channel/thread.
- Channel and thread drafts remain isolated by community, channel, and root
  event ID.
- Existing draft/attachment migration compatibility remains intact. Attachment
  upload completions remain ID- and target-scoped.
- The fix does not alter message content, Nostr event construction, identity
  boundaries, or read-state behavior.

## Design

### 1. Give each stored draft a revision and state

Add migration `0006_draft_submission_state.sql` and register it in
`src/store/migrate.rs`.

Extend `drafts` with:

- `revision TEXT NOT NULL`: a random opaque UUID per editing generation;
- `state TEXT NOT NULL`: constrained to `editing` or `sending`, defaulting to
  `editing` for existing rows;
- `outbox_event_id TEXT`: populated only after the signed event has been stored
  in the local outbox.

The migration assigns a unique revision to every existing row. It does not store
clipboard input or any new message content.

Replace tuple-oriented draft reads with a typed internal `StoredDraft` carrying
body, attachments, mentions, revision, and state. Ordinary composer hydration
returns only an `editing` draft. A `sending` row deliberately opens as an empty
composer while its acknowledgement is pending.

### 2. Make submit a durable state transition

When Enter accepts a sendable composer:

1. Capture its `ComposerTarget` and stored draft revision.
2. Atomically mark that exact draft `sending`; do not delete it and do not rely
   on `composer_target` after it is cleared.
3. Clear the in-memory composer and close it as today.
4. Pass an opaque `DraftSubmission { target, revision }` alongside the outgoing
   request.

If signing or initial outbox insertion fails, return the exact matching row to
`editing`. This preserves the current draft instead of making it look sent.

### 3. Bind draft state to the existing outbox transaction

Extend the message-service send path so that, after signing, the transaction
which inserts the outbox event also stores the event ID on the matching `sending`
draft. Do not derive the association from content.

On the definitive response, in the same store transaction that changes the
outbox state:

- **accepted:** apply the event, mark the outbox row delivered, and delete only
  the matching `sending` draft revision;
- **rejected:** mark the outbox row rejected and restore only that matching
  draft revision to `editing`;
- **transport uncertainty:** mark the outbox row unknown and restore only that
  matching revision to `editing`, with the existing explicit error status.

Each conditional operation matches community, channel, root, state, and
revision. Thus an edit begun after the send has started has a new revision and
cannot be deleted or overwritten by a late result.

At startup, reconcile any residual `sending` rows: a delivered linked outbox
entry is finalized; every other state is restored to `editing` with no automatic
republish. This is crash-safe and preserves the acknowledged-send boundary.

### 4. Surface state without content leakage

While a matching send is pending, show a content-free status such as `sending;
waiting for relay acknowledgement`. A rejected/unknown result uses the existing
sanitized failure status and makes the draft available on the next `i`. Do not
print draft bodies, file paths, clipboard values, or event payloads.

## Implementation sequence

1. Add migration, typed store model, conditional draft-state queries, and
   startup reconciliation.
2. Add opaque `DraftSubmission` plumbing from `App` through `MessageService`.
3. Make outbox insertion, event association, accepted deletion, and failed
   restoration transactional.
4. Replace the current target-clearing/persist no-op flow in `insert_action()`.
5. Add content-free UI status and update user documentation/release notes.

## Tests and validation

### Store tests

- Legacy databases upgrade with unique revisions and `editing` state.
- A matching delivered acknowledgement deletes the draft.
- Matching rejected/unknown outcomes restore it.
- A mismatched revision never changes a newer draft.
- Startup reconciles delivered, rejected, unknown, and unbound interrupted
  submissions safely.

### App/service tests

- Deterministic regression: type a draft, submit it through an accepted fake
  relay, reopen with `i`, and assert an empty composer plus no stored draft.
- Rejected and unavailable fake relay paths re-open the original draft.
- Re-open and edit the same target before an old acknowledgement; the older
  completion must not delete the new text or attachments.
- Channel and thread target isolation; attachment and mention metadata round
  trip through each outcome.
- Verify that no failure/status/snapshot includes message text.

### Release gates

Run `cargo fmt --check`, strict Clippy, `cargo test --locked`, `cargo deny
check`, `cargo audit`, `cargo build --release --locked`, `bzz check`, and
`git diff --check`; then require Linux, macOS (Intel/Apple Silicon), Windows,
and test CI. Manually verify accepted, rejected, and disconnected sends in a
disposable relay before release.

## Acceptance criteria

After an accepted send, pressing `i` in that channel/thread starts empty rather
than showing the previous sent message. A rejected or unacknowledged send remains
recoverable, and a late result cannot erase a subsequently edited draft.
