# v0.11.1 validation record

**Status:** Implementation validated 2026-08-25; publication pending

**Candidate:** `0.11.1`

**Validated implementation:** `e4c2d0a67c5380ddf053e55464f2859bdf110e3d`

**CI:** [32883151235](https://github.com/arpagon/bzz/actions/runs/32883151235)

**Pinned Buzz:** [32883152277](https://github.com/arpagon/bzz/actions/runs/32883152277)

**Buzz baseline:** `9f55bf67456be10ff7c8238bf0d9e12e582848f6`

## Compatibility and boundary

- [x] Buzz dependency pin and locked source revision unchanged
- [x] no relay change, endpoint, deployment, or private Buzz Desktop interface
- [x] no agent key, process, ACP, model, tool, memory, observer, provider, or
  autonomous publication surface
- [x] human composer, structured mention, draft, signing, outbox, and
  acknowledgement boundaries unchanged
- [x] exact bot authority remains mandatory for every non-DM destination

## Schema 8 and membership projection

- [x] migration 0008 derives roles only from the current membership head,
  stored kind 39002 bytes, pinned relay signer, exact `d` channel, and exact
  participant tag
- [x] exact four-field lowercase `bot` is distinct from lookalike/extended roles
- [x] historical `member` projection repairs to `bot` without changing event
  bytes
- [x] immutable duplicate current event repairs later projection drift
- [x] repeated duplicate after convergence is a true no-op
- [x] fresh and version-2/version-7 upgrade fixtures reach schema 8
- [x] unrelated drafts, outbox, messages, profiles, Inbox, search, read state,
  media, and identities retain their existing migrations
- [x] migration backup remains owner-private and bounded

## DM verification

- [x] discovery candidates are bounded exact current DM participants or exact
  bot members
- [x] kind 10100 is queried before profiles for DM candidates; exact bot
  candidates also receive bounded profile hydration
- [x] ordinary DM humans without a declaration receive no durable incomplete
  agent projection
- [x] valid NIP-OA plus either exact bot authority or a DM-only declaration,
  2–9-person participation, and active-owner equality permit DM invocation
- [x] absent public policy is eligible only for the exact verified owner
- [x] non-owner DM invocation fails even under `anyone`
- [x] the same operational `member` role has no authority in a stream/unknown
  destination
- [x] missing, malformed, conflicting, stale, removed, wrong-owner,
  wrong-policy-signer, and wrong-coordinate states remain fail-closed
- [x] visible names, avatars, replies, and `p` tags cannot promote a human

## Conversation presentation

- [x] verified current agent uses textual `◆` independently of color
- [x] active verified owner renders `managed by you`
- [x] another owner uses sanitized profile label or abbreviated pubkey
- [x] stale projection has no solid verified marker or asserted owner wording
- [x] timeline and thread/context receive agent presentation
- [x] one-to-one selected DM header and textual sidebar identify verified agent
- [x] Inbox author rows and detail identify verified agents
- [x] Agents detail identifies owner relationship and remote-runtime boundary
- [x] existing bounded avatar pipeline remains optional with deterministic
  textual fallback
- [x] deterministic wide/narrow rendering contains no terminal controls

## Relay system events

- [x] new kind 40099 ingestion requires the pinned relay signer
- [x] bounded parser covers the reviewed semantic allowlist
- [x] `dm_created` renders a profile-resolved direct-message introduction
- [x] relay pubkey never renders as an ordinary author
- [x] malformed, oversized, and unknown payloads become content-free unsupported
  rows
- [x] raw system JSON is absent from copyable `Message.content`
- [x] system events do not enter search, Inbox projection, authored-message
  channel activity, unread counts, or agent readiness
- [x] cached system rows are rendered only when their author matches the pinned
  community relay key

## Resource behavior

- [x] unconditional five-minute full directory refresh removed
- [x] startup/reconnect, exact membership events/notifications, explicit refresh,
  and send-time revalidation remain triggers
- [x] no-op duplicate membership does not redraw or write
- [x] agent refresh is single-flight/coalesced through existing task ownership
- [x] candidate, author chunk, page, membership, DM participant, and projection
  bounds remain explicit
- [x] existing idle tick and redraw-gate regressions pass
- [x] controlled quiet-relay release-binary idle observation ran for 15 minutes
  after a 60-second settle: database stayed `22491136` bytes, WAL stayed
  `1816952` bytes, journal stayed at 374 bounded records, RSS moved from 26432
  KiB to 25432 KiB, and CPU advanced 778 ticks without a hot loop

## Local deterministic gates

- [x] `cargo fmt --all`
- [x] `cargo check --locked --all-targets`
- [x] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [x] targeted schema, membership, agent policy, DM, system parser, store, and UI
  tests
- [x] final uninterrupted `cargo test --locked --all-features --all-targets`:
  179 library tests, all integration tests, and all benchmark smoke targets pass
- [x] `cargo deny check`
- [x] `cargo audit` with only the two accepted baseline warnings
  (`instant` unmaintained and transitive `lru 0.18.1` advisory)
- [x] release build, `bzz 0.11.1`, `bzz check`, and isolated schema-8/agents
  smoke

## Pinned Buzz and visual acceptance

- [x] pinned Buzz relay integration passes at the unchanged `9f55bf6` checkout
- [x] existing message/DM/search/media/read-state journeys remain green
- [x] deterministic exact mention plus signed reaction/threaded-reply journey
- [x] owner-controlled DM `member` compatibility journey
- [x] copied production cache refresh repaired exact bot roles and verified 15
  agents; Fizz resolved as an eligible owner-controlled remote identity without
  a remotely queryable kind 10100
- [x] Herdr release-binary scenarios pass; live copied-cache review showed
  `◆ @Fizz`, `◆ Fizz · managed by you`, semantic `Direct message started with
  Fizz`, `:agents` owner-only/eligible detail, terminal restoration, and the
  explicit under-50-column fallback
- [x] 15-minute quiet-relay idle observation showed stable SQLite, WAL, journal,
  and memory with no periodic directory refresh

## Cross-platform and publication

- [x] Linux, Windows, Intel macOS, and Apple Silicon macOS CI
- [ ] tagged-head CI
- [ ] release workflow publishes expected assets
- [ ] aggregate and individual SHA-256 checksums
- [ ] SLSA provenance subjects
- [ ] CycloneDX application version/components
- [ ] downloaded Linux version/check/schema-8/agents/no-hosting smoke
- [x] native-keychain and encrypted-file credential regression
- [ ] published artifact verification record
