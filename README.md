<p align="center">
  <img src="site/public/assets/logo.svg" alt="bzz logo: a hexagonal cell with terminal lines" width="88">
</p>

<h1 align="center">bzz</h1>

<p align="center">
  <strong>A fast, keyboard-driven terminal client for <a href="https://github.com/block/buzz">Buzz</a>.</strong><br>
  Your team chats in Buzz. You live in the terminal. Now you don't have to leave it.
</p>

<p align="center">
  <a href="https://github.com/arpagon/bzz/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/arpagon/bzz?color=ffb000&label=release"></a>
  <a href="https://github.com/arpagon/bzz/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/arpagon/bzz/ci.yml?branch=main&label=ci"></a>
  <a href="#license"><img alt="License: MIT or Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-8a8272"></a>
  <a href="https://getbzz.dev"><img alt="Website" src="https://img.shields.io/badge/web-getbzz.dev-ffb000"></a>
</p>

<p align="center">
  <a href="https://getbzz.dev/assets/promo-v016.mp4" title="Watch the launch film, with sound">
    <img src="site/public/assets/poster-promo-v016.png" alt="bzz with the Board app beside the chat, changed in place by the agent's approved version, the same card still selected" width="860">
  </a>
  <br>
  <sub>The real bzz 0.16.0 binary, recorded in a terminal. <a href="https://getbzz.dev/assets/promo-v016.mp4">Watch the 55-second film with sound</a>.</sub>
</p>

<p align="center"><sub>An independent open-source project, not affiliated with, endorsed by, or sponsored by Block, Inc.</sub></p>

<p align="center">
  <a href="#try-it-in-60-seconds">Try it</a> ·
  <a href="#install">Install</a> ·
  <a href="#get-started">Get started</a> ·
  <a href="#keys">Keys</a> ·
  <a href="docs/release-v0.17.0.md">What's new in 0.17</a> ·
  <a href="docs/README.md">Docs</a>
</p>

## Try it in 60 seconds

```sh
curl -fsSL https://getbzz.dev/install.sh | sh
bzz demo
```

