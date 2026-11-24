# The takes of the v0.16 film, as they were run

These scripts recorded `assets/tapes/{mention,hands,app}.jsonl` and the six
model-free tapes on 2026-11-24, in the ignored experiment
`runs/experiments/promo-video/film-v016/final/` (with `six/` there for the
kit's shots). They are kept as a record: their paths are that
experiment's and this machine's.
The recording machine's own paths are variables in `env.sh`
(`BZZ_CHECKOUT`, `FILM_HEAVY`, `FILM_FIXTURE`, `PI_DIR_OF_OWNER`).

- `env.sh`: the paths, and the disposable profile under `/tmp/bzz-film16`
  with a home of its own. The published `bzz 0.16.0` archive
  (`bzz-x86_64-unknown-linux-gnu.tar.xz`, checked against the release's
  `sha256.sum`); `promo_fixture` built from `git archive` of the tag. Pi's
  folder is reached only through a link, `$R/pi`, passed to bzz as
  `PI_CODING_AGENT_DIR`; nothing reads, copies or prints what is in it.
- `session.sh start|stop`: takes the relay harness lock and holds it for
  the whole session, checks that port 3030 and `buzz-harness` are free,
  runs `setup.sh` and the warm-up; `stop` tears the relay down, deletes the
  profile (keys, passphrase) and shows the relay, the port and the lock
  free.
- `setup.sh`: a fresh pinned relay, the generated community, Scout made
  with `bzz agent new Scout --harness pi-durable --model
  openai-codex/gpt-6.1-sol --no-packages` and added to `#team`, the app
  `tasks` from the Board template, approved at the shell. **No folder is
  granted, and nothing under the profile holds a Board with priority.**
- `make-shots.py`: the timelines, in story order: `warmup` (the three cards;
  no model turn), `mention` (line 6), `hands` (line 7), `app` (N1 and N2),
  `replies` (the app mention's thread, to keep the agent's reply; no turn).
  Between `mention` and `hands`, `bzz agent edit Scout --folder
  site=/tmp/bzz-film16/site:rw:exec` granted the generated project of line
  7 (a `test.sh` that prints three checks); between `hands` and `app`,
  `--no-folders` took it away again.
- `record-live.py`: the kit's, plus one option, `"soft": true` on a wait:
  if the text does not show in time, it marks `<name>:timeout` and goes on
  instead of killing bzz in the middle of a model's turn. No soft wait
  timed out in these takes.
- `expect.sh`: what each agent tape must show (15 expectations, all `ok`).
- `scan.py`: searches every tape's bytes for this machine and its owner
  (names, paths, the Modal workspace), the fixture's secret keys and
  passphrase, and the shapes of keys, tokens, e-mail addresses and
  sign-in files.
- `run-six.sh` and `kit-paths.diff`: the six shots without a model, by the
  kit's `shoot-all.sh`, copied with its paths pointed at the tag's fixture,
  its source and the published binary, and with the main checkout's
  `runs/env.sh` read so that the pinned relay is not rebuilt.
- `turns.md`: every model turn spent (three; times UTC).
- `answer.py`, `bzz.sh`, `shoot.sh`, `null-asound.conf`: helpers.
