# Manual E2E test plan

This plan starts with the minimum journey and leaves destructive recovery tests
until the end. Always use a test identity; do not use an administrative key for
messages, reactions, or deletions.

To operate this journey from an automatable terminal pane, see the
[Herdr-assisted E2E guide](e2e-herdr.md).

## 0. Isolated setup

```bash
cp .env.sample .env
$EDITOR .env
set -a; source .env; set +a
cargo build --release --locked
"$BZZ_BIN" --version
"$BZZ_BIN" paths
"$BZZ_BIN" check
```

Set at least `BZZ_RELAY_URL`. The relay must be Buzz-compatible, and its
administrator must add the test pubkey as a member. Use the `release` binary:
debug builds deliberately use separate paths and keychain services.

Expected result:

- candidate version `0.15.0`;
- config, data, and cache under `BZZ_E2E_ROOT`;
- `configuration, theme, media, diagnostics, telemetry, and database are valid`;
- no secrets in `.env` or command output.

## 1. Empty startup and terminal restoration

```bash
"$BZZ_BIN"
```

Verify `Welcome to bzz`, open/close help with `?`/`Esc`, then quit with `q`
and confirm with `y`.
The terminal must restore its cursor, echo, and normal screen.

## 2. Identity and backup (basic)

```bash
"$BZZ_BIN" identity new --label "$BZZ_IDENTITY_LABEL" \
  --backend "$BZZ_IDENTITY_BACKEND"
"$BZZ_BIN" identity list
"$BZZ_BIN" identity verify <IDENTITY_ID>
"$BZZ_BIN" identity backup <IDENTITY_ID> \
  --output "$BZZ_E2E_ROOT/identity.ncryptsec"
```

Set `BZZ_IDENTITY_ID` and `BZZ_PUBLIC_KEY` in `.env`, then run `source .env`
again. Identity creation and backup prompt for passphrases without echo.

Expected result:

- `identity list` displays only UUID, label, pubkey, and backend;
- `identity verify` confirms the exact configured pubkey;
- neither `nsec1…` nor a private hexadecimal key ever appears;
- the backup internally starts with `ncryptsec1`, but the command prints only
  its path and pubkey;
- on Unix, `stat -c '%a' "$BZZ_E2E_ROOT/identity.ncryptsec"` returns `600`;
- repeating the backup with the same path fails without overwriting it.

Temporarily retain the backup and its passphrase; they are required for the
recovery test.

## 3. Community, authentication, and cache (basic)

First, the operator must add `BZZ_PUBLIC_KEY` to the relay as a **regular**
member. In a managed deployment, prefer running `buzz-admin add-member` inside
the relay instead of unlocking an owner key. Then configure bzz:

```bash
"$BZZ_BIN" community add \
  "$BZZ_COMMUNITY_LABEL" \
  "$BZZ_RELAY_URL" \
  "$BZZ_IDENTITY_ID"
"$BZZ_BIN" community list
"$BZZ_BIN" check
```

Set `BZZ_COMMUNITY_ID` to the returned UUID. The local identity label is not a
public profile: if the administrative interface searches for members by name,
first publish a test Nostr kind `0` profile or use an administrative path that
accepts the exact pubkey. bzz never publishes a human kind `0` itself; for a
disposable identity, `cargo run --example publish_test_profile -- <label>
"<Display Name>"` (with the same `BZZ_*` paths and, for an encrypted-file
identity, `BZZ_PASSPHRASE_FD`) publishes a generated one. The DM picker and
mention search only offer people with a cached profile. Create a dedicated active channel (for example,
`bzz-e2e-manual`) and add the test identity. Do not reuse a business channel or
an archived channel.

Open `"$BZZ_BIN"`.

Expected result:

1. `connecting`;
2. `authenticating`;
3. optionally `backfilling`;
4. `online`;
5. the joined E2E channel and its history are visible.

Neither `access denied`, `clock skew`, nor a pubkey change should appear.

## 4. Message and restart (basic)

1. Select the dedicated E2E channel with `j/k` and `Enter`.
2. Press `i`, type `E2E basic <date-time>`, and press `Enter`.
3. Wait for `[pending]` to disappear.
4. Quit with `q`, confirm with `y`, reopen bzz, and locate the same message.

If the relay responds with `channel is archived`, retain the rejection evidence
but switch to an active E2E channel; do not publish in an arbitrary production
channel as an alternative.

Expected result: one message, a confirmed ACK, and recovery from cache after the
restart.

**Stop here during the first session.** If sections 0–4 pass, the essential
journey is validated.

## 4.1 Mouse and mentions (optional)

Use only the dedicated E2E channel and generated text. Do not invoke a model
with production/private messages. Set `ui.mouse = "on"` or use a supported
interactive terminal with the default `"auto"` policy. Click a timeline row,
use the wheel, double-click a message to open its thread, click the composer,
and quit. The selected row must match keyboard navigation and the terminal must
restore normal text selection after exit.

With a second disposable member in the same channel, type `@` in the composer,
select that cached member, save the draft with `Esc`, restart, then send a
generated message. Verify that the visible label remains intact and the
published event carries exactly one lowercase `p` tag for the selected member.
Repeat offline: candidates must come from cache and no profile/member lookup may
be sent.

## 4.2 Verified remote managed agents (v0.11.0)

Use only deterministic disposable owner/agent identities and a dedicated E2E
channel. Publish an agent-signed kind 0 profile with a valid NIP-OA owner tag,
an agent-signed kind 10100 declaration, and an owner-signed kind 30177 policy;
add the agent pubkey to the channel with the relay's exact `bot` role. Do not run
an LLM, ACP adapter, tool, memory writer, or observer process.

```bash
"$BZZ_BIN" agents refresh --community "$BZZ_COMMUNITY_ID"
"$BZZ_BIN" agents list --community "$BZZ_COMMUNITY_ID"
"$BZZ_BIN" agents show <AGENT_PUBKEY> --community "$BZZ_COMMUNITY_ID" --json
```

Verify the CLI says the runtime is remote/not controlled by bzz, identifies the
verified owner and public policy, and emits versioned JSON without secrets or
raw events. In the TUI, run `:agents`; exercise wide/narrow resize, keyboard,
mouse, refresh, and close. The agent must have a textual `◆` marker and policy
state that does not rely on color. Switch community and run `:lock`; stale
directory actions must disappear.

