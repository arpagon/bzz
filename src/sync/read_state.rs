use std::collections::BTreeMap;

use nostr::{Event, EventBuilder, Kind, Tag, Timestamp};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::events::{first_tag, verify},
    store::writer::StoreHandle,
};

pub const FETCH_LIMIT: u32 = 500;
pub const HORIZON_SECONDS: u64 = 7 * 24 * 60 * 60;
pub const MAX_CONTEXTS: usize = 10_000;
pub const MAX_PLAINTEXT_BYTES: usize = 32_768;
pub const MAX_SLOTS: usize = 8;
const D_PREFIX: &str = "read-state:";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadStateBlob {
    pub v: u8,
    pub client_id: String,
    pub contexts: BTreeMap<String, u32>,
}

impl ReadStateBlob {
    pub fn validate(&self) -> Result<()> {
        if self.v != 1
            || self.client_id.is_empty()
            || self.client_id.len() > 64
            || self.contexts.len() > MAX_CONTEXTS
        {
            return Err(Error::Protocol("invalid read-state blob".into()));
        }
        if self.contexts.keys().any(|key| key.len() > 256) {
            return Err(Error::Protocol("read-state context key is too long".into()));
        }
        Ok(())
    }
    pub fn merge(&mut self, other: &Self) {
        for (key, value) in &other.contexts {
            self.contexts
                .entry(key.clone())
                .and_modify(|current| *current = (*current).max(*value))
                .or_insert(*value);
        }
    }
}

pub fn random_id(bytes: usize) -> String {
    let mut data = vec![0_u8; bytes];
    rand::rng().fill(data.as_mut_slice());
    hex::encode(data)
}

pub async fn decrypt_event(event: &Event, signer: &SignerHandle) -> Result<ReadStateBlob> {
    verify(event)?;
    if event.kind.as_u16() != 30_078 || event.pubkey != signer.public_key() {
        return Err(Error::Protocol(
            "read-state event is not self-authored".into(),
        ));
    }
    let d = first_tag(event, "d")
        .ok_or_else(|| Error::Protocol("read-state event has no d tag".into()))?;
    let slot = d
        .strip_prefix(D_PREFIX)
        .ok_or_else(|| Error::Protocol("invalid read-state slot".into()))?;
    if slot.is_empty() || slot.len() > 64 || !slot.is_ascii() {
        return Err(Error::Protocol("invalid read-state slot".into()));
    }
    if first_tag(event, "t").as_deref() != Some("read-state") {
        return Err(Error::Protocol("invalid read-state topic".into()));
    }
    let plaintext = signer.decrypt_self(event.content.clone()).await?;
    if plaintext.len() > MAX_PLAINTEXT_BYTES {
        return Err(Error::Protocol("read-state plaintext is too large".into()));
    }
    let blob: ReadStateBlob = serde_json::from_str(&plaintext)
        .map_err(|_| Error::Protocol("invalid read-state JSON".into()))?;
    blob.validate()?;
    Ok(blob)
}

pub async fn merge_events(
    community_id: Uuid,
    events: &[Event],
    signer: &SignerHandle,
    store: &StoreHandle,
) -> Result<ReadStateBlob> {
    let client_id = random_id(16);
    let mut merged = ReadStateBlob {
        v: 1,
        client_id,
        contexts: BTreeMap::new(),
    };
    for event in events {
        if let Ok(blob) = decrypt_event(event, signer).await {
            merged.merge(&blob);
            let contexts = blob.contexts;
            let client_id = blob.client_id;
            let created = event.created_at.as_secs();
            let event_id = event.id.to_hex();
            let slot_id = first_tag(event, "d")
                .and_then(|value| value.strip_prefix(D_PREFIX).map(str::to_owned))
                .unwrap_or_default();
            let pubkey = signer.public_key().to_hex();
            store
                .call(move |store| {
                    store.merge_read_contexts(community_id, &pubkey, &contexts, created)?;
                    store.record_read_slot(&crate::store::models::ReadSlotRecord {
                        community_id,
                        pubkey,
                        slot_id,
                        client_id,
                        event_id,
                        event_created_at: created,
                        local: false,
                    })
                })
                .await?;
        }
    }
    Ok(merged)
}

pub fn split(mut contexts: BTreeMap<String, u32>, client_id: &str) -> Result<Vec<ReadStateBlob>> {
    contexts.retain(|key, _| key.len() <= 256);
    if contexts.len() > MAX_CONTEXTS {
        let mut prunable = contexts
            .iter()
            .filter(|(key, _)| key.starts_with("msg:") || key.starts_with("thread:"))
            .map(|(key, value)| (key.clone(), *value))
            .collect::<Vec<_>>();
        prunable.sort_by_key(|(_, value)| *value);
        for (key, _) in prunable.into_iter().take(contexts.len() - MAX_CONTEXTS) {
            contexts.remove(&key);
        }
        if contexts.len() > MAX_CONTEXTS {
            return Err(Error::Protocol(
                "read state has more than 10,000 non-evictable channel contexts".into(),
            ));
        }
    }
    let mut slots = vec![BTreeMap::new()];
    for (key, value) in contexts {
        let mut inserted = false;
        for slot in &mut slots {
            slot.insert(key.clone(), value);
            let size = serde_json::to_vec(&ReadStateBlob {
                v: 1,
                client_id: client_id.to_owned(),
                contexts: slot.clone(),
            })
            .map_err(|error| Error::Serialization(error.to_string()))?
            .len();
            if size <= MAX_PLAINTEXT_BYTES {
                inserted = true;
                break;
            }
            slot.remove(&key);
        }
        if !inserted {
            if slots.len() >= MAX_SLOTS {
                return Err(Error::Protocol("read state exceeds eight slots".into()));
            }
            let mut slot = BTreeMap::new();
            slot.insert(key, value);
            slots.push(slot);
        }
    }
    Ok(slots
        .into_iter()
        .map(|contexts| ReadStateBlob {
            v: 1,
            client_id: client_id.to_owned(),
            contexts,
        })
        .collect())
}

pub async fn build_events(
    contexts: BTreeMap<String, u32>,
    client_id: &str,
    slot_ids: &[String],
    signer: &SignerHandle,
    max_seen: u64,
) -> Result<Vec<Event>> {
    let blobs = split(contexts, client_id)?;
    let created = Timestamp::from(Timestamp::now().as_secs().max(max_seen.saturating_add(1)));
    let mut events = Vec::new();
    for (index, blob) in blobs.into_iter().enumerate() {
        let slot = slot_ids
            .get(index)
            .cloned()
            .unwrap_or_else(|| random_id(16));
        let plaintext = serde_json::to_string(&blob)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let ciphertext = signer.encrypt_self(plaintext).await?;
        let tags = vec![
            Tag::parse(["d", &format!("{D_PREFIX}{slot}")])
                .map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["t", "read-state"]).map_err(|error| Error::Protocol(error.to_string()))?,
        ];
        events.push(
            signer
                .sign(
                    EventBuilder::new(Kind::Custom(30_078), ciphertext)
                        .tags(tags)
                        .custom_created_at(created),
                )
                .await?,
        );
    }
    Ok(events)
}
