-- The app API as a script sees it: `ui`, `data`, `host` and `print`.
--
-- Each is a Rust function, put here under its name and called directly:
-- not through a wrapper in Lua. A script writes `return ui.column { ... }`
-- as the last line of its view, and Lua drops the frame of a function that
-- ends in a call of another Lua function. A call of a Rust function keeps
-- it, so an error can still name the script's line.
--
-- An error raised from Rust is a userdata until it is caught; the `pcall`
-- of the bounded library hands the script the string it stands for.
local raw = ...

local next, type = next, type
local find, sub = string.find, string.sub

local tables = { ui = {}, data = {}, host = {} }
for name, value in next, raw do
  local dot = find(name, ".", 1, true)
  if dot then
    tables[sub(name, 1, dot - 1)][sub(name, dot + 1)] = value
  else
    _G[name] = value
  end
end

ui, data, host = tables.ui, tables.data, tables.host
