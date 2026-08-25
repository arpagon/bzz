# bzz

`bzz` is a human-first terminal client for [Buzz](https://github.com/block/buzz):
fast keyboard navigation, local offline history, native Nostr authentication,
and host-isolated communities.

> Status: v0.11.0 is developing verified remote managed-agent interoperability:
> community-scoped NIP-OA ownership, bot membership, public policy, an Agents
> directory, and exact human-authored mentions. bzz does not host or control an
> agent runtime and launches no local assistant process. Protocol compatibility
> is pinned to Buzz `9f55bf67456be10ff7c8238bf0d9e12e582848f6`.

## Build

```sh
cargo build --release --locked
cargo test --locked
```

Rust 1.95.0 is selected by `rust-toolchain.toml`. The first build fetches the
revision-pinned `buzz-core` and `buzz-sdk` crates. Debug builds intentionally
use `bzz-dev` paths and a separate OS-keychain service.

## MVP features

- several configured communities with one active authenticated session;
- cached/offline channel history, profiles, threads, unread markers, and drafts;
- acknowledged sends with durable ambiguous-outcome recovery and distinct
  pending/delivery-unknown/rejected presentation;
- bounded owner-private connection/outbox diagnostics, metadata-only support
  reports, and optional default-off OTLP/HTTP protobuf log export;
- reaction toggles, own-message deletion, and encrypted cross-device read state;
- Vim-style navigation, fuzzy channel finder, safe Markdown, and responsive narrow layouts;
- labelled community/channel directories with local smart/recent/A–Z ordering,
  bounded readable measure, deterministic local author markers, practical safe
  Markdown, date/group rhythm, and a visible writing dock;
- 60 built-in themes plus semantic `theme.toml` customization;
- an active-community Inbox for mentions, relevant threads, workspace DMs,
  read-only needs-action events, unread rows, and drafts;
- Buzz Desktop-compatible one-to-one/group workspace DMs with owner-only hide
  state and explicit non-E2EE labeling;
- unified channel/DM/person/message search using offline SQLite FTS5 plus
  authenticated NIP-50 prefix search and `from:`/`in:`/date operators;
- secure Buzz `imeta`/Blossom attachments with verified offline caching;
- inline JPEG/PNG/GIF/WebP first-frame rendering through Kitty, Sixel, iTerm2,
  or a Unicode half-block fallback;
- image previews, explicit attachment saves, and sanitized image/file uploads;
- configurable semantic terminal-mouse interaction with safe restoration; and
- offline channel-member `@` completion with exact Buzz `p` tags; and
- verified remote managed-agent discovery, owner/policy inspection, distinct
  completion, and send-time revalidation without local runtime control.

NIP-17 gift-wrap authoring/inbox, typing/presence, custom emoji, media playback,
and message editing remain post-MVP. Workspace DMs are private
relay channels, not end-to-end encrypted messages.

## First run

```sh
bzz identity new --label personal
bzz identity list
bzz identity verify <identity-uuid>
bzz identity backup <identity-uuid> --output ~/identity.ncryptsec
bzz community add my-team wss://buzz.example <identity-uuid>
bzz
```

The secret key is placed in the operating-system credential store. If no
credential service is available, use `--backend encrypted-file`; bzz prompts
on the controlling terminal and never accepts secrets in command arguments or
ordinary environment variables. Backups are password-encrypted NIP-49
`ncryptsec` files created atomically with owner-only permissions. Restore a
missing credential without changing its identity or communities with
`bzz identity restore-backup`.

Default navigation uses `1`–`4` to focus workspace surfaces, `Tab`/`h`/`l`
to traverse them, `j`/`k` or `Ctrl-n`/`Ctrl-p` to change selection, and
`gg`/`G` for edges. The workspace shows labelled communities, channels, the
conversation, and an on-demand context pane. Each message has a compact
textual avatar marker; supported graphics terminals may additionally show a
bounded public profile photograph according to `ui.profile_avatars`. `Space` is the
leader: `Space Space` opens the channel/DM switcher, `Space n` opens Inbox,
`Space s` cycles local channel ordering, `Space a` opens contextual actions,
`Space o` opens theme options, and `?` shows the generated effective-keymap
help. On a selected conversation message, `r` opens reactions, `y` explicitly
copies sanitized text through OSC 52, and `v` starts/cancels a logical
multi-message copy range. `q` unwinds owned UI state and asks before quitting.
The visible writing dock activates with `i` or a click; in read-only/locked
state it explains why writing is unavailable. In Inbox, `i`
targets the selected validated channel/thread without silently marking work
read. Inbox uses `f` to
cycle filters, `Enter` for detail, `o` for canonical source context, `m`/`U`
for explicit read/unread state, and `a` for confirmed visible bulk-read. Wide
terminals retain list/detail together; on narrow terminals `Esc` returns from
detail to the list. `@` opens cached channel-member completion. In the composer,
`Ctrl-o` opens the OS file chooser, `Ctrl-v` explicitly pastes copied files, an
image, or text, and `Alt-o` opens the local-path fallback. `Delete` removes the
newest attachment and `Ctrl-r` retries failed uploads. Generate shell
integration with `bzz completions <shell>`. Inside the TUI, `:inbox`, `:search`,
`:dm`, `:agents`, `:reconnect`, `:resync`, `:theme reload`, `:purge-cache`, and `:lock`
cover the main conversation, recovery, appearance, and security operations.

## Media safety

Only descriptor-backed message media on the active Buzz community origin is
fetched; arbitrary Markdown URLs stay inert. Independently, the default
`ui.profile_avatars = "trusted"` may retrieve a bounded kind-0 `picture`.
External pictures use a credential-free public-HTTPS client. A canonical image
path on the active community relay instead receives a short-lived,
content-addressed Blossom read authorization, never sent to another origin.
Set it to `"off"` to keep profile URLs inert. Main blobs are fetched without
redirects, authenticated with short-lived blob-scoped Blossom events,
and size/MIME/SHA-256 verified before decode, display, save, or offline reuse.
Generic files are never auto-downloaded or executed. Local image uploads are
bounded, orientation-corrected, and stripped of private metadata before their
exact uploaded bytes are hashed.

Use `bzz media status` to inspect configured limits and `bzz media clear --all
--yes` to remove plaintext owner-only message-media and profile-avatar cache
files. See
[`docs/media.md`](docs/media.md) for protocols, configuration, key bindings,
cache behavior, and terminal compatibility.

## Validation and releases

Run all local protocol-free gates with:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check
cargo audit
cargo bench --locked
```

The pinned real-relay journey is opt-in because it starts Docker services:

```sh
BZZ_BUZZ_SOURCE=/path/to/block/buzz ./scripts/test-relay.sh
```

In a controlled Herdr pane with a disposable shell target, run release-TUI
acceptance smoke scenarios (they use no identities or secrets):

```sh
BZZ_BIN="$PWD/target/release/bzz" BZZ_HERDR_PANE=<pane-id> \
  ./scripts/test-tui-herdr.sh
```

Tagged releases provide Linux, macOS, and Windows archives, SHA-256 checksums,
CycloneDX SBOMs, and GitHub build-provenance attestations. Verification steps
are in `docs/releasing.md`.

See [`docs/configuration.md`](docs/configuration.md),
[`docs/themes.md`](docs/themes.md),
[`docs/inbox-dms-search.md`](docs/inbox-dms-search.md),
[`docs/security.md`](docs/security.md),
[`docs/protocol-compatibility.md`](docs/protocol-compatibility.md),
[`docs/troubleshooting.md`](docs/troubleshooting.md),
[the v0.11.0 release](docs/release-v0.11.0.md),
[v0.11.0 validation](docs/validation-v0.11.0.md),
[published v0.11.0 artifact verification](docs/release-v0.11.0-verification.md),
[the approved v0.11.0 interoperability plan](docs/planning/2026-08-24/v0.11.0.md),
[the v0.11 remote-agent ADR](docs/adr-v0.11-remote-managed-agent-interoperability.md),
[the v0.10.0 agent-foundation reset](docs/release-v0.10.0.md),
[v0.10.0 validation](docs/validation-v0.10.0.md),
[published v0.10.0 artifact verification](docs/release-v0.10.0-verification.md),
[the Buzz managed-agent architecture research](docs/how-agents-works-in-buzz.md),
[the v0.9.0 diagnostics and observability notes](docs/release-v0.9.0.md),
[the v0.8.0 clipboard-attachment notes](docs/release-v0.8.0.md),
[v0.8.0 validation](docs/validation-v0.8.0.md),
[the v0.7.1 relay-avatar release](docs/release-v0.7.1.md), and the
[basic-first manual E2E plan](docs/e2e-manual.md),
[v0.7.0 avatar validation evidence](docs/validation-v0.7.md),
[v0.7.1 relay-avatar validation](docs/validation-v0.7.1.md),
[published v0.7 artifact verification](docs/release-v0.7-verification.md), and
the [Herdr-assisted E2E guide](docs/e2e-herdr.md).

## License

MIT OR Apache-2.0. See `THIRD_PARTY_LICENSES.md`.
