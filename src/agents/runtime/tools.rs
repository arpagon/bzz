//! The three tools bzz serves to a harness: `send`, `read` and `react`
//! (`docs/agent-host-protocol.md`, section 5).
//!
//! Arguments are checked before anything else happens, and a wrong call is
//! answered with what is wrong and what is right, because a model may guess
//! an argument's name. No argument says which request is answered: that
//! comes from the calling session's live turn.

use serde_json::{Map, Value, json};

/// The longest message text, as the relay's own limit for a chat message.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_MENTIONS: usize = 50;
pub const DEFAULT_READ_LIMIT: usize = 20;
pub const MAX_READ_LIMIT: usize = 100;
const MAX_EMOJI_CHARS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolCall {
    Send {
        text: String,
        /// Ends the turn: the one final reply of the request.
        is_final: bool,
        mentions: Vec<String>,
    },
    Read {
        limit: usize,
        before: Option<String>,
    },
    React {
        emoji: String,
        /// The event reacted to; the request when absent.
        event: Option<String>,
    },
}

/// What a tool call answers (`{"text": …, "final": true}` or
/// `{"text": …, "isError": true}`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolReply {
    pub text: String,
    pub is_error: bool,
    pub is_final: bool,
}

impl ToolReply {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            is_final: false,
        }
    }

    pub fn final_reply(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            is_final: true,
        }
    }

    pub fn refused(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
            is_final: false,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({"text": self.text});
        if self.is_error {
            value["isError"] = true.into();
        }
        if self.is_final {
            value["final"] = true.into();
        }
        value
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            text: value.get("text")?.as_str()?.to_owned(),
            is_error: value
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            is_final: value.get("final").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

const SEND_ARGUMENTS: &str =
    "`text` (string, required), `final` (boolean) and `mentions` (array of public keys in hex)";
const READ_ARGUMENTS: &str = "`limit` (integer, 1 to 100) and `before` (an event id)";
const REACT_ARGUMENTS: &str =
    "`emoji` (string, required) and `event` (an event id; default: the message you are answering)";

fn is_hex_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn object<'a>(
    tool: &str,
    arguments: &'a Value,
    described: &str,
) -> Result<&'a Map<String, Value>, String> {
    match arguments {
        Value::Object(map) => Ok(map),
        // A call with no arguments at all.
        Value::Null => Err(format!("`{tool}` needs arguments: {described}.")),
        _ => Err(format!(
            "The arguments of `{tool}` must be an object with {described}."
        )),
    }
}

fn reject_unknown(
    tool: &str,
    map: &Map<String, Value>,
    known: &[&str],
    described: &str,
) -> Result<(), String> {
    match map.keys().find(|name| !known.contains(&name.as_str())) {
        Some(unknown) => Err(format!(
            "`{tool}` has no argument `{unknown}`. Its arguments are {described}."
        )),
        None => Ok(()),
    }
}

fn parse_send(arguments: &Value) -> Result<ToolCall, String> {
    const NOTHING: &str = " Nothing was sent.";
    let fail = |text: String| text + NOTHING;
    // `{}` and a missing object are the same mistake: no text.
    let empty = Map::new();
    let map = match arguments {
        Value::Null => &empty,
        other => object("send", other, SEND_ARGUMENTS).map_err(fail)?,
    };
    reject_unknown(
        "send",
        map,
        &["text", "message", "content", "final", "mentions"],
        SEND_ARGUMENTS,
    )
    .map_err(fail)?;
    // `message` and `content` are other names for `text`.
    let mut given = ["text", "message", "content"]
        .into_iter()
        .filter_map(|name| map.get(name).map(|value| (name, value)));
    let Some((name, value)) = given.next() else {
        return Err(fail(format!(
            "`send` needs `text` (string): the message to post. Its arguments are {SEND_ARGUMENTS}."
        )));
    };
    if let Some((other, _)) = given.next() {
        return Err(fail(format!(
            "`send` got both `{name}` and `{other}`. Give the message once, as `text`."
        )));
    }
    let Some(text) = value.as_str() else {
        return Err(fail(format!("`{name}` of `send` must be a string.")));
    };
    if text.trim().is_empty() {
        return Err(fail("`text` of `send` is empty.".into()));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(fail(format!(
            "`text` of `send` is {} bytes; the limit is {MAX_TEXT_BYTES}. Send a shorter message.",
            text.len()
        )));
    }
    let is_final = match map.get("final") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(fail("`final` of `send` must be true or false.".into())),
    };
    let mentions = match map.get("mentions") {
        None => Vec::new(),
        Some(Value::Array(values)) => {
            if values.len() > MAX_MENTIONS {
                return Err(fail(format!(
                    "`mentions` of `send` holds {}; the limit is {MAX_MENTIONS}.",
                    values.len()
                )));
            }
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|key| is_hex_id(key))
                        .map(str::to_ascii_lowercase)
                        .ok_or_else(|| {
                            fail(
                                "Each of `mentions` of `send` must be a public key of 64 hex characters."
                                    .into(),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(_) => {
            return Err(fail(
                "`mentions` of `send` must be an array of public keys in hex.".into(),
            ));
        }
    };
    Ok(ToolCall::Send {
        text: text.to_owned(),
        is_final,
        mentions,
    })
}

fn parse_read(arguments: &Value) -> Result<ToolCall, String> {
    let empty = Map::new();
    let map = match arguments {
        Value::Null => &empty,
        other => object("read", other, READ_ARGUMENTS)?,
    };
    reject_unknown("read", map, &["limit", "before"], READ_ARGUMENTS)?;
    let limit = match map.get("limit") {
        None => DEFAULT_READ_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_READ_LIMIT).contains(limit))
            .ok_or_else(|| {
                format!("`limit` of `read` must be an integer from 1 to {MAX_READ_LIMIT}.")
            })?,
    };
    let before = match map.get("before") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|id| is_hex_id(id))
                .map(str::to_ascii_lowercase)
                .ok_or("`before` of `read` must be an event id of 64 hex characters.")?,
        ),
    };
    Ok(ToolCall::Read { limit, before })
}

