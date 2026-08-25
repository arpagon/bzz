use std::collections::BTreeSet;

use serde_json::Value;

use crate::domain::{SystemEvent, SystemEventKind};

/// Relay-authored system payloads are control-plane presentation data, never
/// markdown. Keep parsing substantially below the ordinary message bound.
pub const MAX_SYSTEM_EVENT_BYTES: usize = 16 * 1024;

pub fn parse(content: &str) -> SystemEvent {
    parse_known(content).unwrap_or_else(unsupported)
}

fn parse_known(content: &str) -> Option<SystemEvent> {
    if content.len() > MAX_SYSTEM_EVENT_BYTES {
        return None;
    }
    let object = serde_json::from_str::<Value>(content).ok()?;
    let object = object.as_object()?;
    let kind = object.get("type")?.as_str()?;
    let actor = object
        .get("actor")
        .and_then(Value::as_str)
        .filter(|value| valid_pubkey(value))
        .map(str::to_owned);
    let target = object
        .get("target")
        .and_then(Value::as_str)
        .filter(|value| valid_pubkey(value))
        .map(str::to_owned);

    let event = match kind {
        "dm_created" => {
            let actor = actor?;
            let participants = object
                .get("participants")?
                .as_array()?
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()?;
            if !(2..=9).contains(&participants.len())
                || participants.iter().any(|value| !valid_pubkey(value))
                || participants.iter().copied().collect::<BTreeSet<_>>().len() != participants.len()
                || !participants.contains(&actor.as_str())
            {
                return None;
            }
            SystemEvent {
                kind: SystemEventKind::DmCreated,
                actor: Some(actor),
                target: None,
                participants: participants.into_iter().map(str::to_owned).collect(),
            }
        }
        "channel_created" => SystemEvent {
            kind: SystemEventKind::ChannelCreated,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
        },
        "member_joined" => member_event(SystemEventKind::MemberJoined, actor?, target?),
        "member_left" => {
            let actor = actor?;
            member_event(
                SystemEventKind::MemberLeft,
                actor.clone(),
                target.unwrap_or(actor),
            )
        }
        "member_removed" => member_event(SystemEventKind::MemberRemoved, actor?, target?),
        "channel_archived" => SystemEvent {
            kind: SystemEventKind::ChannelArchived,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
        },
        "channel_unarchived" => SystemEvent {
            kind: SystemEventKind::ChannelUnarchived,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
        },
        "message_deleted" => member_event(SystemEventKind::MessageDeleted, actor?, target?),
        _ => return None,
    };
    Some(event)
}

fn member_event(kind: SystemEventKind, actor: String, target: String) -> SystemEvent {
    SystemEvent {
        kind,
        actor: Some(actor),
        target: Some(target),
        participants: Vec::new(),
    }
}

fn unsupported() -> SystemEvent {
    SystemEvent {
        kind: SystemEventKind::Unsupported,
        actor: None,
        target: None,
        participants: Vec::new(),
    }
}

fn valid_pubkey(value: &str) -> bool {
    value.len() == 64
        && value == value.to_ascii_lowercase()
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AGENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn dm_created_requires_a_bounded_exact_participant_set() {
        let event = parse(
            &serde_json::json!({
                "type":"dm_created", "actor":OWNER, "participants":[OWNER,AGENT]
            })
            .to_string(),
        );
        assert_eq!(event.kind, SystemEventKind::DmCreated);
        assert_eq!(event.participants, vec![OWNER, AGENT]);

        let duplicate = parse(
            &serde_json::json!({
                "type":"dm_created", "actor":OWNER, "participants":[OWNER,OWNER]
            })
            .to_string(),
        );
        assert_eq!(duplicate.kind, SystemEventKind::Unsupported);
    }

    #[test]
    fn malformed_unknown_and_oversized_payloads_are_content_free() {
        for content in [
            "not json".to_owned(),
            r#"{"type":"future","private":"must not render"}"#.to_owned(),
            format!(
                r#"{{"type":"channel_created","actor":"{OWNER}","padding":"{}"}}"#,
                "x".repeat(MAX_SYSTEM_EVENT_BYTES)
            ),
        ] {
            let event = parse(&content);
            assert_eq!(event.kind, SystemEventKind::Unsupported);
            assert!(event.actor.is_none());
            assert!(event.target.is_none());
            assert!(event.participants.is_empty());
        }
    }
}
