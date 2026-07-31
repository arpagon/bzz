# Security

Private keys live in the OS credential service or a versioned
Argon2id/XChaCha20-Poly1305 file. They are not stored in TOML, SQLite, command
arguments, ordinary environment variables, logs, or UI model state. The
fallback passphrase is read without echo from the controlling terminal; a
headless process may use only an inherited descriptor selected by
`BZZ_PASSPHRASE_FD`.

The SQLite cache contains message content. bzz creates it and its directories
with user-only permissions where supported. Use full-disk encryption and
`bzz cache purge` before relinquishing a device. Secure forensic deletion is
not guaranteed on SSD/copy-on-write filesystems.

Relay content is signature-verified and terminal controls/bidi overrides are
replaced before rendering. URLs are inert until an explicit future open
operation. HTTP redirects are disabled so NIP-98 signatures cannot cross
origins.

The unlocked process cannot defend against root, an attached debugger, or
malware running as the same user. `:lock` disconnects and asks the signer to
zeroize its active key.

`cargo audit` currently reports `RUSTSEC-2024-0384` (`instant` is unmaintained)
as a warning through the revision-pinned `nostr` dependency. It is not a known
vulnerability and is not directly selected by bzz; re-evaluate it on every
Buzz compatibility-SHA update and remove the transitive crate when upstream
permits.
