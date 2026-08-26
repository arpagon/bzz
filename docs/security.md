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

## Interaction boundary

The v0.4 interaction layer is a clean-room bzz implementation: it contains no
Concord code, assets, test fixtures, strings, or dependency. Key and mouse
input resolves into bounded typed actions, then presentation reducers emit
named effects. Those reducers, generated help, TestBackend functional harness,
and hit maps have no signer, relay, HTTP client, media uploader, process runner,
shell, or secret-bearing configuration. `App` is the only adapter that may
validate and execute an effect against the active community and identity.

Opening Inbox detail is local presentation only and never acknowledges a
conversation. Context, reply, read, and bulk-read operations independently
revalidate active community, identity, membership, DM visibility, and bounded
local context. In particular, Inbox context is a derived local view over
`events`, not a second message authority. The Herdr acceptance runner accepts
only release-binary paths, isolated non-secret directories, logical key events,
and sanitized visible labels; it deliberately refuses credential-bearing
fixture automation.

A local copy action sanitizes selected source Markdown, limits it to 64 KiB,
and base64-encodes it before emitting OSC 52. It is never automatic, never
prints copied text, never invokes a shell clipboard helper, and can be disabled
with `ui.clipboard = "disabled"`. Native clipboard *reads* are a separate
composer-only capability: `Ctrl-v` reads once only after explicit user input
when `media.clipboard_import = "explicit"`; bzz never polls, watches, logs,
indexes, syncs, or persists clipboard bytes, source paths, URIs, or native
formats. File lists, bitmaps, and text have a fixed precedence and bounds;
files and bitmaps must pass the ordinary private staging/sanitization pipeline
before upload. `"off"` disables native reads without disabling OSC-52 writes.

The separate `Ctrl-o` file chooser is also explicit-only. Linux talks directly
to XDG Desktop Portal without a command fallback; Windows/macOS use native open
dialogs. Up to eight transient local selections are exact-composer scoped and
pass the same regular/non-symlink open-and-revalidate staging boundary. bzz
never logs, displays, or persists their source paths. Cancel, unavailable,
stale-target, and over-capacity outcomes upload nothing and never retry.
`Alt-o` retains the manual path fallback under the same staging rules.
Logical multi-message selection is event-ID
based presentation state: it cannot advance reads, subscribe, fetch, sign, or
publish. The textual author marker derives only from an already-visible public
key. When `ui.profile_avatars = "trusted"`, the kind-0 `picture` field is the
single exception: it is fetched only while unlocked on a graphics-capable
terminal. External URLs use the bounded credential-free profile-avatar client.
A canonical image path at the active community relay may use a separately
bounded, same-origin Blossom media-read authorization. Set it to `"off"` to
keep every profile URL inert.

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

## Verified remote managed agents

A v0.11 remote agent is never trusted from its name, avatar, declaration,
reply, `p` tag, or self-asserted type. Outside DMs, candidate identity begins
with a current relay-signed kind 39002 membership carrying the exact `bot` role.
Buzz represents DM participants with operational role `member`; in that one
bounded case bzz considers only an exact participant in a current 2–9-person DM
and permits invocation only by the cryptographically verified owner. bzz then
verifies the agent's kind 0 signature, every present or required kind 10100
signature, the kind 0 NIP-OA owner
attestation, and any kind 30177 policy signature and `d` coordinate. Conflicting
owners, malformed records, stale cache, membership removal, a wrong policy
signer, and a wrong coordinate fail closed. Exact bot membership may establish the
agent class for an older identity with no kind 10100; a DM-only `member` may
not use that compatibility path. Missing public policy permits only the exact
NIP-OA owner and remains unknown for every other identity.

Directory state is keyed by community and agent pubkey. A valid record from one
relay cannot authorize another. Public `owner-only`, `allowlist`, and `anyone`
policy is evaluated for the active human identity; DMs and unknown channel
kinds remain owner-only. “Eligible” is advisory: it does not prove that a remote
runtime is online, safe, or willing to answer.

Selecting a verified eligible agent inserts visible composer text and a
structured exact-pubkey mention. It never sends. Before the existing human key
signs, bzz refreshes and revalidates the community, destination authority
(exact channel bot role or exact owner-controlled DM participation), ownership,
and policy. Failure preserves the draft; successful events use the
same acknowledgement-aware human outbox. A remote response has its own agent
signature.

bzz stores no agent private key and has no ACP, model, tool, memory, observer,
provider, environment, process, autonomous outbox, or runtime-control surface.
Remote profile/policy fields cannot become commands, arguments, paths, or
environment values. A genuine remote agent can still be malicious or execute
operator-controlled tools after receiving a message; users must not infer local
trust from cryptographic identity.

