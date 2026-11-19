-- Panel: a shared panel of metrics, with a table that sorts, a tab of the
-- metrics in alert, the history and a gauge of the selected one, and an
-- acknowledgement with a note.
-- A metric is a record of type "metric" with content
--   { name = string, unit = string, status = "ok" | "alert" | "acked",
--     max = integer, history = { integer, ... }, acked_by = string or null,
--     note = string or null }.
-- The last number of `history` is the metric's value. `acked_by` is the
-- name of a participant.
-- Keys: j/k and arrows move, Tab switches All/Alerts, s next sort column,
-- d reverses it, a acknowledge (asks for a note), r reload, q close.

local app = {}
app.api = 1
app.types = { "metric" }

local SORTS = { "Name", "Value", "Status" }

local function trim(t) return (t:match("^%s*(.-)%s*$")) end

-- Any member of the channel can write any field: read them with care.
local function text(v)
  return type(v) == "string" and v or ""
end

local function history_of(rec)
  local out = {}
  local h = rec.content.history
  if type(h) ~= "table" then return out end
  for _, v in ipairs(h) do
    out[#out + 1] = math.max(0, math.floor(tonumber(v) or 0))
  end
  return out
end

local function value_of(rec)
  local h = history_of(rec)
  return h[#h] or 0
end

local function copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end

local function load(s)
  s.list_id = data.list { type = "metric", tag = "load" }
  s.touched = {} -- ids changed since this list was asked for
  s.status = "Loading..."
end

function app.init(ctx)
  local s = {
    metrics = {}, loaded = false,
    tab = "all",     -- "all" | "alerts"
    sort = 1,        -- index into SORTS
    desc = false,
    sel = nil,       -- id of the selected metric
    form = nil,      -- { id, name, value, cursor, error, saving, note }
    status = "",
  }
  load(s)
  return s
end

local function index_of(list, id)
  for i, m in ipairs(list) do
    if m.id == id then return i end
  end
  return nil
end

-- The rows of the active tab, sorted. The order is the user's choice, so
-- these records need no order field of their own.
local function visible(s)
  local rows = {}
  for _, m in ipairs(s.metrics) do
    if s.tab ~= "alerts" or m.content.status == "alert" then rows[#rows + 1] = m end
  end
  local col, desc = SORTS[s.sort] or "Name", s.desc
  table.sort(rows, function(a, b)
    local ka, kb
    if col == "Value" then ka, kb = value_of(a), value_of(b)
    elseif col == "Status" then ka, kb = text(a.content.status), text(b.content.status)
    else ka, kb = text(a.content.name), text(b.content.name) end
    if ka ~= kb then
      if desc then return ka > kb end
      return ka < kb
    end
    local na, nb = text(a.content.name), text(b.content.name)
    if na ~= nb then return na < nb end
    return a.id < b.id
  end)
  return rows
end

-- The visible rows and the index of the selected one: the first row when
-- the selected metric is not shown. It changes nothing, so the view may
-- call it.
local function selection(s)
  local rows = visible(s)
  if #rows == 0 then return rows, nil end
  return rows, index_of(rows, s.sel) or 1
end

-- The same, and the selection remembered.
local function fix_selection(s)
  local rows, i = selection(s)
  if i then s.sel = rows[i].id end
  return rows, i
end

local function put(s, rec)
  local i = index_of(s.metrics, rec.id)
  if i then s.metrics[i] = rec else s.metrics[#s.metrics + 1] = rec end
  if s.touched then s.touched[rec.id] = true end
end

local function remove(s, id)
  local i = index_of(s.metrics, id)
  if i then table.remove(s.metrics, i) end
  if s.touched then s.touched[id] = true end
  if s.form and s.form.id == id then s.form = nil end
end

-- A list that was asked for before a change must not undo it: a metric
-- that changed while the list was on its way keeps what the app has.
local function merge(s, records)
  local touched, out = s.touched or {}, {}
  for _, rec in ipairs(records) do
    if not touched[rec.id] then out[#out + 1] = rec end
  end
  for _, rec in ipairs(s.metrics) do
    if touched[rec.id] then out[#out + 1] = rec end
  end
  s.touched = {}
  return out
end

-- The acknowledgement on top of the metric as it is: an update replaces
-- the whole content, so its history and everything else are copied.
local function acknowledged(content, note, me)
  local c = copy(content)
  c.status = "acked"
  c.acked_by = me
  c.note = note
  return c
end

function app.view(s, ctx)
  local rows, i = selection(s)
  local alerts = 0
  for _, m in ipairs(s.metrics) do
    if m.content.status == "alert" then alerts = alerts + 1 end
  end
  local all_tab = "All (" .. #s.metrics .. ")"
  local alerts_tab = "Alerts (" .. alerts .. ")"
  if s.tab == "alerts" then alerts_tab = "*" .. alerts_tab else all_tab = "*" .. all_tab end

  local parts = {
    ui.text { ui.line { all_tab, "  ", alerts_tab }, fg = "accent" },
    ui.text { "Sort: " .. (SORTS[s.sort] or "Name") .. " " .. (s.desc and "desc" or "asc") },
  }

  if s.loaded and #rows == 0 then
    parts[#parts + 1] = ui.column { size = "fill",
      ui.text { "  Name       Value  Unit  Status", bold = true },
      ui.text { "No metrics match." },
    }
  else
    local cells = {}
    if s.loaded then
      for n, m in ipairs(rows) do
        local c = m.content
        cells[n] = { text(c.name), tostring(value_of(m)), text(c.unit), text(c.status) }
      end
    end
    parts[#parts + 1] = ui.table {
      id = "metrics", size = "fill",
      header = { "Name", "Value", "Unit", "Status" },
      rows = cells, selected = s.loaded and i or nil,
      widths = { 11, 6, 5, 7 },
      highlight = "selection", header_style = { bold = true },
    }
  end

  local selected = s.loaded and i and rows[i] or nil
  if selected then
    local c = selected.content
    local value = value_of(selected)
    local max = tonumber(c.max) or 0
    local percent = max > 0 and math.floor(value * 100 / max + 0.5) or 0
    local ratio = max > 0 and math.max(0, math.min(1, value / max)) or 0
    parts[#parts + 1] = ui.text { "History: " .. text(c.name) }
    -- Three rows of history where the region is tall, two in the pane.
    parts[#parts + 1] = ui.sparkline { data = history_of(selected), size = ctx.height >= 24 and 3 or 2, fg = "accent" }
    parts[#parts + 1] = ui.spacer { size = 1 }
    parts[#parts + 1] = ui.gauge { ratio = ratio, label = percent .. "%", size = 1, gauge_style = { fg = "success" } }
  end

  parts[#parts + 1] = ui.text { ui.truncate(s.status, ctx.width), fg = "muted" }
  local main = ui.column(parts)
  if not s.form then return main end

  local f = s.form
  return ui.layers {
    main,
    ui.popup {
      title = ui.truncate("Acknowledge " .. f.name, math.max(1, math.min(46, ctx.width - 4))),
      width = math.min(50, ctx.width - 2), height = 5,
      ui.row { size = 1, ui.text { "Note: ", size = 6 },
        ui.input { value = f.value, cursor = f.cursor, focused = true } },
      ui.text { "Enter save  Esc cancel", fg = "muted" },
      ui.text { f.error or "", fg = "error" },
    },
  }
end

local function form_key(s, key, ctx)
  local f = s.form
  if key.name == "esc" then
    s.form = nil
    return
  end
  if key.name ~= "enter" then
    if not f.saving then ui.edit(f, key) end
    return
  end
  if f.saving then return end -- one confirmation, one write
  local note = trim(f.value or "")
  if note == "" then
    f.error = "Note is required."
    return
  end
  local at = index_of(s.metrics, f.id)
  if not at then
    s.form = nil
    s.status = "Save failed."
    return
  end
  local rec = s.metrics[at]
  f.note, f.error, f.saving = note, nil, true
  data.update {
    record_id = rec.id, rev = rec.rev,
    content = acknowledged(rec.content, note, ctx.user.name), tag = "ack",
  }
  s.status = "Saving..."
end

function app.key(s, key, ctx)
  if s.form then return form_key(s, key, ctx) end
  local n = key.name
  local rows, i = fix_selection(s)
  if n == "q" then host.quit()
  elseif n == "j" or n == "down" then
    if i and i < #rows then s.sel = rows[i + 1].id end
  elseif n == "k" or n == "up" then
    if i and i > 1 then s.sel = rows[i - 1].id end
  elseif n == "tab" then
    s.tab = s.tab == "alerts" and "all" or "alerts"
    fix_selection(s)
  elseif n == "s" then
    s.sort = (s.sort or 1) % #SORTS + 1
    s.desc = false
  elseif n == "d" then
    s.desc = not s.desc
  elseif n == "r" then
    load(s)
  elseif n == "a" then
    local m = i and rows[i]
    if not m or m.content.status ~= "alert" then
      s.status = "Nothing to acknowledge."
    else
      s.form = { id = m.id, name = text(m.content.name), value = "" }
    end
  end
end

function app.paste(s, pasted, ctx)
  if s.form and not s.form.saving then ui.paste(s.form, pasted) end
end

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
  if r.tag == "load" then
    if r.id ~= s.list_id then return end -- a newer list was asked for
    if not r.ok then
      s.status = "Load failed. Press r to retry."
      return
    end
    s.metrics = merge(s, r.records)
    s.loaded = true
    s.status = "Loaded " .. #r.records .. " metrics."
    fix_selection(s)
    return
  end
  if r.tag ~= "ack" and r.tag ~= "merge" then return end
  local f = s.form
  if r.ok then
    put(s, r.record)
    s.form = nil
    s.status = r.tag == "merge" and "Conflict: merged with a newer change." or "Acknowledged."
  elseif r.error.code == "conflict" then
    -- Someone changed the metric first. If they acknowledged it, theirs
    -- stands. A new value is no acknowledgement: ours goes on top of it.
    local now = r.current
    put(s, now)
    if now.content.status == "acked" then
      s.form = nil
      local who = text(now.content.acked_by)
      s.status = "Already acknowledged by " .. (who ~= "" and who or "someone") .. "."
    else
      local note = (f and f.note) or text(r.request.content.note)
      data.update {
        record_id = now.id, rev = now.rev,
        content = acknowledged(now.content, note, ctx.user.name), tag = "merge",
      }
      s.status = "Saving..."
    end
  else
    if f then f.saving = false end
    if r.error.code == "not_found" then remove(s, r.request.record_id) end
    s.status = failure(r.error)
  end
  fix_selection(s)
end

function app.push(s, p, ctx)
  if p.change == "delete" then
    remove(s, p.record.id)
  else
    put(s, p.record)
  end
  fix_selection(s)
end

return app
