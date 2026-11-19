//! The closed API: `ui`, `data`, `host` and `print`. Besides Lua's own
//! `table`, `string`, `math`, `utf8` and base functions, this is everything
//! a script can call (rule 14), and all of it is in `docs/app-api.md`.
//!
//! Every function here is written to three rules:
//!
//! - **It reads the script's tables raw** and never calls script code.
//! - **Its work is bounded by a limit, not by its argument.** A script can
//!   hand over a table of four million entries; the function stops at the
//!   limit it serves and says so.
//! - **It refuses with words and the script's own `file:line`.** An error
//!   raised from Rust travels as a userdata; the `pcall` of the bounded
//!   library gives a script that catches it the string, so that
//!   `"failed: " .. err` works, and so does the entry function for an error
//!   nobody caught.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use mlua::{Function, Lua, MultiValue, Table, Value};
use serde_json::{Map, Value as Json, json};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::clock::SharedClock;
use super::limits::Limits;
use super::records::{RecordError, RecordOp, RecordReply, RecordRequest, valid_type_name};
use super::ui::{self, Element};
use super::value::{self, Refused};
use super::vm::script_position;

const API: &str = include_str!("api.lua");

/// The functions of the API, as a script names them. `data.null` is a
/// value and is listed with them. A test compares this with what an
/// interpreter really has, with the document and with its twin.
pub const FUNCTIONS: &[&str] = &[
    "data.create",
    "data.delete",
    "data.list",
    "data.null",
    "data.update",
    "host.after",
    "host.log",
    "host.quit",
    "host.time",
    "print",
    "ui.bar_chart",
    "ui.block",
    "ui.column",
    "ui.edit",
    "ui.gauge",
    "ui.input",
    "ui.layers",
    "ui.line",
    "ui.line_gauge",
    "ui.list",
    "ui.paste",
    "ui.popup",
    "ui.row",
    "ui.scrollbar",
    "ui.spacer",
    "ui.span",
    "ui.sparkline",
    "ui.table",
    "ui.tabs",
    "ui.text",
    "ui.truncate",
    "ui.width",
];

/// What the top level of a script is told when it asks for something.
const WHILE_LOADING: &str =
    "cannot be called while the script is loaded; call it from app.init or another handler";

/// A request the script made and has no reply to yet.
pub struct Pending {
    pub op: &'static str,
    pub tag: Json,
    /// The request as sent, which the reply carries back.
    pub request: Json,
}

pub struct Timer {
    /// When it fires, on the clock's `elapsed`.
    pub due: Duration,
    /// Timers due at the same moment fire in the order they were set.
    pub order: u64,
    pub tag: Json,
}

/// Counts what happened in the last minute.
#[derive(Default)]
pub struct Window {
    times: VecDeque<Duration>,
}

impl Window {
    /// May one more happen now, with `per_minute` allowed in any minute?
    fn admit(&mut self, now: Duration, per_minute: u32) -> bool {
        let minute = Duration::from_secs(60);
        while self
            .times
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= minute)
        {
            self.times.pop_front();
        }
        if self.times.len() >= per_minute as usize {
            return false;
        }
        self.times.push_back(now);
        true
    }
}

/// What the API functions inside an interpreter share with the worker that
/// runs it. It outlives a reload: requests waiting, timers and known
/// records belong to the app, not to one version of its script.
pub struct Gate {
    pub limits: Limits,
    pub clock: SharedClock,
    /// The fence: the record types the running version declared.
    pub types: Vec<String>,
    /// Records the app has received, by id, with their type. Only these can
    /// be updated or deleted.
    pub known: HashMap<String, String>,
    pub pending: HashMap<u64, Pending>,
    pub next_id: u64,
    /// Requests made during the running call, for the owner.
    pub requests: Vec<RecordRequest>,
    /// Replies the runtime gives by itself (content too large).
    pub local_replies: VecDeque<RecordReply>,
    pub timers: Vec<Timer>,
    pub timer_order: u64,
    /// The script called `host.quit()`.
    pub quit: bool,
    /// The top level of a script is running. Until it is accepted as the
    /// app's version it can only compute: it may not ask for anything.
    pub loading: bool,
    /// Lines logged during the running call.
    pub logs: Vec<String>,
    pub log_window: Window,
    pub logs_dropped: u32,
    pub request_window: Window,
}

