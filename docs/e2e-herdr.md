# Herdr-assisted E2E testing

This guide explains how to run the [manual E2E plan](e2e-manual.md) from a
terminal pane managed by Herdr. Herdr is optional and is not a `bzz`
dependency.

Always use:

- a disposable identity;
- isolated directories;
- a relay and channel dedicated to testing;
- a `release` binary;
- placeholders in screenshots and public evidence.

Never include an `nsec`, passphrase, administrative key, or NIP-49 backup
contents in commands, arguments, ordinary variables, logs, or screenshots.

## 1. Prepare the process

Create `.env` from the template and set only non-secret values:

```bash
cp .env.sample .env
$EDITOR .env
set -a; source .env; set +a
cargo build --release --locked
```

Configure at least one test relay:

```dotenv
BZZ_RELAY_URL=wss://relay.example
BZZ_E2E_ROOT=/tmp/bzz-e2e
BZZ_CONFIG_DIR=/tmp/bzz-e2e/config
BZZ_DATA_DIR=/tmp/bzz-e2e/data
BZZ_CACHE_DIR=/tmp/bzz-e2e/cache
```

Check Herdr and list its panes:

```bash
herdr status
herdr pane list
```

Choose a terminal pane that does not contain another agent and save its ID:

```bash
export BZZ_PANE=w1:p2
herdr pane process-info --pane "$BZZ_PANE"
```

IDs such as `w1:p2` are local Herdr examples, not Buzz identifiers.

## 2. Launch and observe bzz

Run the process inside the pane:

```bash
herdr pane run "$BZZ_PANE" \
  'set -a; source .env; set +a; "$BZZ_BIN"'
```

Read the visible screen without attaching to the terminal:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text
```

Wait for a specific state when synchronizing a script with the TUI:

```bash
herdr pane wait-output "$BZZ_PANE" \
  --source visible \
  --match 'NORMAL · online' \
  --timeout 15000
```

Confirm which process occupies the pane:

```bash
herdr pane process-info --pane "$BZZ_PANE"
```

## 3. Send keys

Use `send-keys` to operate the TUI:

```bash
herdr pane send-keys "$BZZ_PANE" 'shift+?'
sleep 0.3
herdr pane send-keys "$BZZ_PANE" esc
sleep 0.3
herdr pane send-keys "$BZZ_PANE" q
sleep 0.3
herdr pane send-keys "$BZZ_PANE" y
```

Useful conventions:

| bzz action | Herdr key |
|---|---|
| Help `?` | `shift+?` |
| Quit (confirm) | `q`, then `y` |
| Leader / which-key | `space` |
| End `G` | `shift+g` |
| Channel / DM switcher | `space space` |
| Search | `/` |
| Inbox | `space n` |
| Theme options | `space o` |
| Context | `4` or `enter` on a selected message |
| Escape | `esc` |
| Confirm | `enter` |

Send `esc` in a separate call and wait before sending a letter. If Herdr sends
`esc` and a letter without a pause, the terminal may interpret them as
`Alt+letter`.

Do not use `herdr pane send-text` to operate `bzz` modes: Herdr may wrap it as
bracketed paste while the TUI processes keyboard events. Use `send-keys`.

### Test ASCII text

For simple E2E messages, convert ASCII text into key events:

```bash
herdr_type_ascii() {
  local pane=$1
  local text=$2
  local -a keys
  mapfile -t keys < <(python3 - "$text" <<'PY'
import sys
for char in sys.argv[1]:
    if char.isupper():
        print("shift+" + char.lower())
    elif char == " ":
        print("space")
    else:
        print(char)
PY
  )
  herdr pane send-keys "$pane" "${keys[@]}"
}

message="E2E-basic-$(date -u +%Y%m%dT%H%M%SZ)"
herdr pane send-keys "$BZZ_PANE" i
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "$message"
```

Read the screen and verify the text before submitting it:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" enter
```

Use content with no personal information and a dedicated E2E channel.

## 4. Secret prompts

Herdr may launch commands that prompt for passphrases, but the operator must
type or paste the secret directly into the terminal while the prompt has echo
disabled.

Example:

```bash
herdr pane run "$BZZ_PANE" \
  '"$BZZ_BIN" identity backup "$BZZ_IDENTITY_ID" --output "$BZZ_E2E_ROOT/identity.ncryptsec"'
```

When `New backup passphrase:` appears:

1. focus the pane visually;
2. type the passphrase or paste it from a credential manager;
3. press `Enter`;
4. repeat it only if the prompt requests confirmation.

If you use `gopass`, copy without printing:

```bash
gopass show --clip <backup-entry>
```

Do not automate secrets through:

- `herdr pane send-text`;
- `herdr pane send-keys`;
- process arguments;
- ordinary environment variables;
- `.env` files;
- `$(gopass ...)` substitution;
- terminal output or history.

The prompt does not echo its contents, but still review every screenshot before
retaining it.

## 5. Basic journey with Herdr

This corresponds to sections 0–4 of the manual plan:

1. Run version, paths, and check commands with `herdr pane run`.
2. Start `bzz`, open help with `shift+?`, close it with `esc`, then quit with
   `q` followed by `y`.
3. Create the identity from the pane's shell; complete secret prompts manually.
4. Configure a community and channel used exclusively for E2E testing.
5. Start the TUI and wait for `NORMAL · online`.
6. Enter the channel with `enter`.
7. Open the composer with `i`, type a unique marker, and submit with `enter`.
8. Read the screen until `[pending]` disappears.
9. Quit, restart, and confirm that the message appears exactly once.

Example visual check:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text \
  | grep -F "$message"
