# bzz v0.4.0 — Interaction Foundation and Conversational Inbox

**Status:** Approved and active.

**Target:** `v0.4.0`

**Compatibility baseline:** Rust 1.95 / Rust 2024; Ratatui 0.30;
Crossterm 0.29; the existing SQLite cache; Buzz
`ede26863345a518ec46edd6d7692e0281883491b`; and `slk`
`8149c3b18ed04c259efe5feb545d040ab043d922`. The pinned third-party
revisions remain unchanged.

## 1. Product decision

`v0.4.0` is a deliberate interaction refactor, not an Inbox-only increment.
It makes bzz a coherent keyboard-first terminal workspace with an Inbox that
is a personal, conversation-oriented work queue.

The interaction design is informed by the observable behavior of Concord:
explicit focus, Vim-style navigation, leader sequences, contextual actions,
modal input ownership, independent selection and scrolling, and keyboard/mouse
parity. The Inbox data model is informed by Buzz Desktop: stable conversation
identity, latest activity, first unread anchor, per-conversation windowing, and
read-state-aware detail.

### 1.1 Licensing boundary

Concord is GPL-3.0 while bzz is `MIT OR Apache-2.0`. This release must be a
clean-room implementation:

- do not copy, translate, adapt, vendor, link, or depend on Concord source,
  tests, comments, strings, configuration samples, or assets;
- do not change bzz's licence or add a GPL dependency;
- implement bzz-specific typed state, actions, parser, renderer, tests, and
  documentation from this behavior-level specification only; and
- retain bzz's stronger generation-bound semantic hit map instead of copying
  Concord's geometry approach.

### 1.2 Approved interaction decisions

These are intentionally breaking default bindings; conflicting legacy aliases
are not retained.

1. **Concord-style defaults.** `Space` is the leader, `q` is contextual
   back/quit-with-confirmation, `Ctrl-p` means previous selection, and the
   default focus/navigation family follows this plan.
2. **Inbox is a workspace.** It is not a small notification popup. It has a
   conversation list, a real detail pane, and the ordinary composer.
3. **The keymap is configurable in v0.4.0.** A separate, non-secret
   `keymap.toml` supports bindings, sequences, and scoped overrides, and is
   validated before bzz enters the terminal.
4. **Inbox is personal and actionable.** It is not an all-channel unread feed.
   Eligible conversations are visible DMs, direct mentions, threads the active
   identity has joined or owns, supported needs-action cards addressed to the
   active identity, and nonempty local drafts.
5. **Opening is not acknowledgement.** Selecting or opening a detail never
   silently advances NIP-RS state. `m`, the contextual action, and a confirmed
   visible-row bulk action are the only ways to mark Inbox work read.
6. **Reply in place.** `i` in Inbox opens the ordinary composer targeted at the
   selected conversation. `o` remains the explicit escape to canonical channel
   context.
7. **Draft-only work belongs only in `Drafts`.** `All` excludes a conversation
   that exists solely because of a draft. A conversation that is independently
   actionable may display its draft badge in `All` and also be found in
   `Drafts`.

### 1.3 Explicit non-goals

The following remain out of scope:

- copying Concord code or changing bzz's licence;
- Discord-specific features with no bzz equivalent (guild folders, presence,
  voice, external accounts, arbitrary `$EDITOR` execution, or platform
  clipboard integrations);
- relay-visible agents, NIP-OA/ACP, MCP, autonomous posting, Codex write
  access, or changing the draft-only/human-send boundary;
- creating a new message store, an unbounded Inbox scan, or an upstream relay
  feed extension;
- global "all unread channel messages" triage, projects, reminders, workflow
  approval buttons, or new relay workflow actions;
- changing Buzz/slk pins, message protocol compatibility, membership fences,
  secure-media policy, identity isolation, or offline behavior.

## 2. User-facing interaction contract

### 2.1 Routes, focus, and layout

The TUI has a route, focused surface, optional overlay, and optional composer
instead of one overloaded `Mode` enum. The normal workspace has these focus
surfaces:

```text
1 Communities | 2 Channels / DMs | 3 Timeline | 4 Thread / context
```

- `1`, `2`, `3`, and `4` show and focus the named surface when it is available.
  `4` shows the context pane before focusing it. Narrow layouts omit side panes
  rather than rendering unusably narrow columns.
