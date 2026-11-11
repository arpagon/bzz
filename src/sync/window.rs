//! NIP-CW loaders. The selected channel's visible history pages through
//! verified channel windows (rows + aux closure + 39005 summaries in one
//! request, no per-message `#e` fan-out); threads open at their newest replies
//! through verified thread windows. A missing or invalid bounds event on the
//! first page means "unsupported" and the caller uses the legacy path.

use nostr::Event;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::{
        http::HttpClient,
        window::{self, Cursor, Page, ThreadRequest},
    },
    store::{models::SyncCursor, writer::StoreHandle},
};

pub const CHANNEL_PAGE: u32 = 100;
pub const THREAD_PAGE: u32 = 50;
const MAX_PAGES: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct WindowReport {
    pub pages: usize,
    pub rows: usize,
    pub aux: usize,
    /// Relay-signed 39005 summaries for the caller's live summary overlay.
    pub summaries: Vec<Event>,
    pub crossed_watermark: bool,
}

async fn store_page(community: Uuid, page: &Page, store: &StoreHandle) -> Result<()> {
    let events = page
        .rows
        .iter()
        .chain(&page.aux)
        .cloned()
        .collect::<Vec<_>>();
    store
        .call(move |store| {
            for event in events {
                // Aux may reference rows by `e` only; a malformed one is
                // skipped rather than failing the verified page.
                let _ = store.apply_event(community, &event);
            }
            Ok(())
        })
        .await
}

/// Channel history through windows down to the stored watermark, then the
/// watermark advances exactly as the legacy backfill does.
pub async fn channel(
    community: Uuid,
    channel: Uuid,
    relay: &str,
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<WindowReport> {
    let scope = channel.to_string();
    let old = store
        .call({
            let scope = scope.clone();
            move |store| store.sync_cursor(community, "history", &scope)
        })
        .await?;
    let mut report = WindowReport::default();
    let mut newest = (old.high_created_at, old.high_event_id.clone());
    let mut cursor: Option<Cursor> = None;
    loop {
        let events = http
            .query_raw(&[window::channel_filter(
                channel,
                CHANNEL_PAGE,
                cursor.as_ref(),
            )])
            .await?;
        let page = match window::verify_channel(events, channel, cursor.as_ref(), relay) {
            Ok(page) => page,
            Err(error) if report.pages == 0 => {
                return Err(Error::Unsupported(format!("channel windows: {error}")));
            }
            Err(error) => return Err(error),
        };
        report.pages += 1;
        report.rows += page.rows.len();
        report.aux += page.aux.len();
        for row in &page.rows {
            let tuple = (row.created_at.as_secs(), row.id.to_hex());
            if tuple > newest {
                newest = tuple;
            }
        }
        let oldest = page.rows.last().map(|row| row.created_at.as_secs());
        store_page(community, &page, store).await?;
        report.summaries.extend(page.summaries.iter().cloned());
        // `has_more` is the only exhaustion signal; never the row count.
        let crossed = old.high_created_at > 0 && oldest.is_some_and(|at| at < old.high_created_at);
        match page.next {
            Some(next) if !crossed && report.pages < MAX_PAGES => cursor = Some(next),
            Some(_) if !crossed => {
                return Err(Error::Protocol(
                    "channel window exceeded the page cap".into(),
                ));
            }
            _ => {
                report.crossed_watermark = true;
                break;
            }
        }
    }
    let saved = SyncCursor {
        high_created_at: newest.0,
        high_event_id: newest.1,
        complete_through: nostr::Timestamp::now().as_secs(),
    };
    store
        .call(move |store| store.save_sync_cursor(community, "history", &scope, &saved))
        .await?;
    Ok(report)
}

/// One newest-first thread page (`cursor` = the previous page's `next`).
pub async fn thread_page(
    community: Uuid,
    request: &ThreadRequest<'_>,
    relay: &str,
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<Page> {
    let events = http.query_raw(&[window::thread_filter(request)]).await?;
    let page = window::verify_thread(events, request, relay)
        .map_err(|error| Error::Unsupported(format!("thread windows: {error}")))?;
    store_page(community, &page, store).await?;
    Ok(page)
}
