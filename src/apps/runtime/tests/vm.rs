//! The interpreter alone: what a script has, and the three limits every
//! later thing stands on.

use std::time::{Duration, Instant};

use mlua::{Lua, Value};

use crate::apps::runtime::vm::{Failure, Vm};

const MIB: usize = 1 << 20;
/// Far more than any script of these tests needs.
const LONG: Duration = Duration::from_secs(30);

fn vm() -> Vm {
    Vm::new(16 * MIB, LONG).expect("Lua starts")
}

fn first(vm: &Vm, name: &str, source: &str) -> Result<Value, Failure> {
    vm.run(name, source.as_bytes())
        .map(|mut values| values.pop_front().unwrap_or(Value::Nil))
}

fn text(vm: &Vm, source: &str) -> String {
    match first(vm, "test.lua", source) {
        Ok(Value::String(s)) => s.to_string_lossy(),
        other => panic!("{source}: {other:?}"),
    }
}

fn integer(vm: &Vm, source: &str) -> i64 {
    match first(vm, "test.lua", source) {
        Ok(Value::Integer(i)) => i,
        other => panic!("{source}: {other:?}"),
    }
}

fn error(vm: &Vm, name: &str, source: &str) -> String {
    match first(vm, name, source) {
        Err(Failure::Script(message)) => message,
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn a_script_is_evaluated() {
    let vm = vm();
    assert_eq!(integer(&vm, "return 6 * 7"), 42);
    assert_eq!(text(&vm, "return _VERSION"), "Lua 5.4");
    // The four libraries a script gets, one call of each.
    assert_eq!(
        text(
            &vm,
            r#"
            local words = {}
            for w in ("tres tristes tigres"):gmatch("%a+") do table.insert(words, w:upper()) end
            table.sort(words)
            return table.concat(words, " ") .. " " .. math.max(1, 2) .. " " .. utf8.len("añil")
            "#
        ),
        "TIGRES TRES TRISTES 2 4"
    );
    // An error carries the script's own file and line.
    assert_eq!(
        error(&vm, "board.lua", "local t = nil\nreturn t.x"),
        "board.lua:2: attempt to index a nil value (local 't')"
    );
    assert_eq!(
        error(&vm, "board.lua", "return ("),
        "board.lua:1: unexpected symbol near <eof>"
    );
    // So does an error of a function of the bounded library.
    assert_eq!(
        error(&vm, "board.lua", "\n\nlocal s = ('x'):rep()\nreturn s"),
        "board.lua:3: bad argument #2 to 'rep' (number expected, got no value)"
    );
}

#[test]
fn an_error_a_script_catches_is_a_string() {
    let vm = vm();
    // Every way an error can reach a `pcall` of the script: its own
    // `error`, a fault of the language, the bounded library in Lua, the
    // bounded library in Rust, a stopped pattern, and Lua's own C library.
    for raise in [
        r#"error("mine")"#,
        "local t = nil; return t.x",
        "table.insert(1, 2)",
        r#"("x"):find("[")"#,
        r#"("x"):rep(1e100)"#,
        r#"string.format("%d", "x")"#,
        "setmetatable({}, { __gc = function() end })",
        "math.floor({})",
        "utf8.char(-1)",
    ] {
        let source = format!(
            r#"
            local ok, err = pcall(function() {raise} end)
            return tostring(ok) .. "|" .. type(err) .. "|" .. ("failed: " .. err)
            "#
        );
        let seen = text(&vm, &source);
        assert!(seen.starts_with("false|string|failed: "), "{raise}: {seen}");
    }
    // A value that is not a string is the script's own doing, and bzz
    // still words it, inside the call.
    assert_eq!(
        error(&vm, "t.lua", "error({ code = 7 })").split(':').next(),
        Some("table")
    );
    assert_eq!(
        error(
            &vm,
            "t.lua",
            r#"error(setmetatable({}, { __tostring = function() return "worded by the script" end }))"#
        ),
        "worded by the script"
    );
}

#[test]
fn a_script_has_no_library_that_reaches_outside() {
    let vm = vm();
    // Every global of the interpreter alone. A new name here is a new
    // thing a script can reach: it must be looked at before it is added.
    assert_eq!(
        text(
            &vm,
            r#"
            local names = {}
            for name in pairs(_G) do names[#names + 1] = name end
            table.sort(names)
            return table.concat(names, " ")
            "#
        ),
        "_G _VERSION assert error getmetatable ipairs math next pairs pcall rawequal rawget \
         rawlen rawset select setmetatable string table tonumber tostring type utf8 xpcall"
    );
    for name in [
        "io",
        "os",
        "package",
        "debug",
        "coroutine",
        "require",
        "dofile",
        "loadfile",
        "load",
        "collectgarbage",
        "print",
        "warn",
        "string.dump",
    ] {
        assert_eq!(
            text(&vm, &format!("return type({name})")),
            "nil",
            "{name} is there"
        );
    }
    // What each would have been used for.
    for attempt in [
        r#"return io.open("/etc/hostname")"#,
        r#"return os.getenv("HOME")"#,
        r#"return os.execute("true")"#,
        r#"return require("os")"#,
        r#"return dofile("/etc/hostname")"#,
        r#"return load("return 1")()"#,
        r#"return string.dump(function() end)"#,
        r#"return ("x").dump(function() end)"#,
        r#"return debug.getregistry()"#,
        r#"return coroutine.wrap(function() end)"#,
    ] {
        let message = error(&vm, "test.lua", attempt);
        assert!(message.contains("a nil value"), "{attempt}: {message}");
    }
    // The string metatable leads to the script's `string` table and to
    // nothing else: not to Lua's own, which still has `dump`.
    assert_eq!(
        text(
            &vm,
            r#"return tostring(getmetatable("").__index == string) .. " " .. type(getmetatable("").__index.dump)"#
        ),
        "true nil"
    );
}

#[test]
fn bytecode_is_refused() {
    // Bytecode of a harmless function, made by an interpreter that has
    // `string.dump`. Lua does not verify bytecode, so none is loaded.
    let full = Lua::new();
    let bytecode = full
        .load("return 42")
        .into_function()
        .expect("compiles")
        .dump(false);
    assert!(bytecode.starts_with(b"\x1bLua"));
    // What `mlua` does when it is not told the mode: it runs it.
    assert_eq!(full.load(&bytecode[..]).eval::<i64>().ok(), Some(42));

    let vm = vm();
    match vm.run("test.lua", &bytecode) {
        Err(Failure::Script(m)) => assert!(m.contains("attempt to load a binary chunk"), "{m}"),
        other => panic!("bytecode ran: {other:?}"),
    }
}

#[test]
fn the_memory_cap_is_an_error_in_the_script() {
    let vm = vm();
    // One allocation that is too large by itself, and many that add up.
    for script in [
        r#"return #string.rep("x", 1 << 30)"#,
        r#"local t = {} while true do t[#t + 1] = string.rep("x", 100000) .. #t end"#,
        r#"local s = "x" while true do s = s .. s end"#,
    ] {
        assert_eq!(
            first(&vm, "test.lua", script),
            Err(Failure::Memory),
            "{script}"
        );
        assert!(vm.used_memory() <= 16 * MIB, "{}", vm.used_memory());
        // The interpreter is still there.
        vm.collect();
        assert_eq!(integer(&vm, "return 1 + 1"), 2);
    }
    // A script may catch the error and go on.
    assert_eq!(
        text(
            &vm,
            r#"
            local ok, why = pcall(string.rep, "x", 1 << 30)
            return tostring(ok) .. ": " .. tostring(why)
            "#
        ),
        "false: not enough memory"
    );
}

#[test]
fn an_endless_loop_is_stopped_by_time() {
    let budget = Duration::from_millis(50);
    let vm = Vm::new(16 * MIB, budget).expect("Lua starts");
    for script in [
        "while true do end",
        // The stop is not an error: `pcall` has nothing to catch.
        "while true do pcall(function() while true do end end) end",
        "local function f() return f() end return f()",
        "local t = setmetatable({}, { __index = function() while true do end end }) return t.x",
        // Script code that Lua's own C library would have called.
        "table.sort({ 3, 2, 1 }, function() while true do end end)",
        "return tostring(setmetatable({}, { __tostring = function() while true do end end }))",
    ] {
        let started = Instant::now();
        let stopped = first(&vm, "test.lua", script);
        let took = started.elapsed();
        assert!(
            matches!(stopped, Err(Failure::Stopped { .. })),
            "{script}: {stopped:?}"
        );
        assert!(took >= budget, "{script}: stopped early, after {took:?}");
        // Generous: a loaded machine, a debug build of Lua.
        assert!(took < Duration::from_secs(10), "{script}: {took:?}");
        // The same interpreter answers the next call.
        assert_eq!(integer(&vm, "return 1 + 1"), 2);
    }
    // A stop says where the script was.
    assert_eq!(
        first(
            &vm,
            "board.lua",
            "local n = 0\nwhile true do\n  n = n + 1\nend"
        ),
        Err(Failure::Stopped {
            at: Some("board.lua:3".into())
        })
    );
    // A script that ends in time is not touched.
    assert_eq!(
        integer(
            &vm,
            "local n = 0 for i = 1, 100000 do n = n + i end return n"
        ),
        5_000_050_000
    );
}

#[test]
fn a_script_cannot_fake_a_stop() {
    // The words of the hook's own error, raised by the script: it is an
    // error of the script, and the interpreter is not counted as stopped.
    let vm = vm();
    assert_eq!(
        first(
            &vm,
            "t.lua",
            r#"error("the call ran past its time budget and was stopped", 0)"#
        ),
        Err(Failure::Script(
            "the call ran past its time budget and was stopped".into()
        ))
    );
    assert_eq!(
        first(&vm, "t.lua", r#"error("not enough memory")"#),
        Err(Failure::Script("t.lua:1: not enough memory".into()))
    );
}

#[test]
fn a_long_error_message_is_cut() {
    let vm = vm();
    let message = error(&vm, "t.lua", r#"error(string.rep("x", 4000000), 0)"#);
    assert_eq!(message.len(), 2048);
}

#[test]
fn a_thousand_stops_in_a_row_leave_the_interpreter_as_it_was() {
    // A stopped call's coroutine is abandoned: never resumed, never closed.
    // M0 left open what a long run of them costs. Each is garbage like any
    // other: after a thousand the interpreter holds what it held, and
    // answers.
    let vm = Vm::new(16 * MIB, Duration::from_millis(3)).expect("Lua starts");
    let script = r#"
        local held = setmetatable({}, { __close = function() while true do end end })
        local kept <close> = held
        local t = {}
        while true do t[#t % 100 + 1] = { #t } end
    "#;
    let chunk = vm
        .compile("stuck.lua", script.as_bytes())
        .expect("compiles");
    let mut used = Vec::new();
    for round in 0..1000 {
        let stopped = vm.call(&chunk, ());
        assert!(
            matches!(stopped, Err(Failure::Stopped { .. })),
            "{round}: {stopped:?}"
        );
        if round % 100 == 99 {
            vm.collect();
            used.push(vm.used_memory());
        }
    }
    // Memory after every hundred stops: flat, not a hundred coroutines
    // more each time.
    let (least, most) = (
        used.iter().copied().min().unwrap_or(0),
        used.iter().copied().max().unwrap_or(0),
    );
    assert!(most - least < 64 * 1024, "{used:?}");
    assert_eq!(integer(&vm, "return 1 + 1"), 2);
}
