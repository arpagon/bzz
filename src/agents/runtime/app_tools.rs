//! The tools with which an agent uses the records of the apps open in its
//! conversation's channel (v0.16 M7, plan D8): `apps`, `records`,
//! `create_record`, `update_record` and `delete_record`
//! (`docs/agent-host-protocol.md`, section 5).
//!
//! They are checked here as `send`, `read` and `react` are in
//! [`super::tools`]: every argument, with a text that names the right one.
//! No argument names a channel: the channel is the one of the conversation
//! the agent was woken in, which bzz takes from the calling session's live
//! turn. Which apps and which record types are allowed is decided where the
//! call is served (`agent::apps`), against what the owner installed,
//! enabled and opened there.
//!
//! With them, the tools with which an agent writes and changes an app (v0.16
//! M5, plan D17): `app_guide` (the author guide and the contract), `apps`
//! with an `app` (one app's script, manifest, brief and version),
//! `try_app` (a script loaded apart, on records in memory, and the screen
//! it draws) and `propose_app` (a version, put to the owner). Nothing about
//! apps is in an agent's prompt beforehand: these descriptions are how an
//! agent learns that apps exist.

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::apps::dev::{Seed, parse_seed};
use crate::apps::session::headless::Step;
use crate::apps::store::manifest::{BRIEF_BYTES, SCRIPT_BYTES, TITLE_CHARS};
use crate::protocol::record::{MAX_CONTENT_BYTES, valid_name};

/// The most records one `records` call returns.
pub const MAX_LIST_LIMIT: usize = 200;
pub const DEFAULT_LIST_LIMIT: usize = 50;
/// The longest `match`, as a script's (`docs/app-api.md`, "Limits").
pub const MAX_MATCH_BYTES: usize = 256;

/// The tools' names on the wire, in the order they are listed.
pub const NAMES: [&str; 8] = [
    "apps",
    "records",
    "create_record",
    "update_record",
    "delete_record",
    "app_guide",
    "try_app",
    "propose_app",
];

/// The size a script is tried at when none is given: bzz's app pane.
pub const TRY_WIDTH: u16 = 40;
pub const TRY_HEIGHT: u16 = 16;
/// The sizes a script may be tried at.
pub const TRY_WIDTHS: std::ops::RangeInclusive<u16> = 20..=200;
pub const TRY_HEIGHTS: std::ops::RangeInclusive<u16> = 5..=80;
/// The most steps (keys, pastes, waits, sizes) of one try.
pub const MAX_TRY_STEPS: usize = 200;
/// The most milliseconds one try may let pass, all its waits together.
pub const MAX_TRY_WAIT_MS: u64 = 600_000;
/// The most records a try may be given to start with.
pub const MAX_TRY_RECORDS: usize = 200;

/// A script to try, as `try_app` was given it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TryCall {
    pub script: String,
    pub steps: Vec<Step>,
    pub width: u16,
    pub height: u16,
    /// Records to start with.
    pub records: Vec<Seed>,
    /// Start also with a copy of the current records of this app, open in
    /// the conversation's channel. A copy: what the script writes stays in
    /// memory.
    pub from_app: Option<String>,
}

/// One call of an app tool, checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCall {
    /// The apps open in the conversation's channel, with their briefs.
    Apps,
    Records {
        app: String,
        record_type: String,
        matching: Option<String>,
        limit: usize,
    },
    Create {
        app: String,
        record_type: String,
        content: Map<String, Value>,
    },
    Update {
        app: String,
        id: Uuid,
        rev: String,
        content: Map<String, Value>,
    },
    Delete {
        app: String,
        id: Uuid,
        rev: String,
    },
    /// One app open in the conversation's channel: its script, manifest,
    /// brief, the version that runs, and the versions this agent proposed.
    Read {
        app: String,
    },
    /// The author guide and the contract.
    Guide,
    Try(TryCall),
    /// A version of an app, new or installed, with its brief.
    Propose {
        app: String,
        script: String,
        brief: String,
        title: Option<String>,
    },
}

impl AppCall {
    /// Whether the call writes to the relay as the agent.
    pub const fn writes(&self) -> bool {
        matches!(
            self,
            Self::Create { .. } | Self::Update { .. } | Self::Delete { .. }
        )
    }

    /// The app it names, when it names one.
    pub fn app(&self) -> Option<&str> {
        match self {
            Self::Apps | Self::Guide => None,
            Self::Try(call) => call.from_app.as_deref(),
            Self::Records { app, .. }
            | Self::Create { app, .. }
            | Self::Update { app, .. }
            | Self::Delete { app, .. }
            | Self::Read { app }
            | Self::Propose { app, .. } => Some(app),
        }
    }

