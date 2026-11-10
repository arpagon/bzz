# bzz v0.13.0 — Upstream parity

**Status:** Release candidate 2026-11-10; production acceptance and
publication pending (see [`validation-v0.13.0.md`](validation-v0.13.0.md))

v0.13.0 moves the protocol baseline from Buzz `9f55bf67` (2026-08-24) to
`f3b127e3af5c14d2314faa2b63556c3f51ec6743` on Rust 1.98.1, and adopts the
upstream surfaces a terminal client can support. The plan is
[`planning/2026-11-07/v0.13.0.md`](planning/2026-11-07/v0.13.0.md).

## Highlights

- **Channels and roles (M1).** Channel types, announce channels (guests read
  only), unlisted channels hidden from discovery, a collapsed read-only System
  group, live membership changes, a removed channel that keeps its draft, and
  readable notices for every channel-settings change the relay reports.
- **Snapshots (M2).** Verified NIP-CL membership and NIP-RS read-state
  snapshots replace bursts of startup requests. A new profile card
  (`:profile`, `Space p`) shows a person's verified identity, status and, where
  the relay offers NIP-CL `membership_public`, their listed open channels.
- **Federated identity (M3).** `identity_adapter` plus `:signin` browser login
  (NIP-FA), evidence on every relay transport with re-minting, typed denials,
  Buzz Desktop as a credential provider, and verified profiles with a ✓ on
  names, in the mention picker and on the profile card.
- **Artifacts (M4).** `:artifacts` for the selected channel or open thread and
  `:tasks` for `buzz.task` artifacts assigned to you. Rename, delete, restore
  and move (with a confirmation that names the destination audience), with
  conflicts reconciled on the winning revision. Content schemas are unpublished
  upstream, so content is never rewritten and artifacts cannot be created.
- **Custom emoji (M5).** NIP-30/NIP-CE emoji inline in graphics terminals and
  `:shortcode:` elsewhere, plus custom reactions.
- **Presence and status (M6).** Presence dots, opt-in publishing
  (`[ui] publish_presence` or `:presence on|off`), owner presence for agents,
  and `:status` with expiry.
- **Channel and thread windows (M7).** The selected channel and threads page
  through relay-signed NIP-CW windows; threads open at their newest replies.
- **Huddles (M8).** Active-huddle marker, `:huddle` roster with guidelines and
  reactions, start/join/leave/end with confirmations, audio muted on join,
  push-to-talk and device choice. Builds without `huddles-audio` are
  view-only.
- **Local agent broker (M9).** `bzz broker create` makes an agent key held by
  bzz; the agent process calls `POST /v1/action` on loopback with a bearer
  credential for reading, posting, replying, reacting, setting its profile and
  addressing its memory. Per-action policy, one-key pause, a content-free
  activity log in `:broker`, the owner's NIP-OA tag on every agent event and
  AUTH, NIP-FA agent grants in FI communities, and opt-in owner-signed agent
  presence. `agents.create/update/delete` answer `unsupported`.
- **Messages (M10).** `:tochannel` sends a thread draft to the channel, and
  `:report` sends a NIP-56 report to moderators.

## Compatibility and upgrade

- Database migrations 0009–0015 run automatically; the pre-migration backup is
  kept as in earlier releases. A real v0.12.1 data directory upgrades with
  its Inbox, search, history and drafts intact on Linux, macOS and Windows
  (`scripts/upgrade-test.sh`).
- `bzz community add` no longer warns that a Buzz relay does not advertise
  NIP-98 (Buzz relays use it without listing it).
- The bundled SQLite moves to `libsqlite3-sys` 0.39 (RUSTSEC-2026-0459).
- New configuration: `identity_adapter`, `[ui] publish_presence`,
  `broker_agent_presence`, `huddle_input_device`, `huddle_output_device`, and
  `[[hosted_agents]]` (managed by `bzz broker`). See
  [`configuration.md`](configuration.md).
- New dependencies behind the default `huddles-audio` feature:
  `buzz-audio-client` (pinned Buzz revision) and `cpal`; libopus is linked.

## Security notes

Three new trust boundaries — the identity adapter, hosted agent keys and the
loopback broker, and huddle devices — are described in
[`security.md`](security.md) and the three v0.13 ADRs. A secret scan (T8) runs
the broker and NIP-FA flows with real secrets and checks that none reach
files, reports, status output or `Debug`/`Display` forms.

## Known limitations

- Artifact content is shown as a safe fallback (title, type, home, tags) until
  Buzz publishes the `buzz.task`/`buzz.project` schemas.
- The active-huddle marker covers the selected channel only.
- Broker idempotency records are kept in memory and do not survive a restart.
- A hosted agent can write only in channels whose roster its owner sees.
