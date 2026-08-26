# Troubleshooting

## Authentication closes immediately

Buzz allows roughly five seconds to answer its NIP-42 challenge. Unlock the
identity before connecting. Check that the system clock is within 60 seconds.
`banned`, `restricted`, and `not a member` are terminal access errors rather
than reconnect conditions.

## Identity locked

The OS keychain contains the identity but is unavailable to this process. bzz
opens cached data without connecting or signing. Unlock the login keychain or
Linux Secret Service and restart bzz; do not create a replacement identity.

Debug and release builds intentionally use different paths and credential
services. Use `bzz paths` and make sure you are running the same build profile
that created the identity.

## Identity missing or corrupt

Verify first:

```sh
bzz identity verify <identity-id>
```

Restore the same configured identity from a NIP-49 backup:

```sh
bzz identity restore-backup <identity-id> --input identity.ncryptsec
```

The command rejects backups for a different pubkey. A raw nsec can instead be
entered without echo through `bzz identity restore <identity-id>`.

## Writing dock is read-only

The visible dock is deliberately disabled when no joined channel is selected or
the configured identity is missing, locked, offline without an identity, or
otherwise cannot publish. It remains a local presentation surface: clicking it
or pressing `i` must not create a replacement identity, refresh a relay, or
send a message. Select a joined channel, then restore/unlock the same configured
identity and restart. Any existing ordinary draft remains local; the dock does
not use a second draft store.

## No credential service

Create/import with the encrypted-file backend. On Linux, a graphical Secret
Service may be unavailable in SSH sessions; bzz does not silently write a
plaintext key. Its encrypted vault requires the passphrase on each launch.

## Relay signing key changed

bzz pins the NIP-11 `self` signing key used for NIP-29 projections. It refuses
to trust a different key silently. Confirm the change with the community
operator, then remove and re-add that community to establish a new pin.

## Cached history but no live messages

The status line distinguishes offline, authenticating, backfilling, and
access-revoked states. Use `:reconnect`, then `:resync` if an old-timestamp
event is missing.

## Remote agent is missing, stale, or cannot be mentioned

Refresh the active community's public directory:

```sh
bzz agents refresh --community <community-uuid>
bzz agents list --community <community-uuid>
bzz agents show <agent-pubkey> --community <community-uuid>
```

A normal Agents-directory entry requires current relay-signed bot membership,
an agent-signed kind 0 profile with one valid NIP-OA owner, an agent-signed kind
10100 declaration, and a matching owner-signed kind 30177 policy when policy is
published. Missing, malformed, conflicting, removed, cross-community, or stale
records fail closed. A display-name match alone is intentionally insufficient.

If the status bar says `agent typing unavailable for this channel`, the main
session may still be online while the relay has closed only the dedicated kind
`20002` subscription. Reproduce the closure once, then inspect its content-free
local classification:

```sh
bzz diagnostics status
bzz diagnostics status --json
```

`typing subscription closes` reports a bounded count and the latest normalized
class such as `access_denied`, `auth_rejected`, `rate_limited`, `protocol`, or
`unknown`. The journal never retains the relay's raw `CLOSED` text, relay URL,
community/channel/thread identifiers, agent or owner keys, event IDs, names, or
message content. This evidence remains local and is not exported through OTel. A count that
increases continuously at several closures per second indicates the pre-fix
`CLOSE`/`CLOSED` acknowledgement loop rather than repeated WebSocket failures;
stop that client and upgrade before reproducing once. Current bzz treats
`CLOSED` as terminal and does not echo another `CLOSE`.

`policy unknown` means ownership is verified but no usable public invocation
policy is available. `not eligible` means the active human identity does not
satisfy `owner-only`/`allowlist`, or the target is a DM where broader modes are
hardened to owner-only. `online`-looking public metadata is not runtime
readiness; bzz does not control the remote process.

If send-time refresh or validation fails, the exact draft remains in the
composer and nothing is automatically retried. Do not edit SQLite to add an
agent badge or policy. Ask the owner/community operator to repair public records
or membership. There is no local ACP/process log, model setup, start, stop, or
restart command in v0.11.

## Message remains pending or delivery is uncertain

The timeline now distinguishes `[pending]`, `[delivery unknown]`, and
`[rejected]`. Pending means no completed first publish outcome exists. Delivery
unknown means the transport outcome was ambiguous and bzz must reconcile by
event ID before any deliberate retry. Rejected is a definitive relay negative
acknowledgement; raw relay text is not rendered inline.

Inspect content-free local evidence without unlocking an identity or connecting:

```sh
bzz diagnostics status
bzz diagnostics outbox
bzz diagnostics outbox --json
```

