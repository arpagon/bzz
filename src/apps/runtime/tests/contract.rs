//! The contract in three places: the document `docs/app-api.md`, its twin
//! for an editor `docs/app-api.d.lua`, and the runtime. This fails when
//! they name different functions, elements, properties, fields, codes or
//! limits. What the runtime has is asked of a running interpreter where it
//! can be, not read from a list.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::apps::runtime::api;
use crate::apps::runtime::limits;
use crate::apps::runtime::memory::content;
use crate::apps::runtime::records::ERROR_CODES;
use crate::apps::runtime::ui::{COLOR_NAMES, ELEMENTS, STYLE, THEME_NAMES};
use crate::apps::runtime::{HANDLERS, KEY_NAMES};

const DOCUMENT: &str = include_str!("../../../../docs/app-api.md");
const TWIN: &str = include_str!("../../../../docs/app-api.d.lua");

fn set<'a>(names: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    names.into_iter().map(str::to_string).collect()
}

fn is_name(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
}

/// Every `ui.x`, `data.x` and `host.x` a text mentions.
fn api_names(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for prefix in ["ui.", "data.", "host."] {
        let mut rest = text;
        let mut before = ' ';
        while let Some(at) = rest.find(prefix) {
            let preceded = rest[..at].chars().last().unwrap_or(before);
            let name: String = rest[at + prefix.len()..]
                .chars()
                .take_while(|c| is_name(*c))
                .collect();
            if !name.is_empty() && !is_name(preceded) && preceded != '.' {
                found.insert(format!("{prefix}{name}"));
            }
            before = 'x';
            rest = &rest[at + prefix.len()..];
        }
    }
    found
}

/// The words in backticks on the line of the document that starts with
/// `**label:**`.
fn listed(label: &str) -> Vec<String> {
    let start = format!("**{label}:**");
    let line = DOCUMENT
        .lines()
        .find(|l| l.starts_with(&start))
        .unwrap_or_else(|| panic!("the document has no line {start}"));
    line[start.len()..]
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The rows of the table of the document whose head is `head`, as cells.
fn table(head: &str) -> Vec<Vec<String>> {
    let mut lines = DOCUMENT.lines().skip_while(|l| l.trim() != head);
    assert!(lines.next().is_some(), "the document has no table {head}");
    lines
        .skip(1) // the rule under the head
        .take_while(|l| l.starts_with('|'))
        .map(|l| {
            l.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().trim_matches('`').to_string())
                .collect()
        })
        .collect()
}

fn names(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// The fields of `---@class name` in the twin.
fn class(name: &str) -> BTreeSet<String> {
    let head = format!("---@class {name}");
    let mut lines = TWIN
        .lines()
        .skip_while(|l| *l != head && !l.starts_with(&format!("{head}:")));
    assert!(lines.next().is_some(), "the twin has no class {name}");
    lines
        .take_while(|l| l.starts_with("---"))
        .filter_map(|l| l.strip_prefix("---@field "))
        .map(|l| l.chars().take_while(|c| is_name(*c)).collect::<String>())
        .collect()
}

/// The named fields of the table type of `---@param t {...}` that stands
/// before `function <function>(` in the twin.
fn parameter_fields(function: &str) -> Vec<String> {
    let lines: Vec<&str> = TWIN.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with(&format!("function {function}(")))
        .unwrap_or_else(|| panic!("the twin has no function {function}"));
    let parameter = lines[..at]
        .iter()
        .rev()
        .take_while(|l| l.starts_with("---"))
        .find(|l| l.starts_with("---@param "))
        .unwrap_or_else(|| panic!("{function} has no @param in the twin"));
    let open = parameter.find('{').unwrap_or_else(|| panic!("{parameter}"));
    let mut depth = 0;
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut reading = true;
    for c in parameter[open..].chars() {
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                if depth > 1 {
                    reading = false;
                }
            }
            '}' | ']' | ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => {
                field.clear();
                reading = true;
            }
            ':' | '?' if depth == 1 && reading => {
                if !field.trim().is_empty() {
                    fields.push(field.trim().to_string());
                }
                field.clear();
                reading = false;
            }
            c if depth == 1 && reading => field.push(c),
            _ => {}
        }
    }
    fields
}

