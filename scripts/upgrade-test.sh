#!/usr/bin/env bash
# Upgrade acceptance: open a real data directory written by an older bzz.
#
# Builds the old release from its commit in a temporary worktree, lets it
# write a database with disposable keys (tests/upgrade/seed_<v>.rs), records
# what the old release reads back, then runs tests/upgrade_from_<v>_test.rs
# against a copy: migrations, backup, FTS, Inbox and drafts must survive.
# From v0.14.0 the fixture is a whole profile: the settings with a hosted
# agent of v0.13 and its sealed key travel with the database. From v0.15.0
# it also holds local agents made by `bzz agent new`'s code, their inbox and
# reply ledger, what they asked of the owner, and artifacts. From v0.16.0 it
# also holds an installed and approved app open in a channel, its records,
# huddle devices saved under their ALSA names, and a theme and a key map of
# the user's own; v0.17.0 runs no migration on it.
#
# BZZ_UPGRADE_FROM_VERSION selects the old release: 0160 (v0.16.0, default),
# 0150 (v0.15.0), 0140 (v0.14.0), 0130 (v0.13.0) or 0121 (v0.12.1).
# BZZ_UPGRADE_TARGET_DIR is where the old release is built (default: inside
# the temporary folder, removed after).
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
version=${BZZ_UPGRADE_FROM_VERSION:-0160}
case "$version" in
  0160) default_commit=650a6481d794957943ed412ed87e36ad75785445 ;; # tag v0.16.0
  0150) default_commit=f9fc97b1be77478e2873c213374304e9abdc28f6 ;; # tag v0.15.0
  0140) default_commit=80e1b0738891ca110e84b3e3458a2c73ed16d381 ;; # tag v0.14.0
  0130) default_commit=2b3429fd0f0e8667d51b4006807d31cb6cedf187 ;; # tag v0.13.0
  0121) default_commit=712803381e9f6a79f22fbaa550fb68084e90d73f ;; # v0.12.1 validated
  *) echo "unknown BZZ_UPGRADE_FROM_VERSION=$version" >&2; exit 2 ;;
esac
old=${BZZ_UPGRADE_FROM:-$default_commit}
work=$(mktemp -d "${TMPDIR:-/tmp}/bzz-upgrade.XXXXXXXX")
cleanup() {
  git -C "$repo_root" worktree remove --force "$work/old" >/dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

native() {
  # Windows (Git Bash): hand native programs a Windows path.
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi
}

git -C "$repo_root" cat-file -e "$old^{commit}" 2>/dev/null \
  || git -C "$repo_root" fetch --quiet --depth 1 origin "$old"
git -C "$repo_root" worktree add --quiet --detach "$work/old" "$old"
mkdir -p "$work/old/examples"
cp "$repo_root/tests/upgrade/seed_v$version.rs" "$work/old/examples/seed_v$version.rs"

# Build the old release with this checkout's toolchain (newer than its MSRV).
toolchain=$(cd "$repo_root" && rustup show active-toolchain | awk '{print $1}')
fixture="$work/fixture"
(
  cd "$work/old"
  RUSTUP_TOOLCHAIN="$toolchain" CARGO_TARGET_DIR="${BZZ_UPGRADE_TARGET_DIR:-$work/target}" \
    cargo run --quiet --locked --example "seed_v$version" -- "$(native "$fixture")"
)
BZZ_UPGRADE_FIXTURE="$(native "$fixture")" \
  cargo test --locked --test "upgrade_from_v${version}_test" -- --ignored --nocapture
