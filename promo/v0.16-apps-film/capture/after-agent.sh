#!/usr/bin/env bash
# after-agent.sh: after the take agent (chapters 5-8, the real model's one
# turn), keep beside the tape what the model proposed and what each person
# was shown, and void the take if the script is the bench fixture's bytes.
# Writes approved/: app.lua and brief.md of version 2 (as bzz keeps them),
# app-show-you.txt, app-show-ada.txt, agent-show.txt, fixture-compare.txt.
set -uo pipefail
. "$(dirname "$0")/env.sh"
A=$C/approved; mkdir -p "$A"
"$C/bzz.sh" you app show tasks > "$A/app-show-you.txt" 2>&1
"$C/bzz.sh" ada app show tasks > "$A/app-show-ada.txt" 2>&1
"$C/bzz.sh" lin app show tasks > "$A/app-show-lin.txt" 2>&1
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" you agent show Scout > "$A/agent-show-after-agent.txt" 2>&1
v=$(sed -n 's/^files: *//p' "$A/app-show-you.txt" | head -1)
echo "version folder: ${v#$R/}"
ls -la "$v" | sed "s#$R#\$R#g"
cp "$v"/* "$A/" 2>/dev/null
ls -la "$A"
FIXT=$REPO_SRC/tests/fixtures/apps/board-priority/app.lua
{
  echo "the model's script:  $(wc -c < "$A/app.lua") bytes, $(wc -l < "$A/app.lua") lines, sha256 $(sha256sum < "$A/app.lua" | cut -c1-16)"
  echo "board-priority:      $(wc -c < "$FIXT") bytes, $(wc -l < "$FIXT") lines, sha256 $(sha256sum < "$FIXT" | cut -c1-16)"
  if cmp -s "$A/app.lua" "$FIXT"; then echo "IDENTICAL BYTES: the take is void"; else echo "not the same bytes"; fi
  python3 - "$A/app.lua" "$FIXT" "$REPO_SRC/tests/fixtures/apps/board/app.lua" <<'PY'
import sys, pathlib
model, fixture = (pathlib.Path(p).read_text().splitlines() for p in sys.argv[1:3])
tpl = pathlib.Path(sys.argv[3])
base = set(l.strip() for l in tpl.read_text().splitlines()) if tpl.exists() else set()
added = [l.strip() for l in fixture if l.strip() and l.strip() not in base]
shared = [l for l in added if l in set(x.strip() for x in model)]
print(f"lines the fixture adds to the template Board: {len(added)} (template {'found' if base else 'NOT found'}); of them in the model's script: {len(shared)}")
for l in shared[:20]: print("  shared:", l)
PY
} | tee "$A/fixture-compare.txt"
