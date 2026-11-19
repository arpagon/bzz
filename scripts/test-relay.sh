#!/usr/bin/env bash
# Pinned-relay integration. Default: tests/relay_integration.rs.
# --nip-fi: start a test-only NIP-FA issuer (examples/fake_nip_fa.rs) on
# localhost:3093 and the relay in BUZZ_NIP_FI_MODE=enforce trusting it, then
# run tests/relay_nip_fi_integration.rs.
# BZZ_RELAY_OWN_QUOTA=1: keep the relay's own quota for people and run only
# the test of it.
# --herdr: write a disposable viewer profile and generated content with
# examples/harness_fixture.rs, then drive the release TUI through the
# `harness` scenarios of scripts/tui-herdr-scenarios.toml (run this from a
# Herdr pane with BZZ_HERDR_PANE set; see docs/e2e-herdr.md).
set -euo pipefail
: "${BZZ_BUZZ_SOURCE:?set BZZ_BUZZ_SOURCE to the pinned block/buzz checkout}"
# One run at a time on a machine: the harness has one port (3030) and one
# Docker project, and a second run's cleanup would remove the first's
# containers. The lock is shared by every checkout of this user.
if command -v flock >/dev/null 2>&1 && [[ -z "${BZZ_RELAY_LOCK_HELD:-}" ]]; then
  # A fixed path: two shells with different TMPDIR must meet at one lock.
  lock="/tmp/bzz-test-relay-$(id -u).lock"
  echo "waiting for the relay harness lock ($lock)" >&2
  BZZ_RELAY_LOCK_HELD=1 exec flock "$lock" "$0" "$@"
fi
# A run from a checkout that has no lock yet: wait until its harness is gone.
while docker ps --format '{{.Names}}' 2>/dev/null | grep -q '^buzz-harness'; do
  echo "the relay harness is in use; waiting" >&2; sleep 20
done
PROFILE=default
if [[ "${1:-}" == "--nip-fi" ]]; then PROFILE=nip-fi; fi
if [[ "${1:-}" == "--herdr" ]]; then PROFILE=herdr; fi
PIN=bf3bdd0eaefe39289bf9620594f254c7f68897f5
HEAD=$(git -C "$BZZ_BUZZ_SOURCE" rev-parse HEAD)
if [[ "$HEAD" != "$PIN" && "${BZZ_ALLOW_UNPINNED_RELAY:-}" != 1 ]]; then
  echo "expected Buzz $PIN, found $HEAD" >&2; exit 1
fi
RELAY_ENV=(
  BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC
  BUZZ_RATE_LIMIT_HUMAN_API_CALLS_PER_MIN
  BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED BUZZ_MEMBERSHIP_PUBLIC_ENABLED BUZZ_NIP_FI_MODE BUZZ_NIP_FI_ISSUERS
  BUZZ_NIP_FI_COMMUNITIES BUZZ_NIP_FI_MAX_CONNECTION_LIFETIME_SECS
)
ISSUER_PID=
cleanup() {
  status=$?
  if [[ $status -ne 0 && -f /tmp/dawn-relay-run.log ]]; then
    # Disposable harness: only warnings/errors, for diagnosing CI-only failures.
    echo "--- relay warnings/errors (last 60) ---"
    grep -aE 'WARN|ERROR' /tmp/dawn-relay-run.log | tail -n 60 | cut -c1-400 || true
  fi
  tmux kill-session -t "${TMUX_SESSION:-dawn-relay}" 2>/dev/null || true
  if [[ -n "${BZZ_TMUX_KEEPALIVE:-}" ]]; then
    tmux kill-session -t "$BZZ_TMUX_KEEPALIVE" 2>/dev/null || true
  fi
  for name in "${RELAY_ENV[@]}"; do tmux set-environment -gu "$name" 2>/dev/null || true; done
  if [[ -n "$ISSUER_PID" ]]; then kill "$ISSUER_PID" 2>/dev/null || true; fi
  docker compose -p buzz-harness -f "$BZZ_BUZZ_SOURCE/docker-compose.harness.yml" down -v || true
}
trap cleanup EXIT INT TERM
# Dense same-second pagination coverage intentionally publishes more than the
# production human quota inside this isolated, disposable harness.
# BZZ_RELAY_OWN_QUOTA=1 leaves a person's quota as the relay ships it (60
# writes a minute, 50 in five seconds) and runs the one test that has to
# meet it (v0.16 M2); the rest of the suite cannot pass under it.
if [[ -z "${BZZ_RELAY_OWN_QUOTA:-}" ]]; then
  export BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN=5000
  export BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC=5000
