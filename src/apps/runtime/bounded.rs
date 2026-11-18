//! The bounded library for Lua 5.4: the Rust half. `prelude.lua` is the
//! other half and says what is replaced and why. Both come from question 2
//! of M0 (`docs/validation-v0.16.0.md`), where they were measured.
//!
//! Every function here answers `false, message` instead of returning an
//! `Err`: an error raised from Rust reaches a script as a userdata, and a
//! script expects the string Lua's own library raises.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use mlua::{Function, Lua, LuaString, MultiValue, Result, Table, Value};

use super::pattern::{self, Capture, Found, Stop};

const PRELUDE: &str = include_str!("prelude.lua");

/// What the bounded functions need from the host: the deadline of the call
/// that is running, its budget in words, and the interpreter's memory cap.
#[derive(Clone)]
pub struct Bounds {
    pub deadline: Rc<Cell<Instant>>,
    pub budget_ms: u64,
    pub memory_limit: usize,
}

fn fail(lua: &Lua, msg: impl AsRef<str>) -> Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![
        Value::Boolean(false),
        Value::String(lua.create_string(msg.as_ref())?),
    ]))
}

fn stopped(lua: &Lua, b: &Bounds, stop: Stop) -> Result<MultiValue> {
    match stop {
        Stop::Error(m) => fail(lua, m),
        Stop::Timeout => fail(
            lua,
            format!(
                "a pattern ran past the budget of {} ms and was stopped",
                b.budget_ms
            ),
        ),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "no value",
        other => other.type_name(),
    }
}

/// A string argument as `luaL_checklstring` reads it: a string or a number.
fn string_arg(
    lua: &Lua,
    v: &Value,
    i: usize,
    name: &str,
) -> std::result::Result<LuaString, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Integer(_) | Value::Number(_) => match lua.coerce_string(v.clone()) {
            Ok(Some(s)) => Ok(s),
            _ => Err(format!(
                "bad argument #{i} to '{name}' (string expected, got number)"
            )),
        },
        other => Err(format!(
            "bad argument #{i} to '{name}' (string expected, got {})",
            type_name(other)
        )),
    }
}

/// An optional integer argument as `luaL_optinteger` reads it.
fn int_arg(
    lua: &Lua,
    v: &Value,
    default: i64,
    i: usize,
    name: &str,
) -> std::result::Result<i64, String> {
    match v {
        Value::Nil => Ok(default),
        Value::Integer(n) => Ok(*n),
        other => match lua.coerce_integer(other.clone()) {
            Ok(Some(n)) => Ok(n),
            _ if matches!(other, Value::Number(_)) => Err(format!(
                "bad argument #{i} to '{name}' (number has no integer representation)"
            )),
            _ => Err(format!(
                "bad argument #{i} to '{name}' (number expected, got {})",
                type_name(other)
            )),
        },
    }
}

fn push_captures(lua: &Lua, out: &mut Vec<Value>, src: &[u8], caps: &[Capture]) -> Result<()> {
    for c in caps {
        out.push(match *c {
            Capture::Text(at, len) => Value::String(lua.create_string(&src[at..at + len])?),
            Capture::Position(p) => Value::Integer(p as i64),
        });
    }
    Ok(())
}

fn find_aux(
    lua: &Lua,
    b: &Bounds,
    args: (Value, Value, Value, Value),
    find: bool,
) -> Result<MultiValue> {
    let name = if find { "find" } else { "match" };
    let (s, p, init, plain) = args;
    let s = match string_arg(lua, &s, 1, name) {
        Ok(s) => s,
        Err(m) => return fail(lua, m),
    };
    let p = match string_arg(lua, &p, 2, name) {
        Ok(p) => p,
        Err(m) => return fail(lua, m),
    };
    let init = match int_arg(lua, &init, 1, 3, name) {
        Ok(i) => i,
        Err(m) => return fail(lua, m),
    };
    let (src, pat) = (s.as_bytes(), p.as_bytes());
    let init = pattern::start_position(init, src.len()) - 1;
    let plain = !matches!(plain, Value::Nil | Value::Boolean(false));
    match pattern::find(&src, &pat, init, plain, find, b.deadline.get()) {
        Err(stop) => stopped(lua, b, stop),
        Ok(None) => Ok(MultiValue::from_vec(vec![Value::Nil])),
        Ok(Some(Found {
            start,
            end,
            captures,
        })) => {
            let mut out = Vec::with_capacity(captures.len() + 2);
            if find {
                out.push(Value::Integer(start as i64 + 1));
                out.push(Value::Integer(end as i64));
            }
            push_captures(lua, &mut out, &src, &captures)?;
            Ok(MultiValue::from_vec(out))
        }
    }
}

