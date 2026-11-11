//! NIP-AR channel artifacts (kind 45010 revisions, kind 45011 move removals).
//!
//! The relay owns identity, access and the current head; bzz holds only the
//! current head per artifact. Content schemas are client-defined. The
//! `buzz.task` and `buzz.project` schemas are not published by Buzz, so bzz
//! never rewrites `content`: it edits only the envelope (title, home, delete,
//! restore) and carries every other tag and the content forward verbatim.

use nostr::{Event, EventBuilder, Kind, Tag};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::events::{first_tag, verify},
};

pub const KIND_ARTIFACT: u16 = 45_010;
pub const KIND_ARTIFACT_REMOVAL: u16 = 45_011;
/// Envelope tag names; every other tag is client-defined and preserved.
const ENVELOPE: [&str; 8] = ["ar", "d", "h", "type", "title", "op", "root", "prev"];
const MAX_TITLE_BYTES: usize = 512;
/// Conflict reconciliation attempts before the user is told.
pub const MAX_RECONCILE: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Op {
    Create,
    Update,
    Move,
    Delete,
    Restore,
}

impl Op {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Move => "move",
            Self::Delete => "delete",
            Self::Restore => "restore",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "create" => Self::Create,
            "update" => Self::Update,
            "move" => Self::Move,
            "delete" => Self::Delete,
            "restore" => Self::Restore,
            _ => return None,
        })
    }
}

/// A verified current head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artifact {
    pub id: Uuid,
    pub home: Uuid,
    pub artifact_type: String,
    pub title: Option<String>,
    pub op: Op,
    pub root: Option<String>,
    pub head: String,
    pub author: String,
    pub created_at: u64,
    pub event: Event,
}

impl Artifact {
    pub fn is_deleted(&self) -> bool {
        self.op == Op::Delete
    }
    /// Client-defined `[name, value]` tags for display, in event order.
    pub fn client_tags(&self) -> Vec<(String, String)> {
        self.event
            .tags
            .iter()
            .filter_map(|tag| {
                let parts = tag.as_slice();
                let name = parts.first()?;
                (!ENVELOPE.contains(&name.as_str()) && name != "auth")
                    .then(|| (name.clone(), parts.get(1).cloned().unwrap_or_default()))
            })
            .collect()
    }
    pub fn tag(&self, name: &str) -> Option<String> {
        first_tag(&self.event, name)
    }
}

/// Verifies the signature and the envelope with the pinned Buzz validator.
pub fn parse(event: &Event) -> Result<Artifact> {
    verify(event)?;
    let envelope = buzz_core::artifact::validate(event)
        .map_err(|error| Error::Protocol(format!("artifact envelope: {error}")))?;
    let op = Op::parse(&first_tag(event, "op").unwrap_or_default())
        .ok_or_else(|| Error::Protocol("artifact envelope: invalid op".into()))?;
    let title = first_tag(event, "title");
    if op != Op::Delete
        && title
            .as_deref()
            .is_none_or(|title| title.trim().is_empty() || title.len() > MAX_TITLE_BYTES)
    {
        return Err(Error::Protocol("artifact envelope: invalid title".into()));
    }
    Ok(Artifact {
        id: envelope.id,
        home: envelope.home,
        artifact_type: envelope.artifact_type,
        title: title.filter(|_| op != Op::Delete),
        op,
        root: envelope.root.map(hex::encode),
        head: event.id.to_hex(),
        author: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        event: event.clone(),
    })
}

/// A relay-signed move removal: the artifact left `source`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removal {
    pub id: Uuid,
    pub source: Uuid,
}

pub fn parse_removal(event: &Event, relay: Option<&str>) -> Result<Removal> {
    verify(event)?;
    if event.kind.as_u16() != KIND_ARTIFACT_REMOVAL || relay != Some(&event.pubkey.to_hex()) {
        return Err(Error::Protocol(
            "artifact removal must be signed by the pinned relay".into(),
        ));
    }
    let uuid = |name| {
        first_tag(event, name)
            .and_then(|value| buzz_core::artifact::canonical_uuid(&value).ok())
            .ok_or_else(|| Error::Protocol(format!("artifact removal: invalid {name}")))
    };
    Ok(Removal {
        id: uuid("d")?,
        source: uuid("h")?,
    })
}

/// NIP-11 `artifacts` capabilities this client uses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Capabilities {
    pub sync: bool,
    pub head: bool,
    pub max_events: usize,
}

