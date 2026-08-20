# bzz v0.5.0 — Calm Workspace Shell and High-Legibility Conversations

**Status:** Complete locally; owner visual review approved and M1–M5 release-candidate gates recorded. Publishing remains subject to the tagged-release workflow and clean-VM artifact verification.

**Target:** `v0.5.0`

**Intent:** Make the existing keyboard-first bzz workspace easier to scan,
navigate, and trust at a glance. The goal is a dense, calm, terminal-native
conversation workspace—not a visual clone of another application.

**Compatibility baseline:** Rust 1.95 / Rust 2024; Ratatui 0.30; Crossterm
0.29; the existing SQLite cache; Buzz
`ede26863345a518ec46edd6d7692e0281883491b`; and slk
`8149c3b18ed04c259efe5feb545d040ab043d922`. Pin changes are out of scope.

## 1. Evidence and product diagnosis

The supplied wide-terminal captures show that bzz already has useful raw
materials: a transparent/terminal-respecting theme, persistent columns, thin
borders, channel and thread context, and a familiar message chronology.

The main gap is **visual hierarchy**, rather than missing protocol capability:

1. The four-cell community rail communicates only an initial. It does not make
   community identity, switching, or the current selection easy to scan.
2. The flat channel list is dense but weakly structured. Selection, unread
   state, channel type, and the currently open channel compete in the same
   low-contrast row treatment.
3. The timeline title, author, timestamp, body, message state, and selection
   have insufficiently distinct visual weight. There is no compact local visual
   anchor per author, so a large terminal can make the main conversation look
   like ungrouped terminal output.
4. The composer appears only as a five-row overlay after an action. It is
   functional but does not provide the obvious, stable writing box expected of
   a conversation workspace.
5. A context pane reserves substantial width even when it contains little
   useful context. Conversely, the main message measure can become too wide
   for comfortable reading.
6. Keyboard power is present but not visible at the moment it is needed. For
   example, Inbox is discoverable through the leader/help but has no calm,
   route-relevant affordance in the everyday shell.
7. The existing background and border language are close to the desired calm
   terminal aesthetic, but semantic contrast must work without that particular
   background and in 16-, 256-, and true-colour terminals.

The comparison is limited to observable layout and interaction outcomes. It
is **not** a request to copy Concord source, strings, assets, configuration,
or tests.

## 2. Product decision

Build a bzz-owned **workspace shell** around four stable roles:

```text
Community directory | Channel directory | Conversation | Context (on demand)
```

Every role has an explicit purpose, selection state, and keyboard focus. The
conversation remains the visual priority. Context is helpful rather than
permanently expensive. The shell uses restrained borders, colour, spacing,
and typography to make content legible before adding decoration.

The default should feel good at 120×32 and remain useful on an ultrawide
terminal. It must keep the current mouse/keyboard parity, focus model,
semantic hit-map, terminal restoration, and offline/locked behavior.

## 3. Non-negotiable boundaries

This visual/usability release must:

- keep `MIT OR Apache-2.0`; add no GPL dependency;
- use only independently authored bzz code, tests, prose, visual tokens, and
  fixtures—no Concord code, strings, assets, screenshots, or derived tests;
- retain the pinned Buzz/slk revisions and all existing protocol behavior;
- keep `#![forbid(unsafe_code)]`, secret/keychain boundaries, profile
  isolation, NIP-49-only credential transfer, and locked-mode guarantees;
- create no authoritative message store, global all-channel unread feed, or
  unbounded query/refresh path;
- keep Inbox as its existing scoped derived projection, with explicit read
  acknowledgement only;
- preserve draft-only Codex output and the human-send boundary; and
- avoid any new network request solely for presentation. Rendering is derived
  from already-authorized local state; an explicit existing refresh action
  remains the only refresh path.

Out of scope: Discord-specific guild/presence/voice features, remote
profile-picture fetching or arbitrary image URLs, a background image asset, a
desktop/web rewrite, rich message editing, and changing the release/profile
migration flow. Locally derived, non-network avatar markers are in scope.

## 4. Experience contract

### 4.1 Community and channel directories

