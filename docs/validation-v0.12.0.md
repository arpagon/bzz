# bzz v0.12.0 validation

**Status:** Implementation locally and CI validated 2026-08-28; publication pending

## Scope

- [x] Verified-agent typing moved from the composer into a segmented status bar.
- [x] Compact one-cell Braille animation with bounded multiple-agent and
  Unicode-cell forms.
- [x] Existing exact channel/thread authority, freshness, expiry, and
  reply-clearing behavior preserved.
- [x] Theme-derived agent, connection, media, and message-selection groups.
- [x] Detached selected-message background replaced by a message-scoped gutter.
- [x] Wrapped body lines retain the message indent.
- [x] Grouped timestamp and first body line render together.
- [x] No protocol, persistence, configuration, diagnostics, telemetry, or
  publication expansion.

## Source and design review

- Lip Gloss `examples/layout/main.go` was reviewed at
  `77047a83c800e97d198ed80825c7cc698cc6808c` for the visual concept of adjacent
  status nuggets only.
- bzz remains independently implemented with Ratatui and its semantic theme
  system. No Lip Gloss code, dependency, color constants, assets, or runtime
  behavior were copied.
- The Buzz compatibility baseline remains
  `9f55bf67456be10ff7c8238bf0d9e12e582848f6`; no upstream update was required.
- The private defect-report screenshot was not retained in source, fixtures,
  logs, diagnostics, snapshots, or release assets.

## Automated correctness

- [x] Single-agent spinner formatting and deterministic frame wraparound.
- [x] Multiple-agent count collapse and narrow fallback.
- [x] Unicode-cell-safe truncation.
- [x] Wide status render includes agent, connection, graphics, and help
  segments.
- [x] Minimum supported 50-column render keeps agent activity and drops help
  without wrapping.
- [x] Idle ticks still request no redraw.
- [x] Active typing advances at the bounded 200 ms interval.
- [x] Composer hit target begins on its first content row after removing typing.
- [x] Custom theme overrides for `StatusAgent` and `MessageSelected` resolve.
- [x] Theme export includes every new semantic group.
- [x] Grouped follow-up messages use one hanging-indent timestamp/body row.
- [x] Wrapped message body measurement uses the reduced indented width.
- [x] Date separators remain outside the selected-message gutter.
- [x] Library tests: 206 passed.
- [x] Strict all-feature/all-target Clippy.
- [x] Complete integration, session, snapshot, and TUI harness suites.
- [x] Benchmark smoke.
- [x] `cargo deny check`; only accepted duplicate-version warnings remain.
- [x] `cargo audit`; only the accepted `instant 0.1.13` and `lru 0.18.1`
  warnings remain.
- [x] The newly yanked transitive `chacha20 0.10.1` lock entry was advanced to
  compatible patch `0.10.2`; the Buzz Git source/revision and protocol baseline
  did not change.
- [x] Release build reports `bzz 0.12.0`.

## Functional and visual acceptance

- [x] Generated-fixture wide one-agent status bar.
- [x] Multiple-agent compact status is covered by deterministic renderer tests.
- [x] Exact open-thread typing and wrong-thread exclusion remain covered by the
  existing signed-event tests.
- [x] Normal, insert, and notice/activity status combinations are covered by
  renderer tests; existing app mode derivation remains unchanged.
- [x] Minimum-supported 50-column responsive status test and direct 60-column
  Ghostty review.
- [x] Default bzz, Tokyo Night dark, and Light built-in theme raster review;
  custom-group parsing and style resolution are covered independently.
- [x] Selected grouped/wrapped message uses a coherent multi-row gutter without
  a detached stripe; generated reactions remain visually distinct.
- [x] Writing dock is one row smaller and cursor/mouse placement remains exact.
- [x] No retained frame contains private user data.

Sanitized component-level raster evidence was captured from the exact v0.12.0
status and timeline renderers in direct Ghostty 1.3.1 on isolated Xvfb with a
generated in-memory corpus:

- `/tmp/bzz-v012-visual.DiOUmL/tokyo-night.png`;
- `/tmp/bzz-v012-visual.DiOUmL/light.png`;
- `/tmp/bzz-v012-visual.DiOUmL/narrow.png`; and
- `/tmp/bzz-v012-visual.DiOUmL/spinner.mp4` (2 seconds, H.264, 1600×900,
  10 fps capture of the bounded 5 fps one-cell animation).

Early, middle, and final frames plus a magnified status contact sheet were
reviewed. They show distinct spinner cells, stable surrounding segments,
message-aligned wrapping, and a selection gutter attached to the full grouped
message. The temporary fixture source was removed after capture.

## Cross-platform and release gates

- [x] Linux CI.
- [x] Windows x86_64 CI, including native keychain smoke.
- [x] Intel macOS CI, including native keychain smoke.
- [x] Apple Silicon macOS CI, including native keychain smoke.
- [x] Pinned Buzz relay integration CI.
- [ ] Tagged archives, checksums, SBOMs, and provenance attestations.
- [ ] Native keychain and encrypted-file smoke tests against downloaded tagged
  release artifacts.

Exact implementation commit `4d147301e3e0006bddc2d7835c3d55393519f7ed`
passed cross-platform CI in
[`33186774988`](https://github.com/arpagon/bzz/actions/runs/33186774988)
and the pinned Buzz real-relay integration in
[`33186774968`](https://github.com/arpagon/bzz/actions/runs/33186774968).

## Non-regression boundary

v0.12.0 continues to publish no human typing or observer events, stores no
typing state, and makes no runtime-readiness claim. It adds no local agents,
child process, agent key, ACP path, model/tool control, schema migration,
activity archive, Inbox/search/unread integration, diagnostics identifier, or
OTel field.
