//! NIP-CL compact public membership lookup, version 1 (kind 39009).
//!
//! "Which listed, open channels is this person in?" for a profile card. The
//! answer is display data only: it never touches the viewer's own membership
//! authority, rosters or send permissions, and failure is never an empty list.

use nostr::{Event, JsonUtil as _};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    domain::MemberRole,
    error::{Error, Result},
    protocol::membership_snapshot::{Descriptor, MAX_CHANNELS, MAX_RESPONSE_BYTES, RawEntry},
};

pub const KIND_MEMBERSHIP_PUBLIC: u16 = 39_009;
/// Every public type, sorted and unique as the request requires.
pub const PUBLIC_TYPES: [&str; 3] = ["forum", "stream", "workflow"];
const UNSUPPORTED_BODY: &str = "membership_public unsupported";

/// The host-bound NIP-11 `membership_public` descriptor.
pub fn descriptor(info: &Value) -> Option<Descriptor> {
    let value = info.get("membership_public")?;
    if value.get("version")?.as_u64()? != 1
        || value.get("kind")?.as_u64()? != u64::from(KIND_MEMBERSHIP_PUBLIC)
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
pub fn request_body(subject: &str) -> Vec<u8> {
    serde_json::to_vec(&json!([{
        "kinds": [KIND_MEMBERSHIP_PUBLIC],
        "#p": [subject],
        "#t": PUBLIC_TYPES,
        "membership_public": 1
    }]))
    .expect("static request serializes")
}

/// What one outstanding request must be bound to.
#[derive(Clone, Debug)]
pub struct Expected<'a> {
    pub relay_pubkey: &'a str,
    pub community_id: Uuid,
    /// Canonical authority of the NIP-98 `u` URL (`host[:port]`).
    pub host: &'a str,
    /// The current signer.
    pub requester: &'a str,
    pub subject: &'a str,
    /// The NIP-98 event ID of this request.
    pub request_id: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    version: u8,
    scope: String,
    community_id: Uuid,
    host: String,
    requester: String,
    subject: String,
    request_id: String,
    types: Vec<String>,
    complete: bool,
    snapshot_id: String,
    channels: Vec<RawEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicMembership {
    pub channel_id: Uuid,
    /// The subject's role; never a grant to the requester.
    pub role: MemberRole,
    pub channel_type: String,
    pub archived: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPublic {
    pub snapshot_id: String,
    pub memberships: Vec<PublicMembership>,
}

/// The outcome of one lookup. Only `Verified` is a membership list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    Verified(VerifiedPublic),
    /// No descriptor, the exact disabled 404, or an ordinary array.
    Unsupported,
    /// HTTP 429 with the relay's `retry in <seconds>s` delay.
    RateLimited(u64),
}

fn reject(reason: &'static str) -> Error {
    Error::Protocol(format!("public membership rejected: {reason}"))
}

/// Classify a raw HTTP answer: unsupported and rate-limited answers are never
/// verified lists; every other non-200 is an error, never an empty result.
pub fn classify(status: u16, body: &[u8], expected: &Expected<'_>) -> Result<Lookup> {
    match status {
        200 => {
            if serde_json::from_slice::<Value>(body).is_ok_and(|value| value.is_array()) {
                // An ordinary array from an older pod is never a public result.
                return Ok(Lookup::Unsupported);
            }
            verify(body, expected).map(Lookup::Verified)
        }
        404 if serde_json::from_slice::<Value>(body).ok()
            == Some(json!({"error": UNSUPPORTED_BODY})) =>
        {
            Ok(Lookup::Unsupported)
        }
        429 => Ok(Lookup::RateLimited(retry_seconds(body).unwrap_or(60))),
        status => Err(Error::Network(format!(
            "public membership lookup returned HTTP {status}"
        ))),
    }
}

/// `rate-limited: quota exceeded; retry in <seconds>s`.
fn retry_seconds(body: &[u8]) -> Option<u64> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let message = value.get("error")?.as_str()?;
    let tail = message.split("retry in ").nth(1)?;
    tail.trim_end_matches('s').trim().parse().ok()
}

