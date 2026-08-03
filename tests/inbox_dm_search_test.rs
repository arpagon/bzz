mod support;

use std::collections::BTreeSet;

use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    domain::{ChannelKind, InboxCategory},
    protocol::http::HttpClient,
    realtime::supervisor::SupervisorHandle,
    service::{dms::DmService, search::SearchService},
    store::{Store, models::MessageSearchQuery, writer::StoreHandle},
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use uuid::Uuid;

use support::fake_relay::FakeRelay;

struct Fixture {
    store: Store,
    community: Uuid,
    own: Keys,
    relay: Keys,
}

impl Fixture {
    fn new() -> Self {
        let own = Keys::generate();
        let relay = Keys::generate();
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "fixture".into(),
            pubkey: own.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "fixture".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community(
                "fixture".into(),
                "wss://fixture.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        store
            .pin_relay_pubkey(community, &relay.public_key().to_hex())
            .unwrap();
        Self {
            store,
            community,
            own,
            relay,
        }
    }

    fn channel(&mut self, channel: Uuid, dm: bool, members: &[&Keys]) {
        let mut metadata_tags = vec![
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", if dm { "DM" } else { "general" }]).unwrap(),
            Tag::parse(["t", if dm { "dm" } else { "stream" }]).unwrap(),
            Tag::parse(["closed"]).unwrap(),
        ];
        if dm {
            metadata_tags.push(Tag::parse(["private"]).unwrap());
            metadata_tags.push(Tag::parse(["hidden"]).unwrap());
        } else {
            metadata_tags.push(Tag::parse(["public"]).unwrap());
        }
        let metadata = EventBuilder::new(Kind::Custom(39_000), "")
            .tags(metadata_tags)
            .sign_with_keys(&self.relay)
            .unwrap();
        self.store.apply_event(self.community, &metadata).unwrap();
        let mut membership_tags = vec![Tag::parse(["d", &channel.to_string()]).unwrap()];
        membership_tags.push(Tag::parse(["p", &self.own.public_key().to_hex()]).unwrap());
        for member in members {
            membership_tags.push(Tag::parse(["p", &member.public_key().to_hex()]).unwrap());
        }
        let membership = EventBuilder::new(Kind::Custom(39_002), "")
            .tags(membership_tags)
            .sign_with_keys(&self.relay)
            .unwrap();
        self.store.apply_event(self.community, &membership).unwrap();
    }

    fn message(
        &mut self,
        author: &Keys,
        channel: Uuid,
        content: &str,
        root: Option<&str>,
        mention: bool,
        created_at: u64,
    ) -> Event {
        let mut tags = vec![Tag::parse(["h", &channel.to_string()]).unwrap()];
        if let Some(root) = root {
            tags.push(Tag::parse(["e", root, "", "reply"]).unwrap());
        }
        if mention {
            tags.push(Tag::parse(["p", &self.own.public_key().to_hex()]).unwrap());
        }
        let event = EventBuilder::new(Kind::Custom(9), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(author)
            .unwrap();
        self.store.apply_event(self.community, &event).unwrap();
        event
    }
}

#[test]
fn dm_metadata_and_owner_visibility_are_distinct() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let other = Keys::generate();
    fixture.channel(channel, true, &[&other]);

    let visible = fixture.store.channels(fixture.community).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].kind, ChannelKind::Dm);
    assert!(
        !visible[0].is_hidden,
        "the NIP-29 hidden tag only classifies DMs"
    );
    fixture.message(&other, channel, "hiddensearchtoken", None, false, 90);
    let search = MessageSearchQuery {
        fts_query: r#""hiddensearch"*"#.into(),
        author: None,
        channel_id: Some(channel),
        since: None,
        until: None,
        limit: 20,
    };
    let own = fixture.own.public_key().to_hex();
    assert_eq!(
        fixture
            .store
            .search_messages(fixture.community, &own, &search)
            .unwrap()
            .len(),
        1
    );

    let visibility = visibility_event(
        &fixture.relay,
        fixture.own.public_key().to_hex().as_str(),
        &[channel],
        100,
    );
    fixture
        .store
        .apply_event(fixture.community, &visibility)
        .unwrap();
    assert!(
        fixture
            .store
            .channels(fixture.community)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .store
            .hidden_dms(fixture.community, &fixture.own.public_key().to_hex())
            .unwrap(),
        BTreeSet::from([channel])
    );
    assert!(
        fixture
            .store
            .search_messages(fixture.community, &own, &search)
            .unwrap()
            .is_empty(),
        "viewer-hidden DMs must not surface from local FTS"
    );

    let foreign = Keys::generate();
    let forged_owner = visibility_event(&fixture.relay, &foreign.public_key().to_hex(), &[], 101);
    assert!(
        fixture
            .store
            .apply_event(fixture.community, &forged_owner)
            .is_err()
    );

    let empty = visibility_event(
        &fixture.relay,
        fixture.own.public_key().to_hex().as_str(),
        &[],
        102,
    );
    fixture
        .store
        .apply_event(fixture.community, &empty)
        .unwrap();
    assert_eq!(fixture.store.channels(fixture.community).unwrap().len(), 1);
}

