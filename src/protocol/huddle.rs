//! Buzz huddles: a parent channel carries the relay-authenticated lifecycle
//! (48100–48103); a private 1-hour backing channel `E` carries guidelines
//! (48106), reactions (24810) and the audio socket `/huddle/E/audio`.
//! Rosters come from [`super::huddle_tracker`], the upstream CLI port.

use nostr::{Event, EventBuilder, Kind, Tag};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::{
        events::first_tag,
        huddle_tracker::{HuddleTracker, LifecycleEvent},
    },
};

pub const KIND_STARTED: u16 = 48_100;
pub const KIND_JOINED: u16 = 48_101;
pub const KIND_LEFT: u16 = 48_102;
pub const KIND_ENDED: u16 = 48_103;
pub const KIND_GUIDELINES: u16 = 48_106;
pub const KIND_REACTION: u16 = 24_810;
pub const LIFECYCLE_KINDS: [u16; 4] = [KIND_STARTED, KIND_JOINED, KIND_LEFT, KIND_ENDED];
/// A huddle with no end older than this is stale (upstream CLI rule).
pub const JOINABLE_WINDOW_SECONDS: u64 = 3_600;
const BACKING_TTL_SECONDS: i32 = 3_600;

fn tag(parts: &[&str]) -> Result<Tag> {
    Tag::parse(parts.iter().copied()).map_err(|error| Error::Protocol(error.to_string()))
}

fn sdk(error: buzz_sdk::SdkError) -> Error {
    Error::Protocol(error.to_string())
}

/// Step 1 of a start: the private backing channel `E` (kind 9007, ttl 1 h).
pub fn backing_channel(backing: Uuid) -> Result<EventBuilder> {
    let short = backing.simple().to_string();
    buzz_sdk::build_create_channel(
        backing,
        &format!("huddle-{}", &short[..8]),
        Some(buzz_sdk::Visibility::Private),
        Some(buzz_sdk::ChannelKind::Stream),
        None,
        Some(BACKING_TTL_SECONDS),
        false,
    )
    .map_err(sdk)
}

fn lifecycle(kind: u16, parent: Uuid, backing: Uuid) -> Result<EventBuilder> {
    Ok(EventBuilder::new(
        Kind::Custom(kind),
        json!({"ephemeral_channel_id": backing.to_string()}).to_string(),
    )
    .tags([tag(&["h", &parent.to_string()])?]))
}

/// 48100 in the parent, signed by the creator.
pub fn started(parent: Uuid, backing: Uuid) -> Result<EventBuilder> {
    lifecycle(KIND_STARTED, parent, backing)
}

/// 48103 in the parent; only the creator (or the relay) may end it.
pub fn ended(parent: Uuid, backing: Uuid) -> Result<EventBuilder> {
    lifecycle(KIND_ENDED, parent, backing)
}

/// Archive the backing channel after ending (kind 9002).
pub fn archive(backing: Uuid) -> Result<EventBuilder> {
    buzz_sdk::build_archive(backing).map_err(sdk)
}

/// Leave the backing channel when other humans remain (kind 9022).
pub fn leave(backing: Uuid) -> Result<EventBuilder> {
    buzz_sdk::build_leave(backing).map_err(sdk)
}

/// An ephemeral 24810 reaction in the backing channel.
pub fn reaction(backing: Uuid, emoji: &str, sender_name: &str) -> Result<EventBuilder> {
    if emoji.is_empty() || emoji.chars().count() > 64 {
        return Err(Error::Protocol(
            "a huddle reaction is 1 to 64 characters".into(),
        ));
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_REACTION), emoji).tags([
        tag(&["h", &backing.to_string()])?,
        tag(&["reaction", emoji])?,
        tag(&["sender_name", sender_name])?,
    ]))
}

/// `ws(s)://relay/huddle/E/audio` for the relay's websocket base.
pub fn audio_url(relay: &url::Url, backing: Uuid) -> Result<url::Url> {
    let mut url = relay.clone();
    url.set_path(&format!("/huddle/{backing}/audio"));
    url.set_query(None);
    Ok(url)
}

/// Lifecycle filter for a parent channel.
pub fn lifecycle_filter(parent: Uuid) -> Value {
    json!({"kinds": LIFECYCLE_KINDS, "#h": [parent.to_string()], "limit": 200})
}

/// The channel's current huddle, as replayed from its lifecycle events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveHuddle {
    pub backing: Uuid,
    pub creator: String,
    pub started_at: u64,
    /// Present participants, oldest admission first.
    pub participants: Vec<String>,
}

