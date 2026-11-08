//! NIP-RS Buzz Atomic Snapshot Extension, version 1.
//!
//! One NIP-98 `POST /query` returns every non-deleted own-author kind 30078
//! event at one cut. bzz is a frontier-only reader (no manual-unread override
//! layer), so a verified snapshot is used only as a complete startup load; any
//! rejection is "cannot prove complete" and the caller keeps the WebSocket
//! read-state subscription path.

use std::collections::BTreeSet;

use nostr::{Event, JsonUtil as _};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

pub const MAX_EVENTS: usize = 4096;
pub const MAX_EVENT_ARRAY_BYTES: usize = 8_388_608;
/// Response allowance on top of the event array for the success envelope.
pub const ENVELOPE_ALLOWANCE: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Descriptor {
    pub community_id: Uuid,
    pub max_events: usize,
    pub max_event_array_bytes: usize,
}

pub fn descriptor(info: &Value) -> Option<Descriptor> {
    let value = info.get("read_state_snapshot")?;
    if value.get("version")?.as_u64()? != 1 {
        return None;
    }
    let bound = |name: &str, cap: usize| {
        value
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .map(|value| value.min(cap))
    };
    Some(Descriptor {
        community_id: Uuid::parse_str(value.get("community_id")?.as_str()?).ok()?,
        max_events: bound("max_events", MAX_EVENTS)?,
        max_event_array_bytes: bound("max_event_array_bytes", MAX_EVENT_ARRAY_BYTES)?,
    })
}

pub fn request_body(author: &str) -> Vec<u8> {
    serde_json::to_vec(&json!([{"kinds":[30078],"authors":[author],"read_state_snapshot":1}]))
        .expect("static request serializes")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSnapshot {
    pub snapshot_id: String,
    pub events: Vec<Event>,
}

fn reject(reason: &'static str) -> Error {
    Error::Protocol(format!(
        "read-state snapshot cannot prove complete: {reason}"
    ))
}

/// SHA-256 over `buzz-read-state-snapshot-v1\0 || community(16) || author(32)
/// || ids(32 each)` in `created_at DESC, id ASC` order.
pub fn snapshot_id(community_id: Uuid, author: &nostr::PublicKey, events: &[Event]) -> String {
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
    });
    let mut digest = Sha256::new();
    digest.update(b"buzz-read-state-snapshot-v1\0");
    digest.update(community_id.as_bytes());
    digest.update(author.to_bytes());
    for event in ordered {
        digest.update(event.id.as_bytes());
    }
    hex::encode(digest.finalize())
}

