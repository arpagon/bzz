//! Apps in the channel (feature `apps`): the interpreter of a script.
//!
//! v0.16.0 M0 holds only what proves that Lua 5.4 runs inside bzz, with the
//! three limits every later milestone stands on. Nothing in bzz calls this
//! yet. [`docs/adr-v0.16-apps.md`](../../docs/adr-v0.16-apps.md) says what a
//! script is and may do.
//!
//! - **No library that reaches outside.** Only `table`, `string`, `math`,
//!   `utf8` and the base functions are loaded, and from those the ones that
//!   load code or write to the terminal are removed. Source is loaded as
//!   text: `mlua` would run a chunk of bytecode it is handed, and Lua does
//!   not check bytecode.
//! - **A memory cap.** An allocation past it is an error in the script.
//! - **A time budget.** A count hook makes the script's coroutine yield at
//!   the deadline, and it is never resumed. A yield is not an error, so no
//!   `pcall` of the script can swallow it.
//!
//! **Not here yet, and needed before a script from outside runs (M1).** The
//! hook runs between instructions of the script. It does not run inside one
//! call of a C function of Lua's own library that loops by itself
//! (`string.find` with a hostile pattern, `string.rep("", 2^62)`,
//! `table.insert` on a table whose length is a lie), and script code that C
//! calls cannot yield. M0 question 2 lists every such call and has the three
//! things that leave none: the bounded library, an entry function that turns
//! a script's error into a string inside the coroutine, and a host that
//! never touches a script's table except raw
//! (`docs/validation-v0.16.0.md`, "Question 2").

#[cfg(test)]
mod widgets_test;

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::chunk::ChunkMode;
use mlua::thread::ThreadStatus;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};

/// Base functions a script does not get: they load code (`load` also takes
/// bytecode), or write to the terminal bzz draws on, or steer the collector.
/// `require` and `package` are never loaded; they are listed so that a test
/// says so.
const REMOVED: &[&str] = &[
    "collectgarbage",
    "dofile",
    "load",
    "loadfile",
    "print",
    "require",
    "warn",
];

/// How many instructions of a script run between two looks at the clock.
const HOOK_EVERY: u32 = 1000;

/// The hook asks for a yield at the deadline. Where a yield is not possible
/// (script code called by a C function) it asks this many times and then
/// raises an error instead.
const YIELD_ATTEMPTS: u32 = 64;

/// Why a script gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The call ran past its time budget and was stopped.
    Stopped,
    /// An allocation would have passed the memory cap.
    Memory,
    /// Any other error of the script, as Lua words it.
    Script(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Stopped => f.write_str("the script ran past its time budget and was stopped"),
            Failure::Memory => f.write_str("the script ran out of the memory it is allowed"),
            Failure::Script(m) => f.write_str(m),
        }
    }
}

/// What a script returned, as far as M0 needs to look at it.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Nil,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    /// A table, a function or anything else: only its type name.
    Other(&'static str),
}

/// One Lua 5.4 interpreter for one script. It lives and dies on the thread
/// that made it.
pub struct Interpreter {
    lua: Lua,
    deadline: Rc<Cell<Instant>>,
    late: Rc<Cell<u32>>,
}

impl Interpreter {
    /// An interpreter that may hold `memory_limit` bytes.
    pub fn new(memory_limit: usize) -> Result<Self, Failure> {
        let setup = |e: mlua::Error| Failure::Script(format!("cannot start Lua: {e}"));
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::new(),
        )
        .map_err(setup)?;
        lua.set_memory_limit(memory_limit).map_err(setup)?;
        let globals = lua.globals();
        for name in REMOVED {
            globals.raw_set(*name, Value::Nil).map_err(setup)?;
        }
        // `string.dump` turns a function into bytecode.
        globals
            .raw_get::<Table>("string")
            .and_then(|string| string.raw_set("dump", Value::Nil))
            .map_err(setup)?;

