---
name: bzz-prove-it-works
description: Prove a bzz change works before calling it done, by observing the real behavior on the lowest rung that can show it. Use after implementing or fixing anything in bzz, before a commit, PR or release claim, and when filling a docs/validation-*.md checklist.
---

# Prove it works in bzz

A change is done when its **claim** has been observed, not inferred. Compiling,
passing unrelated tests, or "the code looks right" are proxies. The proof is the
real artifact behaving as claimed: a test going from red to green, a fake or
pinned relay receiving the exact frames, the release binary on screen.

## 1. State the claim

Write one sentence naming the observable behavior that changed, in terms a test
or a screen can show. For example: "a `CLOSED rate-limited:` subscription
reopens after the hinted delay and never becomes AccessDenied". A bug fix's
claim is the fixed symptom. A feature's claim is each behavior in its plan
milestone's exit gate.

**Done when:** every claim is a sentence with an observable outcome. A claim
such as "improved" or "handles X" is not observable yet. Sharpen it.

## 2. Pick the rungs

Each claim gets the lowest rung that can show it, plus every higher rung its
behavior depends on. A wire claim proven only by a unit test is unproven.

| Claim touches | Rung | Where |
|---|---|---|
| Pure logic: parsing, policy, reducers, ordering | unit test | `src/**` `#[cfg(test)]`, or `tests/*_test.rs` |
| Rendering and layout in a terminal buffer | TUI buffer test | `ratatui::TestBackend`, `tests/support/tui_harness.rs`, `tests/ui_*_test.rs` |
| Frames on the wire: REQ/EVENT/CLOSE pacing, CLOSED/OK/NOTICE handling | fake relay | `tests/support/fake_relay.rs` (`FakeRelay::start_*`), `tests/session_test.rs` |
| Real Buzz semantics: auth, NIP-29, membership, HTTP bridge, media | pinned relay | `tests/relay_integration.rs` via `scripts/test-relay.sh` |
| Keys, help, keymaps, startup and terminal restoration on the release binary | Herdr runner | `scripts/test-tui-herdr.sh`, `scripts/tui-herdr-scenarios.toml`, `docs/e2e-herdr.md` |
| Pixels: graphics, avatars, media, flicker, theme | visual capture | the `bzz-visual-capture` skill |
| A whole user journey on a real community | manual E2E | `docs/e2e-manual.md` |

When no fake relay constructor can produce the frames a claim needs, add one to
`fake_relay.rs` beside the existing `start_*` variants. Use a new constructor,
not ad-hoc sockets in the test.

**Done when:** every claim from step 1 maps to at least one named rung and one
test or scenario.

## 3. Make it red

Run the new test or scenario against the code without the change, using
`git stash` or by reverting the hunk, and watch it fail for the claimed reason.
A test that is green both before and after the change proves nothing, so
rewrite it until it goes red. For a new feature, red is the test failing
because the behavior is missing, not because it does not compile.

**Done when:** each new test has been seen failing with a message that names
the claim.

## 4. Make it green

Run the narrow test first, then the gates CI runs (`.github/workflows/ci.yml`):

```sh
cargo test --locked --test <name> <filter>
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo deny check && cargo audit   # when dependencies changed
```

Pinned relay rung:

```sh
git -C ~/Workspace/.cache/thirdparty/github.com/block/buzz \
  worktree add /tmp/buzz-pin <pinned-rev>
BZZ_BUZZ_SOURCE=/tmp/buzz-pin ./scripts/test-relay.sh
```

Release-binary rungs:

```sh
cargo build --release --locked
BZZ_BIN=$PWD/target/release/bzz BZZ_HERDR_PANE=<disposable pane> ./scripts/test-tui-herdr.sh
```

**Done when:** every chosen rung is green on the final diff, with no later edit
since the run.

## 5. Show the proof

Report, for each claim, the rung, the exact command and the observed result
(test name and pass, frame counts, screenshot path). For release work, tick the
matching items in `docs/validation-v<version>.md` and record what was run, on
which commit and against which relay revision. Leave an item unticked when it
was not observed, and say what is missing.

**Done when:** a reviewer can re-run every listed command and see the same
result, and every unobserved item is named as unobserved.

## Gotchas

- **Pin in three places.** The Buzz revision is hardcoded in `Cargo.toml`,
  `scripts/test-relay.sh` (`PIN=`) and `tests/relay_integration.rs` (`PIN`).
  `test-relay.sh` refuses a checkout at any other revision, and the shared
  third-party cache tracks `main`. Use a worktree at the pin.
  `BZZ_ALLOW_UNPINNED_RELAY=1` is for exploration only, never for proof.
- **The pinned relay needs Docker and tmux.** It is a Docker Compose harness
  on `localhost:3030`. Its quota overrides apply only inside it, so it cannot
  prove production quota behavior. Use the fake relay's rate-limit
  constructors for that.
- **Release binary for E2E.** Debug builds use separate paths and keychain
  services, so Herdr, visual and manual rungs always run `--release`.
- **Disposable everything.** Use a generated identity, isolated
  `BZZ_CONFIG_DIR`/`BZZ_DATA_DIR`/`BZZ_CACHE_DIR`, and a test relay and
  channel. Proof artifacts contain no nsec, passphrase, token, assertion, real
  name, message content or relay identifier.
- **Herdr uses `send-keys`.** Text paste bypasses the key router being tested,
  so it proves nothing about keys.
- **Time-based behavior** (backoff, TTLs, pacing) needs a test that controls
  time, such as paused Tokio time or an injected clock. A sleep-based test is
  a flake, not proof.
- **Production acceptance** is the only rung that proves behavior against the
  deployed relay. When it has not run, the validation doc says "pending", as
  v0.12.x does today.
