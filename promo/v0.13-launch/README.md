# bzz v0.13 launch film (HyperFrames composition)

The hero video on getbzz.dev and the launch post: 47 s, 1920×1080, 30 fps,
narrated (voice: Fish Audio Drama 3), music generated with ACE-Step 1.5,
captions burned in. Made with the
[`bzz-promo-video`](../../.agents/skills/bzz-promo-video/SKILL.md) skill;
the plan and decisions are in
[`docs/planning/2026-11-11/promo-video.md`](../../docs/planning/2026-11-11/promo-video.md).

| Path | Content |
|---|---|
| `index.html` | the composition: captions, camera, key chips, logo reveal, end card |
| `assets/tapes/*.jsonl` | terminal tapes of the published `bzz 0.13.0` Linux binary on the pinned relay with generated people (`examples/promo_fixture.rs`, `capture/shoot-all.sh`) |
| `lib/` | vendored GSAP 3.15.0, xterm.js 6.0.0 + image and unicode11 addons, `tape-replay.js` |
| `scripts/build-audio.sh` | narration + music → `assets/audio/mix.wav` (−14 LUFS) |

The audio inputs (Drama take 002 cloned for the script, ACE-Step seed 1101)
are synthesis masters and are not committed; `build-audio.sh` reads them from
the owner's experiment or `VOICE=` / `MUSIC=`.

```sh
export HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1
scripts/build-audio.sh
npx -y hyperframes@0.9.13 check
npx -y hyperframes@0.9.13 render -o renders/film.mp4
```

QA of the published render (v1.2): −14.3 LUFS integrated, −1.0 dB peak; all 14
tape expectations pass; two renders have identical audio and visually identical
video (worst-frame PSNR 53 dB, Sixel and theme shots only).
