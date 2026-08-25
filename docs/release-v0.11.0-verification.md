# v0.11.0 published-artifact verification

**Verified:** 2026-08-25

**Tag:** `v0.11.0` → `985e283e12aec109ec33704177e69f3f7290286c`

**Annotated tag object:** `2f1cce6ce6e724714317c7245c60ad4b7b11c6c0`

**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.11.0>

## Workflow evidence

- Implementation cross-platform CI:
  [`32814185676`](https://github.com/arpagon/bzz/actions/runs/32814185676)
- Pinned Buzz integration:
  [`32814185775`](https://github.com/arpagon/bzz/actions/runs/32814185775)
- Tagged-head cross-platform CI:
  [`32814760395`](https://github.com/arpagon/bzz/actions/runs/32814760395)
- Release workflow:
  [`32815397127`](https://github.com/arpagon/bzz/actions/runs/32815397127)

The tagged head passed the test job and release builds/target-OS suites on Linux,
Windows, Intel macOS, and Apple Silicon macOS. The implementation commit passed
the exact pinned Buzz relay journey with deterministic managed-agent public
records, bot membership, mention, reaction, and threaded reply.

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
the downloaded release assets. GitHub's asset API exposes a digest for all 18
uploaded files.

## Provenance attestations

`gh attestation verify --repo arpagon/bzz --format json` succeeded with SLSA
provenance v1 containing the exact subject for:

- `bzz-aarch64-apple-darwin.tar.xz`
- `bzz-aarch64-unknown-linux-gnu.tar.xz`
- `bzz-x86_64-apple-darwin.tar.xz`
- `bzz-x86_64-pc-windows-msvc.zip`
- `bzz-x86_64-unknown-linux-gnu.tar.xz`
- `source.tar.gz`
- `bzz.cdx.xml`

## SBOM

The CycloneDX XML parsed as schema 1.3. Its metadata identifies application
`bzz` version `0.11.0`, and its components list contains 411 entries.

## Linux artifact smoke

The downloaded x86_64 Linux binary:

- reported `bzz 0.11.0`;
- passed isolated `bzz check`;
- migrated an older `[[local_agents]]` section without interpreting it;
- created SQLite schema version 7 with the `remote_agents` projection;
- returned versioned empty `bzz agents list --json` output declaring
  `runtime_control: "remote"`;
- described the namespace as remote/not controlled by bzz; and
- exposed no agent create/start/stop/restart/exec/log, assistant, or Codex
  runtime surface.

Its isolated private diagnostics journal was available. No agent key, model,
ACP process, tool, memory, observer stream, remote runtime, or real message was
used.

## Credential backends

Using disposable isolated profiles and the downloaded binary:

- native-keychain identity create, list, verify, remove, and
  absence-after-remove passed; and
- encrypted-file identity create, list, passphrase-FD verify, owner-only `0600`
  file check, remove, and absence-after-remove passed.

These are regressions for human identities only. v0.11.0 has no agent credential
backend.

## Visual and privacy evidence

The release binary passed both automated Herdr scenarios. A separate public-only
cache fixture showed the Agents directory in a narrow real terminal with the
verified symbol, owner, policy, eligibility, freshness, unknown presence,
capabilities, shared channel, and remote-runtime disclaimer. Keyboard movement,
cache-only refresh refusal, safe mention refusal while locked, close, normal
exit, and foreground-shell restoration passed. Deterministic TestBackend tests
cover wide/narrow layout, semantic mouse selection, community/lock stale-handle
reset, and command-overlay ownership.

Agent diagnostics contain only counts, durations, and closed outcome enums and
are explicitly ineligible for OTel conversion. No private key, passphrase,
profile fixture, downloaded archive, or temporary validation directory remains
in tracked files or release assets.
