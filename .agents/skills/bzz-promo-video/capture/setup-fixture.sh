#!/usr/bin/env bash
# Fresh relay + generated community + broker agent "Scout" for the promo shots.
set -euo pipefail
# Paths: S = this capture kit, REPO = the bzz checkout, W = the work dir
# (an ignored experiment folder holding buzz-pin/, bin/, fixture/, tapes/).
S=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$S/../../../.." && pwd); W=${PROMO_WORK:-$PWD}
C=$W
F=$C/fixture; B=$C/bin/bzz-x86_64-unknown-linux-gnu/bzz
FIX=$REPO/target/release/examples/promo_fixture
IMAGE=$REPO/site/public/assets/og.png
tmux kill-session -t dawn-relay 2>/dev/null || true
(cd "$C/buzz-pin" && docker compose -p buzz-harness -f docker-compose.harness.yml down -v >/dev/null 2>&1) || true
"$S/start-relay.sh" > "$C/start-relay.log" 2>&1
rm -rf "$F"; mkdir -m 700 "$F"; mkdir -p -m 700 "$C/run"
# Keep the relay's start command (with its signing key) so offline shots can restart it.
(umask 077; tmux list-panes -t dawn-relay -F '#{pane_start_command}' | sed 's/^"//; s/"$//' > "$C/run/relay.cmd")
"$FIX" seed "$F" "$C/buzz-pin"
export BZZ_CONFIG_DIR=$F/config BZZ_DATA_DIR=$F/data BZZ_CACHE_DIR=$F/cache BZZ_PASSPHRASE_FD=3
umask 077
"$B" broker create scout --yes 3<"$F/passphrase" \
  | sed -n 's/^credential (shown once; give it to the agent process): //p' > "$F/agent.credential"
test -s "$F/agent.credential"
"$B" broker list 3<"$F/passphrase" | cut -f2 > "$F/agent.pubkey"
"$B" broker allow scout channel.read,message.post,message.reply,reaction.add,profile.set,storage.address 3<"$F/passphrase"
"$FIX" bot "$F" "$(cat "$F/agent.pubkey")"
"$FIX" history "$F" "$IMAGE"
# Warm the cache and let the agent name itself while bzz is unlocked.

"$S/shoot.sh" warmup agent-profile 8
cat "$C/tapes/warmup.driver.log"