- `Tab`/`Shift-Tab`, `h`/`l`, and Left/Right traverse visible focus surfaces.
- `j`/`k`, Up/Down, and `Ctrl-n`/`Ctrl-p` move the focused selection.
- `J`/`K` scroll a focused viewport without changing selection. `Ctrl-d`/
  `Ctrl-u` scroll by a half page and `gg`/`G` select the first/last item.
- `Alt-h`/`Alt-l` (and equivalent configured bindings) resize a focused side
  pane inside safe min/max bounds. Visibility and widths are local UI state.
- `i` opens the composer for the currently valid conversation. `/` opens the
  filter/search suitable to the focused surface. `Esc` and `q` unwind the
  foremost owned input, overlay, or return route before offering quit.

A focus change cannot fabricate a channel, alter membership, mark content read,
or publish anything. It changes presentation state only.

### 2.2 Leader, actions, and help

`Space` opens a transient leader/which-key overlay in non-text input states.
The overlay displays only valid next chords for the current route and focus,
and Escape cancels it. A prefix times out after a short documented interval
without executing a partial action.

Builtin defaults include:

| Binding | Action |
|---|---|
| `Space Space` | channel/DM switcher |
| `Space n` | open Inbox workspace |
| `Space a` | actions for focused surface/row |
| `Space 1` / `Space 2` / `Space 4` | toggle Community / Channel / Context panes |
| `Space r` | request a local redraw / refresh eligible view data |
| `Space o` | options/theme configuration |
| `?` | show generated effective-keymap help |

`Space a` is the discoverable home for contextual actions. It lists enabled
operations first and unavailable operations with a concise reason. It replaces
scattered single-letter affordances, while direct message-level bindings remain
only where explicitly documented. Selection alone never invokes an operation.
Destructive actions retain the current confirmation and authorization checks.

### 2.3 Composer contract

Composer input has priority over normal and leader shortcuts. A literal space,
`j`, or any other printable character always inserts text while composing.

- `Enter` submits only after existing body/attachment validation; `Ctrl-j` and
  `Alt-Enter` insert a newline.
- `Ctrl-w`, `Ctrl-u`, `Ctrl-k`, `Ctrl-Left`, `Ctrl-Right`, Home, and End perform
  bounded Unicode-safe editing.
- Existing attachment, mention, draft, and local-agent review behavior remains
  intact. Local Codex output is still unpersisted until the user deliberately
  places it in the ordinary composer, and normal human send remains the sole
  publication path.
- Composer completion and pickers own input before the composer itself. Escape
  closes a picker before discarding/closing composer state.

No v0.4 keybinding invokes a shell, opens an external program, or sends data to
an external service outside existing user-approved operations.

### 2.4 Mouse contract

Mouse behavior is a peer of the keyboard contract, resolved solely against the
latest completed `HitMap` generation.

| Surface | Primary click | Double click | Wheel |
|---|---|---|---|
| Community/Channel panes | focus and select; channel/DM activates | activate where meaningful | scroll viewport only |
| Timeline/Thread | focus and select event | open thread/context where meaningful | scroll viewport only |
| Inbox list | focus and select conversation | open/toggle detail on narrow layout | scroll Inbox list only |
| Inbox detail | focus detail | no implicit acknowledgement | scroll detail context only |
| Composer | focus and position cursor | no special operation | no action |
| Owned overlay/menu | select its item/control | same as Enter where meaningful | scroll that overlay |

Overlays own pointer input and block the background. Unknown buttons, drag
selection, horizontal scroll, stale hit maps, and empty space are no-ops. The
renderer produces the layout measurements and hit targets in one frame; event
dispatch never recreates approximate geometry.

## 3. Keymap configuration and validation

### 3.1 File and precedence

`keymap.toml` lives beside bzz's normal local configuration but is independent
from credentials and `config.toml`. Missing `keymap.toml` selects the builtin
v0.4 defaults. It is owner-private like other local configuration files, but it
contains no secret and must never be logged verbatim.

Resolution order is:

1. builtin defaults;
2. user global overrides;
3. route/focus/overlay/composer scoped overrides; and
4. a binding explicitly disabled by the user.

