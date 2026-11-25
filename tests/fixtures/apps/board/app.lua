-- Board: shared cards in three columns.
-- A card is a record of type "card" with content
--   { title = string, column = "todo" | "doing" | "done", order = number }.
-- Keys: j/k and arrows move the selection, h/l change column, ] and [ move
-- the card, n new card, / search, r reload, q close.

local app = {}
app.api = 1
app.types = { "card" }

local COLS = { "todo", "doing", "done" }
local NAMES = { todo = "To do", doing = "Doing", done = "Done" }
local MIN_W, MIN_H = 40, 16
-- From this width the three columns stand side by side; below it they are
-- stacked, so that a title of 16 characters always fits.
local WIDE = 60

local function trim(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end

-- Any member of the channel can write any field: read them with care.
local function title(card)
  local t = card.content.title
  return type(t) == "string" and t or "(no title)"
end

local function matches(s, card)
  return s.query == "" or title(card):lower():find(s.query:lower(), 1, true) ~= nil
end

-- A list arrives in no order, and ids do not sort by creation: the order
-- is a field of the card.
local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id
end

local function copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end

-- Cards of each column, in their order.
local function columns(s)
  local by = { todo = {}, doing = {}, done = {} }
  for _, c in ipairs(s.cards) do
    local list = by[c.content.column]
    if list then list[#list + 1] = c end
  end
  return by
end

-- Column number and index of the selected card, or nil.
local function where(s)
  local by = columns(s)
  for ci, name in ipairs(COLS) do
    for i, c in ipairs(by[name]) do
      if c.id == s.sel then return ci, i, by end
    end
  end
  return nil, nil, by
end

local function select_first(s)
  local by = columns(s)
  s.sel = nil
  for _, name in ipairs(COLS) do
    if by[name][1] then s.sel = by[name][1].id return end
  end
end

local function keep_selection(s)
  if not where(s) then select_first(s) end
end

local function load(s)
  s.list_id = data.list { type = "card", match = s.query, tag = "load" }
  s.touched = {} -- ids changed since this list was asked for
  s.status = "Loading..."
end

local function index_of(s, id)
  for i, c in ipairs(s.cards) do
    if c.id == id then return i end
  end
end

local function put(s, rec)
  local i = index_of(s, rec.id)
  if i then s.cards[i] = rec else s.cards[#s.cards + 1] = rec end
  table.sort(s.cards, by_order)
  if s.touched then s.touched[rec.id] = true end
end

local function remove(s, id)
  local i = index_of(s, id)
  if i then table.remove(s.cards, i) end
  if s.touched then s.touched[id] = true end
end

-- A list that was asked for before a change must not undo it: a card that
-- changed while the list was on its way keeps what the app has.
local function merge(s, records)
  local touched, out = s.touched or {}, {}
  for _, rec in ipairs(records) do
    if not touched[rec.id] then out[#out + 1] = rec end
  end
  for _, rec in ipairs(s.cards) do
    if touched[rec.id] then out[#out + 1] = rec end
  end
  table.sort(out, by_order)
  s.touched = {}
  return out
end

function app.init(ctx)
  local s = {
    cards = {}, sel = nil, query = "", search_open = false, form = nil, status = "",
    undo = {}, -- request id -> { id = record id, content = what it was }
    pending = {}, -- record id -> writes of this app to it not answered yet
    gone = {}, gones = 0, -- record id -> who deleted it, of the last few
  }
  load(s)
  return s
end

local function too_small(ctx)
  return ctx.width < MIN_W or ctx.height < MIN_H
end

-- Rows for the three lists when the columns are stacked: one row at a time
-- to each column that still has a card to show, until the rows are used.
local function share(counts, rows)
  local given = { 0, 0, 0 }
  local moved = true
  while rows > 0 and moved do
    moved = false
    for i = 1, 3 do
      if rows > 0 and given[i] < counts[i] then
        given[i] = given[i] + 1
        rows = rows - 1
        moved = true
      end
    end
  end
  return given
end

function app.view(s, ctx)
  if too_small(ctx) then
    return ui.text { "Terminal too small. Need " .. MIN_W .. "x" .. MIN_H .. "." }
  end
  local by = columns(s)
  local wide = ctx.width >= WIDE
  local searching = s.search_open or s.query ~= ""
  local none = #s.cards == 0 and s.query ~= "" and s.status ~= "Loading..."
  -- Rows left for cards when stacked: all but three headers, the status
  -- line, and the lines for the search and for "No cards match.".
  local rows = ctx.height - 4 - (searching and 1 or 0) - (none and 1 or 0)
  local given = share({ #by.todo, #by.doing, #by.done }, rows)

  local sections = {}
  for ci, name in ipairs(COLS) do
    local items, selected = {}, nil
    for i, c in ipairs(by[name]) do
      items[i] = title(c)
      if c.id == s.sel then selected = i end
    end
    sections[ci] = ui.column {
      size = (not wide) and (given[ci] + 1) or nil,
      ui.text { NAMES[name] .. " (" .. #items .. ")", bold = true, fg = "accent" },
      ui.list { id = "col" .. ci, items = items, selected = selected, highlight = { bold = true } },
    }
  end

  local parts = {}
  if wide then
    parts[1] = ui.row(sections)
  else
    for i = 1, 3 do parts[i] = sections[i] end
    parts[4] = ui.spacer {}
  end
  if none then parts[#parts + 1] = ui.text { "No cards match." } end
  if searching then
    parts[#parts + 1] = ui.row {
      size = 1,
      ui.text { "Search: ", size = 8 },
      ui.input { value = s.query, cursor = s.search_cursor, focused = s.search_open },
    }
  end
  parts[#parts + 1] = ui.text { s.status, fg = "muted" }
  local board = ui.column(parts)
  if not s.form then return board end
  local f = s.form
  return ui.layers {
    board,
    ui.popup {
      width = math.min(50, ctx.width - 2), height = 6,
      ui.text { "New card", bold = true },
      ui.row { ui.text { "Title: ", size = 7 }, ui.input { value = f.value, cursor = f.cursor, focused = true } },
      ui.text { f.error or "", fg = "error" },
      ui.text { "Enter save  Esc cancel", fg = "muted" },
    },
  }
end

-- One more write of ours to the card `id` is on its way.
local function on_its_way(s, id)
  s.pending = s.pending or {}
  s.pending[id] = (s.pending[id] or 0) + 1
end

local function move(s, dir)
  local ci, i, by = where(s)
  if not ci then return end
  local target = COLS[ci + dir]
  if not target then return end
  local card = by[COLS[ci]][i]
  local before = card.content
  local content = copy(before)
  content.column = target
  local id = data.update { record_id = card.id, rev = card.rev, content = content, tag = "move" }
  s.undo[id] = { id = card.id, content = before }
  on_its_way(s, card.id)
  -- Shown in its new column at once; undone if the write fails.
  card.content = content
  if s.touched then s.touched[card.id] = true end
  s.status = "Saving..."
end

local function step(s, dir)
  local ci, i, by = where(s)
  if not ci then return end
  local list = by[COLS[ci]]
  local j = math.max(1, math.min(#list, i + dir))
  s.sel = list[j].id
end

local function side(s, dir)
  local ci, i, by = where(s)
  if not ci then return end
  local c = ci + dir
  while COLS[c] do
    local list = by[COLS[c]]
    if #list > 0 then
      s.sel = list[math.min(i, #list)].id
      return
    end
    c = c + dir
  end
end

local function form_key(s, key)
  local f = s.form
  if key.name == "esc" then
    s.form = nil
  elseif key.name == "enter" then
    if f.saving then return end
    local text = trim(f.value)
    if text == "" then
      f.error = "Title is required."
      return
    end
    f.error = nil
    f.saving = true
    data.create {
      type = "card",
      content = { title = text, column = "todo", order = host.time() },
      tag = "create",
    }
    s.status = "Saving..."
  else
    ui.edit(f, key)
  end
end

local function search_key(s, key)
  if key.name == "enter" then
    s.search_open = false
  elseif key.name == "esc" then
    s.search_open = false
    s.query = ""
    load(s)
  else
    local before = s.query
    local field = { value = s.query, cursor = s.search_cursor }
    ui.edit(field, key)
    s.query, s.search_cursor = field.value, field.cursor
    if s.query ~= before then load(s) end
  end
end

function app.key(s, key, ctx)
  if too_small(ctx) then
    if key.name == "q" and not s.form and not s.search_open then host.quit() end
    return
  end
  if s.form then return form_key(s, key) end
  if s.search_open then return search_key(s, key) end
  local n = key.name
  if n == "q" then host.quit()
  elseif n == "j" or n == "down" then step(s, 1)
  elseif n == "k" or n == "up" then step(s, -1)
  elseif n == "h" or n == "left" then side(s, -1)
  elseif n == "l" or n == "right" then side(s, 1)
  elseif n == "]" then move(s, 1)
  elseif n == "[" then move(s, -1)
  elseif n == "n" then s.form = { value = "" }
  elseif n == "/" then s.search_open = true; s.search_cursor = nil
  elseif n == "esc" and s.query ~= "" then s.query = ""; load(s)
  elseif n == "r" then load(s)
  end
end

function app.paste(s, text, ctx)
  if s.form then
    ui.paste(s.form, text)
  elseif s.search_open then
    local field = { value = s.query, cursor = s.search_cursor }
    ui.paste(field, text)
    s.query, s.search_cursor = field.value, field.cursor
    load(s)
  end
end

-- What a failed write says on the status line. `rate_limited` is bzz's own
-- answer, before anything was sent: an app may write about 30 times a
-- minute, and `retry_after` says in how many seconds it may again. Nothing
-- was written; the person decides whether to try again.
local function failure(e)
  if e.code == "rate_limited" then
    return "Too many changes. Retry in " .. e.retry_after .. " s."
  elseif e.code == "refused" then
    return "Refused: " .. e.message
  elseif e.code == "limit" then
    return "The board is full."
  elseif e.code == "not_found" then
    return "This card was deleted meanwhile."
  end
  return "Save failed."
end

-- Cards shown: those of the three columns.
local function shown(s)
  local by = columns(s)
  return #by.todo + #by.doing + #by.done
end

-- A write of ours to `id` was answered: true while another is on its way.
local function answered(s, id)
  s.pending = s.pending or {}
  local left = (s.pending[id] or 1) - 1
  s.pending[id] = left > 0 and left or nil
  return left > 0
end

local function deleted_by(name)
  return "This card was deleted by " .. name .. "."
end

function app.reply(s, r, ctx)
  if r.tag == "load" then
    if r.id ~= s.list_id then return end -- an older search: ignore it
    if not r.ok then
      s.status = "Load failed. Press r to retry."
      return
    end
    s.cards = merge(s, r.records)
    keep_selection(s)
    s.status = "Loaded " .. shown(s) .. " cards."
  elseif r.tag == "create" then
    if not r.ok then
      if s.form then s.form.saving = false end
      s.status = failure(r.error)
      return
    end
    s.form = nil
    if matches(s, r.record) then
      put(s, r.record)
      s.sel = r.record.id
    end
    s.status = "Saved."
  elseif r.tag == "move" or r.tag == "merge" then
    local undo = s.undo[r.id]
    s.undo[r.id] = nil
    local more = undo and answered(s, undo.id)
    if r.ok then
      -- While a later move of ours is on its way the card stays as shown:
      -- bzz sends that one on the revision this one made.
      if more then
        s.status = "Saving..."
      else
        put(s, r.record)
        s.status = r.tag == "merge" and "Conflict: merged with a newer change." or "Saved."
      end
    elseif r.error.code == "conflict" then
      -- Keep their change, put ours (the column) on top, and try again.
      local content = r.current.content
      local theirs = copy(content)
      content.column = r.request.content.column
      put(s, r.current)
      local id = data.update { record_id = r.current.id, rev = r.current.rev, content = content, tag = "merge" }
      s.undo[id] = { id = r.current.id, content = theirs }
      on_its_way(s, r.current.id)
    else
      -- The card goes back where it was.
      local i = undo and index_of(s, undo.id)
      if i then
        s.cards[i].content = undo.content
        table.sort(s.cards, by_order)
      end
      s.status = failure(r.error)
      if r.error.code == "not_found" and undo then
        remove(s, undo.id)
        -- Who deleted it, if bzz said so already; if not, when it does.
        local who = s.gone and s.gone[undo.id]
        if who then s.status = deleted_by(who) else s.lost = undo.id end
      end
      keep_selection(s)
    end
  end
end

-- Someone else changed a record. A push is a real change only: when bzz
-- fetches the channel's records again (a resync) the app hears of what
-- differs, never of everything as deleted and made again. A push of
-- `update` with the `rev` the app has means only that bzz learnt a name.
function app.push(s, p, ctx)
  local rec = p.record
  local i = index_of(s, rec.id)
  if p.change == "delete" then
    -- A state carried from a version before these fields has none.
    if not s.gone or (s.gones or 0) >= 64 then s.gone, s.gones = {}, 0 end
    s.gone[rec.id], s.gones = p.by.name, s.gones + 1
    if s.lost == rec.id then
      s.status, s.lost = deleted_by(p.by.name), nil
    end
    local ci, pos = where(s)
    local was_selected = i and s.sel == rec.id
    remove(s, rec.id)
    if was_selected then
      -- The card now at the same place in its column, if there is one.
      local list = columns(s)[COLS[ci]]
      s.sel = list[pos] and list[pos].id or nil
    end
  elseif matches(s, rec) then
    put(s, rec)
  else
    remove(s, rec.id) -- it no longer matches the search
  end
  keep_selection(s)
end

return app
