use std::{collections::HashSet, process::Command, time::Duration};

use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    media::client::MediaClient,
    protocol::{http::HttpClient, types::QueryFilter},
    realtime::{
        session::{self, SessionEvent},
        supervisor::SupervisorHandle,
    },
    store::{Store, writer::StoreHandle},
    sync::{backfill, directory, outbox, read_state},
};
use nostr::{EventBuilder, Keys, Kind, Timestamp};

const PIN: &str = "ede26863345a518ec46edd6d7692e0281883491b";

#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_mvp_protocol_journey() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let head = Command::new("git")
        .args(["-C", &source, "rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(head.status.success());
    assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), PIN);
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let signer = SignerHandle::spawn(keys);
    let relay = url::Url::parse("ws://localhost:3030/").unwrap();
    let (session, mut events) = session::connect(relay, signer.clone()).await.unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let info = http.nip11().await.unwrap();
    assert!(info.is_object());
    assert_eq!(
        bzz::protocol::http::relay_signing_pubkey(&info),
        Some("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
    );
    let open_channels = http
        .query(&[QueryFilter {
            kinds: vec![39_000],
            limit: Some(100),
            ..QueryFilter::default()
        }])
        .await
        .unwrap();
    assert!(open_channels.iter().any(|event| {
        bzz::protocol::events::first_tag(event, "d").as_deref()
            == Some(channel.to_string().as_str())
    }));
    let join = signer
        .sign(buzz_sdk::build_join(channel).unwrap())
        .await
        .unwrap();
    assert!(session.publish(join).await.unwrap().accepted);
    tokio::time::sleep(Duration::from_millis(250)).await;
    session
        .subscribe(
            "general",
            vec![serde_json::json!({"kinds":[5,7,9],"#h":[channel.to_string()],"limit":10})],
        )
        .await
        .unwrap();
    while !matches!(events.recv().await,Some(SessionEvent::Eose(id)) if id=="general") {}
    let joined = http
        .query(&[QueryFilter {
            kinds: vec![39_002],
            limit: Some(100),
            ..QueryFilter::default()
        }
        .tag("p", [pubkey.clone()])])
        .await
        .unwrap();
    assert!(joined.iter().any(|event| {
        bzz::protocol::events::first_tag(event, "d").as_deref()
            == Some(channel.to_string().as_str())
    }));

    let media_client = MediaClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        "localhost:3030".into(),
        signer.clone(),
        2,
    )
    .unwrap();
    let media_root = tempfile::TempDir::new().unwrap();
    let media_source = media_root.path().join("generated.txt");
    tokio::fs::write(&media_source, b"generated bzz media fixture\n")
        .await
        .unwrap();
    let attachment = media_client
        .upload(
            &media_source,
            "application/octet-stream",
            Some("generated.txt".into()),
        )
        .await
        .unwrap();
    let media_event = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                &attachment.markdown_line(),
                None,
                &[],
                false,
                &[attachment.imeta_tag()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(media_event.clone()).await.unwrap().accepted);
    let media_download = media_root.path().join("downloaded.bin");
    media_client
        .fetch(&attachment, &media_download)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(&media_download).await.unwrap(),
        b"generated bzz media fixture\n"
    );

    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "integration".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "integration".into(),
    };
    let mut local_config = Config::default();
    local_config.identities.push(identity.clone());
    let local_community = local_config
        .add_community(
            "integration".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    let mut local_store = Store::open_memory().unwrap();
    local_store.sync_config(&local_config).unwrap();
    local_store
        .pin_relay_pubkey(
            local_community,
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        )
        .unwrap();
    let local_store = StoreHandle::spawn(local_store).unwrap();
    let directory_report = directory::refresh(local_community, &pubkey, &http, &local_store)
        .await
        .unwrap();
    assert!(directory_report.channel_ids.contains(&channel));
    let cached_channels = local_store
        .call(move |store| store.channels(local_community))
        .await
        .unwrap();
    assert!(
        cached_channels
            .iter()
            .any(|item| item.id == channel && item.is_member)
    );

    let profile = signer
        .sign(EventBuilder::new(
            Kind::Metadata,
            serde_json::json!({"name":"bzz-e2e","display_name":"bzz integration"}).to_string(),
        ))
        .await
        .unwrap();
    assert!(session.publish(profile.clone()).await.unwrap().accepted);
    let root = signer
        .sign(buzz_sdk::build_message(channel, "bzz root", None, &[], false, &[]).unwrap())
        .await
        .unwrap();
    assert!(session.publish(root.clone()).await.unwrap().accepted);
    let direct = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                "direct",
                Some(&buzz_sdk::ThreadRef {
                    root_event_id: root.id,
                    parent_event_id: root.id,
                }),
                &[],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(direct.clone()).await.unwrap().accepted);
    let nested = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                "nested",
                Some(&buzz_sdk::ThreadRef {
                    root_event_id: root.id,
                    parent_event_id: direct.id,
                }),
                &[],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(nested.clone()).await.unwrap().accepted);
    let reaction = signer
        .sign(buzz_sdk::build_reaction(root.id, "👍").unwrap())
        .await
        .unwrap();
    assert!(session.publish(reaction.clone()).await.unwrap().accepted);
    let remove = signer
        .sign(buzz_sdk::build_remove_reaction(reaction.id).unwrap())
        .await
        .unwrap();
    assert!(session.publish(remove.clone()).await.unwrap().accepted);
    let deletion = signer
        .sign(buzz_sdk::build_delete_compat(channel, direct.id).unwrap())
        .await
        .unwrap();
    assert!(session.publish(deletion.clone()).await.unwrap().accepted);

    let context_key = channel.to_string();
    let read_events = read_state::build_events(
        std::collections::BTreeMap::from([(context_key.clone(), 10)]),
        "integration-a",
        &["integration-a".into()],
        &signer,
        0,
    )
    .await
    .unwrap();
    assert!(
        session
            .publish(read_events[0].clone())
            .await
            .unwrap()
            .accepted
    );
    let decrypted = read_state::decrypt_event(&read_events[0], &signer)
        .await
        .unwrap();
    assert_eq!(decrypted.client_id, "integration-a");
    let read_events_b = read_state::build_events(
        std::collections::BTreeMap::from([(context_key.clone(), 20)]),
        "integration-b",
        &["integration-b".into()],
        &signer,
        read_events[0].created_at.as_secs(),
    )
    .await
    .unwrap();
    assert!(
        session
            .publish(read_events_b[0].clone())
            .await
            .unwrap()
            .accepted
    );
    let remote_slots = http
        .query(&[QueryFilter {
            authors: vec![pubkey.clone()],
            kinds: vec![30_078],
            limit: Some(10),
            ..QueryFilter::default()
        }
        .tag("t", ["read-state".to_owned()])])
        .await
        .unwrap();
    assert_eq!(remote_slots.len(), 2);
    let merged = read_state::merge_events(local_community, &remote_slots, &signer, &local_store)
        .await
        .unwrap();
    assert_eq!(merged.contexts[&context_key], 20);

    for event in [
        &profile,
        &root,
        &nested,
        &media_event,
        &read_events[0],
        &read_events_b[0],
    ] {
        let found = http
            .query(&[QueryFilter {
                ids: vec![event.id.to_hex()],
                limit: Some(1),
                ..QueryFilter::default()
            }])
            .await
            .unwrap();
        assert_eq!(
            found.first().map(|value| value.id),
            Some(event.id),
            "kind {} was not queryable after accepted publish",
            event.kind.as_u16()
        );
    }
    local_store
        .call({
            let root = root.clone();
            move |store| store.insert_outbox(local_community, &root)
        })
        .await
        .unwrap();
    let recovery_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let recovery = outbox::flush(local_community, &http, &recovery_supervisor, &local_store)
        .await
        .unwrap();
    assert_eq!(recovery.delivered, 1);
    assert_eq!(recovery.unknown, 0);
    let remaining = local_store
        .call(move |store| store.pending_outbox(local_community))
        .await
        .unwrap();
    assert!(remaining.is_empty());
    recovery_supervisor.shutdown().await;
    local_store
        .call({
            let root_id = root.id.to_hex();
            let root_created_at = root.created_at.as_secs();
            move |store| {
                store.save_sync_cursor(
                    local_community,
                    "history",
                    &channel.to_string(),
                    &bzz::store::models::SyncCursor {
                        high_created_at: root_created_at,
                        high_event_id: root_id,
                        complete_through: root_created_at,
                    },
                )
            }
        })
        .await
        .unwrap();

    for removed_id in [direct.id, reaction.id] {
        let found = http
            .query(&[QueryFilter {
                ids: vec![removed_id.to_hex()],
                limit: Some(1),
                ..QueryFilter::default()
            }])
            .await
            .unwrap();
        assert!(found.is_empty(), "deleted event remained queryable");
    }
    let auxiliary = http
        .query(&[QueryFilter {
            kinds: vec![5],
            limit: Some(20),
            ..QueryFilter::default()
        }
        .tag("e", [direct.id.to_hex(), reaction.id.to_hex()])])
        .await
        .unwrap();
    assert!(auxiliary.iter().any(|event| event.id == deletion.id));
    assert!(auxiliary.iter().any(|event| event.id == remove.id));

    let same_second = Timestamp::now();
    let mut published = Vec::new();
    for index in 0..505_u16 {
        let event = signer
            .sign(
                buzz_sdk::build_message(channel, &format!("dense {index}"), None, &[], false, &[])
                    .unwrap()
                    .custom_created_at(same_second),
            )
            .await
            .unwrap();
        let ack = session.publish(event.clone()).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        published.push(event.id.to_hex());
    }
    let first = http
        .query(&[QueryFilter {
            kinds: vec![9],
            until: Some(same_second.as_secs()),
            limit: Some(500),
            ..QueryFilter::default()
        }
        .tag("h", [channel.to_string()])])
        .await
        .unwrap();
    assert_eq!(first.len(), 500);
    let last = first.last().expect("full page has a continuation row");
    let oldest = (last.created_at.as_secs(), last.id.to_hex());
    let second = http
        .query(&[QueryFilter {
            kinds: vec![9],
            until: Some(oldest.0),
            before_id: Some(oldest.1),
            limit: Some(500),
            ..QueryFilter::default()
        }
        .tag("h", [channel.to_string()])])
        .await
        .unwrap();
    let fetched = first
        .into_iter()
        .chain(second)
        .map(|event| event.id.to_hex())
        .collect::<HashSet<_>>();
    assert!(
        published.iter().all(|id| fetched.contains(id)),
        "composite cursor lost dense IDs"
    );
    let report = backfill::channel(local_community, channel, &http, &local_store, 500)
        .await
        .unwrap();
    assert!(report.crossed_watermark);
    assert!(report.content_events >= published.len());
    let cached = local_store
        .call(move |store| store.messages(local_community, channel, 1_000))
        .await
        .unwrap();
    assert!(
        published
            .iter()
            .all(|id| cached.iter().any(|message| &message.event_id == id))
    );
    let root_id = root.id.to_hex();
    let cached_thread = local_store
        .call({
            let root_id = root_id.clone();
            move |store| store.thread(local_community, &root_id, 100)
        })
        .await
        .unwrap();
    assert!(
        cached_thread
            .iter()
            .any(|message| message.event_id == nested.id.to_hex())
    );
    let cached_reactions = local_store
        .call(move |store| store.reactions(local_community, &root_id))
        .await
        .unwrap();
    assert!(cached_reactions.iter().all(|reaction| reaction.deleted));
    let profile_events =
        directory::hydrate_profiles(local_community, [pubkey.clone()], &http, &local_store)
            .await
            .unwrap();
    assert_eq!(profile_events, 1);
    let cached_profile = local_store
        .call({
            let pubkey = pubkey.clone();
            move |store| store.profile(local_community, &pubkey)
        })
        .await
        .unwrap();
    assert_eq!(
        cached_profile.unwrap().display_name.as_deref(),
        Some("bzz integration")
    );

    session.shutdown().await;
    let (reconnected, _) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .await
    .unwrap();
    let stored = http
        .query(&[QueryFilter {
            ids: vec![root.id.to_hex()],
            limit: Some(1),
            ..QueryFilter::default()
        }])
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    reconnected.shutdown().await;
    signer.lock().await;
}

fn seed_member(source: &str, pubkey: &str) -> uuid::Uuid {
    let compose = format!("{source}/docker-compose.harness.yml");
    let sql = format!(
        "INSERT INTO relay_members (community_id,pubkey,role,added_by) SELECT id,'{pubkey}','owner',NULL FROM communities WHERE lower(host)='localhost:3030' ON CONFLICT (community_id,pubkey) DO UPDATE SET role='owner',updated_at=now();"
    );
    let status = Command::new("docker")
        .args([
            "compose",
            "-p",
            "buzz-harness",
            "-f",
            &compose,
            "exec",
            "-T",
            "postgres",
            "psql",
            "-U",
            "buzz",
            "-d",
            "buzz",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
            &sql,
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let output = Command::new("docker")
        .args([
            "compose",
            "-p",
            "buzz-harness",
            "-f",
            &compose,
            "exec",
            "-T",
            "postgres",
            "psql",
            "-U",
            "buzz",
            "-d",
            "buzz",
            "-Atc",
            "SELECT id FROM channels WHERE name='general' LIMIT 1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    uuid::Uuid::parse_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap()
}
