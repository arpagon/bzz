//! NIP-CW channel windows (kind 39006 bounds) and thread windows (kind 39007
//! bounds), identity-verified profile: every overlay must be signed by the
//! pinned relay key and bound to exactly the request that produced it.
//! Anything else is not a usable page; the caller falls back.

use nostr::Event;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

pub const KIND_THREAD_SUMMARY: u16 = 39_005;
pub const KIND_WINDOW_BOUNDS: u16 = 39_006;
pub const KIND_THREAD_BOUNDS: u16 = 39_007;
pub const CHANNEL_ROW_KINDS: [u32; 3] = [9, 40_002, 40_099];
pub const THREAD_ROW_KINDS: [u32; 2] = [9, 40_002];
const AUX_KINDS: [u16; 4] = [5, 7, 9005, 40_003];

/// Composite cursor `(created_at, id)` for `created_at DESC, id ASC` order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub created_at: u64,
    pub id: String,
}

impl Cursor {
    fn suffix(cursor: Option<&Self>) -> String {
        cursor.map_or_else(
            || "head".to_owned(),
            |cursor| format!("{}:{}", cursor.created_at, cursor.id),
        )
    }
}

/// One verified page.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub rows: Vec<Event>,
    pub aux: Vec<Event>,
    pub summaries: Vec<Event>,
    pub next: Option<Cursor>,
}

impl Page {
    pub const fn has_more(&self) -> bool {
        self.next.is_some()
    }
}

fn reject(reason: &'static str) -> Error {
    Error::Protocol(format!("window page rejected: {reason}"))
}

pub fn channel_filter(channel: Uuid, limit: u32, cursor: Option<&Cursor>) -> Value {
    let mut filter = json!({
        "kinds": CHANNEL_ROW_KINDS,
        "#h": [channel.to_string()],
        "limit": limit.clamp(1, 200),
        "top_level": true,
        "include_summaries": true,
        "include_aux": true,
    });
    if let Some(cursor) = cursor {
        filter["until"] = json!(cursor.created_at);
        filter["before_id"] = json!(cursor.id);
    }
    filter
}

#[derive(Clone, Debug)]
pub struct ThreadRequest<'a> {
    /// Canonical `host[:port]` of the relay (the binding's `host`).
    pub host: &'a str,
    pub reader: &'a str,
    pub channel: Uuid,
    pub root: &'a str,
    pub limit: u32,
    pub depth: u32,
    pub include_aux: bool,
    pub cursor: Option<&'a Cursor>,
}

pub fn thread_filter(request: &ThreadRequest<'_>) -> Value {
    let mut filter = json!({
        "thread_window": true,
        "#h": [request.channel.to_string()],
        "#e": [request.root],
        "kinds": THREAD_ROW_KINDS,
        "depth_limit": request.depth,
        "limit": request.limit,
        "include_aux": request.include_aux,
    });
    if let Some(cursor) = request.cursor {
        filter["until"] = json!(cursor.created_at);
        filter["before_id"] = json!(cursor.id);
    }
    filter
}

/// `tw:1:` + SHA-256 of the compact binding array.
pub fn thread_binding(request: &ThreadRequest<'_>) -> String {
    let cursor = request
        .cursor
        .map_or(Value::Null, |cursor| json!([cursor.created_at, cursor.id]));
    let binding = json!([
        "tw",
        1,
        "older",
        request.host,
        request.reader,
        request.channel.to_string(),
        request.root,
        request.limit,
        request.depth,
        THREAD_ROW_KINDS,
        cursor,
        request.include_aux
    ]);
    format!(
        "tw:1:{}",
        hex::encode(Sha256::digest(binding.to_string().as_bytes()))
    )
}

fn exact_tags(event: &Event, expected: &[(&str, &str)]) -> bool {
    let tags = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    tags.len() == expected.len()
        && expected.iter().all(|(name, value)| {
            tags.iter()
                .any(|tag| tag.len() == 2 && tag[0] == *name && tag[1] == *value)
        })
}

