# Deferred research — Owner-private live agent activity

**Status:** Deferred 2026-08-25 in favor of the bounded v0.11.3 typing-feedback plan

**Historical naming note:** References to v0.12.0 below preserve the original
proposal language only. They do not assign this research to a release. The
approved v0.12.0 scope is the themed agent status bar and message-selection
presentation plan dated 2026-08-28.

**Decision note:** Android and Buzz Desktop already provide the immediate useful
feedback through signed ephemeral kind `20002` typing indicators. Decrypting
NIP-AO observer frames would add a materially larger privacy and compatibility
boundary for a similar initial presentation result. This proposal remains a
research option without a committed target version; it is not approved for
implementation.

**Prerequisite if reconsidered:** publish and verify the accepted v0.11.x
interaction baseline, then obtain a new explicit approval before implementation.

**Research baseline:** `block/buzz` commit
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`, as recorded in
[`docs/how-agents-works-in-buzz.md`](../../how-agents-works-in-buzz.md).

## Recommendation

The next agent increment should be **observation without control or hosting**.

v0.11 established capability 1 from the research note:

1. interact with an existing agent through signed relay messages.

v0.12 should implement only the read-only half of capability 2:

2. observe bounded, owner-private, live activity from an existing externally
   hosted runtime.

It must not implement the write/control half of capability 2 and must not begin
capability 3:

3. host a managed local agent.

This sequencing closes the most important current interaction gap: after an
exact human-authored mention, an owner can tell whether the external agent
reported that it started, remains active, or completed a turn instead of
inferring runtime state from presence, a `👀` reaction, or the eventual reply.
It does so without giving bzz an agent key, executable, workspace, provider
credential, ACP process, autonomous publisher, or lifecycle authority.

## Release thesis

v0.12.0 will add an explicitly enabled, live-only **agent activity signal** for
verified remote agents cryptographically owned by the active bzz identity.

When enabled, bzz will subscribe to owner-addressed NIP-AO kind `24200`
telemetry on the already authenticated community relay, verify and decrypt it
with the active human identity, reduce a strict allowlist of lifecycle events to
small presentation states, and immediately discard the raw frame and decrypted
payload.

The user-visible vocabulary is deliberately narrow:

- `working in this channel`;
- `activity observed recently`;
- `turn completed`;
- `turn ended with an issue`;
- `activity stream interrupted`; or
- `activity unavailable`.

These are best-effort reports from the external runtime. They do not prove that
a model is ready, safe, online, healthy, making progress, or guaranteed to
reply.

v0.12.0 will not display ACP transcripts, prompts, reasoning, tool calls, shell
commands, file paths, model output chunks, errors, usage, costs, or memory. It
will not archive observer frames and will not publish observer controls.

## Why this is the next safe step

### Product value

v0.11 lets a human invoke an agent correctly but leaves a long-running turn
visually ambiguous. Presence cannot solve that ambiguity: kind `20001` is only
presentation state, and a reaction proves only that an event was authored.
NIP-AO carries direct, owner-addressed turn lifecycle signals.

A compact activity indicator makes remote-agent conversations understandable
without inventing unsupported readiness claims.

### Smaller authority increase than control

Observation requires the existing human key only to decrypt data addressed to
its owner. It does not publish as the agent or ask the runtime to act.

By contrast, `cancel_turn`, model switching, `!rotate`, and `!shutdown` are
state-changing commands. They are best-effort, version-sensitive, can race with
side effects, and require an acknowledgement/retry product model. They remain
out of scope until observation has proven its authorization, privacy, and
reconnect behavior independently.

### Much smaller boundary than hosting

Local hosting would require all unresolved gates from the v0.11 ADR: agent key
custody, executable provenance, child-environment isolation, ACP permissions,
autonomous publication, process-tree recovery, durable queues, session
semantics, memory, and observer retention. None of those responsibilities is
needed for read-only live activity.

## Source findings that shape the scope

The reviewed Buzz source establishes the following relevant behavior:

- kind `24200` is signed, NIP-44-v2 encrypted, owner-addressed telemetry;
- telemetry is agent → owner and control is owner → agent;
- the relay checks the agent-owner relationship, but clients must still verify
  signatures, tags, and ownership themselves;
- observer delivery is best-effort and can be dropped by queues, reconnects,
  rate limits, or relay behavior;
- observer payloads can contain highly sensitive ACP frames, model messages,
  tool inputs, paths, commands, and errors;
- the implementation emits useful content-free lifecycle kinds including
  `managed_agent_runtime_lifecycle`, `turn_started`, `turn_liveness`,
  `turn_completed`, and `turn_error`;
- the implementation can batch multiple observer events into a `batch`
  envelope even though that extension is not fully represented by the short
  NIP-AO draft;
- Buzz Desktop archives observer data locally by default, but that is an
  upstream product decision, not a requirement for interoperability; and
- observer controls remain advisory and do not roll back already completed
  filesystem, network, tool, or publication side effects.

The source and draft NIP have material drift. v0.12 therefore treats every
accepted lifecycle shape and batch envelope as an explicitly pinned
compatibility fixture, not a permanent generic observer API.

The Buzz dependency/protocol baseline remains
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`. Any newer upstream revision needed
to complete M0 requires a separate source review, compatibility diff, fixture
update, and explicit plan amendment; it is never an incidental dependency
upgrade.

