//! An app's records (v0.16): NIP-AR artifacts of one channel, of the types
//! `bzz.app.<app>.<record>`.
//!
//! Everything here is a pure function of events and values: the names, the
//! fence of rule 17, the mapping between a record and its revisions, the
//! title, the checks made before a frame is sent, the reading of a refusal
//! and the difference between two sets. `service::records` does the talking.
//! `docs/app-records.md` is the interface in words.

use std::collections::BTreeMap;

use nostr::{Event, EventBuilder, Kind, Tag};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    protocol::{
        artifact::{
            APP_RECORD_TYPE_PREFIX, APP_RECORDS_SUBSCRIPTION_PREFIX, Artifact, KIND_ARTIFACT,
            display,
        },
        envelope,
    },
    realtime::admission,
};

/// An app's name and a record's name are each `[a-z][a-z0-9_-]{0,31}`.
pub const MAX_NAME_BYTES: usize = 32;
/// The record types one app may declare.
pub const MAX_TYPES: usize = 16;
/// Live records of one app in one channel, all its types together. The relay
/// ends a sync at 4096 heads and tells nobody who writes; bzz stops a create
/// here, long before, and says so.
pub const MAX_LIVE_RECORDS: usize = 1000;
/// The content bzz sends for one record, in bytes of JSON. The relay takes
/// 262,144; a record is a card or a vote, not a file.
pub const MAX_CONTENT_BYTES: usize = 65_536;
/// The relay's limit for one WebSocket message, after JSON escaping. Past it
/// the relay drops the connection without an answer.
pub const MAX_FRAME_BYTES: usize = 524_288;
/// An artifact's title, in bytes.
pub const MAX_TITLE_BYTES: usize = 512;
/// Envelope tag names, as in `protocol::artifact`.
const ENVELOPE: [&str; 8] = ["ar", "d", "h", "type", "title", "op", "root", "prev"];

/// One record as an app sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: Uuid,
    /// The app's own short name of the type (`card`), never the long one.
    pub record_type: String,
    /// Opaque: pass it back unchanged to revise or delete. It is the event
    /// id of the head.
    pub revision: String,
    /// The app's own fields.
    pub content: Map<String, Value>,
    /// Who signed this revision (public key, hex). Any writer of the channel
    /// may revise any record, so this is the last writer, not the creator.
    pub author: String,
    /// The signer's clock at this revision, in seconds. It is not an order.
    pub signed_at: u64,
}

/// A change to the set an app holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// Created, revised, restored or moved into the channel.
    Upsert(Record),
    /// Deleted, moved away, or no longer in the set. `record` is its last
    /// state; `by` is who signed the removal, when a revision says so.
    Removed { record: Record, by: Option<String> },
}

impl Change {
    pub const fn record(&self) -> &Record {
        match self {
            Self::Upsert(record) | Self::Removed { record, .. } => record,
        }
    }
}

/// Why the relay, or bzz for it, will not take a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The channel is archived. Records stay readable.
    Archived,
    /// The identity is a guest of the channel.
    Guest,
    /// The identity is not a member of the channel.
    NotMember,
    /// The identity cannot read the channel's records at all.
    Unreadable,
    /// The signer will not sign a record.
    Signer,
    /// Anything else; the reason is the relay's own, cleaned.
    Other,
}

/// What was too large.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Oversize {
    /// The record's content as JSON, against `MAX_CONTENT_BYTES`.
    Content,
    /// The whole escaped frame, against `MAX_FRAME_BYTES`.
    Frame,
}

/// Every way a request for records can fail. `code` is the name the contract
/// gives a script.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    /// The record was revised since the revision the caller saw. `current`
    /// is the record as it is now; what to do with it is the app's rule.
    Conflict {
        current: Box<Record>,
    },
    /// The identity's quota at the relay is used up, or bzz could not send
    /// in time. Nothing was stored; ask again after `seconds`.
    RateLimited {
        seconds: u64,
    },
    TooLarge {
        what: Oversize,
        bytes: usize,
        limit: usize,
    },
    /// The app already holds `limit` live records in this channel.
    Limit {
        limit: usize,
    },
    Refused {
        refusal: Refusal,
        reason: String,
    },
    /// No such record of this app in this channel: never there, deleted, or
    /// moved away.
    NotFound,
    /// No answer: offline, locked, or the relay did not acknowledge. The
    /// write may or may not have been stored.
    Unavailable {
        reason: String,
    },
    /// Rule 17: a type the app did not declare, or another channel.
    Fenced {
        reason: String,
    },
    /// The request itself is wrong: a bad name, content that is not an
    /// object.
    Invalid {
        reason: String,
    },
}

