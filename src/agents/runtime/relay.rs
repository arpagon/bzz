//! The relay as one local agent sees it: its own key, its own connection,
//! its own HTTP client. Every event the agent publishes is signed here with
//! the agent's key and carries the owner's NIP-OA tag, as the v0.13 broker
//! does. No harness ever reaches this module's keys or connection.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use nostr::{Event, EventBuilder, Tag};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    agents::runtime::{
        admission::{ChannelFacts, PlaceKind},
        host::AgentHost,
    },
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::{events, http::HttpClient},
    realtime::{
        admission::is_local_admission_error,
        session::SessionEvent,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    store::writer::StoreHandle,
};

/// What became of one event handed to the relay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Published {
    /// The relay has it (now, or already).
    Accepted,
    /// The relay's own no to this event. It is final: the same event sent
    /// again gets the same answer.
    Refused(String),
    /// Not a verdict on the event, and the relay does not have it: it never
    /// left bzz, or the relay said to come back later (`rate-limited:`,
    /// `error:`). The same event may be sent again.
    Later(String),
    /// No verdict came. It may be stored.
    Unknown,
}

/// Whether a relay's `OK false` is its verdict on the event, or only says
/// that it cannot take anything just now (NIP-01's `rate-limited:` and
/// `error:` prefixes).
fn is_verdict(message: &str) -> bool {
    let prefix = message
        .split_once(':')
        .map_or(message, |(prefix, _)| prefix)
        .trim()
        .to_ascii_lowercase();
    !matches!(prefix.as_str(), "rate-limited" | "rate limited" | "error")
}

/// The agent's side of the relay. Tests replace it with a recorded one.
#[async_trait]
pub trait AgentRelay: Send + Sync + 'static {
    /// Signs with the agent's key, adding the owner's tag.
    async fn sign(&self, builder: EventBuilder) -> Result<Event>;
    async fn publish(&self, event: Event) -> Published;
    /// Reads what the agent itself may read.
    async fn query(&self, filters: Vec<Value>) -> Result<Vec<Event>>;
    /// The relay's own records of one channel. They may be a minute old.
    async fn channel(&self, channel: Uuid) -> Result<ChannelFacts>;
    /// The relay's own records of one channel, asked for now. An
    /// [`Error::Access`] says the agent cannot see the channel: it is not a
    /// member of it.
    async fn channel_now(&self, channel: Uuid) -> Result<ChannelFacts> {
        self.channel(channel).await
    }
    /// Ends the agent's connection and forgets its key, for an agent this
    /// bzz will not run again (it was moved away). A relay counts a key as
    /// present while a connection of it is open: an agent that runs
    /// elsewhere must not look present because of a bzz that let it go.
    async fn close(&self) {}
    /// The records of apps as this agent reads and writes them (v0.16 M7):
    /// a records service on the agent's own key, connection and HTTP
    /// client, so that what it writes is signed by it and spends its own
    /// quota. `None`: this relay has none, and the record tools say so.
    #[cfg(feature = "apps")]
    fn records(&self) -> Option<crate::service::records::RecordsService> {
        None
    }
}

fn newest_first(events: &mut Vec<Event>) {
    events.sort_by(|left, right| {
        (right.created_at, right.id.to_hex()).cmp(&(left.created_at, left.id.to_hex()))
    });
    events.dedup_by(|left, right| left.id == right.id);
}

/// The messages of a place, oldest first: at most `limit`, all older than
/// `before` when given.
pub async fn conversation(
    relay: &dyn AgentRelay,
    place: &str,
    before: Option<&Event>,
    limit: usize,
) -> Result<Vec<Event>> {
    let (channel, root) = match place.split_once('/') {
        Some((channel, root)) => (channel, Some(root)),
        None => (place, None),
    };
    let channel_id =
        Uuid::parse_str(channel).map_err(|_| Error::Protocol("the place has no channel".into()))?;
    let mut filter = json!({"kinds": [9], "#h": [channel], "limit": limit + 20});
    if let Some(before) = before {
        filter["until"] = json!(before.created_at.as_secs());
    }
    let mut filters = Vec::new();
    if let Some(root) = root {
        filter["#e"] = json!([root]);
        filters.push(json!({"ids": [root], "limit": 1}));
    }
    filters.insert(0, filter);
    let mut events = relay
        .query(filters)
        .await?
        .into_iter()
        .filter(|event| {
            event.kind.as_u16() == 9
                && events::channel_id(event) == Some(channel_id)
                && events::verify(event).is_ok()
        })
        .filter(|event| {
            before.is_none_or(|before| {
                (event.created_at, event.id.to_hex()) < (before.created_at, before.id.to_hex())
            })
        })
        .collect::<Vec<_>>();
    newest_first(&mut events);
    events.truncate(limit);
    events.reverse();
    Ok(events)
}

