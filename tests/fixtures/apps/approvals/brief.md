# Approvals

Requests that someone else in the channel approves, or rejects with a
reason. A decided request stays decided.

## Records

One record type, `request`:

| Field | Type | Meaning |
|---|---|---|
| `title` | string | What is asked. |
| `author` | string | The id of who asked: their public key (`ctx.user.id`). |
| `state` | `"pending"`, `"approved"` or `"rejected"` | Anything else counts as pending. |
| `decided_by` | string, or null | The id of who decided. |
| `reason` | string, or null | Why it was rejected. |
| `order` | number | The list is sorted by it, smallest first. A new request takes the time it was made, in seconds since 1970. |

Rules another client must keep to share these records:

- Nobody decides their own request: `author` is compared with the id of
  who decides.
- A decision is never written over another. If someone decided first (a
  conflict, or a request that is no longer pending), the app says
  `Already rejected by bo.` and writes nothing.
- A decision changes `state`, `decided_by` and `reason`; every other field
  is written back as it was read.
- These are conventions between honest participants: any member of the
  channel can set any field, and the relay does not check them.
- A person is stored by id and shown by name (`host.name`): a name can
  change, an id cannot.

## Keys

`j`/`k` or the arrows move, Enter opens a request's detail (Esc closes it),
`n` makes a request, `a` approves the selected one, `x` rejects it (it asks
for a reason), `f` cycles the filter all, pending, approved, rejected; `r`
loads again, `q` closes.

## Screen

A filter line, the requests with their state, title and author, a detail
beside the list from 80 columns and under it below, and a status line. It
fits the 40 columns by 16 rows of bzz's app pane.
