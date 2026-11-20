-- Poll: one question, a list of options, one vote for each participant.
-- Two record types:
--   "poll": { question = string, options = { string, ... } }  (one record)
--   "vote": { voter = string, option = string }  (one for each participant)
-- `voter` is the participant's id (`ctx.user.id`, a public key), which never
-- changes; a vote counts for the option it names. Who voted for the
-- selected option is shown by name, with `host.name(id)`.
-- Keys: j/k and arrows move, Enter or Space vote, a add an option,
-- / search, r reload, q close. With no poll yet, a asks for the question.

local app = {}
app.api = 1
app.types = { "poll", "vote" }

local function trim(t) return (t:gsub("^%s+", ""):gsub("%s+$", "")) end

local function copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end

-- Any member of the channel can write any field: read them with care.
local function options_of(poll)
  local out = {}
  local list = poll and poll.content.options
  if type(list) ~= "table" then return out end
  for _, label in ipairs(list) do
    if type(label) == "string" then out[#out + 1] = label end
  end
  return out
end

-- The poll of the channel. There should be one; of several, always the
-- same one, whoever looks.
local function the_poll(s)
  local first
  for _, p in ipairs(s.polls) do
    if not first or p.id < first.id then first = p end
  end
  return first
end

local function my_vote(s, me)
  for _, v in ipairs(s.votes) do
    if v.content.voter == me then return v end
  end
  return nil
end

-- A load is one list for each type. Nothing of it is shown until both
-- have answered.
local function load(s)
  s.load = {
    poll = data.list { type = "poll", tag = "load" },
    vote = data.list { type = "vote", tag = "load" },
    left = 2, failed = false,
  }
  s.touched = {} -- ids changed since these lists were asked for
  s.status = "Loading..."
end

function app.init(ctx)
  local s = {
    polls = {}, votes = {}, loaded = false,
    sel = nil,          -- the label of the selected option
    query = "", search_open = false,
    form = nil,         -- { kind = "option" | "poll", value, cursor, error, saving }
    saving = false,     -- a vote is on its way
    status = "",
  }
  load(s)
  return s
end

local function matches(s, label)
  return s.query == "" or label:lower():find(s.query:lower(), 1, true) ~= nil
end

-- The options shown under the search, in the poll's order.
local function shown(s)
  local out = {}
  for _, label in ipairs(options_of(the_poll(s))) do
    if matches(s, label) then out[#out + 1] = label end
  end
  return out
end

local function index_of(list, label)
  for i, l in ipairs(list) do
    if l == label then return i end
  end
  return nil
end

-- The selected option stays selected while it is shown; else the first.
local function fix_selection(s)
  local list = shown(s)
  if s.sel and index_of(list, s.sel) then return end
  if list[1] then s.sel = list[1] end
end

local function put(s, rec)
  local list = rec.type == "poll" and s.polls or s.votes
  local at
  for i, r in ipairs(list) do
    if r.id == rec.id then at = i end
  end
  list[at or #list + 1] = rec
  if s.touched then s.touched[rec.id] = true end
end

local function remove(s, rec_type, id)
  local list = rec_type == "poll" and s.polls or s.votes
  for i, r in ipairs(list) do
    if r.id == id then table.remove(list, i) break end
  end
  if s.touched then s.touched[id] = true end
end

-- A list that was asked for before a change must not undo it: a record
-- that changed while the list was on its way keeps what the app has.
local function merge(touched, held, records)
  local out = {}
  for _, rec in ipairs(records) do
    if not touched[rec.id] then out[#out + 1] = rec end
  end
  for _, rec in ipairs(held) do
    if touched[rec.id] then out[#out + 1] = rec end
  end
  return out
end

local function votes_text(n)
  return n .. (n == 1 and " vote" or " votes")
end

local function form_view(s, ctx)
  local f = s.form
  local option = f.kind == "option"
  local label = option and "Option: " or "Question: "
  return ui.popup {
    width = math.min(50, ctx.width - 2), height = 6,
    ui.text { option and "New option" or "New poll", bold = true },
    ui.row { size = 1,
      ui.text { label, size = #label },
      ui.input { value = f.value, cursor = f.cursor, focused = true },
    },
    ui.text { f.error or "", fg = "error" },
    ui.text { "Enter save  Esc cancel", fg = "muted" },
  }
end

function app.view(s, ctx)
  local me = ctx.user.id
  local poll = the_poll(s)
  local options = options_of(poll)
  local tally, total = {}, 0
  for _, label in ipairs(options) do tally[label] = 0 end
  for _, v in ipairs(s.votes) do
    local label = v.content.option
    -- A vote for an option that is not in the poll counts nowhere.
    if type(label) == "string" and tally[label] then
      tally[label] = tally[label] + 1
      total = total + 1
    end
  end
  local mine = my_vote(s, me)
  local voted = mine and mine.content.option

  local parts = {}
  if s.loaded and not poll then
    parts[1] = ui.text { "No poll yet. Press a to ask a question.", size = "fill", wrap = true }
  else
    local question = poll and poll.content.question
    parts[1] = ui.text {
      ui.truncate(type(question) == "string" and question or "", ctx.width),
      bold = true, fg = "accent", size = 1,
    }
    local list = s.loaded and shown(s) or {}
    -- A row is "> [x] Label  N votes": the label gets what the marker, the
    -- box and the widest tally leave of the width.
    local room = math.max(4, ctx.width - 8 - #votes_text(total))
    local width = 1
    for _, label in ipairs(list) do width = math.max(width, math.min(room, ui.width(label))) end
    local items = {}
    for i, label in ipairs(list) do
      local cut = ui.truncate(label, room)
      items[i] = (label == voted and "[x] " or "[ ] ") .. cut
        .. string.rep(" ", width - ui.width(cut) + 2) .. votes_text(tally[label])
    end
    if s.loaded and #list == 0 and s.query ~= "" then
      parts[#parts + 1] = ui.text { "No options match.", size = "fill" }
    else
      parts[#parts + 1] = ui.list {
        id = "options", items = items, selected = index_of(list, s.sel),
        highlight = "selection", size = "fill",
      }
    end
    if s.loaded then parts[#parts + 1] = ui.text { "Total: " .. votes_text(total) } end
    -- Who voted for the selected option. A vote stores an id; the name is
    -- bzz's (`host.name`), and the app is drawn again when bzz learns one.
    local names = {}
    for _, v in ipairs(s.loaded and s.votes or {}) do
      if v.content.option == s.sel and type(v.content.voter) == "string" then
        names[#names + 1] = v.content.voter == me and "you" or host.name(v.content.voter)
      end
    end
    if #names > 0 then
      table.sort(names)
      parts[#parts + 1] = ui.text {
        ui.truncate("By: " .. table.concat(names, ", "), ctx.width), fg = "muted", size = 1,
      }
    end
  end
  if s.search_open or s.query ~= "" then
    parts[#parts + 1] = ui.row { size = 1,
      ui.text { "Search: ", size = 8 },
      ui.input { value = s.query, cursor = s.search_cursor, focused = s.search_open },
    }
  end
  parts[#parts + 1] = ui.text { ui.truncate(s.status, ctx.width), fg = "muted", size = 1 }
  local main = ui.column(parts)
  if not s.form then return main end
  return ui.layers { main, form_view(s, ctx) }
end

local function vote(s, me)
  if s.saving or not s.sel or not index_of(shown(s), s.sel) then return end
  local mine = my_vote(s, me)
  if mine and mine.content.option == s.sel then return end -- already there
  local tag = { kind = "vote", option = s.sel }
  if mine then
    local content = copy(mine.content)
    content.option = s.sel
    data.update { record_id = mine.id, rev = mine.rev, content = content, tag = tag }
  else
    data.create { type = "vote", content = { voter = me, option = s.sel }, tag = tag }
  end
  s.saving = true
  s.status = "Saving..."
end

local function has_option(options, label)
  for _, o in ipairs(options) do
    if o:lower() == label:lower() then return true end
  end
  return false
end

-- Sends the poll with `label` added to its options. An update replaces the
-- whole content, so the rest of the poll is copied.
local function send_option(s, poll, label, tag)
  local content = copy(poll.content)
  local options = options_of(poll)
  options[#options + 1] = label
  content.options = options
  data.update {
    record_id = poll.id, rev = poll.rev, content = content,
    tag = { kind = tag, option = label },
  }
  s.status = "Saving..."
end

local function form_key(s, key)
  local f = s.form
  if key.name == "esc" then
    s.form = nil
    return
  end
  if key.name ~= "enter" then
    ui.edit(f, key)
    return
  end
  if f.saving then return end -- one confirmation, one write
  local value = trim(f.value or "")
  if f.kind == "poll" then
    if value == "" then
      f.error = "Question is required."
      return
    end
    data.create {
      type = "poll", content = { question = value, options = {} },
      tag = { kind = "poll" },
    }
    s.status = "Saving..."
  else
    local poll = the_poll(s)
    if value == "" then
      f.error = "Option is required."
      return
    elseif not poll then
      s.form = nil
      s.status = "Save failed."
      return
    elseif has_option(options_of(poll), value) then
      f.error = "Option already exists."
      return
    end
    send_option(s, poll, value, "option")
  end
  f.error = nil
  f.saving = true
end

local function search_key(s, key)
  if key.name == "enter" then
    s.search_open = false
  elseif key.name == "esc" then
    s.search_open = false
    s.query = ""
  else
    local field = { value = s.query, cursor = s.search_cursor }
    ui.edit(field, key)
    s.query, s.search_cursor = field.value, field.cursor
  end
  fix_selection(s)
end

function app.key(s, key, ctx)
  if s.form then return form_key(s, key) end
  if s.search_open then return search_key(s, key) end
  local n = key.name
  local list = shown(s)
  local i = index_of(list, s.sel)
  if n == "q" then host.quit()
  elseif n == "j" or n == "down" then
    if i and i < #list then s.sel = list[i + 1] end
  elseif n == "k" or n == "up" then
    if i and i > 1 then s.sel = list[i - 1] end
  elseif n == "enter" or n == "space" then vote(s, ctx.user.id)
  elseif n == "a" then
    if s.loaded then s.form = { kind = the_poll(s) and "option" or "poll", value = "" } end
  elseif n == "/" then s.search_open = true; s.search_cursor = nil
  elseif n == "esc" and s.query ~= "" then s.query = ""; fix_selection(s)
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
    fix_selection(s)
  end
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

local function loaded(s, r)
  local l = s.load
  local kind = r.request.type
  if not l or r.id ~= l[kind] then return end -- a newer load was asked for
  l.left = l.left - 1
  if r.ok then l[kind .. "s"] = r.records else l.failed = true end
  if l.left > 0 then return end
  s.load = nil
  if l.failed then
    -- Everything that was shown before stays.
    s.status = "Load failed. Press r to retry."
    return
  end
  local touched = s.touched or {}
  s.polls = merge(touched, s.polls, l.polls)
  s.votes = merge(touched, s.votes, l.votes)
  s.touched = {}
  s.loaded = true
  fix_selection(s)
  s.status = "Loaded " .. #options_of(the_poll(s)) .. " options."
end

function app.reply(s, r, ctx)
  local tag = r.tag
  if tag == "load" then return loaded(s, r) end
  if type(tag) ~= "table" then return end

  if tag.kind == "vote" then
    if r.ok then
      s.saving = false
      put(s, r.record)
      s.status = "Voted for " .. tag.option .. "."
    elseif r.error.code == "conflict" then
      -- The vote record changed under us (the same person, elsewhere):
      -- the newest choice wins.
      local content = copy(r.current.content)
      content.option = tag.option
      put(s, r.current)
      data.update { record_id = r.current.id, rev = r.current.rev, content = content, tag = tag }
    else
      s.saving = false
      if r.error.code == "not_found" then remove(s, "vote", r.request.record_id) end
      s.status = failure(r.error)
    end
    return
  end

  -- The poll itself: a new one, or an option added.
  if r.ok then
    put(s, r.record)
    s.form = nil
    s.status = tag.kind == "poll" and "Poll created."
      or tag.kind == "merge" and "Conflict: merged with a newer change."
      or "Option added."
  elseif r.error.code == "conflict" then
    -- Someone changed the poll first. Their options keep their place and
    -- ours goes last, unless they added the same one.
    put(s, r.current)
    if has_option(options_of(r.current), tag.option) then
      s.form = nil
      s.status = "Option added."
    else
      send_option(s, r.current, tag.option, "merge")
    end
  else
    if s.form then s.form.saving = false end
    s.status = failure(r.error)
  end
  fix_selection(s)
end

function app.push(s, p, ctx)
  if p.change == "delete" then
    remove(s, p.record.type, p.record.id)
  else
    put(s, p.record)
  end
  fix_selection(s)
end

return app
