-- The bounded library of a script, for Lua 5.4.
--
-- A script is stopped by a count hook, which runs between VM instructions
-- and makes the script's coroutine yield; the host never resumes it. Two
-- things defeat that in stock Lua:
--
--   1. a C function that loops by itself: the hook never runs;
--   2. script code that a C function calls, or that runs with hooks off: it
--      cannot yield (and a finalizer or a message handler is not even hooked).
--
-- This file takes both away. After it, no C function a script can reach
-- loops longer than the size of its arguments, and none calls script code:
--
--   string.find/match/gmatch/gsub  matched in Rust, with the deadline; the
--                                  loop that calls a replacement function is
--                                  here in Lua
--   string.rep                     the empty repetition returns at once
--   string.format                  __tostring is called here, not by C
--   table.insert/remove/move/sort  written here in Lua: the hook sees them
--   table.concat/unpack            copy in Lua, then the C function on a
--                                  plain table of known length
--   tostring, ipairs, xpcall       written here: the metamethod, the index
--                                  and the message handler are Lua calls
--   string arithmetic ("10" + 1)   written here: C would call the other
--                                  operand's metamethod
--   setmetatable                   refuses __gc: a finalizer runs unhooked
--   pcall                          Lua's own, and one thing more: an error
--                                  that a function of bzz raised from Rust
--                                  is given to the script as a string
--
-- The stock `string` and `table` stay where Lua's own error messages look
-- for names; the script gets two new tables. Every name used below is a
-- local taken before the script runs, so a script that overwrites a global
-- changes nothing here.
local rs_find, rs_match, rs_gmatch_args, rs_step, rs_gsub, rs_gsub_args, rs_rep, rs_meta, rs_error = ...

local error, type, select, rawget, pcall = error, type, select, rawget, pcall
local raw_setmetatable, raw_tostring, raw_tonumber = setmetatable, tostring, tonumber
local sub, raw_format = string.sub, string.format
local concat, unpack = table.concat, table.unpack
local tointeger, maxint = math.tointeger, math.maxinteger

local S, T = {}, {}
local safe_tostring -- defined below; `tostring` as the script sees it
for k, v in next, string do S[k] = v end
for k, v in next, table do T[k] = v end

-- A Rust function answers `false, message` instead of raising: an error
-- raised in Rust would reach the script as a userdata, not as Lua's string.
-- `ck` is always tail-called, so level 2 is the script's own line.
--
-- An allocation that failed is raised as Lua raises it: the bare words,
-- with no position. The host tells it from a script's own error by that.
local NO_MEMORY = "not enough memory"
local function ck(a, ...)
  if a == false then
    if (...) == NO_MEMORY then error(NO_MEMORY, 0) end
    error((...), 2)
  end
  return a, ...
end

-- A stock C function is called through `pcall`, and its error raised again
-- from here: Lua then names the script's line, as it does without us.
-- Always tail-called too.
local function relay(ok, ...)
  if ok then return ... end
  if (...) == NO_MEMORY then error(NO_MEMORY, 0) end
  error((...), 2)
end

local function bad(i, name, msg)
  return "bad argument #" .. i .. " to '" .. name .. "' (" .. msg .. ")"
end

-- An integer argument as luaL_checkinteger reads it; nil and the reason if not.
local function int(v)
  local n = tointeger(v)
  if n then return n end
  if type(v) == "number" then return nil, "number has no integer representation" end
  return nil, "number expected, got " .. (v == nil and "no value" or type(v))
end

-------------------------------------------------------------------- string

S.find = function(s, p, init, plain) return ck(rs_find(s, p, init, plain)) end
S.match = function(s, p, init) return ck(rs_match(s, p, init)) end
S.rep = function(s, n, sep) return ck(rs_rep(s, n, sep)) end

S.gmatch = function(s, p, init)
  local src, pat, pos = rs_gmatch_args(s, p, init)
  if src == false then error(pat, 2) end
  local last = -1
  local function take(q, e, ...)
    if not q then
      if q == false then error(e, 2) end
      return nil
    end
    pos, last = e, e
    return ...
  end
  return function() return take(rs_step(src, pat, pos, last, false, false)) end
end

