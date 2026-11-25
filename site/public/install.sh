#!/bin/sh
# bzz installer, served at https://getbzz.dev/install.sh
#
#   curl -fsSL https://getbzz.dev/install.sh | sh
#
# It checks this machine first and says in one line what is the matter and
# what to do, then runs the release's own installer (made by cargo-dist; it
# checks the archive's SHA-256 and installs bzz in ~/.cargo/bin). Arguments
# are passed on to that installer, for example `sh -s -- --quiet`.
#
#   BZZ_CHECK_ONLY=1   run the checks, print their answer, install nothing
#   BZZ_VERSION=0.17.0 install that release instead of the latest
#
# Source: https://github.com/arpagon/bzz/blob/main/site/public/install.sh
set -u

# What the Linux binary of the release being served needs to make sound.
#   server: bzz speaks to PipeWire or PulseAudio itself and needs no sound
#           library to start (v0.17.0 and later). Only a note if no sound
#           server is found.
#   alsa:   bzz is linked to ALSA's libasound.so.2 and does not start without
#           it (v0.16.0). The check names the package for this distribution.
BZZ_SOUND=server

REPO=https://github.com/arpagon/bzz
MIN_GLIBC_MAJOR=2
MIN_GLIBC_MINOR=34

say() { printf '%s\n' "$*"; }
stop() {
    printf 'bzz: %s\n' "$*" >&2
    exit 1
}

# One field of /etc/os-release, without quotes; empty when it is absent.
os_field() {
    [ -r /etc/os-release ] || return 0
    sed -n "s/^$1=//p" /etc/os-release | head -n 1 | tr -d "\"'"
}

# The package manager of this distribution: apt, dnf, pacman, zypper or empty.
package_manager() {
    case " $(os_field ID) $(os_field ID_LIKE) " in
        *" ubuntu "* | *" debian "*) say apt ;;
        *" fedora "* | *" rhel "* | *" centos "* | *" rocky "* | *" almalinux "* | *" amzn "*) say dnf ;;
        *" arch "* | *" manjaro "*) say pacman ;;
        *" suse "* | *" opensuse "*) say zypper ;;
        *) say "" ;;
    esac
}

# The name of a package here: `package <apt> <dnf> <pacman> <zypper>`.
package() {
    case "$PM" in
        apt) say "$1" ;;
        dnf) say "$2" ;;
        pacman) say "$3" ;;
        zypper) say "$4" ;;
        *) say "" ;;
    esac
}

# The command that installs these packages here; empty if unknown.
install_cmd() {
    case "$PM" in
        apt) say "sudo apt install $*" ;;
        dnf) say "sudo dnf install $*" ;;
        pacman) say "sudo pacman -S $*" ;;
        zypper) say "sudo zypper install $*" ;;
        *) say "" ;;
    esac
}

# ALSA's libasound.so.2. Ubuntu 24.04 and Debian 13 renamed the Debian
# package for 64-bit time.
alsa_package() {
    id=$(os_field ID)
    version=$(os_field VERSION_ID)
    major=${version%%.*}
    deb=libasound2
    case "$id" in
        ubuntu) [ "${major:-0}" -ge 24 ] 2>/dev/null && deb=libasound2t64 ;;
        debian) { [ -z "$major" ] || [ "$major" -ge 13 ] 2>/dev/null; } && deb=libasound2t64 ;;
    esac
    package "$deb" alsa-lib alsa-lib libasound2
}

