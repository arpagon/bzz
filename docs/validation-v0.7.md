# v0.7 remote-profile-avatar validation evidence

**Recorded:** 2026-08-21 UTC

All fixture and automated checks use bzz-owned synthetic content. No real
profile photograph, production community content, identity secret, or remote
avatar URL is committed as evidence.

## Release gates

```text
cargo fmt --check                                      PASS
cargo clippy --all-targets --locked -- -D warnings     PASS
cargo test --locked                                    PASS (108 tests; one opt-in real-relay test ignored)
cargo deny check                                       PASS (documented duplicate warnings)
cargo audit                                            PASS (two documented transitive advisories)
cargo build --release --locked                         PASS
./target/release/bzz --version                         bzz 0.7.0
```

`cargo deny` reports only the pre-existing dependency duplicates. `cargo audit`
reports only the allowed transitive `instant` unmaintained and `lru`
panic-safety advisories. The Buzz dependency revision remains exactly
`ede26863345a518ec46edd6d7692e0281883491b`.

An isolated release binary with fresh `BZZ_*_DIR` roots passed `bzz media
status`: the default avatar policy is `Trusted`, the dedicated `avatars`
directory exists, and both it and the cache root were mode `0700` on Unix.

## Avatar coverage

- URL policy tests reject HTTP, IP literals, loopback/local names, credentials,
  fragments, non-443 ports, private/documentation IPv4/IPv6, and private 6to4
  destinations.
- Runtime tests prove that avatar work requires the trusted mode, an unlocked
  network-enabled runtime, a supported graphics protocol, and usable width.
  State keys contain SHA-256 digests rather than raw picture URLs and differ by
  author, picture, and rendered width.
- Existing image decoder coverage applies byte, dimension, and format limits;
  existing timeline measurement coverage applies the exact rendering geometry
  to the new allocated image row path.
- Full application, storage, Inbox, signing, attachment, and terminal-restoration
  tests remain green, demonstrating that the optional presentation path does
  not change those boundaries.

## Release exception and post-tag gates

The owner explicitly directed publication of v0.7.0 before a new disposable
public-image graphics-terminal session could be observed in this environment.
That visual scenario remains documented in [`e2e-manual.md`](e2e-manual.md) and
is the first post-release smoke test: scroll, resize, community/identity switch,
and `:media reload` in Kitty/Sixel/iTerm2; repeat in a text terminal and with
`ui.profile_avatars = "off"`.

Before the tag is announced, the pushed release commit must pass GitHub CI and
the pinned-Buzz integration workflow. After the tag workflow produces artifacts,
verify each checksum, SBOM, provenance attestation, archive installation, and
credential-backend/encrypted-fallback smoke according to
[`releasing.md`](releasing.md).
