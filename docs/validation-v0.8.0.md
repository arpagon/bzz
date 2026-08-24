# v0.8.0 clipboard-first attachment validation

**Status:** Local, native-desktop, and pre-release CI validation completed.
Release artifacts are verified after the `v0.8.0` workflow finishes.

## Completed gates

- `cargo fmt --check`
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo test --locked` — 240 unit/integration tests passed; the pinned real
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
malformed/oversize bitmap rejection, PNG encoding, strict config parsing,
typed composer bindings, single-click channel timeline activation, UTF-8
paste/mention behavior, queue rendering on a non-graphics TestBackend, secure
staging, and persisted attachment replacement by opaque draft ID. Existing
media protocol, authenticated upload, cache, identity, locked-mode, Inbox, and
terminal layout tests continue to pass. Relay regressions additionally cover
bounded in-memory event deduplication, idempotent outbox echoes, suppression of
closed-subscription retry loops, input/attachment priority, and inert idle
redraw ticks.

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

## Native desktop and runtime review — 2026-08-23

- A live explicit bitmap paste reached the attachment queue through the native
  clipboard and prioritized staging path without sending automatically.
- The Linux `Ctrl-o` XDG Desktop Portal chooser opened successfully and staged
  a selected file through the same bounded lifecycle without exposing its path
  or contents in evidence.
- The final release binary remained responsive during channel switching. A
  15-second steady-state sample measured about 1% process CPU, zero socket send
  backlog, zero SQLite event growth, and zero WAL growth. This replaced the
  diagnosed closed-subscription retry loop and repeated outbox-echo writes.
- The release build reports `bzz 0.8.0`; `bzz check` validates configuration,
  theme, media, and schema-v6 database state.