S.gsub = function(s, p, repl, max_n)
  local tr = type(repl)
  if tr == "string" or tr == "number" then
    return ck(rs_gsub(s, p, repl, max_n))
  elseif tr ~= "function" and tr ~= "table" then
    error(bad(3, "gsub", "string/function/table expected, got " .. (repl == nil and "no value" or tr)), 2)
  end
  -- A function or a table as the replacement: the loop is here, so that a
  -- call of the script's own function is an ordinary Lua call.
  local src, pat, limit, anchor = rs_gsub_args(s, p, max_n)
  if src == false then error(pat, 2) end
  local parts, np = {}, 0
  local at, last, n, changed = 0, -1, 0, false
  local function take(q, e, ...)
    if not q then
      if q == false then error(e, 3) end
      return false
    end
    if q > at then np = np + 1; parts[np] = sub(src, at + 1, q) end
    local v
    if tr == "function" then v = repl(...) else v = repl[(...)] end
    if not v then
      np = np + 1; parts[np] = sub(src, q + 1, e)
    else
      local tv = type(v)
      if tv ~= "string" and tv ~= "number" then error("invalid replacement value (a " .. tv .. ")", 3) end
      np = np + 1; parts[np] = v
      changed = true
    end
    at, last, n = e, e, n + 1
    return true
  end
  while n < limit do
    if not take(rs_step(src, pat, at, last, anchor, tr == "table")) then break end
    if anchor then break end
  end
  if not changed then return s, n end
  if at < #src then np = np + 1; parts[np] = sub(src, at + 1) end
  return concat(parts), n
end

-- %s of a value with __tostring: stock Lua calls the metamethod from C.
S.format = function(fmt, ...)
  local n = select("#", ...)
  if n == 0 then return relay(pcall(raw_format, fmt)) end
  local args, changed = { ... }, false
  for i = 1, n do
    local v = args[i]
    local tv = type(v)
    if (tv == "table" or tv == "string") and rs_meta(v, "__tostring") ~= nil then
      args[i] = safe_tostring(v)
      changed = true
    end
  end
  if changed then return relay(pcall(raw_format, fmt, unpack(args, 1, n))) end
  return relay(pcall(raw_format, fmt, ...))
end

--------------------------------------------------------------------- table

local function tab(t, i, name)
  if type(t) ~= "table" then
    error(bad(i, name, "table expected, got " .. (t == nil and "no value" or type(t))), 3)
  end
end

T.insert = function(t, ...)
  tab(t, 1, "insert")
  local e = #t + 1
  local n = select("#", ...)
  if n == 1 then
    t[e] = ...
  elseif n == 2 then
    local pos, v = ...
    local why
    pos, why = int(pos)
    if not pos then error(bad(2, "insert", why), 2) end
    if pos < 1 or pos > e then error(bad(2, "insert", "position out of bounds"), 2) end
    for i = e, pos + 1, -1 do t[i] = t[i - 1] end
    t[pos] = v
  else
    error("wrong number of arguments to 'insert'", 2)
  end
end

T.remove = function(t, pos)
  tab(t, 1, "remove")
  local size = #t
  if pos == nil then
    pos = size
  else
    local why
    pos, why = int(pos)
    if not pos then error(bad(2, "remove", why), 2) end
    if pos ~= size and (pos < 1 or pos > size + 1) then
      error(bad(2, "remove", "position out of bounds"), 2)
    end
  end
  local v = t[pos]
  while pos < size do
    t[pos] = t[pos + 1]
    pos = pos + 1
  end
  t[pos] = nil
  return v
end

T.move = function(a1, f, e, t, a2)
  local why
  f, why = int(f); if not f then error(bad(2, "move", why), 2) end
  e, why = int(e); if not e then error(bad(3, "move", why), 2) end
  t, why = int(t); if not t then error(bad(4, "move", why), 2) end
  local dst = a2
  if dst == nil then dst = a1 end
  tab(a1, 1, "move")
  tab(dst, a2 == nil and 1 or 5, "move")
  if e >= f then
    if not (f > 0 or e < maxint + f) then error(bad(3, "move", "too many elements to move"), 2) end
    local n = e - f + 1
    if not (t <= maxint - n + 1) then error(bad(4, "move", "destination wrap around"), 2) end
    if t > e or t <= f or (a2 ~= nil and a1 ~= dst) then
      for i = 0, n - 1 do dst[t + i] = a1[f + i] end
    else
      for i = n - 1, 0, -1 do dst[t + i] = a1[f + i] end
    end
  end
  return dst
end