    /// Whether it is one of the tools of an app's script (M5), served
    /// beside the store of apps rather than on the records.
    pub const fn authors(&self) -> bool {
        matches!(
            self,
            Self::Read { .. } | Self::Guide | Self::Try(_) | Self::Propose { .. }
        )
    }
}

const APPS_ARGUMENTS: &str = "`app` (string): one app, to read its script, manifest and brief";
const GUIDE_ARGUMENTS: &str = "none";
const TRY_ARGUMENTS: &str = "`script` (string, required), `keys` (array of strings), `width` and `height` (integers), `records` (array of {type, content, by}) and `from_app` (string)";
const PROPOSE_ARGUMENTS: &str = "`app` (string, required), `script` (string, required), `brief` (string, required) and `title` (string)";
const RECORDS_ARGUMENTS: &str = "`app` (string, required), `type` (string, required), `match` (string) and `limit` (integer, 1 to 200)";
const CREATE_ARGUMENTS: &str =
    "`app` (string, required), `type` (string, required) and `content` (object, required)";
const UPDATE_ARGUMENTS: &str = "`app` (string, required), `id` (string, required), `rev` (string, required) and `content` (object, required)";
const DELETE_ARGUMENTS: &str =
    "`app` (string, required), `id` (string, required) and `rev` (string, required)";

/// The arguments as an object; none at all is an empty one.
fn object<'a>(
    tool: &str,
    arguments: &'a Value,
    described: &str,
    empty: &'a Map<String, Value>,
) -> Result<&'a Map<String, Value>, String> {
    match arguments {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(empty),
        _ => Err(format!(
            "The arguments of `{tool}` must be an object with {described}."
        )),
    }
}

/// The value of one argument under its name or one other name a model may
/// guess. Both given is a mistake.
fn one_of<'a>(
    tool: &str,
    map: &'a Map<String, Value>,
    names: &[&'static str],
) -> Result<Option<(&'static str, &'a Value)>, String> {
    let mut given = names
        .iter()
        .filter_map(|name| map.get(*name).map(|value| (*name, value)));
    let first = given.next();
    if let (Some((name, _)), Some((other, _))) = (first, given.next()) {
        return Err(format!(
            "`{tool}` got both `{name}` and `{other}`. Give it once, as `{}`.",
            names[0]
        ));
    }
    Ok(first)
}

fn reject_unknown(
    tool: &str,
    map: &Map<String, Value>,
    known: &[&str],
    described: &str,
) -> Result<(), String> {
    match map.keys().find(|name| !known.contains(&name.as_str())) {
        Some(unknown) if described == GUIDE_ARGUMENTS => Err(format!(
            "`{tool}` takes no arguments; `{unknown}` is not one."
        )),
        Some(unknown) if described == APPS_ARGUMENTS => Err(format!(
            "`{tool}` takes only `app`; `{unknown}` is not an argument of it. The conversation's channel is always the one you were asked in."
        )),
        Some(unknown) => Err(format!(
            "`{tool}` has no argument `{unknown}`. Its arguments are {described}."
        )),
        None => Ok(()),
    }
}

fn name_argument(
    tool: &str,
    map: &Map<String, Value>,
    argument: &'static str,
    described: &str,
    what: &str,
) -> Result<String, String> {
    let Some(value) = map.get(argument) else {
        return Err(format!(
            "`{tool}` needs `{argument}` (string): {what}. Its arguments are {described}."
        ));
    };
    let Some(name) = value.as_str() else {
        return Err(format!("`{argument}` of `{tool}` must be a string."));
    };
    if !valid_name(name) {
        return Err(format!(
            "`{argument}` of `{tool}` is not a name an app or a record type can have: a lowercase letter, then up to 31 of a-z, 0-9, _ or -. `apps` lists the apps and their types."
        ));
    }
    Ok(name.to_owned())
}

fn content_argument(tool: &str, map: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(value) = map.get("content") else {
        return Err(format!(
            "`{tool}` needs `content` (object): the record's whole content, every field. The app's brief says what the fields mean."
        ));
    };
    let content = match value {
        Value::Object(content) => content.clone(),
        // A model may send the object as JSON text.
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(content)) => content,
            _ => {
                return Err(format!(
                    "`content` of `{tool}` must be an object of named fields, such as {{\"title\": \"…\"}}."
                ));
            }
        },
        _ => {
            return Err(format!(
                "`content` of `{tool}` must be an object of named fields, such as {{\"title\": \"…\"}}."
            ));
        }
    };
    // The service says the same, but this says it before anything is
    // looked up.
    let bytes = serde_json::to_string(&content).map_or(usize::MAX, |json| json.len());
    if bytes > MAX_CONTENT_BYTES {
        return Err(format!(
            "too_large: `content` of `{tool}` is {bytes} bytes of JSON; a record may hold {MAX_CONTENT_BYTES}. Write less into it."
        ));
    }
    Ok(content)
}

