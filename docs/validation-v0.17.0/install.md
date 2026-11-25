# v0.17.0 — task `install`: A6, A7, F6

Linux x86_64, 2026-11-24/25, on branch `t3/install-onboarding` cut from
`v0.17.0` at `543dbd1`. Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md).
`<scratch>` is a folder outside the repository; its files are local. Every
container is a stock image from Docker Hub (`debian:stable-slim`,
`ubuntu:22.04`, `ubuntu:24.04`, `ubuntu:20.04`, `almalinux:9-minimal`,
`fedora:latest`, `alpine:3`) as pulled on that day. No bzz binary ran on
this machine; every bzz that ran is the published v0.16.0, inside a
container.

## A6 — glibc 2.34 in the generated installer

**Claim.** With `[dist.min-glibc-version] "*" = "2.34"` in
`dist-workspace.toml`, the shell installer cargo-dist generates accepts
glibc 2.34, so AlmaLinux 9 installs; the release workflow is unchanged.

**Rung.** The generated installer itself, run in a clean AlmaLinux 9
container against the published v0.16.0 archives (the version in
`Cargo.toml` is still 0.16.0, so the generated installer downloads them).

```sh
dist generate --check                        # cargo-dist 0.30.2: prints nothing, exit 0
dist build --artifacts=global --output-format=json
grep -o 'check_glibc "2" "3[0-9]"' target/distrib/bzz-installer.sh | sort -u
docker run --rm -v <scratch>/distrib-2.34:/g:ro almalinux:9-minimal sh -c '
  microdnf install -y tar xz
  sh <the published v0.16.0 bzz-installer.sh>     # for contrast
  sh /g/bzz-installer.sh; ~/.cargo/bin/bzz --version
  microdnf install -y alsa-lib; ~/.cargo/bin/bzz --version'
```

**Seen.** `dist generate --check`: no output, exit 0 (the workflow does not
change). The generated installer has `check_glibc "2" "34"` for both Linux
targets; the published v0.16.0 one has `"2" "35"`. In AlmaLinux 9
(`glibc 2.34`):

