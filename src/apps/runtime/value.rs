//! Values across the boundary of an interpreter: a script's tables to JSON
//! (content, tags), JSON to tables (replies, pushes), and a state table from
//! one interpreter into another (reload).
//!
//! Everything here reads a script's tables raw and stops at a bound. A
//! script chooses how large and how deep a table is; bzz's time on it must
//! not follow.

use std::collections::HashMap;
use std::time::Instant;

use mlua::{Lua, Table, Value};
use serde_json::{Map, Number, Value as Json};

use super::records::{Record, Who};

/// Why a script's value did not become JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The value cannot be JSON at all: the words say what and where.
    Bad(String),
    /// It would be more than the bytes allowed.
    TooLarge,
}

/// How much one conversion may produce.
pub struct Room {
    /// Bytes of JSON text still allowed; a lower bound is counted as the
    /// value is read, so that the work stops early.
    bytes: usize,
    depth: usize,
}

impl Room {
    pub fn new(bytes: usize, depth: usize) -> Self {
        Self { bytes, depth }
    }

    fn take(&mut self, n: usize) -> Result<(), Refused> {
        if n > self.bytes {
            return Err(Refused::TooLarge);
        }
        self.bytes -= n;
        Ok(())
    }
}

/// `data.null`: the script's way to write a JSON null inside a table, where
/// a nil field would simply be absent. One value in every interpreter.
pub fn is_null(v: &Value) -> bool {
    matches!(v, Value::LightUserData(p) if p.0.is_null())
}

/// A script's value as JSON, of at most `bytes` of text and `depth` levels.
/// A table whose keys are exactly 1..n is an array, any other an object. An
/// empty table is `[]`. `what` names the value in the error.
pub fn to_json(v: &Value, what: &str, bytes: usize, depth: usize) -> Result<Json, Refused> {
    let mut room = Room::new(bytes, depth);
    let json = to_json_at(v, what, 0, &mut room)?;
    // The count above is a lower bound; this is the text itself.
    match serde_json::to_string(&json) {
        Ok(text) if text.len() <= bytes => Ok(json),
        _ => Err(Refused::TooLarge),
    }
}

/// The content of a record: a table with named fields. An empty table is
/// `{}` here.
pub fn to_content(v: &Value, bytes: usize, depth: usize) -> Result<Map<String, Json>, Refused> {
    match to_json(v, "content", bytes, depth)? {
        Json::Object(map) => Ok(map),
        Json::Array(items) if items.is_empty() => Ok(Map::new()),
        _ => Err(Refused::Bad(format!(
            "'content' must be a table with named fields, got {}",
            describe(v)
        ))),
    }
}

fn describe(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Table(_) => "a list".into(),
        _ if is_null(v) => "data.null".into(),
        other => other.type_name().into(),
    }
}

fn text_of(s: &mlua::LuaString, path: &str) -> Result<String, Refused> {
    match s.to_str() {
        Ok(text) => Ok(text.to_string()),
        Err(_) => Err(Refused::Bad(format!(
            "{path} is not valid UTF-8 text (cut a string with utf8.offset or ui.truncate, not by bytes)"
        ))),
    }
}

fn to_json_at(v: &Value, path: &str, depth: usize, room: &mut Room) -> Result<Json, Refused> {
    if depth > room.depth {
        return Err(Refused::Bad(format!(
            "{path} is nested deeper than {} levels (a cycle?)",
            room.depth
        )));
    }
    Ok(match v {
        Value::Nil => {
            room.take(4)?;
            Json::Null
        }
        _ if is_null(v) => {
            room.take(4)?;
            Json::Null
        }
        Value::Boolean(b) => {
            room.take(4)?;
            Json::Bool(*b)
        }
        Value::Integer(i) => {
            room.take(1)?;
            Json::Number((*i).into())
        }
        Value::Number(n) => {
            room.take(1)?;
            Number::from_f64(*n)
                .map(Json::Number)
                .ok_or_else(|| Refused::Bad(format!("{path} is {n}, which JSON cannot hold")))?
        }
        Value::String(s) => {
            room.take(s.as_bytes().len() + 2)?;
            Json::String(text_of(s, path)?)
        }
        Value::Table(t) => {
            room.take(2)?;
            let n = t.raw_len();
            // Each entry is at least a byte and a comma.
            let mut count = 0usize;
            let mut sequence = true;
            for pair in t.pairs::<Value, Value>() {
                let (k, _) = pair.map_err(|e| Refused::Bad(e.to_string()))?;
                count += 1;
                room.take(2)?;
                match k {
                    Value::Integer(i) if i >= 1 && (i as u64) <= n as u64 => {}
                    _ => sequence = false,
                }
            }
            if sequence && count == n {
                let mut items = Vec::with_capacity(n);
                for i in 1..=n {
                    let item: Value = t.raw_get(i).map_err(|e| Refused::Bad(e.to_string()))?;
                    items.push(to_json_at(&item, &format!("{path}[{i}]"), depth + 1, room)?);
                }
                Json::Array(items)
            } else {
                let mut map = Map::new();
                for pair in t.pairs::<Value, Value>() {
                    let (k, item) = pair.map_err(|e| Refused::Bad(e.to_string()))?;
                    let key = match &k {
                        Value::String(s) => text_of(s, &format!("a key of {path}"))?,
                        Value::Integer(i) => i.to_string(),
                        other => {
                            return Err(Refused::Bad(format!(
                                "{path} has a key of type {} (only strings and whole numbers)",
                                other.type_name()
                            )));
                        }
                    };
                    room.take(key.len() + 1)?;
                    let child = to_json_at(&item, &format!("{path}.{key}"), depth + 1, room)?;
                    map.insert(key, child);
                }
                Json::Object(map)
            }
        }
        other => {
            return Err(Refused::Bad(format!(
                "{path} is a {}, which cannot be stored as data",
                other.type_name()
            )));
        }
    })
}

