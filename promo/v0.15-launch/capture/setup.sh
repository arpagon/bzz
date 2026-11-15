#!/usr/bin/env bash
# Fresh pinned relay + generated community (Ada, Lin, Sam, You) for the v0.15 proof reel.
# Two bzz profiles of the same owner: /tmp/bzz-demo/here and /tmp/bzz-demo/there.
# No agent is made here: every agent is made on camera by bzz itself.
set -euo pipefail
C=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$C/../../../../.." && pwd)
S=$REPO/.agents/skills/bzz-promo-video/capture
export PROMO_WORK=$C
R=/tmp/bzz-demo; F=$R/here; B=$C/bin/bzz-x86_64-unknown-linux-gnu/bzz
FIX=$REPO/target/release/examples/promo_fixture
tmux kill-session -t dawn-relay 2>/dev/null || true
tmux kill-session -t kit 2>/dev/null || true
(cd "$C/buzz-pin" && docker compose -p buzz-harness -f docker-compose.harness.yml down -v >/dev/null 2>&1) || true
"$S/start-relay.sh" > "$C/start-relay.log" 2>&1
rm -rf "$R"; mkdir -m 700 "$R" "$F" "$R/there"; mkdir -p -m 700 "$C/run" "$C/tapes"
"$FIX" seed "$F" "$C/buzz-pin" "$S/../assets/avatars"
"$FIX" history "$F" "$REPO/site/public/assets/og.png"
sed -i 's/^thread_width = .*/thread_width = 56/' "$F/config/config.toml"
# The folder the owner will grant: a tiny generated project.
mkdir -p "$R/site/dist"
printf '#!/bin/sh\n# the checks of the site\necho "links ......... ok"\necho "spelling ...... ok"\necho "build ......... ok"\necho "3 checks passed"\n' > "$R/site/test.sh"
chmod +x "$R/site/test.sh"; echo '<h1>Studio</h1>' > "$R/site/dist/index.html"; echo '# Studio site' > "$R/site/README.md"
# The other bzz: the same owner's key, brought over with bzz's own backup.
cp "$F/passphrase" "$R/there/passphrase"
ID=$(grep -m1 '^id = ' "$F/config/config.toml" | cut -d'"' -f2)
cat > "$C/shots/x-backup.json" <<JSON
[{"type":"bzz identity backup --output $R/owner.backup $ID\r","step":0.01},
 {"until":"Identity passphrase:","timeout":20},{"wait":0.5},{"type":"@PASS@\r","step":0.01},
 {"until":"New backup passphrase:","timeout":20},{"wait":0.5},{"type":"backup-passphrase-demo\r","step":0.01},
 {"until":"Confirm passphrase:","timeout":20},{"wait":0.5},{"type":"backup-passphrase-demo\r","step":0.01},
 {"until":"pubkey:","timeout":30},{"wait":1}]
JSON
cat > "$C/shots/x-import.json" <<JSON
[{"type":"bzz identity import-backup --label demo --input $R/owner.backup --backend encrypted-file\r","step":0.01},
 {"until":"Backup passphrase:","timeout":20},{"wait":0.5},{"type":"backup-passphrase-demo\r","step":0.01},
 {"until":"New identity passphrase:","timeout":20},{"wait":0.5},{"type":"@PASS@\r","step":0.01},
 {"until":"Confirm passphrase:","timeout":20},{"wait":0.5},{"type":"@PASS@\r","step":0.01},
 {"until":"pubkey:","timeout":30},{"wait":1}]
JSON
SHELL_SHOT=1 "$C/shoot.sh" x-backup
PROFILE=there SHELL_SHOT=1 "$C/shoot.sh" x-import
rm -f "$R/owner.backup"
T=$R/there
IDB=$(grep -m1 '^id = ' "$T/config/config.toml" | cut -d'"' -f2)
BZZ_CONFIG_DIR=$T/config BZZ_DATA_DIR=$T/data BZZ_CACHE_DIR=$T/cache "$B" community add Studio ws://localhost:3030/ "$IDB" --allow-insecure-localhost
# The same look in both.
python3 - "$F/config/config.toml" "$T/config/config.toml" <<'PY'
import sys, re
a, b = (open(p).read() for p in sys.argv[1:3])
tail = a[a.index("[ui]"):]
b = b[:b.index("[ui]")] + tail if "[ui]" in b else b + "\n" + tail
open(sys.argv[2], "w").write(b)
PY
echo "fixture ready: $R"
