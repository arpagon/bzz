#!/usr/bin/env bash
# Build the fixture programs of the rehearsal from the tag v0.16.0, on cold
# storage, never in the checkout's target: promo_fixture (the community) and
# film_people (this folder's example: the other people's profiles).
set -euo pipefail
E=$(cd "$(dirname "$0")/.." && pwd)
H=${FILM_HEAVY:?the folder of the film for large files}
. "${BZZ_CHECKOUT:?the bzz checkout}/runs/env.sh"   # the toolchain; it cds into the checkout
export CARGO_TARGET_DIR=${FILM_TARGET:?a cargo target directory of your own}
if [[ ! -d $H/src-v016 ]]; then
  mkdir -p "$H/src-v016"
  git -C "$BZZ_CHECKOUT" archive refs/tags/v0.16.0 | tar -x -C "$H/src-v016"
fi
cp "$E/capture/film_people.rs" "$H/src-v016/examples/film_people.rs"
cd "$H/src-v016"
time cargo build --locked --release --example promo_fixture --example film_people
echo BUILD-EXIT $?
