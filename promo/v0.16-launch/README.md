# bzz v0.16 launch film (HyperFrames composition)

The launch film for v0.16.0: 55.0 s, 1920×1080, 30 fps, narrated (voice:
Fish Audio Drama 3), music generated with ACE-Step 1.5, captions burned in.
It is the [v0.15 film](../v0.15-launch/README.md) with the same voice, music,
look and cut. The two lines v0.15 added (creating an agent; moving it with
its memory) gave way to two v0.16 lines about apps, placed after the two
agent lines, which go back to their v0.13 film times:

| Film time | Line | Shot |
|---|---|---|
| 18.1–22.3 s | Mention an agent, and its reply lands right in the thread. | `mention`: the picker, the message, the reply in the thread |
| 22.3–25.6 s | You decide what each agent is allowed to do. | `hands`: the agent asks to run a command, `Ctrl-Y` shows it, `a` allows it |
| 25.6–29.85 s | **Ask your agent to change the open app.** (new) | `app`: the Board `tasks` open beside `#team`, a card selected, the mention asking Scout for a priority and a filter, the line at the top `tasks: version 2 from your agent Scout, asked by You waits for your approval`, the reply count |
| 29.85–33.6 s | **Approve it, and it changes in place.** (new) | `app`, the same take: `Ctrl-Y` (from your agent Scout, asked by You, brief `CHANGED`), the end of the difference, `a`, `tasks: version 2 runs in place of 1, its state kept`, the Board with the model's priority filter and the same card selected |

Line 8 and every later shot keep their v0.15 film times. The new lines were
cloned from the same narrator take and inserted, one after the other, at the
pause after line 7 (`runs/experiments/promo-video/film-v016/voice/`): one
Drama 3 batch of eight takes, each confirmed word for word by local ASR.
Made with the [`bzz-promo-video`](../../.agents/skills/bzz-promo-video/SKILL.md)
skill.

| Path | Content |
|---|---|
| `index.html` | the composition: captions, camera, key chips, logo reveal, end card |
| `assets/tapes/{overview,keyboard,offline,huddle,media,identity}.jsonl` | terminal tapes of the **published** `bzz 0.16.0` Linux archive (`bzz-x86_64-unknown-linux-gnu.tar.xz`, sha256 `97d02b47…`, checked against the release's `sha256.sum`) on the pinned relay (`bf3bdd0e`) with generated people (`examples/promo_fixture.rs` of the tag), from the kit's `capture/shoot-all.sh` |
| `assets/tapes/{mention,hands,app}.jsonl` | one session on a second fresh fixture, recorded with a recorder that waits for what the screen shows, because a **real model** answers: Scout on Pi Durable with `openai-codex/gpt-6.1-sol` through Pi's own sign-in, three turns, one per shot, none retried. No folder was granted during `app`, and nothing on the profile's disk held a Board with priority: the version on screen is the model's own script (8.5 KiB, 155 lines; it shares none of the 56 lines `tests/fixtures/apps/board-priority/app.lua` adds to the Board) |
| `capture/` | the session as it was run, the expectations, the byte scan |
| `scripts/agent-maps.py` | writes the time maps of the agent shots into `index.html` from the tapes' marks, and prints the film second of each key chip and the length of each cut |
| `lib/` | vendored GSAP 3.15.0, xterm.js 6.0.0 + image and unicode11 addons, `tape-replay.js` |
| `scripts/build-audio.sh` | narration + music → `assets/audio/mix.wav` (−14 LUFS) |
| `scripts/apps-loop.html` | the 12 s loop for the site's `#apps`, cut from the `app` take (as rendered in `film-v016/heavy/loop-final/`, where the tape sits at `assets/app.jsonl`) |

Honest edits in the new shots, all inside one take, nothing reordered:

- **N1** starts on the Board already open (0.35 s after `:app open tasks`
  drew it); `j` `j` at 2.75×; the mention typed at about 4.6×; **a cut of
  135.0 s** from the moment it is sent to just before the line at the top
  appears (the model reading the guide and the Board, trying its script and
  proposing it); then 2.9 s of the take at 1.1×, in which the reply count
  `↳ 1 reply` appears under the mention.
- **N2**: a cut of 1.9 s (the hold before `Ctrl-Y`); the head of the
  approval for 1.25 s; **a cut of 6.8 s** over the reading (the brief and
  the difference, `j` and `G`, to `rows 578–594 of 594: the end`); then `a`
  and 2.3 s of the take at its own speed: the overlay closes, the status
  line says `its state kept`, the Board shows the model's `Priority filter:
  all (f)` and a badge on each card, and `> [N] macOS smoke test` is still
  selected.
- **Line 6**: a cut of 3.3 s over the model's answer and 1.2 s before the
  thread opens. **Line 7**: from `a` to the reply count (2.85 s of the
  take, the command running and the model answering) plays at about 3.5×.

The film's terminal draws faint text (inline code in bzz's default theme) in
a lighter grey than xterm.js would, so that it reads on a near-black ground.

The audio inputs are synthesis masters and are not committed;
`build-audio.sh` reads them from the owner's experiments or `VOICE=` /
`MUSIC=`.

```sh
export HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1
scripts/build-audio.sh
npx -y hyperframes@0.9.13 check
npx -y hyperframes@0.9.13 render -o renders/film.mp4
```

QA of the render (candidate, 2026-11-24): 1920×1080, 30 fps, 1,650 frames,
H.264 High + AAC LC 48 kHz stereo, 55.0 s; −14.3 LUFS integrated, −1.0 dBTP
true peak (the mix alone: −14.2 LUFS, −1.0 dB). Two renders: audio
identical (MD5 `aa550554…`); 1,327 of 1,650 frames identical, the other 323
all between 25.87 s and 36.6 s, with a PSNR of 49.1 dB at the worst frame
and 63.4 dB on average (the v0.13 film's two renders differed in 227 frames,
worst 53.1 dB; v0.15's were identical); both are invisible. `hyperframes
check`: runtime, layout, motion, contrast (46/46), craft and audio pass;
the four lint errors are the vendored xterm.js ones recorded for v0.13. The
15 expectations of the agent take and the kit's 19 for the six re-shot tapes
pass. A frame every second was reviewed for privacy and claims, and the
tapes' bytes were searched: generated people and keys only, no passphrase,
no path of the recording machine but `/tmp/bzz-film16/site` (line 7's
folder, on screen in the command dialog); a planted copy with the
passphrase, a fixture key, a home path, a token shape and an e-mail address
was caught.

The agent cannot hear: the owner's ear is the check of the two new lines.
