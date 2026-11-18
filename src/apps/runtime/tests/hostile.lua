-- Hostile scripts for question 2 of M0: each one tries to run past the time
-- budget or the memory cap of its interpreter. A script starts at a line
-- `-- @case name [engine] : what it tries` and runs alone, in a process of
-- its own (`q2-lua54 suite <variant> hostile.lua`, `q2-luau suite luau
-- hostile.lua`). `[lua54]` and `[luau]` mark a script only one of them can
-- parse or run.
--
-- A script passes when its call ends within the budget plus the grace, by
-- an error or by returning, and the interpreter can be used or replaced
-- afterwards. Three groups: loops in the script's own code, which a count
-- hook stops; single calls of a library function, which a hook never sees;
-- and memory.

---------------------------------------------------------------- group 1: loops

-- @case loop : an endless loop
while true do end

-- @case loop-pcall : an endless loop that catches the stop and goes on
while true do pcall(function() while true do end end) end

-- @case loop-xpcall-handler : an endless loop in a message handler
while true do
  xpcall(function() error("x") end, function() while true do end end)
end

-- @case loop-in-metamethod : an endless loop in __index
local t = setmetatable({}, { __index = function() while true do end end })
return t.x

-- @case loop-in-tostring : an endless loop in __tostring, called by C
return tostring(setmetatable({}, { __tostring = function() while true do end end }))

-- @case loop-in-comparator : an endless loop in the function table.sort calls
table.sort({ 3, 2, 1 }, function() while true do end end)

-- @case loop-in-gsub-callback : an endless loop in the function gsub calls
return (("abc"):gsub(".", function() while true do end end))

-- @case xpcall-handler-at-the-stop : the function loops, and so does its message handler when the stop reaches it
xpcall(function() while true do end end, function() while true do end end)

-- @case error-object-tostring : an error value whose __tostring is an endless loop
error(setmetatable({}, { __tostring = function() while true do end end }))

-- @case loop-in-format : an endless loop in __tostring, called by string.format
return string.format("%s", setmetatable({}, { __tostring = function() while true do end end }))

-- @case loop-in-ipairs : an endless loop in __index, called by the iterator of ipairs
for _ in ipairs(setmetatable({}, { __index = function() while true do end end })) do end

-- @case loop-in-string-arithmetic : an endless loop in __add, reached through "10" + object
return "10" + setmetatable({}, { __add = function() while true do end end })

-- @case loop-in-len : an endless loop in __len, called by table.insert
table.insert(setmetatable({}, { __len = function() while true do end end }), 1)

-- @case loop-in-string-index : the string metatable's __index made an endless loop
getmetatable("").__index = function() while true do end end
return ("x"):upper()

-- @case loop-in-global-index : an endless loop in the __index of the globals
setmetatable(_G, { __index = function() while true do end end })
return undefined_name

-- @case recursion : recursion without end
local function f(i) return 1 + f(i + 1) end
return f(1)

-- @case recursion-pcall : recursion through pcall, which is a C boundary each time
local function f() return pcall(f) end
return f()

-- @case close-handlers [lua54] : thousands of to-be-closed variables, each handler an endless loop
local mt = { __close = function() while true do end end }
local function f(n)
  local a <close> = setmetatable({}, mt)
  local b <close> = setmetatable({}, mt)
  if n > 0 then return f(n - 1) end
  while true do end
end
f(20000)

-- @case close-handler [lua54] : one to-be-closed variable whose handler is an endless loop
local a <close> = setmetatable({}, { __close = function() while true do end end })
while true do end

-- @case close-handler-normal-exit [lua54] : the same handler, reached by leaving the block
do
  local a <close> = setmetatable({}, { __close = function() while true do end end })
end

-- @case close-handler-in-for [lua54] : the fourth value of a generic for is closed like a variable
local closing = setmetatable({}, { __close = function() while true do end end })
for _ in function() return 1 end, nil, nil, closing do
  while true do end
end

-- @case close-handler-after-error [lua54] : an error is raised, and the handler it runs has its own endless handler
local inner = { __close = function() while true do end end }
local outer = { __close = function()
  local b <close> = setmetatable({}, inner)
  while true do end
end }
pcall(function()
  local a <close> = setmetatable({}, outer)
  error("x")
end)