Select **mention** only while the agent is a bot member of the exact active
channel. Confirm that it inserts visible text and one structured lowercase
`p` tag but does not send. Review and explicitly submit a generated message;
then have the deterministic remote fixture publish a signed `👀` reaction and
threaded reply. Verify existing Timeline, thread, unread, and Inbox behavior.

Repeat with an invalid signature, wrong NIP-OA owner, conflicting valid owner
tags, wrong policy signer, wrong policy `d` coordinate, missing declaration,
`owner-only` under a different human, DM `anyone`, stale cache, duplicate relay
delivery, and membership removal. None may remain invocable; every failed send
preparation must preserve the exact draft. Local agent keys, observer data,
profiles, policies, capabilities, messages, and pubkeys must not appear in the
content-free diagnostics journal or OTel payloads.

## 4.3 Diagnostics and opt-in telemetry (v0.9.0)

After one accepted disposable message, one rejected fake-relay publication, and
one deliberately interrupted fake-relay acknowledgement, verify exact local
states and content-free evidence:

```bash
"$BZZ_BIN" diagnostics status --json
"$BZZ_BIN" diagnostics outbox --json
"$BZZ_BIN" diagnostics report --output "$BZZ_E2E_ROOT/diagnostics-report.json"
```

Review all output and the report with sentinel generated message text, relay
notices, a fake nsec, local path, channel/community labels, pubkeys, auth
challenge, and attachment metadata. None may occur. The complete locally
authored event ID may occur. Verify `[pending]`, `[delivery unknown]`, and
`[rejected]` in the TUI, owner-only journal/report permissions, fixed three by
2 MiB journal bounds, report overwrite refusal, and `diagnostics clear --yes`
removing no SQLite, drafts, identities, configuration, or media.

Telemetry is optional. Ordinary E2E and CI must leave it unconfigured and prove
zero OTLP requests. For the controlled Emilia canary only, source the scoped
per-installation `BZZ_OTEL_TOKEN` without printing it, then run:

```bash
"$BZZ_BIN" telemetry configure --endpoint "$BZZ_OTEL_ENDPOINT"
"$BZZ_BIN" telemetry status --json
"$BZZ_BIN" telemetry test
"$BZZ_BIN" telemetry enable
```

The test sends exactly one `telemetry.test` OTLP protobuf log with no relay,
event, community, identity, or message attributes. Then use a dedicated fake
relay and disposable event to verify event-ID/time correlation at the operator,
never user content. Exercise redirect, `400`, `401`, `403`, `413`, `429`, `5xx`,
slow, unreachable, and malformed fake endpoints. Credentials must not follow a
redirect; queue/retries/shutdown stay bounded and relay/UI/SQLite behavior is
unchanged. Finish with `telemetry disable` and `telemetry forget --yes`; revoke
the canary token externally.

## 5. Conversation

- Draft: press `i`, type text, press `Esc`, then press `i` again; the text
  reappears.
- Thread: create one direct and one nested reply. With context closed, the root
  must show `↳ 2 replies · last reply …`; messages without replies consume no
  summary row. Select the root, press `Enter` or use `Space a` → **open
  context**, verify the title says `2 replies` rather than `3 messages`, reply,
  and close with `Esc`.
- Reaction: press `Space a`, choose **react**, then select a reaction and press
  `Enter`; repeat to remove the same reaction.
- Own deletion: select your message, press `Space a`, choose **delete own
  message**, and confirm with `y`.
- Unread: press `Space a`, choose **mark unread**, verify the indicator, open
  another channel with the channel/DM switcher, return to the E2E channel, then
  press `Enter` and `G`; the indicator must disappear even when the remote
  marker was already ahead.
- Finder: press `Space Space`; an empty query must prioritize joined `#` channels.
  Also find an open `+` channel, open it without publishing, and return to the
  E2E channel.
- Channel order: use `Space s` through smart, recent, and alphabetical order.
  The selected channel must stay selected by ID; no read marker, relay refresh,
  or subscription may result from the sort change.
- Reactions: select a generated message and press `r`. Use `1`–`8` or
  `Enter` to choose a reaction, then repeat to remove the same own reaction.
  `Esc` must publish nothing.
- Copy: select a generated message and press `y`; verify the terminal clipboard
  receives only its sanitized source text. Press `v`, move to a second generated
  message, then `y`; verify chronological text from the bounded range and no
  read-state change. Set `ui.clipboard = "disabled"` and confirm no OSC 52 copy
  occurs. For partial text, set `ui.mouse = "off"` and use terminal-native
  selection rather than creating an application range.
- Markdown: publish generated headings, quotes, ordered lists, task items,
  tables, inline code, and a fenced code block. Verify visible structure,
  predictable wrapping, inert links, and no terminal control output.

Delete the newest reply and verify the root count and relative last-reply time
move back to the remaining descendant; delete the final reply and verify the
summary row disappears. Confirm that thread summaries are absent from copied
message text, search, Inbox, and read-state behavior. Verify that threads,
reactions, and deletions are not duplicated and that you can never delete
another user's message.

### Workspace shell and writing dock

Use a disposable channel containing generated text only. At a wide terminal
size, verify that the labelled community and channel directories identify the
active workspace, the conversation has a bounded readable measure, each author
retains a label beside its local marker, and nearby same-author messages group
without losing their timestamps or date context. Open context and press `q` to
close it; the conversation must regain the space rather than leaving an empty
column.

Press `i` and verify that the persistent writing dock becomes the normal
multiline composer, uses the exact channel/thread target, preserves a draft
when cancelled, and sends only through the existing explicit human action.
Resize the terminal until side panes hide: the dock, status line, focused route,
and `?` help must remain usable and must not overlap. Repeat after temporarily
locking/removing the disposable credential: the dock must remain visible,
explain that it is read-only, and neither connect nor publish.

### Remote profile avatars (v0.7.1 candidate)

