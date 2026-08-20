# bzz — Implementation Plan

**Status:** Implemented through M9; MVP gates verified; post-MVP themes, secure media, Inbox, workspace DMs, and unified search delivered
**Repository / binary:** `arpagon/bzz` / `bzz`
**Product shorthand:** “`slk`, but for Buzz”
**Plan date:** 2026-07-30
**Upstream snapshots:** Buzz `ede26863345a518ec46edd6d7692e0281883491b`; slk `8149c3b18ed04c259efe5feb545d040ab043d922`

## Post-MVP amendments

The deferred MVP scope below remains the historical implementation baseline.
Built-in semantic themes were delivered through issue #1. Secure message media
was subsequently designed in issue #2 and implemented with strict `imeta`
projection, origin-bound Blossom authorization, SHA-256/size/MIME verification,
private quota-managed cache/staging, bounded image decode, Kitty/Sixel/iTerm2/
half-block rendering, previews, explicit saves, and sanitized uploads. See
[`docs/media.md`](docs/media.md) for the current contract. Arbitrary external
Markdown/profile images, media playback, and animated playback remain deferred.

Issue #3 subsequently delivered the active-community Inbox, Buzz
workspace-channel DMs, and unified local/remote search. Workspace DMs use
41010/41011/41012 with relay-signed 39000/39002 discovery and owner-only 30622
visibility; they are explicitly relay-readable and not NIP-17/E2EE. Inbox
composes mentions, relevant NIP-10 activity, visible DMs, read-only
46010–46012 cards, drafts, and existing read contexts. Search combines
community-partitioned SQLite FTS5 with authenticated NIP-50 prefix queries and
fails closed on unresolved operators/access. See
[`docs/inbox-dms-search.md`](docs/inbox-dms-search.md) for the current contract.

## 1. TL;DR and recommendation

Build `bzz` as one Rust binary using Tokio, Ratatui, Crossterm, and SQLite. Reuse Buzz's `buzz-core` and `buzz-sdk` crates as **revision-pinned Git dependencies**, but implement a new long-lived WebSocket/session layer and a new TUI. Do not use `buzz-ws-client` as the application transport: it is a useful NIP-42/publish reference, but it has no reconnect supervisor, persistent subscription ownership, concurrent request routing, cache integration, or multi-community lifecycle. Do not shell out to `buzz-cli`.

The MVP will support multiple configured communities and isolated caches, with **one active network connection at a time**. Simultaneous connections are post-MVP. Its useful core is:

- secure identity generation/import and signing;
- proactive NIP-42 authentication to a host-bound Buzz community;
- NIP-29 channel discovery and membership;
- cached channel history, live messages, threads, reactions, and self-deletion;
- kind `0` profiles;
- local and cross-device kind `30078` read state;
- fast, modal, Vim-style navigation and fuzzy channel switching;
- idempotent reconnect/backfill and an acknowledged outbox.

Use WebSocket Nostr for authentication, live subscriptions, and ordinary event publication. Use the NIP-98 HTTP bridge for bounded one-shot queries that need Buzz's composite pagination or thread extensions. SQLite is the startup/offline source of display state, while signed events plus relay acknowledgements are durable network truth.

### Decision in one sentence

Choose **Rust + Ratatui** because protocol correctness and safe signing are the hard, Buzz-specific part; independently reproduce `slk`'s interaction ideas instead of importing its Slack-specific application core.

## 2. Research provenance and evidence ledger

Both repositories were refreshed with the required `thirdparty` checkout script and inspected read-only from the shared cache on **2026-07-30**.

| Upstream | Exact revision | Concrete source path | Finding used by this plan |
|---|---|---|---|
| Buzz | `ede26863345a518ec46edd6d7692e0281883491b` | `ARCHITECTURE.md` | The relay is a Rust/Tokio Nostr service with WebSocket, NIP-98 HTTP, Postgres, Redis, and object-storage boundaries; client state must not assume one REST API owns everything. |
| Buzz | same | `NOSTR.md` | Buzz documents NIP-29 discovery, kind `9` messages, NIP-10 threads, NIP-25 reactions, NIP-42, NIP-50, NIP-17 gift wraps, Blossom, and Buzz-only kinds. |
| Buzz | same | `crates/buzz-core/src/tenant.rs` | Community tenancy is selected from the normalized request authority; default ports and case collapse, non-default ports remain distinct, and clients do not send a community ID. |
| Buzz | same | `crates/buzz-core/src/kind.rs` | Canonical event registry: profile `0`, delete `5`, reaction `7`, message `9`, read state `30078`, discovery `39000`–`39003`, rich/edit kinds, DM commands, presence, and typing. |
| Buzz | same | `crates/buzz-core/Cargo.toml` | `buzz-core` has no Tokio, SQLx, Redis, or Axum dependency and is appropriate as a zero-I/O protocol/types dependency. |
| Buzz | same | `crates/buzz-sdk/src/builders.rs` | Typed builders already enforce content limits and correct `h`, NIP-10 `e`, `p`, reaction, deletion, profile, custom-emoji, and DM command tag shapes. |
| Buzz | same | `crates/buzz-sdk/Cargo.toml` | `buzz-sdk` only depends on core protocol/data crates and holds no key or network state; it is suitable for pinned external reuse. |
| Buzz | same | `crates/buzz-ws-client/src/connection.rs` | The existing client authenticates and publishes, but wraps one mutable socket and buffers messages while waiting for `OK`; it does not supervise a full interactive session. |
| Buzz | same | `crates/buzz-ws-client/src/message.rs` | Its relay-message parsing confirms the wire envelopes `AUTH`, `EVENT`, `EOSE`, `OK`, `NOTICE`, and `CLOSED`. |
| Buzz | same | `crates/buzz-relay/src/connection.rs` | The relay proactively sends a random challenge and closes unauthenticated sockets after **5 seconds**; `bzz` must unlock keys before connecting and answer immediately. |
| Buzz | same | `crates/buzz-auth/src/nip42.rs` | AUTH is kind `22242`; signature, challenge, relay URL, and timestamp are checked, with a ±60-second timestamp tolerance. |
| Buzz | same | `crates/buzz-relay/src/handlers/auth.rs` | Auth can fail for bans, allowlist, or relay membership; failures become terminal for that socket and must not be treated as ordinary transient reconnects. |
| Buzz | same | `crates/buzz-relay/src/handlers/req.rs` | `REQ` requires prior auth, sends historical events then `EOSE`, supports search as one-shot, and enforces channel and `#p` access gates. |
| Buzz | same | `crates/buzz-relay/src/subscription.rs` | Global subscriptions deliberately do not receive channel-scoped events; live channel traffic requires channel-scoped subscriptions. |
| Buzz | same | `crates/buzz-relay/src/handlers/side_effects.rs` | Relay-signed kind `39000`/`39001`/`39002` events project channel metadata/admin/member state; private member lists remain access-gated. |
| Buzz | same | `desktop/src-tauri/src/commands/channels.rs` | Desktop discovers membership with `39002 #p=self`, fetches matching `39000 #d`, separately enumerates open `39000`, uses composite cursors, and derives member counts from `39002`. |
| Buzz | same | `desktop/src-tauri/src/commands/messages.rs` | Desktop queries messages with keyset extensions, resolves thread roots from NIP-10 tags, and fetches complete reply subtrees with `#e`, `depth_limit`, and a composite thread cursor. |
| Buzz | same | `desktop/src/shared/api/relayChannelFilters.ts` | History budgets are reserved for content; reactions/edits/deletions are separately backfilled by `#e` because reaction events do not carry `h`. |
| Buzz | same | `desktop/src/shared/api/relayClientSession.ts` | Desktop replays live subscriptions after reconnect, correlates pending event IDs with `OK`, has bounded backoff, and uses one channel-scoped live subscription per channel. |
| Buzz | same | `crates/buzz-cli/src/client.rs` | NIP-98 uses kind `27235`, exact URL/method, body SHA-256, a nonce, and `Authorization: Nostr <base64-event>`; media uses separate Blossom authorization. |
| Buzz | same | `crates/buzz-auth/src/nip98.rs` | NIP-98 verifies signature, ±60-second timestamp, normalized URL, method, and optional payload hash. |
| Buzz | same | `crates/buzz-auth/src/nip98_replay.rs` | The relay rejects reused NIP-98 event IDs using an atomic, community-scoped seen-set; every request must receive a newly signed auth event/nonce. |
| Buzz | same | `crates/buzz-relay/src/api/bridge.rs` | `POST /query`, `/events`, and `/count` are NIP-98 authenticated and host-bound; the query bridge contains Buzz-only pagination/search extensions. |
| Buzz | same | `desktop/src/features/channels/readState/readStateFormat.ts` | Read blobs are versioned context maps with a 32 KiB plaintext budget, a seven-day fetch horizon, and bounded slots/contexts. |
| Buzz | same | `desktop/src/features/channels/readState/readStateManager.ts` | Read state is NIP-44-to-self encrypted kind `30078`, max-merged across devices/slots, debounced, conflict-rotated, and published with monotonic `created_at`. |
| Buzz | same | `desktop/src/features/channels/readState/readStateSnapshot.ts` | A valid read event must be self-authored, tagged `t=read-state`, have a valid `d=read-state:<slot>`, decrypt, parse, and sanitize. |
| Buzz | same | `desktop/src-tauri/src/commands/profile.rs` | Profiles are community-local replaceable kind `0` events; reads query by authors and profile updates are read/merge/write snapshots. |
| Buzz | same | `desktop/src/shared/api/customEmoji.ts` | Custom emoji are per-member kind `30030` sets whose deterministic client-side union forms a community palette; defer for MVP. |
| Buzz | same | `crates/buzz-relay/src/api/media.rs` and `crates/buzz-test-client/tests/e2e_media.rs` | Blossom upload/download has content, hash, authorization, size, and range behavior that merits a separate post-MVP security pass. |
| Buzz | same | `crates/buzz-cli/src/commands/dms.rs`, `desktop/src-tauri/src/commands/dms.rs` | The human DM surface is relay-created private channels: kind `41010` returns a channel ID in `OK`, and kind `41001` can list relay-confirmed DMs. |
| Buzz | same | `crates/buzz-test-client/tests/e2e_nostr_interop.rs` | Tests validate search, NIP-10, gift wraps, private DM visibility, and owner-gated reads; tests are protocol evidence where docs are broader. |
| Buzz | same | `crates/buzz-test-client/tests/e2e_relay.rs` | End-to-end tests cover NIP-42, `REQ`/`EOSE`, event publication, filtering, and rejection paths against a real relay. |
| slk | `8149c3b18ed04c259efe5feb545d040ab043d922` | `internal/ui/app.go` | A root model owns mode, focus, pane geometry, submodels, and service interfaces; I/O returns typed messages to the update loop. |
| slk | same | `internal/ui/keys.go` and `internal/ui/mode_*.go` | The polished interaction model is modal: `j/k`, `gg/G`, `Ctrl-u/d`, fuzzy switcher, help, pane toggles, and Vim window chords. |
| slk | same | `internal/ui/messages/model.go` | Message rendering precomputes selected/unselected row caches so cursor movement does not redo expensive formatting. |
| slk | same | `internal/ui/thread/model.go` | Threads are a first-class side panel with independent selection, scrolling, reactions, unread boundary, and compose state. |
| slk | same | `internal/cache/db.go` | SQLite uses WAL, foreign keys, and a 5-second busy timeout, storing workspace/channel/message/reaction/thread/user/read state for fast hydration. |
| slk | same | `cmd/slk/reconnect_backfill.go` | Reconnect backfill is bounded, idempotent, per-channel, and only advances a watermark when an uncapped fetch proves the interval complete. |
| slk | same | `internal/cache/messages.go`, `reactions.go`, `threads.go` | Stable composite primary keys make repeated event delivery harmless and support offline rendering. |
| slk | same | `internal/service/workspace.go` and `internal/service/messages.go` | Service interfaces separate cache/network operations from UI state and allow model tests to use fakes. |
| slk | same | `internal/config/config.go` | Configuration has explicit multi-workspace records and default selection instead of burying workspace identity in transient UI state. |
| slk | same | `internal/image/renderer.go` | Kitty, Sixel, and half-block rendering require protocol detection, strict output serialization, fallbacks, and extensive tests; defer from MVP. |
| slk | same | `internal/notify/notifier.go` | Notification policy excludes own/current-channel messages and passes untrusted message text as data, not interpolated shell syntax. |
| slk | same | `internal/ui/app_bench_test.go`, `internal/cache/*_test.go`, `cmd/slk/reconnect_backfill_test.go` | Responsiveness is protected by model/cache/backfill tests and benchmarks, not only end-to-end screenshots. |
| slk | same | `LICENSE` | slk is MIT-licensed; interaction ideas can be independently implemented, while copied code would require preserving its copyright/license notice. |

