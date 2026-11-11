//! NIP-CL compact viewer membership snapshot, version 1 (kind 39008).
//!
//! A relay-signed, request-bound projection of the viewer's own active
//! memberships. bzz trusts it only after verifying every binding the spec
//! lists; anything else is "cannot prove" and the caller falls back to the
//! roster path.

use nostr::{Event, JsonUtil as _};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    domain::MemberRole,
    error::{Error, Result},
};

pub const KIND_MEMBERSHIP_SNAPSHOT: u16 = 39_008;
/// Every type version 1 knows, sorted and unique as the request requires.
pub const ALL_TYPES: [&str; 5] = ["dm", "forum", "stream", "system", "workflow"];
pub const MAX_CHANNELS: usize = 4096;
pub const MAX_RESPONSE_BYTES: usize = 2_097_152;

/// The host-bound NIP-11 descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Descriptor {
    pub community_id: Uuid,
    pub relay_pubkey: String,
    pub max_channels: usize,
    pub max_response_bytes: usize,
}

pub fn descriptor(info: &Value) -> Option<Descriptor> {
    let value = info.get("membership_snapshot")?;
    if value.get("version")?.as_u64()? != 1
        || value.get("kind")?.as_u64()? != u64::from(KIND_MEMBERSHIP_SNAPSHOT)
    {
        return None;
    }
    let community_id = Uuid::parse_str(value.get("community_id")?.as_str()?).ok()?;
    let relay_pubkey = value.get("pubkey")?.as_str()?.to_owned();
    if !is_pubkey(&relay_pubkey) {
        return None;
    }
    let bound = |name: &str, cap: usize| {
        value
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .map_or(cap, |value| value.min(cap))
    };
    Some(Descriptor {
        community_id,
        relay_pubkey,
        max_channels: bound("max_channels", MAX_CHANNELS),
        max_response_bytes: bound("max_response_bytes", MAX_RESPONSE_BYTES),
    })
}

/// The one exact request filter.
pub fn request_body(viewer: &str, types: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&json!([{
        "kinds": [KIND_MEMBERSHIP_SNAPSHOT],
        "#p": [viewer],
        "#t": types,
        "membership_snapshot": 1
    }]))
    .expect("static request serializes")
}

