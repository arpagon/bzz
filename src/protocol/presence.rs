//! Presence (kind 20001), owner presence for agents (kind 20003) and NIP-38
//! user status (kind 30315), matching Buzz desktop and CLI semantics.
//!
//! Presence is ephemeral: the relay keeps it in Redis for 180 s and answers
//! `POST /query {"kinds":[20001],"authors":[...]}` with relay-signed synthetic
//! events (`p` = subject). Absence from a snapshot means offline. Live events
//! are signed by the subject itself (20001) or by the agent's owner (20003).

use std::collections::HashMap;

use nostr::Event;
use serde::{Deserialize, Serialize};

use crate::protocol::events::{first_tag, tag_values};

pub const KIND_PRESENCE: u16 = 20_001;
pub const KIND_AGENT_PRESENCE: u16 = 20_003;
pub const KIND_USER_STATUS: u16 = 30_315;
/// The relay's presence TTL; a live observation older than this is offline.
pub const PRESENCE_TTL_SECONDS: u64 = 180;
/// Publish cadence: three heartbeats per TTL window, as desktop does.
pub const HEARTBEAT_SECONDS: u64 = 60;
/// Inactivity before a published `online` becomes `away`.
pub const AWAY_AFTER_SECONDS: u64 = 300;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Presence {
    Online,
    Away,
    Offline,
}

impl Presence {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "online" => Some(Self::Online),
            "away" => Some(Self::Away),
            "offline" => Some(Self::Offline),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Away => "away",
            Self::Offline => "offline",
        }
    }

    /// Fixed one-cell marker for names.
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Online => "●",
            Self::Away => "◐",
            Self::Offline => "○",
        }
    }
}

/// One presence observation: subjects and their status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub subjects: Vec<String>,
    pub presence: Presence,
    pub observed_at: u64,
}

fn valid_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Parse a 20001 or 20003 event. `relay_pubkey` signs snapshot events;
/// `owner_of` returns the verified owner of an agent pubkey (for 20003).
pub fn observe(
    event: &Event,
    relay_pubkey: Option<&str>,
    owner_of: impl Fn(&str) -> Option<String>,
) -> Option<Observation> {
    let presence = Presence::parse(event.content.trim())?;
    let author = event.pubkey.to_hex();
    let subjects = match event.kind.as_u16() {
        KIND_PRESENCE => {
            let tagged = first_tag(event, "p").filter(|value| valid_pubkey(value));
            match tagged {
                // Relay snapshot: the relay names the subject.
                Some(subject) if Some(author.as_str()) == relay_pubkey => vec![subject],
                // A subject's own update: a p tag, if any, must be itself.
                Some(subject) if subject == author => vec![author],
                Some(_) => return None,
                None => vec![author],
            }
        }
        KIND_AGENT_PRESENCE => {
            // Only agents whose verified owner signed this event.
            let agents = tag_values(event, "p")
                .into_iter()
                .filter(|agent| valid_pubkey(agent) && agent != &author)
                .filter(|agent| owner_of(agent).as_deref() == Some(author.as_str()))
                .take(256)
                .collect::<Vec<_>>();
            if agents.is_empty() {
                return None;
            }
            agents
        }
        _ => return None,
    };
    Some(Observation {
        subjects,
        presence,
        observed_at: event.created_at.as_secs(),
    })
}

/// Bounded presence view with TTL staleness.
#[derive(Clone, Debug, Default)]
pub struct PresenceBook {
    entries: HashMap<String, (Presence, u64)>,
}

impl PresenceBook {
    pub const MAX_ENTRIES: usize = 10_000;

    pub fn apply(&mut self, observation: &Observation, received_at: u64) {
        for subject in &observation.subjects {
            if self.entries.len() >= Self::MAX_ENTRIES && !self.entries.contains_key(subject) {
                continue;
            }
            self.entries
                .insert(subject.clone(), (observation.presence, received_at));
        }
    }

    /// A snapshot is authoritative for the people it was asked about:
    /// anyone absent is offline.
    pub fn apply_snapshot(&mut self, asked: &[String], observations: &[Observation], now: u64) {
        for subject in asked {
            self.entries.remove(subject);
        }
        for observation in observations {
            self.apply(observation, now);
        }
    }

