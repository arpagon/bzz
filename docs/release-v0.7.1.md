# bzz v0.7.1 release notes

> **Published: v0.7.1 (2026-08-21).**

## Relay-hosted profile photographs

v0.7.1 fixes kind-0 profile pictures stored at the active Buzz community relay.
When `ui.profile_avatars = "trusted"`, a canonical relay image URL of the form
`/media/<sha256>.<jpg|jpeg|png|gif|webp>` now uses a short-lived, hash-scoped
Blossom media-read authorization while the identity is unlocked. This restores
profile photos on relays that correctly require authenticated media access.

The authorization is minted only after exact origin/path/hash validation; it
cannot follow redirects or go to another host. The response is streamed under
the authenticated relay-avatar 10 MiB limit and must match its content-addressed
hash, MIME, and image magic.

External profile URLs retain the v0.7.0 credential-free HTTPS path and 2 MiB
limit. Prepared-avatar memory accounting uses the resized terminal allocation,
rather than the source photograph dimensions, preventing large source images
from evicting visible small avatars and causing redraw flicker. `off`,
locked/cache-only sessions, invalid URLs, non-graphics terminals, and failed
loads retain the deterministic textual marker and make no avatar request.

See [`adr-v0.7.1-relay-profile-avatar-auth.md`](adr-v0.7.1-relay-profile-avatar-auth.md)
for the security boundary.
