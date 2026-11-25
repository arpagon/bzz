# v0.17.0 validation: Windows (task `windows`, plan W1 and W2)

Windows 11 (build 26100), PowerShell 5, Windows OpenSSH: every session is a
ConPTY. Driven from Linux through tmux, as
[`docs/e2e-windows.md`](../e2e-windows.md) says. 2026-11-24 and 25.
`<scratch>` is a folder outside the repository on the Linux machine; the
captures named below are there.

The build under test is this branch, built on Linux for
`x86_64-pc-windows-gnu` with Zig as C compiler and linker (the machine has
no `clang-cl`, so `cargo xwin` could not build bzz's C dependencies; a
crossterm-only probe built with `cargo xwin` for MSVC). Its terminal code
is the release's: the same crossterm Windows backend. `bzz --version` of it
says `bzz 0.16.0` (the branch has not changed the version). The installed
v0.16.0 (`bzz --version`: `bzz 0.16.0`, from the README's installer) was
used for the "before" and left in place.

## What works on Windows now, what does not

| | |
|---|---|
| Works (seen) | `?` opens the help and `:` the command line, on the first screen and in a community; channels, a thread, the composer with accents, `¿ ¡ € °`, CJK and emoji, sending; `:search`, `:inbox`, the theme picker (`Space o`), `:apps`, the approval of an app version with `Ctrl+Y` (scroll to the end, `a`), the Board in the channel (`5`, `n`, text, Enter, `]`, `Ctrl+C` leaves) and in `bzz app dev`; Tab and Shift+Tab; a click on a channel and the wheel; resizing to 80×24 and back; `q` then `y` restores the terminal (main screen, cursor, mouse off) |
| Works by tests only | keys a physical keyboard gives that SSH cannot: AltGr text, Caps Lock with letters and with Ctrl |
| Does not work | a paste is not a paste: it arrives as keys. Its line breaks no longer send (on this branch), but it is still typed key by key, so outside the composer its characters are keys of the key map, as on a terminal without bracketed paste. After a kill (`Stop-Process`), the console keeps bzz's last screen and stays in the alternate screen until `clear` (no program can restore what it is killed in) |
| For a person at Windows Terminal | the list at the end of [`e2e-windows.md`](../e2e-windows.md): AltGr and Caps Lock on a real keyboard, a paste with `Ctrl+V` and with a right click (one line and three lines), the look (frame lines, colours, emoji width), clicks |

## The dead keys (W1)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| On v0.16.0, `?` and `:` do nothing on Windows | the published binary on the machine | `bzz` on a scratch profile in the ConPTY, `?`, then `:` | the first screen unchanged after each (`v016-after-question.txt`, `v016-after-colon.txt`) |
| The cause: Windows reports Shift with every shifted symbol, and the key map matches modifiers exactly | the key probe on both sides | `examples/key_probe.rs` (crossterm-only copy built with `cargo xwin`) on Windows; the same example in tmux on Linux; the same keys sent to both (`<scratch>/keys.sh`) | `? : @ ! # $ % ^ & * ( ) _ + { } \| < > " ~` with `SHIFT` on Windows, none on Linux; `Ctrl+J` as Enter with Ctrl on Windows; a Release after every Press on Windows; everything else the same (`probe-windows-conpty.txt`, `probe-linux-tmux.txt`; the table in `e2e-windows.md`) |
| Read from crossterm 0.29's Windows parser (`event/sys/windows/parse.rs`), not seen over SSH | read | | a capital under Caps Lock has no Shift; Ctrl+letter under Caps Lock is the capital; AltGr text carries Ctrl+Alt |
| Fixed at the door, for every platform, Linux and macOS keys unchanged | unit | `cargo test --lib keymap:: keys_enter_in_one_form` | 19 pass. Red without the change (normalisation and the parse of `shift-` made identities): 7 fail, `windows_shifted_symbols_reach_their_bindings`, `windows_capitals_and_ctrl_keys_under_caps_lock`, `windows_altgr_characters_are_text`, `windows_ctrl_enter_is_the_composer_newline`, `shift_tab_reaches_previous_focus`, `user_maps_match_with_or_without_shift`, `keys_enter_in_one_form_for_every_platform`. `keys_from_a_linux_terminal_are_unchanged` passes on both: every key of the Linux probe comes out as it went in |
| A user key map written with or without `shift-` works on both | unit | `user_maps_match_with_or_without_shift` | `shift-!` and `!`, `X`, `shift-x`, `shift-X`, `shift-tab` |
| On the machine with this branch: `?` opens the help, `:` the command line | the build on the machine | as the first row | `w1-welcome-question.txt` (the help), `w1-welcome-colon.txt` (the command line) |

## Found on the walk, and fixed (W1)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| A character the layout cannot type (`¿ ¡ € °` on a US layout) was lost: the console sends it as an Alt code, which crossterm gives as a release with no press | probe, then the build | the probe on Windows; `¿qué?` typed in the composer | the probe: `Char('¿') … kind=Release` and no Press; the message sent read `qué?` (`r1-composer.txt`), a card `sí?` (`r1-app-card.txt`) |
| Fixed: such a release is the press (Windows, non-ASCII only) | unit, then the build | `windows_alt_codes_are_presses_and_releases_stay_releases`; on the machine, `:` then `¿¡€° café ñ 😀`, and a card `¿Funciona? ¡Sí! 20 °C, 5 €` in `bzz app dev` | the command line and the card show every character (`w1b-colon-altcodes.txt`, `w1b-appdev-altcodes.txt`) |
| A paste of two lines into the composer sent the first line, and the next characters ran as keys until an `i` | the build, with the relay | `i`, then a tmux paste of `paste line one⏎paste line two` | `paste line one` sent; the composer held `ne two`: `paste li` were keys (`r1-paste.txt`) |
| Fixed: in the composer on Windows an Enter with key presses already waiting behind it is a line break | unit, then the build | `windows_paste_line_break_is_a_composer_newline_only_with_keys_behind`, `a_paste_on_windows_is_seen_behind_its_line_break` | see the section W2 below |
| Shift+Tab never moved the focus back, on any platform: crossterm reports BackTab with Shift and the builtin `backtab` has none | unit, then the build | `shift_tab_reaches_previous_focus`; on the machine, Shift+Tab twice then Tab | the focused frame went communities → timeline → channels → timeline |

## The walk (W2), on this branch

Against the pinned test relay (`ws://localhost:3030`, reached through `ssh
-R`), the harness fixture's viewer profile (its configuration and its key
only; the store synced fresh).

