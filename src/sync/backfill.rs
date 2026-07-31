use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::{http::HttpClient, types::QueryFilter},
    store::{models::SyncCursor, writer::StoreHandle},
    sync::auxiliary,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackfillReport {
    pub content_events: usize,
    pub auxiliary_events: usize,
    pub pages: usize,
    pub crossed_watermark: bool,
}

pub async fn channel(
    community_id: Uuid,
    channel_id: Uuid,
    http: &HttpClient,
    store: &StoreHandle,
    page_size: u32,
) -> Result<BackfillReport> {
    let scope_id = channel_id.to_string();
    let old = store
        .call({
            let scope_id = scope_id.clone();
            move |store| store.sync_cursor(community_id, "history", &scope_id)
        })
        .await?;
    let watermark = (old.high_created_at, old.high_event_id.clone());
    let mut until = None;
    let mut before_id = None;
    let mut report = BackfillReport::default();
    let cached_ids = store
        .call(move |store| store.channel_content_ids(community_id, channel_id))
        .await?;
    if !cached_ids.is_empty() {
        report.auxiliary_events +=
            auxiliary::fetch_for_events(community_id, &cached_ids, http, store).await?;
    }
    let mut newest = watermark.clone();
    loop {
        let events = http
            .query(&[QueryFilter {
                kinds: vec![9, 40002, 40099],
                until,
                before_id: before_id.clone(),
                limit: Some(page_size),
                ..QueryFilter::default()
            }
            .tag("h", [channel_id.to_string()])])
            .await?;
        report.pages += 1;
        if events.is_empty() {
            report.crossed_watermark = true;
            break;
        }
        let ids = events
            .iter()
            .map(|event| event.id.to_hex())
            .collect::<Vec<_>>();
        for event in &events {
            let tuple = (event.created_at.as_secs(), event.id.to_hex());
            if tuple > newest {
                newest = tuple;
            }
        }
        // Buzz orders pages by created_at DESC, event ID ASC. The last
        // returned row is therefore the exact composite continuation cursor.
        let Some(last) = events.last() else {
            return Err(Error::Protocol(
                "non-empty history page had no cursor".into(),
            ));
        };
        let oldest = (last.created_at.as_secs(), last.id.to_hex());
        let count = events.len();
        report.content_events += count;
        store
            .call(move |store| {
                for event in events {
                    store.apply_event(community_id, &event)?;
                }
                Ok(())
            })
            .await?;
        report.auxiliary_events +=
            auxiliary::fetch_for_events(community_id, &ids, http, store).await?;
        // Do not stop on an event-ID match inside the watermark second. A
        // newly published event can reuse that timestamp with an ID after the
        // old cursor; paging through the entire tied second avoids that hole.
        if watermark.0 > 0 && oldest.0 < watermark.0 {
            report.crossed_watermark = true;
            break;
        }
        if count < page_size as usize {
            report.crossed_watermark = true;
            break;
        }
        if report.pages >= 500 {
            return Err(Error::Protocol(
                "history backfill exceeded the 500-page safety cap".into(),
            ));
        }
        until = Some(oldest.0);
        before_id = Some(oldest.1);
    }
    if report.crossed_watermark {
        let cursor = SyncCursor {
            high_created_at: newest.0,
            high_event_id: newest.1,
            complete_through: nostr::Timestamp::now().as_secs(),
        };
        store
            .call(move |store| store.save_sync_cursor(community_id, "history", &scope_id, &cursor))
            .await?;
    }
    Ok(report)
}
