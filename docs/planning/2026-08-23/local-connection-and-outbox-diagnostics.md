# bzz v0.9.0 — Local connection and outbox diagnostics

**Target release:** v0.9.0

**Status:** Implemented as the local-first foundation of the v0.9.0 candidate.

## Goal

Give a bzz user enough private, content-free local evidence to explain connection
failures and messages which remain visibly unacknowledged. Diagnostics must work
when the relay, the network, and any remote observability service are unavailable.

The user-facing model is:

- bzz keeps a small owner-private operational journal with connection, relay
  acknowledgement, and outbox state transitions;
- the timeline distinguishes a message which has never completed its first send
  from one whose delivery is uncertain;
- `bzz diagnostics` reports current connection/outbox evidence without opening
  the TUI or reading message bodies; and
- a user can create a bounded, reviewable support report or erase diagnostics
  without deleting identities, configuration, conversations, drafts, or media.

This plan is the local foundation for the separate opt-in OTel plan in
[`opt-in-otel-client-observability.md`](opt-in-otel-client-observability.md).
Remote export is not part of this change.

## Problem and current evidence

bzz has a durable outbox and a 25-second WebSocket acknowledgement timeout, but
it has no durable operational history explaining how an item reached its current
state. `pending` and `unknown` are both projected to `Message.pending`, so the
UI renders both as `[pending]`. The outbox retains attempt count and last error,
but those fields are not available through a content-free operator command.

Outbox reconciliation currently runs during initial runtime setup and after an
authenticated reconnect. It first checks the relay HTTP query endpoint by event
ID and then republishes an absent event. It does not run as a periodic repair
loop while a session merely appears online. A transport-uncertain item can
therefore remain visually pending until a reconnect or restart gives the outbox
another reconciliation opportunity.

Connection state is also ephemeral. `realtime::session` knows WebSocket connect,
AUTH, heartbeat, close, frame, publish, and acknowledgement outcomes;
`realtime::supervisor` knows reconnect/backoff and terminal access failures; the
TUI knows broadcast receiver lag. Today those facts become a current status
string or disappear. The existing `tracing` dependencies are not initialized
and there is no event contract which guarantees that logs exclude message
content and credentials.

The recently added lag-yield behavior in `app.rs` must be preserved. Diagnostics
will measure receiver lag and recovery; they must not revert the scheduling fix
or turn a dropped broadcast event into a blocking log write.

## Product decisions

### Typed diagnostics, not arbitrary debug logging

Define a closed, typed `DiagnosticEvent` model. Production connection/outbox
instrumentation emits only variants and fields allowed by that model. The local
writer serializes that model rather than accepting arbitrary formatted strings
from relay payloads, errors, or application state.

Developer `tracing` may use the same normalized fields, but a generic tracing
subscriber is not the diagnostic persistence boundary. This prevents a future
`debug!(?event)` or error chain from silently writing a message body, auth tag,
URL query, filesystem path, or secret into a support artifact.

### Local-first and bounded

The journal is enabled by default, remains local, and is stored under a dedicated
owner-private diagnostics directory in the profile-specific bzz data root.
Debug and release builds retain their existing separate roots. Files are mode
`0600` under a mode-`0700` directory on Unix.

Use deterministic size-based rotation with fixed hard limits. The initial
contract is at most three journal files of 2 MiB each. Rotation, serialization,
or disk-full failures are non-fatal and must never alter relay state, block the
UI, prevent terminal restoration, or recursively generate more diagnostics.
The writer uses a bounded non-blocking channel and records an in-memory dropped
count for the next successful health snapshot.

### Exact delivery states

Replace the lossy presentation-level `pending: bool`/rejected split with a typed
delivery presentation derived from the existing outbox state:

- `pending`: signed and durably queued, with no completed publish outcome yet;
- `unknown`: transport outcome is uncertain; the event may already exist at the
  relay and must be reconciled before a deliberate retry;
- `rejected`: the relay gave a definitive negative acknowledgement;
- delivered/no outbox marker: accepted or observed at the relay.

The timeline should use concise labels such as `[pending]`, `[delivery unknown]`,
and `[rejected]`. It must not display a raw transport error inline. Unknown must
suggest `:reconnect` or the diagnostics command rather than invite blind repeated
publication.

This change is diagnostic presentation only. It does not add periodic retries,
change the 25-second acknowledgement timeout, or modify outbox reconciliation.
Those are remediation decisions which require evidence from this plan.

