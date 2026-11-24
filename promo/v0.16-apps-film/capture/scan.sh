#!/usr/bin/env bash
# scan.sh: search every final tape's bytes (scan.py: the machine, its owner,
# the fixture's secret keys and passphrases, key/token/sign-in shapes), then a
# planted copy that must be caught.
set -uo pipefail
. "$(dirname "$0")/env.sh"
status=0
python3 "$C/scan.py" --state "$F/state.json" --passphrase "$F/passphrase" --passphrase "$R/ada/passphrase" \
  --passphrase "$R/lin/passphrase" "$C"/tapes/*/*.jsonl > "$C/run/scan.txt" 2>&1 && echo "scan: no hit" || { echo "scan: HIT"; status=1; }
sed -i "s#$C/##g" "$C/run/scan.txt"
grep -c 'hits: none' "$C/run/scan.txt"
python3 - "$C/tapes/agent/you.jsonl" "$R/sentinel.jsonl" "$F/state.json" "$R/ada/passphrase" <<'PY'
import base64, json, sys
src, dst, state, pp = sys.argv[1:]
lines = open(src).read().splitlines()
key = json.load(open(state))["keys"]["ada"]
import os
planted = f"{open(pp).read().strip()} {key} /home/{os.environ.get('USER', 'someone')}/.pi/agent eyJhbGciOiJIUzI1NiJ9.e30 someone@example.org".encode()
lines.insert(5, json.dumps({"t": 0.5, "o": base64.b64encode(planted).decode()}))
open(dst, "w").write("\n".join(lines) + "\n")
PY
python3 "$C/scan.py" --state "$F/state.json" --passphrase "$R/ada/passphrase" "$R/sentinel.jsonl" > "$C/run/scan-sentinel.txt" 2>&1 \
  && { echo "SENTINEL NOT CAUGHT"; status=1; } || echo "sentinel caught: $(grep -o 'hits: .*' "$C/run/scan-sentinel.txt")"
sed -i "s#$R#\$R#g" "$C/run/scan-sentinel.txt"
rm -f "$R/sentinel.jsonl"
exit $status
