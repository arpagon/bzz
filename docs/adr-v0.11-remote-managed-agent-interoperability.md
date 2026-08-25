# ADR: v0.11 remote managed-agent interoperability

**Status:** Accepted

**Date:** 2026-08-25

## Context

Buzz Desktop managed agents are separate Nostr identities executed by an
operator-controlled runtime. They are not text-generation buttons. The reviewed
implementation combines NIP-OA ownership, public agent records, relay-authored
bot membership, ACP workers, local tools, queues, per-channel sessions,
autonomous publication, memory, observer telemetry, and process recovery.

The source-first investigation is recorded in
[`how-agents-works-in-buzz.md`](how-agents-works-in-buzz.md) against upstream
commit `9f55bf67456be10ff7c8238bf0d9e12e582848f6`.

bzz v0.10 removed its unrelated one-shot Codex draft assistant. Reusing that
implementation would incorrectly combine a human-reviewed composer helper with
a separately keyed autonomous relay participant.

The research distinguishes three independent capabilities:

1. interact with an existing agent through the relay;
2. observe or control an existing runtime; and
3. host a local managed agent.

Each capability has a different trust boundary.

## Decision

v0.11 implements only capability 1: **verified remote managed-agent
interoperability**.

bzz discovers candidate agents from relay-authored kind `39002` membership
carrying the exact `bot` role. It then verifies signed public records:

- agent-authored kind `0` profile containing a valid NIP-OA `auth` tag;
- agent-authored kind `10100` public declaration;
- optional owner-authored kind `30177` public managed-agent policy whose `d`
  coordinate is the exact agent pubkey; and
- ordinary kind `9` messages, reactions, and replies through existing paths.

A candidate receives an agent identity in bzz only when:

1. all consumed events have valid Nostr signatures;
2. profile and declaration authors equal the candidate pubkey;
3. exactly one effective NIP-OA owner is verified;
4. an optional policy is signed by that owner;
5. the policy coordinate equals the candidate pubkey; and
6. current community-scoped membership still carries the bot role.

Names, presence strings, capabilities, or display-text `@Agent` matches never
substitute for those checks.

The public audience policy is normalized to `owner-only`, `allowlist`,
`anyone`, or unknown. bzz evaluates it for the active human identity. DMs and
unknown channel kinds fail closed to owner-only, matching the reviewed inbound
hardening. Eligibility means only that the public policy appears to permit an
invocation; it does not prove that the remote runtime is online, ready, safe, or
willing to respond.

## Publication boundary

The existing human publication boundary remains unchanged.

Selecting an agent inserts ordinary visible composer text plus a structured
`DraftMention` holding the exact 64-character agent pubkey. The user reviews and
explicitly sends the message. Before signing, bzz refreshes public directory
state and revalidates current bot membership, NIP-OA ownership, owner policy,
community, channel, and active identity. Failure preserves the exact draft.

The accepted event uses the existing acknowledged outbox. Only the human key
signs locally. Remote replies are separately signed remote events.

No directory action, refresh, presence update, policy change, or remote reply
automatically publishes from bzz.

## Isolation and persistence

The SQLite projection is keyed by `(community_id, agent_pubkey)`. The same
pubkey on two relays is independently verified. Membership, owner, policy,
freshness, and eligibility do not cross community boundaries.

Migration `0007_agent_directory.sql` stores only bounded public projections.
It introduces no managed-agent JSON control store and no TOML runtime
configuration. Cache purge can reconstruct the directory from relay records.
Historical signed messages are not rewritten when current agent verification is
revoked.

Only exact current bot membership can make a projection visible. Missing,
invalid, removed, and stale states fail closed. Duplicate relay events and
unchanged reconciliation are durable no-ops.

## Privacy and observability

Agent-directory diagnostics are local typed records containing only counts,
durations, and closed outcome enums. They contain no names, pubkeys, owners,
channel IDs, event IDs, relay URLs, tags, profile text, capabilities, policy
members, or messages.

The v0.11 agent diagnostic events are intentionally excluded from the v0.9 OTel
export allowlist. Existing remote export behavior does not expand implicitly.

bzz does not subscribe to, decrypt, index, archive, or display:

- NIP-AE kind `30174` memory;
- NIP-AO kind `24200` observer/control frames;
- NIP-PMA kind `30179` private state; or
- kind `44200` usage metrics.

## Security consequences

### Positive

- Agent spoofing requires defeating signatures, NIP-OA, owner coordination, and
  relay-authored bot membership rather than copying a name or profile.
- A remote event cannot select or execute a local command.
- bzz gains no agent private key or process authority.
- Human acknowledged sends, draft recovery, locked mode, and identity isolation
  remain authoritative.
- Community-scoped records prevent one relay from authorizing another.

### Residual risks

- A cryptographically genuine remote agent can still be malicious,
  compromised, unavailable, or unsafe.
- Agent responses can contain prompt-influenced or hostile text and media; they
  rely on existing rendering and media defenses.
- Public policy is advisory from bzz's perspective. A remote operator can run a
  different effective policy or no runtime at all.
- Presence is ephemeral metadata, not proof of model readiness.
- Sending a message to a remote agent may cause that remote operator's tools to
  act with authority unknown to bzz.
- Relay observers can infer the same public membership and message metadata
  available to other authorized community clients.

The UI therefore says “verified remote agent” and “eligible to invoke,” never
“trusted,” “safe,” “owned by bzz,” or “guaranteed to respond.”

## Rejected alternatives

### Restore or rename the v0.3 Codex drafter

Rejected because it had no agent identity, NIP-OA owner, relay membership, ACP
session, autonomous publisher, or runtime recovery. Renaming would preserve the
wrong trust model.

### Treat every bot membership or kind `10100` profile as verified

Rejected because a name, role, or self-authored declaration cannot prove the
owner allowed the agent identity or policy.

### Read Buzz Desktop local files or invoke Tauri internals

Rejected because `managed-agents.json`, keyring names, process receipts, logs,
and IPC commands are private implementation details. Mutating them would bypass
Desktop's locks, key migration, publication, reconciliation, and process
invariants.

### Ship local hosting in v0.11

Rejected for this release because hosting introduces key custody, child
processes, ACP permissions, local tool authority, autonomous publication,
queues, sessions, memory, observer retention, and cross-platform process-tree
recovery at once. It requires a separate ADR and explicit owner approval.

### Import remote public projections as executable configuration

Rejected. Kind `30177` is sanitized public metadata, not a private runnable
record. It cannot authorize an executable, provider credential, environment
variable, key, workspace, or start-on-launch behavior.

## Compatibility

v0.11 pins `buzz-core` and `buzz-sdk` to the reviewed commit
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. This revision is an explicit
protocol-baseline update, not an unbounded dependency upgrade.

Consumed wire behavior is covered by deterministic fixtures and the pinned Buzz
relay integration. The implementation does not claim that Tauri IPC names,
Desktop JSON formats, ACP `_meta` extensions, or future upstream event shapes
are compatibility contracts.

## Follow-up gate

Local hosting remains unapproved until a new ADR resolves at least:

- separate agent key custody and backup;
- secret transfer without arguments or persistent plaintext;
- child-environment allowlisting;
- ACP permission defaults and user approval;
- autonomous outbox separation;
- process receipts, nonces, crash circuits, and tree cleanup on every target;
- queue, retry, steering, timeout, and dead-letter durability;
- memory and observer retention; and
- the support contract for provisional ACP v2 extensions.
