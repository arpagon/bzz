//! Who may wake a local agent, and where the conversation happens.
//!
//! bzz decides this before anything reaches a harness (ADR v0.15 local
//! agents, decision 4). A request is admitted when it is addressed to the
//! agent, in a channel where the agent can answer, by an author who passes
//! the agent's rule. DMs are owner only, whatever the rule says.

use nostr::Event;
use uuid::Uuid;

use crate::{config::WakeRule, protocol::events};

/// The kind of channel a request arrived in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaceKind {
    Dm,
    /// A Solo chat: a private stream with the `buzz.solo` label.
    Solo,
    Channel,
}

/// What admission needs to know about a channel, from the relay's own
/// signed records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelFacts {
    pub id: Uuid,
    pub name: String,
    pub kind: PlaceKind,
    /// Public keys (hex) of the channel's members.
    pub members: Vec<String>,
}

/// The reply a `send` builds: the channel and, for a thread, its root and
/// the message answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyTarget {
    pub channel: Uuid,
    /// `(root, parent)`; `None` for a flat message in a DM or a Solo chat.
    pub thread: Option<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Admitted {
    pub request_id: String,
    pub channel: Uuid,
    pub place: String,
    pub author: String,
    pub created_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// Not a signed chat message of this channel.
    NotAMessage,
    /// The agent wrote it.
    OwnMessage,
    /// The agent is not a member of the channel, so it could not answer.
    NotAMember,
    /// Outside a DM, a request must mention the agent.
    NotMentioned,
    /// A DM with anyone besides the agent and its owner, or not from the owner.
    DmNotOwnerOnly,
    /// The author does not pass the agent's rule.
    AuthorNotAllowed,
}

impl Refusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAMessage => "not-a-message",
            Self::OwnMessage => "own-message",
            Self::NotAMember => "not-a-member",
            Self::NotMentioned => "not-mentioned",
            Self::DmNotOwnerOnly => "dm-not-owner-only",
            Self::AuthorNotAllowed => "author-not-allowed",
        }
    }
}

/// The agent as admission sees it.
#[derive(Clone, Copy, Debug)]
pub struct Rule<'a> {
    pub agent: &'a str,
    pub owner: &'a str,
    pub wake: WakeRule,
    pub allowlist: &'a [String],
}

fn same_key(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

impl Rule<'_> {
    fn author_passes(&self, author: &str) -> bool {
        same_key(author, self.owner)
            || match self.wake {
                WakeRule::OwnerOnly => false,
                WakeRule::Allowlist => self
                    .allowlist
                    .iter()
                    .any(|allowed| same_key(allowed, author)),
                WakeRule::Anyone => true,
            }
    }
}

/// The place of a request (`docs/agent-host-protocol.md`, `session/new`):
/// the channel for a DM or a Solo chat, the channel and thread root for a
/// thread. A top-level message in a channel starts its own thread.
pub fn place_of(event: &Event, kind: PlaceKind, channel: Uuid) -> String {
    match kind {
        PlaceKind::Dm | PlaceKind::Solo => channel.to_string(),
        PlaceKind::Channel => {
            let root = events::thread_coordinates(event)
                .0
                .unwrap_or_else(|| event.id.to_hex());
            format!("{channel}/{root}")
        }
    }
}

/// Where a reply to `request` goes, from the request itself and its place.
pub fn reply_target(request: &Event, place: &str, channel: Uuid) -> ReplyTarget {
    let request_id = request.id.to_hex();
    let root = events::thread_coordinates(request).0;
    let thread = if place.contains('/') {
        // A thread in a channel, or the thread a top-level mention starts.
        Some((root.unwrap_or_else(|| request_id.clone()), request_id))
    } else {
        // Flat in a DM or a Solo chat, unless the owner wrote in a thread.
        root.map(|root| (root, request_id))
    };
    ReplyTarget { channel, thread }
}

