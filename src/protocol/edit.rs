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
}
