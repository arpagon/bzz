#!/usr/bin/env bash
# The six shots without a model, with the kit's shoot-all.sh (copied to kit/,
# its paths pointed at the tag's fixture, src-v016 and the published binary).
# Run inside session.sh (it holds the relay lock); the kit's setup tears the
# relay down and starts it fresh.
set -euo pipefail
W=$(cd "$(dirname "$0")" && pwd); . "$W/../env.sh"
[[ -e $R/hold ]] || { echo "no session" >&2; exit 2; }
export PROMO_WORK=$W REPO_SRC FIX KITS=$KIT/scripts KITA=$KIT/assets
: "${RUSTUP_HOME:?}" "${CARGO_HOME:?}"   # the toolchain's homes, from the caller
unset CARGO_TARGET_DIR
cd "$W"; rm -rf tapes; exec "$W/kit/shoot-all.sh" </dev/null