/// JSON as a script's value. `null` becomes `nil` (so `if c.field then`
/// keeps working) and never `data.null`; arrays become sequences from 1. A
/// number written without a point or exponent becomes a Lua integer, every
/// other number a float.
pub fn to_lua(lua: &Lua, v: &Json) -> mlua::Result<Value> {
    Ok(match v {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Value::String(lua.create_string(s)?),
        Json::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        Json::Object(map) => Value::Table(map_to_lua(lua, map)?),
    })
}

/// The content a script writes back over the revision it read, with what
/// Lua could not show it put back as it was read (F1, v0.17).
///
/// A script is handed a `null` as nil and an empty object as an empty
/// table, and nothing it writes can tell those from a field it left out or
/// an empty list; a whole number past what a Lua integer holds is a float
/// to it. So, where the script's value is exactly what it was handed:
///
/// - a field that held `null` and that the script does not name stays
///   `null` (in an object; in a list, a hole at a place that held `null`);
/// - an empty table where the record held `{}` (or an object of nulls)
///   is written as it was read;
/// - a number equal to the one read is written as it was read.
///
/// Whatever the script changed is written as it wrote it.
pub fn as_read(read: &Map<String, Json>, written: Map<String, Json>) -> Map<String, Json> {
    match restore(&Json::Object(read.clone()), Json::Object(written)) {
        Json::Object(map) => map,
        // An object over an object stays one.
        _ => Map::new(),
    }
}

fn is_nothing(v: &Json) -> bool {
    v.is_null()
}

fn restore(read: &Json, written: Json) -> Json {
    match (read, written) {
        (Json::Object(before), Json::Object(mut after)) => {
            for (key, was) in before {
                match after.remove(key) {
                    Some(now) => {
                        after.insert(key.clone(), restore(was, now));
                    }
                    None if is_nothing(was) => {
                        after.insert(key.clone(), Json::Null);
                    }
                    None => {}
                }
            }
            Json::Object(after)
        }
        // `{}`, or an object of nulls, was handed over as an empty table.
        (Json::Object(before), Json::Array(after))
            if after.is_empty() && before.values().all(is_nothing) =>
        {
            Json::Object(before.clone())
        }
        (Json::Array(before), Json::Array(after)) => {
            // A list that ended in nulls is a shorter sequence in Lua.
            let tail = before.get(after.len()..).unwrap_or_default();
            let mut items: Vec<Json> = after
                .into_iter()
                .enumerate()
                .map(|(i, now)| match before.get(i) {
                    Some(was) => restore(was, now),
                    None => now,
                })
                .collect();
            if !tail.is_empty() && tail.iter().all(is_nothing) {
                items.extend(tail.iter().cloned());
            }
            Json::Array(items)
        }
        // A list with a null inside is a table with a hole: an object of
        // the places that hold something.
        (Json::Array(before), Json::Object(after))
            if !after.is_empty()
                && after.keys().all(|key| {
                    key.parse::<usize>()
                        .is_ok_and(|i| i >= 1 && i <= before.len() && key == &i.to_string())
                })
                && before.iter().enumerate().all(|(i, was)| {
                    after.contains_key(&(i + 1).to_string()) || is_nothing(was)
                }) =>
        {
            let mut after = after;
            Json::Array(
                before
                    .iter()
                    .enumerate()
                    .map(|(i, was)| match after.remove(&(i + 1).to_string()) {
                        Some(now) => restore(was, now),
                        None => Json::Null,
                    })
                    .collect(),
            )
        }
        (Json::Number(before), Json::Number(after)) => {
            // Two whole numbers are compared as such: past 2^53 two of them
            // can be one float.
            let same = match (before.as_i64(), after.as_i64()) {
                (Some(a), Some(b)) => a == b,
                _ => match (before.as_u64(), after.as_u64()) {
                    (Some(a), Some(b)) => a == b,
                    _ => before.as_f64().is_some() && before.as_f64() == after.as_f64(),
                },
            };
            if same {
                Json::Number(before.clone())
            } else {
                Json::Number(after)
            }
        }
        (_, written) => written,
    }
}