### Content-free support evidence

A diagnostics report may contain:

- bzz version/build profile, OS family, wall-clock timestamp, and a random
  per-launch session ID;
- relay origin host and port, never credentials, URL query, or fragment;
- normalized connection phase, elapsed durations, reconnect/backoff values,
  heartbeat outcome, WebSocket close code when available, and error class;
- event ID and event kind only for locally authored outbox operations;
- outbox state, attempt count, age, and normalized last-error class;
- receiver-lag count, bounded queue drops, journal write health, and clock-skew
  classification.

It must never contain:

- Nostr event content, tags, complete event JSON, message previews, reactions,
  drafts, agent prompts/results, clipboard data, or attachment metadata;
- nsec values, NIP-98 headers, NIP-42 challenges/auth events, NIP-OA auth tags,
  key references, passphrases, cookies, or future OTel tokens;
- identity/community/channel labels, pubkeys, channel/thread UUIDs, participant
  sets, profile fields, media URLs, local source/staging paths, or config file
  contents; or
- arbitrary relay notices, HTTP response bodies, Rust error debug output, panic
  payloads, or environment variables.

Event IDs are content hashes already used for relay acknowledgement and are the
minimum useful correlation key. They are included only for local outbox lifecycle
events and explicit outbox reports, not for all received traffic.

## User-facing command contract

Add a `diagnostics` command group with output suitable for both humans and
explicit automation:

```text
bzz diagnostics status [--json]
bzz diagnostics outbox [--community <uuid>] [--json]
bzz diagnostics report --output <new-file>
bzz diagnostics clear --yes
```

### `status`

Summarize the latest journal evidence: current/last connection phase, most recent
successful AUTH, most recent disconnect class, reconnect/backoff count, receiver
lag/drop count, journal health, and outbox counts by exact state. It opens the
SQLite database read-only where practical and does not unlock an identity or
open a relay connection.

### `outbox`

List metadata-only rows with event ID, kind, state, attempts, created/updated
age, and normalized error class. Add a dedicated store projection which never
selects or deserializes `event_json`; do not reuse `pending_outbox`, because that
API necessarily loads complete signed events for publication.

Human output may abbreviate an event ID while `--json` returns the complete ID
needed for correlation. Neither mode prints community labels, channel IDs,
message bodies, tags, or raw last errors. A community filter is an explicit
local UUID selector and is not printed unless the user requested it.

### `report`

Write one new mode-`0600` JSON report atomically and fail if the destination
already exists. The report contains a versioned schema, a bounded recent event
window, the metadata-only outbox snapshot, and an explicit redaction manifest
which states what was excluded. It is inspectable with ordinary text tools and
is not automatically uploaded, compressed, encrypted, or sent anywhere.

### `clear`

Delete only rotated diagnostic journals after `--yes`. It does not delete the
SQLite outbox, change message state, remove drafts, clear media, reset config,
or remove credentials. Starting bzz later recreates an empty private journal.

## Event contract

Initial event names and required safe fields:

| Event | Required fields |
|---|---|
| `client.started` | session ID, version, OS, build profile |
| `client.stopped` | session ID, normalized reason |
| `session.connect_started` | session ID, relay origin, attempt |
| `session.transport_connected` | session ID, duration ms |
| `session.auth_started` | session ID; never challenge/auth event |
| `session.authenticated` | session ID, duration ms |
| `session.connect_failed` | session ID, phase, error class, duration ms |
| `session.disconnected` | session ID, error class, close code if known, connection age ms |
| `session.heartbeat_timeout` | session ID, last inbound age ms |
| `session.backoff_scheduled` | session ID, attempt, delay ms |
| `session.reconnect_requested` | session ID, source `user` or `supervisor` |
| `session.receiver_lagged` | session ID, skipped event count |
| `outbox.queued` | session ID, event ID, kind |
| `publish.sent` | session ID, event ID, kind, attempt |
| `publish.acknowledged` | session ID, event ID, accepted, duration ms |
| `publish.uncertain` | session ID, event ID, error class, duration ms |
| `outbox.state_changed` | session ID, event ID, kind, old/new state, attempts |
| `outbox.reconcile_started` | session ID, eligible count |
| `outbox.reconcile_observed` | session ID, event ID, prior state |
| `outbox.reconcile_republished` | session ID, event ID, accepted, duration ms |
| `outbox.reconcile_finished` | session ID, delivered/rejected/unknown counts, duration ms |
| `diagnostics.events_dropped` | session ID, count, queue capacity |

