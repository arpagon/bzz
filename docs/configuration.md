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
message_width = 110        # readable message measure; 48..200 cells
theme = "bzz"
mouse = "auto"              # auto|on|off

[[local_agents]]
id = "00000000-0000-0000-0000-000000000002"
label = "local-drafter"
backend = "codex"
workdir = "/home/example/read-only-workspace" # optional; canonical existing directory

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
community; bzz never sends a client-selected tenant identifier. Inbox, DM
visibility, and local FTS rows inherit that community/identity partition and
have no separate configuration. Online Inbox/search use only the active
community connection; locked mode is cache-only.

`ui.theme` selects the global built-in theme. An optional community `theme`
field takes precedence only while that community is active. `ui.mouse = "auto"`
enables mouse capture only on an interactive, non-`dumb` terminal; set it to
`"on"` to force it or `"off"` to retain terminal text selection and avoid
mouse-capture sequences. `ui.message_width` caps only the rendered text
measure in a wider conversation pane; it never truncates, stores, or changes
message content. The workspace uses labelled local community rows, local
deterministic author markers, and a visible writing dock. Author markers
never fetch `Profile.picture` or another URL. The dock activates with `i` or a
click and uses the ordinary existing draft/outbox; it is visibly disabled when
the active identity cannot publish. Semantic overrides live in a separate,
optional `theme.toml`; see [`themes.md`](themes.md). UI configuration is
presentation-only and is never synchronized through the relay.

## Keymap

`keymap.toml` is an optional, non-secret file beside `config.toml` (`bzz paths`
prints its parent directory). It is read once before bzz enters raw mode; run
`bzz check` after editing it. A missing file uses the v0.4 defaults. A malformed
file is rejected in full and is never partly applied or printed back to the
terminal.

```toml
# Scope defaults to "global". This exact sequence replaces the builtin one
# only while the workspace route is active.
[[binding]]
scope = "workspace"
keys = ["space", "o"]
action = "open-help"

# Disable an inherited global binding in one scope. `action` is forbidden here.
[[binding]]
scope = "workspace"
keys = ["space", "o"]
disabled = true
```

Each `[[binding]]` has only `scope`, `keys`, `action`, and `disabled`. Scopes
are `global`, `workspace`, `inbox`, `composer`, `filter`, and `overlay`.
`keys` is a one-to-four chord sequence. A chord is one printable character,
`space`, `tab`, `backtab`, `enter`, `esc`, `backspace`, `delete`, `up`, `down`,
`left`, `right`, `home`, `end`, `pageup`, or `pagedown`, optionally prefixed
with `ctrl-`, `alt-`, and/or `shift-`. Uppercase characters imply `shift-`.

The file is capped at 64 KiB and 128 declared bindings. Duplicate effective
bindings, action/prefix ambiguity, unknown TOML fields, and text-owning scopes
that capture ordinary printable characters are rejected. Composer bindings are
limited to documented composer editing/completion actions; they cannot become
workspace shortcuts. Overlay bindings likewise do not inherit background
workspace actions.

Defaults use `Space` as leader. Its popup shows valid continuations and expires
after 750 ms without triggering a partial action. `?` opens generated help for
the effective workspace keymap, including scoped overrides, disabled bindings,
and contextual availability. `Alt-h`/`Alt-l` resize the focused channel or
context pane in bounded two-cell steps; the width is saved locally. `q` closes
the foremost owned UI state and asks before quitting the workspace. No keymap
binding can invoke a shell or publish without the normal human
send/confirmation path.

The accepted action names are the kebab-case forms of the generated help
labels: route actions include `open-inbox`, `open-context-actions`,
`activate-focused`, `compose`, `filter`, `mark-read`, `mark-unread`,
`mark-visible-read`, and `open-canonical-context`; navigation and viewport
actions include `select-next`, `select-previous`, `jump-top`, `jump-bottom`,
`scroll-viewport-up`, `scroll-viewport-down`, `half-page-up`, and
`half-page-down`. `mark-read`, `mark-unread`, `mark-visible-read`, and
`open-canonical-context` are Inbox-only. Composer-only editing names are
`submit`, `insert-newline`, `complete`, `delete-previous-word`,
`delete-to-start`, `delete-to-end`, `move-word-left`, `move-word-right`,
`move-line-start`, and `move-line-end`. Use `?` inside the relevant route as
the authoritative effective binding list; it includes local disabled bindings
and actions unavailable for the current selection.

`[[local_agents]]` configures named, local-only Codex draft assistants. It
contains no credentials, Nostr identity, relay URL, prompt, or output. A
configured `workdir` must already exist, be a canonical directory, and is used
read-only; without one, each run receives an empty owner-only scratch
directory. Use `bzz agent add --label <label> [--workdir <directory>]`,
`bzz agent list`, `bzz agent remove <id> --yes`, and `bzz agent doctor` to
manage or check the local installation. Codex authentication remains external
to bzz. A selected assistant only creates an unpersisted review draft; normal
human composer send remains the sole publishing path.

The strict `[media]` section controls terminal rendering and bounded local
resources. Unknown fields are rejected. `autoload = "visible"` fetches valid,
community-origin image descriptors near the rendered timeline; `preview`
fetches only after `p`; `off` permits explicit save/upload but no automatic
fetch. `protocol = "off"` disables graphics while retaining attachment cards.
Cache/concurrency settings are validated against hard safety ceilings. See
[`media.md`](media.md).
