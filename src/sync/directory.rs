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
    pub agent_candidates: usize,
    pub agent_public_events: usize,
    pub verified_agents: usize,
    pub agent_projection_changes: usize,
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
    let agent_report = refresh_agents(community_id, self_pubkey, http, store).await?;
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
        agent_candidates: agent_report.candidates,
        agent_public_events: agent_report.public_events,
        verified_agents: agent_report.verified,
        agent_projection_changes: agent_report.projection_changes,
        channel_ids: ids,
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AgentRefreshReport {
    candidates: usize,
    public_events: usize,
    verified: usize,
    projection_changes: usize,
}

async fn refresh_agents(
    community_id: Uuid,
    self_pubkey: &str,
    http: &HttpClient,
    store: &StoreHandle,
) -> Result<AgentRefreshReport> {
    const AUTHOR_CHUNK: usize = 100;
    let candidates = store
        .call(move |store| store.remote_agent_candidate_pubkeys(community_id))
        .await?;
    if candidates.is_empty() {
        return Ok(AgentRefreshReport::default());
    }
    let bot_candidates = store
        .call(move |store| store.remote_agent_bot_pubkeys(community_id))
        .await?;
    let bot_set = bot_candidates.into_iter().collect::<BTreeSet<_>>();

    let candidate_set = candidates.iter().cloned().collect::<BTreeSet<_>>();
    let mut public_events = Vec::new();
    let mut declared_set = BTreeSet::new();
    // DM membership deliberately uses the operational `member` role. Query
    // declarations first so ordinary human participants do not cause a
    // profile fetch or a durable incomplete-agent projection. Exact bot
    // candidates also receive profiles for older no-declaration compatibility.
    for chunk in candidates.chunks(AUTHOR_CHUNK) {
        let mut declarations = http
            .query(&[QueryFilter {
                kinds: vec![10_100],
                authors: chunk.to_vec(),
                limit: Some(chunk.len() as u32),
                ..QueryFilter::default()
            }])
            .await?;
        declarations.retain(|event| {
            event.kind.as_u16() == 10_100 && candidate_set.contains(&event.pubkey.to_hex())
        });
        declared_set.extend(declarations.iter().map(|event| event.pubkey.to_hex()));
        public_events.extend(declarations);
    }
    let profile_candidates = candidates
        .iter()
        .filter(|candidate| declared_set.contains(*candidate) || bot_set.contains(*candidate))
        .cloned()
        .collect::<Vec<_>>();
    let profile_set = profile_candidates.iter().cloned().collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    for chunk in profile_candidates.chunks(AUTHOR_CHUNK) {
        let mut events = http
            .query(&[QueryFilter {
                kinds: vec![0],
                authors: chunk.to_vec(),
                limit: Some(chunk.len() as u32),
                ..QueryFilter::default()
            }])
            .await?;
        events.retain(|event| {
            event.kind.as_u16() == 0 && profile_set.contains(&event.pubkey.to_hex())
        });
        profiles.extend(events.iter().cloned());
        public_events.extend(events);
    }

    let verified_owners = profiles
        .iter()
        .filter_map(|profile| {
            crate::agents::protocol::verified_owner_pubkey(profile)
                .ok()
                .map(|owner| (profile.pubkey.to_hex(), owner))
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    for chunk in profile_candidates.chunks(AUTHOR_CHUNK) {
        let owners = chunk
            .iter()
            .filter_map(|candidate| verified_owners.get(candidate).cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if owners.is_empty() {
            continue;
        }
        let mut policies = http
            .query(&[QueryFilter {
                kinds: vec![30_177],
                authors: owners,
                limit: Some(chunk.len() as u32),
                ..QueryFilter::default()
            }
            .tag("d", chunk.to_vec())])
            .await?;
        policies.retain(|event| {
            event.kind.as_u16() == 30_177
                && crate::protocol::events::first_tag(event, "d")
                    .is_some_and(|value| profile_set.contains(&value))
        });
        public_events.extend(policies);
    }

    let public_event_count = public_events.len();
    apply(community_id, public_events, store).await?;
    let (projection_changes, verified) = store
        .call({
            let self_pubkey = self_pubkey.to_owned();
            move |store| {
                let changed = store.reconcile_remote_agents(community_id)?;
                let verified = store.list_remote_agents(community_id, &self_pubkey)?.len();
                Ok((changed, verified))
            }
        })
        .await?;
    Ok(AgentRefreshReport {
        candidates: candidates.len(),
        public_events: public_event_count,
        verified,
        projection_changes,
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