A scoped binding augments rather than silently steals a text character from a
composer or filter. Text-owning states may only bind the documented editing and
completion action set.

### 3.2 Typed model

The implementation defines bzz-owned `UiAction`, `KeyChord`, `KeySequence`,
`KeyScope`, and `KeyMap` types. A trie resolves root and prefix sequences; it
does not parse action names dynamically at dispatch time. Actions are typed and
map to a pure UI intent or a typed application effect.

Validation rejects, with an actionable file/field diagnostic:

- unknown keys, modifiers, action names, scopes, or fields;
- empty sequences, sequences longer than four chords, malformed special keys,
  duplicate effective bindings, and prefix/action ambiguity;
- bindings that intercept printable input in a text-owning state;
- bindings reserved for terminal recovery or unsafe to express on supported
  terminals; and
- oversized files, lines, string values, or a keymap exceeding a fixed action
  and sequence count.

`bzz check` validates both configuration files without opening the TUI. A bad
keymap fails closed before raw mode; it never partially applies a map.

## 4. Architecture and ownership

### 4.1 State model and reducer boundary

The `App` currently contains network runtime, data caches, selection, scroll,
all overlays, render details, and a large `Mode` key-handler cascade. v0.4
separates it into bzz-owned modules:

```text
ui/
  action.rs       UiAction, UiEffect, context/action availability
  input.rs        keyboard priority router, sequence state, mouse dispatch
  keymap.rs       typed defaults, TOML parser, validation, trie lookup
  state/          Route, focus, viewport, composer, overlay, inbox state
  layout.rs       responsive geometry and measured row layout
  actions.rs      contextual action menu model
  inbox.rs        Inbox route views and interaction adapter
```

Names may vary, but the invariants may not:

- **Route** selects a persistent workspace (`Channel` or `Inbox`) and remembers
  a return target.
- **Focus** selects a navigable visible surface within the route.
- **Overlay** is a mutually exclusive owned popup/menu/confirmation; it cannot
  be confused with a persistent route.
- **ComposerTarget** identifies the validated channel/root for an active draft;
  it is not a display mode.
- Every list/viewport independently owns selection identity, scroll position,
  horizontal scroll if applicable, viewport dimensions, and
  keep-selection-visible policy.
- Selection uses stable IDs, never a row index, across event insertion,
  filtering, rendering, and background reconciliation.

Input processing has this fixed priority:

```text
active leader sequence
  > overlay
  > composer completion/picker
  > composer
  > focused filter
  > route/global keymap
  > no-op
```

A key or mouse event yields `UiAction`s. The reducer mutates local presentation
state and emits typed `UiEffect`s. A runtime executor alone owns services,
SQLite requests, signer, relay supervisor, HTTP, media, and task lifecycle.
Async results return as typed domain/background events and are reconciled by
the reducer. No UI action may receive a signer, an HTTP client, an unrestricted
command runner, or raw secret material.

The refactor is incremental but never runs two conflicting normal-key routers.
An adapter may bridge an old service operation while its state is migrated;
when a route is cut over, its old `Mode` branch is removed in the same change.
At completion, `Mode`, `Pane`, `awaiting_g`, and the global normal-key cascade
are deleted.

### 4.2 Rendering, metrics, and redraws

Layout is computed once per completed frame and shared by painting, viewport
clamping, hit-map regions, and cursor placement. Variable-height Markdown and
media retain their real measured rows; no event handler assumes one terminal
row per message.

A bounded layout/row cache is keyed by stable event identity plus width, theme,
render-relevant media generation, and source route. Resize, theme change,
content change, and media completion invalidate only affected entries. Cached
presentation data contains sanitized render text/metrics only, never private
keys, raw secrets, or agent prompts/results.

Foreground terminal input redraws immediately. Background updates redraw only
when they affect the visible route, focused selection, overlay, status, or media
rendering. Existing terminal restoration, raw-mode, mouse-capture, and panic
rollback guarantees remain mandatory.

### 4.3 Contextual actions

The action registry derives availability from visible, validated state rather
than dynamically executing a command string. Examples:

- channel: open, compose, filter, create/hide/add DM only where allowed;
- event: reply, open thread/context, react, delete own event, media preview;
- Inbox list/detail: reply, open canonical context, mark read/unread, mark
  visible rows read, filter, restore draft;
