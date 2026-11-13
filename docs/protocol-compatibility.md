# Protocol compatibility

The v0.14.0 compatibility baseline is `block/buzz` revision
`bf3bdd0eaefe39289bf9620594f254c7f68897f5`, with revision-pinned `buzz-core`,
`buzz-sdk` and (feature `huddles-audio`) `buzz-audio-client`, built with Rust
1.98.1. v0.13.0 artifacts remain pinned to
`f3b127e3af5c14d2314faa2b63556c3f51ec6743`; v0.11.x–v0.12.x to
`9f55bf67456be10ff7c8238bf0d9e12e582848f6`; v0.10.0 to
`ede26863345a518ec46edd6d7692e0281883491b`. The v0.13.0 surfaces are listed in
[v0.13.0 additions](#v0130-additions) below.

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
  projection;
- relay-authored kind `39002` bot membership, agent-authored kind `0` NIP-OA
  ownership profiles and kind `10100` declarations, and owner-authored kind
  `30177` public policy for verified remote managed-agent discovery;
- relay-authored kind `40099` control events through a bounded semantic parser,
  never raw authored-message rendering;
- transient relay-authored kind `39005` thread summaries for the selected
  channel, verified against the pinned relay and never stored as messages;
- signed ephemeral kind `20002` typing indicators admitted only from a current
  verified remote agent in the exact selected channel/open thread; and
- exact structured kind `9` `p`-tag invocation through the existing
  acknowledgement-aware human outbox.

Thread badges are reconstructed on restart from the existing verified local
reply archive for at most the 500 retained timeline roots. A selected-channel
kind `39005` may refine live count/activity presentation only after exact
signature, pinned-signer, channel, root-coordinate, and bounded-payload
validation. It never enters SQLite, Inbox, search, unread/read state,
diagnostics, or telemetry. Since v0.13.0 the selected channel and threads load
through verified NIP-CW windows (kinds `39006`/`39007`); see below.

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
discovery comes from relay-signed 39000/39002 plus 44100/44101. Buzz assigns
DM participants operational role `member`; bzz therefore permits an exact
current participant to enter bounded agent verification only when NIP-OA ownership plus
either exact bot authority elsewhere in the community or signed kind 10100
validate. DM-only members require the declaration. DM invocation remains
verified-owner-only; an absent public policy never enables a non-owner. Every
non-DM destination continues to require exact relay-signed `bot` role.

Inbox is a composed local projection rather than a new relay object. It groups
p-tagged mentions, relevant NIP-10 replies, workspace DM activity, local
drafts, and p-gated 46010–46012 cards, while reusing channel/thread/message
NIP-RS contexts. The client uses explicit bounded filters rather than treating
Buzz's partially wired `feed_types` extension as authoritative.

NIP-17 kind 1059 remains unsupported by the UI and is never indexed by local
FTS5. Remote NIP-50 results are signature-verified and checked against active
community/channel/viewer access after delivery. See
[`inbox-dms-search.md`](inbox-dms-search.md).

Kind `20002` is a live-only presentation signal. bzz uses a dedicated exact
`#h` selected-channel subscription with an eight-second TTL, clears matching
state on a signed agent reply, and stores no event, history, Inbox, search,
unread/read-state, diagnostic, or telemetry record. bzz does not publish human
typing, and the row does not prove readiness, health, progress, or a guaranteed
reply.

Remote managed agents remain relay-only interoperability: bzz does not consume
kind `24200` observer/control frames, kind `30179` private managed state, or kind
`44200` usage metrics, and never spawns ACP or model processes. Since v0.13.0 it
can hold keys for **local** agents it hosts through the `buzz-sdk::broker`
contract (see below and
[`adr-v0.13-agent-broker.md`](adr-v0.13-agent-broker.md)).

## v0.13.0 additions

- **Channels (NIP-CL):** channel types and roles, announce channels (guests read
  only), unlisted channels hidden from discovery, a collapsed read-only System
  group, and live membership changes; the viewer's memberships come from a
  verified kind `39008` snapshot when the relay serves one. The profile card
  (`:profile`, `Space p`) lists a person's listed, open channels from a
  verified kind `39009` public lookup where the relay advertises
  `membership_public`; that list is display data only, never membership
  authority, and a failed or unsupported lookup is never shown as empty.
- **People search:** the relay's name-only `POST /profiles/search` (kind 0
  `name`/`display_name`, and relay-signed kind 30382 verified names where
  advertised); NIP-50 prefix search over kind 0 only when it answers 404.
- **Mentions:** a malformed mention or more than 50 is a composer error
  before signing; the draft is kept.
- **Read state (NIP-RS):** a verified atomic snapshot as the complete startup
  load.
- **Federated identity (NIP-FI, NIP-FA):** browser sign-in with an identity
  adapter, `nip-fi+jwt` evidence on the WebSocket upgrade, HTTP and media,
  re-minting before expiry, typed denials, Buzz Desktop as a credential
  provider, and relay-signed verified profiles (kind `30382`).
- **Artifacts (NIP-AR, kinds `45010`/`45011`):** one sync subscription for the
  selected channel (the set is replaced only at EOSE), head lookups including
  deletions, cross-channel assigned `buzz.task` artifacts, and envelope edits —
  rename, delete, restore and move — reconciled on `conflict`. The
  `buzz.task`/`buzz.project` content schemas are unpublished upstream, so bzz
  renders a safe fallback and never rewrites `content`; creating artifacts is
  not supported.
- **Custom emoji (NIP-30, NIP-CE):** kind `30180` entries and `30030` sets,
  inline images in graphics terminals and `:shortcode:` elsewhere, and custom
  reactions.
- **Presence and status:** kind `20001` presence (publishing is opt-in), kind
  `20003` owner presence for agents, and NIP-38 kind `30315` status with expiry.
- **Channel and thread windows (NIP-CW):** relay-signed `39006`/`39007` bounds
  verified against the exact request; paging uses only `has_more`; fallback to
  the previous loading without notice.
- **Huddles (kinds `48100`–`48106`, `24810`):** rosters replayed with the
  upstream CLI/desktop tracker, start and end with confirmation, guidelines and
  reactions, and audio through `/huddle/{id}/audio` (v2 handshake, Opus,
  muted on join). Builds without `huddles-audio` are view-only.
- **Local agent broker (`buzz-sdk::broker`):** `POST /v1/action` on loopback
  for `channel.read`, `message.post`, `message.reply`, `reaction.add`,
  `profile.set` and `storage.address` (NIP-AE kind `30174` addressing).
  `agents.create/update/delete` answer `unsupported`. Agent events and NIP-42
  AUTH carry the owner's NIP-OA tag; FI communities use NIP-FA agent grants and
  `nip-fi-agent+jwt` assertions.
- **Messages:** "send to channel" from a thread and NIP-56 reports (kind
  `1984`).