## Product invariants

### Identity and authorization

1. Only the unlocked active human identity can decrypt activity addressed to
   that exact pubkey.
2. The outer event must have a valid Nostr signature.
3. `event.pubkey`, the exact `agent` tag, and the candidate verified-agent
   pubkey must be identical lowercase 64-character keys.
4. The exact single `p` tag must equal the active human pubkey.
5. The exact single `frame` tag must be `telemetry`; bzz never accepts a
   control frame as activity.
6. The agent must currently be a verified community-scoped agent and its
   verified NIP-OA owner must equal the active identity.
7. Outside a positively identified DM, current exact relay-signed `bot`
   membership remains required. The v0.11.1 DM exception remains unchanged.
8. Relay authorization is defense in depth, never a substitute for local
   signature, ownership, community, and destination checks.
9. The same agent and owner on another relay do not authorize activity in the
   current community.

### Live-only privacy

1. Agent activity observation is default-off and requires explicit owner
   enrollment.
2. Subscriptions begin at the current time. bzz requests no historical
   observer replay, no `until`, and no event-ID query.
3. Ciphertext events are never inserted into `events`, messages, Inbox, search,
   read state, drafts, outbox, diagnostics, support reports, or telemetry.
4. Decrypted plaintext is bounded before structured parsing and is never
   written to SQLite, files, logs, panic messages, diagnostics, OTel, tests, or
   snapshots.
5. Raw payload fields are never retained. The reducer keeps only normalized
   state enums, coarse timestamps, transient source-order/replay metadata,
   community/agent/channel keys, and a process-random keyed digest of the turn
   identifier needed to reject cross-turn terminal/liveness signals.
6. ACP frames, prompts, reasoning, model text, tool calls, commands, paths,
   errors, environment values, usage, and memory are ignored and discarded.
7. Lock, identity switch, community removal, disable, and normal shutdown clear
   all in-memory activity immediately.
8. Crash recovery reconstructs no observer history. After restart the state is
   `activity unavailable` until a new live signal arrives.
9. Support reports expose only bounded aggregate counters and closed reason
   enums. Agent activity diagnostics remain excluded from OTel.
10. Tests use generated disposable owner/agent keys and synthetic payloads,
    never production ciphertext or decrypted content.

### No new control authority

1. bzz publishes no kind `24200` events in v0.12.
2. There is no cancel, rotate, shutdown, retry, wake, start, stop, restart,
   model-switch, prompt, or tool-approval action.
3. No activity signal can trigger a human message, an agent message, a
   reaction, a read acknowledgement, or any local executable.
