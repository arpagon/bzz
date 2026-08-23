# v0.8.0 clipboard-first attachment validation

**Status:** Local and CI validation completed 2026-08-21. Native desktop
clipboard and release-artifact review remain required before tagging.

## Completed gates

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked` — 125 unit/integration tests passed; the pinned real
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
- GitHub [CI run 32519434260](https://github.com/arpagon/bzz/actions/runs/32519434260)
  passed Linux, macOS (Intel/Apple Silicon), Windows, and test jobs for
  implementation commit `83f7760`; pinned relay integration
  [run 32519434326](https://github.com/arpagon/bzz/actions/runs/32519434326)
  also passed.

## Automated behavioral coverage

The deterministic suite covers the dependency-free fake clipboard and native
file-picker boundaries, including picker cancel/unavailable/stale-target,
full-general-queue completion, selection bounds, bounded text normalization,
malformed/oversize bitmap
rejection, PNG encoding, strict config parsing, typed composer bindings, UTF-8
paste/mention behavior, queue rendering on a non-graphics TestBackend, secure
staging, and persisted attachment replacement by opaque draft ID. Existing
media protocol, authenticated upload, cache, identity, locked-mode, Inbox, and
terminal layout tests continue to pass.

## Draft acknowledgement follow-up — 2026-08-22

- Added migration 0006 for opaque draft revisions, sending state, and outbox
  event association. `bzz check` applied it successfully to the local database;
  schema version 6 is valid and no draft was left in `sending` state.
- `cargo fmt --check`, strict Clippy, `cargo test --locked` (132 tests; the
  pinned external relay journey remains intentionally ignored), `cargo deny
  check`, `cargo audit`, `cargo build --release --locked`, `bzz --version`,
  `bzz check`, `bzz media status`, and `git diff --check` passed locally.
- The new deterministic fake-relay and store suite covers accepted/rejected
  delivery, uncertain recovery, crash recovery, late acknowledgements, and
  thread isolation without recording message content.

## Required pre-tag manual review

Use disposable generated files/text/images only:

1. In Ghostty under a supported desktop clipboard, copy a file, an image, and
   plain text in turn and verify `Ctrl-v` queue/text behavior and no automatic
   send.
2. Check the `Ctrl-o` native chooser (single, multiple, cancel, unavailable),
   `Alt-o` path fallback, `Delete`, `Ctrl-r`, `Ctrl-c` confirmation,
   `media.clipboard_import = "off"`, restart persistence, and a late upload
   after removal/clear.
3. Repeat the queue and fallback journey in a non-graphics terminal. On Linux,
   verify direct XDG portal use and no zenity/shell fallback. Retain no
   clipboard content, private paths, or production relay data in evidence.
