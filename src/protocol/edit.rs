//! Message edits (kind `40003`), following upstream
//! `docs/effective-message-edits.md` at the pinned revision.
//!
//! An edit replaces a message's text and its complete attachment set. The
//! target is the first valid 64-hex `e` tag in signed order, compared
//! case-insensitively. Among the live edits in the original's channel that
//! its author (or, for kinds 9 and 40002, the verified owner of the agent that
//! wrote it) made, the greatest `(created_at, id)` wins. An empty `imeta` set
//! on that edit means no attachments; it never falls back to the original.

use nostr::Event;

use crate::error::{Error, Result};

pub const KIND_EDIT: u16 = 40_003;

/// Original kinds whose latest live edit governs text and attachments
/// (`buzz_core::media::EDIT_GOVERNED_KINDS`). bzz renders 9 and 40002.
pub const GOVERNED_KINDS: [u16; 5] = buzz_core::media::EDIT_GOVERNED_KINDS;

/// The edit's target event ID, lowercase, or `None` when it has no valid one.
pub fn target(event: &Event) -> Option<String> {
    buzz_core::media::first_edit_target(event).map(hex::encode)
}

/// One stored edit of a message, as far as selection needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    pub event_id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub deleted: bool,
}

/// Whether `candidate` may edit an original by `author` of `kind`.
/// `agent_owner` is the verified owner of `author` when it is an agent.
pub fn by_author(
    candidate: &Candidate,
    author: &str,
    kind: u16,
    agent_owner: Option<&str>,
) -> bool {
    candidate.pubkey == author
        || (matches!(kind, 9 | 40_002)
            && agent_owner.is_some_and(|owner| candidate.pubkey == owner))
}

/// The edit that governs an original, if any. Channel scoping is the caller's.
pub fn latest<'a>(
    candidates: impl IntoIterator<Item = &'a Candidate>,
    author: &str,
    kind: u16,
    agent_owner: Option<&str>,
) -> Option<&'a Candidate> {
    if !GOVERNED_KINDS.contains(&kind) {
        return None;
    }
    candidates
        .into_iter()
        .filter(|candidate| !candidate.deleted && by_author(candidate, author, kind, agent_owner))
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.event_id.cmp(&b.event_id))
        })
}

/// The relay's view of a message before bzz edits it (`effective_edit` v1).
#[derive(Clone, Debug)]
pub struct Effective {
    pub original: Event,
    pub edit: Option<Event>,
}

/// The exact v1 lookup filter for `target` in `channel`.
pub fn effective_edit_filter(target: &str, channel: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "kinds": GOVERNED_KINDS,
        "ids": [target],
        "#h": [channel.to_string()],
        "effective_edit": 1,
        "consistency": "strong",
    })
}

/// Check a `POST /query` `effective_edit` v1 response for `target` in
/// `channel`. An older relay answers an ordinary array; bzz then refuses to
/// edit rather than drop the message's attachments.
pub fn parse_effective(bytes: &[u8], target: &str, channel: uuid::Uuid) -> Result<Effective> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| Error::Protocol(format!("invalid effective edit response: {error}")))?;
    if value.is_array() || value.get("effective_edit") != Some(&serde_json::json!(1)) {
        return Err(Error::Unsupported(
            "this relay cannot look up edits, so bzz does not edit messages here".into(),
        ));
    }
    let wrong = || Error::Protocol("the relay answered the edit lookup for another message".into());
    let original: Event = value
        .get("original")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or_else(wrong)?;
    crate::protocol::events::verify(&original)?;
    if original.id.to_hex() != target
        || !GOVERNED_KINDS.contains(&original.kind.as_u16())
        || crate::protocol::events::channel_id(&original) != Some(channel)
    {
        return Err(wrong());
    }
    let edit = match value.get("edit") {
        Some(serde_json::Value::Null) => None,
        Some(edit) => {
            let edit: Event = serde_json::from_value(edit.clone()).map_err(|_| wrong())?;
            crate::protocol::events::verify(&edit)?;
            if edit.kind.as_u16() != KIND_EDIT
                || self::target(&edit).as_deref() != Some(target)
                || crate::protocol::events::channel_id(&edit) != Some(channel)
            {
                return Err(wrong());
            }
            Some(edit)
        }
        None => return Err(wrong()),
    };
    Ok(Effective { original, edit })
}

/// When to stamp a new edit and which attachments it keeps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub created_at: u64,
    /// The governing event's complete `imeta` set, kept as signed.
    pub media: Vec<Vec<String>>,
    /// The governing event's text, for attachment markdown.
    pub content: String,
}