4. Existing structured mentions and the human acknowledgement-aware outbox
   remain the only bzz publication path related to agents.
5. Activity does not modify verification, membership, owner, or invocation
   eligibility.

## Explicit enrollment

Add a strict non-secret configuration surface:

```toml
[agent_activity]
enabled = false
```

Properties:

- default is `false` on fresh and upgraded configurations;
- the section configures only owner-private observation, never an agent runtime;
- unknown fields fail closed under the existing strict TOML policy;
- a confirmed in-TUI enable action persists the setting and opens the
  applicable relay subscription without restarting or waking any agent;
- disabling persists the setting, closes the subscription, and clears every
  transient projection;
- `bzz check` explains invalid configuration without exposing identity or relay
  data; and
- no configurable archive, retention period, plaintext log, raw-frame view, or
  buffer-size override exists in v0.12.

The TUI action must explain the sensitive-data boundary and require explicit
confirmation before persisting enrollment. A single accidental keypress cannot
enable it. Equivalent CLI `status`, `enable`, and `disable` commands may edit
only this boolean; a CLI change takes effect in an already running TUI only
after an explicit in-app reload or the next launch.

## Protocol admission pipeline

The admission order is security-significant. Reject cheaply before decryption:

1. receive the event from the active community session's dedicated observer
   subscription;
2. enforce kind `24200`, total event size, content size, tag count, and exact
   tag cardinality;
3. verify the Nostr event ID and signature;
4. require exactly one `p`, `agent`, and `frame` tag;
5. require `frame=telemetry`;
6. require `p=active human`, `agent=event.pubkey`, and lowercase canonical
   pubkeys;
7. if an `h` tag exists, require exactly one valid UUID belonging to the same
   community; it cannot independently grant channel authority;
8. require a current verified agent projection whose NIP-OA owner is the active
   human and whose current destination authority remains valid;
9. reject replayed event IDs and events outside a bounded ±5-minute freshness
   window;
10. decrypt with NIP-44 v2 using the active human key and exact agent pubkey;
11. reject plaintext over 65,535 bytes before JSON reduction;
12. parse one observer event or one depth of the pinned `batch` envelope with
   strict item/count/size bounds;
13. reduce only the allowlisted lifecycle fields, deriving a process-random
    keyed digest when a turn identifier is needed for correlation; and
14. discard ciphertext, plaintext, payload values, session IDs, raw turn IDs,
    and triggering event IDs after reduction.

Unknown kinds and well-formed payload extensions are ignored without becoming
errors visible to the user. Malformed known kinds fail closed and cannot update
activity.

## Accepted lifecycle projection

### `turn_started`

Required:

- exact channel UUID in the observer envelope;
- bounded canonical turn identifier while processing the frame; and
- source equal to `channel`.

Effects:

- mark that exact verified agent as `working` in that exact community/channel;
- record only a coarse local `observed_at` timestamp in memory; and
- schedule bounded expiry.

Ignore heartbeat turns and discard `triggeringEventIds` rather than correlating
them to persisted messages in v0.12.

### `turn_liveness`

A valid current-turn liveness signal refreshes only the expiry deadline. It
cannot create a turn without an admitted `turn_started`, revive a completed
turn, or move activity across channel, agent, or community.

### `turn_completed`

A valid matching terminal signal changes `working` to `completed recently` for
a short presentation window, then expires to no live activity.

### `turn_error` and `agent_panic`

A valid matching signal changes the state to `ended with an issue`. Error text,
codes, stack data, and payload details are discarded and never displayed.
A later matching `turn_completed` may close the turn but does not erase the
short issue indication.

### `managed_agent_runtime_lifecycle`

Accept only normalized lifecycle enums emitted by the pinned baseline. Verify
that any embedded pubkey equals the outer signer. Discard relay URLs, nonces,
errors, process details, and all unknown fields.

