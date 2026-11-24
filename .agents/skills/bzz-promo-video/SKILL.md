---
name: bzz-promo-video
description: Make a bzz promotional, launch or feature film from the real TUI, with narration, music and kinetic captions. Use when asked for a promo, launch or social video of bzz, a longer film about one feature (several clients side by side), or to replace the getbzz.dev hero video.
---

# bzz promotional film

A film is a **story told with the real product**: every product shot is bzz's
own terminal output, recorded as a *tape* on the pinned relay with generated
people, then replayed inside a HyperFrames composition. The reference film
`docs/planning/2026-11-11/promo-video.md` and
[`docs/adr-promo-video-compositor.md`](../../../docs/adr-promo-video-compositor.md)
record why.

Work in an ignored experiment (`experiment-mode` skill):
`runs/experiments/promo-video/{voice-casting,capture,film}`. Only reviewed
deliverables leave it.

A **feature film** (one feature, 2–3 minutes, several people's screens from
one take) follows these steps with the changes in
[references/feature-film.md](references/feature-film.md).

## Steps

1. **Brief and script.** One story: hook → turn ("This is bzz") → proof beats
   → install end card. 45–55 s, ~90–110 words, English. Every claim is a shipped,
   validated feature (`docs/validation-v*.md`); no partial-platform claims.
   The real TUI and the name are on screen by 3 s; the film works muted.
   "bzz" sounds like "buzz", so the picture carries the name.
   *Done:* the owner approved the script and each line's picture (gate 1).
2. **Voice and music** — [references/audio.md](references/audio.md). Narration
   first (its timing sets the cut), then music, then the mix.
   *Done:* owner picked voice and music by ear (gate 2); `scripts/build-audio.sh`
   in the film makes the −14 LUFS mix; each line's film time is measured.
3. **Animatic.** HyperFrames project with real audio, captions, logo reveal,
   end card and one labelled placeholder per product shot.
   *Done:* `hyperframes check` passes layout, motion, contrast and audio; the
   owner approved the timing (gate 3).
4. **Capture** — [references/capture.md](references/capture.md). Fresh fixture,
   then `capture/shoot-all.sh` records every shot and asserts what each tape
   shows. *Done:* every expectation prints `ok`; no tape shows an error the
   line does not intend; `tape-peek.py` / `preview-tape.sh` reviewed.
5. **Compose** — [references/film.md](references/film.md). Replace each
   placeholder with its tape (`mountTape` + time `map`), camera pushes and key
   chips. *Done:* stills reviewed at every line; text ≥ 26 px after push-ins.
6. **Render and QA.** Render twice; compare with `framemd5` and `psnr`. Check
   format (1920×1080, 30 fps, H.264/AAC), loudness (−14 LUFS, ≤ −1 dBTP) and a
   one-frame-per-second contact sheet for privacy and claims.
   *Done:* QA numbers recorded in the film README; owner approved (gate 4).
7. **Publish** (only after gate 4): site assets with a `?v=` bump, poster,
   `site/README.md` (credit "Voice: Fish Audio Drama 3" when Drama is used),
   deploy per `AGENTS.md`, check the live page on desktop and mobile.

## Rules

- **Real, generated, isolated.** Fixture people, messages and images are
  generated or bzz-owned assets on `ws://localhost:3030`. Never a real
  community, identity or keychain. Keychain shots run in
  `capture/identity-shell.sh`, which refuses the user's session bus.
- **Honest time edits.** A `map` may speed up typing, hold, or jump-cut within
  one tape. It never reorders cause and effect or joins two tapes as one take.
- **Shots mutate state.** Reactions toggle and agents reply on every take;
  re-record from a fresh fixture (`setup-fixture.sh`) for a final pass.
- Owner gates are approvals by the owner, not by the agent.

## Kit

| Path | Use |
|---|---|
| `scripts/record-tape.py` | headless PTY recorder: fixed cell/pixel size, key timeline, marks |
| `scripts/record-live.py` | the same tape, for a take whose timing is not yours (a real model answers): a step waits until a text is on screen, and every wait is marked; `"soft": true` marks a timeout and goes on |
| `scripts/record-multi.py` | several panes (each its own bzz) from one timeline on one clock: one tape per pane and `sync.json` (wall-clock origin, keys, marks); also the bzz side of a take a screen recording joins |
| `scripts/same-board.py`, `scripts/first-seen.py` | at a mark, every pane shows the same Board; the first tape second a pane shows a text |
| `scripts/calibrate-multi.py`, `scripts/calibrate-window.py` | the precision of `record-multi.py` among tapes, and of an x11grab window against a tape |
| `scripts/tape-peek.py` | screen text at chosen seconds (drops Sixel) |
| `scripts/tape-expect.py` | assert `+text`/`-text` at a second or mark; exit 1 on failure |
| `scripts/preview-tape.sh` | pixel stills of a tape through HyperFrames |
| `assets/tape-replay.js` | `mountTape()` for compositions (xterm.js, Sixel, Unicode 11) |
| `capture/*.sh`, `capture/shots/*.json` | relay, fixture, one script per shot, the shot list (v0.14: adds `edit` and `sync`) |
| `promo/v0.15-launch/capture/` (repo) | the agent take of the v0.15 film as it was run: a local agent on a real model, created, mentioned, asked for a command, moved |
| `examples/promo_fixture.rs` (repo) | community seed, agent bot role, live "director" scenes |