impl RecordError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Conflict { .. } => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::TooLarge { .. } => "too_large",
            Self::Limit { .. } => "limit",
            Self::Refused { .. } => "refused",
            Self::NotFound => "not_found",
            Self::Unavailable { .. } => "unavailable",
            Self::Fenced { .. } => "fenced",
            Self::Invalid { .. } => "bad_request",
        }
    }

    fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid {
            reason: reason.into(),
        }
    }

    fn fenced(reason: impl Into<String>) -> Self {
        Self::Fenced {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { .. } => write!(formatter, "the record changed meanwhile"),
            Self::RateLimited { seconds } => {
                write!(formatter, "too many writes; wait {seconds} s")
            }
            Self::TooLarge { what, bytes, limit } => write!(
                formatter,
                "the record's {} is {bytes} bytes; the limit is {limit}",
                match what {
                    Oversize::Content => "content",
                    Oversize::Frame => "frame",
                }
            ),
            Self::Limit { limit } => {
                write!(formatter, "this app already holds {limit} records here")
            }
            Self::NotFound => write!(formatter, "no such record"),
            Self::Refused { reason, .. }
            | Self::Unavailable { reason }
            | Self::Fenced { reason }
            | Self::Invalid { reason } => write!(formatter, "{reason}"),
        }
    }
}

/// `[a-z][a-z0-9_-]{0,31}`: an app's name, and a record type's short name.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

/// The fence of rule 17: one app, in one channel, with the record types it
/// declared. Every type an app can name, every head it is shown and every
/// revision bzz signs for it passes through here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fence {
    app: String,
    channel: Uuid,
    /// `(short, long)`, in the order declared.
    types: Vec<(String, String)>,
}

impl Fence {
    pub fn new<S: AsRef<str>>(app: &str, channel: Uuid, types: &[S]) -> Result<Self, RecordError> {
        if !valid_name(app) {
            return Err(RecordError::invalid(format!(
                "an app's name is a lowercase letter followed by up to {} of a-z, 0-9, _ or -",
                MAX_NAME_BYTES - 1
            )));
        }
        if channel.is_nil() {
            return Err(RecordError::invalid("an app is opened in a channel"));
        }
        if types.is_empty() || types.len() > MAX_TYPES {
            return Err(RecordError::invalid(format!(
                "an app declares 1 to {MAX_TYPES} record types"
            )));
        }
        let mut declared: Vec<(String, String)> = Vec::with_capacity(types.len());
        for short in types {
            let short = short.as_ref();
            if !valid_name(short) {
                return Err(RecordError::invalid(format!(
                    "a record type's name is a lowercase letter followed by up to {} of a-z, 0-9, _ or -",
                    MAX_NAME_BYTES - 1
                )));
            }
            if declared.iter().any(|(known, _)| known == short) {
                return Err(RecordError::invalid(format!(
                    "the record type {short} is declared twice"
                )));
            }
            declared.push((
                short.to_owned(),
                format!("{APP_RECORD_TYPE_PREFIX}{app}.{short}"),
            ));
        }
        Ok(Self {
            app: app.to_owned(),
            channel,
            types: declared,
        })
    }

    pub fn app(&self) -> &str {
        &self.app
    }

    pub const fn channel(&self) -> Uuid {
        self.channel
    }

