# bzz launch film and `bzz-promo-video` skill

**Status:** approved 2026-11-11. Owner decisions: Drama 3 narration (for
emotional delivery); English only; story draft approved; revenue is zero, so
the Drama Community License permits commercial use of the output (credit
"Voice: Fish Audio Drama 3" in the video description and `site/README.md`);
generated music without copyright protection is acceptable. Open: which voice
to clone (consent required) and formats beyond 16:9.
**Goal:** replace the outdated hero video on getbzz.dev with a launch film of
the same quality as the Buzz × 21c Studios reference. While making the film,
build `.agents/skills/bzz-promo-video/` so that any agent can make the next
film from a brief.

## Why the current hero video must go

- `site/public/assets/demo.mp4` is from v0.12 (commit `4b7a419`, 2026-08-28).
  It is a 45 s, 1280×720, 15 fps screen recording with no story or sound.
- Its status bar shows `network error: IO error: Connection refused (os error
  111)` (the offline fixture). A promotional video must not show an error.
- It does not show v0.13 features: huddles, the agent broker, verified
  profiles, artifacts, custom emoji, or presence.
- The v0.13 demo (`demo-v013.mp4`) is good evidence, but it is a feature tour.
  It is not a launch film.

## Reference teardown

Source: <https://x.com/itsmaxdemarco/status/2120521101226721316> (downloaded
with `yt-dlp` to `/tmp/bzz-promo-ref/`, contact sheets `sheetA.png`,
`sheetB.png`, transcript from local ASR).

| Property | Measured |
|---|---|
| Length | 50.7 s |
| Video | 1920×1080, 30 fps, H.264, 3.8 Mbit/s |
| Audio | AAC 48 kHz stereo; −9.1 LUFS integrated, LRA 2.0 LU, peak 0 dBFS |
| Narration | about 115 words, one voice, about 2.3 words per second |
| Hard cuts | 12 scene cuts; animation inside each scene between cuts |

| Time | Act | Picture |
|---|---|---|
| 0–11 s | Problem | Word-by-word type with icon chips ("to write code", "to review", "to fix bugs"); a wall of isolated terminals |
| 11–19 s | Turn and reveal | "So what if your agents…" → logo → wordmark with particles |
| 19–28 s | Product in context | Laptop on a desk, push-in to the real UI; the thread fills with messages |
| 29–36 s | Openness | Cursor clicks "+" → row of agent icons; "It's open source, so host it yourself" |
| 37–41 s | Agents | Agent profile cards, mention chips, agents reply in the thread |
| 42–47 s | Thesis | People and agents orbit the wordmark |
| 48–51 s | End card | Wordmark plus a "Get the app" button |

What makes it good, in order of impact:

1. **One story.** Problem → turn → product → proof → call to action. Every
   sentence of narration has its own picture.
2. **Sound.** Professional narration, a music bed, and cuts on the beat.
3. **The real product, staged.** Clean, believable data; a virtual camera
   pushes in to the part that matters; a cursor shows each action.
4. **Kinetic type.** Text carries the message without sound. This is required
   for muted autoplay on a website.
5. **One brand system.** One accent color, one typeface, one motion language
   (soft ease, slow parallax, particles).

## Gap: what we need

| Need | We have | Gap |
|---|---|---|
| Story and script | Feature list, release notes | Write a 45–55 s script and shot list |
| Brand system | Accent `#ffb000` on `#0e0c08`, hexagon logo and owner logo animation, 60 themes | Motion rules, display typeface, end card, lower thirds |
| Real UI at film quality | Direct Ghostty/Xvfb capture, 1280×720, 15 fps (`bzz-visual-capture`) | **Solved by the spike:** terminal tapes replayed by xterm.js inside the film, sharp at 4K ([ADR](../../adr-promo-video-compositor.md)) |
| Staged data | `examples/demo_fixture.rs` (people, channels, thread, emoji) | Story fixture with a live agent reply through the broker, a huddle and an inline image; no error in any status bar |
| Choreography | Paced key injection with `0xFF` pause markers | Timeline-driven injection that logs the time of each marker, so cuts and sound hit actions |
| Motion graphics and composite | `visual-explainer-video` renders HTML on a virtual clock, but cannot play `<video>` | **Solved by the spike:** HyperFrames 0.9.13, frame-identical renders |
| Narration | `voice` skill: Qwen3-TTS locally (saved voices including `Seb_en`) or Drama 3 on Modal (inline delivery tags) | Pick backend and voice by ear (decision 2); Qwen service is stopped |
| Music | `music` skill: ACE-Step 1.5 XL Turbo locally on the RTX 3090, 48 kHz stereo WAV, pinned image and models | Brief, 4–8 seeded takes, owner audition; cut to beats measured in the take |
| Sound effects | None | Keystroke, whoosh and logo sting: record, generate with ACE-Step, or use CC0 sources with a license record |
| Mix and master | `ffmpeg` | Target −14 LUFS integrated, −1 dBTP true peak, narration over music with ducking |
| Captions | None | Burned-in captions for the silent cut; WebVTT for the site |
| Quality control | Frame review by hand | Script that checks format, loudness, frame privacy and claims |

