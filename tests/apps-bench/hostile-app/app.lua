-- A hostile app: it tries to leave what bzz lends it. Each attempt runs in
-- pcall and its outcome is a line of the screen. Keys trigger the attempts
-- that would stop an ordinary script: x an endless loop, y a loop that
-- catches its stop, p a pattern that runs for ever, m memory, s the stack,
-- c l u d records outside the fence. It is the prototype's
-- `tests/hostile/app.lua` at `api = 1`, the script of
-- `apps::runtime::tests::reach` with a count of the keys it handled;
-- `lib/hostile.py` runs it on the example host and puts a path in place of
-- MARKER.
local app = {}
app.api = 1
app.types = { "note" }

local function try(label, f)
  local ok, res = pcall(f)
  return label .. ": " .. (ok and ("REACHED " .. tostring(res)) or ("blocked (" .. tostring(res) .. ")"))
end

function app.init(ctx)
  local out = {}
  local function add(l) out[#out + 1] = l end
  add(try("read file io.open", function() return io.open("/etc/hostname"):read("a") end))
  add(try("read file dofile", function() return dofile("/etc/hostname") end))
  add(try("read file loadfile", function() return loadfile("/etc/hostname") end))
  add(try("require module", function() return require("socket") end))
  add(try("load source", function() return load("return 1")() end))
  add(try("load bytecode", function() return load(string.dump(function() return 1 end))() end))
  add(try("dump bytecode", function() return string.dump(function() return 1 end) end))
  add(try("dump through a string", function() return ("x").dump(function() return 1 end) end))
  add(try("native code package.loadlib", function() return package.loadlib("libc.so.6", "getpid") end))
  add(try("environment os.getenv", function() return os.getenv("HOME") end))
  add(try("process os.execute", function() return os.execute("touch MARKER") end))
  add(try("process io.popen", function() return io.popen("id"):read("a") end))
  add(try("exit os.exit", function() return os.exit(3) end))
  add(try("clock os.clock", function() return os.clock() end))
  add(try("debug library", function() return debug.getregistry() end))
  add(try("coroutine", function() return coroutine.running() end))
  add(try("collectgarbage", function() return collectgarbage("count") end))
  add(try("terminal io.write", function() return io.write("\27[2J") end))
  add(try("terminal warn", function() return warn("@on") end))
  local names = {}
  for k in pairs(_G) do names[#names + 1] = k end
  table.sort(names)
  add("globals: " .. table.concat(names, " "))
  for _, lib in ipairs { "host", "data", "ui" } do
    local members = {}
    for k in pairs(_G[lib]) do members[#members + 1] = k end
    table.sort(members)
    add(lib .. ": " .. table.concat(members, " "))
  end
  return { lines = out, pushes = 0, note = "", keys = 0 }
end

function app.view(s, ctx)
  local lines = {}
  for i, l in ipairs(s.lines) do lines[i] = l end
  lines[#lines + 1] = "pushes received: " .. s.pushes
  lines[#lines + 1] = "note: " .. s.note .. " (" .. s.keys .. " keys)"
  return ui.text { wrap = true, table.unpack(lines) }
end

function app.key(s, key, ctx)
  local n = key.name
  s.keys = s.keys + 1
  s.note = "key " .. n .. " handled"
  if n == "q" then host.quit()
  elseif n == "x" then while true do end
  elseif n == "y" then while true do pcall(function() while true do end end) end
  elseif n == "p" then return string.rep("a", 300000):find(".-.-.-.-x")
  elseif n == "m" then
    local t = {}
    while true do t[#t + 1] = string.rep("x", 1000000) .. #t end
  elseif n == "c" then data.create { type = "card", content = { title = "smuggled", column = "todo" } }
  elseif n == "l" then data.list { type = "card" }
  elseif n == "u" then data.update { record_id = "r0001", rev = "1", content = { title = "hijacked" } }
  elseif n == "d" then data.delete { record_id = "r0001", rev = "1" }
  elseif n == "s" then
    local function f(i) return 1 + f(i + 1) end
    f(1)
  end
end

function app.push(s, p, ctx)
  s.pushes = s.pushes + 1
end

return app
