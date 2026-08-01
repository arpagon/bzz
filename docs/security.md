# Security

## Identity storage

Private keys live in the operating-system credential service by default:
macOS Keychain, Windows Credential Manager, or Linux Secret Service. Each
identity has an independent entry under the release service
`dev.arpagon.bzz`; debug builds use `dev.arpagon.bzz.debug` and separate
`bzz-dev` platform directories. A successful write is read back from the OS
backend and byte-compared before bzz records the identity in configuration.
An unsuccessful verification is rolled back when possible.

`config.toml` stores only the UUID, label, public key, backend, and opaque key
reference. SQLite stores public events and cached message content. Neither
contains the private key. bzz never accepts an nsec in command arguments or an
ordinary environment variable; imports use a no-echo controlling-terminal
prompt.

When no credential service is available, users may explicitly select a
versioned Argon2id/XChaCha20-Poly1305 file. Argon2id uses at least 64 MiB and
three iterations; the authenticated envelope and its directory are owner-only
where supported. The passphrase is not stored. A headless process may supply a
passphrase only through an inherited descriptor selected by
`BZZ_PASSPHRASE_FD`.

bzz does not silently rotate identities. A missing, corrupt, or temporarily
unavailable credential starts the TUI in a distinct cache-only state. Signing,
relay connection, compose, reactions, and deletion remain unavailable until
the same configured pubkey is restored or the OS keychain is unlocked.
`identity restore` and `identity restore-backup` reject a key whose derived
pubkey differs from the configured identity.

## Portable backups

`bzz identity backup` creates a standard password-encrypted NIP-49
`ncryptsec1…` file. New backups use scrypt `log_n=18`, reject passphrases under
12 characters, decrypt-verify against the live pubkey before being returned,
refuse to overwrite an existing destination, and reread the owner-only file
after the atomic write. Decryption caps the advertised KDF cost before doing
expensive work.

A backup is still sensitive: anyone with both file and password controls the
identity. Keep those separately, never attach either to issue reports, and
verify recovery before deleting another copy. Raw `nsec` display/export is not
part of the bzz CLI.

## Runtime and cache

The unlocked key is owned by one bounded signer actor. Protocol, UI, storage,
and transport callers request signatures or NIP-44 operations without owning
the secret. `:lock` disconnects and drops the active signer key. The unlocked
process cannot defend against root, an attached debugger, or malware running as
the same user.

The SQLite cache contains message content. bzz creates it and its directories
with user-only permissions where supported. Use full-disk encryption and
`bzz cache purge` before relinquishing a device. Secure forensic deletion is
not guaranteed on SSD/copy-on-write filesystems.

Relay content is signature-verified and terminal controls/bidi overrides are
replaced before rendering. URLs are inert until an explicit future open
operation. HTTP redirects are disabled so NIP-98 signatures cannot cross
origins.

## Theme files

Themes are presentation-only local state and never cross the relay. The parser
accepts a closed inventory of semantic groups, boolean modifiers, known border
shapes, ANSI names, and six-digit RGB. It rejects raw terminal escapes, URLs,
includes, scripts, and external paths, and limits `theme.toml` to 256 KiB.
Invalid leaves degrade independently with warnings; invalid TOML falls back to
the selected compiled theme so appearance customization cannot block cache-only
recovery. A theme cannot alter layout geometry, protocol behavior, SQLite, or
signing state.

## Dependency advisory

`cargo audit` currently reports `RUSTSEC-2024-0384` (`instant` is unmaintained)
as a warning through the revision-pinned `nostr` dependency. It is not a known
vulnerability and is not directly selected by bzz; re-evaluate it on every
Buzz compatibility-SHA update and remove the transitive crate when upstream
permits.
