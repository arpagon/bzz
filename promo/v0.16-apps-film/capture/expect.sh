#!/usr/bin/env bash
# What each final tape must show, at its marks (the kit's tape-expect.py,
# unchanged: a multi-client tape is an ordinary tape). Usage: expect.sh [take…]
set -uo pipefail
. "$(dirname "$0")/env.sh"
X=$KIT/scripts/tape-expect.py; status=0
want=" ${*:-together poll approvals agent records} "
check() { local t=$C/tapes/$1.jsonl; shift
  if "$X" "$t" "$@" > "$C/run/expect-one.txt" 2>&1; then echo "ok   $(basename "$(dirname "$t")")/$(basename "$t") $1 ${*:2}"
  else echo "FAIL $(basename "$(dirname "$t")")/$(basename "$t") $1 ${*:2}"; tail -30 "$C/run/expect-one.txt"; status=1; fi; }
same() { if "$MKIT/same-board.py" "$@" > "$C/run/expect-one.txt" 2>&1; then echo "ok   same board: ${*#$C/}"; sed 's/^/     /' "$C/run/expect-one.txt" | head -4
  else echo "FAIL same board: $*"; cat "$C/run/expect-one.txt"; status=1; fi; }
if [[ $want == *" together "* ]]; then
# Chapters 0-1: the app beside the chat.
check together/you you-board:seen+0.3 +'Tasks · Ctrl-c leave' +'#team' +'To do (2)' +'Doing (1)' -error
check together/ada lin-board:seen+0.5 +'Tasks · Ctrl-c leave' +'To do (2)' -error
check together/lin lin-board:seen+0.5 +'Tasks · Ctrl-c leave' +'To do (2)' -error
# Chapter 2: Ada's card reaches Lin's and You's Boards.
check together/ada ada-save-0.1 +'Write the release notes'
check together/lin ada-save-0.05 -'Write the release notes'
check together/you ada-save-0.05 -'Write the release notes'
check together/lin lin-sees:seen +'Write the release notes' +'To do (3)'
check together/you you-sees:seen +'Write the release notes' +'To do (3)'
# Chapter 3: the same card moved by two people at the same instant; one is
# saved, the other told of the conflict; the three Boards end the same, the
# card once (which move is saved is the relay's order: not checked).
check together/ada same-moment-0.05 +'> Check the status page' +'Doing (1)'
check together/lin same-moment-0.05 +'> Check the status page' +'Doing (1)'
check together/you same-moment+0.05 +'Doing (1)'
check together/ada settled -'Doing (1)'
same "$C/tapes/together" settled 'Check the status page' ada lin you --conflict
fi
if [[ $want == *" poll "* ]]; then
check poll/lin asked +'Launch day lunch?' +'Tacos' +'Sushi' +'Pizza'
check poll/you voted +'Total: 3 votes' -error
check poll/ada voted +'Total: 3 votes' -error
check poll/lin voted +'Total: 3 votes' -error
fi
if [[ $want == *" approvals "* ]]; then
check approvals/you you-sees:seen +'Order a test Mac mini' +'Ada'
check approvals/ada own-refused:seen +'You cannot decide your own request.'
check approvals/ada decided +'approved' +'Decided by: You' -error
fi
if [[ $want == *" agent "* ]]; then
# Chapters 5-8, the real model: asked, announced, read, approved, in place;
# Ada still on version 1; offered; Ada reads who signed it and approves it.
check agent/you send-0.1 +'@Scout add a priority and a filter to the board'
check agent/you banner:seen+0.2 +'tasks: version 2 from your agent Scout, asked by You waits for your approval' +'> macOS smoke test' -error
check agent/you overlay-shown +'tasks (Tasks), version 2' +'From:     your agent Scout' +'Asked by: You' +'CHANGED from the version that runs' +'Hash:'
check agent/you approve +': the end'
check agent/you in-place +'tasks: version 2 runs in place of 1, its state kept' +'macOS smoke test' -error
check agent/ada ada-still-v1 +'Tasks · Ctrl-c leave' +'To do' -'version 2' -error
check agent/you shared:seen +'is shared in #team'
check agent/ada ada-banner-shown +'tasks: version 2 from You, a person of the channel'
check agent/ada ada-overlay-shown +'From:     You, a person of the channel' +'Offer:    shared in the channel; this exact revision' +'Hash:'
check agent/ada ada-in-place +'tasks: version 2 runs in place of 1' -error
fi
if [[ $want == *" records "* ]]; then
check records/you send-0.1 +'@Scout move the release notes card to done'
check records/lin send-0.1 +'Write the release notes'
check records/lin lin-moved:wait+0.05 +'To do (3)' +'Done (1)'
same "$C/tapes/records" lin-moved:seen 'Write the release notes' you ada lin
same "$C/tapes/records" moved 'Write the release notes' you ada lin
check records/you lin-moved:seen+2.5 +'↳ 1 reply · last reply just' -error   # it wraps at 80 columns: "just / now"
check records/lin moved +'Done (2)' -error
fi
exit $status