/// More known records than the relay lets one sync hold are not kept.
const KNOWN_RECORDS: usize = 8192;

impl Gate {
    pub fn new(limits: Limits, clock: SharedClock) -> Self {
        Self {
            limits,
            clock,
            types: Vec::new(),
            known: HashMap::new(),
            pending: HashMap::new(),
            next_id: 1,
            requests: Vec::new(),
            local_replies: VecDeque::new(),
            timers: Vec::new(),
            timer_order: 0,
            quit: false,
            loading: false,
            logs: Vec::new(),
            log_window: Window::default(),
            logs_dropped: 0,
            request_window: Window::default(),
        }
    }

    pub fn declares(&self, record_type: &str) -> bool {
        self.types.iter().any(|t| t == record_type)
    }

    /// The app has received this record: it may change it from now on.
    pub fn learn(&mut self, id: &str, record_type: &str) {
        if self.declares(record_type)
            && (self.known.len() < KNOWN_RECORDS || self.known.contains_key(id))
        {
            self.known.insert(id.to_string(), record_type.to_string());
        }
    }

    pub fn forget(&mut self, id: &str) {
        self.known.remove(id);
    }

    /// One line for the log, cut to the limit and counted against the
    /// volume. Also used by the runtime for its own lines about the app,
    /// which are not counted.
    pub fn log(&mut self, line: &str, counted: bool) {
        let now = self.clock.elapsed();
        if counted && !self.log_window.admit(now, self.limits.log_lines_per_minute) {
            self.logs_dropped = self.logs_dropped.saturating_add(1);
            return;
        }
        if self.logs_dropped > 0 {
            let dropped = std::mem::take(&mut self.logs_dropped);
            self.logs.push(format!(
                "({dropped} lines were dropped: more than {} in a minute)",
                self.limits.log_lines_per_minute
            ));
        }
        self.logs
            .push(cut(line, self.limits.log_line_bytes).to_string());
    }
}

pub type SharedGate = Rc<RefCell<Gate>>;

/// `text` cut to at most `bytes`, on a character.
pub fn cut(text: &str, bytes: usize) -> &str {
    if text.len() <= bytes {
        return text;
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Why a function of the API did not do what it was asked.
enum No {
    /// The script is wrong: the words are raised at its calling line.
    Say(String),
    /// Lua itself failed, for want of memory.
    Lua(mlua::Error),
}

impl From<mlua::Error> for No {
    fn from(e: mlua::Error) -> Self {
        No::Lua(e)
    }
}

fn say<T>(message: impl Into<String>) -> Result<T, No> {
    Err(No::Say(message.into()))
}

type Answer = Result<Vec<Value>, No>;

/// A function of the API. What it refuses, it refuses with the `file:line`
/// of the script that called it before the words.
fn function<F>(lua: &Lua, f: F) -> mlua::Result<Function>
where
    F: Fn(&Lua, &MultiValue) -> Answer + 'static,
{
    lua.create_function(move |lua, args: MultiValue| match f(lua, &args) {
        Ok(results) => Ok(MultiValue::from_vec(results)),
        Err(No::Say(message)) => Err(mlua::Error::runtime(match script_position(lua) {
            Some(at) => format!("{at}: {message}"),
            None => message,
        })),
        // Only a failed allocation gets here.
        Err(No::Lua(e)) => Err(e),
    })
}

const NIL: Value = Value::Nil;

fn arg(args: &MultiValue, i: usize) -> &Value {
    args.get(i).unwrap_or(&NIL)
}

fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        _ if value::is_null(v) => "data.null",
        other => other.type_name(),
    }
}

