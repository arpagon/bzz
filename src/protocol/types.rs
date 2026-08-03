use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Prefix,
}

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
    pub search_mode: Option<SearchMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub feed_types: Vec<String>,
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

    pub fn validate(&self) -> Result<()> {
        if self.ids.len() > 100
            || self.authors.len() > 100
            || self.kinds.len() > 100
            || self.tags.len() > 16
            || self.tags.iter().any(|(name, values)| {
                name.len() > 65
                    || values.len() > 1_000
                    || values.iter().any(|value| value.len() > 4_096)
            })
        {
            return Err(Error::Config(
                "query filter exceeds a bounded field limit".into(),
            ));
        }
        if self
            .ids
            .iter()
            .chain(&self.authors)
            .any(|value| value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            || self.before_id.as_ref().is_some_and(|value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || self.thread_cursor_id.as_ref().is_some_and(|value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(Error::Config(
                "query contains an invalid event or author identifier".into(),
            ));
        }
        if self.limit.is_some_and(|limit| limit > 10_000)
            || self.depth_limit.is_some_and(|depth| depth > 100)
        {
            return Err(Error::Config("query result limit is out of bounds".into()));
        }
        if self
            .search
            .as_ref()
            .is_some_and(|value| value.len() > 4_096)
        {
            return Err(Error::Config("search query exceeds 4096 bytes".into()));
        }
        if self.page.is_some_and(|page| page >= 500) {
            return Err(Error::Config("search page exceeds the 500-page cap".into()));
        }
        if self.feed_types.len() > 4
            || self.feed_types.iter().any(|value| {
                !matches!(
                    value.as_str(),
                    "mentions" | "needs_action" | "activity" | "agent_activity"
                )
            })
        {
            return Err(Error::Config(
                "query contains an unsupported feed type".into(),
            ));
        }
        if (self.search_mode.is_some() || self.page.is_some()) && self.search.is_none() {
            return Err(Error::Config(
                "search extensions require a search query".into(),
            ));
        }
        Ok(())
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