- a locked, hidden, deleted, foreign, missing, or needs-action card is rendered
  unavailable rather than widened, guessed, or acted upon.

The registry is the single source for action menu labels, generated help,
keymap validity, mouse activation, and reducer dispatch. Publishing actions
still pass the ordinary service-level validation and confirmation boundary.

## 5. Conversational Inbox contract

### 5.1 Projection and identity

Inbox remains a local derived projection, never a second authoritative message
store. Source truth stays in community-partitioned events, drafts, membership,
DM visibility, read contexts, and Inbox overrides.

Each row has an immutable conversation ID:

```text
dm:<channel-uuid>
thread:<validated-NIP-10-root-event-id>
event:<event-id>
draft:<channel-uuid>                 # draft-only root draft
```

For every conversation, the projection retains at least:

- latest activity event and timestamp;
- first unread event/anchor and unread count;
- representative preview/sender/category set;
- channel/root context and validated reply target;
- local draft count and latest draft time; and
- local forced-unread/done override state.

A selected conversation is retained by `conversation_id`, even when a new
event replaces its representative event. Message/event IDs are deduplicated by
`(community_id, event_id)` before projection. The projection is scoped to the
active community and active identity; it must never join or display data across
those boundaries.

### 5.2 Categories and read model

`All`, `Mentions`, `Threads`, `DMs`, `Needs action`, `Unread`, and `Drafts`
remain available as filters. They operate on stable conversations, not raw
message rows.

- Visible DM activity, direct mentions, participated/owned threads, and
  addressed supported cards are actionable candidates.
- A broadcast message in an otherwise unrelated channel is not Inbox work.
- `Unread` uses the maximum valid channel/thread/message NIP-RS context plus
  a local forced-unread override. It never lowers a monotonic remote marker.
- `m` marks the selected conversation read through the narrowest valid context;
  it does not mark a whole unrelated channel. `U` is local forced-unread only.
- Bulk mark-read applies only to the current visible eligible rows and asks for
  confirmation when it would affect more than one row.
- Needs-action cards remain informational. They never create an approval,
  signing, or relay workflow path.

### 5.3 Bounded ingestion, reconciliation, and pagination

No new Buzz relay endpoint is assumed. Online refresh keeps the existing
bounded mention and needs-action queries; DMs, threads, backfill, reconnect,
outbox echo, and realtime subscriptions feed the same local event store.

Migration `0005_inbox_conversations.sql` introduces a rebuildable, derived
Inbox projection and bounded indexes/cursors sufficient to query by latest
activity and stable conversation ID. It contains no message body beyond the
already bounded sanitized preview necessary for the current row, no signer
material, and no new network state. A rebuild from existing local data is
idempotent, transactionally scoped, and safely falls back to a bounded
on-demand rebuild after interrupted migration.

Reconciliation is bounded:

- source scans and draft scans have documented hard caps;
- candidates are grouped by conversation before final row limits;
- each conversation contributes a bounded recent event window so a noisy DM or
  thread cannot starve other conversations;
- pages use a local lexicographic cursor
  `(latest_activity_at DESC, conversation_id ASC)` rather than raw-event
  offset pagination;
- source event query cursors must strictly advance and retain the existing
  safety caps/error behavior; and
- local rows remain usable offline or locked. Locked mode performs no refresh,
  network request, signer use, or implicit identity replacement.

### 5.4 Inbox workspace behavior

Wide terminals render a focusable list and detail pane. Narrow terminals show
list or detail as a route-local screen; Enter activates detail, and `Esc`/`q`
returns to the list before leaving Inbox. The selected conversation and scroll
anchor persist while switching responsive layouts.

Detail renders the selected conversation's bounded context around its stable
unread/selection anchor. It has enough local context to understand and reply,
but it does not pretend to be a new full message cache. `i` targets the normal
composer directly at the validated channel/root. Sending uses the ordinary
outbox, media, mention, NIP-10, acknowledgement, and human-approval paths.
`o` explicitly opens the canonical channel/thread, preserving an Inbox return
target. Missing, deleted, unauthorized, or hidden context fails visibly and
does not navigate to a substitute.

## 6. Data migration and compatibility