Exercise both routes with disposable data: one kind-0 `picture` pointing to a
public, non-sensitive HTTPS JPEG, PNG, GIF, or WebP test asset, and one at the
active relay's canonical `/media/<sha256>.<image-extension>` path that requires
a media-read authorization. In Kitty, Sixel, or iTerm2, open a channel
containing each author's generated message and wait for the photograph. It must
occupy measured timeline rows rather than painting over text; scroll rapidly,
resize, switch channels/communities, run `:media reload`, and return. With
multiple high-resolution source photographs, visible allocated rows must remain
stable (no alternating/flickering images). No stale image cells may remain and
the textual marker must stay useful during loading or failure.

For the relay-owned picture, capture only request metadata from disposable
infrastructure: it must have a short-lived `t=get` authorization bound to the
picture hash. Change the host, scheme, port, path, extension, hash, query, or
fragment and assert that no authorization is sent. External pictures must
receive no authorization. Set `ui.profile_avatars = "off"`, restart, and verify
only the textual marker appears and no request reaches either test host. Repeat
from cache-only/locked startup and a non-graphics terminal: no avatar request
may be made. Try an external profile URL with `http`, loopback, credentials, a
fragment, a non-443 port, and a redirect to one of those shapes; each must
retain the marker and make no request to the prohibited destination. Verify the
cache under the candidate cache root is owner-only and changes neither profile
data, Inbox/read state, subscriptions, drafts, nor relay traffic.

### Appearance

1. Press `Space o`, filter for `nord`, and move the selection: the view changes
   without modifying messages or unread state.
2. Press `Esc`: the exact previous theme must return.
3. Reopen the picker, select a theme, and press `Enter`; restart and verify that
   the selection persists.
4. Use `Tab` in the picker to test global and community scopes.
5. Create a `theme.toml` with one invalid color next to a valid one and run
   `bzz theme check`: only the invalid leaf should produce a warning.
6. Temporarily break the TOML syntax: `bzz theme check` must fail, but the TUI
   must start with the compiled preset and allow the file to be recovered.
7. Restore or remove the override and run `bzz theme reset`.

## 6. Portable backup without destroying the active identity

Import the backup as a second entry:

```bash
"$BZZ_BIN" identity import-backup \
  --label e2e-restored \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec" \
  --backend "$BZZ_IDENTITY_BACKEND"
"$BZZ_BIN" identity list
"$BZZ_BIN" identity verify <RESTORED_ID>
```

Expected result: the two UUIDs differ, but both pubkeys are identical. An
incorrect passphrase must return a generic error without modifying
configuration. Then remove the duplicate entry:

```bash
"$BZZ_BIN" identity remove <RESTORED_ID> --yes
```

You can also verify a safe in-place restoration:

```bash
"$BZZ_BIN" identity restore-backup "$BZZ_IDENTITY_ID" \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec"
"$BZZ_BIN" identity verify "$BZZ_IDENTITY_ID"
```

A backup belonging to a different pubkey must be rejected.

## 7. Keychain recovery and cache mode (advanced/destructive)

Do this only after verifying the NIP-49 backup.

1. Close bzz.
2. From the system credential manager, remove only this entry:
   - release service: `dev.arpagon.bzz`;
   - account: `identity:<BZZ_IDENTITY_ID>`.
3. Open bzz.

Expected result:

- `identity missing` state;
- cached history and channels remain visible;
- no relay connection opens;
- `i`, `r`, and `D` publish nothing;
- the status explains how to restore the identity.

Restore without changing the UUID, community, or pubkey:

```bash
"$BZZ_BIN" identity restore-backup "$BZZ_IDENTITY_ID" \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec"
"$BZZ_BIN" identity verify "$BZZ_IDENTITY_ID"
"$BZZ_BIN"
```

The client must return to `online`. To test `identity locked`, temporarily lock
the keychain/Secret Service and relaunch; bzz must display read-only cache and
must never generate a new identity. Unlock the keychain and relaunch to recover.

## 8. Network, multiple clients, and communities (advanced)

- To avoid disconnecting the host, copy the config/database into a disposable
  directory and change only that copy to a closed loopback port. Verify history
  in `offline cache`, run `:reconnect`, and then return to the real relay.
- For a real multi-client test, use separate SQLite databases. The second
  client must generate another `client_id`; publish in both directions and
  verify one copy, two slots, and the same maximum `read_at`.
- Leave a gap larger than one page, reconnect, and verify complete backfill.
  Do this in the isolated harness, not by generating hundreds of production
  messages.
- Configure a second test community and switch with `1`/`2`; verify isolation.
  If only one relay is available, cover this case with the automated
  multi-community test instead of duplicating the same host.

## 9. Images and attachments (generated fixtures only)

Generate a tiny PNG and a plain-text file inside `BZZ_E2E_ROOT`; never use a
personal photo/document. Copy the PNG in the desktop file manager, open the
composer, and press `Ctrl-v`. Verify a source-free `processing`/`queued` row
becomes `[ready]`, then send. Copy a disposable screenshot or generated bitmap
and repeat: it must become one `pasted-image.png` attachment without retaining
clipboard pixels after staging. Copy normal generated text and press `Ctrl-v`;
it must appear at the composer cursor without creating an attachment or sending
anything. A copied native file list must win over an accompanying text/image
clipboard representation.

Press `Ctrl-o`, select one and then multiple disposable files in the OS chooser,
and verify each becomes a queue row without automatic send. Cancel once and
verify the draft is unchanged. On Linux, repeat with XDG Desktop Portal
available and then unavailable; unavailable must produce only a content-free
status and must not launch zenity or another command. Use `Alt-o` to enter a
generated local path and verify the fallback stages the same way.

Use `Delete` to remove the newest queued/ready item and verify it cannot return
when a late upload finishes. Force an isolated upload failure, verify a
`[failed]` row, then use `Ctrl-r` to retry. Type a body plus an attachment,
press `Ctrl-c`, cancel with `n`, then repeat and confirm with `y`; after reopen,
the persisted draft must be empty. Set `media.clipboard_import = "off"`,
restart, and verify `Ctrl-v` does not read native clipboard data while ordinary
terminal text paste still works. In a clipboard-unavailable desktop session,
use `Ctrl-o` to choose the generated PNG or `Alt-o` to enter its path, and
verify the same secure staging path works. No source path, URI, clipboard text,
bitmap, or native format may
appear in SQLite, normal status, or retained evidence.

