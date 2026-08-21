# Images and attachments

`bzz` implements Buzz message attachments through NIP-92 `imeta` tags and the
Buzz Blossom endpoints pinned at revision
`ede26863345a518ec46edd6d7692e0281883491b`.

## Receiving media

Message-media fetching requires a complete, valid `imeta` descriptor. Its URL
must use the active community's exact HTTP(S) origin and canonical
`/media/<sha256>.<extension>` path. Redirects, external origins, arbitrary
Markdown images, URL credentials, query strings, and fragments are rejected.

Profile photographs are a separate, opt-out presentation feature described in
[configuration](configuration.md). An external kind-0 `picture` uses a
credential-free HTTPS public-domain client: it disables ambient proxies and
automatic redirects, checks every manual redirect target, and pins public DNS
answers for each request.

A `picture` that is instead an exact active-community
`/media/<sha256>.<jpg|jpeg|png|gif|webp>` URL takes a distinct relay-media
branch. bzz validates its scheme, origin, canonical path, lowercase hash, and
image extension *before* minting one short-lived, hash-scoped Blossom read
authorization. Redirects remain refused and the authorization cannot be sent
to another origin. Locked/cache-only sessions never take this branch. The external branch accepts at most 2 MiB; the authenticated relay branch
accepts at most 10 MiB. Both require JPEG, PNG, GIF, or WebP response MIME to
agree with magic bytes; the relay branch additionally verifies its bytes
against the address hash. The existing image decoder then enforces its
pixel/axis bounds. A ready avatar is a measured graphics row in Kitty, Sixel,
or iTerm2; it remains a textual marker in all other cases.

Image bodies are bounded and SHA-256/size/MIME checked before decode. JPEG,
PNG, GIF, and WebP render inline. Animated formats intentionally show their
first frame. Video and generic files remain text cards. When a video carries a
separately content-addressed `image` poster, an explicit preview fetches it with
hash-scoped authorization, bounds and verifies its response MIME/size/hash, and
renders it as a static image. Videos are never played.

Select a message and choose **preview media** from `Space a` to open its
attachment preview:

- `[` / `]`: previous/next attachment;
- `Enter`: reveal a descriptor-backed spoiler;
- `r`: retry;
- `s`: save verified bytes to a new path;
- `Esc`: close.

Saving uses create-new semantics and refuses overwrite. Files are never opened,
executed, or passed to a shell.

## Sending media

While composing, press `Ctrl-a`, enter a local path, and press `Enter`. The path
must identify a regular non-symlink file. `bzz` never persists the source path.
It copies the bytes into an owner-only content-addressed staging directory and
uploads them to the active community.

Static JPEG, PNG, and WebP images are decoded under pixel/allocation limits,
have orientation applied, and are re-encoded without private metadata. GIF,
animated PNG, and animated WebP use bounded structural metadata removal;
formats carrying color/orientation metadata that cannot be removed without
changing appearance fail closed. SVG and executable/active-content types are
rejected.

The upload uses `PUT /upload`, `X-SHA-256`, the exact staged MIME/body, and a
short-lived kind `24242` `t=upload` authorization event. The returned descriptor
must match the local hash, size, MIME, origin, and path before it can be added
to a draft. Message creation then uses the ordinary acknowledged outbox.

Up to eight outbound attachments are accepted. Uploaded blobs may remain
unreferenced if a draft is abandoned or its message is rejected; Buzz exposes
no ownership-scoped deletion contract.

## Terminal protocols

The default `auto` mode reads cell metrics through the operating-system terminal
window-size API and uses conservative terminal-family hints. It deliberately
does not launch a detached stdin capability reader: on terminals that never
answer a probe, such a reader can consume a later user key after timeout. The
renderer uses:

1. Kitty Unicode placeholders;
2. Sixel;
3. iTerm2 inline images;
4. Unicode half blocks.

`ratatui-image` provides protocol encoding and vertical slicing. Unknown
terminals safely use half blocks; explicit protocol settings are available when
environment detection is insufficient. Workers may
fetch, hash, decode, resize, and encode, but never write to the terminal; all
output remains serialized through Ratatui. tmux requires a version/configuration
that permits graphics passthrough. Unsupported or failed protocols degrade to
text attachment cards.

Configure `[media].protocol` as `auto`, `kitty`, `sixel`, `iterm2`,
`halfblocks`, or `off`. Explicit graphics modes are user overrides. `off`
retains cards, preview metadata, and explicit save/upload operations without
inline graphics. After changing terminal/tmux conditions, use `:media reload`
to rebuild the renderer without restarting; configuration-file changes apply on
restart.

## Cache and privacy

Verified message originals are stored under the private cache directory,
partitioned by community UUID. No bytes are shared between communities. Cached
message media can render in locked/offline recovery mode, but no new
authorization or network request is made. Startup removes partial/symlink
entries and reconciles stale SQLite metadata. The default disk quota is 512 MiB
with access-time eviction; staging files are not quota-evicted while referenced
by a draft. Prepared terminal images use a byte-weighted in-memory LRU and are
rejected when one entry cannot fit the configured memory budget. Entries are
charged for their post-resize terminal allocation (including protocol overhead),
not the dimensions of a source photograph, so small avatars do not evict one
another merely because their originals are high resolution.

Profile-avatar bytes use a separate owner-only root partitioned by community
and identity, with SHA-256 profile/URL digest filenames. They are not shown or
fetched while locked. Each scope is pruned to 256 files or 16 MiB; changed
picture URLs form a separate, prunable entry. `media clear` and community/cache
purge remove avatar bytes along with message-media bytes.

Media cache files are plaintext, like the local SQLite message cache. Set
`disk_cache_bytes = 0` for memory-only inline media. Use:

```sh
bzz media status
bzz media prune
bzz media clear --community <uuid> --yes
bzz media clear --all --yes
```

Cache removal cannot guarantee physical secure erasure on SSDs.

## Default bounds

| Resource | Limit |
|---|---:|
| Automatic message image transfer | 25 MiB |
| External profile-avatar transfer | 2 MiB |
| Active-relay profile-avatar transfer | 10 MiB |
| Profile-avatar disk cache | 256 files / 16 MiB per community/identity |
| Image | 50 MiB |
| GIF | 10 MiB |
| Generic file | 100 MiB |
| Video explicit save/upload | 500 MiB |
| Decoded image | 25 megapixels, 16,384 pixels per axis |
| Inline height | 12 terminal rows |
| Downloads/uploads | 4 concurrent |
| Decodes/resizes | 2 concurrent |
| Inbound descriptors rendered per message | 16 |
| Outbound attachments | 8 |

The local composer accepts images up to 50 MiB, generic files up to 100 MiB,
and MP4 video up to 500 MiB. Video playback is not implemented.
