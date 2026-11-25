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
  install.sh      getbzz.dev/install.sh: checks this machine, then runs the release's installer
  _headers        install.sh as text/plain, so a browser shows it
  _redirects      /install.ps1 -> latest GitHub release installer
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

## The installer

`public/install.sh` is what `curl -fsSL https://getbzz.dev/install.sh | sh`
runs. It is POSIX `sh` (keep it `shellcheck` clean). On Linux it checks, in
this order, the architecture, musl, the glibc version (2.34, as
`[dist.min-glibc-version]` in `dist-workspace.toml`), and that `tar` and
`xz` are there to unpack the archive; each refusal is one line that says
what is the matter and what to do. Then it downloads the latest release's
`bzz-installer.sh` (made by cargo-dist, with the archive's checksums) and
runs it with the same arguments. macOS and Windows shells go straight to
that installer. `BZZ_CHECK_ONLY=1` stops after the checks; `BZZ_VERSION`
picks a release.

The variable `BZZ_SOUND` at its top says what the Linux binary of the
release it serves needs to make sound:

- `server` (the default, from v0.17.0): bzz speaks to PipeWire or
  PulseAudio itself and starts without any sound library. The installer
  adds one note if it finds neither.
- `alsa` (v0.16.0 and earlier, or a release whose binary links
  `libasound.so.2` again): bzz does not start without ALSA's library. The
  installer names the package for the distribution (`libasound2t64` on
  Ubuntu 24.04 and Debian 13, `libasound2` before them, `alsa-lib` on
  Fedora and RHEL).

Deploy the site with the value that matches the latest release:
`readelf -d bzz | grep libasound` on its Linux archive says which.

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

The capture in `#apps` (`public/assets/apps-v016.webm`, `apps-v016.mp4`,
`poster-apps-v016.png`: 12 s, 1920×1080, 30 fps, silent, a loop) is cut
from the take of the v0.16 launch film: the `bzz 0.16.0` Linux release
build on an isolated pinned relay with generated people, the Board open
beside `#team`, the owner's agent Scout on Pi Durable with a real model
(`openai-codex/gpt-6.1-sol` through Pi's own sign-in) asked for a priority
and a filter; the line `tasks: version 2 from your agent Scout, asked by
You …`; the approval overlay with the brief `CHANGED`; and the Board after
`a`, running the model's own version in place (`Priority filter: all`, a
badge on each card) with the same card still selected. Two cuts inside the
one take: the 135 s the model took, and the reading of the overlay. Its
project is `runs/experiments/promo-video/film-v016/heavy/loop-final`.
Encodes: H.264 CRF 28; VP9 CRF 36; no audio.

## New in v0.17.0

The "new in v0.17.0" section (`#new`) may say only what
[`docs/release-v0.17.0.md`](../docs/release-v0.17.0.md) says and proves. Its
cards do not say that the join form pairs with Buzz desktop or signs in
through a browser (built, not run against the real thing), give no time for
the demo's start (the measures are of one machine), say "shifted keys" on
Windows and not "every key" (nobody has used it at Windows Terminal), and say
that huddle audio needs PipeWire or PulseAudio running. The v0.16.0 section
stays below it as `#new-016`.

Its three captures are all of `bzz demo`, which shows generated data only:
the people, their messages and the huddle's voices, offline, with no account.
They are the `bzz 0.17.0` Linux release build with the `bzz` theme and motion
at its default, recorded as terminal tapes with the promo kit's
`record-tape.py` and drawn by its player (xterm.js) in HyperFrames, as the
site's other captures are: bzz's real output, not a screenshot of a graphics
terminal. Each exists at 120×32 cells (1920×1080) and, for screens of 640 px
or less, at 60×30 cells (1080×1170, `*-narrow.*`), where bzz shows the
conversation alone.

- `demo-v017.png`: the first screen, four seconds after the start, with
  nothing typed and the screen at rest.
- `palette-v017.png`: `Ctrl+K`, `thr`, five times down to a row that cannot
  run, whose reason is at the palette's foot.
- `huddle-styles-v017.webm`, `.mp4` and `poster-huddle-styles-v017.png`: a
  13.8 s silent loop, 30 fps, one take trimmed at both ends: in `#infra`,
  `:huddle` (the panel, which says the voices are generated), `z`, `v`, `j`
  (bars), `j` (the orb), `Esc`, `z`. Encodes: H.264 CRF 28; VP9 CRF 36.

Before they were placed, the tapes' bytes, the text of every distinct screen
and an OCR of every image and of the loop at two frames a second were
searched for paths, addresses and names of the machine and its owner: none.
Their project is `runs/experiments/bzz/v017-site-captures` (`record.sh`,
`render.sh`, `scan.sh`); remake them with it if a later build changes these
screens.

## Demo media

The hero video `public/assets/promo-v016.webm`, `promo-v016.mp4` and
`poster-promo-v016.png` is the v0.16 launch film: 55.0 s, 1920×1080, 30 fps,
narrated, −14 LUFS, captions burned in so it works muted. It is the v0.15
film with the same voice, music and cut. The two lines v0.15 added (creating
an agent; moving it with its memory) gave way to two v0.16 lines: asking
your agent to change the app open beside the chat, and approving it so that
it changes in place. Every product shot is the `bzz 0.16.0` Linux release
build on an isolated pinned relay with generated people and generated
illustrated avatars, recorded as terminal tapes and composed in HyperFrames.
In the agent shots a real model answers (`openai-codex/gpt-6.1-sol` on Pi
Durable, through Pi's own sign-in), and the Board's new version on screen is
the model's own script. Make or remake it with
the [`bzz-promo-video`](../.agents/skills/bzz-promo-video/SKILL.md) skill;
the composition is in [`promo/v0.16-launch`](../promo/v0.16-launch/README.md).
Credits: voice Fish Audio Drama 3 (Fish Audio Community License; credit
required), music generated with ACE-Step 1.5 (MIT model), avatars generated
with gpt-image-2. Encodes: H.264 CRF 26 + AAC 128 kb/s; VP9 CRF 32 + Opus
96 kb/s.

## Feature films

A feature film is one feature, start to finish, with several people's
screens from one take; it sits in its feature's section, poster first,
`preload="none"`, click to play, with captions and chapters, and the films
strip under the hero links to it. The apps film (`film-apps.mp4`,
`film-apps.webm`, `film-apps-720.mp4` for phones, `poster-film-apps.png`,
`film-apps.en.vtt`, `film-apps.chapters.vtt`) is 2:42, 1920×1080, 30 fps,
narrated, −14 LUFS, captions burned in: You, Ada and Lin, three generated
people each in their own `bzz 0.16.0` (the Linux release build) on an
isolated pinned relay, recorded on one clock; in chapters 5 to 9 a real
model answers (`openai-codex/gpt-6.1-sol` on Pi Durable); chapter 10 shows
Buzz desktop 1.0.1 with a plugin written for the film, not shipped by bzz or
by Buzz, both clients driven by scripts. Its project is
`promo/v0.16-apps-film` (from `runs/experiments/promo-video/feature-apps/film`).
Credits: voice Fish Audio Drama 3, music ACE-Step 1.5. Encodes: H.264 CRF
26 + AAC 128 kb/s (1080p and 720p); VP9 + Opus 96 kb/s. Every file stays
under Pages' 25 MiB. The `#apps` loop (`apps-v016.*`) stays published so
links to it keep working.

The v0.15 film (`promo-v015.*`, `poster-promo-v015.png`,
[`promo/v0.15-launch`](../promo/v0.15-launch/README.md): creating an agent
that bzz runs, moving it with its memory) stays published so links to it
keep working.

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