pub fn map_to_lua(lua: &Lua, map: &Map<String, Json>) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, map.len())?;
    for (key, item) in map {
        t.raw_set(key.as_str(), to_lua(lua, item)?)?;
    }
    Ok(t)
}

pub fn who_to_lua(lua: &Lua, who: &Who) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, 2)?;
    t.raw_set("id", who.id.as_str())?;
    t.raw_set("name", who.name.as_str())?;
    Ok(t)
}

/// A record as the contract's `Record` table.
pub fn record_to_lua(lua: &Lua, record: &Record) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, 5)?;
    t.raw_set("id", record.id.as_str())?;
    t.raw_set("type", record.record_type.as_str())?;
    t.raw_set("rev", record.rev.as_str())?;
    t.raw_set("content", map_to_lua(lua, &record.content)?)?;
    t.raw_set("by", who_to_lua(lua, &record.by)?)?;
    Ok(t)
}

/// A value for a log line: no metamethod is called, and nothing longer than
/// `bytes` is made.
pub fn plain(v: &Value, bytes: usize, depth: usize) -> String {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            String::from_utf8_lossy(&b[..b.len().min(bytes)]).into_owned()
        }
        Value::Nil => "nil".into(),
        _ if is_null(v) => "data.null".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Table(_) => match to_json(v, "value", bytes, depth) {
            Ok(json) => json.to_string(),
            Err(_) => "table".into(),
        },
        other => other.type_name().to_string(),
    }
}

/// Copies a value from one interpreter into another, keeping shared tables
/// shared and cycles intact. Functions, coroutines and userdata cannot move
/// between interpreters: they are dropped and named in `dropped`.
/// Metatables are not copied.
pub struct Copier<'a> {
    destination: &'a Lua,
    seen: HashMap<usize, Table>,
    pub dropped: Vec<String>,
    deadline: Instant,
    steps: u32,
}

/// Why a state did not move.
#[derive(Debug)]
pub enum CopyFailed {
    TookTooLong,
    /// Nested deeper than [`STATE_DEPTH`]: the copy walks a table inside a
    /// table by calling itself, and a script chooses how deep that goes.
    TooDeep,
    /// The new interpreter could not hold it.
    NoRoom,
}

/// How deep a state may nest and still be carried across a reload.
pub const STATE_DEPTH: usize = 128;

impl From<mlua::Error> for CopyFailed {
    fn from(_: mlua::Error) -> Self {
        CopyFailed::NoRoom
    }
}

impl<'a> Copier<'a> {
    pub fn new(destination: &'a Lua, deadline: Instant) -> Self {
        Self {
            destination,
            seen: HashMap::new(),
            dropped: Vec::new(),
            deadline,
            steps: 0,
        }
    }

