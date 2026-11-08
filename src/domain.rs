use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Community {
    pub id: Uuid,
    pub label: String,
    pub relay_url: String,
    pub identity_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChannelKind {
    Stream,
    Forum,
    Dm,
    Workflow,
    /// NIP-CL access-control channel (agent config and similar).
    System,
    /// A type this bzz does not know. Always read-only.
    Other(String),
}

impl ChannelKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "stream" => Self::Stream,
            "forum" => Self::Forum,
            "dm" => Self::Dm,
            "workflow" => Self::Workflow,
            "system" => Self::System,
            other => Self::Other(other.to_owned()),
        }
    }

    pub const fn is_dm(&self) -> bool {
        matches!(self, Self::Dm)
    }

    pub const fn is_system(&self) -> bool {
        matches!(self, Self::System)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Stream => "stream",
            Self::Forum => "forum",
            Self::Dm => "dm",
            Self::Workflow => "workflow",
            Self::System => "system",
            Self::Other(value) => value,
        }
    }
}

/// NIP-29 roles as Buzz publishes them in kind 39002 `["p", pubkey, "", role]`.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub enum MemberRole {
    Owner,
    Admin,
    #[default]
    Member,
    /// Reads and leaves; cannot write (announce channels).
    Guest,
    Bot,
    /// A role this bzz does not know. Fails closed: cannot write.
    Unknown,
}

impl MemberRole {
    /// Parse the fourth `p`-tag field. An absent field is the NIP-29 default
    /// member role; anything unrecognized is `Unknown`.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            None | Some("" | "member") => Self::Member,
            Some("owner") => Self::Owner,
            Some("admin") => Self::Admin,
            Some("guest") => Self::Guest,
            Some("bot") => Self::Bot,
            Some(_) => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Guest => "guest",
            Self::Bot => "bot",
            Self::Unknown => "unknown",
        }
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Member | Self::Bot)
    }
}

/// The role a new member gets in an open channel (block/buzz#8143).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum JoinRole {
    #[default]
    Member,
    Guest,
}

impl JoinRole {
    /// Unknown values fail closed to `Guest`, exactly like the relay.
    pub fn parse(value: &str) -> Self {
        if value == "member" {
            Self::Member
        } else {
            Self::Guest
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Guest => "guest",
        }
    }
}

/// NIP-CL identity and Buzz channel settings carried by relay-signed 39000,
/// plus the viewer's own role from 39002. A missing tag means unknown.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ChannelFacts {
    pub labels: Vec<String>,
    pub creator: Option<String>,
    pub created_at: Option<u64>,
    pub join_role: JoinRole,
    pub unlisted: bool,
    pub archived: bool,
    pub auto_join: bool,
    pub viewer_role: Option<MemberRole>,
}

/// Why the composer, reactions and replies are unavailable in a channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadOnlyReason {
    UnknownType,
    SystemChannel,
    Guest,
    UnknownRole,
    Archived,
}

