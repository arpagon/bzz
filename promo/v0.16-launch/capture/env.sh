# source: paths and the disposable profile of the v0.16 film's final take.
# Everything a bzz of this take reads or writes is under $R (/tmp, mode 700),
# with a home of its own: no keychain, no coding program (named as a missing
# file). The one thing of the owner's it is given is Pi's folder, as the
# owner allowed for this take (brief M8-C, 4): a link $R/pi to it, passed to
# bzz as PI_CODING_AGENT_DIR, the way bzz passes it to its Pi Durable host.
# Nothing here reads, copies or prints what is in that folder.
# The paths of the recording machine are variables here: BZZ_CHECKOUT,
# FILM_HEAVY, FILM_FIXTURE, PI_DIR_OF_OWNER.
C=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
E=$(cd "$C/.." && pwd)
H=${FILM_HEAVY:?the take's folder for large files}
KIT=${BZZ_CHECKOUT:?the bzz checkout}/.agents/skills/bzz-promo-video
REPO_SRC=$H/src-v016                 # git archive of the tag v0.16.0 (650a648)
PIN=$BZZ_CHECKOUT/runs/buzz-pin
FIX=${FILM_FIXTURE:?promo_fixture built from the tag v0.16.0}
B=$H/release/bzz-x86_64-unknown-linux-gnu/bzz   # the published 0.16.0 archive, checksum verified
R=/tmp/bzz-film16
F=$R/here
PI_DIR_OF_OWNER=${PI_DIR_OF_OWNER:?Pi's own folder, only when its owner allowed the take}
MODEL=openai-codex/gpt-6.1-sol
REAL_NODE=${REAL_NODE:-$(command -v node)}      # 26.5.0 for the take
REAL_NPM=${REAL_NPM:-$(command -v npm)}         # 11.14.1 for the take
LOCK=/tmp/bzz-test-relay-$(id -u).lock
bzz_env() {
  export HOME=$R/home BZZ_CONFIG_DIR=$F/config BZZ_DATA_DIR=$F/data BZZ_CACHE_DIR=$F/cache
  export BZZ_NODE=$R/node/bin/node PI_CODING_AGENT_DIR=$R/pi npm_config_cache=$H/npm-cache
  export BZZ_GROK=$R/absent BZZ_CODEX=$R/absent BZZ_CLAUDE=$R/absent
  export ALSA_CONFIG_PATH=$C/null-asound.conf
  export PATH=$R/node/bin:/usr/bin:/bin
  unset SSH_AUTH_SOCK DBUS_SESSION_BUS_ADDRESS BZZ_AGENT_HOST_SCRIPTED_MODEL
}
