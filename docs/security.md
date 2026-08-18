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
replaced before rendering. Ordinary URLs remain inert. HTTP redirects are
disabled so NIP-98 and Blossom authorization cannot cross origins.

## Local Codex drafts

A configured local assistant is not a Nostr identity: it has no signer, relay
connection, membership, publishing path, media uploader, or remote trigger.
The user explicitly invokes it for the selected cached message. Its process is
started without a shell from an absolute local executable, receives context on
stdin rather than arguments, clears inherited environment values (including
`OPENAI_API_KEY`), uses fixed `codex exec` JSON/ephemeral/read-only flags, and
runs in either an empty owner-only scratch directory or an explicitly selected
canonical read-only workspace. Codex authentication and model-network egress
remain external to bzz and the user's responsibility.

At most one run is allowed. Prompts, process output, stderr, thread IDs, and
unapproved drafts are neither logged nor written to configuration or SQLite.
Stdout is bounded and only a completed `agent_message` JSONL item is accepted;
terminal controls in its draft are replaced before review. Timeout, cancel,
lock, community switch, and shutdown discard the result and terminate the
child. A completed draft appears only for human review; accepting it inserts
text into the ordinary composer, whose separate human send action is still the
sole publishing path.

## Inbox, workspace DMs, and search

Buzz workspace DMs use private hidden NIP-29 channels. Relay membership blocks
other community members from reading the channel, but the relay processes and
stores ordinary plaintext kind `9`/`40002` messages. Workspace DMs are not
end-to-end encrypted. NIP-17/NIP-04/NIP-44 DM transport is not implemented and
must not be inferred from the “private” label.

DM command events are signed once, stored in the acknowledged outbox, and
strictly bounded. A channel returned by the relay is displayed only after its
UUID and exact participant set match relay-signed 39000/39002 discovery. The
metadata `hidden` tag only classifies a DM. Viewer hiding comes exclusively
from newest-wins kind 30622 carrying `p=self` and `d=self`, signed by the pinned
NIP-11 relay key. Another viewer's snapshot is rejected even when fetched by
ID. Locked mode cannot open, add to, hide, discover, or refresh a DM.

Inbox does not duplicate message bodies. It derives rows from verified events,
read contexts, drafts, memberships, and local overrides. Mark-unread never
lowers a shared read marker. Needs-action cards are inert/read-only and cannot
execute approval or workflow mutations.

Local FTS5 indexes only sanitized kind 9/40002 searchable text in the active
community partition. Generated attachment Markdown is removed. Hidden DMs,
deleted events, rejected outbox rows, gift wraps, encrypted/user-private kinds,
media bytes/metadata, paths, auth events, and secrets are excluded. Remote
NIP-50 search is same-origin NIP-98 authenticated with redirects disabled.
Every hit is signature-, community-, membership-, viewer-, deletion-, and
kind-checked again before display. Search is never an authorization boundary.

Queries, participant lists, message bodies, auth material, and full identifiers
are not logged. Inputs, pages, response bytes, results, context hydration,
actor queues, participant sets, and Inbox windows are bounded. Locked/offline
search is local-only. See [`inbox-dms-search.md`](inbox-dms-search.md).

## Media

Message media is fetched only from a complete `imeta` descriptor bound to the
active community's exact relay HTTP(S) origin and canonical content-addressed
path. Arbitrary Markdown and profile-picture URLs never trigger a request.
Blossom read/upload authorization uses short-lived signed kind `24242` events;
headers/events, source paths, full hashes, and content are not logged.

Downloads are streamed into owner-only create-new temporary files. Declared
size, hard transfer limit, response MIME, sniffed image type, exact byte count,
and SHA-256 are checked before atomic cache publication or decode. Image
dimensions and decoded allocations are independently bounded. Generic files
are never automatically downloaded, interpreted, executed, or passed to a
shell. Explicit saves refuse overwrite.

Media and staging cache bytes are plaintext and partitioned by community.
Locked recovery can render only already-verified cache entries and performs no
new authenticated media I/O. `bzz media clear` removes logical cache files but,
like SQLite purge, cannot promise forensic erasure on SSD/copy-on-write
storage. See [`media.md`](media.md).

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
