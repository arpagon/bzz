# v0.10.0 published-artifact verification

**Verified:** 2026-08-24

**Tag:** `v0.10.0` → `68e0c4b76e32a5d17d48a2b468904e512c34cdd4`

**Annotated tag object:** `968a613ed24d5bf286ed9cb42f85fabd12ec1ab1`

**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.10.0>

## Workflow evidence

- Cross-platform CI:
  [`32781986061`](https://github.com/arpagon/bzz/actions/runs/32781986061)
- Pinned Buzz integration:
  [`32781985957`](https://github.com/arpagon/bzz/actions/runs/32781985957)
- Release workflow:
  [`32782721985`](https://github.com/arpagon/bzz/actions/runs/32782721985)

CI passed the test job plus release builds and target-OS suites on Linux,
Windows, Intel macOS, and Apple Silicon macOS. The pinned Buzz relay journey
also passed at the exact tagged commit.

## Assets and checksums

The release published 18 assets:

- five platform archives and their individual SHA-256 files;
- `source.tar.gz` and its SHA-256 file;
- `sha256.sum` covering all six archives;
- shell and PowerShell installers;
- the Homebrew formula;
- `dist-manifest.json`; and
- `bzz.cdx.xml`.

Every entry in `sha256.sum` and every individual `*.sha256` file verified against
the downloaded release assets. GitHub's asset API digests were present for all
18 uploaded files.

## Provenance attestations

`gh attestation verify --repo arpagon/bzz --format json` succeeded for each of
the following seven subjects and returned SLSA provenance v1 containing the
exact subject name:

- `bzz-aarch64-apple-darwin.tar.xz`
- `bzz-aarch64-unknown-linux-gnu.tar.xz`
- `bzz-x86_64-apple-darwin.tar.xz`
- `bzz-x86_64-pc-windows-msvc.zip`
- `bzz-x86_64-unknown-linux-gnu.tar.xz`
- `source.tar.gz`
- `bzz.cdx.xml`

## SBOM

The CycloneDX XML parsed successfully as schema 1.3. Its metadata identifies an
application component named `bzz` at version `0.10.0`, and its components list
contains 411 entries.

## Linux artifact smoke

The downloaded x86_64 Linux archive contained the binary, README, and both
license files. The binary:

- reported `bzz 0.10.0`;
- exposed no `agent`, `assistant`, or `codex` command in CLI help;
- accepted an isolated legacy `[[local_agents]]` configuration with `bzz check`;
- atomically rewrote that configuration without the retired section; and
- reported an available empty private diagnostics journal with zero outbox
  counts.

## Credential backends

Using only isolated disposable profile directories and the downloaded binary:

- native-keychain identity create, list, verify, remove, and absence-after-remove
  passed; and
- encrypted-file identity create, list, passphrase-FD verify, mode-`0600` check,
  remove, and absence-after-remove passed.

No passphrase, private key, identity file, downloaded archive, or temporary
profile remains in the repository. No real message, channel, attachment,
clipboard value, or production identity was used.
