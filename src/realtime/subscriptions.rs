use std::collections::BTreeMap;

use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct SubscriptionRegistry {
    desired: BTreeMap<String, Vec<Value>>,
}

impl SubscriptionRegistry {
    pub fn upsert(&mut self, id: impl Into<String>, filters: Vec<Value>) {
        self.desired.insert(id.into(), filters);
    }
    pub fn remove(&mut self, id: &str) -> bool {
        self.desired.remove(id).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Value])> {
        self.desired
            .iter()
            .map(|(id, filters)| (id.as_str(), filters.as_slice()))
    }
    pub fn len(&self) -> usize {
        self.desired.len()
    }
    pub fn is_empty(&self) -> bool {
        self.desired.is_empty()
    }
}

pub fn membership(pubkey: &str, since: u64) -> Vec<Value> {
    vec![json!({"kinds":[44100,44101],"#p":[pubkey],"since":since.saturating_sub(30),"limit":100})]
}

pub fn personal(pubkey: &str, since: u64) -> Vec<Value> {
    vec![json!({
        "kinds":[30622,46010,46011,46012],
        "#p":[pubkey],
        "since":since.saturating_sub(300),
        "limit":500
    })]
}

pub fn read_state(pubkey: &str, since: u64) -> Vec<Value> {
    vec![json!({"kinds":[30078],"authors":[pubkey],"#t":["read-state"],"since":since,"limit":500})]
}

pub fn global_stream(since: u64) -> Vec<Value> {
    vec![
        json!({"kinds":[5,7,9,9005,39000,39002,40002,40003,40099],"since":since.saturating_sub(300),"limit":50}),
    ]
}

pub fn channel(channel_id: Uuid, since: u64) -> Vec<Value> {
    vec![
        json!({"kinds":[5,7,9,9005,39000,39002,40002,40003,40099],"#h":[channel_id.to_string()],"since":since.saturating_sub(300),"limit":1000}),
    ]
}