#[test]
fn membership_snapshots_are_bounded_and_reduce_deterministically() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let first = Keys::generate();
    fixture.channel(channel, true, &[&first]);
    let second = Keys::generate();
    let base = Timestamp::now().as_secs().saturating_add(10);
    let snapshot = |member: &Keys, created_at: u64| {
        EventBuilder::new(Kind::Custom(39_002), "")
            .tags([
                Tag::parse(["d", &channel.to_string()]).unwrap(),
                Tag::parse(["p", &fixture.own.public_key().to_hex()]).unwrap(),
                Tag::parse(["p", &member.public_key().to_hex()]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(&fixture.relay)
            .unwrap()
    };
    let older = snapshot(&first, base);
    let newer = snapshot(&second, base + 1);
    fixture
        .store
        .apply_event(fixture.community, &newer)
        .unwrap();
    fixture
        .store
        .apply_event(fixture.community, &older)
        .unwrap();
    assert_eq!(
        fixture
            .store
            .dm_participants(fixture.community, channel)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            fixture.own.public_key().to_hex(),
            second.public_key().to_hex(),
        ])
    );

    let duplicate = EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &fixture.own.public_key().to_hex()]).unwrap(),
            Tag::parse(["p", &fixture.own.public_key().to_hex()]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(base + 2))
        .sign_with_keys(&fixture.relay)
        .unwrap();
    assert!(
        fixture
            .store
            .apply_event(fixture.community, &duplicate)
            .is_err()
    );
}

#[test]
fn visibility_same_second_uses_nip33_event_id_tie_break() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let other = Keys::generate();
    fixture.channel(channel, true, &[&other]);
    let hidden = visibility_event(
        &fixture.relay,
        fixture.own.public_key().to_hex().as_str(),
        &[channel],
        200,
    );
    let visible = visibility_event(
        &fixture.relay,
        fixture.own.public_key().to_hex().as_str(),
        &[],
        200,
    );
    let (winner, loser, winner_hidden) = if hidden.id.to_hex() < visible.id.to_hex() {
        (hidden, visible, true)
    } else {
        (visible, hidden, false)
    };
    fixture
        .store
        .apply_event(fixture.community, &loser)
        .unwrap();
    fixture
        .store
        .apply_event(fixture.community, &winner)
        .unwrap();
    assert_eq!(
        fixture
            .store
            .channels(fixture.community)
            .unwrap()
            .is_empty(),
        winner_hidden
    );
}

#[test]
fn local_fts_is_tenant_authorized_and_tracks_deletion() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let author = Keys::generate();
    fixture.channel(channel, false, &[&author]);
    let message = fixture.message(
        &author,
        channel,
        "searchable_unique_token",
        None,
        false,
        300,
    );
    let query = MessageSearchQuery {
        fts_query: r#""searchable"*"#.into(),
        author: None,
        channel_id: None,
        since: None,
        until: None,
        limit: 20,
    };
    let own = fixture.own.public_key().to_hex();
    let hits = fixture
        .store
        .search_messages(fixture.community, &own, &query)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].event_id.as_deref(),
        Some(message.id.to_hex().as_str())
    );

    let deletion = buzz_sdk::build_delete_compat(channel, message.id)
        .unwrap()
        .sign_with_keys(&author)
        .unwrap();
    fixture
        .store
        .apply_event(fixture.community, &deletion)
        .unwrap();
    assert!(
        fixture
            .store
            .search_messages(fixture.community, &own, &query)
            .unwrap()
            .is_empty()
    );
    fixture.store.search_integrity().unwrap();
}

