# App spec: Poll

One question, a list of options, one vote per participant that can be changed,
and live tallies. Every participant sees other participants' votes and new
options without reloading. The bench (`run --app poll`) checks only what
this page fixes; anything not fixed here (colours, borders, spacing, extra
hints) is free.

## Start, data and exit

- The app is one script written against `docs/app-api.md`, `api = 1`. The
  bench runs it on `examples/app_host.rs`, which lends it the terminal and
  turns the data service of `data-protocol.md` into the contract's records.
- Records:
  - exactly one of type `poll`: `{"question": string, "options": [string, ...]}`;
  - any number of type `vote`: `{"voter": string, "option": string}`. A
    participant's vote is the `vote` record whose `voter` equals their name,
    `ctx.user.name` (for example `ana`).
- Load with one `data.list` for each of the two types (a list names one
  type); nothing of a load is shown until both have answered. Other
  participants' changes arrive as pushes; never poll.
- `q` (when no form or search input is open) closes the app with `host.quit()`.
  `Ctrl+C` never reaches the script: the host leaves on it. Either way,
  even with requests still pending, the bench expects of the host: exit
  within 2 s with status 0, no child process left, and the terminal
  restored (echo and line mode back, cursor visible, bracketed paste, mouse
  and focus reporting off, alternate screen left).

## Screen

All text below is exact (ASCII except user data, case-sensitive).

- The **question** text.
- One **option row** per option, in the poll's order. In this order, separated
  by single spaces: the selection marker (`>` if selected, else a space), the
  vote box (`[x]` if it holds your vote, else `[ ]`), the label; then one or
  more spaces and the tally `N votes` (`1 vote` when N is 1). Example:
  `> [x] Tacos   1 vote`.
- A tally counts the `vote` records naming that option. Votes naming an option
  that is not in the poll are not counted anywhere.
- `Total: N votes` (`1 vote` when N is 1): sum of the tallies.
- If there are more options than fit, show a contiguous run that includes the
  selected option.
- **Status line**: the last row of the screen. Exact texts:

| When | Status line contains |
| --- | --- |
| A load is pending | `Loading...` |
| A load succeeded | `Loaded N options.` |
| A load failed | `Load failed. Press r to retry.` |
| A write is pending | `Saving...` |
| A vote was saved | `Voted for <label>.` |
| An option was saved | `Option added.` |
| A write failed (not a conflict) | `Save failed.` |
| A conflict was merged and saved | `Conflict: merged with a newer change.` |

- Nothing may be written outside the screen: no line wider than the terminal
  (no autowrap) and no scrolling of text off the top.
- **Small screen**: the poll stays usable down to 40x12. At 40x12 with up to
  six options of up to 12 characters, the question (cut to the width if
  needed), every option row with its tally, the `Total` line and the status
  line are visible, and every key keeps working.
- Every size change is followed by a redraw at the new size.

## Selection and keys

Initial selection: the first option.

| Key | Action |
| --- | --- |
| `Up`/`k`, `Down`/`j` | Previous / next option (stops at the ends) |
| `Enter` or `Space` | Vote for the selected option |
| `a` | Open the new-option form |
| `/` | Open the search input |
| `Esc` | Clear an active search |
| `r` | Reload (both lists again) |
| `q` | Quit |

**Voting.** If you have no vote record, send `data.create` of type `vote`
with `{"voter": <ctx.user.name>, "option": <label>}`; otherwise send
`data.update` of your vote record with its current `rev`. Voting for
the option that already holds your vote does nothing. While a write is pending
(`Saving...`), `Enter` and `Space` are ignored: one confirmation, one vote. On
success the tally, the vote box and the status `Voted for <label>.` update.
If an `update` of your vote gets `conflict`, send it again with `current.rev`.

## New-option form

- Shows the heading `New option`, the field `Option: ` followed by the typed
  text, and the hint `Enter save  Esc cancel`. While it is open, keys go to the
  field (`q`, `j`, `a` are text). `Backspace` deletes one character. Any
  Unicode text can be typed and is shown as typed.
- `Enter` with an empty or blank text shows `Option is required.`; with a
  label equal to an existing one ignoring case, `Option already exists.`. In
  both cases nothing is sent.
- Otherwise `Enter` sends one `update` of the poll record with the label
  (surrounding spaces removed) appended to `options`, and shows `Saving...`.
  Until the reply arrives the form stays open and further `Enter` presses are
  ignored. On success the form closes, the new option is shown last and the
  status is `Option added.`.
- `Esc` closes the form, saves nothing and returns keys to the list with the
  selection it had before.

## Search

- `/` opens the input `Search: ` with the current query. Typing filters the
  option rows to labels containing the query (case-insensitive), at once,
  without contacting the service. `Enter` closes the input and keeps the
  filter (`Search: <query>` stays visible). `Esc` in the input, or on the list
  while a filter is active, clears the filter and closes the input.
- `Total` always counts all options, filtered or not. If nothing matches, show
  `No options match.`.
- Selection with a filter: if the selected option is still shown it stays
  selected; otherwise the first shown option is selected. Clearing the filter
  keeps the selected option selected.

## Paste

Bracketed paste is one piece of text, never keys. In the form or the search
input it is inserted with each line break (CR, LF or CRLF) replaced by one
space. On the list, with no input open, a paste is ignored.

## Loading, errors and other participants

- **Slow load**: while the first load is pending no option rows are shown and
  the status is `Loading...`; `q` and `Ctrl+C` still quit.
- **Load error** (either list fails): show `Load failed. Press r to retry.`,
  keep everything shown before, and keep the selection. `r` retries.
- **Late load**: a reply to `data.list` can arrive after a change that is
  newer than it (your own write's reply, or a push). The contract has no
  counter to compare, so remember which records changed since the list was
  asked for, and keep those as you have them when its reply arrives: a
  slow reload must never undo a newer vote or option.
- **Remote change** (push): apply at once, without a reload. New or changed
  votes move the tallies; a changed poll record changes the question or
  options. The selected option stays selected if it still exists.
- **Write conflict** when adding an option: take `current` from the reply,
  append your label to `current.content.options` if it is not already there
  (ignoring case), and send `update` again with `current.rev`. The other
  participant's options keep their place and order; yours comes last. On
  success show `Conflict: merged with a newer change.`.
