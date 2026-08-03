use std::collections::{HashMap, HashSet};

use nostr::Event;
use nucleo_matcher::{
    Config as MatcherConfig, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use time::{Date, Month, PrimitiveDateTime, Time};
use uuid::Uuid;

use crate::{
    domain::{Channel, Profile, SearchResult, SearchResultKind},
    error::{Error, Result},
    protocol::{
        events::{as_profile, verify},
        http::HttpClient,
        types::{QueryFilter, SearchMode},
    },
    render::sanitize,
    store::{models::MessageSearchQuery, writer::StoreHandle},
};

const INPUT_LIMIT: usize = 4_096;
const DEFAULT_LIMIT: usize = 20;
const REMOTE_PAGE_CAP: u32 = 2;
const SESSION_LIMIT: usize = 500;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParsedSearch {
    pub text: String,
    pub from: Option<String>,
    pub in_channel: Option<String>,
    pub after: Option<u64>,
    pub before: Option<u64>,
}

impl ParsedSearch {
    pub fn enough_text(&self) -> bool {
        self.text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count()
            >= 2
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    pub local_only: bool,
    pub notice: Option<String>,
}

#[derive(Clone)]
pub struct SearchService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
}

impl SearchService {
    pub const fn new(community_id: Uuid, http: HttpClient, store: StoreHandle) -> Self {
        Self {
            community_id,
            http,
            store,
        }
    }

    pub async fn execute(
        &self,
        input: &str,
        identity_pubkey: &str,
        channels: &[Channel],
        profiles: &HashMap<String, Profile>,
    ) -> Result<SearchOutput> {
        let parsed = parse(input)?;
        let mut resolved = resolve(&parsed, channels, profiles);
        if resolved.author_unresolved
            && self
                .resolve_remote_author(&parsed, profiles, &mut resolved)
                .await
                .is_err()
        {
            return Ok(SearchOutput {
                local_only: true,
                notice: Some(
                    "from: could not be resolved while remote search is unavailable".into(),
                ),
                ..SearchOutput::default()
            });
        }
        if let Some(notice) = resolved.notice() {
            return Ok(SearchOutput {
                notice: Some(notice),
                ..SearchOutput::default()
            });
        }
        let mut local = local_results(
            self.community_id,
            &self.store,
            identity_pubkey,
            channels,
            profiles,
            &parsed,
            &resolved,
        )
        .await?;
        if !parsed.enough_text() {
            local.notice = Some("type at least two search characters".into());
            return Ok(local);
        }

        let (profile_result, message_result) = tokio::join!(
            self.remote_profiles(&parsed),
            self.remote_messages(&parsed, &resolved, identity_pubkey)
        );
        let (mut remote_profiles, remote_messages) = match (profile_result, message_result) {
            (Ok(profiles), Ok(messages)) => (profiles, messages),
            _ => {
                local.local_only = true;
                local.notice = Some("remote search unavailable; showing local results".into());
                return Ok(local);
            }
        };
        let community_id = self.community_id;
        let profile_events = remote_profiles
            .iter()
            .map(|(_, event)| event.clone())
            .collect::<Vec<_>>();
        let message_events = remote_messages.clone();
        self.store
            .call(move |store| {
                for event in profile_events.into_iter().chain(message_events) {
                    store.apply_event(community_id, &event)?;
                }
                Ok(())
            })
            .await?;

        let mut remote = Vec::new();
        for (rank, (_, event)) in remote_profiles.drain(..).enumerate() {
            if let Some(profile) = as_profile(&event) {
                remote.push(SearchResult {
                    stable_id: format!("person:{}", profile.pubkey),
                    kind: SearchResultKind::Person,
                    label: sanitize::single_line(&profile.label()),
                    detail: crate::domain::abbreviated_pubkey(&profile.pubkey),
                    channel_id: None,
                    event_id: Some(profile.event_id),
                    thread_root: None,
                    pubkey: Some(profile.pubkey),
                    created_at: profile.created_at,
                    remote_rank: Some(u32::try_from(rank).unwrap_or(u32::MAX)),
                });
            }
        }
        for (rank, event) in remote_messages.iter().enumerate() {
            verify(event)?;
            let event_id = event.id.to_hex();
            let identity = identity_pubkey.to_owned();
            if let Some(mut result) = self
                .store
                .call(move |store| {
                    store.search_result_for_event(community_id, &identity, &event_id)
                })
                .await?
            {
                result.remote_rank = Some(u32::try_from(rank).unwrap_or(u32::MAX));
                remote.push(result);
            }
        }
        merge_remote(&mut local.results, remote);
        local.results.truncate(SESSION_LIMIT);
        Ok(local)
    }

    pub async fn hydrate_profiles(&self, input: &str) -> Result<usize> {
        let parsed = parse(input)?;
        if !parsed.enough_text() {
            return Ok(0);
        }
        let events = self
            .http
            .query(&[QueryFilter {
                kinds: vec![0],
                search: Some(parsed.text),
                search_mode: Some(SearchMode::Prefix),
                page: Some(0),
                limit: Some(50),
                ..QueryFilter::default()
            }])
            .await?;
        let events = events
            .into_iter()
            .filter(|event| event.kind.as_u16() == 0 && verify(event).is_ok())
            .take(50)
            .collect::<Vec<_>>();
        let count = events.len();
        let community = self.community_id;
        self.store
            .call(move |store| {
                for event in events {
                    store.apply_event(community, &event)?;
                }
                Ok(())
            })
            .await?;
        Ok(count)
    }

    pub async fn execute_local(
        community_id: Uuid,
        store: &StoreHandle,
        input: &str,
        identity_pubkey: &str,
        channels: &[Channel],
        profiles: &HashMap<String, Profile>,
    ) -> Result<SearchOutput> {
        let parsed = parse(input)?;
        let resolved = resolve(&parsed, channels, profiles);
        if let Some(notice) = resolved.notice() {
            return Ok(SearchOutput {
                local_only: true,
                notice: Some(notice),
                ..SearchOutput::default()
            });
        }
        let mut output = local_results(
            community_id,
            store,
            identity_pubkey,
            channels,
            profiles,
            &parsed,
            &resolved,
        )
        .await?;
        output.local_only = true;
        if !parsed.enough_text() && !parsed.text.is_empty() {
            output.notice = Some("type at least two search characters".into());
        }
        Ok(output)
    }

    async fn resolve_remote_author(
        &self,
        parsed: &ParsedSearch,
        profiles: &HashMap<String, Profile>,
        resolved: &mut ResolvedSearch,
    ) -> Result<()> {
        let Some(value) = parsed.from.as_deref() else {
            return Ok(());
        };
        let events = self
            .search_pages(QueryFilter {
                kinds: vec![0],
                search: Some(value.to_owned()),
                search_mode: Some(SearchMode::Prefix),
                limit: Some(DEFAULT_LIMIT as u32),
                ..QueryFilter::default()
            })
            .await?;
        let mut matches = events
            .iter()
            .filter(|event| verify(event).is_ok())
            .filter_map(as_profile)
            .filter(|profile| profile_matches(profile, value))
            .map(|profile| profile.pubkey)
            .collect::<Vec<_>>();
        matches.extend(
            profiles
                .values()
                .filter(|profile| profile_matches(profile, value))
                .map(|profile| profile.pubkey.clone()),
        );
        matches.sort();
        matches.dedup();
        if matches.len() == 1 {
            resolved.author = matches.pop();
            resolved.author_unresolved = false;
        }
        Ok(())
    }

    async fn remote_profiles(&self, parsed: &ParsedSearch) -> Result<Vec<(u32, Event)>> {
        if !parsed.enough_text() {
            return Ok(Vec::new());
        }
        let values = self
            .search_pages(QueryFilter {
                kinds: vec![0],
                search: Some(parsed.text.clone()),
                search_mode: Some(SearchMode::Prefix),
                limit: Some(DEFAULT_LIMIT as u32),
                ..QueryFilter::default()
            })
            .await?;
        Ok(values
            .into_iter()
            .filter(|event| verify(event).is_ok())
            .take(DEFAULT_LIMIT * REMOTE_PAGE_CAP as usize)
            .enumerate()
            .map(|(index, event)| (u32::try_from(index).unwrap_or(u32::MAX), event))
            .collect())
    }

    async fn remote_messages(
        &self,
        parsed: &ParsedSearch,
        resolved: &ResolvedSearch,
        _identity_pubkey: &str,
    ) -> Result<Vec<Event>> {
        if !parsed.enough_text() {
            return Ok(Vec::new());
        }
        let mut filter = QueryFilter {
            kinds: vec![9, 40_002],
            search: Some(parsed.text.clone()),
            search_mode: Some(SearchMode::Prefix),
            page: Some(0),
            limit: Some(DEFAULT_LIMIT as u32),
            authors: resolved.author.clone().into_iter().collect(),
            since: parsed.after,
            until: parsed.before.map(|value| value.saturating_sub(1)),
            ..QueryFilter::default()
        };
        if let Some(channel_id) = resolved.channel_id {
            filter = filter.tag("h", [channel_id.to_string()]);
        }
        Ok(self
            .search_pages(filter)
            .await?
            .into_iter()
            .filter(|event| matches!(event.kind.as_u16(), 9 | 40_002) && verify(event).is_ok())
            .take(DEFAULT_LIMIT * REMOTE_PAGE_CAP as usize)
            .collect())
    }

    async fn search_pages(&self, mut filter: QueryFilter) -> Result<Vec<Event>> {
        let page_size = filter.limit.unwrap_or(DEFAULT_LIMIT as u32).min(500);
        filter.limit = Some(page_size);
        let mut events = Vec::new();
        for page in 0..REMOTE_PAGE_CAP {
            filter.page = Some(page);
            let values = self.http.query(&[filter.clone()]).await?;
            let count = values.len();
            events.extend(values);
            if count < page_size as usize {
                break;
            }
        }
        Ok(events)
    }
}

#[derive(Clone, Debug, Default)]
struct ResolvedSearch {
    author: Option<String>,
    channel_id: Option<Uuid>,
    author_unresolved: bool,
    channel_unresolved: bool,
}

impl ResolvedSearch {
    fn notice(&self) -> Option<String> {
        if self.author_unresolved {
            Some("from: did not resolve to one visible person".into())
        } else if self.channel_unresolved {
            Some("in: did not resolve to one visible channel".into())
        } else {
            None
        }
    }
}

pub fn parse(input: &str) -> Result<ParsedSearch> {
    if input.len() > INPUT_LIMIT {
        return Err(Error::Config("search query exceeds 4096 bytes".into()));
    }
    let mut parsed = ParsedSearch::default();
    let mut text = Vec::new();
    for token in input.split_whitespace() {
        if let Some(value) = token
            .strip_prefix("from:")
            .filter(|value| !value.is_empty())
        {
            parsed.from = Some(value.to_owned());
        } else if let Some(value) = token.strip_prefix("in:").filter(|value| !value.is_empty()) {
            parsed.in_channel = Some(value.to_owned());
        } else if let Some(value) = token.strip_prefix("after:") {
            if let Some(timestamp) = parse_date(value) {
                parsed.after = Some(timestamp);
            } else {
                text.push(token);
            }
        } else if let Some(value) = token.strip_prefix("before:") {
            if let Some(timestamp) = parse_date(value) {
                parsed.before = Some(timestamp);
            } else {
                text.push(token);
            }
        } else {
            text.push(token);
        }
    }
    parsed.text = text.join(" ");
    Ok(parsed)
}

fn parse_date(value: &str) -> Option<u64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = Month::try_from(parts.next()?.parse::<u8>().ok()?).ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let local = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    u64::try_from(
        PrimitiveDateTime::new(date, Time::MIDNIGHT)
            .assume_offset(local)
            .unix_timestamp(),
    )
    .ok()
}

