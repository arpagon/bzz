-- Approvals: requests that are pending, approved or rejected.
-- A request is a record of type "request" with content
--   { title = string, author = string, state = "pending" | "approved" |
--     "rejected", decided_by = string or null, reason = string or null,
--     order = number }.
-- `author` and `decided_by` are participants' ids (`ctx.user.id`, a public
-- key), which never change; the app shows them with `host.name(id)`, the
-- name bzz knows for an id, and draws again when it learns one.
-- Keys: j/k and arrows move, Enter detail, Esc closes it, n new request,
-- a approve, x reject (asks for a reason), f filter, r reload, q close.

local app = {}
app.api = 1
app.types = { "request" }

local FILTERS = { "all", "pending", "approved", "rejected" }

local function trim(t) return (t:gsub("^%s+", ""):gsub("%s+$", "")) end

-- Any member of the channel can write any field: read them with care.
local function text(v, default)
  if type(v) == "string" then return v end
  return default or "?"
end

-- A participant stored in a field, as a name to show.
local function who(id, default)
  if type(id) == "string" and id ~= "" then return host.name(id) end
  return default or "?"
end

local function state_of(rec)
  local st = rec.content.state
  if st == "approved" or st == "rejected" then return st end
  return "pending"
end

-- A list arrives in no order, and ids do not sort by creation: the order
-- of the requests is a field of the record.
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
  s.list_id = data.list { type = "request", tag = "load" }
  s.touched = {} -- ids changed since this list was asked for
  s.status = "Loading..."
end

function app.init(ctx)
  local s = {
    reqs = {}, sel = nil, filter = 1, detail = false, loaded = false,
    form = nil,   -- { kind = "new" | "reject", value, cursor, error, target }
    saving = false, status = "",
  }
  load(s)
  return s
end

