# Feature films

A **feature film** is one feature, shown working the way people use it:
two or three people's screens from one take, the feature's story chapter
by chapter, 2–3 minutes. Every step and rule of `SKILL.md` holds; this file
is what a feature film adds. The first one, with its numbers, is
`promo/v0.16-apps-film` (made in `runs/experiments/promo-video/feature-apps/`).

## What differs from a launch film

| Step | Feature film |
|---|---|
| 1. Script | One feature: a cold open (~5 s), **chapters of one claim each** (10–18 s), an end card with the feature's own command and the release notes' limits as fine print. ~210 words; 2:30–2:45, so each file stays under the site host's 25 MiB. Every chapter names its validation section. A chapter whose claim no real run has shown is a **validation run** first (rule 4). *Done:* owner's gate 1. |
| 2. Voice | The approved narrator, one line per chapter, **two takes of each line**, up to eight texts a Drama batch (`audio.md`). Build the narration from one take per line placed at its chapter's slot; keep every other take alone and in context. *Done:* every line has a take that ASR confirms word for word. |
| 4. Capture | **Multi-client takes** (below), a real model where a chapter shows an agent. |
| 5. Compose | Generate the page from the takes' marks (`make-film.py` in the first film): one `mountTape()` per window, **one map per take**, window bars with avatar, name, `bzz <version> · #channel`; chapter kickers; captions split at the take's own pauses. |
| 6. QA | As the launch film, plus a WebVTT captions track and a chapters track made from the same timing, and a look at every second. |
| 7. Publish | In the feature's section of getbzz.dev: poster, `preload="none"`, click to play, `<source media="(max-width: 640px)">` for a 720p MP4; a line in the films strip under the hero. |

## Multi-client takes

`scripts/record-multi.py` records several panes, each its own bzz (its
own profile, home and PTY), from **one timeline on one clock**. Each tape
is the kit's format; `sync.json` beside them holds the origin (monotonic
and wall clock), every key write and every mark. Steps: `record-live.py`'s
with a `pane`, plus `together` (one key to several panes in one instant)
and `press … until` (a key until a text shows). Measured: three panes'
stamps of one instant differ by 0.1 ms (2.9 ms at worst); a frame is 33 ms.

1. **People.** One fixture; a profile for each person on screen
   (`promo_fixture seed` writes the viewer's, `film_people.rs` the others'
   from its `state.json`). Each person installs and approves the feature's
   state **at their own shell**. Only the agent's owner gets Pi's folder
   (a link); the others get an empty one.
   *Done:* every profile opens its channel `online` in its own `bzz.sh <who>`.
2. **Timeline.** One take per scene in which windows show one cause and
   its effect. Wait on what a window shows (`until`), never on seconds;
   mark every beat the film cuts on, and a `compose` mark before each
   mention so the typing can be sped. Wait texts fit the narrowest pane:
   `↳ 1 reply · last reply just now` wraps at 80 columns and never matches
   whole. Leave a minute between two model takes in one channel, or wait
   on a count, so that "just now" is the new turn's.
   *Done:* `make-shots.py` writes `shots/<take>.json` (mode 0600).
3. **Record.** `session.sh start` (waits for the relay lock, refuses a
   3030 or a `buzz-harness` that is not its own), the warm-up, the takes
   without a model, then each real-model take **by hand**, one at a time,
   its evidence kept before the next (`after-agent.sh`, `after-records.sh`),
   then the byte scan, then `session.sh stop`, which shows the relay down,
   the port and the lock free, and deletes the profiles.
   *Done:* every take exits 0, every expectation prints `ok`, the scan finds
   nothing and catches its planted sentinel.
4. **Align.** Build each take's map from its marks and give it to every
   window of the take.
   *Done:* a still at the take's "same instant" mark shows each window's own
   state of that instant.

## A real-model take is a validation run

- Grant the agent **no folder**, and keep nothing on its disk that holds
  the answer; compare what it proposed with the bench fixture (identical
  bytes void the take; count the fixture's added lines it shares).
- **Before teardown**, keep: the version's files (`bzz app show` names the
  folder), the brief, the replies (a model-free take that opens the
  threads), `bzz agent show` (activity, last turns), the agent host's
  conversation (its `memory.sqlite` in the profile: every tool call and
  answer), and the relay's own record of who signed each revision (`psql`
  on the harness's `events`, kind 45010).
- Write the result in `docs/validation-v*.md`, pass or not. If the model
  does not do it within the owner's bound, the chapter comes out.

## A window that is not a terminal

A client that is not bzz (Buzz desktop) is a screen recording: `ffmpeg -f
x11grab -framerate 30 -copyts` of its own X display, in the **same take**
as a `record-multi.py` tape (a single pane is fine), plus a clapper written
to both by one `together` step. A frame's tape second is `pts −
originWall`; add the clapper's median latency (27 ms here) so one instant
is one film frame. Label it with the client and version; when it runs a
plugin, say on screen who wrote it and that no product ships it; when its
input was scripted, say "re-enacted". Crop a window (fewer rows) rather
than show a line the chapter cannot vouch for, and say why in the README.
In HyperFrames the `<video>` clip sits in a window with no `data-start` of
its own, outside `#camera` (a clip inside a clip fails `check`).

## Rules a feature film adds

1. **One take for one cause.** Windows that show one cause and its effect
   come from one take, and a map changes all of them at the same tape
   second. A window from another take is a new scene with a visible cut
   and a tag.
2. **The product's speed is shown.** The time between one window and the
   next is never cut or sped; ½ speed only around a shared instant, for
   every window, labelled. Typing may be sped.
3. **A cut over a wait is shown**: a chip with the real time cut
   (`⏩ 1 min 47 s · the model's turn`), from the take's `:wait`/`:seen`
   marks, on screen at least 2.6 s.
4. **A real-model take is a validation run** (above).
5. **Whose screen** is always on screen: avatar, name, client and channel.

## Composition gotchas (HyperFrames 0.9.13)

- `fromTo` renders its from-state before its start: animate a container's
  opacity with `to` pairs, or it is invisible until the tween.
- Push-ins land the frame point at y 600 under a 270 px caption band:
  frame the rows that prove the line below 270, or the band covers them.
  Three 80-column windows side by side draw text at 16 px; push to ≥1.3
  on the windows the line names.
- The renderer's audio comes out ~0.4 LU under the mix. Mux the mix into
  the render (`-c:v copy`), with the mix's true peak at −1.5 dB so the AAC
  stays under −1 dBTP. A 162 s film renders in about 4 minutes.
- `check` fails on the vendored xterm.js lint (`Math.random`, `Date.now`,
  `performance.now`, `requestAnimationFrame`); judge determinism by two
  renders. Type under 28 px is flagged: use 28 px for kickers and tags.
- A `<source media>` is chosen when the page loads: set a phone viewport
  **before** opening the page to see the 720p file chosen.