fn id_argument(tool: &str, map: &Map<String, Value>, described: &str) -> Result<Uuid, String> {
    let Some((name, value)) = one_of(tool, map, &["id", "record_id"])? else {
        return Err(format!(
            "`{tool}` needs `id` (string): the record's id, as `records` gives it. Its arguments are {described}."
        ));
    };
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id.trim()).ok())
        .ok_or_else(|| {
            format!("`{name}` of `{tool}` must be a record's id, as `records` gives it.")
        })
}

fn rev_argument(tool: &str, map: &Map<String, Value>, described: &str) -> Result<String, String> {
    let Some((name, value)) = one_of(tool, map, &["rev", "revision"])? else {
        return Err(format!(
            "`{tool}` needs `rev` (string): the record's `rev` as you last saw it, from `records` or from an earlier answer. Its arguments are {described}."
        ));
    };
    value
        .as_str()
        .map(str::trim)
        .filter(|rev| rev.len() == 64 && rev.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            format!(
                "`{name}` of `{tool}` must be a record's `rev` exactly as `records` gave it: 64 hex characters."
            )
        })
}

fn parse_records(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "records";
    let empty = Map::new();
    let map = object(TOOL, arguments, RECORDS_ARGUMENTS, &empty)?;
    reject_unknown(
        TOOL,
        map,
        &["app", "type", "match", "limit"],
        RECORDS_ARGUMENTS,
    )?;
    let app = name_argument(TOOL, map, "app", RECORDS_ARGUMENTS, "the app's name")?;
    let record_type = name_argument(
        TOOL,
        map,
        "type",
        RECORDS_ARGUMENTS,
        "one of the record types the app declared",
    )?;
    let matching = match map.get("match") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.len() <= MAX_MATCH_BYTES => {
            Some(text.clone()).filter(|text| !text.is_empty())
        }
        Some(Value::String(_)) => {
            return Err(format!(
                "`match` of `records` is longer than {MAX_MATCH_BYTES} bytes."
            ));
        }
        Some(_) => return Err("`match` of `records` must be a string.".into()),
    };
    let limit = match map.get("limit") {
        None => DEFAULT_LIST_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_LIST_LIMIT).contains(limit))
            .ok_or_else(|| {
                format!("`limit` of `records` must be an integer from 1 to {MAX_LIST_LIMIT}.")
            })?,
    };
    Ok(AppCall::Records {
        app,
        record_type,
        matching,
        limit,
    })
}

fn parse_create(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "create_record";
    let empty = Map::new();
    let map = object(TOOL, arguments, CREATE_ARGUMENTS, &empty)?;
    reject_unknown(TOOL, map, &["app", "type", "content"], CREATE_ARGUMENTS)?;
    Ok(AppCall::Create {
        app: name_argument(TOOL, map, "app", CREATE_ARGUMENTS, "the app's name")?,
        record_type: name_argument(
            TOOL,
            map,
            "type",
            CREATE_ARGUMENTS,
            "one of the record types the app declared",
        )?,
        content: content_argument(TOOL, map)?,
    })
}

fn parse_update(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "update_record";
    let empty = Map::new();
    let map = object(TOOL, arguments, UPDATE_ARGUMENTS, &empty)?;
    reject_unknown(
        TOOL,
        map,
        &["app", "id", "record_id", "rev", "revision", "content"],
        UPDATE_ARGUMENTS,
    )?;
    Ok(AppCall::Update {
        app: name_argument(TOOL, map, "app", UPDATE_ARGUMENTS, "the app's name")?,
        id: id_argument(TOOL, map, UPDATE_ARGUMENTS)?,
        rev: rev_argument(TOOL, map, UPDATE_ARGUMENTS)?,
        content: content_argument(TOOL, map)?,
    })
}

fn parse_delete(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "delete_record";
    let empty = Map::new();
    let map = object(TOOL, arguments, DELETE_ARGUMENTS, &empty)?;
    reject_unknown(
        TOOL,
        map,
        &["app", "id", "record_id", "rev", "revision"],
        DELETE_ARGUMENTS,
    )?;
    Ok(AppCall::Delete {
        app: name_argument(TOOL, map, "app", DELETE_ARGUMENTS, "the app's name")?,
        id: id_argument(TOOL, map, DELETE_ARGUMENTS)?,
        rev: rev_argument(TOOL, map, DELETE_ARGUMENTS)?,
    })
}