Verify another isolated client shows an attachment card and an inline image or
documented half-block fallback. Open **preview media** through `Space a`,
navigate with `[`/`]`, save with `s` to a new path, and compare SHA-256. Repeat
with the text file; it must remain a card and must not download until explicit
save.

Restart online, then disconnect only the disposable copy and verify the image
renders from its verified cache. Run `bzz media status`, clear that community's
media cache, and verify locked/offline mode no longer fetches it. Confirm source
paths and auth headers/events do not appear in output or SQLite. Content hashes
may appear only as required integrity metadata in `imeta` and media-cache rows.
Test an external/mismatched descriptor only with the automated fake server; do
not inject hostile events into a shared relay.

## 10. Inbox, workspace DMs, and search

Use two or three disposable regular-member identities with independent SQLite
databases. Never use the production owner identity.

### Workspace DMs

1. On client A run `:dm`, find B, select with `Space`, and press `Enter`.
   Both clients must discover the same `@` DM and show the explicit
   relay-readable/non-E2EE label.
2. Exchange generated text, one thread reply, reaction, generated attachment,
   draft, and read marker. Confirm one copy after reconnect.
3. Open B again from A. It must reuse the same channel UUID. In the isolated
   harness, drop the first `OK`; recovery must still find the same channel and
   reuse the original signed command event.
4. Run `:dm add` in A/B, add C, and confirm a different group-DM UUID appears
   while A/B remains unchanged.
5. Run `:dm hide` on A. A's row disappears only after visibility confirmation; B's
   row remains. Reopen the exact participant set and confirm A's row returns.
6. With a fourth regular member, query/search the A/B channel. No event,
   count, subscription, or search hit may be returned. The fourth member must
   not read A's kind 30622 snapshot.

### Inbox

1. From B mention A in the dedicated channel and reply to a root A authored.
2. Press `Space n` on A. The mention/thread must appear once even after reconnect;
   the DM, a saved draft, and any generated needs-action fixture use their
   corresponding filters.
3. Verify wide list/detail and narrow list→detail transitions. `Enter` must
   not acknowledge work. `i` opens the ordinary composer at the selected
   channel/thread without leaving Inbox; `o` opens exact canonical context and
   `q` returns to the same selected conversation. Use `m`, `U`, and `a`;
   `a` must confirm when it affects multiple visible rows. Restart and confirm
   state.
4. Repeat while offline/locked. Cached rows remain, no network call occurs, and
   mutations that require signing remain unavailable.

### Search

1. Press `/` and search a generated channel, profile, and unique message token.
   Confirm section ordering and exact navigation.
2. Test `from:`, `in:`, `after:YYYY-MM-DD`, and `before:YYYY-MM-DD`. An unknown
   or ambiguous person/channel must show a no-match notice rather than wider
   results.
3. Disconnect a disposable copy and repeat against cached content; the status
   must say local-only.
4. Hide a DM and delete a generated message. Neither may appear locally. A
   non-member and another community must never receive the private hit.
5. Publish a generated kind 1059 fixture only in the isolated automated relay;
   NIP-50 and local FTS must not return it.

## 11. v0.13.0 surfaces

Use disposable identities and dedicated test channels only. The isolated
pinned harness (`BZZ_BUZZ_SOURCE=<pin> ./scripts/test-relay.sh` brings it up
and tears it down; start the relay alone with
`./scripts/start-isolated-test-relay.sh --profile dev` inside the pinned Buzz
checkout) enables the optional relay capabilities this section needs:
`BUZZ_MEMBERSHIP_SNAPSHOT_ENABLED=true` and
`BUZZ_MEMBERSHIP_PUBLIC_ENABLED=true`. Against the harness, add the community
with `--allow-insecure-localhost` and the URL `ws://localhost:3030`.

### Channels, roles and the profile card

1. Join an announce channel (join role `guest`) with B. B's composer shows
   `read-only · announce channel (guest)`; `i`, `r` and `Enter` publish
   nothing and keep any draft.
2. A system channel stays in the collapsed `⚙` group until `:system`; it is
   read-only for every role.
3. Have the owner remove B from the selected channel. B's selection moves to
   a safe channel and B's draft for the removed channel is kept.
4. Select one of A's messages on B and press `Space p` (or run `:profile`).
   The card lists A's listed, open channels with A's role, marks archived ones,
   and never shows a shared private channel, an unlisted channel or a DM. On a
   relay without `membership_public` the card says the relay does not offer
   the lookup; it never shows an empty list for a failure.

### Snapshots and windows

1. Start with `membership_snapshot` enabled and confirm every joined channel
   (system channels included) appears with the right role; restart with it
   disabled and confirm nothing is lost.
2. In a channel with several hundred generated messages (harness only), scroll
   to the top: no gap, no duplicate, reactions present. Open a long thread: it
   opens at its newest replies, and moving above the oldest loaded reply loads
   older ones.

### Federated identity (FI communities only)

1. With `identity_adapter` configured and no session, the status explains that
   sign-in is required; `:signin` opens the browser, and after login the
   community connects. Media uploads and HTTP queries keep working.
2. Revoke the session at the adapter: the next re-mint fails with a typed
   denial, never a silent retry loop.
3. On Linux or macOS with Buzz Desktop running, `bzz identity pair-desktop
   --label <label> --relay <url>` pairs; approve in Buzz. The identity signs
   through Desktop and bzz never holds its key.

### Artifacts, emoji, presence and status

1. `:artifacts` lists the channel's artifacts; `:tasks` lists `buzz.task`
   artifacts assigned to you. `:artifact rename <title>`, `delete`, `restore`
   and `move <#channel>` ask for confirmation where the audience changes.
2. Press `r` on a message, then `:` and a shortcode prefix: custom emoji from
   the community appear; the reaction shows inline in a graphics terminal and
   as `:shortcode:` elsewhere.
3. With `[ui] publish_presence = true`, a second client shows the presence dot;
   with it off nothing is published. `:status --expires 10m testing` shows the
   status until it expires; `:status clear` removes it.

### Huddles