/// A string argument of at most `bytes`, as text.
fn text_arg(v: &Value, what: &str, bytes: usize) -> Result<String, No> {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            if b.len() > bytes {
                return say(format!(
                    "{what} is {} bytes long; at most {bytes} are taken",
                    b.len()
                ));
            }
            Ok(String::from_utf8_lossy(&b).into_owned())
        }
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        other => say(format!("{what} must be a string, got {}", type_of(other))),
    }
}

/// Puts `ui`, `data`, `host` and `print` into the interpreter's globals.
pub fn install(lua: &Lua, gate: &SharedGate) -> mlua::Result<()> {
    let raw = lua.create_table()?;
    let limits = gate.borrow().limits.clone();

    for element in ui::ELEMENTS {
        let max_items = limits.screen_elements;
        raw.raw_set(
            format!("ui.{}", element.name),
            function(lua, move |lua, args| {
                construct(lua, element, max_items, arg(args, 0))
            })?,
        )?;
    }
    let bytes = limits.text_argument_bytes;
    raw.raw_set(
        "ui.edit",
        function(lua, move |_, args| edit(arg(args, 0), arg(args, 1), bytes))?,
    )?;
    raw.raw_set(
        "ui.paste",
        function(lua, move |_, args| paste(arg(args, 0), arg(args, 1), bytes))?,
    )?;
    raw.raw_set(
        "ui.width",
        function(lua, move |_, args| {
            let text = text_arg(arg(args, 0), "ui.width: the text", bytes)?;
            Ok(vec![Value::Integer(text.width() as i64)])
        })?,
    )?;
    raw.raw_set(
        "ui.truncate",
        function(lua, move |lua, args| {
            let text = text_arg(arg(args, 0), "ui.truncate: the text", bytes)?;
            let width = match arg(args, 1) {
                Value::Integer(w) if *w >= 0 => *w as usize,
                Value::Number(w) if *w >= 0.0 && w.fract() == 0.0 => *w as usize,
                other => {
                    return say(format!(
                        "ui.truncate: the width must be a whole number 0 or more, got {}",
                        type_of(other)
                    ));
                }
            };
            Ok(vec![Value::String(
                lua.create_string(truncate(&text, width))?,
            )])
        })?,
    )?;

    for op in ["list", "create", "update", "delete"] {
        let gate = gate.clone();
        raw.raw_set(
            format!("data.{op}"),
            function(lua, move |_, args| request(&gate, op, arg(args, 0)))?,
        )?;
    }
    // JSON null for content that is sent (`decided_by = data.null`). A null
    // that arrives is nil, never this value.
    raw.raw_set("data.null", Value::NULL)?;

    let shared = gate.clone();
    raw.raw_set(
        "host.quit",
        function(lua, move |_, _| {
            let mut gate = shared.borrow_mut();
            if gate.loading {
                return say(format!("host.quit {WHILE_LOADING}"));
            }
            gate.quit = true;
            Ok(Vec::new())
        })?,
    )?;
    for (name, separator) in [("host.log", " "), ("print", "\t")] {
        let shared = gate.clone();
        raw.raw_set(
            name,
            function(lua, move |_, args| {
                let mut gate = shared.borrow_mut();
                let (bytes, depth) = (gate.limits.log_line_bytes, gate.limits.data_depth);
                // At most a line's worth is read of each value, and no
                // more values than a line has bytes.
                let mut line = String::new();
                for v in args.iter().take(bytes) {
                    if line.len() >= bytes {
                        break;
                    }
                    if !line.is_empty() {
                        line.push_str(separator);
                    }
                    line.push_str(&value::plain(v, bytes, depth));
                }
                gate.log(&line, true);
                Ok(Vec::new())
            })?,
        )?;
    }
    let shared = gate.clone();
    raw.raw_set(
        "host.after",
        function(lua, move |_, args| {
            after(&shared, arg(args, 0), arg(args, 1))
        })?,
    )?;
    let shared = gate.clone();
    raw.raw_set(
        "host.time",
        function(lua, move |_, _| {
            Ok(vec![Value::Number(
                shared.borrow().clock.wall().as_secs_f64(),
            )])
        })?,
    )?;

    lua.load(API)
        .set_name("=api")
        .set_mode(mlua::chunk::ChunkMode::Text)
        .call::<()>(raw)
}

