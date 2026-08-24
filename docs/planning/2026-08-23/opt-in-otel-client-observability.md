# bzz v0.9.0 — Opt-in OTel client observability

**Target release:** v0.9.0

**Status:** Implemented as the second, default-off phase of the v0.9.0 candidate.

## Goal

Allow an explicitly enrolled bzz installation to send a small, content-free
subset of its local connection and outbox diagnostic events to an operator's
OTLP/HTTP logs endpoint. The initial Emilia deployment will use
`https://otel.emiliavision.com/v1/logs` and
`observability.emilia_otel_logs`, enabling client/relay incident correlation by
time and authored event ID.

Remote observability is an optional operator integration, not a condition for
using bzz. A normal public build remains local-first and sends nothing until the
owner configures an endpoint, provides a scoped credential, and enables export.

This plan builds on
[`local-connection-and-outbox-diagnostics.md`](local-connection-and-outbox-diagnostics.md).
That plan's typed event allowlist, error normalization, local journal, exact
outbox presentation, and support commands must land first. Remote export must
consume the same safe event model rather than introducing a second collection
path.

## Development environment already prepared

The repository-local, Git-ignored `/home/arpagon/Workspace/arpagon/bzz/.env`
already contains development values for `BZZ_OTEL_ENDPOINT`, `BZZ_OTEL_TOKEN`,
`BZZ_OTEL_SERVICE_NAME`, `BZZ_OTEL_API_KEY_ID`, and `BZZ_OTEL_SUBJECT_ID`. The
token is a per-installation production OTel token with only
`otel:logs:write`, and the file is owner-private (`0600`). These variables are
for implementing and manually validating the exporter; their values must never
be copied into tracked files, tests, fixtures, logs, diagnostics reports, or
command arguments. The planned keychain enrollment remains the production
credential path.

## Product decision

### Logs only, explicitly enrolled

Implement OTLP/HTTP protobuf log export only. The Emilia pipeline currently
accepts logs, not metrics or traces. bzz will not claim distributed tracing,
create fake spans, collect performance profiles, or add a background analytics
SDK.

Remote export has three states:

- `off`: default; no remote client is created and no endpoint is contacted;
- `configured`: endpoint and credential reference exist, but export is disabled;
- `on`: newly produced allowlisted diagnostic events may be exported best-effort.

Enrollment is an explicit CLI action which shows the destination origin,
collection classes, retention warning, and prohibited data classes before it
reads a token. Configuration-file edits alone cannot make bzz reuse a credential
for a different endpoint.

### Operator-neutral client, Emilia-compatible deployment

The bzz implementation speaks standard OTLP/HTTP protobuf to one exact configured
HTTPS `/v1/logs` endpoint. It does not contain ClickHouse credentials, Emilia
admin credentials, a default bearer token, or an API for issuing tokens.

For Emilia-operated installations, an operator creates one scoped
`otel:logs:write` token per installation outside bzz. The public gateway validates
it, applies rate limits, and stamps authoritative `emilia.*` resource identity.
bzz must not send or trust client-authored `emilia.*` attributes.

The endpoint is not compiled as an active default. Documentation may use Emilia
as the worked deployment example, but another operator can configure a compatible
OTLP/HTTP protobuf logs endpoint without rebuilding bzz.

### Local evidence remains primary

The remote exporter observes the typed local diagnostic stream. It never tails,
parses, or uploads rotated journal files and never backfills historical events
automatically. Local diagnostics remain available when remote export is off,
misconfigured, rate-limited, or unreachable.

A bounded in-memory queue may retain a brief current window through a transient
failure, but there is no durable remote-upload spool. A crash or long outage can
therefore leave evidence only in the local journal by design. This avoids an
unexpected later upload of old operational history.

### Publication independence

OTel uses a separate HTTP client, bounded queue, task, timeout, and retry budget.
It owns no signer, relay supervisor, SQLite handle, media uploader, or TUI state.
It cannot change the bzz connection indicator, trigger a relay reconnect, retry
an outbox item, hold terminal shutdown open indefinitely, or compete with the
WebSocket acknowledgement path through an awaited send.

Exporter failures are recorded locally as normalized exporter health counters.
They are never re-enqueued for remote export, preventing recursive telemetry.

## Privacy and identity contract

### Allowed remote resource attributes

