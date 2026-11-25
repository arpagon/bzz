# bzz documentation

Start with the [README](../README.md). Everything here is plain Markdown.

## Using bzz

| Guide | What it covers |
|---|---|
| [configuration.md](configuration.md) | `config.toml`, identities, communities and every setting |
| [keys.md](keys.md) | Every key, where it works and its `:` form, every command, and what only the shell does; generated from bzz's own table of actions |
| [themes.md](themes.md) | The 60 built-in themes and `theme.toml` |
| [media.md](media.md) | Images, avatars, attachments, terminal graphics and the media cache |
| [inbox-dms-search.md](inbox-dms-search.md) | Inbox, direct messages and search |
| [apps.md](apps.md) | Apps: write one, try it, install it, approve a version, roll back, the pane, your agents writing and changing apps, sharing one in a channel, and what people on Buzz desktop see. The script contract is [app-api.md](app-api.md); the guide an agent writes from is [app-authoring.md](app-authoring.md); bzz's side of the records is [app-records.md](app-records.md) |
| [agents.md](agents.md) | Local agents: create one, add it to a channel, what it can touch, real commands, private repositories, moving it, and Grok, Codex and Claude Code |
| [troubleshooting.md](troubleshooting.md) | Symptoms and fixes |
| [security.md](security.md) | Keys, trust boundaries, diagnostics and telemetry |
| [protocol-compatibility.md](protocol-compatibility.md) | The Buzz revision bzz is pinned to and every protocol surface it supports |

## Releases

