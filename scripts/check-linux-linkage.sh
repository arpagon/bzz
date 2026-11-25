#!/usr/bin/env bash
# check-linux-linkage.sh BINARY: fail if the Linux bzz binary names a sound
# library. Such a library is loaded before bzz's first instruction, so on a
# machine without it bzz could not even answer --version (plan v0.17.0 D3).
# Linux huddle audio speaks the sound server's protocol in pure Rust instead.
set -euo pipefail
binary=${1:?usage: check-linux-linkage.sh BINARY}
needed=$(readelf -d "$binary" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
[[ -n $needed ]] || { echo "readelf found no NEEDED entries in $binary" >&2; exit 2; }
echo "NEEDED: $(printf '%s\n' "$needed" | tr '\n' ' ')"
if sound=$(printf '%s\n' "$needed" | grep -E '^lib(asound|pulse|pulse-simple|pipewire-[0-9.]+|jack|portaudio)\.so'); then
  echo "bzz must not name a sound library; it names: $(printf '%s' "$sound" | tr '\n' ' ')" >&2
  exit 1
fi
