# Board

Shared cards in three columns, To do, Doing and Done. Anyone in the channel
adds cards and moves them; everyone sees the same board.

**Use.** `j`/`k` and the arrows move the selection, `h`/`l` change
column, `]` and `[` move the selected card to the next or previous
column, `n` adds a card to To do, `/` searches titles, `r` lists again,
`q` closes. From 60 columns the three columns stand side by side; below,
they are stacked.

**Records.** One type, `card`:

| Field | Type | Meaning |
|---|---|---|
| `title` | string | what the card says |
| `column` | `"todo"`, `"doing"` or `"done"` | where it stands |
| `order` | number | its place in a column, lowest first; a new card takes the time it was made |

**Rules.** When two people change one card at once, the move is applied on
top of the other's change: a board merges. A field this version does not
know (such as `priority`) is kept when it writes.