```

Visible output is supporting evidence; SQLite and the relay ACK remain the
sources of truth for deduplication and acceptance.

## 6. Conversation and recovery

Common sequences, with a pause when changing modes:

```bash
# Draft
herdr pane send-keys "$BZZ_PANE" i
# Type text with herdr_type_ascii
herdr pane send-keys "$BZZ_PANE" esc
sleep 0.3
herdr pane send-keys "$BZZ_PANE" i

# Selected reaction
herdr pane send-keys "$BZZ_PANE" r
sleep 0.3
herdr pane send-keys "$BZZ_PANE" enter

# Delete own message
herdr pane send-keys "$BZZ_PANE" 'shift+d'
sleep 0.3
herdr pane send-keys "$BZZ_PANE" y

# Mark unread and return to the end
herdr pane send-keys "$BZZ_PANE" 'shift+u'
herdr pane send-keys "$BZZ_PANE" 'shift+g'

# Lock the process
herdr pane send-keys "$BZZ_PANE" : l o c k enter
```

To test `identity missing`, remove only the E2E identity's credential from the
system credential manager, never its configuration. Verify through Herdr:

- history remains visible;
- `identity missing` state;
- no connection from the process;
- `i`, `r`, and `D` are blocked;
- the outbox is unchanged.

Then quit with `q` followed by `y`, run `identity restore-backup`, complete the passphrase
manually, and wait for `NORMAL · online` again.

### Inbox, workspace DMs, and search

Use independent disposable panes/databases and generic generated profiles.
Herdr may type public recipient labels and search terms, but never private
message content intended to remain confidential.

```bash
# Inbox
herdr pane send-keys "$BZZ_PANE" space n
sleep 0.3
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" f down enter esc

# Search
herdr pane send-keys "$BZZ_PANE" /
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "generated-token"
sleep 0.5
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" esc

# New workspace DM (the explicit command opens the recipient picker)
herdr pane send-keys "$BZZ_PANE" : d m enter
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "Generic Person"
herdr pane send-keys "$BZZ_PANE" space enter
```

Verify the DM modal visibly says it is not end-to-end encrypted. Do not automate
an owner/admin identity. Hide/reopen and add-participant tests require the exact
disposable participant set described in section 10 of the manual plan.

### Themes

The picker can also be tested without relay-specific data:

```bash
herdr pane send-keys "$BZZ_PANE" space o
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "nord"
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" down
sleep 0.3
herdr pane send-keys "$BZZ_PANE" esc
```

Verify that `Esc` restores the buffer with the previous theme. Reopen the
picker, use `tab` to switch scope, and confirm with `enter`; restart `bzz` to
verify persistence. Do not retain screenshots containing names or content from
real communities.

## 7. Independent second client

Do not run two clients against the same SQLite file for a real convergence
test. Create another root and a consistent copy:

```bash
export BZZ_SECOND_ROOT=/tmp/bzz-e2e-client2
mkdir -p "$BZZ_SECOND_ROOT"/{config,data,cache}
cp "$BZZ_CONFIG_DIR/config.toml" "$BZZ_SECOND_ROOT/config/config.toml"
sqlite3 "$BZZ_DATA_DIR/bzz.db" \
  ".backup '$BZZ_SECOND_ROOT/data/bzz.db'"
```

Delete local slots from **the copy** so the second client generates another
`client_id`:

```bash
sqlite3 "$BZZ_SECOND_ROOT/data/bzz.db" 'DELETE FROM read_slots;'
```

Create a temporary pane:

```bash
herdr pane split "$BZZ_PANE" \
  --direction down \
  --ratio 0.45 \
  --cwd "$PWD" \
  --focus
herdr pane list
export BZZ_SECOND_PANE=w1:p3
```

Start the second client with different paths:

```bash
herdr pane run "$BZZ_SECOND_PANE" \
  'set -a; source .env; set +a; BZZ_CONFIG_DIR=/tmp/bzz-e2e-client2/config BZZ_DATA_DIR=/tmp/bzz-e2e-client2/data BZZ_CACHE_DIR=/tmp/bzz-e2e-client2/cache "$BZZ_BIN"'
```

Verify:

- both panes are `online`;
- messages propagate A→B and B→A;
- each database contains one copy;
- two distinct `client_id` values;
- the same maximum `read_at`;
- in each database, its own slot has `is_local=1` and the other slot has
  `is_local=0`.

Close and remove only the temporary resources:

```bash
herdr pane send-keys "$BZZ_SECOND_PANE" q
herdr pane send-keys "$BZZ_SECOND_PANE" y
herdr pane close "$BZZ_SECOND_PANE"
rm -rf "$BZZ_SECOND_ROOT"
```

## 8. Offline cache without disconnecting the host

Do not disconnect the host or block a shared relay. Copy the state into another
disposable root, edit only that copy, and change its relay to a closed loopback
port:

```toml
relay_url = "ws://127.0.0.1:9/"
allow_insecure_localhost = true
```

Launch that copy with path overrides. Expected result:

- cached messages remain visible;
- `offline cache` state;
- a non-fatal connection error;
- `:reconnect` retries without losing history.

Close the process, delete the copy, and relaunch the original state to confirm
`online`.

## 9. Evidence and cleanup

Save only output that has already been reviewed:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text \
  > /tmp/bzz-e2e-screen.txt
```

Before attaching it, remove:

- real community hosts, names, or identifiers;
- pubkeys that are not public fixtures;
- real user names and content;
- private paths;
- all authentication material.

Do not retain `.env`, `ncryptsec` backups, clipboard contents, passphrase
prompts, or unreviewed terminal scrollback as evidence. Finish with the cleanup
section of the [manual E2E plan](e2e-manual.md).