Use `:reconnect` for an unknown item so existing outbox reconciliation first
queries the relay by event ID. Do not repeatedly republish it. To prepare owner-
reviewable support evidence, run `bzz diagnostics report --output <new-file>`,
read the JSON before sharing, and later remove only journals with `bzz
diagnostics clear --yes`.

## Telemetry is configured but not exporting

`bzz telemetry status` is local and makes no network request. Confirm it says
enabled, shows the expected sanitized endpoint origin, and reports an available
credential. `401`/`403` stops export for that run; reconfigure a scoped
per-installation token or ask the operator to revoke/replace it. `429`, `5xx`,
TLS, connect, and timeout failures are bounded and never change relay state.
There is no durable remote spool or automatic upload of old journal files.

Use `bzz telemetry test` to send one content-free test record. It contains no
relay, event, outbox, community, or identity attributes. `configure`, `enable`,
and `status` do not probe the endpoint. To stop future requests while retaining
the endpoint and token, use `disable`; to remove enrollment entirely, use
`forget --yes`. A missing or locked telemetry credential never blocks local
history, diagnostics, the TUI, or relay operation.

## Invalid keymap

`bzz check` validates `keymap.toml` before the TUI enters raw mode. A malformed,
conflicting, oversized, or text-stealing binding is rejected in full; bzz does
not apply a partial map. Move the file aside, verify recovery, then reintroduce
small changes:

```sh
bzz paths # copy the printed keymap path
mv /path/from/bzz-paths/keymap.toml /path/from/bzz-paths/keymap.toml.disabled
bzz check
```

The generated `?` help shows the effective route-local map, including disabled
bindings. Do not bind ordinary printable Composer or filter text to workspace
actions; that is intentionally rejected.

## Invalid or unreadable theme

Validate both the selected built-ins and the optional override:

```sh
bzz theme check
bzz check
```

The TUI falls back to the selected compiled theme when `theme.toml` has invalid
TOML and reports a warning. Disable only the override and reset selection if
needed:

```sh
mv "$(bzz theme path)" "$(bzz theme path).disabled"
bzz theme reset
```

An invalid theme never requires deleting identities, configuration, or the
SQLite cache.

## Workspace DM is missing or still visible

Workspace DMs are discovered through relay-signed 39000/39002 state. Use
`:reconnect` and wait for directory refresh if a newly accepted DM reports that
discovery is pending. Opening the same participant set must return the same
channel. Adding a participant intentionally opens a different conversation.

`H`/`:dm hide` does not delete the conversation. The row stays visible until a
newer owner-only kind 30622 snapshot confirms it. Reopen a hidden DM by opening
the same participant set with `:dm`; group DMs require the exact same
set. A relay-key pin or owner mismatch is a security error and cannot be
bypassed by changing local SQLite.

Remember that “Private workspace DM” means relay membership-controlled, not
end-to-end encrypted. NIP-17 gift wraps do not appear in this Inbox.

## Search is local-only or misses a result

Locked/offline mode deliberately uses only cached SQLite FTS5. Restore/unlock
the configured identity and reconnect for NIP-50 completion. Remote typeahead
starts at two characters and is debounced for 300 ms. `from:` and `in:` fail
closed when they resolve to zero or multiple visible records; use a unique
cached profile/channel label or exact channel UUID. Dates are UTC and use
`YYYY-MM-DD`.

Hidden DMs, deleted/rejected events, inaccessible channels, unsupported kinds,
and attachment-only text are intentionally absent. If an accepted accessible
message is missing after migration, restart once so the versioned local index
rebuild/integrity check runs, then use `:resync` in its channel. Do not edit the
FTS tables manually.

## Inbox is empty or stale

Inbox is scoped to the active community and combines mentions, relevant
threads, visible DMs, read-only needs-action cards, and drafts. Online refresh
runs every 30 seconds and live events are projected immediately. `f` cycles
filters; verify that `Unread` or another narrow filter is not selected. In
locked mode, Inbox is cache-only. `m` advances read state; `U` toggles only the
local row override and never moves NIP-RS backward.

## Attachment card but no inline image

Inspect configured media behavior:

```sh
bzz media status
```

`protocol = "off"` always renders text cards. `autoload = "preview"` or `off`
requires `p` before downloading. Images above `auto_download_bytes`, closed
spoilers, generic files, videos, malformed descriptors, and external origins
are intentionally not auto-fetched. A locked client can use only a previously
verified cache entry.

If automatic detection chose half blocks, `bzz` had no conservative hint for a
supported graphics protocol. Kitty needs Unicode-placeholder support; Sixel
must be enabled by the terminal; tmux must permit passthrough. An explicit
`kitty`, `sixel`, or `iterm2` override is user-controlled and should be removed
if it corrupts output.

## Media access denied or integrity failure

