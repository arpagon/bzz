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
- Graphics-capable terminals may render a locally generated public-key identicon
  over bzz's textual marker. No `Profile.picture`, URL, remote avatar download,
  disk avatar cache, or profile-image request exists.
- Markdown is sanitized into bounded local terminal presentation. Links are
  visible inert text, not actions or fetches.
- Clipboard output is produced only by a direct user copy action, encoded as a
  bounded OSC 52 payload. bzz never copies automatically, echoes copied text,
  invokes shell clipboard helpers, or converts logical message selection into a
  read/publish operation.
- Direct reaction keys only open/confirm the existing typed reaction path; they
  do not bypass signer availability or user selection.

The product contract is
[`v0.6.0.md`](planning/2026-08-20/v0.6.0.md).

## Consequences

- Textual author markers remain the accessibility and unsupported-terminal
  fallback.
- Users who do not want terminal clipboard output can set
  `ui.clipboard = "disabled"` and retain terminal-native selection with
  `ui.mouse = "off"`.
- v0.6 adds no dependency or protocol pin and retains locked/cache-only,
  identity isolation, Inbox acknowledgement, and human-send boundaries.