```
=== the published v0.16.0 installer (min glibc 2.35)
System glibc version (`2.34') is too old; checking alternatives
ERROR: no compatible downloads were found for your platform x86_64-unknown-linux-gnu
[exit 1]
=== the installer generated with [dist.min-glibc-version] 2.34
downloading bzz 0.16.0 x86_64-unknown-linux-gnu
no checksums to verify
installing to /root/.cargo/bin
everything's installed!
[exit 0]
/root/.cargo/bin/bzz: error while loading shared libraries: libasound.so.2 ...
--- after microdnf install alsa-lib
bzz 0.16.0
```

"no checksums to verify": a global-only build has no local archives to
hash; the release workflow's installer carries them. v0.16.0 needs ALSA to
start (E1 removes that); the point here is that the installer no longer
refuses glibc 2.34.

## A6 — `getbzz.dev/install.sh`

**Claim.** `site/public/install.sh` gives each machine the answer it should
in one line: glibc too old and musl are refused with what to do; a missing
`tar` or `xz` (which the archive needs) is refused with the one command
that installs it; the sound library is reported in the wording that the
variable `BZZ_SOUND` picks. It is POSIX `sh` and `shellcheck` clean.

**Rung.** The script in the seven clean images with `BZZ_CHECK_ONLY=1`,
twice: as the image comes (no network) and with `tar` and `xz` added (what a
desktop has); each in both wordings (`BZZ_SOUND=server`, the default, and
`alsa`, made with `sed` on a copy).

```sh
docker run --rm koalaman/shellcheck:stable --shell=sh --severity=style install.sh   # 0.11.0
docker run --rm --network none -v <dir>:/i:ro -e BZZ_CHECK_ONLY=1 <image> sh /i/install-<mode>.sh
dash -n, bash --posix -n, busybox sh -n                                            # parse
```

**Seen.** `shellcheck`: nothing, exit 0. All three shells parse it. The
answers (full output in `<scratch>/check-images.txt`):

| Image | As it comes | With tar and xz, `server` | With tar and xz, `alsa` |
|---|---|---|---|
| debian:stable-slim | `no xz … sudo apt install xz-utils` (1) | note: no PipeWire or PulseAudio found (0) | `… sudo apt install libasound2t64` (0) |
| ubuntu:22.04 | `no xz … sudo apt install xz-utils` (1) | note (0) | `… sudo apt install libasound2` (0) |
| ubuntu:24.04 | `no xz … sudo apt install xz-utils` (1) | note (0) | `… sudo apt install libasound2t64` (0) |
| ubuntu:20.04 | `this system has glibc 2.31 and bzz needs 2.34 or newer: use Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9 or newer, or a container of one.` (1) | same (1) | same (1) |
| almalinux:9-minimal | `no tar and xz … sudo dnf install tar xz` (1) | note (0) | `… sudo dnf install alsa-lib` (0) |
| fedora:latest | note (0) | note (0) | `… sudo dnf install alsa-lib` (0) |
| alpine:3 | `there is no download for musl systems such as Alpine yet: run bzz on a glibc system (glibc 2.34 or newer) or in a container such as debian:stable-slim.` (1) | same (1) | same (1) |

In the `alsa` wording, an image without `xz` gets one command for both:
`sudo apt install xz-utils libasound2t64, then run this installer again
(libasound2t64 is the sound library bzz needs to start)`.

The `server` note is skipped when a sound server is there (fedora, with
`BZZ_CHECK_ONLY=1`; `<scratch>/sound-server-detection.txt`): a listening
socket at `$XDG_RUNTIME_DIR/pulse/native`, `PULSE_SERVER` set (as WSLg
does), or `pipewire` on `PATH` each give only `checks done`. With a fake
`uname` that answers `Darwin`, the script goes straight to the checks'
end (exit 0): macOS takes the release's installer as before.

**A full install** of the published v0.16.0 through the script, in
`ubuntu:24.04` with `curl` and `xz-utils`, as a new user, the file piped to
`sh` as `curl … | sh` does (`<scratch>/full-install-ubuntu24-final.txt`):

```
downloading bzz 0.16.0 x86_64-unknown-linux-gnu
installing to /home/newcomer/.cargo/bin
everything's installed!
...
Note: huddle audio needs PipeWire or PulseAudio, and neither was found here; everything else in bzz works without them.

