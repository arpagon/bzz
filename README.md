# bzz

`bzz` is a human-first terminal client for [Buzz](https://github.com/block/buzz):
fast keyboard navigation, local offline history, native Nostr authentication,
and host-isolated communities.

> Status: MVP. Protocol compatibility is pinned to Buzz
> `ede26863345a518ec46edd6d7692e0281883491b`.

## Build

```sh
cargo build --locked
cargo test --locked
```

Rust 1.95.0 is selected by `rust-toolchain.toml`. The first build fetches the
revision-pinned `buzz-core` and `buzz-sdk` crates.

## MVP features

- several configured communities with one active authenticated session;
- cached/offline channel history, profiles, threads, unread markers, and drafts;
- acknowledged sends with durable ambiguous-outcome recovery;
- reaction toggles, own-message deletion, and encrypted cross-device read state;
- Vim-style navigation, fuzzy channel finder, safe Markdown, and narrow layouts.

Search, direct messages, attachments, typing/presence, custom emoji, and message
editing are intentionally post-MVP.

## First run

```sh
bzz identity new --label personal
bzz identity list
bzz community add my-team wss://buzz.example <identity-uuid>
bzz
```

The secret key is placed in the operating-system credential store. If no
credential service is available, use `--backend encrypted-file`; bzz prompts
on the controlling terminal and never accepts secrets in command arguments or
ordinary environment variables. Use `--show-backup` only in a private terminal;
it requires a second explicit confirmation before revealing an `nsec`.

Default keys: `j/k`, `gg/G`, `Ctrl-p`, `Enter`, `i`, `Ctrl-]`, `r`, `D`, `U`,
`?`, and `Q`. Generate shell integration with `bzz completions <shell>`.
Inside the TUI, `:reconnect`, `:resync`, `:purge-cache`, and `:lock` cover the
main recovery and security operations.

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

See `docs/configuration.md`, `docs/security.md`,
`docs/protocol-compatibility.md`, and `docs/troubleshooting.md`.

## License

MIT OR Apache-2.0. See `THIRD_PARTY_LICENSES.md`.
