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

### Automated release-binary smoke scenarios

The versioned bzz-owned runner at `scripts/test-tui-herdr.sh` is the controlled
acceptance smoke gate for Crossterm key encoding, generated help, custom
keymaps, normal exit, and terminal restoration. It creates fresh owner-only
config/data/cache directories for every automated scenario and never creates
an identity, relay, channel, or secret-bearing configuration.

Build the release binary, choose a **disposable shell pane** (not an agent or
user's working pane), then run the default automated scenarios from a
Herdr-managed controlling pane:

```bash
cargo build --release --locked
export BZZ_BIN="$PWD/target/release/bzz"
export BZZ_HERDR_PANE=w1:p2 # replace with the disposable pane ID
./scripts/test-tui-herdr.sh --list
./scripts/test-tui-herdr.sh
```

The runner uses `herdr pane send-keys` only; it never uses `send-text` to
operate bzz. Its manifest is
[`../scripts/tui-herdr-scenarios.toml`](../scripts/tui-herdr-scenarios.toml).
Run a named automated scenario with
`./scripts/test-tui-herdr.sh --scenario startup-help-quit`. `--all` reports an
operator scenario as manual rather than trying to access fixture credentials or
content. Preserve only sanitized visible output and the normal-exit marker;
Inbox/relay authority is established by the manual store/relay postconditions
below.

### Harness scenarios (v0.13.0)

`harness` scenarios drive the release TUI against a **fresh** isolated pinned
relay with generated content. From a Herdr-managed controlling pane:

```bash
export BZZ_BUZZ_SOURCE=/path/to/pinned/buzz   # the pin in Cargo.toml
export BZZ_HERDR_PANE=w1:p2                   # a disposable shell pane, at its prompt
./scripts/test-relay.sh --herdr
```

The wrapper starts the relay with the optional v0.13 capabilities, writes a
disposable viewer profile and content with `examples/harness_fixture.rs`
(`#t9-general` with history, a thread, a task artifact and a custom emoji;
`#t9-announcements` where the viewer is a guest; the viewer's own
`#t9-agent-config` system channel), runs every `harness` scenario, then
removes the profile and tears the relay down. The viewer's passphrase stays
in an owner-only file passed through a descriptor (`BZZ_PASSPHRASE_FD`), and
ALSA's default device is the null device, so the huddle scenario never opens
a real microphone. The scenarios cover the System group, the read-only
announce composer, the artifacts panel, custom emoji in the reaction picker,
the report confirmation, `:presence`, the broker panel and a huddle (start,
join muted, unmute, mute, leave). The runner refuses to start a scenario
while a TUI is still in the foreground of the target pane.

### Harness scenarios for local agents (v0.15.0)

The same run also proves local agents on the release binary: the `t15-*`
scenarios. They need Node.js 22.19 or newer and, the first time, the npm
registry; without such a Node the fixture says so, makes no agent, and those
scenarios fail.

The fixture gives the viewer:

- `Scout`, a local agent on bzz's real Pi Durable host, a member of the
  viewer's own `#t15-agents`, with one folder granted for commands
  (`work`). Its model is the host's stand-in: bzz is given, as `BZZ_NODE`, a
  small program that sets the host's own switch and runs the real Node, and
  an empty folder as Pi's. No real model is called and `~/.pi` is never
  read.
- `Garnet`, `Cobalt` and `Coral`, one agent on each coding harness, with
  `BZZ_GROK`, `BZZ_CODEX` and `BZZ_CLAUDE` naming a file that does not
  exist. No program of your own is ever started; the panel shows what bzz
  says of each harness and of a program that is missing.
- a second profile of the same viewer (`second/`), to receive an agent in.

Those paths are in the fixture's `env` file, which the runner adds to bzz's
environment. It holds paths and never a secret.

What a `t15-*` scenario may use, besides keys and visible text:

| Field | Meaning |
|---|---|
| `args = [...]` | run `bzz <args>` on the fixture's profile instead of the TUI, and answer what it asks on the terminal with keys. Its output is read from the pane's recent lines, unwrapped |
| `profile = "second"` | the second profile |
| `type:<text>` in `keys` | the text, one key event per character; `{fixture}` is the fixture's folder |
| `absent = "text"` | text that must not be on the screen or in the pane's recent output after the step: what was typed at a prompt that does not echo, or a pane that must have closed |
| `timeout = <ms>` | the step's own time limit (default 15 s) |
| `regex = true` | `visible` is a pattern, for text a pane may wrap in more than one place |
| `pause = <ms>` | wait before the step's keys are sent, for what the relay must send back first and no screen shows arriving |

The scenarios run in the order of the manifest and later ones use what
earlier ones made (`Rover` is created, given a credential, moved and
received; the panel later shows it as moved away). They cover: `--version`,
`agent doctor`, `agent new` asking its questions, `agent credential set`
without echo, `agent show`, `agent move` with the passphrase typed twice (and
a second one that differs), `agent receive` in the second profile; an agent
answering a mention and remembering; the agents panel while a turn runs,
`x` stopping that turn, `Space` pausing; a real command approved and
refused, with the waiting line seen from the Inbox, the broker panel and the
agents panel; `:agent new` from its form to an answer; a moved-away agent in
the panel; and each coding harness in the panel.

Two things to know when writing one. Never follow `esc` with another key in
the same step: a terminal reads Esc and a key sent together as Alt and that
key. And a line bzz draws in a narrow pane wraps: match a short piece of it.

### Harness scenarios for the app pane (v0.16.0)

The `t16-*` scenarios prove the app pane on the release binary, in the same
run and on the same fixture. At the shell they write the Board from its
template (`bzz app new board --to {fixture}/board-app`), propose it with
`bzz app add` (answered no: it waits, nothing runs) and approve it with
`bzz app approve`; the second profile adds and approves the same folder;
and the fixture's `looper/`, an app whose script keeps its thread busy, is
added and approved. Then, in the TUI:

- `t16-app-pane`, M3's exit gate: `:apps` lists the Board; `:app open
  board` opens it beside `#t9-general`; letters, digits, `q`, `:`, `?`,
  Tab, Enter and Esc go to the Board's form and none opens anything of
  bzz's; a paste of two lines reaches the form as one piece of text;
  `Ctrl+C` gives the keyboard back; the composer then types, and a paste
  of two lines stays in the draft; the window is narrowed through 144,
  100, 72 and 60 columns (the app is hidden, `5` zooms it, `Ctrl+C` hides
  it again) and widened back; a thread opened beside it takes the right
  side, `5` brings the app back, and at 160 columns both show; `q`, `q`,
  `y` quit, and the terminal is checked to be given back.
- `t16-app-live-second-profile`: the Board is open in both profiles at
  once, in two panes; a card the second adds shows in the first one's pane.
- `t16-app-looper-composer`: the looper is open in the pane and the
  composer types a long line at once; `x` loops forever in the script,
  which is stopped and says so in the pane and the status line.

Fields only these use:

| Field | Meaning |
|---|---|
| `size = "WxH"` (scenario or step) | the window's size. Through Herdr the runner sets it on the pane's terminal with `stty`, which a Herdr pane narrower than that cannot show whole |
| `paste = "text"` (step) | a bracketed paste before the keys. Through Herdr it is the runner's one use of `send-text`, with the bracket sequences around the text |
| `beside = "second"` (scenario) | the second profile's TUI runs in a second pane (`BZZ_HERDR_SECOND_PANE`); steps with `pane = "second"` drive it |
| `restored = true` (scenario) | after the exit, the bytes bzz wrote and tmux's own state show the alternate screen left, the mouse and bracketed paste off and the cursor shown. Only `scripts/test-tui-tmux.py` checks it |

The same manifest runs through tmux on a private socket, for a machine
where Herdr is not running, or to keep text captures of every step:

```bash
export BZZ_BUZZ_SOURCE=/path/to/pinned/buzz
BZZ_TMUX_SCENARIOS=t16-cli-app-new,t16-cli-app-add,t16-cli-app-approve,t16-cli-app-add-second,t16-cli-app-add-looper,t16-app-pane,t16-app-live-second-profile,t16-app-looper-composer \
  BZZ_TMUX_CAPTURES=/tmp/bzz-captures ./scripts/test-relay.sh --tmux
```

`BZZ_TMUX_SCENARIOS=harness` (the default) runs every harness scenario,
v0.13 to v0.16, in order. The runner prints how long each step took to
show what it waits for.

### Manual disposable-fixture journey

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
| Agents directory | type `:agents`, then `enter` |
| Contextual actions | `space a` |
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

## 5.1 Remote-agent directory acceptance

Prepare the deterministic public-agent fixture described in manual E2E section
4.2 before launching bzz; never place an agent private key or model credential
in Herdr commands. Open the overlay with explicit key events:

```bash
herdr pane send-keys "$BZZ_PANE" shift+:
herdr_type_ascii "$BZZ_PANE" agents
herdr pane send-keys "$BZZ_PANE" enter
```

Capture sanitized wide and narrow visible states. Exercise `j`/`k`, mouse row
selection, `r`, `m`, resize, `esc`, community switch, and `:lock`. `m` must only
insert an exact mention into the visible human composer; it must not publish.
Verify the `◆ agent` marker, owner/policy/freshness explanation, and
remote-runtime disclaimer without retaining pubkeys or channel content in
screenshots.

## 6. Conversation and recovery

Common sequences, with a pause when changing modes:

```bash
# Draft
herdr pane send-keys "$BZZ_PANE" i
# Type text with herdr_type_ascii
herdr pane send-keys "$BZZ_PANE" esc
sleep 0.3
herdr pane send-keys "$BZZ_PANE" i

# Contextual actions: inspect the owned menu, choose the visible action with
# j/k, then press Enter. Reaction opens its own picker; deletion still asks y.
herdr pane send-keys "$BZZ_PANE" space a
sleep 0.3
herdr pane read "$BZZ_PANE" --source visible --format text

# Mark unread through the same menu, then return to the end.
herdr pane send-keys "$BZZ_PANE" 'shift+g'

# Lock the process
herdr pane send-keys "$BZZ_PANE" : l o c k enter
```

To test `identity missing`, remove only the E2E identity's credential from the
system credential manager, never its configuration. Verify through Herdr:

- history remains visible;
- `identity missing` state;
- no connection from the process;
- compose and publish-capable contextual actions are unavailable;
- the outbox is unchanged.

Then quit with `q` followed by `y`, run `identity restore-backup`, complete the passphrase
manually, and wait for `NORMAL · online` again.

### Inbox, workspace DMs, and search

This is the `operator-inbox-triage` manifest scenario. Run it only with the
non-admin disposable identity, relay, channel, and generated fixture described
in the manual plan. The visible screen is supporting evidence; after explicit
or bulk read, reply, restart, and relay acknowledgement, query the isolated
SQLite cache and pinned relay as the authoritative postconditions. Do not put
credentials, private content, or real identifiers in the manifest, shell
arguments, environment, output, or screenshots.

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
