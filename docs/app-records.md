# The records of apps: the service

For whoever joins an app's script to its records (v0.16 M2-B), gives agents
record tools (M7), or writes the record calls of `docs/app-api.md`. This is
the Rust interface in words. It knows the relay; it knows nothing of Lua or
of drawing.

- `src/protocol/record.rs`: names, the fence, the mapping between a record
  and its revisions, the title rule, the checks, what a refusal means.
- `src/service/records.rs`: the sync of an open app, and the writes.
- Feature `apps`. Without it neither module is built.

The relay facts behind every rule here were measured on the pinned relay in
M0 question 3 and are summed up in `docs/adr-v0.16-apps.md`, "Records on the
relay".

## What a record is

A record is a NIP-AR artifact (kind 45010) of one channel, of the type
`bzz.app.<app>.<record>`. An app and its script only ever name the short
form, `card`.

```rust
pub struct Record {
    pub id: Uuid,
    pub record_type: String,          // "card", never the long form
    pub revision: String,             // opaque; pass it back unchanged
    pub content: Map<String, Value>,  // the app's own fields
    pub author: String,               // who signed this revision, pubkey hex
    pub signed_at: u64,               // the signer's clock, seconds; not an order
}
```

- `revision` is the event id of the head. A caller treats it as opaque.
- `content` is a JSON object. Integers, floats and `null` come back as they
  were sent (`3`, `3.0`, `null`).
- `author` is the last writer. Any writer of the channel may revise or delete
  any record, so nothing here says who created it. A rule such as "you
  cannot decide your own request" rests on a field any member can set.
- There is no order. Neither the ids, nor `signed_at`, nor the order of
  `records()` says which record came first. A record carries its own order
  field.

## Opening

```rust
let service = RecordsService::new(community, http, store, Writer::new(signer, supervisor));
let (app, mut events) = service.open(channel, "board", &["card", "column"]).await?;
```

`open` checks the names, reads what the store kept and returns at once.
`app.records()` is that set, so a restart shows the records before the relay
answers. One sync is then opened for this app alone: its channel and the
exact list of its types (the relay has no prefix filter). The artifact
panel's channel-wide sync is not used.

One instance of an app per channel: a second `open` of the same app in the
same channel is refused while the first is open. Dropping every clone of
`app` closes the sync.

`events` then tells two things:

| Event | Meaning |
|---|---|
| `State(SyncState)` | where the set stands (below) |
| `Changes { changes, cause }` | changes by other participants, each `Upsert(record)` or `Removed { record, by }` |

`cause` is `Live` for one revision as it happened, and `Sync` when bzz
fetched the whole set and these are all the differences from what the caller
held. `Removed.by` is who signed the removal when a revision says so, and
`None` when the record is simply no longer in the set.

**The caller's own writes are not in `events`.** Each is answered by its
call. A write made through `create_as` and its siblings (another writer of
this bzz, see "Who signs") is told, once, like anyone's.

At any time `app.records()`, `app.record(id)` and `app.state()` read the
current set and state.

### States

| `SyncState` | When | What the pane can do |
|---|---|---|
| `Loading` | from `open` until the first complete set | show the stored records |
| `Live` | the set is complete and kept current | |
| `Resyncing` | the relay ended the sync, or the connection dropped; bzz is asking again | nothing: the set is the last complete one |
| `TooMany { max }` | the relay ended the sync with `error: sync set exceeds max_events` | say so. Final. Records can be revised and deleted (the way out); none can be created. `app.retry()` asks again. |
| `Restricted` | `restricted:`: this identity cannot read the channel | say the app cannot be used here. Final. Writes are refused without a frame. |
| `Unavailable { reason }` | the relay has no artifact sync, or refused the request | show the reason. Final until `app.retry()`. |

### A resync is not a change

The relay ends every artifact sync of a channel on ordinary chat events: a
deleted message (`resync: redaction`), a changed topic or an archive, a
removed member (`resync: channel access changed`). There is no resume: the
whole set must be fetched again.

The caller's set is never emptied in between. bzz waits 250 ms
(`RESYNC_DELAY`; twice as long after each attempt the relay ends before
`EOSE`, up to 30 s), asks again, and at `EOSE` tells the caller the
differences between the set it held and the new one, in one `Changes` with
`cause: Sync`. If nothing differs, nothing is told but `State(Live)`. The
same holds after a reconnect, and when bzz itself lost frames.

A script therefore never sees a resync as "every record deleted, then
created".

## Writing

```rust
app.create("card", content).await        // -> Result<Record, RecordError>
app.update(id, &revision, content).await // the whole content, not a patch
app.delete(id, &revision).await          // answers the record's last state
```

