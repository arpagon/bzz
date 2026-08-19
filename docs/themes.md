# Themes

`bzz` includes 60 themes and supports semantic customization through an
optional `theme.toml`. Themes affect rendering only: they never alter relay
traffic, identities, cached data, read state, drafts, or the outbox.

## Selecting a theme

The default theme is `bzz`, which preserves the original application look.
List the themes compiled into the current binary:

```bash
bzz theme list
```

Select a global theme:

```bash
bzz theme use nord
```

Select a theme only for one configured community:

```bash
bzz theme use dracula --community <COMMUNITY_UUID>
```

Remove a per-community selection so it inherits the global theme:

```bash
bzz theme reset --community <COMMUNITY_UUID>
```

Reset the global selection:

```bash
bzz theme reset
```

Selection is stored in `config.toml`:

```toml
[ui]
sidebar_width = 28
thread_width = 44
theme = "nord"

[[communities]]
id = "00000000-0000-0000-0000-000000000000"
label = "example"
relay_url = "wss://relay.example/"
identity_id = "11111111-1111-1111-1111-111111111111"
theme = "dracula" # optional
```

A community theme wins over `ui.theme`. Without one, the community inherits
the global selection.

## Interactive picker

Press `Space o` in normal mode:

- type to filter;
- use `j/k` or the arrow keys to preview;
- use `Tab` to switch between community and global scope;
- press `Enter` to save;
- press `Esc` to restore exactly the theme active before opening the picker.

A preview never writes configuration. A confirmed selection uses the same
atomic configuration replacement as the other `bzz` settings.

## Personal customization

`bzz theme path` prints the optional customization path. It is normally
`theme.toml` beside `config.toml`. The file applies after the selected global
or per-community built-in theme.

Every field is optional:

```toml
[highlight.Normal]
foreground = "terminal_default"
background = "terminal_default"

[highlight.FocusBorder]
foreground = "light_magenta"

[highlight.FocusedPaneBorder]
link = "FocusBorder"
bold = true

[highlight.SelectedRow]
background = "#262630"

[highlight.MessageTimestamp]
link = "Muted"
italic = true

[ui.border]
default = "plain"
composer = "rounded"
modal = "double"
```

Reload the file while the TUI is running:

```text
:theme reload
```

There is deliberately no filesystem watcher. Reload is explicit and
predictable.

### Highlight fields

| Field | Value | Meaning |
|---|---|---|
| `link` | group name or `"none"` | Inherit unset properties or detach from the default parent |
| `foreground` | color or `"none"` | Foreground color or clear the channel |
| `background` | color or `"none"` | Background color or clear the channel |
| `bold` | boolean | Enable or disable bold |
| `italic` | boolean | Enable or disable italic |
| `dim` | boolean | Enable or disable dim |
| `underline` | boolean | Enable or disable underline |
| `strikethrough` | boolean | Enable or disable strikethrough |

A child inherits only values it does not define. Explicit `false` removes an
inherited modifier. `foreground = "none"` and `background = "none"` clear that
color instead of resetting it to a built-in value. Links may point forward or
backward. Cycles produce a warning and cyclic inheritance is isolated.

Names are exact and case-sensitive in `theme.toml`.

### Colors

Accepted values are:

- `none`;
- `terminal_default`;
- `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`;
- `dark_gray`, `light_red`, `light_green`, `light_yellow`, `light_blue`,
  `light_magenta`, `light_cyan`, `white`;
- six-digit RGB with an optional `#`, such as `#89B4FA` or `89B4FA`.

ANSI names use the terminal palette. `terminal_default` explicitly selects the
terminal default. `none` leaves the underlying cell channel untouched.

Raw escape sequences, indexed strings, URLs, includes, commands, and external
paths are not accepted. `theme.toml` is limited to 256 KiB.

### Semantic groups

Base groups:

- `Normal`, `Strong`, `Emphasis`, `Muted`, `Title`;
- `Border`, `FocusBorder`, `Selection`, `SelectionBorder`;
- `Error`, `Warning`, `Success`, `Info`, `Pending`, `Rejected`.

