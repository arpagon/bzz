//! The boundary between an app and its records: requests go out, replies
//! and pushes come in. Plain types; nothing here knows a relay.
//!
//! The runtime turns a script's `data.*` call into a [`RecordRequest`] and
//! hands it to whoever owns the runtime. That owner answers every request
//! with exactly one [`RecordReply`], and sends a [`RecordPush`] for each
//! change another participant made. Behind the boundary sits the in-memory
//! store of [`super::memory`] (the headless driver, tests) or, from M2, the
//! relay's artifact service.
//!
//! What the two sides owe each other:
//!
//! - **Types are the app's short names** (`card`). Whoever serves a request
//!   makes the long form of them and checks the fence again (rule 17).
//! - **A revision is an opaque string**, handed back unchanged.
//! - **A list has no order** a script may rely on.
//! - **The requester's own writes are not pushed** back to it.
//! - **A resync is not a change**: when the set had to be fetched again,
//!   the owner sends only the real differences, which [`differences`]
//!   computes.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::limits;

/// A participant: an id that does not change, and a name for showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    /// The participant's public key, in hex. It never changes.
    pub id: String,
    /// What to show. It can change, and two people can have the same.
    pub name: String,
}

impl Who {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// One record, as a script sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// Set by the service. Ids do not sort by creation.
    pub id: String,
    /// The app's own short name of the type.
    pub record_type: String,
    /// The revision: opaque, to be passed back unchanged.
    pub rev: String,
    /// The app's own fields.
    pub content: Map<String, Value>,
    /// Who signed this revision. Any member of the channel can revise any
    /// record, so this is the last writer, not an owner.
    pub by: Who,
}

/// What a script asked for. `id` is what `data.*` returned to it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordRequest {
    pub id: u64,
    pub op: RecordOp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecordOp {
    /// The records of one type; with `matching`, those whose title contains
    /// it, ignoring case.
    List {
        record_type: String,
        matching: Option<String>,
    },
    Create {
        record_type: String,
        content: Map<String, Value>,
    },
    /// Replaces the whole content. `rev` is the revision the script holds.
    Update {
        record_id: String,
        record_type: String,
        rev: String,
        content: Map<String, Value>,
    },
    Delete {
        record_id: String,
        record_type: String,
        rev: String,
    },
}

impl RecordOp {
    /// The name the script used: `list`, `create`, `update` or `delete`.
    pub fn name(&self) -> &'static str {
        match self {
            RecordOp::List { .. } => "list",
            RecordOp::Create { .. } => "create",
            RecordOp::Update { .. } => "update",
            RecordOp::Delete { .. } => "delete",
        }
    }

    pub fn record_type(&self) -> &str {
        match self {
            RecordOp::List { record_type, .. }
            | RecordOp::Create { record_type, .. }
            | RecordOp::Update { record_type, .. }
            | RecordOp::Delete { record_type, .. } => record_type,
        }
    }

    pub fn is_write(&self) -> bool {
        !matches!(self, RecordOp::List { .. })
    }
}

/// The one answer to a request.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordReply {
    pub id: u64,
    pub result: Result<RecordAnswer, RecordError>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecordAnswer {
    /// `list`: the matching records, in no order.
    Records(Vec<Record>),
    /// `create` and `update`: the record as stored. `delete`: its last state.
    Record(Record),
}

/// Why a request was not done. [`RecordError::code`] is what a script reads.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    /// No connection, or the service could not answer now. Try again later.
    Unavailable { message: String },
    /// The revision was stale. `current` is the record as it is now.
    Conflict { current: Box<Record> },
    /// No such record: never there, or deleted meanwhile.
    NotFound,
    /// The request itself is wrong.
    BadRequest { message: String },
    /// The person's write quota is used up. Wait this many seconds.
    RateLimited { retry_after_secs: u64 },
    /// The content is larger than a record may be.
    TooLarge { message: String },
    /// The app has as many live records as it may have.
    Limit { message: String },
    /// The relay refused, and says why: a guest, an archived channel.
    Refused { reason: String },
}

impl RecordError {
    pub fn code(&self) -> &'static str {
        match self {
            RecordError::Unavailable { .. } => "unavailable",
            RecordError::Conflict { .. } => "conflict",
            RecordError::NotFound => "not_found",
            RecordError::BadRequest { .. } => "bad_request",
            RecordError::RateLimited { .. } => "rate_limited",
            RecordError::TooLarge { .. } => "too_large",
            RecordError::Limit { .. } => "limit",
            RecordError::Refused { .. } => "refused",
        }
    }

    pub fn message(&self) -> String {
        match self {
            RecordError::Unavailable { message }
            | RecordError::BadRequest { message }
            | RecordError::TooLarge { message }
            | RecordError::Limit { message } => message.clone(),
            RecordError::Conflict { .. } => "the record was changed by someone else".into(),
            RecordError::NotFound => "no such record".into(),
            RecordError::RateLimited { retry_after_secs } => {
                format!("too many writes; try again in {retry_after_secs} s")
            }
            RecordError::Refused { reason } => reason.clone(),
        }
    }
}

/// Every error code a reply can carry, as the contract lists them.
pub const ERROR_CODES: &[&str] = &[
    "unavailable",
    "conflict",
    "not_found",
    "bad_request",
    "rate_limited",
    "too_large",
    "limit",
    "refused",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Create,
    Update,
    Delete,
}

