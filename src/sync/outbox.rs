use std::time::Instant;

use uuid::Uuid;

use crate::{
    diagnostics::{DiagnosticEvent, DiagnosticHandle},
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
    flush_with_diagnostics(
        community_id,
        http,
        supervisor,
        store,
        &DiagnosticHandle::disabled(),
    )
    .await
}

pub async fn flush_with_diagnostics(
    community_id: Uuid,
    http: &HttpClient,
    supervisor: &SupervisorHandle,
    store: &StoreHandle,
    diagnostics: &DiagnosticHandle,
) -> Result<OutboxReport> {
    let started = Instant::now();
    let items = store
        .call(move |store| store.pending_outbox(community_id))
        .await?;
    diagnostics.emit(DiagnosticEvent::ReconcileStarted {
        eligible_count: u32::try_from(items.len()).unwrap_or(u32::MAX),
    });
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
            diagnostics.emit(DiagnosticEvent::ReconcileObserved {
                event_id: id.clone(),
                prior_state: item.state.as_str().into(),
            });
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
        let publish_started = Instant::now();
        match supervisor.publish(item.event).await {
            Ok(ack) if ack.accepted => {
                diagnostics.emit(DiagnosticEvent::ReconcileRepublished {
                    event_id: ack.event_id.clone(),
                    accepted: true,
                    duration_ms: elapsed_millis(publish_started),
                });
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
                diagnostics.emit(DiagnosticEvent::ReconcileRepublished {
                    event_id: ack.event_id.clone(),
                    accepted: false,
                    duration_ms: elapsed_millis(publish_started),
                });
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
                diagnostics.emit(DiagnosticEvent::ReconcileRepublished {
                    event_id: id.clone(),
                    accepted: false,
                    duration_ms: elapsed_millis(publish_started),
                });
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
    diagnostics.emit(DiagnosticEvent::ReconcileFinished {
        delivered: u32::try_from(report.delivered).unwrap_or(u32::MAX),
        rejected: u32::try_from(report.rejected).unwrap_or(u32::MAX),
        unknown: u32::try_from(report.unknown).unwrap_or(u32::MAX),
        duration_ms: elapsed_millis(started),
    });
    Ok(report)
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
