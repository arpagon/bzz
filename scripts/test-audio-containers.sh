#!/usr/bin/env bash
# Huddle audio on Linux, proven inside disposable containers: never on this
# machine's own sound. Each container runs with no network, as user 1000,
# without the host's sound socket or /dev/snd; its sound server has no card,
# only a null sink. Images come from ci/audio-servers/ (built once, cached).
#
#   scripts/test-audio-containers.sh t7 T7_BINARY
#       T7 (tests/huddle_device_loopback_test.rs) through PipeWire on Debian,
#       PipeWire on EL9 and PulseAudio, then with no sound server at all.
#       Then with no cookie anywhere in the test's home (as on a session
#       where no PulseAudio client ever wrote one): PipeWire, PulseAudio of
#       the same user, and a PulseAudio daemon of another user, open to
#       anonymous clients (joins) and not (refused, said so; joins with its
#       cookie in PULSE_COOKIE). T7_BINARY is the built test binary, linked
#       against glibc 2.35 or older (build it in the CI image,
#       ci/linux-build/).
#   scripts/test-audio-containers.sh start BZZ_BINARY
#       bzz --version and bzz paths on clean Debian, Ubuntu 22.04 and 24.04
#       and AlmaLinux 9, which have no sound library.
#   scripts/test-audio-containers.sh serve DIR [pipewire|tone] | stop DIR
#       A PulseAudio with a mono null sink as its defaults, listening on
#       DIR/native for bzz on this machine (PULSE_SERVER=unix:DIR/native):
#       the TUI harness's huddles join it and reach no real device. With
#       `pipewire`, a PipeWire session instead, on DIR/pulse/native; with
#       `tone`, that session's microphone plays a 440 Hz tone (a microphone
#       that speaks), for a call between two clients.
#   scripts/test-audio-containers.sh record DIR SECONDS
#       What the served null sink plays during SECONDS, as raw 48 kHz mono
#       f32 in DIR/played.f32 (a call's receiving side).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/.." && pwd)

image() {
  local name=$1 tag=bzz-audio-$1:local
  if ! docker image inspect "$tag" >/dev/null 2>&1; then
    docker build --quiet -t "$tag" -f "$repo_root/ci/audio-servers/Dockerfile.$name" \
      "$repo_root/ci/audio-servers" >/dev/null
  fi
  printf '%s\n' "$tag"
}

# Inside a container: start a PipeWire session (PipeWire, WirePlumber and
# pipewire-pulse on a private D-Bus) and wait until it answers.
read -r -d '' start_pipewire <<'EOF' || true
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/runtime
mkdir -p "$HOME" && mkdir -m 700 -p "$XDG_RUNTIME_DIR"
dbus-daemon --session --fork --address="unix:path=$XDG_RUNTIME_DIR/bus" >/dev/null
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
pipewire >/tmp/pipewire.log 2>&1 &
sleep 0.5
wireplumber >/tmp/wireplumber.log 2>&1 &
pipewire-pulse >/tmp/pipewire-pulse.log 2>&1 &
for _ in $(seq 1 100); do pactl info >/dev/null 2>&1 && break; sleep 0.1; done
echo "server: $(pactl info | sed -n 's/^Server Name: //p')"
EOF

run() {
  docker run --rm --network none --user 1000:1000 "$@"
}

