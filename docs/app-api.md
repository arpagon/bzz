# The app contract, `api = 1`

An app is one Lua 5.4 file. bzz runs it inside itself, beside a channel's
conversation, with only what this page lists. The script returns elements
and reacts to events; bzz draws, owns the terminal, and signs the records.
Nothing is compiled or installed.

This page is the whole contract: what a script defines, the elements, the
record calls, the limits, and what a script cannot do. The same contract as
declarations for an editor is [`app-api.d.lua`](app-api.d.lua). A test
(`apps::runtime::tests::contract`) fails when this page, that file and the
runtime name different things. Two complete apps written against it are
`tests/fixtures/apps/hello/app.lua` and `tests/fixtures/apps/board/app.lua`.

## What a script defines

```lua
local app = {}
app.api = 1                -- required: the number of this contract
app.types = { "card" }     -- required: the record types the app uses
function app.view(state, ctx) return ui.text { "hello" } end   -- required
return app                 -- the script must end with this
```

bzz refuses a script whose `app.api` is not `1`, and says that it knows 1.

| Field | Called | Purpose |
|---|---|---|
| `app.api` | | The number `1`. |
| `app.types` | | The record types the app may read and write: 1 to 16 names, each `[a-z][a-z0-9_-]{0,31}`. |
| `app.view` | after every round of events | `(state, ctx)`. Returns the screen: an element, or a string. It only builds elements. |
| `app.init` | once, at start | `(ctx)`. Returns the state table every other handler receives. Default: an empty table. |
| `app.key` | a key press | `(state, key, ctx)` |
| `app.paste` | a paste | `(state, text, ctx)`. One piece of text; it may contain line breaks. |
| `app.reply` | the reply to a `data.*` request | `(state, reply, ctx)` |
| `app.push` | a change by another participant | `(state, push, ctx)` |
| `app.resize` | the app's region changed size | `(state, ctx)` |
| `app.timer` | a timer set with `host.after` fired | `(state, tag, ctx)` |
| `app.reload` | a new version replaced the script | `(state, ctx)`. Returns a table to replace the state, or nothing to keep it. |

Handlers change `state` in place; what they return is ignored, except for
`app.init` and `app.reload`.

**A round.** bzz takes every event that is waiting, calls the handler of
each, then calls `app.view` once and draws what it returns.

**The top level** of the script runs when the script is loaded, and again
at every reload. It can only compute: `data.list`, `data.create`,
`data.update`, `data.delete`, `host.after` and `host.quit` are for handlers.
Send the first `data.list` from `app.init`.

**`ctx`** is `{ width, height, user }`: the columns and rows of the app's
region, and the local participant as `{ id, name }` (see "Who" below). `id`
never changes; `name` is for showing and can change. An app is drawn in 40
columns by 16 rows when it is docked beside the conversation, and larger
when it is zoomed: choose the layout from `ctx.width`.

**`key`** is `{ name, text, ctrl, alt, shift }`. `name` is the character
itself for a printable key (`"j"`, `"J"`, `"/"`, `"日"`), `"f1"` to `"f12"`,
or one of the key names below. `text` is what the key types, or nil for a
control key and with Ctrl or Alt. Shift+Tab is `"tab"` with `shift = true`.

**Key names:** `space`, `enter`, `esc`, `backspace`, `tab`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `delete`, `insert`

**bzz keeps one key: Ctrl+C.** It leaves the app and never reaches a
script. Every other key is the app's while the app has the focus. (A
terminal or a multiplexer may keep keys of its own before bzz sees them.)

## The screen

`app.view` returns a tree of elements. Each constructor takes one table:
positional items are children or content, named fields are properties. A
property that does not exist is an error at the line that wrote it. Nothing
is drawn outside the app's region: long text is cut at the edge.

