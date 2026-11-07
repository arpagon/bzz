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
     on Windows.
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