/// Stamp after the observed head, so a same-second edit still wins. A head
/// more than a minute ahead of the local clock is refused, as upstream does.
pub fn plan(effective: &Effective, now: u64) -> Result<Plan> {
    let head = effective.edit.as_ref().unwrap_or(&effective.original);
    let head_at = head.created_at.as_secs();
    if head_at > now.saturating_add(60) {
        return Err(Error::Protocol(
            "the latest edit is from the future; retry when the clock catches up".into(),
        ));
    }
    Ok(Plan {
        created_at: now.max(head_at.saturating_add(1)),
        media: head
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("imeta"))
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        content: head.content.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn edit(tags: Vec<Vec<&str>>) -> Event {
        let tags = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect::<Vec<_>>();
        EventBuilder::new(Kind::Custom(KIND_EDIT), "new")
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign")
    }

    fn candidate(id: char, pubkey: &str, created_at: u64) -> Candidate {
        Candidate {
            event_id: id.to_string().repeat(64),
            pubkey: pubkey.into(),
            created_at,
            deleted: false,
        }
    }

    #[test]
    fn target_is_the_first_valid_e_tag_lowercased() {
        let upper = "AB".repeat(32);
        let lower = "cd".repeat(32);
        let event = edit(vec![
            vec!["h", "00000000-0000-0000-0000-000000000000"],
            vec!["e", "not-hex"],
            vec!["e", &upper],
            vec!["e", &lower],
        ]);
        assert_eq!(target(&event), Some(upper.to_ascii_lowercase()));
        assert_eq!(target(&edit(vec![vec!["e", "short"]])), None);
        assert_eq!(target(&edit(vec![vec!["p", &lower]])), None);
    }

    #[test]
    fn latest_wins_by_created_at_then_greatest_id() {
        let author = "a".repeat(64);
        let older = candidate('1', &author, 10);
        let tie_low = candidate('2', &author, 20);
        let tie_high = candidate('3', &author, 20);
        let all = [older, tie_high.clone(), tie_low];
        assert_eq!(latest(&all, &author, 9, None), Some(&tie_high));
    }

    #[test]
    fn deleted_and_foreign_edits_do_not_apply() {
        let author = "a".repeat(64);
        let stranger = "b".repeat(64);
        let mut deleted = candidate('9', &author, 30);
        deleted.deleted = true;
        let foreign = candidate('8', &stranger, 40);
        let live = candidate('1', &author, 10);
        let all = [deleted, foreign, live.clone()];
        assert_eq!(latest(&all, &author, 9, None), Some(&live));
    }

    #[test]
    fn an_agents_verified_owner_may_edit_messages_only() {
        let agent = "a".repeat(64);
        let owner = "c".repeat(64);
        let by_owner = candidate('5', &owner, 10);
        let all = [by_owner.clone()];
        assert_eq!(latest(&all, &agent, 9, Some(&owner)), Some(&by_owner));
        assert_eq!(latest(&all, &agent, 40_002, Some(&owner)), Some(&by_owner));
        assert_eq!(latest(&all, &agent, 45_001, Some(&owner)), None);
        assert_eq!(latest(&all, &agent, 9, None), None);
    }

    #[test]
    fn ungoverned_kinds_are_never_edited() {
        let author = "a".repeat(64);
        let all = [candidate('1', &author, 10)];
        assert_eq!(latest(&all, &author, 40_099, None), None);
        assert_eq!(latest(&all, &author, 7, None), None);
    }

    fn signed(keys: &Keys, kind: u16, content: &str, tags: Vec<Vec<String>>, at: u64) -> Event {
        EventBuilder::new(Kind::Custom(kind), content)
            .tags(tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    }

    #[test]
    fn effective_responses_are_checked_and_old_relays_refused() {
        let keys = Keys::generate();
        let channel = uuid::Uuid::new_v4();
        let h = vec!["h".to_owned(), channel.to_string()];
        let imeta = vec!["imeta".to_owned(), "url https://x/media/a.png".into()];
        let original = signed(&keys, 9, "hi", vec![h.clone(), imeta.clone()], 100);
        let id = original.id.to_hex();
        let body = |edit: serde_json::Value| {
            serde_json::to_vec(
                &serde_json::json!({"effective_edit":1,"original":original,"edit":edit}),
            )
            .unwrap()
        };
        let unedited = parse_effective(&body(serde_json::Value::Null), &id, channel).unwrap();
        let plan_now = plan(&unedited, 500).unwrap();
        assert_eq!(plan_now.created_at, 500);
        assert_eq!(plan_now.media, vec![imeta]);

        let edit = signed(
            &keys,
            KIND_EDIT,
            "hey",
            vec![h.clone(), vec!["e".into(), id.clone()]],
            600,
        );
        let edited =
            parse_effective(&body(serde_json::to_value(&edit).unwrap()), &id, channel).unwrap();
        let same_second = plan(&edited, 600).unwrap();
        assert_eq!(same_second.created_at, 601, "after the observed head");
        assert!(same_second.media.is_empty(), "the edit's empty set is kept");
        assert!(
            plan(&edited, 539).is_err(),
            "a head over a minute ahead is refused"
        );

        let old = serde_json::to_vec(&serde_json::json!([original])).unwrap();
        assert!(matches!(
            parse_effective(&old, &id, channel),
            Err(Error::Unsupported(_))
        ));
        assert!(parse_effective(&body(serde_json::Value::Null), &"0".repeat(64), channel).is_err());
        assert!(
            parse_effective(&body(serde_json::Value::Null), &id, uuid::Uuid::new_v4()).is_err()
        );
        let stray = signed(
            &keys,
            KIND_EDIT,
            "x",
            vec![h, vec!["e".into(), "1".repeat(64)]],
            600,
        );
        assert!(
            parse_effective(&body(serde_json::to_value(&stray).unwrap()), &id, channel).is_err()
        );
        let missing =
            serde_json::to_vec(&serde_json::json!({"effective_edit":1,"original":original}))
                .unwrap();
        assert!(parse_effective(&missing, &id, channel).is_err());
    }
}