#[test]
fn inbox_groups_mentions_threads_dms_actions_and_drafts() {
    let mut fixture = Fixture::new();
    let stream = Uuid::new_v4();
    let dm = Uuid::new_v4();
    let author = Keys::generate();
    fixture.channel(stream, false, &[&author]);
    fixture.channel(dm, true, &[&author]);
    let root = fixture.message(&fixture.own.clone(), stream, "root", None, false, 400);
    fixture.message(
        &author,
        stream,
        "thread mention",
        Some(&root.id.to_hex()),
        true,
        401,
    );
    fixture.message(&author, dm, "private hello", None, false, 402);
    let broadcast = EventBuilder::new(Kind::Custom(9), "broadcast mention")
        .tags([
            Tag::parse(["h", &stream.to_string()]).unwrap(),
            Tag::parse(["e", &root.id.to_hex(), "", "reply"]).unwrap(),
            Tag::parse(["p", &fixture.own.public_key().to_hex()]).unwrap(),
            Tag::parse(["broadcast", "1"]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(402))
        .sign_with_keys(&author)
        .unwrap();
    fixture
        .store
        .apply_event(fixture.community, &broadcast)
        .unwrap();
    let action = EventBuilder::new(Kind::Custom(46_010), "approval waiting")
        .tags([Tag::parse(["p", &fixture.own.public_key().to_hex()]).unwrap()])
        .custom_created_at(Timestamp::from(403))
        .sign_with_keys(&author)
        .unwrap();
    fixture
        .store
        .apply_event(fixture.community, &action)
        .unwrap();
    fixture
        .store
        .save_draft(
            fixture.community,
            stream,
            Some(&root.id.to_hex()),
            "draft reply",
        )
        .unwrap();

    let own = fixture.own.public_key().to_hex();
    let items = fixture.store.inbox_items(fixture.community, &own).unwrap();
    let thread = items
        .iter()
        .find(|item| item.conversation_id == format!("thread:{}", root.id.to_hex()))
        .unwrap();
    assert!(thread.categories.contains(&InboxCategory::Mention));
    assert!(thread.categories.contains(&InboxCategory::Thread));
    assert!(thread.categories.contains(&InboxCategory::Draft));
    assert_eq!(thread.unread_count, 1);
    assert_eq!(thread.draft_count, 1);
    assert!(
        items
            .iter()
            .any(|item| item.categories.contains(&InboxCategory::Dm))
    );
    assert!(
        items
            .iter()
            .any(|item| item.categories.contains(&InboxCategory::NeedsAction))
    );
    let broadcast_item = items
        .iter()
        .find(|item| item.conversation_id == format!("event:{}", broadcast.id.to_hex()))
        .unwrap();
    assert!(broadcast_item.categories.contains(&InboxCategory::Mention));
    assert!(!broadcast_item.categories.contains(&InboxCategory::Thread));
    assert!(broadcast_item.thread_root.is_none());

    fixture
        .store
        .set_inbox_override(
            fixture.community,
            &own,
            &thread.conversation_id,
            false,
            Some(thread.created_at),
        )
        .unwrap();
    assert_eq!(
        fixture
            .store
            .inbox_items(fixture.community, &own)
            .unwrap()
            .iter()
            .find(|item| item.conversation_id == thread.conversation_id)
            .unwrap()
            .unread_count,
        0,
        "cache-only mark-read must not require a signed marker"
    );

    fixture
        .store
        .advance_read(
            fixture.community,
            &own,
            &format!("thread:{}", root.id.to_hex()),
            500,
            true,
        )
        .unwrap();
    let items = fixture.store.inbox_items(fixture.community, &own).unwrap();
    let thread = items
        .iter()
        .find(|item| item.conversation_id == format!("thread:{}", root.id.to_hex()))
        .unwrap();
    assert_eq!(thread.unread_count, 0);
    fixture
        .store
        .set_inbox_override(fixture.community, &own, &thread.conversation_id, true, None)
        .unwrap();
    assert!(
        fixture
            .store
            .inbox_items(fixture.community, &own)
            .unwrap()
            .iter()
            .find(|item| item.conversation_id == thread.conversation_id)
            .unwrap()
            .forced_unread
    );
}

#[test]
fn exact_channel_and_thread_context_keeps_targets_outside_default_windows() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let author = Keys::generate();
    fixture.channel(channel, false, &[&author]);
    let old_target = fixture.message(&author, channel, "old exact target", None, false, 10);
    for index in 0..501_u64 {
        fixture.message(
            &author,
            channel,
            &format!("newer root {index}"),
            None,
            false,
            20 + index,
        );
    }
    assert!(
        !fixture
            .store
            .messages(fixture.community, channel, 500)
            .unwrap()
            .iter()
            .any(|message| message.event_id == old_target.id.to_hex())
    );
    assert!(
        fixture
            .store
            .messages_around(fixture.community, channel, &old_target.id.to_hex(), 50)
            .unwrap()
            .iter()
            .any(|message| message.event_id == old_target.id.to_hex())
    );

    let root = fixture.message(
        &fixture.own.clone(),
        channel,
        "thread root",
        None,
        false,
        1_000,
    );
    for index in 0..501_u64 {
        fixture.message(
            &author,
            channel,
            &format!("reply {index}"),
            Some(&root.id.to_hex()),
            false,
            1_001 + index,
        );
    }
    let reply_target = fixture.message(
        &author,
        channel,
        "late exact reply",
        Some(&root.id.to_hex()),
        false,
        2_000,
    );
    assert!(
        !fixture
            .store
            .thread(fixture.community, &root.id.to_hex(), 500)
            .unwrap()
            .iter()
            .any(|message| message.event_id == reply_target.id.to_hex())
    );
    assert!(
        fixture
            .store
            .thread_around(
                fixture.community,
                channel,
                &root.id.to_hex(),
                &reply_target.id.to_hex(),
                50,
            )
            .unwrap()
            .iter()
            .any(|message| message.event_id == reply_target.id.to_hex())
    );
}

#[tokio::test]
async fn remote_nip50_results_are_verified_stored_and_merged_with_local_sections() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let author = Keys::generate();
    fixture.channel(channel, false, &[&author]);
    let channels = fixture.store.channels(fixture.community).unwrap();
    let messages = (0..25)
        .map(|index| {
            EventBuilder::new(
                Kind::Custom(9),
                format!("remote_unique searchable message {index}"),
            )
            .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
            .sign_with_keys(&author)
            .unwrap()
        })
        .collect::<Vec<_>>();
    let profile = EventBuilder::new(Kind::Metadata, r#"{"display_name":"remote_unique person"}"#)
        .sign_with_keys(&author)
        .unwrap();
    let mut remote_events = vec![profile];
    remote_events.extend(messages.iter().cloned());
    let (http_base, stop_http) = start_query_server(remote_events).await;
    let store = StoreHandle::spawn(fixture.store).unwrap();
    let signer = SignerHandle::spawn(fixture.own.clone());
    let service = SearchService::new(
        fixture.community,
        HttpClient::new(http_base, signer.clone()).unwrap(),
        store,
    );
    let output = service
        .execute(
            "remote_unique",
            &fixture.own.public_key().to_hex(),
            &channels,
            &std::collections::HashMap::new(),
        )
        .await
        .unwrap();
    assert!(
        output
            .results
            .iter()
            .any(|result| result.event_id.as_deref() == Some(messages[0].id.to_hex().as_str()))
    );
    let remote_messages = output
        .results
        .iter()
        .filter(|result| result.kind == bzz::domain::SearchResultKind::Message)
        .collect::<Vec<_>>();
    assert_eq!(
        remote_messages.len(),
        25,
        "the second bounded page must merge"
    );
    assert!(
        remote_messages
            .iter()
            .all(|result| result.remote_rank.is_some())
    );
    signer.lock().await;
    let _ = stop_http.send(());
}

#[tokio::test]
async fn accepted_duplicate_dm_ack_recovers_from_exact_discovery_state() {
    let mut fixture = Fixture::new();
    let channel = Uuid::new_v4();
    let other = Keys::generate();
    fixture.channel(channel, true, &[&other]);
    let metadata = dm_metadata(&fixture.relay, channel);
    let membership = dm_membership(&fixture.relay, channel, &fixture.own, &other);
    let (http_base, stop_http) = start_query_server_after(vec![metadata, membership], 3).await;
    let store = StoreHandle::spawn(fixture.store).unwrap();
    let signer = SignerHandle::spawn(fixture.own.clone());
    let fake_relay = FakeRelay::start_with_event_ack(true, "duplicate: already processed").await;
    let supervisor = SupervisorHandle::spawn(fake_relay.url.clone(), signer.clone());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let http = HttpClient::new(http_base, signer.clone()).unwrap();
    let service = DmService::new(
        fixture.community,
        signer.clone(),
        http,
        store,
        supervisor.clone(),
    );
    // Duplicate acknowledgements do not repeat the original response JSON.
    // Recovery must match the exact participant set without re-signing.
    let result = service
        .open(vec![other.public_key().to_hex()])
        .await
        .unwrap();
    assert_eq!(result.channel_id, channel);
    assert_eq!(result.created, None);
    supervisor.shutdown().await;
    signer.lock().await;
    fake_relay.stop();
    let _ = stop_http.send(());
}

#[tokio::test]
async fn rejected_dm_command_never_creates_local_authority() {
    let fixture = Fixture::new();
    let other = Keys::generate();
    let (http_base, stop_http) = start_query_server(Vec::new()).await;
    let store = StoreHandle::spawn(fixture.store).unwrap();
    let inspect = store.clone();
    let signer = SignerHandle::spawn(fixture.own.clone());
    let fake_relay = FakeRelay::start_with_event_ack(false, "forbidden: generated fixture").await;
    let supervisor = SupervisorHandle::spawn(fake_relay.url.clone(), signer.clone());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let service = DmService::new(
        fixture.community,
        signer.clone(),
        HttpClient::new(http_base, signer.clone()).unwrap(),
        store,
        supervisor.clone(),
    );
    let error = service
        .open(vec![other.public_key().to_hex()])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("forbidden"));
    let community = fixture.community;
    assert!(
        inspect
            .call(move |store| store.channels(community))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        inspect
            .call(move |store| store.pending_outbox(community))
            .await
            .unwrap()
            .is_empty()
    );
    supervisor.shutdown().await;
    signer.lock().await;
    fake_relay.stop();
    let _ = stop_http.send(());
}

