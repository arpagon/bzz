#!/usr/bin/env bash
# T7: huddle audio through a virtual audio device (tests/huddle_device_loopback_test.rs).
#
# The test opens the *default* devices, as bzz does with no device configured.
# Linux, PipeWire (desktop): a dedicated null sink plus a virtual source fed
#   by its monitor; a self-contained ALSA config (ALSA_CONFIG_PATH, this
#   process only) points ALSA's `default` at them. The user's default devices and microphone
#   are never touched.
# Linux, PulseAudio (CI runners): a null sink as default sink, its monitor as
#   default source, ALSA `default` through the pulse plugin.
# macOS: BlackHole 2ch (`brew install blackhole-2ch`) for input and output.
# Windows: the system default devices, when the machine has any.
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/bzz-t7.XXXXXXXX")
nodes=()
module=
cleanup() {
  # ${nodes[@]+...}: an empty array under `set -u` on bash 3.2 (macOS).
  for node in ${nodes[@]+"${nodes[@]}"}; do pw-cli destroy "$node" >/dev/null 2>&1 || true; done
  if [[ -n $module ]]; then pactl unload-module "$module" >/dev/null 2>&1 || true; fi
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

pw_node_id() {
  pw-dump 2>/dev/null | python3 -c '
import json, sys
for item in json.load(sys.stdin):
    props = (item.get("info") or {}).get("props") or {}
    if props.get("node.name") == sys.argv[1]:
        print(item["id"])
        break' "$1"
}

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
    sink=bzz-t7-sink-$$
    source=bzz-t7-source-$$
    pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$sink node.description=bzz-T7-sink media.class=Audio/Sink object.linger=true audio.position=[MONO] }" >/dev/null
    pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$source node.description=bzz-T7-source media.class=Audio/Source/Virtual object.linger=true audio.position=[MONO] }" >/dev/null
    for name in "$sink" "$source"; do
      id=
      for _ in $(seq 1 50); do id=$(pw_node_id "$name"); [[ -n $id ]] && break; sleep 0.1; done
      [[ -n $id ]] || { echo "PipeWire node $name did not appear" >&2; exit 1; }
      nodes+=("$id")
    done
    for _ in $(seq 1 50); do pw-link "$sink:monitor_MONO" "$source:input_MONO" 2>/dev/null && break; sleep 0.1; done
    # A self-contained config: never the user's ~/.asoundrc or system default.
    printf '%s\n' \
      "pcm.!default { type pipewire playback_node \"$sink\" capture_node \"$source\" }" \
      'ctl.!default { type pipewire }' >"$work/asound.conf"
    export ALSA_CONFIG_PATH="$work/asound.conf"
    # Stream nodes carry `client.id`; the client carries the process id.
    export BZZ_AUDIO_STREAMS_CMD='pw-dump 2>/dev/null | python3 -c "
import json, os, sys
items = json.load(sys.stdin)
props = lambda item: (item.get(\"info\") or {}).get(\"props\") or {}
clients = {item[\"id\"] for item in items if str(props(item).get(\"application.process.id\")) == os.environ[\"BZZ_AUDIO_PID\"]}
print(sum(1 for item in items if props(item).get(\"client.id\") in clients and str(props(item).get(\"media.class\", \"\")).startswith(\"Stream/\")))"'
    ;;
  pulseaudio)
    pulseaudio --check 2>/dev/null || pulseaudio -D --exit-idle-time=-1
    for _ in $(seq 1 50); do pactl info >/dev/null 2>&1 && break; sleep 0.1; done
    module=$(pactl load-module module-null-sink sink_name=bzz_t7 rate=48000 channels=1)
    pactl set-default-sink bzz_t7
    pactl set-default-source bzz_t7.monitor
    printf '%s\n' 'pcm.!default { type pulse }' 'ctl.!default { type pulse }' >"$work/asound.conf"
    export ALSA_CONFIG_PATH="$work/asound.conf"
    export BZZ_AUDIO_STREAMS_CMD='{ pactl list sink-inputs; pactl list source-outputs; } | grep -c "application.process.id = \"$BZZ_AUDIO_PID\"" || true'
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

cd "$repo_root"
cargo test --locked --test huddle_device_loopback_test -- --ignored --nocapture
