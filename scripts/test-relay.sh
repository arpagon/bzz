#!/usr/bin/env bash
set -euo pipefail
: "${BZZ_BUZZ_SOURCE:?set BZZ_BUZZ_SOURCE to the pinned block/buzz checkout}"
PIN=ede26863345a518ec46edd6d7692e0281883491b
HEAD=$(git -C "$BZZ_BUZZ_SOURCE" rev-parse HEAD)
if [[ "$HEAD" != "$PIN" && "${BZZ_ALLOW_UNPINNED_RELAY:-}" != 1 ]]; then
  echo "expected Buzz $PIN, found $HEAD" >&2; exit 1
fi
cleanup() {
  tmux kill-session -t "${TMUX_SESSION:-dawn-relay}" 2>/dev/null || true
  if [[ -n "${BZZ_TMUX_KEEPALIVE:-}" ]]; then
    tmux kill-session -t "$BZZ_TMUX_KEEPALIVE" 2>/dev/null || true
  fi
  tmux set-environment -gu BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN 2>/dev/null || true
  tmux set-environment -gu BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC 2>/dev/null || true
  tmux set-environment -gu BUZZ_RELAY_PRIVATE_KEY 2>/dev/null || true
  docker compose -p buzz-harness -f "$BZZ_BUZZ_SOURCE/docker-compose.harness.yml" down -v || true
}
trap cleanup EXIT INT TERM
# Dense same-second pagination coverage intentionally publishes more than the
# production human quota inside this isolated, disposable harness.
export BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN=5000
export BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC=5000
# Deterministic harness-only relay key enables NIP-29 projection verification.
export BUZZ_RELAY_PRIVATE_KEY="$(printf '%064d' 1)"
# A tmux server exits immediately when it has no sessions. Keep one disposable
# session alive while setting the global environment; the Buzz helper starts
# the actual relay session afterward.
BZZ_TMUX_KEEPALIVE="bzz-relay-env-$$"
export BZZ_TMUX_KEEPALIVE
tmux new-session -d -s "$BZZ_TMUX_KEEPALIVE" "sleep 3600"
tmux set-environment -g BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN "$BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN"
tmux set-environment -g BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC "$BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC"
tmux set-environment -g BUZZ_RELAY_PRIVATE_KEY "$BUZZ_RELAY_PRIVATE_KEY"
(cd "$BZZ_BUZZ_SOURCE" && ./scripts/start-isolated-test-relay.sh --profile dev)
for _ in $(seq 1 60); do curl -fsS http://localhost:3030/ >/dev/null && break; sleep 1; done
BZZ_E2E_RELAY_URL=ws://localhost:3030 cargo test --test relay_integration -- --ignored --test-threads=1
