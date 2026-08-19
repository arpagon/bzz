# bzz v0.4.0 (release draft)

> **Status:** published as [`v0.4.0`](https://github.com/arpagon/bzz/releases/tag/v0.4.0)
> on 2026-08-19. Release and artifact verification evidence is recorded below.

`v0.4.0` is a clean-room interaction and Inbox cutover. It retains bzz's
`MIT OR Apache-2.0` license and the pinned Buzz protocol revision
`ede26863345a518ec46edd6d7692e0281883491b`; neither Buzz nor slk dependency
pins change as part of this release.

## Highlights

- Typed, scoped, validated `keymap.toml` input routing with `Space` leader,
  generated effective help, which-key, bounded sequences, and text ownership.
- Shared route/focus/viewport presentation state, responsive panes, semantic
  generation-bound mouse targets, and explicit contextual action availability.
- Conversational Inbox list/detail workspace with stable selection, narrow
  route-local detail, bounded local source context, and a first-unread anchor.
- `i` opens the normal composer at the selected validated Inbox target without
  acknowledging it. `o` opens canonical context with an Inbox return target;
  `m`, `U`, and confirmed `a` are explicit local/read-state operations.
- Derived, rebuildable schema-v5 Inbox conversations projection. Event bodies
  remain authoritative in `events`; projection pages and per-conversation
  windows stay bounded and partitioned by community and identity.

## Upgrade and downgrade

Opening an older database runs the normal owner-only pre-migration backup,
then applies migrations through schema version 5. Verify the backup is
readable before replacing or deleting an old data directory. A downgrade is
not reverse SQL: stop bzz and restore the pre-migration backup appropriate to
the prior binary. Do not open the same SQLite database with two clients.

The migration does not publish events, modify relay protocol behavior, make an
Inbox global, or create a second authoritative message store. Locked and
missing/corrupt identity modes remain cache-only and must not refresh,
publish, or sign.

## Known non-goals

- Concord code, dependencies, assets, test data, and strings are not included.
- Inbox is not an all-channel unread feed and does not autonomously mark,
  publish, execute needs-action cards, or invoke an agent.
- NIP-17 gift-wrap transport, typing/presence, custom emoji, media playback,
  profile avatars, and editing remain out of scope.
- Workspace DMs remain relay-membership private, not end-to-end encrypted.
- The Herdr suite is a controlled operator/self-hosted acceptance gate, never a
  runtime dependency or a carrier for credentials.

## Release-candidate evidence

### Recorded candidate

- Package candidate: `082fc4bcfc1ba2e338d1f372b8ae5f9ce6c7a70e`
  (`release: v0.4.0`), validated 2026-08-19.
- Required local gates passed against the committed locked dependency graph:
  formatting, strict Clippy, all-target test targets, `cargo deny check`,
  `cargo audit`, release build, and timeline/store/media benchmarks.
- The controlled final release-binary Herdr run passed both declared scenarios
  in a disposable pane without credentials. Pinned real-relay integration
  passed using a temporary checkout at
  `ede26863345a518ec46edd6d7692e0281883491b`.
- CI and pinned-Buzz integration passed for the candidate:
  [CI](https://github.com/arpagon/bzz/actions/runs/32219695390) and
  [integration](https://github.com/arpagon/bzz/actions/runs/32219695457).
- `cargo audit` has only the documented, transitive `instant` unmaintained and
  `lru` panic-safety advisories; dependency pins were deliberately not changed
  for this release. `cargo deny` has only the configured duplicate-version
  warnings.

### Published artifact verification

- The tagged release targets `e83d346a356553c719ed71ba5d7760777b99f2b9` and
  the release workflow passed: [run 32274135708](https://github.com/arpagon/bzz/actions/runs/32274135708).
  It built and smoke-tested release binaries on Linux, macOS (Intel and ARM),
  and Windows hosted runners.
- All six entries in the downloaded `sha256.sum` verified successfully:
  five platform archives and `source.tar.gz`.
- All 17 attested release subjects (archives, per-file hashes, installers,
  SBOM, Homebrew formula, manifest, and source) verified with
  `gh attestation verify`, scoped to `arpagon/bzz`,
  `.github/workflows/release.yml`, and `refs/tags/v0.4.0`.
- The downloaded CycloneDX SBOM identifies `bzz 0.4.0` under
  `MIT OR Apache-2.0` and the exact pinned Buzz revision. The downloaded Linux
  archive unpacked cleanly and its binary reported `bzz 0.4.0`.

Hosted CI supplies the supported-platform clean-runner coverage; a separate
operator clean-VM installer/keychain exercise remains the prerequisite for
publishing any package-manager manifest.

Required commands are:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo deny check
cargo audit
cargo bench --bench timeline --bench store --bench media
BZZ_BUZZ_SOURCE=/path/to/pinned/buzz ./scripts/test-relay.sh
cargo build --release --locked
# Controlled Herdr/operator environment only:
BZZ_BIN="$PWD/target/release/bzz" BZZ_HERDR_PANE=<pane-id> \
  ./scripts/test-tui-herdr.sh
```

The deterministic TestBackend functional harness is ordinary CI authority.
The Herdr run additionally verifies the release binary, real terminal input,
visible lifecycle, and restoration; record a skipped run as a release
exception, not as an equivalent test pass. Verify checksum, archive, SBOM, and
provenance on clean platform VMs as described in [`releasing.md`](releasing.md).
