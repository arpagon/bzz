# bzz v0.13.0 validation

**Status:** Validated 2026-11-10 locally, on the pinned relay (plain and
FI-enforcing), in CI on Linux, macOS Intel, macOS Apple Silicon and Windows,
through Herdr on the release binary, and with raster captures. What remains
needs the owner's production community, an FI community, or the v0.12.x tags
(see [Pending](#pending)).

Branch `v0.13.0` (draft PR #6, CI only), Buzz pin
`f3b127e3af5c14d2314faa2b63556c3f51ec6743`, Rust 1.98.1. Method:
[`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md) — each
claim has a test that was seen failing without the change where noted.

## Gates (final tree)

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint (audio) | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| Lint (view-only) | `cargo clippy --locked --all-targets --no-default-features -- -D warnings` | clean |
| Tests (audio) | `cargo test --locked --all-targets` | 446 passed, 0 failed |
| Tests (view-only) | `cargo test --locked --all-targets --no-default-features` | 445 passed, 0 failed |
| Supply chain | `cargo deny --locked check` / `cargo audit` | ok (recorded exceptions below) |
| Release build | `cargo build --locked --release`; `bzz --version` | `bzz 0.13.0` |
| Pinned relay | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh` | 11 passed |
| FI-enforcing relay (T4) | `BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh --nip-fi` | 4 passed |
| Herdr, harness (T9) | `BZZ_BUZZ_SOURCE=<pin> BZZ_HERDR_PANE=<pane> ./scripts/test-relay.sh --herdr` | 3 scenarios passed |
| Herdr, empty profile | `BZZ_HERDR_PANE=<pane> ./scripts/test-tui-herdr.sh` | 3 scenarios passed |
| Upgrade from v0.12.1 | `./scripts/upgrade-test.sh` | passed (Linux local; CI below) |
| Audio device (T7) | `./scripts/test-audio-loopback.sh` | passed (PipeWire local; CI below) |
| Benchmarks | `cargo bench` with `--save-baseline v0121` on v0.12.1 and `--baseline v0121` here | no regression (below) |

CI on the branch (`ci.yml`, `integration.yml`, `release.yml` plan), all
green: `test` (fmt, clippy, all tests, deny, audit); `build` with `bzz
--version` and the native-keychain smoke on ubuntu-latest, macos-15-intel,
macos-15 (Apple Silicon) and windows-latest; `upgrade` on ubuntu-latest,
macos-15 and windows-latest; `audio` on ubuntu-latest (PulseAudio null sink)
and macos-15 (BlackHole); `relay (default)` and `relay (nip-fi)`; `dist
plan`. `audio (windows-latest)` is recorded as experimental: hosted Windows
runners have no audio endpoint, so opening the default devices fails there.

CI found three real problems, fixed in `ce2b47c`: RUSTSEC-2026-0459 (bundled
SQLite in `libsqlite3-sys` 0.35; `rusqlite` 0.41 now), GitHub's retirement of
the `macos-14` image (CI and `cargo-dist` now build Apple Silicon on
`macos-15`; `dist plan` lists it), and the FI issuer being unreachable where
`localhost` also resolves to `::1` (the job log shows `127.0.0.1 ::1`; the
test issuer now listens on both).

Advisory exceptions (in `deny.toml` and `.cargo/audit.toml` with reasons):
RUSTSEC-2026-0293 (unused `ringbuf` of pinned `neteq`) and RUSTSEC-2026-0387
(`crypto-bigint` 0.5.5 via the dev-only `p256` test JWT signer;
`cargo tree -i crypto-bigint@0.5.5 -e normal,build` is empty).

## Milestones

Pinned-relay tests are in `tests/relay_integration.rs` and
`tests/relay_nip_fi_integration.rs`.

- [x] **M0 baseline** — pin in `Cargo.toml`, `THIRD_PARTY_LICENSES.md` and
  `docs/protocol-compatibility.md`; three ADRs; T1 controllable time and T2
  programmable relay (`e14f245`, `2d35ecc`, `da84bc8`). `bzz --version` on
  the four CI platforms.
- [x] **M1 channels and roles** — `real_relay_channel_types_roles_and_membership`;
  migration 0009 red/green (`fb2b26b`, `805bf00`). Every channel-settings
  notice the pinned relay emits (`join_role_changed`, topic, purpose,
  visibility, unlisted, auto-join, TTL, deletion, auto-archive, admin kick)
  renders in the desktop client's wording (`ed1f3c8`; a capture had shown
  "Unsupported system event"; seen red without the arm).
- [x] **M2 snapshots** — NIP-CL 39008 and NIP-RS snapshots (`0738435`,
  `9988383`). NIP-CL `membership_public` (39009) for the new profile card
  (`e3eb770`): every spec rejection case fails closed (each check seen red
  under mutation), unsupported/429 are never lists, and
  `real_relay_public_membership_lookup` runs the spec's normative example
  (open stream, archived forum and a joined channel listed with their roles;
  a shared private and an unlisted channel excluded; a stranger gets a signed
  empty list). The app applies only its outstanding answer, once (seen red).
- [x] **M3 federated identity** — the four FI-relay tests (deny without
  evidence, admit with NIP-FA evidence, deadline reconnect, startup from a
  stored session), `real_relay_buzz_desktop_identity_signs_through_the_provider`,
  `tests/federated_identity_test.rs` against T3. Verified names carry a ✓ in
  the timeline, the mention picker (`mention_candidates_mark_and_find_verified_people`,
  seen red) and the profile card. Real-browser sign-in: headless Chrome
  followed the fake adapter's login redirect to bzz's callback ("You can
  close this tab and return to bzz") and the community came online.
- [x] **M4 artifacts** — `real_relay_channel_artifacts`: sync set at EOSE,
  assigned tasks, rename keeping unknown tags/content, a competing edit
  reconciled on the winner (`prev` = winner), delete, restore, move (the source
  sync receives the destination head), nothing in the chat timeline. App test
  `artifact_sync_commits_only_at_eose_and_never_touches_chat` fails when the
  EOSE gate is removed; `protocol::artifact` tests fail when unknown tags are
  dropped.
- [x] **M5 custom emoji** — `real_relay_custom_emoji`; the graphics capture
  shows the image inline in Ghostty (Kitty protocol).
- [x] **M6 presence and status** — `real_relay_presence_snapshot_and_status`;
  `:presence [on|off]` toggles and saves the opt-in (`73b7634`).
- [x] **M7 windows** — `real_relay_channel_and_thread_windows`: 230 dense rows
  without gaps or duplicates, aux reaction, 39005 summary, 120 replies
  newest-first in NIP-CW order, the app's service path windowed, unpinned
  store reports unsupported. The scroll video walks 260 messages across three
  window pages; frames at the 60/61 and 160/161 page boundaries show
  contiguous numbering.
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
  FI-enforcing relay. `bzz broker create/list` on the release binary.
- [x] **M10 send to channel and report** — `real_relay_send_to_channel_and_report`.

## Shared infrastructure

- [x] T1 controllable time; T2 programmable relay; T3 fake NIP-FA adapter with
  agent grant, agent assertion and revoke; T4 FI-enforcing relay; T5 fake Buzz
  Desktop provider; T6 broker conformance.
- [x] T7 audio (`tests/huddle_device_loopback_test.rs`,
  `scripts/test-audio-loopback.sh`): the engine's default devices play a
  440 Hz tone into a virtual device and capture it back, then dropping the
  devices closes both streams and the sound server no longer lists them.
  PipeWire (local): first tone frame after 128 ms, peak 0.50, streams 0 / 2 /
  0. PulseAudio (CI Linux): 1407 ms, streams 0 / 2 / 0. BlackHole (CI macOS):
  41 ms. Seen failing when the streams are leaked. Lock and quit drop the
  call and its devices (`stop_huddle_call` in `:lock` and shutdown).
- [x] T8 secret scan (`tests/secret_scan_test.rs`): broker credentials, the
  owner passphrase, the adapter session, agent grant, owner and agent
  assertions and both secret keys are absent from every written file, the
  support report, `diagnostics status --json` and the `Debug`/`Display` forms.
  Seen failing when `AgentGrant`'s `Debug` printed its token.
- [x] T9 Herdr on the release binary: `startup-help-quit`,
  `custom-keymap-inbox` and `v013-panels-empty-profile` (empty profile), and
  the harness scenarios `t9-system-group-read-only`,
  `t9-artifacts-emoji-report` and `t9-presence-broker-huddle` (System group,
  read-only composer, artifacts panel, custom emoji picker, report dialog,
  presence toggle, broker panel, huddle start/join muted/unmute/mute/leave
  with ALSA's null device). The run caught a real bug: command feedback was
  invisible on the empty-profile welcome screen (fixed in `18ccf50`, seen
  red).

## Benchmarks against v0.12.1

`cargo bench` on v0.12.1 (`7128033`, `--save-baseline v0121`) and on this
tree (`--baseline v0121`), same machine, sequential. Improved: FTS search
−8.1 %, Inbox projection rebuild −10.7 %, timeline cursor −2.0 %, sanitize
−8.0 %, composer −10.4 %, hit map −9.8 %, timeline render −7.4 %, redraw gate
−6.3 %. Within noise: PNG decode, latest-500 query, Inbox pages. Two first-run
flags (+3.4 % `parse bounded imeta`, +3.7 % mention candidates over 1k
members) re-measured at 10 s: −0.4 % and +0.8 %, both within noise.

## Upgrade from v0.12.1

`scripts/upgrade-test.sh` builds v0.12.1 (`7128033`) in a worktree, lets it
write a database with disposable keys and record what it reads back, then
opens a copy with v0.13.0: migrations 8 → 15, `PRAGMA integrity_check` ok, a
readable pre-migration backup at version 8, identical Inbox conversations and
unread counts, identical FTS hits (including the DM), history, draft and
channel facts. Seen failing when the pre-migration backup is skipped. Green
in CI on Linux, macOS and Windows.

## Manual E2E (`docs/e2e-manual.md`, v0.13.0)

Run 2026-11-10 with the release binary, a disposable encrypted-file identity
and generated content on the isolated pinned relay, driven from a Herdr pane.

| Section | Result | Evidence/notes |
|---|---|---|
| 0. Setup | pass | `bzz 0.13.0`, isolated paths, `check` valid |
| 1. Empty startup | pass | Herdr `startup-help-quit` (help, quit, terminal restored) |
| 2. Identity/backup | pass | list shows no secret; verify; backup `ncryptsec1…` mode 600; a second backup to the same path is refused |
| 3. Community/auth | pass | connecting → online; found the false NIP-98 warning, fixed in `6ff7b2c` (seen red) |
| 4. Message/restart | pass | published, `[pending]` cleared, visible after restart |
| 4.1 Mouse/mentions | partial | mention picker inserted `@T9 Bob` and the event carries his `p` tag; mouse not exercised (no pointer in Herdr) |
| 4.2 Remote agents | partial | the pinned-relay `real_relay_mvp_protocol_journey` publishes a deterministic remote agent (agent kind 0 with a NIP-OA owner tag, 10100 declaration, owner 30177 policy) and verifies owner and eligibility; the manual `:agents` walk and its negative matrix were not repeated |
| 4.3 Diagnostics/telemetry | pass | `diagnostics status --json`; `diagnostics report` mode 600 with no message token or passphrase; telemetry off |
| 5. Conversation | pass | thread reply (4 replies), 🎉 reaction (kind 7), delete own message (kind 5, `[deleted]`) |
| 6. Portable backup | pass | `import-backup` into a fresh profile restores the same pubkey; both verify |
| 7. Recovery | not run | keychain-destructive; the run used the encrypted-file backend (CI smoke covers native keychains) |
| 8. Network/multi-client | pass | disposable copy on a closed port: `offline cache` with history. Two clients of one identity with separate databases published in turn: one copy of each message, two `read-state:` slots (one `client_id` each), and the same maximum `read_at` in both after the second synced. Two clients of different identities exchanged a DM, a mention, a thread reply and media. One relay only, so the second community is the automated `two_community_cache_and_read_state_are_isolated`, as the manual allows |
| 9. Images/attachments | partial | `Alt-o` path staging → `[ready]` → sent → rendered on the other client; `Ctrl-v`/`Ctrl-o` need a desktop clipboard and file chooser |
| 10. Inbox/DMs/search | pass | DM opened from the picker; B's Inbox shows the mention and the DM, `m` marks read; search finds the token (local + relay) once a member |
| 11. v0.13.0 surfaces | pass | profile card lists the owner's public channels with roles; the rest through the Herdr harness scenarios and captures |
| Cleanup | pass | community and identity removed; no `nsec` or passphrase in any file |

## Captures

Raster captures of the release binary in Ghostty on a private Xvfb display
(`bzz-visual-capture`), generated fixture content only, reviewed frame by
frame: `/tmp/bzz-v013-visual.p6pDzi/` —
`m1-system-readonly.png` (System group open, read-only announce composer,
`join_role_changed` notice), `m3-1-sign-in-required.png`,
`m3-2-finish-in-browser.png`, `m3-3-signed-in.png`, `m4-artifacts.png`,
`m5-inline-emoji.png` (the generated 64×64 emoji served from this repository,
drawn inline) and `m7-scroll.mp4` (34 s, 1130×654, 10 fps). README has no
media.

## Pending

| Item | Why | Who |
|---|---|---|
| Production acceptance on the real community (startup frame count against the 66-channel account, windows, membership changes, a huddle with a second client, a broker agent posting in a test channel) | needs the owner's identity and community | owner |
| FI community acceptance | no FI community available; T4 and the real-browser harness run are the proof | owner |
| Tag v0.12.0 and v0.12.1, then v0.13.0 | the plan's prerequisite; each tag publishes binaries | owner |
| T7 on Windows | hosted runners have no audio endpoint; needs a self-hosted runner or a virtual driver with capture | CI / operator |
| E2E 4.1 mouse, 4.2 remote agents, 7 keychain recovery, 9 clipboard and OS file chooser | need the operator's desktop pointer, keychain or clipboard, or a remote-agent fixture | operator |
| `site/` copy and media for v0.13.0 | the owner has uncommitted site work in the same files, and a deploy publishes | owner |
