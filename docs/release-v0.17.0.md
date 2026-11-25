# bzz v0.17.0 — The first minute, and everything within reach

**Status:** Release candidate, not released. See
[`validation-v0.17.0.md`](validation-v0.17.0.md) for what was proven, how,
and what was not. Every claim below names its proof there, or says it was
not verified.

v0.16.0 let an agent write a small app for a channel. v0.17.0 is about the
first minute: **a person who has never used bzz runs one line, then `bzz
demo`, and is inside a living team in under a second; joins their own
community from inside bzz; one key, `Ctrl+K`, finds and runs anything; a
huddle shows who is speaking and whether your microphone works; and the
screen shows where the focus is, what is new, and what to do when there is
nothing.**

Nothing in v0.17.0 changes the protocol, the store or the app contract: the
schema stays 23, apps stay `api = 1`, and the protocol baseline is still
Buzz `bf3bdd0eaefe39289bf9620594f254c7f68897f5`. Rust stays at 1.98.1. The
plan is [`planning/2026-11-24/v0.17.0.md`](planning/2026-11-24/v0.17.0.md);
its last section says how the build departed from it.

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
  If you close the Board with its own `q`, the status line says `Demo: :app
  open board`, which brings it back ([tidy]).
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
- **`bzz help` lists `demo` first**, and `bzz demo --help` says in two
  lines what it is, that nothing is read or saved, and how to leave (`q`,
  then `y`). Every shell's completion offers it ([tidy]).

### It starts everywhere

- **On Linux bzz names no sound library** (E1, D3). It speaks the sound
  server's protocol itself (PulseAudio's, which PipeWire answers too), in
  pure Rust. The release binary's `NEEDED` is `libgcc_s`, `libm`, `libc` and
  the loader, and CI fails a build that names another. `bzz --version` and
  `bzz paths` run on clean `debian:stable-slim`, `ubuntu:22.04`,
  `ubuntu:24.04` and `almalinux:9-minimal`, which have no sound library and
  where v0.16.0 stopped at `libasound.so.2` ([sound]).
- **It needs no certificate store to start** ([principal]). On a machine
  that has none (a minimal container), bzz could not build its HTTP client
  at all, and `bzz demo`, which speaks plain HTTP to a relay inside its own
  process, ended with "network error: builder error". bzz now builds the
  client with no trusted certificate there: the demo and a relay on the same
  machine work, and an HTTPS address fails saying its certificate cannot be
  verified, which is the truth on that machine until `ca-certificates` is
  installed. Found on the release candidate, in a clean container with
  nothing installed and no network.
- **glibc 2.34** (A6, D12): the installer generated for this release
  accepts glibc 2.34, so RHEL 9 and its rebuilds install; the release
  workflow is unchanged ([install]).
- **`getbzz.dev/install.sh` checks the machine first** (A6): a glibc that
  is too old and musl systems are refused with what to do instead, a
  missing `tar` or `xz` with the one command that installs it, and a
  machine with no sound server gets a note that only huddle audio needs one.
  `BZZ_CHECK_ONLY=1` runs only the checks. Seen on seven clean images
  ([install]).

### Joining your community without leaving bzz

- **`:join`** (A9) opens a form that takes the community's address or the
  invite link an admin sent you. bzz asks the address what it is and says
  so (`✓ Buzz Relay · a Buzz community at …`, `open: anyone who signs in may
  join`), makes you an identity (in the keychain, or in a file with a
  passphrase typed twice and drawn as dots when there is no keychain), and
  lets you in: it opens the first channel you are in, or the finder on the
  open ones. Nothing is typed in a shell but `bzz` ([join]).
- **A community that lets in members only** shows your id, the npub to
  send to an admin, and waits: it asks again on its own (15 s, 30 s, then
  every minute, or on `r`) and comes in by itself once the admin adds you,
  without a restart. `Esc` leaves the wait running; `:join` shows it again
  ([join]).
- **Each failure says what happened and what to try**: nothing answers at
  the address, no community there, not a Buzz relay, `ws://` away from this
  machine, the relay refusing your identity. Nothing is half made: the
  community is written only once the relay lets you in, and an identity this
  join made is taken back unless you keep it ([join]).
