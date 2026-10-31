---
name: bzz-visual-capture
description: Capture sanitized bzz screenshots or demonstration videos, especially graphics-terminal media/avatar behavior. Use when asked to record, screen-capture, screenshot, visually QA, or make a demo of bzz.
---

# bzz visual capture

Produce a **pixel-true**, reviewable capture of a built `bzz` release binary.
Use generated/disposable content. Treat a terminal transcript as functional
proof, not as an image/video capture.

## 1. Classify the requested evidence

| Need | Surface | Evidence |
|---|---|---|
| Key routing, help, startup, terminal restoration | Herdr runner | visible text plus the runner's normal-exit marker |
| Fonts, theme, layout, inline media, avatar placement, scrolling or resize | direct graphics terminal | raster screenshot or video |
| A repeatable raster artifact without the operator desktop | isolated Xvfb + direct Ghostty | PNG/MP4 and extracted frames |

Read `docs/e2e-herdr.md` before using the existing functional runner. Its
`./scripts/test-tui-herdr.sh` and `scripts/tui-herdr-scenarios.toml` are the
bzz-owned automation base; they are not a substitute for graphics-terminal
visual evidence.

## 2. Establish a safe fixture

1. Build the exact binary first:

   ```sh
   cargo build --release --locked
   ./target/release/bzz --version
   ```

2. Use a disposable identity, relay, channel, paths, and generated message
   corpus. Follow the isolation and secret rules in `docs/e2e-herdr.md` and
   `docs/e2e-manual.md`.
3. Put captures in a fresh restrictive temporary directory, such as
   `root=$(mktemp -d /tmp/bzz-capture.XXXXXX); chmod 700 "$root"`.
4. Before retaining or sharing anything, inspect every frame for names,
   message content, URLs, QR codes, tokens, backup paths, user identifiers, and
   relay administration data. Public evidence contains generated bzz-owned
   fixtures only.

## 3. Choose the correct terminal path

### Operator graphics review

Use a native Kitty, Ghostty, Sixel, or iTerm2 window and launch bzz directly.
Keep the graphics path direct from bzz to the emulator: Kitty image escapes
must reach the emulator unmodified. Capture a static screen only after media is
ready; for a movie, begin before the deliberate interaction.

Use Herdr for disposable text/key automation when helpful. Confirm its actual
terminal graphics passthrough before accepting a Herdr capture as graphical
proof. A tmux or other intermediary that only exposes text is appropriate for
functional E2E, not proof of raster images, placements, or flicker.

On GNOME/Wayland, use the operator-approved screenshot/screen-recording
surface. `grim` requires the wlroots screencopy protocol and `scrot` captures
only X11; neither is a reliable capture of a native Wayland Ghostty window.

### Reproducible virtual graphics review

When a noninteractive raster is required, run Ghostty in a private Xvfb display
and capture that display. Pass bzz through a **transparent PTY relay** only if
keys must be injected: preserve raw stdout byte-for-byte, including Kitty
`ESC_G` sequences; copy the outer terminal's cell/pixel size to the child PTY;
set `TERM_PROGRAM=ghostty`; remove `TMUX` from the child environment. The relay
may inject known test keys but never parses/repaints terminal output.

Start an independent Ghostty process so it does not delegate to an existing
GTK instance:

```sh
skill_dir=/home/arpagon/.pi/agent/skills/bzz-visual-capture
fixture=/absolute/path/to/prepared-disposable-fixture
root=$(mktemp -d /tmp/bzz-capture.XXXXXX); chmod 700 "$root"
Xvfb :99 -screen 0 1920x1080x24 -ac +extension GLX +render -noreset >"$root/xvfb.log" 2>&1 &
xvfb_pid=$!
DISPLAY=:99 GDK_BACKEND=x11 \
  BZZ_CONFIG_DIR="$fixture/config" BZZ_DATA_DIR="$fixture/data" BZZ_CACHE_DIR="$fixture/cache" \
  ghostty --gtk-single-instance=false --window-width=180 --window-height=56 \
  --window-decoration=false -e "$skill_dir/scripts/direct-bzz-pty-relay.py" -- \
  /absolute/path/to/bzz >"$root/ghostty.log" 2>&1 &
ghostty_pid=$!
# Wait for the intended fixture/media state, then:
DISPLAY=:99 scrot "$root/bzz.png"
```

The direct relay (`scripts/direct-bzz-pty-relay.py`) starts the release binary
with the isolated fixture variables. For deterministic generated navigation,
write only public fixture key bytes to an owner-only (`0600`) file and pass
`--inject-file "$root/keys"` before the `--` that precedes the bzz command; it
never accepts key content in process arguments.
It must not launch tmux, Herdr, `script`, or a terminal emulator inside the
terminal emulator. End every run by stopping the Ghostty and Xvfb PIDs and
removing the temporary fixture unless approved evidence was promoted.

## 4. Capture a video

Capture a bounded virtual X11 frame sequence while the direct Ghostty session
runs, then encode it. The bzz environment's FFmpeg may not include `x11grab`,
so the skill owns the portable PNG-frame capture step:

```sh
"$skill_dir/scripts/capture-x11-frames.py" \
  --display :99 --output-dir "$root/frames" --fps 10 --seconds 12
ffmpeg -y -framerate 10 -i "$root/frames/frame-%05d.png" \
  -c:v libx264 -crf 12 -pix_fmt yuv420p "$root/bzz-demo.mp4"
```

Drive a short intentional journey: launch → wait for ready media → navigate or
scroll once → resize or switch the relevant surface once → pause on the final
state. Begin frame capture before that journey. Keep the source PNG sequence
until review passes; create a separate delivery encode only afterwards.

For a real operator desktop recording, use the approved Wayland recording tool
rather than the virtual frame path, then apply the same frame review below.

## 5. Verify pixels and artifact integrity

1. Inspect the PNG directly.
2. For each MP4, verify stream metadata and inspect early/middle/final frames:

   ```sh
   ffprobe -v error -show_entries format=duration:stream=codec_name,width,height,avg_frame_rate \
     -of default=noprint_wrappers=1 "$root/bzz-demo.mp4"
   ffmpeg -y -ss 0 -i "$root/bzz-demo.mp4" -frames:v 1 "$root/first.png"
   ffmpeg -y -ss 1 -i "$root/bzz-demo.mp4" -frames:v 1 "$root/middle.png"
   ffmpeg -y -sseof -0.1 -i "$root/bzz-demo.mp4" -frames:v 1 "$root/final.png"
   ```

3. Check the target behavior in the frames: no flicker, stale graphic cells,
   text overlap, clipping, secret/private data, or visible loading state unless
   that state is the subject of the capture.
4. Record the binary version, commit, terminal/emulator, fixture type, scenario,
   and exact artifact path beside the review result. Do not put private fixture
   data in the record.

## Completion criteria

A capture is complete only when it is a raster PNG/MP4 of the requested state,
uses an isolated generated fixture, has passed frame-by-frame privacy and visual
review, and its binary/version plus scenario are recorded. Functional Herdr
output may corroborate the journey but never replaces this evidence.