fn text_argument(
    tool: &str,
    map: &Map<String, Value>,
    argument: &'static str,
    limit: usize,
    what: &str,
    described: &str,
) -> Result<String, String> {
    let Some(value) = map.get(argument) else {
        return Err(format!(
            "`{tool}` needs `{argument}` (string): {what}. Its arguments are {described}."
        ));
    };
    let Some(text) = value.as_str() else {
        return Err(format!(
            "`{argument}` of `{tool}` must be a string: {what}."
        ));
    };
    if text.trim().is_empty() {
        return Err(format!("`{argument}` of `{tool}` is empty: {what}."));
    }
    if text.len() > limit {
        return Err(format!(
            "too_large: `{argument}` of `{tool}` is {} bytes; it may be {limit}.",
            text.len()
        ));
    }
    Ok(text.to_owned())
}

fn size_argument(
    map: &Map<String, Value>,
    argument: &str,
    default: u16,
    range: &std::ops::RangeInclusive<u16>,
) -> Result<u16, String> {
    match map.get(argument) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|size| u16::try_from(size).ok())
            .filter(|size| range.contains(size))
            .ok_or_else(|| {
                format!(
                    "`{argument}` of `try_app` must be an integer from {} to {}. bzz's app pane is {TRY_WIDTH} by {TRY_HEIGHT}.",
                    range.start(),
                    range.end()
                )
            }),
    }
}

fn parse_apps(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "apps";
    let empty = Map::new();
    let map = object(TOOL, arguments, APPS_ARGUMENTS, &empty)?;
    reject_unknown(TOOL, map, &["app"], APPS_ARGUMENTS)?;
    match map.get("app") {
        None | Some(Value::Null) => Ok(AppCall::Apps),
        Some(_) => Ok(AppCall::Read {
            app: name_argument(TOOL, map, "app", APPS_ARGUMENTS, "the app's name")?,
        }),
    }
}

fn parse_try(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "try_app";
    let empty = Map::new();
    let map = object(TOOL, arguments, TRY_ARGUMENTS, &empty)?;
    reject_unknown(
        TOOL,
        map,
        &["script", "keys", "width", "height", "records", "from_app"],
        TRY_ARGUMENTS,
    )?;
    let script = text_argument(
        TOOL,
        map,
        "script",
        SCRIPT_BYTES,
        "the whole Lua script, as it would be proposed",
        TRY_ARGUMENTS,
    )?;
    let steps = match map.get("keys") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(keys)) => {
            if keys.len() > MAX_TRY_STEPS {
                return Err(format!(
                    "`keys` of `try_app` holds {}; a try may have {MAX_TRY_STEPS}.",
                    keys.len()
                ));
            }
            let mut steps = Vec::with_capacity(keys.len());
            let mut waited = 0_u64;
            for (index, key) in keys.iter().enumerate() {
                let Some(written) = key.as_str() else {
                    return Err(format!(
                        "`keys` of `try_app`: item {} is not a string. Each is a key (`j`, `enter`, `ctrl+u`, `shift+tab`), `type:some text`, `paste:some text`, `resize:40x16` or `wait:500`.",
                        index + 1
                    ));
                };
                let step = Step::parse(written)
                    .map_err(|error| format!("`keys` of `try_app`, item {}: {error}", index + 1))?;
                match &step {
                    Step::Paste(text) | Step::Type(text)
                        if text.len() > crate::apps::runtime::limits::PASTE_BYTES =>
                    {
                        return Err(format!(
                            "`keys` of `try_app`, item {}: a paste or a text may be {} bytes.",
                            index + 1,
                            crate::apps::runtime::limits::PASTE_BYTES
                        ));
                    }
                    Step::Resize(width, height)
                        if !TRY_WIDTHS.contains(width) || !TRY_HEIGHTS.contains(height) =>
                    {
                        return Err(format!(
                            "`keys` of `try_app`, item {}: a size is {} to {} columns by {} to {} rows.",
                            index + 1,
                            TRY_WIDTHS.start(),
                            TRY_WIDTHS.end(),
                            TRY_HEIGHTS.start(),
                            TRY_HEIGHTS.end()
                        ));
                    }
                    Step::Advance(ms) => {
                        waited = waited.saturating_add(*ms);
                        if waited > MAX_TRY_WAIT_MS {
                            return Err(format!(
                                "`keys` of `try_app`: the waits come to more than {MAX_TRY_WAIT_MS} ms."
                            ));
                        }
                    }
                    _ => {}
                }
                steps.push(step);
            }
            steps
        }
        Some(_) => {
            return Err(
                "`keys` of `try_app` must be an array of strings, such as [\"j\", \"type:milk\", \"enter\"].".into(),
            );
        }
    };
    let records = match map.get("records") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            if items.len() > MAX_TRY_RECORDS {
                return Err(format!(
                    "`records` of `try_app` holds {}; a try may start with {MAX_TRY_RECORDS}.",
                    items.len()
                ));
            }
            for (index, item) in items.iter().enumerate() {
                let bytes = item
                    .get("content")
                    .map_or(0, |content| content.to_string().len());
                if bytes > MAX_CONTENT_BYTES {
                    return Err(format!(
                        "`records` of `try_app`: record {} has {bytes} bytes of content; a record may hold {MAX_CONTENT_BYTES}.",
                        index + 1
                    ));
                }
            }
            parse_seed(&Value::Array(items.clone()).to_string())
                .map_err(|error| format!("`records` of `try_app`: {error}"))?
        }
        Some(_) => {
            return Err("`records` of `try_app` must be an array of {\"type\": …, \"content\": {…}, \"by\": \"name\"}.".into());
        }
    };
    let from_app = match map.get("from_app") {
        None | Some(Value::Null) => None,
        Some(_) => Some(name_argument(
            TOOL,
            map,
            "from_app",
            TRY_ARGUMENTS,
            "an app open in this channel, whose records the script starts with",
        )?),
    };
    Ok(AppCall::Try(TryCall {
        script,
        steps,
        width: size_argument(map, "width", TRY_WIDTH, &TRY_WIDTHS)?,
        height: size_argument(map, "height", TRY_HEIGHT, &TRY_HEIGHTS)?,
        records,
        from_app,
    }))
}

