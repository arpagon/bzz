use nostr::Event;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxItem {
    pub community_id: Uuid,
    pub event: Event,
    pub state: OutboxState,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxState {
    Pending,
    Unknown,
    Delivered,
    Rejected,
}

impl OutboxState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Unknown => "unknown",
            Self::Delivered => "delivered",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "unknown" => Some(Self::Unknown),
            "delivered" => Some(Self::Delivered),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadSlotRecord {
    pub community_id: Uuid,
    pub pubkey: String,
    pub slot_id: String,
    pub client_id: String,
    pub event_id: String,
    pub event_created_at: u64,
    pub local: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncCursor {
    pub high_created_at: u64,
    pub high_event_id: String,
    pub complete_through: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSearchQuery {
    pub fts_query: String,
    pub author: Option<String>,
    pub channel_id: Option<Uuid>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: usize,
}
