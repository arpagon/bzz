# Protocol compatibility

This release targets `block/buzz` revision
`ede26863345a518ec46edd6d7692e0281883491b` and uses revision-pinned
`buzz-core`/`buzz-sdk` dependencies.

Implemented MVP protocol surface:

- NIP-11 relay information and host-derived community identity;
- NIP-42 kind 22242 WebSocket authentication;
- NIP-98 kind 27235 authenticated `/query` and `/events` requests;
- NIP-29 kinds 39000/39002 plus membership notifications;
- kind 0 profiles; kind 9 messages; NIP-10 threads; kind 7 reactions;
- kind 5 and 9005 tombstones;
- NIP-44-to-self kind 30078 Buzz read-state snapshots.

bzz combines a small global stream (needed for reaction/removal events that
carry `e` but no `h`) with a bounded set of channel subscriptions, then repairs
reconnect gaps with HTTP composite cursors. Signed Nostr timestamps are not
relay sequence numbers, so old-timestamp late publication requires overlap,
periodic reconciliation, or manual full sync.
