# ADR: v0.7 remote profile-avatar boundary

**Status:** Amended by [ADR v0.7.1 relay-hosted profile-avatar authorization](adr-v0.7.1-relay-profile-avatar-auth.md)

## Context

A public Nostr kind-0 profile can name a photograph in its `picture` field.
That is a useful author-recognition cue in a conversation timeline. The v0.6
ADR deliberately made those URLs inert after an unsafe terminal-image overlay
experiment. Following behavior-level product research, bzz will now support
profile photographs without inheriting another client's implementation or its
network policy.

## Decision

`ui.profile_avatars = "trusted"` is the default. It permits a narrow,
unauthenticated profile-avatar path; `"off"` leaves only the deterministic
textual author marker. The setting is strict and local.

The profile-avatar client is separate from authenticated community media:

- It permits only HTTPS public domain hosts on port 443, has no URL
  credentials or fragments, disables ambient proxies and automatic redirects,
  and follows at most three redirects manually.
- Every initial and redirected host is resolved before connection. Only public
  resolved IP addresses are pinned into that request; loopback, private,
  link-local, multicast, documentation, and other non-public destinations are
  rejected.
- It carries no signer, NIP-98, relay authorization, cookie, user-agent,
  membership, or identifying header. It accepts a 2 MiB JPEG, PNG, GIF, or
  WebP response only when its declared MIME agrees with its magic bytes; the
  existing decoder then applies its dimension and decode limits.
- Profile files live in an owner-only avatar cache separate from attachment
  media. Community and identity directories, SHA-256 profile/URL digests, a
  64-item in-flight bound, and a per-scope 256-file/16-MiB pruning bound avoid
  URL filenames, cross-scope reuse, and unbounded retention.
- Fetching is disabled in cache-only/locked mode and when terminal graphics
  are unavailable. Completed work from an old terminal, community, identity,
  cache-clear, or reload generation is ignored.

A ready photograph uses an allocated, measured timeline row through the
existing terminal-image rendering path. It is never painted as a direct text
cell overlay. Textual markers remain visible during loading, failure, disabled
mode, and in non-graphics terminals.

The original product contract is
[`v0.7.0-remote-profile-avatars.md`](planning/2026-08-20/v0.7.0-remote-profile-avatars.md).
The v0.7.1 amendment preserves this external public-URL boundary while adding
a narrowly authenticated same-relay media branch.

## Consequences

- A profile image host learns that an ordinary, unauthenticated client at the
  user's network address requested its public URL. Users needing no such
  contact can set `ui.profile_avatars = "off"`.
- bzz does not send public profile URLs to the community relay or attachment
  media service, and profile images cannot affect Inbox, reads, subscriptions,
  signing, publishing, or draft/human-send boundaries.
- Graphics-terminal visual review remains required before a v0.7 release,
  particularly for scrolling, channel/identity switches, and `:media reload`.

## Research boundary

The decision follows behavior-level observation that another desktop Nostr
client displays kind-0 pictures. bzz's configuration vocabulary, request
policy, cache layout, terminal layout, source, tests, strings, and documents
are independently authored. No external source code, assets, screenshots,
configuration grammar, or derived tests were copied or adapted.