else
  export BZZ_RELAY_OWN_QUOTA
  export BZZ_RELAY_TEST_FILTER="${BZZ_RELAY_TEST_FILTER:-real_relay_app_session_meets_a_persons_own_quota}"
fi
# The Herdr run starts bzz more than twenty times in a few minutes as one
# person, each start a whole sync: its first six starts, twenty seconds
# apart in all, made about 190 queries and the relay's quota for a person
# refused the next. No person starts bzz like that. The other two profiles
# keep the relay's own quota, which is how a client that asks in a loop is
# caught (v0.15 M1).
if [[ "$PROFILE" == herdr ]]; then
  export BUZZ_RATE_LIMIT_HUMAN_API_CALLS_PER_MIN=5000
fi
# v0.13 exercises optional relay capabilities that are off by default.
export BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED=true
export BUZZ_MEMBERSHIP_PUBLIC_ENABLED=true
# The Buzz harness generates a fresh relay key per run. Tests read it from
# NIP-11, the same origin-bound source production pinning uses.
if [[ "$PROFILE" == nip-fi ]]; then
  cargo build --locked --example fake_nip_fa
  key=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
  # The relay resolves the issuer's `localhost` and pins the first address.
  # Where that is ::1, an IPv4-only issuer is unreachable and every FI check
  # is "authorization unavailable" (503), so listen on both loopbacks.
  echo "localhost resolves to: $(getent ahosts localhost | awk '{print $1}' | sort -u | tr '\n' ' ')"
  bind=127.0.0.1:3093
  if python3 -c 'import socket; s=socket.socket(socket.AF_INET6); s.bind(("::1", 0))' 2>/dev/null; then
    bind="$bind,[::1]:3093"
  fi
  ./target/debug/examples/fake_nip_fa "$bind" http://localhost:3093 "$key" &
  ISSUER_PID=$!
  for _ in $(seq 1 30); do curl -fsS http://localhost:3093/.well-known/jwks.json >/dev/null && break; sleep 1; done
  export BUZZ_NIP_FI_MODE=enforce
  # The test issuer is reached over loopback HTTP, which NIP-FI allows only
  # through the operator-asserted `mesh` JWKS transport. Never do this in
  # production.
  export BUZZ_NIP_FI_ISSUERS='[{"issuer":"http://localhost:3093","command_audiences":["https://localhost:3030"],"token_class":"nip-fi+jwt","algorithms":["ES256"],"skew_seconds":5,"maximum_assertion_age_seconds":600,"jwks_uri":"http://localhost:3093/.well-known/jwks.json","jwks_transport":"mesh","jwks_refresh_interval_seconds":60,"jwks_hard_deadline_seconds":3600,"maximum_command_age_seconds":30,"authorized_principals":["bzz-test-operator"],"public_profile":true,"public_issuer_label":"bzz Test IdP"}]'
  export BUZZ_NIP_FI_COMMUNITIES='[{"canonical_uri":"https://localhost:3030","authorized_issuers":["http://localhost:3093"]}]'
  # Short lifetime so the deadline-reconnect test runs in seconds.
  export BUZZ_NIP_FI_MAX_CONNECTION_LIFETIME_SECS=20
