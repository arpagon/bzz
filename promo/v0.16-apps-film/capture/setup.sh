#!/usr/bin/env bash
# Fresh pinned relay + the films' generated community (Ada, Lin, Sam, You in
# "Studio"; #team owned by You) + a bzz profile for Ada and for Lin + Scout,
# a local agent of You on bzz's Pi Durable host with the REAL MODEL
# (openai-codex/gpt-6.1-sol, Pi's own sign-in), in #team, with NO folder
# + the three templates installed by each person at their own shell (Board
# as `tasks`, Poll as `poll`, Approvals as `approvals`), each approved by
# its owner. Nothing under $R holds a Board with priority.
# Run inside session.sh (it holds the relay harness lock).
set -euo pipefail
. "$(dirname "$0")/env.sh"
[[ -e $R/lock.held ]] || { echo "no lock: run session.sh start" >&2; exit 2; }
unset CARGO_TARGET_DIR
export PROMO_WORK=$C
mkdir -p -m 700 "$C/run"
ln -sfn "$PIN" "$C/buzz-pin"
tmux kill-session -t dawn-relay 2>/dev/null || true
(cd "$PIN" && docker compose -p buzz-harness -f docker-compose.harness.yml down -v >/dev/null 2>&1) || true
"$KIT/capture/start-relay.sh" > "$C/run/start-relay.log" 2>&1
(umask 077; tmux list-panes -t dawn-relay -F '#{pane_start_command}' | sed 's/^"//; s/"$//' > "$R/relay.cmd")

find "$R" -mindepth 1 -maxdepth 1 ! -name hold ! -name lock.held ! -name relay.cmd -exec rm -rf {} +
mkdir -m 700 "$R/home" "$R/pi-none" "$R/node" "$R/node/bin" "$R/apps"
ln -s "$PI_DIR_OF_OWNER" "$R/pi"
for who in $PEOPLE_NAMES; do mkdir -m 700 "$R/home/$who"; done
mkdir -p "$H/npm-cache"
printf '#!/bin/sh\nexec %s "$@"\n' "$REAL_NODE" > "$R/node/bin/node"
printf '#!/bin/sh\nexec %s "$@"\n' "$REAL_NPM" > "$R/node/bin/npm"
chmod 755 "$R/node/bin/node" "$R/node/bin/npm"

"$FIX" seed "$F" "$PIN" "$KIT/assets/avatars"
"$PEOPLE" profile "$F" ada "$R/ada"
"$PEOPLE" profile "$F" lin "$R/lin"
for who in $PEOPLE_NAMES; do
  sed -i 's/^thread_width = .*/thread_width = 56/' "$(profile_of "$who")/config/config.toml" || true
done

python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" you agent new Scout --harness pi-durable --model "$MODEL" \
  --instructions "You are Scout, the team's assistant. Answer in one or two short lines." --no-packages --yes
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" you agent add Scout team

"$FIX" history "$F" "$REPO_SRC/site/public/assets/og.png"

for who in $PEOPLE_NAMES; do
  P=$(profile_of "$who")
  for pair in tasks:board poll:poll approvals:approvals; do
    name=${pair%%:*}; template=${pair##*:}
    "$C/bzz.sh" "$who" app new "$name" --template "$template" --to "$R/apps/$who-$name"
    python3 "$C/answer.py" y "$P/passphrase" -- "$C/bzz.sh" "$who" app add "$R/apps/$who-$name"
  done
  "$C/bzz.sh" "$who" app list
done
rm -rf "$R/apps"
"$C/bzz.sh" you agent show Scout
echo "fixture ready: $R"
