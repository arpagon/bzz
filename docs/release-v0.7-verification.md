# v0.7.0 published-artifact verification

**Released:** 2026-08-21 UTC  
**Tag:** `v0.7.0` → `dbfb784ea89dadae86f30f47048179b38a103bbe`  
**Release:** <https://github.com/arpagon/bzz/releases/tag/v0.7.0>

## Pre-tag checks

- GitHub [CI run 32440391068](https://github.com/arpagon/bzz/actions/runs/32440391068)
  passed: Linux/macOS/Windows release builds, target-OS smoke tests, identity,
  backup, config, Inbox, DM, search, migration, and keymap tests.
- GitHub [pinned Buzz integration run 32440391202](https://github.com/arpagon/bzz/actions/runs/32440391202)
  passed its isolated real-relay suite at the pinned Buzz revision.
- Local locked validation passed: fmt, strict Clippy, 108 tests, deny (only
  documented duplicate warnings), audit (only two allowed transitive
  advisories), and release `bzz --version`.

## Artifact verification

The [release workflow 32440883686](https://github.com/arpagon/bzz/actions/runs/32440883686)
completed successfully after a single GitHub Actions artifact-service timeout
on the Intel macOS upload was retried. All five platform archives plus the
source archive passed `sha256sum -c sha256.sum`:

- aarch64 and x86_64 macOS `.tar.xz`;
- aarch64 and x86_64 Linux `.tar.xz`;
- x86_64 Windows `.zip`;
- source `.tar.gz`.

Each archive, the source archive, and `bzz.cdx.xml` passed
`gh attestation verify <file> --repo arpagon/bzz`. The Windows archive passed
`unzip -t`; every tar archive listed the expected binary, README, and licenses.
The CycloneDX 1.3 SBOM declares application `bzz` version `0.7.0` and 360
components.

The downloaded x86_64 Linux archive was unpacked into a clean temporary root,
installed with owner executable permissions, and reported `bzz 0.7.0`. That
same release binary completed fresh-root native-keychain create/verify/remove
and encrypted-file create/verify/backup/remove smokes. The encrypted key file
was mode `0600`; all temporary roots and passphrase material were removed.

## Post-publication correction

v0.7.0's release artifact checks remain valid. Its avatar implementation,
however, treated a same-relay kind-0 image as an anonymous external URL. On a
relay that requires authenticated media reads, this returns `401` and leaves
the marker. The v0.7.1 candidate corrects that interoperability gap with a
strict same-origin, content-addressed authorization branch; see
[`release-v0.7.1.md`](release-v0.7.1.md).

## Deferred visual gate

The owner explicitly authorized v0.7.0 publication before a new disposable
public-image graphics-terminal session was available. The post-release smoke
scenario remains in [`e2e-manual.md`](e2e-manual.md): exercise a public test
picture through scroll, resize, community/identity switch, and `:media reload`
in Kitty/Sixel/iTerm2, then confirm no fetch with `ui.profile_avatars = "off"`
or in a plain terminal.