Panels and navigation:

- `PaneTitle`, `PaneBorder`, `FocusedPaneBorder`;
- `ModalTitle`, `ModalBorder`;
- `CommunityRail`, `CommunitySelected`;
- `Sidebar`, `SidebarText`, `SidebarMuted`, `ChannelUnread`;
- `SelectedRow`, `SelectionMarker`, `UnreadNotice`, `Placeholder`.

Conversation and composer:

- `MessageAuthor`, `MessageTimestamp`, `MessageBody`, `MessageDeleted`;
- `Reaction`, `SelfReaction`;
- `Composer`, `ComposerTitle`, `ComposerBorder`, `ActiveComposerBorder`.

Status and Markdown:

- `StatusBar`, `StatusMode`, `StatusModeInsert`, `StatusModeCommand`;
- `MarkdownLink`, `MarkdownCode`, `MarkdownMarker`.

Component groups link to semantic parents. For example,
`MessageAuthor -> Info`, `Pending -> Warning`,
`FocusedPaneBorder -> FocusBorder`, and `MessageDeleted -> Muted`. Changing a
parent updates children that remain linked; overriding a child changes only
that component.

Themes written for Concord can reuse common group names and field grammar.
Discord-specific groups are unknown to `bzz`, produce a warning, and are
ignored. This is behavioral compatibility for the shared semantic subset, not
source-code reuse or full Concord UI compatibility.

### Border shapes

`[ui.border]` supports these surfaces:

- `default`;
- `pane`;
- `composer`;
- `modal`;
- `picker`;
- `message`.

Supported values are `plain`, `rounded`, `double`, `thick`,
`light_double_dashed`, `heavy_double_dashed`, `light_triple_dashed`,
`heavy_triple_dashed`, `light_quadruple_dashed`,
`heavy_quadruple_dashed`, `quadrant_inside`, and `quadrant_outside`.

A theme changes the Ratatui glyph set but cannot remove border sides or change
layout geometry.

## Inspection, export, and validation

Print an exportable semantic definition:

```bash
bzz theme show catppuccin-mocha
```

Create a new owner-only customization file:

```bash
bzz theme export catppuccin-mocha --output /tmp/theme.toml
```

Export uses create-new semantics and does not overwrite an existing file. On
Unix the resulting mode is `0600`.

Validate selections and `theme.toml`:

```bash
bzz theme check
bzz check
```

Validation covers:

- selected theme IDs;
- TOML syntax and size;
- known groups and fields;
- color and modifier types;
- links and cycles;
- border shapes;
- contrast for known RGB foreground/background pairs.

Unknown or invalid leaves generate warnings while valid siblings still apply.
A complete TOML syntax error makes `theme check` fail. The TUI remains
recoverable: it uses the selected built-in theme and displays a non-secret
warning instead of refusing to start.

To recover:

```bash
mv "$(bzz theme path)" "$(bzz theme path).disabled"
bzz theme reset
bzz theme check
```

## Built-in catalog

The catalog contains `bzz` plus 59 palettes adapted from the pinned MIT-licensed
`slk` revision:

- core: Dark, Light, ANSI Dark, ANSI Light;
- Solarized, Gruvbox, Catppuccin, Tokyo Night, GitHub, Rosé Pine, Everforest,
  Flexoki, Modus, Kanagawa and Ayu light/dark families;
- Dracula, Nord, One Dark, Monokai, Material, Nightfox, Carbonfox, Vesper,
  Night Owl, Poimandres, Zenburn, Iceberg, Cobalt2, Synthwave and others;
- classic sidebar palettes such as Aubergine, Ochin, Choco Mint, Mocha, and
  Nocturne.

`bzz theme list` is authoritative for exact stable IDs and display names in an
installed version.

The palette data is compiled into the binary. Selecting a built-in theme does
not read arbitrary files or access the network. Attribution is recorded in
`THIRD_PARTY_LICENSES.md`.
