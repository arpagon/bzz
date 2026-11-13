mod support;

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

const PIN: &str = "bf3bdd0eaefe39289bf9620594f254c7f68897f5";

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
    // The pinned harness generates a fresh relay key per run; NIP-11 is the
    // trusted origin-bound source, exactly as production pinning uses it.
    let harness_relay_pubkey = bzz::protocol::http::relay_signing_pubkey(&info)
        .expect("pinned relay advertises its signing key")
        .to_owned();

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
    let dm_token = format!("bzzdmsearch{}", uuid::Uuid::new_v4().simple());
    let dm_message = signer
        .sign(
            buzz_sdk::build_message(
                dm_channel,
                &dm_token,
                None,
                &[pubkey_b.as_str()],
                false,
                &[],
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
            vec![serde_json::json!({"kinds":[5,7,9,39005],"#h":[channel.to_string()],"limit":10})],
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
    session
        .subscribe(
            "agent-typing-live",
            bzz::realtime::subscriptions::agent_typing(channel, Timestamp::now().as_secs()),
        )
        .await
        .unwrap();
    while !matches!(events.recv().await,Some(SessionEvent::Eose(id)) if id=="agent-typing-live") {}
    let agent_typing = EventBuilder::new(Kind::Custom(20_002), "")
        .tags([
            nostr::Tag::parse(["h", &channel.to_string()]).unwrap(),
            nostr::Tag::parse(["e", &agent_mention.id.to_hex(), "", "reply"]).unwrap(),
        ])
        .sign_with_keys(&agent_keys)
        .unwrap();
    assert!(
        agent_session
            .publish(agent_typing.clone())
            .await
            .unwrap()
            .accepted
    );
    let observed_typing = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SessionEvent::Event {
                subscription,
                event,
            }) = events.recv().await
                && subscription == "agent-typing-live"
                && event.id == agent_typing.id
            {
                break event;
            }
        }
    })
    .await
    .expect("live agent typing event");
    assert_eq!(observed_typing.pubkey.to_hex(), agent_pubkey);
    session.close("agent-typing-live").await.unwrap();
    session
        .subscribe(
            "agent-typing-history",
            vec![serde_json::json!({
                "kinds":[20_002],
                "#h":[channel.to_string()],
                "since":0,
                "limit":10
            })],
        )
        .await
        .unwrap();
    loop {
        match events.recv().await {
            Some(SessionEvent::Event { subscription, .. })
                if subscription == "agent-typing-history" =>
            {
                panic!("ephemeral agent typing was returned as history")
            }
            Some(SessionEvent::Eose(id)) if id == "agent-typing-history" => break,
            _ => {}
        }
    }
    session.close("agent-typing-history").await.unwrap();

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
                &[],
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
                &[],
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
        .pin_relay_pubkey(local_community, &harness_relay_pubkey)
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
        .sign(buzz_sdk::build_message(channel, "bzz root", None, &[], false, &[], &[]).unwrap())
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
                &[],
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
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(direct.clone()).await.unwrap().accepted);
    let relay_pubkey = bzz::protocol::http::relay_signing_pubkey(&info).unwrap();
    let live_summary = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SessionEvent::Event { event, .. }) = events.recv().await
                && event.kind == Kind::Custom(39_005)
                && let Some(summary) = bzz::protocol::thread_summary::parse(&event, relay_pubkey)
                && summary.root_event_id == root.id.to_hex()
            {
                break summary;
            }
        }
    })
    .await
    .expect("relay emitted a live thread summary");
    assert_eq!(live_summary.channel_id, channel);
    assert!(live_summary.summary.descendant_count >= 1);
    assert!(live_summary.summary.last_reply_at.is_some());
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

    // NIP-RS atomic snapshot: the same two slots, verified and complete.
    let snapshot = http
        .read_state_snapshot(&http.nip11().await.unwrap())
        .await
        .unwrap()
        .expect("pinned relay advertises read_state_snapshot");
    let snapshot_ids = snapshot
        .events
        .iter()
        .map(|event| event.id)
        .collect::<HashSet<_>>();
    assert!(snapshot_ids.contains(&read_events[0].id));
    assert!(snapshot_ids.contains(&read_events_b[0].id));
    let fresh = {
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&local_config).unwrap();
        StoreHandle::spawn(store).unwrap()
    };
    let loaded = read_state::load_snapshot(local_community, &http, &signer, &fresh)
        .await
        .expect("verified snapshot loads");
    assert_eq!(loaded, snapshot.events.len());
    let pubkey_for_contexts = pubkey.clone();
    let contexts = fresh
        .call(move |store| store.read_contexts(local_community, &pubkey_for_contexts, false))
        .await
        .unwrap();
    assert_eq!(contexts[&context_key], 20);

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
                buzz_sdk::build_message(
                    channel,
                    &format!("dense {index}"),
                    None,
                    &[],
                    false,
                    &[],
                    &[],
                )
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
    let local_summary = local_store
        .call({
            let root_id = root_id.clone();
            move |store| store.thread_summaries(local_community, channel, &[root_id])
        })
        .await
        .unwrap();
    assert_eq!(
        local_summary[&root.id.to_hex()].descendant_count,
        2,
        "image and nested replies remain after deleting the direct reply"
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

/// v0.13 M1 against the pinned relay: announce channels make joiners
/// read-only guests, system channels are reachable only through the NIP-CL
/// opt-in, and a kick removes the channel on the next directory refresh.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_channel_types_roles_and_membership() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let owner = Keys::generate();
    let reader = Keys::generate();
    let owner_hex = owner.public_key().to_hex();
    let reader_hex = reader.public_key().to_hex();
    let _ = seed_member(&source, &owner_hex);
    let _ = seed_member(&source, &reader_hex);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let relay_ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let relay_http = url::Url::parse("http://localhost:3030/").unwrap();
    let owner_signer = SignerHandle::spawn(owner.clone());
    let reader_signer = SignerHandle::spawn(reader.clone());
    let (owner_session, mut owner_events) =
        session::connect(relay_ws.clone(), owner_signer.clone())
            .await
            .unwrap();
    assert!(matches!(
        owner_events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let (reader_session, mut reader_events) =
        session::connect(relay_ws.clone(), reader_signer.clone())
            .await
            .unwrap();
    assert!(matches!(
        reader_events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let owner_http = HttpClient::new(relay_http.clone(), owner_signer.clone()).unwrap();
    let reader_http = HttpClient::new(relay_http.clone(), reader_signer.clone()).unwrap();
    let relay_pubkey =
        bzz::protocol::http::relay_signing_pubkey(&owner_http.nip11().await.unwrap())
            .unwrap()
            .to_owned();

    // Announce channel: open stream whose joiners become guests.
    let announce = uuid::Uuid::new_v4();
    let create = owner_signer
        .sign(
            buzz_sdk::build_create_channel(
                announce,
                &format!("announce-{}", &announce.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let ack = owner_session.publish(create).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    let guest_policy = owner_signer
        .sign(
            buzz_sdk::build_update_channel(
                announce,
                None,
                None,
                None,
                None,
                Some(buzz_sdk::ChannelJoinRole::Guest),
                None,
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let ack = owner_session.publish(guest_policy).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    let join = reader_signer
        .sign(buzz_sdk::build_join(announce).unwrap())
        .await
        .unwrap();
    let ack = reader_session.publish(join).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);

    // System channel: private, created by the owner.
    let system = uuid::Uuid::new_v4();
    let create_system = owner_signer
        .sign(
            buzz_sdk::build_create_channel(
                system,
                &format!("agent-config-{}", &system.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Private),
                Some(buzz_sdk::ChannelKind::System),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let ack = owner_session.publish(create_system).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    tokio::time::sleep(Duration::from_secs(1)).await;

    async fn local_directory(
        pubkey: &str,
        relay_pubkey: &str,
        http: &HttpClient,
    ) -> (StoreHandle, uuid::Uuid) {
        let identity = IdentityConfig {
            id: uuid::Uuid::new_v4(),
            label: "m1".into(),
            pubkey: pubkey.to_owned(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "m1".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community("m1".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        store.pin_relay_pubkey(community, relay_pubkey).unwrap();
        let store = StoreHandle::spawn(store).unwrap();
        let report = directory::refresh(community, pubkey, http, &store)
            .await
            .unwrap();
        assert!(
            report.membership_snapshot,
            "pinned relay advertises membership_snapshot; bzz must verify and use it"
        );
        (store, community)
    }
    async fn channel(
        store: &StoreHandle,
        community: uuid::Uuid,
        id: uuid::Uuid,
    ) -> Option<bzz::domain::Channel> {
        store
            .call(move |store| store.channels(community))
            .await
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == id)
    }

    // Reader: a guest in the announce channel, offered no composer.
    let (reader_store, reader_community) =
        local_directory(&reader_hex, &relay_pubkey, &reader_http).await;
    let guest = channel(&reader_store, reader_community, announce)
        .await
        .unwrap();
    assert!(guest.is_member);
    assert_eq!(guest.facts.join_role, bzz::domain::JoinRole::Guest);
    assert_eq!(
        guest.facts.viewer_role,
        Some(bzz::domain::MemberRole::Guest)
    );
    assert_eq!(
        guest.read_only_reason(),
        Some(bzz::domain::ReadOnlyReason::Guest)
    );
    assert_eq!(guest.facts.creator.as_deref(), Some(owner_hex.as_str()));
    // The relay agrees: a guest's message is refused.
    let refused = reader_signer
        .sign(
            buzz_sdk::build_message(
                announce,
                "generated guest write",
                None,
                &[],
                false,
                &[],
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let ack = reader_session.publish(refused).await.unwrap();
    assert!(!ack.accepted, "relay accepted a guest write");
    // The reader cannot see the owner's private system channel at all.
    assert!(
        channel(&reader_store, reader_community, system)
            .await
            .is_none()
    );

    // Owner: the system channel arrives only through the NIP-CL opt-in and
    // stays a member channel, typed System and read-only.
    let (owner_store, owner_community) =
        local_directory(&owner_hex, &relay_pubkey, &owner_http).await;
    let system_channel = channel(&owner_store, owner_community, system)
        .await
        .unwrap();
    assert!(
        system_channel.is_member,
        "system membership was reconciled away"
    );
    assert_eq!(system_channel.kind, bzz::domain::ChannelKind::System);
    assert_eq!(
        system_channel.read_only_reason(),
        Some(bzz::domain::ReadOnlyReason::SystemChannel)
    );
    let owner_announce = channel(&owner_store, owner_community, announce)
        .await
        .unwrap();
    assert_eq!(
        owner_announce.read_only_reason(),
        None,
        "owner writes in announce"
    );
    let legacy_only = owner_http
        .query(&[QueryFilter {
            kinds: vec![39_002],
            ..QueryFilter::default()
        }
        .tag("p", [owner_hex.clone()])])
        .await
        .unwrap();
    assert!(
        legacy_only
            .iter()
            .all(|event| bzz::protocol::events::first_tag(event, "d") != Some(system.to_string())),
        "a read that names no channel must not return system channels"
    );

    // Kick: the owner removes the reader; the reader's next refresh drops it.
    let kick = owner_signer
        .sign(buzz_sdk::build_remove_member(announce, &reader_hex).unwrap())
        .await
        .unwrap();
    let ack = owner_session.publish(kick).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    tokio::time::sleep(Duration::from_secs(1)).await;
    directory::refresh(reader_community, &reader_hex, &reader_http, &reader_store)
        .await
        .unwrap();
    let kicked = channel(&reader_store, reader_community, announce)
        .await
        .unwrap();
    assert!(
        !kicked.is_member,
        "kicked membership survived a directory refresh"
    );

    owner_session.shutdown().await;
    reader_session.shutdown().await;
    owner_signer.lock().await;
    reader_signer.lock().await;
}

/// v0.13 M3: a Buzz Desktop identity (key only in the provider) authenticates,
/// queries with NIP-98 and publishes against the pinned relay.
#[cfg(not(windows))]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_buzz_desktop_identity_signs_through_the_provider() {
    use bzz::auth::provider::{Endpoint, ProviderClient};
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let provider = support::fake_provider::FakeProvider::start(&["ws://localhost:3030"]).await;
    let channel = seed_member(&source, &provider.keys.public_key().to_hex());
    tokio::time::sleep(Duration::from_secs(2)).await;
    let client = std::sync::Arc::new(
        ProviderClient::new(
            Endpoint::Advertised(provider.url.clone()),
            Some(secrecy::SecretString::from(provider.token.clone())),
        )
        .unwrap(),
    );
    let signer = SignerHandle::spawn_remote(client, provider.keys.public_key());
    let (session, mut events) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    http.query(&[QueryFilter {
        kinds: vec![9],
        limit: Some(1),
        ..QueryFilter::default()
    }
    .tag("h", [channel.to_string()])])
        .await
        .unwrap();
    let message = signer
        .sign(
            buzz_sdk::build_message(
                channel,
                "generated desktop-signed",
                None,
                &[],
                false,
                &[],
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(message).await.unwrap().accepted);
    assert!(
        provider
            .calls()
            .iter()
            .filter(|method| *method == "sign_event")
            .count()
            >= 3
    );
    session.shutdown().await;
    signer.lock().await;
}

/// v0.13 M10: "Send to channel" and NIP-56 reports through MessageService.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_send_to_channel_and_report() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys);
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m10".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m10".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "m10".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let service = bzz::service::messages::MessageService::new(
        community,
        signer.clone(),
        store.clone(),
        supervisor.clone(),
    );
    let root_text = format!(
        "**Release** plan for [v0.13](https://x.io) {}",
        "words ".repeat(20)
    );
    let root = service.send(channel, &root_text).await.unwrap();
    let copy = service
        .send_to_channel(
            channel,
            &root.id.to_hex(),
            &root_text,
            "summary for the channel",
            &[],
        )
        .await
        .unwrap();
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let stored = http
        .query(&[QueryFilter {
            ids: vec![copy.id.to_hex()],
            limit: Some(1),
            ..QueryFilter::default()
        }])
        .await
        .unwrap();
    assert_eq!(stored.len(), 1, "relay stored the channel copy");
    let tags = stored[0]
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    let reference = bzz::protocol::sent_from_thread::parse_tag(&tags).unwrap();
    assert_eq!(reference.root_event_id, root.id.to_hex());
    assert_eq!(
        reference.excerpt.as_deref(),
        bzz::protocol::sent_from_thread::summarize_thread_root(&root_text).as_deref()
    );
    assert!(tags.iter().all(|tag| tag[0] != "e"), "no NIP-10 reply tags");

    let report = service
        .report(
            Some(&root.id.to_hex()),
            &pubkey,
            "spam",
            Some("generated report"),
        )
        .await
        .unwrap();
    assert_eq!(report.kind.as_u16(), 1984);
    // M10.3: an author, as a user, with no message.
    let user_report = service
        .report(None, &pubkey, "impersonation", None)
        .await
        .unwrap();
    assert_eq!(user_report.kind.as_u16(), 1984);
    let tags = user_report
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    assert!(tags.iter().all(|tag| tag[0] != "e"), "no message reference");
    assert!(tags.iter().any(|tag| tag[0] == "p"
        && tag[1] == pubkey
        && tag.get(2).map(String::as_str) == Some("impersonation")));
    supervisor.shutdown().await;
    signer.lock().await;
}

/// v0.13 M6: presence via WebSocket and the relay-signed Redis snapshot, and a
/// NIP-38 status with expiry.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_presence_snapshot_and_status() {
    use bzz::protocol::presence::{Presence, observe, status};
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let _ = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys);
    let (session, mut events) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    let snapshot = |http: HttpClient, pubkey: String| async move {
        http.query(&[QueryFilter {
            kinds: vec![20_001],
            authors: vec![pubkey],
            limit: Some(1),
            ..QueryFilter::default()
        }])
        .await
        .unwrap()
    };
    assert!(
        snapshot(http.clone(), pubkey.clone()).await.is_empty(),
        "offline before publishing"
    );

    let online = signer
        .sign(buzz_sdk::build_presence_update("online").unwrap())
        .await
        .unwrap();
    assert!(session.publish(online).await.unwrap().accepted);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let events_now = snapshot(http.clone(), pubkey.clone()).await;
    assert_eq!(events_now.len(), 1);
    let observed = observe(&events_now[0], Some(&relay), |_| None).unwrap();
    assert_eq!(observed.subjects, vec![pubkey.clone()]);
    assert_eq!(observed.presence, Presence::Online);

    let offline = signer
        .sign(buzz_sdk::build_presence_update("offline").unwrap())
        .await
        .unwrap();
    assert!(session.publish(offline).await.unwrap().accepted);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = snapshot(http.clone(), pubkey.clone()).await;
    assert!(
        after.is_empty()
            || observe(&after[0], Some(&relay), |_| None).unwrap().presence == Presence::Offline
    );

    let expires = Timestamp::now().as_secs() + 3600;
    let user_status = signer
        .sign(buzz_sdk::build_user_status_expiring("in review", Some("👀"), Some(expires)).unwrap())
        .await
        .unwrap();
    assert!(session.publish(user_status.clone()).await.unwrap().accepted);
    let stored = http
        .query(&[QueryFilter {
            kinds: vec![30_315],
            authors: vec![pubkey.clone()],
            limit: Some(1),
            ..QueryFilter::default()
        }
        .tag("d", ["general".to_owned()])])
        .await
        .unwrap();
    let parsed = status(&stored[0]).unwrap();
    assert_eq!(
        (parsed.text.as_str(), parsed.emoji.as_str()),
        ("in review", "👀")
    );
    assert_eq!(parsed.expires_at, Some(i64::try_from(expires).unwrap()));
    session.shutdown().await;
    signer.lock().await;
}

/// v0.13 M5: NIP-CE entries and 30030 sets resolve, outgoing messages carry
/// NIP-30 tags, NIP-50 prefix search finds entries, custom reactions land.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_custom_emoji() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys);
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m5".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m5".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m5".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let service = bzz::service::messages::MessageService::new(
        community,
        signer.clone(),
        store.clone(),
        supervisor.clone(),
    );
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let shortcode = format!("bzzparrot{suffix}");
    let url = format!("https://example.invalid/media/{suffix}.gif");
    let entry = service
        .publish_builder(buzz_sdk::build_emoji_entry(&shortcode, &url).unwrap())
        .await
        .unwrap();
    let set = service
        .publish_builder(
            buzz_sdk::build_custom_emoji_set(&[buzz_sdk::CustomEmoji {
                shortcode: format!("bzzwave{suffix}"),
                url: format!("https://example.invalid/media/w{suffix}.png"),
            }])
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(entry.kind.as_u16(), 30_180);
    assert_eq!(set.kind.as_u16(), 30_030);

    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    // Palette from the relay: the set, plus the entry by exact #d lookup.
    bzz::sync::directory::hydrate_emoji_sets(community, &http, &store)
        .await
        .unwrap();
    let looked_up = http
        .query(&[QueryFilter {
            kinds: vec![30_180],
            limit: Some(5),
            ..QueryFilter::default()
        }
        .tag("d", [shortcode.clone()])])
        .await
        .unwrap();
    assert_eq!(looked_up.len(), 1);
    store
        .call(move |store| store.apply_event(community, &looked_up[0]))
        .await
        .unwrap();
    let palette = store
        .call(move |store| store.emoji_palette(community))
        .await
        .unwrap();
    assert_eq!(palette[&shortcode], url);
    assert!(palette.contains_key(&format!("bzzwave{suffix}")));

    // Outgoing message with both shortcodes gets NIP-30 tags.
    let content = format!("ship it :{shortcode}: :bzzwave{suffix}: :unknown{suffix}:");
    let sent = service.send(channel, &content).await.unwrap();
    let emoji = bzz::protocol::emoji::tags_of(
        &sent
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        emoji.len(),
        2,
        "known shortcodes tagged, unknown left as text"
    );

    // NIP-50 prefix search over 30180.
    let found = http
        .query(&[QueryFilter {
            kinds: vec![30_180],
            search: Some(format!("bzzparrot{}", &suffix[..2])),
            search_mode: Some(bzz::protocol::types::SearchMode::Prefix),
            limit: Some(50),
            ..QueryFilter::default()
        }])
        .await
        .unwrap();
    assert!(
        found.iter().any(|event| event.id == entry.id),
        "NIP-50 prefix search finds the entry"
    );

    let reaction = service
        .react_custom(&sent.id.to_hex(), &shortcode, &url)
        .await
        .unwrap();
    assert_eq!(reaction.content, format!(":{shortcode}:"));
    supervisor.shutdown().await;
    signer.lock().await;
}

/// v0.13 M7: verified NIP-CW channel and thread windows on the pinned relay.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_channel_and_thread_windows() {
    use bzz::protocol::window::ThreadRequest;
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let _ = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys.clone());
    let (session, mut events) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    // A fresh channel so the counts are exact.
    let channel = uuid::Uuid::new_v4();
    let create = signer
        .sign(
            buzz_sdk::build_create_channel(
                channel,
                &format!("windows-{}", &channel.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(create).await.unwrap().accepted);
    // 230 top-level rows, many sharing a second, plus one thread of 120 replies.
    let base = Timestamp::now().as_secs() - 200;
    let mut roots = Vec::new();
    for index in 0..230_u64 {
        let event =
            buzz_sdk::build_message(channel, &format!("row {index}"), None, &[], false, &[], &[])
                .unwrap()
                .custom_created_at(Timestamp::from(base + index / 7))
                .sign_with_keys(&keys)
                .unwrap();
        let ack = session.publish(event.clone()).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        roots.push(event);
    }
    let root = roots[200].clone();
    let mut replies = Vec::new();
    for index in 0..120_u64 {
        let event = buzz_sdk::build_message(
            channel,
            &format!("reply {index}"),
            Some(&buzz_sdk::ThreadRef {
                root_event_id: root.id,
                parent_event_id: root.id,
            }),
            &[],
            false,
            &[],
            &[],
        )
        .unwrap()
        .custom_created_at(Timestamp::from(base + 60 + index / 5))
        .sign_with_keys(&keys)
        .unwrap();
        assert!(session.publish(event.clone()).await.unwrap().accepted);
        replies.push(event);
    }
    let reaction = signer
        .sign(buzz_sdk::build_reaction(roots[3].id, "👍").unwrap())
        .await
        .unwrap();
    assert!(session.publish(reaction.clone()).await.unwrap().accepted);

    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m7".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m7".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m7".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    store.pin_relay_pubkey(community, &relay).unwrap();
    let store = StoreHandle::spawn(store).unwrap();

    let report = bzz::sync::window::channel(community, channel, &relay, &http, &store)
        .await
        .unwrap();
    // Every top-level row (plus the relay's kind 40099 channel_created
    // system row), none of the replies.
    assert_eq!(report.rows, 231, "{report:?}");
    assert!(report.pages >= 3, "paged across dense seconds: {report:?}");
    assert!(report.crossed_watermark);
    assert!(report.aux >= 1, "reaction arrived through the aux closure");
    assert!(
        report
            .summaries
            .iter()
            .any(|summary| bzz::protocol::events::first_tag(summary, "e") == Some(root.id.to_hex())),
        "39005 summary for the thread root"
    );
    let stored = store
        .call(move |store| store.messages(community, channel, 1_000))
        .await
        .unwrap();
    let root_ids = roots
        .iter()
        .map(|root| root.id.to_hex())
        .collect::<HashSet<_>>();
    let ids = stored
        .iter()
        .filter(|message| message.system.is_none())
        .map(|message| message.event_id.clone())
        .collect::<HashSet<_>>();
    assert_eq!(
        ids, root_ids,
        "exactly the published rows: no gaps, no extras"
    );
    assert_eq!(stored.len(), 231, "rows plus one system row, no duplicates");
    let target = roots[3].id.to_hex();
    let reactions = store
        .call(move |store| store.reactions(community, &target))
        .await
        .unwrap();
    assert_eq!(reactions.len(), 1);
    // A second load stops at the watermark after one page.
    let again = bzz::sync::window::channel(community, channel, &relay, &http, &store)
        .await
        .unwrap();
    assert_eq!(again.pages, 1);

    // Thread windows newest-first, paged backwards to the start.
    let root_hex = root.id.to_hex();
    let mut cursor: Option<bzz::protocol::window::Cursor> = None;
    let mut seen = Vec::new();
    for _ in 0..10 {
        let request = ThreadRequest {
            host: "localhost:3030",
            reader: &pubkey,
            channel,
            root: &root_hex,
            limit: bzz::sync::window::THREAD_PAGE,
            depth: 100,
            include_aux: true,
            cursor: cursor.as_ref(),
        };
        let page = bzz::sync::window::thread_page(community, &request, &relay, &http, &store)
            .await
            .unwrap();
        seen.extend(page.rows.iter().map(|row| row.id));
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(seen.len(), 120);
    assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 120);
    // NIP-CW total order: created_at DESC, then id ASC within a second.
    let mut expected = replies.clone();
    expected.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.to_hex().cmp(&right.id.to_hex()))
    });
    assert_eq!(
        seen,
        expected.iter().map(|reply| reply.id).collect::<Vec<_>>(),
        "newest first across pages, exact order"
    );

    // The app's entry points: the selected channel takes the windowed path
    // (with summaries) and threads page through the service.
    let service =
        bzz::service::channels::ChannelService::new(community, http.clone(), store.clone());
    let outcome = service.selected_history(channel).await.unwrap();
    assert!(
        outcome.windowed,
        "pinned relay serves verified channel windows"
    );
    assert_eq!(service.relay_host().as_deref(), Some("localhost:3030"));
    let request = ThreadRequest {
        host: "localhost:3030",
        reader: &pubkey,
        channel,
        root: &root_hex,
        limit: bzz::sync::window::THREAD_PAGE,
        depth: 100,
        include_aux: true,
        cursor: None,
    };
    let first = service
        .thread_page(&request)
        .await
        .unwrap()
        .expect("thread windows");
    assert_eq!(first.rows.first().map(|row| row.id), Some(expected[0].id));
    assert!(first.next.is_some(), "120 replies need more than one page");
    // Without a pinned signer the service reports "unsupported", so the app
    // keeps the local archive and the legacy backfill.
    let mut unpinned = Store::open_memory().unwrap();
    unpinned.sync_config(&config).unwrap();
    let unpinned = StoreHandle::spawn(unpinned).unwrap();
    let fallback =
        bzz::service::channels::ChannelService::new(community, http.clone(), unpinned.clone());
    assert!(fallback.thread_page(&request).await.unwrap().is_none());
    session.shutdown().await;
    signer.lock().await;
}

/// Waits for the next sync message on subscription `id`.
async fn next_sync(
    events: &mut tokio::sync::broadcast::Receiver<bzz::realtime::supervisor::SupervisorEvent>,
    id: &str,
) -> SessionEvent {
    use bzz::realtime::supervisor::SupervisorEvent;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(15), events.recv())
            .await
            .expect("sync message in time")
            .unwrap();
        if let SupervisorEvent::Session(
            message @ (SessionEvent::Event { .. }
            | SessionEvent::Eose(_)
            | SessionEvent::Closed { .. }),
        ) = event
        {
            let subscription = match &message {
                SessionEvent::Event { subscription, .. }
                | SessionEvent::Closed { subscription, .. } => subscription.clone(),
                SessionEvent::Eose(subscription) => subscription.clone(),
                _ => unreachable!(),
            };
            if subscription == id {
                return message;
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_channel_artifacts() {
    use bzz::protocol::artifact::{self, Change};
    use bzz::service::artifacts::{Applied, ArtifactService};
    use nostr::Tag;
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys.clone());
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m4".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m4".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m4".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    store.pin_relay_pubkey(community, &relay).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let mut stream = supervisor.subscribe_events();
    let service = ArtifactService::new(
        community,
        http.clone(),
        store.clone(),
        signer.clone(),
        supervisor.clone(),
    );
    let caps = service.capabilities().await.unwrap();
    assert!(caps.sync && caps.head && caps.max_events > 0, "{caps:?}");

    // Another client creates a task assigned to us, with a schema bzz does not
    // know: an unknown tag and future content fields.
    let id = uuid::Uuid::new_v4();
    let content = r#"{"version":9,"status":"open","future":{"x":[1,2]}}"#;
    let tags = [
        ["ar", "1"],
        ["d", &id.to_string()],
        ["h", &channel.to_string()],
        ["type", "buzz.task"],
        ["title", "Fix the payment timeout"],
        ["op", "create"],
        ["assignee", &pubkey],
        ["x-priority", "p1"],
    ]
    .map(|parts| Tag::parse(parts).unwrap());
    let created = EventBuilder::new(Kind::Custom(45_010), content)
        .tags(tags)
        .sign_with_keys(&keys)
        .unwrap();
    let ack = supervisor.publish(created.clone()).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);

    // Sync: the complete set arrives before EOSE.
    supervisor
        .subscribe("ar-test", vec![artifact::sync_filter(channel)])
        .await
        .unwrap();
    let mut initial = Vec::new();
    loop {
        match next_sync(&mut stream, "ar-test").await {
            SessionEvent::Event { event, .. } => initial.push(artifact::parse(&event).unwrap()),
            SessionEvent::Eose(_) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].head, created.id.to_hex());
    store
        .call({
            let initial = initial.clone();
            move |store| store.replace_channel_artifacts(community, channel, &initial)
        })
        .await
        .unwrap();
    // Cross-channel "My tasks".
    assert_eq!(service.refresh_assigned(&pubkey).await.unwrap(), 1);
    let mine = store
        .call({
            let pubkey = pubkey.clone();
            move |store| store.assigned_artifacts(community, &pubkey)
        })
        .await
        .unwrap();
    assert_eq!(mine.len(), 1);

    // Rename keeps unknown tags and content; the live head arrives on sync.
    let renamed = service
        .apply(id, Change::Rename("Fix the timeout".into()))
        .await
        .unwrap();
    let Applied::Published(renamed) = renamed else {
        panic!("expected a publish")
    };
    let SessionEvent::Event { event, .. } = next_sync(&mut stream, "ar-test").await else {
        panic!("live head")
    };
    assert_eq!(event.id.to_hex(), renamed);
    let head = artifact::parse(&event).unwrap();
    assert_eq!(head.title.as_deref(), Some("Fix the timeout"));
    assert_eq!(head.event.content, content);
    assert!(
        head.client_tags()
            .contains(&("x-priority".into(), "p1".into()))
    );

    // A competing edit from another device lands first: bzz's next edit
    // conflicts, reloads the head and rebuilds on it.
    let stored = store
        .call(move |store| store.artifact(community, id))
        .await
        .unwrap()
        .unwrap();
    let competing = artifact::build(&Change::Rename("Other device".into()), &stored.head, None)
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    assert!(
        supervisor
            .publish(competing.clone())
            .await
            .unwrap()
            .accepted
    );
    let _ = next_sync(&mut stream, "ar-test").await;
    let after = service
        .apply(id, Change::Rename("Final title".into()))
        .await
        .unwrap();
    assert!(matches!(after, Applied::Published(_)));
    let head = service.head(id).await.unwrap().unwrap();
    assert_eq!(head.title.as_deref(), Some("Final title"));
    assert_eq!(
        head.tag("prev"),
        Some(competing.id.to_hex()),
        "built on the winner"
    );
    let _ = next_sync(&mut stream, "ar-test").await;

    // Delete, then restore the last live snapshot.
    service.apply(id, Change::Delete).await.unwrap();
    let SessionEvent::Event { event, .. } = next_sync(&mut stream, "ar-test").await else {
        panic!("deletion head")
    };
    assert!(artifact::parse(&event).unwrap().is_deleted());
    assert!(
        service.head(id).await.unwrap().unwrap().is_deleted(),
        "head view sees deletions"
    );
    service.apply(id, Change::Restore).await.unwrap();
    let SessionEvent::Event { event, .. } = next_sync(&mut stream, "ar-test").await else {
        panic!("restore head")
    };
    let restored = artifact::parse(&event).unwrap();
    assert_eq!(restored.title.as_deref(), Some("Final title"));
    assert_eq!(restored.event.content, content);

    // Move to a second channel. This reader can read the destination, so the
    // source sync receives the new head homed elsewhere (NIP-AR Sync rule 3);
    // a reader without destination access gets the 45011 removal instead.
    let destination = uuid::Uuid::new_v4();
    let create = signer
        .sign(
            buzz_sdk::build_create_channel(
                destination,
                &format!("artifacts-{}", &destination.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(supervisor.publish(create).await.unwrap().accepted);
    service.apply(id, Change::Move(destination)).await.unwrap();
    let SessionEvent::Event { event, .. } = next_sync(&mut stream, "ar-test").await else {
        panic!("moved head")
    };
    let moved_head = artifact::parse(&event).unwrap();
    assert_eq!((moved_head.id, moved_head.home), (id, destination));
    assert!(moved_head.root.is_none());
    store
        .call(move |store| store.apply_artifact_head(community, &moved_head))
        .await
        .unwrap();
    let left = store
        .call(move |store| store.channel_artifacts(community, channel, None))
        .await
        .unwrap();
    assert!(
        left.is_empty(),
        "the artifact left the source channel's set"
    );
    let moved = service.head(id).await.unwrap().unwrap();
    assert_eq!(moved.home, destination);

    // Artifacts are state, not chat: nothing entered the message timeline.
    let messages = store
        .call(move |store| store.messages(community, channel, 100))
        .await
        .unwrap();
    assert!(
        messages.iter().all(|message| message.system.is_some()),
        "{messages:?}"
    );
    supervisor.shutdown().await;
    signer.lock().await;
}

/// The broker's HTTP binding, driven by the SDK's validating client.
struct BrokerHttp(String, String);

impl buzz_sdk::broker::BrokerClient for BrokerHttp {
    fn send<'a>(
        &'a self,
        request: &'a buzz_sdk::broker::PreparedRequest,
        _dispatch: buzz_sdk::broker::Dispatch,
    ) -> buzz_sdk::broker::BrokerFuture<'a> {
        Box::pin(async move {
            let unreachable = |error: reqwest::Error| {
                buzz_sdk::broker::BrokerTransportError::Unreachable(error.to_string())
            };
            let bytes = reqwest::Client::new()
                .post(&self.0)
                .header("authorization", format!("Bearer {}", self.1))
                .header("content-type", "application/json")
                .body(request.body().to_vec())
                .send()
                .await
                .map_err(unreachable)?
                .bytes()
                .await
                .map_err(unreachable)?;
            serde_json::from_slice(&bytes).map_err(|error| {
                buzz_sdk::broker::BrokerTransportError::Unreachable(error.to_string())
            })
        })
    }
}

#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_local_agent_broker() {
    use buzz_sdk::broker::{
        ActionArgs, ActionOutcome, BrokerClientExt, BrokerErrorCode, BrokerRequest,
        ChannelReadArgs, MessagePostArgs, MessageReplyArgs, ProfileSetArgs, ReactionAddArgs,
        StorageAddressArgs,
    };
    use bzz::broker::{custody, host::Host, http as broker_http, relay};
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let owner_keys = Keys::generate();
    let owner_hex = owner_keys.public_key().to_hex();
    let channel = seed_member(&source, &owner_hex);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let owner = SignerHandle::spawn(owner_keys.clone());
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m9".into(),
        pubkey: owner_hex.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m9".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m9".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let keys_dir = tempfile::TempDir::new().unwrap();
    let (mut agent, credential) =
        custody::create(keys_dir.path(), &owner, identity.id, community, "helper")
            .await
            .unwrap();
    custody::set_allowed(
        &mut agent,
        &[
            "channel.read",
            "message.post",
            "message.reply",
            "reaction.add",
            "profile.set",
            "storage.address",
        ]
        .map(str::to_owned),
    )
    .unwrap();
    // The agent is not seeded as a relay member: its NIP-42 AUTH carries the
    // owner's NIP-OA tag, which admits it and pins its owner.
    // The owner gives the agent the bot role in its channel.
    let owner_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        owner.clone(),
    );
    // The bot channel is one the owner belongs to, so the owner's directory
    // sees its roster (and the agent's relay-signed role).
    let _ = channel;
    let channel = uuid::Uuid::new_v4();
    let home = owner
        .sign(
            buzz_sdk::build_create_channel(
                channel,
                &format!("agents-{}", &channel.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(owner_supervisor.publish(home).await.unwrap().accepted);
    let add = owner
        .sign(
            buzz_sdk::build_add_member(channel, &agent.pubkey, Some(buzz_sdk::MemberRole::Bot))
                .unwrap(),
        )
        .await
        .unwrap();
    let ack = owner_supervisor.publish(add).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    // A second channel where the agent has no role.
    let elsewhere = uuid::Uuid::new_v4();
    let create = owner
        .sign(
            buzz_sdk::build_create_channel(
                elsewhere,
                &format!("broker-{}", &elsewhere.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(owner_supervisor.publish(create).await.unwrap().accepted);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let owner_http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        owner.clone(),
    )
    .unwrap();
    let relay_pubkey =
        bzz::protocol::http::relay_signing_pubkey(&owner_http.nip11().await.unwrap())
            .unwrap()
            .to_owned();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    store.pin_relay_pubkey(community, &relay_pubkey).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let role = |store: StoreHandle, agent: String| async move {
        store
            .call(move |store| store.member_role(community, channel, &agent))
            .await
            .unwrap()
    };
    directory::refresh(community, &owner_hex, &owner_http, &store)
        .await
        .unwrap();
    assert_eq!(
        role(store.clone(), agent.pubkey.clone()).await.as_deref(),
        Some("bot"),
        "the relay-signed roster gives the agent the bot role"
    );

    let agent_signer = custody::unlock(keys_dir.path(), &owner, &agent)
        .await
        .unwrap();
    let runtime = relay::AgentRuntime {
        signer: agent_signer.clone(),
        http: HttpClient::new(
            url::Url::parse("http://localhost:3030/").unwrap(),
            agent_signer.clone(),
        )
        .unwrap(),
        supervisor: SupervisorHandle::spawn(
            url::Url::parse("ws://localhost:3030/").unwrap(),
            agent_signer.clone(),
        ),
        owner: owner_keys.public_key(),
        auth_tag: relay::auth_tag(&agent.auth_tag).unwrap(),
    };
    let executor = relay::RelayExecutor::new(
        community,
        store.clone(),
        [(agent.id, runtime)].into_iter().collect(),
    );
    let host = Host::new(std::sync::Arc::new(executor), vec![custody::hosted(&agent)]);
    let server = broker_http::serve(host.clone(), None).await.unwrap();
    let client = BrokerHttp(
        format!("http://{}/v1/action", server.address),
        credential.clone(),
    );
    let run = |id: &str, args: ActionArgs| BrokerRequest::new(id, args).unwrap().prepare().unwrap();
    let published =
        |response: &buzz_sdk::broker::ValidatedResponse| match response.result().outcome() {
            Some(
                ActionOutcome::MessagePost(event)
                | ActionOutcome::MessageReply(event)
                | ActionOutcome::ReactionAdd(event)
                | ActionOutcome::ProfileSet(event),
            ) => event.event_id.clone(),
            other => panic!(
                "expected a publish, got {other:?} / {:?}",
                response.result().error()
            ),
        };

    // Post, reply and react as the agent in its bot channel.
    let post = client
        .execute(&run(
            "post-1",
            ActionArgs::MessagePost(MessagePostArgs {
                channel_id: channel.to_string(),
                content: "hello from the broker".into(),
                mentions: Vec::new(),
            }),
        ))
        .await
        .unwrap();
    let post_id = published(&post);
    let reply = client
        .execute(&run(
            "reply-1",
            ActionArgs::MessageReply(MessageReplyArgs {
                channel_id: channel.to_string(),
                reply_to_event_id: post_id.clone(),
                content: "a reply".into(),
                mentions: Vec::new(),
            }),
        ))
        .await
        .unwrap();
    let reply_id = published(&reply);
    let reaction = client
        .execute(&run(
            "react-1",
            ActionArgs::ReactionAdd(ReactionAddArgs {
                channel_id: channel.to_string(),
                target_event_id: post_id.clone(),
                reaction: "👍".into(),
            }),
        ))
        .await
        .unwrap();
    published(&reaction);
    // The stored event is the agent's, with a valid owner NIP-OA tag.
    let stored = owner_http
        .query_raw(&[serde_json::json!({"ids": [post_id], "limit": 1})])
        .await
        .unwrap();
    let stored = stored.first().expect("post stored");
    assert_eq!(stored.pubkey.to_hex(), agent.pubkey);
    let auth = stored
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
        .expect("NIP-OA tag");
    let auth = serde_json::to_string(auth.as_slice()).unwrap();
    assert_eq!(
        buzz_sdk::nip_oa::verify_auth_tag(&auth, &stored.pubkey).unwrap(),
        owner_keys.public_key()
    );

    // channel.read pages newest-first with an opaque cursor.
    let read = |id: &str, cursor: Option<String>| {
        run(
            id,
            ActionArgs::ChannelRead(ChannelReadArgs {
                channel_id: channel.to_string(),
                root_event_id: None,
                mentions_only: false,
                cursor,
                limit: Some(1),
            }),
        )
    };
    let first = client.execute(&read("read-1", None)).await.unwrap();
    let Some(ActionOutcome::ChannelRead(page)) = first.result().outcome() else {
        panic!("{:?}", first.result().error())
    };
    assert_eq!(page.messages.len(), 1);
    let cursor = page.next_cursor.clone().expect("more pages");
    let second = client.execute(&read("read-2", Some(cursor))).await.unwrap();
    let Some(ActionOutcome::ChannelRead(next)) = second.result().outcome() else {
        panic!("{:?}", second.result().error())
    };
    assert_eq!(next.messages.len(), 1);
    let seen = [&page.messages[0].0.id, &next.messages[0].0.id].map(|id| id.to_hex());
    assert_ne!(seen[0], seen[1], "the cursor advanced");
    assert!(seen.contains(&reply_id) || seen.contains(&post_id));

    // No bot role in the other channel: a typed denial, nothing published.
    let denied = client
        .execute(&run(
            "post-2",
            ActionArgs::MessagePost(MessagePostArgs {
                channel_id: elsewhere.to_string(),
                content: "should not land".into(),
                mentions: Vec::new(),
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        denied.result().error().map(|error| error.code),
        Some(BrokerErrorCode::Unauthorized)
    );

    // Profile and NIP-AE memory addressing (the owner derives the same d tag).
    let profile = client
        .execute(&run(
            "profile-1",
            ActionArgs::ProfileSet(ProfileSetArgs {
                display_name: Some("Helper".into()),
                about: None,
                picture: None,
            }),
        ))
        .await
        .unwrap();
    published(&profile);
    let address = client
        .execute(&run(
            "mem-1",
            ActionArgs::StorageAddress(StorageAddressArgs {
                slug: "core".into(),
            }),
        ))
        .await
        .unwrap();
    let Some(ActionOutcome::StorageAddress(address)) = address.result().outcome() else {
        panic!("{:?}", address.result().error())
    };
    let agent_pubkey = nostr::PublicKey::from_hex(&agent.pubkey).unwrap();
    let owner_side = buzz_core::engram::d_tag(
        &buzz_core::engram::conversation_key(owner_keys.secret_key(), &agent_pubkey),
        "core",
    );
    assert_eq!(
        (address.kind, address.d_tag.as_str()),
        (30_174, owner_side.as_str())
    );

    // Owner-signed agent presence (20003). The relay records it only for
    // agents it knows this owner owns; the snapshot shows the effect.
    let snapshot = |http: HttpClient, agent: String| async move {
        http.query_raw(&[serde_json::json!({"kinds": [20001], "authors": [agent]})])
            .await
            .unwrap()
            .into_iter()
            .map(|event| event.content)
            .collect::<Vec<_>>()
    };
    let publish_presence = |signer: SignerHandle, supervisor: SupervisorHandle, presence| {
        let agent = agent.pubkey.clone();
        async move {
            let event = signer
                .sign(
                    bzz::app::agent_presence_builder(presence, std::slice::from_ref(&agent))
                        .unwrap(),
                )
                .await
                .unwrap();
            supervisor.publish(event).await.unwrap()
        }
    };
    // Someone who is not the agent's owner cannot speak for it.
    let stranger_keys = Keys::generate();
    let _ = seed_member(&source, &stranger_keys.public_key().to_hex());
    let stranger = SignerHandle::spawn(stranger_keys);
    let stranger_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        stranger.clone(),
    );
    publish_presence(
        stranger.clone(),
        stranger_supervisor.clone(),
        bzz::protocol::presence::Presence::Away,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        snapshot(owner_http.clone(), agent.pubkey.clone())
            .await
            .is_empty(),
        "a non-owner's 20003 has no effect"
    );
    let ack = publish_presence(
        owner.clone(),
        owner_supervisor.clone(),
        bzz::protocol::presence::Presence::Online,
    )
    .await;
    assert!(ack.accepted, "agent presence: {}", ack.message);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        snapshot(owner_http.clone(), agent.pubkey.clone()).await,
        vec!["online".to_owned()],
        "the owner's 20003 sets the agent online"
    );
    stranger_supervisor.shutdown().await;

    // The activity log carries no content and no credential.
    let activity = format!("{:?}", host.activity());
    assert!(!activity.contains("hello from the broker") && !activity.contains(&credential));
    owner_supervisor.shutdown().await;
}

#[cfg(feature = "huddles-audio")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_huddle_lifecycle_and_audio_round_trip() {
    use bzz::{huddle::audio, protocol::huddle};
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let _ = seed_member(&source, &alice_keys.public_key().to_hex());
    let _ = seed_member(&source, &bob_keys.public_key().to_hex());
    tokio::time::sleep(Duration::from_secs(2)).await;
    let alice = SignerHandle::spawn(alice_keys.clone());
    let bob = SignerHandle::spawn(bob_keys.clone());
    let ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let alice_supervisor = SupervisorHandle::spawn(ws.clone(), alice.clone());
    // A parent channel with both as members.
    let parent = uuid::Uuid::new_v4();
    let publish = |signer: SignerHandle, supervisor: SupervisorHandle, builder: EventBuilder| async move {
        let event = signer.sign(builder).await.unwrap();
        let ack = supervisor.publish(event).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
    };
    publish(
        alice.clone(),
        alice_supervisor.clone(),
        buzz_sdk::build_create_channel(
            parent,
            &format!("talk-{}", &parent.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    publish(
        alice.clone(),
        alice_supervisor.clone(),
        buzz_sdk::build_add_member(parent, &bob_keys.public_key().to_hex(), None).unwrap(),
    )
    .await;
    // Start: backing channel, then 48100 in the parent.
    let backing = uuid::Uuid::new_v4();
    publish(
        alice.clone(),
        alice_supervisor.clone(),
        huddle::backing_channel(backing).unwrap(),
    )
    .await;
    publish(
        alice.clone(),
        alice_supervisor.clone(),
        huddle::started(parent, backing).unwrap(),
    )
    .await;

    // Both join audio; the engine starts muted.
    let alice_joined = audio::join(&ws, backing, parent, &alice).await.unwrap();
    let bob_joined = audio::join(&ws, backing, parent, &bob).await.unwrap();
    let (alice_capture, alice_capture_rx) = tokio::sync::mpsc::channel(64);
    let (alice_playback, _alice_playback_rx) = tokio::sync::mpsc::channel(256);
    let alice_call = audio::start(alice_joined, alice_capture_rx, alice_playback).unwrap();
    let (_bob_capture, bob_capture_rx) = tokio::sync::mpsc::channel(64);
    let (bob_playback, mut bob_playback_rx) = tokio::sync::mpsc::channel(256);
    let bob_call = audio::start(bob_joined, bob_capture_rx, bob_playback).unwrap();
    assert!(
        alice_call.is_muted() && bob_call.is_muted(),
        "muted on join"
    );

    // Muted: Bob hears nothing while Alice "speaks".
    let tone = |frame: usize| {
        (0..960)
            .map(|index| {
                let t = (frame * 960 + index) as f32 / 48_000.0;
                0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect::<Vec<f32>>()
    };
    let loudest = |rx: &mut tokio::sync::mpsc::Receiver<Vec<f32>>| {
        let mut peak = 0.0_f32;
        while let Ok(chunk) = rx.try_recv() {
            peak = chunk
                .iter()
                .fold(peak, |peak, sample| peak.max(sample.abs()));
        }
        peak
    };
    for frame in 0..25 {
        alice_capture.send(tone(frame)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        loudest(&mut bob_playback_rx) < 0.01,
        "muted capture is never sent"
    );

    // Unmuted: the tone round-trips through the relay to Bob's playback.
    alice_call.set_muted(false);
    for frame in 25..100 {
        alice_capture.send(tone(frame)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let peak = loudest(&mut bob_playback_rx);
    assert!(peak > 0.1, "Bob hears Alice: peak {peak}");

    // The relay-signed roster has both.
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        alice.clone(),
    )
    .unwrap();
    let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    let lifecycle = http
        .query_raw(&[huddle::lifecycle_filter(parent)])
        .await
        .unwrap();
    let now = Timestamp::now().as_secs();
    let active = huddle::active(&lifecycle, &relay, now).expect("an active huddle");
    assert_eq!(active.backing, backing);
    let mut roster = active.participants.clone();
    roster.sort();
    let mut expected = vec![
        alice_keys.public_key().to_hex(),
        bob_keys.public_key().to_hex(),
    ];
    expected.sort();
    assert_eq!(roster, expected);

    // Leave and end: the huddle is no longer active.
    bob_call.stop().await;
    alice_call.stop().await;
    publish(
        alice.clone(),
        alice_supervisor.clone(),
        huddle::ended(parent, backing).unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let lifecycle = http
        .query_raw(&[huddle::lifecycle_filter(parent)])
        .await
        .unwrap();
    assert!(huddle::active(&lifecycle, &relay, now).is_none());
    alice_supervisor.shutdown().await;
}

/// v0.13 M2.2: NIP-CL public membership lookup (kind 39009), the spec's
/// normative example on the pinned relay.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_public_membership_lookup() {
    use bzz::protocol::membership_public::Lookup;
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let requester = Keys::generate();
    let subject = Keys::generate();
    let requester_hex = requester.public_key().to_hex();
    let subject_hex = subject.public_key().to_hex();
    let _ = seed_member(&source, &requester_hex);
    let _ = seed_member(&source, &subject_hex);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let relay_ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let relay_http = url::Url::parse("http://localhost:3030/").unwrap();
    let requester_signer = SignerHandle::spawn(requester.clone());
    let subject_signer = SignerHandle::spawn(subject.clone());
    let (requester_session, mut requester_events) =
        session::connect(relay_ws.clone(), requester_signer.clone())
            .await
            .unwrap();
    assert!(matches!(
        requester_events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let (subject_session, mut subject_events) =
        session::connect(relay_ws.clone(), subject_signer.clone())
            .await
            .unwrap();
    assert!(matches!(
        subject_events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    async fn publish(
        session: &session::SessionHandle,
        signer: &SignerHandle,
        builder: nostr::EventBuilder,
    ) {
        let event = signer.sign(builder).await.unwrap();
        let ack = session.publish(event).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
    }
    let create = |id: uuid::Uuid,
                  visibility: buzz_sdk::Visibility,
                  kind: buzz_sdk::ChannelKind,
                  unlisted: bool| {
        buzz_sdk::build_create_channel(
            id,
            &format!("pub-{}", &id.simple().to_string()[..8]),
            Some(visibility),
            Some(kind),
            None,
            None,
            unlisted,
        )
        .unwrap()
    };
    use buzz_sdk::{ChannelKind, Visibility};
    let open_stream = uuid::Uuid::new_v4();
    let archived_forum = uuid::Uuid::new_v4();
    let shared_private = uuid::Uuid::new_v4();
    let unlisted = uuid::Uuid::new_v4();
    let joined = uuid::Uuid::new_v4();
    publish(
        &subject_session,
        &subject_signer,
        create(open_stream, Visibility::Open, ChannelKind::Stream, false),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        create(archived_forum, Visibility::Open, ChannelKind::Forum, false),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        buzz_sdk::build_archive(archived_forum).unwrap(),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        create(
            shared_private,
            Visibility::Private,
            ChannelKind::Stream,
            false,
        ),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        buzz_sdk::build_add_member(shared_private, &requester_hex, None).unwrap(),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        create(unlisted, Visibility::Open, ChannelKind::Stream, true),
    )
    .await;
    // The requester's own open channel, which the subject joins as a member.
    publish(
        &requester_session,
        &requester_signer,
        create(joined, Visibility::Open, ChannelKind::Stream, false),
    )
    .await;
    publish(
        &subject_session,
        &subject_signer,
        buzz_sdk::build_join(joined).unwrap(),
    )
    .await;

    let http = HttpClient::new(relay_http, requester_signer.clone()).unwrap();
    let info = http.nip11().await.unwrap();
    let pinned = bzz::protocol::http::relay_signing_pubkey(&info)
        .unwrap()
        .to_owned();
    assert!(
        bzz::protocol::membership_public::descriptor(&info).is_some(),
        "the harness advertises membership_public"
    );
    let Lookup::Verified(found) = http
        .membership_public(&info, &pinned, &subject_hex)
        .await
        .unwrap()
    else {
        panic!("expected a verified public list");
    };
    let mut expected = vec![open_stream, archived_forum, joined];
    expected.sort();
    assert_eq!(
        found
            .memberships
            .iter()
            .map(|membership| membership.channel_id)
            .collect::<Vec<_>>(),
        expected,
        "public listed-open only: no private (even shared), no unlisted"
    );
    let role = |id| {
        found
            .memberships
            .iter()
            .find(|membership| membership.channel_id == id)
            .unwrap()
            .clone()
    };
    assert_eq!(role(open_stream).role, bzz::domain::MemberRole::Owner);
    assert!(role(archived_forum).archived);
    assert_eq!(role(archived_forum).channel_type, "forum");
    assert_eq!(role(joined).role, bzz::domain::MemberRole::Member);
    // A never-seen subject: a valid signed complete empty list.
    let stranger = Keys::generate().public_key().to_hex();
    let Lookup::Verified(empty) = http
        .membership_public(&info, &pinned, &stranger)
        .await
        .unwrap()
    else {
        panic!("expected a verified empty list");
    };
    assert!(empty.memberships.is_empty());

    // The profile card's service path: pinned key from the store, a fresh
    // host-bound NIP-11 read, the same verified answer.
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m2-public".into(),
        pubkey: requester_hex.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m2-public".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m2".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let mut unpinned_store = Store::open_memory().unwrap();
    unpinned_store.sync_config(&config).unwrap();
    let unpinned = bzz::service::profiles::ProfileService::new(
        community,
        http.clone(),
        StoreHandle::spawn(unpinned_store).unwrap(),
    );
    assert_eq!(
        unpinned.public_memberships(&subject_hex).await.unwrap(),
        Lookup::Unsupported,
        "nothing to verify against without a pinned relay key"
    );
    store.pin_relay_pubkey(community, &pinned).unwrap();
    let service = bzz::service::profiles::ProfileService::new(
        community,
        http.clone(),
        StoreHandle::spawn(store).unwrap(),
    );
    let Lookup::Verified(again) = service.public_memberships(&subject_hex).await.unwrap() else {
        panic!("expected a verified public list through the service");
    };
    assert_eq!(again.memberships, found.memberships);
    requester_session.shutdown().await;
    subject_session.shutdown().await;
}

/// v0.13 M1.8: people search uses the relay's name-only profile search, so a
/// profile whose `about` merely mentions the query is not a match.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_profile_search_matches_names_only() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let named = Keys::generate();
    let mentions = Keys::generate();
    let reader = Keys::generate();
    for keys in [&named, &mentions, &reader] {
        let _ = seed_member(&source, &keys.public_key().to_hex());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let needle = format!("zq{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    for (keys, content) in [
        (
            &named,
            serde_json::json!({"display_name": format!("{needle} Person")}),
        ),
        (
            &mentions,
            serde_json::json!({"name": "someone", "about": format!("fan of {needle}")}),
        ),
    ] {
        let signer = SignerHandle::spawn(keys.clone());
        let (session, mut events) = session::connect(
            url::Url::parse("ws://localhost:3030/").unwrap(),
            signer.clone(),
        )
        .await
        .unwrap();
        assert!(matches!(
            events.recv().await,
            Some(SessionEvent::Authenticated)
        ));
        let event = EventBuilder::new(Kind::Metadata, content.to_string())
            .sign_with_keys(keys)
            .unwrap();
        assert!(session.publish(event).await.unwrap().accepted);
        session.shutdown().await;
    }
    let signer = SignerHandle::spawn(reader.clone());
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let raw = http
        .profile_search(&needle, 20, 1)
        .await
        .unwrap()
        .expect("the pinned relay serves POST /profiles/search");
    assert_eq!(
        raw.iter().map(|event| event.pubkey).collect::<Vec<_>>(),
        vec![named.public_key()],
        "name match only"
    );
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m1-search".into(),
        pubkey: reader.public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m1-search".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m1".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let service = bzz::service::search::SearchService::new(community, http.clone(), store.clone());
    assert_eq!(service.hydrate_profiles(&needle).await.unwrap(), 1);
    let named_hex = named.public_key().to_hex();
    let mentions_hex = mentions.public_key().to_hex();
    assert!(
        store
            .call(move |store| store.profile(community, &named_hex))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .call(move |store| store.profile(community, &mentions_hex))
            .await
            .unwrap()
            .is_none()
    );
    signer.lock().await;
}

/// v0.13 M7 exit: opening a channel through windows sends fewer HTTP
/// requests than the legacy backfill plus per-message `#e` aux fan-out, for
/// the same history. Counted from the relay's own access log.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_windows_send_fewer_requests_than_backfill() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let writer = Keys::generate();
    let _ = seed_member(&source, &writer.public_key().to_hex());
    let readers = [Keys::generate(), Keys::generate()];
    for reader in &readers {
        let _ = seed_member(&source, &reader.public_key().to_hex());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(writer.clone());
    let (session, mut events) = session::connect(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let channel = uuid::Uuid::new_v4();
    let create = signer
        .sign(
            buzz_sdk::build_create_channel(
                channel,
                &format!("frames-{}", &channel.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(session.publish(create).await.unwrap().accepted);
    let base = Timestamp::now().as_secs() - 150;
    for index in 0..120_u64 {
        let message =
            buzz_sdk::build_message(channel, &format!("row {index}"), None, &[], false, &[], &[])
                .unwrap()
                .custom_created_at(Timestamp::from(base + index))
                .sign_with_keys(&writer)
                .unwrap();
        assert!(session.publish(message.clone()).await.unwrap().accepted);
        let reaction = buzz_sdk::build_reaction(message.id, "👍")
            .unwrap()
            .sign_with_keys(&writer)
            .unwrap();
        assert!(session.publish(reaction).await.unwrap().accepted);
    }
    session.shutdown().await;

    let mut counts = Vec::new();
    for (index, reader) in readers.iter().enumerate() {
        let reader_signer = SignerHandle::spawn(reader.clone());
        let http = HttpClient::new(
            url::Url::parse("http://localhost:3030/").unwrap(),
            reader_signer.clone(),
        )
        .unwrap();
        let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
            .unwrap()
            .to_owned();
        let identity = IdentityConfig {
            id: uuid::Uuid::new_v4(),
            label: format!("frames-{index}"),
            pubkey: reader.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: format!("frames-{index}"),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community(
                "frames".into(),
                "ws://localhost:3030".into(),
                identity.id,
                true,
            )
            .unwrap();
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        store.pin_relay_pubkey(community, &relay).unwrap();
        let store = StoreHandle::spawn(store).unwrap();
        if index == 0 {
            bzz::sync::window::channel(community, channel, &relay, &http, &store)
                .await
                .unwrap();
        } else {
            bzz::sync::backfill::channel(community, channel, &http, &store, 500)
                .await
                .unwrap();
        }
        let channel_id = channel;
        let stored = store
            .call(move |store| store.messages(community, channel_id, 1_000))
            .await
            .unwrap();
        assert!(stored.len() >= 120, "both paths load the history");
        tokio::time::sleep(Duration::from_millis(500)).await;
        let log = std::fs::read_to_string(
            std::env::var("BZZ_RELAY_LOG").unwrap_or_else(|_| "/tmp/dawn-relay-run.log".into()),
        )
        .expect("relay access log");
        let pubkey = format!("\"pubkey\":\"{}\"", reader.public_key().to_hex());
        counts.push(
            log.lines()
                .filter(|line| {
                    line.contains("\"event\":\"http\"")
                        && line.contains("\"route\":\"/query\"")
                        && line.contains(&pubkey)
                })
                .count(),
        );
        reader_signer.lock().await;
    }
    println!(
        "M7 /query requests: windows {} vs backfill {}",
        counts[0], counts[1]
    );
    assert!(counts[0] >= 1 && counts[1] >= 1, "{counts:?}");
    assert!(
        counts[0] < counts[1],
        "windows {} vs backfill {}",
        counts[0],
        counts[1]
    );
}

/// v0.14 M1: an edit from bzz keeps the attachments the relay governs, wins
/// even within the same second, and keeps the media readable.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_message_edits() {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys);
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m1".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m1".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m1".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let service = bzz::service::messages::MessageService::new(
        community,
        signer.clone(),
        store.clone(),
        supervisor.clone(),
    );
    let media_client = MediaClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        "localhost:3030".into(),
        signer.clone(),
        2,
    )
    .unwrap();
    let media_root = tempfile::TempDir::new().unwrap();
    let file = media_root.path().join("generated.txt");
    tokio::fs::write(&file, b"generated edit fixture\n")
        .await
        .unwrap();
    let attachment = media_client
        .upload(
            &file,
            "application/octet-stream",
            Some("generated.txt".into()),
        )
        .await
        .unwrap();
    let original = service
        .send_with_media(channel, "teh attachment", std::slice::from_ref(&attachment))
        .await
        .unwrap();
    let target = original.id.to_hex();

    let first = service
        .edit(&http, channel, &target, &pubkey, "the attachment")
        .await
        .unwrap();
    let imeta = |event: &nostr::Event| {
        event
            .tags
            .iter()
            .filter(|tag| tag.as_slice()[0] == "imeta")
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        imeta(&first),
        vec![attachment.imeta_tag()],
        "attachments kept"
    );
    assert!(first.content.starts_with("the attachment\n"));
    assert!(first.content.contains(&attachment.markdown_line()));
    // Same second: the second edit is stamped after the first.
    let second = service
        .edit(&http, channel, &target, &pubkey, "the attachment, again")
        .await
        .unwrap();
    assert!(second.created_at > first.created_at);
    let effective = http.effective_edit(&target, channel).await.unwrap();
    assert_eq!(effective.original.id, original.id);
    assert_eq!(effective.edit.map(|edit| edit.id), Some(second.id));
    let download = media_root.path().join("downloaded.bin");
    media_client.fetch(&attachment, &download).await.unwrap();
    assert_eq!(
        tokio::fs::read(&download).await.unwrap(),
        b"generated edit fixture\n",
        "the latest edit still references the media"
    );
    // Someone else's message cannot be edited from bzz.
    let stranger = Keys::generate().public_key().to_hex();
    assert!(
        service
            .edit(&http, channel, &target, &stranger, "nope")
            .await
            .is_err()
    );
}

/// v0.14 M4: sections and sort written by bzz read back through the `buzz`
/// CLI and the other way round; a Solo chat bzz creates carries the relay's
/// `buzz.solo` label and its creator.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_sidebar_documents_and_solo_chats() {
    use bzz::protocol::{sidebar::Lane, sidebar_doc};
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let cli = std::env::var("BZZ_BUZZ_CLI").expect("relay wrapper builds the buzz CLI");
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let channel = seed_member(&source, &pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let signer = SignerHandle::spawn(keys.clone());
    let mut config = Config::default();
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m4".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m4".into(),
    };
    config.identities.push(identity.clone());
    let community = config
        .add_community("m4".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        signer.clone(),
    );
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let messages =
        bzz::service::messages::MessageService::new(community, signer.clone(), store, supervisor);
    use nostr::nips::nip19::ToBech32 as _;
    let nsec = keys.secret_key().to_bech32().unwrap();
    let buzz = |args: &[&str]| -> serde_json::Value {
        let output = Command::new(&cli)
            .args(args)
            .env("BUZZ_RELAY_URL", "http://localhost:3030")
            .env("BUZZ_PRIVATE_KEY", &nsec)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "buzz {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };

    // bzz creates a section and puts #general in it; the CLI sees both.
    let design = uuid::Uuid::new_v4().to_string();
    let general = channel.to_string();
    bzz::service::sidebar::write(&http, &signer, &messages, Lane::Sections, |edit, _| {
        edit.create_section(&design, "Design", None);
        edit.set(&["a", &general], serde_json::json!(design));
        Ok(())
    })
    .await
    .unwrap()
    .expect("a change");
    let listed = buzz(&["sidebar", "sections", "list"]);
    assert_eq!(listed["sections"][0]["name"], "Design");
    assert_eq!(listed["assignments"][&general], serde_json::json!(design));

    // The CLI adds a section after it; bzz reads both, in order.
    buzz(&["sidebar", "sections", "create", "--name", "Ops"]);
    let (_, tree) = bzz::service::sidebar::read_head(&http, &signer, Lane::Sections)
        .await
        .unwrap()
        .expect("a head");
    let names = sidebar_doc::project(&tree)
        .sections
        .into_iter()
        .map(|section| section.name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["Design", "Ops"]);

    // bzz reorders on top of the CLI's write; the CLI sees the new order.
    bzz::service::sidebar::write(&http, &signer, &messages, Lane::Sections, |edit, head| {
        let ops = sidebar_doc::project(head).sections[1].id.clone();
        edit.reorder(&[ops]);
        Ok(())
    })
    .await
    .unwrap()
    .expect("a change");
    let listed = buzz(&["sidebar", "sections", "list"]);
    assert_eq!(listed["sections"][0]["name"], "Ops");
    assert_eq!(listed["sections"][1]["name"], "Design");

    // Sort modes: bzz sets one, the CLI lists it.
    bzz::service::sidebar::write(&http, &signer, &messages, Lane::Sort, |edit, _| {
        edit.set(&["g", "channels"], serde_json::json!("recent"));
        Ok(())
    })
    .await
    .unwrap()
    .expect("a change");
    let sorts = buzz(&["sidebar", "sort", "list"]);
    assert!(sorts.to_string().contains("recent"), "{sorts}");

    // A Solo chat: the relay's 39000 carries the label and the creator.
    let solo = uuid::Uuid::new_v4();
    let created = messages
        .publish_builder(bzz::protocol::sidebar::solo_creation(solo, "Plans").unwrap())
        .await
        .unwrap();
    assert_eq!(created.kind.as_u16(), 9007);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let metadata = http
        .query_raw(&[serde_json::json!({"kinds": [39000], "#d": [solo.to_string()]})])
        .await
        .unwrap();
    let tags = metadata
        .first()
        .expect("relay metadata for the Solo chat")
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    assert!(
        tags.contains(&vec!["t".into(), "buzz.solo".into()]),
        "{tags:?}"
    );
    assert!(tags.contains(&vec!["P".into(), pubkey.clone()]), "{tags:?}");
    let parsed = bzz::protocol::events::as_channel(&metadata[0]).unwrap();
    assert_eq!(parsed.visibility, bzz::domain::Visibility::Private);
    assert_eq!(parsed.facts.labels, vec!["buzz.solo".to_owned()]);
}
