# bzz v0.17.0 — The first minute, and everything within reach

**Status:** Release candidate, not released. See
[`validation-v0.17.0.md`](validation-v0.17.0.md) for what was proven, how,
and what was not. Every claim below names its proof there, or says it was
not verified.

v0.16.0 let an agent write a small app for a channel. v0.17.0 is about the
first minute: **a person who has never used bzz runs one line, then `bzz
demo`, and is inside a living team in under a second; one key, `Ctrl+K`,
finds and runs anything; a huddle shows who is speaking and whether your
microphone works; and the screen shows where the focus is, what is new, and
what to do when there is nothing.**

Nothing in v0.17.0 changes the protocol, the store or the app contract: the
schema stays 23, apps stay `api = 1`, and the protocol baseline is still
Buzz `bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Rust stays at 1.98.1. The
plan is [`planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md).

## What is new, in the order you meet it

### Try it with `bzz demo`

- **`bzz demo` opens a generated team in your terminal** (plan A1, D2):
  the real bzz, unchanged, against a small relay held in memory on a
  loopback port, with a generated world (Ada, Lin, Sam and the agent Scout)
  written to a store in memory first. It always opens `#team`, and the
  status line says `DEMO · generated, offline` at all times. You can move
  through channels, a thread and a DM, react, search (`rollback`), write a
  message and be answered by a teammate, and mention `@Scout`, who shows as
  typing and answers in the thread of your mention. Offline in a network
  namespace the first full screen came in 0.13 to 0.50 s in 19 runs of 20
  on this machine's release build, and 1.14 s once, cold from a hard disk
  under load ([demo]).
- **The Board in `#launch`** (A2): the shipped Board, approved in the
  demo's own store, with eight cards. `5`, `n`, a title, `]` moves your card
  on. A teammate moves a card while you watch and says so in the channel,
  never while you are typing in the Board; `@Scout add <title>` and `@Scout
  move <title> to <column>` change the Board when Scout answers ([demo2]).
- **A huddle in progress in `#infra`** (A3): a generated recording that
  feeds the visualizer. No sound device is opened (`strace`: no `/dev/snd`,
  no sound server socket) ([demo]).
- **Scout is a verified agent** (A8): it carries the agent mark `◆` and the
  typing line before it answers ([demo]).
- **The demo is closed** (rule 22): no address outside loopback in the
  generated world, its relay answers only under a token and only the viewer
  may write, its folder (`bzz-demo-<pid>-…`, mode 0700) is removed on `q y`
  and on SIGINT, SIGTERM and SIGHUP, and a folder left by `kill -9` is
  removed by the next `bzz demo`. Your real configuration is byte for byte
  the same after a demo in which a theme was chosen and saved ([demo]).
- **The first screen when you have no community** (A4) offers four ways
  in: try the demo (`d`, in the same process), join a community, pair with
  Buzz, import a key; each shows the line to copy ([demo]).

### It starts everywhere

- **On Linux bzz names no sound library** (E1, D3). It speaks the sound
  server's protocol itself (PulseAudio's, which PipeWire answers too), in
  pure Rust. The release binary's `NEEDED` is `libgcc_s`, `libm`, `libc` and
  the loader, and CI fails a build that names another. `bzz --version` and
  `bzz paths` run on clean `debian:stable-slim`, `ubuntu:22.04`,
  `ubuntu:24.04` and `almalinux:9-minimal`, which have no sound library and
  where v0.16.0 stopped at `libasound.so.2` ([sound]).
- **glibc 2.34** (A6, D12): the installer generated for this release
  accepts glibc 2.34, so RHEL 9 and its rebuilds install; the release
  workflow is unchanged ([install]).
- **`getbzz.dev/install.sh` checks the machine first** (A6): a glibc that
  is too old and musl systems are refused with what to do instead, a
  missing `tar` or `xz` with the one command that installs it, and a
  machine with no sound server gets a note that only huddle audio needs one.
  `BZZ_CHECK_ONLY=1` runs only the checks. Seen on seven clean images
  ([install]).
- **The first commands say what to do** (A5): `bzz identity new` on a
  machine with no keychain says so and prints the command with `--backend
  encrypted-file`; it prints the npub too. `:community add` inside bzz names
  the shell command ([find]).

### `Ctrl+K`: one key for everything

