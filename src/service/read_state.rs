use crate::{
    auth::signer::SignerHandle,
    error::Result,
    realtime::supervisor::SupervisorHandle,
    store::{models::OutboxState, writer::StoreHandle},
    sync::read_state,
};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone)]
pub struct ReadStateService {
    community_id: Uuid,
    signer: SignerHandle,
    store: StoreHandle,
    supervisor: SupervisorHandle,
    generated_client_id: String,
}
impl ReadStateService {
    pub fn new(
        community_id: Uuid,
        signer: SignerHandle,
        store: StoreHandle,
        supervisor: SupervisorHandle,
    ) -> Self {
        Self {
            community_id,
            signer,
            store,
            supervisor,
            generated_client_id: read_state::random_id(16),
        }
    }
    pub async fn mark(&self, context: &str, read_at: u32) -> Result<u32> {
        let community = self.community_id;
        let pubkey = self.signer.public_key().to_hex();
        let context = context.to_owned();
        self.store
            .call(move |store| store.advance_read(community, &pubkey, &context, read_at, true))
            .await
    }
    pub async fn publish(&self, max_seen: u64) -> Result<usize> {
        let community = self.community_id;
        let pubkey = self.signer.public_key().to_hex();
        let contexts: BTreeMap<String, u32> = self
            .store
            .call({
                let pubkey = pubkey.clone();
                move |store| store.read_contexts(community, &pubkey, true)
            })
            .await?;
        let generated_client = self.generated_client_id.clone();
        let generated_slot = read_state::random_id(16);
        let (client_id, slot_ids, stored_max_seen) = self
            .store
            .call({
                let pubkey = pubkey.clone();
                move |store| {
                    store.ensure_local_read_slots(
                        community,
                        &pubkey,
                        &generated_client,
                        &generated_slot,
                    )
                }
            })
            .await?;
        let events = read_state::build_events(
            contexts,
            &client_id,
            &slot_ids,
            &self.signer,
            max_seen.max(stored_max_seen),
        )
        .await?;
        for event in &events {
            let stored = event.clone();
            let slot_id = crate::protocol::events::first_tag(event, "d")
                .and_then(|value| value.strip_prefix("read-state:").map(str::to_owned))
                .unwrap_or_default();
            let event_id = event.id.to_hex();
            let event_created_at = event.created_at.as_secs();
            let local_client = client_id.clone();
            let local_pubkey = pubkey.clone();
            self.store
                .call(move |store| {
                    store.insert_outbox(community, &stored)?;
                    store.record_read_slot(&crate::store::models::ReadSlotRecord {
                        community_id: community,
                        pubkey: local_pubkey,
                        slot_id,
                        client_id: local_client,
                        event_id,
                        event_created_at,
                        local: true,
                    })
                })
                .await?;
            let id = event.id.to_hex();
            let result = self.supervisor.publish_maintenance(event.clone()).await;
            let (state, error) = match result {
                Ok(ack) if ack.accepted => (OutboxState::Delivered, None),
                Ok(ack) => (OutboxState::Rejected, Some(ack.message)),
                Err(error) => (OutboxState::Unknown, Some(error.to_string())),
            };
            self.store
                .call(move |store| store.set_outbox_state(community, &id, state, error.as_deref()))
                .await?;
        }
        Ok(events.len())
    }
}
