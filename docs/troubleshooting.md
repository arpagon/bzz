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

The composer accepts regular non-symlink paths up to 100 MiB. SVG, executable,
and active-content types are blocked. Images above 25 megapixels fail before
upload. Animated PNG/WebP carrying ICC or EXIF data that cannot be removed
without changing appearance fails closed. The relay remains authoritative and
may enforce stricter limits.

## Broken terminal after a crash

bzz installs a restoration panic hook. If the process is force-killed, run
`reset` or `stty sane`.

## Development relay

Use `ws://localhost:3030` only with the explicit insecure-localhost flag. The
integration wrapper expects `BZZ_BUZZ_SOURCE` at the pinned Buzz checkout.
