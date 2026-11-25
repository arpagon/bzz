# v0.17.0 — task `release`: the upgrade from v0.16.0, the version (F8)

Linux x86_64, 2026-11-25, branch `t3/release` cut from `v0.17.0` at
`60f6a8b` (the merge of `board`). Plan:
[`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
D11, rule 19. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder outside the repository on the machine the work was
done on. No sound server of this machine was asked for anything: the one
that opened the saved devices ran in a container, with no network and a null
sink.

## The upgrade from v0.16.0

**Claim.** v0.17.0 opens a whole v0.16.0 profile, written by v0.16.0's own
code, without a migration and without losing anything; the settings it saves
again are the bytes v0.16.0 wrote; a theme of the user's own that sets only
`[highlight.Border]` loads, with the one change the release notes name; a
key map of the user's own binds as it did; the huddle devices v0.16.0 saved
under their ALSA names open the default devices, with one notice in a run.

**Rung.** The real fixture: `tests/upgrade/seed_v0160.rs` compiled inside a
checkout of the tag `v0.16.0` (`650a648`) and run there, then
`tests/upgrade_from_v0160_test.rs` on a copy of what it wrote.

What the seed writes, with v0.16.0's own functions: an identity in an
encrypted file and a community; the local agent Scout (`manage::create`, the
code of `bzz agent new`; its key sealed to the owner); `ui.theme = "nord"`;
`huddle_input_device = "hw:CARD=PCH,DEV=0"` and `huddle_output_device =
"front:CARD=PCH,DEV=0"`, as v0.16.0's device list gave ALSA's names;
`theme.toml` with only `[highlight.Border] foreground = "#ff8800"`;
`keymap.toml` binding `u` (free in v0.16.0's workspace, bound by v0.17.0,
D5) to `open-inbox`, `!` to `open-help`, `ctrl-g` to `channel-switcher`, and
turning `Space o` off; messages, an applied edit, a mention, a DM, a draft;
the Board template proposed, approved by its hash and open in `#general`
(`Apps::propose`, `approve`, `open_in`, the code of `bzz app add`, `approve`
and `:app open`), a second version proposed by Scout at a person's request
and waiting; two of the Board's cards as the relay's sync stores them, one
with a `null`, an empty object and seventeen digits. It records what v0.16.0
reads back (`expected.json`): the settings' bytes, the theme's colours, what
each key does, the apps, the cards, the history, and the row count of every
table.

```sh
./scripts/upgrade-test.sh                              # BZZ_UPGRADE_FROM_VERSION=0160 is the default
# and, for the devices, with a sound server of a test:
scripts/test-audio-containers.sh serve <tmp>/snd       # PulseAudio, null sink, in a container
BZZ_UPGRADE_AUDIO=1 PULSE_SERVER=unix:<tmp>/snd/native BZZ_UPGRADE_FIXTURE=<scratch>/fixture \
  cargo test --locked --test upgrade_from_v0160_test -- --ignored --nocapture
scripts/test-audio-containers.sh stop <tmp>/snd
```

| Claim | Seen |
|---|---|
| No migration (D11): schema 23 before and after; every table's row count equal; no `bzz.pre-migration-*.bak` made; `PRAGMA integrity_check` ok; `search_integrity` ok | pass |
| The history, the edit, the Inbox, search (`upgradeneedle`, `editedneedle`), the draft are what v0.16.0 showed | pass |
| The local agent is what v0.16.0 saved, field for field; its key, sealed by v0.16.0, opens with the owner's identity | pass |
| The Board: installed, enabled, version 1 runs with the hash v0.16.0 approved (`Apps::runnable` checks the files' bytes), open in `#general`, version 2 from the agent, asked by the person, waiting and its agent not told; its two cards byte for byte | pass |
| Loaded and saved again by v0.17.0, `config.toml` is the bytes v0.16.0 wrote | **red first**, then pass (below) |
| The theme loads without a warning; `Border` and `FocusBorder` are the user's and Nord's as in v0.16.0; an unfocused frame (`PaneBorder`) is now `Muted` (`Rgb(123, 136, 161)` in Nord), where v0.16.0 drew it in the user's `#ff8800`; the focused frame is `FocusBorder` as before | pass |
| The user's key map loads; `u`, `!`, `ctrl-g`, `?`, `Space o` (off) and the Inbox's `u` do what v0.16.0 did, `u` included (`OpenInbox`, not v0.17.0's `MarkUnread`); `!` as Windows sends it (with Shift) is the same key; `d` is v0.17.0's `Delete` where the user bound nothing | pass |
| `bzz check` says nothing is wrong; `bzz app list` shows the Board at version 1; `bzz agent list` shows Scout; none of them changes the settings | pass |
| With a test sound server: the saved ALSA names open the default devices, and the one notice is `saved input "hw:CARD=PCH,DEV=0" not found; using the default; saved output "front:CARD=PCH,DEV=0" not found; using the default`; opened again in the same run, no notice | pass; red when the seed's input is `default` (then only the output is named) |

**Red, and what it found.** The first run failed at the settings:

```
assertion `left == right` failed: saved again by v0.17.0, the settings are the bytes v0.16.0 wrote: v0.16.0 can read them
  left: "…huddle_output_device = \"front:CARD=PCH,DEV=0\"\nmotion = \"full\"\nhuddle_style = \"lanes\"\n\n[media]…"
```

v0.17.0 wrote `motion` and `huddle_style` on every save of the settings
(choosing a theme, resizing a pane, choosing a device). v0.16.0's own
binary, built from the tag, on a copy of the fixture with those two lines:
`bzz: configuration error: …/config.toml contains invalid or unknown
settings`; on the fixture as v0.16.0 wrote it: `configuration, theme, media,
diagnostics, telemetry, and database are valid`. So whoever tried v0.17.0
could not go back to v0.16.0. Fixed in `src/config.rs` (both settings are
written only when not the default); the test then passes. A person who sets
`motion` or a huddle style still writes a line v0.16.0 refuses; the release
notes say so.

**What it cost.** The script builds v0.16.0 from its tag in a worktree of
its own. Cold, into a target folder on the hard disk
(`BZZ_UPGRADE_TARGET_DIR`, `CARGO_BUILD_JOBS=4`, other agents building at
once): 4 min 22 s and 2.7 GB for v0.16.0 and the seed, about 1 min to write
the fixture; with that folder warm, the whole script took 4 min 54 s (v0.16.0
is compiled again for the new worktree's path, then the test, 5 s). The
default, a target folder inside the temporary folder, is removed after.

## The last release before: v0.15.0

The CI job keeps the one before the last release on Linux, now v0.15.0
(`BZZ_UPGRADE_FROM_VERSION=0150 ./scripts/upgrade-test.sh`). On this
branch, with the settings change above in it: `test
v0150_profile_upgrades_with_its_agents_ledger_and_artifacts_and_no_app ...
ok` (migrations 21 to 23, the ledger, no reply sent twice). Cold, v0.15.0
built into a target folder of its own on the hard disk: 5 min 4 s in all.

## Not verified

- The upgrade on macOS and Windows: the CI job runs it there
  (`macos-15-intel`, `windows-latest`) when the branch is pushed.
- The devices part on PipeWire (the test server here is PulseAudio) and on
  a machine with a real card: on a real card, a saved `hw:CARD=…` name of a
  card that is there opens that card's device, not the default
  (`huddle::pulse_devices` unit tests); not seen on hardware.
- The TUI drawn on the upgraded profile: the test reads what the TUI reads
  (store, apps, theme, key map), and `bzz check`; it does not start the
  interface.
