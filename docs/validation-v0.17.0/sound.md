# v0.17.0 validation: sound (E1, decision D3)

Linux huddle devices through the PulseAudio protocol in pure Rust, so that
bzz names no sound library and starts on every Linux machine. Branch
`t3/sound`, cut from `v0.17.0` at `543dbd1`. Linux x86_64, 2026-11-24/25,
on a machine shared with six other builds. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).

`<scratch>` is a folder outside the repository on the machine the work was
done on. Every sound server below ran inside a disposable container with
`--network none`, as user 1000, with no host sound socket and no `/dev/snd`;
this machine's own sound was never played through, recorded from or
reconfigured.

**Builds.** "CI image" means bzz's release image (`bzz-build:smoke`, the
local build of `ci/linux-build/Dockerfile`: Ubuntu 22.04, glibc 2.35), with
the host's Rust 1.98.1 mounted: binaries built on the host need glibc 2.43
and do not run in the containers. The release binary is the `release`
profile as shipped (thin LTO, one codegen unit). The T7 test binary is the
`release` profile with LTO off and 16 codegen units (to rebuild it in about
three minutes while the buffers were tuned); a debug build of T7 was too
slow to measure underruns and its numbers were thrown away
(`<scratch>/tune2-debug-build.txt`).

## The release binary names no sound library

**Claim.** The Linux release binary's `NEEDED` has no sound library, and
CI fails one that does. **Rung:** the artifact itself, and the check run
against a binary that has the defect.

```sh
scripts/check-linux-linkage.sh <scratch>/bin/bzz            # this branch, CI image
scripts/check-linux-linkage.sh <scratch>/v017-r6/bin/bzz-base/bzz   # 543dbd1's code, CI image (study R6)
```

| Binary | `NEEDED` | Exit |
|---|---|---|
| this branch | libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2 | 0 |
| before (same code as `543dbd1`) | **libasound.so.2** libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2 | 1, "bzz must not name a sound library; it names: libasound.so.2" |

The `build` job of `.github/workflows/ci.yml` runs the check on the Linux
release binary. Size: 31,701,352 bytes against 31,366,072, **+335 KB**
(+1.1 %).

`cargo tree --target <t> -i <crate> -e normal`: `pulseaudio` is in the graph
for x86_64 and aarch64 Linux only; `cpal` for aarch64-apple-darwin and
x86_64-pc-windows-msvc only; `alsa-sys` for none of the four.

## bzz starts where there is no sound library

**Claim.** `bzz --version` and `bzz paths` work on clean images with no
sound library. **Rung:** the release binary in clean containers.

```sh
scripts/test-audio-containers.sh start <scratch>/bin/bzz
```

| Image | Sound libraries in it | This branch | Before |
|---|---|---|---|
| `debian:stable-slim` | 0 | `bzz 0.16.0`, paths all under the scratch home | `error while loading shared libraries: libasound.so.2` |
| `ubuntu:22.04` | 0 | the same | the same error |
| `ubuntu:24.04` | 0 | the same | the same error |
| `almalinux:9-minimal` | 0 | the same | the same error |

