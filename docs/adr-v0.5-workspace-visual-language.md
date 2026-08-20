# ADR: v0.5 clean-room workspace visual language

**Status:** Accepted

## Context

v0.5 improves bzz's terminal workspace hierarchy: labelled local community and
channel directories, readable conversation measure, deterministic local author
markers, date/group rhythm, an on-demand context surface, a persistent writing
dock, and semantic theme groups.

The product investigation considered observable terminal interaction outcomes
from other clients. One reference client, Concord, is GPL-3.0. bzz remains
`MIT OR Apache-2.0`.

## Decision

The v0.5 workspace visual language is independently authored for bzz. It uses
bzz-owned layout measurements, Ratatui components, local state, semantic theme
groups, documentation, test fixtures, and benchmark cases. No external source,
asset, screenshot, user-facing string, configuration grammar, or test was
copied, translated, adapted, vendored, linked, or used as a derived fixture.

The implementation has these deliberate boundaries:

- layout rectangles are resolved once in `ui::layout` and reused by rendering
  and the generation-bound semantic hit map;
- author markers are deterministic local text derived from an already-authorized
  public key and sanitized label; profile pictures and URLs are never read or
  fetched for presentation;
- the writing dock is only a view onto the existing draft/composer/outbox and
  remains visibly disabled when publication is unavailable;
- readable measure, grouping, dates, and theme semantics are local presentation
  decisions only; they do not alter messages, identity, reads, membership,
  Inbox eligibility, or network scope; and
- a dirty redraw gate coalesces work after visible events without caching raw
  untrusted output or independently recalculating pointer geometry.

The behavior contract and acceptance criteria are in
[`v0.5.0.md`](planning/2026-08-20/v0.5.0.md). This ADR supplements the v0.4
interaction boundary in [`adr-v0.4-clean-room-interaction.md`](adr-v0.4-clean-room-interaction.md).

## Consequences

- bzz retains its `MIT OR Apache-2.0` license with no GPL dependency.
- Themes document bzz semantic groups only; unknown groups are ignored with a
  warning instead of being treated as compatibility promises.
- Visual review captures and fixtures must use synthetic bzz-owned content.
  Personal/community screenshots may provide private operator sign-off but are
  not committed or used as release artwork.
- Reviewers audit this boundary together with dependency pins, identity
  isolation, locked/cache-only operation, Inbox acknowledgement semantics, and
  the human-send boundary.