case ${1:-} in
  t7)
    binary=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
    mounts=(-v "$binary:/t7:ro" -v "$here/test-audio-loopback.sh:/test-audio-loopback.sh:ro")
    status=0
    for name in pipewire-debian pipewire-el9; do
      echo "### T7, $name"
      run "${mounts[@]}" -e BZZ_T7_BINARY=/t7 -e BZZ_AUDIO_BACKEND=pipewire "$(image "$name")" \
        bash -c "$start_pipewire"$'\n''exec bash /test-audio-loopback.sh' || status=1
    done
    echo "### T7, pulseaudio"
    run "${mounts[@]}" -e HOME=/tmp -e BZZ_T7_BINARY=/t7 -e BZZ_AUDIO_BACKEND=pulseaudio \
      "$(image pulseaudio)" bash /test-audio-loopback.sh || status=1
    echo "### T7, no sound server (expected to fail with the one-line message)"
    out=$(run -v "$binary:/t7:ro" -e HOME=/tmp -e XDG_RUNTIME_DIR=/tmp debian:stable-slim \
      /t7 --ignored --nocapture 2>&1 || true)
    printf '%s\n' "$out" | grep -E 'needs PipeWire or PulseAudio|test result' || true
    printf '%s\n' "$out" | grep -q 'huddle audio needs PipeWire or PulseAudio running' \
      || { echo "the no-server message is missing" >&2; status=1; }
    # No cookie anywhere: bzz 0.17.0 sent an empty one and every server
    # refused it (docs/validation-v0.17.1/cookie.md).
    echo "### T7, pipewire-debian, no cookie anywhere"
    run "${mounts[@]}" -e BZZ_T7_BINARY=/t7 -e BZZ_AUDIO_BACKEND=pipewire -e BZZ_T7_HOME=/tmp/no-cookie \
      "$(image pipewire-debian)" bash -c "$start_pipewire"$'\n''exec bash /test-audio-loopback.sh' || status=1
    echo "### T7, pulseaudio of the same user, no cookie anywhere"
    run "${mounts[@]}" -e HOME=/tmp -e BZZ_T7_BINARY=/t7 -e BZZ_AUDIO_BACKEND=pulseaudio \
      -e BZZ_T7_HOME=/tmp/no-cookie "$(image pulseaudio)" bash /test-audio-loopback.sh || status=1
    # A daemon of another user (uid 1234) on a shared volume; T7 as 1000.
    for anonymous in 1 0; do
      volume=bzz-t7-other-$$-$anonymous name=bzz-t7-other-$$-$anonymous
      docker volume create "$volume" >/dev/null
      docker run --rm --network none -v "$volume:/sound" "$(image pulseaudio)" chown 1234:1234 /sound
      docker run -d --rm --name "$name" --network none --user 1234:1234 -e HOME=/tmp \
        -v "$volume:/sound" "$(image pulseaudio)" sh -c '
          printf "%s\n" \
            "load-module module-native-protocol-unix socket=/sound/native auth-anonymous='"$anonymous"' auth-cookie=/sound/cookie" \
            "load-module module-null-sink sink_name=bzz_null rate=48000 channels=1" \
            "set-default-sink bzz_null" "set-default-source bzz_null.monitor" >/tmp/bzz.pa
          exec pulseaudio -n --daemonize=no --exit-idle-time=-1 --disallow-exit -F /tmp/bzz.pa' >/dev/null
      for _ in $(seq 1 100); do
        docker exec "$name" sh -c 'PULSE_SERVER=unix:/sound/native PULSE_COOKIE=/sound/cookie pactl info 2>/dev/null | grep -q "Default Sink: bzz_null"' && break
        sleep 0.1
      done
      t7_other() {
        run "${mounts[@]}" -v "$volume:/sound" -e PULSE_SERVER=unix:/sound/native -e BZZ_T7_BINARY=/t7 \
          -e BZZ_AUDIO_BACKEND=default -e HOME=/tmp "$@" "$(image pulseaudio)" bash /test-audio-loopback.sh 2>&1
      }
      if [[ $anonymous == 1 ]]; then
        echo "### T7, pulseaudio of another user, anonymous clients allowed, no cookie anywhere"
        t7_other -e BZZ_T7_HOME=/tmp/no-cookie || status=1
      else
        echo "### T7, pulseaudio of another user, its cookie needed, no cookie anywhere (expected to fail, saying so)"
        out=$(t7_other -e BZZ_T7_HOME=/tmp/no-cookie || true)
        printf '%s\n' "$out" | grep -E 'refused bzz|needs PipeWire|test result' || true
        printf '%s\n' "$out" | grep -q 'the sound server refused bzz: check PULSE_COOKIE' \
          || { echo "the refused message is missing" >&2; status=1; }
        echo "### T7, pulseaudio of another user, its cookie in PULSE_COOKIE"
        docker exec "$name" cat /sound/cookie \
          | docker run --rm -i --network none -v "$volume:/sound" "$(image pulseaudio)" \
            sh -c 'cat >/sound/given && chown 1000 /sound/given && chmod 600 /sound/given'
        t7_other -e PULSE_COOKIE=/sound/given || status=1
      fi
      docker rm -f "$name" >/dev/null 2>&1 || true
      docker volume rm "$volume" >/dev/null
    done
    exit $status
    ;;
  start)
    binary=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
    status=0
    for base in debian:stable-slim ubuntu:22.04 ubuntu:24.04 almalinux:9-minimal; do
      sound=$(run --entrypoint sh "$base" -c \
        'ls /usr/lib*/libasound.so* /usr/lib*/*/libasound.so* /usr/lib*/libpulse.so* /usr/lib*/*/libpulse.so* 2>/dev/null | wc -l')
      out=$(run -e HOME=/tmp/h -e XDG_CONFIG_HOME=/tmp/h/c -e XDG_DATA_HOME=/tmp/h/d \
        -e XDG_CACHE_HOME=/tmp/h/k -e XDG_STATE_HOME=/tmp/h/s -v "$binary:/bzz:ro" "$base" \
        sh -c 'mkdir -p /tmp/h && /bzz --version && /bzz paths' 2>&1) || status=1
      printf '%-20s sound libraries: %s  %s\n' "$base" "$sound" "$(printf '%s' "$out" | tr '\n' ' ' | cut -c1-200)"
    done
    exit $status
    ;;
  serve)
    dir=$(mkdir -p "$2" && cd "$2" && pwd)
    name=bzz-sound-$(printf '%s' "$dir" | cksum | cut -d' ' -f1)
    mode=${3:-}
    if [[ $mode == pipewire || $mode == tone ]]; then
      # A PipeWire session; pipewire-pulse listens on DIR/pulse/native. With
      # `tone` the default source is the monitor of a second null sink that
      # plays a 440 Hz tone in a loop (PulseAudio's own sine source works in
      # fixed 2 s blocks, which a call hears as bursts).
      mic=:
      if [[ $mode == tone ]]; then
        python3 -c 'import math, struct, sys
