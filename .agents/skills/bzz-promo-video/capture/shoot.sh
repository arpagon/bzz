#!/usr/bin/env bash
# Record one shot: shoot.sh <name> [driver-scene [delay-seconds [args...]]]
# Uses shots/<name>.json as the key timeline; writes tapes/<name>.jsonl.
set -euo pipefail
# Paths: S = this capture kit, REPO = the bzz checkout, W = the work dir
# (an ignored experiment folder holding buzz-pin/, bin/, fixture/, tapes/).
S=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$S/../../../.." && pwd); W=${PROMO_WORK:-$PWD}
C=$W
name=$1; scene=${2:-}; delay=${3:-0}; shift $(( $# < 3 ? $# : 3 ))
F=$C/fixture; B=$C/bin/bzz-x86_64-unknown-linux-gnu/bzz
FIX=$REPO/target/release/examples/promo_fixture
REC=$S/../scripts/record-tape.py
mkdir -p -m 700 "$C/run" "$C/tapes"; install -m 600 "$S/shots/$name.json" "$C/run/$name.timeline.json"
rm -f "$C/tapes/$name.jsonl"
if [ -n "$scene" ]; then (sleep "$delay"; "$FIX" drive "$F" "$scene" "$@" > "$C/tapes/$name.driver.log" 2>&1) & fi
BZZ_CONFIG_DIR=$F/config BZZ_DATA_DIR=$F/data BZZ_CACHE_DIR=$F/cache BZZ_PASSPHRASE_FD=3 \
ALSA_CONFIG_PATH=$C/null-asound.conf \
  python3 "$REC" --out "$C/tapes/$name.jsonl" --cols "${COLS:-120}" --rows "${ROWS:-32}" \
  --cell-width 12 --cell-height 26 --timeline "$C/run/$name.timeline.json" --start-after "${START:-3}" --tail "${TAIL:-2}" \
  -- ${CMD:-"$B"} 3<"$F/passphrase"
wait
