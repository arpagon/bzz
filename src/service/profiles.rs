use crate::{
    error::Result, protocol::http::HttpClient, store::writer::StoreHandle, sync::directory,
};
use uuid::Uuid;
#[derive(Clone)]
pub struct ProfileService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
}
impl ProfileService {
    pub const fn new(community_id: Uuid, http: HttpClient, store: StoreHandle) -> Self {
        Self {
            community_id,
            http,
            store,
        }
    }
    pub async fn hydrate(&self, authors: impl IntoIterator<Item = String>) -> Result<usize> {
        directory::hydrate_profiles(self.community_id, authors, &self.http, &self.store).await
    }
}