(The version still reads 0.16.0: the version bump is F8's.)

## T7 on PipeWire and on PulseAudio, and the message with no server

**Claim.** `tests/huddle_device_loopback_test.rs` passes on PipeWire on
Debian, PipeWire on EL9 and PulseAudio, with the streams released on drop;
with no sound server, opening the devices answers at once with the one
line. **Rung:** the real device layer against real sound servers (null
sinks), in containers.

```sh
scripts/test-audio-containers.sh t7 <scratch>/bin/t7     # three passes
```

Three passes, all green (`<scratch>/t7-final.txt`):

| Server | Round trip (first tone frame) | Holes in 3 s of paced playout | Streams before / open / after drop |
|---|---|---|---|
| PipeWire 1.4.2, Debian (named null sink and monitor) | 16, 21, 15 ms | 1, 1, 1 | 0 / 2 / 0 |
| PipeWire 1.4.11, AlmaLinux 9 (named) | 21, 15, 20 ms | 0, 4, 2 | 0 / 2 / 0 |
| PulseAudio, private server as CI runs it (defaults) | 1362, 1008, 1088 ms | 0, 1, 0 | 0 / 2 / 0 |
| none (`debian:stable-slim`) | `Unsupported("huddle audio on Linux needs PipeWire or PulseAudio running in your session")` in 0.01 s | | |

The PulseAudio round trip is the container's null sink, whose clock runs
at about 36,700 samples a second instead of 48,000; study R6 measured the
same 1.35 s for v0.16.0's ALSA path there. It is not a bzz number.

A "hole" is a run of 1 ms or more of near silence inside the captured tone
while 10 ms chunks are fed on a 10 ms clock with `try_send`, from an empty
queue, as the engine plays out: an underrun. T7 prints it and does not
assert it.

## Buffer tuning

**Claim.** The chosen buffers give the lowest round trip that does not
underrun at the engine's pace. **Rung:** T7 in the containers, three runs
per setting; `BZZ_PULSE_TUNE` and `BZZ_PULSE_CUSHION` were a temporary
override for this table only and are not in the code.

Playback as the study had it (silence handed to the server when the engine
was late), paced phase started with a full queue (`<scratch>/tune.txt`):

| target / request / fragment | PipeWire Debian | PipeWire EL9 |
|---|---|---|
| 20 / 10 / 10 ms (the study's) | 37–47 ms, holes 0, 2, 0 | 32–42 ms, holes 0, 1, 0 |
| 10 / 5 / 5 ms (the study's "18 ms tuned") | 17–21 ms, holes 5, 12, 33 | 18–21 ms, holes 24, 26, 7 |

Playback that waits for the engine (the source is pending until the next
chunk; this branch), paced phase from an empty queue (`<scratch>/tune5.txt`):

| target / request / fragment, cushion | PipeWire Debian | PipeWire EL9 | PulseAudio holes |
|---|---|---|---|
| **20 / 10 / 10 ms, 10 ms (chosen)** | **13–16 ms, holes 1, 0, 0** | **15–26 ms, holes 0, 0, 0** | 0, 1, 0 |
| 20 / 10 / 10 ms, none | 15–21 ms, holes 0, 1, 0 | 15–21 ms, holes 1, 1, 0 | 0, 2, 2 |
| 10 / 5 / 5 ms, none | 18–23 ms, holes 9, 6, 2 | 20–24 ms, holes 5, 9, 20 | 1, 2, 2 |
| 30 / 10 / 10 ms, none | 21–23 ms, holes 9, 4, 5 | 22–26 ms, holes 12, 11, 9 | 0, 0, 0 |

So: **13 to 26 ms** round trip through PipeWire, against 37 ms untuned in
the study, without the underruns that 10 / 5 / 5 ms costs. A larger target
(30 ms) underran more on PipeWire, not less; not investigated.

## Unit tests

`cargo test --lib pulse_devices` (9 tests): the socket is looked for where
libpulse looks (`PULSE_RUNTIME_PATH/native` as CI sets it, `XDG_RUNTIME_DIR`,
`/run/user/<uid>`, the system socket); `PULSE_SERVER` is the only place when
set, with lists, machine-id prefixes and network entries skipped; names
match a server name or a label; **names saved by v0.16.0** (`default`,
`pipewire`, `pulse`, `sysdefault` → the default; `hw:CARD=Headset,DEV=0`,
`plughw:1,0`, `sysdefault:CARD=Headset`, `front:CARD=PCH,DEV=0` → the source
on that card, never its monitor; a card that is gone, a device it lacks or
an unknown card id → the default); the notice is told once a run; the
playout waits instead of playing silence and holds its cushion; the byte
framer cuts 960-sample frames from bytes of any size; with nowhere to
connect the one line comes back in under a second.

Not verified: the card mapping against a real card's properties
(`alsa.card`, `api.alsa.pcm.card`): the containers have no card, and this
machine's own server was not to be asked.

## A call between two clients on the pinned relay

**Claim.** Two bzz clients on the pinned relay hold a huddle and the sound
crosses: what one microphone takes, the other plays. A v0.16.0 device name
in the configuration opens the default device and the join says so; with no
sound server, a join answers in one line and bzz keeps working. **Rung:**
the release binary (host build) in tmux, `t17-sound-call`, on the pinned
relay (`scripts/test-relay.sh --tmux`), each client with its own sound
server in a container.

```sh
# A: the viewer; its microphone is a PipeWire session playing 440 Hz.
# B: another person (profile other), a PipeWire session with a null sink.
scripts/test-audio-containers.sh serve <scratch>/a tone       # -> PULSE_SERVER for A
scripts/test-audio-containers.sh serve <scratch>/b pipewire   # -> PULSE_SERVER for B
scripts/test-audio-containers.sh record <scratch>/b 900 &     # what B plays
BZZ_TEST_PULSE_SERVER=unix:<scratch>/a/pulse/native \
BZZ_TEST_PULSE_SERVER_OTHER=unix:<scratch>/b/pulse/native \
BZZ_TMUX_SCENARIOS=t17-sound-call ./scripts/test-relay.sh --tmux
```

(`<scratch>/call.sh` does this and analyses the recording.)

- `t17-sound-call` passed in 13.4 s, 24 steps. The viewer saved
  `hw:CARD=PCH,DEV=0` with `:huddle input`, as v0.16.0 saved ALSA's names;
  the join opened the default microphone and the status line said
  `saved input "hw:CARD=PCH,DEV=0" not found; using the default · joined…`
  (capture `<scratch>/call-captures/t17-sound-call/07.txt`, 160×45).
- **The sound crossed.** B's sink played the tone from 0.1 s after B joined
  until it left: 6.0 s, 588 of the 598 10 ms windows with sound, 10 silent;
  the frequency by zero crossings in the 100 ms windows was 440 Hz in most
  and 260 to 510 Hz in all. Both clients' streams were on their servers
  while the call held and gone after (polled every half second with
  `pactl` in each container).
- Before this, three things were found and fixed in the scenario and the rig
  (runs kept in `<scratch>/call-run-{1..7}.log`): the viewer's "second"
  profile is the viewer again, so its leaving ended the huddle (the call is
  now with another person); after a join the huddle panel is open, so the
  leave is `l`; and PulseAudio's sine source in a container works in fixed
  2 s blocks (`pactl list sources`: "configured 2000000 usec"), which the
  call heard as bursts of 0.2 to 0.6 s every 2 s. The same 2 s fixed latency
  of PulseAudio's null sink there is the likelier cause of T7's 1.3 s round
  trip on PulseAudio than the slow clock study R6 supposed; not checked
  further.
- **No sound server** (`<scratch>/no-server-scenario.toml`, run with
  `BZZ_HERDR_MANIFEST` and `BZZ_TEST_PULSE_SERVER_OTHER` pointing at nothing;
  it is not in the shared manifest because the normal runs give every
  profile a server): the other person's `:huddle join` and `y` showed
  `huddle audio on Linux needs PipeWire or PulseAudio running in your
  session` in the status line at 160×45, Esc went back to the channel,
  `online`, and bzz quit cleanly. Passed in 6.4 s. **At 80×24 the line is
  cut** after "needs PipeWire or" (`<scratch>/call-captures-5/t17-sound-no-server/11.txt`):
  the status line gives the left part to the mode and the huddle.
- **The harness as anyone runs it** (no `BZZ_TEST_PULSE_SERVER`): the tmux
  runner served its own PulseAudio with a null sink in a container
  (`sound server: a container with a null sink (unix:/tmp/bzz-tmux.…/sound/native)`),
  `t9-presence-broker-huddle` (start, join muted, unmute, mute, leave) and
  `t17-sound-call` passed, 2 of 2, and the container was removed. No sound
  or relay container was left after any run.

## Gates

On the host, at `23b2cc7` (before the status-line commit `184ac6e`; rerun
after it: GATES-AFTER):

| Gate | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --locked --all-targets -- -D warnings` | ok |
| the same with `--no-default-features` | ok |
| `cargo build --locked --example scripted_harness` | ok |
| `cargo test --locked --all-targets` | 64 test binaries, 1,585 passed, 0 failed |
| `cargo test --locked --no-default-features --lib` | ok |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo audit` | 5 allowed warnings (unmaintained: chrono, instant, zbus_names, zvariant, zvariant_derive), the same 5 as `543dbd1`'s lock file; none new |

Lock file: three crates added (`pulseaudio` 0.3.2, `enum-primitive-derive`
0.3.0, `futures` 0.3.33, pinned to the `futures-*` already locked). Adding
them made cargo re-pick `windows-sys` 0.61.2 for thirteen crates that
accept a range (`errno`, `rustix`, `tempfile`, `socket2`…); Windows only,
and deny and audit pass.

## Not verified here

- macOS and Windows builds: the targets are not installed in the shared
  toolchain and were not added. The non-Linux code is today's, behind
  `cfg(not(target_os = "linux"))`, with one field added; CI on the pull
  request will tell.
- PipeWire in CI: the CI image has PulseAudio only, and product workflows
  install no packages (`tests/build_environment_test.rs`). The Linux audio
  job runs T7 on PulseAudio; PipeWire is proven here in containers.
- A machine with a real card and a desktop session, WSLg
  (`PULSE_SERVER=unix:/mnt/wslg/PulseServer`, covered by a unit test only),
  and a sound server restarted in the middle of a call.