impl ReadOnlyReason {
    /// Fixed one-line notice; never includes relay-provided text.
    pub const fn notice(self) -> &'static str {
        match self {
            Self::UnknownType => "read-only · channel type not supported by this bzz",
            Self::SystemChannel => "read-only · system channel",
            Self::Guest => "read-only · announce channel (guest)",
            Self::UnknownRole => "read-only · unknown channel role",
            Self::Archived => "read-only · archived channel",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Channel {
    pub id: Uuid,
    pub name: String,
    pub about: String,
    pub kind: ChannelKind,
    pub visibility: Visibility,
    pub is_member: bool,
    pub is_hidden: bool,
    pub member_count: u32,
    pub last_event_at: Option<u64>,
    pub facts: ChannelFacts,
}

impl Channel {
    /// Local write gate for composer, reactions and replies. The relay stays
    /// the authority; this only avoids offering actions it will refuse.
    pub fn read_only_reason(&self) -> Option<ReadOnlyReason> {
        match self.kind {
            ChannelKind::Other(_) => return Some(ReadOnlyReason::UnknownType),
            ChannelKind::System => return Some(ReadOnlyReason::SystemChannel),
            _ => {}
        }
        if self.facts.archived {
            return Some(ReadOnlyReason::Archived);
        }
        match self.facts.viewer_role {
            Some(MemberRole::Guest) => Some(ReadOnlyReason::Guest),
            Some(MemberRole::Unknown) => Some(ReadOnlyReason::UnknownRole),
            // Not yet a member of an announce channel: joining makes a guest.
            None if self.facts.join_role == JoinRole::Guest
                && matches!(self.visibility, Visibility::Public) =>
            {
                Some(ReadOnlyReason::Guest)
            }
            _ => None,
        }
    }

    /// Whether browse and finder may list this channel. Unlisted open channels
    /// are named-only for non-members (NIP-CL); they stay reachable by link.
    pub fn listed_for_viewer(&self) -> bool {
        self.is_member || !self.facts.unlisted
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Profile {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub event_id: String,
    pub created_at: u64,
    /// Relay-signed NIP-FI verified name (kind 30382), when current.
    #[serde(default)]
    pub verified: Option<crate::protocol::verified_profile::VerifiedName>,
}

impl Profile {
    pub fn label(&self) -> String {
        self.display_name
            .as_ref()
            .or(self.name.as_ref())
            .or(self
                .verified
                .as_ref()
                .map(|verified| &verified.display_name))
            .cloned()
            .unwrap_or_else(|| abbreviated_pubkey(&self.pubkey))
    }

    /// A person known only through a verified profile.
    pub fn verified_only(
        pubkey: &str,
        name: crate::protocol::verified_profile::VerifiedName,
        event_id: String,
        created_at: u64,
    ) -> Self {
        Self {
            pubkey: pubkey.to_owned(),
            display_name: None,
            name: None,
            picture: None,
            nip05: None,
            about: None,
            event_id,
            created_at,
            verified: Some(name),
        }
    }

    /// Author label with a verified marker for NIP-FI verified people.
    pub fn author_label(&self) -> String {
        match &self.verified {
            Some(_) => format!("{} ✓", self.label()),
            None => self.label(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DraftMention {
    pub byte_start: usize,
    pub byte_end: usize,
    pub pubkey: String,
}

impl DraftMention {
    pub fn valid_for(&self, body: &str) -> bool {
        self.byte_start < self.byte_end
            && self.byte_end <= body.len()
            && body.is_char_boundary(self.byte_start)
            && body.is_char_boundary(self.byte_end)
            && body[self.byte_start..self.byte_end].starts_with('@')
            && self.pubkey.len() == 64
            && self.pubkey.bytes().all(|byte| {
                byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
            })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MentionCandidate {
    pub pubkey: String,
    pub label: String,
    pub is_agent: bool,
    pub agent_eligibility: Option<crate::agents::Eligibility>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// No outbox marker remains: relay acceptance or observation is authoritative.
    #[default]
    Delivered,
    /// Signed and durably queued, without a completed publication outcome.
    Pending,
    /// Publication may have succeeded; reconciliation is required before retry.
    Unknown,
    /// The relay gave a definitive negative acknowledgement.
    Rejected,
}

impl DeliveryState {
    pub fn from_outbox(value: Option<&str>) -> Self {
        match value {
            Some("pending") => Self::Pending,
            Some("unknown") => Self::Unknown,
            Some("rejected") => Self::Rejected,
            Some("delivered") | None | Some(_) => Self::Delivered,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemEventKind {
    DmCreated,
    ChannelCreated,
    MemberJoined,
    MemberLeft,
    MemberRemoved,
    ChannelArchived,
    ChannelUnarchived,
    MessageDeleted,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SystemEvent {
    pub kind: SystemEventKind,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub participants: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    pub event_id: String,
    pub channel_id: Uuid,
    pub pubkey: String,
    pub created_at: u64,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<crate::media::Attachment>,
    pub root_event_id: Option<String>,
    pub parent_event_id: Option<String>,
    pub deleted: bool,
    #[serde(default)]
    pub delivery: DeliveryState,
    #[serde(default)]
    pub system: Option<SystemEvent>,
    /// Desktop "Send to channel" provenance (`buzz:sent-from-thread`).
    #[serde(skip)]
    pub sent_from_thread: Option<crate::protocol::sent_from_thread::SentFromThread>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadSummary {
    pub descendant_count: u32,
    pub last_reply_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Reaction {
    pub event_id: String,
    pub target_event_id: String,
    pub pubkey: String,
    pub emoji: String,
    pub created_at: u64,
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadState {
    pub client_id: String,
    pub contexts: BTreeMap<String, u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum InboxCategory {
    Mention,
    Thread,
    Dm,
    NeedsAction,
    Draft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxItem {
    pub conversation_id: String,
    pub categories: Vec<InboxCategory>,
    pub event_id: Option<String>,
    pub channel_id: Option<Uuid>,
    pub thread_root: Option<String>,
    pub sender_pubkey: Option<String>,
    pub created_at: u64,
    pub preview: String,
    pub unread_count: u32,
    pub first_unread_event_id: Option<String>,
    pub first_unread_at: Option<u64>,
    pub draft_count: u32,
    pub latest_draft_at: Option<u64>,
    pub forced_unread: bool,
}

impl InboxItem {
    pub const fn unread(&self) -> bool {
        self.forced_unread || self.unread_count > 0
    }

    pub fn draft_only(&self) -> bool {
        self.categories.contains(&InboxCategory::Draft)
            && self
                .categories
                .iter()
                .all(|category| *category == InboxCategory::Draft)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxCursor {
    pub latest_activity_at: u64,
    pub conversation_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxPage {
    pub items: Vec<InboxItem>,
    pub next_cursor: Option<InboxCursor>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum SearchResultKind {
    Channel,
    Dm,
    Person,
    Message,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchResult {
    pub stable_id: String,
    pub kind: SearchResultKind,
    pub label: String,
    pub detail: String,
    pub channel_id: Option<Uuid>,
    pub event_id: Option<String>,
    pub thread_root: Option<String>,
    pub pubkey: Option<String>,
    pub created_at: u64,
    pub remote_rank: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Locked,
    IdentityMissing,
    IdentityCorrupt,
    Offline,
    Connecting,
    Authenticating,
    Online,
    Backfilling,
    AccessDenied,
    ClockSkew,
}

pub fn abbreviated_pubkey(pubkey: &str) -> String {
    if pubkey.len() <= 12 {
        pubkey.to_owned()
    } else {
        format!("{}…{}", &pubkey[..8], &pubkey[pubkey.len() - 4..])
    }
}
