# bzz v0.7.0 release notes

## Remote profile avatars

bzz can display a public Nostr kind-0 `picture` beside its existing textual
author marker. The default `ui.profile_avatars = "trusted"` enables this only
in Kitty, Sixel, and iTerm2 terminals; `"off"`, locked/cache-only sessions,
missing pictures, invalid URLs, failed loads, and ordinary text terminals keep
the marker without making an avatar request.

The request path is intentionally independent of authenticated attachment
media. It accepts only bounded JPEG, PNG, GIF, and WebP HTTPS resources from
validated public hosts, manually checks each redirect, uses no proxy,
cookies, signer, NIP-98, relay authorization, user-agent, or community header,
and stores validated bytes under a private community/identity-isolated cache.
A picture host can still observe an ordinary unauthenticated request from the
user's network; use `ui.profile_avatars = "off"` to avoid that contact.

Ready avatar images occupy measured timeline rows rather than text overlays,
so wrapping and scrolling include their height. The behavior is specified in
[`adr-v0.7-remote-profile-avatars.md`](adr-v0.7-remote-profile-avatars.md).

## Compatibility

Profile avatars do not modify Nostr profile storage, profile hydration, relay
subscriptions, Inbox/read state, attachment authorization, drafts, signing,
publishing, identity isolation, locked mode, or the explicit human-send
boundary. Existing Buzz protocol compatibility remains pinned.