- The form can also pair with Buzz desktop, import a key you have, claim an
  invite link, and sign in through the browser where the community asks for
  it; of these only the invite was run, on a scripted relay (see "Known
  limits") ([join]).
- **The first screen when you have no community** (A4) offers four ways
  in: try the demo (`d`, in the same process), join a community (`j`), pair
  with Buzz (`p`), import a key (`k`); the last three open the join form
  ([demo], [join]). `:community add` opens it too.
- **The first commands in a shell say what to do** (A5): `bzz identity new`
  on a machine with no keychain says so and prints the command with
  `--backend encrypted-file`; it prints the npub too ([find]).

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

- **The status line's hint follows the focus** (C10): on a message `Enter
  thread · r react · i write · Ctrl-K actions`, in the channels `Enter open ·
  Space Space find · i write`, in the composer `Enter send · Ctrl-J newline ·
  Esc close`, in the Inbox `o open · i reply · m read`, and so on: up to
  three keys that can run there, each from your own key map, then the
  palette's key where it applies. A narrow window takes fewer
  ([registry]). A click on the
  hint opens the palette ([layout]).
- **Tab completes the `:` line** (C11): `:hu`, Tab gives `:huddle`, Tab
  again its sub-commands in turn (`start`, `join`, …), then what each takes
  (themes, installed apps, channels, fixed words); Shift-Tab goes back.
  Under the line: what Tab offers and the command's usage, or for a wrong
  word the nearest command ([registry]).
- **`?` scrolls and lists the `:` commands** after the keys (C3): `j`,
  PageDown and `G` reach the last row at 80×24. The keys are grouped as the
  palette groups them (Message, Go, View, Inbox, Channel, …) and named as
  the palette names them ([find], [registry]).
- **[`docs/keys.md`](keys.md)** lists every key by group with where it
  works and its `:` form, every command, and what only the shell does. It is
  written by bzz from the same table as the key map, the help, the leader
  panel, the actions menu and the palette (C8), and a test fails when it is
  stale ([registry]).
- **The `:` line answers a mistake** (C4): `:hudle` says `did you mean
  :huddle?`; a known command with wrong arguments shows its usage; `:q`,
  `:quit`, `:help` and `:theme [name]` exist ([find]).
- **The channel finder moves** (C5): `Space Space`, then ↑ ↓, Tab,
  Shift-Tab, `Ctrl-n`, `Ctrl-p`; a DM is found by the person's name
  ([find]).
- **The leader panel shows every key** at 80×24 (C6), appears 300 ms after
  `Space`, and names each next key by its action's title ([find],
  [registry]).
- A notice in the status line goes away after about 5 s (C2) ([look]).

### The look, and motion

- **Quiet frames, visible focus** (L2): unfocused frames are in the theme's
  muted colour; the focused one is in the primary colour, with its title as
  a label. Keys left the titles for the frame's foot, in colour ([look]).
- **The status line** (L3): the mode as a label, the channel, `● online` or
  `● offline`; no protocol words ([look]).
- **Code in a sentence can be read** ([principal]). A key or a command
  inside a message (`` `k` ``, `` `:join` ``) is drawn in the theme's
  accent. It was muted and dim, as a block of code still is, and was the
  hardest text of a message to read: the demo's welcome names its keys
  that way. A new theme group, `MarkdownInlineCode`, carries it
  ([themes](themes.md#semantic-groups)).
- **A person's badge never looks like the agent's mark** ([principal]).
  The shape in a text badge (`[●L]`) comes from the person's key, and one
  of the four was `◆`, which beside a name means "agent" everywhere in
  bzz. That shape is now `▼`.
- **`NO_COLOR` and terminals without true colour** (L1): with `NO_COLOR`
  no cell carries a colour, and the focused pane has a heavy frame and a
  reversed title. Without true colour every colour goes to the nearest of
  256 or 16 (rule 21) ([look]).
- **Overlays dim what is behind them** (L4); **empty and offline states say
  what to do next**, and the approval banner has a row of its own instead of
  covering the panes' titles (L5); **a `── new ──` line** marks the first
  unread message and **the unread count** stands beside each channel (L6)
  ([look]).
- **Motion** (L7, L8, D9): bzz's own small module, on bzz's clock. A toast
  slides in above the composer, stays 3 s and goes, for bzz's own
  confirmations (an attachment saved or removed, messages copied, the
  sidebar updated). An arriving message is tinted for 250 ms; a rising
  unread count flashes once; the focus colour runs round the pane that takes
  the focus in 150 ms; an overlay unfolds in 150 ms and folds in 80 ms;
  placeholder rows shimmer while a channel's history loads, for 10 s at most
  ([motion]).
- **One setting turns it down or off**: `[ui] motion = "full"` (the
  default), `"calm"` (keeps what informs: the toast, the tint, the flash)
  or `"off"` (the end of every animation at once), and the environment
  variable `BZZ_NO_MOTION` turns it off whatever the file says
  ([configuration](configuration.md#v0130-settings), [motion]).
- **bzz rests when nothing changes** (L9, rule 20). The loop no longer
  ticks; it sleeps until its next deadline, and at rest that is the store's
  refresh once a second. Measured by task `motion` on this machine (Linux
  x86_64, debug builds, `bzz demo` in `#team` at 160×45, 10 s of nothing,
  with other builds loading the machine): wake-ups of the whole process
  fell from 192 to 227 a second to 46 to 55, and CPU in 10 s from 633 to
  638 ms to 80 to 86 ms; the principal measured 70 ms on the merged build.
  No frame is drawn at rest; the 25 to 50 bytes in those 10 s are the
  terminal's answers and the cursor, before and after. A key that waits is
  handled before a frame of an animation ([motion], [the principal's own
  checks][principal]).

