# v0.9.0 published-artifact verification

**Released:** 2026-08-24 UTC

**Tag:** `v0.9.0` → `82f22463fe00089e383818711999edec2fe8c8c2`

**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.9.0>

## Pre-tag checks

- GitHub [CI run 32753141731](https://github.com/arpagon/bzz/actions/runs/32753141731)
  passed locked formatting, strict Clippy, tests, deny, audit, and release builds
  and smoke tests on Linux, Windows, Intel macOS, and Apple Silicon macOS for
  the tagged commit.
- GitHub [pinned Buzz integration run 32689026337](https://github.com/arpagon/bzz/actions/runs/32689026337)
  passed its isolated real-relay suite at the pinned Buzz revision for the
  implementation commit `02697f8`; subsequent pre-tag commits changed only
  release and validation documentation.
- Local locked validation passed formatting, strict Clippy, 269 unit and
  integration tests, deny, audit with only the two documented allowed
  transitive warnings, release build/version/check, isolated diagnostics and
  media smokes, and bounded fake OTLP receiver tests.
- Scoped debug and release `telemetry.test` canaries were accepted by the
  Emilia gateway and the isolated enrollment was forgotten. The release host
  has only write-scoped ingestion access, so the owner explicitly authorized
  release with gateway-stamped row identity and disposable relay-log
  correlation deferred to the operator rather than recorded as completed
  evidence.

## Artifact verification

The [release workflow 32754030482](https://github.com/arpagon/bzz/actions/runs/32754030482)
completed successfully and published 18 assets. Every entry in `sha256.sum`
passed `sha256sum -c`:

- aarch64 and x86_64 macOS `.tar.xz` archives;
- aarch64 and x86_64 Linux `.tar.xz` archives;
- the x86_64 Windows `.zip`; and
- the source `.tar.gz` archive.

All five platform archives, the source archive, and `bzz.cdx.xml` each passed
`gh attestation verify <file> --repo arpagon/bzz` with one verified provenance
attestation. The CycloneDX 1.3 SBOM declares application `bzz` version `0.9.0`
and 411 component entries.

The downloaded x86_64 Linux archive was unpacked into an isolated temporary
root and reported `bzz 0.9.0`. Its `bzz check` accepted a fresh XDG root and
`bzz diagnostics status` reported an available empty private journal without
connecting. That same downloaded binary completed native-keychain and
encrypted-file create/verify/remove smokes. The encrypted key had mode `0600`;
all temporary identity roots and passphrase material were removed after
verification.