async fn start_query_server(events: Vec<Event>) -> (url::Url, tokio::sync::oneshot::Sender<()>) {
    start_query_server_after(events, 0).await
}

async fn start_query_server_after(
    events: Vec<Event>,
    empty_responses: usize,
) -> (url::Url, tokio::sync::oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let events = std::sync::Arc::new(events);
    let request_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                accepted = listener.accept() => {
                    let (mut stream, _) = accepted.unwrap();
                    let count = request_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let delayed = count < empty_responses;
                    let events = events.clone();
                    tokio::spawn(async move {
                        let mut request = Vec::new();
                        let mut chunk = [0_u8; 4096];
                        loop {
                            let read = stream.read(&mut chunk).await.unwrap_or(0);
                            if read == 0 { break; }
                            request.extend_from_slice(&chunk[..read]);
                            if let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                                let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                                let length = headers.lines().find_map(|line| {
                                    line.split_once(':').and_then(|(name, value)|
                                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten())
                                }).unwrap_or(0);
                                if request.len() >= header_end + 4 + length { break; }
                            }
                        }
                        let header_end = request.windows(4).position(|value| value == b"\r\n\r\n").unwrap_or(request.len());
                        let filter = serde_json::from_slice::<serde_json::Value>(
                            request.get(header_end.saturating_add(4)..).unwrap_or_default(),
                        )
                        .ok()
                        .and_then(|value| value.as_array()?.first().cloned())
                        .unwrap_or_default();
                        let kinds = filter
                            .get("kinds")
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(serde_json::Value::as_u64)
                            .collect::<std::collections::BTreeSet<_>>();
                        let page = filter.get("page").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
                        let limit = filter.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(500) as usize;
                        let response_events = if delayed {
                            Vec::new()
                        } else {
                            events
                                .iter()
                                .filter(|event| kinds.contains(&u64::from(event.kind.as_u16())))
                                .skip(page.saturating_mul(limit))
                                .take(limit)
                                .cloned()
                                .collect::<Vec<_>>()
                        };
                        let body = serde_json::to_vec(&response_events).unwrap();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        stream.write_all(response.as_bytes()).await.unwrap();
                        stream.write_all(&body).await.unwrap();
                    });
                }
            }
        }
    });
    (
        url::Url::parse(&format!("http://{address}/")).unwrap(),
        stop_tx,
    )
}