    /// The declared short names, in the order declared.
    pub fn short_types(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|(short, _)| short.as_str())
    }

    /// The artifact types this app owns: `bzz.app.<app>.<record>`.
    pub fn long_types(&self) -> Vec<String> {
        self.types.iter().map(|(_, long)| long.clone()).collect()
    }

    /// The artifact type of a declared short name; `Fenced` for any other.
    pub fn long_type(&self, short: &str) -> Result<&str, RecordError> {
        self.types
            .iter()
            .find(|(known, _)| known == short)
            .map(|(_, long)| long.as_str())
            .ok_or_else(|| {
                RecordError::fenced(format!(
                    "the app {} did not declare the record type {}",
                    self.app,
                    display(short, 40)
                ))
            })
    }

    fn short_type(&self, long: &str) -> Option<&str> {
        self.types
            .iter()
            .find(|(_, known)| known == long)
            .map(|(short, _)| short.as_str())
    }

    /// Whether `head` is inside the fence: homed in this channel, and of a
    /// declared type. A deletion and a head moved elsewhere are judged the
    /// same way, by what they say.
    pub fn admits(&self, head: &Artifact) -> bool {
        head.home == self.channel && self.short_type(&head.artifact_type).is_some()
    }

    /// The one sync of this app: its channel and the exact list of its
    /// types. The relay has no prefix filter.
    pub fn sync_filter(&self) -> Value {
        json!({
            "artifact": "current",
            "#h": [self.channel.to_string()],
            "#type": self.long_types(),
        })
    }

    /// The subscription of that sync. Stable for an app in a channel, and
    /// short enough for any relay (a subscription id is at most 64 bytes).
    pub fn subscription_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.channel.as_bytes());
        hasher.update([0]);
        hasher.update(self.app.as_bytes());
        format!(
            "{APP_RECORDS_SUBSCRIPTION_PREFIX}{}",
            &hex::encode(hasher.finalize())[..24]
        )
    }

    /// The record a live head stands for. `None` for a deletion, for a head
    /// outside the fence, and for content that is not a JSON object: such an
    /// artifact was not written by this app's contract.
    pub fn record(&self, head: &Artifact) -> Option<Record> {
        if head.is_deleted() || head.home != self.channel {
            return None;
        }
        let short = self.short_type(&head.artifact_type)?;
        let Ok(Value::Object(content)) = serde_json::from_str::<Value>(&head.event.content) else {
            return None;
        };
        Some(Record {
            id: head.id,
            record_type: short.to_owned(),
            revision: head.head.clone(),
            content,
            author: head.author.clone(),
            signed_at: head.created_at,
        })
    }

    /// The first revision of a new record.
    pub fn create(
        &self,
        short: &str,
        id: Uuid,
        content: &Map<String, Value>,
    ) -> Result<EventBuilder, RecordError> {
        let long = self.long_type(short)?.to_owned();
        let json = serialize(content)?;
        let title = title(&self.app, short, content);
        let tags = vec![
            tag(&["ar", "1"])?,
            tag(&["d", &id.to_string()])?,
            tag(&["h", &self.channel.to_string()])?,
            tag(&["type", &long])?,
            tag(&["title", &title])?,
            tag(&["op", "create"])?,
        ];
        Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), json).tags(tags))
    }

    /// The next revision over `head`: a whole snapshot with the new content.
    /// Tags another client put on the record are carried forward as they are.
    pub fn update(
        &self,
        head: &Artifact,
        content: &Map<String, Value>,
    ) -> Result<EventBuilder, RecordError> {
        let short = self.revisable(head)?;
        let json = serialize(content)?;
        let title = title(&self.app, short, content);
        let mut tags = vec![
            tag(&["ar", "1"])?,
            tag(&["d", &head.id.to_string()])?,
            tag(&["h", &self.channel.to_string()])?,
            tag(&["type", &head.artifact_type])?,
            tag(&["title", &title])?,
            tag(&["op", "update"])?,
        ];
        if let Some(root) = &head.root {
            tags.push(tag(&["root", root])?);
        }
        tags.push(tag(&["prev", &head.head])?);
        tags.extend(
            head.event
                .tags
                .iter()
                .filter(|tag| {
                    tag.as_slice()
                        .first()
                        .is_some_and(|name| !ENVELOPE.contains(&name.as_str()) && name != "auth")
                })
                .cloned(),
        );
        Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), json).tags(tags))
    }

    /// The deletion of the record whose head is `head`: envelope tags only,
    /// no title, empty content.
    pub fn delete(&self, head: &Artifact) -> Result<EventBuilder, RecordError> {
        self.revisable(head)?;
        let mut tags = vec![
            tag(&["ar", "1"])?,
            tag(&["d", &head.id.to_string()])?,
            tag(&["h", &self.channel.to_string()])?,
            tag(&["type", &head.artifact_type])?,
            tag(&["op", "delete"])?,
        ];
        if let Some(root) = &head.root {
            tags.push(tag(&["root", root])?);
        }
        tags.push(tag(&["prev", &head.head])?);
        Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), "").tags(tags))
    }

    fn revisable(&self, head: &Artifact) -> Result<&str, RecordError> {
        if head.home != self.channel {
            return Err(RecordError::fenced("the record belongs to another channel"));
        }
        let short = self.short_type(&head.artifact_type).ok_or_else(|| {
            RecordError::fenced(format!(
                "the app {} did not declare the type of this record",
                self.app
            ))
        })?;
        if head.is_deleted() {
            return Err(RecordError::NotFound);
        }
        Ok(short)
    }

    /// The last look at a signed revision before it is sent, because a frame
    /// the relay refuses costs the same quota as one it stores: the relay's
    /// own envelope validator, the fence once more on what was actually
    /// signed, the content and the escaped frame.
    pub fn check(&self, event: &Event) -> Result<(), RecordError> {
        let envelope = buzz_core::artifact::validate(event)
            .map_err(|error| RecordError::invalid(format!("the relay would refuse it: {error}")))?;
        if envelope.home != self.channel {
            return Err(RecordError::fenced("the revision names another channel"));
        }
        if self.short_type(&envelope.artifact_type).is_none() {
            return Err(RecordError::fenced(format!(
                "the app {} did not declare the type of this revision",
                self.app
            )));
        }
        if event.content.len() > MAX_CONTENT_BYTES {
            return Err(RecordError::TooLarge {
                what: Oversize::Content,
                bytes: event.content.len(),
                limit: MAX_CONTENT_BYTES,
            });
        }
        let frame = envelope::publish(event).len();
        if frame > MAX_FRAME_BYTES {
            return Err(RecordError::TooLarge {
                what: Oversize::Frame,
                bytes: frame,
                limit: MAX_FRAME_BYTES,
            });
        }
        Ok(())
    }
}

