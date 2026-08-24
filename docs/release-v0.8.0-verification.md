# v0.8.0 published-artifact verification

**Released:** 2026-08-24 UTC

**Tag:** `v0.8.0` → `99e23cdeb37bbd66cf2c6c14e95af2c5c9c19b80`

**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.8.0>

## Pre-tag checks

- GitHub [CI run 32681353917](https://github.com/arpagon/bzz/actions/runs/32681353917)
  passed the locked test, formatting, strict Clippy, deny, audit, Linux,
  Windows, Intel macOS, and Apple Silicon macOS jobs for the tagged commit.
- GitHub [pinned Buzz integration run 32681353949](https://github.com/arpagon/bzz/actions/runs/32681353949)
  passed its isolated real-relay suite at the pinned Buzz revision.
- Local locked validation passed formatting, strict Clippy, 240 unit and
  integration tests, deny (documented duplicate warnings only), audit (the two
  allowed transitive warnings only), release build/version/check, and media
  status.
- Native desktop review confirmed explicit bitmap paste and the Linux
  `Ctrl-o` XDG Desktop Portal picker enter the bounded attachment lifecycle
  without automatic sending.
- The final release binary remained responsive during channel switching. A
  15-second steady-state sample measured about 1% process CPU and zero socket
  send backlog; a concurrent 20-second sample observed zero event-row and WAL
  growth.

## Artifact verification

The [release workflow 32681719429](https://github.com/arpagon/bzz/actions/runs/32681719429)
completed successfully and published 18 assets. Every entry in `sha256.sum`
passed `sha256sum -c`:

- aarch64 and x86_64 macOS `.tar.xz` archives;
- aarch64 and x86_64 Linux `.tar.xz` archives;
- the x86_64 Windows `.zip`; and
- the source `.tar.gz` archive.

All five platform archives, the source archive, and `bzz.cdx.xml` passed
`gh attestation verify <file> --repo arpagon/bzz`. The CycloneDX 1.3 SBOM
declares application `bzz` version `0.8.0` and 404 component entries.

The downloaded x86_64 Linux archive was unpacked into an isolated temporary
root and reported `bzz 0.8.0`. Its `bzz check` accepted a fresh XDG root. That
same downloaded binary completed native-keychain and encrypted-file
create/verify/remove smokes. The encrypted key had mode `0600`; all temporary
identity roots and passphrase material were removed after verification.
