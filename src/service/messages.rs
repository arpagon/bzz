use nostr::{Event, EventId};
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    realtime::supervisor::SupervisorHandle,
    store::{models::OutboxState, writer::StoreHandle},
};

#[derive(Clone)]
pub struct MessageService {
    community_id: Uuid,
    signer: SignerHandle,
    store: StoreHandle,
    supervisor: SupervisorHandle,
}

impl MessageService {
    pub const fn new(
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
        }
    }
    pub async fn send(&self, channel: Uuid, content: &str) -> Result<Event> {
        self.send_builder(
            buzz_sdk::build_message(channel, content, None, &[], false, &[])
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn reply(
        &self,
        channel: Uuid,
        root: &str,
        parent: &str,
        content: &str,
    ) -> Result<Event> {
        let thread = buzz_sdk::ThreadRef {
            root_event_id: EventId::from_hex(root)
                .map_err(|error| Error::Protocol(error.to_string()))?,
            parent_event_id: EventId::from_hex(parent)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        };
        self.send_builder(
            buzz_sdk::build_message(channel, content, Some(&thread), &[], false, &[])
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn react(&self, target: &str, emoji: &str) -> Result<Event> {
        let target =
            EventId::from_hex(target).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_reaction(target, emoji)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn remove_reaction(&self, reaction: &str) -> Result<Event> {
        let reaction =
            EventId::from_hex(reaction).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_remove_reaction(reaction)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn delete(&self, channel: Uuid, target: &str, author: &str) -> Result<Event> {
        if author != self.signer.public_key().to_hex() {
            return Err(Error::Access("only your own message can be deleted".into()));
        }
        let target =
            EventId::from_hex(target).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_delete_compat(channel, target)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    async fn send_builder(&self, builder: nostr::EventBuilder) -> Result<Event> {
        let event = self.signer.sign(builder).await?;
        let stored = event.clone();
        let community = self.community_id;
        self.store
            .call(move |store| store.insert_outbox(community, &stored))
            .await?;
        let id = event.id.to_hex();
        match self.supervisor.publish(event.clone()).await {
            Ok(ack) if ack.accepted => {
                let event_id = id;
                let accepted = event.clone();
                self.store
                    .call(move |store| {
                        store.apply_event(community, &accepted)?;
                        store.set_outbox_state(community, &event_id, OutboxState::Delivered, None)
                    })
                    .await?;
            }
            Ok(ack) => {
                let message = ack.message;
                let stored_message = message.clone();
                let event_id = id;
                self.store
                    .call(move |store| {
                        store.set_outbox_state(
                            community,
                            &event_id,
                            OutboxState::Rejected,
                            Some(&stored_message),
                        )
                    })
                    .await?;
                return Err(Error::Access(message));
            }
            Err(error) => {
                let message = error.to_string();
                let event_id = id;
                self.store
                    .call(move |store| {
                        store.set_outbox_state(
                            community,
                            &event_id,
                            OutboxState::Unknown,
                            Some(&message),
                        )
                    })
                    .await?;
                return Err(error);
            }
        }
        Ok(event)
    }
}
