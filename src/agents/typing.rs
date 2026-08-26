use std::collections::HashMap;

use nostr::{Event, JsonUtil as _};
use uuid::Uuid;

use crate::protocol::events::verify;

pub const KIND_TYPING_INDICATOR: u16 = 20_002;
pub const TYPING_TTL_SECONDS: u64 = 8;
pub const POST_MESSAGE_SUPPRESSION_SECONDS: u64 = 2;
pub const MAX_TYPING_AGENTS: usize = 16;

const MAX_EVENT_BYTES: usize = 16 * 1024;
const MAX_CONTENT_BYTES: usize = 1024;
const MAX_TAGS: usize = 8;
const MAX_TAG_VALUES: usize = 5;
const MAX_TAG_VALUE_BYTES: usize = 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TypingScope {
    Channel,
    Thread(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTypingSignal {
    pub event_id: String,
    pub agent_pubkey: String,
    pub channel_id: Uuid,
    pub scope: TypingScope,
    pub source_created_at: u64,
    pub expires_at: u64,
}

/// Parse the public, signed, ephemeral typing envelope. Agent verification and
/// destination authority are deliberately applied by the caller from the
/// current community projection.
pub fn parse(event: &Event, selected_channel: Uuid, now: u64) -> Option<AgentTypingSignal> {
    if event.kind.as_u16() != KIND_TYPING_INDICATOR
        || event.as_json().len() > MAX_EVENT_BYTES
        || event.content.len() > MAX_CONTENT_BYTES
        || event.tags.len() > MAX_TAGS
        || event.tags.iter().any(|tag| {
            tag.as_slice().len() > MAX_TAG_VALUES
                || tag
                    .as_slice()
                    .iter()
                    .any(|value| value.len() > MAX_TAG_VALUE_BYTES)
        })
        || verify(event).is_err()
    {
        return None;
    }

    let h_tags = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("h"))
        .collect::<Vec<_>>();
    if h_tags.len() != 1 {
        return None;
    }
    let h = h_tags[0].as_slice();
    if h.len() != 2 || h.get(1).map(String::as_str) != Some(selected_channel.to_string().as_str()) {
        return None;
    }

    let scope = scope_from_tags(event)?;

    let source_created_at = event.created_at.as_secs();
    let source_expiry = source_created_at.saturating_add(TYPING_TTL_SECONDS);
    if source_expiry <= now {
        return None;
    }
    let expires_at = source_expiry.min(now.saturating_add(TYPING_TTL_SECONDS));
    Some(AgentTypingSignal {
        event_id: event.id.to_hex(),
        agent_pubkey: event.pubkey.to_hex(),
        channel_id: selected_channel,
        scope,
        source_created_at,
        expires_at,
    })
}

pub fn message_scope(event: &Event, channel_id: Uuid) -> Option<TypingScope> {
    if !matches!(event.kind.as_u16(), 9 | 40_002) {
        return None;
    }
    let h = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("h"))
        .collect::<Vec<_>>();
    if h.len() != 1
        || h[0].as_slice().len() != 2
        || h[0].as_slice().get(1).map(String::as_str) != Some(channel_id.to_string().as_str())
    {
        return None;
    }
    scope_from_tags(event)
}

fn scope_from_tags(event: &Event) -> Option<TypingScope> {
    let mut root = None;
    let mut reply = None;
    let mut event_tag_count = 0_usize;
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        if values.first().map(String::as_str) != Some("e") {
            continue;
        }
        event_tag_count = event_tag_count.saturating_add(1);
        if event_tag_count > 2
            || values.len() != 4
            || values.get(2).is_none_or(|relay| !relay.is_empty())
        {
            return None;
        }
        let id = values.get(1)?;
        if !canonical_event_id(id) {
            return None;
        }
        match values.get(3).map(String::as_str) {
            Some("root") if root.is_none() => root = Some(id.clone()),
            Some("reply") if reply.is_none() => reply = Some(id.clone()),
            _ => return None,
        }
    }
    if root.is_some() && reply.is_none() {
        return None;
    }
    match (root, reply) {
        (None, None) => Some(TypingScope::Channel),
        (root, Some(reply)) => Some(TypingScope::Thread(root.unwrap_or(reply))),
        (Some(_), None) => None,
    }
}

