# Change request: Board with priorities

A delta on top of `board.md`. Everything in `board.md` still holds, and the
bench (`run --app board-priority`) runs all twelve Board cases first,
then the cases for this change. Only the differences are written here.

## Data

- A card's content gains an optional field `"priority"`: `1`, `2` or `3`.
  Absent (or `null`) means no priority. Example:
  `{"title": "Alpha", "column": "todo", "order": 1, "priority": 1}`.
- New cards from the form still have no priority.
- Every `data.update` you send carries the whole content: a move keeps the
  card's priority, a priority change keeps its title, column and `order`.

## Screen

- A card with a priority shows it after the title, separated by one space:
  `P1`, `P2` or `P3`. Example: `> Alpha P1`, `  Echo P3`. A card without one
  shows nothing after the title. Titles of up to 12 characters and their
  priority are shown in full at any size the board is drawn (40x16 and up).
- **Priority filter indicator**, always visible while the board is drawn:
  `Priority: all`, `Priority: P1`, `Priority: P2` or `Priority: P3`.
- With a filter, the board shows only cards with that priority, and the
  column counts `To do (N)`, `Doing (N)`, `Done (N)` count only the cards
  shown. If no card is shown because of the filter or the search, show
  `No cards match.`.
- `Loaded N cards.` gives the number of cards on the board, in its three
  columns, whatever the filter.

## Keys (on the board, no input open)

| Key | Action |
| --- | --- |
| `1`, `2`, `3` | Set the selected card's priority to 1, 2 or 3 |
| `0` | Remove the selected card's priority |
| `p` | Cycle the priority filter: all, P1, P2, P3, all... |

Setting the priority the card already has does nothing. Otherwise send
`data.update` with the card's current `rev` and its content with the new
`"priority"` (removing it: content without `"priority"`, or `null`). The card
shows its new priority at once; `Saving...`, then `Saved.`; `Save failed.` on
failure. A conflict is handled like a move: take `current`, apply your
priority on top of it (keeping the other participant's title and column),
send again with `current.rev`, and show `Conflict: merged with a newer
change.`.

In the form and the search input, `0`-`3` and `p` are text.

## Filter rules

- `p` changes the view at once, without contacting the service.
- The priority filter combines with the search: a card is shown only if it
  matches both. Clearing the search (`Esc`) keeps the priority filter;
  cycling the priority filter keeps the search.
- Selection: if the selected card is still shown it stays selected; otherwise
  the first shown card (first non-empty column, top card) is selected. Going
  back to `Priority: all` keeps the selected card selected.
- A card whose priority you change so that it no longer matches the filter
  leaves the view; the first shown card is then selected.
- Remote changes respect the filter: a created or updated card is shown only
  if it matches the filter (and the search); a card updated to another
  priority leaves the view; the counts follow.
