# Poll

One question, a list of options, one vote for each person.

**Use.** With no poll yet, `a` asks for the question. Then `j`/`k` and
the arrows move, Enter or Space votes for the selected option (again: moves
the vote), `a` adds an option, `/` searches the options, `r` lists
again, `q` closes. Each option shows its votes, and `x` marks the one
you voted for.

**Records.** Two types:

| Type | Field | Type | Meaning |
|---|---|---|---|
| `poll` | `question` | string | the question; there is one poll record |
| | `options` | list of strings | the options, in the order they were added |
| `vote` | `voter` | string | the name of who voted; one vote record for each person |
| | `option` | string | the option voted for, as written in `options` |

**Rules.** A vote for an option that is not in `options` counts for
nothing. When adding an option conflicts, it is added again on top of the
current list; when a vote conflicts, it is applied on top of the current
vote.

People in fields are written by name (`"ana"`), as the bench's data has
them. The contract advises the id (`ctx.user.id`), which never changes; a
version for real use should store the id and show the name.
