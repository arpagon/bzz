# bzz v0.11.0 — Verified remote managed agents

**Status:** Released 2026-08-25

v0.11.0 adds relay-only interoperability with existing Buzz managed agents.
bzz verifies ownership and public policy, presents a community-scoped directory,
and emits exact agent mentions through the existing human composer. It does not
host or control an agent runtime.

## Highlights

- Pins `buzz-core` and `buzz-sdk` to the source-reviewed Buzz commit
  `9f55bf67456be10ff7c8238bf0d9e12e582848f6`.
- Discovers candidates only from relay-signed kind `39002` membership carrying
  the exact `bot` role.
- Verifies agent-signed kind `0` NIP-OA ownership profiles, agent-signed kind
  `10100` declarations, and optional owner-signed kind `30177` public policy.
- Rejects missing, malformed, conflicting, wrong-owner, wrong-coordinate,
  removed, stale, and cross-community authority.
- Adds `bzz agents list|show|refresh` with remote-runtime wording and versioned
  JSON output.
- Adds TUI `:agents` with wide/narrow list/detail layouts, keyboard/mouse
  selection, owner/policy/freshness explanations, refresh, and exact composer
  insertion.
- Distinguishes verified agents in ordinary `@` completion without replacing
  human channel members.
- Revalidates the active community, channel, bot role, NIP-OA owner, public
  policy, and active identity before signing an agent mention.
- Preserves the exact draft whenever refresh, eligibility, signing,
  publication, or relay acknowledgement does not complete safely.
- Projects deterministic agent-signed reactions and threaded replies through
  existing Timeline, thread, unread, and Inbox behavior.

## Identity and publication boundary

The local author remains the configured human identity. Selecting an agent only
adds visible text and a structured lowercase 64-character `p` tag to the draft.
A human must review and explicitly submit it. The ordinary acknowledged outbox
is unchanged.

Remote responses are signed by the remote agent identity. bzz never requests,
imports, stores, exports, or logs that identity's private key.

“Verified” proves the reviewed public identity relationship. “Eligible” means
the active human appears to satisfy public `owner-only`, `allowlist`, or
`anyone` policy for that channel. Neither means the remote runtime is trusted,
online, ready, safe, or guaranteed to answer.

## Local data migration

Migration `0007_agent_directory.sql` adds a reconstructable, community-isolated
SQLite projection of bounded public agent records and an index for exact bot
membership. It does not change message, outbox, draft, identity, media, Inbox,
DM, search, diagnostics, or telemetry schemas.

There is no TOML `[agents]` section, executable configuration, managed-agent
JSON store, agent key, provider credential, environment map, or start-on-launch
state. Retired v0.3–v0.9 `[[local_agents]]` values remain discarded and are not
reinterpreted.

## Diagnostics and privacy

Two local typed outcomes cover agent directory refresh and mention validation.
They contain only bounded counts, durations, and closed outcome enums. They do
not contain names, pubkeys, owners, communities, channels, events, tags,
capabilities, policies, profiles, messages, or drafts.

These new events are explicitly excluded from the OTel export allowlist. Remote
telemetry behavior does not expand in v0.11.0.

## Explicit non-goals

v0.11.0 does not:

- create, own, back up, or restore an agent key;
- mint NIP-OA attestations or create an agent;
- add an agent to a channel;
- spawn `buzz-acp`, Codex, Claude, Goose, Buzz Agent, or another executable;
- install or authenticate a model/provider;
- expose tool permissions or local tool authority;
- publish autonomously as an agent;
- consume NIP-AE memory, NIP-AO observer/control, NIP-PMA private state, or
  usage metrics;
- deploy provider-backed agents; or
- claim Buzz Desktop feature parity or control of a remote runtime.

Local hosting requires a separate approved ADR and release plan.

## Validation

Deterministic tests cover NIP-OA, owner conflicts, signatures, policy modes, DM
hardening, wrong coordinates, size bounds, community isolation, exact bot roles,
idempotent projection, removal, staleness, exact mention validation,
acknowledgement-aware drafts, local-only diagnostics, wide/narrow rendering,
and semantic hit targets.

The pinned Buzz relay journey creates a disposable dedicated agent identity,
publishes valid public records, assigns a bot role, discovers it, sends one
exact human-authored mention, receives a deterministic agent-signed reaction
and threaded reply, and validates local projection. It starts no LLM, ACP
adapter, tool, memory, or observer process.

Full evidence is tracked in [`validation-v0.11.0.md`](validation-v0.11.0.md)
and [`release-v0.11.0-verification.md`](release-v0.11.0-verification.md).
The trust decision is in
[`adr-v0.11-remote-managed-agent-interoperability.md`](adr-v0.11-remote-managed-agent-interoperability.md).
