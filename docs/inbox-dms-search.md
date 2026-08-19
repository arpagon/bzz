# Inbox, workspace DMs, and search

This document describes the Inbox, Buzz workspace DM, and unified search
surfaces added after `v0.1.0`. Protocol behavior remains pinned to Buzz
`ede26863345a518ec46edd6d7692e0281883491b`.

## Keys and commands

| Surface/action | Key | Command |
|---|---|---|
| Open Inbox | `Space n` | `:inbox` |
| Open unified search | `/` | `:search` |
| Open a workspace DM | — | `:dm` |
| Hide the selected DM | — | `:dm hide` |
| Add one DM participant | — | `:dm add` |

Inbox uses `j/k`, `f` to cycle filters, `Enter` for detail, `o` to open the
source, `i` to open and reply, `m` to mark read, `U` to toggle a local unread
override, and `a` to mark every loaded row read. On narrow terminals, `Esc`
returns from detail before closing Inbox.

Search accepts ordinary text and `from:`, `in:`, `after:YYYY-MM-DD`, and
`before:YYYY-MM-DD`. Dates use local midnight; `after:` is inclusive and
`before:` excludes the named day. Operators must begin at a whitespace-delimited
token. Later duplicates win. Invalid dates remain literal search text. An unresolved or
ambiguous person/channel fails closed and does not widen the search. Remote
typeahead begins after two non-whitespace characters and waits 300 ms after the
last edit. Arrow keys select a result; `Enter` opens its exact channel/thread
context. Selecting a person opens a one-to-one workspace DM.

The DM picker uses text filtering, arrows, `Space` to select, and `Enter` to
open. A group supports one to eight recipients in addition to the active
identity. Duplicate recipients and self-selection are rejected. Adding a
participant opens or reuses a **different** immutable participant-set DM; it
does not alter the previous conversation.

## Workspace DM security model

Buzz Desktop-compatible workspace DMs are private, hidden NIP-29 channels.
They are access-controlled by relay membership, but ordinary kind `9`/`40002`
message bodies are visible to the relay. They are **not end-to-end encrypted**.
Do not use this feature when the relay operator must not see the plaintext.

The wire contract is:

- kind `41010`: open/reopen a canonical participant-set DM;
- kind `41011`: open a new DM after adding a participant;
- kind `41012`: hide a DM for the current viewer;
- kind `39000`: relay-signed private/hidden DM metadata (`t=dm`);
- kind `39002`: relay-signed participant set;
- kinds `44100/44101`: p-gated membership notifications;
- kind `30622`: relay-signed, owner-only hidden-DM snapshot (`d=self`,
  `p=self`, one `h` per hidden channel).

The relay returns kind `41010/41011` results in an `OK` payload beginning with
`response:`. `bzz` strictly bounds and parses that payload. It validates the
returned UUID and exact 2–9 participant set against relay-signed discovery.
Commands are signed once and persisted before publication. A lost or malformed
acknowledgement is recovered by querying discovery and matching the exact
participant set; retries reuse the same signed event.

The NIP-29 metadata-only `hidden` tag identifies DM channels and is not viewer
hide state. Only the newest valid kind `30622` snapshot for the active pubkey
controls sidebar visibility. Same-second snapshots follow the relay's NIP-33
ordering (`created_at` descending, event ID ascending). A foreign viewer's
snapshot and an event not signed by the pinned NIP-11 relay key are rejected.

NIP-17 kind `1059` gift wraps, NIP-04, NIP-44 DM transport, and kind `10050`
relay lists are separate protocols and are not authored or displayed by this
feature.

## Inbox model

Inbox is an active-community local read model, not a second message store. It
combines durable events, drafts, channel type/membership, read contexts, and
local overrides. It has `All`, `Mentions`, `Threads`, `DMs`, `Needs action`,
`Unread`, and `Drafts` filters.

Included rows are:

- kind `9`/`40002` events carrying `p=self`;
- replies to roots authored or previously joined by the active identity;
- visible workspace DM activity;
- read-only kind `46010/46011/46012` cards addressed to `p=self`;
- nonempty channel/thread drafts.