// --------------------------------------------------------------------- ui

/// A `ui.*` constructor: checks property names at once, so that a typo
/// fails at the line that wrote it, and marks the table as an element.
fn construct(lua: &Lua, element: &'static Element, max_items: usize, given: &Value) -> Answer {
    let name = element.name;
    let t = match given {
        Value::Table(t) => t.clone(),
        Value::Nil if !element.positional => lua.create_table()?,
        Value::String(_) | Value::Integer(_) | Value::Number(_) if element.positional => {
            let t = lua.create_table()?;
            t.raw_set(1, given.clone())?;
            t
        }
        other => {
            return say(format!("ui.{name} expects a table, got {}", type_of(other)));
        }
    };
    let mut seen = 0usize;
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        seen += 1;
        if seen > max_items {
            return say(format!(
                "ui.{name} has more than {max_items} items; a screen cannot hold them"
            ));
        }
        match k {
            Value::Integer(_) if element.positional => {}
            Value::Integer(i) => {
                return say(format!(
                    "ui.{name} takes no positional items (got item [{i}]); use its named properties"
                ));
            }
            Value::String(s) => {
                let s = s.to_string_lossy();
                if !(ui::accepts(element, &s) || s == "kind" || s == "_at") {
                    let shown: String = s.chars().take(40).collect();
                    return say(format!(
                        "ui.{name}: unknown property '{shown}'. Known: {}",
                        ui::known_properties(element)
                    ));
                }
            }
            other => {
                return say(format!("ui.{name}: bad key of type {}", other.type_name()));
            }
        }
    }
    t.raw_set("kind", name)?;
    if let Some(at) = script_position(lua) {
        t.raw_set("_at", at)?;
    }
    Ok(vec![Value::Table(t)])
}

/// The field `{value =, cursor =}` of `ui.edit` and `ui.paste`: its
/// characters and the cursor among them.
fn field_parts(field: &Value, name: &str, bytes: usize) -> Result<(Table, Vec<char>, usize), No> {
    let Value::Table(t) = field else {
        return say(format!(
            "ui.{name}: the field must be a table {{value =, cursor =}}, got {}",
            type_of(field)
        ));
    };
    let text = match t.raw_get::<Value>("value")? {
        Value::Nil => String::new(),
        v => text_arg(&v, &format!("ui.{name}: field.value"), bytes)?,
    };
    let chars: Vec<char> = text.chars().collect();
    let cursor = match t.raw_get::<Value>("cursor")? {
        Value::Nil => chars.len(),
        Value::Integer(c) => (c.max(0) as u64).min(chars.len() as u64) as usize,
        Value::Number(c) if c.fract() == 0.0 => {
            (c.max(0.0) as u64).min(chars.len() as u64) as usize
        }
        other => {
            return say(format!(
                "ui.{name}: field.cursor must be a whole number, got {}",
                type_of(&other)
            ));
        }
    };
    Ok((t.clone(), chars, cursor))
}

fn store(field: &Table, chars: &[char], cursor: usize) -> mlua::Result<()> {
    field.raw_set("value", chars.iter().collect::<String>())?;
    field.raw_set("cursor", cursor)
}

fn key_flag(key: &Table, name: &str) -> mlua::Result<bool> {
    Ok(matches!(key.raw_get::<Value>(name)?, Value::Boolean(true)))
}

