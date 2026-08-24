use nostr::Event;
use uuid::Uuid;

use crate::{
    domain::{Channel, ChannelKind, Message, Profile, Reaction, Visibility},
    error::{Error, Result},
};

pub fn verify(event: &Event) -> Result<()> {
    buzz_core::verify_event(event)
        .map_err(|error| Error::Protocol(format!("event verification failed: {error}")))
}

pub fn tag_values(event: &Event, name: &str) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some(name))
                .then(|| values.get(1).cloned())
                .flatten()
        })
        .collect()
}

pub fn first_tag(event: &Event, name: &str) -> Option<String> {
    tag_values(event, name).into_iter().next()
}

pub fn channel_id(event: &Event) -> Option<Uuid> {
    first_tag(event, "h").and_then(|value| Uuid::parse_str(&value).ok())
}

pub fn event_references(event: &Event) -> Vec<String> {
    tag_values(event, "e")
}

pub fn thread_coordinates(event: &Event) -> (Option<String>, Option<String>) {
    let mut root = None;
    let mut parent = None;
    let mut unmarked = Vec::new();
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        if values.first().map(String::as_str) != Some("e") {
            continue;
        }
        let Some(id) = values.get(1) else { continue };
        match values.get(3).map(String::as_str) {
            Some("root") => root = Some(id.clone()),
            Some("reply") => parent = Some(id.clone()),
            _ => unmarked.push(id.clone()),
        }
    }
    if root.is_none() && parent.is_some() {
        root.clone_from(&parent);
    }
    if root.is_none() && !unmarked.is_empty() {
        root = unmarked.first().cloned();
        parent = unmarked.last().cloned();
    }
    (root, parent)
}

pub fn as_message(event: &Event) -> Option<Message> {
    if !matches!(event.kind.as_u16(), 9 | 40_002 | 40_099) {
        return None;
    }
    let channel_id = channel_id(event)?;
    let (root_event_id, parent_event_id) = thread_coordinates(event);
    Some(Message {
        event_id: event.id.to_hex(),
        channel_id,
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        content: event.content.clone(),
        attachments: Vec::new(),
        root_event_id,
        parent_event_id,
        deleted: false,
        delivery: crate::domain::DeliveryState::Delivered,
    })
}

pub fn as_reaction(event: &Event) -> Option<Reaction> {
    if event.kind.as_u16() != 7 {
        return None;
    }
    let target_event_id = event_references(event).first()?.clone();
    Some(Reaction {
        event_id: event.id.to_hex(),
        target_event_id,
        pubkey: event.pubkey.to_hex(),
        emoji: event.content.clone(),
        created_at: event.created_at.as_secs(),
        deleted: false,
    })
}

pub fn as_profile(event: &Event) -> Option<Profile> {
    if event.kind.as_u16() != 0 {
        return None;
    }
    let metadata: serde_json::Value = serde_json::from_str(&event.content).ok()?;
    let string = |name: &str| {
        metadata
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    Some(Profile {
        pubkey: event.pubkey.to_hex(),
        display_name: string("display_name"),
        name: string("name"),
        picture: string("picture"),
        nip05: string("nip05"),
        about: string("about"),
        event_id: event.id.to_hex(),
        created_at: event.created_at.as_secs(),
    })
}

pub fn as_channel(event: &Event) -> Option<Channel> {
    if event.kind.as_u16() != 39_000 {
        return None;
    }
    let id = first_tag(event, "d").and_then(|value| Uuid::parse_str(&value).ok())?;
    let visibility = if first_tag(event, "private").is_some() {
        Visibility::Private
    } else {
        Visibility::Public
    };
    let kind =
        first_tag(event, "t").map_or(ChannelKind::Stream, |value| ChannelKind::parse(&value));
    Some(Channel {
        id,
        name: first_tag(event, "name").unwrap_or_else(|| id.to_string()),
        about: first_tag(event, "about").unwrap_or_default(),
        kind,
        visibility,
        is_member: false,
        // NIP-29's bare `hidden` tag classifies workspace DMs and is not
        // viewer-specific visibility. Kind 30622 owns the latter.
        is_hidden: false,
        member_count: 0,
        last_event_at: None,
    })
}
