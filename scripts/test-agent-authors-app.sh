#!/usr/bin/env bash
# v0.16 M5's last gate, prepared: an agent changes the open Board on a
# mention, and what it proposed passes the bench's board-priority cases.
#
#   scripts/test-agent-authors-app.sh <harness>
#
#   <harness>  scripted     the scripted harness stands in for the model
#                           (examples/scripted_harness.rs): it proves this
#                           script and the gate's plumbing, not a model
#              pi-durable | grok | codex | claude-code
#                           the owner's own program and sign-in, with a real
#                           model. Only when the owner allowed that exact run
#                           (plan D13): set BZZ_AUTHOR_ALLOWED=yes to say so.
#                           BZZ_AUTHOR_MODEL=provider/model names the model
#                           (and BZZ_AUTHOR_THINKING its level); without it the
#                           harness uses its own default, which for Pi must be
#                           a provider this host can load.
#
# A real harness must not find the answer on the disk. Grok and Codex read
# whatever their owner can read (docs/agents.md, "Reads"), and this
# repository holds a Board with priority that passes the bench: the first
# run of grok proposed that file, byte for byte. So while a real harness
# runs, the files of tests/fixtures/apps/board-priority and of
# tests/apps-bench are made unreadable (mode 000), and so is every file or
# folder named in BZZ_AUTHOR_HIDE (colon separated): other checkouts of this
# repository, earlier outputs of this script. They get their modes back
# before the bench runs, and when the script ends for any reason but a kill.
# Not hidden: git's objects, and what the harness's own program remembers.
#
# What it does, on the pinned relay (scripts/test-relay.sh, which takes its
# lock and tears the relay down after):
#
# 1. A disposable profile: an owner identity, an agent Scout on <harness>
#    with the `bot` role in a new private channel, and the Board installed,
#    approved and open there with three cards
#    (tests/relay_integration.rs,
#    real_relay_an_agent_changes_the_open_board_and_is_told_it_was_applied).
# 2. The owner mentions the agent with the change request of
#    tests/apps-bench/spec/board-priority.md. The agent has bzz's tools:
#    app_guide, apps, try_app, propose_app. One reply per mention is
#    checked on the relay.
# 3. The version it proposed waits, from the agent, asked by the owner; it
#    is approved as the overlay approves it, and the agent's next turn is
#    told. Its script, brief and manifest go to $BZZ_AUTHOR_OUT/approved.
# 4. The bench (tests/apps-bench) runs the board-priority cases against
#    the approved script on examples/app_host, at 80x24 and at 40x16.
#    The brief is printed: whoever reads the result judges whether it says
#    what the script now does.
#
# Needs: BZZ_BUZZ_SOURCE (the pinned block/buzz checkout, built), Docker,
# Python 3.10 or newer, Linux. BZZ_AUTHOR_OUT: where the approved version
# and the bench's screens go (default: a new temporary folder). Exit status
# 0 only if the relay test and every bench case passed.
set -euo pipefail

harness="${1:-}"
case "$harness" in
  scripted) ;;
  pi-durable|grok|codex|claude-code)
    if [[ "${BZZ_AUTHOR_ALLOWED:-}" != yes ]]; then
      echo "test-agent-authors-app: $harness runs a real model with the owner's sign-in." >&2
      echo "Run it only when the owner allowed this run (plan D13), with BZZ_AUTHOR_ALLOWED=yes." >&2
      exit 2
    fi
    ;;
  -h|--help)
    sed -n '2,37p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  "") echo "usage: scripts/test-agent-authors-app.sh <scripted|pi-durable|grok|codex|claude-code>" >&2; exit 2 ;;
  *) echo "test-agent-authors-app: unknown harness '$harness'" >&2; exit 2 ;;
esac
: "${BZZ_BUZZ_SOURCE:?set BZZ_BUZZ_SOURCE to the pinned block/buzz checkout}"
if [[ "$(uname -s)" != Linux ]]; then
  echo "test-agent-authors-app: the bench drives a pseudo-terminal: Linux only" >&2
  exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${BZZ_AUTHOR_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/bzz-agent-authors-app.XXXXXX")}"
mkdir -p "$out"
rm -rf -- "$out/approved"
export BZZ_AUTHOR_HARNESS="$harness" BZZ_AUTHOR_OUT="$out"
echo "== harness: $harness"
echo "== output: $out"

# Out of a real harness's sight: the answer and the bench (see the top).
hidden_list="$out/hidden.txt"
: >"$hidden_list"
show_again() {
  [[ -s "$hidden_list" ]] || return 0
  while read -r mode path; do chmod "$mode" "$path" 2>/dev/null || true; done <"$hidden_list"
  : >"$hidden_list"
}
trap show_again EXIT
if [[ "$harness" != scripted ]]; then
  # Built first: once the files cannot be read, nothing may need compiling.
  (cd "$repo" && cargo build --locked --example scripted_harness \
    && cargo test --locked --test relay_integration --no-run) >"$out/build.txt" 2>&1 \
    || { echo "FAIL: the build failed (see $out/build.txt)"; exit 1; }
  hide() {
    printf '%s %s\n' "$(stat -c %a "$1")" "$1" >>"$hidden_list"
    chmod 000 "$1"
  }
  while IFS= read -r -d '' file; do hide "$file"; done < <(find \
    "$repo/tests/fixtures/apps/board-priority" "$repo/tests/apps-bench" -type f -print0)
  IFS=: read -r -a more <<<"${BZZ_AUTHOR_HIDE:-}"
  for path in "${more[@]}"; do
    [[ -e "$path" && "$(stat -c %a "$path")" != 0 ]] && hide "$path"
  done
  echo "== hidden from $harness while it runs: $(wc -l <"$hidden_list") files and folders"
fi

echo "== the agent changes the open Board, on the pinned relay"
set +e
(cd "$repo" && BZZ_RELAY_TEST_FILTER=real_relay_an_agent_changes_the_open_board \
  BZZ_RELAY_NOCAPTURE=1 ./scripts/test-relay.sh) 2>&1 | tee "$out/relay.txt"
status=${PIPESTATUS[0]}
set -e
if [[ $status -ne 0 || ! -f "$out/approved/app.lua" ]]; then
  echo "FAIL: the relay test failed, or no version was approved (see $out/relay.txt)"
  exit 1
fi

show_again
if [[ "$harness" != scripted ]]; then
  if cmp -s "$out/approved/app.lua" "$repo/tests/fixtures/apps/board-priority/app.lua"; then
    echo "FAIL: $harness proposed this repository's own fixture, byte for byte: it found it, it did not write it"
    exit 1
  fi
fi

echo "== the approved brief"
sed 's/^/  | /' "$out/approved/brief.md"

echo "== building examples/app_host"
(cd "$repo" && cargo build --locked --example app_host)
host="${CARGO_TARGET_DIR:-$repo/target}/debug/examples/app_host"
export APPS_BENCH_OUT="$out/bench"
mkdir -p "$APPS_BENCH_OUT"
failed=0
for size in 80x24 40x16; do
  log="$out/bench-$size.txt"
  echo "== board-priority at $size"
  "$repo/tests/apps-bench/run" --app board-priority --option "agent-$harness" --size "$size" \
    -- "$host" "$out/approved" >"$log" 2>&1 || failed=1
  grep -E '^(FAIL|RESULT)' "$log" || failed=1
done
echo
if [[ $failed -eq 0 ]]; then
  echo "PASS: $harness changed the Board, and the approved version passed every board-priority case. Output: $out"
else
  echo "FAIL: see above. Output: $out"
fi
exit $failed
