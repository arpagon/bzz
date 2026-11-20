# Writing an app for bzz

An app is one Lua 5.4 script that bzz runs inside itself, beside a
channel's conversation, in a pane of **40 columns by 16 rows**. The script
returns elements and reacts to events; bzz draws them, owns the terminal,
and signs the records the people of the channel share through the app. The
whole API is the contract, `docs/app-api.md` (`api = 1`), which `app_guide`
returns after this page. Read both before you write a line.

## The shape

```lua
-- tally: a shared list of named counters. n new, j/k move, + and - count.
local app = { api = 1, types = { "counter" } }   -- api and the fence: required

-- A list arrives in no order: the order is a field of the record.
local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id
end

local function copy(t) local c = {} for k, v in pairs(t) do c[k] = v end return c end

function app.init(ctx)
  data.list { type = "counter", tag = "load" }    -- the first list: pushes begin
  return { items = {}, sel = 1, form = nil, status = "Loading..." }
end

local function bump(s, by)
  local item = s.items[s.sel]
  if not item then return end
  local content = copy(item.content)              -- keep fields you do not know
  content.count = (tonumber(content.count) or 0) + by
  data.update { record_id = item.id, rev = item.rev, content = content, tag = { bump = by } }
end

function app.key(s, key, ctx)
  if s.form then
    if key.name == "esc" then s.form = nil
    elseif key.name == "enter" and s.form.value ~= "" then
      data.create { type = "counter", tag = "create",
        content = { title = s.form.value, count = 0, order = host.time(), by = ctx.user.id } }
      s.form = nil
    else ui.edit(s.form, key) end
    return                                       -- q types a q while a field is open
  end
  if key.name == "q" then host.quit()
  elseif key.name == "n" then s.form = { value = "" }
  elseif key.name == "j" then s.sel = math.min(s.sel + 1, #s.items)
  elseif key.name == "k" then s.sel = math.max(s.sel - 1, 1)
  elseif key.name == "+" then bump(s, 1)
  elseif key.name == "-" then bump(s, -1) end
end

function app.paste(s, text, ctx) if s.form then ui.paste(s.form, text) end end

local function put(s, record)
  for i, item in ipairs(s.items) do
    if item.id == record.id then s.items[i] = record return end
  end
  s.items[#s.items + 1] = record
end

function app.reply(s, r, ctx)
  if not r.ok then
    local code = r.error.code
    if code == "conflict" then                  -- someone counted first: count on top
      local content = copy(r.current.content)
      content.count = (tonumber(content.count) or 0) + r.tag.bump
      data.update { record_id = r.current.id, rev = r.current.rev, content = content, tag = r.tag }
    elseif code == "rate_limited" then
      s.status = "Too many changes: wait " .. r.error.retry_after .. " s"
    else
      s.status = code .. ": " .. r.error.message
    end
    return
  end
  if r.tag == "load" then s.items = r.records
  elseif r.record then put(s, r.record) end       -- your own writes come back here only
  table.sort(s.items, by_order)
  s.status = #s.items .. " counters"
end

function app.push(s, p, ctx)                      -- another participant's change
  if p.change == "delete" then
    for i, item in ipairs(s.items) do
      if item.id == p.record.id then table.remove(s.items, i) break end
    end
  else put(s, p.record) end
  table.sort(s.items, by_order)
  s.sel = math.max(1, math.min(s.sel, #s.items))
end

function app.view(s, ctx)
  local rows, room = {}, math.max(ctx.width - 8, 4)
  for _, item in ipairs(s.items) do
    local name = ui.truncate(tostring(item.content.title or "(no title)"), room)
    rows[#rows + 1] = name .. string.rep(" ", room - ui.width(name)) .. string.format(" %4d", tonumber(item.content.count) or 0)
  end
  local made = s.items[s.sel] and s.items[s.sel].content.by
  local main = ui.column {
    ui.text { "Tally", bold = true },
    ui.list { items = rows, selected = s.sel, id = "items" },
    ui.text { made and ("made by " .. host.name(made)) or "", fg = "muted" },
    ui.text { s.status .. "  n new  + -  q", fg = "muted" },
  }
  if not s.form then return main end
  return ui.layers { main, ui.popup { title = " New counter ", width = 30, height = 3,
    ui.input { value = s.form.value, cursor = s.form.cursor, focused = true } } }
end

return app                                        -- the script ends with this
```

1. The top level runs at load and at every reload: it only defines.
2. `app.init(ctx)` returns the state; send the first `data.list` of each
   type there. Handlers change the state in place; only `app.view` draws,
   and it only builds elements.
