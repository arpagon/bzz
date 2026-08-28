# bzz v0.12.1 — Quota-aware relay admission

**Status:** Release candidate 2026-08-28

v0.12.1 prevents authenticated startup and reconnect from overwhelming a
bounded Buzz relay when an identity has many joined channels. WebSocket
`REQ`/`EVENT` operations now pass through one bounded admission scheduler,
while control frames remain immediate.

## Highlights

- Paces billable WebSocket frames at eight per second, below the current Buzz
  human allowance of ten per second.
- Prioritizes an explicitly authorized publication, the selected channel, and
  selected-channel agent typing ahead of background channel replay.
- Coalesces same-ID subscription replacement and reconstructs paced desired
  state after reconnect.
- Keeps `CLOSED rate-limited:` subscriptions desired and reopens them after the
  bounded relay hint plus deterministic jitter.
- Applies bounded exponential recovery only to recognizable transient
  closures; access and protocol failures remain terminal and fail closed.
- Keeps an authenticated socket online during quota pressure instead of
  reporting access denied or permanently disabling typing.
- Replaces raw sticky quota notices with a one-row transient status such as
  `relay busy · retrying in 1s`.
- Allows only one EVENT to await acknowledgement. A correlated rejection is not
  republished automatically; a legacy uncorrelated NOTICE remains delivery
  uncertain.
- Preserves the exact draft until relay acceptance or authoritative echo.
- Adds owner-private local activation/recovery counters without relay text,
  identifiers, filters, URLs, identities, or content. These new records are not
  exported through OTel.

## Publication boundary

Admission can delay a signed EVENT before its first wire send, but it cannot
create publication intent. Conversation actions remain explicitly human
initiated and are durably staged before admission. An explicit
`OK(event_id, false, "rate-limited:...")` restores the exact draft generation
and requires the existing deliberate retry path; bzz does not silently send it
later.

An older relay may reject an EVENT with an uncorrelated `NOTICE`. bzz permits at
most one in-flight EVENT, marks that outcome uncertain, and reconciles by event
ID before any deliberate retry.

## Privacy and compatibility

Relay-provided quota and closure text is classified and discarded before the
new local diagnostics or status presentation. Reports contain only typed source
and duration buckets plus aggregate activation/recovery counts. No
subscription, community, channel, event, identity, URL, tag, filter, or message
content is retained by this feature.

The Buzz protocol/dependency pin remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. v0.12.1 adds no protocol kind,
schema migration, configuration key, dependency, HTTP polling path, agent
runtime capability, or infrastructure requirement.

## Explicit non-goals

This release does not modify the Buzz relay, Traefik, Kubernetes, server quota,
Buzz Desktop, NIP-AO, remote-agent authority, typing publication, or local-agent
execution. Relay acknowledgement correlation and Desktop startup pacing remain
separate upstream work.

## Validation

Candidate evidence is recorded in
[`validation-v0.12.1.md`](validation-v0.12.1.md). The approved scope and
acceptance boundary are in
[`planning/2026-08-28/v0.12.1.md`](planning/2026-08-28/v0.12.1.md).
