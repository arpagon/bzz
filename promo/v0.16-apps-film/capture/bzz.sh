#!/usr/bin/env bash
# bzz.sh <you|ada|lin> [args…]: the published 0.16.0 binary as that generated
# person, in their own profile and home, with their disposable passphrase on
# descriptor 3. The multi-recorder runs one of these per pane.
. "$(dirname "$0")/env.sh"
who=$1; shift
bzz_env "$who"
export BZZ_PASSPHRASE_FD=3
cd "$R/home/$who"
exec "$B" "$@" 3<"$(profile_of "$who")/passphrase"
