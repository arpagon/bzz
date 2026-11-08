# bzz v0.13.0 validation

**Status:** Local and pinned-relay validation complete on Linux 2026-11-08.
Cross-platform CI, Herdr runs, production acceptance and publication pending
(see [Pending](#pending)).

Branch `v0.13.0`, Buzz pin `f3b127e3af5c14d2314faa2b63556c3f51ec6743`, Rust
1.98.1. Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md)
— each claim has a test that was seen failing without the change where noted.

## Gates (Linux, 2026-11-08, final tree)

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint (audio) | `cargo clippy --locked --all-targets --examples -- -D warnings` | clean |
| Lint (view-only) | `cargo clippy --locked --all-targets --no-default-features -- -D warnings` | clean |
| Tests (audio) | `cargo test --locked --all-targets` | 438 passed, 0 failed |
| Tests (view-only) | `cargo test --locked --all-targets --no-default-features` | 437 passed, 0 failed |
| Supply chain | `cargo deny --locked check` / `cargo audit` | ok (two recorded exceptions, below) |
| Release build | `cargo build --locked --release`; `bzz --version` | `bzz 0.13.0` |
| Pinned relay | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh` | 10 passed |
| FI-enforcing relay (T4) | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh --nip-fi` | 4 passed |

Advisory exceptions (in `deny.toml` and `.cargo/audit.toml` with reasons):
RUSTSEC-2026-0293 (unused `ringbuf` of pinned `neteq`) and RUSTSEC-2026-0387
(`crypto-bigint` 0.5.5 via the dev-only `p256` test JWT signer;
`cargo tree -i crypto-bigint@0.5.5 -e normal,build` is empty).

## Milestones

Pinned-relay tests are in `tests/relay_integration.rs` and
`tests/relay_nip_fi_integration.rs`.

- [x] **M0 baseline** — pin in `Cargo.toml`, `THIRD_PARTY_LICENSES.md` and
  `docs/protocol-compatibility.md`; three ADRs; T1 controllable time and T2
  programmable relay (`e14f245`, `2d35ecc`, `da84bc8`).
- [x] **M1 channels and roles** — `real_relay_channel_types_roles_and_membership`;
  migration 0009 red/green (`fb2b26b`, `805bf00`).
- [x] **M2 snapshots** — NIP-CL 39008 and NIP-RS snapshots (`0738435`,
  `9988383`). `membership_public` is pending (needs a profile card).
- [x] **M3 federated identity** — the four FI-relay tests (deny without
  evidence, admit with NIP-FA evidence, deadline reconnect, startup from a
  stored session), `real_relay_buzz_desktop_identity_signs_through_the_provider`,
  `tests/federated_identity_test.rs` against T3.
- [x] **M4 artifacts** — `real_relay_channel_artifacts`: sync set at EOSE,
  assigned tasks, rename keeping unknown tags/content, a competing edit
  reconciled on the winner (`prev` = winner), delete, restore, move (the source
  sync receives the destination head), nothing in the chat timeline. App test
  `artifact_sync_commits_only_at_eose_and_never_touches_chat` fails when the
  EOSE gate is removed; `protocol::artifact` tests fail when unknown tags are
  dropped.
- [x] **M5 custom emoji** — `real_relay_custom_emoji`.
- [x] **M6 presence and status** — `real_relay_presence_snapshot_and_status`.
- [x] **M7 windows** — `real_relay_channel_and_thread_windows`: 230 dense rows
  without gaps or duplicates, aux reaction, 39005 summary, 120 replies
  newest-first in NIP-CW order, the app's service path windowed, unpinned
  store reports unsupported.
- [x] **M8 huddles** — all 14 upstream recorded tracker sequences match their
  rosters and end state; `real_relay_huddle_lifecycle_and_audio_round_trip`:
  muted capture never reaches the peer, an unmuted 440 Hz tone round-trips
  through the relay (peak > 0.1), the relay-signed roster lists both, ending
  clears it. App test for roster, creator-only guidelines and reactions.
- [x] **M9 broker** — the SDK's own `BrokerClientExt::execute` validates every
  host answer in process and over HTTP (T6): success, replay, conflict, typed
  denials, indeterminate, pause, unknown credential, explicit `null`, unknown
  members. `real_relay_local_agent_broker`: an unseeded agent posts, replies
  and reacts with a verifying owner NIP-OA tag, newest-first paging with an
  advancing cursor, typed denial without the `bot` role, profile set, the owner
  derives the same NIP-AE `d` tag, a non-owner's 20003 has no effect on the
  presence snapshot while the owner's sets the agent online.
  `fi_relay_admits_a_hosted_agent_only_with_its_owner_grant` on the
  FI-enforcing relay. CLI exercised end to end with a disposable profile.
- [x] **M10 send to channel and report** — `real_relay_send_to_channel_and_report`.

## Shared infrastructure

- [x] T1 controllable time; T2 programmable relay; T3 fake NIP-FA adapter with
  agent grant, agent assertion and revoke; T4 FI-enforcing relay; T5 fake Buzz
  Desktop provider; T6 broker conformance.
- [x] T8 secret scan (`tests/secret_scan_test.rs`): broker credentials, the
  owner passphrase, the adapter session, agent grant, owner and agent
  assertions and both secret keys are absent from every written file, the
  support report, `diagnostics status --json` and the `Debug`/`Display` forms.
  Seen failing when `AgentGrant`'s `Debug` printed its token.
- [~] T7 audio: round-trip proven through the relay with synthetic PCM; virtual
  devices (PipeWire null sink, BlackHole, Windows render device) not run.
- [~] T9 Herdr: scenario `v013-panels-empty-profile` added and its
  deterministic twin `v013_commands_on_an_empty_profile_explain_or_open_their_panels`
  passes; the Herdr run itself is pending.

A timing flake in the M3 re-mint test (the fake adapter stamps `exp` from the
real clock) was found during this validation and fixed in `2d24ce7`.

## Pending

| Item | Why | Who |
|---|---|---|
| CI on macOS Intel, macOS Apple Silicon, Windows; `integration.yml` both jobs | needs the branch pushed | owner approval to push |
| T7 virtual audio devices and device release on lock/quit measured on 3 OSes | needs CI runners with virtual devices | CI |
| T9 Herdr run | needs a Herdr-managed pane (`HERDR_ENV=1`) | operator |
| Benchmarks against v0.12.1 | not run | next |
| `docs/e2e-manual.md` update to v0.13.0 and end-to-end run | not done | next |
| Upgrade test from a v0.12.1 data directory on 3 OSes | migrations are tested locally only | CI / operator |
| Production acceptance on the real community (startup frames, windows, membership changes, a huddle with a second client, a broker agent) | needs the owner's identity and community | owner |
| FI community acceptance | no FI community available; T4 is the only proof | owner |
| README/site captures | must be sanitized real captures (`bzz-visual-capture`) | next |
| `membership_public` (M2) | needs a profile card bzz does not have | v0.13.x |