fn tag(parts: &[&str]) -> Result<Tag, RecordError> {
    Tag::parse(parts.iter().copied()).map_err(|error| RecordError::invalid(error.to_string()))
}

/// A record's content as the JSON bzz sends, or `TooLarge`.
pub fn serialize(content: &Map<String, Value>) -> Result<String, RecordError> {
    let json =
        serde_json::to_string(content).map_err(|error| RecordError::invalid(error.to_string()))?;
    if json.len() > MAX_CONTENT_BYTES {
        return Err(RecordError::TooLarge {
            what: Oversize::Content,
            bytes: json.len(),
            limit: MAX_CONTENT_BYTES,
        });
    }
    Ok(json)
}

/// **The title rule.** An artifact needs a title of 1 to 512 bytes; a record
/// has only its fields. The title is the record's field `title` when that is
/// a string with something to show: control characters become spaces, the
/// ends are trimmed, and it is cut at 512 bytes, on a character. Otherwise it
/// is `<app> <record>`, such as `board card`. The title is for other clients'
/// lists; an app never reads it back.
pub fn title(app: &str, short: &str, content: &Map<String, Value>) -> String {
    content
        .get("title")
        .and_then(Value::as_str)
        .map(clean_title)
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| format!("{app} {short}"))
}

fn clean_title(raw: &str) -> String {
    let clean = raw
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let clean = clean.trim();
    let mut end = clean.len().min(MAX_TITLE_BYTES);
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean[..end].trim_end().to_owned()
}

/// What the relay's refusal of a write means.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    RateLimited {
        seconds: u64,
    },
    /// `conflict: artifact head changed`: someone revised the record first.
    /// The refusal does not carry the head.
    HeadChanged,
    /// The record is not there to revise: deleted, moved away, never made.
    Gone,
    TooLarge,
    Refused {
        refusal: Refusal,
        reason: String,
    },
}

/// Reads a relay's refusal. Only a stale head is a reason to look at the
/// record again: `conflict: artifact identity is taken` and `conflict:
/// artifact head unavailable` also begin with `conflict:`, and a client that
/// answered them by reading the head and trying again would loop.
pub fn verdict(message: &str) -> Verdict {
    if let Some(wait) = admission::rate_limit_retry_after(message) {
        return Verdict::RateLimited {
            seconds: wait.as_secs().max(1),
        };
    }
    let lower = message.trim().to_ascii_lowercase();
    let refused = |refusal, reason: &str| Verdict::Refused {
        refusal,
        reason: reason.to_owned(),
    };
    if lower.starts_with("conflict: artifact head changed") {
        Verdict::HeadChanged
    } else if lower.starts_with("conflict: artifact head unavailable")
        || lower.contains("deleted artifacts require restore")
    {
        Verdict::Gone
    } else if lower.contains("channel is archived") {
        refused(
            Refusal::Archived,
            "this channel is archived; its records can be read, not changed",
        )
    } else if lower.starts_with("restricted: only members can post") {
        refused(
            Refusal::Guest,
            "you are a guest of this channel; guests cannot change its records",
        )
    } else if lower.starts_with("restricted: not a channel member") {
        refused(Refusal::NotMember, "you are not a member of this channel")
    } else if lower.contains("content exceeds maximum size") {
        Verdict::TooLarge
    } else {
        Verdict::Refused {
            refusal: Refusal::Other,
            reason: display(message.trim(), 160),
        }
    }
}

