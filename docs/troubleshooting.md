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

## Upload rejected

The composer accepts regular non-symlink paths up to 50 MiB for images,
100 MiB for generic files, and 500 MiB for MP4 video. SVG, executable, and
active-content types are blocked. Images above 25 megapixels fail before
upload. Animated PNG/WebP carrying ICC or EXIF data that cannot be removed
without changing appearance fails closed. The relay remains authoritative and
may enforce stricter limits.

## Mouse prevents terminal selection or looks wrong

Set `ui.mouse = "off"` in `config.toml` to preserve terminal text selection;
this prevents bzz from emitting mouse-capture sequences. `"auto"` is the
default and enables capture only in an interactive non-`dumb` terminal, while
`"on"` is an explicit override. Button 2/3, drag selection, horizontal scroll,
and unknown mouse events intentionally do nothing.

## Local Codex assistant is unavailable

The assistant is optional and never affects normal Buzz operation. Check the
local executable and its required read-only flags without starting a model run:

```sh
bzz agent doctor
```

Install/authenticate Codex separately, then add a non-secret profile with
`bzz agent add --label <label>`. bzz accepts only a capability-compatible local
binary; it does not download, log in to, or receive a Codex credential. A
configured workdir must be a canonical existing directory and remains
read-only. `:agent` requires an unlocked identity and a selected cached message;
its result is a review draft, not a published Buzz message.

## Broken terminal after a crash

bzz installs a restoration panic hook, including mouse-capture restoration. If
the process is force-killed, run `reset` or `stty sane`.

## Development relay

Use `ws://localhost:3030` only with the explicit insecure-localhost flag. The
integration wrapper expects `BZZ_BUZZ_SOURCE` at the pinned Buzz checkout.
