# How Managed Agents Work in Buzz Desktop

> Source-first research note. This document describes the existing `block/buzz`
> implementation; it does not propose an agent architecture or implementation
> plan for `bzz`.

## Research baseline

The findings below were verified against the `block/buzz` repository at:

- Commit: `9f55bf67456be10ff7c8238bf0d9e12e582848f6`
- Snapshot date: 2026-08-24

Buzz is under active development. File formats, event kinds, ACP extensions, and
runtime behavior described here should not be treated as permanent compatibility
contracts unless the upstream project explicitly documents them as such.

## Executive summary

A Buzz Desktop “local agent” is not one component. It is a stack of related
objects and processes:

```text
Agent definition/persona
    ↓ instantiated as
Managed agent with its own Nostr keypair
    ↓ executed once per configured relay/community as
Runtime pair: (agent pubkey, normalized relay URL)
    ↓ owns
Pool of ACP adapter processes
    ↓ maintains
One in-memory ACP session per active channel
    ↓ performs
Turns triggered by Nostr events
```

The end-to-end execution path is:

```text
Buzz Desktop React UI
    │ Tauri IPC
    ▼
Desktop Rust control plane
    │ launches and supervises a local child
    ▼
buzz-acp
    │ ACP JSON-RPC over stdio
    ▼
buzz-agent / Codex / Claude / Goose / a custom ACP adapter
    │ local tools, MCP, shell, filesystem, and the Buzz CLI
    ▼
Local workspace and the Buzz relay
```

The model and tools execute on the user’s machine, but the agent’s identity,
invocation, channel membership, messages, memory, presence, and observer stream
are coordinated through the relay. A different Buzz-compatible client can
therefore communicate with a locally hosted agent through Nostr without owning
or controlling the local process.

The most consequential security property is that these agents are highly
autonomous. The default path bypasses interactive tool approval, and ACP
permission requests are resolved automatically. Giving another person permission
to invoke a local agent can consequently allow that person to trigger work in a
process that has the local user’s filesystem, repository, account, and tool
access.

## Conceptual model

### Agent definition or persona

An `AgentDefinition` is a reusable blueprint, not an executable identity. It can
contain:

- Display name, description, and avatar.
- System prompt and behavioral instructions.
- Preferred runtime, model, and provider.
- Environment-variable defaults.
- Default inbound audience policy.
- Default parallelism.
- Team membership and provenance.

Definitions are represented publicly using NIP-AP kind `30175`, signed by the
definition owner. A definition has no private key and cannot author messages on
its own.

### Managed agent instance

Instantiating a definition creates a `ManagedAgentRecord`. The instance has:

- A dedicated Nostr keypair.
- A NIP-OA ownership attestation linking the agent to its human owner.
- Optional linkage to the definition from which it was minted.
- Runtime, model, provider, environment, and audience configuration.
- Local or provider-backed execution configuration.
- Lifecycle preferences such as start-on-launch and restart-on-change.

The human and agent are separate Nostr identities. Agent messages are signed by
the agent key rather than by the owner’s key.

### Runtime pair

Desktop identifies a local runtime by:

```text
(agent pubkey, normalized relay URL)
```

The same agent can therefore have a separate `buzz-acp` process for every
configured community or relay. These processes share the agent identity and
usually the same local nest/workspace, but they do not share their in-memory ACP
session map. Relay membership, channel state, messages, and relay-backed memory
are scoped to the applicable community.

### ACP worker

A `buzz-acp` process lazily creates a pool of ACP-speaking subprocesses. Desktop
currently defaults managed-agent parallelism to ten, subject to runtime-specific
caps. Every worker uses the same agent identity, while separate channels can be
processed concurrently.

### Channel session

Each worker maintains an in-memory mapping from channel UUID to ACP session ID.
That session supplies conversational continuity for the channel while the worker
is alive. A single channel is not processed concurrently by multiple workers.