        let deadline = Rc::new(Cell::new(Instant::now()));
        let late = Rc::new(Cell::new(0u32));
        let (at, asked) = (deadline.clone(), late.clone());
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_EVERY),
            move |_, _| {
                if Instant::now() <= at.get() {
                    return Ok(VmState::Continue);
                }
                asked.set(asked.get().saturating_add(1));
                if asked.get() <= YIELD_ATTEMPTS {
                    Ok(VmState::Yield)
                } else {
                    Err(mlua::Error::runtime(Failure::Stopped.to_string()))
                }
            },
        )
        .map_err(setup)?;
        Ok(Self {
            lua,
            deadline,
            late,
        })
    }

    /// Runs `source` as one chunk named `name` and returns its first result.
    /// The call may take `budget`.
    pub fn eval(&self, name: &str, source: &[u8], budget: Duration) -> Result<Answer, Failure> {
        // Text only: a chunk that starts with Lua's bytecode signature is
        // refused here, by Lua itself.
        let chunk = self
            .lua
            .load(source)
            .set_name(format!("@{name}"))
            .set_mode(ChunkMode::Text)
            .into_function()
            .map_err(failure)?;
        // A coroutine of its own for every call. One that was stopped is
        // dropped: never resumed, and never closed either, because closing
        // would run the script's pending `__close` handlers.
        let thread = self.lua.create_thread(chunk).map_err(failure)?;
        self.late.set(0);
        self.deadline.set(Instant::now() + budget);
        let result = thread.resume::<Value>(());
        // A script has no `coroutine` library: only the hook yields.
        if thread.status() == ThreadStatus::Resumable {
            return Err(Failure::Stopped);
        }
        match result.map_err(failure)? {
            Value::Nil => Ok(Answer::Nil),
            Value::Boolean(b) => Ok(Answer::Boolean(b)),
            Value::Integer(i) => Ok(Answer::Integer(i)),
            Value::Number(n) => Ok(Answer::Number(n)),
            Value::String(s) => Ok(Answer::Text(s.to_string_lossy())),
            other => Ok(Answer::Other(other.type_name())),
        }
    }

    /// Bytes the interpreter holds now.
    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }
}

fn failure(e: mlua::Error) -> Failure {
    match &e {
        mlua::Error::MemoryError(_) => Failure::Memory,
        mlua::Error::CallbackError { cause, .. } => failure(cause.as_ref().clone()),
        mlua::Error::RuntimeError(m) if *m == Failure::Stopped.to_string() => Failure::Stopped,
        mlua::Error::SyntaxError { message, .. } => Failure::Script(own_words(message)),
        mlua::Error::RuntimeError(m) => Failure::Script(own_words(m)),
        other => Failure::Script(own_words(&other.to_string())),
    }
}