    pub fn get(&self, pubkey: &str, now: u64) -> Presence {
        match self.entries.get(pubkey) {
            Some((presence, at)) if now.saturating_sub(*at) < PRESENCE_TTL_SECONDS => *presence,
            _ => Presence::Offline,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// NIP-38 `d:general` status.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserStatus {
    pub text: String,
    pub emoji: String,
    pub expires_at: Option<i64>,
}

impl UserStatus {
    pub fn visible(&self, now: u64) -> bool {
        !(self.text.is_empty() && self.emoji.is_empty())
            && self
                .expires_at
                .is_none_or(|at| at > i64::try_from(now).unwrap_or(i64::MAX))
    }
}

/// Desktop's `parseInt` on `expiration`: optional sign, leading digits.
pub fn parse_expiration(value: &str) -> Option<i64> {
    let value = value.trim_start();
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    let digits = unsigned
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(unsigned.len());
    let end = value.len() - unsigned.len() + digits;
    value[..end].parse().ok()
}

/// Parse a kind 30315 `d:general` event. Bounded; emoji is one short token.
pub fn status(event: &Event) -> Option<UserStatus> {
    if event.kind.as_u16() != KIND_USER_STATUS
        || first_tag(event, "d").as_deref() != Some("general")
    {
        return None;
    }
    let text = event.content.trim().chars().take(140).collect::<String>();
    let emoji = first_tag(event, "emoji")
        .map(|value| value.trim().chars().take(16).collect::<String>())
        .unwrap_or_default();
    Some(UserStatus {
        text,
        emoji,
        expires_at: first_tag(event, "expiration").and_then(|value| parse_expiration(&value)),
    })
}

/// `--expires` input: a unix timestamp, or a duration `N[s|m|h|d|w]`.
pub fn parse_expiry(input: &str, now: u64) -> Option<u64> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.bytes().all(|byte| byte.is_ascii_digit()) {
        return input.parse().ok();
    }
    let (number, unit) = input.split_at(input.len() - 1);
    let number: u64 = number.parse().ok().filter(|value| *value > 0)?;
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    now.checked_add(number.checked_mul(scale)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn event(keys: &Keys, kind: u16, content: &str, tags: &[&[&str]], at: u64) -> Event {
        EventBuilder::new(Kind::Custom(kind), content)
            .tags(
                tags.iter()
                    .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
            )
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    }

    #[test]
    fn presence_subjects_follow_the_signer_rules() {
        let relay = Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let user = Keys::generate();
        let user_hex = user.public_key().to_hex();
        let other = "c".repeat(64);
        let none = |_: &str| None;
        let own = observe(
            &event(&user, 20_001, "online", &[], 10),
            Some(&relay_hex),
            none,
        )
        .unwrap();
        assert_eq!(own.subjects, vec![user_hex.clone()]);
        let snapshot = observe(
            &event(&relay, 20_001, "away", &[&["p", &other]], 10),
            Some(&relay_hex),
            none,
        )
        .unwrap();
        assert_eq!(
            (snapshot.subjects, snapshot.presence),
            (vec![other.clone()], Presence::Away)
        );
        assert!(
            observe(
                &event(&user, 20_001, "online", &[&["p", &other]], 10),
                Some(&relay_hex),
                none
            )
            .is_none(),
            "a user cannot speak for someone else"
        );
        assert!(observe(&event(&user, 20_001, "busy", &[], 10), None, none).is_none());

        let owner = Keys::generate();
        let owner_hex = owner.public_key().to_hex();
        let agent = "a".repeat(64);
        let stranger = "b".repeat(64);
        let owners = |pubkey: &str| (pubkey == agent).then(|| owner_hex.clone());
        let agents = observe(
            &event(
                &owner,
                20_003,
                "online",
                &[&["p", &agent], &["p", &stranger]],
                10,
            ),
            Some(&relay_hex),
            owners,
        )
        .unwrap();
        assert_eq!(
            agents.subjects,
            vec![agent.clone()],
            "only verified-owned agents"
        );
        let impostor = observe(
            &event(&Keys::generate(), 20_003, "online", &[&["p", &agent]], 10),
            Some(&relay_hex),
            |pubkey: &str| (pubkey == agent).then(|| owner_hex.clone()),
        );
        assert!(impostor.is_none());
    }

    #[test]
    fn presence_expires_after_the_ttl_and_snapshots_are_authoritative() {
        let mut book = PresenceBook::default();
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        book.apply(
            &Observation {
                subjects: vec![a.clone(), b.clone()],
                presence: Presence::Online,
                observed_at: 0,
            },
            1_000,
        );
        assert_eq!(
            book.get(&a, 1_000 + PRESENCE_TTL_SECONDS - 1),
            Presence::Online
        );
        assert_eq!(
            book.get(&a, 1_000 + PRESENCE_TTL_SECONDS),
            Presence::Offline
        );
        book.apply_snapshot(std::slice::from_ref(&a), &[], 1_010);
        assert_eq!(
            book.get(&a, 1_010),
            Presence::Offline,
            "absent from snapshot"
        );
        assert_eq!(
            book.get(&b, 1_010),
            Presence::Online,
            "not asked, unchanged"
        );
    }

    #[test]
    fn status_expiry_matches_desktop_parse_int() {
        assert_eq!(parse_expiration("1790000000"), Some(1_790_000_000));
        assert_eq!(parse_expiration(" +12abc"), Some(12));
        assert_eq!(parse_expiration("-5"), Some(-5));
        assert_eq!(parse_expiration("abc"), None);
        let keys = Keys::generate();
        let live = status(&event(
            &keys,
            30_315,
            " lunch ",
            &[&["d", "general"], &["emoji", "🍜"], &["expiration", "200"]],
            1,
        ))
        .unwrap();
        assert_eq!(live.text, "lunch");
        assert!(live.visible(199));
        assert!(!live.visible(200), "expiry is exclusive");
        let clear = status(&event(&keys, 30_315, "", &[&["d", "general"]], 1)).unwrap();
        assert!(!clear.visible(0));
        assert!(status(&event(&keys, 30_315, "x", &[&["d", "music"]], 1)).is_none());
    }

    #[test]
    fn expiry_input_accepts_timestamps_and_durations() {
        assert_eq!(parse_expiry("1790000000", 5), Some(1_790_000_000));
        assert_eq!(parse_expiry("30m", 100), Some(1_900));
        assert_eq!(parse_expiry("2h", 0), Some(7_200));
        assert_eq!(parse_expiry("1w", 0), Some(604_800));
        assert_eq!(parse_expiry("0h", 0), None);
        assert_eq!(parse_expiry("5y", 0), None);
    }
}
