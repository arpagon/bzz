# v0.8.0 clipboard-first attachment validation

**Status:** Local automated validation completed 2026-08-21. Native desktop
clipboard and release-artifact review remain required before tagging.

## Completed gates

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked` — 123 unit/integration tests passed; the pinned real
  relay test remains intentionally ignored unless its isolated harness is
  configured.
- `cargo deny check` — advisories, bans, licenses, and sources passed. Existing
  duplicate-version warnings remain; the pinned clipboard dependency's Windows
  BSL-1.0 bridge is explicitly reviewed in `deny.toml` and
  `THIRD_PARTY_LICENSES.md`.
- `cargo audit` — only the repository's existing allowed warnings for
  `instant` and `lru` were reported.
- `cargo build --release --locked`, `target/release/bzz --version` (`bzz 0.8.0`),
  `target/release/bzz check`, and `target/release/bzz media status` passed.

## Automated behavioral coverage

The deterministic suite covers the dependency-free fake clipboard boundary,
bounded text normalization, malformed/oversize bitmap
rejection, PNG encoding, strict config parsing, typed composer bindings, UTF-8
paste/mention behavior, queue rendering on a non-graphics TestBackend, secure
staging, and persisted attachment replacement by opaque draft ID. Existing
media protocol, authenticated upload, cache, identity, locked-mode, Inbox, and
terminal layout tests continue to pass.

## Required pre-tag manual review

Use disposable generated files/text/images only:

1. In Ghostty under a supported desktop clipboard, copy a file, an image, and
   plain text in turn and verify `Ctrl-v` queue/text behavior and no automatic
   send.
2. Check `Delete`, `Ctrl-r`, `Ctrl-c` confirmation, `Ctrl-o` fallback,
   `media.clipboard_import = "off"`, restart persistence, and a late upload
   after removal/clear.
3. Repeat the queue and fallback journey in a non-graphics terminal. Retain no
   clipboard content, private paths, or production relay data in evidence.
