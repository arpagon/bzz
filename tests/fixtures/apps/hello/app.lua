-- hello: the smallest complete app. A shared list of notes.
-- Keys: n new note, j/k move, r reload, q close. In the form: Enter saves,
-- Esc cancels.

local app = {}

-- The contract this script is written against. Required.
app.api = 1

-- The fence: the only record types this app may read or write.
app.types = { "note" }

-- A list arrives in no order: keep one in the record and sort by it.
local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id
end

-- Called once at start. Returns the state table every handler receives.
function app.init(ctx)
  data.list { type = "note", tag = "load" }
  return { notes = {}, selected = 1, form = nil, status = "Loading..." }
end

-- Called after every round of events. Returns the screen as ui elements.
function app.view(s, ctx)
  local items = {}
  for _, n in ipairs(s.notes) do
    items[#items + 1] = ui.line {
      ui.span { tostring(n.content.author) .. ": ", fg = "accent" },
      tostring(n.content.text),
    }
  end
  local main = ui.column {
    ui.text { "Notes (" .. #s.notes .. ")", bold = true },
    ui.list { items = items, selected = s.selected, highlight = "selection" },
    ui.text { s.status, fg = "muted" },
  }
  if not s.form then return main end
  -- The form floats over the list.
  return ui.layers {
    main,
    ui.popup {
      title = " New note ", width = math.min(50, ctx.width - 2), height = 5,
      ui.input { value = s.form.value, cursor = s.form.cursor, focused = true, placeholder = "type a note" },
      ui.text { s.form.error or "Enter save  Esc cancel", fg = s.form.error and "error" or nil },
    },
  }
end

function app.key(s, key, ctx)
  if s.form then
    if key.name == "esc" then
      s.form = nil
    elseif key.name == "enter" then
      local text = s.form.value:match("^%s*(.-)%s*$")
      if text == "" then
        s.form.error = "Text is required."
      else
        data.create {
          type = "note",
          content = { text = text, author = ctx.user.name, order = host.time() },
          tag = "create",
        }
        s.form = nil
        s.status = "Saving..."
      end
    else
      ui.edit(s.form, key) -- typing, Backspace, arrows
    end
    return
  end
  if key.name == "q" then host.quit()
  elseif key.name == "n" then s.form = { value = "" }
  elseif key.name == "j" or key.name == "down" then s.selected = math.min(s.selected + 1, #s.notes)
  elseif key.name == "k" or key.name == "up" then s.selected = math.max(s.selected - 1, 1)
  elseif key.name == "r" then data.list { type = "note", tag = "load" }; s.status = "Loading..."
  end
end

function app.paste(s, text, ctx)
  if s.form then ui.paste(s.form, text) end
end

-- Replies to data.* requests, matched by the tag given in the request.
function app.reply(s, r, ctx)
  if not r.ok then
    if r.error.code == "rate_limited" then
      s.status = "Too many changes. Retry in " .. r.error.retry_after .. " s."
    else
      s.status = "Error: " .. r.error.message
    end
  elseif r.tag == "load" then
    s.notes = r.records
    table.sort(s.notes, by_order)
    s.status = "Loaded " .. #r.records .. " notes."
  elseif r.tag == "create" then
    table.insert(s.notes, r.record)
    table.sort(s.notes, by_order)
    for i, n in ipairs(s.notes) do
      if n.id == r.record.id then s.selected = i end
    end
    s.status = "Saved."
  end
end

-- Changes made by other participants.
function app.push(s, p, ctx)
  local at
  for i, n in ipairs(s.notes) do
    if n.id == p.record.id then at = i break end
  end
  if p.change == "delete" then
    if at then table.remove(s.notes, at) end
  elseif at then
    s.notes[at] = p.record
  else
    table.insert(s.notes, p.record)
  end
  table.sort(s.notes, by_order)
  s.selected = math.max(1, math.min(s.selected, #s.notes))
  s.status = p.by.name .. " changed a note."
end

return app
