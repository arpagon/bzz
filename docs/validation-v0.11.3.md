# bzz v0.11.3 validation

**Status:** Implementation locally validated 2026-08-26; publication pending

## Scope

- [x] Fresh signed ephemeral kind `20002` admission for current verified remote
  agents.
- [x] Exact selected-channel and open-thread scoping.
- [x] Eight-second bounded in-memory reduction, reply clearing, and stale-signal
  suppression.
- [x] Compact width-aware `◆ <agent> is typing…` composer-boundary presentation.
- [x] Dedicated selected-channel subscription with bounded overlap.
- [x] Explicit durable-store rejection.
- [x] Content-free local classification of a relay-closed typing subscription,
  with raw text and all relay/scope/identity/event fields discarded and no OTel
  export.
- [x] No human typing publication, migration, configuration, observer
  decryption, runtime control, Inbox/search/unread/read-state integration,
  diagnostics content, or OTel export.

## Source review

- Pinned Buzz reviewed at
  `9f55bf67456be10ff7c8238bf0d9e12e582848f6`.
- `buzz-core` defines kind `20002`.
- `buzz-acp` signs empty-content exact-`h` events with optional root/reply tags
  and refreshes them every three seconds while processing.
- Buzz Desktop subscribes with exact kind/`#h`, `limit: 10`, and a ten-second
  overlap; its reducer uses an eight-second lifetime and clears on matching
  authored messages.
- Pinned relay conformance tests establish ephemeral non-history behavior and
  community-scoped live fan-out.

## Automated correctness

- [x] Fresh channel/direct-thread/nested-thread parser tests.
- [x] Invalid signature/event, wrong channel, expired, malformed, duplicate,
  ambiguous-coordinate, and future-clamp behavior.
- [x] Current verified-agent authorization and ordinary-author rejection.
- [x] Refresh-without-duplicate, exact reply clearing, delayed-signal
  suppression, deterministic expiry, and bounded state.
- [x] Exact selected-channel subscription filter.
- [x] Single/multiple/narrow/Unicode-cell presentation tests.
- [x] Renderer smoke proves the typing row is visible and has no direct hit
  target.
- [x] Store-level kind `20002` rejection and no message persistence.
- [x] Typing-subscription `CLOSED` classification, status aggregation, local
  journal emission, hostile-text redaction, scope-identifier exclusion, and
  explicit OTel exclusion tests.
- [x] `cargo fmt --all -- --check` and `git diff --check`.
- [x] Strict all-feature/all-target Clippy.
- [x] `cargo test --locked --all-features --all-targets`: 200 library tests,
  every integration suite, and benchmark smoke targets passed.
- [x] Pinned Buzz real-relay live typing and no-history journey.
- [x] `cargo deny check`; only accepted duplicate-version warnings remain.
- [x] `cargo audit`; no denied vulnerability and only the accepted transitive
  `instant 0.1.13` and `lru 0.18.1` warnings remain.
- [x] Release build reports `bzz 0.11.3`; isolated `bzz check` reports
  configuration, theme, media, diagnostics, telemetry, and database valid.

## Functional and visual acceptance

- [x] Sanitized wide generated-fixture channel indicator.
- [x] Sanitized generated-fixture open-thread indicator; deterministic tests
  cover closed/wrong-thread exclusion.
- [x] Narrow indicator remains one readable row without overlap.
- [x] Matching signed reply clears only the exact author/scope without moving
  selection or cursor.
- [x] Channel/community switch, disconnect, lock, and shutdown clear state.
- [x] Twelve seconds of two-second live refreshes left the isolated SQLite file
  unchanged at 380,928 bytes and the same SHA-256; ignored refreshes do not
  request redraws.
- [x] Direct release-binary `q`/confirmation exit restored Ghostty normally.

Sanitized raster evidence from `bzz 0.11.3`, direct Ghostty/Xvfb, a generated
encrypted identity, and the pinned disposable Buzz relay:

- `/tmp/bzz-v0113-visual-evidence.bqBo6U/wide.png`;
- `/tmp/bzz-v0113-visual-evidence.bqBo6U/narrow.png`; and
- `/tmp/bzz-v0113-visual-evidence.bqBo6U/thread.png`.

All visible names, message text, channel names, identities, and relay data in
these frames were generated for the disposable fixture. The fixture credentials,
keys, database, logs, containers, relay process, and temporary source helper
were removed after review.

## Cross-platform and release gates

- [x] Linux CI.
- [x] Windows x86_64 CI.
- [x] Intel macOS CI.
- [x] Apple Silicon macOS CI.
- [x] Pinned Buzz relay integration CI.
- [ ] Tagged archives, checksums, SBOMs, and provenance attestations.
- [ ] Native keychain and encrypted-file release-artifact smoke tests.

Implementation CI passed in
[`32930904850`](https://github.com/arpagon/bzz/actions/runs/32930904850),
including Linux, Windows, Intel macOS, Apple Silicon, tests, deny, audit, and
native target smoke. Pinned Buzz integration passed in
[`32930904820`](https://github.com/arpagon/bzz/actions/runs/32930904820).

## Environment note

Local all-feature builds use the established development environment path:

```sh
PKG_CONFIG_PATH=/opt/arpagon/pixi/envs/conda-smithy/lib/pkgconfig
```

Without it, `libdbus-sys` cannot locate the available `dbus-1.pc`; this is an
environment configuration issue rather than a source failure.