A `401`/`403` indicates that the relay requires Blossom read authorization and
the current identity could not prove membership. Restore/unlock the configured
identity and verify community membership. Hash, size, MIME, redirect, or decode
failures are not bypassable; retry with `p`, then `r`, or ask the sender to
upload the file again.

Clear only media bytes without deleting messages:

```sh
bzz media clear --community <community-uuid> --yes
```

## Textual marker but no profile photograph

A profile photograph is optional. It is shown only with
`ui.profile_avatars = "trusted"`, while the active identity is unlocked, and
when the selected terminal-media protocol is Kitty, Sixel, or iTerm2. Restart
or run `:media reload` after changing terminal capability settings. Halfblocks
and `protocol = "off"` intentionally keep the textual marker and make no
avatar request.

An external profile URL must be public HTTPS on port 443 with a supported
JPEG, PNG, GIF, or WebP response. Loopback/private hosts, IP literals,
credentials, fragments, unsafe redirects, oversized responses, bad MIME/magic,
and failed decodes remain markers only.

A profile URL at the active community relay may instead be an authenticated
canonical media image: `/media/<64-lowercase-hex>.<jpg|jpeg|png|gif|webp>` at
that exact origin. bzz signs this narrow same-origin read only while unlocked;
a `401` here usually means the identity no longer has community access. Other
relay paths, external hosts, redirects, and non-image extensions never receive
the authorization. Set `ui.profile_avatars = "off"` to prevent all
profile-avatar requests. `bzz media clear --community <community-uuid> --yes`
also removes that community's private avatar files.

## Upload rejected

The composer accepts regular non-symlink paths up to 50 MiB for images,
100 MiB for generic files, and 500 MiB for MP4 video. SVG, executable, and
active-content types are blocked. Images above 25 megapixels fail before
upload. Animated PNG/WebP carrying ICC or EXIF data that cannot be removed
without changing appearance fails closed. The relay remains authoritative and
may enforce stricter limits.

## Ctrl-v cannot import a copied file or image

bzz reads the native clipboard only after `Ctrl-v` in an open, writable
composer. Verify `[media].clipboard_import` is `"explicit"`; `"off"` disables
native reads independently of `ui.clipboard`, which governs OSC-52 writes.
Remote, sandboxed, and some Wayland desktop sessions may not expose a supported
clipboard backend. Use `Ctrl-o` for the independent OS file chooser, `Alt-o`
to enter a local regular-file path, or normal terminal bracketed paste for
text. bzz never invokes a shell helper.

A native file list is preferred over an image or text representation and is
limited to eight entries. Copied images are limited to the same 25-megapixel,
16,384-axis, and 50 MiB image limits as local uploads. Inspect the composer
queue for `processing`, `queued`, `ready`, or `failed`; use `Ctrl-r` for a
failed staged upload, `Delete` to remove the newest item, and `Ctrl-c` then
`y` to discard the whole draft. bzz never displays the source path or clipboard
contents in a status error.

## Ctrl-o does not open the OS file chooser

On Linux, `Ctrl-o` requires an available XDG Desktop Portal file-chooser
backend on the desktop session bus. Windows and macOS use their native open
dialogs. bzz does not fall back to zenity or another command. If the content-free
unavailable status appears, use `Alt-o` to enter a regular non-symlink local
path. Cancelling a chooser changes no draft and triggers no retry or upload.

## Copying a message does not reach the clipboard

`y` copies only the selected timeline/context message or the local range started
with `v`; bzz never copies text automatically. It sanitizes source Markdown,
limits one copy to 64 KiB, and emits OSC 52 only when `ui.clipboard = "osc52"`.
Set that value if an older config disabled it. Some terminal emulators ask for
permission or disable OSC 52; check their clipboard/privacy setting. bzz does
not invoke shell clipboard helpers or print copied content.

For an arbitrary character range, use the terminal's own selection: set
`ui.mouse = "off"`, then use your terminal emulator's normal drag/copy gesture.
Logical `v` selection is whole messages by design and never changes read state.

## Mouse prevents terminal selection or looks wrong

Set `ui.mouse = "off"` in `config.toml` to preserve terminal text selection;
this prevents bzz from emitting mouse-capture sequences. `"auto"` is the
default and enables capture only in an interactive non-`dumb` terminal, while
`"on"` is an explicit override. Button 2/3, drag selection, horizontal scroll,
and unknown mouse events intentionally do nothing.

## Broken terminal after a crash

bzz installs a restoration panic hook, including mouse-capture restoration. If
the process is force-killed, run `reset` or `stty sane`.

## Development relay

Use `ws://localhost:3030` only with the explicit insecure-localhost flag. The
integration wrapper expects `BZZ_BUZZ_SOURCE` at the pinned Buzz checkout.
