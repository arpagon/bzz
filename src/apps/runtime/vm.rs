//! One Lua 5.4 interpreter for one script, and the only way into it.
//!
//! What stops a script is three things together (ADR v0.16 apps, decision 6;
//! `docs/validation-v0.16.0.md`, question 2):
//!
//! 1. **The hook yields.** A count hook looks at the clock every
//!    [`HOOK_EVERY`] instructions. Past the deadline it makes the coroutine
//!    of the call yield, and nothing ever resumes that coroutine. A yield is
//!    not an error: no `pcall` of the script can swallow it and no handler
//!    of the script runs because of it.
//! 2. **The bounded library** ([`super::bounded`]) leaves no C function a
//!    script can call that loops longer than its arguments are large, and
//!    none that calls script code.
//! 3. **One way in.** [`Vm::call`] is the only place bzz runs script code.
//!    It starts a coroutine on the library's entry function, which hands an
//!    error back as a string made inside the coroutine. Everything else in
//!    this module tree reads a script's tables raw (`raw_get`, `raw_len`,
//!    `pairs`), which runs no metamethod.
//!
//! A script is loaded as text: `mlua` would run a chunk of bytecode it is
//! handed, and Lua does not check bytecode.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::chunk::ChunkMode;
use mlua::thread::ThreadStatus;
use mlua::{
    Function, HookTriggers, IntoLuaMulti, Lua, LuaOptions, MultiValue, StdLib, Table, Value,
    VmState,
};

use super::bounded::{self, Bounds};

/// Base functions a script does not get: they load code (`load` also takes
/// bytecode), or steer the collector, or write to the terminal bzz draws on.
/// `print` is put back by the app API as a function that writes to the log.
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

/// The hook asks for a yield at the deadline. Where a yield changes nothing
/// (script code that a C function called, which the bounded library is meant
/// to leave none of) it asks this many times and then raises an error
/// instead: the second line.
const YIELD_ATTEMPTS: u32 = 64;

/// What the interpreter's own setup may take.
const SETUP_TIME: Duration = Duration::from_secs(60);

/// What the hook's error says. A script cannot make [`Failure::Stopped`] by
/// raising this text: [`Vm::call`] looks at the deadline too.
const SECOND_LINE: &str = bounded::STOPPED_WHERE_NO_YIELD_HELPS;

/// An error message is cut to this many bytes: a script chooses how long
/// the value it raises is.
const ERROR_BYTES: usize = 2048;

const NO_MEMORY: &str = bounded::NO_MEMORY;

/// Why a call gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The call ran past its time budget and was stopped. `at` is the
    /// script's `file:line` where it was, when that is known.
    Stopped { at: Option<String> },
    /// An allocation would have passed the memory cap.
    Memory,
    /// Any other error of the script, as Lua words it: `file:line: message`.
    Script(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Stopped { at: Some(at) } => write!(f, "{at}: {SECOND_LINE}"),
            Failure::Stopped { at: None } => f.write_str(SECOND_LINE),
            Failure::Memory => f.write_str("the script ran out of the memory it is allowed"),
            Failure::Script(message) => f.write_str(message),
        }
    }
}

/// What the hook and the calls share.
struct Watch {
    /// When the running call must end. The pattern matcher of the bounded
    /// library reads the same cell.
    deadline: Rc<Cell<Instant>>,
    /// How often the hook has fired after the deadline of the running call.
    late: Cell<u32>,
    /// Where the script was when the hook first asked it to stop.
    stopped_at: RefCell<Option<String>>,
}

/// One interpreter. It lives and dies on the thread that made it.
pub struct Vm {
    lua: Lua,
    watch: Rc<Watch>,
    entry: Function,
    budget: Duration,
    /// The prototype's sandbox, for the red run of the hostile suite.
    #[cfg(test)]
    plain: bool,
}

