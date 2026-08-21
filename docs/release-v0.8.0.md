# bzz v0.8.0 — Clipboard-first attachments

> **Status: Unreleased.**

## Explicit paste/import

The composer now makes `Ctrl-v` the primary attachment path. One explicit
native clipboard read uses a fixed precedence: copied local files, then a
copied bitmap/screenshot, then plain text. Files and screenshots appear in the
same bounded attachment queue and still require the normal explicit `Enter`
send action.

`Delete` removes the newest attachment, `Ctrl-r` retries failed staged uploads,
`Ctrl-c` confirmably clears the draft, and `Ctrl-o` provides the explicit
local-path fallback. The old `Ctrl-a` path shortcut is retired.

## Privacy and recovery

Native clipboard reads are explicit-only and controlled by
`media.clipboard_import = "explicit" | "off"`, separately from OSC-52 copy
writes. bzz does not watch the clipboard, call shell helpers, retain source
paths/URIs, or persist raw clipboard representations. File and bitmap imports
use the ordinary private staging, image sanitation, media authorization, and
human-send boundaries.

Attachment workers now carry opaque draft IDs and exact composer targets. A
clear, remove, composer close, target change, or late completion cannot restore
a removed item or overwrite a different draft. Existing drafts with older
pending attachment metadata are repaired locally on open without a database
schema migration.

## Upgrade notes

No relay or SQLite schema migration is required. Existing pending attachment
JSON is accepted with a locally generated opaque ID on the next composer open.
A desktop whose native clipboard backend is unavailable continues to support
terminal text paste and the `Ctrl-o` local-path fallback.