/// Decides whether `event` wakes the agent, and in which place.
pub fn admit(event: &Event, rule: Rule<'_>, channel: &ChannelFacts) -> Result<Admitted, Refusal> {
    if event.kind.as_u16() != 9
        || events::channel_id(event) != Some(channel.id)
        || events::verify(event).is_err()
    {
        return Err(Refusal::NotAMessage);
    }
    let author = event.pubkey.to_hex();
    if same_key(&author, rule.agent) {
        return Err(Refusal::OwnMessage);
    }
    if !channel
        .members
        .iter()
        .any(|member| same_key(member, rule.agent))
    {
        return Err(Refusal::NotAMember);
    }
    if channel.kind == PlaceKind::Dm {
        // The relay refuses an agent's message in a DM that holds anyone
        // besides its owner; there every message is for the agent.
        let only_the_two = channel.members.len() == 2
            && channel
                .members
                .iter()
                .any(|member| same_key(member, rule.owner));
        if !only_the_two || !same_key(&author, rule.owner) {
            return Err(Refusal::DmNotOwnerOnly);
        }
    } else {
        if !events::tag_values(event, "p")
            .iter()
            .any(|mentioned| same_key(mentioned, rule.agent))
        {
            return Err(Refusal::NotMentioned);
        }
        if !rule.author_passes(&author) {
            return Err(Refusal::AuthorNotAllowed);
        }
    }
    Ok(Admitted {
        request_id: event.id.to_hex(),
        channel: channel.id,
        place: place_of(event, channel.kind, channel.id),
        author,
        created_at: event.created_at.as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    struct World {
        owner: Keys,
        agent: Keys,
        other: Keys,
        channel: Uuid,
    }

    impl World {
        fn new() -> Self {
            Self {
                owner: Keys::generate(),
                agent: Keys::generate(),
                other: Keys::generate(),
                channel: Uuid::new_v4(),
            }
        }

        fn hex(keys: &Keys) -> String {
            keys.public_key().to_hex()
        }

        fn facts(&self, kind: PlaceKind, members: &[&Keys]) -> ChannelFacts {
            ChannelFacts {
                id: self.channel,
                name: "general".into(),
                kind,
                members: members.iter().map(|keys| Self::hex(keys)).collect(),
            }
        }

        fn message(
            &self,
            author: &Keys,
            mention: bool,
            thread: Option<&buzz_sdk::ThreadRef>,
        ) -> Event {
            let agent = Self::hex(&self.agent);
            let mentions = if mention {
                vec![agent.as_str()]
            } else {
                Vec::new()
            };
            buzz_sdk::build_message(self.channel, "hello", thread, &mentions, false, &[], &[])
                .unwrap()
                .sign_with_keys(author)
                .unwrap()
        }

        fn admit(
            &self,
            event: &Event,
            wake: WakeRule,
            allowlist: &[String],
            facts: &ChannelFacts,
        ) -> Result<Admitted, Refusal> {
            let (agent, owner) = (Self::hex(&self.agent), Self::hex(&self.owner));
            admit(
                event,
                Rule {
                    agent: &agent,
                    owner: &owner,
                    wake,
                    allowlist,
                },
                facts,
            )
        }
    }

    #[test]
    fn the_owner_always_wakes_and_others_only_by_the_rule() {
        let world = World::new();
        let facts = world.facts(
            PlaceKind::Channel,
            &[&world.owner, &world.agent, &world.other],
        );
        let from_owner = world.message(&world.owner, true, None);
        let from_other = world.message(&world.other, true, None);
        let listed = [World::hex(&world.other).to_ascii_uppercase()];
        for wake in [WakeRule::OwnerOnly, WakeRule::Allowlist, WakeRule::Anyone] {
            assert!(
                world.admit(&from_owner, wake, &[], &facts).is_ok(),
                "{wake:?}"
            );
        }
        assert_eq!(
            world.admit(&from_other, WakeRule::OwnerOnly, &listed, &facts),
            Err(Refusal::AuthorNotAllowed),
            "a list is ignored under owner-only"
        );
        assert_eq!(
            world.admit(&from_other, WakeRule::Allowlist, &[], &facts),
            Err(Refusal::AuthorNotAllowed)
        );
        assert!(
            world
                .admit(&from_other, WakeRule::Allowlist, &listed, &facts)
                .is_ok()
        );
        assert!(
            world
                .admit(&from_other, WakeRule::Anyone, &[], &facts)
                .is_ok()
        );
    }

    #[test]
    fn a_request_must_be_a_mention_in_a_channel_the_agent_can_answer_in() {
        let world = World::new();
        let facts = world.facts(PlaceKind::Channel, &[&world.owner, &world.agent]);
        assert_eq!(
            world.admit(
                &world.message(&world.owner, false, None),
                WakeRule::Anyone,
                &[],
                &facts
            ),
            Err(Refusal::NotMentioned)
        );
        // The agent never wakes itself, even when it mentions itself.
        assert_eq!(
            world.admit(
                &world.message(&world.agent, true, None),
                WakeRule::Anyone,
                &[],
                &facts
            ),
            Err(Refusal::OwnMessage)
        );
        let not_a_member = world.facts(PlaceKind::Channel, &[&world.owner]);
        assert_eq!(
            world.admit(
                &world.message(&world.owner, true, None),
                WakeRule::Anyone,
                &[],
                &not_a_member
            ),
            Err(Refusal::NotAMember)
        );
        // Another channel's facts, another kind, and a forged author.
        let mut elsewhere = facts.clone();
        elsewhere.id = Uuid::new_v4();
        let mention = world.message(&world.owner, true, None);
        assert_eq!(
            world.admit(&mention, WakeRule::Anyone, &[], &elsewhere),
            Err(Refusal::NotAMessage)
        );
        let reaction = buzz_sdk::build_reaction(mention.id, "👍")
            .unwrap()
            .sign_with_keys(&world.owner)
            .unwrap();
        assert_eq!(
            world.admit(&reaction, WakeRule::Anyone, &[], &facts),
            Err(Refusal::NotAMessage)
        );
        let mut forged = serde_json::to_value(world.message(&world.other, true, None)).unwrap();
        forged["pubkey"] = World::hex(&world.owner).into();
        let forged = serde_json::from_value::<Event>(forged).unwrap();
        assert_eq!(
            world.admit(&forged, WakeRule::OwnerOnly, &[], &facts),
            Err(Refusal::NotAMessage),
            "a message that claims the owner's key without its signature"
        );
    }

    #[test]
    fn a_dm_is_owner_only_whatever_the_rule_and_needs_no_mention() {
        let world = World::new();
        let dm = world.facts(PlaceKind::Dm, &[&world.owner, &world.agent]);
        let plain = world.message(&world.owner, false, None);
        let admitted = world.admit(&plain, WakeRule::OwnerOnly, &[], &dm).unwrap();
        assert_eq!(admitted.place, world.channel.to_string());
        // Someone else in a DM with the agent, under the widest rule.
        let listed = [World::hex(&world.other)];
        let theirs = world.facts(PlaceKind::Dm, &[&world.other, &world.agent]);
        for (wake, allowlist) in [
            (WakeRule::Anyone, &[][..]),
            (WakeRule::Allowlist, &listed[..]),
        ] {
            assert_eq!(
                world.admit(
                    &world.message(&world.other, true, None),
                    wake,
                    allowlist,
                    &theirs
                ),
                Err(Refusal::DmNotOwnerOnly)
            );
        }
        // A group DM: the relay would refuse the agent's answer there.
        let group = world.facts(PlaceKind::Dm, &[&world.owner, &world.agent, &world.other]);
        assert_eq!(
            world.admit(&plain, WakeRule::Anyone, &[], &group),
            Err(Refusal::DmNotOwnerOnly)
        );
    }

    #[test]
    fn the_place_is_the_channel_or_the_thread_and_replies_follow_it() {
        let world = World::new();
        let channel = world.facts(PlaceKind::Channel, &[&world.owner, &world.agent]);
        let top = world.message(&world.owner, true, None);
        let top_id = top.id.to_hex();
        let admitted = world
            .admit(&top, WakeRule::OwnerOnly, &[], &channel)
            .unwrap();
        assert_eq!(admitted.place, format!("{}/{top_id}", world.channel));
        assert_eq!(
            reply_target(&top, &admitted.place, world.channel).thread,
            Some((top_id.clone(), top_id.clone())),
            "a top-level mention starts a thread"
        );
        let thread = buzz_sdk::ThreadRef {
            root_event_id: top.id,
            parent_event_id: top.id,
        };
        let inside = world.message(&world.owner, true, Some(&thread));
        let admitted = world
            .admit(&inside, WakeRule::OwnerOnly, &[], &channel)
            .unwrap();
        assert_eq!(
            admitted.place,
            format!("{}/{top_id}", world.channel),
            "the same place"
        );
        assert_eq!(
            reply_target(&inside, &admitted.place, world.channel).thread,
            Some((top_id.clone(), inside.id.to_hex()))
        );
        // A Solo chat is one conversation; replies are flat unless threaded.
        let solo = world.facts(PlaceKind::Solo, &[&world.owner, &world.agent]);
        let admitted = world.admit(&top, WakeRule::OwnerOnly, &[], &solo).unwrap();
        assert_eq!(admitted.place, world.channel.to_string());
        assert_eq!(
            reply_target(&top, &admitted.place, world.channel).thread,
            None
        );
        assert_eq!(
            reply_target(&inside, &admitted.place, world.channel).thread,
            Some((top_id, inside.id.to_hex()))
        );
    }
}