/// `ui.edit(field, key)`: applies a key to a one-line field. Answers
/// whether the key edited or moved in the field.
fn edit(field: &Value, key: &Value, bytes: usize) -> Answer {
    let (table, mut chars, mut cursor) = field_parts(field, "edit", bytes)?;
    let Value::Table(key) = key else {
        return say(format!(
            "ui.edit: the key must be the table app.key received, got {}",
            type_of(key)
        ));
    };
    let name = match key.raw_get::<Value>("name")? {
        Value::String(s) => s.to_string_lossy(),
        _ => String::new(),
    };
    let text = match key.raw_get::<Value>("text")? {
        Value::String(s) => Some(text_arg(&Value::String(s), "ui.edit: key.text", bytes)?),
        _ => None,
    };
    let (ctrl, alt) = (key_flag(key, "ctrl")?, key_flag(key, "alt")?);
    match name.as_str() {
        "backspace" if cursor > 0 => {
            chars.remove(cursor - 1);
            cursor -= 1;
        }
        "backspace" => {}
        "delete" if cursor < chars.len() => {
            chars.remove(cursor);
        }
        "delete" => {}
        "left" => cursor = cursor.saturating_sub(1),
        "right" => cursor = (cursor + 1).min(chars.len()),
        "home" => cursor = 0,
        "end" => cursor = chars.len(),
        "u" if ctrl => {
            chars.drain(..cursor);
            cursor = 0;
        }
        _ => match text {
            Some(typed) if !ctrl && !alt && !typed.is_empty() => {
                let add: Vec<char> = typed.chars().collect();
                let n = add.len();
                chars.splice(cursor..cursor, add);
                cursor += n;
            }
            _ => return Ok(vec![Value::Boolean(false)]),
        },
    }
    store(&table, &chars, cursor)?;
    Ok(vec![Value::Boolean(true)])
}

/// `ui.paste(field, text)`: inserts pasted text at the cursor; each CR, LF
/// or CRLF becomes one space.
fn paste(field: &Value, text: &Value, bytes: usize) -> Answer {
    let (table, mut chars, cursor) = field_parts(field, "paste", bytes)?;
    let text = text_arg(text, "ui.paste: the text", bytes)?;
    let one_line = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    let add: Vec<char> = one_line.chars().collect();
    let n = add.len();
    chars.splice(cursor..cursor, add);
    store(&table, &chars, cursor + n)?;
    Ok(vec![Value::Boolean(true)])
}

/// `text` cut to at most `width` cells, ending in "…" when it was cut.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

// ------------------------------------------------------------------- data

const LIST_FIELDS: &[&str] = &["type", "match", "tag"];
const CREATE_FIELDS: &[&str] = &["type", "content", "tag"];
const UPDATE_FIELDS: &[&str] = &["record_id", "rev", "content", "tag"];
const DELETE_FIELDS: &[&str] = &["record_id", "rev", "tag"];

/// The named fields each `data.*` call takes, for the contract test.
pub fn request_fields(op: &str) -> &'static [&'static str] {
    match op {
        "list" => LIST_FIELDS,
        "create" => CREATE_FIELDS,
        "update" => UPDATE_FIELDS,
        _ => DELETE_FIELDS,
    }
}

