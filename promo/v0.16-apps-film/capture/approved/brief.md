# Board with priorities
Shared cards in To do, Doing and Done. Adds priority badges and a local priority filter; existing cards need no migration.

## Records
One record type: `card`.

| Field | Type | Meaning |
|---|---|---|
| title | string | Required title; missing/invalid displays (no title). |
| column | string | Required: todo, doing, done. Other values retained but hidden. |
| order | number | Creation time in seconds; ascending within column, ties by id. Invalid/missing treated as 0. |
| priority | string | Optional: normal, high, low. Missing/invalid displays and filters as normal. New cards use normal. |

New cards start in todo. Updates copy all fields and change only column or priority. Conflicts retry the intended field/value on the latest content, retaining other fields including unknown ones. No cards are deleted by this app; external deletions disappear. Priority filters and case-insensitive literal title search combine locally and never change records. Sorting remains by order, not priority. Failed writes report an error; rate limiting displays retry seconds.

## Keys
j/k or up/down select; h/l or left/right change column; ]/[ move a card. n opens new card (Enter saves, Esc cancels). p cycles selected priority normal → high → low → normal. f cycles filter all → high → normal → low → all. / edits title search; Enter finishes, Esc clears search while editing. Outside editing, Esc resets both search and filter. r reloads; q closes. Badges [N], [H], [L] show priority.

## Changes
Adds priority and filter, with combined local search. No records rewritten on upgrade. Reload initializes filter defaults and loads all records while keeping search. Layout remains stacked below 60 columns and side by side at 60+. Writes appear after confirmation rather than optimistically.