1. `:huddle start` (confirm), then join from a second client with
   `:huddle join`. Both enter muted; in the `:huddle` panel `m` toggles mute
   and `Space` is push-to-talk (each press or key repeat keeps you live for
   600 ms).
2. `:huddle devices` lists devices; `:huddle input <name>` and
   `:huddle output <name>` select them. Lock (`:lock`) and quit release the
   microphone.
3. `:huddle end` (creator only, confirmed) ends it for everyone.

### Local agent broker

1. `bzz broker create test-agent` prints the credential once. `bzz broker list`
   never shows it. `bzz broker allow test-agent channel.read,message.post`
   opts in to writes; an unknown action name is refused with the hosted list.
2. With bzz unlocked, an agent process posts through `POST /v1/action` on the
   loopback address shown in `:broker`; the post carries your NIP-OA tag. `p`
   in `:broker` pauses every agent; `o` toggles owner-signed agent presence.
   In the same panel `j`/`k` select an agent, `1`–`6` allow or deny one
   action for it (shown as `[x]`), and `Space` pauses only that agent; the
   next agent request sees the change.
3. `bzz broker delete test-agent --yes` revokes and removes the key.

### Send to channel and report

1. In an open thread, type a reply and run `:tochannel`: it is posted to the
   channel as a new message whose sent-from-thread tag carries the thread
   root and a short excerpt of it.
2. Select a generated message and run `:report spam generated fixture`; confirm.
   Run `:report author impersonation` to report its author instead; the
   prompt says so.
   The relay accepts the kind 1984 report.

## 12. v0.14.0 surfaces

Use the isolated pinned harness from section 11 and two disposable
identities, A and B, in the same channel. The harness builds the pinned
`buzz` CLI; use it as the "other Buzz app" where a step needs one.

### Message edits

1. As A, send `edit me`. Select it, press `e`, change it to `edit me please`
   and press `Enter`. A and B both show `edit me please (edited)`; the
   message keeps its place and its thread replies.
2. As B, select A's message and press `e`: the status says only your own
   message can be edited. Nothing is published.
3. Edit a message that has an image attachment: the image stays.
4. Search for a word that only the edited text contains: the message is
   found. The Inbox preview of an edited mention shows the edited text.

### Names and mentions

1. Give A and B the same display name. In a channel with both, each shows
   with a short key suffix (`Honey · 1a2b`), in the timeline and in `@`.
2. Type `@` and a unique exact name, then `Space`: it completes to the
   member. Send: the message carries that member's `p` tag only.
3. Mention someone who left the channel in a stale draft and send: bzz
   refuses with a reason and keeps the draft.

### Solo chats and sidebar sections

1. `:solo Plans` creates a Solo chat. It appears under `Solo` with a lock and
   only you as member.
2. Select a channel and run `:section new Ops Team`: the channel moves into an
   `Ops Team` group. `:section rename Ops`, `:section up`, `:section remove`
   and `:section delete` do what they say. `:sort alpha` sorts the group.
3. Restart bzz (or open a second client with the same identity): sections,
   sort and the Solo chat come back from the relay. Where Buzz desktop or
   mobile is available with the same identity, the sections match there.

### Buzz 1.0 as the key holder (Linux or macOS, Buzz 1.0 running)

1. `bzz identity pair-desktop --label buzz1 --relay <url>`; approve in Buzz
   1.0. bzz connects, posts, reacts and edits through Buzz.
2. Try to delete a message or start a huddle: bzz says `Buzz does not let
   other apps do this; do it in Buzz` before asking Buzz, and publishes
   nothing.

### Search paging and connection details

1. In a channel with more than 40 messages containing a common word, search
   it and press `Ctrl-o` until bzz says `no older results`. Rows already
   shown never move; newer results stay on top.
2. Add a community with the right host but a path the relay does not serve
   (or a relay that hosts another community): bzz reports `this relay serves
   no community at this address; check the community URL` and stops
   retrying.

## 13. v0.15.0 surfaces: local agents

Use the isolated pinned harness from section 11, one disposable identity and
a channel that identity created (it must be able to add members). You need
Node.js 22.19 or newer. **Use no real model for the repeatable part:** give
bzz, as `BZZ_NODE`, the stand-in that switches on the agent host's scripted
model, and an empty folder as `PI_CODING_AGENT_DIR`, as
`examples/harness_fixture.rs` does. The model is then `scripted/scripted`.
It answers `pong: this conversation holds N message(s) from people`, or
follows a line `do: [...]` in the message
(`agent-host/test/support/scripted-model.mjs`). `Ctrl-j` adds a line in the
composer. `./scripts/test-relay.sh --herdr` runs most of the first five
parts on the release binary by itself (the `t15-*` scenarios,
[`e2e-herdr.md`](e2e-herdr.md)); what it leaves to you is named in
[`validation-v0.15.0.md`](validation-v0.15.0.md), "Manual E2E".

Never point any of this at `~/.pi`, `~/.grok`, `~/.codex` or `~/.claude`,
and never at a real community.

### Create, show, add

1. `bzz --version` prints `bzz 0.15.0`. `bzz agent doctor` names Node, npm
   and whether the agent host is installed, and installs nothing.
2. `bzz agent new` on a terminal asks its questions in order: name, harness,
   model, instructions, who may wake it, folders, hosts, picture, Pi
   packages. Answer `scripted/scripted` for the model and
   `work=<a folder>:exec` for the folders. It prints the checks, asks you to
   confirm the owner authorization, asks for the identity's passphrase, and
   ends with `created in <community>`, the capability record and the next
   step. The first creation installs the agent host (it needs the npm
   registry once).
3. `bzz agent show <name>`: state, harness, model, who may wake it, the
   folder with its real path and its rights (`read-only; real commands,
   each asked of you first`), `git credentials: none stored`, and the
   capability record with
   `one final reply per mention: bzz`.
4. `bzz agent add <name> <channel>`: `added to #<channel>`. `bzz agents
   list` shows it as verified.
5. In the TUI, `:agent new <name>` opens the form with the same questions.
   `Ctrl-s` runs the checks; `Enter` then signs and creates. The form ends
   with the capability record. `:agents` lists the agent as `idle` within a
   few seconds, without a restart. `i` adds it to the open channel.
