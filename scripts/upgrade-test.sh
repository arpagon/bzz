#!/usr/bin/env bash
# Upgrade acceptance: open a real data directory written by an older bzz.
#
# Builds the old release from its commit in a temporary worktree, lets it
# write a database with disposable keys (tests/upgrade/seed_<v>.rs), records
# what the old release reads back, then runs tests/upgrade_from_<v>_test.rs
# against a copy: migrations, backup, FTS, Inbox and drafts must survive.
#
# BZZ_UPGRADE_FROM_VERSION selects the old release: 0130 (v0.13.0, default)
# or 0121 (v0.12.1).
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
version=${BZZ_UPGRADE_FROM_VERSION:-0130}
case "$version" in
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