fn parse_propose(arguments: &Value) -> Result<AppCall, String> {
    const TOOL: &str = "propose_app";
    let empty = Map::new();
    let map = object(TOOL, arguments, PROPOSE_ARGUMENTS, &empty)?;
    reject_unknown(
        TOOL,
        map,
        &["app", "script", "brief", "title"],
        PROPOSE_ARGUMENTS,
    )?;
    let app = name_argument(
        TOOL,
        map,
        "app",
        PROPOSE_ARGUMENTS,
        "the app's name: an installed app's to change it, or a new one",
    )?;
    let script = text_argument(
        TOOL,
        map,
        "script",
        SCRIPT_BYTES,
        "the whole Lua script of the version",
        PROPOSE_ARGUMENTS,
    )?;
    let brief = text_argument(
        TOOL,
        map,
        "brief",
        BRIEF_BYTES,
        "the brief, kept true: what the app does, its keys, and each record type with its fields",
        PROPOSE_ARGUMENTS,
    )?;
    let title = match map.get("title") {
        None | Some(Value::Null) => None,
        Some(Value::String(title))
            if !title.trim().is_empty() && title.chars().count() <= TITLE_CHARS =>
        {
            Some(title.trim().to_owned())
        }
        Some(_) => {
            return Err(format!(
                "`title` of `propose_app` must be a string of 1 to {TITLE_CHARS} characters."
            ));
        }
    };
    Ok(AppCall::Propose {
        app,
        script,
        brief,
        title,
    })
}

/// Checks one call of an app tool. `None`: `name` is not one. The error is
/// the text the model reads.
pub fn parse(name: &str, arguments: &Value) -> Option<Result<AppCall, String>> {
    let nothing = |text: String| text + " Nothing was changed.";
    let empty = Map::new();
    Some(match name {
        "apps" => parse_apps(arguments),
        "app_guide" => object(name, arguments, GUIDE_ARGUMENTS, &empty)
            .and_then(|map| reject_unknown(name, map, &[], GUIDE_ARGUMENTS))
            .map(|()| AppCall::Guide),
        "try_app" => parse_try(arguments).map_err(|text| text + " Nothing was tried."),
        "propose_app" => parse_propose(arguments)
            .map_err(|text| text + " Nothing was proposed, and the owner was not asked."),
        "records" => parse_records(arguments),
        "create_record" => parse_create(arguments).map_err(nothing),
        "update_record" => parse_update(arguments).map_err(nothing),
        "delete_record" => parse_delete(arguments).map_err(nothing),
        _ => return None,
    })
}

