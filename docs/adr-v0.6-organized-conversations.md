# ADR: v0.6 organized conversations and local copy boundary

**Status:** Accepted

## Context

v0.6 improves bzz's channel scan order, visual author markers, practical
Markdown, copying, and reaction discovery. Other terminal clients informed
behavior-level product research only. bzz remains `MIT OR Apache-2.0`; some
research references are GPL-licensed.

## Decision

The v0.6 implementation is independently authored with bzz types, layout,
configuration, strings, fixtures, tests, and documentation. No external source
code, asset, screenshot, configuration grammar, or derived test is copied or
adapted.

- Channel order is a local `UiConfig` preference over already-authorized
  channel/unread/activity facts. It neither changes stored order nor causes
  relay activity.
- Every terminal renders the same compact public-key-derived textual author
  marker. A raster-identicon experiment was removed after visual review showed
  terminal-image overlay artefacts while scrolling; it is deferred pending a
  dedicated terminal-graphics lifecycle design. No `Profile.picture`, URL,
  remote avatar download, disk avatar cache, or profile-image request exists.
- Markdown is sanitized into bounded local terminal presentation. Measured
  Unicode grids render tables that fit; wide tables become labelled records so
  columns never wrap into misleading fragments. Links are visible inert text,
  not actions or fetches.
- Clipboard output is produced only by a direct user copy action, encoded as a
  bounded OSC 52 payload. bzz never copies automatically, echoes copied text,
  invokes shell clipboard helpers, or converts logical message selection into a
  read/publish operation.
- Direct reaction keys only open/confirm the existing typed reaction path; they
  do not bypass signer availability or user selection.

The product contract is
[`v0.6.0.md`](planning/2026-08-20/v0.6.0.md).

## Consequences

- Textual author markers are the sole v0.6 author-avatar presentation and work
  consistently in graphics and non-graphics terminals.
- Users who do not want terminal clipboard output can set
  `ui.clipboard = "disabled"` and retain terminal-native selection with
  `ui.mouse = "off"`.
- v0.6 adds no dependency or protocol pin and retains locked/cache-only,
  identity isolation, Inbox acknowledgement, and human-send boundaries.

## Research boundary

The rendering decision was informed only at the behavior level by public,
permissively licensed Rust terminal Markdown projects (including the
MIT/Apache-2.0 `joshka/tui-markdown` documentation): measured table grids,
labelled wide-table fallback, visible inert links, and semantic style hooks.
No external source code, fixtures, visual assets, test cases, configuration,
or strings were copied or adapted.