### Research conclusions that change the bootstrap assumptions

1. **Buzz Desktop DMs and NIP-17 are not one feature.** The relay accepts standard kind `1059` NIP-17 gift wraps, but Desktop's workspace DM UX is private NIP-29 channels opened by kind `41010` and discovered through relay-signed `39000`/`39002` plus membership notifications. The pinned relay does not emit legacy kind `41001`. Workspace DMs are now implemented post-MVP and must never be labeled “NIP-17” or E2EE.
2. **A global live subscription is insufficient.** Buzz intentionally isolates global and channel-scoped fan-out, so the client needs a channel subscription set rather than one Slack-like firehose.
3. **Timestamp-only history cursors are insufficient.** Dense same-second traffic requires Buzz's HTTP bridge `before_id`/thread cursor extensions.
4. **Read state is protocol state, not just a SQLite flag.** It is encrypted, grow-only/max-merged, multi-slot kind `30078` state and is part of MVP correctness.
5. **The upstream client timeout is not the relay deadline.** `buzz-ws-client` waits up to 20 seconds for a challenge, but the relay closes an unauthenticated connection at 5 seconds. `bzz` must target the stricter server behavior.

## 3. Product scope and feature parity

Legend: **Yes** = supported, **Partial** = present but not the product-quality target, **MVP** = required in this plan, **Later** = explicitly deferred, **N/A** = Slack-specific or absent.

| Capability | Buzz Desktop | `buzz-cli` | `slk` | Proposed `bzz` MVP |
|---|---:|---:|---:|---:|
| Human full-screen terminal UI | No | No | Yes | **MVP** |
| Configure several communities | Yes | Per invocation | Several workspaces | **MVP; one active connection** |
| Simultaneous community connections | No/active override | No | Yes | Later |
| Secure local Nostr identity | OS/Desktop storage | Environment/config patterns | Slack token/cookie model | **MVP; keychain + encrypted fallback** |
| Proactive NIP-42 auth | Yes | WS commands | N/A | **MVP** |
| NIP-98 query bridge | Yes | Yes | N/A | **MVP for keyset reads** |
| Visible/joined channel discovery | Yes | Partial commands | Yes | **MVP** |
| SQLite startup/offline cache | Frontend/Tauri stores | No | Yes | **MVP** |
| Historical + live channel timeline | Yes | Query/JSON | Yes | **MVP** |
| Basic kind `9` message send | Yes | Yes | Yes | **MVP** |
| Threads | Yes | Yes | Yes | **MVP** |
| Reactions / remove own reaction | Yes | Yes | Yes | **MVP** |
| Delete own message | Yes | Yes | Yes | **MVP** |
| Profiles / author labels | Yes | Yes | Yes | **MVP, read-only profile UI** |
| Durable local unread state | Yes | No | Yes | **MVP** |
| Cross-device Buzz read state | Yes | No | N/A | **MVP** |
| Vim navigation / fuzzy switcher | No | No | Yes | **MVP** |
| Reconnect gap repair | Yes | Per command retries | Yes | **MVP** |
| Optimistic UI with acknowledged truth | Yes | No | Yes | **MVP, pending outbox rows** |
| Private-channel DMs/group DMs | Yes | Yes | Slack DMs | **Implemented post-MVP; relay-readable NIP-29 channels** |
| Raw NIP-17 gift-wrap inbox | Relay support | Partial/raw | N/A | Later, distinct from workspace DMs |
| Message/profile search | Yes | Yes | Yes | **Implemented post-MVP; local FTS5 + NIP-50** |
| Attachments / upload | Yes | Yes | Yes | **Implemented post-MVP** |
| Inline terminal images | N/A | No | Yes | **Implemented post-MVP** |
| Presence and typing | Yes | Presence only | Yes | Later |
| Message edit / kind `40003` overlay | Yes | Yes | Yes | Later; cache raw events in MVP |
| Rich kind `40002` rendering | Yes | Partial | Slack Block Kit | **Partial: safe text fallback only** |
| Custom emoji kind `30030` | Yes | Builder/commands | Yes | Later |
| Desktop notifications | Yes | No | Yes | Later |
| Themes / configurable bindings | Yes | No | Yes | **60 semantic themes implemented post-MVP; custom bindings later** |
| Activity feed / approvals | Yes | Yes | N/A | **Inbox/read-only approval status implemented; mutations later** |
| Agents, ACP/MCP, Git, canvases, huddles | Yes | Yes | N/A | Explicit non-goal |

### MVP success statement

A user can configure two Buzz relay URLs, associate an identity without putting its secret in config or SQLite, switch to either community, instantly see cached channels/messages, authenticate and reconcile, read and write channel conversations and threads, react/delete, and quit/restart/reconnect without duplicate rows or backward read markers.

## 4. Protocol map

### 4.1 Transport policy

- **WebSocket:** NIP-42, persistent channel/global subscriptions, basic signed event writes, and `OK` correlation. This keeps one authenticated realtime stream and provides immediate acknowledgements.
- **NIP-98 HTTP `POST /query`:** channel directory batches, composite-key history pages, complete thread subtrees, profile batches, and auxiliary-event backfill. A new kind `27235` auth event with a new nonce is signed for every request.
- **NIP-98 HTTP `POST /events`:** fallback for an already-signed outbox event only when the WebSocket is unavailable and the caller explicitly chooses retry; normal writes use WebSocket. Never re-sign an ambiguous write.
- **Plain HTTP GET with `Accept: application/nostr+json`:** initial NIP-11 capability/relay-key probe.
- **Local only:** community add/remove, identity selection, drafts, UI state, cache deletion.

### 4.2 Exact MVP operation map

