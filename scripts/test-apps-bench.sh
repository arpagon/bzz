#!/usr/bin/env bash
# The apps bench: bzz's app runtime judged with real apps in a real terminal.
#
# Builds `examples/app_host.rs` (the runtime alone in a terminal, on the
# bench's simulated data service) and drives the six fixtures of
# `tests/fixtures/apps/` through the cases of their specs in a
# pseudo-terminal, at the bench's own size (80x24) and at the size of bzz's
# app pane (40x16). Then reload with the six apps, when pushes begin, and
# the hostile script. Prints one table; exit status 0 only if everything passed.
#
# Needs: Linux, Python 3.10 or newer (standard library only), and cargo.
# No Docker, no relay, no network. About seven minutes.
#
#   scripts/test-apps-bench.sh [--release] [--measure] [--saves N] [app ...]
#
#   --release   build and run the release host (default: the debug build)
#   --measure   also measure the Board and the Panel with 200 records
#               (first draw, peak memory, key to screen); use with --release
#   --saves N   saves of each app in the reload test (default 25)
#   app ...     only these of: board poll rota approvals panel board-priority
#               (reload, pushes and the hostile script still run)
#
# APP_HOST_BIN=<path> uses a host that is already built. APPS_BENCH_OUT=<dir>
# is where the runs leave their screens and logs (default: a new folder
# under the system's temporary directory). Nothing is written in the
# repository.
set -uo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bench="$repo/tests/apps-bench"
fixtures="$repo/tests/fixtures/apps"

profile=debug
measure=0
saves=25
apps=()
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile=release ;;
    --measure) measure=1 ;;
    --saves) shift; saves="${1:?--saves needs a number}" ;;
    -h|--help) sed -n '2,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    board|poll|rota|approvals|panel|board-priority) apps+=("$1") ;;
    *) echo "test-apps-bench: unknown argument '$1'" >&2; exit 2 ;;
  esac
  shift
done
[ ${#apps[@]} -gt 0 ] || apps=(board poll rota approvals panel board-priority)

if [ "$(uname -s)" != "Linux" ]; then
  echo "test-apps-bench: the bench drives a pseudo-terminal and reads /proc: Linux only." >&2
  echo "The same cases run everywhere as cargo tests: cargo test --lib apps::" >&2
  exit 2
fi
python3 -c 'import sys; sys.exit(sys.version_info < (3, 10))' 2>/dev/null || {
  echo "test-apps-bench: needs python3, 3.10 or newer" >&2
  exit 2
}

if [ -z "${APP_HOST_BIN:-}" ]; then
  flag=()
  [ "$profile" = release ] && flag=(--release)
  echo "== building examples/app_host ($profile)"
  (cd "$repo" && cargo build --locked --example app_host "${flag[@]}") || exit 2
  target="${CARGO_TARGET_DIR:-$repo/target}"
  APP_HOST_BIN="$target/$profile/examples/app_host"
fi
[ -x "$APP_HOST_BIN" ] || { echo "test-apps-bench: no host at $APP_HOST_BIN" >&2; exit 2; }
export APP_HOST_BIN
export APPS_BENCH_OUT="${APPS_BENCH_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/bzz-apps-bench.XXXXXX")}"
mkdir -p "$APPS_BENCH_OUT"
echo "== host: $APP_HOST_BIN"
echo "== output: $APPS_BENCH_OUT"

failed=0
declare -A cell
sizes=(80x24 40x16)

# The count of a RESULT line: "RESULT ...: 12/12 passed, 0 failed ...".
count() { sed -n 's/^RESULT [^:]*: \([0-9]*\/[0-9]*\) passed.*/\1/p' "$1" | tail -1; }

for app in "${apps[@]}"; do
  for size in "${sizes[@]}"; do
    log="$APPS_BENCH_OUT/run-$app-$size.txt"
    echo "== $app at $size"
    "$bench/run" --app "$app" --option "bzz" --size "$size" -- "$APP_HOST_BIN" "$fixtures/$app" >"$log" 2>&1
    status=$?
    grep -E '^(FAIL|RESULT)' "$log"
    [ $status -eq 0 ] || { failed=1; grep -A40 '^FAIL' "$log" | head -120; }
    cell["$app-$size"]="$(count "$log")"
    [ -n "${cell["$app-$size"]}" ] || cell["$app-$size"]="did not run"
  done
done

for size in "${sizes[@]}"; do
  log="$APPS_BENCH_OUT/reload-$size.txt"
  echo "== reload at $size"
  "$bench/reload" --saves "$saves" --size "$size" >"$log" 2>&1 || failed=1
  grep -E '^(FAIL|RESULT)' "$log"
  grep -A3 '^FAIL' "$log" | grep 'failed:'
  cell["reload-$size"]="$(count "$log")"
  [ -n "${cell["reload-$size"]}" ] || cell["reload-$size"]="did not run"
  cell["saves-$size"]="$(sed -n 's/^RESULT reload.*; \([0-9]* saves.*\)$/\1/p' "$log" | tail -1)"
done

log="$APPS_BENCH_OUT/pushes.txt"
echo "== when pushes begin"
"$bench/pushes" >"$log" 2>&1 || failed=1
grep -E '^(FAIL|RESULT)' "$log"
pushes="$(count "$log")"
[ -n "$pushes" ] || pushes="did not run"

log="$APPS_BENCH_OUT/hostile.txt"
echo "== the hostile script"
"$bench/hostile" >"$log" 2>&1 || failed=1
grep -E '^(FAIL|RESULT)' "$log"
hostile="$(count "$log")"
[ -n "$hostile" ] || hostile="did not run"

if [ $measure -eq 1 ]; then
  for app in board panel; do
    for size in "${sizes[@]}"; do
      echo "== measure $app at $size"
      "$bench/measure" --app "$app" --option "bzz-$profile" --size "$size" -- "$APP_HOST_BIN" "$fixtures/$app" \
        | tee "$APPS_BENCH_OUT/measure-$app-$size.txt" | grep -v '^{' || failed=1
    done
  done
fi

echo
echo "The apps bench on examples/app_host ($profile build)"
echo
printf '| %-34s | %-9s | %-9s |\n' "" "80x24" "40x16"
printf '| %-34s | %-9s | %-9s |\n' "----------------------------------" "---------" "---------"
for app in "${apps[@]}"; do
  printf '| %-34s | %-9s | %-9s |\n' "$app, cases passed" "${cell["$app-80x24"]}" "${cell["$app-40x16"]}"
done
printf '| %-34s | %-9s | %-9s |\n' "reload: edit, swap, bad moment" "${cell["reload-80x24"]}" "${cell["reload-40x16"]}"
printf '| %-34s | %-21s |\n' "when pushes begin, checks passed" "$pushes (80x24)"
printf '| %-34s | %-21s |\n' "the hostile script, checks passed" "$hostile (120x40)"
echo
for size in "${sizes[@]}"; do
  [ -n "${cell["saves-$size"]}" ] && echo "reload at $size: ${cell["saves-$size"]}"
done
echo
if [ $failed -eq 0 ]; then
  echo "PASS: everything passed. Screens and logs: $APPS_BENCH_OUT"
else
  echo "FAIL: see above. Screens and logs: $APPS_BENCH_OUT"
fi
exit $failed