The UI may say `runtime reported listening/waking/ready/failed` with a timestamp.
It must never collapse this to `online`, `healthy`, or `ready to answer`, and it
must never use lifecycle as invocation authority.

### `batch`

Accept one wrapper level, a fixed maximum number of inner events, a fixed total
plaintext budget, and deterministic inner order. Nested batches, duplicate
inner sequence keys, over-limit arrays, and malformed inner records fail closed
for the wrapper. Sensitive ignored records may coexist with lifecycle records;
no ignored payload becomes retained merely because it arrived in a batch.

### Always ignored

At minimum:

- `acp_read` and `acp_write`;
- prompts and session configuration;
- agent/model/reasoning/message chunks;
- tool calls, tool results, commands, paths, diffs, and terminal output;
- memory and canvas material;
- raw errors and process logs;
- `control_result` because bzz publishes no controls; and
- unknown future event kinds.

## Transient state model and bounds

Use a dedicated in-memory reducer keyed by:

```text
(community_id, agent_pubkey, channel_id)
```

Each active entry may retain a process-random keyed digest of the raw turn ID.
The digest is never persisted, displayed, logged, exported, or reused after
restart; the raw ID is discarded immediately after derivation.

Required bounds:

- at most 64 observed verified agents per active community;
- at most 256 live/recent turn projections total;
- at most 1,024 replay event IDs;
- at most 50 inner records per batch;
- at most 65,535 decrypted bytes per relay frame;
- a bounded command/decrypt queue separate from local input and sends;
- one coalesced redraw per visible state change;
- no database writes or redraws for ignored, duplicate, stale, or unchanged
  liveness frames; and
- one scheduled expiry deadline rather than per-frame or per-turn polling.

Suggested presentation windows:

- `working`: valid until 120 seconds after the most recent admitted start or
  liveness signal;
- `completed recently` / `ended with an issue`: 30 seconds; and
- runtime report freshness: 120 seconds, after which wording becomes
  `last reported …` or disappears.

The exact liveness timeout must be validated against the pinned harness cadence.
Missing or dropped telemetry produces `activity unknown`, never `completed`,
`failed`, or `offline`.

## Signer boundary

Extend `SignerHandle` with one narrowly typed operation conceptually equivalent
to:

```text
decrypt_from(agent_pubkey, ciphertext) -> bounded plaintext
```

Requirements:

- validate the peer pubkey before queueing;
- keep the human secret key inside the existing signer task;
- use NIP-44 v2 only;
- use content-free error variants;
- never include ciphertext, plaintext, pubkeys, relay URLs, or payload details
  in errors or diagnostics;
- clear intermediate buffers as far as supported by the selected libraries;
- keep queue capacity bounded and let agent activity drop before it can starve
  AUTH, read-state, signing, or acknowledged human publication; and
- lock terminates pending observer work and clears transient state.

v0.12 does not expose generic arbitrary-peer decrypt functionality through the
CLI or UI.

## Subscription lifecycle

Add a dedicated desired subscription only when all gates hold:

- `agent_activity.enabled=true`;
- identity unlocked;
- community authenticated and active; and
- at least one current verified agent is owned by the active identity.

Filter shape:

```json
{
  "kinds": [24200],
  "#p": ["<active-human-pubkey>"],
  "since": 1777410000
}
```

Do not add `until`, `ids`, or historical lookback. If the relay requires a
bounded `limit`, use it only as an implementation safety cap and never as a
request for prior history.

On reconnect:

1. clear active-turn claims or mark them interrupted;
2. create a fresh `since=now` subscription;
3. accept only new live signals; and
4. do not infer completion for a turn whose terminal frame was missed.

A terminal `CLOSED` or authorization rejection is quarantined until reconnect,
explicit retry, ownership refresh, or configuration change. It must not create
a retry or redraw loop.

## User experience

### Channel activity row

When an owned verified agent has an admitted active turn in the selected
channel, render a compact row above the composer or immediately below the
channel title:

```text
◆ Fizz · working in this channel · observed just now
```