6. A creation that cannot finish leaves nothing: answer a model the host
   does not have (`nope/none`). It is refused before the confirmation, and
   `bzz agent list` does not show the agent.

### An agent answers

1. With bzz open and unlocked, in the agent's channel: `i`, `@`, the first
   letters of its name, `Tab`, a space, a few words, `Enter`. The agent's
   typing indicator shows, then **one** reply in the thread the mention
   started.
2. Open the thread (`Enter` on the message) and mention it again there: the
   count in its reply goes up by one. That is the conversation's memory. A
   message in the thread that does not mention it is not answered.
3. Quit bzz and start it again; mention it in the same thread. The count
   goes on from where it was.
4. `bzz agent run` answers the same way with no TUI. Interrupt it: the
   agent's process is gone (`bzz agent list` shows no process).

### The agents panel, and stopping a turn

1. Send `@<name>`, `Ctrl-j`, then
   `do: [["sleep", 60000], ["buzz_send", {"text": "late", "final": true}]]`.
2. `:agents` while it runs: the agent's row says `answering 1` and what it
   is doing; below, `state`, `doing now`, `last outcome`, its settings and
   its capability record. `l` shows the end of its log.
3. `x`: `its running turn was stopped`. The thread gets one notice that you
   stopped the answer, and `last outcome` says so. `x` again: `is not
   answering anything now`.
4. `Space` pauses the agent (`paused`, its process is gone) and resumes it.

### A real command: approve and refuse

The agent needs a folder granted with `:exec`.

1. Send `@<name>`, `Ctrl-j`, then
   `do: [["run", {"folder": "work", "command": "echo approved > made.txt; cat made.txt"}], ["report"]]`.
2. A line appears at the top: `<name> asks to run a command in /mnt/work …
   Ctrl-Y to see and answer it · refused in 4:5x`. Open the Inbox, the
   broker panel, a search: the line is still there. A plain `y` or `a`
   answers nothing.
3. `Ctrl-y`: the dialog names the agent, the conversation and that it is
   answering you, the folder with its real path, that the command runs as
   your user, and the whole command with its line numbers.
4. `a`: `allowed: <name> runs its command in /mnt/work`. The reply in the
   thread holds the command's output (`approved`), and `made.txt` is in the
   folder.
5. Ask again with another command. `Ctrl-y`, `r`, write a reason, `Enter`:
   `refused: <name> was told no, and nothing was started`. The reply says
   `Your owner refused this command, and says: <reason>`, and the command's
   file is not there.
6. Ask again and wait five minutes without answering: the agent says you did
   not answer within 300 seconds. Ask again and press `x` in `:agents`: the
   line at the top goes, and a `Ctrl-y` afterwards says nothing waits.
7. `bzz agent show <name>` lists each question with its answer under its
   activity.
8. Under `bzz agent run`, the same request is refused at once and the agent
   tells the chat that its owner must open bzz.

### Private repository, move and receive

1. `bzz agent credential set <name> github.com` on a terminal: the user name
   and the token are asked for and **not shown**. It prints `stored a git
   credential`. `credential list` prints the host only.
2. `bzz agent move <name> --to <file>` asks for the identity's passphrase
   and then for a passphrase for the file, **twice**; nothing typed is
   shown. A second passphrase that differs is refused and nothing is
   written. It ends with `moved into <file>`, what is in it, what stays, and
   `marked moved away here`.
3. `bzz agent list` shows `moved away`. `:agents` shows the agent as
   `stopped` with `moved away: it runs elsewhere`, and `bzz agent resume`
   refuses.
4. In a second profile of the same identity and community,
   `bzz agent receive <file>`: `received in <community>`, `no folder of this
   machine is granted to it`. The wrong passphrase, and a file with one byte
   changed, are each refused by name. Receiving the file again says the
   agent is already here.
5. Run the second profile and mention the agent in a thread it answered in
   before the move: the count goes on.
6. `bzz agent launch-kit <name> --to <folder>` says the key will be in the
   clear and asks. The steps on the other machine are in the folder's
   `README.md`; `bzz agent shutdown <name>` stops the copy and says whether
   it left, and `bzz agent receive <folder>` takes it back.

### Coding harnesses (optional; your own programs and accounts)

Only with your own `grok`, `codex` or `claude`, signed in. This spends your
usage: a few short prompts on each program's cheapest model.

1. `bzz agent doctor` names each program, its version, whether it is in the
   range bzz starts, its adapter, and the sign-in in use.
2. `bzz agent new <name> --harness grok --folder work=<folder>:rw` prints
   the harness's record with "it runs a real shell as your user" before you
   confirm; for `claude-code` also the warning that it has no sandbox. In
   `:agents` the row reads `grok`, `codex (experimental)` or `claude-code
   (experimental)`.
3. Mention it: one reply. Ask it to write a file in the granted folder (it
   does) and outside it (it does not; `bzz agent show` lists what bzz
   refused).
4. Quit bzz and the program, start bzz again, and ask in the same thread
   what was said before: it answers from its own saved session.

### Upgrade and the broker

1. Open a v0.14.0 profile that has a `bzz broker` agent: `bzz broker list`
   shows it as before, `bzz agent list` does not, and the agent's process
   still posts through `:broker` with its old credential.
2. `bzz broker delete <a local agent>` is refused and says to use
   `bzz agent`.

## 14. v0.16.0 surfaces: apps

Use the isolated pinned harness from section 11, one disposable identity in
a channel it can write to, and for the last parts a **second disposable
identity** in the same channel and the agent of section 13 on its stand-in
model (`scripted/scripted`). Apps need nothing installed. Work in a
window of at least 120 by 30. `./scripts/test-relay.sh --tmux` (or
`--herdr`) runs most of this on the release binary by itself (the `t16-*`
scenarios, [`e2e-herdr.md`](e2e-herdr.md)); what it leaves to you is named
in [`validation-v0.16.0.md`](validation-v0.16.0.md), M8, "Manual E2E".
`$APPS` below is a folder of yours for the app's files, outside
`BZZ_E2E_ROOT` or inside it, as you like.

Never point any of this at a real community, and never approve a script
you have not read.

### Write, check, try and install, at the shell

