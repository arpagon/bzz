#!/usr/bin/env bash
# Fresh pinned relay + the generated community of the films (Ada, Lin, Sam,
# You in "Studio", #team owned by You) + a local agent Scout on bzz's Pi
# Durable host with a REAL MODEL (openai-codex/gpt-6.1-sol, Pi's own sign-in)
# + the app `tasks` (the Board template) installed and approved by You
# + a tiny generated project, site/, granted to Scout only for line 7.
# Nothing under $R holds a Board with priority. Run inside session.sh.
set -euo pipefail
. "$(dirname "$0")/env.sh"
unset CARGO_TARGET_DIR
export PROMO_WORK=$C
mkdir -p -m 700 "$C/run" "$C/tapes"
ln -sfn "$PIN" "$C/buzz-pin"
tmux kill-session -t dawn-relay 2>/dev/null || true
(cd "$PIN" && docker compose -p buzz-harness -f docker-compose.harness.yml down -v >/dev/null 2>&1) || true
"$KIT/capture/start-relay.sh" > "$C/run/start-relay.log" 2>&1
(umask 077; tmux list-panes -t dawn-relay -F '#{pane_start_command}' | sed 's/^"//; s/"$//' > "$C/run/relay.cmd")

# The disposable profile, its home, a neutral Node, Pi's folder by a link.
find "$R" -mindepth 1 -maxdepth 1 ! -name hold -exec rm -rf {} +
mkdir -m 700 "$R/home" "$R/node" "$R/node/bin"
ln -s "$PI_DIR_OF_OWNER" "$R/pi"
printf '#!/bin/sh\nexec %s "$@"\n' "$REAL_NODE" > "$R/node/bin/node"
printf '#!/bin/sh\nexec %s "$@"\n' "$REAL_NPM" > "$R/node/bin/npm"
chmod 755 "$R/node/bin/node" "$R/node/bin/npm"
mkdir -p "$H/npm-cache"
# The project for line 7 (as v0.15's): its checks, nothing else.
mkdir -p "$R/site/dist"
printf '#!/bin/sh\n# the checks of the site\necho "links ......... ok"\necho "spelling ...... ok"\necho "build ......... ok"\necho "3 checks passed"\n' > "$R/site/test.sh"
chmod +x "$R/site/test.sh"; echo '<h1>Studio</h1>' > "$R/site/dist/index.html"; echo '# Studio site' > "$R/site/README.md"

"$FIX" seed "$F" "$PIN" "$KIT/assets/avatars"
sed -i 's/^thread_width = .*/thread_width = 56/' "$F/config/config.toml" || true

# Scout: a local agent of You, on the real model, in #team. No folder yet.
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" agent new Scout --harness pi-durable --model "$MODEL" \
  --instructions "You are Scout, the team's assistant. Answer in one or two short lines." --no-packages --yes
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" agent add Scout team

"$FIX" history "$F" "$REPO_SRC/site/public/assets/og.png"

"$C/bzz.sh" app new tasks --template board --to "$R/tasks"
python3 "$C/answer.py" y "$F/passphrase" -- "$C/bzz.sh" app add "$R/tasks"
rm -rf "$R/tasks"
"$C/bzz.sh" app list
echo "fixture ready: $R"