| User/client operation | Read/write and wire shape | Validation / result handling |
|---|---|---|
| Add community | Local URL validation, then NIP-11 `GET /`; derive HTTP URL by `wss→https`, `ws→http`. | Store canonical relay URL and normalized authority; require NIP-42/NIP-29 support or an explicit compatibility warning. No client-supplied tenant ID. |
| Remove community | Local transaction; stop session first. | Default keeps cache pending confirmation; “remove and purge” deletes community-scoped rows. Never deletes the shared identity secret automatically. |
| Authenticate | Receive `['AUTH', challenge]`; sign kind `22242` with `challenge` and exact `relay` tags; send `['AUTH', event]`; await matching `OK`. | Challenge ≤1024 bytes, fresh event, correct host/path, ±60 s clock, response inside relay's 5 s deadline. `restricted`, banned, or membership failures become a terminal UI state. |
| Discover joined channels | `/query` filter `{kinds:[39002], '#p':[self]}` paged with `(until,before_id)`; collect `d` UUIDs. | Verify signatures and accept relay-signed state; dedupe IDs. |
| Discover open/visible channels | `/query` `{kinds:[39000]}` paged; then fetch member-channel metadata by `#d`. | Parse `d`, `name`, `about`, `private/public`, `hidden`, and `t`; hide DM metadata in MVP but preserve raw events. Open non-member channels appear in the finder, not the primary joined section. |
| Refresh membership | Global WS `REQ` for kinds `44100/44101` with `#p=self`; channel-scoped `39002` events; periodic directory reconciliation. | A membership notification triggers a full directory delta query. A `CLOSED restricted` removes that channel subscription and refreshes access. |
| Fetch profiles | `/query` `{kinds:[0],authors:[...],limit:n}` in bounded batches; lazy fetch unknown authors. | Verify event, newest replaceable event wins per `(community,pubkey,kind)`; parse `display_name`, `name`, `picture`, `nip05`, `about` defensively. Fallback is abbreviated pubkey. |
| Initial timeline | `/query` filter `{kinds:[9,40002,40099], '#h':[channel], limit:200}`. | Store raw signed events first; display top-level events ordered `(created_at,event_id)`. Kind `40002/40099` gets sanitized text fallback. |
| Older timeline page | Same plus `until=<oldest.created_at>` and `before_id=<oldest.id>`. | Continue until short page; never use timestamp alone. Keep replies in the event graph but omit them from top-level rows. |
| Live timeline/aux | Per joined channel WS `REQ` with `#h=channel`, kinds `[5,7,9,9005,39000,39002,40002,40003,40099]`, overlap `since`, bounded history, then live after `EOSE`. | Upsert every verified event by `(community,event_id)`. Cache edit events even though edit overlay is deferred. Handle `NOTICE`, `CLOSED`, `EOSE`, ping/pong, and unknown envelopes without panic. |
| Auxiliary backfill | For IDs in each visible history page, `/query` in chunks: `{kinds:[5,7,9005,40003], '#e':[ids], limit:10000}`; then query kind `5/9005` targeting returned reaction IDs. | No `#h` on reference queries because reactions only carry `e`; apply author/target checks and tombstones deterministically. |
| Send message | `buzz_sdk::build_message(channel_uuid,text,None,mentions,false,[])`, sign once as kind `9`, persist outbox, WS `['EVENT',event]`. | Trim only surrounding composer whitespace according to documented UX; max 64 KiB enforced by SDK. Pending until matching `OK true` or observed event. |
| Send direct thread reply | Resolve selected parent. For a root parent use one `e=<root>,reply`; for nested reply use `e=<root>,root` and `e=<parent>,reply`; always carry `h`. | Use `buzz_sdk::ThreadRef`; do not guess a root from display position. Unknown/restricted parents surface relay rejection. |
| Fetch a thread | Fetch root by `ids`; `/query` with `#e=<root>`, non-p-gated timeline kinds, `depth_limit=64`, `thread_cursor` + `thread_cursor_id`, max page 500. | Oldest-first in UI after complete paging. Parent/reply graph is based on marked NIP-10 tags, not array order. |
| Add reaction | Build kind `7`, content Unicode emoji, `e=<message-id>`; sign once, outbox, WS publish. | MVP offers a fixed Unicode picker. Same-user/same-emoji duplicates collapse in rendered aggregate but raw events remain. |
| Remove own reaction | Find own matching accepted kind `7`; build kind `5` targeting that reaction event ID. | Only enable when an own reaction ID is known; wait for `OK`/echo before final removal. |
| Delete own message | Build NIP-09 kind `5` with `e=<target>` and non-standard `h=<channel>` via `build_delete_compat`. | UI checks author locally; relay is authoritative. Preserve tombstone row and hide body in normal rendering. Kind `9005` is accepted on reads for relay/moderator tombstones. |
| Hydrate read state | WS or `/query`: `{kinds:[30078],authors:[self],'#t':['read-state'],since:now-7d,limit:500}`. | Decrypt NIP-44 v2 to self; validate slot/blob; max-merge all valid contexts, never latest-event-wins. |
| Mark channel/thread read | Local transaction advances bare channel UUID, `thread:<root-id>`, and when needed `msg:<reply-id>` to the greatest actually observed event timestamp. | Mark only when pane is at its live bottom and focused; never use a lower value. UI updates from SQLite immediately. |
| Publish read state | Refetch own slots, max-merge, NIP-44 encrypt JSON `{v:1,client_id,contexts}`, sign kind `30078` with `d=read-state:<slot>` and `t=read-state`, publish by WS. | Debounce 5 s; `created_at=max(now,max_seen_created_at+1)`; 32 KiB plaintext/8-slot cap; rotate a colliding slot ID; retry without reducing markers. |
| Quit | Flush SQLite writer, make one bounded read-state/outbox flush, send `CLOSE` for subscriptions, close socket, restore terminal. | A timeout leaves signed outbox rows for the next start; no event is re-signed merely to make it look newer. |

### 4.3 Supported event-kind policy

- **Fully interpreted in MVP:** `0`, `5`, `7`, `9`, `30078`, `39000`, `39002`, `44100`, `44101`.
- **Safely shown/recorded:** `40002` and `40099` as sanitized text; `9005` as deletion; `40003` stored but no edit overlay.
- **Ignored but retained as raw signed events when encountered:** other channel-scoped kinds. Unknown events never execute terminal escapes or shell commands.
- **Implemented post-MVP:** relay-signed owner-only `30622`, workspace DM commands `41010`–`41012`, read-only Inbox events `46010`–`46012`, NIP-50 search, and secure media.
- **Deferred:** `1059`, `1984`, `20001`, `20002`, `30030`, `30315`, `39005`, `39006`, `40003` rendering, `40008`, legacy `41001`, forums, jobs, workflow mutations, Git, huddles, media playback, and agents.

## 5. Architecture decision record

### 5.1 Decision matrix (1 poor, 5 strong)

| Criterion | Weight | A: Rust + Ratatui | B: Go + Bubble Tea | C: Go TUI + Rust/CLI sidecar |
|---|---:|---:|---:|---:|
| Buzz signing/protocol fidelity | 5 | **5** — direct `nostr`, `buzz-core`, `buzz-sdk` | 3 — mature Go Nostr exists, Buzz extensions must be rebuilt | 5 in sidecar, split contract |
| UI architecture leverage from slk | 3 | 3 — concepts only | **5** — same framework/language | 4 |
| Realtime async/session control | 4 | **5** — Tokio fits relay code and bounded actors | 4 | 2 — stream/process lifecycle crosses IPC |
| Secret containment | 5 | **4** — one in-process signer actor | 3 — separate protocol implementation | 2 — secret ownership and IPC add attack surface |
| SQLite/cache implementation | 3 | 4 — Rusqlite actor | **5** — slk patterns map directly | 3 |
| Packaging/operation | 4 | **5** — one native binary | **5** — one native binary | 1 — two artifacts, version negotiation, child supervision |
| Compile/build complexity | 2 | 3 — Rust + native targets are slower | **5** | 2 |
| Upstream maintenance alignment | 4 | **5** — compiler catches Buzz crate drift | 3 | 3 |
| Expected responsiveness/memory | 3 | **5** | 4 | 3 |
| Weighted total (max 165) |  | **150** | 124 | 91 |

### 5.2 Why not Go + Bubble Tea

Go/Bubble Tea would make direct adaptation of `slk`'s UI organization easier, but the client would need to independently maintain Buzz's host binding, custom query fields, exact event kinds/tags, NIP-42 edge cases, NIP-44 read state, and command acknowledgements. The UI concepts are framework-independent; the signing and event compatibility are not. Reusing slk source would also import assumptions about Slack timestamps, one firehose, private Slack APIs, cookie reminting, and authoritative Slack unread counters.

### 5.3 Why not a hybrid

A sidecar introduces protocol framing, process crashes, version compatibility, backpressure across stdio/socket IPC, secret transfer or split ownership, and two binaries to package. Shelling out to `buzz-cli` cannot provide reliable persistent subscription fan-out and would expose more process/logging surfaces. There is no compensating capability that a Rust Ratatui binary lacks, so Option C is rejected.

### 5.4 Upstream crate policy

Pin in the root manifest and lockfile:

```toml
buzz-core = { git = "https://github.com/block/buzz", rev = "ede26863345a518ec46edd6d7692e0281883491b" }
buzz-sdk  = { git = "https://github.com/block/buzz", rev = "ede26863345a518ec46edd6d7692e0281883491b" }
nostr = { version = "0.44", features = ["nip44", "nip98"] }
```

