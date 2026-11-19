# Mini-app acceptance bench

Judges a terminal mini-app by driving it like a user in a pseudo-terminal.
Linux, Python 3.10+ standard library only; no tmux, Herdr or visible terminal.

| App (`--app`) | Spec | Cases |
| --- | --- | --- |
| `board` | `spec/board.md` | 01-12 |
| `poll` | `spec/poll.md` | 01-12 |
| `rota` | `spec/rota.md` | 01-14 |
| `approvals` | `spec/approvals.md` | 01-15 |
| `panel` | `spec/panel.md` (sparkline, gauge, table, tabs) | 01-15 |
| `board-priority` | `spec/board.md` + change request `spec/board-priority.md` | Board's 01-12, then 13-17 |

Data protocol for all: `spec/data-protocol.md`.

## Run

```sh
bench/run --app board --option A -- node e2/a-ink/board.mjs
bench/run --app panel --option f1-s-1 --case 3,14 -- ./target/debug/host panel.toml
```

- Everything after `--` is the command (no shell; use `sh -c '...'` if needed).
  It runs in your current directory.
- Each case starts a fresh data service and a fresh program, at 80x24 unless
  the case says otherwise, with `MINIAPP_DATA_ADDR`, `MINIAPP_USER=ana`,
  `TERM=xterm-256color` and `LANG=C.UTF-8`.
- `--case 3,5` runs a subset. `--artifacts DIR` keeps the per-case files
  there (default: a new folder under `/tmp/miniapp-bench/`).
- Exit status 0 only if every case passed. Every invocation appends one line to
  `bench/runs.jsonl`: UTC `time`, `app`, `option`, `passed`, `failed`,
  `failed_cases`, `cases_run`, `command`, `cwd`.

## Cases

Same numbers and categories for every app; 13 and up are each app's own rules.

| # | Category | Board / Poll (round one) | Rota | Approvals | Panel |
| --- | --- | --- | --- | --- | --- |
| 01 | First draw | 80x24, 120x40 | 40x16, 80x24, 120x40: seed, `Loaded N ...`, no overflow, still running | same | same, plus sparkline and gauge properties |
| 02 | Form cancel | `n`/`a`, `Esc` | `g` confirmation: `q`, `j` do nothing, `Esc` gives nothing up | `n` form, `Esc` | `a` note form, `Esc` |
| 03 | Save once | slow writes, `Enter` twice | `t` twice, confirm `Enter` twice: one update each | empty title refused, one create | empty note refused, one update |
| 04 | Filter or search | `/` search | `f`: free, mine, back to all keeps selection | `f` through four states | `Tab` to Alerts and back |
| 05 | Late reply | stale search / reload | slow reload must not undo a take | ... an approval | ... an acknowledgement |
| 06 | Narrow size | 40x12 | resize to 40x16, act there, grow back | same, with detail and reject form | same, with sort and tabs |
| 07 | Paste | ignored / one line | ignored on list and confirmation | ignored on list, one line in form | same |
| 08 | Load error | retry, keep data | same | same | same |
| 09 | Quit while pending | `q` / `Ctrl+C` | `q` during a 10 s write | `Ctrl+C` during slow first load | `q` during a 10 s reload |
| 10 | Repeatable | three identical screens | same | same | same |
| 11 | Remote change | pushes, no reload | take and give up by others, counts follow | create, decide, delete | new value: row, sparkline, gauge grow; status, create, delete |
| 12 | Conflict | merge | lost race: `Tue was taken by bo.`, no overwrite | `Already rejected by bo.`, no overwrite | merge, keep bo's new value |
| 13 | | | `t` on a held day, `g` on another's day | own request cannot be decided | sort: `s` cycles, `d` reverses, selection kept |
| 14 | | | day count follows own writes; `Show: mine` | reject needs a reason; detail shows it | sparkline and gauge follow the selection |
| 15 | | | | decided request cannot be decided again | `Nothing to acknowledge.`; `Already acknowledged by bo.` |

