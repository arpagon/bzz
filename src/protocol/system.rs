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
                detail: None,
            }
        }
        "channel_created" => SystemEvent {
            kind: SystemEventKind::ChannelCreated,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
            detail: None,
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
            detail: None,
        },
        "channel_unarchived" => SystemEvent {
            kind: SystemEventKind::ChannelUnarchived,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
            detail: None,
        },
        // The relay names the removed event (`target_event_id`), never a
        // person; a moderator removal adds a sanitized `public_reason`.
        "message_deleted" => SystemEvent {
            kind: SystemEventKind::MessageDeleted,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
            detail: match object.get("public_reason") {
                None | Some(Value::Null) => None,
                Some(reason) => {
                    let reason = crate::render::sanitize::single_line(reason.as_str()?);
                    let reason = reason
                        .trim()
                        .chars()
                        .take(MAX_DETAIL_CHARS)
                        .collect::<String>();
                    (!reason.is_empty()).then_some(reason)
                }
            },
        },
        "topic_changed" | "purpose_changed" => {
            let field = if kind == "topic_changed" {
                "topic"
            } else {
                "purpose"
            };
            let text = object.get(field).map_or(Some(""), Value::as_str)?;
            let text = crate::render::sanitize::single_line(text);
            let text = text
                .trim()
                .chars()
                .take(MAX_DETAIL_CHARS)
                .collect::<String>();
            setting(
                if field == "topic" {
                    SystemEventKind::TopicChanged
                } else {
                    SystemEventKind::PurposeChanged
                },
                actor?,
                text,
            )
        }
        "visibility_changed" => match object.get("visibility")?.as_str()? {
            value @ ("open" | "private") => {
                setting(SystemEventKind::VisibilityChanged, actor?, value.to_owned())
            }
            _ => return None,
        },
        "join_role_changed" => match object.get("join_role")?.as_str()? {
            value @ ("guest" | "member") => {
                setting(SystemEventKind::JoinRoleChanged, actor?, value.to_owned())
            }
            _ => return None,
        },
        "unlisted_changed" => setting(
            SystemEventKind::UnlistedChanged,
            actor?,
            object.get("unlisted")?.as_bool()?.to_string(),
        ),
        "auto_join_changed" => setting(
            SystemEventKind::AutoJoinChanged,
            actor?,
            object.get("auto_join")?.as_bool()?.to_string(),
        ),
        "ttl_changed" => {
            let ttl = match object.get("ttl_seconds") {
                None | Some(Value::Null) => String::new(),
                Some(value) => value.as_u64().filter(|seconds| *seconds > 0)?.to_string(),
            };
            setting(SystemEventKind::TtlChanged, actor?, ttl)
        }
        "channel_deleted" => SystemEvent {
            kind: SystemEventKind::ChannelDeleted,
            actor: Some(actor?),
            target: None,
            participants: Vec::new(),
            detail: None,
        },
        "channel_auto_archived" => SystemEvent {
            kind: SystemEventKind::ChannelAutoArchived,
            actor: None,
            target: None,
            participants: Vec::new(),
            detail: None,
        },
        "admin_kick" => SystemEvent {
            kind: SystemEventKind::AdminKick,
            actor: None,
            target: Some(target?),
            participants: Vec::new(),
            detail: None,
        },
        _ => return None,
    };
    Some(event)
}

/// Topic and purpose are shown on one row; longer text is cut.
const MAX_DETAIL_CHARS: usize = 120;

fn setting(kind: SystemEventKind, actor: String, detail: String) -> SystemEvent {
    SystemEvent {
        kind,
        actor: Some(actor),
        target: None,
        participants: Vec::new(),
        detail: Some(detail),
    }
}

fn member_event(kind: SystemEventKind, actor: String, target: String) -> SystemEvent {
    SystemEvent {
        kind,
        actor: Some(actor),
        target: Some(target),
        participants: Vec::new(),
        detail: None,
    }
}

