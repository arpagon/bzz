# v0.7.1 published-artifact verification

**Released:** 2026-08-21 UTC

**Tag:** `v0.7.1` → `0215edaf7312fc743af60c340a1ee43841c6359f`

**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.7.1>

## Pre-tag checks

- GitHub [CI run 32495279118](https://github.com/arpagon/bzz/actions/runs/32495279118)
  passed Linux/macOS/Windows release builds, target-OS smoke tests, identity,
  backup, config, Inbox, DM, search, migration, and keymap tests.
- GitHub [pinned Buzz integration run 32493969731](https://github.com/arpagon/bzz/actions/runs/32493969731)
  passed its isolated real-relay suite at the pinned Buzz revision. The later
  tag commit changed only release-status documentation.
- Local locked validation passed: fmt, strict Clippy, 110 unit tests plus the
  integration tests, deny (only documented duplicate warnings), audit (only
  two allowed transitive advisories), and release `bzz --version`.
- Direct Ghostty graphics review confirmed profile photos remain in the left
  message gutter through scrolling, a channel switch/return, `:media reload`,
  and resize. Private review captures were removed.

## Artifact verification

The [release workflow 32495783362](https://github.com/arpagon/bzz/actions/runs/32495783362)
completed successfully. All six archives passed `sha256sum -c sha256.sum`:

- aarch64 and x86_64 macOS `.tar.xz` archives;
- aarch64 and x86_64 Linux `.tar.xz` archives;
- x86_64 Windows `.zip`; and
- the source `.tar.gz` archive.

The Windows archive passed `unzip -t`; each tar archive listed its expected
binary or source tree plus project license/readme material. Each of those six
archives and `bzz.cdx.xml` passed `gh attestation verify <file> --repo
arpagon/bzz`.

The CycloneDX 1.3 SBOM declares application `bzz` version `0.7.1` and 363
component entries.

The downloaded x86_64 Linux archive was unpacked into a clean owner-only root,
installed with owner-executable permissions, and reported `bzz 0.7.1`. That
same release binary completed fresh-root native-keychain create/verify/remove
and encrypted-file create/verify/backup/remove smokes. The encrypted key file,
NIP-49 backup, and passphrase file were mode `0600`; all temporary roots and
passphrase material were removed after verification.