- Migration `0005` follows the existing checksum, transaction, backup, and
  pre-migration backup rules. There is no downgrade migration; restoration
  uses the existing backup process.
- Existing `inbox_overrides`, read contexts, DM visibility, drafts, event
  deletion, outbox delivery state, and active membership checks remain the
  authoritative inputs. The projection is invalidated/rebuilt after changes to
  any of them.
- Unknown/malformed local conversation IDs, roots, category state, or legacy
  rows are ignored or rebuilt; they must never broaden visibility or crash the
  TUI.
- The current protocol compatibility document is updated only to describe local
  behavior. No event kind, tag shape, relay command, or pin changes for Inbox
  are introduced.

## 7. Implementation milestones

### M0 — Contract, clean-room record, and baselines

- Add this plan and an ADR/implementation note recording the Concord GPL
  boundary and behavior-only inspiration.
- Add a public v0.4 tracking issue and a local implementation TODO.
- Capture current strict test, Clippy, audit, deny, benchmark, and pinned-relay
  baselines.
- Add failing/characterization tests for current route transitions, terminal
  restoration, selected-row stability, and Inbox read semantics.

**Exit gate:** the team can review the breaking keymap, explicit read contract,
Inbox scope, and licensing boundary before a user-facing default changes.

### M1 — Typed input, state, and default keymap foundation

- Introduce typed actions/chords/scopes, builtin defaults, a sequence trie, and
  `keymap.toml` parsing/validation without changing services or protocol pins.
- Add state modules for route/focus/overlay/composer/viewport and a pure action
  reducer/effect boundary.
- Move one vertical slice (normal navigation plus help/leader) to the new
  router, then remove its matching old normal-mode path.
- Implement generated effective-keymap help and which-key rendering.

**Exit gate:** default global navigation, a configurable sequence, malformed
keymap rejection, text-input priority, and Escape cancellation are fully tested
without terminal I/O.

### M2 — Workspace layout, navigation, and action registry

- Move Community, Channel, Timeline, and Thread focus/selection/scroll state
  to independent viewports with stable IDs.
- Implement responsive visibility, resize persistence, measured layout cache,
  focus traversal, detached scroll, and keyboard/mouse parity.
- Implement the typed contextual action registry/menu and migrate direct
  message/channel operations through it.
- Remove the old `Pane`, `awaiting_g`, and remaining normal navigation mapper.

**Exit gate:** variable-height/media timelines, resize, mouse, selection,
scroll, filters, and canonical message operations remain correct in wide and
narrow layouts.

### M3 — Inbox projection, migration, and bounded reconciliation

- Add migration `0005`, projection rebuild/update APIs, indexes, stable
  conversation identity, first-unread anchors, and activity cursor pagination.
- Adapt Inbox service/realtime/backfill/outbox hooks to invalidate or reconcile
  the local projection under bounded work budgets.
- Enforce actionable inclusion, drafts-only-in-Drafts, access/identity fences,
  explicit acknowledgement, and offline/locked behavior.
- Benchmark rebuild, update, first page, and deep pagination with noisy
  conversations and large local datasets.

**Exit gate:** Inbox rows are stable, bounded, rebuildable, cross-community
safe, and do not miss a quiet conversation merely because another one is busy.

### M4 — Inbox workspace and in-place reply

- Replace the Inbox modal with route-local list/detail layouts using common
  focus, viewport, action, mouse, and keymap primitives.
- Render bounded context/unread anchor; retain selection through updates and
  responsive transitions.
- Implement `i` in-place reply, `o` canonical context with return target, and
  explicit/bulk read actions through the existing read-state and outbox paths.
- Remove the old Inbox `Mode` branch and duplicate render/input handling.

**Exit gate:** a user can triage, read, reply, return, and resume a selected
conversation without accidental marking, publication, identity crossing, or
loss of draft.

### M5 — Cutover, hardening, and release

- Migrate remaining overlays/pickers/composer to owned-input routing; delete
  the `Mode` enum and monolithic handler only after every route is cut over.
- Update README, configuration, Inbox/DM/search, security, troubleshooting,
  themes/help, manual E2E, and release documentation in English.