/// `data.list/create/update/delete{...}`: checks the fence and the limits,
/// hands the request to the owner and answers its id. The reply arrives
/// later in `app.reply`.
fn request(gate: &SharedGate, op: &'static str, given: &Value) -> Answer {
    let Value::Table(t) = given else {
        return say(format!(
            "data.{op} expects a table, e.g. data.{op}{{...}}, got {}",
            type_of(given)
        ));
    };
    let allowed = request_fields(op);
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        let known = matches!(&k, Value::String(s) if allowed.contains(&&*s.to_string_lossy()));
        if !known {
            let shown = match &k {
                Value::String(s) => s.to_string_lossy().chars().take(40).collect::<String>(),
                other => format!("[{}]", other.type_name()),
            };
            return say(format!(
                "data.{op}: unknown field '{shown}'; allowed: {}",
                allowed.join(", ")
            ));
        }
    }
    let mut gate = gate.borrow_mut();
    if gate.loading {
        return say(format!("data.{op} {WHILE_LOADING}"));
    }
    let limits = gate.limits.clone();
    let bad = |m: String| No::Say(format!("data.{op}: {m}"));
    let json = |v: &Value, what: &str, bytes: usize| -> Result<Json, No> {
        match value::to_json(v, what, bytes, limits.data_depth) {
            Ok(json) => Ok(json),
            Err(Refused::Bad(m)) => Err(bad(m)),
            Err(Refused::TooLarge) => Err(bad(format!(
                "'{what}' is larger than {bytes} bytes as JSON"
            ))),
        }
    };
    let tag = json(&t.raw_get::<Value>("tag")?, "tag", limits.tag_bytes)?;

    let mut echo = Map::new();
    echo.insert("op".into(), json!(op));
    // Content that is too large is answered here, with `too_large`.
    let mut too_large = None;
    let mut content_of = |echo: &mut Map<String, Json>| -> Result<Map<String, Json>, No> {
        let given = t.raw_get::<Value>("content")?;
        match value::to_content(&given, limits.content_bytes, limits.data_depth) {
            Ok(content) => {
                echo.insert("content".into(), Json::Object(content.clone()));
                Ok(content)
            }
            Err(Refused::Bad(m)) => Err(bad(m)),
            Err(Refused::TooLarge) => {
                too_large = Some(RecordError::TooLarge {
                    message: format!(
                        "the content is larger than {} bytes as JSON",
                        limits.content_bytes
                    ),
                });
                Ok(Map::new())
            }
        }
    };
    let op_value = match op {
        "list" | "create" => {
            let record_type = match t.raw_get::<Value>("type")? {
                Value::String(s) => s.to_string_lossy(),
                Value::Nil => return Err(bad("'type' is required".into())),
                other => {
                    return Err(bad(format!(
                        "'type' must be a string, got {}",
                        type_of(&other)
                    )));
                }
            };
            if !gate.declares(&record_type) {
                let shown: String = record_type.chars().take(40).collect();
                return Err(bad(format!(
                    "record type \"{shown}\" is not declared in app.types ({})",
                    gate.types.join(", ")
                )));
            }
            echo.insert("type".into(), json!(record_type));
            if op == "list" {
                let matching = match t.raw_get::<Value>("match")? {
                    Value::Nil => None,
                    Value::String(s) if s.as_bytes().is_empty() => None,
                    Value::String(s) => {
                        if s.as_bytes().len() > limits.match_bytes {
                            return Err(bad(format!(
                                "'match' is longer than {} bytes",
                                limits.match_bytes
                            )));
                        }
                        Some(s.to_string_lossy())
                    }
                    other => {
                        return Err(bad(format!(
                            "'match' must be a string, got {}",
                            type_of(&other)
                        )));
                    }
                };
                if let Some(m) = &matching {
                    echo.insert("match".into(), json!(m));
                }
                RecordOp::List {
                    record_type,
                    matching,
                }
            } else {
                RecordOp::Create {
                    record_type,
                    content: content_of(&mut echo)?,
                }
            }
        }
        _ => {
            let record_id = match t.raw_get::<Value>("record_id")? {
                Value::String(s) => s.to_string_lossy(),
                Value::Nil => return Err(bad("'record_id' is required".into())),
                other => {
                    return Err(bad(format!(
                        "'record_id' must be a string, got {}",
                        type_of(&other)
                    )));
                }
            };
            let shown: String = record_id.chars().take(64).collect();
            let record_type = match gate.known.get(&record_id) {
                Some(known) if gate.declares(known) => known.clone(),
                Some(known) => {
                    return Err(bad(format!(
                        "record {shown} has type \"{known}\", which is not declared in app.types"
                    )));
                }
                None => {
                    return Err(bad(format!(
                        "record {shown} is unknown: only records of the declared types that the app received in a reply or a push can be changed"
                    )));
                }
            };
            let rev = match t.raw_get::<Value>("rev")? {
                Value::String(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
                Value::Nil => {
                    return Err(bad("'rev' is required (the record's current rev)".into()));
                }
                other => {
                    return Err(bad(format!(
                        "'rev' must be the string the record came with, got {}",
                        type_of(&other)
                    )));
                }
            };
            echo.insert("record_id".into(), json!(record_id));
            echo.insert("rev".into(), json!(rev));
            if op == "update" {
                RecordOp::Update {
                    record_id,
                    record_type,
                    rev,
                    content: content_of(&mut echo)?,
                }
            } else {
                RecordOp::Delete {
                    record_id,
                    record_type,
                    rev,
                }
            }
        }
    };
    // The limits last, so that a wrong request is told it is wrong.
    if gate.pending.len() >= limits.pending_requests {
        return Err(bad(format!(
            "{} requests are still waiting for their reply; no more are taken until one arrives",
            limits.pending_requests
        )));
    }
    let now = gate.clock.elapsed();
    if !gate.request_window.admit(now, limits.requests_per_minute) {
        return Err(bad(format!(
            "more than {} requests in a minute",
            limits.requests_per_minute
        )));
    }
    let id = gate.next_id;
    gate.next_id += 1;
    echo.insert("id".into(), json!(id));
    gate.pending.insert(
        id,
        Pending {
            op,
            tag,
            request: Json::Object(echo),
        },
    );
    match too_large {
        Some(error) => gate.local_replies.push_back(RecordReply {
            id,
            result: Err(error),
        }),
        None => gate.requests.push(RecordRequest { id, op: op_value }),
    }
    Ok(vec![Value::Integer(id as i64)])
}

