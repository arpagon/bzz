#!/usr/bin/env bash
# What the final take's tapes must show, at their marks (the kit's tape-expect.py).
set -uo pipefail
. "$(dirname "$0")/env.sh"
X=$KIT/scripts/tape-expect.py; status=0
check() { local t=$C/tapes/$1.jsonl; shift
  "$X" "$t" "$@" > /dev/null 2>"$C/run/expect-fail.txt" && echo "ok   $(basename "$t") $1 ${*:2}" || {
    echo "FAIL $(basename "$t") $1 ${*:2}"; cat "$C/run/expect-fail.txt"; "$X" "$t" "$@" | tail -40; status=1; }; }
# Line 6: the mention, and the reply in the thread.
check mention send-0.1 +'@Scout what is 17 times 23?'
check mention answer:seen+0.2 +'@Scout what is 17 times 23?' +'↳ 1 reply' -error
check mention shown +'context · 1 reply' +'Scout · managed by you' +'391' -error
# Line 7: the agent asks to run a command; Ctrl-Y shows it; a allows it; the result.
check hands banner +'Scout asks to run a command in /mnt/site' +'Ctrl-Y to see and answer it'
check hands dialog +'a real command waits for your answer' +'./test.sh' +'that is the whole command'
check hands allowed+0.3 +'allowed: Scout runs its command in /mnt/site'
check hands shown +'context · 1 reply' +'3 checks passed' -error
# N1: the app beside the chat, a card selected, the agent asked, its version announced.
check app opened:seen+0.6 +'Tasks · Ctrl-c leave' +'To do (3)' +'#team' +'Loaded 3 cards.' -error
check app selected:seen+0.3 +'> macOS smoke test' -error
check app send-0.1 +'@Scout add a priority and a filter to the board'
check app banner:seen+0.2 +'tasks: version 2 from your agent Scout, asked by You waits for your approval' +'> macOS smoke test' -'Priority filter' -error
check app banner-shown +'↳ 1 reply · last reply just now' +'> macOS smoke test' -'Priority filter' -error
# N2: read in the overlay, approved, the open app changed in place with its state.
check app overlay-shown +'tasks (Tasks), version 2' +'From:     your agent Scout' +'Asked by: You' +'CHANGED from the version that runs' +'Record types: card' +'The same as the version that runs.'
check app approve +': the end'
check app in-place +'tasks: version 2 runs in place of 1, its state kept' +'Priority filter: all' +'> [N] macOS smoke test' +'To do (3)' -error
exit $status