/// Display names of `pubkeys` from their newest profiles. A key without a
/// name is absent.
pub async fn names(relay: &dyn AgentRelay, pubkeys: &[String]) -> HashMap<String, String> {
    if pubkeys.is_empty() {
        return HashMap::new();
    }
    let Ok(mut profiles) = relay
        .query(vec![
            json!({"kinds": [0], "authors": pubkeys, "limit": pubkeys.len() * 2}),
        ])
        .await
    else {
        return HashMap::new();
    };
    newest_first(&mut profiles);
    let mut names = HashMap::new();
    for profile in profiles.iter().rev() {
        if events::verify(profile).is_err() {
            continue;
        }
        if let Some(name) = events::as_profile(profile).and_then(|profile| {
            profile
                .display_name
                .or(profile.name)
                .map(|name| crate::protocol::artifact::display(name.trim(), 64))
                .filter(|name| !name.is_empty())
        }) {
            names.insert(profile.pubkey.to_hex(), name);
        }
    }
    names
}

/// The members of a member list (39002) who may not write: those with the
/// `guest` role, in the exact shape `["p", pubkey, relay, role]`.
fn read_only(members: &Event) -> Vec<String> {
    members
        .tags
        .iter()
        .filter_map(|tag| match tag.as_slice() {
            [name, pubkey, _, role] if name == "p" && role == "guest" => Some(pubkey.clone()),
            _ => None,
        })
        .collect()
}

/// A channel's facts from its relay-signed metadata (39000) and member list
/// (39002). `None` when either is missing or not signed by `relay_pubkey`.
pub fn channel_facts(channel: Uuid, relay_pubkey: &str, records: &[Event]) -> Option<ChannelFacts> {
    let newest = |kind: u16| {
        records
            .iter()
            .filter(|event| {
                event.kind.as_u16() == kind
                    && event.pubkey.to_hex() == relay_pubkey
                    && events::first_tag(event, "d").as_deref()
                        == Some(channel.to_string().as_str())
                    && events::verify(event).is_ok()
            })
            .max_by_key(|event| (event.created_at, event.id.to_hex()))
    };
    let metadata = newest(39_000)?;
    let members = newest(39_002)?;
    let declared = events::first_tag(metadata, "t");
    let labels = events::tag_values(metadata, "t");
    let hidden = metadata
        .tags
        .iter()
        .any(|tag| tag.as_slice().first().map(String::as_str) == Some("hidden"));
    // As the pinned Buzz reads it: a declared `dm`, or `hidden` on metadata
    // that declares no type.
    let kind = if declared.as_deref() == Some("dm") || (declared.is_none() && hidden) {
        PlaceKind::Dm
    } else if labels.iter().skip(1).any(|label| label == "buzz.solo") {
        PlaceKind::Solo
    } else {
        PlaceKind::Channel
    };
    Some(ChannelFacts {
        id: channel,
        name: events::first_tag(metadata, "name").unwrap_or_else(|| channel.to_string()),
        kind,
        members: events::tag_values(members, "p"),
        read_only: read_only(members),
    })
}

