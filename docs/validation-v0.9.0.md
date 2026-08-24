# v0.9.0 validation record

## Scope

This record covers the combined local-diagnostics and opt-in OTLP observability
release defined in `docs/planning/2026-08-23/v0.9.0.md`.

## Automated gates

Local release-candidate execution:

- [x] `cargo fmt --check`
- [x] `cargo check --locked`
- [x] `cargo test --locked --all-targets` (269 tests passed; the opt-in real-relay
  test remains intentionally ignored outside its wrapper)
- [x] `cargo clippy --all-targets --all-features --locked -- -D warnings`
- [x] `cargo deny check` (advisories, bans, licenses, and sources passed; existing
  duplicate-version warnings remain informational)
- [x] `cargo audit` (only the two documented allowed `instant`/`lru` warnings)
- [x] `cargo build --release --locked`
- [x] release `bzz 0.9.0`, `bzz check`, diagnostics CLI/report/mode smoke, and
  media status in isolated profile directories
- [x] Linux, Windows, Intel macOS, and Apple Silicon macOS CI — tagged-head run
  [`32753141731`](https://github.com/arpagon/bzz/actions/runs/32753141731)
- [x] pinned Buzz integration workflow — run
  [`32689026337`](https://github.com/arpagon/bzz/actions/runs/32689026337)

## Security and behavior evidence

Implemented automated coverage includes:

- typed diagnostic schema and stable event names;
- raw error reduction to a closed class and hostile sentinel leak checks;
- owner-only journal/report permissions, fixed rotation, and overwrite refusal;
- read-only metadata outbox projection proving database bytes are unchanged and
  message/event JSON, labels, paths, credentials, and raw errors are absent;
- exact pending/delivery-unknown/rejected timeline rendering;
- strict default-on local/default-off remote configuration and endpoint binding;
- dedicated telemetry credential-service isolation;
- exhaustive OTLP attribute filtering, stable protobuf body/resource/scope,
  empty trace/span fields, and local-only event exclusion;
- fake receiver verification of exact `/v1/logs`, protobuf content type, bearer
  boundary, decoded test event, and redirect refusal;
- hard queue count/byte saturation without awaiting application work; and
- all pre-v0.9 identity, draft acknowledgement, outbox, relay, media, Inbox,
  read-state, terminal, and human-send tests.

## Manual gates

- [x] clean isolated configuration/status/check paths create no exporter while
  telemetry is off
- [x] diagnostics status/outbox/report and overwrite/privacy behavior against
  disposable evidence
- [x] release report mode `0600`; journal mode/rotation covered automatically
- [x] scoped-token `telemetry.test` canary accepted by the Emilia endpoint from
  both debug and release binaries
- [ ] gateway-stamped identity and expected schema confirmed operator-side
- [ ] disposable event-ID/time client/relay correlation
- [x] isolated canary enrollment forgotten after validation; token lifecycle
  remains external operator policy
- [x] missing/tampered endpoint or credential startup degrades to local-only;
  redirect/status/offline/queue behavior is covered by bounded fake receivers

## Release decision

The bzz host has only the intended write-scoped ingestion credential, so it
cannot independently query gateway-stamped rows or relay Kubernetes logs. On
2026-08-24 the owner authorized release with the two unchecked operator-only
correlation items explicitly deferred. Accepted debug/release canaries,
client-side protobuf/privacy coverage, and default-off enrollment remain hard
gates; this exception does not treat an accepted HTTP response as operator-side
evidence.

No real message, identity, attachment, clipboard value, source path, or
production channel is used as validation evidence.
