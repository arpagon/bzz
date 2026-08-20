# ADR: v0.4 interaction design clean-room boundary

**Status:** Accepted

## Context

bzz v0.4.0 adopts behavior-level interaction patterns seen in Concord: explicit
focus, Vim-style navigation, leader sequences, configurable scoped keymaps,
contextual actions, modal input ownership, selection independent from scrolling,
and keyboard/mouse parity.

Concord is GPL-3.0. bzz is licensed `MIT OR Apache-2.0`.

## Decision

bzz implements its v0.4 interaction model independently. Concord is not a
source dependency or a code source for bzz.

Contributors must not copy, translate, adapt, vendor, link, or derive bzz code,
tests, comments, user-facing strings, configuration samples, or assets from
Concord. The implementation uses bzz-owned Rust types, keymap grammar, reducer,
rendering, tests, and documentation. The behavior contract is specified in
[`v0.4.0.md`](planning/2026-08-18/v0.4.0.md).

The work also retains bzz's existing generation-bound semantic `HitMap`; pointer
events are resolved against the most recently completed bzz render generation
rather than through separately recomputed geometry.

## Consequences

- bzz retains its `MIT OR Apache-2.0` license and no GPL dependency is added.
- The release can adopt useful terminal interaction conventions without
  importing Concord's Discord-specific feature set.
- Reviews of v0.4 changes must verify this boundary alongside bzz's existing
  protocol-pin, identity-isolation, offline/locked-mode, and human-publication
  invariants.
