# Driving bzz on Windows over SSH

How bzz was checked in a real Windows console for v0.17.0, from a Linux
machine, without a person at the Windows screen. It is the same method as
[`e2e-herdr.md`](e2e-herdr.md)'s tmux runner: a tmux pane on a private
socket, keys sent with `send-keys`, the screen read with `capture-pane`. The
pane holds an SSH session to the Windows machine, and bzz runs in that
session.

## What this proves, and what it does not

Windows' OpenSSH server runs each session's shell in a **ConPTY**, the
pseudo-console that Windows Terminal also uses. bzz in that session is a
real Windows console program: crossterm reads `INPUT_RECORD`s through the
console API and writes VT sequences, exactly as it does in Windows Terminal.
What reaches the console differs: over SSH, the keys are the bytes of the
Linux terminal on the other side, which the console turns into key records
with the Windows machine's keyboard layout. So:

- proven over SSH: the console API path (key records, modifiers, Release
  events, resize, mouse records, raw mode, the alternate screen and how it
  is left), the Windows build itself, its paths and its sockets;
- not proven over SSH: keys as a physical keyboard gives them (AltGr, dead
  keys, an IME, Caps Lock), the paste of Windows Terminal (`Ctrl+V`, right
  click), its font, colours and Unicode width, and mouse clicks there. A
  person at a Windows Terminal window checks these (the list at the end).

## The machine

- Windows 10 or 11 with the OpenSSH server, key login, and a user with no
  administrator rights needed. PowerShell 5 is enough.
- Nothing is installed on it for the checks but bzz. The builds under test
  go in a folder of their own under `%LOCALAPPDATA%\Temp`, under a name of
  their own, and the installed bzz stays as it is.
- Each run gets a scratch profile: `BZZ_CONFIG_DIR`, `BZZ_DATA_DIR` and
  `BZZ_CACHE_DIR` set to three folders under that scratch folder. Check with
  `bzz paths` that every path is inside it.

## Building for Windows on Linux

The release is built by CI on Windows runners (MSVC). For a quick check on
a Linux machine:

- `cargo xwin build --target x86_64-pc-windows-msvc` needs `clang-cl` for
  bzz's C dependencies (aws-lc, Lua, SQLite, Opus, secp256k1). A pure-Rust
  program such as a small crossterm probe needs no C compiler and builds
  with `cargo xwin` alone.
