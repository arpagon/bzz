# bzz v0.15 launch film (HyperFrames composition)

The launch film for v0.15.0: 55.0 s, 1920×1080, 30 fps, narrated (voice:
Fish Audio Drama 3), music generated with ACE-Step 1.5, captions burned in.
It is the [v0.13 film](../v0.13-launch/README.md) with the same voice, music,
look and cut. The two lines the [v0.14 film](../v0.14-launch/README.md) added
(editing a message; Solo chats and sections) made room for two v0.15 lines,
and the two agent lines of v0.13 now show an agent that bzz itself runs:

| Film time | Line | Shot |
|---|---|---|
| 18.1–22.35 s | **Create your own agent. It runs on your machine.** (new) | `create`: `:agent new Scout`, the form, the creation with its capability record, the agent idle in `:agents` |
| 22.35–26.55 s | Mention an agent, and its reply lands right in the thread. | `create`, the same take: the mention picker, the message, the reply in the thread |
| 26.55–29.85 s | You decide what each agent is allowed to do. | `hands`: the agent asks to run a command, `Ctrl-Y` shows it, `a` allows it |
| 29.85–33.6 s | **Then move it, memory and all.** (new) | `move`: `bzz agent move` in a shell; then `there`: the bzz that received it, where the agent recalls the word and still has its files |

Every later shot moves by 8.0 s against the v0.13 film. The new lines were
cloned from the same narrator take and inserted at pauses of the approved
narration (`runs/experiments/promo-video/voice-casting/v015/`): one Drama 3
batch of eight takes, each confirmed word for word by local ASR. Made with
the [`bzz-promo-video`](../../.agents/skills/bzz-promo-video/SKILL.md) skill.

| Path | Content |
|---|---|
| `index.html` | the composition: captions, camera, key chips, logo reveal, end card |
| `assets/tapes/*.jsonl` | terminal tapes of the `bzz 0.15.0` Linux release build (`dacb3b0`) on the pinned relay (`bf3bdd0e`) with generated people (`examples/promo_fixture.rs`). `overview`, `keyboard`, `offline`, `huddle`, `media` and `identity` come from the kit's `capture/shoot-all.sh` |
| `assets/tapes/{create,hands,move,there}.jsonl` | one story on a second fresh fixture, recorded with a recorder that waits for what the screen shows, because a **real model** answers: `openai-codex/gpt-6-luna` through Pi's own sign-in, six short turns. Node was found at a neutral path, so no path of the recording machine is in a tape; `move` ends before a later `ls -l` that printed the user's name |
| `scripts/agent-maps.py` | writes the time maps of the agent shots into `index.html` from the tapes' marks |
| `lib/` | vendored GSAP 3.15.0, xterm.js 6.0.0 + image and unicode11 addons, `tape-replay.js` |
| `scripts/build-audio.sh` | narration + music → `assets/audio/mix.wav` (−14 LUFS) |

Honest edits in the agent shots: the form is typed at about seven times its
speed; the first-use install of the agent host and the seconds the model
took to answer are cut inside one take; the passphrase prompts of
`agent move` run fast. "Move" is shown between two bzz profiles of one owner
on one machine; the film says "move it", not where.

The film's terminal draws faint text (inline code in bzz's default theme) in
a lighter grey than xterm.js would, so that it reads on a near-black ground.

The audio inputs are synthesis masters and are not committed;
`build-audio.sh` reads them from the owner's experiment or `VOICE=` / `MUSIC=`.

```sh
export HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1
scripts/build-audio.sh
npx -y hyperframes@0.9.13 check
npx -y hyperframes@0.9.13 render -o renders/film.mp4
```

QA of the render (v3.1): −14.2 LUFS integrated, −1.0 dB peak. The thirteen
expectations of the agent take and the kit's expectations for the six
re-shot tapes pass. Two renders are identical, audio and every one of the
1,650 frames. `hyperframes check`: runtime, motion, contrast (47/47), craft
and audio pass; the four lint errors are the vendored xterm.js ones recorded
for v0.13. A frame every second was reviewed, and the tapes' bytes were
searched: generated people and keys only, no passphrase, and no path of the
recording machine but `/tmp/bzz-demo` and `/tmp/bzz-node`.

The agent cannot hear: the owner's ear is the check of the two new lines.
