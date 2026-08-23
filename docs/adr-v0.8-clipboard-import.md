# ADR: v0.8 explicit clipboard import

**Status:** Accepted  
**Date:** 2026-08-21

## Context

Attaching a copied local file or screenshot should not require retyping a
filesystem path in a terminal composer. That convenience is sensitive: desktop
clipboards can contain personal text, paths, images, and opaque application
formats. An attachment flow must not convert passive clipboard observation into
an upload capability or weaken bzz's existing staging, identity, and
human-send boundaries.

## Decision

bzz reads the native clipboard only after an explicit `Ctrl-v` action while an
unlocked composer owns input. The native adapter is an isolated, pinned,
cross-platform dependency with a small bzz interface and a deterministic fake
for tests. The clipboard adapter does not start a shell, helper process, file
picker, daemon, or clipboard watcher.

One read chooses exactly one representation in this order:

1. a native list of at most eight paths;
2. an RGBA bitmap, encoded as a bounded PNG named `pasted-image.png`;
3. bounded normalized plain text; or
4. an inert, content-free unavailable/empty/rejected status.

File source paths and native representation names do not enter drafts, SQLite,
normal status, logs, or screenshots. File bytes use the established
regular-file/non-symlink open-and-revalidate staging path. Bitmaps are bounded
for dimensions, pixels, RGBA length, and encoded PNG bytes, then pass through
the same sanitized image staging path. Text is only inserted into the local
composer after control-character normalization; it is never sent merely by
pasting.

`[media].clipboard_import = "explicit" | "off"` controls only this native
clipboard *read*. It defaults to `"explicit"` and is independent of
`[ui].clipboard`, which controls explicit OSC-52 *writes*. Terminal
bracketed-paste text remains normal terminal input and never requires a native
clipboard read.

`Ctrl-o` is a separate explicit OS file-chooser capability. Linux calls XDG
Desktop Portal directly, without a shell or zenity fallback; Windows and macOS
use their native open dialogs. Up to eight returned local paths remain
transient, exact-target scoped, and enter the same secure staging pipeline.
Cancel or unavailable outcomes reveal no path and never retry. `Alt-o` retains
the manual local-path fallback.

Every staged attachment owns an opaque random local ID. Background staging and
upload completions carry the exact composer target and this ID. Clear, remove,
close, target change, and stale completions cannot attach a removed item or
mutate another draft. A completed upload after its composer closes updates only
its exact persisted draft ID; an unreferenced uploaded blob is not deleted
because the relay offers no ownership-safe deletion operation.

## Consequences

- The primary composer flow is `Ctrl-o` for the OS file chooser, `Ctrl-v` for
  paste/import, `Delete` for the newest attachment, `Ctrl-r` to retry failed
  uploads, `Ctrl-c` to confirmably clear, and `Alt-o` for the clearly labelled
  local-path fallback. `Ctrl-a` no longer claims an attachment role.
- The composer presents processing, queued/uploading, ready, and failed
  attachment rows even without terminal graphics. Sending remains disabled
  until every queued attachment is ready and still requires explicit `Enter`.
- Native clipboard access may be unavailable in a remote, sandboxed, or
  unsupported desktop session. That is an expected local status, not a fallback
  to a shell command; `Ctrl-o`, `Alt-o`, and terminal text paste remain
  available according to their own platform capabilities.
- Linux file selection uses pinned `ashpd 0.13.13` (MIT) with only its Tokio and
  file-chooser features. Windows/macOS selection uses pinned `rfd 0.17.2` (MIT)
  with default Linux backends disabled.
- The clipboard dependency is pinned to `arboard 3.6.1` (MIT OR Apache-2.0, Rust 1.71
  minimum). Its Wayland data-control feature is included for explicit clipboard
  reads on Wayland desktops; environments without the compositor extension
  degrade to unavailable rather than broadening privileges.

This is an independently authored bzz design informed only by observed
high-level terminal-client behavior. It does not reuse another client's source,
strings, configuration grammar, test cases, or platform fallback code.