### The window at any width, the conversation alone, the mouse

- **Panes by the window's width** (L10): the conversation keeps 38 columns
  while anything is beside it; panes drop by priority as the window
  narrows (the one with the focus first, then the right pane asked for
  last, the sidebar, the other right pane, the community rail) and come
  back as it widens. The conversation's frame names what is hidden and the
  key that brings it (`hidden · channels 2 · thread 4`). A focused pane that
  cannot sit beside the conversation takes its place, and `3` gives it
  back. Text wraps to the pane's real width and loses no character
  ([layout]).
- **`z` shows the conversation alone** with the writing dock; the frame
  says `hidden · the other panes z`, and `z` again restores ([layout]).
- **The mouse** (L11): a click focuses; a double click on a message opens
  its thread; dragging the border between panes resizes the sidebar or the
  thread and saves the width; the wheel scrolls the pane under the pointer
  and leaves the focus where it is. Everything the mouse does has a key
  ([layout]).
- **`Esc` no longer leads to quitting** (L12): it closes what is open, and
  with nothing open says `nothing to close · q quits` ([layout]).
- **Small fixes** (F7): a huddle's backing channel is no longer listed in
  the sidebar, the finder or the palette (`:system` shows it); a DM's
  composer says `message @Scout`; `:tochannel` says what it did and clears
  the thread's draft; `:huddle end` with no huddle says `no huddle is
  running here` ([layout]).

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
  round trip through PipeWire was 13 to 26 ms there ([sound]). With no
  sound server, joining says `huddle audio needs PipeWire or PulseAudio
  running`, whole in an 80-column status line, and `:huddle devices` says
  why its lists are empty ([tidy]).

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
- The release binary CI builds for Windows was walked through `bzz demo`,
  the help, the palette, Tab on the `:` line, the Board and the zoomed
  huddle in a console over SSH ([the principal's own checks][principal]).

## What changes for people who already use bzz

Every key and `:` command of v0.16.0 does what it did, except the four that
decisions changed: a bare `:status`, `d`, `u` and `Esc` (rule 19;
`tests/v016_pins_test.rs` pins every v0.16.0 binding and names each change)
([find], [layout]).

**Keys and commands**

| What | v0.16.0 | v0.17.0 |
|---|---|---|
| A bare `:status` (D6) | cleared your status | shows it, with when it ends; `:status clear` clears ([find]) |
| `d` in the workspace (D5) | nothing | deletes your selected message, after asking ([find]) |
| `u` in the workspace (D5) | nothing | marks the channel unread (the plan said "from the selected message"; it is the whole channel) ([find]). A key map of yours that binds `u` keeps its binding ([release]) |
| `Esc` in the workspace and the Inbox, with nothing open (L12) | asked to quit | says `nothing to close · q quits`; `q` asks, as before. `Esc` still closes what is open ([layout]) |
| `z` in the workspace | nothing | the conversation alone; `z` again restores ([layout]) |
| A key of yours against a key v0.17.0 added | — | yours wins in that scope: a v0.16.0 map that binds a sequence such as `z i` still loads and keeps it, though `z` is now a key of its own. Between v0.16.0's own keys an ambiguity is still refused ([layout]) |
| The leader panel (`Space`) | dropped the sequence 750 ms after `Space` | appears after 300 ms and stays until a key; names each next key by its action's title, grouped ([find], [registry]) |
| `:community add` | `use the CLI to add/remove communities` | opens the join form, as `:join` does; `:community add <address>` fills it in ([join]) |
| `:q`, `:quit` | unknown command | ask before quitting, as `q` does ([find]) |
| `:theme <name>` | unknown command (only `:theme reload`) | sets and saves that theme; an unknown name lists the themes ([find]) |
| An unknown `:` command | `unknown command: …` | names the nearest command, or the usage ([find]) |
| Tab on the `:` line | nothing | completes; under the line, what Tab offers and the usage, so the prompt is taller ([registry]) |
| Shift+Tab | did nothing | moves the focus back ([windows]) |
| `:dm` | `:dm`, `:dm add`, `:dm hide` | also `:dm <npub>…` (a DM with these people) and `:dm add <npub>` ([palette2]) |
| `:huddle` | `start`, `join`, `leave`, `end`, `mute`, devices | also `zoom` and `style [lanes\|bars\|orb]`; in the panel, `z` and `v` ([viz2]) |
| `:huddle end` with no huddle | "only the huddle's creator can end it" | `no huddle is running here` ([layout]) |
| `:tochannel` | sent the thread's draft to the channel and kept the draft | says `sent to #… as a new message · the thread's draft is cleared`, and clears it ([layout]) |