fi
# A tmux server exits immediately when it has no sessions. Keep one disposable
# session alive while setting the global environment; the Buzz helper starts
# the actual relay session afterward.
BZZ_TMUX_KEEPALIVE="bzz-relay-env-$$"
export BZZ_TMUX_KEEPALIVE
tmux new-session -d -s "$BZZ_TMUX_KEEPALIVE" "sleep 3600"
for name in "${RELAY_ENV[@]}"; do
  if [[ -n "${!name:-}" ]]; then tmux set-environment -g "$name" "${!name}"; fi
done
# Buzz's script takes Postgres as ready as soon as it accepts a connection,
# which on its first start is before its database exists: on a busy machine
# the next step then fails with `database "buzz" does not exist`. Bring
# Postgres up here and wait for the database itself, twice in a row, because
# the first start stops and starts the server once more.
harness_compose=(docker compose -p buzz-harness -f "$BZZ_BUZZ_SOURCE/docker-compose.harness.yml")
"${harness_compose[@]}" up -d postgres
ready=0
for _ in $(seq 1 90); do
  if "${harness_compose[@]}" exec -T postgres psql -U buzz -d buzz -c 'select 1' >/dev/null 2>&1; then
    ready=$((ready + 1)); [[ $ready -ge 2 ]] && break
  else
    ready=0
  fi
  sleep 2
done
(cd "$BZZ_BUZZ_SOURCE" && ./scripts/start-isolated-test-relay.sh --profile dev)
for _ in $(seq 1 60); do curl -fsS http://localhost:3030/ >/dev/null && break; sleep 1; done
if [[ "$PROFILE" == nip-fi ]]; then
  cargo test --locked --test relay_nip_fi_integration -- --ignored --test-threads=1
elif [[ "$PROFILE" == herdr ]]; then
  cargo build --locked --release
  cargo build --locked --example harness_fixture
  fixture=$(mktemp -d "${TMPDIR:-/tmp}/bzz-harness-fixture.XXXXXXXX")
  chmod 700 "$fixture"
  ./target/debug/examples/harness_fixture "$fixture/profile" "$BZZ_BUZZ_SOURCE"
  status=0
  BZZ_BIN="$PWD/target/release/bzz" BZZ_HERDR_FIXTURE="$fixture/profile" BZZ_HERDR_SCENARIOS=harness \
    ./scripts/test-tui-herdr.sh || status=$?
  rm -rf -- "$fixture"
  exit "$status"
else
  # The `buzz` CLI at the pin, for sidebar interop (v0.14 M4), and Buzz's
  # own `buzz-acp`, which runs a moved agent from a launch kit (v0.15 M5).
  (cd "$BZZ_BUZZ_SOURCE" && cargo build -p buzz-cli -p buzz-acp)
  # The scripted agent harness (v0.15 M1); `cargo test --test` does not build examples.
  cargo build --locked --example scripted_harness
  # BZZ_RELAY_TEST_FILTER runs only the tests whose name contains it.
  # BZZ_RELAY_NOCAPTURE=1 shows what the tests print: the times they measure.
  BZZ_BUZZ_CLI="$BZZ_BUZZ_SOURCE/target/debug/buzz" \
  BZZ_BUZZ_ACP="$BZZ_BUZZ_SOURCE/target/debug/buzz-acp" \
  BZZ_E2E_RELAY_URL=ws://localhost:3030 cargo test --locked --test relay_integration ${BZZ_RELAY_TEST_FILTER:+"$BZZ_RELAY_TEST_FILTER"} -- --ignored --test-threads=1 ${BZZ_RELAY_NOCAPTURE:+--nocapture}
  # bzz's real agent host against a recorded relay (v0.15 M2). It needs
  # Node.js 22.19 or newer and no relay; it runs here because this is the
  # job that has Node (BZZ_TEST_REQUIRE_NODE=1 turns its skip into a failure).
  if [[ -z "${BZZ_RELAY_TEST_FILTER:-}" ]]; then
    cargo test --locked --test agent_host_test
    # A move with the real host between two data folders (v0.15 M5).
    cargo test --locked --test agent_move_test
  fi
fi