pub fn capabilities(info: &Value) -> Capabilities {
    let artifacts = &info["artifacts"];
    let modes = artifacts["modes"].as_array().cloned().unwrap_or_default();
    let has_mode = |mode: &str| modes.iter().any(|value| value.as_str() == Some(mode));
    let max_events = artifacts["sync"]["max_events"].as_u64().unwrap_or(0) as usize;
    Capabilities {
        sync: artifacts["sync"]["version"].as_u64() == Some(1)
            && max_events > 0
            && has_mode("current"),
        head: has_mode("head"),
        max_events,
    }
}

/// The sync subscription for one channel (one consistent cut, then live heads).
pub fn sync_filter(channel: Uuid) -> Value {
    json!({"artifact": "current", "#h": [channel.to_string()]})
}

/// Head lookup including deletions (`artifact: head` requires `#d`).
pub fn head_filter(id: Uuid) -> Value {
    json!({"artifact": "head", "#d": [id.to_string()]})
}

/// Current `buzz.task` artifacts assigned to `pubkey` across readable channels.
pub fn assigned_filter(pubkey: &str) -> Value {
    json!({"artifact": "current", "#type": ["buzz.task"], "#assignee": [pubkey], "limit": 200})
}

/// `set_digest` over a held set (NIP-AR Sync): sorted `<d>:<head>\n` lines.
pub fn set_digest<'a>(heads: impl IntoIterator<Item = &'a Artifact>) -> String {
    let mut lines = heads
        .into_iter()
        .map(|artifact| format!("{}:{}\n", artifact.id, artifact.head))
        .collect::<Vec<_>>();
    lines.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"nip-ar-set-v1\0");
    for line in lines {
        hasher.update(line.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// An envelope-level change bzz can make to any type without a content schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    Rename(String),
    Delete,
    /// Revive `snapshot` (the last live revision) over the deletion head.
    Restore,
    Move(Uuid),
}

/// What reconciling a change against a newer head requires.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reconcile {
    /// The new head already has the requested effect.
    Done,
    /// Rebuild the change on the new head.
    Retry,
    /// The change no longer applies; tell the user why.
    Impossible(&'static str),
}

pub fn reconcile(change: &Change, head: &Artifact) -> Reconcile {
    match change {
        Change::Rename(_) if head.is_deleted() => {
            Reconcile::Impossible("the artifact was deleted meanwhile")
        }
        Change::Rename(title) if head.title.as_deref() == Some(title.as_str()) => Reconcile::Done,
        Change::Rename(_) => Reconcile::Retry,
        Change::Delete if head.is_deleted() => Reconcile::Done,
        Change::Delete => Reconcile::Retry,
        Change::Restore if !head.is_deleted() => Reconcile::Done,
        Change::Restore => Reconcile::Retry,
        Change::Move(_) if head.is_deleted() => {
            Reconcile::Impossible("the artifact was deleted meanwhile")
        }
        Change::Move(destination) if head.home == *destination => Reconcile::Done,
        // The confirmed preview showed the old snapshot; a changed artifact
        // must be confirmed again before it is shared with a new audience.
        Change::Move(_) => Reconcile::Impossible(
            "the artifact changed after you confirmed the move; review it and move again",
        ),
    }
}

fn tag(parts: &[&str]) -> Result<Tag> {
    Tag::parse(parts.iter().copied()).map_err(|error| Error::Protocol(error.to_string()))
}

/// Builds the next revision over `head`. `snapshot` is required for `Restore`
/// and is the last live revision whose title, root, content and client tags
/// become current again.
pub fn build(
    change: &Change,
    head: &Artifact,
    snapshot: Option<&Artifact>,
) -> Result<EventBuilder> {
    let id = head.id.to_string();
    let (op, home, source) = match change {
        Change::Rename(_) => (Op::Update, head.home, head),
        Change::Delete => (Op::Delete, head.home, head),
        Change::Restore => (
            Op::Restore,
            head.home,
            snapshot.ok_or_else(|| {
                Error::Protocol("no live snapshot of this artifact to restore".into())
            })?,
        ),
        Change::Move(destination) => (Op::Move, *destination, head),
    };
    if (op == Op::Restore) != head.is_deleted() {
        return Err(Error::Protocol(match op {
            Op::Restore => "only a deleted artifact can be restored".into(),
            _ => "a deleted artifact can only be restored".into(),
        }));
    }
    let title = match change {
        Change::Rename(title) => {
            let title = title.trim();
            if title.is_empty() || title.len() > MAX_TITLE_BYTES {
                return Err(Error::Protocol(
                    "an artifact title must be 1 to 512 bytes".into(),
                ));
            }
            Some(title.to_owned())
        }
        Change::Delete => None,
        _ => source.title.clone(),
    };
    let home = home.to_string();
    let mut tags = vec![
        tag(&["ar", "1"])?,
        tag(&["d", &id])?,
        tag(&["h", &home])?,
        tag(&["type", &head.artifact_type])?,
    ];
    if let Some(title) = &title {
        tags.push(tag(&["title", title])?);
    }
    tags.push(tag(&["op", op.as_str()])?);
    // A move's root must belong to the destination; drop it. A deletion keeps
    // the current root; a restore takes the snapshot's.
    let root = match op {
        Op::Move => None,
        Op::Delete => head.root.clone(),
        _ => source.root.clone(),
    };
    if let Some(root) = &root {
        tags.push(tag(&["root", root])?);
    }
    tags.push(tag(&["prev", &head.head])?);
    // A deletion carries only envelope tags; every other revision keeps the
    // client-defined tags and content it did not understand, verbatim.
    let content = if op == Op::Delete {
        String::new()
    } else {
        for client in source.event.tags.iter().filter(|tag| {
            tag.as_slice()
                .first()
                .is_some_and(|name| !ENVELOPE.contains(&name.as_str()) && name != "auth")
        }) {
            tags.push(client.clone());
        }
        source.event.content.clone()
    };
    Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content).tags(tags))
}