-- @case close-handler-in-tostring [lua54] : a to-be-closed variable inside __tostring, called by tostring
local evil = { __close = function() while true do end end }
return tostring(setmetatable({}, { __tostring = function()
  local a <close> = setmetatable({}, evil)
  while true do end
end }))

-- @case close-handler-in-comparator [lua54] : a to-be-closed variable inside the function table.sort calls
local evil = { __close = function() while true do end end }
table.sort({ 3, 2, 1 }, function()
  local a <close> = setmetatable({}, evil)
  while true do end
end)

-- @case close-handler-in-error-value [lua54] : the call fails with a value whose __tostring holds a to-be-closed variable: who reads the error runs it
local evil = { __close = function() while true do end end }
error(setmetatable({}, { __tostring = function()
  local a <close> = setmetatable({}, evil)
  while true do end
end }))

-- @case close-handler-added-later [lua54] : __close put in the metatable after setmetatable
local mt = {}
local v = setmetatable({}, mt)
mt.__close = function() while true do end end
local a <close> = v
while true do end

-- @case gc-finalizer [lua54] : an endless loop in a finalizer; Lua runs finalizers with hooks off
setmetatable({}, { __gc = function() while true do end end })
local junk
for i = 1, 5000000 do junk = { i } end
return "the finalizer was never run"

-- @case gc-finalizer-at-close [lua54] : the same finalizer, on an object that lives until the interpreter is dropped
KEEP = setmetatable({}, { __gc = function() while true do end end })
return "kept"

-- @case gc-finalizer-set-later [lua54] : __gc first a placeholder, a function afterwards
local mt = { __gc = true }
setmetatable({}, mt)
mt.__gc = function() while true do end end
local junk
for i = 1, 5000000 do junk = { i } end
return "the finalizer was never run"

------------------------------------------------ group 2: one call of a C function

-- @case pattern-find : the prototype's case, four lazy quantifiers over 300,000 characters
return string.rep("a", 300000):find(".-.-.-.-x")

-- @case pattern-match : five captures that can each take anything
return ("a"):rep(5000):match("(.-)(.-)(.-)(.-)(.-)b")

-- @case pattern-gmatch : the same through gmatch
for _ in ("a"):rep(100000):gmatch(".-.-.-.-x") do end

-- @case pattern-gsub : the same through gsub
return (("a"):rep(100000):gsub(".-.-.-.-x", ""))

-- @case pattern-greedy : eight greedy quantifiers
return ("a"):rep(3000):find("a*a*a*a*a*a*a*a*b")

-- @case pattern-backreference : a back-reference tried at every length
return ("a"):rep(30000):find("(a*)%1%1%1b")

-- @case pattern-balance : %b() from every position of 200,000 open brackets
return ("("):rep(200000):find("%b()x")

-- @case pattern-short-subject : 60 characters are enough when the pattern is long
return ("a"):rep(60):find(("a*"):rep(30) .. "b")

-- @case find-plain-quadratic : a plain search, no pattern: 4 MiB of a, a needle of 2 MiB of a and one b
local n = 4 * 1024 * 1024
local s = ("a"):rep(n)
local needle = ("a"):rep(n / 2) .. "b"
return s:find(needle, 1, true)

-- @case rep-empty : the empty string, as many times as an integer can say
return #string.rep("", math.maxinteger or 2147483647)

-- @case rep-empty-separator : the same with an empty separator
return #string.rep("", math.maxinteger or 2147483647, "")

-- @case table-move-range : move a range of every integer inside an empty table
return table.move({}, 1, math.maxinteger or 2147483646, 1)

-- @case table-insert-len : insert at 1 into a table whose __len says it is huge
local MAX = (math.maxinteger or 9007199254740992) - 1
return table.insert(setmetatable({}, { __len = function() return MAX end }), 1, "x")

-- @case table-remove-len : remove at 1 from the same table
local MAX = (math.maxinteger or 9007199254740992) - 1
return table.remove(setmetatable({}, { __len = function() return MAX end }), 1)