/// Checks `app.types` as the script declared it.
pub fn check_types(types: &[String], limits: &Limits) -> Result<(), String> {
    if types.is_empty() {
        return Err("app.types is empty; declare at least one record type".into());
    }
    if types.len() > limits.types {
        return Err(format!(
            "app.types has {} types; an app may declare {}",
            types.len(),
            limits.types
        ));
    }
    for (i, name) in types.iter().enumerate() {
        if !valid_type_name(name) {
            let shown: String = name.chars().take(40).collect();
            return Err(format!(
                "app.types[{}] = \"{shown}\" is not a record type's name: a lower-case letter, then up to 31 of a-z, 0-9, _ and -",
                i + 1
            ));
        }
        if types[..i].contains(name) {
            return Err(format!("app.types names \"{name}\" twice"));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- host

/// `host.after(ms, tag)`: calls `app.timer(state, tag, ctx)` once, later.
fn after(gate: &SharedGate, ms: &Value, tag: &Value) -> Answer {
    let mut gate = gate.borrow_mut();
    if gate.loading {
        return say(format!("host.after {WHILE_LOADING}"));
    }
    let (min, max) = (gate.limits.timer_min_ms, gate.limits.timer_max_ms);
    let ms = match ms {
        Value::Integer(n) => *n,
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => *n as i64,
        other => {
            return say(format!(
                "host.after: the first argument is the delay in milliseconds, a whole number, got {}",
                type_of(other)
            ));
        }
    };
    if ms < min as i64 || ms > max as i64 {
        return say(format!(
            "host.after: the delay must be from {min} to {max} ms, got {ms}"
        ));
    }
    let tag = match value::to_json(tag, "tag", gate.limits.tag_bytes, gate.limits.data_depth) {
        Ok(tag) => tag,
        Err(Refused::Bad(m)) => return say(format!("host.after: {m}")),
        Err(Refused::TooLarge) => {
            return say(format!(
                "host.after: the tag is larger than {} bytes as JSON",
                gate.limits.tag_bytes
            ));
        }
    };
    if gate.timers.len() >= gate.limits.timers {
        return say(format!(
            "host.after: {} timers are waiting already; no more are taken until one fires",
            gate.limits.timers
        ));
    }
    let due = gate.clock.elapsed() + Duration::from_millis(ms as u64);
    let order = gate.timer_order;
    gate.timer_order += 1;
    gate.timers.push(Timer { due, order, tag });
    Ok(Vec::new())
}