    pub fn copy(&mut self, v: &Value, path: &str, depth: usize) -> Result<Value, CopyFailed> {
        if depth > STATE_DEPTH {
            return Err(CopyFailed::TooDeep);
        }
        self.steps = self.steps.wrapping_add(1);
        if self.steps.is_multiple_of(1024) && Instant::now() > self.deadline {
            return Err(CopyFailed::TookTooLong);
        }
        Ok(match v {
            Value::Nil => Value::Nil,
            Value::Boolean(b) => Value::Boolean(*b),
            Value::Integer(i) => Value::Integer(*i),
            Value::Number(n) => Value::Number(*n),
            _ if is_null(v) => Value::NULL,
            Value::String(s) => Value::String(self.destination.create_string(s.as_bytes())?),
            Value::Table(t) => {
                let key = t.to_pointer() as usize;
                if let Some(done) = self.seen.get(&key) {
                    return Ok(Value::Table(done.clone()));
                }
                let out = self.destination.create_table()?;
                self.seen.insert(key, out.clone());
                for pair in t.pairs::<Value, Value>() {
                    let (k, item) = pair?;
                    let label = if self.dropped.len() < 8 {
                        match &k {
                            Value::String(s) => format!("{path}.{}", s.to_string_lossy()),
                            Value::Integer(i) => format!("{path}[{i}]"),
                            _ => format!("{path}[?]"),
                        }
                    } else {
                        String::new()
                    };
                    let k2 = self.copy(&k, &label, depth + 1)?;
                    let item2 = self.copy(&item, &label, depth + 1)?;
                    if !k2.is_nil() && !item2.is_nil() {
                        out.raw_set(k2, item2)?;
                    }
                }
                Value::Table(out)
            }
            other => {
                if self.dropped.len() < 8 {
                    self.dropped.push(format!("{path} ({})", other.type_name()));
                }
                Value::Nil
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::runtime::limits;
    use crate::apps::runtime::vm::Vm;
    use std::time::Duration;

    fn vm() -> Vm {
        Vm::new(16 << 20, Duration::from_secs(20)).expect("Lua starts")
    }

    fn value(vm: &Vm, source: &str) -> Value {
        vm.run("test.lua", source.as_bytes())
            .expect("runs")
            .pop_front()
            .unwrap_or(Value::Nil)
    }

    #[test]
    fn a_table_becomes_json_and_back() {
        let vm = vm();
        let v = value(
            &vm,
            r#"return { title = "Alpha", n = 3, w = 3.0, f = 2.5, list = { 1, nil, 3 },
                        empty = {}, nested = { b = true }, [7] = "seven" }"#,
        );
        // `nil` in a constructor leaves a hole: the list is then an object.
        let json = to_json(&v, "content", 4096, 32).expect("json");
        assert_eq!(
            json.to_string(),
            r#"{"7":"seven","empty":[],"f":2.5,"list":{"1":1,"3":3},"n":3,"nested":{"b":true},"title":"Alpha","w":3.0}"#
        );
        // And back: whole numbers without a point are integers.
        let back = to_lua(vm.lua(), &json).expect("lua");
        let Value::Table(t) = back else { panic!() };
        assert!(matches!(t.raw_get::<Value>("n"), Ok(Value::Integer(3))));
        assert!(matches!(t.raw_get::<Value>("w"), Ok(Value::Number(_))));
    }

    /// What a script writes back of `read` when it changes nothing: JSON to
    /// Lua and back, as an update carries it.
    fn round_trip(vm: &Vm, read: &Json) -> Map<String, Json> {
        let Json::Object(read) = read else { panic!() };
        let table = Value::Table(map_to_lua(vm.lua(), read).expect("lua"));
        to_content(&table, 1 << 20, 32).expect("content")
    }

    #[test]
    fn an_update_keeps_what_lua_could_not_show() {
        let vm = vm();
        let read = serde_json::json!({
            "gone": null, "nested": { "a": 1, "b": null }, "meta": {}, "nulls": { "x": null },
            "list": [1, null, 3], "tail": [1, null], "one": [null], "labels": [],
            "big": 12345678901234567890_u64, "exact": 1795537221.1991239, "float": 1.0,
            "deep": [{ "e": {} }, { "n": null }],
        });
        let written = round_trip(&vm, &read);
        // Lua's view alone loses these.
        assert_ne!(Json::Object(written.clone()), read);
        let Json::Object(map) = &read else { panic!() };
        assert_eq!(Json::Object(as_read(map, written)), read);
    }

    #[test]
    fn an_update_writes_what_the_script_changed() {
        let read = serde_json::json!({
            "gone": null, "meta": {}, "big": 9007199254740992_i64, "f": 2.5, "list": [1, null],
        });
        let Json::Object(read) = read else { panic!() };
        let written = serde_json::json!({
            "gone": 1, "meta": [1], "big": 9007199254740993_i64, "f": 2.0, "list": [1, 2], "new": {},
        });
        let Json::Object(map) = written.clone() else {
            panic!()
        };
        assert_eq!(Json::Object(as_read(&read, map)), written);
        // Fields left out that held something stay out; a null stays.
        assert_eq!(
            Json::Object(as_read(&read, Map::new())),
            serde_json::json!({ "gone": null })
        );
    }

    #[test]
    fn what_json_cannot_hold_is_refused_with_its_place() {
        let vm = vm();
        for (source, wanted) in [
            ("return { a = { b = 0/0 } }", "content.a.b is NaN"),
            (
                "return { f = function() end }",
                "content.f is a function, which cannot be stored as data",
            ),
            ("return { [1.5] = 1 }", "content has a key of type number"),
            (
                "return { s = '\\xff' }",
                "content.s is not valid UTF-8 text",
            ),
            (
                "local t = {} t.t = t return t",
                "is nested deeper than 32 levels (a cycle?)",
            ),
        ] {
            match to_json(&value(&vm, source), "content", 4096, 32) {
                Err(Refused::Bad(m)) => assert!(m.contains(wanted), "{source}: {m}"),
                other => panic!("{source}: {other:?}"),
            }
        }
        assert_eq!(
            to_content(&value(&vm, "return { 1, 2 }"), 4096, 32),
            Err(Refused::Bad(
                "'content' must be a table with named fields, got a list".into()
            ))
        );
        assert_eq!(
            to_content(&value(&vm, "return {}"), 4096, 32),
            Ok(Map::new())
        );
    }

    #[test]
    fn a_value_past_the_size_stops_the_reading() {
        let vm = Vm::new(256 << 20, Duration::from_secs(60)).expect("Lua starts");
        // Exactly at the limit, and one byte over.
        let fits = value(&vm, r#"return { s = string.rep("x", 100 - 8) }"#);
        assert!(to_json(&fits, "content", 100, 32).is_ok());
        assert_eq!(to_json(&fits, "content", 99, 32), Err(Refused::TooLarge));
        // Four million entries: the reading must stop near the limit, not
        // walk the table. A whole walk takes a second in a debug build.
        let huge = value(
            &vm,
            "local t = {} for i = 1, 4000000 do t[i] = i end return t",
        );
        let started = Instant::now();
        assert_eq!(
            to_json(&huge, "content", limits::CONTENT_BYTES, 32),
            Err(Refused::TooLarge)
        );
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(plain(&huge, 64, 32), "table");
    }

    #[test]
    fn a_metatable_of_the_script_is_never_asked() {
        let vm = vm();
        // Every metamethod a careless host would run: none may.
        let v = value(
            &vm,
            r#"
            local calls = 0
            local function bump() calls = calls + 1 return 1 end
            local t = setmetatable({ title = "x" }, {
              __index = bump, __newindex = bump, __len = bump, __pairs = bump,
              __tostring = bump, __name = "evil", __call = bump, __eq = bump,
            })
            return { t = t, calls = function() return calls end }
            "#,
        );
        let Value::Table(outer) = &v else { panic!() };
        let inner: Value = outer.raw_get("t").expect("t");
        assert_eq!(
            to_json(&inner, "content", 4096, 32).map(|j| j.to_string()),
            Ok(r#"{"title":"x"}"#.to_string())
        );
        assert_eq!(plain(&inner, 4096, 32), r#"{"title":"x"}"#);
        let other = self::vm();
        let mut copier = Copier::new(other.lua(), Instant::now() + Duration::from_secs(20));
        assert!(copier.copy(&inner, "state", 0).is_ok());
        let calls: mlua::Function = outer.raw_get("calls").expect("calls");
        let n = vm.call(&calls, ()).expect("calls").pop_front();
        assert!(matches!(n, Some(Value::Integer(0))), "{n:?}");
    }

    #[test]
    fn a_state_moves_to_another_interpreter() {
        let (old, new) = (vm(), vm());
        let state = value(
            &old,
            r#"
            local shared = { n = 1 }
            local s = { a = shared, b = shared, list = { "x", "y" }, f = function() end,
                        meta = setmetatable({ k = 1 }, { __index = function() return 2 end }) }
            s.self = s
            return s
            "#,
        );
        let mut copier = Copier::new(new.lua(), Instant::now() + Duration::from_secs(20));
        let copied = copier.copy(&state, "state", 0).expect("copies");
        assert_eq!(copier.dropped, ["state.f (function)"]);
        new.lua().globals().raw_set("S", copied).expect("set");
        let check = new
            .run(
                "check.lua",
                br#"
                assert(S.a == S.b and S.a.n == 1, "shared tables stay shared")
                assert(S.self == S, "a cycle stays a cycle")
                assert(#S.list == 2 and S.list[2] == "y")
                assert(S.f == nil, "a function does not move")
                assert(getmetatable(S.meta) == nil and S.meta.k == 1 and S.meta.other == nil)
                return "ok"
                "#,
            )
            .expect("checks");
        assert!(matches!(check.front(), Some(Value::String(_))));
    }
}