```text
service.name = bzz
service.version = <package version>
service.namespace = arpagon
os.type = linux | macos | windows | other
deployment.environment = desktop
```

The gateway/operator may add authoritative resource identity. The client does
not send pubkeys, identity UUIDs, labels, usernames, hostnames, local account
names, IP addresses, device model, locale, terminal environment, or arbitrary
environment variables.

A random installation ID is created only during explicit enrollment. It is
pseudonymous, contains no machine-derived bytes, and is not used for signing,
configuration partitioning, or relay identity. For Emilia, the per-installation
token's trusted subject identity is authoritative; a client installation ID is
optional correlation metadata and must not be presented as authenticated.

### Allowed remote log attributes

Remote events are a strict subset of the local diagnostic contract:

```text
bzz.event
bzz.schema_version
bzz.session_id
bzz.relay_origin
bzz.phase
bzz.attempt
bzz.error_class
bzz.duration_ms
bzz.backoff_ms
bzz.websocket_close_code
bzz.last_inbound_age_ms
bzz.receiver_lagged_count
bzz.event_id             # authored outbox/publish lifecycle only
bzz.event_kind           # authored outbox/publish lifecycle only
bzz.outbox_state
bzz.outbox_attempts
bzz.outbox_age_ms
bzz.export_dropped_count
```

Attribute values have fixed type/length/cardinality bounds. `Body` is a stable,
content-free event label rather than a formatted error or message. Severity is
mapped from event class: normal lifecycle `INFO`, recoverable transport/backoff/
rate-limit `WARN`, and terminal local exporter corruption/configuration `ERROR`.
A rejected relay publication is not automatically an exporter/system error.

### Prohibited remote data

In addition to every prohibition in the local diagnostics plan, remote export
must never include:

- pubkeys, Nostr identity IDs, participant IDs, community/channel/thread UUIDs,
  event authors, received event IDs, or subscription filters/IDs;
- message, draft, reaction, profile, mention, Inbox, agent, clipboard, attachment,
  media, or file data;
- raw relay notices, acknowledgement messages, HTTP response bodies/headers,
  WebSocket frame payloads, URLs beyond sanitized relay/export origins, SQL,
  config, panic, stdout/stderr, or generic tracing events;
- bearer tokens, key references, keychain diagnostics, auth challenges/events,
  NIP-98/NIP-OA material, cookies, TLS key material, or credential errors which
  might echo input; or
- local journal/report bytes or historical backfill.

A full authored event ID is allowed because it is the minimum key for proving
whether the relay received a pending publication. Its use is restricted to
publish/outbox lifecycle events and the documented retention boundary. Operators
can join it to relay logs; bzz does not send the author pubkey with it.

## Enrollment and command contract

Add a separate `telemetry` CLI group so a user cannot confuse local diagnostics
with remote transmission:

```text
bzz telemetry configure --endpoint <https-url>
bzz telemetry enable
bzz telemetry disable
bzz telemetry status [--json]
bzz telemetry test
bzz telemetry forget --yes
```

### `configure`

- Accept only an exact canonical HTTPS endpoint with no user-info, query, or
  fragment and with the final path `/v1/logs`.
- Display the canonical destination origin and privacy summary, then read the
  bearer token from the controlling terminal without echo. A documented
  `BZZ_OTEL_TOKEN` environment source is allowed for ephemeral/managed launches,
  but the value is never accepted as a command argument.
- Store a persisted token only in the OS credential service under a dedicated
  bzz telemetry service/key, separate from Nostr identities. Never write it to
  `config.toml`, SQLite, journal, report, shell history, CLI output, or panic
  context.
- Bind the credential reference to a digest of the canonical endpoint. Changing
  the endpoint leaves export disabled and requires a new explicit credential,
  preventing token forwarding after config tampering.
- Probe no endpoint and emit no log until the user runs `test` or `enable`.

If the OS credential service is unavailable, persistent enrollment fails safely.
The user may still provide an ephemeral environment token for that process. Do
not silently fall back to a plaintext or encrypted-file telemetry token which
would require an unattended passphrase policy.

### `enable` and `disable`

`enable` validates configuration/credential availability and writes only the
remote-export switch. It does not emit a test record implicitly. `disable`
stops future export and drains/drops the in-memory queue within a short bound;
it retains endpoint and credential so it can be re-enabled deliberately.

### `status`

