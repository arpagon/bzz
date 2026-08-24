# bzz v0.9.0 — Private diagnostics and opt-in observability

v0.9.0 makes connection and delivery failures explainable without turning bzz
into a content logger. It also adds an optional, explicitly enrolled OTLP logs
path for controlled operator correlation.

## Highlights

- A typed, owner-private local journal records bounded connection, AUTH,
  heartbeat, reconnect/backoff, publish acknowledgement, receiver-lag, and
  committed outbox-transition evidence.
- `bzz diagnostics status`, `outbox`, `report`, and `clear` work without opening
  the TUI, unlocking an identity, or connecting to a relay.
- Outbox inspection never selects `event_json`; support reports are bounded,
  create-new, owner-only JSON with an explicit redaction manifest.
- The timeline distinguishes `[pending]`, `[delivery unknown]`, and `[rejected]`
  rather than collapsing uncertain delivery into pending.
- `bzz telemetry configure|enable|disable|status|test|forget` manages a separate,
  default-off OTLP/HTTP protobuf logs exporter.
- Persistent telemetry tokens use a dedicated OS credential service and are
  cryptographically bound to the exact canonical HTTPS `/v1/logs` endpoint.
- The exporter refuses redirects and proxies, sends no traces/metrics/generic
  logs, never uploads old journal files, and has hard queue, byte, batch, retry,
  timeout, expiry, and shutdown limits.

## Privacy boundaries

Neither local nor remote diagnostics accept message/event content, tags,
drafts, reactions, profiles, prompts/results, clipboard data, attachment/media
metadata, source paths, labels, pubkeys, community/channel/thread/participant
identifiers, credentials, auth material, raw relay payloads, response bodies,
configuration, environment values, or raw errors. Complete event IDs are
restricted to locally authored outbox lifecycle events for receipt correlation.

The local journal defaults to `on` and is capped at three 2 MiB files. Remote
telemetry defaults to `off` for every build and sends no request until an owner
configures a scoped credential and enables it. Missing, rejected, offline, or
rate-limited telemetry cannot block local diagnostics, relay operation, SQLite,
input, terminal restoration, or application exit.

## Configuration and dependencies

New strict `[diagnostics]` and `[telemetry]` configuration sections are backward
compatible. No database migration is required. v0.9.0 adds pinned
`opentelemetry-proto`/`prost` message dependencies only; it does not add the
OpenTelemetry SDK exporter pipeline.

See [`configuration.md`](configuration.md), [`security.md`](security.md),
[`troubleshooting.md`](troubleshooting.md), and
[`validation-v0.9.0.md`](validation-v0.9.0.md).
