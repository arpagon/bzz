# Rota

Who covers each day of one week. A person takes a free day or gives up one
of their own.

**Use.** `j`/`k` and the arrows move, `t` takes the selected day, `g`
gives it up (Enter confirms, Esc cancels), `f` shows all days, the free
ones, or yours; `r` lists again, `q` closes. Below the days: how many
each person holds.

**Records.** One type, `day`:

| Field | Type | Meaning |
|---|---|---|
| `day` | string | the day's label: `Mon` |
| `holder` | string, or null | the name of who covers it; null when it is free |
| `order` | number | the day's place in the week |

**Rules.** A day held by someone else cannot be taken, nor given up by you.
When two people take one day at once, the one who loses is told
`Tue was taken by bo.` and nothing is overwritten.

People in fields are written by name (`"ana"`), as the bench's data has
them. The contract advises the id (`ctx.user.id`), which never changes; a
version for real use should store the id and show the name.
