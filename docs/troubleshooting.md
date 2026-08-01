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

## Broken terminal after a crash

bzz installs a restoration panic hook. If the process is force-killed, run
`reset` or `stty sane`.

## Development relay

Use `ws://localhost:3030` only with the explicit insecure-localhost flag. The
integration wrapper expects `BZZ_BUZZ_SOURCE` at the pinned Buzz checkout.