fn parse_react(arguments: &Value) -> Result<ToolCall, String> {
    const NOTHING: &str = " No reaction was added.";
    let fail = |text: String| text + NOTHING;
    let empty = Map::new();
    let map = match arguments {
        Value::Null => &empty,
        other => object("react", other, REACT_ARGUMENTS).map_err(fail)?,
    };
    reject_unknown("react", map, &["emoji", "event"], REACT_ARGUMENTS).map_err(fail)?;
    let Some(emoji) = map.get("emoji") else {
        return Err(fail(format!(
            "`react` needs `emoji` (string). Its arguments are {REACT_ARGUMENTS}."
        )));
    };
    let emoji = emoji
        .as_str()
        .map(str::trim)
        .filter(|emoji| !emoji.is_empty() && emoji.chars().count() <= MAX_EMOJI_CHARS)
        .ok_or_else(|| fail("`emoji` of `react` must be one emoji, as a string.".into()))?;
    let event = match map.get("event") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|id| is_hex_id(id))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| {
                    fail("`event` of `react` must be an event id of 64 hex characters.".into())
                })?,
        ),
    };
    Ok(ToolCall::React {
        emoji: emoji.to_owned(),
        event,
    })
}

/// Checks one call. The error is the text the model reads.
pub fn parse(name: &str, arguments: &Value) -> Result<ToolCall, String> {
    match name {
        "send" => parse_send(arguments),
        "read" => parse_read(arguments),
        "react" => parse_react(arguments),
        other => Err(format!(
            "There is no tool `{other}`. The tools are `send`, `read` and `react`."
        )),
    }
}