Show enabled/configured state, endpoint origin, credential availability (never
its value/reference), last successful batch time, last normalized failure class,
queued/dropped counts, and effective queue/batch limits. It must not contact the
endpoint, unlock a Nostr identity, or expose installation identity in default
human output.

### `test`

Send one purpose-built `telemetry.test` record and report only HTTP success or a
normalized failure/status class. It contains no relay, event, outbox, community,
or identity attribute. A successful gateway response does not prove a later
ClickHouse row; operator-side verification remains separate.

### `forget`

After `--yes`, disable export, delete the endpoint-bound telemetry credential,
remove the pseudonymous installation ID and endpoint configuration, and discard
the in-memory queue. It does not clear local diagnostic journals or any bzz
conversation/identity state.

## OTLP/HTTP wire contract

- POST one protobuf `ExportLogsServiceRequest` to the exact configured endpoint.
- Set `Content-Type: application/x-protobuf` and
  `Authorization: Bearer <token>`; accept no redirect, because a redirect could
  disclose the bearer token.
- Use stable OTLP protobuf message types directly with the existing async HTTP
  stack. Audit and pin compatible `opentelemetry-proto`/`prost` dependencies;
  do not add the full OpenTelemetry SDK/exporter pipeline merely to send logs.
- Set `time_unix_nano` from the diagnostic event timestamp and
  `observed_time_unix_nano` at batch encoding. Monotonic durations remain
  integer attributes rather than reconstructed wall-clock intervals.
- Use one instrumentation scope such as `bzz-diagnostics` with bzz's version.
- Leave `TraceId`, `SpanId`, and trace flags empty. Logs-only correlation uses
  session and event IDs.

The exporter accepts only success-class HTTP responses as delivery. Normalize
responses without persisting their bodies:

| Result | Behavior |
|---|---|
| `2xx` | mark batch delivered and update local success health |
| `400` | drop batch as permanent schema/protobuf failure; local ERROR health |
| `401/403` | stop export for the run; local credential/config health warning |
| `413` | split once within record limits, then drop offending bounded record |
| `429` | honor a bounded parsed `Retry-After` if safe, otherwise capped backoff |
| `5xx`/connect/TLS/timeout | bounded retry with jitter, then drop/current health |
| redirect | permanent refusal; never forward Authorization |

Response bodies are drained only within a tiny fixed limit and discarded. They
are never inserted into local or remote diagnostics.

## Queue, batching, and rate limits

Initial compiled bounds:

- at most 256 queued records or 512 KiB of encoded estimate, whichever comes
  first;
- at most 64 records or 128 KiB per request;
- flush after five seconds when nonempty;
- five-second connect/request timeout;
- no more than three attempts per batch with capped exponential backoff and
  jitter;
- queued records expire after five minutes; and
- shutdown flush budget no longer than one second, after which records are
  dropped and remain represented only in the local journal.

When full, prefer retaining terminal connection/publish outcome events over
routine lifecycle records using a fixed priority policy; never evict an older
publish result in favor of repeated backoff noise. Coalesce repeated receiver
lag/backoff/export-health events into counters. Queue behavior is deterministic
and covered by virtual-time tests.

These limits remain below the Emilia example token policy of 120 requests/minute
and 1 MiB/minute under normal operation. bzz must still handle gateway `429`
without increasing relay traffic or changing the relay rate-limit policy.

## Architecture and ownership

Expected seams extending the local diagnostics implementation:

```text
src/diagnostics/event.rs       remote eligibility and attribute allowlist
src/diagnostics/mod.rs         fan-out to local journal and optional exporter
src/telemetry/
  mod.rs                       explicit runtime and exporter health
  config.rs                    canonical endpoint, enabled state, endpoint binding
  credential.rs                dedicated keychain/env token source and zeroization
  otlp.rs                      protobuf mapping/encoding with closed attributes
  exporter.rs                  bounded queue, batching, retries and shutdown
src/config.rs                  strict non-secret telemetry configuration
src/paths.rs                   pseudonymous installation metadata path if retained
src/main.rs                    telemetry CLI and TUI runtime wiring
src/error.rs                   normalized exporter failure classes
```

No generic `tracing` layer forwards application logs. The diagnostics event
model exposes a pure `remote_record()` conversion returning `None` for local-only
events and an allowlisted OTLP record for eligible events. Tests exhaustively
match every event variant so adding a future local diagnostic event defaults to
not exported until reviewed.