/// The tools as an MCP server lists them, after `send`, `read` and `react`.
pub fn definitions() -> Vec<Value> {
    let app = json!({"type": "string", "description": "The app's name, as `apps` lists it."});
    let record_type = json!({"type": "string", "description": "One of the record types the app declared, as `apps` lists them."});
    let id = json!({"type": "string", "description": "The record's id, as `records` gives it."});
    let rev = json!({"type": "string", "description": "The record's `rev` as you last saw it. If the record changed since, nothing is written and you get its current state."});
    let content = json!({"type": "object", "description": "The record's whole content: every field, not only the ones you change. The app's brief says what the fields mean."});
    vec![
        json!({
            "name": "apps",
            "description": "List the apps open in the channel of the conversation you were asked in: each with its brief, which says what its records mean, its record types and the version that runs. People of the channel share an app's records; you can read and change them with `records`, `create_record`, `update_record` and `delete_record`. With `app`, one app whole: its script, manifest and brief, the version that runs, and what became of the versions you proposed of it. An app is a small Lua script that bzz draws beside the conversation; `app_guide` says how one is written, and `propose_app` asks the owner to install a new one or a change.",
            "inputSchema": {
                "type": "object",
                "properties": {"app": {"type": "string", "description": "An app open in this channel, to read its script, manifest and brief."}},
                "additionalProperties": false
            }
        }),
        json!({
            "name": "records",
            "description": "List the records of one type of an app open in this channel. Each record is {id, type, rev, content, by}: `rev` is what you pass back to change or delete it, `by` who wrote it last. Records come in no order: an order, when the app has one, is a field of the content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app": app,
                    "type": record_type,
                    "match": {"type": "string", "description": "Only records whose title (their `title` field) contains this, ignoring case."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT, "description": "How many records at most. Default 50."}
                },
                "required": ["app", "type"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "create_record",
            "description": "Create a record of an app open in this channel. Everyone who has the app open sees it at once. Answers the record as stored, with its id and rev.",
            "inputSchema": {
                "type": "object",
                "properties": {"app": app, "type": record_type, "content": content},
                "required": ["app", "type", "content"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "update_record",
            "description": "Replace the whole content of a record of an app open in this channel. If someone changed it after the `rev` you give, nothing is written and the answer holds the record as it is now: decide again from that, as the app's brief says.",
            "inputSchema": {
                "type": "object",
                "properties": {"app": app, "id": id, "rev": rev, "content": content},
                "required": ["app", "id", "rev", "content"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "delete_record",
            "description": "Delete a record of an app open in this channel, under the same rule as `update_record`. Answers the record's last state.",
            "inputSchema": {
                "type": "object",
                "properties": {"app": app, "id": id, "rev": rev},
                "required": ["app", "id", "rev"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "app_guide",
            "description": "The guide to writing an app for bzz, and the contract (`api = 1`): how a script is shaped, its handlers, its records, the pane's size, what it cannot do, and what its brief must say. Read it before you write or change an app's script.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }),
        json!({
            "name": "try_app",
            "description": "Load a Lua script apart, give it keys and pastes, and get back the screen it draws as text, with any error as file:line. It runs on records kept in memory: give it some with `records`, or start from a copy of the records of an app open in this channel with `from_app`. Nothing is stored, nothing reaches the relay, and nobody sees it. Default size: 40 by 16, bzz's app pane.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": {"type": "string", "description": "The whole script."},
                    "keys": {"type": "array", "items": {"type": "string"}, "description": "What is done to it, in order: a key (`j`, `enter`, `esc`, `ctrl+u`, `shift+tab`, `f1`), `type:some text` (typed key by key), `paste:some text`, `resize:60x20`, `wait:500` (milliseconds pass, timers fire)."},
                    "width": {"type": "integer", "minimum": 20, "maximum": 200, "description": "Columns. Default 40."},
                    "height": {"type": "integer", "minimum": 5, "maximum": 80, "description": "Rows. Default 16."},
                    "records": {"type": "array", "items": {"type": "object"}, "description": "Records to start with: {\"type\": \"card\", \"content\": {…}, \"by\": \"bo\"}, `by` optional."},
                    "from_app": {"type": "string", "description": "An app open in this channel: start with a copy of its current records. What the script writes changes only the copy."}
                },
                "required": ["script"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "propose_app",
            "description": "Propose a version of an app, with its brief: a new app, or a change to an app installed here. bzz loads it apart first; a script that does not load, or a brief that does not name its record types, comes back at once with the error and is not offered. Otherwise the owner is asked to approve these exact bytes, and the answer is \"waiting for the owner\": you are told what became of it (applied, refused with the owner's reason, or failed with the error) at your next turn in this channel, and `apps` with `app` shows it any time. You cannot approve, open or install it yourself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app": {"type": "string", "description": "The app's name: an installed app's, to change it, or a new one (a lowercase letter, then up to 31 of a-z, 0-9, _ or -)."},
                    "script": {"type": "string", "description": "The whole Lua script."},
                    "brief": {"type": "string", "description": "What the app does, its keys, and each record type with its fields and rules: what the owner reads before approving, and what another client needs to share the records. Keep it true to the script."},
                    "title": {"type": "string", "description": "Its title, if it should change."}
                },
                "required": ["app", "script", "brief"],
                "additionalProperties": false
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(name: &str, arguments: Value) -> Result<AppCall, String> {
        parse(name, &arguments).expect("an app tool")
    }

    #[test]
    fn every_argument_is_checked_and_a_wrong_one_is_named() {
        assert_eq!(parse("send", &json!({})), None);
        assert_eq!(parsed("apps", Value::Null), Ok(AppCall::Apps));
        assert_eq!(parsed("apps", json!({})), Ok(AppCall::Apps));
        let smuggled = parsed("apps", json!({"channel": "x"})).unwrap_err();
        assert!(
            smuggled.contains("`channel` is not an argument of it"),
            "{smuggled}"
        );
        assert_eq!(
            parsed("apps", json!({"app": "board"})),
            Ok(AppCall::Read {
                app: "board".into()
            })
        );
        assert_eq!(
            parsed("records", json!({"app": "rota", "type": "day"})),
            Ok(AppCall::Records {
                app: "rota".into(),
                record_type: "day".into(),
                matching: None,
                limit: DEFAULT_LIST_LIMIT,
            })
        );
        assert_eq!(
            parsed(
                "records",
                json!({"app": "board", "type": "card", "match": "milk", "limit": 200})
            ),
            Ok(AppCall::Records {
                app: "board".into(),
                record_type: "card".into(),
                matching: Some("milk".into()),
                limit: 200,
            })
        );
        let error = |name: &str, arguments: Value| parsed(name, arguments).unwrap_err();
        assert!(error("records", json!({"type": "day"})).contains("needs `app`"));
        assert!(error("records", json!({"app": "rota"})).contains("needs `type`"));
        assert!(error("records", json!({"app": "Rota", "type": "day"})).contains("lowercase"));
        assert!(
            error(
                "records",
                json!({"app": "rota", "type": "bzz.app.rota.day"})
            )
            .contains("lowercase"),
            "the long form is not a type an agent names"
        );
        assert!(
            error("records", json!({"app": "rota", "type": "day", "limit": 0}))
                .contains("1 to 200")
        );
        assert!(
            error(
                "records",
                json!({"app": "rota", "type": "day", "match": "x".repeat(257)})
            )
            .contains("256")
        );
        // No argument may name the channel, the place or the session.
        for smuggled in ["channel", "h", "place", "sessionId", "community"] {
            for tool in ["records", "create_record", "update_record", "delete_record"] {
                let refused = error(tool, json!({"app": "rota", smuggled: "x"}));
                assert!(
                    refused.contains(&format!("no argument `{smuggled}`")),
                    "{tool}: {refused}"
                );
            }
        }
        let id = Uuid::new_v4();
        let rev = "A".repeat(64);
        assert_eq!(
            parsed(
                "update_record",
                json!({"app": "rota", "record_id": id.to_string(), "revision": rev, "content": "{\"holder\": \"ana\"}"})
            ),
            Ok(AppCall::Update {
                app: "rota".into(),
                id,
                rev: "a".repeat(64),
                content: json!({"holder": "ana"}).as_object().unwrap().clone(),
            }),
            "the names a script uses, and content sent as JSON text"
        );
        let both = error(
            "delete_record",
            json!({"app": "rota", "id": id.to_string(), "record_id": id.to_string(), "rev": rev}),
        );
        assert!(both.contains("once, as `id`"), "{both}");
        assert!(
            error(
                "delete_record",
                json!({"app": "rota", "id": "7", "rev": rev})
            )
            .contains("a record's id")
        );
        assert!(
            error(
                "delete_record",
                json!({"app": "rota", "id": id.to_string(), "rev": "1"})
            )
            .contains("64 hex")
        );
        assert!(
            error(
                "delete_record",
                json!({"app": "rota", "id": id.to_string()})
            )
            .contains("needs `rev`")
        );
        assert!(
            error("create_record", json!({"app": "rota", "type": "day"}))
                .contains("needs `content`")
        );
        assert!(
            error(
                "create_record",
                json!({"app": "rota", "type": "day", "content": [1]})
            )
            .contains("object of named fields")
        );
        let large = error(
            "create_record",
            json!({"app": "rota", "type": "day", "content": {"x": "y".repeat(MAX_CONTENT_BYTES)}}),
        );
        assert!(large.starts_with("too_large:"), "{large}");
        for tool in ["create_record", "update_record", "delete_record"] {
            assert!(
                error(tool, json!({})).ends_with("Nothing was changed."),
                "{tool}"
            );
        }
    }

    #[test]
    fn the_tools_of_a_script_check_every_argument_and_say_what_was_not_done() {
        let error = |name: &str, arguments: Value| parsed(name, arguments).unwrap_err();
        assert_eq!(parsed("app_guide", Value::Null), Ok(AppCall::Guide));
        assert!(error("app_guide", json!({"app": "x"})).contains("takes no arguments"));
        // A try: the pane's size by default, keys as `bzz app try` reads them.
        let Ok(AppCall::Try(call)) = parsed(
            "try_app",
            json!({"script": "return {}", "keys": ["j", "type:milk", "enter", "wait:500", "resize:60x20"],
                   "records": [{"type": "card", "content": {"title": "a"}, "by": "bo"}]}),
        ) else {
            panic!("a try");
        };
        assert_eq!((call.width, call.height), (TRY_WIDTH, TRY_HEIGHT));
        assert_eq!(
            call.steps,
            [
                Step::Key(crate::apps::runtime::Key::named("j")),
                Step::Type("milk".into()),
                Step::Key(crate::apps::runtime::Key::named("enter")),
                Step::Advance(500),
                Step::Resize(60, 20),
            ]
        );
        assert_eq!(call.records.len(), 1);
        assert_eq!(call.records[0].by.as_deref(), Some("bo"));
        assert_eq!(call.from_app, None);
        for (arguments, expected) in [
            (json!({}), "needs `script`"),
            (json!({"script": "  "}), "is empty"),
            (json!({"script": "x".repeat(SCRIPT_BYTES + 1)}), "too_large"),
            (json!({"script": "x", "keys": "j"}), "must be an array"),
            (
                json!({"script": "x", "keys": ["nokey"]}),
                "item 1: nokey: not a key",
            ),
            (
                json!({"script": "x", "keys": ["resize:1000x5"]}),
                "a size is",
            ),
            (json!({"script": "x", "keys": ["wait:600001"]}), "more than"),
            (json!({"script": "x", "width": 10}), "from 20 to 200"),
            (json!({"script": "x", "height": 500}), "from 5 to 80"),
            (
                json!({"script": "x", "records": [{"type": "Card", "content": {}}]}),
                "needs \"type\"",
            ),
            (json!({"script": "x", "from_app": "Board"}), "lowercase"),
            (
                json!({"script": "x", "channel": "y"}),
                "no argument `channel`",
            ),
        ] {
            let refused = error("try_app", arguments.clone());
            assert!(refused.contains(expected), "{arguments}: {refused}");
            assert!(refused.ends_with("Nothing was tried."), "{refused}");
        }
        let many = (0..=MAX_TRY_STEPS).map(|_| json!("j")).collect::<Vec<_>>();
        assert!(error("try_app", json!({"script": "x", "keys": many})).contains("may have 200"));
        // A proposal: the app, the whole script and its brief.
        assert_eq!(
            parsed(
                "propose_app",
                json!({"app": "board", "script": "s", "brief": "b", "title": " Board "})
            ),
            Ok(AppCall::Propose {
                app: "board".into(),
                script: "s".into(),
                brief: "b".into(),
                title: Some("Board".into()),
            })
        );
        for (arguments, expected) in [
            (json!({"script": "s", "brief": "b"}), "needs `app`"),
            (json!({"app": "board", "brief": "b"}), "needs `script`"),
            (json!({"app": "board", "script": "s"}), "needs `brief`"),
            (
                json!({"app": "board", "script": "s", "brief": " "}),
                "`brief` of `propose_app` is empty",
            ),
            (
                json!({"app": "board", "script": "s", "brief": "b".repeat(BRIEF_BYTES + 1)}),
                "too_large",
            ),
            (
                json!({"app": "board", "script": "s", "brief": "b", "title": ""}),
                "`title`",
            ),
            (
                json!({"app": "board", "script": "s", "brief": "b", "approve": true}),
                "no argument `approve`",
            ),
            (
                json!({"app": "board", "script": "s", "brief": "b", "version": 3}),
                "no argument `version`",
            ),
        ] {
            let refused = error("propose_app", arguments.clone());
            assert!(refused.contains(expected), "{arguments}: {refused}");
            assert!(
                refused.ends_with("Nothing was proposed, and the owner was not asked."),
                "{refused}"
            );
        }
        assert!(AppCall::Guide.authors() && !AppCall::Apps.authors());
        for call in [AppCall::Guide, AppCall::Read { app: "x".into() }] {
            assert!(!call.writes(), "{call:?} publishes nothing");
        }
    }

    #[test]
    fn the_definitions_name_every_tool_once() {
        let names = definitions()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, NAMES);
        for tool in definitions() {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }
}
