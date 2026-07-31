use crate::{
    error::Result,
    protocol::http::HttpClient,
    store::writer::StoreHandle,
    sync::{
        backfill::{self, BackfillReport},
        directory::{self, DirectoryReport},
    },
};
use uuid::Uuid;

#[derive(Clone)]
pub struct ChannelService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
}
impl ChannelService {
    pub const fn new(community_id: Uuid, http: HttpClient, store: StoreHandle) -> Self {
        Self {
            community_id,
            http,
            store,
        }
    }
    pub async fn refresh(&self, pubkey: &str) -> Result<DirectoryReport> {
        directory::refresh(self.community_id, pubkey, &self.http, &self.store).await
    }
    pub async fn backfill(&self, channel: Uuid) -> Result<BackfillReport> {
        backfill::channel(self.community_id, channel, &self.http, &self.store, 500).await
    }
}
