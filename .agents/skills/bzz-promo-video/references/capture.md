# Capture: fixture, tapes and shots

All commands run from a work directory (`PROMO_WORK`, default `$PWD`), usually
`runs/experiments/promo-video/capture`, holding:

- `buzz-pin/`: `git worktree add --detach` of block/buzz at the pinned commit;
- `bin/bzz-x86_64-unknown-linux-gnu/bzz`: the **published** release archive,
  checksum-verified against the release's `sha256.sum`;
- `fixture/`, `run/`, `tapes/`: created by the kit, owner-only.

Build `examples/promo_fixture.rs` from the release tag, into a target of
your own (never the checkout's `target/`), and name it `PROMO_FIXTURE`:
`git archive refs/tags/<tag> | tar -x -C <src>`, then in `<src>`
`CARGO_TARGET_DIR=<yours> cargo build --locked --release --example promo_fixture`.

## Fixture

`capture/setup-fixture.sh` tears down the `buzz-harness` project, starts a fresh
relay (saving its start command, with the signing key, to `run/relay.cmd`), then:

1. `promo_fixture seed`: Ada, Lin, Sam and You, each with an avatar; `#team` owned by You, `#design`,
   `#infra`, `#launch`, a DM; the viewer profile (encrypted-file identity,
   `[media] protocol = "sixel"`).
2. `bzz broker create scout` and `allow`; `promo_fixture bot` gives Scout the
   bot role in `#team`.
3. `promo_fixture history` (after a pause, so system rows sit above it):
   backdated messages, a thread, reactions, an inline image (`site/public/assets/og.png`
   via the relay's Blossom endpoint), a custom emoji, a DM message, a task.
4. A warm-up run in which Scout sets its name and avatar through the broker.

### Avatars

Without kind-0 `picture`s bzz shows text badges (`[■A]`), which look worse than
a real community. `assets/avatars/{ada,lin,sam,you,scout}.png` (256×256) are
fictional illustrated characters made with the `codex-gpt-image-2` skill: one
shared style prompt ("square profile avatar, flat vector illustration with soft
shading, readable at 48 px, plain solid background, clearly fictional, no
text") plus a description and a distinct background colour each. The fixture
uploads them to the relay's Blossom store, so the picture is a same-origin
`/media/<sha256>.png` URL that bzz's relay-avatar path fetches with a scoped
read (`ui.profile_avatars = "trusted"`, graphics terminal). Never use photos of
real people. The mention picker and composer show text badges; that is bzz.

## Shots

`capture/shoot.sh <name> [scene delay args…]` records `tapes/<name>.jsonl` from
`capture/shots/<name>.json` (steps: `{"wait"}`, `{"type","step"}`, `{"mark"}`)
at 120×32 cells of 12×26 px. A scene runs `promo_fixture drive` in parallel:

| Scene | What happens during the take |
|---|---|
| `live` | Lin and Sam post into `#team` |
| `agent-reply` | waits for the viewer's `@Scout` mention, then replies in the thread through the broker |
| `huddle <s>` | Ada starts a huddle; Ada and Sam join with live audio (Ada sends a tone) |

`capture/shoot-all.sh` runs setup and every shot and then `tape-expect.py`
checks. Shots that change state run after the shots that show it as seeded
(v0.14: `edit` and `sync` before `huddle`). Offline: the shot kills the relay mid-take and
`restart-relay.sh` brings it back **with the same signing key**; a new key
breaks the pinned relay identity.

Keys learned the hard way:

- Open a channel with the switcher (`Space Space`, type, Enter); list order
  changes with smart sorting.
- Reaction picker: `r`, `j`/`k` to move, Enter sends and closes. A second take
  toggles the reaction off.
- Mention picker: Tab or Enter accepts. Huddle join asks for `y`; `m` unmutes.
- Theme picker: `Space o`, `j` previews live, Esc restores.
- Esc at top level asks to quit; leave ≥ 0.3 s between Esc and the next key.
  Sending a message (Enter) already closes the composer: no Esc after it.
- Edit: `G` selects your newest message, `e` opens it, Backspace is `\u007f`.
- Sidebar commands (`:section …`, `:solo …`) show in bzz's own command box;
  no key chip is needed.

Known product states seen in takes (honest, but frame around them):
"restricted: relay denied the subscription" before joining a huddle (bzz
subscribes to the backing channel before membership); a `#huddle-…` backing
channel appears in the sidebar after joining, so record the huddle after
every sidebar shot;
offline search first shows "searching the relay…" then "offline · local results".

## Agent shots with a real model (v0.15)

A local agent (`bzz agent`) answers through a real model, so its shots cannot
follow a fixed timeline. `scripts/record-live.py` records the same tape
format with steps that wait: `{"until": "text on screen", "timeout": 120}`
(also `"gone"`, a list of texts, and `"count"`), each written as two marks,
`<name>:wait` and `<name>:seen`. Cut or shorten exactly those spans in the
film's `map`, and build the map from the marks
(`promo/v0.15-launch/scripts/agent-maps.py`), not from seconds read by eye.

`promo/v0.15-launch/capture/` is the take of the v0.15 film as it was run:
`setup.sh` (a fresh relay, the generated community, two bzz profiles of one
owner under `/tmp/bzz-demo`), `make-shots.py` (the timelines) and
`shoot-film.sh` (the shots in story order, then what each tape must show).
Its paths are those of the experiment it ran in; move it to an ignored
experiment before running it.

- **It spends the owner's model usage.** Ask first, name the model, use the
  cheapest one that answers well, and keep prompts short. The v0.15 take is
  six turns.
- **No path of the machine in a tape.** bzz prints where it found Node.js.
  Give it a neutral one: a folder with `bin/node`, `bin/npm` and `lib` linked
  to the real installation, and `BZZ_NODE=<folder>/bin/node`. Then search
  the tapes' bytes for the user's name and home folder, as well as for every
  passphrase and generated key. A tape may be ended early (drop its last
  events and write a new `stop` line); it may not lose events in the middle.
- Passphrases are typed where bzz asks, without echo: write `@PASS@` in a
  shot and let `shoot.sh` fill it in an owner-only copy of the timeline.
- A shell shot (`SHELL_SHOT=1`, 100×24) runs `bash --noprofile --norc` with
  the profile's variables set; `mountTape({align: "center"})` fits it.
- Delete the profiles when the take is done: they hold the generated keys.
- **Sound, from bzz 0.17.** On Linux bzz speaks to the session's sound server
  (PipeWire or PulseAudio), so a huddle in a take would open this machine's
  real microphone. `shoot.sh` sets `PULSE_SERVER` to a socket that does not
  exist, and joining then says there is no sound server. For a take with
  live audio, start a disposable server with
  `scripts/test-audio-containers.sh serve <folder>` and pass what it prints
  as `BZZ_CAPTURE_PULSE_SERVER`. Never record a huddle without one of the two.

## Review

- `scripts/tape-peek.py tape.jsonl 7.5 12` prints the screen as text.
- `FILM=<film project> scripts/preview-tape.sh tape.jsonl 7.5,12 out/` makes
  pixel stills through the same xterm.js replay the film uses.
- A tape's last line `{"stop": true}` records when the recording ended.
