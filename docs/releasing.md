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

Cache schema changes are called out in release notes. Downgrades restore the
pre-migration backup rather than running reverse SQL.