#[derive(Clone, Debug)]
struct TypingEntry {
    signal: AgentTypingSignal,
    first_seen_order: u64,
}

#[derive(Clone, Debug)]
struct Suppression {
    message_created_at: u64,
    until: u64,
    retain_until: u64,
}

#[derive(Clone, Debug, Default)]
pub struct AgentTypingState {
    entries: HashMap<(String, TypingScope), TypingEntry>,
    suppressions: HashMap<(String, TypingScope), Suppression>,
    next_order: u64,
}

impl AgentTypingState {
    /// Returns true only when visible membership changes. Refreshes extend the
    /// deadline without forcing a redraw.
    pub fn apply(&mut self, signal: AgentTypingSignal, now: u64) -> bool {
        self.prune_suppressions(now);
        let key = (signal.agent_pubkey.clone(), signal.scope.clone());
        if self.suppressions.get(&key).is_some_and(|suppression| {
            now < suppression.until || signal.source_created_at <= suppression.message_created_at
        }) {
            return false;
        }
        if let Some(current) = self.entries.get_mut(&key) {
            if (
                current.signal.source_created_at,
                current.signal.event_id.as_str(),
            ) >= (signal.source_created_at, signal.event_id.as_str())
            {
                return false;
            }
            current.signal = signal;
            return false;
        }
        if self.entries.len() >= MAX_TYPING_AGENTS {
            return false;
        }
        let order = self.next_order;
        self.next_order = self.next_order.wrapping_add(1);
        self.entries.insert(
            key,
            TypingEntry {
                signal,
                first_seen_order: order,
            },
        );
        true
    }

    pub fn observe_message(
        &mut self,
        author: &str,
        scope: TypingScope,
        message_created_at: u64,
        now: u64,
    ) -> bool {
        let key = (author.to_owned(), scope);
        let changed = self.entries.remove(&key).is_some();
        if self.suppressions.len() < MAX_TYPING_AGENTS || self.suppressions.contains_key(&key) {
            self.suppressions.insert(
                key,
                Suppression {
                    message_created_at,
                    until: now.saturating_add(POST_MESSAGE_SUPPRESSION_SECONDS),
                    retain_until: now.saturating_add(TYPING_TTL_SECONDS),
                },
            );
        }
        changed
    }

    pub fn expire(&mut self, now: u64) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| entry.signal.expires_at > now);
        self.prune_suppressions(now);
        before != self.entries.len()
    }

    pub fn retain_agents(&mut self, authorized: &std::collections::HashSet<String>) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|(author, _), _| authorized.contains(author));
        self.suppressions
            .retain(|(author, _), _| authorized.contains(author));
        before != self.entries.len()
    }

    pub fn active_pubkeys(&self, scope: &TypingScope, now: u64) -> Vec<&str> {
        let mut active = self
            .entries
            .values()
            .filter(|entry| &entry.signal.scope == scope && entry.signal.expires_at > now)
            .collect::<Vec<_>>();
        active.sort_by(|left, right| {
            (left.first_seen_order, left.signal.agent_pubkey.as_str())
                .cmp(&(right.first_seen_order, right.signal.agent_pubkey.as_str()))
        });
        active
            .into_iter()
            .map(|entry| entry.signal.agent_pubkey.as_str())
            .collect()
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.entries.is_empty();
        self.entries.clear();
        self.suppressions.clear();
        changed
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn prune_suppressions(&mut self, now: u64) {
        self.suppressions
            .retain(|_, suppression| suppression.retain_until > now);
    }
}