Terminal states briefly render as:

```text
◆ Fizz · turn completed
◆ Fizz · turn ended with an issue
```

Rules:

- never insert these rows into message history;
- never affect timeline selection, copy, search, Inbox, unread, read markers, or
  thread summaries;
- support multiple active agents with a bounded `N agents working` summary and
  deterministic expansion in the Agents directory;
- render narrow and monochrome layouts without relying on color; and
- do not animate or redraw on every tick.

### Agents directory

Extend the existing verified remote-agent detail with:

- `live activity: disabled`, `unavailable`, `working`, `recently completed`, or
  `stream interrupted`;
- exact privacy wording: `owner-private live signal; not stored by bzz`;
- last-observed relative time while fresh; and
- runtime-report wording only when a valid lifecycle frame exists.

For agents not owned by the active identity, show:

```text
live activity: unavailable (only the verified owner can decrypt it)
```

Do not make owner-private activity a prerequisite for mentioning an otherwise
eligible remote agent.

### Status and help

Configuration and help must explain:

- observation is default-off;
- enabling it decrypts owner-addressed frames that may contain sensitive data,
  although bzz retains only lifecycle state;
- the relay can observe routing metadata and may violate the ephemerality
  recommendation;
- activity is best-effort and gaps are expected; and
- bzz still cannot start, stop, wake, retry, inspect tools, or recover the
  runtime.

### CLI

Keep `bzz agents list/show/refresh` as public directory commands and add only
configuration management:

```text
bzz agents activity status
bzz agents activity enable
bzz agents activity disable
```

These commands print no live frame, agent identifier, relay URL, or raw JSON.
`enable` must show the privacy warning and require an explicit confirmation or
`--yes`; non-interactive invocation without confirmation fails closed.

Do not add a historical observer dump. A future `bzz agents watch` is deferred
because a short-lived CLI process has a different subscription, terminal
cleanup, privacy, and buffering model. v0.12 live activity remains an in-session
TUI surface only.

## Architecture

Expected modules:

```text
src/agents/
  observer.rs     outer event admission, NIP-44 payload reduction
  activity.rs     bounded transient state machine and expiry

src/auth/
  signer.rs       narrowly scoped peer decryption command

src/realtime/
  subscriptions.rs  opt-in owner observer subscription

src/ui/
  agents.rs       directory activity presentation
  activity.rs     compact selected-channel activity row
```

The generic event store must explicitly reject kind `24200` so an application
routing bug cannot make observer frames durable. Observer handling must occur
before ordinary event persistence, Inbox, search, unread, and message routing.

No observer parser may import SQLite write APIs. No observer UI type may carry
raw payload JSON. v0.12 requires no SQLite schema migration: enrollment is a
strict non-secret config boolean and all activity state is transient memory.

## Milestones

### M0 — ADR and compatibility fixtures

1. Publish/verify the v0.11.x baseline and freeze the v0.12 starting commit.
2. Add an ADR for owner-private observation without control or hosting.
3. Record exact pinned fixtures for single and batched lifecycle events.
4. Document source/NIP drift, especially batching, turn completion, liveness,
   and runtime lifecycle extensions.
5. Confirm relay subscription authorization and ephemerality behavior with the
   pinned Buzz relay.

**Exit:** accepted lifecycle shapes and privacy boundaries are explicit; no
implementation relies on Desktop private storage, Tauri, archives, or logs.

### M1 — Pure admission and reduction

1. Implement bounded outer-event and exact-tag validation.
2. Implement strict single/batch payload parsing.
3. Reduce only lifecycle allowlist fields.
4. Add freshness, replay, sequence, ownership, community, and channel checks.
5. Fuzz/property-test malformed JSON, batch bombs, oversized text, and unknown
   extensions.

**Exit:** arbitrary observer input yields a deterministic small enum or is
ignored; no raw payload reaches the returned type.

### M2 — Signer and transient runtime