Try it now, offline, with nothing to set up:   bzz demo
Then join your team's community:               bzz
(bzz is in /home/newcomer/.cargo/bin, not on this shell's PATH yet: open a new terminal, or run /home/newcomer/.cargo/bin/bzz demo)
[installer exit 0]
bzz: error while loading shared libraries: libasound.so.2 ...   [exit 127]
--- after apt install libasound2t64
bzz 0.16.0
```

The `server` wording is for v0.17.0 and is wrong for v0.16.0, which still
needs ALSA: this run shows exactly that. Until v0.17.0 is published (and if
E1 does not ship), the site must serve `BZZ_SOUND=alsa`
(`site/README.md`, "The installer"). Also seen in the same container:
`BZZ_VERSION=0.16.0 sh -s -- --no-modify-path` installs that release and
passes the argument on (no `~/.cargo/env` written); `BZZ_VERSION=9.9.9`
stops with `bzz: could not download …/v9.9.9/bzz-installer.sh`, exit 1.

**Not verified.** The script on a real macOS or Windows shell (Git Bash);
on arm64 Linux; the full chain through the script on AlmaLinux 9 (the
latest release's installer still asks for 2.35 until v0.17.0 is
published); Cloudflare Pages serving `install.sh` with `_headers`
(nothing was deployed).

## A7 — README and site

**Claim.** The README's hero shows the v0.16 film and its image, a "Try it
in 60 seconds" block, a requirements line and a compatibility line, and the
"not affiliated with Block" sentence near the top; the site has the same
block and requirements and a current social card description.

**Rung.** Links resolved from the file; the page drawn by headless Chrome.

```sh
python3 <check every relative link and #anchor in README.md, and that each
         getbzz.dev/assets/… link has its file in site/public/assets/>
curl -sI https://getbzz.dev/assets/promo-v016.mp4         # HTTP/2 200, video/mp4
google-chrome --headless=new --window-size=1280,900 --screenshot … http://127.0.0.1:<port>/
```

**Seen.** 20 relative links, none missing; every `#anchor` matches a
heading; `promo-v016.mp4` exists in `site/public/assets/` and is served.
The compatibility line names what `docs/protocol-compatibility.md` says:
relay `block/buzz` `bf3bdd0e…`, pairing proven with Buzz desktop 1.0.1.
Screenshots at 1280 and 390 wide (`<scratch>/shots/site-{1280,390}-top.png`,
`site-{1280,390}-install.png`): the hero line "then `bzz demo`: …" under the
install command, and the install section "Try it in 60 seconds." with the
requirements, the two-line block and the installer's note; nothing clipped
at 390. `styles.css` gained one rule (`.hero p.then`), so its `?v=` went
from 7 to 8.

**Not verified.** The block pasted into a clean Debian reaching the demo's
first screen: `bzz demo` is built by another task and is not in this
branch.

## F6 — CI on `main`

**Claim.** The two jobs that kept `main` red are fixed in the workflow.

**Rung.** The logs of the runs on `main` (read with `gh run list` and
`gh run view`; nothing started or cancelled), and `actionlint` on the
changed workflow. The proof proper is CI itself: it is not run from here.

**Seen in the logs.**

- **`agent-host (macos-15)`, cancelled.** Runs 45462401762, 45427321605
  and 45391983168 (2026-11-24) failed only through cancelled `macos-15`
  jobs: `agent-host`, and in two of them `upgrade (macos-15, 0150)`,
  `audio (macos-15, false)` and `build (macos-15, …)` too. Each has zero
  steps and ends 15 minutes after it was queued with the annotation `The
  job was not acquired by Runner of type hosted even after multiple
  attempts`, and three times `Due to capacity constraints, jobs targeting
  macOS arm64 runners may experience longer queue times`. The label exists;
  hosted Apple Silicon capacity is what is missing. `macos-15-intel` jobs
  of the same runs (`build (macos-15-intel, …)`) started and ran.
  **Change:** `agent-host` (Node only) and `upgrade` (a profile opened by
  this build) run on `macos-15-intel`. That halves the Apple Silicon jobs
  of a run (left: `build` for `aarch64-apple-darwin`, and `audio`).
- **`audio (windows-latest, true)`, failed.** It fails in every run since
  at least 2026-11-15: the loopback test panics with `the virtual devices
  open: Unsupported("microphone unavailable: …")`; hosted Windows has no
  audio endpoint, as the workflow's comment says. The job already had
  `continue-on-error: ${{ matrix.experimental }}`, and that works for the
  run: run 45322414306 (2026-11-24) concluded `success` with this job
  failed. But the job and the pull request's check still show a red ✗.
  **Change:** the same `continue-on-error` on the test step, so an
  experimental row passes its job and records the failure in the step.
  The step's command is untouched (agent `sound` owns it).

`actionlint` (rhysd/actionlint, latest): no findings, before and after.
`main` has no branch protection and no rulesets (`gh api`), so no required
check carries the old job names.

**Not verified.** Two consecutive green runs on `main`: only CI can show
them, after the principal pushes. `build (macos-15, aarch64-apple-darwin)`
and `audio (macos-15, false)` still ask for Apple Silicon and can still be
cancelled the same way; neither is moved here (the first must build and
run on that processor; the second belongs to agent `sound`).