/// Hands `event` to the relay on `supervisor` and says what became of it. A
/// relay that already has the event has accepted it.
pub async fn publish_on(supervisor: &SupervisorHandle, event: Event, limit: Duration) -> Published {
    match tokio::time::timeout(limit, supervisor.publish(event)).await {
        Ok(Ok(ack)) if ack.accepted || ack.message.starts_with("duplicate:") => Published::Accepted,
        Ok(Ok(ack)) if is_verdict(&ack.message) => {
            Published::Refused(crate::protocol::artifact::display(&ack.message, 160))
        }
        Ok(Ok(ack)) => Published::Later(crate::protocol::artifact::display(&ack.message, 160)),
        // Refused by bzz's own queue: it was never sent.
        Ok(Err(error)) if is_local_admission_error(&error) => {
            Published::Later("bzz could not send it now; try again shortly".into())
        }
        Ok(Err(_)) | Err(_) => Published::Unknown,
    }
}

const CHANNEL_CACHE: Duration = Duration::from_secs(60);
/// How far back a start looks for requests it missed while bzz was closed.
/// Short on purpose: it covers a restart or a crash, and an agent does not
/// answer what was asked hours before it was started.
pub const CATCH_UP: Duration = Duration::from_secs(600);
/// A message older than this is never a request, whoever sends it. Far more
/// than a start catches up, and far less than a finished request is kept
/// (`SETTLED_RETENTION_SECONDS`): a request that was pruned cannot come back
/// as a new one.
pub const MAX_REQUEST_AGE: Duration = Duration::from_secs(24 * 3600);
const INTAKE: &str = "agent-intake";

fn too_old(created_at: u64, now: u64) -> bool {
    created_at.saturating_add(MAX_REQUEST_AGE.as_secs()) < now
}
/// The least time between two subscriptions of one intake, whatever asks.
const RESUBSCRIBE_INTERVAL: Duration = Duration::from_secs(2);
/// How long a reason to subscribe again waits for others to join it.
const RESUBSCRIBE_DEBOUNCE: Duration = Duration::from_millis(500);
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Membership notices already acted on. The relay sends a stored notice
/// again with every new subscription, so acting on one twice would
/// subscribe again for ever.
#[derive(Default)]
struct Notices(std::collections::HashSet<String>);

impl Notices {
    /// Whether `event` is a notice not seen before.
    fn is_new(&mut self, event: &Event) -> bool {
        if self.0.len() > 4096 {
            self.0.clear();
        }
        self.0.insert(event.id.to_hex())
    }
}

/// One agent's key, connection and HTTP client on the real relay.
pub struct RelayPort {
    pub signer: SignerHandle,
    pub http: HttpClient,
    pub supervisor: SupervisorHandle,
    auth_tag: Tag,
    /// The relay's own signing key, pinned from NIP-11.
    relay_pubkey: String,
    publish_timeout: Duration,
    channels: Mutex<HashMap<Uuid, (Instant, ChannelFacts)>>,
    /// Set once the intake's subscription is live.
    listening: AtomicBool,
    /// Where the relay's records of the agent's channels are kept as bzz
    /// reads them, for `bzz agent show`: it then says the role the agent
    /// has now, also when no bzz of this profile is open.
    records: Option<(StoreHandle, Uuid)>,
    /// The agent's own records service, made with the store above.
    #[cfg(feature = "apps")]
    app_records: Option<crate::service::records::RecordsService>,
}

impl RelayPort {
    pub fn new(
        signer: SignerHandle,
        http: HttpClient,
        supervisor: SupervisorHandle,
        auth_tag: Tag,
        relay_pubkey: String,
    ) -> Self {
        Self {
            signer,
            http,
            supervisor,
            auth_tag,
            relay_pubkey,
            publish_timeout: Duration::from_secs(15),
            channels: Mutex::new(HashMap::new()),
            listening: AtomicBool::new(false),
            records: None,
            #[cfg(feature = "apps")]
            app_records: None,
        }
    }

    /// Keeps the relay's own records of the agent's channels in `store`,
    /// as any bzz keeps them, each time they are read. With the feature
    /// `apps`, the records of apps the agent reads and writes are kept
    /// there too.
    #[must_use]
    pub fn keeping_records(mut self, store: StoreHandle, community: Uuid) -> Self {
        #[cfg(feature = "apps")]
        {
            self.app_records = Some(crate::service::records::RecordsService::new(
                community,
                self.http.clone(),
                store.clone(),
                crate::service::records::Writer::new(self.signer.clone(), self.supervisor.clone()),
            ));
        }
        self.records = Some((store, community));
        self
    }

