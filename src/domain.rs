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
pub struct Channel {
    pub id: Uuid,
    pub name: String,
    pub about: String,
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
pub struct Message {
    pub event_id: String,
    pub channel_id: Uuid,
    pub pubkey: String,
    pub created_at: u64,
    pub content: String,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Locked,
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