/// Verify a 200 body against its outstanding request.
pub fn verify(body: &[u8], expected: &Expected<'_>) -> Result<VerifiedPublic> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(reject("response exceeds the version-1 byte bound"));
    }
    let envelope: Value = serde_json::from_slice(body).map_err(|_| reject("not JSON"))?;
    let object = envelope
        .as_object()
        .ok_or_else(|| reject("not an object"))?;
    // The self-mode envelope (`membership_snapshot`) is a cross-mode answer.
    if object.len() != 2 || object.get("membership_public") != Some(&json!(1)) {
        return Err(reject("envelope version or mode"));
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
    if event.kind.as_u16() != KIND_MEMBERSHIP_PUBLIC
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
            vec!["p".to_owned(), expected.subject.to_owned()],
        ]
    {
        return Err(reject("tags"));
    }
    let content: Content =
        serde_json::from_str(&event.content).map_err(|_| reject("content shape"))?;
    if content.version != 1
        || content.scope != "public"
        || content.community_id != expected.community_id
        || content.host != expected.host
        || content.requester != expected.requester
        || content.subject != expected.subject
        || content.request_id != expected.request_id
        || content.types != PUBLIC_TYPES
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
        if !PUBLIC_TYPES.contains(&entry.channel_type.as_str()) || entry.visibility != "open" {
            return Err(reject("type or visibility"));
        }
        memberships.push(PublicMembership {
            channel_id: entry.id,
            role,
            channel_type: entry.channel_type.clone(),
            archived: entry.archived,
        });
    }
    let identity = serde_json::to_vec(&(
        "nip-cl-public-membership-v1",
        content.community_id,
        &content.subject,
        &content.types,
        &content.channels,
    ))
    .map_err(|error| Error::Serialization(error.to_string()))?;
    if hex::encode(Sha256::digest(identity)) != content.snapshot_id {
        return Err(reject("snapshot_id"));
    }
    Ok(VerifiedPublic {
        snapshot_id: content.snapshot_id,
        memberships,
    })
}