**What you see**

| What | v0.16.0 | v0.17.0 |
|---|---|---|
| Widths at which panes show (L10) | the conversation kept 30 columns; an app docked from 72, beside the sidebar from 100; thread and app from 116, with the sidebar from 144, with the rail from 166; a thread from 74 at its own width, beside the sidebar from 102; under 70 columns the conversation alone | the conversation keeps **38**; an app docks from **80** (at 72 a focused app takes the window, an unfocused one is hidden with `app hidden · 5`), beside the sidebar from **108**; thread and app from **124**, with the sidebar from **152**, with the rail from **174**; a thread from **68** at 30 wide, beside the sidebar from **110**; the same rules at every width, and the frame names what is hidden. With one community the rail shows from 100 columns ([layout]) |
| A double click on a message | toggled the thread, and closed another message's thread | opens the clicked message's thread, and keeps it open if it is the one open ([layout]) |
| Unfocused frames | the theme's `Border` colour | the theme's `Muted` colour. A theme of yours that set only `[highlight.Border]` still loads, and no longer colours them: set `[highlight.PaneBorder]` for that ([look], [release]) |
| Code inside a sentence of a message | muted and dim, as a block of code | the theme's accent, not dim (`MarkdownInlineCode`). A theme of yours that sets `[highlight.MarkdownCode]` and not the new group keeps both alike ([principal]) |
| The text badge of one person in four | `[◆A]` | `[▼A]`; `◆` is the agent's mark only ([principal]) |
| Titles of panes and overlays | carried their keys (`Board · Ctrl-c leave`) | the keys are at the frame's foot ([look]) |
| The status line | `online  HALFBLOCKS  ? help · q quit` | ` NORMAL  #channel  ● online` and, at the right, the keys of where the focus is and `Ctrl-K actions`; a notice goes after about 5 s ([look], [registry]) |
| bzz's own confirmations (an attachment saved or removed, messages copied, the sidebar updated) | a notice in the status line | a toast above the composer, for 3 s. An app's own words (`Saved.`) are still the app's, in its pane ([motion]) |
| The help, the leader panel, the actions menu | their own wording and order | the registry's titles and groups ("Open Inbox", "Go to channel or DM", "Write a message", "Delete my message") ([registry]) |
| Inside tmux without `COLORTERM` (`TERM=tmux-256color`) | true colour | 256 colours; set `COLORTERM=truecolor` for true colour ([look]) |
| `NO_COLOR` | ignored | honoured ([look]) |
| A huddle's backing channel (`huddle-` and eight hex digits) | listed in the sidebar | hidden; `:system` shows it ([layout]) |
| A DM's composer | `message #Scout` | `message @Scout` ([layout]) |
| During a call | `🎧 huddle · N in it · you: muted · :huddle` in the status line | the huddle strip: who speaks, `you're muted` ([viz]) |
| Joining a huddle | the confirmation, then the devices | the confirmation opens your microphone (and speaker, which plays nothing) to show your lane, before anything is sent ([viz2]; not seen on a real device) |
| The first screen with no community | — | four ways in; `j`, `p` and `k` open the join form ([demo], [join]) |
| `bzz help` | `demo` did not exist | `demo` first ([tidy]) |

