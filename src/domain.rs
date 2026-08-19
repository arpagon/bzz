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
    Other(String),
}

impl ChannelKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "stream" => Self::Stream,
            "forum" => Self::Forum,
            "dm" => Self::Dm,
            other => Self::Other(other.to_owned()),
        }
    }

    pub const fn is_dm(&self) -> bool {
        matches!(self, Self::Dm)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Stream => "stream",
            Self::Forum => "forum",
            Self::Dm => "dm",
            Self::Other(value) => value,
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
}

impl Profile {
    pub fn label(&self) -> String {
        self.display_name
            .as_ref()
            .or(self.name.as_ref())
            .cloned()
            .unwrap_or_else(|| abbreviated_pubkey(&self.pubkey))
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
    pub pending: bool,
    pub rejected: Option<String>,
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