-- @case table-insert-sparse [lua54] : no metatable at all: 62 keys in the hash part make the length 2^61
local t = { [1] = 1, [2] = 1, [4] = 1, [8] = 1, [16] = 1, [32] = 1, [64] = 1, [128] = 1, [256] = 1, [512] = 1, [1024] = 1, [2048] = 1, [4096] = 1, [8192] = 1, [16384] = 1, [32768] = 1, [65536] = 1, [131072] = 1, [262144] = 1, [524288] = 1, [1048576] = 1, [2097152] = 1, [4194304] = 1, [8388608] = 1, [16777216] = 1, [33554432] = 1, [67108864] = 1, [134217728] = 1, [268435456] = 1, [536870912] = 1, [1073741824] = 1, [2147483648] = 1, [4294967296] = 1, [8589934592] = 1, [17179869184] = 1, [34359738368] = 1, [68719476736] = 1, [137438953472] = 1, [274877906944] = 1, [549755813888] = 1, [1099511627776] = 1, [2199023255552] = 1, [4398046511104] = 1, [8796093022208] = 1, [17592186044416] = 1, [35184372088832] = 1, [70368744177664] = 1, [140737488355328] = 1, [281474976710656] = 1, [562949953421312] = 1, [1125899906842624] = 1, [2251799813685248] = 1, [4503599627370496] = 1, [9007199254740992] = 1, [18014398509481984] = 1, [36028797018963968] = 1, [72057594037927936] = 1, [144115188075855872] = 1, [288230376151711744] = 1, [576460752303423488] = 1, [1152921504606846976] = 1, [2305843009213693952] = 1 }
return table.insert(t, 1, "x")

-- @case table-remove-sparse [lua54] : remove at 1 from the same table
local t = { [1] = 1, [2] = 1, [4] = 1, [8] = 1, [16] = 1, [32] = 1, [64] = 1, [128] = 1, [256] = 1, [512] = 1, [1024] = 1, [2048] = 1, [4096] = 1, [8192] = 1, [16384] = 1, [32768] = 1, [65536] = 1, [131072] = 1, [262144] = 1, [524288] = 1, [1048576] = 1, [2097152] = 1, [4194304] = 1, [8388608] = 1, [16777216] = 1, [33554432] = 1, [67108864] = 1, [134217728] = 1, [268435456] = 1, [536870912] = 1, [1073741824] = 1, [2147483648] = 1, [4294967296] = 1, [8589934592] = 1, [17179869184] = 1, [34359738368] = 1, [68719476736] = 1, [137438953472] = 1, [274877906944] = 1, [549755813888] = 1, [1099511627776] = 1, [2199023255552] = 1, [4398046511104] = 1, [8796093022208] = 1, [17592186044416] = 1, [35184372088832] = 1, [70368744177664] = 1, [140737488355328] = 1, [281474976710656] = 1, [562949953421312] = 1, [1125899906842624] = 1, [2251799813685248] = 1, [4503599627370496] = 1, [9007199254740992] = 1, [18014398509481984] = 1, [36028797018963968] = 1, [72057594037927936] = 1, [144115188075855872] = 1, [288230376151711744] = 1, [576460752303423488] = 1, [1152921504606846976] = 1, [2305843009213693952] = 1 }
return table.remove(t, 1)

-- @case table-concat-c-index : __index is a C function that returns "", so nothing grows
local MAX = math.maxinteger or 2147483647
local t = setmetatable({}, { __index = table.concat })
return #table.concat(t, "", 1, MAX)

-- @case table-sort-c-proxy : two billion zeros read and written through C metamethods
local t = setmetatable({}, { __index = rawlen, __newindex = rawequal, __len = function() return 2147483646 end })
table.sort(t)

-- @case table-sort-c-comparator : two million numbers, compared by a C function: no instruction of the script runs
local t = {}
for i = 1, 2000000 do t[i] = i end
table.sort(t, rawequal)

-- @case table-unpack-range : ask for more results than the stack can hold
return table.unpack({}, 1, 100000000)

-- @case tonumber-long-mantissa : a number with four million digits after the point
return tonumber("1." .. ("9"):rep(4000000))

-- @case arithmetic-on-long-string : the same string, coerced by an addition
return ("1." .. ("9"):rep(4000000)) + 0

-- @case tonumber-long-exponent : a long run of zeros before a digit and an exponent
return tonumber("0." .. ("0"):rep(4000000) .. "1e4000000")

