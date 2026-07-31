# Troubleshooting

## Authentication closes immediately

Buzz allows roughly five seconds to answer its NIP-42 challenge. Unlock the
identity before connecting. Check that the system clock is within 60 seconds.
`banned`, `restricted`, and `not a member` are terminal access errors rather
than reconnect conditions.

## No credential service

Create/import with the encrypted-file backend. On Linux, a graphical Secret
Service may be unavailable in SSH sessions; bzz does not silently write a
plaintext key.

## Relay signing key changed

bzz pins the NIP-11 `self` signing key used for NIP-29 projections. It refuses
to trust a different key silently. Confirm the change with the community
operator, then remove and re-add that community to establish a new pin.

## Cached history but no live messages

The status line distinguishes offline, authenticating, backfilling, and
access-revoked states. Use `:reconnect`, then `:resync` if an old-timestamp
event is missing.

## Broken terminal after a crash

bzz installs a restoration panic hook. If the process is force-killed, run
`reset` or `stty sane`.

## Development relay

Use `ws://localhost:3030` only with the explicit insecure-localhost flag. The
integration wrapper expects `BZZ_BUZZ_SOURCE` at the pinned Buzz checkout.
