# App spec: Board

A shared board of cards in three columns. Every participant sees the same cards
and sees other participants' changes without reloading. The bench
(`run --app board`) checks only what this page fixes; anything not fixed
here (colours, borders, spacing, extra hints) is free.

## Start, data and exit

- The app is one script written against `docs/app-api.md`, `api = 1`. The
  bench runs it on `examples/app_host.rs`, which lends it the terminal and
  turns the data service of `data-protocol.md` into the contract's records.
- A card is a record of type `card` with content
  `{"title": string, "column": "todo" | "doing" | "done", "order": number}`.
  A list arrives in no order: `order` is the card's place, and a new card
  gets one that puts it last (`host.time()`).
- Other participants' changes arrive as pushes; never poll.
- `q` (when no form or search input is open) closes the app with `host.quit()`.
  `Ctrl+C` never reaches the script: the host leaves on it. Either way,
  even with requests still pending, the bench expects of the host: exit
  within 2 s with status 0, no child process left, and the terminal
  restored (echo and line mode back, cursor visible, bracketed paste, mouse
  and focus reporting off, alternate screen left).

## Screen

All text below is exact (ASCII, case-sensitive).

- **Column headers** `To do (N)`, `Doing (N)`, `Done (N)`, where N is the
  number of cards shown in that column: on one row, left to right, or, where
  the region is too narrow for that, one above the other in that order, each
  followed by its own cards.
- **Cards** below their header, one row per card, in the order of their
  `order` field (newly created cards last). A card row is the selection
  marker then the title: `> Title` for the selected card, two spaces then the
  title otherwise. With the headers on one row the bench assigns a card to
  the column whose header starts nearest (in cells) to the card's marker, so
  keep cards left-aligned with their header.
- Titles of up to 16 characters are shown in full at any size the board is
  drawn. Longer titles may be cut.
- If a column has more cards than fit, it shows a contiguous run of its cards
  that includes the selected card.
- **Status line**: the last row of the screen. Exact texts:

| When | Status line contains |
| --- | --- |
| A load (`list`) is pending | `Loading...` |
| A load succeeded | `Loaded N cards.` (N = cards shown in the three columns) |
| A load failed | `Load failed. Press r to retry.` |
| A write is pending | `Saving...` |
| A write succeeded | `Saved.` |
| A write failed (not a conflict) | `Save failed.` |
| A move failed: the card was deleted meanwhile | `This card was deleted by NAME.`, or `This card was deleted meanwhile.` until bzz says who; the card leaves the board |
| A conflict was merged and saved | `Conflict: merged with a newer change.` |

- Nothing may be written outside the screen: no line wider than the terminal
  (no autowrap) and no scrolling of text off the top.
- **Small screen**: the board works at 40 columns by 16 rows, the size of
  bzz's app pane, and at any larger size. Narrower than 40 columns or shorter
  than 16 rows, show only `Terminal too small. Need 40x16.` (it may be cut to
  the width) and nothing else. `q` and `Ctrl+C` still quit. When the region
  grows back, the board returns with the same selection, search and data.
- Every size change is followed by a redraw at the new size.

## Selection and keys

Initial selection: the first card of the first non-empty column.

| Key | Action on the board |
| --- | --- |
| `Up`/`k`, `Down`/`j` | Previous / next card in the column (stops at the ends) |
| `Left`/`h`, `Right`/`l` | Nearest non-empty column in that direction; select the card at the same position, or its last card if shorter |
| `]` / `[` | Move the selected card to the next / previous column (no-op at the ends). It stays selected |
| `n` | Open the new-card form |
| `/` | Open the search input |
| `Esc` | Clear an active search |
| `r` | Reload (`data.list` again) |
| `q` | Quit |
| `Enter`, other keys | Nothing |

Moving a card sends `data.update` with the card's current `rev` and its whole
content with the new column (its title, its `order` and any other field are
kept); the card is shown in its new column at once.

## New-card form

- Shows the heading `New card`, the field `Title: ` followed by the typed text,
  and the hint `Enter save  Esc cancel`. While it is open, keys go to the field
  (letters such as `q`, `j`, `n` are text). `Backspace` deletes one character.
  Any Unicode text can be typed and is shown as typed.
- `Enter` with an empty or blank title shows `Title is required.` in the form
  and sends nothing.
- `Enter` with a title sends one `data.create` with
  `{"title": <title with surrounding spaces removed>, "column": "todo",
  "order": <a number that puts it last>}` and shows `Saving...`. Until the reply arrives, the form stays open and further
  `Enter` presses are ignored: one confirmation, one card. On success the form
  closes, the new card is selected and the status is `Saved.`.
- `Esc` closes the form, saves nothing, and returns keys to the board with the
  selection it had before.

## Search

- `/` opens the input `Search: ` with the current query. Typing changes the
  query; every change sends `data.list` with `type = "card", match = <query>`
  (an empty query: no `match`). The board shows only the cards of the
  newest request's reply. A reply to an older request that arrives later is
  ignored entirely, even if it arrives last.
- `Enter` closes the input and keeps the filter (the `Search: <query>` text
  stays visible). `Esc` in the input, or on the board while a filter is
  active, clears the query, reloads all cards and closes the input.
- If no card matches, show `No cards match.`.
- Selection with a filter: if the selected card is still shown it stays
  selected; otherwise the first shown card (first non-empty column, top card)
  is selected. Clearing the filter keeps the selected card selected.

## Paste

Bracketed paste is one piece of text, never keys. In the form or the search
input it is inserted with each line break (CR, LF or CRLF) replaced by one
space. On the board, with no input open, a paste is ignored.

## Loading, errors and other participants

- **Slow load**: while the first load is pending the board shows no cards and
  the status `Loading...`; `q` still quits.
- **Load error**: show `Load failed. Press r to retry.`, keep every card that
  was shown before the failed load, and keep the selection. `r` retries.
- **Remote change** (a push): apply it at once without a reload. Created cards appear, updated cards change title or column, deleted
  cards disappear; the counts follow. The selected card stays selected; if it
  was deleted, select the card now at the same position in its column, else the
  first shown card.
- **Write conflict** (reply `conflict` to a move): take `current` from the
  reply, apply your own change (the new column) on top of it, and send
  `data.update` again with `current.rev`. The other participant's change (for example a new
  title) is kept. On success show `Conflict: merged with a newer change.`.
- Remote changes and conflicts respect an active search: a card is shown only
  if its title contains the query (case-insensitive).