Error classes are a closed enum such as `dns`, `connect`, `tls`, `websocket`,
`closed`, `heartbeat_timeout`, `ack_timeout`, `auth_clock`, `auth_rejected`,
`access_denied`, `rate_limited`, `http_4xx`, `http_5xx`, `protocol`, `database`,
`io`, and `unknown`. Classification may inspect typed errors/statuses internally,
but the persisted event never stores the source string.

Timestamps use UTC plus monotonic elapsed durations where ordering/latency
matters. A wall-clock adjustment must not produce a negative connection or ACK
duration.

## Architecture and ownership

Expected bzz-owned seams, with exact names selected during implementation:

```text
src/diagnostics/
  mod.rs          typed handle, bounded channel, lifecycle
  event.rs        closed event/error/schema model and redaction contract
  journal.rs      private JSONL rotation and failure isolation
  report.rs       status/outbox/report assembly and versioned JSON output
src/realtime/session.rs       transport, AUTH, heartbeat, publish/ACK events
src/realtime/supervisor.rs    attempts, backoff, reconnect and terminal failure
src/sync/outbox.rs            reconciliation lifecycle and outcomes
src/store/queries.rs          metadata-only diagnostic outbox projection
src/store/models.rs           typed outbox diagnostic row/state
src/domain.rs                 exact message delivery presentation
src/ui/timeline.rs            distinct delivery labels
src/app.rs                    session ownership, receiver lag, user reconnect source
src/main.rs                   diagnostics CLI and startup/shutdown wiring
src/paths.rs                  private diagnostics directory/journal paths
src/config.rs                 bounded local-diagnostics policy/default
```

`DiagnosticHandle` is an explicit, cloneable dependency. It owns no signer,
store, relay client, HTTP client, or UI state. Producers must be able to emit
with a non-awaiting `try_emit` path. The writer task is started after paths and
configuration validate, and it is flushed only with a short bounded shutdown
budget after terminal restoration is already guaranteed.

The store remains authoritative for outbox state. Diagnostics describe committed
transitions only: `outbox.state_changed` is emitted after the SQLite transaction
commits, never before. A failed transaction produces a normalized database event
but must not claim that the state changed.

## Configuration

Add a strict local diagnostics section with conservative defaults, for example:

```toml
[diagnostics]
local_journal = "on"
```

File count and size remain compiled safety bounds rather than user-expandable
unbounded settings in the first release. `off` disables journal creation but
keeps `bzz diagnostics outbox` available because it reads existing metadata
directly. Invalid values fail during `bzz check` before raw mode.

Do not add an OTel endpoint, bearer token, remote-export switch, installation
identifier, metrics, or traces in this plan.

## Milestones

### M0 — Contract and characterization

- Add the typed event/error/redaction contract and synthetic serialization
  fixtures before instrumenting production paths.
- Characterize current pending/unknown/rejected projection, ACK timeout,
  reconnect flush, observed-echo delivery, and attempt-count behavior.
- Add leak tests with sentinel message bodies, tags, secrets, URLs, paths,
  control characters, and relay error strings; assert none can enter a
  diagnostic event or report.

**Exit:** every allowed field and prohibited data class is explicit, and current
outbox behavior has deterministic tests without changing it.

### M1 — Private bounded journal

- Add diagnostics paths, strict configuration, session IDs, bounded channel,
  owner-private JSONL writer, deterministic rotation, and non-recursive failure
  handling.
- Wire startup/shutdown after configuration validation while preserving terminal
  restoration and debug/release profile isolation.
- Prove a full/drop/disk-error journal cannot block input, SQLite, WebSocket
  processing, or application exit.

**Exit:** synthetic events survive restart inside fixed disk bounds, and forced
writer failure leaves bzz behavior unchanged.

### M2 — Connection and publish instrumentation

- Instrument session and supervisor boundaries with monotonic durations and
  normalized error classes.
- Instrument outbox queueing, committed state transitions, flush lookup,
  republish, and final report without event JSON or relay response text.
- Record broadcast receiver lag from the existing yield path without logging
  every skipped event or introducing a new ready-loop/starvation path.

**Exit:** fake relay tests distinguish connect failure, AUTH failure, heartbeat
timeout, accepted ACK, rejected ACK, lost ACK, disconnect, observed delivery,
and reconnect reconciliation from journal evidence alone.

