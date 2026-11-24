#!/usr/bin/env bash
# Start the pinned isolated harness relay (buzz-harness project, :3030) from
# $W/buzz-pin, a worktree of block/buzz at the pin in docs/protocol-compatibility.md.
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd); REPO=${BZZ_CHECKOUT:-$(cd "$S/../../../.." && pwd)}; W=${PROMO_WORK:-$PWD}
# Optional local toolchain setup (Rust homes); absent on machines that need none.
# It cds into the checkout: go back.
[ -f "$REPO/runs/env.sh" ] && { . "$REPO/runs/env.sh"; cd "$W"; }
# Never build the pinned relay from here: a copy of the kit outside the checkout once did.
ls "$W"/buzz-pin/target/*/buzz-relay >/dev/null 2>&1 || {
  echo "no built relay under $W/buzz-pin/target: build the pin first (its own scripts), then run this" >&2; exit 1; }
export BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN=5000 BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC=5000
export BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED=true BUZZ_MEMBERSHIP_PUBLIC_ENABLED=true
export RELAY_LOG="$W/relay.log"
tmux has-session -t bzz-keep 2>/dev/null || tmux new-session -d -s bzz-keep "sleep 86400"
for n in BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC \
  BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED BUZZ_MEMBERSHIP_PUBLIC_ENABLED PATH; do
  tmux set-environment -g "$n" "${!n}"
done
for n in RUSTUP_HOME CARGO_HOME; do [ -n "${!n:-}" ] && tmux set-environment -g "$n" "${!n}"; done
cd "$W/buzz-pin" && ./scripts/start-isolated-test-relay.sh --profile dev
