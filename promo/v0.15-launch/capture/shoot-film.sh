#!/usr/bin/env bash
# The agent shots of the v0.15 launch film: one story on a fresh fixture, with
# the final release build and a real model (openai-codex/gpt-6-luna through
# Pi's own sign-in; six short turns). Node is found at a neutral path, so no
# path of the recording machine is in a tape.
set -uo pipefail
C=$(cd "$(dirname "$0")" && pwd); E=$(cd "$C/.." && pwd); cd "$E"
X=scripts/expect; T=capture/tapes; status=0
export BZZ_NODE=/tmp/bzz-node/bin/node
rm -f $T/*.jsonl
capture/setup.sh > capture/setup.log 2>&1 || { echo "setup failed"; tail -5 capture/setup.log; exit 1; }
rm -f $T/x-*.jsonl
python3 capture/make-shots.py >/dev/null
tui() { capture/shoot.sh "$1" || { echo "SHOT FAILED: $1"; status=1; }; }
sh_() { SHELL_SHOT=1 COLS=100 ROWS=24 capture/shoot.sh "$1" || { echo "SHOT FAILED: $1"; status=1; }; }
check() { "$X" "$@" || status=1; }
tui 02-create
tui 03-restart
sh_ 05-grant
tui 06-hands
sh_ 11-move
PROFILE=there sh_ 12-receive
PROFILE=there tui 13-there
check $T/02-create.jsonl checked +'Node.js 26.5.0 at /tmp/bzz-node/bin/node' +'model openai-codex/gpt-6-luna' +"with Pi's own sign-in" -arpagon
check $T/02-create.jsonl created +'created in Studio' +'capability record: pi-durable'
check $T/02-create.jsonl panel +'Scout · pi-durable · idle'
check $T/02-create.jsonl shown +'context · 1 reply' +'Scout · managed by you' +'391' -error
check $T/03-restart.jsonl memory-shown +'marigold' +'launch.txt' -error
check $T/03-restart.jsonl git-shown +'first commit' -error
check $T/06-hands.jsonl banner +'Scout asks to run a command in /mnt/site' +'Ctrl-Y to see and answer it'
check $T/06-hands.jsonl dialog +'./test.sh' +'that is the whole command'
check $T/06-hands.jsonl shown +'context · 1 reply' +'3 checks passed' -error
check $T/06-hands.jsonl refused +'context · 3 replies' -error
check $T/11-move.jsonl moved:seen +'sealed with your passphrase' +'Scout is marked moved away here'
check $T/12-receive.jsonl end +'the passphrase is wrong' +'received in Studio'
check $T/13-there.jsonl shown +'context · 7 replies' +'marigold' +'hello' -error
exit $status