## Story draft (for owner review)

Audience: Buzz users who live in the terminal, and people who build agents.
Language: English, as on the site. About 110 words.

| # | Narration | Picture |
|---|---|---|
| 1 | Your team talks in Buzz. Your agents work in the terminal. | Type on dark; Buzz-style chat window and terminals drift apart |
| 2 | And so do you. | One terminal comes forward; cursor blinks |
| 3 | So why leave it? | Hard cut to black, then the hexagon draws in amber |
| 4 | This is bzz — all of Buzz, at terminal speed. | Wordmark with honey particles; the TUI opens behind it |
| 5 | Channels, threads and reactions, all from the keyboard. | Push-in: `j`/`k`, open a thread, react |
| 6 | Mention an agent, and it answers in the thread. | `@` picker → a broker agent reply appears |
| 7 | Jump into a huddle with real audio. | 🎧 markers; push-in on the roster |
| 8 | Pick from sixty themes. | Fast cut on the beat through six themes |
| 9 | Your keys stay in your keychain. Your history stays on your disk. It even works offline. | Lock and disk icons; relay disconnects, history still scrolls |
| 10 | Your agents get only the permissions you give them. | `:broker` panel, toggles flip |
| 11 | bzz. Install it in one line. | End card: logo, `curl -fsSL https://getbzz.dev/install.sh \| sh`, getbzz.dev |

Every claim maps to a shipped, validated v0.13 feature (`docs/validation-v0.13.0.md`).
Avoid claims about platforms where evidence is partial (Windows audio).
Show the unofficial-client disclaimer from the site on the end card, and do not
use Buzz or Block brand assets as if bzz were an official product.

## Production pipeline

Sources live in `promo/<slug>/` (brief, script, shot list, compositions).
Intermediates go to a restricted temporary directory. Only reviewed outputs go
to `site/public/assets/`.

0. **Compositor spike — done 2026-11-11.** HyperFrames plus terminal tapes
   passed every criterion: frame-identical renders, sharp TUI text at a 4K
   push-in, and a render time of 9.7 s for 7 s of film. Decision and evidence:
   [ADR](../../adr-promo-video-compositor.md).
1. **Brief, script and shot list** → owner gate 1.
2. **Animatic.** Type and boards at final timing with temporary narration or
   captions → owner gate 2 (timing and story).
3. **Capture.** Story fixture on an isolated pinned relay with generated
   identities, `[media] protocol = "sixel"`, and the release binary. Record
   one tape per shot with `record-tape.py`, adding two seconds of handles and
   named marks for each action. Review the rendered frames for private data and
   error text (the spike's offline fixture showed `network error` in the status
   bar; the live relay removes it).
4. **Motion graphics and composite.** Window frames, virtual camera, cursor and
   key overlays, type, particles, end card. Build the window and any desk
   scene in code; a generated still may only be a background, never a fake UI.
5. **Audio.** Run the GPU services one at a time: ACE-Step refuses to start
   while `qwen3-tts-server` or ComfyUI runs, and needs 20 GiB of free VRAM.
   - **Narration first** (`voice` skill), because its timing sets the cut.
     Make three candidates of lines 1–4: Qwen with `Seb_en`, Qwen with another
     saved voice, and Drama with delivery tags. The owner picks by ear → gate 3.
     Then render every line as its own clip and measure each duration.
   - **Music second** (`music` skill): an instrumental brief at the edit's
     length plus about 4 s, for example "warm analog synth, 110–122 BPM, soft
     intro under the problem, lift at the logo reveal, confident resolve at the
     end card, no vocals". 4–8 sequential seeds; keep each request, seed and
     pins beside the take. Owner auditions → gate 3b.
   - Requested BPM and cue times do not guarantee beat positions. Measure
     beats in the chosen take (onset detection) and move the cuts to them, or
     trim the music with fades; never claim sync without measuring it.
   - Sound effects, then mix: duck music about 8–10 dB under narration and
     master to −14 LUFS integrated, −1 dBTP true peak.