| Element | Properties | Draws |
|---|---|---|
| `ui.text` | align, wrap, scroll | Items: strings (a line break starts a new line), `ui.line` or `ui.span`, one line each. `align`: `"left"`, `"center"`, `"right"`. |
| `ui.span` | | `ui.span { "text", fg = "red" }`: a piece of a line with its own style. |
| `ui.line` | align | One line of strings and spans. |
| `ui.block` | title, title_bottom, title_align, borders, border_type, border_style, padding, spacing | A border around its children, stacked as a column. `borders`: `"all"` (default), `"none"`, or a list of `"top"`, `"bottom"`, `"left"`, `"right"`. `border_type`: `"plain"`, `"rounded"`, `"double"`, `"thick"`. `padding`: `n`, `{horizontal, vertical}` or `{left, right, top, bottom}`. |
| `ui.column` | spacing, flex, margin | Its children one under the other. |
| `ui.row` | spacing, flex, margin | Its children side by side. |
| `ui.list` | items, selected, highlight_symbol, highlight, id, direction, scroll_padding | One row for each item. `selected` counts from 1 (nil: none); the list scrolls to keep it in sight. Rows that are not selected are indented by the width of `highlight_symbol` (default `"> "`). `id` keeps the scroll position between rounds. `direction`: `"top_to_bottom"`, `"bottom_to_top"`. |
| `ui.table` | header, rows, footer, widths, selected, highlight, highlight_symbol, column_spacing, header_style, id | `rows` is a list of rows, each a list of cells (a string, `ui.span` or `ui.line`). `widths`: one size for each column (default: equal). A cell is one row high. `selected`, `highlight`, `highlight_symbol` and `id` as in `ui.list`. |
| `ui.tabs` | titles, selected, divider, highlight | One row of titles. |
| `ui.gauge` | ratio, percent, label, gauge_style, use_unicode | A filled bar. `ratio` 0 to 1, or `percent` 0 to 100. |
| `ui.line_gauge` | ratio, percent, label, filled_style, unfilled_style | A progress line of one row. |
| `ui.sparkline` | data, max, direction | `data`: numbers 0 or more, newest last. `direction`: `"left_to_right"`, `"right_to_left"`. |
| `ui.bar_chart` | bars, bar_width, bar_gap, max, horizontal, value_style, label_style | Each bar is `{ label =, value =, text = }` and may have style fields. |
| `ui.scrollbar` | orientation, length, position, viewport, thumb_style, track_style | Along an edge of its own area. `orientation`: `"vertical_right"`, `"vertical_left"`, `"horizontal_bottom"`, `"horizontal_top"`. |
| `ui.input` | value, cursor, placeholder, focused | A text field of one row. `cursor` counts characters (0: before the first; nil: at the end). `focused = true` shows the terminal's cursor in it. |
| `ui.layers` | | Its children on the same area, later ones on top. |
| `ui.popup` | width, height, title, border_type, spacing | A bordered box in the middle of its area, cleared underneath. Put it last in `ui.layers` for a form or a dialog. |
| `ui.spacer` | | Empty space; give it a `size`. |

Every element but `ui.span` and `ui.line` also takes `size`, and every
element takes the style fields.

**Style fields:** `fg`, `bg`, `bold`, `dim`, `italic`, `underline`, `reversed`, `crossed_out`

A property that ends in `_style`, and `highlight`, takes a table of style
fields, or the name of a style of the theme.

**Colours.** `fg` and `bg` take a name of the theme, a fixed colour name,
`"#rrggbb"`, or an index 0 to 255. Prefer the names of the theme: the app
then follows the user's theme.

**Theme names:** `accent`, `muted`, `error`, `warning`, `success`, `selection`, `border`, `title`

**Colour names:** `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`, `dark_gray`, `light_red`, `light_green`, `light_yellow`, `light_blue`, `light_magenta`, `light_cyan`, `white`, `reset`

**Sizes.** `size` is the element's width in a row or its height in a
column: cells (`10`), `"50%"`, `"1/3"`, `"min:10"`, `"max:10"`, `"fill"` or
`"fill:2"` (a share of what is left). Without a `size`, in a column a text
takes as many rows as it has lines, an input, a line gauge and tabs take
one, and anything else fills; in a row everything fills. What is left after
the sized parts goes to the parts that fill, by their shares. With no part
that fills it goes to the last part, or, with `flex` (`"start"`, `"center"`,
`"end"`, `"space_between"`, `"space_around"`, `"space_evenly"`; default
`"legacy"`), it becomes space between and around the parts. When the parts
want more than there is, the last ones give way first.

**Text fields.** Keep `{ value = "", cursor = nil }` in the state, draw it
with `ui.input`, and give it keys and pastes:

