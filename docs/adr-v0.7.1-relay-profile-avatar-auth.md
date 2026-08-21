# ADR: v0.7.1 relay-hosted profile-avatar authorization

**Status:** Accepted  
**Date:** 2026-08-21

## Context

v0.7.0 correctly kept arbitrary kind-0 `picture` URLs out of the authenticated
media path, but treated a picture stored at the active Buzz relay exactly like
an external URL. Communities that require a scoped Blob/Buzz media read
therefore return `401`, even though the authenticated client can read the same
content-addressed image. This is an observable interoperability regression.

## Decision

Keep the credential-free avatar client as the *only* handler for external
pictures. Add a second handler only for a URL that, before any signing:

- has the exact active community origin (scheme, host, and effective port);
- has no credentials, query, or fragment;
- has canonical path `/media/<64-lowercase-hex>.<extension>`; and
- uses one of `jpg`, `jpeg`, `png`, `gif`, or `webp`.

That handler mints the existing short-lived, hash-scoped Blossom/Buzz media
read event (`kind 24242`, `t=get`, `x=<sha256>`, expiry, and active relay
authority) and sends it only to the already validated URL. It refuses redirects
and verifies the streamed 10 MiB-or-less response against the path hash as well
as its response MIME and image magic.

The request occurs only while the matching identity is unlocked and an active
community runtime exists. Locked/cache-only sessions, `profile_avatars =
"off"`, and non-graphics terminals make no avatar request. The avatar cache
remains owner-only and partitioned by community and identity.

## Consequences

- Relay-hosted profile photographs work on communities that authorize media
  reads. This branch permits up to 10 MiB; third-party profile hosts retain the
  more conservative 2 MiB cap and receive no credentials, authorization,
  cookie, proxy setting, user-agent, or community data.
- A signer may now be invoked for a *strictly same-origin, hash-addressed relay
  image*; it is never invoked merely because a profile supplied an arbitrary
  URL.
- Same-origin but noncanonical paths, unsupported extensions, URL decorations,
  redirects, and external URLs cannot receive the authorization.
- v0.7.0's public-URL behavior and deterministic text marker are retained.

This is an independently authored behavior correction informed by protocol
interoperability, not a reuse of Desktop source, assets, configuration grammar,
or tests.
