# bzz

`bzz` is a human-first terminal client for [Buzz](https://github.com/block/buzz):
fast keyboard navigation, local offline history, native Nostr authentication,
and host-isolated communities.

> Status: post-MVP development. Protocol compatibility is pinned to Buzz
> `ede26863345a518ec46edd6d7692e0281883491b`.

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
- acknowledged sends with durable ambiguous-outcome recovery;
- reaction toggles, own-message deletion, and encrypted cross-device read state;
- Vim-style navigation, fuzzy channel finder, safe Markdown, and narrow layouts;
- 60 built-in themes plus semantic `theme.toml` customization;
- secure Buzz `imeta`/Blossom attachments with verified offline caching;
- inline JPEG/PNG/GIF/WebP first-frame rendering through Kitty, Sixel, iTerm2,
  or a Unicode half-block fallback;
- image previews, explicit attachment saves, and sanitized image/file uploads.

Search, direct messages, typing/presence, custom emoji, media playback, profile
avatars, and message editing remain post-MVP.

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

Default keys: `j/k`, `gg/G`, `Ctrl-p`, `Ctrl-y`, `Enter`, `i`, `Ctrl-]`, `p`,
`r`, `D`, `U`, `?`, and `Q`. `p` previews attachments and `Ctrl-a` adds a file
while composing. `Ctrl-y` opens the reversible theme picker. Generate
shell integration with `bzz completions <shell>`. Inside the TUI, `:reconnect`,
`:resync`, `:theme reload`, `:purge-cache`, and `:lock` cover the main recovery,
appearance, and security operations.

## Media safety

Only descriptor-backed media on the active Buzz community origin is fetched;
arbitrary Markdown and profile-picture URLs stay inert. Main blobs are fetched
without redirects, authenticated with short-lived blob-scoped Blossom events,
and size/MIME/SHA-256 verified before decode, display, save, or offline reuse.
Generic files are never auto-downloaded or executed. Local image uploads are
bounded, orientation-corrected, and stripped of private metadata before their
exact uploaded bytes are hashed.

Use `bzz media status` to inspect configured limits and `bzz media clear --all
--yes` to remove plaintext owner-only media cache files. See
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

Tagged releases provide Linux, macOS, and Windows archives, SHA-256 checksums,
CycloneDX SBOMs, and GitHub build-provenance attestations. Verification steps
are in `docs/releasing.md`.

See `docs/configuration.md`, [`docs/themes.md`](docs/themes.md),
`docs/security.md`, `docs/protocol-compatibility.md`,
`docs/troubleshooting.md`, the
[basic-first manual E2E plan](docs/e2e-manual.md), and the
[Herdr-assisted E2E guide](docs/e2e-herdr.md).

## License

MIT OR Apache-2.0. See `THIRD_PARTY_LICENSES.md`.