- **The palette** (C1, D4) opens with `Ctrl+K` or `Space k`, over the
  workspace. It finds actions, channels and DMs together, then people and
  agents, installed apps, other communities, the 60 themes, and, last, a
  row that searches the messages for what you typed (C12). `>` narrows to
  actions, `#` to channels, `@` to people and DMs, `$` to shell commands,
  `/` to the search row. Each row shows its key from your key map, so the
  palette teaches the keys. An action that cannot run now stays listed with
  its reason, and Enter does nothing. A shell-only action shows its command
  and Enter copies it ([palette], [palette2]).
- **Arguments as steps** (C9): "Start a DM" asks for a person, "Set my
  status" for the text and then an expiry, "React" for an emoji, and so on.
  The footer shows the `:` command being built (`:status --expires 2h In a
  meeting`), and Enter runs exactly that command. Themes preview live as you
  move; Esc puts yours back ([palette2]).
- The filter takes 0.60 ms at the 95th percentile over 4,611 rows in a
  release build ([palette2]).

### Finding your way

- **`?` scrolls and lists the `:` commands** after the keys (C3): `j`,
  PageDown and `G` reach the last row at 80×24 ([find]).
- **The `:` line answers a mistake** (C4): `:hudle` says `did you mean
  :huddle?`; a known command with wrong arguments shows its usage; `:q`,
  `:quit`, `:help` and `:theme [name]` exist ([find]).
- **The channel finder moves** (C5): `Space Space`, then ↑ ↓, Tab,
  Shift-Tab, `Ctrl-n`, `Ctrl-p`; a DM is found by the person's name
  ([find]).
- **The leader panel shows every key** at 80×24 (C6) and appears 300 ms
  after `Space` ([find]).
- **The status line** says `Ctrl-K actions · ? keys` at the right (C2), and
  a notice goes away after about 5 s ([look]).

### The look

- **Quiet frames, visible focus** (L2): unfocused frames are in the theme's
  muted colour; the focused one is in the primary colour, with its title as
  a label. Keys left the titles for the frame's foot, in colour ([look]).
- **The status line** (L3): the mode as a label, the channel, `● online` or
  `● offline`; no protocol words ([look]).
- **`NO_COLOR` and terminals without true colour** (L1): with `NO_COLOR`
  no cell carries a colour, and the focused pane has a heavy frame and a
  reversed title. Without true colour every colour goes to the nearest of
  256 or 16 (rule 21) ([look]).
- **Overlays dim what is behind them** (L4); **empty and offline states say
  what to do next**, and the approval banner has a row of its own instead of
  covering the panes' titles (L5); **a `── new ──` line** marks the first
  unread message and **the unread count** stands beside each channel (L6)
  ([look]).

### The huddle

- **One lane per person, your microphone last** (B3, D7), in the huddle
  panel: the last seconds of each voice, the right edge is now. Your own
  lane moves while you are muted, says that nobody hears you, and turns
  amber with `muted!` when you speak muted ([viz]).
- **A strip in the status line** while the panel is closed (B2): who
  speaks, and `you're muted` in amber ([viz]).
- **A person who joins after you has a name** (B1): bzz now reads the
  relay's `joined`, `left` and `roster` messages. Seen on the pinned relay
  ([viz]).
- **The zoomed view and its styles** (B5, B6): `z` in the huddle panel (or
  `:huddle zoom`) gives the visualizer the window; `v` (or `:huddle style`)
  opens a picker that previews lanes, spectrum bars and the orb live; Enter
  keeps the style, Esc puts the saved one back ([viz2]).
- **The microphone check** (B7): the join confirmation shows your lane
  before anything is sent, and after 3 s of nothing names the device and how
  to pick another ([viz2]). Seen with a fake device only.
- **No frame when nothing moves** (B4, rule 20): none while the panel is
  hidden, none 2 s after silence; `[ui] motion = "off"` gives a still meter
  ([viz], [viz2]).
- **On Linux, through PipeWire or PulseAudio** (E1): a call between two
  clients on the pinned relay carried a tone from one microphone to the
  other's speaker, each client on its own sound server in a container. The
  round trip through PipeWire was 13 to 26 ms there ([sound]).

### The Board

- **It keeps what it did not change** (F1): a card another client wrote
  with a `null`, an empty object, a list with a `null` or a very large
  number comes back as written, but its column ([board]).
- **Your own quick moves do not conflict** (F2): 25 moves before any reply
  are sent one after another, none refused. The Board says `Saving...`
  until the last is answered ([board]).
- **It says what happened** (F3): `This card was deleted by <name>.` when
  someone deleted the card you moved, and `Loaded N cards.` counts the cards
  it shows ([board]).