6. **Conform and encode.**
   - Film: 1920×1080, 30 fps, H.264 MP4 and VP9/AV1 WebM, with sound.
   - Site hero: 20–30 s silent loop in which type carries the message,
     at most 4 MB per format, plus poster and WebVTT.
   - Optional: 1080×1920 and 1080×1350 cuts for X (decision 4).
7. **Quality control** (script) → owner gate 4 (final). Then publish: replace
   the hero video, bump the `?v=` queries, update `site/README.md`, deploy and
   check the live site on desktop and mobile.

## The `bzz-promo-video` skill

Write the skill **during** production, one stage at a time, from what worked.
Follow `writing-for-agents` and its `SKILL-MECHANICS.md`.

```
.agents/skills/bzz-promo-video/
  SKILL.md            # steps 0–7 above, owner gates, completion criteria
  references/
    story.md          # act structure, word budget, claim rules
    brand.md          # tokens, type, motion rules, end card (points to site CSS)
    audio.md          # routes to the voice and music skills; brief templates,
                      # GPU order, beat measurement, mix targets, license record
    qa.md             # delivery specs and review checklist
  assets/
    tape-replay.js    # done: replays a tape in the film (xterm.js + Sixel)
  scripts/
    record-tape.py    # done: headless PTY recorder with timeline and marks
    capture-shots     # shot list → fixture, one tape per shot
    render            # compositor entry point
    mix               # narration + music + effects → mastered stereo
    qa                # ffprobe, loudness, privacy and claim checks, contact sheets
```

It reuses `bzz-visual-capture` for capture, `bzz-prove-it-works` for claims,
and the user's `voice` and `music` skills for sound, instead of copying them.
Those two skills live outside this repository; `audio.md` names them as
prerequisites and says what to do on a machine without them (silent cut with
captions).

The skill is done when:

- `scripts/qa` fails on planted defects: a frame containing a fake `nsec1…`
  string, loudness of −6 LUFS, a 24 fps file, and a narration claim that is
  absent from the claims list. It passes on the final film.
- A fresh agent with only the skill and a one-paragraph brief makes a 15 s
  promotional clip that passes `scripts/qa` without further help.
- `site/README.md` points to the skill for new media.

## Decisions

1. **Language:** English only, or a Spanish version as well? Saved voices
   exist for English, Spanish and Portuguese.
2. **Narration backend and voice:** Qwen locally (Apache-2.0 model, no cloud
   cost, uses the local GPU) or Drama 3 on Modal (delivery tags such as
   `[confident]`, uses Modal credits and uploads the text and reference clip).
   Is `Seb_en` your voice, and do you consent to its use for the film (and to
   its upload to Modal if we use Drama)?
3. **Drama 3 license:** its Fish Audio Community License allows commercial
   use of output only below US$500,000 annual revenue for the organization
   and affiliates, and asks for attribution when a product uses the model.
   Confirm that this applies, or we use Qwen only.
4. **Music rights:** the ACE-Step 1.5 model repositories are MIT and the model
   card permits commercial use of output. The project's Hugging Face demo Space
   has a stricter separate license; we use the self-hosted MIT weights, not the
   Space. Record model revisions and seeds for every track. Generated music
   may have no copyright protection; confirm that this is acceptable.
5. **Formats:** 16:9 only, or also vertical and 4:5 cuts for X?
6. **Message:** do you approve the story draft and the line "all of Buzz, at
   terminal speed"?

## Effort estimate

About four to six working days, depending on revisions: spike 0.5, script and
animatic 0.5, capture 1, motion and composite 1.5–2, audio 0.5, quality
control and publication 0.5. The skill is written along the way and tested
for half a day at the end. Measured setup costs: ACE-Step model startup about
four minutes, then about 16 s for a 20 s take; a Drama cold start takes
minutes on Modal.
