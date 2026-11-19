# The records of apps: the service and the session

For whoever shows an app in a pane (v0.16 M3), gives agents record tools
(M7), or writes the record calls of `docs/app-api.md`. This is the Rust
interface in words: first the service, which knows the relay and nothing of
Lua or of drawing; then, under "The session", what joins it to a script.

- `src/protocol/record.rs`: names, the fence, the mapping between a record
  and its revisions, the title rule, the checks, the budget of writes, what
  a refusal means.
- `src/service/records.rs`: the sync of an open app, and the writes.
- `src/apps/session/`: one open app: a runtime, its records, and the rules
  between them.
- Feature `apps`. Without it none of these is built.

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
  were sent (`3`, `3.0`, `null`). An artifact of the app's type whose
  content is not a JSON object (another client wrote it) is a record with
  no fields: it is shown, counts against the limit, and can be replaced or
  deleted.
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
`app` closes the sync; `service.is_open(channel, app)` stays true until its
task has ended, a moment later.

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
- the content: at most 16,384 bytes of JSON, the number of the contract;
- the signed revision, with the relay's own envelope validator
  (`buzz_core::artifact::validate`), which covers the title, the tags and
  the identifiers;
- the whole escaped frame: at most 524,288 bytes. Past it the relay drops
  the connection without an answer. With content at 16,384 bytes the
  largest revision the relay's validator passes is about 491,000 bytes, so
  this check cannot fire at the pin; it stays for a relay that takes more
  tag bytes;
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
| Content of one record | 16,384 bytes of JSON | `MAX_CONTENT_BYTES` |
| One frame | 524,288 bytes after escaping (the relay's `max_message_length`) | `MAX_FRAME_BYTES` |
| Title | 1 to 512 bytes | `MAX_TITLE_BYTES` |
| Writes | the relay's: 60 a minute per person, shared with chat | `PERSON_WRITES_PER_MINUTE` |
| Writes of one person's apps | 30 a minute, all the person's open apps together | `APP_WRITES_PER_MINUTE`, `WriteBudget` |

These are the one set of numbers: `apps::runtime::limits` names the same
constants, and `docs/app-api.md` publishes them.

The relay ends a sync at 4096 heads **of that sync's filter**, so an app is
limited by its own records. The writer is never told; 1000 is where bzz
stops a create and says so.

**The budget of an app's writes.** `service.budget()` is one `WriteBudget`
for every app the owner has open in this community: 30 writes in any
minute. The service does not spend it; whoever runs an app for the owner
does, before `create`, `update` or `delete` (the session, below). Writes
through `create_as` and its siblings are an agent's and count against the
agent's own quota at the relay, not against this.

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
script has nothing to do for it. It is also the text the `match` of
`data.list` looks at; the contract says both.

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

## The session

`apps::session::AppSession` is one open app in one channel: it owns the
script's runtime and the records, and is what the app's pane owns.

```rust
let mut session = AppSession::on_relay(source, SessionOptions::new("board").waker(waker).names(names), &service, channel)?;
session.key(key);                    // paste, resize, reload, tick, close
for output in session.poll() { … }   // when woken: Log, Loaded, Close, Redraw
session.draw(area, buffer, &theme);  // the cursor of a focused input comes back
let status = session.status();       // phase, error, sync, notice, wait
```

No call waits for the script or for the relay. `on_relay` must be called
inside a Tokio runtime; its tasks do the waiting and call the waker. The
same session runs on a store in memory (`AppSession::in_memory`), which is
what the headless driver, `bzz app try` and the tests of every platform
use: `apps::session::backend::Records` is the boundary, in the service's
own types, and `RelayRecords` and `SharedStore` are its two sides.

What the session does between the two halves:

| Rule | How |
|---|---|
| Every request gets exactly one reply | a write is answered by its task, also when the task ends without an answer (`unavailable`); a request made before the records are open waits for them |
| The fence, once more | a type the running version did not declare is `bad_request`; a record the session does not hold, or holds under another type, is `not_found`. The runtime refuses before, the service after |
| `list` | from the session's set, with `match` over the title rule, at once. With no record held and the first complete set not there, it waits; after 5 s it is `unavailable` |
| A push | one for each `Change` the service tells, of a type the script has listed. A `Removed` without `by` is by the record's last signer. The same revision again is no push |
| Own writes | answered, never pushed. The set takes the answer if it is what the service holds; if the service already holds something later that the script was told, the script is told it again after the reply |
| Names | `{ id, name }` from the names its owner gives (`set_names`); a short form of the key until one is known (`unnamed()` lists whom to look up); the records of someone whose name became known or changed are pushed again as updates |
| The budget | one write out of `service.budget()` before each write; past it `rate_limited` with the wait, and no call of the service. The relay's own `RateLimited` closes the same gate |
| A new version | `Loaded { types }`: if the types changed, the records are opened again for them; requests wait meanwhile |
| A long batch | replies and pushes are sent to the script as it takes them, never more than half of what it may be owed |

What a script sees for each `SyncState`, and what the pane shows by itself
(`status().notice`; `status().sync_words` is the state in a word or two):

| State | `list` | a write | The pane's sentence |
|---|---|---|---|
| `Loading` | the stored records; with none, waits for the first set | sent | none |
| `Live` | the set | sent | none |
| `Resyncing` | the set as it was | sent | none |
| `TooMany { max }` | the set bzz holds | a create is `limit`; an update or a delete is sent | "This app has more than 4096 records here, too many to load. None can be added." |
| `Restricted` | `refused` | `refused`, not sent | "You cannot read this channel's records: this app cannot be used here." |
| `Unavailable { reason }` | the stored records; with none, `unavailable` | `unavailable`, not sent | "This app's records are not available: <reason>." |
| the channel is archived | as above | `refused`; not sent once bzz knows | "This channel is archived: its records can be read, not changed." |
| a wait at the quota | as above | `rate_limited`, not sent | "Too many changes: this app can write again in 23 s." |
| 1000 records held | as above | a create is `limit`, not sent | "This app holds 1000 records here, the most it may. Delete some to add more." |

The session learns of an archived channel from its owner
(`set_archived`, which the pane takes from the channel's own state) or
from the relay's refusal of a write; the second is forgotten when the
relay takes a write or the sync starts anew. `status().wait` is the
seconds left; `tick()` answers true when that number has changed, for the
owner to draw again.

For M7: `session.relay()` is the `AppRecords` of the open app, for
`create_as` and its siblings.

Not in the session: where the sentence is drawn (the pane's border is the
place that costs the app no row), the title of the pane, the approval of a
version, and any command.
