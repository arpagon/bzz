# Linux build environment

Release and CI jobs use `ghcr.io/arpagon/bzz-build` **by multi-architecture
SHA-256 digest**, not by tag. Both amd64 and arm64 are Ubuntu 22.04 (glibc
2.35). Linux release builds run natively on the corresponding GitHub runner.

## Why

Repeated v0.12.1 release and v0.13 CI failures occurred before compilation:
`apt-get` could not resolve `azure.archive.ubuntu.com`, `archive.ubuntu.com`
and `ports.ubuntu.com`. The logs establish DNS failures in those jobs, not
which provider caused them. Retrying whole releases did not fix them.

System dependencies are now installed **once, when building this image**.
Product build/test workflows never run apt. A mirror outage can delay an
image refresh, but cannot affect a build using the pinned, published image.
Rust/Cargo, source repositories, GitHub Actions and GHCR still need their
normal network access; this is not a fully hermetic build.

Current bzz also enables `keyring/vendored`: libdbus is compiled from its
locked crate source without pkg-config's `dbus-1` dependency. This does not
change the Secret Service API or the identity store. The image keeps DBus
headers for the *unmodified* v0.12.1 upgrade fixture. ALSA is dynamically
linked; release users still need the system's `libasound.so.2`.

The image contains compilers, CMake, pkg-config, ALSA, PulseAudio and its ALSA
plugin, DBus headers, Docker/Compose clients, PostgreSQL client, tmux and
archive/cache tools. `/usr/local/share/bzz-build-packages.txt` records exact
installed package versions. It contains no credentials or product keys.

## Refresh and promote

1. Review/update the Ubuntu digest and packages in `Dockerfile`.
2. On a trusted machine with Docker buildx and arm64 support, build an OCI
   archive (on x86, QEMU/binfmt is needed for the arm64 package installation):

   ```sh
   docker buildx build --platform linux/amd64,linux/arm64 \
     -f ci/linux-build/Dockerfile ci/linux-build \
     --output type=oci,dest=/tmp/bzz-build.oci.tar \
     --metadata-file /tmp/bzz-build-metadata.json
   sha256sum /tmp/bzz-build.oci.tar
   ```

3. Smoke-test **both** architectures and archive/cache tools. With a fetched
   Rust/Cargo cache, run bzz tests with `--network none --offline` and
   `DBUS_1_NO_PKG_CONFIG=1`; also run the virtual-audio loopback test. Do not
   include local caches or credentials in the published image.
4. Upload the archive to a draft release named
   `ci-linux-build-source-YYYY-MM-DD[-N]`. This is an internal image source,
   not a product release. Record its tag, archive SHA-256 and a fresh image
   tag in `publish.json`. Commit the Dockerfile and manifest.
5. `publish-build-image.yml` downloads the archive, checks its hash, and
   copies **all original OCI manifests preserving their digests** to GHCR.
   This needs `packages: write`; reading the draft also needs
   `contents: write`. Ordinary build/test workflows remain read-only. A
   maintainer's local token does not need package-write access. Promotion
   never runs apt and cannot substitute another image without failing the
   recorded hash check.
6. Confirm anonymous pulls work. Set GHCR visibility to public if necessary
   before using it in public CI. Inspect the published index:

   ```sh
   docker buildx imagetools inspect ghcr.io/arpagon/bzz-build:<new-tag>
   ```

7. Update the index digest in `dist-workspace.toml`, `.github/workflows/ci.yml`
   and `.github/workflows/integration.yml`. Run:

   ```sh
   cargo test --locked --test build_environment_test
   dist plan --output-format=json
   ```

   The plan must select the pinned image for both Linux targets and have no
   `packages_install` command. The regression test prevents apt from being
   reintroduced in product workflows. Keep `.github/workflows/release.yml`'s
   dependency **verification** step rather than installing anything there.
8. Require Linux build/test, upgrade, virtual audio, pinned relay, and both
   actual release builds to pass. Changing a pin without these observations
   is not proof. To refresh older releases, backport only packaging changes
   onto their validated commit; never move a tag with a published release.

The relay integration job shares the runner's Docker socket and host network
so Compose's isolated localhost ports remain reachable. It starts no nested
Docker daemon. The image is not an application sandbox; only trusted CI jobs
receive this access.