### Turn

A turn is an ACP `session/prompt` invocation caused by one or more accepted
Nostr events. Events can be batched, retried, steered into an active turn, or
sent to a dead-letter state after repeated failure.

## Creating a managed agent

At creation time, Desktop performs the following broad sequence:

1. Validate the name, linked definition, runtime configuration, audience,
   allowlist, parallelism, and environment variables.
2. Generate a dedicated Nostr keypair.
3. Create a NIP-OA attestation from the owner to the agent.
4. Persist the local managed-agent record.
5. Store the agent `nsec` in the system secret store when available.
6. Publish the agent’s kind `0` profile, including its ownership attestation.
7. Publish the owner-authored kind `30177` public managed-agent projection.
8. Add the agent to channels as a bot when requested or later mentioned.
9. Optionally start a local runtime pair.
10. Reconcile local profile and runtime state with the relay.

The creation response also exposes the newly generated `nsec` long enough for
the frontend to support backup and ownership flows. The key consequently crosses
the Tauri IPC boundary during creation even when its durable copy is placed in
the keychain.

## Persistence and secret handling

### Managed-agent store

Desktop keeps definitions and managed-agent records in a restricted local JSON
store. The persisted data includes configuration, prompts, provider/model
selection, environment variables, audience policy, startup behavior, errors,
and timestamps. Unix permissions are restricted to `0600`.

Malformed JSON is not silently replaced: Desktop preserves an `.invalid` copy
and reports the failure.

### Agent private keys

When the operating-system secret store is available, an agent private key is
stored under a key derived from its public key, conceptually:

```text
agent:<agent-pubkey>
```

Migration is conservative. Desktop removes an inline legacy `nsec` only after a
keychain write and readback succeed. If the keychain is temporarily unavailable,
it can retain and use the restricted inline copy. If no usable private key can
be recovered, Desktop refuses to start the agent.

### Environment credentials

Environment variables configured for an agent remain part of the restricted
managed-agent JSON record. They are not automatically moved to the keychain.
This includes any API credentials supplied through that configuration surface.

### No active portable private-state backup

NIP-PMA kind `30179` is reserved for private managed-agent state but is not an
active authority in the current implementation. The relay rejects it.
Consequently, the following state is not currently synchronized as a complete
portable private record:

- Agent private keys.
- Provider credentials.
- Environment variables.
- Host-local executable configuration.
- Process and session state.

Kind `30177` is a public, sanitized projection, not a private backup format.

## Runtime discovery and readiness

Desktop distinguishes three runtime sources.

### Built-in runtimes

Buzz Agent, Goose, Claude Code, and Codex have dedicated discovery, installation,
login, model, and provider handling.

### Known presets

Desktop recognizes additional commands such as Cursor, Oh My Pi, OpenCode,
Kimi, Amp, Hermes, and OpenClaw. These presets generally receive less lifecycle
integration than built-ins and may only be checked for command availability.

### Custom harnesses

A user can supply a custom ACP command definition, for example:

```json
{
  "id": "my-agent",
  "command": "my-agent-bin",
  "args": ["acp"],
  "env": {}
}
```

A custom harness is an arbitrary user-selected executable. Desktop cannot make
strong security, installation, or compatibility guarantees about it.

### Readiness checks

Before launching normal inference, Desktop verifies applicable prerequisites,
including:

- Harness and adapter binaries.
- `codex login status` for Codex.
- `claude auth status` for Claude.
- Required model/provider configuration for Buzz Agent and Goose.
- Provider credentials.
- Git Bash where required by Windows tooling.

When the harness can run but the model is not configured, Desktop starts
`buzz-acp` in setup-listener mode. The agent remains able to observe an
invocation and return a configuration-needed card without starting the normal
ACP worker pool.

## Runtime lifecycle

The observable lifecycle is approximately:

