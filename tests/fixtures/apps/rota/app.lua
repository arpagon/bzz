-- Rota: who covers each day of one week.
-- A day is a record of type "day" with content
--   { day = string, holder = string or null, order = number }.
-- `holder` is the name of whoever covers the day; null means it is free.
-- Keys: j/k and arrows move, t take, g give up (Enter confirms, Esc
-- cancels), f filter all/free/mine, r reload, q close.

local app = {}
app.api = 1
app.types = { "day" }

local FILTERS = { "all", "free", "mine" }

-- Any member of the channel can write any field: read them with care.
local function holder_of(rec)
  local h = rec.content.holder
  if type(h) == "string" and h ~= "" then return h end
  return nil
end

local function label(rec)
  local d = rec.content.day
  return type(d) == "string" and d or "?"
end

-- A list arrives in no order, and ids do not sort by creation: the place
-- of a day in the week is a field of the record.
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

local function load(s)
  s.list_id = data.list { type = "day", tag = "load" }
  s.touched = {} -- ids changed since this list was asked for
  s.status = "Loading..."
end

function app.init(ctx)
  local s = {
    days = {}, sel = nil, filter = 1, loaded = false,
    saving = false, confirm = nil, status = "",
  }
  load(s)
  return s
end

local function shown(s, me)
  local out = {}
  local f = FILTERS[s.filter] or "all"
  for _, d in ipairs(s.days) do
    local h = holder_of(d)
    if f == "all" or (f == "free" and h == nil) or (f == "mine" and h == me) then
      out[#out + 1] = d
    end
  end
  return out
end

local function index_of(list, id)
  for i, d in ipairs(list) do
    if d.id == id then return i end
  end
  return nil
end

-- The selected day stays selected while it is shown; else the first shown.
local function fix_selection(s, me)
  local list = shown(s, me)
  if s.sel and index_of(list, s.sel) then return end
  if list[1] then s.sel = list[1].id end
end

local function put(s, rec)
  local i = index_of(s.days, rec.id)
  if i then s.days[i] = rec else s.days[#s.days + 1] = rec end
  table.sort(s.days, by_order)
  if s.touched then s.touched[rec.id] = true end
end

local function remove(s, id)
  local i = index_of(s.days, id)
  if i then table.remove(s.days, i) end
  if s.touched then s.touched[id] = true end
  if s.confirm and s.confirm.id == id then s.confirm = nil end
end

-- A list that was asked for before a change must not undo it: a day that
-- changed while the list was on its way keeps what the app has.
local function merge(s, records)
  local touched, out = s.touched or {}, {}
  for _, rec in ipairs(records) do
    if not touched[rec.id] then out[#out + 1] = rec end
  end
  for _, rec in ipairs(s.days) do
    if touched[rec.id] then out[#out + 1] = rec end
  end
  table.sort(out, by_order)
  s.touched = {}
  return out
end

function app.view(s, ctx)
  local me = ctx.user.name
  local list = shown(s, me)
  local width = 3
  for _, d in ipairs(s.days) do width = math.max(width, ui.width(label(d))) end
  local items = {}
  for i, d in ipairs(list) do
    local l = label(d)
    items[i] = l .. string.rep(" ", width - ui.width(l) + 2) .. (holder_of(d) or "free")
  end

  local counts, names = {}, {}
  for _, d in ipairs(s.days) do
    local h = holder_of(d)
    if h then
      if not counts[h] then names[#names + 1] = h; counts[h] = 0 end
      counts[h] = counts[h] + 1
    end
  end
  table.sort(names)
  local held = {}
  for i, n in ipairs(names) do held[i] = n .. " " .. counts[n] end

  local parts = { ui.text { "Show: " .. (FILTERS[s.filter] or "all"), bold = true, fg = "accent" } }
  if s.loaded and #list == 0 then
    parts[#parts + 1] = ui.text { "No days match.", size = "fill" }
  else
    parts[#parts + 1] = ui.list {
      id = "days", items = items, selected = index_of(list, s.sel),
      highlight = "selection", size = "fill",
    }
  end
  if s.confirm then
    parts[#parts + 1] = ui.text { "Give up " .. s.confirm.day .. "?", fg = "warning", bold = true }
    parts[#parts + 1] = ui.text { "Enter confirm  Esc cancel", fg = "warning" }
  end
  parts[#parts + 1] = ui.text { "Days: " .. (#held > 0 and table.concat(held, ", ") or "none") }
  parts[#parts + 1] = ui.text { s.status, fg = "muted" }
  return ui.column(parts)
end

-- An update replaces the whole content: copy it, so that the day keeps its
-- place and any field another version of the app added.
local function send(s, rec, holder, tag)
  local content = copy(rec.content)
  content.holder = holder or data.null
  data.update { record_id = rec.id, rev = rec.rev, content = content, tag = tag }
  s.saving = true
  s.status = "Saving..."
end

function app.key(s, key, ctx)
  local me = ctx.user.name
  local n = key.name
  if s.confirm then
    -- Only Enter and Esc act here.
    if n == "esc" then
      s.confirm = nil
    elseif n == "enter" and not s.saving then
      local i = index_of(s.days, s.confirm.id)
      if i then send(s, s.days[i], nil, "give") else s.confirm = nil end
    end
    return
  end
  if n == "q" then host.quit() return end
  if n == "r" then load(s) return end
  if n == "f" then
    s.filter = (s.filter or 1) % #FILTERS + 1
    fix_selection(s, me)
    return
  end
  local list = shown(s, me)
  local i = index_of(list, s.sel)
  local day = i and list[i]
  if n == "j" or n == "down" then
    if i and i < #list then s.sel = list[i + 1].id end
  elseif n == "k" or n == "up" then
    if i and i > 1 then s.sel = list[i - 1].id end
  elseif n == "t" and day and not s.saving then
    local h = holder_of(day)
    if h == nil then
      send(s, day, me, "take")
    elseif h ~= me then
      s.status = "Held by " .. h .. "."
    end
  elseif n == "g" and day and not s.saving then
    if holder_of(day) == me then
      s.confirm = { id = day.id, day = label(day) }
    else
      s.status = "Not your day."
    end
  end
end

-- The rota has no text field: a paste is ignored everywhere.
function app.paste(s, text, ctx) end

-- What a failed write says on the status line.
local function failure(e)
  if e.code == "rate_limited" then
    return "Too many changes. Retry in " .. e.retry_after .. " s."
  elseif e.code == "refused" then
    return "Refused: " .. e.message
  end
  return "Save failed."
end

function app.reply(s, r, ctx)
  local me = ctx.user.name
  if r.tag == "load" then
    if r.id ~= s.list_id then return end -- a newer list was asked for
    if not r.ok then
      s.status = "Load failed. Press r to retry."
      return
    end
    s.days = merge(s, r.records)
    s.loaded = true
    s.status = "Loaded " .. #r.records .. " days."
    fix_selection(s, me)
    return
  end
  if r.tag ~= "take" and r.tag ~= "give" then return end
  s.saving = false
  if r.ok then
    put(s, r.record)
    if r.tag == "take" then
      s.status = "Took " .. label(r.record) .. "."
    else
      s.confirm = nil
      s.status = "Gave up " .. label(r.record) .. "."
    end
  elseif r.error.code == "conflict" then
    -- Someone changed the day first. A rota never overwrites: whoever
    -- took the day has it.
    local now = r.current
    put(s, now)
    local h = holder_of(now)
    if r.tag == "take" then
      if h == nil then
        send(s, now, me, "take") -- changed, but still free: take it
      elseif h == me then
        s.status = "Took " .. label(now) .. "."
      else
        s.status = label(now) .. " was taken by " .. h .. "."
      end
    elseif h == me then
      send(s, now, nil, "give") -- changed, but still ours: give it up
    else
      s.confirm = nil
      s.status = "Save failed."
    end
  else
    if r.error.code == "not_found" then remove(s, r.request.record_id) end
    s.confirm = nil
    s.status = failure(r.error)
  end
  fix_selection(s, me)
end

function app.push(s, p, ctx)
  if p.change == "delete" then
    remove(s, p.record.id)
  else
    put(s, p.record)
  end
  fix_selection(s, ctx.user.name)
end

return app