impl Vm {
    /// An interpreter that may hold `memory` bytes and gives each call
    /// `budget`. It has `table`, `string`, `math`, `utf8` and the base
    /// functions, with the bounded library in place and without the names
    /// in [`REMOVED`] and `string.dump`.
    pub fn new(memory: usize, budget: Duration) -> Result<Self, String> {
        Self::build(memory, budget, false)
    }

    /// The same, with Lua's own functions kept in a global `stock` beside
    /// the bounded ones: only for the differential test.
    #[cfg(test)]
    pub fn with_stock(memory: usize, budget: Duration) -> Result<Self, String> {
        Self::build(memory, budget, true)
    }

    fn build(memory: usize, budget: Duration, keep_stock: bool) -> Result<Self, String> {
        let setup = |e: mlua::Error| format!("cannot start Lua: {e}");
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::new(),
        )
        .map_err(setup)?;
        lua.set_memory_limit(memory).map_err(setup)?;
        // Until the first call the deadline is far away: setting up the
        // libraries runs Lua code too, and is not a script's.
        let watch = Rc::new(Watch {
            deadline: Rc::new(Cell::new(Instant::now() + SETUP_TIME)),
            late: Cell::new(0),
            stopped_at: RefCell::new(None),
        });
        let seen = watch.clone();
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_EVERY),
            move |lua, _| {
                if Instant::now() <= seen.deadline.get() {
                    return Ok(VmState::Continue);
                }
                let late = seen.late.get().saturating_add(1);
                seen.late.set(late);
                if late == 1 {
                    *seen.stopped_at.borrow_mut() = script_position(lua);
                }
                if late <= YIELD_ATTEMPTS {
                    Ok(VmState::Yield)
                } else {
                    Err(mlua::Error::runtime(SECOND_LINE))
                }
            },
        )
        .map_err(setup)?;

        let globals = lua.globals();
        for name in REMOVED {
            globals.raw_set(*name, Value::Nil).map_err(setup)?;
        }
        if keep_stock {
            keep_stock_functions(&lua).map_err(setup)?;
        }
        let entry = bounded::install(
            &lua,
            Bounds {
                deadline: watch.deadline.clone(),
                budget_ms: budget.as_millis() as u64,
                memory_limit: memory,
            },
        )
        .map_err(setup)?;
        // `string.dump` turns a function into bytecode. The bounded library
        // gave the script a new `string` table: take it out of that one.
        globals
            .raw_get::<Table>("string")
            .and_then(|string| string.raw_set("dump", Value::Nil))
            .map_err(setup)?;
        Ok(Self {
            lua,
            watch,
            entry,
            budget,
            #[cfg(test)]
            plain: false,
        })
    }

    /// Lua as the prototype of the experiment ran it: the library as Lua
    /// ships it, the names removed, a hook that raises an error, a `pcall`
    /// that raises again after the deadline, and a call on the main state.
    /// Only so that the hostile suite can be seen to fail without the three
    /// parts of [`Vm::new`].
    #[cfg(test)]
    pub fn plain_for_the_red_run(memory: usize, budget: Duration) -> Result<Self, String> {
        let setup = |e: mlua::Error| format!("cannot start Lua: {e}");
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::new(),
        )
        .map_err(setup)?;
        lua.set_memory_limit(memory).map_err(setup)?;
        let watch = Rc::new(Watch {
            deadline: Rc::new(Cell::new(Instant::now() + SETUP_TIME)),
            late: Cell::new(0),
            stopped_at: RefCell::new(None),
        });
        let seen = watch.clone();
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_EVERY),
            move |_, _| {
                if Instant::now() <= seen.deadline.get() {
                    return Ok(VmState::Continue);
                }
                seen.late.set(YIELD_ATTEMPTS + 1);
                Err(mlua::Error::runtime(SECOND_LINE))
            },
        )
        .map_err(setup)?;
        let globals = lua.globals();
        for name in REMOVED {
            globals.raw_set(*name, Value::Nil).map_err(setup)?;
        }
        globals
            .raw_get::<Table>("string")
            .and_then(|string| string.raw_set("dump", Value::Nil))
            .map_err(setup)?;
        let deadline = watch.deadline.clone();
        let expired = lua
            .create_function(move |_, ()| Ok(Instant::now() > deadline.get()))
            .map_err(setup)?;
        let entry: Function = lua
            .load(
                r#"
                local expired, raw_pcall, raw_xpcall, pack, unpack, raise = ...
                pcall = function(f, ...)
                  local r = pack(raw_pcall(f, ...))
                  if not r[1] and expired() then raise(r[2], 0) end
                  return unpack(r, 1, r.n)
                end
                xpcall = function(f, h, ...)
                  local r = pack(raw_xpcall(f, h, ...))
                  if not r[1] and expired() then raise(r[2], 0) end
                  return unpack(r, 1, r.n)
                end
                return function(f, ...) return true, f(...) end
                "#,
            )
            .set_name("=host")
            .call((
                expired,
                globals.raw_get::<Function>("pcall").map_err(setup)?,
                globals.raw_get::<Function>("xpcall").map_err(setup)?,
                globals
                    .raw_get::<Table>("table")
                    .and_then(|t| t.raw_get::<Function>("pack"))
                    .map_err(setup)?,
                globals
                    .raw_get::<Table>("table")
                    .and_then(|t| t.raw_get::<Function>("unpack"))
                    .map_err(setup)?,
                globals.raw_get::<Function>("error").map_err(setup)?,
            ))
            .map_err(setup)?;
        Ok(Self {
            lua,
            watch,
            entry,
            budget,
            plain: true,
        })
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// Bytes the interpreter holds now.
    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    /// A full collection. Abandoned coroutines and what a failed call left
    /// behind are garbage like any other. No script code runs: a script
    /// cannot set a finalizer.
    pub fn collect(&self) {
        let _ = self.lua.gc_collect();
    }

    /// Compiles `source` as the chunk `name`. Text only. Compiling is not
    /// under the time budget: it is linear in the source, whose size the
    /// caller has checked.
    pub fn compile(&self, name: &str, source: &[u8]) -> Result<Function, Failure> {
        self.lua
            .load(source)
            .set_name(format!("@{name}"))
            .set_mode(ChunkMode::Text)
            .into_function()
            .map_err(|e| failure(&e))
    }

    /// Gives the next call a fresh budget.
    fn arm(&self) {
        self.watch.deadline.set(Instant::now() + self.budget);
        self.watch.late.set(0);
        *self.watch.stopped_at.borrow_mut() = None;
    }

    /// Calls `f(args)`: the one way into a script. The call runs in a
    /// coroutine of its own, on the library's entry function, with a fresh
    /// time budget.
    ///
    /// A coroutine that the hook made yield is dropped: never resumed, and
    /// never closed either, because closing would run the script's pending
    /// `__close` handlers where nothing can stop them.
    pub fn call(&self, f: &Function, args: impl IntoLuaMulti) -> Result<MultiValue, Failure> {
        let mut all = args.into_lua_multi(&self.lua).map_err(|e| failure(&e))?;
        #[cfg(test)]
        if self.plain {
            self.arm();
            return match f.call::<MultiValue>(all) {
                Ok(results) => Ok(results),
                Err(_) if self.watch.late.get() > 0 => Err(Failure::Stopped { at: None }),
                Err(e) => Err(failure(&e)),
            };
        }
        all.push_front(Value::Function(f.clone()));
        let thread = self
            .lua
            .create_thread(self.entry.clone())
            .map_err(|e| failure(&e))?;
        self.arm();
        let resumed = thread.resume::<MultiValue>(all);
        // A script has no `coroutine` library: only the hook yields.
        if thread.status() == ThreadStatus::Resumable {
            return Err(self.stopped());
        }
        let mut results = match resumed {
            Ok(results) => results,
            Err(e) => {
                // The second line: the hook raised an error where a yield
                // did nothing, and it came out of the entry function's own
                // `pcall`. Or Lua could not even start the call.
                let late = self.watch.late.get() > YIELD_ATTEMPTS;
                return Err(if late { self.stopped() } else { failure(&e) });
            }
        };
        match results.pop_front() {
            Some(Value::Boolean(true)) => Ok(results),
            _ => {
                let message = match results.pop_front() {
                    Some(Value::String(s)) => {
                        let bytes = s.as_bytes();
                        let shown = &bytes[..bytes.len().min(ERROR_BYTES)];
                        String::from_utf8_lossy(shown).into_owned()
                    }
                    _ => "(no message)".to_string(),
                };
                if self.watch.late.get() > YIELD_ATTEMPTS && message.ends_with(SECOND_LINE) {
                    Err(self.stopped())
                } else if message == NO_MEMORY {
                    Err(Failure::Memory)
                } else {
                    Err(Failure::Script(message))
                }
            }
        }
    }

    fn stopped(&self) -> Failure {
        Failure::Stopped {
            at: self.watch.stopped_at.borrow_mut().take(),
        }
    }

    /// Runs `source` as one chunk and returns its results: load and call.
    pub fn run(&self, name: &str, source: &[u8]) -> Result<MultiValue, Failure> {
        let chunk = self.compile(name, source)?;
        self.call(&chunk, ())
    }
}