    /// Stores the relay's own signed records among `records`.
    async fn keep(&self, records: &[Event]) {
        let Some((store, community)) = &self.records else {
            return;
        };
        let (community, relay) = (*community, self.relay_pubkey.clone());
        let own = records
            .iter()
            .filter(|event| event.pubkey.to_hex() == relay && events::verify(event).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        if own.is_empty() {
            return;
        }
        let _ = store
            .call(move |store| {
                for record in &own {
                    let _ = store.apply_event(community, record);
                }
                Ok(())
            })
            .await;
    }

    /// Whether requests published from now on will reach the agent.
    pub fn listening(&self) -> bool {
        self.listening.load(Ordering::Acquire)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, (Instant, ChannelFacts)>> {
        self.channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every channel the relay lists the agent in, by its own records.
    async fn memberships(&self) -> Result<Vec<ChannelFacts>> {
        let agent = self.signer.public_key().to_hex();
        let lists = self
            .http
            .query_raw(&[json!({"kinds": [39_002], "#p": [agent], "limit": 500})])
            .await?;
        let ids = lists
            .iter()
            .filter_map(|event| events::first_tag(event, "d"))
            .filter_map(|id| Uuid::parse_str(&id).ok())
            .collect::<std::collections::BTreeSet<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let names = ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
        let mut records = self
            .http
            .query_raw(&[json!({"kinds": [39_000], "#d": names, "limit": names.len() * 2})])
            .await?;
        records.extend(lists);
        self.keep(&records).await;
        let facts = ids
            .into_iter()
            .filter_map(|id| channel_facts(id, &self.relay_pubkey, &records))
            .collect::<Vec<_>>();
        let now = Instant::now();
        let mut cache = self.cache();
        for channel in &facts {
            cache.insert(channel.id, (now, channel.clone()));
        }
        Ok(facts)
    }
}

#[async_trait]
impl AgentRelay for RelayPort {
    async fn sign(&self, builder: EventBuilder) -> Result<Event> {
        self.signer.sign(builder.tag(self.auth_tag.clone())).await
    }

    async fn publish(&self, event: Event) -> Published {
        publish_on(&self.supervisor, event, self.publish_timeout).await
    }

    async fn query(&self, filters: Vec<Value>) -> Result<Vec<Event>> {
        self.http.query_raw(&filters).await
    }

    async fn channel(&self, channel: Uuid) -> Result<ChannelFacts> {
        if let Some((at, facts)) = self.cache().get(&channel)
            && at.elapsed() < CHANNEL_CACHE
        {
            return Ok(facts.clone());
        }
        self.channel_now(channel).await
    }

    async fn channel_now(&self, channel: Uuid) -> Result<ChannelFacts> {
        let id = channel.to_string();
        let records = self
            .http
            .query_raw(&[
                json!({"kinds": [39_000], "#d": [id], "limit": 2}),
                json!({"kinds": [39_002], "#d": [id], "limit": 2}),
            ])
            .await?;
        self.keep(&records).await;
        let Some(facts) = channel_facts(channel, &self.relay_pubkey, &records) else {
            self.cache().remove(&channel);
            return Err(Error::Access("the agent cannot see this channel".into()));
        };
        self.cache()
            .insert(channel, (Instant::now(), facts.clone()));
        Ok(facts)
    }

    async fn close(&self) {
        self.listening.store(false, Ordering::Release);
        self.supervisor.shutdown().await;
        self.signer.lock().await;
    }

    #[cfg(feature = "apps")]
    fn records(&self) -> Option<crate::service::records::RecordsService> {
        self.app_records.clone()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The subscription that brings an agent its requests: mentions in the
/// channels it is a member of, every message of the DMs it shares with its
/// owner alone, and its own membership changes.
fn intake_filters(agent: &str, owner: &str, channels: &[ChannelFacts], since: u64) -> Vec<Value> {
    let mut by_kind = BTreeMap::<bool, Vec<String>>::new();
    for channel in channels {
        let dm = channel.kind == PlaceKind::Dm;
        let with_owner_alone = channel.members.len() == 2
            && channel
                .members
                .iter()
                .any(|member| member.eq_ignore_ascii_case(owner));
        if !dm || with_owner_alone {
            by_kind.entry(dm).or_default().push(channel.id.to_string());
        }
    }
    let mut filters = vec![json!({
        "kinds": [44_100, 44_101],
        "#p": [agent],
        "since": unix_now().saturating_sub(30),
        "limit": 50,
    })];
    for (dm, ids) in by_kind {
        for ids in ids.chunks(100) {
            let mut filter = json!({"kinds": [9], "#h": ids, "since": since, "limit": 500});
            if !dm {
                filter["#p"] = json!([agent]);
            }
            filters.push(filter);
        }
    }
    filters
}

/// Runs until `stop` is set: subscribes on the agent's own connection and
/// offers every message that arrives to the host, which decides admission.
pub async fn intake(
    port: Arc<RelayPort>,
    host: AgentHost,
    store: StoreHandle,
    agent: Uuid,
    owner: String,
    stop: Arc<tokio::sync::Notify>,
) {
    let agent_pubkey = port.signer.public_key().to_hex();
    let mut events = port.supervisor.subscribe_events();
    let cursor = store
        .call(move |store| store.agent_cursor(agent))
        .await
        .ok()
        .flatten();
    let now = unix_now();
    // The first start of an agent reads nothing from before it existed.
    let mut since = cursor.map_or(now, |seen| {
        seen.saturating_sub(30)
            .max(now.saturating_sub(CATCH_UP.as_secs()))
    });
    let mut notices = Notices::default();
    // When to subscribe next: now, and again whenever the agent's channels
    // may have changed. Several reasons close together make one subscription.
    let mut due = Some(tokio::time::Instant::now());
    let mut subscribed_at = None::<tokio::time::Instant>;
    loop {
        let event = tokio::select! {
            () = stop.notified() => break,
            () = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                subscribed_at = Some(tokio::time::Instant::now());
                match port.memberships().await {
                    Ok(channels) => {
                        let filters = intake_filters(&agent_pubkey, &owner, &channels, since);
                        if port.supervisor.subscribe(INTAKE, filters).await.is_err() {
                            return;
                        }
                        // The relay may have added the agent to a channel
                        // without a notice reaching it: look again later.
                        due = Some(tokio::time::Instant::now() + REFRESH_INTERVAL);
                    }
                    // Not reachable just now: try again shortly.
                    Err(_) => {
                        port.listening.store(false, Ordering::Release);
                        due = Some(tokio::time::Instant::now() + RETRY_INTERVAL);
                    }
                }
                continue;
            }
            event = events.recv() => event,
        };
        // Soon, but never faster than the interval, whatever the relay sends.
        let soon = || {
            let earliest = subscribed_at
                .map_or_else(tokio::time::Instant::now, |at| at + RESUBSCRIBE_INTERVAL);
            Some(earliest.max(tokio::time::Instant::now() + RESUBSCRIBE_DEBOUNCE))
        };
        match event {
            Ok(SupervisorEvent::Session(SessionEvent::Event {
                subscription,
                event,
            })) if subscription == INTAKE => {
                if matches!(event.kind.as_u16(), 44_100 | 44_101) {
                    // The agent joined or left a channel: look again, once.
                    if notices.is_new(&event) {
                        port.cache().clear();
                        due = soon();
                    }
                    continue;
                }
                let Some(channel) = events::channel_id(&event) else {
                    continue;
                };
                let Ok(facts) = port.channel(channel).await else {
                    continue;
                };
                let seen = event.created_at.as_secs();
                if too_old(seen, unix_now()) {
                    continue;
                }
                let _ = host.offer(agent, &event, &facts).await;
                // Only after the request is in the inbox (or refused).
                since = since.max(seen.saturating_sub(30));
                let _ = store
                    .call(move |store| store.advance_agent_cursor(agent, seen))
                    .await;
            }
            Ok(SupervisorEvent::Session(SessionEvent::Eose(subscription)))
                if subscription == INTAKE =>
            {
                port.listening.store(true, Ordering::Release);
            }
            Ok(SupervisorEvent::Session(SessionEvent::Disconnected(_))) => {
                // The supervisor subscribes again by itself when it is back.
                port.listening.store(false, Ordering::Release);
            }
            // Events were dropped: ask again from the last one handled.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => due = soon(),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Ok(_) => {}
        }
    }
    port.listening.store(false, Ordering::Release);
    let _ = port.supervisor.close(INTAKE).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Keys, Kind};

    fn record(relay: &Keys, kind: u16, tags: &[&[&str]]) -> Event {
        EventBuilder::new(Kind::Custom(kind), "")
            .tags(
                tags.iter()
                    .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
            )
            .sign_with_keys(relay)
            .unwrap()
    }

    #[test]
    fn channel_facts_come_only_from_the_relays_own_records() {
        let (relay, stranger) = (Keys::generate(), Keys::generate());
        let relay_hex = relay.public_key().to_hex();
        let channel = Uuid::new_v4();
        let id = channel.to_string();
        let (agent, owner) = ("a".repeat(64), "b".repeat(64));
        let members = record(
            &relay,
            39_002,
            &[&["d", &id], &["p", &agent, "bot"], &["p", &owner]],
        );
        let stream = record(
            &relay,
            39_000,
            &[&["d", &id], &["name", "general"], &["t", "stream"]],
        );
        let facts = channel_facts(channel, &relay_hex, &[stream.clone(), members.clone()]).unwrap();
        assert_eq!(
            (facts.kind, facts.name.as_str()),
            (PlaceKind::Channel, "general")
        );
        assert_eq!(facts.members, vec![agent.clone(), owner.clone()]);
        assert!(facts.read_only.is_empty());
        // "Viewing only" in Buzz 1.0 is the `guest` role: a member who may
        // not write. Any other role may.
        let viewing = record(
            &relay,
            39_002,
            &[
                &["d", &id],
                &["p", &agent, "", "guest"],
                &["p", &owner, "", "owner"],
                &["p", &"d".repeat(64), "", "bot"],
            ],
        );
        let facts_viewing = channel_facts(channel, &relay_hex, &[stream.clone(), viewing]).unwrap();
        assert_eq!(facts_viewing.read_only, vec![agent.clone()]);
        assert_eq!(
            facts_viewing.may_write(&agent),
            Err(crate::agents::runtime::admission::Unwritable::ReadOnly)
        );
        assert_eq!(facts_viewing.may_write(&owner), Ok(()));
        assert_eq!(facts_viewing.may_write(&"d".repeat(64)), Ok(()));
        assert_eq!(
            facts_viewing.may_write(&"e".repeat(64)),
            Err(crate::agents::runtime::admission::Unwritable::NotAMember)
        );
        let kind_of = |metadata: Event| {
            channel_facts(channel, &relay_hex, &[metadata, members.clone()])
                .unwrap()
                .kind
        };
        assert_eq!(
            kind_of(record(
                &relay,
                39_000,
                &[&["d", &id], &["t", "dm"], &["hidden"]]
            )),
            PlaceKind::Dm
        );
        assert_eq!(
            kind_of(record(&relay, 39_000, &[&["d", &id], &["hidden"]])),
            PlaceKind::Dm
        );
        assert_eq!(
            kind_of(record(
                &relay,
                39_000,
                &[
                    &["d", &id],
                    &["t", "stream"],
                    &["t", "buzz.solo"],
                    &["private"]
                ]
            )),
            PlaceKind::Solo
        );
        // A hidden system channel is not a DM; a label is not a type.
        assert_eq!(
            kind_of(record(
                &relay,
                39_000,
                &[&["d", &id], &["t", "system"], &["hidden"]]
            )),
            PlaceKind::Channel
        );
        assert_eq!(
            kind_of(record(&relay, 39_000, &[&["d", &id], &["t", "buzz.solo"]])),
            PlaceKind::Channel
        );
        // Records signed by anyone else, or of another channel, are not facts.
        let forged = record(
            &stranger,
            39_002,
            &[&["d", &id], &["p", &agent], &["p", &"c".repeat(64)]],
        );
        assert_eq!(
            channel_facts(channel, &relay_hex, &[stream.clone(), forged]),
            None
        );
        let elsewhere = record(
            &relay,
            39_002,
            &[&["d", &Uuid::new_v4().to_string()], &["p", &agent]],
        );
        assert_eq!(
            channel_facts(channel, &relay_hex, &[stream, elsewhere]),
            None
        );
    }

    /// Found on the pinned relay: a stored membership notice comes back with
    /// every new subscription. Acted on each time, the intake subscribed
    /// twice a second until the relay's quota stopped it.
    /// Found against Buzz 1.0: a publish that never left bzz was taken for
    /// the relay's refusal, and the reply it carried was thrown away.
    #[test]
    fn only_the_relays_own_no_is_a_verdict() {
        for refusal in [
            "restricted: only members can post in this channel",
            "restricted: not a channel member",
            "blocked: you are banned",
            "invalid: bad signature",
            "some words of a relay bzz does not know",
            "",
        ] {
            assert!(is_verdict(refusal), "{refusal}");
        }
        for later in [
            "rate-limited: quota exceeded; retry in 17s",
            "Rate-Limited: slow down",
            "error: database error: connection reset",
        ] {
            assert!(!is_verdict(later), "{later}");
        }
    }

    #[test]
    fn a_membership_notice_is_acted_on_once() {
        let relay = Keys::generate();
        let notice = record(&relay, 44_100, &[&["p", &"a".repeat(64)]]);
        let another = record(&relay, 44_101, &[&["p", &"a".repeat(64)]]);
        let mut notices = Notices::default();
        assert!(notices.is_new(&notice));
        assert!(!notices.is_new(&notice), "the same notice replayed");
        assert!(notices.is_new(&another));
        assert!(!notices.is_new(&notice) && !notices.is_new(&another));
    }

    #[test]
    fn the_intake_asks_for_mentions_and_for_the_owners_dm_only() {
        let (agent, owner, other) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
        let facts = |kind, members: &[&String]| ChannelFacts {
            id: Uuid::new_v4(),
            name: String::new(),
            kind,
            members: members.iter().map(|member| (*member).clone()).collect(),
            read_only: Vec::new(),
        };
        let channel = facts(PlaceKind::Channel, &[&agent, &owner, &other]);
        let solo = facts(PlaceKind::Solo, &[&agent, &owner]);
        let dm = facts(PlaceKind::Dm, &[&agent, &owner]);
        let group = facts(PlaceKind::Dm, &[&agent, &owner, &other]);
        let theirs = facts(PlaceKind::Dm, &[&agent, &other]);
        let filters = intake_filters(
            &agent,
            &owner,
            &[channel.clone(), solo.clone(), dm.clone(), group, theirs],
            100,
        );
        assert_eq!(filters.len(), 3);
        assert_eq!(filters[0]["kinds"], json!([44_100, 44_101]));
        // Mentions in channels and Solo chats.
        assert_eq!(
            filters[1]["#h"],
            json!([channel.id.to_string(), solo.id.to_string()])
        );
        assert_eq!(filters[1]["#p"], json!([agent]));
        assert_eq!(filters[1]["since"], 100);
        // Every message of the DM with the owner alone, and of no other DM.
        assert_eq!(filters[2]["#h"], json!([dm.id.to_string()]));
        assert!(filters[2].get("#p").is_none());
        assert_eq!(intake_filters(&agent, &owner, &[], 100).len(), 1);
    }

    #[test]
    fn a_message_older_than_a_day_is_never_a_request() {
        let now = 1_800_000_000;
        assert!(!too_old(now, now));
        assert!(!too_old(now - CATCH_UP.as_secs(), now));
        assert!(!too_old(now - MAX_REQUEST_AGE.as_secs(), now));
        assert!(too_old(now - MAX_REQUEST_AGE.as_secs() - 1, now));
        assert!(too_old(0, now));
        // A finished request is kept far longer than a message can be a
        // request, so one that was pruned cannot be admitted again.
        assert!(
            MAX_REQUEST_AGE.as_secs() * 7 < crate::store::agent_runtime::SETTLED_RETENTION_SECONDS
        );
    }
}
