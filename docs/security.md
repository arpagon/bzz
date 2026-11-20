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
with a current relay-signed kind 39002 membership: carrying the exact `bot`
role, or, since v0.15, any role when the member's kind 0 profile carries its
owner's valid NIP-OA proof (Buzz 1.0 and bzz add an agent as a plain member).
A member without that proof is a person: it never enters the directory, and a
profile that only says `"bot": true`, or carries an authorization made for
another key, proves nothing.
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
(current membership of the channel, or exact owner-controlled DM
participation), ownership, and policy. Failure preserves the draft; successful events use the
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

## v0.13.0 trust boundaries

**Federated identity (NIP-FI / NIP-FA).** The identity adapter is a new party:
it authenticates you in the browser and mints short-lived relay assertions for
your key. bzz talks to it only at the configured origin, never follows
redirects, proves every request with a fresh NIP-98 event over the exact body,
and stores the adapter session NIP-44-sealed to your key. Sessions, assertions
and agent grants never appear in `Debug` output, errors, journals, reports or
telemetry (T8 secret scan). See
[`adr-v0.13-federated-identity.md`](adr-v0.13-federated-identity.md).

**Local agent broker.** bzz now holds keys for identities other than yours.
Each agent key is created by bzz, sealed with NIP-44 to your own key (it opens
only while you are unlocked) and never handed to the agent. Agents
authenticate to `127.0.0.1` with a 256-bit bearer credential shown once and
stored only as a hash; any local process holding a credential can act as that
agent within its policy until you rotate or delete it. Writes require the
relay-signed `bot` role in a channel whose roster you can see; the relay stays
the final authority. Nothing is served while bzz is locked, `p` in `:broker`
pauses every agent, and the activity log records action, outcome and a short
channel label only. The only automatic event signed with your key is the
opt-in kind 20003 agent presence. See
[`adr-v0.13-agent-broker.md`](adr-v0.13-agent-broker.md).

**Huddle audio.** Joining opens your microphone and speaker after a
confirmation, starts muted and offers push-to-talk. Remote Opus is decoded by
libopus, as in every Buzz client. Locking, quitting, leaving, a dropped socket
or an ended huddle releases both devices. Audio, levels, transcripts and
device names never enter logs, diagnostics or telemetry; device names are
stored only in `config.toml`. On macOS the microphone permission belongs to
your terminal app. See [`adr-v0.13-huddle-audio.md`](adr-v0.13-huddle-audio.md).

## v0.15.0 trust boundaries

