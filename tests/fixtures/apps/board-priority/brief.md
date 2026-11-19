# Board with priority

The Board, with a priority on each card and a filter by priority. It is
the owner's example of v0.16.0: an agent added the priority and the filter
to the running Board, and the board changed in place.

**Use.** As the Board, and: `1`, `2`, `3` set the selected card's
priority, `0` removes it, `p` cycles the filter all, P1, P2, P3 (with
how many cards each holds). The filter and the search combine. A card
shows its priority after its title: `Charlie P2`.

**Records.** The Board's `card`, with one more field:

| Field | Type | Meaning |
|---|---|---|
| `title`, `column`, `order` | | as in the Board |
| `priority` | `1`, `2` or `3`, or absent | 1 is the most urgent; absent is none |

The Board without priorities reads these cards and keeps `priority` when
it moves one. **Rules.** A conflict is merged: the move or the priority is
applied on top of the other's change.