**Sound, the Board, the platform, settings**

| What | v0.16.0 | v0.17.0 |
|---|---|---|
| Huddle audio on Linux | ALSA, through `libasound.so.2`, which bzz needed to start at all | PipeWire or PulseAudio in your session. **A machine with ALSA alone has no huddle audio** (D3), and joining says `huddle audio needs PipeWire or PulseAudio running` ([sound], [tidy]) |
| Devices saved by v0.16.0 (`hw:CARD=PCH,DEV=0`) | ALSA's names | open that card's device when the server says which it is, else the default, and the join says once which name was not found ([sound], [release]) |
| `:huddle devices` | ALSA's names | the sound server's descriptions; `:huddle input <description>` takes one; with no server, says why the lists are empty ([sound], [tidy]) |
| The Board | `Saved.` at the first reply; `Save failed.`; counted cards it did not show; a burst of moves was refused | `Saving...` until the last; who deleted a card; counts what it shows; its own writes queued, one at a time ([board]) |
| Profiles | a name changed elsewhere reached bzz only by chance | every client receives the profile changes of its community ([board]) |
| Linux | glibc 2.35 or newer | glibc 2.34 or newer; getbzz.dev's installer checks the machine before it downloads ([install]) |
| Windows | `?`, `:` and other shifted symbols did nothing | they work; a paste keeps its lines in the writing dock ([windows]) |
| Settings | — | two new ones, `[ui] motion` and `[ui] huddle_style`, written only when you choose a value other than the default ([configuration](configuration.md#v0130-settings)) |

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
  them ([release]). A pane width you drag with the mouse is the setting
  v0.16.0 already had.
- **The agent host is unchanged** (`agent-host/` is v0.16.0's), so a
  `pi-durable` agent's install is kept: nothing is installed again on
  first use.
- **Apps:** `api = 1` as before. `app-api.md` gained two paragraphs: an
  update keeps what Lua cannot show, and an app's own writes to one record
  do not conflict ([board]).
- **The protocol:** the same Buzz revision. The one change is what bzz asks
  for: its global stream now carries kind 0 profiles (F5)
  ([`protocol-compatibility.md`](protocol-compatibility.md)). Joining reads
  the relay's NIP-11 document and claims an invite as Buzz desktop does
  ([join]).
- **Dependencies:** on Linux `pulseaudio` 0.3.2 (MIT), pinned, with
  `enum-primitive-derive` 0.3.0 (MIT) and `futures` 0.3.33, takes the place
  of `cpal` and ALSA, which macOS and Windows keep. `serde_json` turns on
  its `float_roundtrip` feature (F4), which adds no crate. `cargo deny
  check` and `cargo audit` pass with the five allowed warnings of v0.16.0
  ([sound], [board]).
- **Size:** the demo added 326,528 bytes to the Linux release binary, of
  the plan's 350,000 ([demo], [demo2]), and the sound server's protocol
  335 KB ([sound]). The size of the final binary was not measured.
- **A real model wrote an app once on this version, and its script missed
  one case.** One run of v0.16's gate on the candidate, allowed by the
  owner, with Pi Durable and `openai-codex/gpt-6.1-sol`: the agent proposed
  version 2 of the Board 247 s after the mention, it waited for approval
  with its brief, was approved, and the agent's next turn was told; one
  reply per mention held. What bzz promises of the harness held. The
  model's script then passed 16 of the bench's 17 `board-priority` cases
  at both sizes: it searched the cards it had instead of asking the
  service, which one of the Board's own cases checks. On v0.16.0 the same
  gate passed 17 of 17, twice. The coding harnesses were not run again
  ([principal]).
- **Benchmarks** against v0.16.0 were not run.
- **Building from source:** a debug build (`cargo build`, `cargo run`)
  builds the `pulseaudio` crate optimised (`[profile.dev.package.pulseaudio]
  opt-level = 3` in `Cargo.toml`). Unoptimised, it starves a huddle's
  playback on PipeWire and PulseAudio; release builds were never affected
  ([ci]).

## Known limits

What v0.17.0 does not do, and what was not seen. Each is in
[`validation-v0.17.0.md`](validation-v0.17.0.md) with its reason.

**Where it ran**

- **macOS was opened once, over SSH** (W3, [principal]): the Intel release
  binary CI built for the candidate, on macOS 14 in a virtual machine,
  driven through tmux in a terminal session of that system: the demo, the
  help, the palette, Tab on the `:` line, a message with an accent and
  Scout's answer, the Board, the huddle's lanes moving and the orb, leaving
  with nothing left behind, the first screen and the join form. Not Apple
  Silicon, not Terminal.app or another terminal program at the machine, no
  keychain, no relay, no sound. Beyond that, CI builds the
  release binary for Apple Silicon and Intel and runs `--version`, the
  library tests and the integration tests of identity, configuration, the
  Inbox, DMs, search, the store, the terminal's restore and apps there, the
  system keychain with a generated identity, the upgrade test on Intel, the agent host
  on Intel, and the tone round trip through a virtual audio device on
  Apple Silicon. CI at the last commits had not finished when this was
  written ([the principal's own checks][principal]).
- **No person has used this version in Windows Terminal.** It was driven
  in a console over SSH (a ConPTY, from Linux through tmux): by task
  `windows` with a build of its own, and by the principal with the release
  binary CI builds. What only a person at the machine can check is listed
  in [`e2e-windows.md`](e2e-windows.md): AltGr and Caps Lock on a real
  keyboard, a paste with `Ctrl+V` and a right click, the look, the mouse
  ([windows], [principal]).
- **On Windows a paste still arrives as keys**: the terminal library gives
  no paste event there. In the writing dock its lines stay lines; elsewhere,
  and in an app, its characters act as keys ([windows]).
- **PipeWire was proven in containers** (Debian and AlmaLinux 9), each with
  a null sink, **PulseAudio in CI**, not on a desktop with a real card,
  WSLg, or a sound server restarted during a call. No real microphone or
  speaker was opened on the machine bzz was built on, by design. The mapping
  of a v0.16.0 device name to a real card's device was tested with the
  server's answers written by hand ([sound], [ci]).
- **The microphone check** was seen with a fake device only, and it names
  "the default microphone", not the device the server chose ([viz2]).
- **The look and motion** were read in tmux, in test buffers and in a film
  of the demo played in a browser terminal. No graphics terminal (kitty,
  Ghostty, WezTerm, iTerm2, Terminal.app, Windows Terminal) and no Linux
  console was looked at, for the frames, the colours, the huddle's braille
  and block characters, or the animations; motion was not watched over SSH
  ([look], [viz], [viz2], [motion]).
- **The mouse** was driven by SGR reports sent through tmux, not by a hand
  in a terminal ([layout]).
- Linux on arm64 is built for the release and runs no test, as before.

**Joining**

- **Pairing with Buzz desktop from the join form was not run against a
  real Buzz desktop**: no Buzz desktop may be started on the machine bzz was
  built on. The form calls the same two functions as `bzz identity
  pair-desktop`; without a running desktop it says so ([join]).
- **The sign-in through the browser** was not run against a real sign-in
  service: tested at the form only ([join]).
- **An invite** was claimed on a scripted relay, not on the pinned relay
  (minting one needs an admin) ([join]).

**The demo**

- **The demo's agent is scripted.** Scout answers from a script, and the
  screen says the data is generated; the teammates answer a few times, then
  go quiet ([demo], [demo2]).
- `Ctrl-C` as a key does not leave the demo, as it does not leave bzz
  (rule 19); `q` then `y` does ([demo]).
- After `kill -9` the demo's folder stays until the next `bzz demo`; on
  macOS and Windows that one removes only folders older than a day. The
  demo was run on macOS and on Windows only over SSH ([demo],
  [principal]).

**The palette and the hints**

- **The palette does not open while an app has the keyboard**: the app has
  every key but `Ctrl-c` (`app-api.md`). `Ctrl-c`, then `Ctrl+K`
  ([palette2]).
- **An app's own messages are not toasts.** `Saved.` and `Loaded N
  cards.` are the app's own text in its pane; making them toasts would
  change the app contract, which v0.17.0 keeps (D10) ([motion]).
- In the writing dock `Ctrl+K` stays "delete to the end of the line". A
  terminal that keeps `Ctrl+K` (VS Code's, with chords) still has `Space k`
  ([palette]).
- Seen and not fixed: a space typed before Tab on the `:` line leaves two
  spaces; in the composer the hint repeats two keys its frame already shows
  ([principal], [registry]).

**Other defects seen and not fixed**

- **Search:** a relay's answer that arrives after `Ctrl-o` clears the
  "older results" notice of the page `Ctrl-o` loaded ([motion]).
- **The huddle visualizer redraws the whole screen** at its rate while it
  moves; frames that redraw only its region (B8) were not built ([viz2]).
- With motion on, housekeeping wakes every 100 ms while local agents run or
  an app session is open; at rest otherwise, once a second ([motion]).
- **Once, a local agent did not answer a mention, and it is not
  explained.** In one run of the scenario suite at 80×24 on the candidate,
  the scripted agent on Pi Durable gave no reply within 90 s to a mention in
  its channel; the next scenario's mention, a new start of bzz on the same
  profile, was answered. The run's folders were not kept. It did not happen
  again in eight runs of that scenario with its neighbours or in six runs of
  the whole suite at that size, nor in any of the three runs at 200×50
  ([principal]).

**Not in this version**

- Frames that redraw only the visualizer (B8), for v0.17.1 ([viz2]).
- macOS at the machine, in a terminal program, by a person (W3 was done
  over SSH).
- A paste that arrives as a paste on Windows (W2's last part).
- Use of the palette remembered between sessions, an interface inside the
  conversation (D10), a build for Alpine and older systems: as the plan's
  "After the announcement" says.

**Still as v0.16.0 said:** apps were run in a terminal on Linux only (and
the Board, in the demo, in a Windows console over SSH); an agent's tests run
on Unix only; Grok was not seen to write an app; Buzz desktop shows nothing
of an app but its brief; one app shown in a channel at a time
([`release-v0.16.0.md`](release-v0.16.0.md), "Known limits").

**Pending acceptance**

- The version (0.17.0 in `Cargo.toml`) is on the candidate since
  2026-11-26 ([release], [principal]).
- The installer on clean machines with this release published: until then
  the full install was seen with v0.16.0's archives only ([install]).
- CI green at the candidate on the four platforms, and two consecutive green
  runs on `main` (F6), which run once the branch is merged ([ci],
  [install]).
- The checks only a person can do ([`e2e-manual.md`](e2e-manual.md), section
  15), and the owner's release acceptance: `bzz demo` from the published
  installer; `Ctrl+K` to do three things he did not know the key for; a
  huddle with one other person where he sees both lanes and his muted
  microphone.

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
[join]: validation-v0.17.0.md#v0170--join-joining-a-community-without-leaving-bzz-a9
[viz]: validation-v0.17.0.md#v0170--viz-the-huddle-shows-who-is-speaking-b1-to-b4
[viz2]: validation-v0.17.0.md#v0170--viz2-the-zoomed-huddle-its-styles-the-microphone-check-b5-to-b7
[palette]: validation-v0.17.0.md#v0170-validation-the-palette-c1
[find]: validation-v0.17.0.md#v0170-validation--task-find-c3-to-c7-a5-rule-19
[palette2]: validation-v0.17.0.md#v0170-validation-arguments-as-steps-and-more-to-find-c9-c12
[registry]: validation-v0.17.0.md#v0170-validation--task-registry-c8-c10-c11-rules-19-20-and-23
[look]: validation-v0.17.0.md#v0170--task-look-l1-to-l6-c2
[motion]: validation-v0.17.0.md#v0170--task-motion-l7-l8-l9-one-look
[layout]: validation-v0.17.0.md#v0170--task-layout-l10-l11-l12-f7
[sound]: validation-v0.17.0.md#v0170-validation-sound-e1-decision-d3
[board]: validation-v0.17.0.md#v0170--task-board-f1-f2-f3-f4-f5
[tidy]: validation-v0.17.0.md#v0170--task-tidy-loose-ends-of-the-reviews
[ci]: validation-v0.17.0.md#v0170--task-ci-ci-green-and-quick
[release]: validation-v0.17.0.md#v0170--task-release-the-upgrade-from-v0160-the-version-f8
[windows]: validation-v0.17.0.md#v0170-validation-windows-task-windows-plan-w1-and-w2
[principal]: validation-v0.17.0.md#the-principals-own-checks
