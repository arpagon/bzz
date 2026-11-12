#!/usr/bin/env bash
# A throwaway shell whose `bzz` uses a private, unlocked gnome-keyring on a
# private D-Bus session bus and a temporary HOME. The user's session bus and
# keychain are never reachable: the outer DBUS_SESSION_BUS_ADDRESS is dropped
# and only the address that dbus-run-session creates is passed on.
set -euo pipefail
# Paths: S = this capture kit, REPO = the bzz checkout, W = the work dir
# (an ignored experiment folder holding buzz-pin/, bin/, fixture/, tapes/).
S=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$S/../../../.." && pwd); W=${PROMO_WORK:-$PWD}
C=$W
T=$(mktemp -d /tmp/bzz-keychain.XXXXXX); chmod 700 "$T"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/run"; chmod 700 "$T/run"
ln -s "$C/bin/bzz-x86_64-unknown-linux-gnu/bzz" "$T/bin/bzz"
unset DBUS_SESSION_BUS_ADDRESS
T="$T" dbus-run-session -- bash -c '
  case "$DBUS_SESSION_BUS_ADDRESS" in *"/run/user/"*) echo "refusing: user session bus" >&2; exit 1;; esac
  exec env -i HOME="$T" XDG_DATA_HOME="$T/share" XDG_RUNTIME_DIR="$T/run" PATH="$T/bin:/usr/bin:/bin" \
    TERM="${TERM:-xterm-256color}" DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" \
    BZZ_CONFIG_DIR="$T/config" BZZ_DATA_DIR="$T/data" BZZ_CACHE_DIR="$T/cache" \
    bash -c "printf demo-keyring | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1
      PS1=\"\[\e[38;2;255;176;0m\]\\$\[\e[0m\] \" exec bash --norc --noprofile -i"'
