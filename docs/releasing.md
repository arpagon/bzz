# Releasing

1. Confirm CI and the pinned real-relay workflow pass.
2. Run `cargo fmt --check`, Clippy, all tests, `cargo deny check`, and
   `cargo audit` against the committed lockfile.
3. Update `docs/protocol-compatibility.md` only through a dedicated Buzz SHA
   compatibility change.
4. Tag `vMAJOR.MINOR.PATCH`. GitHub Actions builds Linux, macOS, and Windows
   archives, SHA-256 checksums, a CycloneDX SBOM, and keyless build-provenance
   attestations.
5. On clean VMs, verify checksums with `sha256sum -c`, verify each archive and
   SBOM with `gh attestation verify <file> --repo arpagon/bzz`, inspect the
   `*.cdx.xml` SBOM, unpack/install the native artifact, and run `bzz --version`.
   Smoke-test both the OS credential backend and encrypted fallback before
   publishing package-manager manifests.

Cache schema changes are called out in release notes. Schema v5 adds the
rebuildable, community/identity-scoped Inbox conversations projection while
schema v4 added bounded local `drafts.mentions_json` metadata. Release
validation must open upgraded v2/v3/v4 fixtures on Linux, macOS, and Windows,
pass FTS and Inbox-projection integrity/rebuild checks, verify owner-only 30622
isolation and Inbox identity/DM fences, and confirm malformed stored mention
metadata degrades to ordinary draft text before publishing. Downgrades restore
the pre-migration backup rather than running reverse SQL.

For v0.9, verify the release binary's diagnostics CLI against disposable
sentinels, owner-only journal/report permissions, fixed rotation, exact delivery
labels, and an unchanged read-only database. A clean and upgraded profile must
make zero OTLP requests by default. Run fake receiver redirect/status/timeout/
queue gates, then one scoped-token `telemetry.test` canary; record operator-side
schema/identity evidence without secrets and revoke/forget the canary before
release. Missing, revoked, offline, and rate-limited telemetry must leave local
bzz usable.

For v0.4 and later interaction changes, also run the deterministic functional
TUI harness in ordinary CI and record a controlled release-binary Herdr
acceptance run (or an explicit release exception). See
[`release-v0.4.0.md`](release-v0.4.0.md).
