# Protocol compatibility

This release targets `block/buzz` revision
`ede26863345a518ec46edd6d7692e0281883491b` and uses revision-pinned
`buzz-core`/`buzz-sdk` dependencies.

Implemented MVP protocol surface:

- NIP-11 relay information and host-derived community identity;
- NIP-42 kind 22242 WebSocket authentication;
- NIP-98 kind 27235 authenticated `/query` and `/events` requests;
- NIP-29 kinds 39000/39002 plus membership notifications;
- workspace DM commands 41010/41011/41012 and relay-signed, owner-only NIP-DV
  kind 30622 visibility snapshots;
- kind 0 profiles; kind 9 messages; NIP-10 threads; kind 7 reactions;
- kind 5 and 9005 tombstones;
- NIP-44-to-self kind 30078 Buzz read-state snapshots;
- NIP-92 `imeta` message attachments;
- Blossom/BUD-01/BUD-11 kind 24242 blob-scoped read and upload authorization;
- exact-byte `PUT /upload` with the narrowly retried `/media/upload` legacy alias;
- tenant-bound `GET /media/{hash.ext}` with optional relay membership enforcement;
- one-shot NIP-50 profile/message prefix search with explicit `page`,
  `search_mode`, author, channel, and UTC-time filters;
- p-gated mention and read-only workflow status (`46010`–`46012`) Inbox
  projection.

bzz combines a small global stream (needed for reaction/removal events that
carry `e` but no `h`) with a bounded set of channel subscriptions, then repairs
reconnect gaps with HTTP composite cursors. Signed Nostr timestamps are not
relay sequence numbers, so old-timestamp late publication requires overlap,
periodic reconciliation, or manual full sync.

Outbound attachment messages retain Buzz Desktop's interoperable body lines
(`![image](url)`, `![video](url)`, or `[filename](url)`) and ordered `imeta`
tags. `bzz` requires current Buzz descriptors to include `url`, `m`, `x`, and
`size`; malformed or external legacy NIP-92 entries degrade to inert cards.
Video playback and arbitrary external NIP-92 hosts are not in the implemented
message-media protocol surface. External kind-0 `picture` URLs are an
independent credential-free presentation path. A canonical picture at the
active relay's `/media/{hash.ext}` path may use the existing hash-scoped
Blossom read authorization only after exact origin/path validation; it neither
extends NIP-92 nor authorizes any other profile URL.

Buzz workspace DMs are ordinary relay-readable kind `9`/`40002` events inside
private hidden NIP-29 channels. They are not NIP-17 gift wraps and are not
end-to-end encrypted. Participant sets are immutable and canonical: 41010
opens/reuses a 2–9-person channel, 41011 opens/reuses a different expanded
set, and 41012 updates only the current viewer's hidden state. `bzz` does not
depend on legacy kind 41001 because the pinned relay does not emit it;
discovery comes from relay-signed 39000/39002 plus 44100/44101.

Inbox is a composed local projection rather than a new relay object. It groups
p-tagged mentions, relevant NIP-10 replies, workspace DM activity, local
drafts, and p-gated 46010–46012 cards, while reusing channel/thread/message
NIP-RS contexts. The client uses explicit bounded filters rather than treating
Buzz's partially wired `feed_types` extension as authoritative.

NIP-17 kind 1059 remains unsupported by the UI and is never indexed by local
FTS5. Remote NIP-50 results are signature-verified and checked against active
community/channel/viewer access after delivery. See
[`inbox-dms-search.md`](inbox-dms-search.md).
