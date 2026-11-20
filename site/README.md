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

## Agent harnesses

The "agent harnesses" section sells the four harnesses, Pi Durable first. It
may say only what [`docs/agents.md`](../docs/agents.md) and the capability
records (`src/agents/capability.rs`) say: which harness is supported or
experimental, what each can touch, and the sentence bzz prints of every
coding harness (it never gives it the key; it cannot promise the key is out
of reach). Change them together.

Each harness has its mark beside its name, inlined in `index.html`: Pi's own
logo in its colours, and the Grok, Codex and Claude marks in the page's text
colour. Their sources are in `public/assets/harness-logos-attribution.txt`.
They are other people's trademarks, used only to name the programs: do not
redraw them, and keep the footer's line that says so.

## Apps

The "apps" section (`#apps`) and the "new in v0.16.0" cards may say only
what [`docs/apps.md`](../docs/apps.md) and
[`docs/release-v0.16.0.md`](../docs/release-v0.16.0.md) say: what a script
reaches and does not, approval of exact bytes, shared records, sharing in
the channel, the three templates, and what people on Buzz desktop see. No
number is on it yet: the measures of M8 are one run on one machine.

**A capture is pending** (the v0.16 film is a later task), and its place is
marked by a comment in `#apps`. It needs a sanitized capture of the real
release TUI through `bzz-visual-capture`, on an isolated pinned relay with
generated people: the Board open beside a channel; the line `tasks:
version 2 from your agent Scout, asked by …` at the top; the approval
overlay with the brief `CHANGED` and the difference; and the Board after
`a`, with `Priority: all` and the same card still selected. The scenario
`t16-m5-agent-changes-the-open-board` (`scripts/tui-herdr-scenarios.toml`)
drives exactly this on the stand-in model.

## Demo media

The hero video `public/assets/promo-v015.webm`, `promo-v015.mp4` and
`poster-promo-v015.png` is the v0.15 launch film: 55.0 s, 1920×1080, 30 fps,
narrated, −14 LUFS, captions burned in so it works muted. It is the v0.13
film with the same voice, music and cut. Its two agent lines now show an
agent that bzz itself runs, and two lines are new: creating an agent, and
moving it with its memory. Every product shot is the `bzz 0.15.0` Linux
release build on an isolated pinned relay with generated people and
generated illustrated avatars, recorded as terminal tapes and composed in
HyperFrames. In the agent shots a real model answers
(`openai-codex/gpt-6-luna` through Pi's own sign-in). Make or remake it with
the [`bzz-promo-video`](../.agents/skills/bzz-promo-video/SKILL.md) skill;
the composition is in [`promo/v0.15-launch`](../promo/v0.15-launch/README.md).
Credits: voice Fish Audio Drama 3 (Fish Audio Community License; credit
required), music generated with ACE-Step 1.5 (MIT model), avatars generated
with gpt-image-2. Encodes: H.264 CRF 26 + AAC 128 kb/s; VP9 CRF 32 + Opus
96 kb/s.

The v0.14 film (`promo-v014.*`, `poster-promo-v014.png`,
[`promo/v0.14-launch`](../promo/v0.14-launch/README.md): editing a message,
Solo chats and sidebar sections) stays published so links to it keep
working.

The v0.13 film (`promo-v013.*`, `poster-promo-v013.png`,
[`promo/v0.13-launch`](../promo/v0.13-launch/README.md)) stays published so
links to it keep working.

The previous hero (`demo.*`, `poster.png`, a v0.11 Ghostty capture) was
removed; it is in git history.

`public/assets/demo-v013.webm`, `demo-v013.mp4`, and `poster-v013.png` (no
longer on the page; linked from the v0.13.0 release notes) show
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
