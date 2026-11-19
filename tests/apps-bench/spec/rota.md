# App spec: Rota

Who covers each day of one week. Every participant sees who holds each day,
takes a free day, gives up their own, and sees other participants' changes
without reloading. The bench (`run --app rota`) checks only what this
page fixes; anything not fixed here (colours, borders, spacing, extra hints)
is free.

## Start, data and exit

- The app is one script written against `docs/app-api.md`, `api = 1`. The
  bench runs it on `examples/app_host.rs`, which lends it the terminal and
  turns the data service of `data-protocol.md` into the contract's records.
- A day is a record of type `day` with content
  `{"day": string, "holder": string | null, "order": number}`; `null` means
  free, and a holder is a participant's name. The local participant's is
  `ctx.user.name` (for example `ana`). Day labels are at most 8 characters
  (`Mon`...`Sun` in a normal week).
- Load with one `data.list` with `type = "day"`; show one row per record, in
  the order of their `order` field (a list arrives in no order). Other
  participants' changes arrive as pushes; never poll.
- `q` (when no confirmation is open) closes the app with `host.quit()`.
  `Ctrl+C` never reaches the script: the host leaves on it. Either way,
  even with requests still pending, the bench expects of the host: exit
  within 2 s with status 0, no child process left, and the terminal
  restored (echo and line mode back, cursor visible, bracketed paste, mouse
  and focus reporting off, alternate screen left).

## Screen

All text below is exact (ASCII except user data, case-sensitive).

- **Filter indicator**: `Show: all`, `Show: free` or `Show: mine`.
- **Day rows**, in this order separated by spaces: the selection marker (`>`
  if selected, else a space), the day label, then one or more spaces and the
  holder's name, or `free`. Example: `> Tue  free`, `  Wed  cy`.
- **Day count**: `Days: ` then, for every participant holding at least one
  day, `name N`, in name order, separated by `, `. Example:
  `Days: ana 1, bo 2, cy 1`. With no day held: `Days: none`. It always counts
  every day, whatever the filter, and follows every change at once.
- If the filter shows no day: `No days match.`.
- If there are more days than fit, show a contiguous run that includes the
  selected day.
- **Status line**: the last row of the screen. Exact texts:

| When | Status line contains |
| --- | --- |
| A load is pending | `Loading...` |
| A load succeeded | `Loaded N days.` (N = records returned) |
| A load failed | `Load failed. Press r to retry.` |
| A write is pending | `Saving...` |
| You took a day | `Took <day>.` |
| You gave up a day | `Gave up <day>.` |
| `t` on a day someone else holds | `Held by <name>.` |
| `g` on a day you do not hold | `Not your day.` |
| Another participant took the day first | `<day> was taken by <name>.` |
| A write failed (not a conflict) | `Save failed.` |

- Nothing may be written outside the screen: no line wider than the terminal
  (no autowrap) and no scrolling of text off the top.
- **Narrow screen**: the rota is fully usable at 40x16 and at any larger
  size. At 40x16 with seven days, the filter indicator, every day row, the
  day count and the status line are visible, and every key works. Never show
  a "terminal too small" screen at 40x16 or larger. Every size change is
  followed by a redraw at the new size, with the same selection, filter and
  data.

## Selection and keys

Initial selection: the first day.

| Key | Action |
| --- | --- |
| `Up`/`k`, `Down`/`j` | Previous / next shown day (stops at the ends) |
| `t` | Take the selected day |
| `g` | Give up the selected day (asks for confirmation) |
| `f` | Cycle the filter: all, free, mine, all... |
| `r` | Reload (`data.list` again) |
| `q` | Quit |
| `Enter`, other keys | Nothing |

**Taking.** `t` on a free day sends one `data.update` of that day with its
current `rev` and its whole content with `"holder": <ctx.user.name>` (its
`order` and any other field are kept), and shows `Saving...`. On success the row shows your name, the count follows and the
status is `Took <day>.`. `t` on a day you hold does nothing. `t` on a day
someone else holds sends nothing and shows `Held by <name>.`.

**Giving up.** `g` on a day you hold opens a confirmation showing
`Give up <day>?` and the hint `Enter confirm  Esc cancel`. While it is open,
only `Enter`, `Esc` and `Ctrl+C` act (`q`, `j`, `t` and pastes do nothing).
`Enter` sends one `data.update` with `"holder": null` (`data.null`) and
shows `Saving...`; on
success the confirmation closes, the row shows `free` and the status is
`Gave up <day>.`. `Esc` closes it, sends nothing and keeps the selection. `g`
on a day you do not hold opens nothing and shows `Not your day.`.

**One confirmation, one write.** While a write is pending (`Saving...`), `t`,
`g` and `Enter` are ignored.

## Filter

`f` cycles `Show: all` -> `Show: free` (days with no holder) -> `Show: mine`
(days you hold) -> `Show: all`, at once, without contacting the service.
Selection with a filter: if the selected day is still shown it stays
selected; otherwise the first shown day is selected. Going back to `all`
keeps the selected day selected. Remote changes respect the filter (a free
day someone takes leaves `Show: free`).

## Paste

Bracketed paste is one piece of text, never keys. The rota has no text input:
a paste is ignored everywhere.

## Loading, errors and other participants

- **Slow load**: while the first load is pending no day rows are shown and the
  status is `Loading...`; `q` and `Ctrl+C` still quit.
- **Load error**: show `Load failed. Press r to retry.`, keep everything shown
  before, and keep the selection. `r` retries.
- **Late load**: a reply to `data.list` can arrive after a change that is
  newer than it (your own write's reply, or a push). The contract has no
  counter to compare, so remember which records changed since the list was
  asked for, and keep those as you have them when its reply arrives: a
  slow reload must never undo a newer change.
- **Remote change** (push): apply at once, without a reload. Rows and the day
  count follow. The selected day stays selected while it is shown.
- **Two take the same day** (reply `conflict` to your `update`): take
  `current` from the reply. If its holder is another participant, that
  participant won: send nothing more, show `current` (their name on the row)
  and the status `<day> was taken by <name>.`. Never merge or overwrite. If
  `current` is still free, send the `data.update` again with `current.rev`. When
  giving up, if `current` is still yours send again with `current.rev`,
  otherwise show `current` and `Save failed.`.
