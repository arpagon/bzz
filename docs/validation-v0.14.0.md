# bzz v0.14.0 validation

**Status:** In progress on branch `v0.14.0`. Plan:
[`docs/planning/2026-11-12/v0.14.0.md`](planning/2026-11-12/v0.14.0.md).
Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5` (2026-11-13), Rust 1.98.1.
`block/buzz-app` reference revision `ce63b93d`.

## M0 — Baseline

All checks ran locally on Linux x86_64 on 2026-11-12. CI on four platforms
runs when the branch's PR opens.

| Gate | Command | Result |
|---|---|---|
| Lock change | `Cargo.lock` diff | 3 `source` lines (`buzz-core`, `buzz-sdk`, `buzz-audio-client`); no other package moved |
| Format | `cargo fmt --check` | clean |
| Lint (audio) | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| Lint (view-only) | `cargo clippy --locked --all-targets --no-default-features -- -D warnings` | clean |
| Tests (audio) | `cargo test --locked --all-features` | 456 passed, 0 failed, 19 ignored (same count as v0.13.0) |
| Tests (view-only) | `cargo test --locked --no-default-features` | 455 passed, 0 failed |
| Supply chain | `cargo deny check`; `cargo audit` | ok; the 4 allowed warnings recorded for v0.13.0, no new advisory |
| Pinned relay | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh` | 13 passed |
| FI-enforcing relay (T4) | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh --nip-fi` | 4 passed |

The pin checkout is a worktree of the third-party cache at `runs/buzz-pin`.

**Note:** `target/debug` with full debuginfo had grown to 58 GB and filled
`/home`. `runs/env.sh` now sets `CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`;
a full test build then takes 6.8 GB.

## Pending

| Item | Why |
|---|---|
| M0 on macOS Intel, macOS Apple Silicon and Windows | CI runs on the PR |
| M1–M5 | not started |
