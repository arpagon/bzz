# bzz v0.10.0 — Agent-foundation reset

> **Status: Release candidate.**

v0.10.0 removes bzz's one-shot local Codex reply drafter. That feature was a
human-reviewed composition helper rather than a Buzz managed agent, but its
module, configuration, CLI, and TUI all used agent terminology. Removing it now
creates an unambiguous foundation for separately planned managed-agent support.

## Removed surface

- The `src/agent/` Codex process wrapper and draft runtime.
- `bzz agent add|list|remove|doctor`.
- TUI `:agent`, local-assistant picker, running state, and draft-review overlay.
- `LocalAgentConfig`, labels/workdirs, and the unused Tokio process feature.
- Current README, configuration, security, troubleshooting, and E2E instructions
  for the retired drafter.

No replacement agent feature is included in this release.

## Upgrade behavior

Older configurations containing the retired top-level `[[local_agents]]`
section are accepted once, stripped, and rewritten atomically. The old values
are never executed. All other unknown configuration fields remain rejected.

There is no database migration. Message drafts, identities, communities,
attachments, diagnostics, telemetry, and cached relay data are unchanged.

## Managed-agent research

`docs/how-agents-works-in-buzz.md` records the existing Buzz Desktop architecture
at an upstream source snapshot. It distinguishes definitions, agent identities,
per-relay runtimes, ACP workers, per-channel sessions, queues, memory, observer
frames, permissions, and interoperability boundaries. It is research, not a
claim that bzz now implements those capabilities.

## Security boundary

Current bzz no longer launches Codex or any other assistant process. It has no
local agent key, ACP adapter, remote agent trigger, autonomous publisher, agent
memory, or observer control plane. The ordinary human composer and acknowledged
send path remain the only local publication authority.

## Compatibility

The Buzz protocol baseline remains pinned to
`ede26863345a518ec46edd6d7692e0281883491b`. Dependencies and SQLite schema are
unchanged.

See [`how-agents-works-in-buzz.md`](how-agents-works-in-buzz.md),
[`configuration.md`](configuration.md), [`security.md`](security.md), and
[`validation-v0.10.0.md`](validation-v0.10.0.md).