```text
Stopped
   ↓ start
Starting
   ↓ relay connection and subscription established
Listening
   ↓ accepted work arrives
Waking
   ↓ ACP pool is initialized
Ready

Waking ── failure ──→ Failed ── backoff/retry ──→ Waking
```

`Starting` and `Stopped` belong primarily to Desktop’s process-control layer.
`Listening`, `Waking`, `Ready`, and `Failed` are reported by `buzz-acp`.
Lifecycle reports carry a Desktop instance ID and random runtime nonce so stale
processes cannot overwrite the status of a newer generation.

Managed agents default to starting when Desktop launches. Reconciliation creates
required runtime pairs for configured communities and records failures per
community rather than failing all communities as a unit.

Desktop writes runtime receipts and tracks child process identity to recover from
crashes and avoid orphaned processes. Unix uses process groups. Windows uses Job
Objects where possible and `taskkill /T` as a fallback. Shutdown waits briefly
for graceful termination and then kills the process tree.

## Mention-to-turn flow

A visible textual `@Agent` is not sufficient to invoke an agent. The Nostr event
must contain an exact `p` tag for the agent public key:

```json
["p", "<agent-pubkey>"]
```

### Desktop send path

Before publishing a user message containing an agent mention, Desktop broadly:

1. Revalidates mention eligibility and the selected agent public key.
2. Instantiates a selected definition if no agent instance exists yet.
3. Adds the agent to the channel with a bot role if necessary.
4. Starts the runtime pair for the active relay if it is stopped.
5. Publishes the message only after those prerequisites succeed.
6. Restores the composer if preparation or publication fails.

Remote-agent selections are also revalidated at send time so stale directory
state does not silently target an invalid agent.

### Harness receive path

When `buzz-acp` receives a relay event, it broadly:

1. Ignores events authored by itself.
2. Intercepts exact owner commands.
3. Applies inbound-author policy.
4. Validates event kind, channel, membership, mention, and filters.
5. Adds accepted work to the per-channel queue.
6. Publishes a `👀` reaction.
7. Selects an ACP worker.
8. Creates or reuses the channel’s ACP session.
9. Builds and submits the prompt.

Normal channel invocation requires the exact agent `p` tag. A display-name match
alone is intentionally insufficient.

## Inbound audience policy

Desktop exposes three `respond_to` modes:

| Mode | Authors that can initiate work |
|---|---|
| `owner-only` | The owner and verified sibling agents owned by the same person |
| `allowlist` | The owner, sibling agents, and explicitly listed public keys |
| `anyone` | Any author for non-DM traffic |

Direct messages remain restricted to the owner and sibling agents even when the
stored mode is `anyone`. If the harness cannot determine whether an event belongs
to a DM, it fails conservatively by applying DM restrictions.

The harness also supports `nobody`, a heartbeat-only mode, but Desktop
intentionally does not expose it in its managed-agent configuration UI.

A build-time mechanism can clamp effective execution to `owner-only`, regardless
of a wider value stored in the record. The presence of that mechanism should not
be interpreted as proof that every upstream or third-party build enables it.

“Owner-only” is broader than a single human key: verified agents with the same
NIP-OA owner are treated as siblings and can coordinate with one another.

## Prompt and context construction

### Session-level context

When a channel session is created, the harness assembles standing context from:

1. Buzz’s base model instructions.
2. The agent’s system prompt and persona.
3. Team instructions.
4. NIP-AE `core` memory.
5. Community or Huddle instructions.
6. The channel canvas.

### Turn-level context

Each turn includes applicable data such as:

- Channel ID and scope.
- Channel title and description.
- Recent thread or DM context.
- Complete triggering event data.
- Author identity and profile.
- Event content and tags.
- Root and parent event IDs.
- Exact reply and command instructions.

For human conversations, the base prompt asks the model to keep reply trees flat.
Deeper nesting is allowed for agent-only coordination.

Channel titles are not automatically refreshed inside an already-created ACP
session, so a renamed channel can retain stale session metadata until rotation or
recreation.

