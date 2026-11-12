# Capture: fixture, tapes and shots

All commands run from a work directory (`PROMO_WORK`, default `$PWD`), usually
`runs/experiments/promo-video/capture`, holding:

- `buzz-pin/`: `git worktree add --detach` of block/buzz at the pinned commit;
- `bin/bzz-x86_64-unknown-linux-gnu/bzz`: the **published** release archive,
  checksum-verified against the release's `sha256.sum`;
- `fixture/`, `run/`, `tapes/`: created by the kit, owner-only.

Build `examples/promo_fixture.rs` first:
`cargo build --locked --release --example promo_fixture`.

## Fixture

`capture/setup-fixture.sh` tears down the `buzz-harness` project, starts a fresh
relay (saving its start command, with the signing key, to `run/relay.cmd`), then:

1. `promo_fixture seed`: Ada, Lin, Sam and You; `#team` owned by You, `#design`,
   `#infra`, `#launch`, a DM; the viewer profile (encrypted-file identity,
   `[media] protocol = "sixel"`).
2. `bzz broker create scout` and `allow`; `promo_fixture bot` gives Scout the
   bot role in `#team`.
3. `promo_fixture history` (after a pause, so system rows sit above it):
   backdated messages, a thread, reactions, an inline image (`site/public/assets/og.png`
   via the relay's Blossom endpoint), a custom emoji, a DM message, a task.
4. A warm-up run in which Scout names itself through the broker.

## Shots

`capture/shoot.sh <name> [scene delay args…]` records `tapes/<name>.jsonl` from
`capture/shots/<name>.json` (steps: `{"wait"}`, `{"type","step"}`, `{"mark"}`)
at 120×32 cells of 12×26 px. A scene runs `promo_fixture drive` in parallel:

| Scene | What happens during the take |
|---|---|
| `live` | Lin and Sam post into `#team` |
| `agent-reply` | waits for the viewer's `@Scout` mention, then replies in the thread through the broker |
| `huddle <s>` | Ada starts a huddle; Ada and Sam join with live audio (Ada sends a tone) |

`capture/shoot-all.sh` runs setup and every v0.13 shot in film order and then
`tape-expect.py` checks. Offline: the shot kills the relay mid-take and
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

Known product states seen in takes (honest, but frame around them):
"restricted: relay denied the subscription" before joining a huddle (bzz
subscribes to the backing channel before membership); a `#huddle-…` backing
channel appears in the sidebar after joining, so record the huddle last;
offline search first shows "searching the relay…" then "offline · local results".

## Review

- `scripts/tape-peek.py tape.jsonl 7.5 12` prints the screen as text.
- `FILM=<film project> scripts/preview-tape.sh tape.jsonl 7.5,12 out/` makes
  pixel stills through the same xterm.js replay the film uses.
- A tape's last line `{"stop": true}` records when the recording ended.