- Replace the initial-only community rail with a compact **community directory**
  that renders sanitized labels, a stable active marker, and a bounded unread
  indicator derived from existing state. It remains keyboard-selectable and
  scrollable with stable community IDs.
- Preserve fast switching (`Space Space`) and numeric focus. A directory is a
  visible navigation aid, not a second community data source.
- Split the channel directory into visually labelled local sections only when
  they are backed by existing validated facts (for example joined channels and
  visible DMs). Do not infer fake categories or alter membership.
- Make active, focused, selected, unread, muted, direct-message, and private
  states distinguishable without relying on colour alone. Keep labels
  sanitized and preserve existing visibility/membership fences.
- Keep list rows comfortably targetable by mouse. Empty and loading states
  explain what is missing without suggesting a network action that cannot run
  in locked mode.

### 4.2 Conversation surface

- Add a compact channel header with the sanitized channel/DM label and a
  small, truthful context summary. It must not leak membership or identity
  data the active view is not authorized to display.
- Give each author a compact locally derived avatar marker. It is deterministic
  from the public key, has a text/shape fallback, and never fetches a profile
  picture or causes a network request. It complements—not replaces—the
  sanitized author label, so identity is never communicated by colour alone.
- Use a bzz-owned message rhythm: clearly weighted author, subdued timestamp,
  readable body, compact reaction/attachment metadata, and deliberately
  separated message groups. Same-author grouping and date separators may be
  used only if their ordering remains deterministic and unambiguous.
- Give selected and pending/rejected/deleted messages visible, semantic
  treatment; selection remains presentation-only and never changes read
  state.
- Introduce a configurable bounded message measure for very wide timelines.
  Content must wrap predictably, preserve Markdown/media semantics, and never
  reduce the usable reading width below a documented minimum.
- Reserve a visible writing dock at the bottom of an eligible conversation.
  Its inactive state names the safe activation key/click target; its active
  state is a real bounded multiline box, grows up to a documented height, and
  shows the exact validated channel/thread target, attachment state, and
  send/review state.
- The dock accepts text only after explicit activation (`i` or a primary click)
  so normal navigation remains safe. In locked, offline-without-an-identity,
  restricted, or missing-credential states it remains visibly disabled with a
  concise reason and never pretends a send is possible. It reuses the ordinary
  existing draft/composer and human-send path; it is not a second draft store.

### 4.3 Context and Inbox

- Context opens only for a validated thread/context selection or an explicit
  user toggle. It defaults closed when there is no useful detail.
- Give context an explicit close/collapse affordance and preserve independent
  selection/scroll position. A narrow terminal shows context as a route-local
  surface rather than a crushed fourth column.
- Keep Inbox as a first-class personal workspace. Align its list/detail
  hierarchy, empty/loading states, filter visibility, and return behavior with
  the new shell without changing its eligibility or acknowledgement rules.

### 4.4 Discoverability and feedback

- Make the status line concise and stateful: mode, connection/locked state,
  the one most relevant next action, and a stable help entry point. Do not put
  transient errors, secrets, full identifiers, or a long key cheat-sheet in
  every frame.
- Improve `?` help and the leader/which-key presentation so route-local
  actions (including Inbox) are visible before memorization is required.
- Preserve input priority: leader/overlay/composer/filter/focused route/global
  actions. Printable composer text must never become a shortcut.
- Maintain visible focus and pointer targets from the completed semantic hit
  map only. No geometry is recreated independently during dispatch.

## 5. Layout model

Replace the current fixed `4 | sidebar | timeline | thread` partition in
`src/ui/layout.rs` with an independently designed, role-aware solver.

| Role | Wide default | Safe bounds | Narrow behavior |
|---|---:|---:|---|
| Community directory | 18–28 cells | 14–32 | hides first; switcher remains available |
| Channel directory | 22–34 cells | 18–48 | hides after community directory |
| Conversation | remainder | at least 48 cells | sole workspace surface |
| Context | 30–44 cells | 28–56 | opens as focused local surface, never a squeezed column |
| Writing dock | 3 rows inactive; 5–12 active | 3–12 rows | retains an activation affordance; never overlaps content |

