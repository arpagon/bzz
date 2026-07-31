use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QueryFilter {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub kinds: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_cursor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_cursor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_limit: Option<u16>,
    #[serde(flatten, default)]
    pub tags: BTreeMap<String, Vec<String>>,
}

impl QueryFilter {
    pub fn tag(mut self, name: &str, values: impl IntoIterator<Item = String>) -> Self {
        self.tags
            .insert(format!("#{name}"), values.into_iter().collect());
        self
    }

    pub fn value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub created_at: u64,
    pub event_id: [u8; 32],
}

impl Cursor {
    pub fn before_id_hex(&self) -> String {
        hex::encode(self.event_id)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum QueryResponse {
    Events(Vec<nostr::Event>),
    Wrapped { events: Vec<nostr::Event> },
}

impl QueryResponse {
    pub fn into_events(self) -> Vec<nostr::Event> {
        match self {
            Self::Events(events) | Self::Wrapped { events } => events,
        }
    }
}
