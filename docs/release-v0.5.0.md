# bzz v0.5.0 (release candidate)

> **Status:** candidate — publish only after the checks in this document and
> [`releasing.md`](releasing.md) are recorded for the tagged commit.

v0.5.0 is a clean-room workspace readability release. It retains the
`MIT OR Apache-2.0` license, Rust 1.95 baseline, existing SQLite schema and
identity/backup flow, and the pinned Buzz/slk dependency revisions. It adds no
protocol capability or presentation-driven network request.

## Highlights

- A calm, role-aware workspace shell: labelled local communities, channel
directory, conversation priority, and context only when useful.
- Deterministic local author markers, date separators, compact nearby-author
grouping, and configurable `ui.message_width` (48–200 cells; default 110) for
wide conversations. Markers never fetch profile pictures or URLs.
- A visible writing dock that activates the existing composer with `i` or a
mouse target. It remains explicit and disabled in locked/missing-identity or
non-publishable states; drafts, attachments, review, and human-send semantics
are unchanged.
- Inbox list/detail hierarchy now shares the conversation visual language,
without changing eligibility, read acknowledgement, or visibility rules.
- New semantic theme groups for avatar, date separator, composer hint, and
composer-disabled meaning. Text/weight markers remain available in transparent
and low-colour terminals.
- Event-driven redraw admission coalesces visible work and eliminates the old
unconditional 100-ms idle-frame loop.

## Visual evidence

The committed visual evidence uses only deterministic bzz-owned synthetic
TestBackend messages and layouts; it is covered by `tests/ui_snapshot_test.rs`,
`tests/ui_timeline_test.rs`, and `tests/theme_test.rs`. The visual-review
rubric and private-owner approval record are in
[`visual-review-v0.5.md`](visual-review-v0.5.md). No user screenshot,
community message, external artwork, or third-party comparison asset is part
of this release.

## Performance evidence

On an AMD Ryzen 7 3700X/Linux 6.11 host with rustc 1.95, a 500-message
TestBackend timeline render at a 110-cell measure took **1.775–1.805 ms**.
The redraw gate admits zero frames across 1,000 idle ticks after the initial
frame. Reproduction command and limitations are in
[`benchmark-v0.5.md`](benchmark-v0.5.md).

## Compatibility and non-goals

- Existing `config.toml` gains an optional `ui.message_width`; absent values
  retain the safe 110-cell default. Out-of-range values fail before raw mode.
- No migration changes, no secret format changes, and no Buzz/slk pin change.
- Inbox remains scoped and derived; it does not become a global unread feed or
  automatically acknowledge rows.
- Profile avatars, arbitrary remote images, typing/presence, message editing,
  and new protocol features remain out of scope.
- The v0.5 implementation is independently authored. The clean-room decision
  is documented in
  [`adr-v0.5-workspace-visual-language.md`](adr-v0.5-workspace-visual-language.md).

## Required candidate gates

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check
cargo audit
cargo bench --bench timeline --locked
BZZ_BUZZ_SOURCE=/path/to/pinned/buzz ./scripts/test-relay.sh
cargo build --release --locked
BZZ_BIN="$PWD/target/release/bzz" BZZ_HERDR_PANE=<disposable-pane> \
  ./scripts/test-tui-herdr.sh
```

All commands above passed locally on 2026-08-20. `cargo deny check` emitted
only the repository's documented duplicate-version warnings; `cargo audit`
reported only the existing allowed transitive `instant` unmaintained and `lru`
panic-safety advisories. The pinned real-relay journey passed using a detached
disposable checkout at `ede26863345a518ec46edd6d7692e0281883491b`. The
release-binary Herdr scenarios both passed in a disposable pane without a
credential or relay.

The Herdr scenario target must be a disposable shell pane and must never
receive identities or passphrases. Follow the archive, checksum, SBOM,
provenance, supported-platform, and clean-VM steps in [`releasing.md`](releasing.md)
before publishing.