Credential access occurs once when starting the exporter and returns a
zeroizing in-memory token. The token is not cloneable through the general
application model. HTTP request construction receives it only at the final
transport boundary. Debug implementations for credential/request types must
redact it.

## Emilia operating contract

For the initial deployment:

- issue one API key per bzz installation with only `otel:logs:write`,
  `subject_type=process`, a pseudonymous subject ID, environment, allowed signal
  `logs`, and conservative request/byte limits;
- configure exactly `https://otel.emiliavision.com/v1/logs`;
- query authoritative identity through the materialized `Emilia*` columns in
  `observability.emilia_otel_logs`, not client resource hints;
- retain remote rows according to the operator table policy (currently 30 days),
  independently of the much smaller local rotating journal; and
- correlate authored `bzz.event_id` and timestamps with Buzz relay Kubernetes
  logs. Never require ClickHouse credentials on the bzz host.

Token issuing, rotation, revocation, ClickHouse queries, dashboards, and gateway
operations are external operator responsibilities. This bzz plan documents the
producer contract only and creates no Pi skill or Emilia repository change.

## Milestones

### M0 — Consent, schema, and threat model

- Freeze remote-eligible event variants, attributes, severity, resource fields,
  retention disclosure, endpoint validation, and credential lifecycle.
- Add golden OTLP protobuf fixtures and exhaustive tests proving local-only
  variants cannot be converted for remote export.
- Threat-model token forwarding, config tampering, redirects, proxy behavior,
  response-body leakage, queue amplification, endpoint outages, and shutdown.

**Exit:** reviewers can enumerate every byte class bzz may send before any
production endpoint is contacted.

### M1 — Configuration and credential enrollment

- Add strict off-by-default config, canonical endpoint binding, random
  installation enrollment identity, dedicated credential storage/env source,
  and configure/status/enable/disable/forget commands.
- Ensure `bzz check`, `bzz paths`, diagnostics reports, debug output, and errors
  never disclose token material or keychain references.
- Add endpoint-change and keychain-unavailable tests with no plaintext fallback.

**Exit:** a normal install sends nothing; an enrolled install can be enabled and
forgotten without changing Nostr identities or local diagnostics.

### M2 — Pure OTLP encoding and test command

- Add audited stable protobuf dependencies and a pure event-to-OTLP mapping with
  fixed resource/scope/log attributes.
- Implement no-redirect authenticated POST, tiny discarded response handling,
  normalized status classification, and the explicit test event.
- Validate encoded requests against a local fake OTLP receiver; no real token or
  Emilia endpoint is used in ordinary CI.

**Exit:** byte-level tests prove protobuf/content type/auth behavior and the
absence of prohibited attributes/body data.

### M3 — Non-blocking runtime export

- Fan out eligible typed diagnostic events through a bounded non-awaiting queue.
- Add deterministic batching, priority/coalescing, expiry, bounded retries,
  jitter, 401/403 stop behavior, 429 handling, health snapshots, and short
  shutdown.
- Force slow, unreachable, rate-limited, malformed, and adversarial fake
  endpoints while measuring TUI/session/outbox latency and queue memory.

**Exit:** remote failure cannot delay relay frames, publish ACKs, SQLite, input,
terminal restoration, or process exit, and local diagnostics retain normalized
exporter evidence.

### M4 — Emilia canary and correlation

- Issue a disposable per-installation scoped token externally and enroll one
  development profile against the Emilia gateway.
- Send only `telemetry.test`, then a synthetic/fake-relay connection journey,
  and verify trusted identity, resource fields, event schema, timestamp, and
  severity in ClickHouse.
- With a dedicated non-user Buzz identity/channel, publish one disposable event
  and demonstrate event-ID/time correlation between client OTel logs and relay
  Kubernetes logs. Revoke the canary token after validation.

**Exit:** one controlled incident timeline can identify client connect/AUTH,
publication outcome, and relay receipt without message content or user identity
in client telemetry.

### M5 — Documentation and release gate

- Update configuration, security, troubleshooting, diagnostics, manual E2E,
  privacy/retention, credential rotation/revocation, and release documentation.
- Record dependency audit, strict CI, fake exporter stress/virtual-time tests,
  release binary smoke, local-only regression, and explicit owner consent UX.
- Verify a clean install, upgrade, disabled configuration, missing keychain,
  revoked token, offline launch, and `forget` all remain usable locally.