T.concat = function(t, sep, i, j)
  tab(t, 1, "concat")
  local why
  if sep == nil then
    sep = ""
  elseif type(sep) ~= "string" and type(sep) ~= "number" then
    error(bad(2, "concat", "string expected, got " .. type(sep)), 2)
  end
  if i == nil then i = 1 else
    i, why = int(i); if not i then error(bad(3, "concat", why), 2) end
  end
  if j == nil then j = #t else
    j, why = int(j); if not j then error(bad(4, "concat", why), 2) end
  end
  -- The values are read here, one Lua instruction each, into a plain table;
  -- the C function then joins exactly `n` strings and calls nothing.
  local buf, n = {}, 0
  for k = i, j do
    local v = t[k]
    local tv = type(v)
    if tv ~= "string" and tv ~= "number" then
      error("invalid value (" .. tv .. ") at index " .. k .. " in table for 'concat'", 2)
    end
    n = n + 1
    buf[n] = v
  end
  return relay(pcall(concat, buf, sep, 1, n))
end

-- LUAI_MAXSTACK is 1,000,000 slots: stock Lua refuses more results too.
local MAX_UNPACK = 999000

T.unpack = function(t, i, j)
  local why
  if i == nil then i = 1 else
    i, why = int(i); if not i then error(bad(2, "unpack", why), 2) end
  end
  if j == nil then j = #t else
    j, why = int(j); if not j then error(bad(3, "unpack", why), 2) end
  end
  if i > j then return end
  local n = j - i + 1
  if n <= 0 or n >= MAX_UNPACK then error("too many results to unpack", 2) end
  local buf = {}
  for k = 1, n do buf[k] = t[i + k - 1] end
  return relay(pcall(unpack, buf, 1, n))
end

-- A stable merge sort. Stock Lua's quicksort is a C loop that calls the
-- comparison function; here every comparison is a Lua instruction.
local RUN = 8

local function sort_with(a, n, lt)
  for lo = 1, n, RUN do
    local hi = lo + RUN - 1
    if hi > n then hi = n end
    for i = lo + 1, hi do
      local v = a[i]
      local j = i - 1
      while j >= lo and lt(v, a[j]) do
        a[j + 1] = a[j]
        j = j - 1
      end
      a[j + 1] = v
    end
  end
  local b = {}
  local width = RUN
  while width < n do
    local i = 1
    while i <= n do
      local mid, hi = i + width, i + 2 * width
      if mid > n + 1 then mid = n + 1 end
      if hi > n + 1 then hi = n + 1 end
      local l, r, k = i, mid, i
      while l < mid and r < hi do
        if lt(a[r], a[l]) then
          b[k] = a[r]; r = r + 1
        else
          b[k] = a[l]; l = l + 1
        end
        k = k + 1
      end
      while l < mid do b[k] = a[l]; l = l + 1; k = k + 1 end
      while r < hi do b[k] = a[r]; r = r + 1; k = k + 1 end
      i = hi
    end
    a, b = b, a
    width = width * 2
  end
  return a
end

local function sort_plain(a, n)
  for lo = 1, n, RUN do
    local hi = lo + RUN - 1
    if hi > n then hi = n end
    for i = lo + 1, hi do
      local v = a[i]
      local j = i - 1
      while j >= lo and v < a[j] do
        a[j + 1] = a[j]
        j = j - 1
      end
      a[j + 1] = v
    end
  end
  local b = {}
  local width = RUN
  while width < n do
    local i = 1
    while i <= n do
      local mid, hi = i + width, i + 2 * width
      if mid > n + 1 then mid = n + 1 end
      if hi > n + 1 then hi = n + 1 end
      local l, r, k = i, mid, i
      while l < mid and r < hi do
        if a[r] < a[l] then
          b[k] = a[r]; r = r + 1
        else
          b[k] = a[l]; l = l + 1
        end
        k = k + 1
      end
      while l < mid do b[k] = a[l]; l = l + 1; k = k + 1 end
      while r < hi do b[k] = a[r]; r = r + 1; k = k + 1 end
      i = hi
    end
    a, b = b, a
    width = width * 2
  end
  return a
end

T.sort = function(t, comp)
  tab(t, 1, "sort")
  local n = #t
  if n > 1 then
    if n >= 2147483647 then error(bad(1, "sort", "array too big"), 2) end
    if comp ~= nil and type(comp) ~= "function" then
      error(bad(2, "sort", "function expected, got " .. type(comp)), 2)
    end
    local a = {}
    for k = 1, n do a[k] = t[k] end
    if comp == nil then a = sort_plain(a, n) else a = sort_with(a, n, comp) end
    for k = 1, n do t[k] = a[k] end
  end