1. Add narrowly scoped peer decryption to the signer task.
2. Add the bounded in-memory activity reducer and expiry scheduler.
3. Clear state on lock, identity/community switch, disconnect, disable, and
   shutdown.
4. Add the durable-store kind `24200` rejection guard.
5. Add content-free local diagnostics excluded from OTel.

**Exit:** valid telemetry changes only bounded memory and presentation state;
SQLite remains byte-for-byte unchanged under an observer stream.

### M3 — Opt-in subscription and UI

1. Add strict default-off configuration and documentation.
2. Add subscription lifecycle and terminal rejection quarantine.
3. Add the selected-channel compact activity row.
4. Extend Agents detail with privacy and best-effort wording.
5. Add wide, narrow, monochrome, mouse, keyboard, resize, lock, and reconnect
   snapshots/scenarios.

**Exit:** an owner can see live turn lifecycle without seeing or retaining ACP
content and without changing message behavior.

### M4 — Pinned relay and release hardening

1. Register disposable deterministic owner/agent identities with the pinned
   relay.
2. Publish valid encrypted lifecycle frames and rejected spoof variants.
3. Exercise single and batched signals, gaps, duplicates, reconnect, stale
   frames, lock, and disable.
4. Prove no observer frame or plaintext enters SQLite, logs, diagnostics, OTel,
   support archives, crash output, or release fixtures.
5. Complete cross-platform, visual, performance, artifact, and release gates.

**Exit:** tagged artifacts demonstrate read-only owner-private live activity and
no runtime control or hosting authority.

## Deterministic test matrix

### Admission and cryptography

- valid agent → exact verified owner telemetry;
- wrong signature, wrong author, wrong `p`, wrong `agent`, wrong `frame`;
- duplicate/missing routing tags;
- active human is not the verified owner;
- same pubkey on another community/relay;
- removed bot membership and stale verified projection;
- valid DM owner exception and invalid non-owner DM participant;
- malformed or oversized NIP-44 ciphertext;
- valid decryption with oversized or invalid plaintext;
- stale, far-future, duplicate, and reordered event frames; and
- lock while decryption is queued.

### Payload reduction

- single `turn_started`, `turn_liveness`, `turn_completed`, `turn_error`,
  `agent_panic`, and runtime lifecycle;
- heartbeat start ignored;
- liveness cannot create or revive a turn;
- terminal event cannot close another agent/channel/turn;
- error text and unknown payload fields never reach projection or diagnostics;
- valid one-level batch preserves deterministic order;
- nested, oversized, duplicate-sequence, and malformed batches fail closed;
- sensitive ACP/tool/message frames are ignored; and
- unknown future kinds are inert.

### Lifecycle and resources

- disabled configuration creates no subscription;
- enable/disable adds/removes exactly one subscription and clears state;
- reconnect begins at now and marks previous activity interrupted/unknown;
- terminal subscription rejection does not hot-loop;
- duplicate and unchanged liveness creates no DB write or redraw;
- expiry uses one bounded deadline scheduler;
- flood at and beyond relay-rate recommendations remains memory bounded;
- observer traffic cannot starve terminal input, channel switch, attachment
  staging, AUTH, structured mention revalidation, or acknowledged send; and
- normal exit restores the terminal and clears plaintext/transient state.

### Non-interference

- no observer record in events/messages/search/Inbox/unread/read state;
- no observer status in copied message text or thread summaries;
- no automatic reaction, message, control, or local command;
- no change to verification or invocation policy;
- no names, keys, IDs, relay URLs, ciphertext, plaintext, errors, or payloads in
  diagnostics/OTel; and
- production cache and diagnostic journal remain unchanged in sanitized visual
  tests except for pre-existing non-observer activity.

## Release gates

### Local

- format, strict locked all-feature/all-target Clippy, and all tests;
- NIP-44 interop and bounded-parser property tests;
- durable-store rejection and no-write idle/flood proofs;
- `cargo deny`, `cargo audit`, release build, and isolated `bzz check`;
- TUI visual review at wide and minimum supported dimensions; and
- explicit plaintext/ciphertext grep over generated diagnostics, reports,
  fixtures, and release staging.

