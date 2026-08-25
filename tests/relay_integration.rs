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

const PIN: &str = "9f55bf67456be10ff7c8238bf0d9e12e582848f6";

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
    let keys_b = Keys::generate();
    let keys_c = Keys::generate();
    let keys_outsider = Keys::generate();
    let pubkey_b = keys_b.public_key().to_hex();
    let pubkey_c = keys_c.public_key().to_hex();
    let pubkey_outsider = keys_outsider.public_key().to_hex();
    let _ = seed_member(&source, &pubkey_b);
    let _ = seed_member(&source, &pubkey_c);
    let _ = seed_member(&source, &pubkey_outsider);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let owner_keys = keys.clone();
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

    let signer_b = SignerHandle::spawn(keys_b);
    let (session_b, _) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer_b.clone(),
    )
    .await
    .unwrap();
    let http_b = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer_b.clone(),
    )
    .unwrap();
    let signer_outsider = SignerHandle::spawn(keys_outsider);
    let http_outsider = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer_outsider.clone(),
    )
    .unwrap();

    let dm_open = signer
        .sign(buzz_sdk::build_dm_open(&[pubkey_b.as_str()]).unwrap())
        .await
        .unwrap();
    let dm_ack = session.publish(dm_open).await.unwrap();
    assert!(dm_ack.accepted, "{}", dm_ack.message);
    let dm_channel = command_channel_id(&dm_ack.message);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let dm_token = format!("bzz-dm-search-{}", uuid::Uuid::new_v4().simple());
    let dm_message = signer
        .sign(
            buzz_sdk::build_message(
                dm_channel,
                &dm_token,
                None,
                &[pubkey_b.as_str()],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(dm_message.clone()).await.unwrap().accepted);
    let dm_reply = signer_b
        .sign(
            buzz_sdk::build_message(
                dm_channel,
                "generated DM reply",
                Some(&buzz_sdk::ThreadRef {
                    root_event_id: dm_message.id,
                    parent_event_id: dm_message.id,
                }),
                &[pubkey.as_str()],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session_b.publish(dm_reply.clone()).await.unwrap().accepted);

    let b_dm_results = http_b
        .query(&[QueryFilter {
            kinds: vec![9],
            search: Some(dm_token.clone()),
            search_mode: Some(bzz::protocol::types::SearchMode::Prefix),
            page: Some(0),
            limit: Some(20),
            ..QueryFilter::default()
        }
        .tag("h", [dm_channel.to_string()])])
        .await
        .unwrap();
    assert!(b_dm_results.iter().any(|event| event.id == dm_message.id));
    let outsider_dm = http_outsider
        .query(&[QueryFilter {
            kinds: vec![9],
            search: Some(dm_token.clone()),
            search_mode: Some(bzz::protocol::types::SearchMode::Prefix),
            page: Some(0),
            limit: Some(20),
            ..QueryFilter::default()
        }
        .tag("h", [dm_channel.to_string()])])
        .await
        .unwrap();
    assert!(
        outsider_dm.is_empty(),
        "a non-member received a DM search hit"
    );
    let mentions = http_b
        .query(&[QueryFilter {
            kinds: vec![9, 40_002],
            limit: Some(100),
            ..QueryFilter::default()
        }
        .tag("p", [pubkey_b.clone()])])
        .await
        .unwrap();
    assert!(mentions.iter().any(|event| event.id == dm_message.id));

    let add_c = signer
        .sign(buzz_sdk::build_dm_add_member(dm_channel, &pubkey_c).unwrap())
        .await
        .unwrap();
    let add_ack = session.publish(add_c).await.unwrap();
    assert!(add_ack.accepted, "{}", add_ack.message);
    let group_dm = command_channel_id(&add_ack.message);
    assert_ne!(group_dm, dm_channel, "adding a member must open a new DM");

    let hide = signer
        .sign(
            EventBuilder::new(Kind::Custom(41_012), "").tags([nostr::Tag::parse([
                "h",
                &dm_channel.to_string(),
            ])
            .unwrap()]),
        )
        .await
        .unwrap();
    assert!(session.publish(hide).await.unwrap().accepted);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let hidden = http
        .query(&[QueryFilter {
            kinds: vec![30_622],
            limit: Some(1),
            ..QueryFilter::default()
        }
        .tag("p", [pubkey.clone()])
        .tag("d", [pubkey.clone()])])
        .await
        .unwrap();
    assert!(hidden.iter().any(|event| {
        bzz::protocol::events::tag_values(event, "h").contains(&dm_channel.to_string())
    }));
    let foreign_visibility = http_outsider
        .query(&[QueryFilter {
            kinds: vec![30_622],
            limit: Some(1),
            ..QueryFilter::default()
        }
        .tag("p", [pubkey.clone()])
        .tag("d", [pubkey.clone()])])
        .await;
    assert!(foreign_visibility.is_err());
    let reopen = signer
        .sign(buzz_sdk::build_dm_open(&[pubkey_b.as_str()]).unwrap())
        .await
        .unwrap();
    let reopen_ack = session.publish(reopen).await.unwrap();
    assert!(reopen_ack.accepted);
    assert_eq!(command_channel_id(&reopen_ack.message), dm_channel);

    let gift_token = format!("gift-private-{}", uuid::Uuid::new_v4().simple());
    let ephemeral = Keys::generate();
    let gift = EventBuilder::new(Kind::Custom(1_059), &gift_token)
        .tags([nostr::Tag::parse(["p", &pubkey_b]).unwrap()])
        .sign_with_keys(&ephemeral)
        .unwrap();
    assert!(session.publish(gift).await.unwrap().accepted);
    let gift_search = http_b
        .query(&[QueryFilter {
            kinds: vec![1_059],
            search: Some(gift_token),
            search_mode: Some(bzz::protocol::types::SearchMode::Prefix),
            page: Some(0),
            limit: Some(20),
            ..QueryFilter::default()
        }
        .tag("p", [pubkey_b.clone()])])
        .await
        .unwrap();
    assert!(
        gift_search.is_empty(),
        "NIP-17 gift wraps must not be searchable"
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

    // Deterministic remote managed-agent interoperability fixture. It uses a
    // dedicated Nostr identity and signed public records, but no model, ACP
    // process, tool, memory, or observer stream.
    let agent_keys = Keys::generate();
    let agent_pubkey = agent_keys.public_key().to_hex();
    let _ = seed_member(&source, &agent_pubkey);
    let agent_signer = SignerHandle::spawn(agent_keys.clone());
    let (agent_session, _) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        agent_signer.clone(),
    )
    .await
    .unwrap();
    let add_agent = signer
        .sign(
            buzz_sdk::build_add_member(channel, &agent_pubkey, Some(buzz_sdk::MemberRole::Bot))
                .unwrap(),
        )
        .await
        .unwrap();
    let add_agent_ack = session.publish(add_agent).await.unwrap();
    assert!(add_agent_ack.accepted, "{}", add_agent_ack.message);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let auth =
        buzz_sdk::nip_oa::compute_auth_tag(&owner_keys, &agent_keys.public_key(), "").unwrap();
    let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
    let agent_profile = EventBuilder::new(
        Kind::Metadata,
        serde_json::json!({"display_name":"bzz deterministic remote agent"}).to_string(),
    )
    .tags([nostr::Tag::parse(auth).unwrap()])
    .sign_with_keys(&agent_keys)
    .unwrap();
    assert!(
        agent_session
            .publish(agent_profile.clone())
            .await
            .unwrap()
            .accepted
    );
    let agent_declaration = EventBuilder::new(
        Kind::Custom(10_100),
        serde_json::json!({
            "display_name":"bzz deterministic remote agent",
            "capabilities":["messages"],
            "status":"online"
        })
        .to_string(),
    )
    .sign_with_keys(&agent_keys)
    .unwrap();
    assert!(
        agent_session
            .publish(agent_declaration.clone())
            .await
            .unwrap()
            .accepted
    );
    let agent_policy = signer
        .sign(
            EventBuilder::new(
                Kind::Custom(30_177),
                serde_json::json!({
                    "name":"bzz deterministic remote agent",
                    "parallelism":1,
                    "respond_to":"owner-only"
                })
                .to_string(),
            )
            .tags([nostr::Tag::parse(["d", &agent_pubkey]).unwrap()]),
        )
        .await
        .unwrap();
    assert!(
        session
            .publish(agent_policy.clone())
            .await
            .unwrap()
            .accepted
    );

    let agent_mention = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                "@bzz-agent deterministic invocation",
                None,
                &[agent_pubkey.as_str()],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        session
            .publish(agent_mention.clone())
            .await
            .unwrap()
            .accepted
    );
    assert_eq!(
        bzz::protocol::events::tag_values(&agent_mention, "p"),
        vec![agent_pubkey.clone()]
    );
    let agent_reaction = agent_signer
        .sign(buzz_sdk::build_reaction(agent_mention.id, "👀").unwrap())
        .await
        .unwrap();
    assert!(
        agent_session
            .publish(agent_reaction)
            .await
            .unwrap()
            .accepted
    );
    let agent_reply = agent_signer
        .sign(
            buzz_sdk::build_message(
                channel,
                "deterministic remote-agent reply",
                Some(&buzz_sdk::ThreadRef {
                    root_event_id: agent_mention.id,
                    parent_event_id: agent_mention.id,
                }),
                &[pubkey.as_str()],
                false,
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        agent_session
            .publish(agent_reply.clone())
            .await
            .unwrap()
            .accepted
    );

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

    let mut generated_png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(3, 2)
        .write_to(&mut generated_png, image::ImageFormat::Png)
        .unwrap();
    let generated_png = generated_png.into_inner();
    let image_source = media_root.path().join("generated.png");
    tokio::fs::write(&image_source, &generated_png)
        .await
        .unwrap();
    let image_attachment = media_client
        .upload(&image_source, "image/png", Some("generated.png".into()))
        .await
        .unwrap();
    let image_event = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                &image_attachment.markdown_line(),
                None,
                &[],
                false,
                &[image_attachment.imeta_tag()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(image_event.clone()).await.unwrap().accepted);
    let image_download = media_root.path().join("downloaded.png");
    media_client
        .fetch(&image_attachment, &image_download)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(&image_download).await.unwrap(),
        generated_png
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
    assert!(directory_report.channel_ids.contains(&dm_channel));
    assert!(directory_report.channel_ids.contains(&group_dm));
    assert!(directory_report.agent_candidates >= 1);
    assert!(directory_report.verified_agents >= 1);
    let verified_agent = local_store
        .call({
            let pubkey = pubkey.clone();
            let agent_pubkey = agent_pubkey.clone();
            move |store| store.remote_agent(local_community, &agent_pubkey, &pubkey, Some(channel))
        })
        .await
        .unwrap()
        .expect("deterministic remote agent must be verified in its channel");
    assert_eq!(verified_agent.owner_pubkey, pubkey);
    assert_eq!(
        verified_agent.eligibility,
        bzz::agents::Eligibility::Eligible
    );
    local_store
        .call({
            let pubkey = verified_agent.owner_pubkey.clone();
            let agent_pubkey = verified_agent.pubkey.clone();
            move |store| {
                store.validate_agent_mentions(local_community, channel, &pubkey, &[agent_pubkey])
            }
        })
        .await
        .unwrap();
    let agent_backfill = backfill::channel(local_community, channel, &http, &local_store, 100)
        .await
        .unwrap();
    assert!(agent_backfill.content_events >= 2);
    let agent_reply_cached = local_store
        .call(move |store| {
            Ok(store
                .thread(local_community, &agent_mention.id.to_hex(), 500)?
                .iter()
                .any(|message| message.event_id == agent_reply.id.to_hex()))
        })
        .await
        .unwrap();
    assert!(agent_reply_cached);
    let cached_channels = local_store
        .call(move |store| store.channels(local_community))
        .await
        .unwrap();
    assert!(
        cached_channels
            .iter()
            .any(|item| item.id == channel && item.is_member)
    );
    assert!(cached_channels.iter().any(|item| {
        item.id == dm_channel && item.is_member && item.kind == bzz::domain::ChannelKind::Dm
    }));
    let dm_backfill = backfill::channel(local_community, dm_channel, &http, &local_store, 100)
        .await
        .unwrap();
    assert!(dm_backfill.content_events >= 2);
    let dm_search_query = bzz::store::models::MessageSearchQuery {
        fts_query: format!("\"{dm_token}\"*"),
        author: None,
        channel_id: Some(dm_channel),
        since: None,
        until: None,
        limit: 20,
    };
    let local_dm_search = local_store
        .call({
            let pubkey = pubkey.clone();
            move |store| store.search_messages(local_community, &pubkey, &dm_search_query)
        })
        .await
        .unwrap();
    assert!(
        local_dm_search
            .iter()
            .any(|result| result.event_id.as_deref() == Some(dm_message.id.to_hex().as_str()))
    );
    let inbox = local_store
        .call({
            let pubkey = pubkey.clone();
            move |store| store.inbox_items(local_community, &pubkey)
        })
        .await
        .unwrap();
    assert!(inbox.iter().any(|item| {
        item.conversation_id == format!("dm:{dm_channel}")
            && item.categories.contains(&bzz::domain::InboxCategory::Dm)
    }));

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
    let image_thread = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                &image_attachment.markdown_line(),
                Some(&buzz_sdk::ThreadRef {
                    root_event_id: root.id,
                    parent_event_id: root.id,
                }),
                &[],
                false,
                &[image_attachment.imeta_tag()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        session
            .publish(image_thread.clone())
            .await
            .unwrap()
            .accepted
    );
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
        &image_event,
        &image_thread,
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
    for expected in [&media_event, &image_event] {
        let projected = cached
            .iter()
            .find(|message| message.event_id == expected.id.to_hex())
            .expect("media message was cached");
        assert_eq!(projected.attachments.len(), 1);
    }
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
    let cached_thread_image = cached_thread
        .iter()
        .find(|message| message.event_id == image_thread.id.to_hex())
        .expect("thread image was cached");
    assert_eq!(cached_thread_image.attachments.len(), 1);
    assert_eq!(cached_thread_image.attachments[0].mime, "image/png");
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

    agent_session.shutdown().await;
    agent_signer.lock().await;
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
    session_b.shutdown().await;
    signer_b.lock().await;
    signer_outsider.lock().await;
    signer.lock().await;
}

fn command_channel_id(message: &str) -> uuid::Uuid {
    let json = message.strip_prefix("response:").unwrap_or(message);
    let value: serde_json::Value = serde_json::from_str(json).expect("DM command response JSON");
    uuid::Uuid::parse_str(
        value["channel_id"]
            .as_str()
            .expect("DM command response channel_id"),
    )
    .expect("DM command response UUID")
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