/// The message without the stack `mlua` may append to it.
fn own_words(message: &str) -> String {
    message
        .split("\nstack traceback:")
        .next()
        .unwrap_or(message)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1 << 20;
    /// Far more than any script of these tests needs.
    const LONG: Duration = Duration::from_secs(30);

    fn interpreter() -> Interpreter {
        Interpreter::new(16 * MIB).expect("Lua starts")
    }

    fn text(lua: &Interpreter, source: &str) -> String {
        match lua.eval("test.lua", source.as_bytes(), LONG) {
            Ok(Answer::Text(s)) => s,
            other => panic!("{source}: {other:?}"),
        }
    }

    #[test]
    fn a_script_is_evaluated() {
        let lua = interpreter();
        assert_eq!(
            lua.eval("test.lua", b"return 6 * 7", LONG),
            Ok(Answer::Integer(42))
        );
        assert_eq!(text(&lua, "return _VERSION"), "Lua 5.4");
        // The four libraries a script gets, one call of each.
        assert_eq!(
            text(
                &lua,
                r#"
                local words = {}
                for w in ("tres tristes tigres"):gmatch("%a+") do table.insert(words, w:upper()) end
                table.sort(words)
                return table.concat(words, " ") .. " " .. math.max(1, 2) .. " " .. utf8.len("añil")
                "#
            ),
            "TIGRES TRES TRISTES 2 4"
        );
        // A table comes back as what it is, not as a value bzz reads here.
        assert_eq!(
            lua.eval("test.lua", b"return {}", LONG),
            Ok(Answer::Other("table"))
        );
        // An error carries the script's own file and line.
        assert_eq!(
            lua.eval("board.lua", b"local t = nil\nreturn t.x", LONG),
            Err(Failure::Script(
                "board.lua:2: attempt to index a nil value (local 't')".into()
            ))
        );
        assert_eq!(
            lua.eval("board.lua", b"return (", LONG),
            Err(Failure::Script(
                "board.lua:1: unexpected symbol near <eof>".into()
            ))
        );
    }

    #[test]
    fn a_script_has_no_library_that_reaches_outside() {
        let lua = interpreter();
        // Every global there is. A new name here is a new thing a script
        // can reach: it must be looked at before it is added to this list.
        assert_eq!(
            text(
                &lua,
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
                text(&lua, &format!("return type({name})")),
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
            r#"return debug.getregistry()"#,
        ] {
            match lua.eval("test.lua", attempt.as_bytes(), LONG) {
                Err(Failure::Script(m)) => {
                    assert!(m.contains("a nil value"), "{attempt}: {m}")
                }
                other => panic!("{attempt}: {other:?}"),
            }
        }
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

        let lua = interpreter();
        match lua.eval("test.lua", &bytecode, LONG) {
            Err(Failure::Script(m)) => {
                assert!(m.contains("attempt to load a binary chunk"), "{m}")
            }
            other => panic!("bytecode ran: {other:?}"),
        }
    }

    #[test]
    fn the_memory_cap_is_an_error_in_the_script() {
        let lua = interpreter();
        // One allocation that is too large by itself, and many that add up.
        for script in [
            r#"return #string.rep("x", 1 << 30)"#,
            r#"local t = {} while true do t[#t + 1] = string.rep("x", 100000) .. #t end"#,
            r#"local s = "x" while true do s = s .. s end"#,
        ] {
            assert_eq!(
                lua.eval("test.lua", script.as_bytes(), LONG),
                Err(Failure::Memory),
                "{script}"
            );
            assert!(lua.used_memory() <= 16 * MIB, "{}", lua.used_memory());
            // The interpreter is still there, and a script may catch the
            // error and go on.
            assert_eq!(
                lua.eval("test.lua", b"return 1 + 1", LONG),
                Ok(Answer::Integer(2))
            );
        }
        assert_eq!(
            text(
                &lua,
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
        let lua = interpreter();
        let budget = Duration::from_millis(50);
        for script in [
            "while true do end",
            // The stop is not an error: `pcall` has nothing to catch.
            "while true do pcall(function() while true do end end) end",
            "local function f() return f() end return f()",
            "local t = setmetatable({}, { __index = function() while true do end end }) return t.x",
        ] {
            let started = Instant::now();
            assert_eq!(
                lua.eval("test.lua", script.as_bytes(), budget),
                Err(Failure::Stopped),
                "{script}"
            );
            let took = started.elapsed();
            assert!(took >= budget, "{script}: stopped early, after {took:?}");
            // Generous: a loaded CI machine, a debug build of Lua.
            assert!(took < Duration::from_secs(10), "{script}: {took:?}");
            // The same interpreter answers the next call.
            assert_eq!(
                lua.eval("test.lua", b"return 1 + 1", LONG),
                Ok(Answer::Integer(2))
            );
        }
        // A script that ends in time is not touched.
        assert_eq!(
            lua.eval(
                "test.lua",
                b"local n = 0 for i = 1, 100000 do n = n + i end return n",
                LONG
            ),
            Ok(Answer::Integer(5_000_050_000))
        );
    }

    #[test]
    fn a_stop_inside_a_c_function_falls_back_to_an_error() {
        // `table.sort` is C and calls the comparison: that code cannot
        // yield. The hook then raises an error, which stops it too. (M1's
        // bounded library leaves no such place; this is the second line.)
        let lua = interpreter();
        assert_eq!(
            lua.eval(
                "test.lua",
                b"table.sort({ 3, 2, 1 }, function() while true do end end)",
                Duration::from_millis(50)
            ),
            Err(Failure::Stopped)
        );
        assert_eq!(
            lua.eval("test.lua", b"return 1 + 1", LONG),
            Ok(Answer::Integer(2))
        );
    }
}