`bzz demo` opens a generated team in your terminal: channels, a thread, a
DM, an agent that answers when you mention it, a huddle in progress. It runs
offline, asks for no account and no key, and leaves nothing behind when you
quit. On Windows, install with the [PowerShell line](#install), then run
`bzz demo`.

**Requirements:** Linux (x86_64 and arm64) with glibc 2.34 or newer (Ubuntu
22.04, Debian 12, Fedora 35, RHEL 9 or newer); macOS (Apple Silicon and
Intel); Windows (x86_64). Huddle audio on Linux needs PipeWire or
PulseAudio. No build for Alpine or other musl systems yet.

**Compatibility:** tested against the Buzz relay at `block/buzz` revision
[`bf3bdd0e`](docs/protocol-compatibility.md), and paired with Buzz desktop
1.0.1.

## Who it's for

You spend your day in a terminal: an editor, a shell, a coding agent or
three in tmux. Your team, and more and more your agents, talk in Buzz.
Switching to a desktop app for every mention breaks your flow.

bzz puts Buzz in a pane next to your work. It is a single Rust binary
that runs in any terminal, over SSH and offline, and is driven entirely from
the keyboard. It speaks the Buzz protocol directly and is tested against
the real Buzz relay, so your teammates on Buzz desktop and mobile see exactly
what you send.

> **New in 0.17 — the first minute.** `bzz demo` opens a generated team in
> your terminal, offline. `Ctrl+K` finds and runs any action, channel,
> person or theme, and shows its key. The huddle shows who is speaking, and
> your own microphone, muted or not. Quiet frames with a visible focus,
> `NO_COLOR`, unread counts. On Linux bzz needs no sound library, and on
> Windows every key works. [Release notes](docs/release-v0.17.0.md), with
> what changes if you already use bzz.
>
> **Since 0.16 — apps in the channel.** A small app beside the
> conversation: a task board, a poll, a rota. Ask your agent for one, or
> for a change to the one that is open ("add a priority and a filter to the
> board"); you read and approve its exact bytes, and the board changes in
> place. The people of the channel share its records, and you can offer it
> to them. A script runs inside bzz with no file, network or key to reach.
> [Release notes](docs/release-v0.16.0.md) · [guide](docs/apps.md).
>
> **Since 0.15 — your own agents.** `bzz agent new` creates an agent and
> bzz runs it on your machine. [Guide](docs/agents.md).

## Why bzz

- **Keyboard first.** Channels, threads, DMs, reactions, edits and search,
  all without leaving the home row. Vim-style motions, a `Space` leader and
  a `?` cheat sheet.
- **Works offline.** History, threads, profiles and drafts live in a local
  SQLite database. Search it with no connection; it fills in from the relay
  when you are back.
- **Feels like Buzz.** Solo chats and your sidebar sections sync with Buzz
  desktop and mobile. Names, @mentions, edits, custom emoji, presence and
  status follow the same rules as Buzz 1.0.
- **Agents, on your terms.** Mention an agent and its reply lands in the
  thread. Create your own with one command: bzz holds its key, runs it and
  signs what it says, one reply per mention. It works on its own disk and
  gets a folder, a host or a real command only by your grant. Or bring your
  own process and let it act through bzz's broker, with per-action
  permissions.
- **Apps beside the conversation.** A board, a poll, an approvals queue,
  or whatever your agent writes for the channel: a small Lua script that
  bzz draws in a pane and runs only after you approve its exact bytes. Its
  records are the channel's, shared by its people and their agents.
- **Real media.** Images, avatars and custom emoji render inline in Kitty,
  Sixel and iTerm2 terminals, with a Unicode fallback everywhere else.
  Huddles have real audio.
- **Your keys stay yours.** Keys live in your OS keychain (or an encrypted
  file), never in arguments or environment variables. Or pair bzz with the
  Buzz app and let it keep the key.
- **60 themes**, plus your own in `theme.toml`.

## Install

macOS and Linux:

```sh
curl -fsSL https://getbzz.dev/install.sh | sh
```

It checks this machine first (on Linux: the C library, and `tar` and `xz`
to unpack) and says in one line what is missing, then runs the release's own
installer. The script is [`site/public/install.sh`](site/public/install.sh).
To run only the checks:

```sh
curl -fsSL https://getbzz.dev/install.sh | BZZ_CHECK_ONLY=1 sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/arpagon/bzz/releases/latest/download/bzz-installer.ps1 | iex"
```

Or download an archive from the
[latest release](https://github.com/arpagon/bzz/releases/latest). Every
release ships SHA-256 checksums, a CycloneDX SBOM and GitHub build
attestations; [`docs/releasing.md`](docs/releasing.md) shows how to verify
them.

From source (Rust 1.98, pinned in `rust-toolchain.toml`):

```sh
cargo install --git https://github.com/arpagon/bzz --locked
```

## Get started

Create an identity, add your community and open bzz:

```sh
bzz identity new --label personal
bzz identity list                      # shows the identity's id
bzz community add my-team wss://my-team.communities.buzz.xyz <identity-id>
bzz
```

Back up your key once, to a password-encrypted file:

```sh
bzz identity backup <identity-id> --output ~/bzz-identity.ncryptsec
```

Already have a Buzz key? `bzz identity import` reads it from the terminal.
No credential store on this machine? Add `--backend encrypted-file`.

### Using bzz next to the Buzz app

You don't have to choose. To use the identity you already have in Buzz
(desktop 0.X or Buzz 1.0) without copying its key, pair bzz with the
running app:

```sh
bzz identity pair-desktop --label buzz --relay wss://my-team.communities.buzz.xyz
```

Approve the request in Buzz. The key stays in Buzz, so Buzz must be open
while you use bzz. Buzz signs everyday work for other apps (messages, edits,
reactions, status), but not deleting, reporting, huddles, channels or profile
changes; bzz tells you before you try. To do everything from bzz, import the
key instead. Pairing is not available on Windows.

### Your own agent

An agent bzz creates and runs for you, in your team's Buzz. It needs
Node.js 22.19 or newer and a model you signed in to with
[Pi](https://github.com/earendil-works/pi) (`pi`, then `/login`); bzz
without agents stays one binary.

```sh
bzz agent doctor                    # Node, npm and what is missing; installs nothing
bzz agent new Scout                 # asks: model, instructions, who may wake it, folders
bzz agent add Scout general         # put it in a channel, as its owner
bzz                                 # it answers while bzz is open…
bzz agent run                       # …or without the TUI
```

Mention `@Scout` in that channel and it answers in a thread. By default
only you can wake it, and it can touch nothing of your machine: it has a
disk of its own with a shell and `git` that start no program. You grant the
rest, one thing at a time:

```sh
bzz agent edit Scout --allow-host github.com               # a host its git may reach
bzz agent edit Scout --folder site=$HOME/site:rw           # a folder of yours, as /mnt/site
bzz agent edit Scout --folder work=$HOME/work:rw:exec      # real commands there, each after your yes
bzz agent move Scout --to scout.bzzagent                   # to another bzz, with its memory
```

`--folder` and `--allow-host` each replace their list: name every folder the
agent should keep.

Grok, Codex and Claude Code can run an agent too, as the programs you have
installed (`--harness grok`; Codex and Claude Code are experimental). They
are your real shell at work, and [the guide](docs/agents.md) says what each
can reach.

### An app beside the conversation

An app is a small Lua script for a channel that bzz draws in a pane at the
right. Nothing of it runs until you approve its exact bytes (your own
folder under `bzz app dev` is the one exception), and it reaches no file,
network, process or key: only its pane, your keys while it has them, and
the records of its channel.

```sh
bzz app new tasks                   # ./tasks, from the Board (also: poll, approvals)
bzz app try tasks n "type:Buy milk" enter   # the screen it draws, as text
bzz app add tasks                   # shows it, then asks you to approve it
```

In bzz, select a channel and type `:app open tasks`. `5` gives the app the
keyboard, and `Ctrl-c` gives it back. Mention your agent and ask it for a
change; its version waits for you at the top of the screen (`Ctrl-y` to
read it, `a` to approve) and the app changes in place, keeping its state.
`:app share` offers the app to the channel: another bzz there shows the
offer and runs it after that person approves it. People on Buzz desktop
see none of this but the brief, which bzz posts as a message
([guide](docs/apps.md#buzz-desktop)).

## Keys

Press `?` in bzz for the complete, current list, and the commands after it
(`j`, `PageDown` and `G` scroll it). Every key, command and shell-only
action is in [docs/keys.md](docs/keys.md), generated from bzz's own table.
The tables below are the ones to start with.

| Key | Action |
|---|---|
| `j` `k` · `gg` `G` | Move down, up · jump to top, bottom |
| `1`–`4` · `Tab` · `h` `l` | Focus communities, channels, conversation, context |
| `Space` `Space` | Jump to any channel or DM (`↓` `↑` or `Tab` choose, `Enter` opens) |
| `Enter` | Open the selected thread |
| `i` | Write (`Enter` sends, `Ctrl-j` adds a line) |
| `@` | Mention someone; `Space` completes a unique name, `Tab` picks |
| `r` · `e` · `d` | React · edit your own message · delete it (asks) |
| `u` | Mark unread from the selected message |
| `/` | Search (`from:` `in:` `after:` `before:`; `Ctrl-o` loads older) |
| `Space n` | Inbox: mentions, threads, DMs and drafts |
| `Space a` | Every action for the selected item |
| `Space o` | Themes |
| `y` · `v` | Copy a message · select a range to copy |
| `Ctrl-o` · `Ctrl-v` (writing) | Attach a file · paste a file, image or text |
| `5` · `Space 5` | Give the app the keyboard · zoom it (`Ctrl-c` gives the keyboard back) |
| `Ctrl-y` | Read and answer a command an agent asks to run, or a version of an app |
| `:` | Commands (below) |
| `q` | Back, then quit |
| `Ctrl-K` · `Space k` | Find and run any action, channel, person, app or theme |

In `:agents`: `Space` pauses or resumes an agent, `x` stops the turn it is
running, `i` adds it to the open channel, `l` shows its log, `n` creates
another, `Tab` shows the agents other people run.

## Commands

| Command | Does |
|---|---|
| `:solo <name>` | Start a Solo chat |
| `:section new <name>` | Put the selected channel in a new sidebar section; also `rename`, `delete`, `move <name>`, `remove`, `up`, `down` |
| `:sort alpha\|recent\|default` | Sort the selected channel's sidebar group |
| `:dm` · `:dm add` · `:dm hide` | New direct message · add someone · hide it; also `:dm <npub>…`, `:dm add <npub>` |
| `:inbox` · `:search` | Inbox · search |
| `:agents` · `:agents directory` | Your agents: state, turns, pause, stop · the agents other people run |
| `:agent new [name]` | Create an agent, in a form that asks what `bzz agent new` asks |
| `:approvals` | The command an agent waits to run, or a version of an app (also `Ctrl-y`) |
| `:apps` · `:app open <name>` · `:app close` · `:app zoom` | The installed apps · open one beside the channel · close it · zoom it |
| `:app add <folder>` · `:app new <name>` · `:app dev <folder>` | Propose your folder · write one from a template · run your folder on the channel's records |
| `:app pending\|approve\|refuse\|rollback\|enable\|disable\|remove <name>` | Answer and manage apps |
| `:app share\|unshare\|brief [name]` | Offer the open app to the channel · withdraw it · post its brief |
| `:huddle start\|join\|leave\|end\|mute` | Huddles with audio; `:huddle zoom`, `:huddle style` for the visualizer |
| `:status` · `:status <text>` · `:status clear` | Show your status · set it · clear it |
| `:presence on\|off` | Publish your presence (off by default) |
| `:profile` | The selected author's profile and verified identity |
| `:artifacts` · `:tasks` | Channel artifacts · tasks assigned to you |
| `:report <reason>` · `:tochannel` | Report a message · send a thread reply to the channel |
| `:broker` | Agents whose process you run yourself: permissions, activity, pause |
| `:signin` | Sign in to a community that requires federated identity |
| `:system` | Show or hide System channels |
| `:reconnect` · `:resync` · `:lock` | Recover the connection · reload history · lock the identity |
| `:theme [name]` · `:help` · `:q` | Pick or set a theme · the keys and commands · quit |

Outside the TUI, `bzz --help` lists everything: identities, communities,
themes, media cache, your agents (`bzz agent`: `new`, `list`, `show`, `add`,
`edit`, `pause`, `resume`, `delete`, `doctor`, `run`, `move`, `receive`,
`launch-kit`, `shutdown`, `credential`), apps (`bzz app`: `new`, `check`,
`try`, `dev`, `add`, `pending`, `approve`, `refuse`, `list`, `show`,
`rollback`, `enable`, `disable`, `remove`), the broker for an agent process
of your own (`bzz broker`), diagnostics and shell completions.

## Privacy and security

- Keys are stored in the OS keychain or a passphrase-encrypted file, and
  never accepted as arguments or environment variables.
- bzz publishes nothing about your activity (presence, typing) unless you
  turn it on.
- Message media is fetched only from your community's relay, verified by
  size, type and SHA-256, and never executed; links in messages stay inert.
  Profile pictures load over credential-free HTTPS, and can be turned off
  (`ui.profile_avatars = "off"`).
- An agent you create never holds a key: bzz keeps it and signs for the
  agent. Its model and its tools see only the conversation they were woken
  in. A real command an agent wants to run waits for your yes, and you read
  the whole command first.
- An app runs inside bzz only after you approve its exact bytes, by their
  SHA-256, whoever wrote it: you, your agent or a person of the channel. Its
  script has no file, network, process, environment or key to reach, a call
  that runs too long is stopped, and its memory is capped.
- Diagnostics are local, content-free and owner-only. Telemetry is off unless
  you configure it.
- Workspace DMs are private relay channels, **not** end-to-end encrypted.

Details: [`docs/security.md`](docs/security.md).

## Compatibility

bzz follows the Buzz protocol and is tested against a pinned `block/buzz`
relay revision (`bf3bdd0eaefe39289bf9620594f254c7f68897f5`); the revision and
every supported surface are listed in
[`docs/protocol-compatibility.md`](docs/protocol-compatibility.md). Pairing
with the Buzz app was proven with Buzz desktop 1.0.1. It runs on Linux
(x86_64 and arm64, glibc 2.34 or newer), macOS (Intel and Apple Silicon) and
Windows. On Linux bzz needs no sound library; huddle audio needs PipeWire or
PulseAudio running in your session, as every current desktop has (a machine
with ALSA alone has no huddle audio, and joining says so). Huddle audio on
Windows is experimental. Agents need Node.js 22.19 or newer and were proven on Linux;
on macOS and Windows no agent has been run, only its tests in CI. Apps need
nothing installed; they were run on Linux, and on macOS and Windows only
their tests run, in CI. On Windows a paste reaches an app (and the writing
dock) as keys.

## Documentation

- [Apps](docs/apps.md): write one, try it, install and approve it, the
  pane, your agents writing and changing apps, sharing one in a channel;
  the script contract is [`app-api.md`](docs/app-api.md)
- [Your own agents](docs/agents.md): create one, what it can touch, real
  commands, private repositories, moving it, Grok, Codex and Claude Code
- [Configuration](docs/configuration.md) · [Themes](docs/themes.md) ·
  [Media and terminal graphics](docs/media.md) ·
  [Inbox, DMs and search](docs/inbox-dms-search.md)
- [Troubleshooting](docs/troubleshooting.md) · [Security](docs/security.md)
- [All documentation](docs/README.md): release notes, validation records,
  plans and design decisions for every version.

## Development

```sh
cargo build --locked
cargo test --locked
cargo fmt --check && cargo clippy --locked --all-targets --all-features -- -D warnings
cargo deny check && cargo audit
```

`--no-default-features` builds without huddle audio and without apps (the
Lua interpreter). Debug builds use
separate `bzz-dev` paths and keychain entries, so they never touch your real
identity.

Integration tests against a real Buzz relay start Docker services and are
opt-in:

```sh
BZZ_BUZZ_SOURCE=/path/to/block/buzz ./scripts/test-relay.sh            # also --nip-fi
BZZ_HERDR_PANE=<pane> ./scripts/test-relay.sh --herdr                  # drives the release TUI
```

The agent host is a Node program in `agent-host/` with tests of its own
(Node.js 22.19 or newer):

```sh
cd agent-host && npm ci --ignore-scripts --omit=optional && npm test
```

The app runtime has a bench of its own, which drives the six fixture apps
in a pseudo-terminal (Linux, Python 3.10 or newer):

```sh
./scripts/test-apps-bench.sh --release
```

How changes are proven before release: [`.agents/skills/bzz-prove-it-works`](.agents/skills/bzz-prove-it-works/SKILL.md).

## License

MIT OR Apache-2.0, at your option. See [`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE) and
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

bzz is an independent open-source project compatible with the Buzz protocol.
It is not affiliated with, endorsed by, or sponsored by Block, Inc.
