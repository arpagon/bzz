# v0.11.0 validation record

**Status:** In progress

**Candidate:** `0.11.0`

**Buzz baseline:** `9f55bf67456be10ff7c8238bf0d9e12e582848f6`

## Compatibility gate

- [x] `buzz-core` and `buzz-sdk` pin updated to the exact reviewed revision
- [x] existing code compiles against the updated locked dependency graph
- [x] protocol compatibility and third-party license records updated
- [x] no Buzz Desktop Tauri IPC, JSON store, keyring name, receipt, log, or
  database is used as an integration surface

## Verification and policy

- [x] Nostr signatures verified before projection
- [x] kind `0` NIP-OA owner verified against the agent author
- [x] multiple valid owners fail as a conflict
- [x] kind `10100` declaration author equals the agent
- [x] optional kind `30177` signer equals the verified owner
- [x] kind `30177` `d` coordinate equals the exact agent pubkey
- [x] `owner-only`, `allowlist`, `anyone`, and policy-unknown are distinct
- [x] DM and unknown-channel policy fails closed to owner-only
- [x] oversized names, records, capabilities, and allowlists fail closed
- [x] declaration status is not misrepresented as ephemeral runtime readiness

## Persistence and synchronization

- [x] migration `0007_agent_directory.sql`
- [x] community-scoped `(community_id, agent_pubkey)` authority
- [x] exact relay-signed `bot` role preserved from membership tags
- [x] bot membership is the bounded candidate source
- [x] profile/declaration/policy queries are author/tag/count bounded
- [x] replaceable event ordering uses timestamp plus event-ID tie break
- [x] unchanged reconciliation produces no SQLite write
- [x] membership removal revokes directory visibility without rewriting history
- [x] stale projections are non-invocable
- [x] full refresh runs at startup, every five minutes, explicit TUI refresh,
  CLI refresh, and before an agent mention enters the outbox

## CLI and TUI

- [x] `bzz agents list [--community] [--json]`
- [x] `bzz agents show <pubkey> [--community] [--json]`
- [x] `bzz agents refresh [--community]`
- [x] CLI explicitly says bzz does not control the remote runtime
- [x] versioned JSON carries public projection only
- [x] TUI `:agents` wide and narrow list/detail layouts
- [x] stable pubkey selection and semantic mouse hit targets
- [x] owner, policy, eligibility, freshness, channels, and remote-control
  disclaimer
- [x] verified agent marker does not rely only on color
- [x] community switch and lock clear stale directory state
- [x] `@` completion distinguishes verified agents from humans

## Draft and publication boundary

- [x] selecting an agent inserts text and a structured exact pubkey mention
- [x] selection never sends
- [x] duplicate mentions deduplicate to one lowercase `p` tag
- [x] send-time refresh and verification happen before draft submission state
- [x] invalid, ineligible, unknown-policy, stale, removed, offline, rejected, and
  ambiguous outcomes preserve the exact draft
- [x] human identity and acknowledged outbox remain the only local publication
  authority
- [x] remote replies remain independently signed ordinary events

## Privacy and observability

- [x] no agent private key, process, ACP, model, tool, provider, environment,
  memory, observer, or autonomous outbox exists
- [x] public remote strings never reach a command, shell, path, executable, or
  child environment
- [x] agent diagnostics contain only counts, duration, and closed outcome enums
- [x] agent diagnostics reject unsafe persisted values
- [x] new agent diagnostics are explicitly excluded from OTel export
- [x] no NIP-AE `30174`, NIP-AO `24200`, NIP-PMA `30179`, or usage `44200`
  subscription/archive/display path exists

## Deterministic local gates

- [x] baseline `cargo check --locked --all-targets`
- [x] targeted agent protocol, store, policy, UI, migration, and diagnostics
  tests
- [x] strict all-target Clippy during implementation
- [x] full locked all-target suite during implementation
- [x] final `cargo fmt --all -- --check`
- [x] final `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [x] final `cargo test --locked --all-targets` — 174 library tests plus all
  integration and benchmark targets; real-relay test intentionally uses wrapper
- [x] final `cargo deny check`
- [x] final `cargo audit` — only accepted `instant` and `lru` warnings
- [x] release build and isolated `bzz 0.11.0` check/legacy-config/agents JSON
  smoke

## Pinned Buzz relay

- [x] exact reviewed Buzz source revision enforced by wrapper and test
- [x] pre-existing NIP-42/NIP-98/NIP-29/message/DM/search/media/read-state
  journey remains green
- [x] deterministic dedicated agent identity and valid NIP-OA public records
- [x] exact relay bot membership discovered
- [x] exact human-authored agent `p` tag accepted
- [x] deterministic agent-signed `👀` reaction and threaded reply accepted
- [x] verified directory, policy eligibility, backfill, and local thread
  projection confirmed
- [x] no model, ACP adapter, tool, memory, observer, or production key used
- [ ] CI pinned Buzz integration run recorded

The local wrapper passed after removing an unrelated local `LD_LIBRARY_PATH`
that shadowed the system curl library. The successful run used only
`PKG_CONFIG_PATH`/`LIBRARY_PATH` for the local DBus development link and fully
cleaned the disposable Docker project.

## Visual acceptance

- [x] deterministic TestBackend wide/narrow rendering and hit-map regression
- [x] Herdr release-binary narrow Agents directory journey with public-only
  SQLite fixture and no credential
- [x] keyboard selection in Herdr and deterministic semantic mouse-selection
  regression
- [x] cache-only refresh explanation and mention refusal; exact online mention
  insertion is covered by deterministic composer/store tests and the pinned
  relay journey
- [x] community/lock stale-handle reset regression
- [x] automated release-binary startup/help/custom-keymap scenarios
- [x] terminal restoration confirmed by foreground shell process after exit;
  fixture and created pane removed

## Cross-platform and publication

- [ ] Linux, Windows, Intel macOS, and Apple Silicon macOS CI
- [ ] release workflow and expected assets
- [ ] aggregate and individual checksums
- [ ] SLSA provenance for platform archives, source, and SBOM
- [ ] CycloneDX application/version/component validation
- [ ] downloaded Linux version/check/agents smoke
- [ ] native-keychain and encrypted-file credential regression
- [ ] published-artifact verification record
