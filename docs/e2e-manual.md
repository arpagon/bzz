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

- version `0.1.0`;
- config, data, and cache under `BZZ_E2E_ROOT`;
- `configuration, theme, and database are valid`;
- no secrets in `.env` or command output.

## 1. Empty startup and terminal restoration

```bash
"$BZZ_BIN"
```

Verify `Welcome to bzz`, open/close help with `?`/`Esc`, and quit with `Q`.
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
4. Quit with `Q`, reopen bzz, and locate the same message.

If the relay responds with `channel is archived`, retain the rejection evidence
but switch to an active E2E channel; do not publish in an arbitrary production
channel as an alternative.

Expected result: one message, a confirmed ACK, and recovery from cache after the
restart.

**Stop here during the first session.** If sections 0–4 pass, the essential
journey is validated.

## 5. Conversation

- Draft: press `i`, type text, press `Esc`, then press `i` again; the text
  reappears.
- Thread: select a message, press `Enter`/`Ctrl-]`, reply, and close with `Esc`.
- Reaction: press `r`, select a reaction, and press `Enter`; repeat to remove the
  same reaction.
- Own deletion: select your message, press `D`, and confirm with `y`.
- Unread: press `U`, verify the indicator, open another channel, return to the
  E2E channel with the finder, then press `Enter` and `G`; the indicator must
  disappear even when the remote marker was already ahead.
- Finder: press `Ctrl-p`; an empty query must prioritize joined `#` channels.
  Also find an open `+` channel, open it without publishing, and return to the
  E2E channel.

Verify that threads, reactions, and deletions are not duplicated and that you
can never delete another user's message.

### Appearance

1. Press `Ctrl-y`, filter for `nord`, and move the selection: the view changes
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

## Cleanup

```bash
"$BZZ_BIN" community remove "$BZZ_COMMUNITY_ID" --purge --yes
"$BZZ_BIN" identity remove "$BZZ_IDENTITY_ID" --yes
rm -rf "$BZZ_E2E_ROOT"
```

Also delete any backup copied outside `BZZ_E2E_ROOT` when it is no longer
needed. Never include `.env`, `nsec`, backups, or passphrases in evidence.
