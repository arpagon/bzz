# The agent take of the v0.15 film, as it was run

These scripts recorded `assets/tapes/{create,hands,move,there}.jsonl` on
2026-11-15, in the ignored experiment `runs/experiments/v015-proof/proof-reel`
(`capture/` there, with `scripts/record-live.py` and `tape-expect.py` of the
[`bzz-promo-video`](../../../.agents/skills/bzz-promo-video/SKILL.md) skill
beside it as `scripts/record-live.py` and `scripts/expect`). They are kept
as a record: their paths are that experiment's.

- `setup.sh`: a fresh pinned relay, the generated community, two profiles of
  one generated owner (`/tmp/bzz-demo/here`, `/tmp/bzz-demo/there`), a small
  generated project to grant.
- `make-shots.py`: every timeline of the recorded run. The film uses
  `02-create`, `06-hands`, `11-move` and `13-there`; `03-restart`, `05-grant`
  and `12-receive` are the story between them.
- `shoot-film.sh`: those shots in order, then what each tape must show.
- `shoot.sh`, `env.sh`: one shot, and the profile's variables.

A real model answered (`openai-codex/gpt-6-luna` through Pi's own sign-in,
six turns). `/tmp/bzz-demo` was deleted after the take.