- Run fuzz/property tests, snapshot tests, benchmarks, full strict CI,
  `cargo deny check`, `cargo audit`, real pinned-relay integration, release
  artifacts, SBOM, provenance, checksums, and installer smoke tests.

**Exit gate:** bzz v0.4.0 is interaction-coherent, release-quality, and retains
all existing human-first security and protocol invariants.

## 8. Acceptance criteria

### Interaction and keymap

- [ ] Default `Space` leader, `q`, focus, Vim movement, selection/scroll split,
      `gg/G`, half-page, resize, and contextual actions work as documented.
- [ ] Key sequences are scoped, bounded, cancellable, discoverable, and never
      steal printable composer/filter text.
- [ ] A malformed/conflicting/oversized `keymap.toml` fails before terminal
      setup with a non-secret actionable diagnostic.
- [ ] Generated help and which-key expose the effective user configuration,
      including disabled/conflicting unavailable actions.
- [ ] Every pointer action has a safe keyboard equivalent and uses only the
      latest semantic hit-map generation.
- [ ] Route/focus/overlay/composer state cannot be confused, and all overlays
      block background input.

### Inbox

- [ ] Rows are stable conversations, not raw messages; representative updates
      retain selected conversation and viewport anchor.
- [ ] Inbox includes only personal actionable work and visible local drafts,
      preserves community/identity/membership/DM visibility fences, and never
      becomes an all-channel unread feed.
- [ ] Draft-only rows appear only in `Drafts`; draft badges on independently
      actionable rows are accurate.
- [ ] Opening detail does not mark read. Explicit and bulk actions are narrow,
      monotonic, confirmed where appropriate, and idempotent.
- [ ] Wide/narrow list-detail navigation, in-place reply, canonical-context
      return, missing context, offline cache, and locked mode behave safely.
- [ ] Candidate scans, event windows, cursor pages, rebuilds, and network
      refreshes have hard bounds and strict cursor-progress checks.
- [ ] A migration interruption, malformed local projection state, and a stale
      background result recover without data exposure, crash, or duplicate row.

### Security and compatibility

- [ ] No code path adds a Concord dependency/code fragment or changes bzz's
      MIT/Apache licence.
- [ ] The refactor does not give UI actions a signer, relay client, HTTP client,
      media uploader, agent runner, arbitrary shell, or secret-bearing config.
- [ ] Existing outbox acknowledgement, NIP-10, media, mention, read-state,
      locked-mode, and local Codex draft-only tests remain green.
- [ ] Buzz and slk pins are byte-for-byte unchanged from the compatibility
      baseline.

## 9. Test and benchmark strategy

v0.4 uses a test pyramid. Concord's observable testing approach informs the
separation of input, state, rendering, and async updates, but no Concord test
source, fixtures, strings, or helpers may be copied because it is GPL-3.0. All
bzz test code and scenario data are independently authored.

### 9.1 Hermetic CI layers

| Layer | Coverage |
|---|---|
| Unit | Chord parsing/trie lookup, scope precedence, disabling, collision rejection, sequence cancellation, reducer transitions, focus cycles, scroll/selection split, action availability, explicit read decisions, stable conversation reconciliation. |
| Property/fuzz | Arbitrary valid terminal key streams cannot steal text input or panic; sequence tries remain bounded; generated Inbox event/draft orders retain stable IDs, caps, and visibility fences. |
| Functional UI harness | Independently authored table-driven journeys inject key/mouse events into the router and reducer, record typed effects, and render through `ratatui::TestBackend`. It has fake/recording service effects only: no signer, real HTTP, relay, external process, or terminal is required. It proves, for example, that a leader sequence opens Inbox, a composer inserts literal text, a failed prefix is consumed, opening detail does not mark read, and an in-place reply emits only the ordinary user-approved send effect. |
| Store/migration | `0005` checksum/backup/rebuild, interruption recovery, identity/community partitioning, cursor order, event-window fairness, projection invalidation, read/override behavior. |
| UI snapshot | `TestBackend` buffer assertions/snapshots for which-key/help, action menus, focus highlights, wide/narrow workspace, Inbox list/detail/unread anchor, disabled actions, malformed-context error states, themes, and mouse-derived selection. Snapshots contain generated public fixtures only and assert no terminal control bytes. |
| Input/mouse | Latest-generation hit map, overlay ownership, click/double-click equivalence, viewport wheel behavior, wrapped Markdown/media, resize, detached scroll, terminal restoration, and no background action from stale/empty hits. |
| Relay integration | Pinned Buzz relay coverage for ordinary roots/replies/media/mentions, outbox acknowledgement, read state, DMs, Inbox refresh and access fences. No new protocol capability is assumed. |
| Benchmarks | Keymap lookup, action derivation, measured row/hit-map construction, projection rebuild/update, first/deep Inbox pages, and noisy-conversation fairness at realistic and adversarial local-store sizes. |