fn unsupported() -> SystemEvent {
    SystemEvent {
        kind: SystemEventKind::Unsupported,
        actor: None,
        target: None,
        participants: Vec::new(),
        detail: None,
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

    /// The pinned relay's exact `message_deleted` payloads (self-delete and
    /// moderator removal); production showed them as unsupported.
    #[test]
    fn message_deleted_matches_the_relay_payload() {
        use std::collections::HashMap;
        let profiles = HashMap::new();
        let owner = crate::domain::abbreviated_pubkey(OWNER);
        let event = "e".repeat(64);
        let plain =
            serde_json::json!({"type":"message_deleted","actor":OWNER,"target_event_id":event});
        assert_eq!(
            crate::ui::timeline::system_summary(&parse(&plain.to_string()), &profiles, None),
            format!("{owner} removed a message")
        );
        let moderated = serde_json::json!({
            "type":"message_deleted","actor":OWNER,"target_event_id":event,"public_reason":"spam\nlinks"
        });
        assert_eq!(
            crate::ui::timeline::system_summary(&parse(&moderated.to_string()), &profiles, None),
            "Removed by community moderators · spam links"
        );
        let no_actor = serde_json::json!({"type":"message_deleted","target_event_id":event});
        assert_eq!(
            parse(&no_actor.to_string()).kind,
            SystemEventKind::Unsupported
        );
    }

    /// Every channel-settings notice the pinned relay emits, rendered the
    /// way the desktop client words it; unknown values fail closed.
    #[test]
    fn upstream_settings_notices_parse_and_render() {
        use std::collections::HashMap;
        let profiles = HashMap::new();
        let render = |payload: serde_json::Value| {
            crate::ui::timeline::system_summary(&parse(&payload.to_string()), &profiles, None)
        };
        let owner = crate::domain::abbreviated_pubkey(OWNER);
        let agent = crate::domain::abbreviated_pubkey(AGENT);
        for (payload, expected) in [
            (
                serde_json::json!({"type":"join_role_changed","actor":OWNER,"join_role":"guest"}),
                format!("{owner} set new members to join as guests (read-only)"),
            ),
            (
                serde_json::json!({"type":"topic_changed","actor":OWNER,"topic":"Q4\nplans"}),
                format!("{owner} changed the topic to “Q4 plans”"),
            ),
            (
                serde_json::json!({"type":"purpose_changed","actor":OWNER,"purpose":"  "}),
                format!("{owner} cleared the purpose"),
            ),
            (
                serde_json::json!({"type":"visibility_changed","actor":OWNER,"visibility":"private"}),
                format!("{owner} made the channel private"),
            ),
            (
                serde_json::json!({"type":"unlisted_changed","actor":OWNER,"unlisted":true}),
                format!("{owner} unlisted the channel"),
            ),
            (
                serde_json::json!({"type":"auto_join_changed","actor":OWNER,"auto_join":false}),
                format!("{owner} turned auto-join off"),
            ),
            (
                serde_json::json!({"type":"ttl_changed","actor":OWNER,"ttl_seconds":604_800}),
                format!("{owner} set the channel to archive after 7 days of inactivity"),
            ),
            (
                serde_json::json!({"type":"ttl_changed","actor":OWNER,"ttl_seconds":null}),
                format!("{owner} made the channel permanent"),
            ),
            (
                serde_json::json!({"type":"channel_deleted","actor":OWNER}),
                format!("Channel deleted by {owner}"),
            ),
            (
                serde_json::json!({"type":"channel_auto_archived"}),
                "Channel archived after inactivity".to_owned(),
            ),
            (
                serde_json::json!({"type":"admin_kick","target":AGENT,"action_id":"x"}),
                format!("{agent} was removed by a relay administrator"),
            ),
        ] {
            assert_eq!(render(payload.clone()), expected, "{payload}");
        }
        let long = parse(
            &serde_json::json!({"type":"topic_changed","actor":OWNER,"topic":"x".repeat(500)})
                .to_string(),
        );
        assert_eq!(long.detail.unwrap().chars().count(), MAX_DETAIL_CHARS);
        for payload in [
            serde_json::json!({"type":"visibility_changed","actor":OWNER,"visibility":"public"}),
            serde_json::json!({"type":"join_role_changed","actor":OWNER,"join_role":"owner"}),
            serde_json::json!({"type":"unlisted_changed","actor":OWNER,"unlisted":"yes"}),
            serde_json::json!({"type":"ttl_changed","actor":OWNER,"ttl_seconds":-5}),
            serde_json::json!({"type":"topic_changed","topic":"no actor"}),
            serde_json::json!({"type":"admin_kick"}),
        ] {
            assert_eq!(
                parse(&payload.to_string()).kind,
                SystemEventKind::Unsupported,
                "{payload}"
            );
        }
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