fn dm_metadata(relay: &Keys, channel: Uuid) -> Event {
    EventBuilder::new(Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", "DM"]).unwrap(),
            Tag::parse(["t", "dm"]).unwrap(),
            Tag::parse(["closed"]).unwrap(),
            Tag::parse(["private"]).unwrap(),
            Tag::parse(["hidden"]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(
            Timestamp::now().as_secs().saturating_add(10),
        ))
        .sign_with_keys(relay)
        .unwrap()
}

fn dm_membership(relay: &Keys, channel: Uuid, own: &Keys, other: &Keys) -> Event {
    EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &own.public_key().to_hex()]).unwrap(),
            Tag::parse(["p", &other.public_key().to_hex()]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(
            Timestamp::now().as_secs().saturating_add(10),
        ))
        .sign_with_keys(relay)
        .unwrap()
}

fn visibility_event(relay: &Keys, owner: &str, hidden: &[Uuid], created_at: u64) -> Event {
    let mut tags = vec![
        Tag::parse(["d", owner]).unwrap(),
        Tag::parse(["p", owner]).unwrap(),
    ];
    for channel in hidden {
        tags.push(Tag::parse(["h", &channel.to_string()]).unwrap());
    }
    EventBuilder::new(Kind::Custom(30_622), "")
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(relay)
        .unwrap()
}
