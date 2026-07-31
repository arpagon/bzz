use uuid::Uuid;

use crate::{
    error::Result,
    protocol::{http::HttpClient, types::QueryFilter},
    realtime::supervisor::SupervisorHandle,
    store::{models::OutboxState, writer::StoreHandle},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutboxReport {
    pub delivered: usize,
    pub rejected: usize,
    pub unknown: usize,
}

pub async fn flush(
    community_id: Uuid,
    http: &HttpClient,
    supervisor: &SupervisorHandle,
    store: &StoreHandle,
) -> Result<OutboxReport> {
    let items = store
        .call(move |store| store.pending_outbox(community_id))
        .await?;
    let mut report = OutboxReport::default();
    for item in items {
        let id = item.event.id.to_hex();
        let found = http
            .query(&[QueryFilter {
                ids: vec![id.clone()],
                limit: Some(1),
                ..QueryFilter::default()
            }])
            .await
            .unwrap_or_default();
        if let Some(event) = found.into_iter().next() {
            store
                .call(move |store| {
                    store.apply_event(community_id, &event)?;
                    store.set_outbox_state(community_id, &id, OutboxState::Delivered, None)
                })
                .await?;
            report.delivered += 1;
            continue;
        }
        let retry_event = item.event.clone();
        match supervisor.publish(item.event).await {
            Ok(ack) if ack.accepted => {
                let id = ack.event_id;
                store
                    .call(move |store| {
                        store.apply_event(community_id, &retry_event)?;
                        store.set_outbox_state(community_id, &id, OutboxState::Delivered, None)
                    })
                    .await?;
                report.delivered += 1;
            }
            Ok(ack) => {
                let id = ack.event_id;
                let message = ack.message;
                store
                    .call(move |store| {
                        store.set_outbox_state(
                            community_id,
                            &id,
                            OutboxState::Rejected,
                            Some(&message),
                        )
                    })
                    .await?;
                report.rejected += 1;
            }
            Err(error) => {
                let message = error.to_string();
                store
                    .call(move |store| {
                        store.set_outbox_state(
                            community_id,
                            &id,
                            OutboxState::Unknown,
                            Some(&message),
                        )
                    })
                    .await?;
                report.unknown += 1;
            }
        }
    }
    Ok(report)
}