| Step | Seen | Capture |
|---|---|---|
| Start | the passphrase prompt, then the workspace in about ten seconds | `r1-first.txt` |
| A channel | `2`, `k`, Enter opens `#t9-general` with its history | `r1-general.txt` |
| The composer | `café, ñandú, über, 日本語, 😀👍` typed and sent; acknowledged | `r1-composer.txt`, `r1-sent.txt` |
| A thread | Enter on the message with replies opens them | `r1-thread.txt` |
| `:` commands | `:search` then `Windows` finds the message just sent; `:inbox` | `r1-search2.txt`, `r1-inbox.txt` |
| Help in the Inbox | `?` | `r1-inbox-help.txt` |
| Theme picker | `Space o`, `j j` Enter: Light, the colours change (truecolor) | `r1-options.txt`, `r1-apps.txt` (status line) |
| An app | `bzz app add`, `Ctrl+Y`, PgDn to the end, `a`; `:apps`, Enter; `5`, `n`, a card, `]` moves it to Doing, `Ctrl+C` leaves | `r1-ctrly.txt`, `r1-approved.txt`, `r1-app-open.txt`, `r1-app-card.txt`, `r1-app-left.txt` |
| The mouse | a click on a channel opens it; the wheel | `r1-click.txt`, `r1-click2.txt`, `r1-wheel.txt` |
| Resize | 160×45 to 80×24 and back | `r1-80x24.txt`, `w1-welcome-80x24.txt`, `w1-welcome-160x45.txt` |
| Quit | `q`, `y`: main screen, cursor shown, mouse off (tmux's own state) | `w1-after-quit.txt` |
| Kill | `Stop-Process`: the console keeps the alternate screen and bzz's last frame under the prompt; the cursor and the mouse are back; `clear` gives a clean prompt | `r1-after-kill.txt`, `r1-after-kill-clear.txt` |
| `bzz app dev` | the Board offline: a card with accents and emoji, `]`, `Ctrl+C` | `appdev-*.txt` |

Seen in passing, not fixed (not Windows, or not small):

- On the first screen `:theme` is "unknown command" (C4 adds it).
- At 80×24, a wrapped system line under the selection bar loses its first
  character (`▌d8c869d…` for `6d8c869d…`, `r1-80x24.txt`): the same code on
  every platform; L10's layout work is where it belongs.
- The approval banner covers the pane titles (`r1-banner.txt`; L5).
- `q` in a thread closes the thread; a `y` typed right after it copies the
  selected message (`r1-oops.txt` shows what the driver typed into bzz when
  it took a closed thread for a quit): no defect, a reason the driver now
  refuses to type unless the pane shows a prompt.

## Paste (W2)

| Claim | Lowest rung | Command | Seen |
|---|---|---|---|
| crossterm on Windows never produces a paste event | read | `crossterm 0.29` `event/sys/windows/parse.rs`, `event.rs` (`EnableBracketedPaste` writes `CSI ?2004h`; the Windows reader has no `Paste`) | |
| Over SSH the console swallows the bracketed-paste markers | probe | `ESC [ 200 ~ ab LF c ESC [ 201 ~` sent to the probe | `a`, `b`, Ctrl+Enter, `c`: no marker, no paste |
| A tmux paste (bracketed when the pane asks) arrives as keys | probe | `paste-buffer -p` of two lines | one key per character, Enter for the line break |
| With this branch, a paste of three lines stays in the composer, and a typed Enter still sends | the build, with the relay (a second run) | `i`, a tmux paste of `paste line one ¿sí?⏎paste line two⏎qy line three`; then Enter; then `i`, `typed line`, Enter | the three lines in the composer and nothing sent (`w1b-paste.txt`); Enter sent them as one message (`w1b-paste-sent.txt`); the typed line was sent at once (`w1b-typed-sent.txt`). The `q` and `y` of the third line stayed text |

What this is: a guess from what waits in the input, which the brief allows
only where it is the only way and safe. It is the only way (no marker
reaches bzz), and it fails safe: a person who types Enter and the next key
before bzz reads the Enter gets a line break instead of a send, and sends
with the next Enter. Over SSH, the console puts each key's release right
behind its press, so the release alone is not "more waiting". A paste that
ends in a line break still sends at its last Enter, and a paste outside the
composer is still keys. A real paste event on Windows needs the console's
VT input mode and a reader that parses it, which crossterm 0.29 does not
have: a plan question, not this task's.

## Windows Terminal

Not installed on the machine, and nothing was installed. What was proven
in a ConPTY over SSH is the console side, which Windows Terminal shares.
What remains for a person is listed at the end of `e2e-windows.md`.

## On the machine at the end

The last build under test: `w1-bzz.exe`, SHA-256 `fa5ed6664dd7c6af…`,
`bzz --version` `bzz 0.16.0` (the branch's version). Then, in one
PowerShell script over SSH: no process of the build or the probe left
(none was running; `Stop-Process` on the list anyway), the scratch folder
removed (`Test-Path` false), the installed binary in its place with
`bzz 0.16.0` and its date from the installer, no `bzz` process, nothing
listening on port 3030. The tmux server of the driver was stopped, which
closed the SSH session and its port forward. The relay run of this task
reported `relay down` and its process is gone; the lock was then held by
another task's run.

The scratch folder also held a `tasks` app folder from the principal's own
v0.16.0 check, made before this task began; it went with the folder, as
the brief asks. `bzz app new tasks` makes it again.

## Gates

On this branch at its last code commit, Linux x86_64:

```
cargo fmt --check                                   ok
cargo clippy --locked --all-targets -- -D warnings  ok
cargo build --locked --example scripted_harness     ok
cargo test --locked --all-targets                   ok (1,598 passed over all targets with the next line, 0 failed)
cargo test --locked --no-default-features --lib     ok
```

`Cargo.toml` and `Cargo.lock` are unchanged: no `cargo deny`, no
`cargo audit`. The Windows build was made with `--ignore-rust-version` on
Rust 1.95.0, the only toolchain of the machine with a Windows target; it
compiled with the two dead-code warnings the Windows build already had
(`agents/runtime/process.rs` `boot_id`, `auth/provider.rs` `Owner`).
