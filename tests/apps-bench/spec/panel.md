# App spec: Panel

A shared panel of metrics: a sortable table, a tab with only the metrics in
alert, a sparkline of the selected metric's recent values, a gauge of its
latest value, and an acknowledge action. Every participant sees new values
and other participants' acknowledgements without reloading. The bench
(`run --app panel`) reads text only: colours, bold and reverse video
are never read. Anything not fixed here (borders, spacing, extra hints) is
free.

## Start, data and exit

- The app is one script written against `docs/app-api.md`, `api = 1`. The
  bench runs it on `examples/app_host.rs`, which lends it the terminal and
  turns the data service of `data-protocol.md` into the contract's records.
- A metric is a record of type `metric` with content `{"name": string,
  "unit": string, "status": "ok" | "alert" | "acked", "max": integer,
  "history": [integer, ...], "acked_by": string | null, "note": string |
  null}`. Names have at most 10 characters, units at most 4. `history` holds
  1 to 20 non-negative integers, oldest first; the last one is the **value**.
  `acked_by` is a participant's name; the local participant's is
  `ctx.user.name` (for example `ana`). The table's order is the user's
  choice of sort, so a metric carries no `order`.
- Load with one `data.list` with `type = "metric"`. Other participants'
  changes arrive as pushes; never poll.
- `q` (when no form is open) closes the app with `host.quit()`.
  `Ctrl+C` never reaches the script: the host leaves on it. Either way,
  even with requests still pending, the bench expects of the host: exit
  within 2 s with status 0, no child process left, and the terminal
  restored (echo and line mode back, cursor visible, bracketed paste, mouse
  and focus reporting off, alternate screen left).

## Screen

All text is exact (ASCII except user data and the glyphs named below).

- **Tabs**: one row with `All (N)` and `Alerts (M)`: N counts every metric,
  M those with status `alert`. The active tab's title is preceded directly by
  `*` (for example `*All (5)`); the other has no `*`.
- **Sort indicator**: `Sort: <Name|Value|Status> <asc|desc>`.
- **Table**: a header row with `Name`, `Value`, `Unit`, `Status` in this
  order, then one row per metric of the active tab: the name, the value
  (decimal), the unit and the status word, in this order, separated by one or
  more spaces. The selected row has `>` before the name (spaces may separate
  them); other rows have no `>`. Example: `>CPU   93   %   alert`.
- If the active tab has no metric: `No metrics match.`. If there are more
  rows than fit, show a contiguous run that includes the selected row.
- **Sparkline** of the selected metric: a row containing
  `History: <name>`, directly followed by one or more rows that draw the
  history with the glyphs `▁▂▃▄▅▆▇█` (U+2581..U+2588, one to eight eighths
  of a cell) and spaces, followed by a row with none of those glyphs. One
  column per history value, oldest on the left, side by side; a column's
  height is the sum of its cells' eighths. A higher value is never drawn
  lower than a smaller one; the largest value is drawn with the tallest
  column, taller than the smallest when the smallest is at most 3/4 of it;
  every value of at least 1/8 of the largest is visible. (Ratatui's
  `Sparkline` with no `max` meets this; so does a hand-drawn row.)
- **Gauge** of the selected metric's value against its `max`: one row with
  the label `P%`, P = value * 100 / max rounded to the nearest integer,
  and the filled part drawn with `█` (U+2588) from the left, longer for a
  larger P (a last partial cell `▏`..`▉` is allowed). No other row shows a
  number directly followed by `%`. (Ratatui's `Gauge` with its default label
  meets this.)
- **Status line**: the last row of the screen. Exact texts:

| When | Status line contains |
| --- | --- |
| A load is pending | `Loading...` |
| A load succeeded | `Loaded N metrics.` (N = records returned) |
| A load failed | `Load failed. Press r to retry.` |
| A write is pending | `Saving...` |
| You acknowledged | `Acknowledged.` |
| `a` on a metric not in alert | `Nothing to acknowledge.` |
| Someone else acknowledged it first | `Already acknowledged by <name>.` |
| A conflict was merged and saved | `Conflict: merged with a newer change.` |
| A write failed (not a conflict) | `Save failed.` |

