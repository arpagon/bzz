#!/usr/bin/env bash
# The order the final session was run in (each step was run by hand, so that
# a real model's turn is never spent by a script that went on after a
# failure): session.sh start; make-shots.py; the takes warmup, together,
# poll, approvals (no model); expect.sh for them; agent (the real model, one
# turn), after-agent.sh, expect.sh agent; a pause of two minutes (so that
# "last reply just now" in #team is the records turn's own); DONE_BEFORE from
# the together take; records (the real model, one turn), after-records.sh,
# expect.sh records; scan.sh; session.sh stop.
set -uo pipefail
C=$(cd "$(dirname "$0")" && pwd)
step() { echo "--- $(date -u +%T) $*"; "$@"; }
step "$C/session.sh" start || exit 1
step python3 "$C/make-shots.py"
for take in warmup together poll approvals; do step "$C/shoot.sh" "$take" || exit 1; done
step "$C/expect.sh" together poll approvals
step "$C/shoot.sh" agent; step "$C/after-agent.sh"; step "$C/expect.sh" agent
sleep 120
export DONE_BEFORE=$("$C/done-before.py")
step python3 "$C/make-shots.py"
step "$C/shoot.sh" records; step "$C/after-records.sh"; step "$C/expect.sh" records
step "$C/scan.sh"
step "$C/session.sh" stop