fn is_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUESTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SUBJECT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const REQUEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Forge {
        relay: nostr::Keys,
        community: Uuid,
        channels: Vec<RawEntry>,
    }

    impl Forge {
        fn new() -> Self {
            let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
            ids.sort();
            Self {
                relay: nostr::Keys::generate(),
                community: Uuid::new_v4(),
                channels: vec![
                    RawEntry {
                        id: ids[0],
                        role: "member".into(),
                        channel_type: "stream".into(),
                        visibility: "open".into(),
                        archived: false,
                    },
                    RawEntry {
                        id: ids[1],
                        role: "guest".into(),
                        channel_type: "forum".into(),
                        visibility: "open".into(),
                        archived: true,
                    },
                ],
            }
        }

        fn expected(&self) -> Expected<'_> {
            Expected {
                relay_pubkey: Box::leak(self.relay.public_key().to_hex().into_boxed_str()),
                community_id: self.community,
                host: "team.example",
                requester: REQUESTER,
                subject: SUBJECT,
                request_id: REQUEST,
            }
        }

        fn content(&self) -> Value {
            let identity = serde_json::to_vec(&(
                "nip-cl-public-membership-v1",
                self.community,
                SUBJECT,
                PUBLIC_TYPES,
                &self.channels,
            ))
            .unwrap();
            json!({
                "version": 1, "scope": "public", "community_id": self.community,
                "host": "team.example", "requester": REQUESTER, "subject": SUBJECT,
                "request_id": REQUEST, "types": PUBLIC_TYPES, "complete": true,
                "snapshot_id": hex::encode(Sha256::digest(identity)),
                "channels": self.channels,
            })
        }

        fn body(&self, kind: u16, mode: &str, content: &Value, tags: &[[&str; 2]]) -> Vec<u8> {
            let event = nostr::EventBuilder::new(nostr::Kind::Custom(kind), content.to_string())
                .tags(tags.iter().map(|tag| nostr::Tag::parse(*tag).unwrap()))
                .sign_with_keys(&self.relay)
                .unwrap();
            serde_json::to_vec(&json!({mode: 1, "event": event})).unwrap()
        }

        fn valid(&self) -> Vec<u8> {
            self.body(
                KIND_MEMBERSHIP_PUBLIC,
                "membership_public",
                &self.content(),
                &[["d", REQUEST], ["p", SUBJECT]],
            )
        }
    }

    #[test]
    fn valid_public_answer_preserves_subject_roles_and_archive_state() {
        let forge = Forge::new();
        let verified = verify(&forge.valid(), &forge.expected()).unwrap();
        assert_eq!(verified.memberships.len(), 2);
        assert_eq!(verified.memberships[1].role, MemberRole::Guest);
        assert!(verified.memberships[1].archived);
        assert_eq!(
            request_body(SUBJECT),
            br##"[{"#p":["cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"],"#t":["forum","stream","workflow"],"kinds":[39009],"membership_public":1}]"##
        );
    }

    #[test]
    fn every_spec_rejection_case_fails_closed() {
        let forge = Forge::new();
        let expected = forge.expected();
        let mutate = |edit: &dyn Fn(&mut Value)| {
            let mut content = forge.content();
            edit(&mut content);
            forge.body(
                KIND_MEMBERSHIP_PUBLIC,
                "membership_public",
                &content,
                &[["d", REQUEST], ["p", SUBJECT]],
            )
        };
        // Entry edits are re-signed with a recomputed snapshot_id, so only the
        // check under test can reject them.
        let entries = |edit: &dyn Fn(&mut Vec<RawEntry>)| {
            let mut edited = Forge {
                relay: forge.relay.clone(),
                community: forge.community,
                channels: forge.channels.clone(),
            };
            edit(&mut edited.channels);
            edited.valid()
        };
        let other = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("plain array", b"[]".to_vec()),
            ("requester C", mutate(&|c| c["requester"] = json!(other))),
            ("subject C", mutate(&|c| c["subject"] = json!(other))),
            ("other host", mutate(&|c| c["host"] = json!("evil.example"))),
            (
                "other community",
                mutate(&|c| c["community_id"] = json!(Uuid::new_v4())),
            ),
            ("type subset", mutate(&|c| c["types"] = json!(["stream"]))),
            ("other request", mutate(&|c| c["request_id"] = json!(other))),
            ("self scope", mutate(&|c| c["scope"] = json!("self"))),
            ("incomplete", mutate(&|c| c["complete"] = json!(false))),
            (
                "private entry",
                entries(&|c| c[0].visibility = "private".into()),
            ),
            ("dm entry", entries(&|c| c[0].channel_type = "dm".into())),
            ("bad role", entries(&|c| c[0].role = "king".into())),
            (
                "archived string",
                mutate(&|c| c["channels"][0]["archived"] = json!("no")),
            ),
            ("extra field", mutate(&|c| c["viewer"] = json!(REQUESTER))),
            ("duplicate", entries(&|c| c[1].id = c[0].id)),
            ("unordered", entries(&|c| c.swap(0, 1))),
            (
                "snapshot id",
                mutate(&|c| c["snapshot_id"] = json!("0".repeat(64))),
            ),
            (
                "self envelope",
                forge.body(
                    KIND_MEMBERSHIP_PUBLIC,
                    "membership_snapshot",
                    &forge.content(),
                    &[["d", REQUEST], ["p", SUBJECT]],
                ),
            ),
            (
                "kind 39008",
                forge.body(
                    39_008,
                    "membership_public",
                    &forge.content(),
                    &[["d", REQUEST], ["p", SUBJECT]],
                ),
            ),
            (
                "extra tag",
                forge.body(
                    KIND_MEMBERSHIP_PUBLIC,
                    "membership_public",
                    &forge.content(),
                    &[["d", REQUEST], ["p", SUBJECT], ["t", "stream"]],
                ),
            ),
        ];
        for (name, body) in cases {
            assert!(verify(&body, &expected).is_err(), "{name}");
        }
        // Forged signer: same content, another key.
        let forged = Forge {
            relay: nostr::Keys::generate(),
            community: forge.community,
            channels: forge.channels.clone(),
        };
        assert!(verify(&forged.valid(), &expected).is_err(), "forged signer");
    }

    #[test]
    fn unsupported_and_rate_limited_answers_are_never_lists() {
        let forge = Forge::new();
        let expected = forge.expected();
        assert_eq!(
            classify(200, b"[]", &expected).unwrap(),
            Lookup::Unsupported
        );
        assert_eq!(
            classify(
                404,
                br#"{"error":"membership_public unsupported"}"#,
                &expected
            )
            .unwrap(),
            Lookup::Unsupported
        );
        assert!(classify(404, br#"{"error":"not found"}"#, &expected).is_err());
        assert_eq!(
            classify(
                429,
                br#"{"error":"rate-limited: quota exceeded; retry in 17s"}"#,
                &expected
            )
            .unwrap(),
            Lookup::RateLimited(17)
        );
        for status in [400, 401, 403, 413, 503] {
            assert!(classify(status, b"{}", &expected).is_err(), "{status}");
        }
        assert!(matches!(
            classify(200, &forge.valid(), &expected).unwrap(),
            Lookup::Verified(_)
        ));
    }
}
