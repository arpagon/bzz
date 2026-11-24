#!/usr/bin/env bash
# shoot.sh <name>: record tapes/<name>.jsonl from shots/<name>.json with
# record-live.py (the kit's, plus soft waits), 120x32 cells of 12x26 px.
set -euo pipefail
. "$(dirname "$0")/env.sh"
name=$1
[[ -e $R/hold ]] || { echo "the session does not hold the relay lock: run session.sh start" >&2; exit 2; }
mkdir -p -m 700 "$C/tapes" "$C/run"
install -m 600 "$C/shots/$name.json" "$C/run/$name.timeline.json"
[[ -e $C/tapes/$name.jsonl ]] && mv "$C/tapes/$name.jsonl" "$C/run/$name.$(date +%s).void.jsonl"
exec "$C/record-live.py" --out "$C/tapes/$name.jsonl" --cols "${COLS:-120}" --rows "${ROWS:-32}" \
  --cell-width 12 --cell-height 26 --timeline "$C/run/$name.timeline.json" \
  --start-after "${START:-3}" --tail "${TAIL:-2}" -- "$C/bzz.sh" </dev/null
