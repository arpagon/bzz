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

- candidate version `0.5.0`;
- config, data, and cache under `BZZ_E2E_ROOT`;
- `configuration, theme, media, and database are valid`;
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
accepts the exact pubkey. Create a dedicated active channel (for example,
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

## 4.1 Mouse, mentions, and local draft assistance (optional)

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

The following optional assistant check creates external Codex inference egress
through the user's separately authenticated installation; it is never required
for the relay journey:

```bash
"$BZZ_BIN" agent add --label e2e-local-drafter
"$BZZ_BIN" agent doctor
```

Select a generated cached message, use `:agent`, select `e2e-local-drafter`,
and wait for the review overlay. Discard once. On a second run, accept the
review draft and verify it appears only in the ordinary composer; quit without
sending. Start a third run, cancel with `Esc`, then lock or switch community
while it is running. No Nostr event, outbox row, attachment, or saved draft may
be created by any of these actions. Remove the test profile afterward with
`"$BZZ_BIN" agent remove <id> --yes`.

## 5. Conversation

- Draft: press `i`, type text, press `Esc`, then press `i` again; the text
  reappears.
- Thread: select a message, press `Enter` or use `Space a` → **open context**,
  reply, and close with `Esc`.
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

Verify that threads, reactions, and deletions are not duplicated and that you
can never delete another user's message.

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

### Remote profile avatars (v0.7 candidate)

Use a disposable kind-0 profile whose `picture` points to a publicly reachable,
non-sensitive HTTPS JPEG, PNG, GIF, or WebP test asset. In Kitty, Sixel, or
iTerm2, open a channel containing that author's generated message and wait for
its author photograph. It must occupy measured timeline rows rather than
painting over text; scroll rapidly, resize, switch channels/communities, run
`:media reload`, and return. No stale image cells may remain and the textual
marker must stay useful during loading or failure.

Set `ui.profile_avatars = "off"`, restart, and verify only the textual marker
appears and no request reaches the disposable image host. Repeat from
cache-only/locked startup and a non-graphics terminal: no avatar request may be
made. Try a profile URL with `http`, loopback, credentials, a fragment, a
non-443 port, and a redirect to one of those shapes; each must retain the
marker and make no request to the prohibited destination. Verify the cache
under the candidate cache root is owner-only and changes neither profile data,
Inbox/read state, subscriptions, drafts, nor relay traffic.

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
personal photo/document. In the composer press `Ctrl-a`, enter the PNG path,
wait for `1 attachment(s) ready`, and send. Verify another isolated client
shows an attachment card and an inline image or documented half-block fallback.
Open **preview media** through `Space a`, navigate with `[`/`]`, save with `s`
to a new path, and compare
SHA-256. Repeat with the text file; it must remain a card and must not download
until explicit save.

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

## Results log

| Section | Result | Evidence/notes |
|---|---|---|
| 0. Setup | ☐ | |
| 1. Empty startup | ☐ | |
| 2. Identity/backup | ☐ | |
| 3. Community/auth | ☐ | |
| 4. Message/restart | ☐ | |
| 5. Conversation | ☐ | |
| 6. Portable backup | ☐ | |
| 7. Recovery | ☐ | |
| 8. Network/multi-client | ☐ | |
| 9. Images/attachments | ☐ | |
| 10. Inbox/DMs/search | ☐ | |

## Cleanup

```bash
"$BZZ_BIN" community remove "$BZZ_COMMUNITY_ID" --purge --yes
"$BZZ_BIN" identity remove "$BZZ_IDENTITY_ID" --yes
rm -rf "$BZZ_E2E_ROOT"
```

Also delete any backup copied outside `BZZ_E2E_ROOT` when it is no longer
needed. Never include `.env`, `nsec`, backups, or passphrases in evidence.