Exact defaults follow measurement in M1; these are product bounds, not a
promise of a particular pixel layout. User-resized side-pane widths remain
private local UI configuration, clamp safely after terminal resize, and must
not change membership, reads, or network scope.

The solver produces one measured layout for rendering and hit-map generation.
It must reserve status and writing-dock space first, then resolve visible roles
in priority order. The renderer must not assume a background image or colour
capability.

## 6. Implementation milestones

### M0 — Baseline and measurable visual contract

1. Capture bzz-owned deterministic fixture screens for wide, medium, narrow,
   16-colour, 256-colour, true-colour, transparent, locked, offline, Inbox,
   DM, thread, media, enabled/disabled writing dock, avatar fallback, and
   empty states. Do not commit external screenshots or user message content.
2. Add a small visual-review rubric: hierarchy, selection discoverability,
   unread distinction, reading measure, resize behavior, help discoverability,
   and no unauthorized data in chrome.
3. Record frame-time/redraw, allocation, and terminal-output baselines. This
   starts from the known idle-redraw investigation rather than assuming a new
   chrome layer is free.

**Exit:** reviewers can compare bzz-owned fixtures before/after without using
Concord assets or data.

### M1 — Layout solver, local UI migration, and shell chrome

1. Introduce typed shell-layout roles and measured rectangles in `ui/layout`.
   Rendering, hit map, scroll viewport sizes, and mouse dispatch consume the
   same output.
2. Evolve `UiConfig` conservatively for community/message-width preferences.
   Missing fields receive safe defaults; malformed/out-of-range values fail
   configuration validation before raw mode. Existing sidebar/thread widths
   migrate without silently widening a user layout.
3. Implement the community and channel directory shells, contextual headers,
   writing-dock measurement/placement, status-line composition, and responsive
   hide/restore behavior.
4. Retain `1`–`4`, Tab, h/l, mouse selection, leader actions, and the current
   route/focus state model. Update help as part of the same change.

**Exit:** layout snapshots cover every visibility threshold; keyboard and
mouse targets are identical measured regions; old valid config continues to
load.

### M2 — Message readability and context economy

1. Refactor timeline row measurement into a bzz-owned presentation model that
   can render deterministic local avatar markers, message groups, date
   dividers, a bounded text measure, selection, and state labels without
   changing `Message`/store semantics.
2. Render compact metadata and attachments while preserving media bounds and
   safe Markdown sanitization. Inline image placement continues to use actual
   measured rows.
3. Make context adaptive and independently scrollable; preserve the exact
   NIP-10 thread root and authorization checks when opening it.
4. Apply the same spacing, headings, focus, and empty/loading treatment to
   Inbox list/detail without changing its data model or read actions.

**Exit:** arbitrary Unicode/Markdown/media fixtures have correct measured
height, no clipping, stable selection after new events, and readable content
at all supported widths.

### M3 — Theme semantics and accessibility across terminals

1. Define bzz-owned semantic tokens for chrome, active/focused state, primary
   text, muted metadata, unread, status, and destructive/pending states. Map
   existing built-in palettes and custom `theme.toml` overrides to them with
   backwards-compatible fallbacks.
2. Improve contrast warnings to evaluate critical foreground/background pairs
   where both colours are known. Transparent-terminal themes remain supported;
   warnings must be advisory and never guess the terminal wallpaper colour.
3. Ensure every meaning expressed in colour also has a textual, weight, or
   marker cue. Test 16-colour fallback, no-colour/terminal-default behavior,
   and monochrome borders.

**Exit:** theme validation is bounded and safe; the default and an overridden
transparent theme pass the visual rubric without a particular image background.

### M4 — Usability verification and redraw discipline

1. Implement a dirty/event-driven draw gate: redraw after terminal input,
   resize, domain/UI state change, explicit refresh, or a bounded animation;
   do not redraw merely because the old 100-ms tick fired with no visible
   change. Preserve liveness and terminal restore behavior.
2. Measure render/cache work separately. Cache layout/message presentation only
   with generation- and width-aware invalidation; never cache untrusted escape
   output or stale hit geometry.