- `ui.edit(field, key)` applies a key: typing, Backspace, Delete, arrows,
  Home, End, Ctrl+U. It returns false for a key it does not use (Enter, Esc).
- `ui.paste(field, text)` inserts text at the cursor, each line break as one
  space.
- `ui.width(text)` is the width in cells (a wide character counts 2).
- `ui.truncate(text, width)` cuts to at most `width` cells, with `…`.

## Records

Records are what the people of a channel share through an app. They live on
the relay; bzz signs them, and a script never sees a key.

| Call | Fields | Does |
|---|---|---|
| `data.list` | type, match, tag | Asks for the records of one type. With `match`, those whose title contains it, ignoring case. bzz answers from the records it holds, at once. |
| `data.create` | type, content, tag | Creates a record. `content` is a table with named fields. |
| `data.update` | record_id, rev, content, tag | Replaces the whole content. `rev` is the record's current `rev`. |
| `data.delete` | record_id, rev, tag | Deletes a record. |

Each call returns a request id at once. The answer comes later, to
`app.reply`. Replies can arrive in any order: for a search, keep the id of
the newest `data.list` and ignore a reply with another id.

- **A record** is `{ id, type, rev, content, by }`. `rev` is an opaque
  string: pass it back unchanged. `by` is who signed this revision, as
  `{ id, name }`. A record that another client wrote with content that is
  no table of named fields arrives with an empty `content`; it can be
  replaced and deleted like any other.
- **A reply** is `{ id, tag, op, ok, request }`, and `records` (a list), or
  `record` (create and update: the record as stored; delete: its last
  state). When `ok` is false it has `error = { code, message }`. `request`
  is what was sent; `tag` is the tag given, to say what the reply is for.
- **A push** is `{ change, record, by }`: `change` is `"create"`,
  `"update"` or `"delete"`, and `by` is who made it. The app's own writes do
  not come back as pushes: apply them from the reply. The same person's
  writes from another place do. **Pushes of a type begin with the app's
  first list of it**: a push says that what the app was given has changed,
  so before a list there is nothing to say. Send the first `data.list` of
  each type from `app.init`.
- **The fence.** A type is the app's own short name, such as `card`. bzz
  makes the type the relay sees from it; a script never writes that form.
  A request for a type that is not in `app.types` is an error, pushes of
  other types never arrive, and only a record the app has received in a
  reply or a push can be updated or deleted.

| Code | Means |
|---|---|
| `unavailable` | For a write: no connection, or the relay did not answer; it may or may not have been stored, and if it was it arrives as a push. For a list: bzz holds no record of the app and the relay has not sent any within five seconds. Try again later. |
| `conflict` | The `rev` was stale. `reply.current` is the record as it is now: apply the change on top of `reply.current.content` and send it again with `reply.current.rev`. What a conflict means is the app's rule: a board merges, an approval must not. |
| `not_found` | No such record: it was deleted meanwhile. |
| `bad_request` | The request itself is wrong: bzz or the relay would not take it, whoever sent it. |
| `rate_limited` | The app's writes for this minute are used up, or the person's at the relay. `reply.error.retry_after` is the seconds to wait; say so on the screen. Until then every write is answered the same way and nothing is sent. |
| `too_large` | The content is larger than a record may be. `reply.request` then has no `content`. |
| `limit` | The app has as many records as it may have, or more than bzz can load. Records can still be changed and deleted. |
| `refused` | The relay refused, or bzz knew it would: `message` is the reason. A guest, an archived channel, a channel whose records this person cannot read. |

**No promised order.** A list arrives in no order a script may rely on, and
ids do not sort by creation. Keep the order in the record and sort by it:

```lua
local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id                  -- two records made in the same moment
end
-- when creating:   content = { title = text, order = host.time() }
-- after any change: table.sort(state.cards, by_order)
```