1. `bzz --version` prints `bzz 0.16.0`. `bzz app list` says `no apps are
   installed in <community>`.
2. `bzz app new --templates` lists `board`, `poll` and `approvals`.
   `bzz app new tasks --to "$APPS/tasks"` writes `app.lua`, `app.toml`
   and `brief.md`, `from the template board`. Running it again writes
   nothing: the folder exists already.
3. `bzz app check "$APPS/tasks"` says it loads and names its record type,
   `card`. Append a line `return app(` to `app.lua`: `check` now names the
   file and the line (`app.lua:<n>: …`). Take the line out again.
4. `bzz app try "$APPS/tasks" n "type:Buy milk" enter` prints the Board as
   text, 40 by 16, with `To do (1)` and `Buy milk`, and sends nothing
   anywhere.
5. `bzz app add "$APPS/tasks"` shows the version as every version is shown
   before an answer: `tasks (Tasks), version 1`, `From: you`, its hash,
   `Record types: card` (all new), the brief and the script. Answer `n`:
   `version 1 waits for your answer`, and `bzz app list` says so.
6. `bzz app approve tasks` shows it again and asks; answer `y`: `version 1
   is approved and is the version that runs now`. `bzz app show tasks` says
   `version 1 runs`, with its hash, and `open in: no channel`.
7. Optional: `bzz app dev "$APPS/tasks"` runs the folder in this terminal
   without approval, on records in memory. Make a card, change `"To do"`
   to `"Backlog"` in `app.lua` and save: the column is renamed and the
   card is still there. Save a broken script: the error and its line show,
   and the old version goes on. `Ctrl-c` leaves, says its records are gone,
   and the terminal is the shell's again.

### The pane

1. Open bzz and select the channel. `:apps` lists `tasks (Tasks) · version
   1 · open nowhere`. `:app open tasks`: the Board is drawn at the right of
   the conversation, `Tasks · Ctrl-c leave`, and the status line says `APP`
   and `Ctrl-c leaves the app`.
2. `5` gives the app the keyboard. `n`, a title with `q`, `:` and `?` in
   it, `Enter`: the card is saved under `To do (1)`, and none of those keys
   reached bzz (no quit question, no command line, no help). `Esc` stays in
   the app.
3. Paste two lines while the card's form is open: they arrive as one
   title, not as keys. `Ctrl-c`: the keyboard is the conversation's again,
   and `i`, a paste of two lines, leaves both lines in the draft unsent.
4. Narrow the window: at 100 columns the communities go, at 72 the Board is
   still beside the conversation, at 60 the status line says `app hidden ·
   5`, and `5` zooms the app over the window; `Ctrl-c` hides it again.
   Widen it back.
5. Open a thread (`Enter` on a message) at 120 columns: the thread takes
   the right side and the status line says `app behind thread · 5`. At 160
   both are drawn.
6. Select another channel: no app is drawn there. Come back: the Board is
   as you left it.
7. `q` (from the conversation) quits; the terminal is as it was before bzz.

### A new version, in place

1. With the Board open and a card selected, change `"To do"` to
   `"Backlog"` in `$APPS/tasks/app.lua`, and in bzz `:app add
   $APPS/tasks`. The overlay shows version 2 with its difference, one
   line. `a` before the end: `scroll to the end first`. `G`, then `a`:
   `tasks: version 2 runs in place of 1, its state kept`. The column is
   `Backlog`, and the same card is selected.
2. `:app rollback tasks`: `back to version 1 from 2`, `To do` again.
3. Add `"tag"` to `app.types` in the script and `:app add` it: the overlay
   marks `CHANGED from the version that runs: + tag`. `r`, a reason,
   `Enter`: refused, nothing of it runs, and `bzz app show tasks` keeps the
   reason.
4. `:app dev $APPS/tasks` runs the folder in the pane, on the channel's
   real cards, marked `not approved · dev`. A save reloads it in place.
   `:app close` ends it and the installed Board shows again.

### An agent changes the app

The agent of section 13, on `scripted/scripted`, in the channel where the
Board is open. Copy `tests/fixtures/apps/board-priority/app.lua` and
`brief.md` of the repository to `$APPS/priority.lua` and
`$APPS/priority.md`.

1. Mention it: `@<name>`, `Ctrl-j`, then
   `do: [["with_files", "buzz_propose_app", {"app": "tasks"}, {"script": "$APPS/priority.lua", "brief": "$APPS/priority.md"}], ["report"]]`
   (with the folder written out). One reply in the thread. A line at the
   top: `tasks: version <n> from your agent <name>, asked by <you> waits
   for your approval`. The Board does not change.
2. `Ctrl-y`: `From: your agent <name>`, `Asked by:`, the brief `CHANGED`,
   the difference. `G`, `a`: the Board shows `Priority: all` with the same
   card selected; `2` on it gives it priority 2.
3. Mention the agent again in the thread with `do: [["buzz_apps", {"app":
   "tasks"}], ["report"]]`: its reply says the version was applied.
4. `:app brief`: the brief of the version that runs is posted to the
   channel as your message.
5. `bzz agent show <name>` lists the proposal and who asked for it.

### Share it with the second identity

Start the second identity's bzz in another terminal, in the same channel.

1. In the first: `:app share`: `tasks, version <n>, is shared in
   #<channel>; its brief was posted there`. The brief shows in the
   timeline of both.
2. In the second: a line at the top, `tasks: version 1 from <the first
   identity>, a person of the channel … waits for your approval`.
   `:app open tasks` refuses: nothing of it runs yet. `Ctrl-y` shows who
   signed this exact revision, and the brief. `G`, `a`: it is approved and
   opens in that channel.
3. A card made in either pane shows in the other within a second or two.
4. In the first: `:app unshare`. The second keeps its installed copy and
   the cards. `:artifacts` in the channel listed the offer, before the
   unshare, as `bzz.app · an app offered here`, with what a rename and a
   delete there do.
5. Optional, in a disposable sandbox of Buzz desktop in the same
   community: the channel shows the brief's message, and nothing of the
   app or its records.

### Without an app, and after an upgrade

1. `:app remove tasks`: `tasks uninstalled; <n> records of bzz.app.tasks.*
   stay in the channel`. Everything of sections 11 to 13 works as before.
