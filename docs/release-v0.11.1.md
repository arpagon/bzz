# bzz v0.11.1 — Agent identity and conversation corrections

**Status:** Release candidate 2026-08-25

v0.11.1 is a client-only correctness patch for verified remote-agent
interoperability. It repairs historical exact bot roles on upgrade, recognizes
owner-controlled managed agents in Buzz DMs without relaxing channel authority,
presents verified agent/owner identity in conversation surfaces, and renders
relay system events semantically rather than as raw JSON.

The Buzz compatibility pin remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. No relay deployment or Buzz
Desktop private integration is required.

## Highlights

- Adds schema migration 0008 to rederive `memberships.role` only from the
  current, already-verified, relay-signed kind 39002 source event.
- Repairs a current membership projection from an immutable duplicate event and
  returns to true no-op behavior once rows converge.
- Keeps exact four-field lowercase `bot` authority mandatory outside DMs.
- Treats only current participants in bounded 2–9-person DMs as additional
  discovery candidates, queries kind 10100 before profile data, and projects no
  ordinary human participant as an incomplete agent.
- Allows exact relay-signed bot membership plus valid kind 0 NIP-OA ownership to
  verify older managed identities that publish no kind 10100. A DM-only
  operational member still requires a valid declaration.
- Requires exact current DM participation and active-owner equality before a DM
  invocation can proceed. Any present owner policy must validate; with no public
  policy, only the cryptographically verified owner is eligible.
- Shows a textual `◆` verified-agent marker plus `managed by you` or a sanitized
  verified owner label in timeline, thread/context, selected DM header,
  sidebar, Inbox, mention completion, and Agents detail presentation.
- Retains profile name/avatar presentation for unverified identities without
  claiming agent or owner authority.
- Accepts kind 40099 only from the pinned relay signer and parses a bounded
  allowlist of system event types.
- Renders `dm_created` as `Direct message started with …`; malformed, oversized,
  and unknown records become a content-free unsupported row.
- Excludes system records from authored copy text, unread counts, human-message
  activity, search, Inbox projection, and readiness claims.
- Removes the unconditional five-minute directory refresh. Startup/reconnect,
  relevant membership events, explicit refresh, and exact send-time
  revalidation remain authoritative and coalesced.

## Trust boundary

Names, avatars, replies, declarations by themselves, visible `@name` text, and
`p` tags do not establish an agent or owner. A signed reply proves authorship of
that reply and end-to-end transport only; it does not prove runtime readiness or
future availability.

Selecting an agent still inserts visible draft text and an exact structured
lowercase 64-character `p` tag. The active human reviews and explicitly sends
through the existing acknowledgement-aware outbox. Failure at refresh,
verification, policy, signing, publication, or acknowledgement preserves the
exact draft. bzz stores no agent private key and starts no ACP, model, tool,
memory, observer, provider, or runtime process.

## Migration

Schema version 8 changes only reconstructable membership roles. It does not
rewrite event bytes, profiles, messages, Inbox, search, read state, drafts,
outbox, media, identities, credentials, diagnostics, or telemetry. Existing
v0.11.0 databases are backed up before migration under the normal bounded
backup policy.

Fresh and upgraded databases converge to the same authoritative membership
rows. A cache-only upgrade can reconcile already-present public agent records;
a missing public declaration remains fail-closed for a DM-only member. An exact
bot candidate can be reconstructed from cached NIP-OA profile evidence; absent
public policy remains verified-owner-only.

## System event presentation

The initial reviewed allowlist covers:

- `dm_created`;
- `channel_created`;
- `member_joined`;
- `member_left`;
- `member_removed`;
- `channel_archived`;
- `channel_unarchived`; and
- `message_deleted`.

Control payload JSON remains archived as a signed relay record but is never
exposed as ordinary timeline content. Unsupported payload details are not
copied into UI or diagnostics.

## Explicit non-goals

v0.11.1 does not:

- change or deploy Buzz relay;
- read Buzz Desktop runtime files, Tauri commands, keyring values, process
  receipts, databases, or logs;
- add manual agent approval or a trust list;
- relax exact bot membership for a non-DM channel;
- create, host, wake, stop, inspect, recover, or retry an agent runtime;
- import or store an agent key;
- publish autonomously; or
- ingest NIP-AE memory, NIP-AO observer/control, NIP-PMA private state, or usage
  records.

## Validation

The complete candidate and publication evidence is tracked in
[`validation-v0.11.1.md`](validation-v0.11.1.md). The accepted design is
[`planning/2026-08-25/v0.11.1.md`](planning/2026-08-25/v0.11.1.md), with the DM
compatibility decision recorded as an amendment to
[`adr-v0.11-remote-managed-agent-interoperability.md`](adr-v0.11-remote-managed-agent-interoperability.md).