3. `ctx` is `{ width, height, user }`; `user` is `{ id, name }`.

## Records

- Declare every type in `app.types` (1 to 16 names, `[a-z][a-z0-9_-]{0,31}`,
  short: `card`, not `bzz.app.board.card`). Another type is an error.
- A record is `{ id, type, rev, content, by }`. `rev` is opaque: pass it back.
  `data.update` replaces the **whole** content: copy `record.content` and
  change your fields, so that a field another version wrote survives.
- **Order** is a field (`order = host.time()` when created) and you sort by
  it. Ids do not sort by creation, and a list comes in no order.
- **Conflicts.** On `conflict`, `r.current` is the record now. What it means
  is your rule, and the brief says it: a board or a counter merges on top; a
  vote or an approval must not overwrite.
- **Rate limit.** An owner's apps share 30 writes a minute. On
  `rate_limited` say on screen how long to wait (`r.error.retry_after`).
  Write on a key, never on a timer or in a loop.
- Your own writes come back in `app.reply`, never as pushes. Pushes of a
  type begin with your first list of it.
- `data.null` sends a JSON `null`; a `null` that arrives is `nil`.

## People: ids and `host.name`

Store a person's `ctx.user.id` (or `record.by.id`), never a name, and show
it with `host.name(id)`, which may be a short form of the id until bzz
learns the name. Anyone in the channel can write any field of any record:
a rule such as "not your own request" is a convention, not a lock.

## The pane: 40 by 16

Lay out from `ctx.width` and `ctx.height`. At 40 columns stack with
`ui.column`, shorten labels and drop table columns; from about 60 put
things side by side. Nothing is drawn outside the region; cut text with
`ui.truncate`. A form is `ui.popup` last in `ui.layers`. Ctrl-c is bzz's;
every other key is yours while the app has the focus.

## Reload, and a state from an older version

A new version replaces the script in place: the state, waiting requests
and timers are kept, and `app.init` is not called again. A field your
version adds is `nil` at first: treat `nil` as its default, or set it in
`app.reload`. When a field changes meaning, migrate it there:

```lua
local app = { api = 1, types = { "card" } }
function app.init(ctx) return { filter = "all", cards = {} } end
-- The version before kept `filter` as a number of columns: start it again.
function app.reload(s, ctx)
  if type(s.filter) ~= "string" then s.filter = "all" end
  if s.cards == nil then return app.init(ctx) end   -- or start afresh
end
function app.view(s, ctx) return ui.text { "filter: " .. s.filter } end
return app
```

A version that does not load leaves the old one running, with its error.

## What is not there

No `io`, `os`, `require`, `load`, `debug`, `coroutine`: no file, network,
process or environment, no other app. Time is `host.time()`; a timer is
`host.after(ms, tag)` (100 ms at least) and calls `app.timer`. A call has
250 ms; a pattern on user text is `s:find(q, 1, true)`. Memory is 64 MiB,
a script 128 KiB, an app 1000 live records of 16 KiB each. `print` goes to
the app's log. Lua counts from 1; `0` and `""` are true; not-equal is `~=`.

## Try it, then propose it

- `try_app` loads your script apart at 40×16 (or the size you give), does
  the keys you list (`j`, `enter`, `type:milk`, `paste:…`, `wait:500`,
  `resize:60x20`), and answers the screen as text and any error as
  `file:line: message`. Give it records, or `from_app` to start from a copy
  of the channel's records of an app open here. Nothing is stored. Try the
  first screen, every key you added, a narrow and a wide size, and a reload
  of your version over the old one's state when you change an app.
- `apps` with `app` gives an installed app's script, manifest and brief:
  start a change from the version that runs, and keep what you do not mean
  to change.
- `propose_app` sends the whole script and its brief. It is loaded apart
  first: an error comes back at once and the owner is not asked. Otherwise
  the owner approves or refuses these exact bytes; you are told which, with
  his reason, at your next turn in the channel.

## The brief

The brief is what the owner reads before he approves, and what an agent of
another client, Buzz included, needs to write the same app over the same
records. Keep it true to the script, every version. It says, in plain text:

1. What the app does, in a sentence or two, and its keys.
2. Each record type, by its name, with a table of its fields: name, type,
   meaning, and which are required. bzz refuses a brief that does not name
   every type the script declares.
3. The rules another client must keep: what a conflict does, what is never
   deleted, fields kept as they were read.
4. What changed from the version before, when you change an app.