- Use Rust `1.95.0`, matching the inspected Buzz `rust-toolchain.toml`; retain Buzz's declared APIs as the compatibility floor.
- Do not depend on `buzz-ws-client` in MVP. Independently implement the session around `tokio-tungstenite`, while testing behavior against its parser and relay tests.
- Do not depend on `buzz-cli`; independently implement NIP-98 from the standard event shape and cited source.
- CI compiles against the pinned revision. Updating that revision is a dedicated PR with protocol fixture and real-relay integration runs.
- If Git/workspace dependency resolution becomes fragile, the fallback is a small local protocol module using `nostr 0.44`, not vendoring the whole Buzz tree.

Principal non-upstream dependencies: Ratatui, Crossterm, Tokio, Tokio Tungstenite/rustls, Reqwest/rustls with redirects disabled, Rusqlite (`bundled`), Serde/TOML, `keyring`, Argon2, XChaCha20-Poly1305, `secrecy`/`zeroize`, `directories`, `tracing`, `unicode-width`/`unicode-segmentation`, `pulldown-cmark`, and `nucleo-matcher`. Commit `Cargo.lock`; gate licenses/advisories with `cargo-deny`.

## 6. Proposed module boundaries and data flow

```text
crossterm input/tick
        │
        ▼
ui::App reducer ── Effect ──► service tasks
        ▲                         │
        │ DomainEvent             ├──► signer actor (only owner of Keys)
        │                         ├──► active community session
        │                         ├──► NIP-98 query client
        │                         └──► SQLite writer actor
        │                                      │
        └──────── immutable view snapshots ◄───┘

WS reader ─► parse + verify ─► normalize/store transaction ─► DomainEvent ─► UI
WS writer ◄─ signed outbox event / REQ / CLOSE; matching OK ─► outbox/store/UI
```

### Modules

- `config` / `paths`: non-secret settings, data directories, URL policy, startup overrides.
- `auth`: keychain and encrypted-file backends, identity lifecycle, zeroization, signer actor, NIP-44-to-self operations.
- `protocol`: relay envelopes, URL/tenant normalization, NIP-42/NIP-98 construction, Buzz event parsing, typed domain conversion.
- `store`: migrations, one SQLite owner task, query DTOs, idempotent event application, outbox, drafts, sync/read state.
- `realtime`: one active `CommunitySession`, WebSocket reader/writer, subscription registry, `OK` router, stall detection, reconnect supervisor.
- `sync`: directory hydration, history and auxiliary backfill, cursor rules, read-state merge/publish.
- `service`: use cases invoked by UI (`switch_community`, `open_channel`, `send`, `open_thread`, `react`, `delete`, `mark_read`). It knows interfaces, not Ratatui widgets.
- `domain`: transport-independent community/channel/profile/event/thread/reaction/read models.
- `ui`: reducer, effects, modal key handling, pane models, render-only widgets, terminal lifecycle.
- `render`: control-character sanitizer, minimal Markdown-to-Spans, mention/link presentation.

### Invariants

1. Ratatui rendering and reducer methods perform no network or database I/O.
2. Private key material never enters `App`, an effect payload, SQLite, config, logs, panic text, or command-line arguments.
3. Every community-owned primary/foreign key starts with `community_id`; no query defaults to “current community” inside the store.
4. Inbound events are signature-verified before becoming domain state.
5. Event upsert and derived reaction/thread/deletion updates happen in one SQLite transaction.
6. A signed event is placed in outbox before network send and is never re-signed after an ambiguous outcome.
7. Read timestamps only move by `max`.
8. Bounded channels protect the UI from network/backfill floods. Bulk updates become one `DomainEvent::StoreChanged` rather than one redraw per row.

### Responsiveness rules borrowed as concepts from slk

- Hydrate SQLite before connecting.
- Keep selection/scroll state per channel and per thread.
- Precompute sanitized/wrapped message rows for selected and unselected variants; invalidate on width/content/theme change, not every keypress.
- Coalesce network event redraws at roughly one frame (16 ms).
- Bound backfill/profile concurrency to four and SQLite writes through one owner.
- Debounce thread fetch while the cursor is moving.
- Never discard a useful cache because refresh failed.

## 7. Local database schema and migrations

Use Rusqlite with `foreign_keys=ON`, `journal_mode=WAL`, `synchronous=NORMAL`, and `busy_timeout=5000`. The database contains message content and is created with user-only permissions. It contains **no private key, passphrase, NIP-98 header, or decrypted key backup**.

### 7.1 Initial schema

```sql
identities(
  id TEXT PRIMARY KEY, pubkey TEXT UNIQUE NOT NULL, label TEXT NOT NULL,
  key_backend TEXT NOT NULL, key_ref TEXT NOT NULL, created_at INTEGER NOT NULL
)

communities(
  id TEXT PRIMARY KEY, identity_id TEXT NOT NULL REFERENCES identities(id),
  relay_url TEXT UNIQUE NOT NULL, authority TEXT NOT NULL, http_base_url TEXT NOT NULL,
  label TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
  relay_pubkey TEXT, last_connected_at INTEGER, last_error_code TEXT,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
)

channels(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL, name TEXT NOT NULL, about TEXT NOT NULL DEFAULT '',
  channel_type TEXT NOT NULL DEFAULT 'stream', visibility TEXT NOT NULL,
  is_member INTEGER NOT NULL DEFAULT 0, is_hidden INTEGER NOT NULL DEFAULT 0,
  member_count INTEGER NOT NULL DEFAULT 0, metadata_event_id TEXT,
  metadata_created_at INTEGER, last_event_at INTEGER,
  PRIMARY KEY(community_id, channel_id)
)

memberships(
  community_id TEXT NOT NULL, channel_id TEXT NOT NULL, pubkey TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'member', source_event_id TEXT NOT NULL,
  PRIMARY KEY(community_id, channel_id, pubkey),
  FOREIGN KEY(community_id,channel_id) REFERENCES channels(community_id,channel_id) ON DELETE CASCADE
)

profiles(
  community_id TEXT NOT NULL, pubkey TEXT NOT NULL, display_name TEXT,
  name TEXT, picture TEXT, nip05 TEXT, about TEXT,
  event_id TEXT NOT NULL, created_at INTEGER NOT NULL, raw_json TEXT NOT NULL,
  PRIMARY KEY(community_id,pubkey)
)

events(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  event_id TEXT NOT NULL, kind INTEGER NOT NULL, pubkey TEXT NOT NULL,
  created_at INTEGER NOT NULL, channel_id TEXT, content TEXT NOT NULL,
  tags_json TEXT NOT NULL, raw_json TEXT NOT NULL,
  root_event_id TEXT, parent_event_id TEXT,
  deleted_by_event_id TEXT, received_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,event_id)
)
CREATE INDEX events_channel_order ON events(community_id,channel_id,created_at,event_id);
CREATE INDEX events_thread_order ON events(community_id,root_event_id,created_at,event_id);
CREATE INDEX events_kind_author ON events(community_id,kind,pubkey,created_at);

reactions(
  community_id TEXT NOT NULL, reaction_event_id TEXT NOT NULL,
  target_event_id TEXT NOT NULL, pubkey TEXT NOT NULL, emoji TEXT NOT NULL,
  created_at INTEGER NOT NULL, deleted_by_event_id TEXT,
  PRIMARY KEY(community_id,reaction_event_id)
)
CREATE INDEX reactions_target ON reactions(community_id,target_event_id,emoji);

read_contexts(
  community_id TEXT NOT NULL, identity_pubkey TEXT NOT NULL,
  context_id TEXT NOT NULL, read_at INTEGER NOT NULL,
  source_created_at INTEGER NOT NULL DEFAULT 0, publishable INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(community_id,identity_pubkey,context_id)
)

read_slots(
  community_id TEXT NOT NULL, identity_pubkey TEXT NOT NULL, slot_id TEXT NOT NULL,
  client_id TEXT NOT NULL, event_id TEXT, event_created_at INTEGER NOT NULL DEFAULT 0,
  is_local INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(community_id,identity_pubkey,slot_id)
)

sync_cursors(
  community_id TEXT NOT NULL, scope TEXT NOT NULL, scope_id TEXT NOT NULL,
  high_created_at INTEGER NOT NULL DEFAULT 0, high_event_id TEXT NOT NULL DEFAULT '',
  complete_through INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,scope,scope_id)
)

outbox(
  community_id TEXT NOT NULL, event_id TEXT NOT NULL, event_json TEXT NOT NULL,
  kind INTEGER NOT NULL, channel_id TEXT, state TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0, last_error_code TEXT,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,event_id)
)

drafts(
  community_id TEXT NOT NULL, channel_id TEXT NOT NULL,
  thread_root_id TEXT NOT NULL DEFAULT '', body TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,channel_id,thread_root_id)
)

ui_state(
  community_id TEXT NOT NULL, key TEXT NOT NULL, value_json TEXT NOT NULL,
  PRIMARY KEY(community_id,key)
)
```

Checks constrain UUID/hex lengths, booleans, known outbox states, and nonnegative timestamps. The event table preserves raw canonical JSON even when the current client cannot render the kind.

### 7.2 Migration approach

- Embed numbered SQL files and apply each in an exclusive transaction using `PRAGMA user_version` plus a `schema_migrations(version,sha256,applied_at)` checksum table.
- Refuse a changed checksum for an already-applied migration.
- Before the first migration of a nonempty database, use SQLite's backup API to create one timestamped backup; retain the newest two.
- Migrations are forward-only. Downgrade means restoring the pre-migration backup, never ad-hoc reverse SQL.
- Every migration gets tests for fresh creation, previous-version upgrade, rerun idempotence, foreign keys, and representative queries.
- Add FTS5 and media-cache tables only in post-MVP migrations; do not burden the first schema with deferred features.

