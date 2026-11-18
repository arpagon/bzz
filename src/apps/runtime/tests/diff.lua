-- The bounded functions against the stock ones, on random arguments.
-- Runs in the variant `bounded+stock`, where `stock` holds Lua's own.
-- Returns: comparisons, differences, report.
local N = N or 200000
math.randomseed(SEED or 20261118)
local rnd = math.random
local spcall = stock.pcall

local function pick(t) return t[rnd(#t)] end

local ATOMS = { "a", "b", "c", ".", "%a", "%d", "%s", "%w", "%p", "%u", "%A", "%S", "[abc]", "[^a]", "[a-c]", "[%d%s]",
  "[^%w]", "[]]", "[%]]", "%.", "%%", " ", "1", "(", ")", "()", "%b()", "%b[]", "%f[%a]", "%f[%s]", "%1", "%2", "x", "-", "^", "$",
  "[", "]", "%", "%z", "\0", "é" }
local QUANT = { "", "", "", "*", "+", "-", "?" }
local SUBJ = { "a", "a", "b", "c", " ", "1", "2", "(", ")", "[", "]", ".", "%", "x", "A", "\n", "\0", "é", "-" }

local function pattern()
  local n = rnd(0, 7)
  local t = {}
  if rnd(6) == 1 then t[#t + 1] = "^" end
  for _ = 1, n do t[#t + 1] = pick(ATOMS) .. pick(QUANT) end
  if rnd(6) == 1 then t[#t + 1] = "$" end
  return table.concat(t)
end

local function subject()
  local n = rnd(0, 14)
  local t = {}
  for i = 1, n do t[i] = pick(SUBJ) end
  return table.concat(t)
end

local REPL = { "", "x", "%0", "<%1>", "%2%1", "%%", "%", "%9", "[%0%0]", 7 }

-- Lua's own errors name the function and carry a position; compare the rest.
local function norm(msg)
  if type(msg) ~= "string" then return tostring(msg) end
  msg = stock.gsub(msg, "^[^:]*:%d+: ", "")
  -- the name is how the caller wrote the call ('f', 'string.rep', 'rep')
  msg = stock.gsub(msg, "to '[^']*'", "to 'F'")
  return msg
end

local function same(a, b)
  if a.n ~= b.n then return false end
  for i = 1, a.n do
    local x, y = a[i], b[i]
    if i == 2 and a[1] == false then x, y = norm(x), norm(y) end
    if x ~= y and not (x ~= x and y ~= y) then return false end
  end
  return true
end

local function show(r)
  local t = {}
  for i = 1, r.n do
    local v = r[i]
    t[i] = type(v) == "string" and string.format("%q", v) or tostring(v)
  end
  return "(" .. table.concat(t, ", ") .. ")"
end

local ran, bad, report = 0, 0, {}
local function check(what, f_stock, f_new, ...)
  local a = table.pack(spcall(f_stock, ...))
  local b = table.pack(spcall(f_new, ...))
  ran = ran + 1
  if not same(a, b) then
    bad = bad + 1
    if bad <= 25 then
      local args = table.pack(...)
      report[#report + 1] = what .. show(args) .. "\n    stock   " .. show(a) .. "\n    bounded " .. show(b)
    end
  end
end

local function all(f)
  return function(s, p, init)
    local out = {}
    for a, b, c in f(s, p, init) do
      out[#out + 1] = tostring(a) .. "|" .. tostring(b) .. "|" .. tostring(c)
      if #out > 64 then break end
    end
    return table.concat(out, ";")
  end
end

local function copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end

local function dump(t)
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = k end
  stock.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
  local out = {}
  for i, k in ipairs(keys) do out[i] = tostring(k) .. "=" .. tostring(t[k]) end
  return stock.concat(out, ",")
end

-- A table function is compared by what it returns and by the table after it.
local function on_copy(f)
  return function(t, ...)
    local c = copy(t)
    local r = table.pack(f(c, ...))
    local out = { dump(c) }
    for i = 1, r.n do out[#out + 1] = type(r[i]) == "table" and dump(r[i]) or tostring(r[i]) end
    return stock.concat(out, " / ")
  end
end

local function array()
  local n = rnd(0, 9)
  local t = {}
  for i = 1, n do t[i] = rnd(4) == 1 and pick({ "b", "a", "c" }) or rnd(-3, 9) end
  if rnd(8) == 1 and n > 2 then t[rnd(n)] = nil end
  return t
end

local function numbers()
  local n = rnd(0, 40)
  local t = {}
  for i = 1, n do t[i] = rnd(-5, 20) end
  return t
end

local INTS = { -2, -1, 0, 1, 2, 3, 5, 9, 12, math.maxinteger, math.mininteger, 1.0, 2.5, "3", "x" }
local function int() return pick(INTS) end
local function maybe(v) if rnd(3) == 1 then return nil end return v end

for i = 1, N do
  local s, p = subject(), pattern()
  local k = i % 12
  if k == 0 then check("find", stock.find, string.find, s, p)
  elseif k == 1 then check("find", stock.find, string.find, s, p, rnd(-4, 16), rnd(3) == 1)
  elseif k == 2 then check("match", stock.match, string.match, s, p, maybe(rnd(-4, 16)))
  elseif k == 3 then check("gmatch", all(stock.gmatch), all(string.gmatch), s, p, maybe(rnd(-4, 16)))
  elseif k == 4 then check("gsub", stock.gsub, string.gsub, s, p, pick(REPL), maybe(rnd(-1, 4)))
  elseif k == 5 then
    local map = { a = "A", b = false, [" "] = "_", ["1"] = 11, c = {} }
    check("gsub{table}", stock.gsub, string.gsub, s, p, map, maybe(rnd(0, 3)))
  elseif k == 6 then
    local function f(a, b)
      if a == "a" then return nil end
      if a == "b" then return false end
      if a == "1" then return 42 end
      if a == "x" then return {} end
      return "<" .. tostring(a) .. tostring(b) .. ">"
    end
    check("gsub{function}", stock.gsub, string.gsub, s, p, f)
  elseif k == 7 then check("rep", stock.rep, string.rep, pick({ "", "a", "ab", 5, {} }), pick({ -1, 0, 1, 3, 2.0, 2.5, "2", "x" }), maybe(pick({ "", ",", 1 })))
  elseif k == 8 then
    local t = array()
    local which = rnd(4)
    if which == 1 then check("insert", on_copy(stock.insert), on_copy(table.insert), t, pick({ 1, "v" }))
    elseif which == 2 then check("insert", on_copy(stock.insert), on_copy(table.insert), t, int(), "v")
    elseif which == 3 then check("remove", on_copy(stock.remove), on_copy(table.remove), t, maybe(int()))
    else check("move", on_copy(stock.move), on_copy(table.move), t, rnd(-1, 6), rnd(-1, 8), rnd(-1, 8), maybe({ 7, 7, 7 })) end
  elseif k == 9 then
    local t = array()
    if rnd(2) == 1 then check("concat", stock.concat, table.concat, t, maybe(pick({ "", ",", 1, {} })), maybe(int()), maybe(int()))
    else check("unpack", stock.unpack, table.unpack, t, maybe(int()), maybe(int())) end
  elseif k == 10 then
    local t = numbers()
    local function sorted(f) return function(a, c) local x = copy(a); f(x, c); return stock.concat(x, ",") end end
    if rnd(2) == 1 then check("sort", sorted(stock.sort), sorted(table.sort), t)
    else check("sort>", sorted(stock.sort), sorted(table.sort), t, function(a, b) return a > b end) end
  else
    -- a metatable with __index and __len: the Lua versions must honour them as C does
    local base = array()
    local n = #base
    local function proxy()
      return setmetatable({}, { __index = function(_, i) return base[i] end, __len = function() return n end })
    end
    local which = rnd(3)
    if which == 1 then check("concat(proxy)", stock.concat, table.concat, proxy(), ",")
    elseif which == 2 then check("unpack(proxy)", stock.unpack, table.unpack, proxy())
    else check("insert(proxy)", on_copy(stock.insert), on_copy(table.insert), proxy(), "v") end
  end
end

return ran, bad, table.concat(report, "\n")