local function shown(s)
  local out = {}
  local f = FILTERS[s.filter] or "all"
  for _, r in ipairs(s.reqs) do
    if f == "all" or state_of(r) == f then out[#out + 1] = r end
  end
  return out
end

local function index_of(list, id)
  if id == nil then return nil end
  for i, r in ipairs(list) do
    if r.id == id then return i end
  end
  return nil
end

local function find(s, id)
  local i = index_of(s.reqs, id)
  return i and s.reqs[i] or nil
end

-- The selected request stays selected while it is shown; else the first.
local function fix_selection(s)
  local list = shown(s)
  if index_of(list, s.sel) then return end
  s.sel = list[1] and list[1].id or nil
end

local function put(s, rec)
  local i = index_of(s.reqs, rec.id)
  if i then s.reqs[i] = rec else s.reqs[#s.reqs + 1] = rec end
  table.sort(s.reqs, by_order)
  if s.touched then s.touched[rec.id] = true end
end

local function remove(s, id)
  local i = index_of(s.reqs, id)
  if i then table.remove(s.reqs, i) end
  if s.touched then s.touched[id] = true end
end

-- A list that was asked for before a change must not undo it: a request
-- that changed while the list was on its way keeps what the app has.
local function merge(s, records)
  local touched, out = s.touched or {}, {}
  for _, rec in ipairs(records) do
    if not touched[rec.id] then out[#out + 1] = rec end
  end
  for _, rec in ipairs(s.reqs) do
    if touched[rec.id] then out[#out + 1] = rec end
  end
  table.sort(out, by_order)
  s.touched = {}
  return out
end

local function already(rec)
  return "Already " .. state_of(rec) .. " by " .. who(rec.content.decided_by, "someone") .. "."
end

-- The decision on top of the record as it is: an update replaces the whole
-- content, so everything else of it is copied. A null is nil when it
-- arrives and must be written as data.null to stay in the record.
local function send_decision(s, rec, state, reason, me)
  local content = copy(rec.content)
  content.state = state
  content.decided_by = me
  content.reason = reason or content.reason or data.null
  data.update {
    record_id = rec.id, rev = rec.rev, content = content,
    tag = { kind = "decide", state = state, reason = reason },
  }
  s.saving = true
  s.status = "Saving..."
end

-- The selected request, if it may be decided here; else says why not.
-- "Not your own" rests on a field any member can set: it is a convention
-- between honest participants, and nothing enforces it.
local function decidable(s, me)
  local rec = find(s, s.sel)
  if not rec then return nil end
  if rec.content.author == me then
    s.status = "You cannot decide your own request."
    return nil
  end
  if state_of(rec) ~= "pending" then
    s.status = already(rec)
    return nil
  end
  return rec
end

local function detail_lines(rec)
  if not rec then return { "No request selected." } end
  local c = rec.content
  local lines = {
    "Request: " .. text(c.title),
    "Author: " .. who(c.author),
    "State: " .. state_of(rec),
  }
  if state_of(rec) ~= "pending" then lines[#lines + 1] = "Decided by: " .. who(c.decided_by) end
  if state_of(rec) == "rejected" then lines[#lines + 1] = "Reason: " .. text(c.reason, "") end
  return lines
end

local function form_view(s, ctx)
  local f = s.form
  local new = f.kind == "new"
  local label = new and "Title: " or "Reason: "
  return ui.popup {
    width = math.min(50, ctx.width - 2), height = 6,
    ui.text { new and "New request" or "Reject request", bold = true },
    ui.row { size = 1,
      ui.text { label, size = #label },
      ui.input { value = f.value, cursor = f.cursor, focused = true },
    },
    ui.text { f.error or "", fg = "error" },
    ui.text { "Enter save  Esc cancel", fg = "muted" },
  }
end

function app.view(s, ctx)
  local w = ctx.width
  local list = shown(s)
  local head = "Filter: " .. (FILTERS[s.filter] or "all")
  if w >= 70 then head = head .. "   n new  a approve  x reject  f filter  r reload  q close" end

  local body
  if not s.loaded then
    body = ui.spacer { size = "fill" }
  elseif #list == 0 then
    body = ui.text { "No requests match.", size = "fill" }
  else
    local items, sel = {}, nil
    for i, r in ipairs(list) do
      local st = state_of(r)
      items[i] = ui.line {
        st .. string.rep(" ", 8 - #st) .. "  " .. text(r.content.title) .. "  ",
        ui.span { who(r.content.author, ""), fg = "muted" },
      }
      if r.id == s.sel then sel = i end
    end
    body = ui.list { id = "requests", items = items, selected = sel, highlight = "selection", size = "fill" }
  end

  if s.detail then
    local lines = detail_lines(find(s, s.sel))
    if w >= 80 then
      -- Beside the list where there is room for both.
      body = ui.row { size = "fill", spacing = 1,
        body,
        ui.block { title = " Detail ", size = math.min(44, w // 2), ui.text(lines) },
      }
    else
      body = ui.column { size = "fill",
        body,
        ui.block { title = " Detail ", borders = { "top" }, size = #lines + 1, ui.text(lines) },
      }
    end
  end

  local main = ui.column {
    ui.text { ui.truncate(head, w), bold = true, fg = "accent", size = 1 },
    body,
    ui.text { ui.truncate(s.status or "", w), fg = "muted", size = 1 },
  }
  if not s.form then return main end
  return ui.layers { main, form_view(s, ctx) }
end

local function form_key(s, key, ctx)
  local f = s.form
  if key.name == "esc" then
    s.form = nil
    return
  end
  if key.name ~= "enter" then
    ui.edit(f, key)
    return
  end
  if s.saving then return end -- one confirmation, one write
  local value = trim(f.value or "")
  if f.kind == "new" then
    if value == "" then
      f.error = "Title is required."
      return
    end
    data.create {
      type = "request", tag = { kind = "create" },
      content = {
        title = value, author = ctx.user.id, state = "pending",
        decided_by = data.null, reason = data.null, order = host.time(),
      },
    }
    s.saving = true
    s.status = "Saving..."
  else
    if value == "" then
      f.error = "Reason is required."
      return
    end
    local rec = find(s, f.target)
    if not rec then
      s.form = nil
      s.status = "Save failed."
    elseif state_of(rec) ~= "pending" then
      s.form = nil
      s.status = already(rec)
    else
      send_decision(s, rec, "rejected", value, ctx.user.id)
    end
  end
  f.error = nil
end

function app.key(s, key, ctx)
  local me = ctx.user.id
  if s.form then return form_key(s, key, ctx) end
  local n = key.name
  local list = shown(s)
  local i = index_of(list, s.sel)
  if n == "q" then host.quit()
  elseif n == "j" or n == "down" then
    if i and i < #list then s.sel = list[i + 1].id end
  elseif n == "k" or n == "up" then
    if i and i > 1 then s.sel = list[i - 1].id end
  elseif n == "enter" then s.detail = true
  elseif n == "esc" then s.detail = false
  elseif n == "n" then s.form = { kind = "new", value = "" }
  elseif n == "f" then
    s.filter = (s.filter or 1) % #FILTERS + 1
    fix_selection(s)
  elseif n == "r" then load(s)
  elseif n == "a" and not s.saving then
    local rec = decidable(s, me)
    if rec then send_decision(s, rec, "approved", nil, me) end
  elseif n == "x" and not s.saving then
    local rec = decidable(s, me)
    if rec then s.form = { kind = "reject", value = "", target = rec.id } end
  end
end

function app.paste(s, pasted, ctx)
  if s.form then ui.paste(s.form, pasted) end
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
    return "No room for more requests."
  end
  return "Save failed."
end

function app.reply(s, r, ctx)
  local tag = r.tag
  if tag == "load" then
    if r.id ~= s.list_id then return end -- a newer list was asked for
    if not r.ok then
      s.status = "Load failed. Press r to retry."
      return
    end
    s.reqs = merge(s, r.records)
    s.loaded = true
    s.status = "Loaded " .. #r.records .. " requests."
    fix_selection(s)
    return
  end
  if type(tag) ~= "table" then return end

  if r.ok then
    s.saving = false
    put(s, r.record)
    if tag.kind == "create" then
      if s.form and s.form.kind == "new" then s.form = nil end
      s.sel = r.record.id
      s.status = "Request filed."
    else
      if s.form and s.form.kind == "reject" then s.form = nil end
      s.status = tag.state == "approved" and "Approved." or "Rejected."
    end
  elseif r.error.code == "conflict" and tag.kind == "decide" then
    -- Someone changed the request first. A decision is never overwritten:
    -- if it is decided now, theirs stands.
    local now = r.current
    put(s, now)
    if state_of(now) ~= "pending" then
      s.saving = false
      if s.form and s.form.kind == "reject" then s.form = nil end
      s.status = already(now)
    else
      send_decision(s, now, tag.state, tag.reason, ctx.user.id) -- still pending: decide it
    end
  else
    s.saving = false
    if r.error.code == "not_found" and r.request.record_id then
      remove(s, r.request.record_id)
      if s.form and s.form.kind == "reject" then s.form = nil end
    end
    s.status = failure(r.error)
  end
  fix_selection(s)
end

-- Someone else changed a record. A push is a real change only: when bzz
-- fetches the channel's records again (a resync) the app hears of what
-- differs, never of everything as deleted and made again. A push of
-- `update` with the `rev` the app has means only that bzz learnt a name.
function app.push(s, p, ctx)
  if p.change == "delete" then
    remove(s, p.record.id)
  else
    put(s, p.record)
  end
  fix_selection(s)
end

return app
