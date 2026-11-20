//! The boundary between an app and its records: requests go out, replies
//! and pushes come in. Plain types; nothing here knows a relay.
//!
//! The runtime turns a script's `data.*` call into a [`RecordRequest`] and
//! hands it to whoever owns the runtime: an app session
//! (`apps::session`). The session answers every request with exactly one
//! [`RecordReply`], and sends a [`RecordPush`] for each change another
//! participant made. Behind the session sit the relay's records service
//! (`service::records`) or the store in memory of [`super::memory`].
//!
//! What the two sides owe each other:
//!
//! - **Types are the app's short names** (`card`). Whoever serves a request
//!   makes the long form of them and checks the fence again (rule 17).
//! - **A revision is an opaque string**, handed back unchanged.
//! - **A list has no order** a script may rely on.
//! - **The requester's own writes are not pushed** back to it.
//! - **A resync is not a change**: when the set had to be fetched again,
//!   the session sends only the real differences
//!   (`protocol::record::difference`).

use serde_json::{Map, Value};

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
/// The rule is the records service's own.
pub fn valid_type_name(name: &str) -> bool {
    crate::protocol::record::valid_name(name)
}

/// The size of `content` as the JSON text that is sent.
pub fn content_bytes(content: &Map<String, Value>) -> usize {
    serde_json::to_string(content).map_or(usize::MAX, |text| text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