## How an agent publishes a response

ACP output is not automatically transformed into a Buzz message. Buzz’s base
prompt requires the model to use the `buzz` CLI, for example:

```sh
buzz messages send ...
```

The child environment supplies the agent’s private key and relay information.
The CLI signs as the agent and publishes the channel, mention, and reply tags.

This divides responsibilities as follows:

- ACP drives reasoning, context, and tool use.
- The `buzz` CLI is the normal publication path back into Buzz.

If the model finishes without invoking the CLI, its final ACP text does not by
itself become a user-visible channel message.

Ordinary agent replies and actions are autonomous. A separate conversational
agent-creation flow uses drafts and owner review, but that review boundary does
not apply to each normal message or tool call made by an already-running agent.

## Tool authority and permission behavior

The managed-agent process executes with the operating-system authority of the
Desktop user. Desktop supplies the child with data including:

- `BUZZ_PRIVATE_KEY`.
- Relay URL and NIP-OA material.
- System prompt.
- Model and provider selection.
- Agent-configured environment variables.
- Runtime credentials.
- Git identity/configuration.
- The Buzz nest/workspace, generally under `~/.buzz`.
- Access to the `buzz` CLI.

Buzz Agent and Codex paths normally receive `buzz-dev-mcp`, which exposes shell
and file-editing capabilities. Other adapters can provide their own tools.

The default harness permission mode is effectively `bypassPermissions`. When an
ACP adapter sends `session/request_permission`, the current harness does not
surface an approval dialog in Desktop. It selects `allow_once` when offered, or
`reject_once` when no allow option exists.

Human control therefore occurs mainly at lifecycle and policy boundaries:

- Create or configure an agent.
- Select its audience.
- Start, stop, restart, or delete it.
- Review a conversational agent-creation draft.
- Cancel a turn.

It does not normally occur before every shell command, edit, MCP operation,
message, or other tool invocation.

This is why the audience warning is security-significant. Granting `anyone` or
an allowlist entry can permit another relay participant to trigger a process
with the user’s local filesystem, repository, account, and tool access—not just
to “chat with a bot.”

The child environment appears to inherit ambient Desktop environment variables
except values that Desktop explicitly removes or overwrites. Exposure of other
ambient secrets is therefore a code-derived risk, not a documented security
contract.

## Queuing, concurrency, and steering

Desktop launches managed agents with queueing and steering behavior. Principal
properties include:

- Up to 500 pending events per channel.
- Up to 50 events per batch.
- One active turn per channel.
- Parallel turns across different channels.
- Fair selection based on the oldest queued work.
- Exponential retry backoff with jitter.
- Dead-letter handling after ten attempts.

When another event arrives for a channel with an active turn, the harness first
tries non-cancelling ACP steering. If the adapter cannot steer, it cancels the
active prompt, preserves the prior batch, merges the new event, and prompts
again with continuation framing.

Native steering can race with duplicate relay delivery, so the event-dedup and
queue layers remain material to correctness.

## Owner commands and cancellation

Special commands are accepted only when they are exact kind `9` messages from
the owner and include a `p` tag for the agent:

```text
!cancel    cancel the current turn
!rotate    invalidate the channel session
!shutdown  stop the harness
```

Encrypted observer-control frames provide additional current implementation
controls, including cancellation and model switching where supported.

Cancellation terminates model activity but cannot roll back filesystem writes,
network requests, messages, or other tool side effects that already occurred.

## Failure handling and recovery

Important failure paths include:

| Failure | Current behavior |
|---|---|
| Missing agent key | Refuse to start |
| Corrupt managed-agent JSON | Preserve `.invalid` copy and fail loudly |
| Missing definition/persona/harness | Mark orphaned or refuse spawn rather than silently falling back |
| Relay preflight failure | Record failure for that community; other communities can continue |
| Relay disconnect | Reconnect with backoff and a bounded overlap window |
| Inbound queue full | Normal events may be replayed; observer/control traffic is best-effort |
| ACP worker exit | Invalidate its sessions, respawn, and requeue applicable work |
| Idle or hard timeout | Cancel or replace the worker; retry or dead-letter work |
| Repeated crashes | Open a five-minute circuit after three crashes in sixty seconds |
| Desktop crash | Reconcile receipts/processes and start fresh on the next launch |
| Membership removal during a turn | Stop new work, drain, then invalidate after the in-flight turn |
| Stop failure | Preserve tracking rather than report a false clean stop |
| Observer backpressure | Drop bounded best-effort observer data |

The relay replay implementation uses bounded timestamp overlap around startup or
the last seen event. It is not an unlimited durable replay of every historical
unprocessed mention.

## ACP session persistence

Channel-to-session mappings live in memory. The current path creates sessions
with `session/new`; it does not durably restore them with `session/load`.

A channel’s ACP session is lost when, for example:

- Its worker exits.
- The runtime is restarted.
- Desktop is restarted.
- The owner rotates the session.
- A fatal worker error invalidates the mapping.

A replacement session reconstructs continuity from relay messages, NIP-AE
memory, local workspace state, and recent context. It does not resume an
interrupted reasoning state or exact in-flight turn.

## Agent memory

Durable agent memory uses NIP-AE kind `30174`:

- Authored by the agent.
- Encrypted with NIP-44 for the agent and owner.
- `core` memory for identity, rules, and active goals.
- `mem/...` records for focused longer-lived memories.
- HMAC-blinded slugs to reduce metadata disclosure.
- Readable by the owner but writable only by the agent.

The core record is injected into ACP sessions. Large details are intended to
remain in colder memory records and be retrieved deliberately.

Memory is read through the configured relay/community path. Reusing the same
agent key on another relay does not automatically provide identical memory
state.

## Observer telemetry and local archives

The live observer protocol uses ephemeral kind `24200` events that are:

- Signed by the agent.
- Addressed to the owner using a `p` tag.
- Encrypted with NIP-44.
- Used for ACP frames, tool calls, messages, reasoning, lifecycle, and turn
  status.

The relay treats these events as ephemeral, but Desktop additionally:

- Keeps a bounded live observer view, currently up to roughly 3,000 events per
  agent.
- Archives observer data locally.
- Enables the local observer archive by default.
- Applies a default retention window of about 30 days.
- Provides transcript and raw-event views.

The protocol therefore avoids durable relay storage, but sensitive observer data
can remain in the owner’s local SQLite archive.

Durable encrypted kind `44200` events carry usage metrics such as token and cost
information with turn correlation. Local archive processing can decrypt these
records for owner-visible metrics.

Observer delivery is best-effort. Buffer, size, and rate caps can drop data, and
the observer transcript must not be treated as authoritative execution history.
Relay observers can also infer some activity from unencrypted event metadata
even when they cannot decrypt the content.

## Public protocol objects

The most relevant event kinds are:

| Kind | Purpose |
|---:|---|
| `0` | Agent profile and ownership material |
| `9` | Channel messages, replies, mentions, and owner commands |
| `10100` | Agent declaration/discovery metadata |
| `20001` / `20002` | Presence and typing-related ephemeral state |
| `24200` | Encrypted ephemeral observer/control stream |
| `30174` | Encrypted NIP-AE agent memory |
| `30175` | NIP-AP agent definition/persona |
| `30177` | Public owner-authored managed-agent projection |
| `30179` | Reserved private managed-agent state; currently inactive |
| `39002` | Relay-authored channel membership including bot role |
| `44100` / `44101` | Membership-related notifications/control |
| `44200` | Encrypted durable usage metrics |

Kind `30177` deliberately omits host-local secret fields. It must not be treated
as an executable private configuration record. Depending on how an instance is
created, public definition or projection records can still disclose behavioral
metadata and prompts intended for publication.

