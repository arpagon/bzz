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

#[derive(Clone, Debug, Default)]
pub struct HistoryOutcome {
    pub windowed: bool,
    pub summaries: Vec<nostr::Event>,
}

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

    /// History of the selected channel: verified NIP-CW channel windows when
    /// the relay serves them (rows, aux closure and 39005 summaries per page),
    /// otherwise the legacy backfill. Returns the relay-signed summaries.
    pub async fn selected_history(&self, channel: Uuid) -> Result<HistoryOutcome> {
        let community = self.community_id;
        let relay = self
            .store
            .call(move |store| store.pinned_relay_pubkey(community))
            .await?;
        if let Some(relay) = relay {
            match crate::sync::window::channel(community, channel, &relay, &self.http, &self.store)
                .await
            {
                Ok(report) => {
                    return Ok(HistoryOutcome {
                        windowed: true,
                        summaries: report.summaries,
                    });
                }
                // No valid bounds on the first page: the relay does not serve
                // windows (or not verifiably); use the standard path.
                Err(crate::Error::Unsupported(_)) => {}
                Err(error) => return Err(error),
            }
        }
        self.backfill(channel).await?;
        Ok(HistoryOutcome {
            windowed: false,
            summaries: Vec::new(),
        })
    }

    /// One newest-first thread page; `None` when the relay has no verifiable
    /// thread windows (the caller keeps the local archive).
    pub async fn thread_page(
        &self,
        request: &crate::protocol::window::ThreadRequest<'_>,
    ) -> Result<Option<crate::protocol::window::Page>> {
        let community = self.community_id;
        let Some(relay) = self
            .store
            .call(move |store| store.pinned_relay_pubkey(community))
            .await?
        else {
            return Ok(None);
        };
        match crate::sync::window::thread_page(community, request, &relay, &self.http, &self.store)
            .await
        {
            Ok(page) => Ok(Some(page)),
            Err(crate::Error::Unsupported(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn relay_host(&self) -> Option<String> {
        crate::protocol::membership_snapshot::canonical_host(self.http.base())
    }
}
