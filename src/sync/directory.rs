use std::collections::BTreeSet;

use nostr::Event;
use uuid::Uuid;

use crate::{
    error::Result,
    protocol::{http::HttpClient, types::QueryFilter},
    store::writer::StoreHandle,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DirectoryReport {
    pub membership_events: usize,
    pub metadata_events: usize,
    pub visibility_events: usize,
    pub channel_ids: BTreeSet<Uuid>,
}

pub async fn refresh(
    community_id: Uuid,
    self_pubkey: &str,
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<DirectoryReport> {
    let mut joined = query_all(
        http,
        QueryFilter {
            kinds: vec![39_002],
            ..QueryFilter::default()
        }
        .tag("p", [self_pubkey.to_owned()]),
    )
    .await?;
    joined.retain(|event| event.kind.as_u16() == 39_002);
    let mut ids = joined
        .iter()
        .filter(|event| {
            crate::protocol::events::tag_values(event, "p")
                .into_iter()
                .any(|value| value == self_pubkey)
        })
        .filter_map(|event| crate::protocol::events::first_tag(event, "d"))
        .filter_map(|value| Uuid::parse_str(&value).ok())
        .collect::<BTreeSet<_>>();
    apply(community_id, joined.clone(), store).await?;
    let joined_ids = ids.clone();
    store
        .call(move |store| store.reconcile_self_memberships(community_id, &joined_ids))
        .await?;

    let mut metadata = query_all(
        http,
        QueryFilter {
            kinds: vec![39_000],
            ..QueryFilter::default()
        },
    )
    .await?;
    if !ids.is_empty() {
        let member_ids = ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        for chunk in member_ids.chunks(100) {
            let member_metadata = query_all(
                http,
                QueryFilter {
                    kinds: vec![39_000],
                    ..QueryFilter::default()
                }
                .tag("d", chunk.to_vec()),
            )
            .await?;
            metadata.extend(member_metadata);
        }
    }
    metadata.retain(|event| event.kind.as_u16() == 39_000);
    metadata.sort_by_key(|event| event.id);
    metadata.dedup_by_key(|event| event.id);
    ids.extend(
        metadata
            .iter()
            .filter_map(|event| crate::protocol::events::first_tag(event, "d"))
            .filter_map(|value| Uuid::parse_str(&value).ok()),
    );
    apply(community_id, metadata.clone(), store).await?;
    let mut visibility = http
        .query(&[QueryFilter {
            kinds: vec![30_622],
            limit: Some(1),
            ..QueryFilter::default()
        }
        .tag("p", [self_pubkey.to_owned()])
        .tag("d", [self_pubkey.to_owned()])])
        .await?;
    visibility.retain(|event| event.kind.as_u16() == 30_622);
    apply(community_id, visibility.clone(), store).await?;
    Ok(DirectoryReport {
        membership_events: joined.len(),
        metadata_events: metadata.len(),
        visibility_events: visibility.len(),
        channel_ids: ids,
    })
}

pub async fn hydrate_profiles(
    community_id: Uuid,
    authors: impl IntoIterator<Item = String>,
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<usize> {
    let authors = authors
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut count = 0;
    for chunk in authors.chunks(100) {
        let mut events = http
            .query(&[QueryFilter {
                kinds: vec![0],
                authors: chunk.to_vec(),
                limit: Some(chunk.len() as u32),
                ..QueryFilter::default()
            }])
            .await?;
        events.retain(|event| event.kind.as_u16() == 0);
        count += events.len();
        apply(community_id, events, store).await?;
    }
    Ok(count)
}

async fn query_all(http: &HttpClient, mut filter: QueryFilter) -> Result<Vec<Event>> {
    const PAGE_SIZE: u32 = 500;
    let mut events = Vec::new();
    for _ in 0..500 {
        filter.limit = Some(PAGE_SIZE);
        let page = http.query(&[filter.clone()]).await?;
        let count = page.len();
        let Some(last) = page.last() else {
            return Ok(events);
        };
        let next = (last.created_at.as_secs(), last.id.to_hex());
        if filter.until == Some(next.0) && filter.before_id.as_deref() == Some(next.1.as_str()) {
            return Err(crate::Error::Protocol(
                "directory query cursor did not advance".into(),
            ));
        }
        events.extend(page);
        if count < PAGE_SIZE as usize {
            return Ok(events);
        }
        filter.until = Some(next.0);
        filter.before_id = Some(next.1);
    }
    Err(crate::Error::Protocol(
        "directory query exceeded the 500-page safety cap".into(),
    ))
}

async fn apply(community_id: Uuid, events: Vec<nostr::Event>, store: &StoreHandle) -> Result<()> {
    store
        .call(move |store| {
            for event in events {
                store.apply_event(community_id, &event)?;
            }
            Ok(())
        })
        .await
}
