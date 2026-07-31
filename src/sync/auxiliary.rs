use std::collections::BTreeSet;

use uuid::Uuid;

use crate::{
    error::Result,
    protocol::{http::HttpClient, types::QueryFilter},
    store::writer::StoreHandle,
};

pub async fn fetch_for_events(
    community_id: Uuid,
    event_ids: &[String],
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<usize> {
    let targets = event_ids.to_vec();
    let mut reaction_ids = store
        .call(move |store| {
            let mut ids = BTreeSet::new();
            for target in targets {
                ids.extend(
                    store
                        .reactions(community_id, &target)?
                        .into_iter()
                        .map(|reaction| reaction.event_id),
                );
            }
            Ok(ids)
        })
        .await?;
    let mut all = Vec::new();
    for chunk in event_ids.chunks(100) {
        let events = http
            .query(&[QueryFilter {
                kinds: vec![5, 7, 9005, 40003],
                limit: Some(10_000),
                ..QueryFilter::default()
            }
            .tag("e", chunk.iter().cloned())])
            .await?;
        reaction_ids.extend(
            events
                .iter()
                .filter(|event| event.kind.as_u16() == 7)
                .map(|event| event.id.to_hex()),
        );
        all.extend(events);
    }
    let reaction_ids = reaction_ids.into_iter().collect::<Vec<_>>();
    for reactions in reaction_ids.chunks(100) {
        all.extend(
            http.query(&[QueryFilter {
                kinds: vec![5, 9005],
                limit: Some(10_000),
                ..QueryFilter::default()
            }
            .tag("e", reactions.iter().cloned())])
                .await?,
        );
    }
    all.sort_by_key(|event| event.id);
    all.dedup_by_key(|event| event.id);
    let count = all.len();
    store
        .call(move |store| {
            for event in all {
                store.apply_event(community_id, &event)?;
            }
            Ok(())
        })
        .await?;
    Ok(count)
}
