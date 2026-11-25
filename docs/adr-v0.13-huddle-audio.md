# ADR: v0.13 huddle audio

**Status:** Accepted

**Date:** 2026-11-07

## Context

Buzz huddles are live audio rooms attached to a channel. Lifecycle is
relay-signed:

- 48100 started;
- 48101 participant joined;
- 48102 participant left;
- 48103 ended;
- 48104 liveness;
- 48106 guidelines;
- 24810 reactions.

Audio flows over the relay's huddle WebSocket using the shared
`buzz-audio-client` crate: the v2 handshake, Opus encoding and a NetEQ jitter
buffer. The crate deliberately leaves identity, room admission, devices and UI
to the host. It neither loads keys nor starts background tasks.

Before v0.13, bzz never touched a microphone, speaker or native audio library.

## Decision

1. **Build feature.** Audio lives behind the `huddles-audio` Cargo feature.
   - Release artifacts enable it.
   - Builds without it show huddles view-only and say so.
   - Lifecycle, rosters, guidelines and reactions work in both builds.
2. **Dependencies.**
   - `buzz-audio-client` at the pinned Buzz revision.
   - `cpal` for device I/O: ALSA/PipeWire on Linux, CoreAudio on macOS, WASAPI
     on Windows. (Linux replaced on 2026-11-25; see the section at the end.)
   - `opus` through `buzz-audio-client`. Its libopus link is accepted under
     the existing `cargo deny` license policy (BSD-3-Clause).
3. **Defaults.**
   - Joining is an explicit action with a confirmation.
   - The microphone starts muted, and push-to-talk is available.
   - Starting a huddle also asks for confirmation (block/buzz#9155 parity).
4. **Devices.**
   - Input and output devices are chosen in a picker. The choice is stored as
     the device name in `config.toml` and is not secret.
   - Permission failures, such as macOS microphone permission for the
     terminal app, produce a fixed actionable message.
5. **Lifecycle.**
   - Locking bzz, quitting, losing the relay session, or a relay-signed
     48102/48103 for the viewer stops capture and playback and releases both
     devices.
   - No audio runs while bzz is locked.
6. **Privacy.** Audio frames, levels, transcripts and device names never enter
   logs, diagnostics, telemetry, support reports or test snapshots. Huddle
   diagnostics are fixed enums only: joined, left, permission denied, device
   lost, transport failed.

## Security consequences

### Positive

- The terminal client reaches parity with every other Buzz surface for live
  conversation.
- Mute-by-default and explicit joining keep accidental capture unlikely.

### Residual risks

- A native audio stack and a C codec enlarge the attack surface. Remote Opus
  payloads are decoded by libopus, the same exposure every Buzz client
  carries.
- Terminal apps vary in how operating systems attribute microphone
  permission. On macOS the permission belongs to the terminal emulator, not
  bzz.

## Rejected alternatives

- **Shell out to an external audio tool.** That is unbounded process
  execution, and device lifetime can no longer be tied to bzz's lock state.
- **View-only huddles.** That leaves the main huddle use case unsupported, and
  the plan requires full participation.

## 2026-11-25: Linux devices through the sound server's protocol (v0.17.0)

**Context.** On Linux `cpal` links ALSA, so the binary named
`libasound.so.2`. The system loads a named library before the program's
first instruction: on a machine without it (a clean Debian 13 or Ubuntu
22.04 container, a server, WSL without the package) v0.16.0 could not start
at all, not even `bzz --version`, although only a huddle uses sound (plan
v0.17.0, D3, and the study `r6-audio-start` behind it).

**Decision.**

- On Linux the huddle devices speak the PulseAudio protocol in pure Rust
  (the crate `pulseaudio`, MIT, no C and no `unsafe` in bzz). PulseAudio
  answers it, and so does PipeWire through pipewire-pulse, which current
  desktops run. macOS and Windows keep `cpal` unchanged.
- The Linux binary names no sound library (`readelf -d`: libgcc_s, libm,
  libc). CI fails a build that names one (`scripts/check-linux-linkage.sh`).
- The socket is looked for as libpulse does: `PULSE_SERVER` alone when it is
  set (its local entries; bzz speaks to no network server), else
  `PULSE_RUNTIME_PATH/native`, `XDG_RUNTIME_DIR/pulse/native` or
  `/run/user/<uid>/pulse/native`, then the system socket. bzz does this
  itself: the crate's own lookup appends `pulse/native` to
  `PULSE_RUNTIME_PATH` and gives up at the first missing candidate.
- Streams are 48 kHz mono f32 and the server converts. Playback asks the
  server to hold 10 ms and to request 5 ms at a time; capture hands over
  5 ms fragments.
- Devices are chosen by the server's name or by the description the picker
  shows. A name saved by v0.16.0 is ALSA's: `default`, `pipewire`, `pulse`
  and `sysdefault` lead to the server's default; `hw:`, `plughw:`, `front:`
  and the like with a card and device lead to the source or sink on that
  card when the server says which card it is. A saved name that matches
  nothing opens the default device, and the join says once in a run which
  name was not found (in the status line only; it is never logged).
- With no sound server, joining answers in one line: "huddle audio on
  Linux needs PipeWire or PulseAudio running in your session". bzz keeps
  working.

**What it loses.** A Linux machine with ALSA and no PipeWire or PulseAudio
in the session had huddle audio with v0.16.0 and has none now; it is told
so. A network sound server named in `PULSE_SERVER` (`tcp:`) is not used.

**Test harnesses.** The TUI harnesses set `PULSE_SERVER` for every bzz they
start, to a PulseAudio with a null sink in a disposable container, or to
nothing: on Linux a harness huddle can no longer reach the machine's own
microphone through the session's server. The loopback test (T7) names its
null sink on a PipeWire session and uses a private PulseAudio otherwise; in
containers it runs on PipeWire (Debian and EL9) and PulseAudio
(`scripts/test-audio-containers.sh`).