// --------------------------------------------------------------- functions

/// What an interpreter of an app really has, asked of one.
const WHAT_IS_THERE: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init()
  local functions, globals = {}, {}
  for name, value in pairs(_G) do
    globals[#globals + 1] = name
    if type(value) == "table" and (name == "ui" or name == "data" or name == "host") then
      for member in pairs(value) do functions[#functions + 1] = name .. "." .. member end
    end
  end
  functions[#functions + 1] = type(print) == "function" and "print" or nil
  table.sort(functions); table.sort(globals)
  return { lines = { table.concat(functions, " "), table.concat(globals, " ") } }
end
function app.view(s) return ui.text { s.lines[1], s.lines[2], wrap = true } end
return app
"#;

fn there() -> (Vec<String>, Vec<String>) {
    let mut app = Headless::start(
        WHAT_IS_THERE,
        HeadlessOptions::new("there.lua")
            .size(2000, 2)
            .limits(Limits::patient()),
    )
    .expect("starts");
    assert_eq!(app.error(), None);
    let screen = app.screen();
    let mut lines = screen.lines();
    let mut words = || {
        lines
            .next()
            .unwrap_or("")
            .split(' ')
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    (words(), words())
}

#[test]
fn the_document_the_twin_and_the_runtime_name_the_same_functions() {
    let (functions, globals) = there();
    // The runtime's own list is what an interpreter really has.
    assert_eq!(functions, api::FUNCTIONS);

    // The document: every function is mentioned, and nothing else of that
    // shape is.
    let mut documented = api_names(DOCUMENT);
    assert!(DOCUMENT.contains("`print`"));
    documented.insert("print".into());
    assert_eq!(documented, set(api::FUNCTIONS.iter().copied()));

    // The twin declares each.
    let mut declared: BTreeSet<String> = TWIN
        .lines()
        .filter_map(|l| l.strip_prefix("function "))
        .map(|l| l.split('(').next().unwrap_or("").to_string())
        .collect();
    assert!(TWIN.lines().any(|l| l == "data.null = nil"));
    declared.insert("data.null".into());
    assert_eq!(declared, set(api::FUNCTIONS.iter().copied()));
    // And mentions nothing that is not there.
    let mentioned = api_names(TWIN);
    let unknown: Vec<&String> = mentioned
        .iter()
        .filter(|name| !api::FUNCTIONS.contains(&name.as_str()))
        .collect();
    assert!(unknown.is_empty(), "the twin mentions {unknown:?}");

    // Everything a script can name at all.
    assert_eq!(listed("Globals"), globals);
}

#[test]
fn the_three_name_the_same_elements_and_properties() {
    let rows = table("| Element | Properties | Draws |");
    let documented: Vec<(String, Vec<String>)> = rows
        .iter()
        .map(|row| (row[0].clone(), names(&row[1])))
        .collect();
    let runtime: Vec<(String, Vec<String>)> = ELEMENTS
        .iter()
        .map(|e| {
            (
                format!("ui.{}", e.name),
                e.properties.iter().map(|p| p.to_string()).collect(),
            )
        })
        .collect();
    // The same eighteen, in the same order, each with the same properties.
    assert_eq!(documented, runtime);
    assert_eq!(runtime.len(), 18);

    for element in ELEMENTS {
        let mut declared = parameter_fields(&format!("ui.{}", element.name));
        // `size` is of every element, and declared where an element has
        // nothing else.
        declared.retain(|field| field != "size");
        assert_eq!(
            set(declared.iter().map(String::as_str)),
            set(element.properties.iter().copied()),
            "ui.{} in the twin",
            element.name
        );
    }
    assert_eq!(listed("Style fields"), STYLE);
    assert_eq!(class("Style"), set(STYLE.iter().copied()));
    assert_eq!(
        listed("Theme names"),
        THEME_NAMES
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
    );
    assert_eq!(listed("Colour names"), COLOR_NAMES);
    // The twin's colours are those names too.
    let colour = TWIN
        .split("---A colour:")
        .nth(1)
        .and_then(|rest| rest.split("---@alias Color").next())
        .expect("the colours of the twin");
    let words: BTreeSet<&str> = colour.split(|c: char| !is_name(c)).collect();
    for name in THEME_NAMES.iter().map(|(name, _)| name).chain(COLOR_NAMES) {
        assert!(
            words.contains(name),
            "{name} is not among the twin's colours"
        );
    }
    assert_eq!(listed("Key names"), KEY_NAMES);
    for name in KEY_NAMES {
        assert!(TWIN.contains(&format!("\"{name}\"")), "{name} in the twin");
    }
}

#[test]
fn the_three_name_the_same_handlers() {
    let mut wanted: BTreeSet<String> = set(HANDLERS.iter().copied());
    wanted.extend(set(["api", "types", "view"]));
    // The document: the rows of its table, and every `app.x` in backticks.
    let rows = table("| Field | Called | Purpose |");
    let in_table: BTreeSet<String> = rows
        .iter()
        .filter_map(|row| row[0].strip_prefix("app.").map(str::to_string))
        .collect();
    assert_eq!(in_table, wanted);
    let mentioned: BTreeSet<String> = DOCUMENT
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|code| code.strip_prefix("app."))
        .map(|rest| rest.chars().take_while(|c| is_name(*c)).collect::<String>())
        .collect();
    assert_eq!(mentioned, wanted);
    assert_eq!(class("App"), wanted);
}

#[test]
fn the_three_name_the_same_record_calls_and_error_codes() {
    let rows = table("| Call | Fields | Does |");
    assert_eq!(rows.len(), 4);
    for row in &rows {
        let op = row[0].strip_prefix("data.").expect("a data call");
        assert_eq!(
            names(&row[1]),
            api::request_fields(op),
            "{op} in the document"
        );
        assert_eq!(
            parameter_fields(&row[0]),
            api::request_fields(op),
            "{op} in the twin"
        );
    }
    let codes: Vec<String> = table("| Code | Means |")
        .into_iter()
        .map(|row| row[0].clone())
        .collect();
    assert_eq!(codes, ERROR_CODES);
    let declared = TWIN
        .lines()
        .find_map(|l| l.strip_prefix("---@field code "))
        .expect("the codes of the twin");
    assert_eq!(
        declared
            .split('|')
            .map(|c| c.trim_matches('"'))
            .collect::<Vec<_>>(),
        ERROR_CODES
    );
}

#[test]
fn the_document_publishes_the_limits() {
    let rows = table("| Limit | Value |");
    let documented: Vec<(String, String)> = rows
        .into_iter()
        .map(|row| (row[0].replace('`', ""), row[1].clone()))
        .collect();
    let published: Vec<(String, String)> = limits::published()
        .into_iter()
        .map(|(name, value)| (name.replace('`', ""), value))
        .collect();
    assert_eq!(documented, published);
    // And the limits a runtime starts with are the published ones.
    let limits = Limits::default();
    assert_eq!(limits.call_time, limits::CALL_TIME);
    assert_eq!(limits.memory, limits::MEMORY);
    assert_eq!(limits.script_bytes, limits::SCRIPT_BYTES);
    assert_eq!(limits.content_bytes, limits::CONTENT_BYTES);
    assert_eq!(limits.call_time, Duration::from_millis(250));
    // What the document says in words.
    assert!(DOCUMENT.contains("deeper\n  than 128 tables"));
    assert_eq!(crate::apps::runtime::value::STATE_DEPTH, 128);
    assert!(DOCUMENT.contains("stopped three times in a row"));
    assert_eq!(limits.stops_before_giving_up, 3);
    assert!(DOCUMENT.contains("cut to 512 bytes"));
    assert_eq!(limits::TITLE_BYTES, 512);
    assert!(DOCUMENT.contains("1 to 16 names, each `[a-z][a-z0-9_-]{0,31}`"));
    assert_eq!((limits::TYPES, limits::TYPE_NAME_BYTES), (16, 32));
    assert!(TWIN.contains("after `ms` milliseconds (100 at\n---least)"));
    // What the session holds an app to.
    assert!(DOCUMENT.contains("has not sent any within five seconds"));
    assert_eq!(limits::FIRST_SET_WAIT, Duration::from_secs(5));
    assert!(DOCUMENT.contains("the apps of one person share half of\nit"));
    assert_eq!(
        limits::WRITES_PER_MINUTE * 2,
        limits::PERSON_WRITES_PER_MINUTE
    );
    assert!(TWIN.contains("of one person may write 30 times a minute"));
    assert_eq!(limits::WRITES_PER_MINUTE, 30);
}

// ------------------------------------------------------------------ shapes

/// A script that writes down the fields of every table it is handed.
const SHAPES: &str = r#"
local app = { api = 1, types = { "note" } }
local function fields(t)
  local names = {}
  for k in pairs(t) do names[#names + 1] = tostring(k) end
  table.sort(names)
  return table.concat(names, " ")
end
function app.init(ctx)
  data.list { type = "note", tag = "t" }
  return { seen = { "ctx: " .. fields(ctx), "who: " .. fields(ctx.user) } }
end
local function note(s, what, t) s.seen[#s.seen + 1] = what .. ": " .. fields(t) end
function app.key(s, key, ctx)
  note(s, "key", key)
  if key.name == "u" then data.update { record_id = s.first.id, rev = s.first.rev, content = {}, tag = "t" } end
  if key.name == "c" then data.create { type = "note", content = {}, tag = "t" } end
end
function app.reply(s, r, ctx)
  note(s, "reply " .. r.op .. (r.ok and "" or " " .. r.error.code), r)
  if r.records then s.first = r.records[1]; note(s, "record", r.records[1]) end
  if r.error then note(s, "error " .. r.error.code, r.error) end
end
function app.push(s, p, ctx) note(s, "push", p) end
function app.view(s) return ui.text(s.seen) end
return app
"#;

#[test]
fn the_tables_a_script_is_handed_have_the_fields_the_twin_declares() {
    let mut app = start_on("shapes.lua", SHAPES, (100, 30), |store| {
        store.seed("note", content(json!({ "text": "one" })), &bo());
    });
    let first = app.store().records()[0].id.clone();
    // A key with text and one without; a conflict; a write past the quota;
    // another participant's change.
    app.key("j").key("enter");
    app.store()
        .arm_conflict(&first, &bo(), content(json!({ "text": "bo's" })));
    app.key("u");
    app.store().set_quota(0, Duration::from_secs(60));
    app.key("c");
    let (_, pushes) = app.store().remote_create(&bo(), "note", content(json!({})));
    app.deliver(pushes);
    assert_eq!(app.error(), None);

    let screen = app.screen();
    let seen = |what: &str| -> BTreeSet<String> {
        let prefix = format!("{what}: ");
        screen
            .lines()
            .filter_map(|l| l.strip_prefix(&prefix))
            .flat_map(|l| l.split(' '))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(seen("ctx"), class("Ctx"), "{screen}");
    assert_eq!(seen("who"), class("Who"));
    // Nil fields are not there: a key without text has no `text`.
    let mut key = class("Key");
    assert_eq!(seen("key"), key);
    key.remove("text");
    assert!(screen.contains("key: alt ctrl name shift\n"), "{screen}");
    assert_eq!(seen("record"), class("Record"));
    assert_eq!(seen("push"), class("Push"));
    // A reply has the fields of its kind; together they are the class.
    let mut reply = seen("reply list");
    assert_eq!(reply, set(["id", "ok", "op", "records", "request", "tag"]));
    let conflict = seen("reply update conflict");
    assert_eq!(
        conflict,
        set(["current", "error", "id", "ok", "op", "request", "tag"])
    );
    reply.extend(conflict);
    reply.extend(seen("reply create rate_limited"));
    reply.insert("record".into());
    assert_eq!(reply, class("Reply"));
    let mut error = seen("error conflict");
    assert_eq!(error, set(["code", "message"]));
    error.extend(seen("error rate_limited"));
    assert_eq!(error, class("DataError"));
}