Relay control kind 40099 is accepted only from the pinned relay signer. A
bounded allowlist parser produces muted system rows; malformed, oversized, and
unknown payloads become a content-free unsupported row. Raw control JSON and
the relay pubkey never become an authored message, copy payload, unread count,
search document, or agent-readiness signal.

Agent-directory diagnostics contain only counts, durations, and closed outcome
enums. They exclude names, pubkeys, owners, channels, events, tags, policies,
capabilities, and content, and remain local-only rather than entering the OTel
allowlist. See
[`adr-v0.11-remote-managed-agent-interoperability.md`](adr-v0.11-remote-managed-agent-interoperability.md).

## Media

Message media is fetched only from a complete `imeta` descriptor bound to the
active community's exact relay HTTP(S) origin and canonical content-addressed
path. Arbitrary Markdown URLs never trigger a request. The enabled profile
avatar path has two non-interchangeable branches:

- External kind-0 pictures require public HTTPS domain hosts on port 443. They
  reject credentials, fragments, private/local destinations, and unsafe
  redirects; every accepted hop is DNS-pinned. This client disables proxies and
  sends no signer, cookie, user-agent, relay authorization, or community data.
- A same-origin picture may receive authorization only if it exactly matches
  `/media/<64-lowercase-hex>.<jpg|jpeg|png|gif|webp>` at the active community
  origin. Before a request, bzz validates the full URL and binds the media hash;
  it then signs a short-lived kind `24242` Blossom `t=get` event. The media
  client disables proxies and refuses redirects, and the header is constructed
  only after that same-origin validation, so it cannot reach another origin or
  an ambient proxy. The downloaded bytes must match both image MIME/magic and
  the address hash.

The external branch is capped at 2 MiB; the authenticated relay branch is
capped at 10 MiB. Both use the same owner-only avatar cache, isolated by
community and identity with SHA-256 profile/URL digest filenames, 256 files,
and 16 MiB per scope. Headers/events, source paths, full hashes, and content
are not logged.

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

## Diagnostics and optional telemetry

Local diagnostics are a separate, typed privacy boundary rather than a generic
`tracing` log. The bounded non-blocking writer accepts only reviewed connection,
AUTH phase, heartbeat, reconnect/backoff, relay acknowledgement, receiver-lag,
committed outbox-transition fields, and fixed managed-agent typing-subscription
closure classes. It cannot accept message/event content,
tags, drafts, reactions, profiles, prompts/results, clipboard data, attachment
metadata, media URLs, participant/community/channel identifiers, source paths,
configuration, environment values, credentials, auth challenges/events, raw
relay notices, response bodies, or Rust error strings. Internal errors are
reduced to a closed class before persistence. Complete event IDs appear only
for locally authored outbox operations so an owner can correlate relay receipt.

The local journal is enabled by default, owner-only, profile-isolated, and hard
bounded to three 2 MiB files. Its dedicated thread uses a bounded `try_send`
queue: saturation, serialization failure, an unwritable disk, or shutdown can
only lose diagnostics and cannot delay input, relay ACKs, SQLite, terminal
restoration, or publication. Support reports are create-new JSON files, never
automatically uploaded, and contain a redaction manifest. Metadata-only outbox
inspection never selects or deserializes `event_json`.

Remote telemetry is an independent, explicit enrollment. A fresh or upgraded
installation creates no exporter and sends zero telemetry requests. When an
owner configures an exact HTTPS `/v1/logs` endpoint and enables export, only a
strict subset of the typed records is encoded as OTLP protobuf logs. The client
disables proxies and redirects, sends no trace/span IDs, never tails or
backfills journals, and owns no signer, relay, database, media, or UI state. Its
queue (256 records/512 KiB), batch (64 records/128 KiB), retries, five-minute
record age, request timeout, and one-second shutdown are hard bounded.
Authentication failure stops export for the run without affecting local bzz.

Persistent telemetry tokens use the dedicated release credential service
`dev.arpagon.bzz.telemetry` (debug:
`dev.arpagon.bzz.debug.telemetry`), separate from Nostr identities, and are
bound to a SHA-256 digest of the canonical endpoint. Tokens are never command
arguments, config/SQLite/journal/report values, output, or redirectable
headers. `BZZ_OTEL_TOKEN` is supported only as a zeroized in-process ephemeral
source. `telemetry forget --yes` removes enrollment without changing local
identity, conversation, outbox, media, or diagnostics data.

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
through the revision-pinned `nostr` dependency and `RUSTSEC-2026-0253` (panic
safety in `lru`) through the pinned `ratatui` stack. They are transitive and
not directly selected by bzz. Re-evaluate both on every Buzz or Ratatui
compatibility update; do not change a pin solely to silence an advisory.