/// The tools as an MCP server lists them (`tools/list`).
pub fn definitions() -> Value {
    json!([
        {
            "name": "send",
            "description": "Post a message in the conversation you were asked in, as a reply to the message you are answering. Set `final` to true for your answer: it ends your turn, and only one final message is posted per request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The message to post."},
                    "final": {"type": "boolean", "description": "True for your answer, which ends your turn."},
                    "mentions": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Public keys (64 hex characters) of people to mention."
                    }
                },
                "required": ["text"],
                "additionalProperties": false
            }
        },
        {
            "name": "read",
            "description": "Read recent messages of the conversation you were asked in, newest last.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_READ_LIMIT},
                    "before": {"type": "string", "description": "Only messages older than this event id."}
                },
                "additionalProperties": false
            }
        },
        {
            "name": "react",
            "description": "Add an emoji reaction to a message of the conversation; by default, to the message you are answering.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "emoji": {"type": "string"},
                    "event": {"type": "string", "description": "The event id to react to."}
                },
                "required": ["emoji"],
                "additionalProperties": false
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_takes_text_under_three_names_and_says_what_is_wrong() {
        assert_eq!(
            parse("send", &json!({"text": "On it.", "final": true})),
            Ok(ToolCall::Send {
                text: "On it.".into(),
                is_final: true,
                mentions: Vec::new(),
            })
        );
        // The other names a model guesses (seen with Codex in M0).
        for name in ["message", "content"] {
            assert_eq!(
                parse("send", &json!({name: "hi"})),
                Ok(ToolCall::Send {
                    text: "hi".into(),
                    is_final: false,
                    mentions: Vec::new(),
                }),
                "{name}"
            );
        }
        let key = "AB".repeat(32);
        assert_eq!(
            parse("send", &json!({"text": "hi", "mentions": [key]})),
            Ok(ToolCall::Send {
                text: "hi".into(),
                is_final: false,
                mentions: vec!["ab".repeat(32)],
            })
        );
        let error = |arguments: Value| parse("send", &arguments).unwrap_err();
        let misnamed = error(json!({"msg": "hi"}));
        assert!(
            misnamed.contains("no argument `msg`")
                && misnamed.contains("`text` (string, required)"),
            "{misnamed}"
        );
        let missing = error(json!({"final": true}));
        assert!(missing.contains("needs `text`"), "{missing}");
        assert!(error(Value::Null).contains("needs `text`"));
        assert!(error(json!("hi")).contains("must be an object"));
        assert!(error(json!({"text": 5})).contains("`text` of `send` must be a string"));
        assert!(error(json!({"message": 5})).contains("`message` of `send` must be a string"));
        assert!(error(json!({"text": "  "})).contains("empty"));
        assert!(error(json!({"text": "a", "message": "b"})).contains("once, as `text`"));
        assert!(
            error(json!({"text": "a", "final": "true"}))
                .contains("`final` of `send` must be true or false")
        );
        assert!(error(json!({"text": "a", "mentions": "x"})).contains("array"));
        assert!(error(json!({"text": "a", "mentions": ["npub1x"]})).contains("64 hex"));
        assert!(error(json!({"text": "x".repeat(MAX_TEXT_BYTES + 1)})).contains("limit"));
        // No argument may name the request, the place or the session.
        for smuggled in [
            "request",
            "event",
            "replyTo",
            "channel",
            "sessionId",
            "place",
        ] {
            let refused = error(json!({"text": "hi", smuggled: "a".repeat(64)}));
            assert!(
                refused.contains(&format!("no argument `{smuggled}`")),
                "{refused}"
            );
        }
        for refused in [misnamed, missing] {
            assert!(refused.ends_with("Nothing was sent."), "{refused}");
        }
    }

    #[test]
    fn read_and_react_are_checked_the_same_way() {
        assert_eq!(
            parse("read", &Value::Null),
            Ok(ToolCall::Read {
                limit: DEFAULT_READ_LIMIT,
                before: None,
            })
        );
        assert_eq!(
            parse("read", &json!({"limit": 100, "before": "c".repeat(64)})),
            Ok(ToolCall::Read {
                limit: 100,
                before: Some("c".repeat(64)),
            })
        );
        for wrong in [
            json!({"limit": 0}),
            json!({"limit": 101}),
            json!({"limit": "5"}),
            json!({"limit": 1.5}),
        ] {
            assert!(
                parse("read", &wrong).unwrap_err().contains("1 to 100"),
                "{wrong}"
            );
        }
        assert!(
            parse("read", &json!({"before": "abc"}))
                .unwrap_err()
                .contains("64 hex")
        );
        assert!(
            parse("read", &json!({"count": 5}))
                .unwrap_err()
                .contains("no argument `count`")
        );
        assert_eq!(
            parse("react", &json!({"emoji": " 👍 "})),
            Ok(ToolCall::React {
                emoji: "👍".into(),
                event: None,
            })
        );
        assert!(
            parse("react", &json!({}))
                .unwrap_err()
                .contains("needs `emoji`")
        );
        assert!(
            parse("react", &json!({"emoji": ""}))
                .unwrap_err()
                .contains("one emoji")
        );
        assert!(
            parse("react", &json!({"emoji": "👍", "event": "x"}))
                .unwrap_err()
                .contains("64 hex")
        );
        assert!(
            parse("react", &json!({"reaction": "👍"}))
                .unwrap_err()
                .contains("no argument `reaction`")
        );
        let unknown = parse("post", &json!({"text": "hi"})).unwrap_err();
        assert!(
            unknown.contains("no tool `post`") && unknown.contains("`send`, `read` and `react`")
        );
    }

    #[test]
    fn replies_keep_their_shape_on_the_wire() {
        assert_eq!(
            ToolReply::final_reply("sent as abc").to_json(),
            json!({"text": "sent as abc", "final": true})
        );
        assert_eq!(
            ToolReply::ok("3 messages").to_json(),
            json!({"text": "3 messages"})
        );
        let refused = ToolReply::refused("why");
        assert_eq!(refused.to_json(), json!({"text": "why", "isError": true}));
        assert_eq!(ToolReply::from_json(&refused.to_json()), Some(refused));
        assert_eq!(ToolReply::from_json(&json!({"isError": true})), None);
        let listed = definitions();
        let names = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["send", "read", "react"]);
    }
}