### M3 — Exact delivery presentation and CLI

- Replace lossy message delivery projection with exact typed state and render
  distinct, concise timeline labels.
- Add metadata-only store queries and implement status/outbox JSON and human
  output without unlocking credentials or connecting to a relay.
- Implement atomic report creation and diagnostics-only clear semantics.

**Exit:** an operator can identify whether a message is pending, uncertain,
rejected, or delivered and can obtain the event ID/attempt/age evidence without
reading its content.

### M4 — Documentation and hardening

- Update configuration, security, troubleshooting, manual E2E, CLI help, and
  release documentation with journal location, bounds, redaction, state labels,
  report review, and clear/recovery instructions.
- Run strict CI, store/session/outbox integration, terminal snapshots, release
  build, dependency audit, and pinned-relay journeys with disposable content.
- Use a controlled real relay test to correlate one accepted event ID and one
  deliberately interrupted acknowledgement without publishing user data.

**Exit:** local diagnostics are safe enough to leave enabled, useful without a
network, and proven not to alter publication or reconciliation semantics.

## Test strategy

- Unit tests for every event variant, error classification, UTC/monotonic time,
  serialization schema, rotation threshold, and redaction invariant.
- Property tests generating arbitrary hostile error strings, URLs, tags, paths,
  Unicode/control input, and large counts; output remains bounded and contains
  only allowlisted fields.
- Store tests proving metadata-only diagnostics never select `event_json`, exact
  state/attempt/age reporting, transaction-after-commit emission, and no state
  mutation from diagnostics commands.
- Fake WebSocket tests for connect/AUTH/heartbeat/close/publish/ACK outcomes and
  no writer-induced timing changes.
- Outbox tests for pending crash windows, unknown timeout, event-found delivery,
  republish acceptance/rejection, and attempt preservation.
- TestBackend snapshots for the three non-delivered labels and actionable but
  content-free status guidance.
- Filesystem tests for mode, atomic report creation, existing-file refusal,
  rotation, clear scope, disk-full/permission failure, and debug/release roots.
- End-to-end CLI tests asserting JSON stability and absence of fixture message
  content, pubkeys, channel IDs, secrets, paths, and raw errors.

## Non-goals

- No Android, Buzz Desktop, relay, Kubernetes, ClickHouse, Grafana, or Emilia
  repository changes.
- No remote upload, OTel encoding, shared support service, crash reporting,
  metrics, distributed traces, session replay, terminal capture, or screenshots.
- No message content, broad received-event logging, SQL dump, config dump, or
  automatic support-bundle sharing.
- No periodic outbox retry, timeout change, retry button, connection policy
  change, relay rate-limit workaround, or deduplication protocol change.
- No use of diagnostics as an authoritative delivery state; SQLite and relay
  acknowledgement/observation remain authoritative.

## Acceptance criteria

1. A user can distinguish `pending`, `delivery unknown`, and `rejected` without
   opening SQLite, and delivered messages retain the current clean presentation.
2. `bzz diagnostics outbox --json` reports event ID, kind, exact state, attempts,
   age, and normalized error class while never selecting/deserializing message
   content.
3. A bounded local journal explains connection, AUTH, heartbeat, backoff,
   publish/ACK, receiver lag, and outbox reconciliation outcomes across restart.
4. Journal/report/CLI output contains none of the prohibited content, identity,
   credential, path, relay-payload, or configuration data, including under
   hostile generated input.
5. Diagnostics remain non-blocking and bounded; queue saturation, serialization
   failure, unwritable disk, or shutdown cannot freeze the TUI, delay an ACK,
   change outbox state, or break terminal restoration.
6. Journal files and reports are private, profile-isolated, explicitly clearable,
   and never transmitted automatically.
7. Existing outbox acknowledgement, draft recovery, reconnect, media, read-state,
   locked/cache-only, identity isolation, and human-send tests remain green.
8. The implementation produces enough event-ID/timestamp evidence to correlate
   a future incident with relay logs without first enabling remote telemetry.

## Approved decisions

1. The default local journal budget is three 2 MiB files.
2. Complete event IDs are accepted in owner-private local reports; human output
   abbreviates them and JSON output retains the complete correlation key.
3. Local journaling defaults to `on`; an explicit `off` remains available.
4. v0.9.0 observes existing reconciliation behavior only and does not add
   periodic outbox repair until evidence identifies the failure mode.
