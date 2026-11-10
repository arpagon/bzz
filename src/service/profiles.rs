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

    /// NIP-CL public membership lookup for a profile card. Display data only:
    /// the result never reaches the store, rosters or the viewer's authority.
    /// Without a pinned relay key there is nothing to verify against.
    pub async fn public_memberships(
        &self,
        subject: &str,
    ) -> Result<crate::protocol::membership_public::Lookup> {
        let community = self.community_id;
        let Some(pinned) = self
            .store
            .call(move |store| store.pinned_relay_pubkey(community))
            .await?
        else {
            return Ok(crate::protocol::membership_public::Lookup::Unsupported);
        };
        // A fresh host-bound NIP-11 read rechecks the descriptor every time.
        let info = self.http.nip11().await?;
        self.http.membership_public(&info, &pinned, subject).await
    }
}