2. `./scripts/upgrade-test.sh` opens a v0.15.0 profile with agents: the
   agents, their ledger and the artifacts are as v0.15.0 left them, and
   `bzz app list` says no app is installed. With a v0.15.0 profile of your
   own: copy it first, open the copy, and check the same by hand.

## 15. v0.17.0: what only a person can check

Most of v0.17.0 runs by itself: the `t17-*` scenarios through tmux
([`e2e-herdr.md`](e2e-herdr.md)), the demo in a network namespace
(`tests/demo_offline_test.rs`), the sound servers in containers
(`scripts/test-audio-containers.sh`), the upgrade
(`scripts/upgrade-test.sh`). What is left needs a person, a real terminal or
real hardware. [`validation-v0.17.0.md`](validation-v0.17.0.md) says, for
each, why it was not seen. Use the release binary, `bzz --version` printing
`bzz 0.17.0`.

### The first minute, on a clean machine

1. On a machine that never had bzz (Debian 13, Ubuntu 22.04 or 24.04,
   AlmaLinux 9, Fedora): `curl -fsSL https://getbzz.dev/install.sh | sh`.
   It checks the machine, installs, and names `bzz demo`. Then `bzz demo`
   in a new terminal: `#team` with Ada's welcome, `DEMO · generated,
   offline` in the status line, in under a second.
2. In the demo: a thread, the DM, `/rollback`, a reaction, a message of
   yours answered, `@Scout` typing then answering in the thread, `#launch`
   with the Board (`5`, `n`, a title, Enter, `]`), `#infra` with the huddle
   moving. `q`, `y`: the terminal is as before, and no `bzz-demo-*` folder
   is left in the temporary folder.
3. `Ctrl+K`: three things you did not know the key for. Each row shows its
   key at the right.

### Windows, at the machine

In Windows Terminal, as a person, the list at the end of
[`e2e-windows.md`](e2e-windows.md): `?`, `:` and `Ctrl+K`; AltGr on a
Spanish, German or French layout (`@`, `{`, `[`, `\`, `|`, `€` in the
composer); Caps Lock on (`J` scrolls, `Ctrl+C` leaves an app); a paste of
one and of three lines with `Ctrl+V` and with a right click, nothing sent
before Enter; `¿`, `€`, `ñ` from the keyboard; the frame lines, the theme's
colours, emoji width, and the screen given back after `q` then `y`; a click,
a double click and the wheel in the timeline. Then `bzz demo` there.

### macOS, in a terminal

No one has opened v0.17.0 on macOS. In Terminal.app and in iTerm2 (or
Ghostty): `bzz demo`, `Ctrl+K`, the help, a channel's huddle panel, `q`
then `y`. With the owner's word, a huddle on the test relay with the Mac's
microphone, as in section 11.

### Huddle audio on Linux, with real devices

On a desktop with a microphone and a speaker, once with PipeWire
(`pactl info` says `PulseAudio (on PipeWire …)`) and once with PulseAudio,
against the isolated pinned harness of section 11 with a second identity on
another machine or headset:

1. `:huddle devices` lists the server's descriptions, not ALSA's names.
   `:huddle input <a description>` saves it.
2. `:huddle join` (or `start`): the confirmation shows your lane moving
   when you speak, before anything is sent, and says your microphone works.
   Unplug or mute the microphone at the system: after 3 s it says `no sound
   from …` and how to pick another.
3. `y`: both people hear each other. In the panel each lane moves with its
   voice, yours last; muted, yours still moves, says nobody hears you, and
   turns amber with `muted!` when you speak. The status line, with the panel
   closed, names who speaks.
4. `z`, then `v`: the zoomed view and its picker; each style moves with the
   call; Enter keeps one, Esc puts the saved one back.
5. A `huddle_input_device` saved by v0.16.0 (`hw:CARD=…,DEV=0`): the
   join opens that card's microphone, or the default and says once which
   name was not found.
6. With no sound server (stop PipeWire or PulseAudio for the user):
   joining says in one line what is missing, at 80×24 too, and bzz goes on
   working.

### The look in a graphics terminal

In kitty, Ghostty or WezTerm (true colour), then in the Linux console
(`TERM=linux`, 16 colours) and in tmux without `COLORTERM` (256): the
focused frame in the theme's colour with its title as a label, the others
muted; the status line's labels; behind the palette every cell dim; the
huddle's braille and block characters; with `NO_COLOR=1`, no colour at all
and a heavy frame on the focused pane. Avatars and images still draw (an
image protocol), and in `bzz demo` the five pictures load.

### Keys a physical keyboard gives

On Linux and macOS too: AltGr characters typed into the composer and the
palette, Caps Lock with letters and with `Ctrl`, `Shift+Tab` moving the
focus back. A key map of yours (`keymap.toml`) with `shift-` and without it
binds the same key.

## Results log

| Section | Result | Evidence/notes |
|---|---|---|
| 0. Setup | ☐ | |
| 1. Empty startup | ☐ | |
| 2. Identity/backup | ☐ | |
| 3. Community/auth | ☐ | |
| 4. Message/restart | ☐ | |
| 4.2 Diagnostics/telemetry | ☐ | |
| 5. Conversation | ☐ | |
| 6. Portable backup | ☐ | |
| 7. Recovery | ☐ | |
| 8. Network/multi-client | ☐ | |
| 9. Images/attachments | ☐ | |
| 10. Inbox/DMs/search | ☐ | |
| 11. v0.13.0 surfaces | ☐ | |
| 12. v0.14.0 surfaces | ☐ | |
| 13. v0.15.0 surfaces: local agents | ☐ | |
| 14. v0.16.0 surfaces: apps | ☐ | |
| 15. v0.17.0: what only a person can check | ☐ | |

## Cleanup

```bash
"$BZZ_BIN" community remove "$BZZ_COMMUNITY_ID" --purge --yes
"$BZZ_BIN" identity remove "$BZZ_IDENTITY_ID" --yes
rm -rf "$BZZ_E2E_ROOT"
```

Also delete any backup copied outside `BZZ_E2E_ROOT` when it is no longer
needed. Never include `.env`, `nsec`, backups, or passphrases in evidence.