---------------------------------------------- group 2b: linear calls, at full size

-- @case scale-rep : one string of 28 MiB
return #string.rep("x", 28 * 1024 * 1024)

-- @case scale-upper : upper of 12 MiB
return #string.rep("x", 12 * 1024 * 1024):upper()

-- @case scale-reverse : reverse of 12 MiB
return #string.rep("xy", 6 * 1024 * 1024):reverse()

-- @case scale-format-q : %q of 12 MiB
return #string.format("%q", string.rep("x\n", 6 * 1024 * 1024))

-- @case scale-format-s : %s of 12 MiB
return #string.format("%s", string.rep("x", 12 * 1024 * 1024))

-- @case scale-utf8-len : utf8.len of 24 MiB of two-byte characters
return utf8.len(string.rep("é", 12 * 1024 * 1024))

-- @case scale-concat-operator : .. of two strings of 24 MiB
local s = string.rep("x", 24 * 1024 * 1024)
return #(s .. "y")

-- @case scale-compare : < between two strings of 24 MiB that differ at the end
local a = string.rep("x", 24 * 1024 * 1024)
local b = a .. "y"
return a < b

-- @case scale-find-plain : a plain search through 28 MiB
return string.rep("x", 28 * 1024 * 1024):find("y", 1, true)

-- @case scale-tonumber-hex : tonumber of 24 MiB of hexadecimal digits
return tonumber("0x" .. string.rep("f", 24 * 1024 * 1024))

-- @case scale-table-concat : table.concat of 400,000 short strings
local t = {}
for i = 1, 400000 do t[i] = "item" end
return #table.concat(t, ",")

-- @case scale-table-sort : table.sort of 100,000 numbers
local t = {}
local x = 1
for i = 1, 100000 do x = (x * 16807) % 2147483647; t[i] = x end
table.sort(t)
return t[1] <= t[2]

-- @case scale-gsub : gsub over 8 MiB, one replacement per character
return #(string.rep("x", 8 * 1024 * 1024):gsub("x", "y"))

------------------------------------------------------------ group 3: memory

-- @case memory-table-growth : the prototype's case: megabyte strings kept in a table
local t = {}
while true do t[#t + 1] = string.rep("x", 1000000) .. #t end

-- @case memory-doubling : a string that doubles
local s = "x"
while true do s = s .. s end

-- @case memory-one-allocation : one string of 4 GiB
return #string.rep("x", 4 * 1024 * 1024 * 1024)

-- @case memory-one-allocation-huge : one string of 2^60 bytes
return #string.rep("x", 2 ^ 60)

-- @case memory-format : four times 20 MiB through string.format
local big = string.rep("x", 20 * 1024 * 1024)
return #string.format("%s%s%s%s", big, big, big, big)

-- @case memory-gsub-growth : every one of a million characters replaced by a megabyte
return #(("a"):rep(1048576):gsub("a", ("b"):rep(1048576)))

-- @case memory-gsub-growth-function : the same with a function
local mb = ("b"):rep(1048576)
return #(("a"):rep(1048576):gsub("a", function() return mb end))

-- @case memory-table-concat : a hundred megabytes joined
local t = {}
local mb = string.rep("x", 1048576)
for i = 1, 100 do t[i] = mb end
return #table.concat(t)

-- @case memory-upper : upper of 40 MiB needs 40 MiB more
return #string.rep("x", 40 * 1024 * 1024):upper()

-- @case memory-deep-table : tables nested without end
local root = {}
local c = root
while true do c[1] = {}; c = c[1] end

-- @case memory-many-keys : string keys without end
local t = {}
local i = 0
while true do i = i + 1; t["key" .. i] = i end

-- @case memory-varargs : arguments that grow with each call
local function f(...) return f(1, 2, 3, 4, 5, 6, 7, 8, ...) end
return f()

-- @case memory-error-value : an error whose message is 40 MiB
error(string.rep("x", 40 * 1024 * 1024))

-- @case memory-closures : closures without end
local t = {}
while true do local n = #t; t[n + 1] = function() return n end end

-- @case memory-table-array : one table asked to hold 2^40 slots
local t = {}
t[2 ^ 40] = 1
for i = 1, 2 ^ 40 do t[i] = i end
