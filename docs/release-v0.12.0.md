# bzz v0.12.0 — Themed agent status and coherent message selection

**Status:** Release candidate 2026-08-28

v0.12.0 turns the one-row footer into a responsive, theme-aware segmented
status bar. Fresh verified-agent typing now appears as a one-cell animated
nugget instead of occupying the writing dock:

```text
 NORMAL  ◆ Fizz ⠹                              online  KITTY  ? help · q quit
```

The release also replaces the detached full-width selected-message stripe with
a compact message-scoped gutter. Wrapped body lines retain the avatar/text
indent, and grouped follow-up messages keep their timestamp and first body line
together.

## Highlights

- Moves the existing verified remote-agent kind `20002` presentation from the
  composer to the status bar without changing admission, subscription, scope,
  expiry, or reply-clearing behavior.
- Uses a bounded one-cell Braille spinner at five frames per second only while
  fresh typing is visibly active.
- Collapses several agents to `◆ Fizz +2 ⠹`, with cell-safe narrow forms.
- Keeps exact selected-channel and canonical open-thread scope.
- Reclaims the typing row from the writing dock and removes its former cursor
  and mouse-hit offset.
- Uses adjacent mode, agent, notice, connection, graphics, and help segments
  that degrade without wrapping on constrained terminals.
- Adds `StatusAgent`, `StatusConnection`, and `StatusMedia` semantic theme
  groups, with built-in palette derivation and safe defaults for existing
  custom themes.
- Adds `MessageSelected`, linked by default to `SelectionBorder`, for a compact
  message-level selection gutter.
- Keeps wrapped message continuation lines in the six-cell text column and
  combines grouped timestamps with their first body line.
- Preserves message identity, scrolling, mouse hits, copy ranges, thread
  opening, reactions, unread/read state, and publication behavior.

## Trust and privacy boundary

`◆ Fizz ⠹` means only that bzz recently admitted a valid signed typing signal
from that currently verified remote agent for the exact visible scope. The
spinner does not prove that a runtime is working, thinking, online, healthy,
ready, making progress, or guaranteed to reply.

Typing remains bounded in-memory presentation state. Kind `20002`, raw tags,
identities, coordinates, and content remain excluded from SQLite, Inbox,
search, unread state, copy, support reports, and OTel. bzz publishes no typing
or observer event.

The private screenshot used to report the selected-message visual defect was
not retained or copied into the repository, fixtures, diagnostics, or release
assets. Automated and visual tests use generated content only.

## Compatibility

The protocol/dependency pin remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. v0.12.0 adds no protocol kind,
subscription, schema migration, configuration key, diagnostic identifier, or
agent authority.

Existing `theme.toml` files remain valid when they omit the new groups. Users
may optionally override the exact semantic names documented in
[`themes.md`](themes.md).

## Explicit non-goals

v0.12.0 does not add human typing, cross-channel activity, NIP-AO kind `24200`,
presence/readiness/progress claims, local-agent execution, ACP, process
inspection, model/provider configuration, runtime controls, durable activity,
or a second status row.

## Validation

Candidate evidence is recorded in
[`validation-v0.12.0.md`](validation-v0.12.0.md). The approved scope is in
[`planning/2026-08-28/v0.12.0.md`](planning/2026-08-28/v0.12.0.md). The larger
NIP-AO observer proposal remains deferred in
[`planning/2026-08-25/deferred-nip-ao-agent-activity.md`](planning/2026-08-25/deferred-nip-ao-agent-activity.md).
