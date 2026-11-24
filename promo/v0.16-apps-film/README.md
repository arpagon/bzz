# Apps in bzz: the v0.16 feature film (HyperFrames composition)

The first feature film: one feature, apps, shown working the way people use
it. 2 min 42 s (162.0 s), 1920×1080, 30 fps, narrated by the launch films'
narrator (voice: Fish Audio Drama 3), music generated for it with ACE-Step
1.5, captions burned in, a WebVTT captions track and a chapters track. Its
format is the experiment's `FORMAT.md`, its script `SCRIPT.md` (alternative
A, the calm opening, chapter 10a), both approved by the owner at gate 1.

Every product window is the **published `bzz 0.16.0` Linux archive**
(`bzz-x86_64-unknown-linux-gnu.tar.xz`, sha256 `97d02b47…`, checked against
the release's `sha256.sum`) on the pinned relay (`bf3bdd0e`) with three
generated people, You, Ada and Lin, each in their own bzz, profile, home and
pseudo-terminal, recorded by `capture/kit/record-multi.py` on **one clock**:
the windows of a take are replayed with **one time map**, so a cut or a
speed change happens at the same tape second in each. Every key comes from a
script. In chapters 5 to 9 a **real model** answers: Scout on bzz's Pi
Durable host with `openai-codex/gpt-6.1-sol` through Pi's own sign-in, one
turn for chapters 5–8 and one for chapter 9, neither retried, with **no
folder of the machine** granted. Chapter 10 is another take: Buzz desktop
1.0.1 with a plugin written for this film (not shipped by bzz or by Buzz)
beside bzz, both driven by scripts.

| Film time | Chapter · narration | What is on screen | Take, and every edit |
|---|---|---|---|
| 0.0–6.0 | 0 · One channel. Three people. One app, beside the chat. | Ada's, Lin's and You's windows, each with the Board `tasks` open beside `#team`; the title | `together`, 1.4 s of the take at 1× then held. **The opening is from later in the take** (its Boards already open); chapter 1 goes back to the take's start, after a dip the viewer sees |
| 6.0–18.0 | 1 · An app is a small script that bzz runs for a channel… | You alone: `:app open tasks`, the Board beside `#team`; then the three windows, Ada's and Lin's Boards opening | `together` from its start, 1×; holds |
| 18.0–32.0 | 2 · Its records belong to the channel. Ada adds a card… | Ada: `n`, `Write the release notes`, Enter; the card on all three Boards (Ada's 0.15 s after her Enter, You's and Lin's 0.01 s after hers, from the bytes); tag `same take · real speed` | `together`, 1×, then held |
| 32.0–48.0 | 3 · Two people move the same card at the same instant… Nothing is lost. | Ada and Lin select `Check the status page`; Ada `]` and Lin `[` written 0.03 ms apart; **½ speed** for 0.6 s of the take around that instant, in every window, tagged; Lin's `Saved.`, Ada's `Conflict: merged with a newer change.`; three identical Boards, the card once, in Done | `together`, 1× except the labelled ½ speed; held |
| 48.0–67.4 | 4 · Three apps ship with bzz… where nobody signs off their own request. | `#launch`: Lin asks `Launch day lunch?` (Tacos, Sushi, Pizza), three votes, `Total: 3 votes` on each; `#design`: Ada files `Order a test Mac mini`, it shows on You's window, her own `a` is refused (`You cannot decide your own request.`), You approves it | `poll`: Lin's typing at 3.4×, Ada's and You's `:app open poll` at 1.5×, the votes at 1×. `approvals`: Ada's typing at 2×, then 1× |
| 67.4–81.4 | 5 · Want something else? Ask your agent… | You (large): `j` to `macOS smoke test`, `@Scout add a priority and a filter to the board`, Enter; **⏩ 1 min 47 s · the model's turn**; the line at the top `tasks: version 2 from your agent Scout, asked by You waits for your approval`, `↳ 1 reply`; Ada's window beside | `agent`: typing at 2×; **a cut of 106.6 s** (send → the line at the top), shown by the chip; then 1× |
| 81.4–97.4 | 6 · Nothing it wrote runs until you have read it… | `Ctrl-Y`: `From: your agent Scout`, `Asked by: You`, `Hash`, `Script: 6.5 KiB`, `Brief: … CHANGED`; `j`×12 through the brief and the difference; `G` to `the end` | `agent`, 1×, a hold on the head of the approval |
| 97.4–107.4 | 7 · Approve it, and the open app changes in place. The same card is still selected. | `a`: `tasks: version 2 runs in place of 1, its state kept`; the model's Board: `Priority: all  f filter / search`, a badge `[N]` on each card, `> [N] macOS smoke test` still selected; Ada's Board beside, still version 1 (tag) | `agent`, 1× |
| 107.4–125.4 | 8 · To give it to the others, offer it in the channel… | You: `:app share`; the windows swap sizes; Ada's line at the top `tasks: version 2 from You, a person of the channel`; her `Ctrl-Y`: `From: You`, `Offer: shared in the channel; this exact revision … is signed by You`, the same hash; `G`, `a`: version 2 runs in place on her screen | `agent`, 1× |
| 125.4–139.4 | 9 · Agents work with an app's records too… Move the release notes card to Done. | `@Scout move the release notes card to done`; **⏩ 9 s · the model's turn**; `Write the release notes` in Done on all three Boards (You's and Ada's version 2, Lin's version 1) within 40 ms; `↳ 1 reply` | `records`: typing at 2.5×; **a cut of 9.05 s** (mention → the card on Lin's Board), shown by the chip; then 1× |
| 139.4–150.0 | 10 · The records travel, and each client draws its own board. Here, Buzz desktop, with a plugin written from the brief. | Buzz desktop 1.0.1 (Ada) beside bzz (You), and always the words `Buzz desktop 1.0.1 with a plugin written for this demo from the board's brief. Not shipped by bzz or by Buzz.`, tag `another take · re-enacted by scripts`: You makes `Update the launch checklist` and it appears in Buzz desktop; Ada moves it to Doing there and it moves in bzz; both move it at once and both end on To do | Another take (`assets/interop/`, see `align.json`): 10.6 s of it at 1×, video second = tape second + 1.6879 s + 0.027 s (its clapper's median latency). **bzz's window shows rows 0–25 of 32**: its Board's status row read `Loaded 5 cards.` while four were shown, until tape 17.37 (unexplained, see below), so that row is out of the picture for the whole chapter |
| 150.0–162.0 | 11 · bzz. Apps in the channel. | the end card: `bzz app new tasks`, the install line, `getbzz.dev/#apps`, the fine print | — |

**Not said or shown**: any speed of Buzz desktop (its take ran on a loaded
machine); that any agent does this (only this model was run); that an
agent makes a new app; any platform but Linux; that Buzz desktop supports
bzz apps.

**Found while recording, not in the picture:** in the Buzz desktop take,
bzz's Board said `Loaded 5 cards.` with four cards, after another client
deleted records between takes (the take's `NOTES.md`). In chapter 9's
take, the card's `order` came back one unit in the last place off
(`1795537221.1991239` → `…199124`) through the agent's `records` tool
(`capture/validation/`, and `VALIDATION-M7-real-model.md` in the
experiment). Both are for the principal.

| Path | Content |
|---|---|
| `index.html` | the composition, **generated** by `scripts/make-film.py` from the takes' marks (do not edit by hand) |
| `scripts/make-film.py`, `scripts/film.css` | maps, layouts, camera, chips, captions; `timing/film.json` (chapters, every cut and speed with what it hides) |
| `scripts/build-audio.sh` | narration + music → `assets/audio/mix.wav` (−14 LUFS, −1.5 dBTP); `VOICE=`, `MUSIC=`, `MUSIC_END=`, `OUT=` |
| `scripts/make-vtt.py` | the two WebVTT tracks and the chapter list, from the same timing |
| `timing/narration.json` | each line's take, slot, measured speech and caption splits (made by the experiment's `voice/build-narration.py`) |
| `assets/tapes/<take>/<pane>.jsonl`, `sync.json` | the five takes: `together`, `poll`, `approvals`, `agent`, `records` |
| `assets/interop/` | chapter 10's take: `buzz.mp4`, `bzz.jsonl`, `align.json` (placement, the keys of its chips, the cropped rows) |
| `capture/` | the session as it was run (paths of the recording machine are variables in `env.sh`: `BZZ_CHECKOUT`, `FILM_HEAVY`, `FILM_BUILD`, `PI_DIR_OF_OWNER`, `REAL_NODE`, `REAL_NPM`), its kit, `run/` (takes, turns, expectations, scan, teardown), `approved/` (the model's version 2: `app.lua`, `brief.md`, and what each person was shown), `validation/` (chapter 9's evidence) |
| `lib/`, `assets/fonts/`, `assets/avatars/` | vendored GSAP 3.15.0, xterm.js 6.0.0 + image and unicode11 addons, `tape-replay.js`; JetBrains Mono; the kit's generated avatars |

```sh
export HYPERFRAMES_NO_TELEMETRY=1 DO_NOT_TRACK=1
python3 scripts/make-film.py
VOICE=<narration.wav> MUSIC=<take.wav> MUSIC_END=164.1 scripts/build-audio.sh
npx -y hyperframes@0.9.13 check
npx -y hyperframes@0.9.13 render -o renders/film.mp4
ffmpeg -i renders/film.mp4 -i assets/audio/mix.wav -map 0:v -map 1:a -c:v copy -c:a aac -b:a 256k -movflags +faststart renders/film-master.mp4
```

The last line puts the mix itself in the master: the renderer's own audio
came out 0.4 LU under it (−14.7 against −14.3 LUFS).

## QA of the candidate (2026-11-24)

- **Format:** 1920×1080, 30 fps, 4,860 frames, H.264 High + AAC LC 48 kHz
  stereo, 162.0 s; master 36.4 MB.
- **Loudness:** −14.4 LUFS integrated, −1.3 dBTP true peak, LRA 4.8 LU
  (the mix alone: −14.4 LUFS, −1.5 dB).
- **Two renders:** audio identical (MD5 `f82545d2…`); 3,676 of 4,860 frames
  identical; the other 1,184 in five spans (0–22.6 s, 34.0–39.3, 82.3–86.4,
  88.8–94.7, 138.0–139.4), worst-frame PSNR 45.5 dB (82.4 s), mean 60.9 dB;
  the worst frame's difference, at eight times the contrast, is the edge of
  terminal text: invisible. (v0.16 launch film: 323 frames, 49.1 dB.)
- **`hyperframes check`:** runtime, layout (0 errors; 4 notes: windows past
  the canvas during push-ins, intended), motion, contrast (69/69 AA) and
  audio pass. It exits 1 on the four lint errors of the vendored xterm.js
  recorded since v0.13. Craft: one warning, the end card's fine print
  (51 words) is on screen 10.3 s against the 11.9 s it asks for.
- **Takes:** 37 of 37 expectations `ok` (`capture/run/expect-final.txt`);
  the 15 tapes' bytes searched for the machine, its owner, the fixture's
  four secret keys and three passphrases, and key, token, e-mail and
  sign-in shapes: no hit; a planted copy was caught. Chapter 10's tape and
  video were searched by its own take (`NOTES.md` there) and again here:
  no hit.
- **Frames:** one every second (162) looked at for privacy and claims, and
  read by OCR for the same terms: no hit; a planted frame was caught.
- **Type:** captions 54 px; kickers, tags and fine print 28 px; terminal
  text in three windows side by side is 16 px at rest, pushed to 1.3× on
  the windows each line names.

The agent cannot hear: the owner's ear is the check of the voice and music.