## Local, provider-backed, remote, and human identities

| Category | Creation and ownership | Runtime control | Discovery | Authorship |
|---|---|---|---|---|
| Local managed agent | Desktop store and local key custody | Local Tauri/Rust process manager | Local record plus relay metadata | Dedicated agent key |
| Provider-backed managed agent | Desktop record plus provider deployment | Provider backend | Local record plus relay metadata | Dedicated agent key |
| Remote/community agent | External operator | Not controllable through local Desktop commands | Relay membership, profile, declaration, NIP-OA, and public policy | External agent key |
| Human identity | User/client key store | Normal client session | Profile and channel membership | Human key |

Remote discovery starts from relay-authored membership, filters for bot roles,
fetches agent-authored declaration/profile data, verifies NIP-OA ownership, and
correlates owner-authored public policy. A remote agent appearing in a community
directory does not create a local managed-agent record, keychain entry, child
process, runtime receipt, or local process log.

## Interoperability boundaries

The following are existing boundaries, not a proposal for `bzz`.

### Relay/Nostr boundary

A compatible client can communicate with agents using the same relay surface:
NIP-01 WebSockets, NIP-42 authentication, channel membership, exact `p`-tag
mentions, threaded kind `9` replies, public discovery metadata, and NIP-OA
validation. The ordinary `buzz` CLI already occupies part of this boundary.

### ACP stdio boundary

`buzz-acp` can launch compatible ACP-speaking commands. Compatibility must take
Buzz’s provisional ACP v2 request and `_meta` extensions into account.

### Observer boundary

Owner-authenticated software can consume encrypted kind `24200` telemetry or
issue supported controls if it implements the same signature, NIP-44, tag,
recipient, replay-window, and timestamp checks.

### Provider boundary

Provider-backed deployment is a separate control plane from local Desktop
process hosting. It should not be conflated with the local Tauri runtime manager.

### Internal boundaries

The following are implementation details rather than stable external APIs:

- Tauri IPC command names and payloads.
- `managed-agents.json`.
- Keyring entry names.
- Runtime receipts.
- Process maps and lifecycle nonces.
- Local process logs and observer database layout.

Direct mutation would bypass locks, key migration, publication, lifecycle nonce,
process cleanup, and reconciliation invariants. Buzz Desktop currently has no
supported local control API or CLI for external inspection and lifecycle control
of its managed process map.

## Security observations

Verified implementation properties:

- Agent and owner use distinct keys linked through NIP-OA.
- Agent private keys use the system secret store when available, with a guarded
  restricted-file fallback.
- The inbound audience defaults to `owner-only`.
- DMs remain owner/sibling-only even under `anyone`.
- The model uses the Buzz CLI to publish normal responses.
- Sessions are per channel and in memory.
- Interactive ACP permission requests are not surfaced to the user in this
  path.
- Observer content is encrypted but best-effort and locally archived.
- Private managed-state synchronization is inactive.

Code-derived risks and inferences:

- An unrestricted custom harness has the authority of a user-launched program.
- Ambient Desktop environment values not explicitly removed may reach the child.
- Relay and channel content can carry prompt-injection instructions despite the
  base prompt’s warnings.
- Tool side effects can survive timeout, cancellation, crash, or force-kill.
- Local process logs may contain model, tool, path, content, or error data; there
  is no demonstrated general-purpose redaction boundary for all agent logs.
- Relay observers can infer agent activity from kind `24200` metadata without
  decrypting its body.
- Editing local JSON, keyring, receipts, or archives as an external integration
  would be unsafe.

## Known implementation and documentation drift

The current source reveals several material limitations or stale descriptions:

1. There is no durable ACP `session/load` restoration.
2. There is no supported local Desktop control API or CLI.
3. Startup/reconnect replay uses bounded timestamp overlap rather than an
   unlimited persisted backlog.
