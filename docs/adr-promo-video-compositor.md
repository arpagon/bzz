# ADR: compositor and TUI source for promotional films

**Status:** accepted 2026-11-11 (spike for
[`planning/2026-11-11/promo-video.md`](planning/2026-11-11/promo-video.md)).

## Decision

1. **Compositor: HyperFrames** (`hyperframes@0.9.13`, Apache-2.0). It renders
   HTML, CSS and GSAP to video by seeking a paused timeline frame by frame,
   mixes `<audio>` tracks, measures audio onsets and loudness, and supersamples
   HTML to 4K.
2. **TUI source: terminal tapes, not screen recordings.**
   `.agents/skills/bzz-promo-video/scripts/record-tape.py` runs the release
   binary on a private PTY with a fixed cell and pixel size and stores every
   output chunk byte-for-byte with its time, plus named marks for scripted
   actions. In the film, `assets/tape-replay.js` replays the tape in a real
   terminal emulator (xterm.js 6.0.0 with its image addon for Sixel) at the
   seeked film time.

## Why tapes

The earlier capture path (Ghostty in Xvfb, PNG frames from `scrot`) gives at
most 15 fps at 720p and needs a display server. A tape is the program's real
output, so no UI is mocked, but it is re-drawn at any resolution and frame
rate: text stays sharp under a 1.75× push-in at 4K, and the camera, window
frame and type are composed around it.

Limits:

- The emulator is xterm.js, not Ghostty. Glyph shapes come from the bundled
  JetBrains Mono; colours come from bzz's own truecolor theme output.
- Images must use Sixel (`[media] protocol = "sixel"` in the fixture config):
  xterm.js does not implement the Kitty graphics protocol.
- bzz sends no terminal queries (cell size comes from the PTY window size), so
  the recorder never answers or modifies output. Re-check this if bzz starts
  querying the terminal.
- `tape-replay.js` calls xterm's private `_renderRows`, because the paused film
  clock never runs xterm's `requestAnimationFrame` refresh. The first attempt,
  which waited for `onRender`, stalled the render at frame 1 for 60 s.
  Re-verify when upgrading xterm.

## Evidence (spike, 7 s, 1920×1080, 30 fps)

Real v0.13.0 release binary on a generated offline fixture, 9.4 s tape.

| Criterion | Result |
|---|---|
| Determinism | Two renders: all 210 video frames and all audio frames identical (`framemd5`). A third render through the extracted `tape-replay.js` was also identical. |
| Sharpness | 4K render: terminal text crisp at the 1.75× push-in (frame review) |
| Render time | 1080p: 9.7 s for 7 s of film; 4K: 21 s (RTX 3090, hardware GPU capture) |
| Checks | `hyperframes check`: runtime, layout, motion, audio pass. Lint flags `Math.random` inside xterm.js (no visible effect: frames are identical) and CSS transitions on xterm's scrollbar (hidden). Contrast flags bzz's intentionally dim timestamps; mark terminal hosts `data-layout-ignore`. |

## Alternatives not built

The plan proposed building the same test in Remotion and in a custom
Playwright renderer. They were not built because HyperFrames met every
criterion on the first attempt.

- **Remotion:** source-available license with seat limits above three people,
  and it needs React rewrites of each scene.
- **Custom Playwright renderer:** this would reimplement seeking, audio mixing
  and encoding that HyperFrames already provides.

## Operating rules

- Set `HYPERFRAMES_NO_TELEMETRY=1` and `DO_NOT_TRACK=1` for every command.
  The skill's usage beacon and publish commands are not used.
- npm enforces a seven-day `min-release-age` here. Pin the newest version that
  satisfies it, and vendor GSAP, xterm and fonts into the project. Nothing is
  loaded from a CDN.