Rows group by `dm:<channel-uuid>`, `thread:<validated-NIP-10-root>`, or a stable
event/draft identifier. Delivery through HTTP, live subscriptions, reconnect,
search, and outbox echo deduplicates by `(community,event_id)`. The selected
conversation remains stable when newer activity changes its representative
event.

Unread state reuses the existing channel, `thread:<root>`, and
`msg:<event-id>` contexts. `m` advances those markers monotonically. `U` is a
local row override and never lowers a NIP-RS marker or marks an entire channel
unread. Channel-less needs-action cards use local done state only. Approval
buttons are deliberately not implemented; the cards are informational.

Inbox performs a bounded five-page mention query, two-page needs-action query,
30-second online refresh, and local live/reconnect projection. It displays at
most 500 conversations from a bounded candidate window. Locked mode performs
no refresh and renders only SQLite state.

## Unified search model

Search presents channels, visible DMs, people, and messages in that order.
Empty input shows recent local channels/DMs without network access.

Local search uses:

- fuzzy matching for cached channel/profile labels;
- community-partitioned SQLite FTS5 (`unicode61`) for kind `9`/`40002`
  message text;
- author, channel, and UTC date constraints resolved before the query;
- current membership, deletion/outbox state, and kind `30622` visibility joins
  on every result.

The local index contains sanitized searchable text only. Generated attachment
Markdown is removed before indexing. Attachment bytes, media-cache metadata,
source paths, profile-picture URLs, gift wraps, visibility snapshots, auth
events, encrypted backups, and secrets are never indexed. Authoritative
deletion or rejected outbox state removes the document transactionally. The
index has an integrity check and deterministic one-time rebuild after schema
migration.

Online search adds same-origin, NIP-98-authenticated NIP-50 `/query` requests:

- profile kind `[0]` and message kinds `[9,40002]`;
- `search_mode="prefix"`, `page`, `limit`, `authors`, `#h`, `since`, and
  `until` extensions;
- default 20 results, page maximum 100, and 500 hydrated results per session;
- an asynchronous generation `(community,identity,query revision)` that drops
  stale responses.

Remote relevance order is preserved within people/messages. Every event is
signature-verified, stored under the active community, and checked again for
channel membership, viewer visibility, deletion, and supported kind before it
can render. Navigation stores the exact result then opens a bounded channel or
thread context. Missing/restricted/deleted results produce an error rather than
opening a different row. Locked/offline search is local-only.

## Persistence and migration

Migration `0003_inbox_dm_search.sql` adds:

- `event_mentions`;
- `channel_membership_heads` for deterministic replaceable membership state;
- `dm_visibility_heads` and `dm_visibility`;
- `inbox_overrides`;
- `search_documents`, external-content `search_fts`, and integrity triggers.

The existing pre-migration SQLite backup/checksum policy applies. Downgrading
requires restoring the generated pre-migration backup; no reverse migration is
provided. All rows remain community-partitioned, and viewer-specific rows also
carry the identity pubkey.

## Bounds and failure behavior

Search input is limited to 4 KiB. Query filters, response bodies, command
responses, actor queues, pages, participants, Inbox candidates, and rendered
rows all have hard caps. HTTP redirects remain disabled. Search text,
participant lists, message bodies, auth material, and full identifiers are not
logged. Workers emit no terminal control output; all rendering remains inside
the serialized Ratatui draw.

The closeout benchmark over 100,000 generated accepted messages measured the
latest-500 query at approximately 1.21–1.31 ms, FTS5 search at 0.84–0.87 ms,
and Inbox projection at 6.92–7.71 ms on the release-validation host. These
operations execute on the bounded SQLite owner thread, never in a Ratatui draw.
Re-run with `cargo bench --bench store` when changing indexes or projections.

A relay search or Inbox refresh failure leaves local results usable. A DM
command rejection leaves no authoritative channel. An accepted DM command with
delayed metadata reports pending discovery instead of inventing local state.
A hide remains visible until the owner snapshot confirms it; a later directory
refresh completes the transition.