## 8. Authentication, key storage, and security design

### 8.1 Threat model

Protect against accidental secret disclosure, malware with ordinary file access but not an already-compromised logged-in desktop, malicious relay payloads, hostile links/media, replayed auth, wrong-community host binding, terminal escape injection, crash/log leakage, and supply-chain compromise. The design cannot protect keys from a process debugger/root/admin or from a malicious binary running as the same user while `bzz` is unlocked; document that boundary.

The SQLite cache itself contains private team conversation content. File permissions, OS full-disk encryption, lock-screen hygiene, and explicit purge matter even though the signing key is separate. SQLite deletion is not guaranteed forensic erasure on copy-on-write/SSD filesystems.

### 8.2 Identity lifecycle

- `bzz identity new`: generate inside the signer/key service and show only the public key by default. A one-time nsec backup requires a second confirmation and writes only to the controlling TTY, never logs/clipboard automatically.
- `bzz identity import`: read nsec/hex through a no-echo TTY prompt. Reject secret CLI flags and secret environment variables.
- Default backend: OS credential store through `keyring`, service `dev.arpagon.bzz`, account `identity:<uuid>`.
- Encrypted fallback: `${data_dir}/keys/<uuid>.key`, mode `0600`, versioned envelope using Argon2id (64 MiB, 3 iterations, parallelism 1, random 16-byte salt) and XChaCha20-Poly1305 (random 24-byte nonce). Store KDF parameters/salt/nonce/ciphertext, never a verifier derived from the raw key.
- Interactive fallback passphrases come from the controlling TTY. Headless operation may supply a passphrase only through an already-open inherited file descriptor named by `BZZ_PASSPHRASE_FD`; read once, close it, and never accept the passphrase itself in argv/environment/stdin pipelines.
- `secrecy` and `zeroize` wrap transient secret bytes. One signer actor owns `nostr::Keys`; UI/network/store tasks receive signatures/ciphertext only. Disable core dumps where the OS permits and redact panic/error chains.
- Auto-lock after a configurable idle period is post-MVP, but explicit `:lock` clears the active signer and disconnects in MVP.

### 8.3 Relay URL and host policy

- Default allow only `wss://` with system-root TLS verification.
- Permit `ws://` only for loopback addresses unless the user passes and persists an explicit insecure-development acknowledgement. Never silently downgrade.
- Reject URL credentials, fragments, query strings, non-root paths in MVP, empty hosts, and schemes other than WS/WSS. Preserve explicit non-default ports.
- Canonical authority follows `buzz_core::tenant::relay_url_authority`; community database IDs are local opaque UUIDs, never sent to the relay.
- HTTP calls use the exact same authority and mapped TLS scheme. Disable redirects because the signed NIP-98 `u` tag and community host must not move to another origin.
- NIP-11's relay pubkey is pinned on first successful add and a later change requires an explicit trust confirmation before relay-signed discovery updates are accepted.

### 8.4 NIP-42/NIP-98 rules

- Unlock before opening WebSocket. Start a 4-second client AUTH timer to stay inside the server's 5-second close.
- Accept one challenge per connection, cap at 1024 bytes, sign it with exact canonical relay URL, and never reuse an AUTH event on reconnect.
- Validate the `OK` event ID and accepted flag. Classify malformed, clock-skew, banned, not-member, and transient network failures separately.
- Reconnect always performs a fresh NIP-42 exchange before replaying subscriptions/outbox.
- Every NIP-98 request signs its exact method/full URL/body hash and a random nonce. Never redirect or reuse the header; rate-limit retries create a new auth event but reuse an already-signed payload event.

### 8.5 Content and operational safety

- Strip or visibly replace C0/C1 controls, ESC, CSI/OSC/APC sequences, and bidi override controls before rendering. Preserve newline/tab only through the layout parser.
- Links are inert text until an explicit open command; show the resolved URL and require confirmation for non-HTTP(S) schemes. Invoke an opener without a shell.
- Media is deferred. Its later gate must enforce same-origin by default, MIME sniffing, byte/dimension/decompression limits, SHA-256, a cache quota, and serialized terminal graphics output.
- Tracing is opt-in and metadata-only. Never log full event JSON/content at normal levels, AUTH events, NIP-98 headers, key references containing secrets, passphrases, or decrypted read blobs.
- Release profile uses `panic=abort` only after terminal restoration is guaranteed by a panic hook; debug builds retain backtraces with redacted data.

### 8.6 Supply chain and licensing

- Root project license: dual MIT OR Apache-2.0, unless the maintainer chooses one before Milestone 0 merges.
- Buzz is Apache-2.0; pinned source dependencies retain their license. Include its license in generated third-party notices. The inspected Buzz tree has no root `NOTICE` file.
- slk is MIT. This plan copies concepts, not code/assets. If implementation copies a nontrivial function, test fixture, text asset, or generated table, mark it in the source header and preserve the MIT copyright/license in `THIRD_PARTY_LICENSES.md`.
- Run `cargo deny check bans licenses advisories sources`, `cargo audit`, locked/frozen builds, and SBOM generation. Git dependencies are allowed only for the two pinned Buzz crates.
- Dependabot/Renovate updates are separate reviewable PRs; no floating Git branches/tags.

## 9. Realtime, reconnect, backfill, deduplication, and read algorithms

### 9.1 Startup

1. Resolve paths/config; open and migrate SQLite; load communities and cached UI state.
2. Prompt to unlock the selected identity; start the signer actor.
3. Enter alternate screen/raw mode and render cache immediately.
4. Start the active community supervisor. Probe NIP-11 if stale, connect WS, answer AUTH, and wait for accepted `OK`.
5. Start global own-event subscriptions (membership notifications and read state) and one channel-scoped subscription per joined channel. Include the currently viewed open non-member channel on demand.
6. Reconcile directory/profiles/history in background; emit coalesced store-change events.
7. Retry pending outbox entries only after auth and access reconciliation.

### 9.2 WebSocket ownership

One session has one reader task and one writer task. The writer is the only sink owner and accepts bounded commands: `Auth`, `Req`, `Close`, `Publish`, `Pong`, `Shutdown`. The reader parses envelopes and routes by subscription/event ID into bounded channels. Unknown/malformed server frames become protocol errors, not panics.

Heartbeat responds to server ping immediately and sends a client ping after 30 seconds idle. No pong or inbound frame for 60 seconds marks the session stalled and reconnects. Slow UI/store consumers cannot block pong/auth/control messages; control has a separate priority lane.

### 9.3 Subscription and backfill algorithm

For each joined channel with saved high-water `(t,id)`:

1. Open the channel-scoped live `REQ` first with `since=max(0,t-300)` and a bounded historical limit. The relay registers the subscription around its historical delivery and ends catch-up with `EOSE`; all overlap duplicates are harmless.
2. After `EOSE`, execute newest-to-oldest `/query` pages using `(until,before_id)` until the oldest returned tuple reaches/passes the old high-water or the relay returns a short page.
3. For each content page, fetch auxiliary events by `#e` in chunks of 100, then fetch deletions of reaction IDs. Commit verified raw and derived rows transactionally.
4. Advance the high-water only after the pass proves it crossed the old watermark without an error/cap ambiguity. Otherwise leave it unchanged so the next pass repeats safely, following the correctness idea in slk's `reconnect_backfill.go`.
5. The open live subscription closes the race between the HTTP snapshot and ongoing publication. Event-ID upsert closes overlap between WS history, HTTP, echo, and outbox.

A signed Nostr `created_at` is not a relay sequence. A message first published after a long offline period with an arbitrarily old timestamp can evade any time-based backfill. Mitigate with a five-minute overlap, periodic recent-window reconciliation, and explicit “full resync channel”; document that perfect discovery would require a relay sequence/cursor extension.

