# v0.5 workspace validation evidence

**Recorded:** 2026-08-20 UTC

This is implementation-stage evidence for the workspace visual changes. It is
not a release note and contains no user conversations, identities, relays, or
external screenshots.

## Automated checks

```text
cargo fmt --check                                      PASS
cargo clippy --all-targets --locked -- -D warnings     PASS
cargo test --locked                                    PASS (98 unit tests; opt-in relay test ignored here)
cargo deny check                                       PASS (documented duplicate warnings)
cargo audit                                            PASS (two documented transitive advisories)
cargo build --release --locked                         PASS
./target/release/bzz --version                         bzz 0.5.0
```

The opt-in real-relay journey then passed against a detached disposable
worktree at the exact pinned Buzz revision
`ede26863345a518ec46edd6d7692e0281883491b`. The harness created and removed
its Docker/tmux resources. No dependency pin or protocol behavior changed in
this work. `cargo audit` reports only the existing documented transitive
`instant` unmaintained and `lru` panic-safety advisories; pins were not changed
to suppress them.

## Release-binary terminal acceptance

The `bzz 0.5.0` release binary was exercised with the bzz-owned automated
Herdr runner:

```sh
BZZ_BIN="$PWD/target/release/bzz" \
BZZ_HERDR_PANE=<disposable-shell-pane> \
./scripts/test-tui-herdr.sh
```

Both non-secret, isolated-profile scenarios passed:

- `startup-help-quit`: startup, generated help, normal quit confirmation, and
  terminal restoration;
- `custom-keymap-inbox`: typed two-key custom binding, Inbox help route,
  back behavior, quit confirmation, and terminal restoration.

The runner created owner-only temporary profile roots and removed them on exit.
It did not create an identity, contact a relay, or access real community data.

## Visual/model coverage

The deterministic TestBackend tests cover wide/narrow non-overlap, semantic hit
maps, avatars without profile-image I/O, writing-dock placement and cursor
Unicode bounds, date separators, compact same-author grouping, Inbox list/detail
rendering, all built-in themes, and read-only composer messaging. The specific
rendering and idle-gate measurements are recorded in
[`benchmark-v0.5.md`](benchmark-v0.5.md).

## Owner visual review

The owner approved the wide live-shell appearance and interaction on
2026-08-20. The supplied screen contained private community content and is not
stored in the repository. The committed manual rubric and privacy boundary are
in [`visual-review-v0.5.md`](visual-review-v0.5.md).