fn canonical_event_id(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use super::*;

    fn event(channel: Uuid, created_at: u64, tags: Vec<Tag>) -> Event {
        event_with_keys(channel, created_at, tags, &Keys::generate())
    }

    fn event_with_keys(channel: Uuid, created_at: u64, tags: Vec<Tag>, keys: &Keys) -> Event {
        EventBuilder::new(Kind::Custom(KIND_TYPING_INDICATOR), "ignored")
            .tags(std::iter::once(Tag::parse(["h", &channel.to_string()]).unwrap()).chain(tags))
            .custom_created_at(nostr::Timestamp::from_secs(created_at))
            .sign_with_keys(keys)
            .unwrap()
    }

    #[test]
    fn parses_fresh_channel_and_thread_signals() {
        let channel = Uuid::new_v4();
        let channel_event = event(channel, 100, vec![]);
        let parsed = parse(&channel_event, channel, 100).unwrap();
        assert_eq!(parsed.scope, TypingScope::Channel);
        assert_eq!(parsed.expires_at, 108);

        let root = "a".repeat(64);
        let parent = "b".repeat(64);
        let thread_event = event(
            channel,
            101,
            vec![
                Tag::parse(["e", &root, "", "root"]).unwrap(),
                Tag::parse(["e", &parent, "", "reply"]).unwrap(),
            ],
        );
        assert_eq!(
            parse(&thread_event, channel, 101).unwrap().scope,
            TypingScope::Thread(root)
        );
    }

    #[test]
    fn rejects_expired_wrong_channel_and_ambiguous_tags() {
        let channel = Uuid::new_v4();
        assert!(parse(&event(channel, 100, vec![]), channel, 108).is_none());
        assert!(parse(&event(channel, 100, vec![]), Uuid::new_v4(), 100).is_none());
        let root = "a".repeat(64);
        let malformed = event(
            channel,
            100,
            vec![Tag::parse(["e", &root, "", "root"]).unwrap()],
        );
        assert!(parse(&malformed, channel, 100).is_none());
    }

    #[test]
    fn future_timestamp_never_extends_local_visibility_past_ttl() {
        let channel = Uuid::new_v4();
        let parsed = parse(&event(channel, 10_000, vec![]), channel, 100).unwrap();
        assert_eq!(parsed.expires_at, 108);
    }

    #[test]
    fn message_scope_uses_the_same_exact_thread_coordinates() {
        let channel = Uuid::new_v4();
        let root = "a".repeat(64);
        let message = EventBuilder::new(Kind::Custom(9), "reply")
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["e", &root, "", "reply"]).unwrap(),
            ])
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(
            message_scope(&message, channel),
            Some(TypingScope::Thread(root))
        );
        assert!(message_scope(&message, Uuid::new_v4()).is_none());
    }

    #[test]
    fn reducer_refreshes_expires_and_clears_on_matching_message() {
        let channel = Uuid::new_v4();
        let keys = Keys::generate();
        let first = parse(&event_with_keys(channel, 100, vec![], &keys), channel, 100).unwrap();
        let author = first.agent_pubkey.clone();
        let mut state = AgentTypingState::default();
        assert!(state.apply(first, 100));
        let refresh = parse(&event_with_keys(channel, 103, vec![], &keys), channel, 103).unwrap();
        assert!(!state.apply(refresh, 103));
        assert_eq!(state.len(), 1);
        assert!(state.observe_message(&author, TypingScope::Channel, 104, 104));
        assert!(state.is_empty());
    }

    #[test]
    fn delayed_signal_cannot_resurrect_after_message() {
        let channel = Uuid::new_v4();
        let keys = Keys::generate();
        let signal = parse(&event_with_keys(channel, 100, vec![], &keys), channel, 100).unwrap();
        let author = signal.agent_pubkey.clone();
        let mut state = AgentTypingState::default();
        state.observe_message(&author, TypingScope::Channel, 101, 101);
        assert!(!state.apply(signal, 104));
        assert!(state.is_empty());

        let next_turn = parse(&event_with_keys(channel, 104, vec![], &keys), channel, 104).unwrap();
        assert!(state.apply(next_turn, 104));
    }

    #[test]
    fn expiry_changes_visibility_once() {
        let channel = Uuid::new_v4();
        let signal = parse(&event(channel, 100, vec![]), channel, 100).unwrap();
        let mut state = AgentTypingState::default();
        assert!(state.apply(signal, 100));
        assert!(!state.expire(107));
        assert!(state.expire(108));
        assert!(!state.expire(109));
    }
}
