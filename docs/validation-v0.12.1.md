# bzz v0.12.1 validation

**Status:** Local implementation and pinned-relay validation complete
2026-08-28; CI, production acceptance, and publication pending

## Scope

- [x] Eight-frame-per-second outbound WebSocket admission.
- [x] Interactive publication, selected scope, baseline, recovery/maintenance,
  and background priorities.
- [x] Same-ID subscription coalescing and paced reconnect replay.
- [x] Typed bounded rate-limit hint parsing, gate extension, jitter, and retry.
- [x] Rate-limited subscriptions remain desired; terminal closures fail closed.
- [x] At most one publication awaits relay acknowledgement.
- [x] No automatic EVENT after explicit rejection or legacy uncertainty.
- [x] Transient one-row quota status without false offline/access-denied state.
- [x] Local-only identifier-free activation/recovery evidence.
- [x] No protocol, schema, configuration, dependency, or pinned-Buzz change.

## Deterministic correctness

- [x] Canonical, mixed-case, absent, zero, huge, and malformed retry hints.
- [x] Retry hints are clamped to 300 seconds.
- [x] Stable 100–500 ms subscription jitter and one-to-30-second transient
  backoff.
- [x] Unknown/hostile closures fail closed rather than driving a retry loop.
- [x] Foreground and baseline desired-subscription ordering is deterministic.
- [x] Same-ID replacement retains only the newest filter and priority.
- [x] Interactive publication outranks recovery and maintenance queues.
- [x] Local publication queue is capped at 64 and expires before wire send.
- [x] Rate-limit status keeps `online`, overrides an older notice only while
  active, and restores the prior notice after expiry.
- [x] New diagnostics serialize only typed source/duration buckets and aggregate
  counts.

## Fake-relay acceptance

- [x] Twelve immediate subscriptions are spaced by the 125 ms admission clock.
- [x] A first-REQ `CLOSED rate-limited: retry in 1s` produces no terminal
  closure, remains desired, and reaches EOSE on exactly one retry.
- [x] A correlated `OK false rate-limited` is returned once and is never
  republished automatically.
- [x] A legacy uncorrelated rate-limit NOTICE resolves the single pending EVENT
  as uncertain, activates the gate, and sends no duplicate.
- [x] Client `CLOSE` acknowledgement remains consumed without feedback.
- [x] Terminal relay closure remains forgotten without a defensive wire CLOSE.

## Local gates

- [x] `cargo fmt --all -- --check`.
- [x] `git diff --check`.
- [x] Strict locked all-feature/all-target Clippy.
- [x] All-feature/all-target compile check.
- [x] Library tests: 217 passed.
- [x] Session/fake-relay tests: 11 passed.
- [x] Complete integration, snapshot, TUI, storage, and platform-neutral test
  suite.
- [x] Benchmark smoke for media, store/Inbox/search, timeline, and redraw gates.
- [x] `cargo deny check`; only the repository's accepted duplicate-version
  warnings remain.
- [x] `cargo audit`; only accepted `instant 0.1.13` and `lru 0.18.1` warnings
  remain.
- [x] Release build reports `bzz 0.12.1`.

The Linux development host required only a temporary external `pkg-config`
search path for its already installed Nix dbus development output. No build
configuration, dependency, or repository file was changed for that host setup.

## Cross-platform and release gates

- [ ] Linux CI.
- [ ] Windows x86_64 CI, including native keychain smoke.
- [ ] Intel macOS CI, including native keychain smoke.
- [ ] Apple Silicon macOS CI, including native keychain smoke.
- [x] Local pinned Buzz relay integration at
  `9f55bf67456be10ff7c8238bf0d9e12e582848f6`.
- [ ] Pinned Buzz relay integration CI at the same revision.
- [ ] Controlled production startup/reconnect acceptance.
- [ ] Tagged archives, checksums, SBOMs, and provenance attestations.
- [ ] Native keychain and encrypted-file smoke tests against downloaded tagged
  artifacts.

## Production acceptance checklist

- [ ] Startup with the known joined-channel set emits paced bzz REQs.
- [ ] A quota closure self-recovers without socket reconnect, AccessDenied, or
  permanent typing quarantine.
- [ ] `relay busy · retrying in Ns` clears after the gate.
- [ ] Selected-channel messages and verified-agent typing resume.
- [ ] A rejected/uncertain publication retains its exact draft and is not
  duplicated.
- [ ] Concurrent Buzz Desktop traffic is isolated or explicitly accounted for
  when evaluating the shared-principal quota.

## Non-regression boundary

v0.12.1 keeps the v0.12 presentation, Inbox, unread/read state, search, media,
clipboard, native picker, identity isolation, locked mode, agent verification,
structured mentions, thread summaries, transient typing, and explicit human
publication boundary unchanged. It adds no local agents, runtime controls,
observer stream, migration, server mutation, or private diagnostic field.