**Local agents.** bzz now starts agent processes
([`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md)). A harness is not
trusted with anything of bzz's:

- **Keys.** An agent's key is unsealed only inside its signer, as for a v0.13
  hosted agent. A harness is started with an environment of `PATH`, `HOME`,
  `LANG` and `TMPDIR` and nothing else, and is told the agent's public key,
  name and settings. No agent key, owner key, passphrase or relay address is
  in its environment, arguments, messages or tool results
  (`tests/secret_scan_test.rs`).
- **What it may do.** Only the tools `send`, `read` and `react`, served by
  bzz. The message answered comes from the calling session's live turn, never
  from an argument; one final reply per message is kept in bzz's database
  before anything is published. A permission request for anything but those
  tools is refused, but for a coding harness, whose requests are answered
  by rule (below). bzz's own host may also ask for a git credential and for
  leave to start a real command (below); no other harness may.
- **`bzz agent-tools`.** A harness that is not bzz's own host reaches the
  tools through this MCP server and a loopback endpoint, with a credential
  made for one session, held in memory as a hash and revoked when the agent's
  process exits. The credential is in the harness's own hands, so a harness
  can do through it exactly what its tools allow and nothing more.
- **Who wakes it.** bzz decides before anything reaches a harness: the owner
  by default, listed people or anyone when the owner says so, and only the
  owner in a DM.
- **Processes.** Each agent runs in its own process group and is stopped as a
  group. It is handed its three standard streams and no other open
  descriptor: before a start, bzz marks everything it holds above them as
  not to be passed on, also what bzz itself inherited from the terminal it
  was started in (on Linux and macOS; on Windows a handle bzz inherited as
  inheritable is passed on). On Linux, a bzz that runs agents marks itself
  as a process whose memory other processes of the same user may not read
  or attach to (`PR_SET_DUMPABLE` 0), and so leaves no core dump.
- **Coding harnesses (Grok, Codex, Claude Code).** Each is the owner's own
  installed program with his own sign-in. It runs a real shell as the
  owner's user, and bzz cannot promise the key is out of its reach, only
  that bzz never gives it. `bzz agent show` prints each harness's
  capability record, with who enforces each rule.
  - **The guard is bzz's, not the owner's settings of the program.** In the
    mode those name, all three write anywhere without asking. bzz writes a
    sandbox profile for Grok before every start, and sets a mode on every
    session of Codex (`workspace-write`) and of Claude Code (`default`)
    before its first prompt. A session that does not take the mode is never
    prompted.
  - **Who holds the boundary.** Grok's and Codex's own sandboxes hold their
    writes; bzz does not check them. Claude Code has none: bzz's answers to
    its requests are the only boundary, and bzz says so at creation and in
    `agent show`.
  - **The rule for a request**: bzz's chat tools, allowed; a file edit, only
    when every path it names is inside the agent's working folder or a
    folder granted for writing, where the path really leads; a shell
    command, by the folders granted for commands and the owner's answer, as
    above; anything else, and anything bzz cannot read a kind, a path or a
    command from, refused. Never "always".
  - **What a sandbox does not do.** Grok and Codex read everything the
    owner can: his files, bzz's database of his messages, other programs'
    settings. Both write in the machine's temporary folders. Their commands
    run without asking. bzz tells Grok's sandbox to hide the folder of
    bzz's keys (sealed agent keys, the owner's key when it is kept in a
    file, git credentials); Codex's sandbox takes no such instruction.
  - **The owner's own settings of the program still apply to Grok and
    Codex**: his MCP servers and their tools, hooks, rules, plugins. A Grok
    agent uses them without asking. Claude Code is told to load none, and
    none of its working folder's, which the agent can write.
  - **What bzz answers is the program's question.** For a shell command the
    owner sees the command as the program reports it, and the program starts
    it. For an edit, bzz checks where the path leads at that moment: a link
    put in its place afterwards, by a command the owner allowed, is not
    seen.
  - **A model that cannot post looks for another way.** In one test a Grok
    agent whose `send` tool did not work ran fifty commands in four
    minutes, among them a search of bzz's files and of its parent's memory
    for a key or a credential. It found none it could use; the key folder
    was readable then and is hidden now. An agent anyone may wake can be
    asked to do the same on purpose.
  - **A program's own notices are not the agent's words.** Codex's adapter
    writes Codex's warnings, and the text of a provider's error, as message
    text. bzz publishes a turn's plain text when the agent sent nothing
    through its tool, so such a notice could have left bzz as the agent's
    reply, with whatever of the owner's configuration it named. The adapter
    gives the model's own text a message id and those notices none; for
    Codex, bzz keeps only text that has one.
  - **Versions and adapters.** bzz starts only versions it knows to work.
    The two adapters are pinned by lock files inside the binary, installed
    without install scripts and without their optional packages, and used
    only when whole.
  - **A harness's own messages can hold the owner's secrets.** Grok lists
    his MCP servers, with their arguments and environment, in a notification
    to its client. bzz ignores what it does not know and writes none of it,
    not to a log and not to a recorded transcript.
  - **Its log is the program's own.** `harness.log` holds what the program
    wrote to its standard error, for the owner only. bzz writes nothing of
    its own there, and cannot say what a program of the owner's writes.
- **bzz's own agent host (`pi-durable`).** A Node program inside the bzz
  binary, run with the owner's Node. bzz installs it on first use from the
  lock file inside the binary with `npm ci --ignore-scripts --omit=optional
  --omit=dev`: no package runs an install script, and npm gets a small
  environment with nothing of bzz's in it. The install is built in a folder
  of its own and renamed into place only when whole. The host process itself
  is trusted code: it reads the owner's Pi sign-in to call the model, and it
  is what keeps the agent to its own disk, its granted folders and its
  allowed hosts. The model's shell runs inside that process with an
  environment of its own, starts no program of the machine, and cannot read
  the sign-in. A granted folder is checked, then used: another program of
  the machine that swaps a link in between can win that race; the agent
  cannot.
- **Real commands (`:exec`).** A `pi-durable` agent starts a program of the
  machine only in a folder its owner granted for commands, and only after
  bzz said yes to that very command:
  - bzz says yes only for a folder its own settings grant for commands, in
    the settings it started with and in the settings file at that moment, so
    a grant taken away holds at once; only for a turn that is running; and,
    unless the owner allowed that folder without asking, only after he
    answered yes in the TUI.
  - The owner is shown the whole command as `/bin/sh` will get it. Every
    line is numbered, a long line goes on without a number, and a character
    that would not show (a terminal escape, a carriage return, a
    right-to-left override, an invisible space) is written out. A yes is
    taken only after the command's end has been on screen. The dialog opens
    only when the owner asks for it (`Ctrl-Y`), so nothing typed elsewhere
    answers it. What it cannot show: a letter that looks like another one;
    it says when a command holds characters outside ASCII.
  - Without the TUI there is nobody to ask and the answer is no. A question
    is refused after 300 s, and is void when its turn is cancelled, the
    agent paused or stopped, or bzz locked or closed. It is answered once.
  - bzz answers yes or no and passes nothing of the command back.
  - **The folder is where a command starts, not a wall.** The command runs
    as the owner's user, with `PATH`, `HOME`, `LANG` and `TMPDIR` and nothing
    else in its environment, and can reach whatever the owner can. Nothing
    in bzz or its host confines it. `write = false` binds the agent's file
    tools only.
  - **A folder allowed without asking** (`exec = "without-asking"`) lets
    whoever may wake the agent run commands as its owner, also under `bzz
    agent run`. It is never the default. `bzz agent new` and `edit` refuse
    it for an agent that anyone may wake, and bzz asks the owner about every
    command of such an agent whatever the settings file says.
  - Each question and its answer is kept in
    `<data>/agents/<id>/activity.jsonl`: when, the folder, the command's
    first line, the answer. `bzz agent show` prints it.
- **Git credentials.** A user name and a token, stored per agent and host
  with `bzz agent credential set`. They are read without echo, never taken
  as arguments, and sealed with NIP-44 to the owner's own key in
  `<data>/keys/agent-<id>.git/`, so they open only while the owner is
  unlocked. They are never in `config.toml`. What is sealed names the agent
  and the host: a file moved to another agent or host opens for nobody.
  - bzz gives one to the agent's host only as the answer to its request,
    only for a host on that agent's list of allowed hosts (as bzz started
    with it, and as the settings file has it at that moment), only from that
    agent's own store, and only to a session with a turn running. The owner
    is not asked at that moment: storing it was the grant.
  - It is in no log, no database, no activity line and no tool result. The
    host uses it for one git command, keeps it nowhere and stars it out of
    what a remote echoes back; the model never sees it.
    `tests/secret_scan_test.rs` and the tests with the real host search
    every file of a profile for it, in four forms.
  - What it does not give: the token stays in the memory of bzz and of the
    host until each frees it, and it lets the agent do at that host whatever
    the token allows. Give an agent a token made for it.
- **Pi packages.** A package named in Pi's `settings.json` with a Pi Durable
  entry is loaded by the agent's host. **Its code runs inside the host, with
  the owner's rights, outside the agent's sandbox**: it can read the owner's
  files and Pi's sign-in. They are on by default, as in Pi; `packages =
  false` (`--no-packages`) switches them off for an agent. Checking a model
  for `agent new` loads them too, unless they are off. A package that hangs
  at its start is left out after 10 s; one that never gives the thread back
  holds the host, and bzz stops a host that has not answered in 30 s.
- **The settings file has one writer at a time.** A save takes a lock on
  `config.toml.lock`, reads the file as it is, and writes that with only
  what its own copy changed. A command no longer loses an edit to a TUI that
  was open, and a setting that makes an agent safer (a grant taken away, a
  pause) is not undone by another bzz's save.
- **Creating and deleting.** `bzz agent new` publishes two records: the
  agent's profile, signed by the agent, and the owner's record of it, signed
  by the owner after a confirmation. Creation is all or nothing; a failure
  removes what was published while bzz still holds the key, and names what
  the relay would not take back. `bzz agent delete` removes the owner's
  record first and refuses to go on without it, so that no client lists an
  agent whose key is gone. The agent's messages and profile stay.
- **Agents that wake each other.** With `wake = "anyone"`, another agent's
  message wakes a local agent at most six times an hour in one conversation,
  so two agents cannot answer each other for ever. An agent's message is
  known by the owner's authorization it carries; an agent that signs without
  one is counted as a person.
- **What bzz keeps.** The text of each request and of each signed reply stays
  in bzz's database for 30 days after the turn (at most 1000 per agent), then
  is removed. An agent's memory and disk are its two files; `bzz agent
  delete` removes them with its key, its stored git credentials, its
  activity and its rows. What a real command printed is kept by the agent as
  it was printed, so paths of the machine can be in its memory.

**Moving an agent.** An agent's key leaves bzz's custody only by one of two
explicit acts of its owner, each of which needs the owner's identity
unlocked. The owner's guide is [`agents.md`](agents.md#moving-an-agent).

- **The sealed file (`bzz agent move`).** It holds the agent's key, its
  memory, its disk and its settings: everything needed to be the agent.
  - It is sealed with a passphrase the owner types twice, of at least 12
    characters: Argon2id (64 MiB, 3 passes, a new salt per file) makes the
    key, and XChaCha20-Poly1305 seals the content in pieces of at most 1 MiB.
    These are the primitives of bzz's encrypted identity file; no
    cryptography was added.
  - **The passphrase is the whole protection.** Whoever has the file can try
    passphrases without limit and without bzz. The owner's identity is not
    needed to open it. Choose a long passphrase, and delete the file once it
    was received: it opens for as long as it exists.
  - Nothing of the content is readable in the file: not the key, a name, a
    file's name or a file's size beyond the file's total length.
  - A file that was cut short, a file that was changed and a wrong passphrase
    are each refused whole, before anything is used, and named. Two plain
    checksums let bzz tell them apart; they protect nothing. What cannot be
    forged is the sealing: each piece is bound to its place, to being the
    last or not, and to the file's head with its length.
  - What a file cannot do: say that it is the newest. An older file of the
    same agent opens as well, and receiving it puts back an older memory.
- **Receiving (`bzz agent receive`).** The file is refused unless the owner's
  authorization (NIP-OA) inside it was signed for the agent's key and names
  the identity this bzz uses in the agent's community; the key inside must be
  the agent's. What arrives is checked as `bzz agent new` checks an owner's
  answers. The agent arrives with no folder granted, whatever the file says:
  a file that names a folder is not read at all. Receiving needs no relay.
  An agent that is already here and was not moved away is never replaced.
- **The launch kit (`bzz agent launch-kit`).** The one place where an agent's
  key leaves bzz **in the clear**, because Buzz's `buzz-acp` reads the key
  from its environment: under `buzz-acp` the key sits beside the agent
  (decision D8).
  - bzz says so and asks before it writes.
  - The key is in `buzz-acp.env` (mode 600), with the owner's authorization
    and the relay's address, and in no other file of the kit. The folder is
    mode 700; the agent's files in it are mode 600.
  - The kit also holds the agent's memory and disk, **not sealed**: every
    conversation it had, and every file it made.
  - Whoever reads the folder can act as the agent, read what it remembers,
    and keep doing so after the owner took the agent back: a key that was in
    the clear somewhere cannot be taken back. Only a new agent has a new key.
  - On the other machine a Pi package runs inside the agent's host and can
    read the key from the environment. `bzz agent edit --packages off` before
    the kit is made keeps every package off there.
  - A kit whose last step fails is removed, so that no key is left in the
    clear for an agent that did not move.
- **Moved away.** A move ends by marking the agent moved away, in the
  settings and in the agent's own folder (`move.json`), which every bzz reads
  before it starts the agent's process and twice a second while it serves the
  agent. A bzz that was open during the move, with an older copy of the
  settings, therefore stops the agent and does not start it again. bzz keeps
  the sealed key and the files of a moved-away agent until `bzz agent
  delete`.
- **Two copies.** Nothing prevents the same key from running in two places:
  not bzz, and not Buzz. A file or a kit that was copied can be started
  wherever it lies, and both copies answer. bzz refuses only to run what it
  has moved away.
- **A moved agent's stop.** `bzz agent shutdown` publishes one message as the
  owner: `!shutdown`, with the agent mentioned, in a channel both are members
  of. Others in that channel see it. `buzz-acp` obeys it only from the
  agent's owner. Whether the agent left is read from its presence on the
  relay, which the relay forgets three minutes after a crash.

## v0.16.0 trust boundaries

**Apps.** bzz runs a Lua script it did not ship, inside itself, with only
what it lends the script ([`adr-v0.16-apps.md`](adr-v0.16-apps.md),
[`app-api.md`](app-api.md)). The user's guide is [`apps.md`](apps.md).

- **What a script reaches.** No file, network, process, environment variable
  or native code; no key, credential or passphrase. It is told the local
  participant's public key and name. `tests/secret_scan_test.rs` runs every
  `bzz app` command on a profile with a real owner key, an agent key and a
  broker credential, with a script that writes down every global it can
  reach, and finds none of them in anything printed or written.
- **Nothing runs unapproved.** A version runs only after the owner approved
  its exact bytes, by their SHA-256, also one he added himself. One function
  hands out a script to run (`Apps::runnable`); it reads the approved
  version's file and compares it with the approved hash each time, so a file
  edited in `<data>/apps/` behind bzz's back is not run, and bzz says why.
  The database holds the decisions; the files hold only bytes.
- **What the owner is shown** before he answers: who proposed the version,
  the brief, the record types with any change in them marked, the sizes and
  the line difference from the version that runs. What a proposer wrote is
  shown with every control and bidirectional character written out
  (`<U+001B>`), so that it cannot draw on the terminal. Inside bzz the same
  review is an overlay that opens only when the owner asks (`Ctrl+Y`,
  `:approvals`, `:app pending`), keeps the hash and the types on screen,
  and takes a yes only after its last row was drawn; no command approves
  by itself.
- **A new version of an open app** replaces the running one in its session
  only if it loads, declares the approved types and runs its `app.reload`
  and first `app.view`; otherwise the version before, whose approved bytes
  bzz holds, is given back and the new one is recorded as failed. What the
  pane runs is always bytes `Apps::runnable` handed out.
- **The owner's own folder.** `bzz app dev <folder>` runs a folder without
  approval (plan D5), on records kept in memory. `:app dev <folder>` runs
  it in the selected channel's pane on that channel's real records, signed
  by the owner as any app's: it is a folder only the owner can name, typed
  in his own bzz, and the pane's border says `not approved`. It is never
  remembered, so a restart does not run it again.
- **Whose shell it is.** `bzz app approve`, `refuse` and `rollback` are the
  owner's answers because they are typed at his shell. An agent that he lets
  run commands without asking can type them too.
- **Storage.** `<data>/apps/` and every file in it are private (`0700` and
  `0600` on Unix), as bzz's other data. Uninstalling an app deletes its
  versions and leaves the channel's records alone.

<!-- M5, M6: an agent's proposal and an app offered in the channel reach
     the same approval; say here what each may propose and what is checked
     of an offer (size, hash, signer). -->

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

## Dependencies added in v0.15.0

`close_fds` 0.3 (MIT; it depends on `cfg-if` and `libc`, which bzz had
already), on Unix only. bzz uses one function of it,
`set_fds_cloexec_threadsafe`, to mark the descriptors it holds as not to be
handed to an agent's harness. Safe Rust cannot name a descriptor it did not
open, and `unsafe` is forbidden in bzz.

## Dependency advisory

`cargo audit` currently reports `RUSTSEC-2024-0384` (`instant` is unmaintained)
through the revision-pinned `nostr` dependency and `RUSTSEC-2026-0253` (panic
safety in `lru`) through the pinned `ratatui` stack. They are transitive and
not directly selected by bzz. Re-evaluate both on every Buzz or Ratatui
compatibility update; do not change a pin solely to silence an advisory.
