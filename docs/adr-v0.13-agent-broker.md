# ADR: v0.13 local agent broker and key custody

**Status:** Accepted. Supersedes the agent-key non-goal of
[`adr-v0.11-remote-managed-agent-interoperability.md`](adr-v0.11-remote-managed-agent-interoperability.md).

**Date:** 2026-11-07

## Context

ADR v0.11 limited bzz to interacting with remote managed agents. It held no
agent key, spawned no runtime and published nothing autonomously.

Upstream `buzz-sdk::broker` (block/buzz#6742) defines an agent-to-broker
action contract. An agent holds only its public key and a session credential,
and asks a host to perform named operations:

- `channel.read`, `message.post`, `message.reply`, `reaction.add`;
- `profile.set`, `storage.address`;
- `agents.create`, `agents.update`, `agents.delete`.

Every operation goes through `POST /v1/action` with a bearer credential. The
host authenticates, authorizes, validates and executes. No secret crosses the
boundary in either direction.

## Decision

bzz v0.13 implements a **local broker host**.

1. **Process boundary.**
   - bzz does not spawn, supervise or configure agent processes. The user runs
     them.
   - bzz serves the broker on loopback only (`127.0.0.1`), on a port chosen at
     startup and written with owner-only permissions to bzz's runtime
     directory.
   - Nothing is served while bzz is locked or in cache-only mode.
2. **Keys.**
   - Agent keys are created by bzz and stored with the same backend and
     read-back verification as the human key, under separate references.
   - Agents never receive them.
   - Creating an agent requires an explicit human confirmation that signs the
     owner's NIP-OA `auth` tag once.
3. **Credentials.**
   - One bearer credential per agent, at least 256 bits, shown once, stored
     only as a hash, and revocable from the broker panel.
   - Requests whose credential does not resolve are rejected before any
     parsing beyond the header.
4. **Authority.**
   - Write actions are allowed only in channels where the agent has the
     relay-signed `bot` role, and only within its per-action policy.
   - `channel.read` is limited to channels the agent itself can read.
   - The relay remains the final authority.
5. **Pacing.**
   - Broker publications use a broker priority below every human and
     maintenance class in the v0.12.1 admission scheduler.
   - Each agent has its own bounded queue, and one key pauses all agents.
6. **Automatic owner-signed events.** The only automatic event signed with the
   human key is kind 20003 owner presence for hosted agents. It is opt-in in
   the broker panel and listed there. Human messages still require an
   explicit human action.
7. **Federated identity.** For FI communities, bzz requests NIP-FA agent
   grants under the owner's adapter session and mints agent assertions with
   them. Deleting an agent revokes its grant (`/v1/nip-fi/agent-revoke`).
8. **Observability.** The broker keeps a bounded local activity log with
   action, outcome, timestamp and channel label only. No content enters it,
   diagnostics or telemetry.

## Security consequences

### Positive

- Agents gain a relay presence without holding long-lived secrets.
- Per-action policy and `bot` role checks make every agent action explicit
  and attributable to an owner-visible rule.

### Residual risks

- Any local process that obtains an agent's credential can act as that agent
  within its policy until the credential is revoked. Credentials are
  equivalent to the agent's capability set, not to its key.
- Loopback is reachable by every local user on multi-user hosts. The bearer
  credential, not the network position, is the authority.
- bzz now holds keys for identities other than the human's. A compromise of
  bzz's credential store exposes them too.

## Rejected alternatives

- **Give agents their keys.** That defeats the contract's
  no-secret-crosses-the-boundary rule.
- **Expose a `sign(bytes)` primitive.** That is blind signing, which the
  contract rejects in favor of named operations with policy.
- **Spawn agents from bzz.** Runtime supervision is a separate product with a
  much larger trust boundary, and it stays out of scope.

## Implementation notes (v0.13.0)

Recorded while implementing M9; they refine the decision above.

- **Managed-agent actions.** `agents.create`, `agents.update` and
  `agents.delete` create agents that some runtime must run (runtime, model,
  system prompt). bzz runs none (decision 1), so its host answers them
  `failed`/`unsupported`, which the contract allows for any action. The six
  other actions are fully hosted. Upstream has no reference host and the pinned
  revision lacks `docs/agent-broker.md`; the SDK's own validating client is
  the conformance check.
- **Key custody.** Each agent key is sealed with NIP-44 to the owner's own key
  (`keys/agent-<id>.nip44`, owner-only) and verified by reading it back. It
  opens only while the owner identity is unlocked, on every backend; a Buzz
  Desktop identity cannot host agents. The NIP-OA tag and the NIP-AE memory
  `d` tag are computed inside the signer actors, so no secret key leaves them.
- **Owner on AUTH.** Agent NIP-42 AUTH events carry the owner's NIP-OA tag, the
  relay's path for admitting a non-member agent and pinning its owner. Every
  agent event carries it too.
- **Bot role.** Writes need the relay-signed `bot` role in a channel whose
  roster the owner's directory sees, so the owner must be a member of every
  channel an agent writes in.
- **Admission.** Each agent has its own connection and HTTP client, separate
  from the human's admission budget, plus a per-agent write budget (burst 5,
  one every two seconds).
- **Federated identity.** In FI communities the broker enrolls each agent with
  the owner's stored adapter session (`:signin` first) and the agent's
  connection presents only its own `nip-fi-agent+jwt` evidence. `bzz broker
  delete` revokes the agent first and keeps it if revocation fails.
- **Idempotency.** Replay records are in memory (1024 per agent) and do not
  survive a restart.
- **Management.** `bzz broker create|list|allow|rotate|pause|resume|delete`;
  the TUI `:broker` panel shows agents, policy and the activity log, `p`
  pauses every agent and `o` toggles owner-signed presence.