| Version | Notes | Validation | Plan |
|---|---|---|---|
| v0.17.0 (candidate) | [notes](release-v0.17.0.md) | [validation](validation-v0.17.0.md) (per task: [validation-v0.17.0/](validation-v0.17.0/)) | [plan](planning/2026-11-24/v0.17.0.md), [how it closed](planning/2026-11-24/v0.17.0.md#how-it-closed-2026-11-25) |
| v0.16.0 | [notes](release-v0.16.0.md) | [validation](validation-v0.16.0.md) | [plan](planning/2026-11-17/v0.16.0.md) |
| v0.15.0 | [notes](release-v0.15.0.md) | [validation](validation-v0.15.0.md) | [plan](planning/2026-11-13/v0.15.0.md) |
| v0.14.0 | [notes](release-v0.14.0.md) | [validation](validation-v0.14.0.md) | [plan](planning/2026-11-12/v0.14.0.md) |
| v0.13.0 | [notes](release-v0.13.0.md) | [validation](validation-v0.13.0.md) | [plan](planning/2026-11-07/v0.13.0.md) |
| v0.12.1 | [notes](release-v0.12.1.md) | [validation](validation-v0.12.1.md) | [plan](planning/2026-08-28/v0.12.1.md) |
| v0.12.0 | [notes](release-v0.12.0.md) | [validation](validation-v0.12.0.md) | [plan](planning/2026-08-28/v0.12.0.md) |
| v0.11.3 | [notes](release-v0.11.3.md) | [validation](validation-v0.11.3.md) | [plan](planning/2026-08-25/v0.11.3.md) |
| v0.11.2 | [notes](release-v0.11.2.md) | [validation](validation-v0.11.2.md) | [plan](planning/2026-08-25/v0.11.2.md) |
| v0.11.1 | [notes](release-v0.11.1.md) | [validation](validation-v0.11.1.md) | [plan](planning/2026-08-25/v0.11.1.md) |
| v0.11.0 | [notes](release-v0.11.0.md) · [artifact verification](release-v0.11.0-verification.md) | [validation](validation-v0.11.0.md) | [plan](planning/2026-08-24/v0.11.0.md) |
| v0.10.0 | [notes](release-v0.10.0.md) · [artifact verification](release-v0.10.0-verification.md) | [validation](validation-v0.10.0.md) | [plan](planning/2026-08-24/v0.10.0.md) |
| v0.9.0 | [notes](release-v0.9.0.md) · [artifact verification](release-v0.9.0-verification.md) | [validation](validation-v0.9.0.md) | [plan](planning/2026-08-23/v0.9.0.md) |
| v0.8.0 | [notes](release-v0.8.0.md) · [artifact verification](release-v0.8.0-verification.md) | [validation](validation-v0.8.0.md) | [plan](planning/2026-08-21/v0.8.0-clipboard-first-attachments.md) |
| v0.7.1 | [notes](release-v0.7.1.md) · [artifact verification](release-v0.7.1-verification.md) | [validation](validation-v0.7.1.md) |  |
| v0.7.0 | [notes](release-v0.7.0.md) · [artifact verification](release-v0.7-verification.md) | [validation](validation-v0.7.md) | [plan](planning/2026-08-20/v0.7.0-remote-profile-avatars.md) |
| v0.6.0 | [notes](release-v0.6.0.md) | [validation](validation-v0.6.md) | [plan](planning/2026-08-20/v0.6.0.md) |
| v0.5.0 | [notes](release-v0.5.0.md) | [validation](validation-v0.5.md) | [plan](planning/2026-08-20/v0.5.0.md) |
| v0.4.0 | [notes](release-v0.4.0.md) |  | [plan](planning/2026-08-18/v0.4.0.md) |
| v0.3.0 | [notes](release-v0.3.0.md) |  | [plan](planning/2026-08-15/v0.3.0.md) |
| v0.2.0 | [notes](release-v0.2.0.md) |  | [MVP plan](planning/2026-07-31/mvp.md) |

Releasing: [releasing.md](releasing.md). End-to-end checks: [e2e-manual.md](e2e-manual.md) (manual), [e2e-herdr.md](e2e-herdr.md) (automated in Herdr) and [e2e-windows.md](e2e-windows.md) (Windows over SSH).

## Design records

- [ADR: v0.16 apps](adr-v0.16-apps.md): bzz runs a Lua script it did not ship
- [ADR: v0.15 local agents](adr-v0.15-local-agents.md), with the [agent host protocol](agent-host-protocol.md)
- [ADR: compositor and TUI source for promotional films](adr-promo-video-compositor.md)
- [ADR: v0.4 interaction design clean-room boundary](adr-v0.4-clean-room-interaction.md)
- [ADR: v0.5 clean-room workspace visual language](adr-v0.5-workspace-visual-language.md)
- [ADR: v0.6 organized conversations and local copy boundary](adr-v0.6-organized-conversations.md)
- [ADR: v0.7 remote profile-avatar boundary](adr-v0.7-remote-profile-avatars.md)
- [ADR: v0.7.1 relay-hosted profile-avatar authorization](adr-v0.7.1-relay-profile-avatar-auth.md)
- [ADR: v0.8 explicit clipboard import](adr-v0.8-clipboard-import.md)
- [ADR: v0.11 remote managed-agent interoperability](adr-v0.11-remote-managed-agent-interoperability.md)
- [ADR: v0.13 local agent broker and key custody](adr-v0.13-agent-broker.md)
- [ADR: v0.13 huddle audio](adr-v0.13-huddle-audio.md)
- [ADR: v0.13 federated identity (NIP-FI, NIP-FA, local provider)](adr-v0.13-federated-identity.md)

## Background and other plans

- [How agents work in Buzz](how-agents-works-in-buzz.md)
- [v0.5 benchmark](benchmark-v0.5.md) and [visual review](visual-review-v0.5.md)
- [Brand guide](brand/README.md) (Spanish)
- [Draft acknowledgement and stale-composer recovery](planning/2026-08-22/draft-send-acknowledgement.md) (2026-08-22)
- [bzz v0.9.0 — Local connection and outbox diagnostics](planning/2026-08-23/local-connection-and-outbox-diagnostics.md) (2026-08-23)
- [Native file picker attachments](planning/2026-08-23/native-file-picker-attachments.md) (2026-08-23)
- [bzz v0.9.0 — Opt-in OTel client observability](planning/2026-08-23/opt-in-otel-client-observability.md) (2026-08-23)
- [bzz website — getbzz.dev](planning/2026-08-24/website-getbzz-dev.md) (2026-08-24)
- [Deferred research — Owner-private live agent activity](planning/2026-08-25/deferred-nip-ao-agent-activity.md) (2026-08-25)
- [bzz launch film and `bzz-promo-video` skill](planning/2026-11-11/promo-video.md) (2026-11-11)
