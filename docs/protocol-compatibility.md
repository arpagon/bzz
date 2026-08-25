# Protocol compatibility

The v0.11.2 compatibility baseline remains `block/buzz` revision
`9f55bf67456be10ff7c8238bf0d9e12e582848f6` and uses revision-pinned
`buzz-core`/`buzz-sdk` dependencies. Existing v0.10.0 artifacts remain pinned to
`ede26863345a518ec46edd6d7692e0281883491b`.

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
  channel, verified against the pinned relay and never stored as messages; and
- exact structured kind `9` `p`-tag invocation through the existing
  acknowledgement-aware human outbox.

Thread badges are reconstructed on restart from the existing verified local
reply archive for at most the 500 retained timeline roots. A selected-channel
kind `39005` may refine live count/activity presentation only after exact
signature, pinned-signer, channel, root-coordinate, and bounded-payload
validation. It never enters SQLite, Inbox, search, unread/read state,
diagnostics, or telemetry; kind `39006` and the full Buzz channel-window model
remain unsupported.

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

Managed-agent support in this baseline is relay-only interoperability. bzz does
not consume kind `30174` memory, kind `24200` observer/control frames, kind
`30179` private managed state, or kind `44200` usage metrics. It does not own an
agent key, spawn ACP or model processes, publish autonomously, or claim control
of a remote runtime. See
[`adr-v0.11-remote-managed-agent-interoperability.md`](adr-v0.11-remote-managed-agent-interoperability.md).