/// What this request must be bound to.
#[derive(Clone, Debug)]
pub struct Expected<'a> {
    pub relay_pubkey: &'a str,
    pub community_id: Uuid,
    /// Canonical authority of the NIP-98 `u` URL (`host[:port]`).
    pub host: &'a str,
    pub viewer: &'a str,
    /// The NIP-98 event ID of this single outstanding request.
    pub request_id: &'a str,
    pub types: &'a [&'a str],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawEntry {
    pub id: Uuid,
    pub role: String,
    pub channel_type: String,
    pub visibility: String,
    pub archived: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    version: u8,
    community_id: Uuid,
    host: String,
    viewer: String,
    request_id: String,
    types: Vec<String>,
    complete: bool,
    snapshot_id: String,
    channels: Vec<RawEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Membership {
    pub channel_id: Uuid,
    pub role: MemberRole,
    pub channel_type: String,
    pub open: bool,
    pub archived: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSnapshot {
    pub snapshot_id: String,
    pub types: Vec<String>,
    pub memberships: Vec<Membership>,
}

fn reject(reason: &'static str) -> Error {
    Error::Protocol(format!("membership snapshot rejected: {reason}"))
}

/// Verify a response body against its outstanding request.
pub fn verify(body: &[u8], expected: &Expected<'_>) -> Result<VerifiedSnapshot> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(reject("response exceeds the version-1 byte bound"));
    }
    let envelope: Value = serde_json::from_slice(body).map_err(|_| reject("not JSON"))?;
    let object = envelope
        .as_object()
        .ok_or_else(|| reject("not an object"))?;
    if object.len() != 2 || object.get("membership_snapshot") != Some(&json!(1)) {
        return Err(reject("envelope version"));
    }
    let event = Event::from_json(
        object
            .get("event")
            .ok_or_else(|| reject("missing event"))?
            .to_string(),
    )
    .map_err(|_| reject("malformed event"))?;
    event
        .verify()
        .map_err(|_| reject("invalid id or signature"))?;
    if event.kind.as_u16() != KIND_MEMBERSHIP_SNAPSHOT
        || event.pubkey.to_hex() != expected.relay_pubkey
    {
        return Err(reject("kind or relay key"));
    }
    let tags = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    if tags
        != [
            vec!["d".to_owned(), expected.request_id.to_owned()],
            vec!["p".to_owned(), expected.viewer.to_owned()],
        ]
    {
        return Err(reject("tags"));
    }
    let content: Content =
        serde_json::from_str(&event.content).map_err(|_| reject("content shape"))?;
    let expected_types = expected
        .types
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    if content.version != 1
        || content.community_id != expected.community_id
        || content.host != expected.host
        || content.viewer != expected.viewer
        || content.request_id != expected.request_id
        || content.types != expected_types
        || !content.complete
    {
        return Err(reject("binding"));
    }
    if content.channels.len() > MAX_CHANNELS
        || content
            .channels
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
    {
        return Err(reject("channel order, uniqueness or count"));
    }
    let mut memberships = Vec::with_capacity(content.channels.len());
    for entry in &content.channels {
        let role = match entry.role.as_str() {
            "owner" | "admin" | "member" | "bot" | "guest" => MemberRole::parse(Some(&entry.role)),
            _ => return Err(reject("role")),
        };
        if !expected.types.contains(&entry.channel_type.as_str())
            || !matches!(entry.visibility.as_str(), "open" | "private")
        {
            return Err(reject("type or visibility"));
        }
        memberships.push(Membership {
            channel_id: entry.id,
            role,
            channel_type: entry.channel_type.clone(),
            open: entry.visibility == "open",
            archived: entry.archived,
        });
    }
    let identity = serde_json::to_vec(&(
        "nip-cl-membership-state-v1",
        content.community_id,
        &content.viewer,
        &content.types,
        &content.channels,
    ))
    .map_err(|error| Error::Serialization(error.to_string()))?;
    if hex::encode(Sha256::digest(identity)) != content.snapshot_id {
        return Err(reject("snapshot_id"));
    }
    Ok(VerifiedSnapshot {
        snapshot_id: content.snapshot_id,
        types: content.types,
        memberships,
    })
}

/// Canonical `host[:port]` of a URL, default ports omitted.
pub fn canonical_host(url: &url::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn is_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
pub mod fixtures {
    //! Relay-side construction used by tests to forge valid and invalid
    //! responses exactly as the pinned relay builds them.
    use super::*;

    pub struct Request<'a> {
        pub relay: &'a nostr::Keys,
        pub community_id: Uuid,
        pub host: &'a str,
        pub viewer: &'a str,
        pub request_id: &'a str,
        pub types: &'a [&'a str],
        pub channels: Vec<RawEntry>,
    }

    pub fn content(request: &Request<'_>) -> Value {
        let types = request
            .types
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        let identity = serde_json::to_vec(&(
            "nip-cl-membership-state-v1",
            request.community_id,
            request.viewer,
            &types,
            &request.channels,
        ))
        .unwrap();
        json!({
            "version": 1,
            "community_id": request.community_id,
            "host": request.host,
            "viewer": request.viewer,
            "request_id": request.request_id,
            "types": types,
            "complete": true,
            "snapshot_id": hex::encode(Sha256::digest(identity)),
            "channels": request.channels,
        })
    }

    pub fn envelope(request: &Request<'_>, content: &Value, tags: Vec<Vec<String>>) -> Vec<u8> {
        let event = nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_MEMBERSHIP_SNAPSHOT),
            content.to_string(),
        )
        .tags(tags.into_iter().map(|tag| nostr::Tag::parse(tag).unwrap()))
        .sign_with_keys(request.relay)
        .unwrap();
        serde_json::to_vec(&json!({"membership_snapshot": 1, "event": event})).unwrap()
    }

    pub fn valid(request: &Request<'_>) -> Vec<u8> {
        envelope(
            request,
            &content(request),
            vec![
                vec!["d".into(), request.request_id.into()],
                vec!["p".into(), request.viewer.into()],
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};

    const VIEWER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const REQUEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn entries() -> Vec<RawEntry> {
        let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
        ids.sort();
        vec![
            RawEntry {
                id: ids[0],
                role: "guest".into(),
                channel_type: "stream".into(),
                visibility: "open".into(),
                archived: false,
            },
            RawEntry {
                id: ids[1],
                role: "owner".into(),
                channel_type: "system".into(),
                visibility: "private".into(),
                archived: true,
            },
        ]
    }

    #[test]
    fn valid_snapshot_verifies_and_every_binding_mismatch_is_rejected() {
        let relay = nostr::Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let community = Uuid::new_v4();
        let request = Request {
            relay: &relay,
            community_id: community,
            host: "team.example",
            viewer: VIEWER,
            request_id: REQUEST,
            types: &ALL_TYPES,
            channels: entries(),
        };
        let expected = Expected {
            relay_pubkey: &relay_hex,
            community_id: community,
            host: "team.example",
            viewer: VIEWER,
            request_id: REQUEST,
            types: &ALL_TYPES,
        };
        let verified = verify(&valid(&request), &expected).unwrap();
        assert_eq!(verified.memberships.len(), 2);
        assert_eq!(verified.memberships[0].role, MemberRole::Guest);
        assert!(verified.memberships[1].archived);

        let other_relay = nostr::Keys::generate();
        let forged = Request {
            relay: &other_relay,
            ..Request {
                channels: entries(),
                ..request
            }
        };
        assert!(
            verify(&valid(&forged), &expected).is_err(),
            "wrong relay key"
        );

        let request = Request {
            relay: &relay,
            community_id: community,
            host: "team.example",
            viewer: VIEWER,
            request_id: REQUEST,
            types: &ALL_TYPES,
            channels: entries(),
        };
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        for (field, value) in [
            ("version", json!(2)),
            ("community_id", json!(Uuid::new_v4())),
            ("host", json!("other.example")),
            ("viewer", json!("c".repeat(64))),
            ("request_id", json!("d".repeat(64))),
            ("types", json!(["stream"])),
            ("complete", json!(false)),
            ("snapshot_id", json!("e".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut content = content(&request);
            content[field] = value;
            cases.push((
                field,
                envelope(
                    &request,
                    &content,
                    vec![
                        vec!["d".into(), REQUEST.into()],
                        vec!["p".into(), VIEWER.into()],
                    ],
                ),
            ));
        }
        let mut bad_role = content(&request);
        bad_role["channels"][0]["role"] = json!("superuser");
        cases.push((
            "role",
            envelope(
                &request,
                &bad_role,
                vec![
                    vec!["d".into(), REQUEST.into()],
                    vec!["p".into(), VIEWER.into()],
                ],
            ),
        ));
        cases.push((
            "tags",
            envelope(
                &request,
                &content(&request),
                vec![vec!["p".into(), VIEWER.into()]],
            ),
        ));
        cases.push(("array", b"[]".to_vec()));
        cases.push(("empty", Vec::new()));
        let mut tampered: Value = serde_json::from_slice(&valid(&request)).unwrap();
        tampered["event"]["content"] =
            json!(content(&request).to_string().replace("guest", "owner"));
        cases.push(("tampered", serde_json::to_vec(&tampered).unwrap()));
        for (name, body) in cases {
            assert!(verify(&body, &expected).is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn duplicate_or_unordered_channels_are_rejected() {
        let relay = nostr::Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let community = Uuid::new_v4();
        let mut channels = entries();
        channels.reverse();
        let request = Request {
            relay: &relay,
            community_id: community,
            host: "team.example",
            viewer: VIEWER,
            request_id: REQUEST,
            types: &ALL_TYPES,
            channels,
        };
        let expected = Expected {
            relay_pubkey: &relay_hex,
            community_id: community,
            host: "team.example",
            viewer: VIEWER,
            request_id: REQUEST,
            types: &ALL_TYPES,
        };
        assert!(verify(&valid(&request), &expected).is_err());
    }

    #[test]
    fn descriptor_requires_version_kind_and_caps_bounds() {
        let community = Uuid::new_v4();
        let pubkey = "f".repeat(64);
        let info = json!({"membership_snapshot":{"version":1,"community_id":community,"pubkey":pubkey,"kind":39008,"max_channels":99999,"max_response_bytes":10}});
        let parsed = descriptor(&info).unwrap();
        assert_eq!(parsed.max_channels, MAX_CHANNELS);
        assert_eq!(parsed.max_response_bytes, 10);
        assert!(descriptor(&json!({})).is_none());
        let mut wrong = info.clone();
        wrong["membership_snapshot"]["kind"] = json!(39009);
        assert!(descriptor(&wrong).is_none());
        let canonical = |value: &str| canonical_host(&url::Url::parse(value).unwrap()).unwrap();
        assert_eq!(canonical("https://Team.Example/"), "team.example");
        assert_eq!(canonical("http://localhost:3030/"), "localhost:3030");
        assert_eq!(canonical("https://team.example:443/"), "team.example");
    }
}
