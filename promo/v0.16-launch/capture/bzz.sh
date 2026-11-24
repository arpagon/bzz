#!/usr/bin/env bash
# bzz.sh [args…]: the published 0.16.0 binary in the take's own environment,
# with the disposable passphrase on descriptor 3. The recorder runs this.
. "$(dirname "$0")/env.sh"
bzz_env
export BZZ_PASSPHRASE_FD=3
cd "$R"
exec "$B" "$@" 3<"$F/passphrase"
