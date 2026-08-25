# bzz v0.11.2 validation

**Status:** In progress 2026-08-25

## Scope

- [x] Bounded local descendant summaries for retained timeline roots.
- [x] Strict transient pinned-relay kind `39005` parser and selected-channel
  subscription.
- [x] Compact timeline reply count and last-activity presentation.
- [x] Context title counts replies rather than root-plus-reply messages.
- [x] No database migration, durable summary event, polling, read-state, Inbox,
  search, diagnostics, telemetry, or publication-boundary change.

## Source review

- Concord reviewed at `0bac5125ddbecb34b357c40d0348bf660c6099ba`.
  Its Discord thread cards validate visible count/activity metadata but are not
  copied because Discord threads are separate channels.
- Pinned Buzz reviewed at
  `9f55bf67456be10ff7c8238bf0d9e12e582848f6`; current Buzz reviewed at
  `7a1b7d8e09f96c10b6617a66bb8589e9984ffb3c`.
- Buzz's `MessageThreadSummaryRow`, descendant-count merge, relative activity,
  and relay-signed kind `39005` contract are the behavioral reference.

## Automated correctness

- [x] Strict valid/wrong-signer/mismatched-coordinate/inconsistent/zero kind
  `39005` parser tests.
- [x] Visible-root/channel scoping and deterministic live-summary replacement.
- [x] Local direct+nested count, pending exclusion, delivered inclusion,
  deletion recomputation, latest activity, and channel isolation.
- [x] Singular/plural summary rendering and expanded relative-time boundaries.
- [x] Existing timeline call sites and benchmark targets compile with explicit
  summary inputs.
- [x] `cargo fmt --check` and `git diff --check`.
- [x] Strict Clippy with all features and targets.
- [x] `cargo test --locked --all-features --all-targets`: 185 library tests,
  all integration suites, and all benchmark smoke targets passed.
- [x] Pinned Buzz real-relay suite passed, including a signer-validated live
  kind `39005` and a locally reconstructed post-deletion count.
- [x] `cargo deny check`: advisories, bans, licenses, and sources passed; only
  accepted duplicate-version warnings remain.
- [x] `cargo audit`: no denied vulnerability; accepted transitive warnings are
  `instant 0.1.13` (unmaintained) and `lru 0.18.1` (RUSTSEC-2026-0253).
- [x] Release build passed; binary reports `bzz 0.11.2`; `bzz check` reports
  configuration, theme, media, diagnostics, telemetry, and database valid.

## Functional and visual acceptance

- [x] Release binary shows summaries on a sanitized production-derived isolated
  fixture without modifying the production profile.
- [x] One, two, and eleven-reply roots are distinguishable while zero-reply
  roots retain their previous height.
- [x] Opening the selected root reports `context · 11 replies`, keeps the root
  first, and renders all eleven descendants.
- [x] Direct reply, nested reply, accepted delivery, and deletion updates pass
  deterministic local and pinned-relay tests without polling.
- [x] A 75×28 Ghostty/Xvfb review keeps counts/activity aligned without overlap;
  the summary remains part of the root's measured selection block and `Enter`
  opens exact context.
- [x] Summary state is renderer-only/transient and absent from copied authored
  content, search, Inbox, diagnostics, and OTel code paths.
- [x] Direct release-binary `q` exit returned status 0. During a 30-second quiet
  cache-only observation, the isolated DB SHA-256 and 4,222,976-byte size were
  unchanged, CPU advanced 15 ticks, and RSS moved 16,660→16,840 KiB.

Sanitized local raster evidence:

- `/tmp/bzz-v0112-visual.B1EEwr/closed.png`;
- `/tmp/bzz-v0112-visual.B1EEwr/open.png`; and
- `/tmp/bzz-v0112-visual.B1EEwr/narrow.png`.

## Cross-platform and release gates

- [ ] Linux CI.
- [ ] Windows x86_64 CI.
- [ ] Intel macOS CI.
- [ ] Apple Silicon macOS CI.
- [ ] Pinned Buzz relay integration.
- [ ] Tagged archives, checksums, SBOMs, and provenance attestations.
- [ ] Native keychain and encrypted-file release-artifact smoke tests.

## Notes

The first local all-target check required the established environment path:

```sh
PKG_CONFIG_PATH=/opt/arpagon/pixi/envs/conda-smithy/lib/pkgconfig
```

Without it, `libdbus-sys` could not locate `dbus-1.pc`; this is an environment
configuration issue, not a source failure.
