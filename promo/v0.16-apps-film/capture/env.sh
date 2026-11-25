# source: paths and the disposable profiles of the apps film's final takes.
# Three generated people, each a bzz of their own: You (the viewer of
# promo_fixture, owner of the agent Scout), Ada and Lin, under $R (/tmp, mode
# 700), each with a home of their own: no keychain, no coding program (named
# as a missing file), no session bus, an encrypted-file identity.
# The model is REAL in the takes agent and records: Scout on bzz's Pi Durable
# host with openai-codex/gpt-6.1-sol through the owner's Pi sign-in, as the
# owner allowed (brief C2, 5). The one thing of the owner's a bzz is given is
# Pi's folder: a link $R/pi to it, passed to You's bzz as PI_CODING_AGENT_DIR.
# Nothing here reads, lists, copies or prints what is in that folder. Ada's
# and Lin's bzz get an empty folder instead.
# The machine's paths are variables: BZZ_CHECKOUT, FILM_HEAVY, FILM_BUILD,
# PI_DIR_OF_OWNER, REAL_NODE, REAL_NPM (local.env sets them, when present).
C=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
E=$(cd "$C/.." && pwd)
[[ -f $C/local.env ]] && . "$C/local.env"
H=${FILM_HEAVY:?the folder of the film for large files}
KIT=${BZZ_CHECKOUT:?the bzz checkout}/.agents/skills/bzz-promo-video
MKIT=${FILM_KIT:-$C/kit}                      # record-multi.py, same-board.py, first-seen.py
PIN=$BZZ_CHECKOUT/runs/buzz-pin
REPO_SRC=$H/src-v016                         # git archive of refs/tags/v0.16.0 (650a648)
FIX=${FILM_BUILD:?promo_fixture and film_people built from the tag}/promo_fixture
PEOPLE=$FILM_BUILD/film_people
B=$H/release/bzz-x86_64-unknown-linux-gnu/bzz   # the published 0.16.0 archive, sha256 97d02b47…
R=/tmp/bzz-fa-c2
F=$R/here                                    # You: promo_fixture's root
MODEL=openai-codex/gpt-6.1-sol
REAL_NODE=${REAL_NODE:-$(command -v node)}
REAL_NPM=${REAL_NPM:-$(command -v npm)}
LOCK=/tmp/bzz-test-relay-$(id -u).lock
PEOPLE_NAMES="you ada lin"
profile_of() { case "$1" in you) echo "$F";; *) echo "$R/$1";; esac; }
bzz_env() {   # bzz_env <who>
  local P; P=$(profile_of "$1")
  export HOME=$R/home/$1 BZZ_CONFIG_DIR=$P/config BZZ_DATA_DIR=$P/data BZZ_CACHE_DIR=$P/cache
  if [[ $1 == you ]]; then export PI_CODING_AGENT_DIR=$R/pi; else export PI_CODING_AGENT_DIR=$R/pi-none; fi
  export BZZ_NODE=$R/node/bin/node npm_config_cache=$H/npm-cache
  export BZZ_GROK=$R/absent BZZ_CODEX=$R/absent BZZ_CLAUDE=$R/absent
  export ALSA_CONFIG_PATH=$C/null-asound.conf
  export PULSE_SERVER=${BZZ_CAPTURE_PULSE_SERVER:-unix:$C/no-sound-server/native}
  export PATH=$R/node/bin:/usr/bin:/bin
  unset SSH_AUTH_SOCK DBUS_SESSION_BUS_ADDRESS DISPLAY WAYLAND_DISPLAY BZZ_AGENT_HOST_SCRIPTED_MODEL
}