**Exit:** OTel export is safe, understandable, reversible, operationally useful,
strictly optional, and does not weaken bzz's local-first boundaries.

## Test strategy

- Exhaustive event eligibility tests: every local `DiagnosticEvent` is explicitly
  remote or local-only, defaulting to local-only on future schema additions.
- Golden protobuf decode tests for resource/scope/log fields, timestamps,
  severity, event IDs, and empty trace/span fields.
- Leak/property tests with sentinel secrets, messages, tags, pubkeys, UUIDs,
  paths, URLs, control bytes, environment values, and arbitrary errors; none
  appear in body, attributes, headers except the test-only bearer assertion, or
  exporter health.
- Credential tests for no command-argument token, controlling-terminal input,
  environment source, keychain persistence, endpoint digest binding,
  zeroization boundaries, redacted Debug, disable, forget, and endpoint change.
- HTTP tests for exact URL, no redirects, no credential forwarding, content
  type, timeout, 2xx, 400, 401, 403, 413, 429/Retry-After, 5xx, truncated/large
  body, connect/TLS failure, and cancellation.
- Virtual-time queue tests for count/byte limits, batch thresholds, priority,
  coalescing, expiry, retries, jitter range, drop counters, stop state, and
  one-second shutdown.
- Integration tests proving exporter saturation adds no awaited operation to
  session/supervisor/outbox paths and does not alter relay request count.
- CLI/config/filesystem tests for default-off, invalid endpoint, no probe during
  configure/enable/status, test-only emission, private installation metadata,
  and complete forgetting.
- Manual canary queries verifying gateway-stamped identity and 30-day operator
  retention disclosure, using disposable events only.

## Non-goals

- No Android or Buzz Desktop instrumentation, mobile SDK, relay protocol change,
  server image change, Kubernetes deployment, ClickHouse schema/migration,
  Grafana dashboard, alert, or Pi/Emilia skill in this repository change.
- No OpenTelemetry metrics, traces, baggage, profiling, crash dumps, panic
  upload, session replay, screenshots, terminal capture, generic logs, or
  automatic historical journal upload.
- No shared embedded token, anonymous public ingestion, client-side token
  issuance, admin/config API credential, ClickHouse credential, or plaintext
  secret configuration.
- No automatic enablement, first-run consent dark pattern, background endpoint
  discovery, remote configuration, feature flags, or telemetry-controlled bzz
  behavior.
- No retry/outbox remediation, timeout tuning, WebSocket heartbeat change,
  Android connection fix, or relay rate-limit change based merely on adding
  telemetry.

## Acceptance criteria

1. A fresh, upgraded, or public bzz install emits zero remote requests until an
   owner explicitly configures credentials and enables telemetry.
2. Remote export sends only the reviewed typed event subset as OTLP/HTTP
   protobuf with fixed bounded attributes, empty trace/span fields, and no raw
   application/tracing log stream.
3. No message/identity/community/channel/media/agent/clipboard/path/config/raw
   error/credential data can enter OTLP body, attributes, exporter health, or
   support output under deterministic and generated hostile tests.
4. A bearer token is per installation, endpoint-bound, never an argument or
   plaintext file value, never redirected, and fully removed by `forget`.
5. Queue, memory, batch, retries, rate, retention-in-memory, and shutdown are
   hard-bounded; endpoint failure cannot delay bzz relay/network/UI/storage
   behavior or produce recursive telemetry.
6. Local diagnostics remain complete within their own bounds when remote export
   is disabled or failing, and no historical journal is uploaded later.
7. Emilia can correlate a disposable authored event ID and client timestamps
   with relay logs using gateway-stamped trusted identity, without giving bzz
   ClickHouse or admin credentials.
8. Existing identity, locked/cache-only, outbox acknowledgement, draft recovery,
   media, Inbox/read-state, terminal restoration, and human-send boundaries
   remain unchanged.

## Approved decisions

1. Remote telemetry is default-off for every build, including internal Emilia
   builds; managed launch configuration may enable it explicitly.
2. Each installation uses one externally issued token; no shared bzz token.
3. Complete authored event IDs may be retained remotely under the operator's
   current 30-day OTel table policy.
4. There is no durable remote spool or historical backfill; the local journal is
   the recovery path after long outages or crashes.
5. Persistent tokens require the OS credential service; environment tokens are
   the only non-persistent fallback.
