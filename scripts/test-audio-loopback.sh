#!/usr/bin/env bash
# T7: huddle audio through a virtual audio device (tests/huddle_device_loopback_test.rs).
#
# On Linux bzz speaks the PulseAudio protocol (PulseAudio, or PipeWire's
# pipewire-pulse); no ALSA is involved.
# Linux, PipeWire (desktop or container): a dedicated mono null sink on the
#   session's server, named to the test as its output and its monitor as its
#   input (BZZ_AUDIO_OUTPUT / BZZ_AUDIO_INPUT), so the choice by name is what
#   is tested. The user's default devices and microphone are never touched.
# Linux, PulseAudio (CI runners): a private server (its own runtime path) with
#   a null sink as default sink and its monitor as default source: the test
#   opens the *default* devices, as bzz does with no device configured.
# macOS: BlackHole 2ch (`brew install blackhole-2ch`) for input and output.
# Windows: the system default devices, when the machine has any.
#
# BZZ_T7_BINARY: run that already built test binary instead of cargo (the
# container runs of scripts/test-audio-containers.sh have no toolchain).
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/bzz-t7.XXXXXXXX")
module=
cleanup() {
  if [[ -n $module ]]; then pactl unload-module "$module" >/dev/null 2>&1 || true; fi
  if [[ -n ${PULSE_RUNTIME_PATH:-} && ${PULSE_RUNTIME_PATH:-} == "$work/pulse-runtime" ]]; then
    pulseaudio --kill >/dev/null 2>&1 || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

# The test process's streams on the server: the client carries the process
# id (application.process.id), with PulseAudio and with pipewire-pulse alike.
streams_cmd='{ pactl list sink-inputs; pactl list source-outputs; } | grep -c "application.process.id = \"$BZZ_AUDIO_PID\"" || true'

backend=${BZZ_AUDIO_BACKEND:-}
if [[ -z $backend ]]; then
  case $(uname -s) in
    Darwin) backend=blackhole ;;
    MINGW*|MSYS*|CYGWIN*) backend=default ;;
    *) if pw-cli info 0 >/dev/null 2>&1; then backend=pipewire; else backend=pulseaudio; fi ;;
  esac
fi
echo "T7 backend: $backend"

case $backend in
  pipewire)
    pactl info >/dev/null 2>&1 || { echo "PipeWire without pipewire-pulse: bzz has no server to speak to" >&2; exit 1; }
    sink=bzz_t7_$$
    module=$(pactl load-module module-null-sink "sink_name=$sink" rate=48000 channels=1 "sink_properties=device.description=bzz-T7")
    # Named, never the defaults: refuse to run if the server does not list
    # them (bzz would fall back to the default microphone).
    for _ in $(seq 1 50); do pactl list short sinks | grep -q "[[:space:]]$sink[[:space:]]" && break; sleep 0.1; done
    pactl list short sinks | grep -q "[[:space:]]$sink[[:space:]]" || { echo "null sink $sink did not appear" >&2; exit 1; }
    pactl list short sources | grep -q "[[:space:]]$sink.monitor[[:space:]]" || { echo "no monitor of $sink" >&2; exit 1; }
    export BZZ_AUDIO_OUTPUT=$sink BZZ_AUDIO_INPUT=$sink.monitor
    export BZZ_AUDIO_STREAMS_CMD=$streams_cmd
    ;;
  pulseaudio)
    # Container jobs may run as root with an Actions-owned HOME. Give this
    # disposable server private runtime/state paths instead of inheriting
    # that home (or a desktop's existing PulseAudio instance).
    export PULSE_RUNTIME_PATH="$work/pulse-runtime"
    export PULSE_STATE_PATH="$work/pulse-state"
    export PULSE_COOKIE="$work/pulse-cookie"
    mkdir -m 700 "$PULSE_RUNTIME_PATH" "$PULSE_STATE_PATH"
    pulseaudio -D --exit-idle-time=-1
    for _ in $(seq 1 50); do pactl info >/dev/null 2>&1 && break; sleep 0.1; done
    pactl info >/dev/null 2>&1 || { echo "the private PulseAudio did not answer in 5 s" >&2; exit 1; }
    module=$(pactl load-module module-null-sink sink_name=bzz_t7 rate=48000 channels=1)
    pactl set-default-sink bzz_t7
    pactl set-default-source bzz_t7.monitor
    export BZZ_AUDIO_STREAMS_CMD=$streams_cmd
    ;;
  blackhole)
    export BZZ_AUDIO_INPUT="BlackHole 2ch" BZZ_AUDIO_OUTPUT="BlackHole 2ch"
    ;;
  default) ;;
  *)
    echo "unknown BZZ_AUDIO_BACKEND $backend" >&2
    exit 2
    ;;
esac

if [[ -n ${BZZ_T7_BINARY:-} ]]; then
  "$BZZ_T7_BINARY" --ignored --nocapture
else
  cd "$repo_root"
  # The `pulseaudio` crate optimised even in this debug build. Unoptimised,
  # its reader zero-fills a 1 MiB buffer one byte at a time before every
  # read of the socket (4 ms here, more on a CI runner) and writes playback
  # only once a read finds nothing waiting; with capture arriving every few
  # milliseconds that moment never comes, playback starves and T7 took 12
  # minutes or never ended in CI (docs/validation-v0.17.0/ci.md). Release
  # builds, which people run, are not affected.
  cargo test --locked --config 'profile.dev.package.pulseaudio.opt-level=3' \
    --test huddle_device_loopback_test -- --ignored --nocapture
fi
