#!/usr/bin/env bash
# shoot.sh <take>: record tapes/<take>/<pane>.jsonl (+ sync.json) from
# shots/<take>.json with kit/record-multi.py: every pane on one clock.
set -euo pipefail
. "$(dirname "$0")/env.sh"
take=$1
[[ -e $R/lock.held ]] || { echo "the session does not hold the relay lock: run session.sh start" >&2; exit 2; }
mkdir -p -m 700 "$C/tapes" "$C/run"
install -m 600 "$C/shots/$take.json" "$C/run/$take.session.json"
[[ -d $C/tapes/$take ]] && mv "$C/tapes/$take" "$C/run/$take.$(date +%s).void"
echo "$(date -u +%FT%TZ) take $take start" >> "$C/run/takes.log"
set +e
"$MKIT/record-multi.py" --session "$C/run/$take.session.json" --out-dir "$C/tapes/$take" </dev/null
rc=$?
echo "$(date -u +%FT%TZ) take $take exit $rc" >> "$C/run/takes.log"
exit $rc
