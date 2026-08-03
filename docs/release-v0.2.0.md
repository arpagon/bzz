# bzz v0.2.0

`v0.2.0` adds an active-community Inbox, Buzz Desktop-compatible workspace
DMs, and unified offline/online search. Protocol compatibility remains pinned
to Buzz `ede26863345a518ec46edd6d7692e0281883491b`.

## Highlights

- Inbox filters for all activity, mentions, relevant threads, workspace DMs,
  read-only needs-action status, unread rows, and drafts.
- Stable NIP-10/DM grouping, shared channel/thread/message read contexts, local
  mark-unread overrides, exact-context navigation, and cache-only locked mode.
- One-to-one and group workspace DMs using kinds 41010/41011/41012,
  relay-signed 39000/39002 discovery, and owner-only kind 30622 visibility.
- Immutable 2–9-person participant sets, strict command responses, durable
  ambiguity recovery, hide/reopen, and add-participant-as-new-conversation.
- Unified channel/DM/person/message search with local SQLite FTS5 and
  NIP-98-authenticated NIP-50 prefix completion.
- `from:`, `in:`, `after:YYYY-MM-DD`, and `before:YYYY-MM-DD` operators that
  fail closed when identity/channel resolution is ambiguous.
- New keys: `I` Inbox, `/` search, `Ctrl-n` new DM, `H` hide DM, and `A` add a
  DM participant. Equivalent commands are `:inbox`, `:search`, `:dm`,
  `:dm hide`, and `:dm add`.

Workspace DMs are private relay channels but are **not end-to-end encrypted**.
The relay can read ordinary DM message bodies. NIP-17 gift wraps remain a
separate unsupported UI protocol and are never indexed.

## Security and privacy

- Kind 30622 is accepted only for the active viewer (`p=self`, `d=self`) and
  only from the pinned NIP-11 relay signer.
- Hidden DMs, deleted/rejected messages, gift wraps, private unsupported kinds,
  attachment bytes/metadata, source paths, and secrets do not enter local FTS.
- Every remote search hit is signature-, community-, membership-, visibility-,
  deletion-, and kind-checked after delivery.
- Query text, participant lists, message content, auth material, and full real
  identifiers are not logged.
- Query input, filters, pages, response bytes, command responses, participant
  sets, actor queues, Inbox windows, and result hydration are bounded.

## Database migration

Schema version 3 adds `event_mentions`, `channel_membership_heads`,
`dm_visibility_heads`, `dm_visibility`, `inbox_overrides`, `search_documents`, and external-content
`search_fts`. Existing v2 databases receive the normal owner-only
pre-migration backup. Searchable message text is rebuilt once and checked with
FTS5 integrity validation. Downgrade by restoring that backup; no reverse SQL
is provided.

## Validation

- formatting, Clippy with warnings denied, all-target tests, release builds,
  `cargo deny`, and `cargo audit`; the lockfile uses patched `nostr` 0.44.7;
- Linux/macOS/Windows migration and feature tests;
- pinned real-relay two/three-client DM, hide/reopen, private-access, Inbox,
  NIP-50, NIP-17 exclusion, media, reconnect, read-state, and dense-cursor
  journey;
- benchmark over 100,000 generated messages: latest-500 approximately
  1.21–1.31 ms, FTS5 approximately 0.84–0.87 ms, Inbox projection
  approximately 6.92–7.71 ms on the validation host;
- cargo-dist archives/installers/checksums/SBOMs and GitHub attestations on the
  existing five-target release matrix.

See [`inbox-dms-search.md`](inbox-dms-search.md),
[`protocol-compatibility.md`](protocol-compatibility.md), and
[`security.md`](security.md).
