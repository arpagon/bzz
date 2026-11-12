#!/usr/bin/env bash
# Fresh fixture, then every shot in film order, then on-screen expectations.
set -euo pipefail
# Paths: S = this capture kit, REPO = the bzz checkout, W = the work dir
# (an ignored experiment folder holding buzz-pin/, bin/, fixture/, tapes/).
S=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$S/../../../.." && pwd); W=${PROMO_WORK:-$PWD}
C=$W; cd "$C"
SK=$S/../scripts
E="$SK/tape-expect.py"
[ "${SKIP_SETUP:-0}" = 1 ] || "$S/setup-fixture.sh"
"$S/shoot.sh" overview live 8
"$S/shoot.sh" keyboard
(sleep 7.7; tmux kill-session -t dawn-relay) & "$S/shoot.sh" offline; wait; "$S/restart-relay.sh"
"$S/shoot.sh" agent agent-reply 0
"$S/shoot.sh" broker
"$S/shoot.sh" media
"$S/shoot.sh" huddle huddle 0 26
CMD="$S/identity-shell.sh" COLS=130 ROWS=14 START=1.5 "$S/shoot.sh" identity
T=tapes; status=0
check() { "$E" "$@" || status=1; }
check $T/overview.jsonl ready+0.2 +online +'#team' -error
check $T/overview.jsonl end +'Launch post is scheduled for 9am.' +'Status page is ready too.' -error
check $T/keyboard.jsonl thread+2 +'context · 2 replies' +'🎉 1' -error
check $T/offline.jsonl cut-0.3 +online -offline
check $T/offline.jsonl open-0.2 +'offline · local results' +'Load test passed.'
check $T/agent.jsonl pick-0.1 +'@Scout' +'remote agent · eligible'
check $T/agent.jsonl end +'◆ Scout · managed by you' +'One left: the macOS smoke test.' -error
check $T/broker.jsonl toggle+0.9 +'2[ ] message.post' -error
check $T/broker.jsonl pause+0.5 +'scout · paused'
check $T/broker.jsonl end +'online' -error
check $T/media.jsonl media +'launch-card.png' +'Looks great' -error
check $T/media.jsonl themes+1.5 +'themes ·'
check $T/huddle.jsonl end +'3 in it' +'LIVE (others hear you)' -restricted -error
check $T/identity.jsonl end +'Keychain' +'work'
exit $status
