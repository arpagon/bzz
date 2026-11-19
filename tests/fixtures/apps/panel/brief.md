# Panel

A shared panel of metrics: a table that sorts, a tab of the metrics in
alert, the history and a gauge of the selected one, and an acknowledgement
with a note.

**Use.** `j`/`k` and the arrows move, Tab switches between All and
Alerts, `s` sorts by the next column, `d` reverses the sort, `a`
acknowledges the selected metric in alert (asks for a note), `r` lists
again, `q` closes. Under the table: the selected metric's history as a
sparkline and its value against its maximum as a gauge.

**Records.** One type, `metric`:

| Field | Type | Meaning |
|---|---|---|
| `name` | string | `CPU` |
| `unit` | string | `%`, `jobs` |
| `status` | `"ok"`, `"alert"` or `"acked"` | |
| `max` | integer | the gauge's full scale |
| `history` | list of integers | the values, oldest first; the last is the metric's value |
| `acked_by` | string, or null | the name of who acknowledged the alert |
| `note` | string, or null | what they wrote |

**Rules.** Only a metric in alert is acknowledged, once. When an
acknowledgement conflicts with a new value, it is applied on top of the
new history; when someone acknowledged first, the app says
`Already acknowledged by bo.` and writes nothing.

People in fields are written by name (`"ana"`), as the bench's data has
them. The contract advises the id (`ctx.user.id`), which never changes; a
version for real use should store the id and show the name.
