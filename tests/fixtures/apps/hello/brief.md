# Hello

A shared list of short notes: the smallest complete app, to start from.

**Use.** `n` writes a note (Enter saves, Esc cancels), `j`/`k` move,
`r` lists again, `q` closes. The status line says who changed a note
last.

**Records.** One type, `note`:

| Field | Type | Meaning |
|---|---|---|
| `text` | string | the note |
| `author` | string | the name of who wrote it |
| `order` | number | seconds since 1970 when it was made; the list is sorted by it |

A note is never changed after it is made.