Every revision is a whole snapshot. `revision` is the one the caller saw.
There is no restore: the contract has none, and a deleted record keeps no
row in the store.

On success the record is in `app.records()` and in the store before the call
returns, so the next write can build on the revision just given.

### The answers

| `RecordError` | `code()` | Meaning | A frame was sent |
|---|---|---|---|
| `Conflict { current }` | `conflict` | the record was revised since `revision`. `current` is the record as it is now. What to do is the app's rule: a board merges, an approval must not. | only if bzz did not already hold the newer head |
| `RateLimited { seconds }` | `rate_limited` | the identity's quota at the relay is used up. Nothing was stored. | the first time; while the gate is closed, no |
| `TooLarge { what, bytes, limit }` | `too_large` | the content, or the escaped frame | no |
| `Limit { limit }` | `limit` | the app already holds 1000 live records in this channel | no |
| `Refused { refusal, reason }` | `refused` | `Archived`, `Guest`, `NotMember`, `Unreadable`, `Signer`, `Other`. `reason` is a sentence to show; for `Other` it is the relay's own, cleaned and cut at 160 characters. | yes, except `Unreadable` and `Signer` |
| `NotFound` | `not_found` | no such record of this app in this channel: never there, deleted, moved away | only if the relay had to say so |
| `Unavailable { reason }` | `unavailable` | offline, locked, or no acknowledgement. The write may or may not have been stored; if it was, it arrives as a change. | perhaps |
| `Fenced { reason }` | `fenced` | a type the app did not declare | no |
| `Invalid { reason }` | `bad_request` | a bad name, or a revision the relay's validator would refuse | no |

**Conflict.** The relay's refusal (`conflict: artifact head changed`) does
not carry the head. bzz takes it from the app's own sync, which on the pinned
relay delivers it 2 to 8 ms after the refusal; after 300 ms
(`CONFLICT_SYNC_WAIT`) it reads the head over HTTP, which also shows a
deletion (then the answer is `NotFound`). The winner's revision is told in
`events` as well, as the change by another participant that it is. bzz does
not retry: each retry costs one more of the person's quota, and whether to
merge is the script's rule.

Two other refusals begin with `conflict:` and are not a stale head:
`artifact identity is taken` (`Refused`) and `artifact head unavailable`
(`NotFound`).

**Rate limit.** The relay counts every write frame, the refused ones too,
against one budget per identity: 60 a minute at the pin, **shared with the
person's chat messages**; an agent has its own 120. `seconds` comes from the
relay's `rate-limited: quota exceeded; retry in 40s`. While that gate is
closed bzz sends nothing for the identity; an app's write is answered at once
with what is left of the wait, instead of waiting in a queue.

### Checked before sending

A refused frame costs the same quota as an accepted one, so bzz sends
nothing it knows the relay would refuse:

- the names (below), and the fence;
- that the app holds fewer than 1000 live records;
- the content: at most 65,536 bytes of JSON;
- the signed revision, with the relay's own envelope validator
  (`buzz_core::artifact::validate`), which covers the title, the tags and
  the identifiers;
- the whole escaped frame: at most 524,288 bytes. Past it the relay drops
  the connection without an answer;
- for a revision or a deletion: that the record is in the app's set, at the
  revision the caller names.

## The fence (rule 17)

`Fence` (one app, one channel, its declared types) is the only way in or
out:

- **A type an app names** must be one it declared, else `Fenced`. The long
  form, another app's type and `buzz.task` are all "not declared".
- **A record an app revises or deletes** must be in its own set, which holds
  only its types in its channel. A record of another type or channel that
  this bzz happens to know is `NotFound`.
- **A head the relay sends** is taken only if its type is declared and it is
  homed in the channel. bzz does not take the relay's filter on trust.
- **A revision bzz signed** is checked once more for channel and type before
  it is sent.

This holds for every writer, the owner and the agents of M7 alike: the
checks are in `AppRecords`, not in its callers.

## Names and limits

