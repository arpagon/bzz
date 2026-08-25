use nostr::Event;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::ThreadSummary,
    protocol::events::{tag_values, verify},
};

const MAX_SUMMARY_BYTES: usize = 64 * 1024;
const MAX_PARTICIPANTS: usize = 1_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveThreadSummary {
    pub root_event_id: String,
    pub channel_id: Uuid,
    pub summary: ThreadSummary,
    pub source_created_at: u64,
    pub source_event_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    reply_count: u32,
    descendant_count: u32,
    last_reply_at: Option<u64>,
    participants: Vec<String>,
}

/// Parse relay-synthesized kind 39005 metadata without granting it durable
/// event, message, read-state, or publication authority.
pub fn parse(event: &Event, relay_pubkey: &str) -> Option<LiveThreadSummary> {
    if event.kind.as_u16() != 39_005
        || event.content.len() > MAX_SUMMARY_BYTES
        || verify(event).is_err()
        || !event.pubkey.to_hex().eq_ignore_ascii_case(relay_pubkey)
    {
        return None;
    }
    let roots = tag_values(event, "e");
    let coordinates = tag_values(event, "d");
    let channels = tag_values(event, "h");
    if roots.len() != 1 || coordinates.len() != 1 || channels.len() != 1 {
        return None;
    }
    let root = roots.into_iter().next()?;
    if coordinates.first() != Some(&root) || !is_lower_hex_key(&root) {
        return None;
    }
    let channel_id = Uuid::parse_str(channels.first()?).ok()?;
    let payload: Payload = serde_json::from_str(&event.content).ok()?;
    if payload.reply_count > payload.descendant_count
        || payload.participants.len() > MAX_PARTICIPANTS
        || payload
            .participants
            .iter()
            .any(|key| !is_lower_hex_key(key))
        || (payload.descendant_count == 0
            && (payload.reply_count != 0
                || payload.last_reply_at.is_some()
                || !payload.participants.is_empty()))
        || (payload.descendant_count > 0
            && (payload.reply_count == 0 || payload.last_reply_at.is_none()))
    {
        return None;
    }
    Some(LiveThreadSummary {
        root_event_id: root,
        channel_id,
        summary: ThreadSummary {
            descendant_count: payload.descendant_count,
            last_reply_at: payload.last_reply_at,
        },
        source_created_at: event.created_at.as_secs(),
        source_event_id: event.id.to_hex(),
    })
}

fn is_lower_hex_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use super::*;

    fn summary(relay: &Keys, root: &str, channel: Uuid, content: serde_json::Value) -> Event {
        EventBuilder::new(Kind::Custom(39_005), content.to_string())
            .tags([
                Tag::parse(["e", root]).unwrap(),
                Tag::parse(["d", root]).unwrap(),
                Tag::parse(["h", &channel.to_string()]).unwrap(),
            ])
            .sign_with_keys(relay)
            .unwrap()
    }

    #[test]
    fn accepts_strict_relay_summary() {
        let relay = Keys::generate();
        let root = "ab".repeat(32);
        let channel = Uuid::new_v4();
        let event = summary(
            &relay,
            &root,
            channel,
            serde_json::json!({
                "reply_count":2,
                "descendant_count":3,
                "last_reply_at":42,
                "participants":["cd".repeat(32)]
            }),
        );
        let parsed = parse(&event, &relay.public_key().to_hex()).unwrap();
        assert_eq!(parsed.root_event_id, root);
        assert_eq!(parsed.channel_id, channel);
        assert_eq!(parsed.summary.descendant_count, 3);
        assert_eq!(parsed.summary.last_reply_at, Some(42));
    }

    #[test]
    fn rejects_wrong_signer_coordinates_and_inconsistent_payload() {
        let relay = Keys::generate();
        let other = Keys::generate();
        let root = "ab".repeat(32);
        let channel = Uuid::new_v4();
        let valid = serde_json::json!({
            "reply_count":1,
            "descendant_count":1,
            "last_reply_at":42,
            "participants":[]
        });
        assert!(
            parse(
                &summary(&relay, &root, channel, valid.clone()),
                &other.public_key().to_hex()
            )
            .is_none()
        );

        let mismatched = EventBuilder::new(Kind::Custom(39_005), valid.to_string())
            .tags([
                Tag::parse(["e", &root]).unwrap(),
                Tag::parse(["d", &"cd".repeat(32)]).unwrap(),
                Tag::parse(["h", &channel.to_string()]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        assert!(parse(&mismatched, &relay.public_key().to_hex()).is_none());

        let inconsistent = summary(
            &relay,
            &root,
            channel,
            serde_json::json!({
                "reply_count":2,
                "descendant_count":1,
                "last_reply_at":42,
                "participants":[]
            }),
        );
        assert!(parse(&inconsistent, &relay.public_key().to_hex()).is_none());
    }

    #[test]
    fn accepts_consistent_zero_summary() {
        let relay = Keys::generate();
        let root = "ab".repeat(32);
        let event = summary(
            &relay,
            &root,
            Uuid::new_v4(),
            serde_json::json!({
                "reply_count":0,
                "descendant_count":0,
                "last_reply_at":null,
                "participants":[]
            }),
        );
        assert_eq!(
            parse(&event, &relay.public_key().to_hex()).unwrap().summary,
            ThreadSummary::default()
        );
    }
}