- Without `clang-cl`, the GNU target builds with Zig as the C compiler and
  linker: `CC_x86_64_pc_windows_gnu`, `CXX_…`, `AR_…` and
  `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` point at small wrappers that
  call `zig cc -target x86_64-windows-gnu` (dropping cc's own `--target=`),
  `zig ar`, and a linker wrapper that drops the MinGW libraries Zig brings
  itself (`-lgcc`, `-lgcc_s`, `-lmsvcrt`, `-lmingw32`, `-lmingwex`,
  `-l:libpthread.a`, `-Wl,-Bstatic`, `-Wl,-Bdynamic`, `-Wl,-O1` and
  rustc's `crt2.o`, `dllcrt2.o`, `rsbegin.o`, `rsend.o`) and turns
  `-lgcc_eh` into `-lunwind`; `x86_64-w64-mingw32-dlltool` on the `PATH`
  calls `zig dlltool` without `-f` and its value. This binary is not the
  release one (another C runtime and linker), but the terminal code is the
  same crossterm Windows backend, so it serves for everything about keys
  and screens.

Copy the binary with `scp` into the scratch folder.

## The pane

```sh
tmux -S "$scratch/sock" new-session -d -s win -x 160 -y 45 \
  "ssh -tt -R 3030:localhost:3030 <user>@<windows-host>"
```

- `-tt` gives the session a terminal, so bzz gets a ConPTY.
- `-R 3030:localhost:3030` makes the test relay of
  [`e2e-herdr.md`](e2e-herdr.md) (`scripts/test-relay.sh`) reachable on the
  Windows machine's own loopback, as `ws://localhost:3030`, the address
  the harness fixture's community has. The relay need not listen beyond
  loopback.
- In the PowerShell prompt, set the three `BZZ_*_DIR` variables and start
  the binary with `&`. Send text with `send-keys -l`, named keys
  (`Escape`, `Enter`, `BTab`, `C-y`) without it.
- After `Escape`, wait about a second before the next key: the console
  joins a lone `Esc` and a following character into Alt+character.
- `tmux resize-window -t win -x 80 -y 24` resizes the console; bzz receives
  the resize.
- `tmux display -p -t win '#{alternate_on} #{cursor_flag} #{mouse_any_flag}'`
  tells whether bzz left the alternate screen, showed the cursor again and
  turned mouse reporting off.

A one-shot command (a process list of your own processes, a file check)
goes through `ssh <user>@<windows-host> 'powershell -NoProfile
-NonInteractive -Command -'` with the script on standard input.

## The key probe

`examples/key_probe.rs` prints every event crossterm delivers, with what
bzz makes of it (`normalize_key_event`), and leaves on F12 or after the
seconds given. It needs bzz's library; a standalone copy of it that depends
on crossterm only builds with `cargo xwin` and no C compiler. Run it on both
sides and send the same keys to compare.

What it showed on Windows 11 (build 26100) over SSH, against tmux on Linux,
with crossterm 0.29:

| Key | Linux | Windows console |
|---|---|---|
| `? : @ ! # $ % ^ & * ( ) _ + { } \| < > " ~` | the character, no modifier | the character **with Shift** |
| `J K G U` (Shift) | capital with Shift | the same |
| a capital under Caps Lock (read from crossterm's source) | capital with Shift | capital **without Shift** |
| Ctrl+C under Caps Lock (read from crossterm's source) | `c` with Ctrl | **`C`** with Ctrl |
| AltGr+key, e.g. `@` on a Spanish or German layout (read from crossterm's source) | the character | the character **with Ctrl+Alt** |
| `Ctrl+J` | `j` with Ctrl | **Enter with Ctrl** |
| `é ñ 😀` | the character | the same |
| Tab, Shift+Tab | Tab; BackTab with Shift | the same |
| `Ctrl+K Ctrl+Y Ctrl+P Ctrl+N Ctrl+D Ctrl+U Ctrl+C` | the letter with Ctrl | the same |
| `Alt+h Alt+l Alt+o Alt+Enter` | with Alt | the same |
| Space, Enter, Backspace, F1, F5, Up, Shift+Up, Home, PageUp, Delete, Esc | the key | the same |
| every key | one Press | a Press and a **Release** |
| a paste | one `Paste` event (bracketed paste) | **one key event per character**, `Enter` for a line break |

bzz now gives every key the Linux form where keys enter it
(`ui::keymap::normalize_key_event`); Release events were already ignored.

## Paste

crossterm's Windows backend reads key records and never produces a `Paste`
event. Over SSH the console also swallows the bracketed-paste markers: the
bytes `ESC [ 200 ~ ab LF c ESC [ 201 ~` sent to the probe arrived as `a`,
`b`, Ctrl+Enter, `c`, nothing else. A Windows Terminal paste goes through
the same console and the same backend, so it arrives as keys there too
(not seen: no Windows Terminal on the machine). See the validation record
for what bzz does about it.

## Cleaning up

Stop every bzz you started (`Get-Process` filtered by the binary's name),
remove the scratch folder, close the SSH session (the port forward goes
with it), and bring the relay down as [`e2e-herdr.md`](e2e-herdr.md) says.

## For a person at Windows Terminal

- `?` opens the help, `:` the command line, `Ctrl+K` the palette.
- With a layout that has AltGr (Spanish, German, French): type `@`, `{`,
  `[`, `\`, `|`, `€` in the composer.
- With Caps Lock on: `J` scrolls, `Ctrl+C` leaves an app.
- Paste one line and three lines into the composer with `Ctrl+V` and with a
  right click.
- The look: the frame lines, the colours of the theme, emoji width, and
  that the cursor and the screen come back after `q` then `y`.
- A click, a double click and the wheel in the timeline.