/// `has_more` and `next_cursor` from a bounds content object, with the
/// invariant `next_cursor == null ⇔ has_more == false`.
fn bounds_next(content: &Value) -> Result<Option<Cursor>> {
    let has_more = content
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or_else(|| reject("has_more"))?;
    let next = content
        .get("next_cursor")
        .ok_or_else(|| reject("next_cursor"))?;
    match (has_more, next) {
        (false, Value::Null) => Ok(None),
        (true, Value::Object(cursor)) => {
            let created_at = cursor
                .get("created_at")
                .and_then(Value::as_u64)
                .ok_or_else(|| reject("cursor created_at"))?;
            let id = cursor
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| {
                    id.len() == 64
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                .ok_or_else(|| reject("cursor id"))?;
            Ok(Some(Cursor {
                created_at,
                id: id.to_owned(),
            }))
        }
        _ => Err(reject("has_more/next_cursor disagree")),
    }
}

fn verified(event: &Event, relay: &str) -> bool {
    event.pubkey.to_hex() == relay && event.verify().is_ok()
}

/// Partition and verify a channel window response.
pub fn verify_channel(
    events: Vec<Event>,
    channel: Uuid,
    cursor: Option<&Cursor>,
    relay: &str,
) -> Result<Page> {
    let channel_id = channel.to_string();
    let mut page = Page::default();
    let mut bounds = Vec::new();
    for event in events {
        match event.kind.as_u16() {
            KIND_WINDOW_BOUNDS => bounds.push(event),
            KIND_THREAD_SUMMARY => {
                if verified(&event, relay) {
                    page.summaries.push(event);
                }
            }
            kind if AUX_KINDS.contains(&kind) => page.aux.push(event),
            kind if CHANNEL_ROW_KINDS.contains(&u32::from(kind)) => {
                if crate::protocol::events::channel_id(&event) != Some(channel) {
                    return Err(reject("row outside the requested channel"));
                }
                page.rows.push(event);
            }
            _ => return Err(reject("unexpected kind")),
        }
    }
    let [bound] = bounds.as_slice() else {
        return Err(reject("exactly one 39006 is required"));
    };
    if !verified(bound, relay)
        || !exact_tags(
            bound,
            &[
                ("d", &format!("{channel_id}:{}", Cursor::suffix(cursor))),
                ("h", &channel_id),
            ],
        )
    {
        return Err(reject("39006 signer, tags or request binding"));
    }
    let content: Value =
        serde_json::from_str(&bound.content).map_err(|_| reject("39006 content"))?;
    page.next = bounds_next(&content)?;
    Ok(page)
}

/// Partition and verify a thread window response.
pub fn verify_thread(events: Vec<Event>, request: &ThreadRequest<'_>, relay: &str) -> Result<Page> {
    let channel_id = request.channel.to_string();
    let mut page = Page::default();
    let mut bounds = Vec::new();
    for event in events {
        match event.kind.as_u16() {
            KIND_THREAD_BOUNDS => bounds.push(event),
            kind if AUX_KINDS.contains(&kind) => page.aux.push(event),
            kind if THREAD_ROW_KINDS.contains(&u32::from(kind)) => {
                if crate::protocol::events::channel_id(&event) != Some(request.channel) {
                    return Err(reject("reply outside the requested channel"));
                }
                page.rows.push(event);
            }
            _ => return Err(reject("unexpected kind")),
        }
    }
    let [bound] = bounds.as_slice() else {
        return Err(reject("exactly one 39007 is required"));
    };
    if !verified(bound, relay)
        || !exact_tags(
            bound,
            &[
                ("d", &thread_binding(request)),
                ("h", &channel_id),
                ("e", request.root),
            ],
        )
    {
        return Err(reject("39007 signer, tags or request binding"));
    }
    let content: Value =
        serde_json::from_str(&bound.content).map_err(|_| reject("39007 content"))?;
    if content.get("version").and_then(Value::as_u64) != Some(1)
        || content.get("direction").and_then(Value::as_str) != Some("older")
    {
        return Err(reject("39007 version or direction"));
    }
    page.next = bounds_next(&content)?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn bounds(relay: &Keys, kind: u16, tags: &[&[&str]], content: &Value) -> Event {
        EventBuilder::new(Kind::Custom(kind), content.to_string())
            .tags(
                tags.iter()
                    .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
            )
            .sign_with_keys(relay)
            .unwrap()
    }

    fn row(channel: Uuid, at: u64) -> Event {
        buzz_sdk::build_message(channel, "row", None, &[], false, &[], &[])
            .unwrap()
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(&Keys::generate())
            .unwrap()
    }

    #[test]
    fn channel_bounds_bind_to_the_request_cursor() {
        let relay = Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let channel = Uuid::new_v4();
        let id = channel.to_string();
        let next =
            json!({"has_more": true, "next_cursor": {"created_at": 5, "id": "a".repeat(64)}});
        let head = bounds(
            &relay,
            39_006,
            &[&["d", &format!("{id}:head")], &["h", &id]],
            &next,
        );
        let page = verify_channel(
            vec![row(channel, 9), head.clone()],
            channel,
            None,
            &relay_hex,
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.next.unwrap().created_at, 5);

        let cursor = Cursor {
            created_at: 5,
            id: "a".repeat(64),
        };
        // A head bounds answering a cursor request is not this page.
        assert!(verify_channel(vec![head.clone()], channel, Some(&cursor), &relay_hex).is_err());
        let bound = bounds(
            &relay,
            39_006,
            &[&["d", &format!("{id}:5:{}", "a".repeat(64))], &["h", &id]],
            &json!({"has_more": false, "next_cursor": null}),
        );
        assert!(
            !verify_channel(vec![bound], channel, Some(&cursor), &relay_hex)
                .unwrap()
                .has_more()
        );

        for (name, events) in [
            ("missing", vec![row(channel, 1)]),
            ("duplicate", vec![head.clone(), head.clone()]),
            (
                "forged",
                vec![bounds(
                    &Keys::generate(),
                    39_006,
                    &[&["d", &format!("{id}:head")], &["h", &id]],
                    &next,
                )],
            ),
            (
                "extra tag",
                vec![bounds(
                    &relay,
                    39_006,
                    &[&["d", &format!("{id}:head")], &["h", &id], &["x", "1"]],
                    &next,
                )],
            ),
            (
                "disagree",
                vec![bounds(
                    &relay,
                    39_006,
                    &[&["d", &format!("{id}:head")], &["h", &id]],
                    &json!({"has_more": true, "next_cursor": null}),
                )],
            ),
            ("foreign row", vec![row(Uuid::new_v4(), 1), head.clone()]),
        ] {
            assert!(
                verify_channel(events, channel, None, &relay_hex).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn thread_bounds_bind_every_request_input() {
        let relay = Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let channel = Uuid::new_v4();
        let root = "b".repeat(64);
        let reader = "c".repeat(64);
        let request = ThreadRequest {
            host: "team.example",
            reader: &reader,
            channel,
            root: &root,
            limit: 50,
            depth: 100,
            include_aux: true,
            cursor: None,
        };
        let content =
            json!({"version": 1, "direction": "older", "has_more": false, "next_cursor": null});
        let good = bounds(
            &relay,
            39_007,
            &[
                &["d", &thread_binding(&request)],
                &["h", &channel.to_string()],
                &["e", &root],
            ],
            &content,
        );
        assert!(verify_thread(vec![good.clone()], &request, &relay_hex).is_ok());
        let other_limit = ThreadRequest {
            limit: 49,
            ..request.clone()
        };
        assert!(verify_thread(vec![good.clone()], &other_limit, &relay_hex).is_err());
        let other_host = ThreadRequest {
            host: "evil.example",
            ..request.clone()
        };
        assert!(verify_thread(vec![good], &other_host, &relay_hex).is_err());
        let wrong_direction = bounds(
            &relay,
            39_007,
            &[
                &["d", &thread_binding(&request)],
                &["h", &channel.to_string()],
                &["e", &root],
            ],
            &json!({"version": 1, "direction": "newer", "has_more": false, "next_cursor": null}),
        );
        assert!(verify_thread(vec![wrong_direction], &request, &relay_hex).is_err());
    }
}