/// Puts the bounded library in place of the stock functions. Call it after
/// the standard libraries are loaded and before any script runs.
///
/// Returns the entry function every call into a script goes through:
/// `entry(f, ...)` answers `true, results...` or `false, message`, the
/// message a string made inside the call's coroutine.
pub fn install(lua: &Lua, bounds: Bounds) -> Result<Function> {
    let string: Table = lua.globals().raw_get("string")?;
    let stock_rep: Function = string.raw_get("rep")?;

    let b = bounds.clone();
    let rs_find = lua.create_function(move |lua, args: (Value, Value, Value, Value)| {
        find_aux(lua, &b, args, true)
    })?;
    let b = bounds.clone();
    let rs_match = lua.create_function(move |lua, (s, p, init): (Value, Value, Value)| {
        find_aux(lua, &b, (s, p, init, Value::Nil), false)
    })?;

    let rs_gmatch_args = lua.create_function(|lua, (s, p, init): (Value, Value, Value)| {
        let s = match string_arg(lua, &s, 1, "gmatch") {
            Ok(s) => s,
            Err(m) => return fail(lua, m),
        };
        let p = match string_arg(lua, &p, 2, "gmatch") {
            Ok(p) => p,
            Err(m) => return fail(lua, m),
        };
        let init = match int_arg(lua, &init, 1, 3, "gmatch") {
            Ok(i) => i,
            Err(m) => return fail(lua, m),
        };
        let len = s.as_bytes().len();
        let mut at = pattern::start_position(init, len) - 1;
        if at > len {
            at = len + 1;
        }
        Ok(MultiValue::from_vec(vec![
            Value::String(s),
            Value::String(p),
            Value::Integer(at as i64),
        ]))
    })?;

    let b = bounds.clone();
    let rs_step = lua.create_function(
        move |lua, (s, p, at, last, here, first): (LuaString, LuaString, i64, i64, bool, bool)| {
            let (src, pat) = (s.as_bytes(), p.as_bytes());
            let last = usize::try_from(last).ok();
            match pattern::step(
                &src,
                &pat,
                at.max(0) as usize,
                last,
                here,
                first,
                b.deadline.get(),
            ) {
                Err(stop) => stopped(lua, &b, stop),
                Ok(None) => Ok(MultiValue::from_vec(vec![Value::Nil])),
                Ok(Some(Found {
                    start,
                    end,
                    captures,
                })) => {
                    let mut out = vec![Value::Integer(start as i64), Value::Integer(end as i64)];
                    push_captures(lua, &mut out, &src, &captures)?;
                    Ok(MultiValue::from_vec(out))
                }
            }
        },
    )?;

    let b = bounds.clone();
    let rs_gsub = lua.create_function(
        move |lua, (s, p, repl, max_n): (Value, Value, Value, Value)| {
            let subject = match string_arg(lua, &s, 1, "gsub") {
                Ok(s) => s,
                Err(m) => return fail(lua, m),
            };
            let p = match string_arg(lua, &p, 2, "gsub") {
                Ok(p) => p,
                Err(m) => return fail(lua, m),
            };
            let repl = match string_arg(lua, &repl, 3, "gsub") {
                Ok(r) => r,
                Err(m) => return fail(lua, m),
            };
            let (src, pat, repl) = (subject.as_bytes(), p.as_bytes(), repl.as_bytes());
            let max_n = match int_arg(lua, &max_n, src.len() as i64 + 1, 4, "gsub") {
                Ok(n) => n,
                Err(m) => return fail(lua, m),
            };
            // The result is built outside Lua's memory: it may not be larger
            // than what Lua could still hold.
            let room = b.memory_limit.saturating_sub(lua.used_memory());
            match pattern::gsub(&src, &pat, &repl, max_n, room, b.deadline.get()) {
                Err(stop) => stopped(lua, &b, stop),
                Ok((None, n)) => Ok(MultiValue::from_vec(vec![s, Value::Integer(n)])),
                Ok((Some(out), n)) => match lua.create_string(&out) {
                    Ok(r) => Ok(MultiValue::from_vec(vec![
                        Value::String(r),
                        Value::Integer(n),
                    ])),
                    Err(_) => fail(lua, "not enough memory"),
                },
            }
        },
    )?;

    let rs_gsub_args = lua.create_function(|lua, (s, p, max_n): (Value, Value, Value)| {
        let s = match string_arg(lua, &s, 1, "gsub") {
            Ok(s) => s,
            Err(m) => return fail(lua, m),
        };
        let p = match string_arg(lua, &p, 2, "gsub") {
            Ok(p) => p,
            Err(m) => return fail(lua, m),
        };
        let len = s.as_bytes().len();
        let max_n = match int_arg(lua, &max_n, len as i64 + 1, 4, "gsub") {
            Ok(n) => n,
            Err(m) => return fail(lua, m),
        };
        let pat = p.as_bytes();
        let anchor = pat.first() == Some(&b'^');
        let p = if anchor {
            lua.create_string(&pat[1..])?
        } else {
            p.clone()
        };
        Ok(MultiValue::from_vec(vec![
            Value::String(s),
            Value::String(p),
            Value::Integer(max_n),
            Value::Boolean(anchor),
        ]))
    })?;

    let rs_rep = lua.create_function(move |lua, (s, n, sep): (Value, Value, Value)| {
        let subject = match string_arg(lua, &s, 1, "rep") {
            Ok(s) => s,
            Err(m) => return fail(lua, m),
        };
        let n = match n {
            Value::Nil => {
                return fail(
                    lua,
                    "bad argument #2 to 'rep' (number expected, got no value)",
                );
            }
            n => match int_arg(lua, &n, 0, 2, "rep") {
                Ok(n) => n,
                Err(m) => return fail(lua, m),
            },
        };
        let sep = match sep {
            Value::Nil => None,
            sep => match string_arg(lua, &sep, 3, "rep") {
                Ok(s) => Some(s),
                Err(m) => return fail(lua, m),
            },
        };
        let piece = subject.as_bytes().len() + sep.as_ref().map_or(0, |s| s.as_bytes().len());
        // Stock Lua runs `n` empty copies here: `("").rep(s, 2^62)` never ends.
        if n <= 0 || piece == 0 {
            return Ok(MultiValue::from_vec(vec![Value::String(
                lua.create_string("")?,
            )]));
        }
        if piece
            .checked_mul(n as usize)
            .is_none_or(|total| total > isize::MAX as usize)
        {
            return fail(lua, "resulting string too large");
        }
        match stock_rep.call::<Value>((subject, n, sep)) {
            Ok(v) => Ok(MultiValue::from_vec(vec![v])),
            Err(_) => fail(lua, "not enough memory"),
        }
    })?;

    // The raw field of a value's metatable, whatever `__metatable` says.
    let rs_meta = lua.create_function(|lua, (v, name): (Value, LuaString)| {
        let mt = match &v {
            Value::Table(t) => t.metatable(),
            Value::String(_) => lua.type_metatable::<LuaString>(),
            _ => None,
        };
        match mt {
            Some(mt) => mt.raw_get::<Value>(name),
            None => Ok(Value::Nil),
        }
    })?;

    // What an error raised from Rust says, for whoever catches it; and
    // whether it is the hook's own, which no script may catch.
    let rs_error = lua.create_function(|lua, v: Value| match v {
        Value::Error(e) => {
            let text = error_text(&e);
            let stop = text.ends_with(STOPPED_WHERE_NO_YIELD_HELPS);
            Ok((Some(lua.create_string(&text)?), stop))
        }
        _ => Ok((None, false)),
    })?;

    lua.load(PRELUDE)
        .set_name("=lib")
        .set_mode(mlua::chunk::ChunkMode::Text)
        .call((
            rs_find,
            rs_match,
            rs_gmatch_args,
            rs_step,
            rs_gsub,
            rs_gsub_args,
            rs_rep,
            rs_meta,
            rs_error,
        ))
}

/// What the hook's error says: the second line, raised where a yield
/// changed nothing.
pub const STOPPED_WHERE_NO_YIELD_HELPS: &str = "the call ran past its time budget and was stopped";

/// Lua's own words for an allocation past the cap.
pub const NO_MEMORY: &str = "not enough memory";

/// The words of an error that a Rust function raised: the message alone,
/// without `mlua`'s label before it and its stack after it.
pub fn error_text(e: &mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { cause, .. } => error_text(cause),
        mlua::Error::MemoryError(_) => NO_MEMORY.to_string(),
        mlua::Error::RuntimeError(message) => message.clone(),
        other => other
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
    }
}