sys.stdout.buffer.write(b"".join(struct.pack("<f", 0.4 * math.sin(2 * math.pi * 440 * n / 48000)) for n in range(48000 * 10)))' >"$dir/tone.f32"
        mic='pactl load-module module-null-sink sink_name=bzz_mic rate=48000 channels=1 >/dev/null
          pactl set-default-source bzz_mic.monitor
          ( while :; do pacat --raw --format=float32le --rate=48000 --channels=1 --latency-msec=20 -d bzz_mic /sound/tone.f32; done ) &'
      fi
      docker run -d --rm --name "$name" --network none --user "$(id -u):$(id -g)" \
        -e PULSE_SERVER=unix:/sound/pulse/native -v "$dir:/sound" "$(image pipewire-debian)" \
        bash -c "${start_pipewire//XDG_RUNTIME_DIR=\/tmp\/runtime/XDG_RUNTIME_DIR=/sound}"'
          pactl load-module module-null-sink sink_name=bzz_null rate=48000 channels=1 >/dev/null
          pactl set-default-sink bzz_null && pactl set-default-source bzz_null.monitor
          '"$mic"'
          exec sleep infinity' >/dev/null
      socket=$dir/pulse/native
    else
      docker run -d --rm --name "$name" --network none --user "$(id -u):$(id -g)" -e HOME=/tmp \
        -e PULSE_SERVER=unix:/sound/native -v "$dir:/sound" "$(image pulseaudio)" sh -c '
          printf "%s\n" \
            "load-module module-native-protocol-unix socket=/sound/native auth-anonymous=1" \
            "load-module module-null-sink sink_name=bzz_null rate=48000 channels=1" \
            "set-default-sink bzz_null" "set-default-source bzz_null.monitor" >/tmp/bzz.pa
          exec pulseaudio -n --daemonize=no --exit-idle-time=-1 --disallow-exit -F /tmp/bzz.pa' >/dev/null
      socket=$dir/native
    fi
    for _ in $(seq 1 100); do [[ -S $socket ]] && break; sleep 0.1; done
    [[ -S $socket ]] || { docker logs "$name" >&2; docker rm -f "$name" >/dev/null; exit 1; }
    # The defaults are set once the server answers; wait for them too.
    for _ in $(seq 1 50); do
      docker exec "$name" sh -c 'pactl info 2>/dev/null | grep -q "Default Sink: bzz_null"' && break; sleep 0.1
    done
    printf 'unix:%s\n' "$socket"
    ;;
  record)
    dir=$(cd "$2" && pwd)
    docker exec "bzz-sound-$(printf '%s' "$dir" | cksum | cut -d' ' -f1)" sh -c \
      'timeout "$1" parec --raw --format=float32le --rate=48000 --channels=1 -d bzz_null.monitor >/sound/played.f32; true' sh "$3"
    ;;
  stop)
    dir=$(cd "$2" && pwd)
    docker rm -f "bzz-sound-$(printf '%s' "$dir" | cksum | cut -d' ' -f1)" >/dev/null 2>&1 || true
    ;;
  *)
    sed -n '2,27p' "$0" >&2
    exit 2
    ;;
esac