fn resolve(
    parsed: &ParsedSearch,
    channels: &[Channel],
    profiles: &HashMap<String, Profile>,
) -> ResolvedSearch {
    let mut resolved = ResolvedSearch::default();
    if let Some(value) = &parsed.from {
        let mut matches = profiles
            .values()
            .filter(|profile| profile_matches(profile, value))
            .map(|profile| profile.pubkey.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        if matches.len() == 1 {
            resolved.author = matches.pop();
        } else {
            resolved.author_unresolved = true;
        }
    }
    if let Some(value) = &parsed.in_channel {
        let value = value.strip_prefix('#').unwrap_or(value);
        let exact_uuid = Uuid::parse_str(value).ok();
        let mut matches = channels
            .iter()
            .filter(|channel| {
                Some(channel.id) == exact_uuid
                    || channel.name.eq_ignore_ascii_case(value)
                    || channel
                        .name
                        .to_ascii_lowercase()
                        .starts_with(&value.to_ascii_lowercase())
            })
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        if matches.len() == 1 {
            resolved.channel_id = matches.pop();
        } else {
            resolved.channel_unresolved = true;
        }
    }
    resolved
}

fn profile_matches(profile: &Profile, value: &str) -> bool {
    let value = value
        .strip_prefix('@')
        .unwrap_or(value)
        .to_ascii_lowercase();
    profile.pubkey.eq_ignore_ascii_case(&value)
        || profile
            .display_name
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().starts_with(&value))
        || profile
            .name
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().starts_with(&value))
}

