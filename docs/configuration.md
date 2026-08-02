# Configuration

`bzz` stores non-secret configuration in the platform configuration directory
(`bzz paths` prints it). `BZZ_CONFIG_DIR`, `BZZ_DATA_DIR`, and `BZZ_CACHE_DIR`
override directories for testing and managed deployments; they never contain
secret values. Without overrides, debug builds use a separate `bzz-dev`
platform directory and `dev.arpagon.bzz.debug` credential service. Release
builds use `bzz` and `dev.arpagon.bzz`.

```toml
default_community = "00000000-0000-0000-0000-000000000000"

[[identities]]
id = "00000000-0000-0000-0000-000000000001"
label = "personal"
pubkey = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
backend = "keychain"
key_ref = "identity:00000000-0000-0000-0000-000000000001"

[[communities]]
id = "00000000-0000-0000-0000-000000000000"
label = "team"
relay_url = "wss://buzz.example/"
identity_id = "00000000-0000-0000-0000-000000000001"
allow_insecure_localhost = false
theme = "dracula" # optional; overrides the global UI theme

[ui]
sidebar_width = 28
thread_width = 44
theme = "bzz"

[media]
enabled = true
protocol = "auto"            # auto|kitty|sixel|iterm2|halfblocks|off
autoload = "visible"         # visible|preview|off
max_inline_rows = 12
auto_download_bytes = 26214400
memory_cache_bytes = 67108864
disk_cache_bytes = 536870912
download_concurrency = 4
decode_concurrency = 2
```

The `key_ref` is an opaque OS-keychain account name, not a secret. Use
`bzz identity verify <id>` to test availability and
`bzz identity restore-backup <id> --input <file>` to restore the same configured
pubkey without editing TOML.

Only `wss://` root URLs are accepted by default. `ws://` requires both a
loopback host and explicit acknowledgement. Credentials, queries, fragments,
and non-root paths are rejected. Each relay authority is an isolated Buzz
community; bzz never sends a client-selected tenant identifier.

`ui.theme` selects the global built-in theme. An optional community `theme`
field takes precedence only while that community is active. Semantic overrides
live in a separate, optional `theme.toml`; see [`themes.md`](themes.md). Theme
configuration is presentation-only and is never synchronized through the
relay.

The strict `[media]` section controls terminal rendering and bounded local
resources. Unknown fields are rejected. `autoload = "visible"` fetches valid,
community-origin image descriptors near the rendered timeline; `preview`
fetches only after `p`; `off` permits explicit save/upload but no automatic
fetch. `protocol = "off"` disables graphics while retaining attachment cards.
Cache/concurrency settings are validated against hard safety ceilings. See
[`media.md`](media.md).