### Cross-platform

- Linux, Windows, Intel macOS, and Apple Silicon builds/tests;
- native keychain and encrypted-file identities decrypt disposable frames;
- lock interrupts pending work on every target; and
- no observer implementation depends on Unix process behavior.

### Pinned relay

- authenticated owner subscription accepted;
- valid NIP-OA-owned agent telemetry delivered and decrypted;
- non-owner subscription/publish variants rejected by relay and client;
- batch and lifecycle fixtures match the pinned implementation;
- reconnect has no historical replay dependency; and
- no LLM, ACP adapter, tool, provider credential, or production agent key is
  required.

### Publication

- tag resolves to the fully validated commit;
- archives, checksums, SBOM, and provenance verify;
- downloaded binary reports v0.12.0 and passes isolated `bzz check`;
- default configuration proves observer activity is off;
- enabled disposable smoke proves live-only state and no SQLite persistence; and
- release evidence records exact CI and artifact results.

## Explicit non-goals

v0.12.0 does not:

- create, import, export, back up, rotate, or store an agent key;
- create an agent, persona, public projection, owner attestation, or bot
  membership;
- host, install, launch, wake, stop, restart, inspect, retry, or recover an
  agent runtime;
- spawn `buzz-acp`, Codex, Claude, Goose, Buzz Agent, or any custom harness;
- implement ACP, MCP, model/provider setup, or tool permissions;
- publish kind `24200` control frames or owner commands;
- cancel turns, rotate sessions, switch models, or shut down agents;
- display or archive raw observer events, ACP transcripts, prompts, reasoning,
  tool calls, output chunks, errors, or logs;
- ingest NIP-AE kind `30174`, NIP-PMA kind `30179`, or usage kind `44200`;
- treat presence or activity as readiness or health;
- add a historical activity/search/Inbox surface;
- read Buzz Desktop archives, local databases, managed-agent JSON, keyring
  entries, process receipts, or Tauri APIs;
- add provider-backed deployment; or
- claim Buzz Desktop managed-agent parity.

## Follow-up sequence

If v0.12 proves observation authorization, privacy, and reconnect behavior, the
recommended sequence is:

1. **v0.13 candidate — owner control with acknowledgements:** evaluate one
   explicitly confirmed `cancel_turn` action, with signed owner publication,
   control-result correlation, timeouts, and clear best-effort semantics. This
   requires a separate ADR and is not pre-approved by this plan.
2. **Later — public NIP-AP persona catalog:** read-only definition provenance
   and discovery can extend capability 1 independently of runtime control.
3. **Later — memory/usage views:** only after separate privacy, retention, and
   encryption decisions; neither should be bundled with control.
4. **Separate major architecture program — local hosting:** only after every
   unresolved key, process, ACP permission, autonomous publication, queue,
   session, recovery, and cross-platform gate in the v0.11 ADR is resolved.

Local hosting is not the automatic v0.13 step.

## Definition of done

v0.12.0 is complete only when:

1. owner-private activity is default-off, live-only, bounded, and transient;
2. only current verified agents owned by the active identity can update it;
3. strict signed routing checks occur before NIP-44 decryption;
4. sensitive payload classes are ignored and never persisted or logged;
5. visible states are best-effort lifecycle reports, never readiness claims;
6. disconnect, lock, switch, disable, expiry, and shutdown clear authority and
   transient data deterministically;
7. kind `24200` cannot enter generic durable storage even after a routing bug;
8. messages, drafts, Inbox, unread, search, copy, threads, and human publication
   remain unchanged;
9. no control frame, agent key, ACP process, model, tool, memory, usage, or local
   runtime exists in the implementation; and
10. deterministic, pinned-relay, cross-platform, privacy, performance, visual,
    and artifact gates pass at the tagged commit.