/// Replays `events` (any order) and returns the newest live, non-stale
/// huddle. Join/leave are trusted only from `relay`; ends only from the
/// relay or the creator (the tracker's rules).
pub fn active(events: &[Event], relay: &str, now: u64) -> Option<ActiveHuddle> {
    let parsed = events
        .iter()
        .filter(|event| LIFECYCLE_KINDS.contains(&event.kind.as_u16()))
        .filter(|event| crate::protocol::events::verify(event).is_ok())
        .filter_map(|event| serde_json::to_value(event).ok())
        .filter_map(|value| LifecycleEvent::parse(&value).ok())
        .collect::<Vec<_>>();
    let mut tracker = HuddleTracker::new(relay);
    tracker.apply_history(&parsed);
    tracker
        .sessions()
        .iter()
        .filter(|(_, session)| session.ended_at.is_none())
        .filter(|(_, session)| {
            now.saturating_sub(session.start_created_at) < JOINABLE_WINDOW_SECONDS
        })
        .filter_map(|(id, session)| {
            Some(ActiveHuddle {
                backing: Uuid::parse_str(id).ok()?,
                creator: session.start_creator.clone(),
                started_at: session.start_created_at,
                participants: session
                    .participants()
                    .into_iter()
                    .map(|(pubkey, _)| pubkey)
                    .collect(),
            })
        })
        .max_by(|left, right| {
            left.started_at
                .cmp(&right.started_at)
                .then_with(|| left.backing.cmp(&right.backing))
        })
}

/// A displayable 24810 reaction: `(sender pubkey, emoji)`.
pub fn parse_reaction(event: &Event, backing: Uuid) -> Option<(String, String)> {
    (event.kind.as_u16() == KIND_REACTION
        && first_tag(event, "h").as_deref() == Some(backing.to_string().as_str())
        && crate::protocol::events::verify(event).is_ok())
    .then(|| first_tag(event, "reaction"))
    .flatten()
    .filter(|emoji| !emoji.is_empty() && emoji.chars().count() <= 64)
    .map(|emoji| (event.pubkey.to_hex(), emoji))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::huddle_tracker::{HuddleTracker, LifecycleEvent};

    /// The upstream recorded sequences (also replayed by desktop's tracker).
    #[test]
    fn recorded_sequences_match_the_expected_rosters() {
        let sequences: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/huddle_sequences.json"))
                .unwrap();
        assert!(sequences.len() >= 10);
        for sequence in sequences {
            let name = sequence["name"].as_str().unwrap();
            let relay = sequence["relay_self"].as_str().unwrap();
            let huddle = sequence["huddle"].as_str().unwrap();
            let events = sequence["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| LifecycleEvent::parse(event).unwrap())
                .collect::<Vec<_>>();
            let mut tracker = HuddleTracker::new(relay);
            tracker.apply_history(&events);
            let session = tracker.sessions().get(huddle);
            let mut got = session
                .map(|session| {
                    session
                        .participants()
                        .into_iter()
                        .map(|(pubkey, _)| pubkey)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            got.sort();
            let want: Vec<String> =
                serde_json::from_value(sequence["expected_participants"].clone()).unwrap();
            assert_eq!(got, want, "{name}: roster");
            assert_eq!(
                session.is_some_and(|session| session.ended_at.is_some()),
                sequence["expected_ended"].as_bool().unwrap(),
                "{name}: ended"
            );
        }
    }

    #[test]
    fn active_trusts_the_relay_for_joins_and_the_creator_for_ends() {
        let relay = nostr::Keys::generate();
        let alice = nostr::Keys::generate();
        let mallory = nostr::Keys::generate();
        let parent = Uuid::new_v4();
        let backing = Uuid::new_v4();
        let now = nostr::Timestamp::now().as_secs();
        let sign =
            |builder: EventBuilder, keys: &nostr::Keys| builder.sign_with_keys(keys).unwrap();
        let join = |who: &nostr::Keys, signer: &nostr::Keys, admission: &str, revision: u64| {
            sign(
                EventBuilder::new(
                    Kind::Custom(KIND_JOINED),
                    json!({"ephemeral_channel_id": backing.to_string(), "roster_revision": revision, "admission_id": admission}).to_string(),
                )
                .tags([
                    tag(&["h", &parent.to_string()]).unwrap(),
                    tag(&["p", &who.public_key().to_hex()]).unwrap(),
                ]),
                signer,
            )
        };
        let mut events = vec![
            sign(started(parent, backing).unwrap(), &alice),
            join(&alice, &relay, "a1", 1),
            // A join not signed by the relay is ignored.
            join(&mallory, &mallory, "m1", 2),
        ];
        let relay_hex = relay.public_key().to_hex();
        let huddle = active(&events, &relay_hex, now).unwrap();
        assert_eq!(huddle.backing, backing);
        assert_eq!(huddle.participants, vec![alice.public_key().to_hex()]);
        // Only the creator (or the relay) can end it.
        events.push(sign(ended(parent, backing).unwrap(), &mallory));
        assert!(active(&events, &relay_hex, now).is_some());
        events.push(sign(ended(parent, backing).unwrap(), &alice));
        assert!(active(&events, &relay_hex, now).is_none());
        // Stale after an hour without an end.
        let fresh = vec![sign(started(parent, backing).unwrap(), &alice)];
        assert!(active(&fresh, &relay_hex, now + JOINABLE_WINDOW_SECONDS + 10).is_none());
        assert_eq!(
            audio_url(&url::Url::parse("ws://localhost:3030/").unwrap(), backing)
                .unwrap()
                .as_str(),
            format!("ws://localhost:3030/huddle/{backing}/audio")
        );
    }
}
