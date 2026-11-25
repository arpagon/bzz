#!/usr/bin/env bash
# shoot.sh <name>: record tapes/<name>.jsonl from shots/<name>.json.
# TUI=1 (default) runs bzz itself with the passphrase on descriptor 3;
# SHELL_SHOT=1 runs a plain shell where bzz is typed as a user would.
# PROFILE=here|there picks the bzz ("this machine" / "the other").
set -euo pipefail
C=$(cd "$(dirname "$0")" && pwd); E=$(cd "$C/.." && pwd)
name=$1; P=${PROFILE:-here}; F=/tmp/bzz-demo/$P; B=$C/bin/bzz-x86_64-unknown-linux-gnu
mkdir -p -m 700 "$C/tapes" "$C/run"
# The disposable passphrase is typed where bzz asks for it: @PASS@ in a shot.
TL=$C/run/$name.timeline.json
(umask 077; sed "s/@PASS@/$(cat "$F/passphrase" 2>/dev/null || true)/g" "$C/shots/$name.json" > "$TL")
export BZZ_CONFIG_DIR=$F/config BZZ_DATA_DIR=$F/data BZZ_CACHE_DIR=$F/cache
export ALSA_CONFIG_PATH=$C/null-asound.conf
export PULSE_SERVER=${BZZ_CAPTURE_PULSE_SERVER:-unix:$C/no-sound-server/native}
# BG="command": something a generated teammate does during the take.
if [ -n "${BG:-}" ]; then (eval "$BG" > "$C/run/$name.bg.log" 2>&1) & fi
if [ "${SHELL_SHOT:-0}" = 1 ]; then
  export PATH=$B:$E/../../../buzz-pin/target/debug:$PATH PS1="$P \$ " HISTFILE=/dev/null
  cd /tmp/bzz-demo
  exec "$E/scripts/record-live.py" --out "$C/tapes/$name.jsonl" --cols "${COLS:-120}" --rows "${ROWS:-32}" \
    --timeline "$TL" --start-after "${START:-1}" --tail "${TAIL:-2}" \
    -- bash --noprofile --norc
else
  export BZZ_PASSPHRASE_FD=3
  exec "$E/scripts/record-live.py" --out "$C/tapes/$name.jsonl" --cols "${COLS:-120}" --rows "${ROWS:-32}" \
    --timeline "$TL" --start-after "${START:-3}" --tail "${TAIL:-2}" \
    -- "$B/bzz" 3<"$F/passphrase"
fi