async fn local_results(
    community_id: Uuid,
    store: &StoreHandle,
    identity_pubkey: &str,
    channels: &[Channel],
    profiles: &HashMap<String, Profile>,
    parsed: &ParsedSearch,
    resolved: &ResolvedSearch,
) -> Result<SearchOutput> {
    let identity = identity_pubkey.to_owned();
    let hidden_dms = store
        .call(move |store| store.hidden_dms(community_id, &identity))
        .await?;
    let mut results = Vec::new();
    for channel in rank_channels(&parsed.text, channels)
        .into_iter()
        .filter(|channel| !channel.kind.is_dm() || !hidden_dms.contains(&channel.id))
        .take(DEFAULT_LIMIT)
    {
        results.push(SearchResult {
            stable_id: format!("channel:{}", channel.id),
            kind: if channel.kind.is_dm() {
                SearchResultKind::Dm
            } else {
                SearchResultKind::Channel
            },
            label: sanitize::single_line(&channel.name),
            detail: if channel.kind.is_dm() {
                "Private workspace DM".into()
            } else {
                channel.id.to_string()
            },
            channel_id: Some(channel.id),
            event_id: None,
            thread_root: None,
            pubkey: None,
            created_at: channel.last_event_at.unwrap_or(0),
            remote_rank: None,
        });
    }
    if parsed.enough_text() {
        for profile in rank_profiles(&parsed.text, profiles)
            .into_iter()
            .take(DEFAULT_LIMIT)
        {
            if profile.pubkey == identity_pubkey {
                continue;
            }
            results.push(SearchResult {
                stable_id: format!("person:{}", profile.pubkey),
                kind: SearchResultKind::Person,
                label: sanitize::single_line(&profile.label()),
                detail: crate::domain::abbreviated_pubkey(&profile.pubkey),
                channel_id: None,
                event_id: Some(profile.event_id.clone()),
                thread_root: None,
                pubkey: Some(profile.pubkey.clone()),
                created_at: profile.created_at,
                remote_rank: None,
            });
        }
    }
    if parsed.enough_text() {
        let query = MessageSearchQuery {
            fts_query: fts_prefix_query(&parsed.text),
            author: resolved.author.clone(),
            channel_id: resolved.channel_id,
            since: parsed.after,
            until: parsed.before,
            limit: DEFAULT_LIMIT,
        };
        let identity = identity_pubkey.to_owned();
        results.extend(
            store
                .call(move |store| store.search_messages(community_id, &identity, &query))
                .await?,
        );
    }
    section_sort(&mut results);
    Ok(SearchOutput {
        results,
        local_only: false,
        notice: None,
    })
}