has_libasound() {
    if command -v ldconfig >/dev/null 2>&1 && ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2'; then
        return 0
    fi
    for f in /usr/lib/libasound.so.2 /usr/lib64/libasound.so.2 \
        /usr/lib/*-linux-gnu/libasound.so.2 /lib/*-linux-gnu/libasound.so.2; do
        [ -e "$f" ] && return 0
    done
    return 1
}

has_sound_server() {
    [ -n "${PULSE_SERVER:-}" ] && return 0
    [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -S "$XDG_RUNTIME_DIR/pulse/native" ] && return 0
    for p in pipewire pipewire-pulse pulseaudio; do
        command -v "$p" >/dev/null 2>&1 && return 0
    done
    return 1
}

# The C library's version, "2.39"; empty when it cannot be read.
glibc_version() {
    v=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{ print $2 }')
    if [ -z "$v" ] && command -v ldd >/dev/null 2>&1; then
        v=$(ldd --version 2>/dev/null | head -n 1 | awk '{ print $NF }')
    fi
    case "$v" in
        [0-9]*.[0-9]*) say "$v" ;;
        *) say "" ;;
    esac
}

is_musl() {
    for f in /lib/ld-musl-*; do
        [ -e "$f" ] && return 0
    done
    command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl
}

# Lines to print after the install (and with BZZ_CHECK_ONLY), set by the checks.
NOTE=""

check_linux() {
    case "$(uname -m)" in
        x86_64 | amd64 | aarch64 | arm64) ;;
        *) stop "there is no Linux download for $(uname -m) yet, only x86_64 and arm64; build it from source with: cargo install --git $REPO --locked" ;;
    esac

    if is_musl; then
        stop "there is no download for musl systems such as Alpine yet: run bzz on a glibc system (glibc $MIN_GLIBC_MAJOR.$MIN_GLIBC_MINOR or newer) or in a container such as debian:stable-slim."
    fi

    glibc=$(glibc_version)
    if [ -n "$glibc" ]; then
        major=${glibc%%.*}
        minor=${glibc#*.}
        minor=${minor%%.*}
        if [ "$major" -lt "$MIN_GLIBC_MAJOR" ] ||
            { [ "$major" -eq "$MIN_GLIBC_MAJOR" ] && [ "$minor" -lt "$MIN_GLIBC_MINOR" ]; }; then
            stop "this system has glibc $glibc and bzz needs $MIN_GLIBC_MAJOR.$MIN_GLIBC_MINOR or newer: use Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9 or newer, or a container of one."
        fi
    fi

    PM=$(package_manager)
    need_alsa=""
    if [ "$BZZ_SOUND" = alsa ] && ! has_libasound; then
        need_alsa=$(alsa_package)
    elif [ "$BZZ_SOUND" != alsa ] && ! has_sound_server; then
        NOTE="Note: huddle audio needs PipeWire or PulseAudio, and neither was found here; everything else in bzz works without them."
    fi

    # The release's installer unpacks a .tar.xz with tar, which calls xz.
    missing=""
    packages=""
    for tool in tar xz; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            missing="$missing $tool"
            if [ "$tool" = xz ]; then
                packages="$packages $(package xz-utils xz xz xz)"
            else
                packages="$packages $(package tar tar tar tar)"
            fi
        fi
    done
    if [ -n "$missing" ]; then
        words=$(say "${missing# }" | sed 's/ / and /')
        # One command for everything: what unpacking needs, and the sound
        # library too when this bzz does not start without it.
        cmd=$(install_cmd "${packages# }${need_alsa:+ $need_alsa}")
        why=""
        [ -n "$need_alsa" ] && [ -n "$cmd" ] && why=" ($need_alsa is the sound library bzz needs to start)"
        stop "this system has no $words, which the download needs to unpack: ${cmd:-install $words}, then run this installer again${why}."
    fi

    if [ -n "$need_alsa" ]; then
        NOTE="bzz needs ALSA's sound library libasound.so.2 to start, and it is not installed: $(install_cmd "$need_alsa")"
    elif [ "$BZZ_SOUND" = alsa ] && ! has_libasound; then
        NOTE="bzz needs ALSA's sound library libasound.so.2 to start, and it is not installed: install your distribution's ALSA library package."
    fi
}

case "$(uname -s)" in
    Linux) check_linux ;;
    Darwin) ;;
    MINGW* | MSYS* | CYGWIN*) ;;
    *) stop "there is no download for $(uname -s); build it from source with: cargo install --git $REPO --locked" ;;
esac

if [ "${BZZ_CHECK_ONLY:-0}" = 1 ]; then
    [ -n "$NOTE" ] && say "$NOTE"
    say "bzz: checks done; nothing installed (BZZ_CHECK_ONLY=1)"
    exit 0
fi

if [ -n "${BZZ_VERSION:-}" ]; then
    url="$REPO/releases/download/v${BZZ_VERSION#v}/bzz-installer.sh"
else
    url="$REPO/releases/latest/download/bzz-installer.sh"
fi

tmp=$(mktemp "${TMPDIR:-/tmp}/bzz-installer.XXXXXX") || stop "could not create a temporary file"
trap 'rm -f "$tmp"' EXIT
trap 'rm -f "$tmp"; exit 1' HUP INT TERM

if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$tmp" || stop "could not download $url"
elif command -v wget >/dev/null 2>&1; then
    wget --https-only -q -O "$tmp" "$url" || stop "could not download $url"
else
    stop "this installer needs curl or wget; install one and run it again."
fi

sh "$tmp" "$@" || exit 1

say ""
[ -n "$NOTE" ] && say "$NOTE" && say ""
say "Try it now, offline, with nothing to set up:   bzz demo"
say "Then join your team's community:               bzz"
if ! command -v bzz >/dev/null 2>&1; then
    # Where the release's installer puts it: $BZZ_INSTALL_DIR/bin, or
    # $CARGO_HOME/bin (cargo-dist's "cargo-home" layout).
    dir=${BZZ_INSTALL_DIR:-${CARGO_HOME:-$HOME/.cargo}}/bin
    say "(bzz is in $dir, not on this shell's PATH yet: open a new terminal, or run $dir/bzz demo)"
fi
