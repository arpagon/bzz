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
//! The tools of M5 (read, try and propose a script) belong beside these.

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::protocol::record::{MAX_CONTENT_BYTES, valid_name};

/// The most records one `records` call returns.
pub const MAX_LIST_LIMIT: usize = 200;
pub const DEFAULT_LIST_LIMIT: usize = 50;
/// The longest `match`, as a script's (`docs/app-api.md`, "Limits").
pub const MAX_MATCH_BYTES: usize = 256;

/// The tools' names on the wire, in the order they are listed.
pub const NAMES: [&str; 5] = [
    "apps",
    "records",
    "create_record",
    "update_record",
    "delete_record",
];

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
            Self::Apps => None,
            Self::Records { app, .. }
            | Self::Create { app, .. }
            | Self::Update { app, .. }
            | Self::Delete { app, .. } => Some(app),
        }
    }
}

const APPS_ARGUMENTS: &str = "none";
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
        Some(unknown) if described == APPS_ARGUMENTS => Err(format!(
            "`{tool}` takes no arguments; `{unknown}` is not one. The conversation's channel is always the one you were asked in."
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

/// Checks one call of an app tool. `None`: `name` is not one. The error is
/// the text the model reads.
pub fn parse(name: &str, arguments: &Value) -> Option<Result<AppCall, String>> {
    let nothing = |text: String| text + " Nothing was changed.";
    let empty = Map::new();
    Some(match name {
        "apps" => object(name, arguments, APPS_ARGUMENTS, &empty)
            .and_then(|map| reject_unknown(name, map, &[], APPS_ARGUMENTS))
            .map(|()| AppCall::Apps),
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
            "description": "List the apps open in the channel of the conversation you were asked in: each with its brief, which says what its records mean, and its record types. People of the channel share an app's records; you can read and change them with `records`, `create_record`, `update_record` and `delete_record`.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
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
        assert!(smuggled.contains("takes no arguments"), "{smuggled}");
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
