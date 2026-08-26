# bzz v0.11.3 — Verified-agent typing feedback

**Status:** Release candidate 2026-08-26

v0.11.3 closes the immediate feedback gap after a structured remote-agent
mention. When a currently verified Buzz managed agent emits its existing signed
ephemeral kind `20002` signal, bzz now shows:

```text
◆ Fizz is typing…
```

The row is live presentation state, not a message or readiness claim. It
expires after eight seconds, refreshes without duplication, and clears when the
matching signed reply arrives.

## Highlights

- Uses a dedicated exact-`#h` selected-channel subscription with a ten-second,
  ten-event overlap matching the pinned Buzz client.
- Verifies the event signature, exact channel, bounded tags, freshness,
  canonical thread coordinates, current verified-agent projection, and exact
  destination authority.
- Keeps channel and open-thread typing distinct; thread-only typing cannot leak
  into the main channel composer.
- Shows one compact width-aware row with the stable monochrome `◆` marker and
  bounded multiple-agent wording.
- Uses the existing sanitized profile label with verified-agent fallback.
- Refreshes the eight-second deadline without redraw churn and suppresses stale
  post-reply signals.
- Handles kind `20002` before generic ingestion and explicitly rejects it from
  the durable store.
- Clears transient state on scope/community changes, disconnect, lock, and
  shutdown.
- Publishes no human typing and changes no composer, draft, outbox, ACK, mention,
  attachment, Inbox, unread, search, copy, or thread-summary authority.

The compatibility pin remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. The pinned `buzz-acp` harness
already emits kind `20002` every three seconds while a turn is in flight.

## Trust and privacy boundary

A typing event proves only that the verified agent identity signed a recent
signal for the exact conversation scope. It does not prove that a model is
ready, healthy, progressing, or guaranteed to reply.

Typing content, raw tags, identities, coordinates, and timestamps are excluded
from SQLite, Inbox, search, unread/read state, copy, diagnostics, support
reports, and OTel. Only a bounded normalized in-memory projection exists for the
visible scope.

## Explicit non-goals

v0.11.3 does not publish human typing, show general human typing, add presence,
consume/decrypt NIP-AO kind `24200`, expose ACP transcripts, observe tools or
reasoning, control or host a runtime, own an agent key, or start/stop/retry an
agent.

## Validation

Candidate evidence is recorded in
[`validation-v0.11.3.md`](validation-v0.11.3.md). The accepted scope and source
comparison are in
[`planning/2026-08-25/v0.11.3.md`](planning/2026-08-25/v0.11.3.md).