Limit live subscriptions to 900 (below the relay's 1024 maximum). If a user joins more channels, keep active/recent/unread channels subscribed and poll remaining channel heads through batched `/query`; surface degraded-live status rather than silently dropping updates.

### 9.4 Deduplication and derived state

- Identity key is `(community_id,event_id)`, not event ID alone, because the same global event can be reposted in more than one host-bound community.
- Verify ID/hash/signature before insert.
- `INSERT ... ON CONFLICT` may update only receipt metadata or byte-identical canonical fields; conflicting bytes for an existing ID are a protocol violation.
- Derived thread parent/root comes only from valid marked `e` tags.
- Reaction aggregates include nondeleted kind `7` rows grouped by `(target,emoji)`; own-state is whether a row's pubkey equals self.
- A kind `5/9005` applies only to its referenced event according to relay-authorized data. Preserve the target and deletion event; render a tombstone. Late reaction/edit events against an already-deleted target remain raw but do not revive it.
- Replaceable kinds choose greatest `(created_at,event_id)` within their proper community/author/kind/`d` coordinate.

### 9.5 Outbox and optimistic actions

1. Build and sign once.
2. In one SQLite transaction insert raw event plus `outbox(state='pending')`; render it with a pending marker.
3. Send exact JSON and await matching `OK` for 25 seconds.
4. `OK true` or observing the same event ID marks delivered. A definitive `OK false` marks rejected and exposes retry/copy text.
5. Socket loss/timeout is `unknown`, not rejected. On reconnect query by ID; if found, mark delivered; otherwise resend the exact signed event.
6. A duplicate rejection after resend is accepted only after query-by-ID confirms storage.

### 9.6 Read and unread correctness

- Context IDs follow Desktop: bare channel UUID, `thread:<64hex-root>`, `msg:<64hex-reply>`.
- Effective read frontier is `max(own_context,parent_context)` where thread/message parent relationships come from the local event graph. Missing parent degrades to own marker.
- `has_unread(channel)` is true when a non-self conversational event's `created_at` is greater than the effective channel/thread context marker, or when the user explicitly marked it unread locally. Self-authored events never create an unread badge.
- Opening a channel does not immediately mark it read. Advance only when it is focused and scrolled to the live bottom, using the maximum event timestamp actually observed.
- Local advancement and sidebar update commit synchronously. Network publication is debounced and may fail without rolling back local state.
- Merge every valid remote context with `max(local,remote)`. Never delete a context because another slot omitted it.
- Before publishing, refetch local slot coordinates and merge. Persist stable random `client_id`, 16-byte hex primary slot ID, and extra slots.
- Follow Desktop's limits: seven-day fetch horizon, 500 events, 10,000 parsed contexts, 32 KiB plaintext, at most eight slots. Evict oldest `msg:` then `thread:` contexts; never evict channel keys. Split channel keys round-robin if needed.
- Publish `created_at=max(now,max_fetched+1)`. If another client ID occupies the local `d` coordinate, rotate rather than overwrite.
- On quit, attempt one bounded flush; durable SQLite state guarantees retry next start.

## 10. UI information architecture and keymap

### 10.1 Main screen

```text
┌ communities ┬ channels ┬ #channel topic/status ┬ thread (optional) ┐
│ active mark │ unread   │ timeline              │ root + replies    │
│ cached/off  │ private  │ date/new separators   │ own composer      │
├─────────────┴──────────┴───────────────────────┴───────────────────┤
│ mode · connection/backfill/outbox status · contextual key hints   │
└────────────────────────────────────────────────────────────────────┘
```

Responsive states:

- `>=110` columns: community rail + channel sidebar + timeline + optional thread.
- `70–109`: compact community indicator; sidebar togglable; thread overlays or replaces sidebar.
- `<70` or `<15` rows: focused single pane and a clear “terminal too small” state, never corrupt layout.
- Cache renders while offline with a persistent offline/stale badge. Loading does not blank cached rows.

### 10.2 Modes and major states

- `Normal`: pane/message selection and commands.
- `Insert`: channel or thread composer; draft persists on every debounce.
- `Finder`: fuzzy channel/community selection with joined first and open discovery second.
- `Reaction`: fixed Unicode reaction palette.
- `Confirm`: delete, insecure URL, purge, quit with pending work.
- `Help`: complete key map and connection legend.
- Error states: locked identity, no communities, connecting/authenticating, not a member, clock skew, offline cache, rate limited, protocol incompatible, migration failure.

### 10.3 Default MVP keymap

| Context | Keys | Action |
|---|---|---|
| Global normal | `Ctrl-c`, `Q` | Quit confirmation |
| Global normal | `?` | Help overlay |
| Global normal | `Ctrl-t`, `Ctrl-p` | Fuzzy channel/community finder |
| Global normal | `Ctrl-b` | Toggle channel sidebar |
| Global normal | `Ctrl-]` | Toggle selected thread |
| Global normal | `Tab`, `Shift-Tab` | Next/previous pane |
| Global normal | `Ctrl-w h/j/k/l`, `Ctrl-w w` | Vim-style pane focus/cycle |
| Lists/timeline | `j/k`, arrows | Next/previous item |
| Lists/timeline | `gg`, `G` | First/latest item |
| Timeline/thread | `Ctrl-u/d`, `PgUp/PgDn` | Half/full-page scroll without changing semantic selection |
| Sidebar | `Enter` | Open channel; subscribe on demand |
| Timeline | `Enter` | Open selected message's thread |
| Timeline/thread | `i` | Enter composer for current context |
| Timeline/thread | `r` | Reaction picker |
| Timeline/thread | `D` | Delete own selected message with confirmation |
| Timeline | `U` | Mark selected channel unread locally |
| Composer | `Enter` | Send |
| Composer | `Alt-Enter` or `Ctrl-j` | Newline (both documented because terminals differ) |
| Composer | `Esc` | Normal mode, preserving draft |
| Overlays | `Esc`, `q` | Close/cancel |
| Global normal | `1`–`9` | Switch configured community by slot |
| Global normal | `:` | Command line: `community add/remove`, `lock`, `reconnect`, `resync`, `purge-cache` |

Do not implement splits, themes, custom keybindings, mouse actions, image preview, edit, search, presence, or notification controls in MVP. Reserve bindings documented by slk only when the associated feature lands.

### 10.4 Rendering rules

- Render author, local time, sanitized Markdown subset, reaction pills, reply count, pending/rejected/tombstone state, and “new” divider.
- Use grapheme-aware width/wrapping and no raw ANSI from content.
- Keep selection anchored by event ID across inserts and width changes.
- If at live bottom, new events keep the pane at bottom; if reading history, preserve anchor and show “N newer,” never jump.
- Store scroll/selection/draft per `(community,channel,thread)`.

## 11. Testing strategy

### 11.1 Test pyramid

- **Pure unit/property tests:** URL normalization, NIP-42/98 tags, relay envelopes, event validation, NIP-10 roots, replaceable ordering, sanitizer, read max-merge/hierarchy/slot splitting, cursor progression, outbox transitions, unread predicates.
- **Store tests:** temporary SQLite databases, migrations from every version, duplicate and out-of-order event application, dense same-second ordering, WAL concurrency, cascade isolation, malformed raw events, outbox restart.
- **Session tests with fake relay:** proactive challenge, five-second behavior, wrong/multiple challenge, interleaved `EVENT`/`EOSE`/`OK`, `CLOSED`, ping/pong priority, reconnect/backoff, replayed subscriptions, slow consumer, malformed frames.
- **Reducer/widget tests:** fake services, deterministic actions/effects, mode transitions, anchor behavior, narrow layouts, Unicode, offline/error states.
- **Deterministic render tests:** Ratatui `TestBackend` at fixed sizes with stable time/profile fixtures; snapshot the final cell buffer and assert no control escapes.
- **Real Buzz relay integration:** auth, directory, send/echo, thread root/nested reply, reaction/remove, delete, read-state round trip, reconnect, same-second multipage history, access revocation, and community host isolation.
- **Performance gates:** warm startup to first cached frame, 10k-message cache query, 1k-event burst ingestion, `j/k` render benchmark, and memory smoke test.

### 11.2 Local relay harness

`BZZ_BUZZ_SOURCE` points to a read-only checkout at the recorded revision. The project wrapper:

1. verifies `git -C "$BZZ_BUZZ_SOURCE" rev-parse HEAD` equals the pinned SHA unless an explicit compatibility-test override is set;
2. invokes upstream `scripts/start-isolated-test-relay.sh --profile dev`, which uses `docker-compose.harness.yml` on isolated Postgres/Redis/MinIO and relay ports;
3. waits for readiness, runs ignored `cargo test --test relay_integration -- --ignored --test-threads=1` with seeded identities/community;
4. always kills the scoped tmux session and `docker compose -p buzz-harness ... down -v`.

CI checks out `block/buzz` at the exact SHA into a sibling temporary directory; it never copies or edits upstream source in this repository. Fast pull requests run fake-relay tests; the Docker-backed suite runs on Linux main/nightly or when protocol files change.

### 11.3 Required correctness scenarios

- Kill the network after relay storage but before `OK`; restart and prove one event row and a delivered outbox item.
- Deliver the same event over WS history, live echo, HTTP backfill, and outbox query; prove one row and one rendered message.
- Seed more than one page with identical `created_at`; prove all IDs are reachable exactly once.
- Disconnect while more than the cap arrives; prove watermark is not advanced until all pages cross the old cursor.
- Merge two read slots/devices in every order; prove commutativity, associativity, idempotence, and monotonicity.
- Switch communities with identical channel UUID/event ID fixtures; prove no cross-community rows, badges, profiles, or subscriptions leak.
- Revoke private-channel access live; prove subscription closes, cached history stays offline-readable, and no later event is accepted into that channel from network.
- Render malicious ANSI/OSC/bidi and huge Unicode fixtures; prove terminal buffer contains only safe output.

## 12. Packaging and release strategy

- Supported MVP targets: Linux x86_64 and aarch64, macOS x86_64 and arm64, Windows x86_64.
- GitHub Actions builds locked release archives (`tar.xz` on Unix, `.zip` on Windows), SHA-256 checksums, SPDX SBOM, and shell completions/man pages.
- Use `cargo-dist` or equivalent reproducible release orchestration, with pinned action/tool versions. Sign checksums/artifacts with keyless Sigstore/Cosign and attach provenance.
- Linux uses rustls and bundled SQLite; test Secret Service keyring and encrypted fallback. Avoid an OpenSSL runtime dependency.
- macOS tests Keychain and notarization. Codesigning/notarization credentials are release-environment secrets, never repository secrets in fork workflows.
- Windows tests Credential Manager, ConPTY/Crossterm behavior, path permissions, and zip install.
- Publish GitHub Releases first. Add Homebrew and Scoop manifests after two successful releases; crates.io publishing is optional because the binary uses revision-pinned Git protocol crates.
- Versioning begins `0.1.0`; cache schema migration compatibility is part of semver release notes. Release notes name the tested Buzz SHA/protocol compatibility.

## 13. Milestones and exact file plan

Line counts are implementation estimates, not quotas. `C` means create; `M` means modify. Generated `Cargo.lock`/snapshots are reviewed but excluded from hand-written line totals. No implementation begins until this plan is approved.

### M0 — Repository, terminal shell, configuration (about 1,150 lines)

Goal: `bzz` starts/restores the terminal safely, loads non-secret config, and shows deterministic no-community/help screens.

| Action | File | Est. lines |
|---|---|---:|
| C | `Cargo.toml` | 105 |
| C | `Cargo.lock` | generated |
| C | `rust-toolchain.toml` | 4 |
| C | `.gitignore` | 20 |
| C | `deny.toml` | 70 |
| C | `LICENSE-APACHE` | standard text |
| C | `LICENSE-MIT` | standard text |
| C | `THIRD_PARTY_LICENSES.md` | 45 |
| C | `README.md` | 140 |
| C | `src/main.rs` | 85 |
| C | `src/lib.rs` | 35 |
| C | `src/error.rs` | 90 |
| C | `src/paths.rs` | 100 |
| C | `src/config.rs` | 230 |
| C | `src/app.rs` | 180 |
| C | `src/ui/mod.rs` | 35 |
| C | `src/ui/terminal.rs` | 155 |
| C | `src/ui/keymap.rs` | 115 |
| C | `tests/config_test.rs` | 140 |
| C | `tests/terminal_restore_test.rs` | 80 |

Acceptance: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo run` from a temporary HOME; panic/quit restores terminal; config rejects unknown/insecure relay settings without exposing raw secrets.

### M1 — Identity, signer actor, and protocol primitives (about 2,050 lines)

Goal: generate/import/unlock an identity and pass fake-relay NIP-42/NIP-98/event-builder tests without networking the TUI.

| Action | File | Est. lines |
|---|---|---:|
| M | `Cargo.toml` | +35 |
| C | `src/auth/mod.rs` | 150 |
| C | `src/auth/keychain.rs` | 170 |
| C | `src/auth/encrypted_file.rs` | 260 |
| C | `src/auth/signer.rs` | 300 |
| C | `src/protocol/mod.rs` | 70 |
| C | `src/protocol/types.rs` | 190 |
| C | `src/protocol/url.rs` | 170 |
| C | `src/protocol/envelope.rs` | 210 |
| C | `src/protocol/nip42.rs` | 150 |
| C | `src/protocol/nip98.rs` | 190 |
| C | `src/protocol/events.rs` | 230 |
| C | `tests/auth_test.rs` | 230 |
| C | `tests/protocol_test.rs` | 260 |
| M | `src/main.rs` | +45 |
| M | `src/lib.rs` | +10 |

Acceptance: no secret accepted via argv/env; encrypted fallback round-trips and fails closed on tampering; known Buzz tag fixtures match; signer zeroizes input buffers; `cargo deny check` passes.

### M2 — SQLite cache, migration, and event reducer (about 2,150 lines)

Goal: open/migrate isolated stores and idempotently apply raw events into channel/profile/thread/reaction/deletion/read/outbox state.

| Action | File | Est. lines |
|---|---|---:|
| C | `migrations/0001_init.sql` | 260 |
| C | `src/store/mod.rs` | 180 |
| C | `src/store/migrate.rs` | 170 |
| C | `src/store/models.rs` | 260 |
| C | `src/store/writer.rs` | 270 |
| C | `src/store/events.rs` | 360 |
| C | `src/store/queries.rs` | 340 |
| C | `src/domain.rs` | 240 |
| C | `tests/store_migration_test.rs` | 190 |
| C | `tests/store_events_test.rs` | 300 |
| M | `src/app.rs` | +40 |
| M | `src/lib.rs` | +10 |

Acceptance: schema/query tests, cross-community collision test, randomized duplicate/order test, and database backup/checksum behavior pass under a temporary directory.

### M3 — Authenticated session, NIP-98 client, and relay harness (about 2,350 lines)

Goal: authenticate, subscribe, query, publish, reconnect, and run a minimal real-relay smoke test.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/realtime/mod.rs` | 80 |
| C | `src/realtime/session.rs` | 420 |
| C | `src/realtime/supervisor.rs` | 300 |
| C | `src/realtime/subscriptions.rs` | 250 |
| C | `src/realtime/pending.rs` | 180 |
| C | `src/protocol/http.rs` | 260 |
| C | `tests/support/mod.rs` | 30 |
| C | `tests/support/fake_relay.rs` | 330 |
| C | `tests/session_test.rs` | 340 |
| C | `tests/relay_integration.rs` | 260 |
| C | `scripts/test-relay.sh` | 120 |
| M | `src/main.rs` | +30 |
| M | `src/app.rs` | +70 |

Acceptance: challenge is answered within four seconds; terminal vs transient auth errors differ; interleaved envelopes route correctly; reconnect reauthenticates before subscriptions; real relay authenticates and round-trips one kind `9` event.

### M4 — Community/channel/profile hydration and navigable shell (about 2,100 lines)

Goal: configure/switch communities, render cached joined channels, browse open channels, and show human author names.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/service/mod.rs` | 80 |
| C | `src/service/community.rs` | 260 |
| C | `src/service/channels.rs` | 320 |
| C | `src/service/profiles.rs` | 180 |
| C | `src/sync/mod.rs` | 50 |
| C | `src/sync/directory.rs` | 320 |
| C | `src/ui/layout.rs` | 170 |
| C | `src/ui/sidebar.rs` | 260 |
| C | `src/ui/finder.rs` | 230 |
| C | `src/ui/status.rs` | 130 |
| M | `src/app.rs` | +300 |
| M | `src/ui/keymap.rs` | +40 |
| M | `src/store/queries.rs` | +110 |
| C | `tests/directory_test.rs` | 240 |
| C | `tests/ui_shell_test.rs` | 180 |

Acceptance: two fixture communities with colliding channel IDs stay isolated; SQLite channels appear before network completion; finder ranks joined/frecent/open channels; membership notification causes safe refresh.

### M5 — Timeline, deterministic rendering, and complete backfill (about 2,250 lines)

Goal: cached/live/older channel history with safe Markdown, stable scrolling, aux state, and no reconnect gaps/duplicates.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/sync/backfill.rs` | 430 |
| C | `src/sync/auxiliary.rs` | 230 |
| C | `src/render/mod.rs` | 35 |
| C | `src/render/sanitize.rs` | 150 |
| C | `src/render/markdown.rs` | 240 |
| C | `src/ui/timeline.rs` | 430 |
| M | `src/realtime/subscriptions.rs` | +150 |
| M | `src/store/events.rs` | +180 |
| M | `src/store/queries.rs` | +150 |
| M | `src/app.rs` | +220 |
| C | `tests/backfill_test.rs` | 330 |
| C | `tests/render_test.rs` | 260 |
| C | `tests/ui_timeline_test.rs` | 220 |

Acceptance: dense same-second multipage fixture is complete; disconnect burst crosses old cursor before watermark advances; event delivered four ways renders once; malicious control payload cannot emit a control sequence; latest-anchor behavior is deterministic.

### M6 — Compose, outbox, threads, reactions, deletion (about 2,550 lines)

Goal: all conversational MVP writes and thread UI work with pending/rejected/ambiguous states.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/service/messages.rs` | 330 |
| C | `src/sync/outbox.rs` | 300 |
| C | `src/ui/composer.rs` | 330 |
| C | `src/ui/thread.rs` | 390 |
| C | `src/ui/reaction_picker.rs` | 180 |
| C | `src/ui/confirm.rs` | 120 |
| M | `src/protocol/events.rs` | +180 |
| M | `src/realtime/session.rs` | +120 |
| M | `src/store/events.rs` | +160 |
| M | `src/store/queries.rs` | +110 |
| M | `src/app.rs` | +300 |
| C | `tests/outbox_test.rs` | 280 |
| C | `tests/thread_test.rs` | 300 |
| C | `tests/conversation_ui_test.rs` | 260 |

Acceptance: direct and nested replies have exact tags; reaction add/remove aggregates correctly; delete is own-only in UI and relay-authoritative; dropped-`OK` scenario recovers one event; drafts survive restart.

### M7 — Cross-device read state and unread correctness (about 1,700 lines)

Goal: monotonic local unread state and interoperable encrypted kind `30078` synchronization.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/sync/read_state.rs` | 520 |
| C | `src/service/read_state.rs` | 230 |
| C | `src/ui/unread.rs` | 160 |
| M | `src/auth/signer.rs` | +120 |
| M | `src/realtime/subscriptions.rs` | +90 |
| M | `src/store/queries.rs` | +140 |
| M | `src/ui/sidebar.rs` | +100 |
| M | `src/ui/timeline.rs` | +90 |
| M | `src/ui/thread.rs` | +80 |
| M | `src/app.rs` | +130 |
| C | `tests/read_state_test.rs` | 380 |
| C | `tests/unread_test.rs` | 220 |
| M | `tests/relay_integration.rs` | +140 |

Acceptance: property tests prove max-merge algebra; two client slots converge through real relay; failed publish never regresses local marker; background channel/thread parent resolution cannot borrow the active channel; read badges survive restart/reconnect.

### M8 — UX hardening, full MVP integration, and performance (about 1,450 lines)

Goal: polish all screen/error states, multi-community switching, help, locking, throttling, and benchmark gates.

| Action | File | Est. lines |
|---|---|---:|
| C | `src/ui/help.rs` | 190 |
| C | `src/ui/command.rs` | 230 |
| M | `src/ui/status.rs` | +130 |
| M | `src/ui/layout.rs` | +100 |
| M | `src/ui/timeline.rs` | +120 |
| M | `src/app.rs` | +260 |
| M | `src/service/community.rs` | +120 |
| M | `src/realtime/supervisor.rs` | +110 |
| C | `tests/ui_snapshot_test.rs` | 270 |
| C | `tests/mvp_journey_test.rs` | 300 |
| C | `benches/timeline.rs` | 150 |
| C | `benches/store.rs` | 130 |
| M | `README.md` | +120 |

Acceptance: all MVP journey scenarios pass; 50 ms target for warm cached first model and no visible `j/k` frame over 16 ms on reference hardware; 1,000-event burst remains bounded; lock disconnects/clears signer; all supported terminal sizes snapshot cleanly.

### M9 — CI, release, and operator documentation (about 950 lines)

Goal: reproducible signed artifacts for all supported platforms and complete user/security docs.

| Action | File | Est. lines |
|---|---|---:|
| C | `.github/workflows/ci.yml` | 170 |
| C | `.github/workflows/integration.yml` | 150 |
| C | `.github/workflows/release.yml` | 180 |
| C | `dist-workspace.toml` | 90 |
| C | `release.toml` | 70 |
| C | `docs/configuration.md` | 180 |
| C | `docs/security.md` | 210 |
| C | `docs/protocol-compatibility.md` | 150 |
| C | `docs/troubleshooting.md` | 180 |
| C | `scripts/generate-completions.rs` | 110 |
| M | `Cargo.toml` | +30 |
| M | `README.md` | +100 |
| M | `THIRD_PARTY_LICENSES.md` | +80 |

Acceptance: clean VMs install and run every archive; checksum/signature/SBOM verify; keychain and encrypted fallback smoke tests pass on each OS; CI is locked and protocol docs name exact tested Buzz revision.

### Post-MVP order

1. Workspace private/group DMs (`41010/41001/30622`) with a separate NIP-17 design note.
2. Simultaneous community sessions and aggregate unread rail.
3. NIP-50 profile/message search and local FTS.
4. Editing/rich `40002`/diff/system overlays.
5. Attachments, Blossom, terminal images, and cache quotas.
6. Presence, typing, notifications.
7. Custom emoji, themes, configurable bindings.
8. Activity feed and workflow approvals.

## 14. Risks, open questions, assumptions, and non-goals

### Risks and mitigations

| Risk | Impact | Mitigation / gate |
|---|---|---|
| Buzz protocol and crates are pre-1.0 and fast-moving | Compile or behavior drift | Exact Git SHA, lockfile, fixture tests, real-relay suite, one dedicated upgrade PR. |
| 5-second auth deadline plus locked keychain | Reconnect loop | Unlock before socket; 4-second client timer; terminal auth state after rejection. |
| No relay sequence cursor | Old-timestamp offline publication can evade time catch-up | Live-first race closure, overlap, periodic reconciliation, full-resync command, document limit. |
| One channel live subscription each | Subscription/CPU pressure at very large membership | 900 cap, active/LRU prioritization, head polling, visible degraded status. |
| Read-state implementation is large | Incorrect cross-device unreads | Port behavior, not source, from Desktop tests; algebra/property tests and two-client E2E before release. |
| Git workspace dependencies | Cargo resolver/build fragility | Restrict to core/sdk, compile pinned SHA in CI; local protocol fallback instead of vendoring. |
| Keyring backend differences | Lockout or unsafe fallback | Per-OS smoke tests, encrypted fallback, export backup flow, no automatic plaintext migration. |
| Terminal text can be hostile | Escape injection/spoofing | Central sanitizer, no bypassing raw write, property/fuzz tests. |
| Cached private content at rest | Local disclosure | `0600`, documented disk-encryption boundary, purge flow, no secrets in DB. |
| Cross-platform Ratatui/Crossterm differences | Broken keys/restoration | TestBackend snapshots plus PTY/ConPTY smoke tests on release matrix. |

### Open questions to resolve before the named milestone

1. **M0 licensing choice:** accept recommended dual MIT/Apache-2.0 or choose one. This does not change architecture.
2. **M1 keychain crate backends:** validate Linux Secret Service behavior without a desktop bus and Windows credential size limits; encrypted fallback is required regardless.
3. **M3 relay key pinning:** confirm NIP-11 `pubkey` stability for every supported deployment/proxy and define the exact trust-reset prompt.
4. **M4 open-channel UX:** whether open non-member channels should create unread badges. Plan default: finder-only until opened/joined.
5. **M7 read marker scope:** test `msg:` contexts against current Desktop behavior for nested replies before freezing interoperability fixtures.
6. **Release MSRV:** Rust 1.95 follows the inspected Buzz toolchain; lower MSRV is not an MVP goal and can be evaluated after dependency stabilization.

### Explicit assumptions

- The relay is compatible with the inspected Buzz revision and exposes root WebSocket/NIP-11 plus NIP-98 `/query` and `/events` on the mapped HTTP origin.
- User identity is already admitted to closed communities; invite claiming/onboarding is not MVP.
- Relay/client clocks are within 60 seconds. `bzz` diagnoses skew but does not alter signed time.
- MVP sends basic kind `9` text and does not attempt to author rich kind `40002`.
- One identity can be associated with several community hosts, but profiles/events remain host-local.
- Cached history remains readable after membership loss; the client stops new reads/writes and clearly marks revoked/offline state.

### Explicit non-goals

- Relay hosting, provisioning, or administration.
- Invite/policy onboarding in the first release.
- Managed agents, ACP/MCP, Git forge/project management, canvases, workflows, or moderation administration.
- Huddles, audio, video, web/mobile/GUI clients.
- Full Buzz Desktop parity.
- Slack authentication/private APIs or direct reuse of slk's Slack transport.
- DMs/NIP-17, search, media/images, presence/typing, edit/rich authoring, custom emoji, notifications, themes, and configurable keys in MVP.
- Guaranteed forensic deletion or protection against root/debugger access to an unlocked process.

## 15. Definition of done for MVP

The MVP is done only when all are true:

- [x] A new user can generate or interactively import a Nostr identity; no test/log/config/DB/process argument contains the secret.
- [x] OS keychain and encrypted-file fallback pass documented restore/recovery tests.
- [x] At least two communities can be configured, cached, switched, removed, and purged without cross-community leakage.
- [x] Every connection completes fresh host-bound NIP-42 auth and correctly classifies membership, banned, skew, and transient failures.
- [x] Joined/open channel discovery and kind `0` author labels match the real relay.
- [x] Warm startup renders cached channels/history while offline.
- [x] History, live tail, direct/nested threads, reactions/removal, and self-delete work against the pinned relay.
- [x] Same event delivered by every path stores/renders once; dense same-second pages lose no IDs.
- [x] Ambiguous publish recovery produces one durable event and correct pending/delivered UI.
- [x] Reconnect with a gap larger than one page repairs all messages before advancing its cursor.
- [x] Channel/thread unread markers never move backward and two clients' kind `30078` states converge.
- [x] Vim navigation, fuzzy switching, help, drafts, latest/history anchoring, narrow/offline/error states, and terminal restoration have deterministic tests.
- [x] Malicious event content cannot emit terminal controls, invoke a shell, or auto-open a URL.
- [x] Unit, property, store, fake-relay, real-relay, snapshot, and benchmark gates pass.
- [x] Linux/macOS/Windows artifacts install, run, verify signatures/checksums/SBOM, and state the tested Buzz SHA.
- [x] License/advisory/source checks pass and any adapted source is attributed.
- [x] README, configuration, security, protocol compatibility, and troubleshooting documentation are complete.

## 16. Implementation handoff (completed)

Implementation proceeded through M0–M9 with the milestone boundaries and pinned upstream evidence above. The final audit includes the protocol-free test suite, Clippy, supply-chain checks, benchmarks, terminal quit/signal restoration smoke tests, encrypted-file and OS-keychain smoke tests, cargo-dist/SBOM planning, and the pinned real-relay journey.

## 17. Intuitive summary

Think of `bzz` as three machines sharing one screen:

1. **The safe** holds the Nostr key and signs small, explicit requests. The rest of the app never handles the secret.
2. **The librarian** owns SQLite. It remembers every community separately, accepts the same signed event as many times as necessary, and reduces those events into messages, threads, reactions, and unread markers.
3. **The radio** maintains one authenticated Buzz connection, subscribes to each relevant channel, repairs missed history, and reports acknowledgements. If the radio fails, the librarian still lets the user read cached history.

Ratatui is only the view and keyboard controller over those machines. A keypress creates an effect; an effect asks a service to sign, store, query, or publish; the resulting domain event updates the view. That separation is what makes the app feel immediate without pretending that an optimistic send is already durable.

The subtle work is not drawing boxes. Buzz chooses a community from the relay host, demands a fresh challenge response very quickly, routes live channel events differently from global events, paginates dense history with an event-ID tiebreak, and synchronizes encrypted read markers by maximum rather than replacement. Encoding those rules once in protocol/store/session layers—and testing reconnects as aggressively as slk tests its cache and models—is how `bzz` can feel simple to a human while remaining correct underneath.
