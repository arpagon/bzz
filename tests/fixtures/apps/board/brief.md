# Board

A task board that a channel shares: cards in three columns, To do, Doing
and Done. Anyone in the channel adds a card and moves it between columns,
and everyone who has the board open sees the change at once.

## Records

One record type, `card`. A card's content:

| Field | Type | Meaning |
|---|---|---|
| `title` | string | What the card says. Required; a card without one is shown as "(no title)". |
| `column` | string | `"todo"`, `"doing"` or `"done"`. A card with another value is kept and not shown. |
| `order` | number | Where the card stands in its column, smallest first. A new card gets the time it was made, in seconds since 1970. Two cards with the same `order` are ordered by their id. |

Rules another client must keep to share these records:

- A new card starts in `"todo"`.
- Moving a card changes only `column`. Every other field is written back as
  it was read, also fields this version does not know.
- When two people move the same card at once, the later one takes the card
  as the first left it and sets its own `column` on top.
- Cards are never deleted by this app. A card that another client deletes
  disappears from the board.

## Keys

`j`/`k` or the arrows move the selection, `h`/`l` change column, `]` and `[`
move the selected card to the next or the previous column, `n` opens a form
for a new card (Enter saves, Esc cancels), `/` searches the titles (Esc
clears the search), `r` loads the cards again, `q` closes the board.

## Screen

From 60 columns the three columns stand side by side; below that they are
stacked, so that it fits the 40 columns by 16 rows of bzz's app pane.