| | Rule | Constant |
|---|---|---|
| App name, record type name | `[a-z][a-z0-9_-]{0,31}` | `valid_name`, `MAX_NAME_BYTES` |
| Types one app declares | 1 to 16, no duplicates | `MAX_TYPES` |
| Artifact type | `bzz.app.<app>.<record>`, at most 73 bytes; the relay takes 128 | |
| Live records of one app in one channel | 1000, all its types together | `MAX_LIVE_RECORDS` |
| Content of one record | 65,536 bytes of JSON | `MAX_CONTENT_BYTES` |
| One frame | 524,288 bytes after escaping (the relay's `max_message_length`) | `MAX_FRAME_BYTES` |
| Title | 1 to 512 bytes | `MAX_TITLE_BYTES` |
| Writes | the relay's: 60 a minute per person, shared with chat | |

The relay ends a sync at 4096 heads **of that sync's filter**, so an app is
limited by its own records. The writer is never told; 1000 is where bzz
stops a create and says so. Records that other clients wrote with content
that is not a JSON object are not shown; `app.unreadable()` counts them, and
they count against the 1000.

`RecordsService::channel_total(channel)` is the relay's count of every live
artifact of the channel. Past 4096 the artifact panel's channel-wide sync
fails for every client; an app can warn near it (3500 was proposed in M0).

### The title rule

An artifact needs a title; a record has only its fields.

> The title is the record's field `title` when that is a string with
> something to show: control characters become spaces, the ends are trimmed,
> and it is cut at 512 bytes, on a character. Otherwise it is
> `<app> <record>`, such as `board card`.

The title is for other clients' lists. An app never reads it back, and a
script has nothing to do for it. The contract should say that a field named
`title` is what other clients show.

## Who signs

A write is signed by a `Writer`: a signer and the connection it is sent on.
`RecordsService::new` takes the owner's, the identity bzz is unlocked with;
`create`, `update` and `delete` use it. `app.identity()` is that identity's
public key, which does not change.

`create_as`, `update_as` and `delete_as` take another `Writer`. M7 makes one
from a local agent's signer and that agent's own connection: the relay
accepts an event only from a connection authenticated as its signer, and
counts an agent's writes against the agent's quota. The fence and every check
are the same. The open app is told of such a write once, as of any
participant's change. On the pinned relay an agent with the role `bot`,
writing this way, was stopped at the relay's own 120 a minute while its
owner's write went through.

## Where app writes stand in bzz's own queue

`SupervisorHandle::publish_app` sends in a fourth class,
`PublishPriority::App`:

- it goes out after a person's messages, their recovery and bzz's
  maintenance, and before background subscriptions;
- it has a queue of its own (32), so no number of app writes fills the queue
  messages use;
- it has an in-flight slot of its own: a message does not wait for the
  acknowledgement of an app's write;
- while the quota gate is closed, or its queue is full, or it waited 25 s
  unsent, it is answered with `rate-limited: retry in <n>s`, which the
  service gives as `RateLimited`.

What this does **not** do: the relay's budget is one, so an app that spends
it leaves none for the person's next message until the minute passes. See
`docs/validation-v0.16.0.md`, M2 (service), "Not proven".

## What the supervisor now says about a closed sync

`SessionEvent::Closed.message` is still a fixed text of bzz's, never the
relay's. `realtime::admission::closed_reason` reads it:

| Relay | Reported as | `ClosedReason` |
|---|---|---|
| `resync: <anything>` | `resync: relay asked for a new sync` | `Resync` |
| `error: sync set exceeds max_events` | the same text | `SyncOverflow` |
| `restricted:`, `blocked:`, `auth-required:` | `restricted: relay denied the subscription` | `Restricted` |
| anything else | `invalid: relay closed the subscription` | `Other` |

All four are final for the supervisor: it does not ask again by itself. The
owner of the subscription decides.

## The store

Records are rows of `artifacts` (migration 15); the schema stays at 20.

- `replace_artifact_types(community, channel, types, heads)`: the complete
  set of those types in that channel, in one transaction. Rows of other
  types and channels are not touched. No deletion row is kept.
- `typed_artifacts(community, channel, types)`: every live head, no cut.
- `channel_artifacts`, which the artifact panel lists from, leaves out
  `bzz.app.<app>.<record>`. The type `bzz.app` itself (a shared script, M6)
  is still listed.

## For the script contract

What `docs/app-api.md` can promise from this:

| Contract | Here |
|---|---|
| `data.list` | `app.records()`, filtered by `record_type` |
| `data.create` / `update` / `delete` | the three calls; `rev` is `revision` |
| a push | one `Change` of a `Changes` event; `by` is `record.author` for an upsert, `Removed.by` for a removal |
| error codes | `RecordError::code()`; `conflict` carries `current`, `rate_limited` carries `seconds`, `refused` carries `reason` |
| the local participant's id | `app.identity()` |
| an undeclared type | the runtime raises at the calling line; if it did not, the service answers `fenced` |
| limits to publish | the table above |

Not here: a name to show for an identity (bzz's profiles have it), a filter
such as `match` (the caller filters `records()`), and any order.
