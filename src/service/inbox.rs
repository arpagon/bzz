use nostr::Event;
use uuid::Uuid;

use crate::{
    domain::{InboxCursor, InboxItem, InboxPage},
    error::{Error, Result},
    protocol::{http::HttpClient, types::QueryFilter},
    store::writer::StoreHandle,
};

#[derive(Clone)]
pub struct InboxService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
}

impl InboxService {
    pub const fn new(community_id: Uuid, http: HttpClient, store: StoreHandle) -> Self {
        Self {
            community_id,
            http,
            store,
        }
    }

    pub async fn refresh(&self, identity_pubkey: &str) -> Result<usize> {
        let mentions = self
            .query_pages(
                QueryFilter {
                    kinds: vec![9, 40_002],
                    ..QueryFilter::default()
                }
                .tag("p", [identity_pubkey.to_owned()]),
                5,
            )
            .await?;
        let actions = self
            .query_pages(
                QueryFilter {
                    kinds: vec![46_010, 46_011, 46_012],
                    ..QueryFilter::default()
                }
                .tag("p", [identity_pubkey.to_owned()]),
                2,
            )
            .await?;
        let mut events = mentions
            .into_iter()
            .filter(|event| {
                matches!(event.kind.as_u16(), 9 | 40_002)
                    && crate::protocol::events::tag_values(event, "p")
                        .into_iter()
                        .any(|value| value == identity_pubkey)
            })
            .collect::<Vec<_>>();
        events.extend(actions.into_iter().filter(|event| {
            matches!(event.kind.as_u16(), 46_010..=46_012)
                && crate::protocol::events::tag_values(event, "p")
                    .into_iter()
                    .any(|value| value == identity_pubkey)
        }));
        events.sort_by_key(|event| event.id);
        events.dedup_by_key(|event| event.id);
        let count = events.len();
        let community_id = self.community_id;
        self.store
            .call(move |store| {
                for event in events {
                    store.apply_event(community_id, &event)?;
                }
                Ok(())
            })
            .await?;
        Ok(count)
    }

    pub async fn items(&self, identity_pubkey: &str) -> Result<Vec<InboxItem>> {
        Ok(self.page(identity_pubkey, None, 500).await?.items)
    }

    pub async fn page(
        &self,
        identity_pubkey: &str,
        cursor: Option<InboxCursor>,
        limit: usize,
    ) -> Result<InboxPage> {
        let identity_pubkey = identity_pubkey.to_owned();
        let community_id = self.community_id;
        self.store
            .call(move |store| {
                store.inbox_page(community_id, &identity_pubkey, cursor.as_ref(), limit)
            })
            .await
    }

    async fn query_pages(&self, mut filter: QueryFilter, pages: usize) -> Result<Vec<Event>> {
        const PAGE_SIZE: u32 = 100;
        let mut result = Vec::new();
        for _ in 0..pages.min(5) {
            filter.limit = Some(PAGE_SIZE);
            let page = self.http.query(&[filter.clone()]).await?;
            let count = page.len();
            let Some(last) = page.last() else {
                break;
            };
            let next = (last.created_at.as_secs(), last.id.to_hex());
            if filter.until == Some(next.0) && filter.before_id.as_deref() == Some(next.1.as_str())
            {
                return Err(Error::Protocol("Inbox query cursor did not advance".into()));
            }
            result.extend(page);
            if count < PAGE_SIZE as usize {
                break;
            }
            filter.until = Some(next.0);
            filter.before_id = Some(next.1);
        }
        Ok(result)
    }
}
