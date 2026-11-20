# Poll

One question for the channel, a list of options, and one vote for each
person. Anyone adds an option or moves their vote; everyone sees the
tallies change at once.

## Records

Two record types.

`poll`, one record for the channel:

| Field | Type | Meaning |
|---|---|---|
| `question` | string | The question. |
| `options` | list of strings | The options, in the order they were added. |

`vote`, one record for each person who voted:

| Field | Type | Meaning |
|---|---|---|
| `voter` | string | The id of who voted: their public key (`ctx.user.id`), which never changes. |
| `option` | string | The option voted for, as written in `options`. |

Rules another client must keep to share these records:

- A person's vote is the `vote` record whose `voter` is their id. Voting
  again changes that record; it never makes a second one.
- A vote for an option that is not in `options` counts for nothing.
- Of several `poll` records, the one with the smallest id is the poll.
- When adding an option conflicts with someone else's change, the option
  is added on top of the current list: theirs keep their place, the new
  one goes last. When a vote conflicts, it is applied on top of the
  current vote record.
- A person is stored by id and shown by name (`host.name`): a name can
  change, an id cannot.

## Keys

With no poll yet, `a` asks for the question. Then `j`/`k` or the arrows
move, Enter or Space votes for the selected option (again: moves the vote),
`a` adds an option, `/` searches the options (Esc clears it), `r` loads
again, `q` closes.

## Screen

The question, each option with its votes (`[x]` marks yours), the total,
who voted for the selected option, and a status line. It fits the 40
columns by 16 rows of bzz's app pane.
