# bzz v0.3.0

`v0.3.0` adds terminal mouse interaction, offline channel-member `@` mention
completion, and an opt-in local Codex **draft** assistant. Protocol
compatibility remains pinned to Buzz `ede26863345a518ec46edd6d7692e0281883491b`.

## Highlights

- `ui.mouse = "auto" | "on" | "off"` enables semantic mouse interaction for
  community/channel rails, timeline and thread rows, composer, and selectable
  overlays. `off` preserves terminal text selection. Capture is restored on
  normal exit, startup rollback, and panic recovery.
- Cached channel members can be selected from an `@` picker without a network
  lookup. Accepted members are persisted as structured draft metadata and emit
  lowercase, deduplicated Buzz `p` tags for roots and replies.
- `:agent` opens a configured local Codex picker for the selected cached
  message. A result is an unpersisted review draft: accepting it inserts text
  into the ordinary composer, and only the normal human send action can
  publish.
- `bzz agent add/list/remove/doctor` manages non-secret local assistant
  profiles. The optional workdir is canonical and read-only; otherwise each
  run has a fresh owner-only scratch directory.

## Codex privacy boundary

A local assistant is not a Buzz user, bot, relay identity, workspace DM,
NIP-OA/ACP client, or autonomous publisher. It has no signer, relay/session
handle, membership, media uploader, or remote trigger.

bzz invokes an absolute local `codex` executable without a shell. It supplies
only fixed `codex exec` JSON/ephemeral/read-only flags, clears inherited
environment values (including `OPENAI_API_KEY`), sends the bounded prompt on
stdin, and accepts only a bounded completed `agent_message` JSONL record.
Prompts, output, stderr, and unapproved drafts are not persisted or logged.
A 120-second limit, cancellation, identity lock, community switch, and shutdown
discard the result and terminate the direct child.

Codex authentication and model inference are separately configured by the
user. Invoking it can therefore send the explicitly selected bounded context to
Codex's external service; bzz does not otherwise initiate that egress.

## Database migration

Schema version 4 adds the bounded local `drafts.mentions_json` column. It stores
only valid byte spans and lowercase public keys for draft mention reconstruction;
it never stores profile bodies, keys, prompts, or agent output. Existing
Databases receive the usual owner-only pre-migration backup. Downgrade by
restoring that backup; no reverse SQL is supplied.

## Validation

- formatting, warnings-as-errors Clippy, all tests, `cargo deny`, `cargo audit`,
  release build, benchmarks, and the pinned real-relay journey;
- fake-process tests verify fixed Codex flags, stdin-only prompt delivery,
  cleared API-key environment, output sanitization, bounds, cancellation, and
  scratch cleanup; no CI job uses a Codex account or sends a fixture to a model;
- mouse, mention, and real-Codex review remain documented manual E2E journeys.
  The real-Codex journey is intentionally optional because it requires the
  user's separately authenticated account and external inference egress.

See [`configuration.md`](configuration.md), [`security.md`](security.md),
[`troubleshooting.md`](troubleshooting.md), and
[`e2e-manual.md`](e2e-manual.md).
