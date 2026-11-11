# ADR: v0.13 federated identity (NIP-FI, NIP-FA, local provider)

**Status:** Accepted

**Date:** 2026-11-07

## Context

Buzz relays can require NIP-FI federated identity: every WebSocket upgrade and
every protected HTTP request carries
`Nostr-Federated-Identity: Bearer <compact JWS>`, minted by an issuer for the
exact Nostr key that then signs NIP-42 or NIP-98. A relay that requires it
answers missing evidence with `401` plus `WWW-Authenticate: Nostr`, answers bad
evidence with `403 evidence rejected`, and closes legacy sockets with 1008 when
a community starts requiring it (block/buzz#7224, #8991).

Clients obtain assertions in one of two ways:

- **NIP-FA:** the client runs a browser login against an enterprise adapter,
  holds the resulting adapter session, and exchanges it plus a NIP-98 proof for
  short-lived assertions.
- **Local credential provider** (`docs/local-credential-provider.md`
  upstream): a running Buzz Desktop holds the key and the session. A local
  caller asks it over loopback for the public key, signatures, NIP-44,
  NIP-OA tags and NIP-FI evidence.

Before v0.13, bzz held exactly one class of secret: the human's Nostr private
key, in the OS credential service or an Argon2id file.

## Decision

1. **Requirement comes from the relay.** bzz reads `requires_nip_fi` and
   `limitation.federated_identity` from NIP-11 on the configured origin, and
   requests evidence only when the host requires it (the same rule Buzz mobile
   adopted in block/buzz#9225).
2. **The adapter comes from configuration.** NIP-FA leaves adapter discovery
   out of scope. Each community may name an `identity_adapter` HTTPS origin.
   `http://127.0.0.1` and `http://[::1]` are accepted only for local testing.
   bzz never derives an adapter from relay data.
3. **The adapter session is a second secret class.**
   - It is stored with the identity's backend (OS credential service, or a
     separate Argon2id envelope), keyed by `(identity id, adapter origin)`.
   - It is never written to `config.toml`, SQLite, logs, diagnostics,
     telemetry or support reports.
   - It is sent only to the adapter origin, by a no-redirect HTTP client.
   - Locking bzz drops it from memory together with the signer.
4. **Assertions are memory-only.** Each assertion is cached per
   `(identity, canonical relay URL)` until shortly before `expires_at`, never
   persisted, and attached only to requests for that relay origin. Relay HTTP
   and WebSocket clients follow no redirects.
5. **Buzz Desktop provider is an identity type.** A `buzz-desktop` identity
   holds no private key in bzz.
   - Its token is a third secret class, stored like the adapter session.
   - Every signature request carries the complete unsigned event so the
     desktop can classify it.
   - Unavailable on Windows, because provider v1 has no Windows transport.
6. **Failure is typed and quiet.** NIP-FA denial codes map exactly to the
   spec's client actions. Relay denials become typed community states. Raw
   relay or adapter text never reaches the UI or diagnostics.
7. **Profile projection stays private.** bzz never publishes
   `profile_projection` fields unless the operator has opted in. In v0.13 that
   means never, because there is no opt-in signal yet.

## Security consequences

### Positive

- Communities that require enterprise identity become reachable without
  weakening their policy.
- Evidence never appears in URLs, events, tags, filters or logs, matching
  NIP-FI's transport rule.

### Residual risks

- A stolen adapter session lets its holder mint assertions for keys they
  control until the adapter binds the user to a key. It is protected exactly
  as well as the Nostr key next to it.
- A loopback callback listener is reachable by other local processes during
  login. The S256 handoff challenge makes a stolen code useless without the
  secret, which never leaves bzz.
- Assertion lifetime is operator policy. A revoked key keeps working until its
  last assertion expires, or the relay's deny entry fires.

## Rejected alternatives

- **Discover the adapter from NIP-11 or the relay URL.** No spec defines it,
  and a relay-controlled pointer would let a hostile relay redirect sessions.
- **Persist assertions.** They are short-lived, and keeping them in memory
  removes a whole class of leakage.
- **Read Buzz Desktop's files directly.** The provider protocol exists so the
  desktop can classify and approve what it signs.