end

---------------------------------------------------------------------- base

string, table = S, T

setmetatable = function(t, mt)
  if type(mt) == "table" and rawget(mt, "__gc") ~= nil then
    error("the __gc metamethod is not available to an app", 2)
  end
  return relay(pcall(raw_setmetatable, t, mt))
end

tostring = function(v)
  local tv = type(v)
  if tv == "table" or tv == "string" then
    local f = rs_meta(v, "__tostring")
    if f ~= nil then
      local s = f(v)
      local ts = type(s)
      if ts == "number" then return raw_tostring(s) end
      if ts ~= "string" then error("'__tostring' must return a string", 2) end
      return s
    end
  end
  return raw_tostring(v)
end
safe_tostring = tostring

local function inext(t, i)
  i = i + 1
  local v = t[i]
  if v ~= nil then return i, v end
end

ipairs = function(...)
  if select("#", ...) == 0 then error(bad(1, "ipairs", "table expected, got no value"), 2) end
  return inext, (...), 0
end

-- A message handler runs where the error is raised, also inside the hook
-- that stops a script, where nothing can stop the handler. Here it is called
-- after `pcall` has returned, as an ordinary function. A script has no
-- `debug` library, so the stack it could have looked at is no loss.
local function handled(h, ok, ...)
  if ok then return true, ... end
  return false, (h(...))
end

-- The functions of the app API are Rust functions, called directly, so
-- that an error of theirs carries the line of the script that made the
-- call also when that call is a `return ui.column { ... }`. An error raised
-- from Rust is a userdata on its way up. Whoever catches it must get the
-- string it stands for, as from Lua's own library: `"failed: " .. err` has
-- to work. A script catches with `pcall` and `xpcall` and nothing else.
--
-- One error is not handed to the script at all: the hook's own, raised
-- where a yield could not stop the script. It is raised again here, so
-- that no `pcall` of the script swallows it either.
local function caught(ok, e, ...)
  if ok or type(e) ~= "userdata" then return ok, e, ... end
  local text, stop = rs_error(e)
  if stop then error(e, 0) end
  return false, text or e
end

local script_pcall = function(...) return caught(pcall(...)) end

xpcall = function(f, h, ...)
  if type(h) ~= "function" then
    error(bad(2, "xpcall", "function expected, got " .. (h == nil and "no value" or type(h))), 2)
  end
  return handled(h, script_pcall(f, ...))
end

-- "10" + 1: in Lua 5.4 the string metatable does this, in C, and for an
-- operand that is no number it calls that operand's metamethod.
local smt = getmetatable("")
smt.__index = S
local function num(v)
  if type(v) == "string" then
    local n = raw_tonumber(v)
    if n == nil then error("attempt to perform arithmetic on a string value", 3) end
    return n
  end
  return v
end
smt.__add = function(a, b) return num(a) + num(b) end
smt.__sub = function(a, b) return num(a) - num(b) end
smt.__mul = function(a, b) return num(a) * num(b) end
smt.__div = function(a, b) return num(a) / num(b) end
smt.__mod = function(a, b) return num(a) % num(b) end
smt.__pow = function(a, b) return num(a) ^ num(b) end
smt.__idiv = function(a, b) return num(a) // num(b) end
smt.__unm = function(a) return -num(a) end

--------------------------------------------------------------------- entry

-- Every call of the host into a script goes through this function, inside
-- the call's coroutine. An error leaves it as `false, message`, a string:
-- an error *value* with a `__tostring` would otherwise be turned into text
-- by whoever reads it, outside the coroutine, where that code cannot yield.
local function settle(ok, e, ...)
  if ok then return true, e, ... end
  if type(e) ~= "string" then
    local own = type(e) == "userdata" and rs_error(e)
    if own then return false, own end
    local shown, text = pcall(safe_tostring, e)
    if shown and type(text) == "string" then e = text else e = "(an error value that cannot be shown)" end
  end
  return false, e
end

local entry = function(f, ...) return settle(pcall(f, ...)) end

-- The script's `pcall`. In this file `pcall` is a local, and Lua's own.
_G.pcall = script_pcall

return entry