3. Run keyboard-first and mouse journeys: community switch, channel switch,
   unread selection, thread open/close, compose/cancel/send boundary, Inbox
   filter/detail/mark-read, search, resize, locked startup, reconnect, and
   terminal recovery.
4. Run the release binary through existing isolated Herdr acceptance scenarios
   using disposable data only; do not automate identities or passphrases.

**Exit:** the release binary has an observed idle redraw reduction with no
lost input, stale hit target, scroll jump, unexpected network refresh, or
terminal-restoration regression.

### M5 — Documentation, audit, and release decision

1. Update configuration, themes, README keymap/help, manual E2E, and
   troubleshooting documentation with user-facing visual behavior and narrow
   layout behavior.
2. Add an ADR describing the clean-room visual-language boundary and the
   layout/semantic-token decision; link this plan.
3. Run format, clippy, unit/property/snapshot tests, integration tests,
   `cargo deny check`, `cargo audit`, benchmarks, release binary smoke tests,
   and the opt-in pinned relay test. Keep existing documented advisory policy.
4. Produce a release note with before/after bzz-owned captures and measured
   redraw evidence, not third-party comparison art.

## 7. Architecture and test ownership

Expected bzz-owned seams (names may differ):

```text
ui/layout.rs        role-aware responsive shell measurements
ui/shell.rs         shell/chrome models and rendering
ui/sidebar.rs       community/channel directory row presentation
ui/timeline.rs      measured conversation presentation and grouping
ui/inbox.rs         Inbox visual adapter only
ui/theme/*          semantic visual tokens and validation
ui/hit_map.rs       generation-bound targets from measured layout
app.rs              route/focus/effect wiring; no duplicate geometry
```

Tests must include:

- pure layout/property tests for widths, heights, bounds, and hide/restore
  decisions;
- snapshots using synthetic bzz-owned messages only;
- deterministic selection/scroll reconciliation when events arrive, resize, or
  a pane opens/closes;
- keyboard/mouse parity and stale hit-map rejection;
- config parsing/migration and theme fallback/contrast warnings;
- locked/offline tests proving chrome does not request network data;
- avatar determinism/fallback tests that prove no profile image URL is read or
  fetched;
- composer-dock activation, disabled-state, resize, Unicode-cursor, attachment,
  mention, and human-send-boundary tests;
- media/Markdown measurement and sanitization regression tests; and
- redraw-gate tests plus a benchmark for idle CPU/terminal output and a
  timeline rendering benchmark at realistic bounded message counts.

## 8. Acceptance criteria

The release is ready only when all of the following are demonstrated:

1. A new user can identify the active community, channel/DM, focused surface,
   unread work, and available help within one screen without opening a modal.
2. A keyboard-only user can reach Inbox, switch community/channel, inspect and
   close context, activate the visible writing box, compose, and return without
   a mouse or hidden global state. A mouse user can activate the same box from
   its generation-bound hit target.
3. On wide terminals, message content remains readable and the context pane
   does not waste material conversation space; on narrow terminals no pane is
   unusably thin.
4. Selection, unread/read behavior, membership, DM visibility, and Inbox
   acknowledgements retain their v0.4 semantics exactly.
5. Custom/transparent themes, 16-colour terminals, and no-colour fallback
   retain focus, avatar, and status meaning without relying solely on colour.
6. Locked/offline behavior renders only authorized local state and makes no
   presentation-driven network requests.
7. Idle terminal output/CPU is materially lower than the v0.4 baseline, while
   rendering remains responsive to input, resize, live events, and media.
8. All security, licensing, pinning, protocol, audit, test, and release gates
   remain green.

## 9. Decisions needed before implementation

1. **Community directory default:** full labels by default (recommended) or an
   icon/initial rail with a labelled expansion? The captures strongly support
   full labels.
2. **Conversation measure:** bounded readable column by default (recommended)
   or use every available timeline cell? The former is more legible on the
   supplied ultrawide screen.
3. **Date/group treatment:** compact separators plus same-author grouping
   (recommended) or a full header on every message? The former is calmer but
   needs careful deterministic testing.
4. **Scope:** make this a focused `v0.5.0` usability release (recommended),
   rather than mixing it with new protocol features.
