#!/usr/bin/env bash
# session.sh start|stop|status: one real-model session of the final take.
# start: take the relay harness lock (held by a background holder until
#        stop), check port 3030 and the harness are free, run setup.sh and
#        the warm-up (cards, the host installed; no model turn).
# stop:  tear the relay down and show it, delete $R (keys, passphrase), free
#        the lock.
set -uo pipefail
. "$(dirname "$0")/env.sh"
case "${1:-}" in
start)
  [[ -e $R/hold ]] && { echo "already started"; exit 1; }
  mkdir -p -m 700 "$R" "$C/run"
  touch "$R/hold"
  setsid flock "$LOCK" bash -c 'echo locked > "$0/lock.held"; while [ -e "$0/hold" ]; do sleep 2; done; rm -f "$0/lock.held"' "$R" </dev/null >/dev/null 2>&1 &
  echo "waiting for the relay harness lock"
  until [[ -e $R/lock.held ]]; do sleep 2; done; echo "lock held"
  if ss -ltn | grep -q ':3030 '; then echo "port 3030 is taken by something not ours: stopping"; rm -f "$R/hold"; exit 1; fi
  if docker ps --format '{{.Names}}' | grep -q '^buzz-harness'; then echo "buzz-harness is running and not ours: stopping"; rm -f "$R/hold"; exit 1; fi
  "$C/setup.sh" > "$C/run/setup.log" 2>&1 || { echo "setup failed"; tail -30 "$C/run/setup.log"; exit 1; }
  python3 "$C/make-shots.py"
  "$C/shoot.sh" warmup > "$C/run/warmup.log" 2>&1 || { echo "warmup failed"; tail -40 "$C/run/warmup.log"; exit 1; }
  echo "session ready";;
stop)
  tmux kill-session -t dawn-relay 2>/dev/null || true
  tmux kill-session -t bzz-keep 2>/dev/null || true
  for n in BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC \
    BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED BUZZ_MEMBERSHIP_PUBLIC_ENABLED; do tmux set-environment -gu "$n" 2>/dev/null || true; done
  (cd "$PIN" && docker compose -p buzz-harness -f docker-compose.harness.yml down -v) >> "$C/run/teardown.log" 2>&1 || true
  rm -f "$R/hold"; sleep 3
  # $R/pi is a link: rm removes the link, never what it points to.
  rm -rf "$R"; echo "removed $R"
  echo "--- docker ps -a | grep buzz-harness (empty is down)"; docker ps -a --format '{{.Names}}' | grep '^buzz-harness' || echo "(none)"
  echo "--- port 3030"; ss -ltn | grep ':3030 ' || echo "(free)"
  echo "--- lock"; flock -n "$LOCK" true && echo "(free)" || echo "still held";;
status)
  ls -la "$R" 2>&1 | head; docker ps --format '{{.Names}}' | grep '^buzz-harness' || echo "no harness";;
*) echo "usage: session.sh start|stop|status"; exit 2;;
esac
