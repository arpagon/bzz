# getbzz.dev

Static marketing site for bzz, deployed to Cloudflare Pages
(project `getbzz-dev`, account `Arpagon@gmail.com`).

Plan: [`docs/planning/2026-08-24/website-getbzz-dev.md`](../docs/planning/2026-08-24/website-getbzz-dev.md).

## Layout

```
public/
  index.html      one-pager (hero, features, security, keys, install)
  styles.css      theme tokens mirroring src/ui/theme/builtin.rs palettes
  app.js          theme switcher, install tabs, copy button
  _redirects      /install.sh + /install.ps1 -> latest GitHub release installer
  assets/         logo.svg, favicon.svg, logo-terminal.txt, mdi-attribution.txt, og.png
```

The site theme switcher uses real bzz built-in palettes (bzz, Dracula, Nord,
Gruvbox Dark, Catppuccin Mocha, Tokyo Night). When palettes change in
`src/ui/theme/builtin.rs`, update the token blocks at the top of `styles.css`.

## Logo animation

The hero animates the terminal-compatible Nerd Fonts v3 sequence
`nf-md-hexagon_outline` (`U+F02D9`) → `nf-md-hexagon_slice_1…6`
(`U+F0AC3…U+F0AC8`), then resolves into the existing bzz honeycomb and terminal
lines. `public/assets/logo-terminal.txt` records the copyable glyph sequence.
The website embeds matching SVG paths so it does not depend on a visitor having
Nerd Fonts installed. The animation respects `prefers-reduced-motion` and
pauses while the page is hidden. Hexagon frame geometry comes from
Pictogrammers Material Design Icons 7.4.47 under Apache-2.0; attribution is
recorded in `public/assets/mdi-attribution.txt` and the license text is at
[`LICENSE-APACHE`](../LICENSE-APACHE). Usage rules live in the short
[`docs/brand` manual](../docs/brand/README.md).

## Demo media

`public/assets/demo.webm`, `demo.mp4`, and `poster.png` are a pixel capture of
`bzz 0.11.0` in Ghostty 1.3.1 at 1280×720 and 15 fps. The 45-second journey
uses an isolated, generated offline fixture and covers a workspace DM, fuzzy
navigation, Markdown, verified Kitty media, threads, reactions, Inbox, local
search, remote-agent mention completion, the writing dock, and live previews of
Tokyo Night, Kanagawa Dragon, Nord, and Catppuccin Mocha.

`public/assets/demo-v013.webm`, `demo-v013.mp4`, and `poster-v013.png` show
`bzz 0.13.0` (Ghostty, Xvfb, 1280×720, 15 fps, 49 s) on an isolated pinned
harness relay filled by `examples/demo_fixture.rs` (generated people and
messages): a read-only announce channel, the System group, a thread, a profile
card with public channels, the artifacts panel, the custom emoji reaction
picker and `:presence on`.

Recapture through the `bzz-visual-capture` skill. Review every retained frame
for private data, stale graphics, overlap, clipping, and loading states before
replacing these assets.

## Develop

```sh
npx wrangler pages dev public
```

## Deploy

```sh
npx wrangler pages deploy public --project-name getbzz-dev --branch main
```

Requires `CLOUDFLARE_API_TOKEN` with Pages write on the account above.
Custom domains `getbzz.dev` and `www.getbzz.dev` are attached to the project;
DNS CNAMEs point at `getbzz-dev.pages.dev` (proxied).

## Pending (per plan)

- M4: measured performance stats (do not publish invented numbers),
  Lighthouse >= 95 pass.
