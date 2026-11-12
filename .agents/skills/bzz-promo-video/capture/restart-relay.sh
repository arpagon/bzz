#!/usr/bin/env bash
# Restart the harness relay with its original signing key (fixture/relay.cmd).
set -euo pipefail
# Paths: S = this capture kit, REPO = the bzz checkout, W = the work dir
# (an ignored experiment folder holding buzz-pin/, bin/, fixture/, tapes/).
S=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$S/../../../.." && pwd); W=${PROMO_WORK:-$PWD}
C=$W
tmux kill-session -t dawn-relay 2>/dev/null || true
tmux new-session -d -s dawn-relay "$(cat "$C/run/relay.cmd")"
for _ in $(seq 60); do curl -fs -o /dev/null http://localhost:3030/ && { echo relay up; exit 0; }; sleep 1; done
echo "relay did not come back" >&2; exit 1