- U+2581..U+2588 appear only in the sparkline and the gauge. Nothing may be
  written outside the screen: no line wider than the terminal (no autowrap)
  and no scrolling of text off the top.
- **Narrow screen**: fully usable at 40x16 and at any larger size: the tabs,
  the sort indicator, the header and at least five table rows (or all), the
  sparkline (up to 20 values), the gauge and the status line are visible,
  and every key works. Never show a "terminal too small" screen at 40x16 or
  larger. Every size change is followed by a redraw at the new size, with the
  same tab, sort, selection and data.

## Selection, sort and keys

Initial state: tab All, `Sort: Name asc`, the first row selected.

| Key | Action |
| --- | --- |
| `Up`/`k`, `Down`/`j` | Previous / next row (stops at the ends) |
| `Tab` | Switch between the All and Alerts tabs |
| `s` | Next sort column (Name, Value, Status, Name...), ascending |
| `d` | Reverse the sort direction |
| `a` | Acknowledge the selected metric (opens the note form) |
| `r` | Reload (`data.list` again) |
| `q` | Quit |

Sort by Name compares names, Value the values as numbers, Status the status
words (`acked` < `alert` < `ok`); ties go by name ascending in both
directions. Sorting and switching tabs never contact the service and keep the
selected metric selected if it is shown; otherwise the first row is
selected. Going back to All keeps the selected metric selected. The
sparkline and gauge always show the selected metric.

## Acknowledge form

- `a` on a metric whose status is not `alert` opens nothing, sends nothing and
  shows `Nothing to acknowledge.`.
- Otherwise it shows the heading `Acknowledge <name>`, the field `Note: `
  followed by the typed text, and the hint `Enter save  Esc cancel`. While it
  is open, keys go to the field (`q`, `j`, `s` are text); `Backspace` deletes
  one character; any Unicode text can be typed and is shown as typed.
- `Enter` with an empty or blank note shows `Note is required.` and sends
  nothing. Otherwise it sends one `data.update` of the metric with its
  current `rev` and its whole content with `"status": "acked"`, `"acked_by":
  <ctx.user.name>`, `"note": <trimmed note>`, and shows `Saving...`. Until the
  reply arrives the form stays open and further `Enter` presses are ignored.
  On success the form closes, the row shows `acked`, the counts follow and
  the status is `Acknowledged.`.
- `Esc` closes the form, sends nothing and keeps the selection.

## Paste

Bracketed paste is one piece of text, never keys. In the note field it is
inserted with each line break (CR, LF or CRLF) replaced by one space. With no
form open, a paste is ignored.

## Loading, errors and other participants

- **Slow load**: while the first load is pending no table rows are shown and
  the status is `Loading...`; `q` and `Ctrl+C` still quit.
- **Load error**: show `Load failed. Press r to retry.`, keep everything shown
  before, and keep the selection. `r` retries.
- **Late load**: a reply to `data.list` can arrive after a change that is
  newer than it (your own write's reply, or a push). The contract has no
  counter to compare, so remember which records changed since the list was
  asked for, and keep those as you have them when its reply arrives: a
  slow reload must never undo a newer change.
- **Remote change** (push): apply at once, without a reload. A new value
  (a longer `history`) changes the row, the sparkline and the gauge; status
  changes move the counts and the Alerts tab; created metrics appear and
  deleted ones disappear. The selected metric stays selected while shown.
- **Write conflict** (reply `conflict` to an acknowledge): take `current`. If
  its status is `acked`, send nothing more: show `current` and
  `Already acknowledged by <acked_by>.`. Otherwise apply your acknowledgement
  (`status`, `acked_by`, `note`) on top of `current.content`, keeping the
  other participant's `history`, and send again with `current.rev`. On
  success show `Conflict: merged with a newer change.`.