- **Numbers arrive as the relay holds them** (F4), to an app and to an
  agent's `records` tool ([board]).
- **A new name arrives** (F5): bzz now receives profile changes of its
  community while it runs, and asks once a run for each person it shows
  ([board]).

### Windows

- **Shifted keys work** (W1): `?`, `:`, `@`, `!` and every other shifted
  symbol reach their keys. On v0.16.0 Windows reported them with Shift and
  bzz matched nothing: `?` and `:` did nothing there. Keys now enter bzz in
  one form on every platform; Linux and macOS keys are unchanged
  ([windows]).
- **Characters the layout cannot type** (`¿ ¡ € °` on a US layout, which
  the console sends as Alt codes) are no longer lost ([windows]).
- **A paste into the writing dock keeps its lines** (W2): a line break with
  more keys waiting behind it is a new line, not a send ([windows]).
- **Shift+Tab moves the focus back**, on every platform ([windows]).

## What changes for people who already use bzz

Every key and `:` command of v0.16.0 does what it did, but the three that
decisions changed (rule 19; `tests/v016_pins_test.rs` pins the list)
([find]).

| What | v0.16.0 | v0.17.0 |
|---|---|---|
| A bare `:status` (D6) | cleared your status | shows it, with when it ends; `:status clear` clears ([find]) |
| `d` in the workspace (D5) | nothing | deletes your selected message, after asking ([find]) |
| `u` in the workspace (D5) | nothing | marks the channel unread from the selected message ([find]). A key map of yours that binds `u` keeps its binding ([release]) |
| The leader panel (`Space`) | dropped the sequence 750 ms after `Space` | appears after 300 ms and stays until a key ([find]) |
| `:q`, `:quit` | unknown command | ask before quitting, as `q` does ([find]) |
| `:theme <name>` | unknown command (only `:theme reload`) | sets and saves that theme; an unknown name lists the themes ([find]) |
| An unknown `:` command | `unknown command: …` | names the nearest command, or the usage ([find]) |
| Shift+Tab | did nothing | moves the focus back ([windows]) |
| Unfocused frames | the theme's `Border` colour | the theme's `Muted` colour. A theme of yours that set only `[highlight.Border]` still loads, and no longer colours them: set `[highlight.PaneBorder]` for that ([look], [release]) |
| Titles of panes and overlays | carried their keys (`Board · Ctrl-c leave`) | the keys are at the frame's foot ([look]) |
| The status line | `online  HALFBLOCKS  ? help · q quit` | ` NORMAL  #channel  ● online … Ctrl-K actions · ? keys`; a notice goes after about 5 s ([look]) |
| Inside tmux without `COLORTERM` (`TERM=tmux-256color`) | true colour | 256 colours; set `COLORTERM=truecolor` for true colour ([look]) |
| `NO_COLOR` | ignored | honoured ([look]) |
| During a call | `🎧 huddle · N in it · you: muted · :huddle` in the status line | the huddle strip: who speaks, `you're muted` ([viz]) |
| Joining a huddle | the confirmation, then the devices | the confirmation opens your microphone (and speaker, which plays nothing) to show your lane, before anything is sent ([viz2]; not seen on a real device) |
| Huddle audio on Linux | ALSA, through `libasound.so.2`, which bzz needed to start at all | PipeWire or PulseAudio in your session. **A machine with ALSA alone has no huddle audio** (D3), and joining says so in one line ([sound]) |
| Devices saved by v0.16.0 (`hw:CARD=PCH,DEV=0`) | ALSA's names | open that card's device when the server says which it is, else the default, and the join says once which name was not found ([sound], [release]) |
| `:huddle devices` | ALSA's names | the sound server's descriptions; `:huddle input <description>` takes one ([sound]) |
| The Board | `Saved.` at the first reply; `Save failed.`; counted cards it did not show; a burst of moves was refused | `Saving...` until the last; who deleted a card; counts what it shows; its own writes queued, one at a time ([board]) |
| Profiles | a name changed elsewhere reached bzz only by chance | every client receives the profile changes of its community ([board]) |
| `:dm` | `:dm`, `:dm add`, `:dm hide` | also `:dm <npub>…` (a DM with these people) and `:dm add <npub>` ([palette2]) |
| `:huddle` | `start`, `join`, `leave`, `end`, `mute`, devices | also `zoom` and `style [lanes\|bars\|orb]`; in the panel, `z` and `v` ([viz2]) |
| Linux | glibc 2.35 or newer | glibc 2.34 or newer; getbzz.dev's installer checks the machine before it downloads ([install]) |
| Windows | `?`, `:` and other shifted symbols did nothing | they work; a paste keeps its lines in the writing dock ([windows]) |
| Settings | — | two new ones, `[ui] motion` and `[ui] huddle_style`, written only when you choose a value ([configuration](configuration.md#v0130-settings)) |

## Requirements

- **Linux** (x86_64 and arm64) with glibc 2.34 or newer: Ubuntu 22.04,
  Debian 12, Fedora 35, RHEL 9 or newer. **No sound library is needed.**
  Huddle audio needs PipeWire or PulseAudio running in your session.
- **macOS** (Apple Silicon and Intel) and **Windows** (x86_64), as before.
- Your agents need what they needed in v0.16.0: Node.js 22.19 or newer for
  a `pi-durable` agent.
- No build for Alpine or other musl systems yet.

## Compatibility and upgrade

- **No migration.** The schema stays 23 (D11). A real v0.16.0 profile (a
  local agent, the Board installed, approved and open in a channel with an
  agent's version waiting, its cards, saved huddle devices, a theme and a
  key map of the user's own) opens with every table's rows as v0.16.0 left
  them, and no backup is made, because there is nothing to migrate
  (`scripts/upgrade-test.sh`; [release]).
- **Going back to v0.16.0** needs nothing but the old binary, as long as you
  did not set `[ui] motion` or `[ui] huddle_style`: v0.16.0 refuses a
  setting it does not know, so delete those two lines from `config.toml`
  (`bzz paths` prints where) first. Saved again by v0.17.0, settings you did
  not change are the bytes v0.16.0 wrote, and v0.16.0's `bzz check` accepts
  them ([release]).
- **The agent host is unchanged** (`agent-host/` is v0.16.0's), so a
  `pi-durable` agent's install is kept: nothing is installed again on
  first use.
- **Apps:** `api = 1` as before. `app-api.md` gained two paragraphs: an
  update keeps what Lua cannot show, and an app's own writes to one record
  do not conflict ([board]).
- **The protocol:** the same Buzz revision. The one change is what bzz asks
  for: its global stream now carries kind 0 profiles (F5)
  ([`protocol-compatibility.md`](protocol-compatibility.md)).
- **Dependencies:** on Linux `pulseaudio` 0.3.2 (MIT), pinned, with
  `enum-primitive-derive` 0.3.0 (MIT) and `futures` 0.3.33, takes the place
  of `cpal` and ALSA, which macOS and Windows keep. `serde_json` turns on
  its `float_roundtrip` feature (F4), which adds no crate. `cargo deny
  check` and `cargo audit` pass with the five allowed warnings of v0.16.0
  ([sound], [board]).
- **Size:** the demo added 326,528 bytes to the Linux release binary, of
  the plan's 350,000 ([demo], [demo2]), and the sound server's protocol
  335 KB ([sound]). The size of the final binary was not measured.
- **Benchmarks** against v0.16.0 were not run.

## Known limits

What v0.17.0 does not do, and what was not seen. Each is in
[`validation-v0.17.0.md`](validation-v0.17.0.md) with its reason.

**Where it ran**

- **No one has opened this version in a terminal on macOS.** CI builds and
  tests it there.
- **Windows was driven in a console over SSH** (a ConPTY, from Linux
  through tmux), not by a person in Windows Terminal. What only a person at
  the machine can check is listed in [`e2e-windows.md`](e2e-windows.md):
  AltGr and Caps Lock on a real keyboard, a paste with `Ctrl+V` and a right
  click, the look, clicks ([windows]).
- **On Windows a paste still arrives as keys**: the terminal library gives
  no paste event there. In the writing dock its lines stay lines; elsewhere,
  and in an app, its characters act as keys ([windows]).
- **PipeWire was proven in containers** (Debian and AlmaLinux 9), each with
  a null sink, not on a desktop with a real card, WSLg, or a sound server
  restarted during a call. The mapping of a v0.16.0 device name to a real
  card's device was tested with the server's answers written by hand
  ([sound]).
- **The microphone check** was seen with a fake device only, and it names
  "the default microphone", not the device the server chose ([viz2]).
- **The look** was read in tmux and in test buffers. No graphics terminal
  (kitty, Ghostty, WezTerm, iTerm2, Terminal.app, Windows Terminal) and no
  Linux console was looked at, for the frames, the colours or the huddle's
  braille and block characters ([look], [viz], [viz2]).
- Linux on arm64 is built for the release and runs no test, as before.

**The demo**

- **The demo's agent is scripted.** Scout answers from a script, and the
  screen says the data is generated; the teammates answer a few times, then
  go quiet ([demo], [demo2]).
- `Ctrl-C` as a key does not leave the demo, as it does not leave bzz
  (rule 19); `q` then `y` does ([demo]).
- After `kill -9` the demo's folder stays until the next `bzz demo`; on
  macOS and Windows that one removes only folders older than a day. The
  demo was not run on macOS or Windows ([demo]).

**The palette**

- **The palette does not open while an app has the keyboard**: the app has
  every key but `Ctrl-c` (`app-api.md`). `Ctrl-c`, then `Ctrl+K`
  ([palette2]).
- In the writing dock `Ctrl+K` stays "delete to the end of the line". A
  terminal that keeps `Ctrl+K` (VS Code's, with chords) still has `Space k`
  ([palette]).

**Not in this version** (wave 2 of the plan; each may still land):

- Joining a community inside bzz (A9).
- One registry of actions behind the key map, the help and the palette
  (C8), a footer that follows the focus (C10), and completion on the `:`
  line (C11).
- Motion beyond the huddle (L7, L8) and a loop that sleeps until its next
  deadline (L9). At rest bzz still wakes about 40 times a second with
  nothing to draw (the plan's "Evidence"), and in ten seconds of rest it
  wrote 0 to 50 bytes, an empty frame's cursor sequence, against rule 20's
  none ([demo2]).
- Layout by width (L10), the mouse beyond what v0.16.0 did (L11), `Esc` in
  the timeline (L12), and the small fixes of F7.
- Frames that redraw only the visualizer (B8) ([viz2]).
- macOS checked by a person (W3).

**Still as v0.16.0 said:** apps were run in a terminal on Linux only; an
agent's tests run on Unix only; Grok was not seen to write an app; Buzz
desktop shows nothing of an app but its brief; one app shown in a channel at
a time ([`release-v0.16.0.md`](release-v0.16.0.md), "Known limits").

**Pending acceptance**

- The installer on clean machines with this release published: until then
  the full install was seen with v0.16.0's archives only ([install]).
- CI on the four platforms, and two consecutive green runs on `main` (F6),
  which run when the branch is pushed ([install]).
- The owner's release acceptance: `bzz demo` from the published installer;
  `Ctrl+K` to do three things he did not know the key for; a huddle with one
  other person where he sees both lanes and his muted microphone.

## Verify the downloads

Every release ships SHA-256 checksums, a CycloneDX SBOM and GitHub build
attestations ([`releasing.md`](releasing.md)):

```sh
sha256sum -c sha256.sum --ignore-missing
gh attestation verify bzz-x86_64-unknown-linux-gnu.tar.xz --repo arpagon/bzz
gh attestation verify bzz.cdx.xml --repo arpagon/bzz
tar xf bzz-x86_64-unknown-linux-gnu.tar.xz && ./bzz-x86_64-unknown-linux-gnu/bzz --version   # bzz 0.17.0
```

On Linux, `readelf -d bzz | grep NEEDED` names no sound library.

[demo]: validation-v0.17.0.md#v0170-validation--task-demo-a1-a4-a8
[demo2]: validation-v0.17.0.md#v0170-validation--task-demo2-a2-and-the-demos-loose-ends
[install]: validation-v0.17.0.md#v0170--task-install-a6-a7-f6
[viz]: validation-v0.17.0.md#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4
[viz2]: validation-v0.17.0.md#v0170--viz2-the-zoomed-huddle-its-styles-the-microphone-check-b5-to-b7
[palette]: validation-v0.17.0.md#v0170-validation-the-palette-c1
[find]: validation-v0.17.0.md#v0170-validation--task-find-c3-to-c7-a5-rule-19
[palette2]: validation-v0.17.0.md#v0170-validation-arguments-as-steps-and-more-to-find-c9-c12
[look]: validation-v0.17.0.md#v0170--task-look-l1-to-l6-c2
[sound]: validation-v0.17.0.md#v0170-validation-sound-e1-decision-d3
[board]: validation-v0.17.0.md#v0170--task-board-f1-f2-f3-f4-f5
[release]: validation-v0.17.0.md#v0170--task-release-the-upgrade-from-v0160-the-version-f8
[windows]: validation-v0.17.0.md#v0170-validation-windows-task-windows-plan-w1-and-w2
