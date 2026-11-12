# Composition patterns (HyperFrames)

Pin `hyperframes@0.9.13` (newest that passes npm's seven-day `min-release-age`
here) and set `HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1`. Vendor every library
and font into `lib/` and `assets/`: GSAP 3.15.0, `@xterm/xterm` 6.0.0,
`@xterm/addon-image` 0.9.0, `@xterm/addon-unicode11` 0.9.0, JetBrains Mono.

Brand: ground `#0e0c08`, surface `#14110d`, ink `#ece5d8`, muted `#8a8272`,
accent `#ffb000` (mirror `site/public/styles.css`); display type Inter, terminal
JetBrains Mono; the logo is `site/public/assets/logo.svg`, drawn on screen
(hexagon stroke, then the three lines and cursor, then the `bzz` wordmark).

## Product shots

```js
mountTape({ id: "s6", el: "#t-s6", url: "assets/tapes/agent.jsonl",
            map: [[18.1, 9.9], [19.3, 11.5], [20.2, 13.7], [22.3, 16.4]],
            width: 1440, height: 752 });
```

- One `.shot` clip per line inside the window, each with a `.term` host and
  `data-layout-ignore` (terminal text is the product, not film type).
- `map` keyframes `[filmSecond, tapeSecond]`: linear between, held outside.
  Find tape seconds with `tape-peek.py` and marks, then put the visible proof
  inside the narration's segment.
- `align: "center"` for a small shell terminal.

## Camera and overlays

- `look(at, px, py, scale)` pushes the window so frame point (px, py) lands at
  (960, 600) under the caption band; `rest()` returns. 0.4–0.6 s moves,
  `power3.inOut`; scale 1.15–1.45 keeps terminal text ≥ 26 px.
- A gradient caption backdrop (`#capbg`) keeps captions readable over pushed-in
  UI. Captions: Inter 700, 58 px, the key phrase in accent, words rise in.
- Key chips (accent, JetBrains Mono) at the tape's key times plus the map offset.

## Checks

- `hyperframes check`: layout, motion, contrast and audio must pass. Lint errors
  for `Math.random`, `Date.now`, `performance.now` and `requestAnimationFrame`
  come from vendored xterm.js; replay renders rows synchronously, so judge
  determinism by rendering twice instead.
- Two renders of the v0.13 film: audio identical; 227 of 1,410 frames differed,
  worst-frame PSNR 53.1 dB (invisible), all during Sixel-image and theme shots.
- `snapshot --at …` stills at every line; check the first frame is not empty.