/// For the differential test: Lua's own functions, in a global `stock`.
fn keep_stock_functions(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let stock = lua.create_table()?;
    for (library, names) in [
        ("string", &["find", "match", "gmatch", "gsub", "rep"][..]),
        (
            "table",
            &["insert", "remove", "move", "concat", "unpack", "sort"][..],
        ),
    ] {
        let table: Table = globals.raw_get(library)?;
        for name in names {
            stock.raw_set(*name, table.raw_get::<Value>(*name)?)?;
        }
    }
    stock.raw_set("pcall", globals.raw_get::<Value>("pcall")?)?;
    globals.raw_set("stock", stock)
}

/// `file:line` of the innermost frame that is the script's own code (its
/// chunk name starts with `@`), seen from a hook or from a function the
/// script called.
pub fn script_position(lua: &Lua) -> Option<String> {
    for level in 0..24 {
        let found = lua.inspect_stack(level, |debug| {
            let source = debug.source();
            let own = source.source.as_deref().is_some_and(|s| s.starts_with('@'));
            if !own {
                return None;
            }
            let file = source.short_src.map(|s| s.to_string())?;
            let line = debug.current_line()?;
            Some(format!("{file}:{line}"))
        });
        match found {
            Some(Some(at)) => return Some(at),
            Some(None) => continue,
            None => return None,
        }
    }
    None
}

/// An error of `mlua` as a [`Failure`]. Only for what happens outside a
/// call: compiling, and making values. Inside a call the entry function has
/// already made the error a string.
pub fn failure(e: &mlua::Error) -> Failure {
    match e {
        mlua::Error::MemoryError(_) => Failure::Memory,
        mlua::Error::CallbackError { cause, .. } => failure(cause),
        mlua::Error::SyntaxError { message, .. } => Failure::Script(own_words(message)),
        mlua::Error::RuntimeError(message) => Failure::Script(own_words(message)),
        other => Failure::Script(own_words(&other.to_string())),
    }
}

/// The message without the stack `mlua` may append to it.
fn own_words(message: &str) -> String {
    let words = message
        .split("\nstack traceback:")
        .next()
        .unwrap_or(message)
        .trim_start_matches("runtime error: ");
    let mut end = words.len().min(ERROR_BYTES);
    while !words.is_char_boundary(end) {
        end -= 1;
    }
    words[..end].to_string()
}
