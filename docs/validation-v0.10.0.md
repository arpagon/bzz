# v0.10.0 validation record

## Scope

This record covers the removal-only agent-foundation reset defined in
`docs/planning/2026-08-24/v0.10.0.md`.

## Automated gates

Local release-candidate execution:

- [x] `cargo fmt --check`
- [x] `cargo check --all-targets`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo test --locked --all-targets`; all unit/integration tests and
  benchmarks passed, while the opt-in real-relay test remained intentionally
  ignored outside its wrapper
- [x] removed-config migration regression
- [x] `cargo deny check` (advisories, bans, licenses, and sources passed;
  existing duplicate-version warnings remain informational)
- [x] `cargo audit` (only the two documented allowed `instant`/`lru` warnings)
- [x] release build
- [x] isolated release-binary `bzz check` with a legacy `[[local_agents]]`
  section, followed by verification that the section was removed
- [x] release CLI help contains no `agent`, `assistant`, or `codex` command
- [ ] Linux, Windows, Intel macOS, and Apple Silicon macOS CI
- [ ] pinned Buzz integration workflow

## Removal evidence

- [x] `src/agent/` and its public module export are absent
- [x] no `AgentRun`, Codex executable wrapper, local-agent config model, CLI
  command, TUI command, overlay, or hit target remains
- [x] Tokio no longer enables its process feature for bzz
- [x] current README, configuration, security, troubleshooting, and manual E2E
  documentation no longer advertise the retired feature
- [x] historical v0.3 records explicitly say the feature is retired
- [x] the new Buzz managed-agent research document makes no bzz support claim
- [x] all existing identity, outbox, draft acknowledgement, media, Inbox,
  diagnostics, telemetry, terminal, and human-send tests remain green

## Manual gate

A Herdr run is explicitly excepted for v0.10.0 because this release only removes
an overlay/process flow and adds no replacement visual interaction. Deterministic
TUI functional and snapshot tests remain required and pass locally.

## Published-artifact gates

- [ ] release workflow succeeds and publishes all expected platform artifacts
- [ ] every `sha256.sum` entry verifies
- [ ] provenance attestations verify for platform archives, source archive, and
  CycloneDX SBOM
- [ ] SBOM identifies `bzz` v0.10.0 and is structurally valid
- [ ] downloaded Linux binary reports v0.10.0 and passes isolated `bzz check`
- [ ] downloaded CLI help contains no retired assistant surface
- [ ] native-keychain create/verify/remove smoke passes
- [ ] encrypted-file create/verify/remove smoke passes with owner-only storage

No production message, identity, attachment, clipboard value, source path, or
channel content is used as release evidence.
