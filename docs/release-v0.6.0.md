# bzz v0.6.0 (release candidate)

> **Status:** candidate — publish only after the tagged commit completes the
> release workflow and the artifact checks in [`releasing.md`](releasing.md).

v0.6.0 is a clean-room conversation organization and message-action release.
It retains the existing SQLite schema, NIP-49/keychain flow, pinned Buzz/slk
revisions, Inbox projection, and human-send boundary. No new network endpoint,
protocol feature, or dependency is introduced.

## Highlights

- **Channel order:** private local `ui.channel_sort` supports smart (unread,
  then activity), recent, and alphabetical modes. `Space s` cycles modes while
  preserving selection by channel ID; it cannot mutate a read marker,
  subscription, stored channel, or relay state.
- **Local author markers:** every terminal uses a compact deterministic shape
  and initial derived from already-visible local identity data. No
  `Profile.picture`, URL, remote download, avatar disk cache, or terminal-image
  overlay is used.
- **Practical Markdown:** headings, quote gutters, ordered/unordered lists,
  task items, rules, inline code, and fenced code blocks render with safe local
  structure. Tables use measured Unicode grids where they fit, or labelled row
  records when wide data would make columns misleading. Links remain inert
  visible text.
- **Copy and select:** `y` explicitly copies sanitized message source via a
  64-KiB-bounded OSC 52 payload. `v` starts a logical event-ID range and `y`
  copies it in chronological order. `ui.clipboard = "disabled"` suppresses all
  bzz clipboard output; terminal-native partial selection remains available
  with `ui.mouse = "off"`.
- **Reactions:** `r` opens the existing picker on a selected timeline/context
  message; `1`–`8` choose a displayed reaction directly. Signing/publishing
  checks are unchanged.

## Security and clean-room boundary

The implementation and all fixtures are bzz-owned. The decisions for local
author markers, safe presentation, clipboard encoding, and direct reaction
entry are in
[`adr-v0.6-organized-conversations.md`](adr-v0.6-organized-conversations.md).
In particular, presentation does not fetch profile images or URLs, copy text
automatically, echo clipboard content, invoke a shell, alter Inbox reads, or
bypass signer availability.

## Required candidate gates

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check
cargo audit
cargo bench --bench timeline --locked
BZZ_BUZZ_SOURCE=/path/to/pinned/buzz ./scripts/test-relay.sh
cargo build --release --locked
BZZ_BIN="$PWD/target/release/bzz" BZZ_HERDR_PANE=<disposable-pane> \
  ./scripts/test-tui-herdr.sh
```

All listed local gates passed on 2026-08-20. `cargo deny check` emitted only
its documented duplicate-version warnings; `cargo audit` emitted only the
existing allowed transitive `instant`/`lru` advisories. The pinned real-relay
journey and the release-binary Herdr scenarios passed using disposable data.
Measurements and the remaining manual review are recorded in
[`validation-v0.6.md`](validation-v0.6.md).

Use only generated/disposable content for visual review and the release TUI
runner. Follow archive, checksum, SBOM, provenance, supported-platform, and
clean-VM verification steps in [`releasing.md`](releasing.md) before publishing.