pub fn verify(
    body: &[u8],
    descriptor: &Descriptor,
    author: &nostr::PublicKey,
) -> Result<VerifiedSnapshot> {
    let envelope: Value = serde_json::from_slice(body).map_err(|_| reject("not JSON"))?;
    let object = envelope
        .as_object()
        .ok_or_else(|| reject("ordinary array or non-object"))?;
    if object.get("read_state_snapshot") != Some(&json!(1))
        || object.get("complete") != Some(&json!(true))
    {
        return Err(reject("version or completeness"));
    }
    if object.get("community_id").and_then(Value::as_str)
        != Some(descriptor.community_id.to_string().as_str())
        || object.get("pubkey").and_then(Value::as_str) != Some(author.to_hex().as_str())
    {
        return Err(reject("community or pubkey binding"));
    }
    let claimed = object
        .get("snapshot_id")
        .and_then(Value::as_str)
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| reject("snapshot_id"))?;
    let array = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| reject("events"))?;
    if array.len() > descriptor.max_events
        || serde_json::to_vec(array).map_or(usize::MAX, |bytes| bytes.len())
            > descriptor.max_event_array_bytes
    {
        return Err(reject("bounds"));
    }
    let mut events = Vec::with_capacity(array.len());
    let mut coordinates = BTreeSet::new();
    for value in array {
        let event = Event::from_json(value.to_string()).map_err(|_| reject("event envelope"))?;
        event.verify().map_err(|_| reject("event signature"))?;
        if event.kind.as_u16() != 30_078 || event.pubkey != *author {
            return Err(reject("foreign kind or author"));
        }
        let d = crate::protocol::events::tag_values(&event, "d");
        if d.len() > 1 || !coordinates.insert(d.into_iter().next().unwrap_or_default()) {
            return Err(reject("duplicate or conflicting coordinate"));
        }
        events.push(event);
    }
    if snapshot_id(descriptor.community_id, author, &events) != claimed {
        return Err(reject("snapshot_id mismatch"));
    }
    Ok(VerifiedSnapshot {
        snapshot_id: claimed.to_owned(),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn slot(keys: &Keys, d: &str, at: u64) -> Event {
        EventBuilder::new(Kind::Custom(30_078), "ciphertext")
            .tags([
                Tag::parse(["d", d]).unwrap(),
                Tag::parse(["t", "read-state"]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    }

    fn body(
        community: Uuid,
        keys: &Keys,
        events: &[Event],
        mutate: impl Fn(&mut Value),
    ) -> Vec<u8> {
        let mut value = json!({
            "read_state_snapshot": 1,
            "complete": true,
            "community_id": community,
            "pubkey": keys.public_key().to_hex(),
            "snapshot_id": snapshot_id(community, &keys.public_key(), events),
            "events": events,
        });
        mutate(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn verified_snapshot_and_every_unprovable_shape_is_rejected() {
        let keys = Keys::generate();
        let community = Uuid::new_v4();
        let descriptor = Descriptor {
            community_id: community,
            max_events: 4096,
            max_event_array_bytes: MAX_EVENT_ARRAY_BYTES,
        };
        let events = vec![
            slot(&keys, "read-state:a", 10),
            slot(&keys, "app:other", 20),
        ];
        let verified = verify(
            &body(community, &keys, &events, |_| {}),
            &descriptor,
            &keys.public_key(),
        )
        .unwrap();
        assert_eq!(verified.events.len(), 2);
        // An empty valid snapshot is complete.
        assert!(
            verify(
                &body(community, &keys, &[], |_| {}),
                &descriptor,
                &keys.public_key()
            )
            .is_ok()
        );

        let other = Keys::generate();
        let mut duplicate = events.clone();
        duplicate.push(slot(&keys, "read-state:a", 30));
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("array", b"[]".to_vec()),
            (
                "incomplete",
                body(community, &keys, &events, |v| v["complete"] = json!(false)),
            ),
            (
                "version",
                body(community, &keys, &events, |v| {
                    v["read_state_snapshot"] = json!(2)
                }),
            ),
            (
                "community",
                body(community, &keys, &events, |v| {
                    v["community_id"] = json!(Uuid::new_v4())
                }),
            ),
            (
                "pubkey",
                body(community, &keys, &events, |v| {
                    v["pubkey"] = json!(other.public_key().to_hex())
                }),
            ),
            (
                "snapshot_id",
                body(community, &keys, &events, |v| {
                    v["snapshot_id"] = json!("0".repeat(64))
                }),
            ),
            (
                "missing event",
                body(community, &keys, &events, |v| {
                    v["events"].as_array_mut().unwrap().pop();
                }),
            ),
            (
                "foreign author",
                body(community, &keys, &[slot(&other, "read-state:x", 1)], |_| {}),
            ),
            ("duplicate", body(community, &keys, &duplicate, |_| {})),
        ];
        for (name, bytes) in cases {
            assert!(
                verify(&bytes, &descriptor, &keys.public_key()).is_err(),
                "{name}"
            );
        }
        let tight = Descriptor {
            max_events: 1,
            ..descriptor
        };
        assert!(
            verify(
                &body(community, &keys, &events, |_| {}),
                &tight,
                &keys.public_key()
            )
            .is_err()
        );
    }
}
