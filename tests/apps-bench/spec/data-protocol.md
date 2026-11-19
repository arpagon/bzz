# Data protocol (simulated relay artifacts)

**This is not what an app sees.** It is the protocol of the bench's
simulated data service, kept as the experiment had it. An app is written
against `docs/app-api.md`; `examples/app_host.rs` is the only program that
speaks this protocol, and the table at its top lists how the two differ
(an opaque revision for the number, ids that do not sort, no `seq`, a
participant as `{ id, name }`). The bench's cases use the control port
below to play other participants and to make the service slow or failing.

The program under test talks to one TCP address, `MINIAPP_DATA_ADDR`
(`127.0.0.1:PORT`). Each message is one JSON object on one line (UTF-8, ends
with `\n`). Open one connection at start and keep it.

## Records

```json
{"id": "r0001", "type": "card", "rev": 1, "content": {"title": "Alpha", "column": "todo", "order": 1}}
```

`id` is set by the service and sorts in creation order. `rev` starts at 1 and
grows by one on each update. `content` is any JSON object; its shape belongs to
the app (see `board.md`, `poll.md`, `rota.md`, `approvals.md`,
`panel.md`, `board-priority.md`).

## Requests and replies

Every request has an `id` of your choice (number or string) and an `op`. The
reply carries the same `id`. **Replies may arrive in a different order than
the requests**: match them by `id`. Every successful reply carries `seq`, the
service's change counter (it grows by one on every create, update or delete by
anyone).

| Request | Success reply |
| --- | --- |
| `{"id":1,"op":"subscribe"}` | `{"id":1,"ok":true,"seq":7}` |
| `{"id":2,"op":"list","type":"card","match":"al"}` | `{"id":2,"ok":true,"records":[...],"seq":7}` |
| `{"id":3,"op":"create","type":"card","content":{...}}` | `{"id":3,"ok":true,"record":{...},"seq":8}` |
| `{"id":4,"op":"update","record_id":"r0001","rev":1,"content":{...}}` | `{"id":4,"ok":true,"record":{...},"seq":9}` |
| `{"id":5,"op":"delete","record_id":"r0001","rev":2}` | `{"id":5,"ok":true,"record":{...},"seq":10}` |

- `list`: `type` and `match` are optional. `match` keeps records whose
  `content.title` contains it, ignoring case. Records come in `id` order. The
  snapshot is taken when the request arrives, even if the reply comes later;
  `seq` is the counter at that moment.
- `update` replaces the whole `content`. `rev` must be the record's current
  `rev`. `delete` also needs the current `rev`.
- `update` and `delete` with a stale `rev` fail with a conflict that carries
  the record as it is now:
  `{"id":4,"ok":false,"error":{"code":"conflict","message":"rev is stale"},"current":{...}}`

Errors: `{"id":…,"ok":false,"error":{"code":C,"message":"…"}}` with `C` one of
`unavailable` (try again later), `conflict`, `not_found`, `bad_request`.

## Pushes

After `subscribe`, the service sends changes made by **other** participants
(never the ones you made on this connection):

```json
{"push":"change","change":"update","record":{...},"by":"bo","seq":11}
```

`change` is `create`, `update` or `delete` (for `delete`, `record` is its last
state). Pushes have no `id`. Subscribe before the first `list`, so no change
falls between the two.

## Running it by hand

```sh
tests/apps-bench/data-service --app board --scenario default
# --app board | poll | rota | approvals | panel | board-priority
# prints {"data_addr": "127.0.0.1:41234", "control_addr": "127.0.0.1:41235"}
MINIAPP_DATA_ADDR=127.0.0.1:41234 MINIAPP_USER=ana \
    cargo run --example app_host -- tests/fixtures/apps/board
```

`--log FILE` appends every message in and out to `FILE` (JSON lines).

## Scenarios (`--scenario a,b`)

| Name | Behaviour |
| --- | --- |
| `default` | Small seed data, no delays, no failures |
| `slow-load` | The first `list` reply is delayed 3000 ms |
| `load-error` | The first `list` fails with `unavailable`; later ones succeed |
| `late-search` | The reply to the first `list` with a non-empty `match` is delayed 1500 ms |
| `slow-write` | Every `create`, `update` and `delete` reply is delayed 500 ms (the write itself happens at once) |
| `big` | 200 records: 200 cards (`Card 001`…; with priorities for `board-priority`), one poll with 200 options and 199 votes, 200 days (`Day 001`…), 200 requests (`Request 001`…) or 200 metrics (`M001`…) |

## Control port (for the bench, not for programs)

One JSON command per line on `control_addr`, one reply line each.
`{"cmd":"remote","user":"bo","op":"create|update|delete",...}` makes a change
as another participant and pushes it now (`update` and `delete` take
`record_id` and ignore `rev`). `{"cmd":"arm_conflict","record_id":…,"user":"bo","content":{…}}`
makes the next `update` of that record first apply `content` as `bo` (pushed
to you) and then answer `conflict`. `{"cmd":"fail_next","op":"list"}` and
`{"cmd":"delay_next","op":"list","ms":10000}` affect the next request of that
op. `{"cmd":"dump"}` returns all records; `{"cmd":"stats"}` returns request
counts and the number of subscribed connections.