`board-priority` 13-17: set and clear a priority (`2`, `0`); `p` cycles
all/P1/P2/P3 with counts; priority filter and search combine; remote changes
respect the filter; a move keeps the priority.

## Reading a failure

```
FAIL rota 12 conflict (3.1s): expected rota rows '  Mon  bo', '> Tue  bo', ... and status 'Tue was taken by bo.' ...
     screen seen:
     +----...----+ 80x24
     |> Tue      ana      ...|  2      <- row number on the right
     artifacts: /tmp/miniapp-bench/rota-A-.../case-12
```

`expected` is the state the bench waited for (exact texts from the spec);
the box is the emulated screen at the moment it gave up. In the artifacts
folder: `screen-N.txt` (last screen of launch N), `output-N.raw` (bytes the
program wrote) and `service-N.jsonl` (every message to and from the data
service, with `dir` = in, out, push or control).

How the bench reads the screen:

- Text only; colours and styles are ignored. Character widths follow Unicode
  East Asian Width (wide and fullwidth = 2 cells; private-use = 1).
- Board: the header row is the one with `To do (N)`, `Doing (N)`, `Done (N)`;
  each known title below it belongs to the column whose header starts nearest
  to its `> `/two-space marker; `board-priority` reads ` P1`..` P3` right after
  a title. Poll: rows match `[> ] [x| ] Label  N votes`. Rota: `[>] Day  holder|free`
  and `Days: ...`. Approvals: `[>] state  Title`. Panel: `[>] Name  value  unit
  status`, `*All (N)`/`Alerts (M)`, `Sort: Col dir` (`lib/cases_*.py`).
- Panel sparkline and gauge (`lib/panel_props.py`): under the row with
  `History: <name>`, the rows with `▁▂▃▄▅▆▇█` give one column per value; a
  column's height is the sum of its eighths; checks are order properties, not
  pictures. The gauge is the row with `N%` and `█` cells. Both were written
  against what stock Ratatui 0.30 `Sparkline` and `Gauge` print (`f2/probe/`).
- **Overflow** = a character autowrapped past the last column, or a non-blank
  line scrolled off the top. A full-screen erase (`ESC[2J`, or `ESC[J` from
  the top-left) forgives overflow written before it.
- The emulator answers cursor-position (`ESC[6n`) and device-attribute
  (`ESC[c`) queries; it does not answer kitty-keyboard or other queries.
- Keys: arrows as `ESC[A` (or `ESC OA` if the program enabled application
  cursor keys), `Enter` = CR, `Esc` = a lone ESC followed by a pause, `Tab` =
  HT, `Backspace` = DEL, typed text one character every 30 ms. Paste is wrapped
  in `ESC[200~ … ESC[201~` only if the program enabled mode 2004.
- Waits: 10 s for the first load, 3 s for most states, 2 s for remote changes
  and for exit after quit. Exit checks: process tree by session id, termios
  echo/line mode, alternate screen, cursor, bracketed paste, mouse, focus.

## Measure

```sh
bench/measure --app rota --option A -- <command>
```

Uses the `big` data set (200 records) at 80x24. Prints and appends to
`bench/measures.jsonl`: time from start to first output and to the first
complete draw (`Loaded 200 cards.` / `options.` / `days.` / `requests.` /
`metrics.` on the status line), peak resident memory of the process tree (sum
of `VmRSS` over the session, sampled every 10 ms), and key-to-screen-change
p50/p95/max over 120 presses of `j`/`k`.

## Data service by hand

`bench/data-service --app panel --scenario slow-load` prints its addresses;
see `spec/data-protocol.md` for scenarios and the control port.

## Self-test (bench developers only)

`bench/reference/` holds plain-Python references of all six apps that exist
only to validate the bench, with deliberate faults (`--fault NAME`; list in
each app file, round-one faults in `common.py`). The Panel reference draws its
sparkline and gauge by hand, without a widget library.
`bench/reference/selftest [--app A,B] [--jobs N]` runs each clean reference
three times and every fault once, and checks that each fault fails exactly its
case(s). Last full output: `reference/selftest-last.txt`.
