#!/usr/bin/env bash
# Release acceptance 3: open a real bzz v0.12.1 data directory with this build.
#
# Builds v0.12.1 from its validated commit in a temporary worktree, lets it
# write a database with disposable keys (tests/upgrade/seed_v0121.rs), records
# what v0.12.1 reads back, then runs tests/upgrade_from_v0121_test.rs against
# a copy: migrations, backup, FTS, Inbox and drafts must survive.
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
# docs: record v0.12.1 validation (the v0.12.1 release candidate).
old=${BZZ_UPGRADE_FROM:-712803381e9f6a79f22fbaa550fb68084e90d73f}
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
cp "$repo_root/tests/upgrade/seed_v0121.rs" "$work/old/examples/seed_v0121.rs"

# Build the old release with this checkout's toolchain (newer than its MSRV).
toolchain=$(cd "$repo_root" && rustup show active-toolchain | awk '{print $1}')
fixture="$work/fixture"
(
  cd "$work/old"
  RUSTUP_TOOLCHAIN="$toolchain" CARGO_TARGET_DIR="${BZZ_UPGRADE_TARGET_DIR:-$work/target}" \
    cargo run --quiet --locked --example seed_v0121 -- "$(native "$fixture")"
)
BZZ_UPGRADE_FIXTURE="$(native "$fixture")" \
  cargo test --locked --test upgrade_from_v0121_test -- --ignored --nocapture
