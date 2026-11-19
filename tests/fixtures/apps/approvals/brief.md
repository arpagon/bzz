# Approvals

Requests that someone else approves or rejects, with a reason for a
rejection.

**Use.** `j`/`k` and the arrows move, Enter opens a request's detail
(Esc closes it), `n` makes a request, `a` approves the selected one,
`x` rejects it (asks for a reason), `f` cycles the filter all, pending,
approved, rejected; `r` lists again, `q` closes.

**Records.** One type, `request`:

| Field | Type | Meaning |
|---|---|---|
| `title` | string | what is asked |
| `author` | string | the name of who asked |
| `state` | `"pending"`, `"approved"` or `"rejected"` | |
| `decided_by` | string, or null | the name of who decided |
| `reason` | string, or null | why it was rejected |
| `order` | number | the list is sorted by it; a new request takes the time it was made |

**Rules.** Nobody decides their own request, and a decided request is not
decided again. A conflict is not merged: if someone decided first, the app
says `Already rejected by bo.` and writes nothing. These are conventions
between honest participants: any member can set any field.

People in fields are written by name (`"ana"`), as the bench's data has
them. The contract advises the id (`ctx.user.id`), which never changes; a
version for real use should store the id and show the name.