/// The real differences between the set an app holds and a fresh complete
/// one, in the order of the records' ids: what a resync shows an app. A
/// record with the same revision in both is not a change, so a resync of an
/// unchanged set is no change at all.
pub fn difference(held: &BTreeMap<Uuid, Record>, fresh: &BTreeMap<Uuid, Record>) -> Vec<Change> {
    let mut changes = Vec::new();
    for (id, record) in held {
        if !fresh.contains_key(id) {
            changes.push(Change::Removed {
                record: record.clone(),
                by: None,
            });
        }
    }
    for (id, record) in fresh {
        if held
            .get(id)
            .is_none_or(|known| known.revision != record.revision)
        {
            changes.push(Change::Upsert(record.clone()));
        }
    }
    changes.sort_by_key(|change| change.record().id);
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::artifact::{Op, parse};
    use nostr::Keys;

    fn fence() -> (Fence, Uuid) {
        let channel = Uuid::new_v4();
        (
            Fence::new("board", channel, &["card", "column"]).unwrap(),
            channel,
        )
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn signed(builder: EventBuilder, keys: &Keys) -> Artifact {
        parse(&builder.sign_with_keys(keys).unwrap()).unwrap()
    }

    #[test]
    fn names_follow_the_rule_of_the_contract() {
        for good in [
            "a",
            "card",
            "task_board",
            "card-v2",
            "b0ard",
            &"a".repeat(32),
        ] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "",
            "Card",
            "2048",
            "_x",
            "-x",
            "a.b",
            "a b",
            "ó",
            "a:b",
            "a/b",
            "a*",
            &"a".repeat(33),
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        let channel = Uuid::new_v4();
        for (app, types) in [
            ("Board", vec!["card"]),
            ("board", vec![]),
            ("board", vec!["Card"]),
            ("board", vec!["card", "card"]),
            ("board.x", vec!["card"]),
        ] {
            assert!(
                matches!(
                    Fence::new(app, channel, &types),
                    Err(RecordError::Invalid { .. })
                ),
                "{app} {types:?}"
            );
        }
        let many = (0..=MAX_TYPES)
            .map(|index| format!("t{index}"))
            .collect::<Vec<_>>();
        assert!(Fence::new("board", channel, &many).is_err());
        assert!(Fence::new("board", Uuid::nil(), &["card"]).is_err());
        // The longest names give a type the relay's own rule accepts.
        let long = "z".repeat(MAX_NAME_BYTES);
        let fence = Fence::new(&long, channel, std::slice::from_ref(&long)).unwrap();
        let widest = &fence.long_types()[0];
        assert_eq!(widest.len(), 8 + 32 + 1 + 32);
        let event = fence
            .create(&long, Uuid::new_v4(), &Map::new())
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(buzz_core::artifact::validate(&event).is_ok());
    }

    #[test]
    fn a_fence_gives_only_its_types_in_its_channel() {
        let (fence, channel) = fence();
        assert_eq!(
            fence.long_types(),
            ["bzz.app.board.card", "bzz.app.board.column"]
        );
        assert_eq!(fence.long_type("card").unwrap(), "bzz.app.board.card");
        assert_eq!(
            fence.sync_filter(),
            json!({"artifact":"current","#h":[channel.to_string()],
                   "#type":["bzz.app.board.card","bzz.app.board.column"]})
        );
        // A type that was not declared, the long form, another app's type.
        for outside in ["vote", "bzz.app.board.card", "buzz.task", "", "card "] {
            let error = fence.long_type(outside).unwrap_err();
            assert_eq!(error.code(), "fenced", "{outside}");
            assert!(
                matches!(
                    fence.create(outside, Uuid::new_v4(), &Map::new()),
                    Err(RecordError::Fenced { .. })
                ),
                "{outside}"
            );
        }
        // The subscription is the app's in that channel, and fits a relay.
        let id = fence.subscription_id();
        assert!(id.starts_with(APP_RECORDS_SUBSCRIPTION_PREFIX) && id.len() <= 64);
        assert_eq!(id, fence.subscription_id());
        assert_ne!(
            id,
            Fence::new("board", Uuid::new_v4(), &["card"])
                .unwrap()
                .subscription_id()
        );
        assert_ne!(
            id,
            Fence::new("poll", channel, &["card"])
                .unwrap()
                .subscription_id()
        );
    }

    #[test]
    fn a_record_and_its_revisions_map_both_ways() {
        let (fence, channel) = fence();
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let content = object(
            json!({"title":"Write the brief","column":"todo","order":1024.0,"n":3,"gone":null}),
        );
        let created = signed(fence.create("card", id, &content).unwrap(), &keys);
        // The shape the pinned relay accepted in M0 question 3.
        let tags = created
            .event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(
            tags,
            [
                vec!["ar".to_owned(), "1".into()],
                vec!["d".into(), id.to_string()],
                vec!["h".into(), channel.to_string()],
                vec!["type".into(), "bzz.app.board.card".into()],
                vec!["title".into(), "Write the brief".into()],
                vec!["op".into(), "create".into()],
            ]
        );
        assert_eq!(created.event.kind.as_u16(), KIND_ARTIFACT);
        fence.check(&created.event).unwrap();
        let record = fence.record(&created).unwrap();
        assert_eq!(
            (record.id, record.record_type.as_str(), &record.revision),
            (id, "card", &created.head)
        );
        assert_eq!(record.content, content, "integers, floats and null survive");
        assert_eq!(record.content["n"], json!(3));
        assert_eq!(record.content["order"].to_string(), "1024.0");
        assert_eq!(record.author, keys.public_key().to_hex());

        // A revision is a whole snapshot over the head, by whoever signs it.
        let other = Keys::generate();
        let revised = object(json!({"column":"doing"}));
        let updated = signed(fence.update(&created, &revised).unwrap(), &other);
        assert_eq!(updated.op, Op::Update);
        assert_eq!(updated.tag("prev"), Some(created.head.clone()));
        assert_eq!(updated.title.as_deref(), Some("board card"));
        fence.check(&updated.event).unwrap();
        let record = fence.record(&updated).unwrap();
        assert_eq!(record.content, revised);
        assert_eq!(record.author, other.public_key().to_hex());
        assert_ne!(record.revision, created.head);

        // A deletion has only its envelope, and is no record.
        let deleted = signed(fence.delete(&updated).unwrap(), &keys);
        assert!(deleted.is_deleted() && deleted.event.content.is_empty());
        assert_eq!(deleted.tag("prev"), Some(updated.head.clone()));
        assert!(deleted.client_tags().is_empty() && deleted.title.is_none());
        fence.check(&deleted.event).unwrap();
        assert!(fence.record(&deleted).is_none());
        assert_eq!(
            fence.update(&deleted, &revised).unwrap_err(),
            RecordError::NotFound
        );
        assert_eq!(fence.delete(&deleted).unwrap_err(), RecordError::NotFound);
    }

    fn foreign(
        keys: &Keys,
        channel: Uuid,
        artifact_type: &str,
        content: &str,
        extra: &[&[&str]],
    ) -> Artifact {
        let mut tags = vec![
            tag(&["ar", "1"]).unwrap(),
            tag(&["d", &Uuid::new_v4().to_string()]).unwrap(),
            tag(&["h", &channel.to_string()]).unwrap(),
            tag(&["type", artifact_type]).unwrap(),
            tag(&["title", "Theirs"]).unwrap(),
            tag(&["op", "create"]).unwrap(),
        ];
        tags.extend(extra.iter().map(|parts| tag(parts).unwrap()));
        signed(
            EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content).tags(tags),
            keys,
        )
    }

    #[test]
    fn what_another_client_wrote_is_carried_or_left_alone() {
        let (fence, channel) = fence();
        let keys = Keys::generate();
        let root = "c".repeat(64);
        let theirs = foreign(
            &keys,
            channel,
            "bzz.app.board.card",
            "{\"title\":\"Theirs\"}",
            &[&["root", &root], &["assignee", "p1"], &["k", "v", "extra"]],
        );
        let updated = signed(
            fence
                .update(&theirs, &object(json!({"title":"Ours"})))
                .unwrap(),
            &keys,
        );
        assert_eq!(updated.client_tags(), theirs.client_tags());
        assert_eq!(updated.root.as_deref(), Some(root.as_str()));
        fence.check(&updated.event).unwrap();

        // Not an object: not a record of this contract.
        for content in ["", "not json", "[1,2]", "\"text\"", "3", "null"] {
            let head = foreign(&keys, channel, "bzz.app.board.card", content, &[]);
            assert!(fence.admits(&head));
            assert!(fence.record(&head).is_none(), "{content}");
        }
    }

    #[test]
    fn the_fence_holds_for_heads_and_for_signed_revisions() {
        let (fence, channel) = fence();
        let keys = Keys::generate();
        let elsewhere = Uuid::new_v4();
        let cases = [
            foreign(&keys, channel, "bzz.app.poll.vote", "{}", &[]),
            foreign(&keys, channel, "bzz.app.board", "{}", &[]),
            foreign(&keys, channel, "bzz.app.board.card.x", "{}", &[]),
            foreign(&keys, channel, "buzz.task", "{}", &[]),
            foreign(&keys, elsewhere, "bzz.app.board.card", "{}", &[]),
        ];
        for head in &cases {
            assert!(!fence.admits(head), "{}", head.artifact_type);
            assert!(fence.record(head).is_none());
            for error in [
                fence.update(head, &Map::new()).unwrap_err(),
                fence.delete(head).unwrap_err(),
                fence.check(&head.event).unwrap_err(),
            ] {
                assert_eq!(error.code(), "fenced", "{}", head.artifact_type);
            }
        }
        let inside = foreign(&keys, channel, "bzz.app.board.column", "{}", &[]);
        assert!(fence.admits(&inside) && fence.record(&inside).is_some());
    }

    #[test]
    fn the_title_is_the_title_field_or_the_names() {
        let title_of = |value: Value| title("board", "card", &object(value));
        assert_eq!(
            title_of(json!({"title":"Write the brief"})),
            "Write the brief"
        );
        assert_eq!(title_of(json!({"title":"  two\nlines\t "})), "two lines");
        for none in [
            json!({}),
            json!({"title":""}),
            json!({"title":"  \n "}),
            json!({"title":7}),
            json!({"title":null}),
            json!({"title":["a"]}),
            json!({"name":"x"}),
        ] {
            assert_eq!(title_of(none.clone()), "board card", "{none}");
        }
        // Cut at 512 bytes on a character, never blank, and the relay's own
        // validator takes every one of them.
        let (fence, _) = fence();
        let keys = Keys::generate();
        for raw in [
            "a".repeat(600),
            "ñ".repeat(400),
            format!("{}é", "a".repeat(511)),
            format!("{} é", "a".repeat(510)),
            "日本語".repeat(200),
            "\u{1b}[31mred".into(),
        ] {
            let content = object(json!({"title": raw}));
            let made = title("board", "card", &content);
            assert!(
                !made.trim().is_empty() && made.len() <= MAX_TITLE_BYTES,
                "{made}"
            );
            assert!(!made.chars().any(char::is_control));
            let event = fence
                .create("card", Uuid::new_v4(), &content)
                .unwrap()
                .sign_with_keys(&keys)
                .unwrap();
            fence.check(&event).unwrap();
        }
        assert_eq!(title_of(json!({"title": "a".repeat(600)})).len(), 512);
        assert_eq!(title_of(json!({"title": "ñ".repeat(400)})).len(), 512);
        assert_eq!(
            title_of(json!({"title": format!("{}é", "a".repeat(511))})).len(),
            511
        );
    }

    #[test]
    fn size_is_checked_before_anything_is_sent() {
        let (fence, _) = fence();
        let keys = Keys::generate();
        let with = |bytes: usize| object(json!({"t": "x".repeat(bytes)}));
        // `{"t":"…"}` is 8 bytes around the text.
        let fits = with(MAX_CONTENT_BYTES - 8);
        assert_eq!(serialize(&fits).unwrap().len(), MAX_CONTENT_BYTES);
        let event = fence
            .create("card", Uuid::new_v4(), &fits)
            .unwrap()
            .sign_with_keys(&keys)
            .unwrap();
        fence.check(&event).unwrap();
        let over = with(MAX_CONTENT_BYTES - 7);
        for error in [
            serialize(&over).unwrap_err(),
            fence.create("card", Uuid::new_v4(), &over).unwrap_err(),
        ] {
            assert_eq!(
                error,
                RecordError::TooLarge {
                    what: Oversize::Content,
                    bytes: MAX_CONTENT_BYTES + 1,
                    limit: MAX_CONTENT_BYTES
                }
            );
            assert_eq!(error.code(), "too_large");
        }
        // Escaping counts in the content: a quote is two bytes of JSON.
        let quotes = object(json!({"t": "\"".repeat(MAX_CONTENT_BYTES / 2)}));
        assert!(matches!(
            serialize(&quotes),
            Err(RecordError::TooLarge {
                what: Oversize::Content,
                ..
            })
        ));
        // And once more in the frame, where a control character is six
        // bytes. A revision that carries what another client left on the
        // record (content of control characters, sixteen long tags) passes
        // the relay's validator and bzz's content limit, and would be a
        // frame of more than 524,288 bytes: the relay answers that by
        // dropping the connection.
        let noise = "\u{1}".repeat(4000);
        let tags = (0..16)
            .map(|index| vec![format!("x{index}"), noise.clone()])
            .collect::<Vec<_>>();
        let tags = tags
            .iter()
            .map(|parts| parts.iter().map(String::as_str).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let tags = tags.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let head = foreign(
            &keys,
            fence.channel(),
            "bzz.app.board.card",
            &"\u{1}".repeat(MAX_CONTENT_BYTES),
            &tags,
        );
        assert!(buzz_core::artifact::validate(&head.event).is_ok());
        let frame = envelope::publish(&head.event).len();
        assert!(frame > MAX_FRAME_BYTES, "{frame}");
        assert_eq!(
            fence.check(&head.event).unwrap_err(),
            RecordError::TooLarge {
                what: Oversize::Frame,
                bytes: frame,
                limit: MAX_FRAME_BYTES
            }
        );
        // Content the relay itself would take (262,144 bytes) is over bzz's
        // own limit for a record.
        let head = foreign(
            &keys,
            fence.channel(),
            "bzz.app.board.card",
            &"x".repeat(262_144),
            &[],
        );
        assert!(matches!(
            fence.check(&head.event),
            Err(RecordError::TooLarge {
                what: Oversize::Content,
                ..
            })
        ));
    }

    #[test]
    fn a_refusal_is_read_for_what_it_means() {
        assert_eq!(
            verdict("rate-limited: quota exceeded; retry in 40s"),
            Verdict::RateLimited { seconds: 40 }
        );
        assert_eq!(
            verdict("rate-limited: retry in 44s"),
            Verdict::RateLimited { seconds: 44 }
        );
        assert_eq!(
            verdict("conflict: artifact head changed"),
            Verdict::HeadChanged
        );
        // The two other conflicts are not a stale head.
        assert_eq!(
            verdict("conflict: artifact head unavailable"),
            Verdict::Gone
        );
        assert!(matches!(
            verdict("conflict: artifact identity is taken"),
            Verdict::Refused {
                refusal: Refusal::Other,
                ..
            }
        ));
        assert_eq!(
            verdict("invalid: deleted artifacts require restore; live artifacts cannot restore"),
            Verdict::Gone
        );
        for (message, expected) in [
            ("invalid: channel is archived", Refusal::Archived),
            (
                "restricted: only members can post in this channel",
                Refusal::Guest,
            ),
            ("restricted: not a channel member", Refusal::NotMember),
            ("invalid: artifact type is immutable", Refusal::Other),
            ("blocked: nope", Refusal::Other),
            ("", Refusal::Other),
        ] {
            let Verdict::Refused { refusal, reason } = verdict(message) else {
                panic!("{message}")
            };
            assert_eq!(refusal, expected, "{message}");
            assert!(refusal != Refusal::Other || reason == message);
        }
        assert_eq!(
            verdict("invalid: content exceeds maximum size of 262144 bytes (got 262145)"),
            Verdict::TooLarge
        );
        // The relay's words are cleaned and bounded before anyone shows them.
        let Verdict::Refused { reason, .. } =
            verdict(&format!("invalid: \u{1b}[31m{}", "x".repeat(500)))
        else {
            panic!()
        };
        assert!(reason.chars().count() <= 160 && !reason.chars().any(char::is_control));
    }

    #[test]
    fn a_resync_shows_only_what_really_differs() {
        let (fence, _) = fence();
        let keys = Keys::generate();
        let make = |title: &str| {
            let head = signed(
                fence
                    .create("card", Uuid::new_v4(), &object(json!({"title": title})))
                    .unwrap(),
                &keys,
            );
            (head.clone(), fence.record(&head).unwrap())
        };
        let ((a_head, a), (_, b), (_, c)) = (make("a"), make("b"), make("c"));
        let set = |records: &[&Record]| {
            records
                .iter()
                .map(|record| (record.id, (*record).clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let held = set(&[&a, &b, &c]);
        // The same set again: no change at all, so no deletion and no creation.
        assert!(difference(&held, &held.clone()).is_empty());
        assert!(difference(&BTreeMap::new(), &BTreeMap::new()).is_empty());

        // `a` was revised, `b` is gone, `d` is new, `c` is as it was.
        let a2 = fence
            .record(&signed(
                fence
                    .update(&a_head, &object(json!({"title":"a2"})))
                    .unwrap(),
                &keys,
            ))
            .unwrap();
        let (_, d) = make("d");
        let fresh = set(&[&a2, &c, &d]);
        let mut expected = vec![
            Change::Upsert(a2.clone()),
            Change::Removed {
                record: b.clone(),
                by: None,
            },
            Change::Upsert(d.clone()),
        ];
        expected.sort_by_key(|change| change.record().id);
        assert_eq!(difference(&held, &fresh), expected);

        // From nothing, the whole set; to nothing, every record removed.
        assert_eq!(difference(&BTreeMap::new(), &held).len(), 3);
        assert!(
            difference(&held, &BTreeMap::new())
                .iter()
                .all(|change| matches!(change, Change::Removed { by: None, .. }))
        );
    }
}