The functional UI harness is the deterministic CI authority for interaction
semantics. It has a small public interface around terminal events and typed
application effects; it must not duplicate renderer geometry, use sleeps, or
assert a network side effect from a UI action.

### 9.2 Herdr acceptance suite

Herdr validates what hermetic tests cannot: the release binary in a real
terminal/multiplexer, Crossterm input encoding, terminal restoration, and
visible end-to-end behavior. It is an acceptance/self-hosted gate, not a
replacement for unit/functional CI and not a bzz runtime dependency.

Add a bzz-owned, independently authored scenario runner (planned as
`scripts/test-tui-herdr.sh`) and versioned scenario manifest. Each scenario
must specify only:

- isolated config/data/cache directories, a release binary, and a disposable
  non-admin identity/community/channel;
- `send-keys` inputs, visible-output readiness predicates, and bounded
  timeouts; never `send-text` for mode keys;
- generated non-sensitive fixture text and expected visible labels; and
- authoritative postconditions from the local store and/or pinned relay, not
  merely terminal pixels.

The suite covers, at minimum:

1. startup/help/quit and restoration after normal/error exit;
2. default and customized `keymap.toml`, leader/which-key cancellation,
   focus traversal, narrow/wide resize, and composer text ownership;
3. mouse click, double-click, wheel, overlay ownership, and semantic row
   selection with wrapped/media messages;
4. Inbox list/detail transitions, no implicit acknowledgement, explicit/bulk
   read, in-place reply, canonical-context return, draft recovery, and offline
   or locked cache behavior; and
5. restart/outbox/relay acknowledgement verification using the existing pinned
   protocol journey.

Herdr scenarios may run only in a controlled self-hosted environment or by an
operator. They use no secret in arguments, ordinary environment variables,
fixtures, logs, screenshots, terminal automation, or repository files. Secret
prompts, if a local test setup requires one, remain manual operator input.
Failures retain sanitized visible output and structural evidence only.

### 9.3 Gates by milestone

- **Every change / ordinary CI:** format, strict Clippy, all deterministic unit,
  property, store, functional-harness, and TestBackend tests.
- **M1/M2 cutovers:** add an interaction journey before changing a default
  binding or focus/input-owner transition.
- **M3/M4 cutovers:** add migration/projection fairness coverage and a matching
  Inbox functional journey before schema or route removal.
- **Release candidate:** all ordinary CI, benchmarks, pinned relay integration,
  and the Herdr acceptance suite against the release binary. A skipped Herdr
  run is documented as a release exception rather than silently treated as
  equivalent coverage.

Mandatory release commands remain:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo deny check
cargo audit
cargo bench --bench timeline --bench store --bench media
./scripts/test-relay.sh
# Controlled self-hosted/manual environment only:
./scripts/test-tui-herdr.sh
cargo build --release --locked
```

## 10. Documentation and release deliverables

Before tagging `v0.4.0`, update:

- `README.md` for the new default interaction model and breaking key changes;
- `docs/configuration.md` with complete, bounded `keymap.toml` grammar and
  recovery instructions;
- `docs/inbox-dms-search.md` with the conversation/read/reply model;
- `docs/security.md` with clean-room, local-state, input, and no-external-shell
  boundaries;
- help/screenshots/manual E2E/troubleshooting for leader, focus, responsive
  routes, broken-keymap recovery, and terminal restore; and
- `docs/release-v0.4.0.md` with compatibility, migration/backup, and known
  non-goals.

The release uses the existing reproducible release workflow, SBOM, provenance,
checksums, attestations, installers, and GitHub release process. No tag is cut
until every acceptance criterion and mandatory validation gate has evidence.