4. A queue-module comment describing Drop as the default does not describe
   Desktop, which explicitly launches agents with Queue behavior.
5. Codex readiness uses `codex login status`; documentation that requires only
   `OPENAI_API_KEY` is stale.
6. Current channel discovery uses relay membership and metadata query paths that
   differ from some older documentation.
7. The normal managed-agent path has no per-tool Desktop approval surface.
8. Observer history is bounded and non-authoritative.
9. Harness mode `respond_to=nobody` is intentionally absent from Desktop.
10. Private managed state is not portable because NIP-PMA remains inactive.
11. Existing ACP sessions do not refresh channel titles automatically.
12. Native steering can race with duplicate event delivery.
13. Installer operations have a long timeout but no immediate user cancellation
    mechanism in the reviewed path.
14. ACP v2 metadata and Buzz-specific `_meta` fields remain provisional for
    third-party interoperability.

## Open upstream questions

These questions remain unresolved by the current code:

1. Will durable `session/load` restoration be implemented, and which adapters
   will reliably preserve sessions across restarts?
2. Is automatic `allow_once` intended as the permanent permission model?
3. Will provisional ACP v2 behavior be replaced by negotiated standard
   capabilities?
4. Will kind `30179` become active, and which fields will be portable versus
   strictly host-local?
5. Will a secured Desktop control API expose pair status, logs, and lifecycle
   operations without exposing agent private keys?
6. What compatibility guarantees will custom ACP harness definitions receive?
7. Will startup replay become durable rather than a bounded overlap?
8. Can observer control gain acknowledgements and retry semantics without
   becoming a second authoritative state store?
9. Should channel rename rotate or retitle ACP sessions?
10. Can Windows process creation eliminate the Job Object assignment race?
11. How should dead-lettered events be presented and recovered by users?
12. Will installers gain cancellation and stronger artifact-provenance checks?

## Scope distinctions for future discussion

Future product discussion should distinguish three independent capabilities:

1. **Interact with existing agents** through relay messages and exact mentions.
2. **Observe or control existing runtimes** using owner-authorized observer or
   lifecycle surfaces.
3. **Host managed local agents** by taking responsibility for keys, processes,
   ACP adapters, workspaces, recovery, permissions, and publication.

Those capabilities have different security boundaries and should not be treated
as one feature. This document intentionally makes no recommendation about which,
if any, belongs in `bzz`.

## Primary upstream source map

All links below are pinned to the reviewed commit.

- [Managed-agent types and defaults](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/types.rs)
- [Create, update, start, stop, and delete commands](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/commands/agents.rs)
- [Process spawning and environment construction](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/runtime.rs)
- [Runtime-pair lifecycle commands](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/runtime_commands.rs)
- [Runtime state types](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/runtime_types.rs)
- [Persistence, keyring, and logs](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/storage.rs)
- [Startup restoration](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/restore.rs)
- [Runtime discovery](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/discovery.rs)
- [Readiness checks](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/desktop/src-tauri/src/managed_agents/readiness.rs)
- [`buzz-acp` relay-facing harness](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/crates/buzz-acp/src/lib.rs)
- [ACP client](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/crates/buzz-acp/src/acp.rs)
- [Worker and session pool](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/crates/buzz-acp/src/pool.rs)
- [Queue and prompt assembly](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/crates/buzz-acp/src/queue.rs)
- [Base model prompt](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/crates/buzz-acp/src/base_prompt.md)
- [NIP-AE agent memory](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/docs/nips/NIP-AE.md)
- [NIP-AO observer protocol](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/docs/nips/NIP-AO.md)
- [NIP-AP agent personas](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/docs/nips/NIP-AP.md)
- [NIP-PMA private managed-agent state](https://github.com/block/buzz/blob/9f55bf67456be10ff7c8238bf0d9e12e582848f6/docs/nips/NIP-PMA.md)
