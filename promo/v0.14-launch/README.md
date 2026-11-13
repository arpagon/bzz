# bzz v0.14 launch film (HyperFrames composition)

The hero video on getbzz.dev for v0.14.0: 54.9 s, 1920×1080, 30 fps,
narrated (voice: Fish Audio Drama 3), music generated with ACE-Step 1.5,
captions burned in. It is the [v0.13 film](../v0.13-launch/README.md) with
the same voice, music, look and cut, plus two v0.14 lines:

| Film time | Line | Shot |
|---|---|---|
| 13.3–16.55 s | Made a typo? Just edit it. | `edit`: a typo sent in #team, `e`, the fix, `(edited)` |
| 28.85–33.5 s | Solo chats and sidebar sections stay in sync with Buzz. | `sync`: `:section new Launch week`, then `:solo Notes` |

Every later shot moves by 3.25 s and then 4.65 s. The new lines were cloned
from the same narrator take and inserted at pauses of the approved narration
(`runs/experiments/promo-video/voice-casting/v014/`). Made with the
[`bzz-promo-video`](../../.agents/skills/bzz-promo-video/SKILL.md) skill.

| Path | Content |
|---|---|
| `index.html` | the composition: captions, camera, key chips, logo reveal, end card |
| `assets/tapes/*.jsonl` | terminal tapes of the `bzz 0.14.0` Linux release build on the pinned relay (`bf3bdd0e`) with generated people (`examples/promo_fixture.rs`, `capture/shoot-all.sh`) |
| `lib/` | vendored GSAP 3.15.0, xterm.js 6.0.0 + image and unicode11 addons, `tape-replay.js` |
| `scripts/build-audio.sh` | narration + music → `assets/audio/mix.wav` (−14 LUFS) |

The audio inputs are synthesis masters and are not committed;
`build-audio.sh` reads them from the owner's experiment or `VOICE=` / `MUSIC=`.

```sh
export HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1
scripts/build-audio.sh
npx -y hyperframes@0.9.13 check
npx -y hyperframes@0.9.13 render -o renders/film.mp4
```

QA of the published render (v2.0): −14.2 LUFS integrated, −1.0 dB peak; all
19 tape expectations pass. Two renders have identical audio; 468 of 1,647
video frames differ, worst-frame PSNR 48.2 dB (not visible), during the
Sixel-image, avatar and theme shots. `hyperframes check`: layout, motion,
contrast (43/43) and audio pass; the four lint errors are the vendored
xterm.js ones recorded for v0.13.