/// Classifies a rejected publish: `conflict:` means reload and reconcile.
pub fn is_conflict(message: &str) -> bool {
    message.starts_with("conflict:")
}

/// Untrusted text for one line: control characters removed, bounded.
pub fn display(value: &str, limit: usize) -> String {
    let clean = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if clean.chars().count() > limit {
        clean
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>()
            + "…"
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    fn create(keys: &Keys, home: Uuid, extra: &[&[&str]], content: &str) -> Event {
        let id = Uuid::new_v4().to_string();
        let home = home.to_string();
        let mut tags = vec![
            tag(&["ar", "1"]).unwrap(),
            tag(&["d", &id]).unwrap(),
            tag(&["h", &home]).unwrap(),
            tag(&["type", "buzz.task"]).unwrap(),
            tag(&["title", "Fix the payment timeout"]).unwrap(),
            tag(&["op", "create"]).unwrap(),
        ];
        tags.extend(extra.iter().map(|parts| tag(parts).unwrap()));
        EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content)
            .tags(tags)
            .sign_with_keys(keys)
            .unwrap()
    }

    fn sign(builder: EventBuilder, keys: &Keys) -> Artifact {
        parse(&builder.sign_with_keys(keys).unwrap()).unwrap()
    }

    #[test]
    fn parses_and_rejects_invalid_envelopes() {
        let keys = Keys::generate();
        let event = create(
            &keys,
            Uuid::new_v4(),
            &[&["assignee", "abc"]],
            "{\"version\":1}",
        );
        let artifact = parse(&event).unwrap();
        assert_eq!(artifact.op, Op::Create);
        assert_eq!(
            artifact.client_tags(),
            vec![("assignee".into(), "abc".into())]
        );
        // Duplicate envelope tag.
        let bad = EventBuilder::new(Kind::Custom(KIND_ARTIFACT), "")
            .tags(
                event
                    .tags
                    .iter()
                    .cloned()
                    .chain([tag(&["op", "update"]).unwrap()]),
            )
            .sign_with_keys(&keys)
            .unwrap();
        assert!(parse(&bad).is_err());
        // Forged signature.
        let mut forged = event.clone();
        forged.content = "{\"version\":2}".into();
        assert!(parse(&forged).is_err());
    }

    #[test]
    fn rename_preserves_unknown_tags_and_content_and_chains_prev() {
        let keys = Keys::generate();
        let home = Uuid::new_v4();
        let content = "{\"version\":7,\"future\":{\"x\":1}}";
        let head = parse(&create(
            &keys,
            home,
            &[&["assignee", "p1"], &["k", "v", "extra"]],
            content,
        ))
        .unwrap();
        let next = sign(
            build(&Change::Rename(" New title ".into()), &head, None).unwrap(),
            &keys,
        );
        assert_eq!(next.op, Op::Update);
        assert_eq!(next.title.as_deref(), Some("New title"));
        assert_eq!(next.home, home);
        assert_eq!(next.event.content, content);
        assert_eq!(next.tag("prev"), Some(head.head.clone()));
        assert_eq!(next.client_tags(), head.client_tags());
        let extra = next
            .event
            .tags
            .iter()
            .find(|tag| tag.as_slice()[0] == "k")
            .unwrap();
        assert_eq!(extra.as_slice(), ["k", "v", "extra"]);
        assert!(build(&Change::Rename("   ".into()), &head, None).is_err());
    }

    #[test]
    fn delete_then_restore_round_trips_the_snapshot() {
        let keys = Keys::generate();
        let root = "a".repeat(64);
        let head = parse(&create(
            &keys,
            Uuid::new_v4(),
            &[&["root", &root], &["assignee", "p1"]],
            "{\"v\":1}",
        ))
        .unwrap();
        let deleted = sign(build(&Change::Delete, &head, None).unwrap(), &keys);
        assert!(deleted.is_deleted());
        assert_eq!(deleted.title, None);
        assert!(deleted.event.content.is_empty());
        assert!(
            deleted.client_tags().is_empty(),
            "only envelope tags on delete"
        );
        assert_eq!(deleted.root.as_deref(), Some(root.as_str()));
        assert!(build(&Change::Rename("x".into()), &deleted, None).is_err());
        assert!(
            build(&Change::Restore, &deleted, None).is_err(),
            "needs a snapshot"
        );
        let restored = sign(
            build(&Change::Restore, &deleted, Some(&head)).unwrap(),
            &keys,
        );
        assert_eq!(restored.op, Op::Restore);
        assert_eq!(restored.tag("prev"), Some(deleted.head));
        assert_eq!(restored.title, head.title);
        assert_eq!(restored.event.content, head.event.content);
        assert_eq!(restored.client_tags(), head.client_tags());
    }

    #[test]
    fn move_changes_home_and_drops_the_root() {
        let keys = Keys::generate();
        let root = "b".repeat(64);
        let head = parse(&create(&keys, Uuid::new_v4(), &[&["root", &root]], "{}")).unwrap();
        let destination = Uuid::new_v4();
        let moved = sign(
            build(&Change::Move(destination), &head, None).unwrap(),
            &keys,
        );
        assert_eq!(moved.op, Op::Move);
        assert_eq!(moved.home, destination);
        assert_eq!(moved.root, None);
        assert_eq!(moved.title, head.title);
    }

    #[test]
    fn reconcile_reapplies_or_reports_instead_of_overwriting() {
        let keys = Keys::generate();
        let head = parse(&create(&keys, Uuid::new_v4(), &[], "{}")).unwrap();
        let deleted = sign(build(&Change::Delete, &head, None).unwrap(), &keys);
        assert_eq!(
            reconcile(&Change::Rename("Other".into()), &head),
            Reconcile::Retry
        );
        assert_eq!(
            reconcile(&Change::Rename("Fix the payment timeout".into()), &head),
            Reconcile::Done
        );
        assert!(matches!(
            reconcile(&Change::Rename("x".into()), &deleted),
            Reconcile::Impossible(_)
        ));
        assert_eq!(reconcile(&Change::Delete, &deleted), Reconcile::Done);
        assert_eq!(reconcile(&Change::Restore, &head), Reconcile::Done);
        assert!(matches!(
            reconcile(&Change::Move(Uuid::new_v4()), &head),
            Reconcile::Impossible(_)
        ));
        assert_eq!(reconcile(&Change::Move(head.home), &head), Reconcile::Done);
        assert!(is_conflict("conflict: artifact head unavailable"));
        assert!(!is_conflict("invalid: bad"));
    }

    #[test]
    fn removal_requires_the_pinned_relay() {
        let relay = Keys::generate();
        let id = Uuid::new_v4().to_string();
        let source = Uuid::new_v4().to_string();
        let event = EventBuilder::new(Kind::Custom(KIND_ARTIFACT_REMOVAL), "")
            .tags([
                tag(&["ar", "1"]).unwrap(),
                tag(&["d", &id]).unwrap(),
                tag(&["h", &source]).unwrap(),
                tag(&["reason", "moved"]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        let pinned = relay.public_key().to_hex();
        assert_eq!(
            parse_removal(&event, Some(&pinned))
                .unwrap()
                .source
                .to_string(),
            source
        );
        assert!(parse_removal(&event, Some(&Keys::generate().public_key().to_hex())).is_err());
        assert!(parse_removal(&event, None).is_err());
    }

    #[test]
    fn capabilities_and_digest() {
        let caps = capabilities(
            &json!({"artifacts":{"modes":["current","history","head"],"sync":{"version":1,"max_events":4096}}}),
        );
        assert_eq!(
            caps,
            Capabilities {
                sync: true,
                head: true,
                max_events: 4096
            }
        );
        assert!(!capabilities(&json!({})).sync);
        let keys = Keys::generate();
        let a = parse(&create(&keys, Uuid::new_v4(), &[], "{}")).unwrap();
        let b = parse(&create(&keys, Uuid::new_v4(), &[], "{}")).unwrap();
        assert_eq!(
            set_digest([&a, &b]),
            set_digest([&b, &a]),
            "order-independent"
        );
        assert_ne!(set_digest([&a]), set_digest([&a, &b]));
        assert_eq!(display("a\u{1b}[31mb", 80), "a [31mb");
    }
}