**Who.** A participant is `{ id, name }`. `id` is the person's or the
agent's public key, 64 hexadecimal digits; it never changes, and it is what
to compare and to store in a record. `name` is the name bzz knows for it
when it hands the table over, and a short form of the id (`c0c0c0c0…c0c0`)
while it knows none. When bzz learns the name of someone, or a name
changes, the records that person signed are handed over again, each as a
push with `change = "update"` and the `rev` the app already has; a new name
of the local participant is in `ctx.user` from the next call on. To show
a person whose id is stored in a field, call `host.name(id)` (see "The
host"): never store a name to show it.

Any member of the channel, an agent included, can revise or delete
any record and set any field of it. `record.by` is whoever wrote it last,
not an owner. A rule such as "you cannot approve your own request" rests on
a field any member can set: it is a convention between honest participants,
and bzz cannot enforce it. Read fields with care: another client may have
written a record the app did not expect.

**What bzz holds.** bzz keeps an app's records on disk, and keeps them
current while the app is open. A list is answered from them at once, also
before the relay has answered and without a connection; what the relay
then says differently arrives as pushes. Only an app of which bzz holds no
record yet waits for the relay's first complete set, so that an app opened
for the first time gets its records in one reply.

**A resync is not a change.** When bzz has had to fetch the records again
(the connection came back, or the relay restarted the sync), the script
sees only the real differences, as pushes: a record that did not change is
neither deleted nor created again. A record that is simply gone from the
set is pushed as a delete by whoever signed it last, because nothing says
who deleted it. Without a connection a write is answered `unavailable`.

**What the pane says by itself.** Some things concern the person whatever
the app draws, and bzz says them beside the app: the seconds to wait when
writes are used up, that the app has too many records, that the channel is
archived, that this person cannot read the channel's records. An app need
not repeat them, and may: a script is told each through its replies.

**A new version with other types.** `app.types` of the running version is
what counts. A version that declares a type more gets no push of it before
it lists it; one that drops a type no longer hears of it.

**Content as JSON.** A table with keys 1 to n is a list, any other an
object; an empty table inside content is a list. A nil field is left out;
`data.null` is sent as `null`. A null that arrives is nil, never
`data.null`, so `data.null` belongs only in content that is sent. A whole
number arrives as a Lua integer, any other as a float. Text must be valid
UTF-8: cut a string with `ui.truncate` or `utf8.offset`, not by bytes.

**The title.** Other clients list a record by its title. bzz takes
`content.title` when it is a string with something to show: control
characters become spaces, the ends are trimmed, and it is
cut to 512 bytes. Otherwise the title is the app's name and the type's,
such as `board card`. `match` looks at the same text.

## The host

- `host.quit()` closes the app after the handler returns. In bzz's
  window that is `:app close`: the pane closes and the app is no longer
  open in that channel; its records stay.
- `host.log(...)` writes a line to the app's log. `print` is the same.
- `host.after(ms, tag)` calls `app.timer(state, tag, ctx)` once, after `ms`
  milliseconds. `tag` is plain data.
- `host.time()` is the seconds since 1970-01-01 UTC, with fractions.
- `host.name(id)` is the name bzz knows for a participant's id, or a short
  form of the id (`c0c0c0c0…c0c0`) while it knows none. It is how an app
  shows a person it stores by id (`voter = ctx.user.id`) and who has not
  signed a record it holds. bzz then looks the name up, as it does for an
  author in the conversation, and when it learns it, the app is drawn
  again (`app.view` is called; no handler is). Call it from `view` and
  from handlers alike; it never waits. An id is a string of at most 256
  bytes.

## When a call fails or is stopped

- **An error** in a handler is shown on the last row or two of the app's own
  region, as `file:line: message`, and written to the log. It goes with the
  next key or paste. The app goes on. Every error a script catches with
  `pcall` is a string.
- **A stopped call.** A call that runs longer than its time is stopped where
  it is, and no `pcall` catches that. What it had changed stays changed,
  what it had not reached did not happen, and what it had asked for (a
  request, a timer) is still done. A handler that must not be left half done
  computes first and assigns last.
- **Giving up.** When the same handler is stopped three times in a row, bzz
  stops calling the app until it is reloaded. It then starts with
  `app.init`: the state is gone.
- **Memory.** An allocation past the limit is an error in the call that
  made it. The app goes on with the memory it has.
- **A bad screen.** When `app.view` fails or returns what is not a screen,
  the last good screen stays, with the error.
- **Reload.** A new version replaces the script in place. The state is kept
  (functions and metatables in it are dropped, and a state nested deeper
  than 128 tables is not kept), and so are requests that wait for a reply
  and timers. `app.init` is not called again, so a field the new version
  adds is nil: treat nil as its default, or set it in `app.reload`. A
  version that fails to load leaves the old one running, with the error.

## Limits

| Limit | Value |
|---|---|
| Time for one call | 250 ms |
| Memory | 64 MiB |
| Script | 128 KiB |
| Elements in a screen | 10000 |
| Text in a screen | 128 KiB |
| Nesting of a screen | 32 levels |
| Timers waiting | 32 |
| A timer | 100 ms to 86400000 ms |
| Requests waiting for a reply | 64 |
| Requests in a minute | 600 |
| Log lines in a minute | 120, of 1 KiB each |
| A tag | 4 KiB |
| A `match` | 256 bytes |
| A paste | 64 KiB |
| A string given to `ui.width`, `ui.truncate`, `ui.edit`, `ui.paste` | 64 KiB |
| Record types of one app | 16 |
| Content of one record | 16384 bytes of JSON |
| Nesting of content | 32 levels |
| Live records of one app | 1000 |
| Writes in a minute | 30 for the apps of one person: half of the 60 the relay gives that person, chat included |

Every element, line, span, table cell, bar and sparkline point counts as an
element of a screen. Past a limit the call that passed it gets an error that
names the limit; a write past the limit of records or of writes is answered
`limit` or `rate_limited`. The relay has one budget of writes for a person,
which that person's chat spends too; the apps of one person share half of
it, so that no app can keep its owner from sending a message. Content
another client wrote can be larger than bzz lets an app write. A thousand
records of text fit in the memory; a thousand records that are each a long
list of small values do not, and the app that lists them gets a memory
error.

## What a script has, and has not

**Globals:** `_G`, `_VERSION`, `assert`, `data`, `error`, `getmetatable`, `host`, `ipairs`, `math`, `next`, `pairs`, `pcall`, `print`, `rawequal`, `rawget`, `rawlen`, `rawset`, `select`, `setmetatable`, `string`, `table`, `tonumber`, `tostring`, `type`, `ui`, `utf8`, `xpcall`

`string`, `table`, `math` and `utf8` are Lua's, without `string.dump`.
There is no `io`, `os`, `package`, `require`, `load`, `loadfile`, `dofile`,
`debug`, `coroutine` or `collectgarbage`: a script has no file, network,
process, environment variable, native code or bytecode to reach, and no
other app.

Some of Lua's functions are bzz's own versions, so that every call can be
stopped: the pattern functions, `string.rep` and `string.format`, the
`table` functions, `tostring`, `ipairs`, `pcall`, `xpcall` and
`setmetatable`. They answer as Lua's do, with these differences:

| In Lua | Here | Instead |
|---|---|---|
| `__gc` finalizers | `setmetatable` refuses a metatable with `__gc` | Release what must be released in the handler that is done with it. |
| The message handler of `xpcall` runs where the error is raised | It runs after the stack has unwound | The message already has the file and the line; there is no stack to walk. |
| `table.sort` is not stable, and can raise "invalid order function" | It is stable, and never raises that | Nothing to do; a comparison must still be consistent. |
| `table.unpack` of about a million values | Fewer than 999,000 | Use a loop. |
| A pattern runs as long as it takes | A pattern that runs past the call's time is an error | Search plain text with `s:find(q, 1, true)`; keep patterns simple. |
| `string.rep("", n)` loops `n` times | Returns `""` at once | Nothing to do. |
| A coroutine | None | Keep the step in the state and go on in the next handler or a timer. |
| `load`, `require` | None | One file; a table of functions is a module. |
| `os.time`, `os.clock`, `os.date` | None | `host.time()` and `host.after`. |
| `print` writes to the terminal | It writes to the app's log | |
| An error message of a library function is adjusted for a method call (`s:rep()`) | It names the function as it is | |

## Pitfalls

- Lua counts from 1, and `selected` does too. `0` and `""` are true.
  Not-equal is `~=`, and `..` joins strings.
- `string.find(s, q)` reads `q` as a pattern. For text a person typed use
  `s:lower():find(q:lower(), 1, true)`.
- A handler that returns `ui.text { ... }` by mistake changes nothing: only
  `app.view` draws.
- Handle `q` yourself with `host.quit()`, and not while a field is open.
- Two versions of an app can be open at once in one channel, on two
  machines. A version that does not know a field must keep it when it
  writes: copy `record.content` and change what it means to change.
