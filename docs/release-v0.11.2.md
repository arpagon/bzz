# bzz v0.11.2 — Discoverable thread summaries

**Status:** Release candidate 2026-08-25

v0.11.2 makes threaded conversations visible before the context pane is
opened. A top-level message with accepted, non-deleted descendants now carries
a compact text-first row:

```text
      ↳ 13 replies · last reply 6 minutes ago
```

The context pane reports `13 replies` instead of counting the root as a
fourteenth message. Existing `Enter`, contextual-action, reply, draft, and `q`
close behavior is unchanged.

## Highlights

- Counts every direct or nested descendant exactly once by its validated root
  coordinate.
- Excludes the root, deleted replies, system rows, reactions, and
  pending/unknown/rejected local publications.
- Reconstructs summaries from the verified local archive for only the retained
  timeline roots; no schema migration or summary table is added.
- Accepts the pinned Buzz relay's signed kind `39005` only as bounded transient
  selected-channel presentation metadata.
- Requires exact matching lowercase `e`/`d` roots, the selected `h` channel,
  consistent counts/timestamps, valid participant keys, and the pinned relay
  signer.
- Never stores kind `39005` in events, search, Inbox, unread/read state,
  diagnostics, or telemetry.
- Updates counts and last activity after replies, nested replies, accepted
  outbox transitions, and deletions without polling.
- Uses explicit singular/plural wording and expanded relative activity in
  monochrome terminals.
- Leaves messages with no replies at their previous height.

The compatibility pin remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. The pinned baseline already
defines relay-synthesized kind `39005`; v0.11.2 does not adopt kind `39006` or
Buzz's complete channel-window architecture.

## Trust and privacy boundary

A thread summary is presentation state. It does not establish message
authorship, membership, read acknowledgement, notification eligibility, agent
status, or publication authority. Local authored content remains the only text
copied from a selected message. Summary counts, timestamps, participant keys,
and payload bytes are excluded from diagnostics and OTel.

The human-send and acknowledgement-aware draft boundary is unchanged. A reply
is counted only after delivery is accepted or observed from the relay.

## Explicit non-goals

v0.11.2 does not add inline expanded trees, participant facepiles, per-thread
unread badges, thread following/muting, forum cards, periodic polling, a schema
migration, kind `39006`, local agent hosting, runtime control, or autonomous
publication.

## Validation

Candidate evidence is recorded in
[`validation-v0.11.2.md`](validation-v0.11.2.md). The accepted design and source
comparison are in
[`planning/2026-08-25/v0.11.2.md`](planning/2026-08-25/v0.11.2.md).