impl Change {
    pub fn name(self) -> &'static str {
        match self {
            Change::Create => "create",
            Change::Update => "update",
            Change::Delete => "delete",
        }
    }
}

/// A change by another participant, or by the same person from another
/// place. For a delete, `record` is its last state.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordPush {
    pub change: Change,
    pub record: Record,
    /// Who made the change.
    pub by: Who,
}

/// Is `name` a record type an app may declare: `[a-z][a-z0-9_-]{0,31}`?
pub fn valid_type_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= limits::TYPE_NAME_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

/// The title bzz gives a record for other clients: `content.title` when it
/// is a string with something in it, cut to the relay's 512 bytes, and the
/// type's name otherwise. It is also what `match` of a list looks at.
pub fn title_of(record_type: &str, content: &Map<String, Value>) -> String {
    match content.get("title") {
        Some(Value::String(title)) if !title.trim().is_empty() => {
            let mut end = title.len().min(limits::TITLE_BYTES);
            while !title.is_char_boundary(end) {
                end -= 1;
            }
            title[..end].to_string()
        }
        _ => record_type.to_string(),
    }
}

/// Does `record` pass the `match` of a list: its title contains `matching`,
/// ignoring case?
pub fn matches(record: &Record, matching: Option<&str>) -> bool {
    match matching {
        None | Some("") => true,
        Some(wanted) => title_of(&record.record_type, &record.content)
            .to_lowercase()
            .contains(&wanted.to_lowercase()),
    }
}

/// The size of `content` as the JSON text that is sent.
pub fn content_bytes(content: &Map<String, Value>) -> usize {
    serde_json::to_string(content).map_or(usize::MAX, |text| text.len())
}

/// What a script must be told after the set of records was fetched again:
/// **only the real differences**, as pushes. A record in `after` and not in
/// `before` was created; one in both with another revision was updated; one
/// only in `before` was deleted, and is pushed with its last known state. A
/// record that is the same in both gives nothing: a resync is not a change.
///
/// Creations and updates come in the order of `after`, then deletions in
/// the order of `before`.
pub fn differences(before: &[Record], after: &[Record]) -> Vec<RecordPush> {
    let old: HashMap<&str, &Record> = before.iter().map(|r| (r.id.as_str(), r)).collect();
    let new: HashMap<&str, &Record> = after.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut pushes = Vec::new();
    for record in after {
        let change = match old.get(record.id.as_str()) {
            None => Change::Create,
            Some(was) if was.rev != record.rev => Change::Update,
            Some(_) => continue,
        };
        pushes.push(RecordPush {
            change,
            record: record.clone(),
            by: record.by.clone(),
        });
    }
    for record in before {
        if !new.contains_key(record.id.as_str()) {
            pushes.push(RecordPush {
                change: Change::Delete,
                record: record.clone(),
                // Who deleted it is not in either set.
                by: record.by.clone(),
            });
        }
    }
    pushes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, rev: &str, title: &str) -> Record {
        let Value::Object(content) = json!({ "title": title }) else {
            unreachable!()
        };
        Record {
            id: id.into(),
            record_type: "card".into(),
            rev: rev.into(),
            content,
            by: Who::new("aa", "ana"),
        }
    }

    #[test]
    fn a_type_name_follows_the_rule() {
        for good in ["card", "a", "vote-2", "day_off", &"a".repeat(32)] {
            assert!(valid_type_name(good), "{good}");
        }
        for bad in [
            "",
            "Card",
            "2cards",
            "-card",
            "bzz.app.board.card",
            "card ",
            "tarjeta\u{f1}",
            &"a".repeat(33),
        ] {
            assert!(!valid_type_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_title_is_the_content_title_or_the_type() {
        let Value::Object(with) = json!({ "title": "Alpha" }) else {
            unreachable!()
        };
        let Value::Object(without) = json!({ "title": "  ", "n": 1 }) else {
            unreachable!()
        };
        let Value::Object(long) = json!({ "title": "\u{e9}".repeat(400) }) else {
            unreachable!()
        };
        assert_eq!(title_of("card", &with), "Alpha");
        assert_eq!(title_of("card", &without), "card");
        // 400 characters of two bytes: cut at 512 bytes, on a character.
        assert_eq!(title_of("card", &long), "\u{e9}".repeat(256));
        assert!(matches(&record("1", "a", "Write Tests"), Some("TEST")));
        assert!(!matches(&record("1", "a", "Write Tests"), Some("ship")));
        assert!(matches(&record("1", "a", "Write Tests"), None));
    }

    #[test]
    fn a_resync_gives_only_the_real_differences() {
        let before = vec![
            record("1", "a", "Alpha"),
            record("2", "a", "Bravo"),
            record("3", "a", "Charlie"),
        ];
        // The same set, in another order: nothing to tell.
        let shuffled = vec![before[2].clone(), before[0].clone(), before[1].clone()];
        assert_eq!(differences(&before, &shuffled), Vec::new());

        let after = vec![
            record("4", "a", "Delta"),
            record("2", "b", "Bravo v2"),
            record("1", "a", "Alpha"),
        ];
        let pushes = differences(&before, &after);
        let seen: Vec<(&str, &str)> = pushes
            .iter()
            .map(|p| (p.change.name(), p.record.id.as_str()))
            .collect();
        assert_eq!(seen, [("create", "4"), ("update", "2"), ("delete", "3")]);
        // A deletion carries the record's last state.
        assert_eq!(pushes[2].record, before[2]);
    }
}