fn fts_prefix_query(value: &str) -> String {
    value
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim();
            (!token.is_empty()).then(|| format!("\"{}\"*", token.replace('"', "\"\"")))
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn rank_channels<'a>(query: &str, channels: &'a [Channel]) -> Vec<&'a Channel> {
    if query.is_empty() {
        let mut values = channels.iter().collect::<Vec<_>>();
        values.sort_by_key(|channel| std::cmp::Reverse(channel.last_event_at.unwrap_or(0)));
        return values;
    }
    fuzzy_rank(
        query,
        channels
            .iter()
            .map(|channel| (channel.name.as_str(), channel)),
    )
}

fn rank_profiles<'a>(query: &str, profiles: &'a HashMap<String, Profile>) -> Vec<&'a Profile> {
    if query.is_empty() {
        let mut values = profiles.values().collect::<Vec<_>>();
        values.sort_by_key(|profile| profile.label().to_ascii_lowercase());
        return values;
    }
    let labels = profiles
        .values()
        .map(|profile| (profile.label(), profile))
        .collect::<Vec<_>>();
    fuzzy_rank(
        query,
        labels
            .iter()
            .map(|(label, profile)| (label.as_str(), *profile)),
    )
}

fn fuzzy_rank<'a, T: Copy + 'a>(query: &str, values: impl Iterator<Item = (&'a str, T)>) -> Vec<T> {
    let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut ranked = values
        .filter_map(|(label, value)| {
            let mut buffer = Vec::new();
            pattern
                .score(Utf32Str::new(label, &mut buffer), &mut matcher)
                .map(|score| (score, label.to_ascii_lowercase(), value))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.into_iter().map(|(_, _, value)| value).collect()
}

fn merge_remote(local: &mut Vec<SearchResult>, remote: Vec<SearchResult>) {
    let mut seen = HashSet::new();
    let mut grouped = Vec::new();
    for kind in [
        SearchResultKind::Channel,
        SearchResultKind::Dm,
        SearchResultKind::Person,
        SearchResultKind::Message,
    ] {
        if matches!(kind, SearchResultKind::Person | SearchResultKind::Message) {
            grouped.extend(
                remote
                    .iter()
                    .filter(|result| result.kind == kind && seen.insert(result.stable_id.clone()))
                    .cloned(),
            );
        }
        grouped.extend(
            local
                .iter()
                .filter(|result| result.kind == kind && seen.insert(result.stable_id.clone()))
                .cloned(),
        );
    }
    *local = grouped;
}

fn section_sort(results: &mut [SearchResult]) {
    results.sort_by(|left, right| {
        section_order(left.kind)
            .cmp(&section_order(right.kind))
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.stable_id.cmp(&right.stable_id))
    });
}

const fn section_order(kind: SearchResultKind) -> u8 {
    match kind {
        SearchResultKind::Channel => 0,
        SearchResultKind::Dm => 1,
        SearchResultKind::Person => 2,
        SearchResultKind::Message => 3,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{fts_prefix_query, parse, parse_date};

    #[test]
    fn operators_are_token_bounded_and_later_values_win() {
        let parsed =
            parse("before:2026-08-02 hello from:first from:second in:general after:not-a-date")
                .unwrap();
        assert_eq!(parsed.from.as_deref(), Some("second"));
        assert_eq!(parsed.in_channel.as_deref(), Some("general"));
        assert_eq!(parsed.text, "hello after:not-a-date");
        assert_eq!(parsed.before, parse_date("2026-08-02"));
    }

    #[test]
    fn fts_input_is_quoted_and_prefix_bounded() {
        assert_eq!(
            fts_prefix_query("one two\"three"),
            r#""one"* AND "two""three"*"#
        );
    }

    proptest! {
        #[test]
        fn arbitrary_search_input_is_bounded_and_never_panics(value in ".{0,5000}") {
            let result = parse(&value);
            if value.len() > 4096 {
                prop_assert!(result.is_err());
            } else if let Ok(parsed) = result {
                prop_assert!(parsed.text.len() <= value.len());
                let fts = fts_prefix_query(&parsed.text);
                prop_assert!(fts.len() <= parsed.text.len().saturating_mul(8).saturating_add(32));
            }
        }
    }
}
