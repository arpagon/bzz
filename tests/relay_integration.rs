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

    // v0.14 M5 (upstream #9371): an agent cannot write in a DM that has
    // anyone besides its owner; the broker answers that as `unauthorized`.
    let stranger_hex = stranger.public_key().to_hex();
    let open = owner
        .sign(buzz_sdk::build_dm_open(&[agent.pubkey.as_str(), stranger_hex.as_str()]).unwrap())
        .await
        .unwrap();
    let ack = owner_supervisor.publish(open).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    let dm: serde_json::Value =
        serde_json::from_str(ack.message.strip_prefix("response:").unwrap()).unwrap();
    let dm = uuid::Uuid::parse_str(dm["channel_id"].as_str().unwrap()).unwrap();
    let agent_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        agent_signer.clone(),
    );
    let in_dm = agent_signer
        .sign(
            buzz_sdk::build_message(dm, "agent in a group DM", None, &[], false, &[], &[])
                .unwrap()
                .tag(relay::auth_tag(&agent.auth_tag).unwrap()),
        )
        .await
        .unwrap();
    let refused = agent_supervisor.publish(in_dm).await.unwrap();
    assert!(!refused.accepted);
    assert!(
        refused
            .message
            .starts_with("restricted: agents cannot post in direct messages"),
        "{}",
        refused.message
    );
    assert!(relay::relay_refused_agent(&refused.message));
    agent_supervisor.shutdown().await;

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

/// v0.14 M5 (upstream #9382): search loads older pages by moving the time
/// bound below the oldest message shown; every page is older than the last.
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_search_loads_older_pages() {
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
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    let channel = uuid::Uuid::new_v4();
    let create = signer
        .sign(
            buzz_sdk::build_create_channel(
                channel,
                &format!("paging-{}", &channel.simple().to_string()[..8]),
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
    let token = format!(
        "pagetoken{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let now = Timestamp::now().as_secs();
    const TOTAL: u64 = 45;
    for index in 0..TOTAL {
        let event = signer
            .sign(
                buzz_sdk::build_message(
                    channel,
                    &format!("{token} note {index}"),
                    None,
                    &[],
                    false,
                    &[],
                    &[],
                )
                .unwrap()
                .custom_created_at(Timestamp::from(now - 600 + index * 10)),
            )
            .await
            .unwrap();
        assert!(session.publish(event).await.unwrap().accepted);
    }
    session.shutdown().await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "m5-search".into(),
        pubkey: pubkey.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "m5-search".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m5".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        signer.clone(),
    )
    .unwrap();
    let relay_pubkey = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    store.pin_relay_pubkey(community, &relay_pubkey).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    directory::refresh(community, &pubkey, &http, &store)
        .await
        .unwrap();
    let channels = store
        .call(move |store| store.channels(community))
        .await
        .unwrap();
    let service = bzz::service::search::SearchService::new(community, http.clone(), store.clone());
    let profiles = std::collections::HashMap::new();
    let messages = |output: bzz::service::search::SearchOutput| {
        output
            .results
            .into_iter()
            .filter(|result| result.kind == bzz::domain::SearchResultKind::Message)
            .map(|result| (result.stable_id, result.created_at))
            .collect::<Vec<_>>()
    };
    let mut seen = messages(
        service
            .execute(&token, &pubkey, &channels, &profiles)
            .await
            .unwrap(),
    );
    assert!(
        !seen.is_empty() && (seen.len() as u64) < TOTAL,
        "first page: {}",
        seen.len()
    );
    let mut pages = 1;
    loop {
        let bound = seen.iter().map(|(_, at)| *at).min().unwrap();
        let page = messages(
            service
                .older(&token, bound, &pubkey, &channels, &profiles)
                .await
                .unwrap(),
        );
        assert!(
            page.iter().all(|(_, at)| *at < bound),
            "every older result is older"
        );
        let fresh = page
            .into_iter()
            .filter(|(id, _)| !seen.iter().any(|(shown, _)| shown == id))
            .collect::<Vec<_>>();
        if fresh.is_empty() {
            break;
        }
        seen.extend(fresh);
        pages += 1;
        assert!(pages < 10, "paging terminates");
    }
    assert_eq!(
        seen.len() as u64,
        TOTAL,
        "every message reached in {pages} pages"
    );
    signer.lock().await;
}

/// One bzz profile of the M5 journey: its three folders, the owner's
/// passphrase on descriptor 3 and the passphrase of a moved agent's file on
/// descriptor 4, as a script gives them.
#[cfg(unix)]
struct MoveProfile {
    dirs: std::path::PathBuf,
    variables: Vec<(String, String)>,
}

#[cfg(unix)]
impl MoveProfile {
    fn new(dirs: std::path::PathBuf, passphrase: &str, variables: &[(String, String)]) -> Self {
        std::fs::create_dir_all(&dirs).unwrap();
        std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
        let profile = Self {
            dirs,
            variables: variables.to_vec(),
        };
        profile.file_passphrase("the passphrase of the moved file");
        profile
    }

    /// What `bzz agent move` and `receive` will read as the file's
    /// passphrase from now on.
    fn file_passphrase(&self, passphrase: &str) {
        std::fs::write(self.dirs.join("move-pass"), format!("{passphrase}\n")).unwrap();
    }

    fn paths(&self) -> bzz::paths::Paths {
        bzz::paths::Paths {
            config_dir: self.dirs.join("c"),
            data_dir: self.dirs.join("d"),
            cache_dir: self.dirs.join("k"),
        }
    }

    fn command(&self, args: &[&str], output: Option<&std::path::Path>) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("exec \"$0\" \"$@\" 3<\"$BZZ_M5_PASSPHRASE\" 4<\"$BZZ_M5_FILE_PASSPHRASE\"")
            .arg(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_M5_PASSPHRASE", self.dirs.join("pass"))
            .env("BZZ_M5_FILE_PASSPHRASE", self.dirs.join("move-pass"))
            .env("BZZ_CONFIG_DIR", self.dirs.join("c"))
            .env("BZZ_DATA_DIR", self.dirs.join("d"))
            .env("BZZ_CACHE_DIR", self.dirs.join("k"))
            .env("BZZ_PASSPHRASE_FD", "3")
            .env("BZZ_MOVE_PASSPHRASE_FD", "4")
            .envs(self.variables.iter().cloned())
            .stdin(std::process::Stdio::null());
        if let Some(output) = output {
            let file = std::fs::File::create(output).unwrap();
            command.stdout(file.try_clone().unwrap()).stderr(file);
        }
        command
    }

    /// Runs one command to its end: whether it succeeded, and what it said.
    fn said(&self, args: &[&str]) -> (bool, String) {
        let output = self.command(args, None).output().unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    fn ok(&self, args: &[&str]) -> String {
        let (success, text) = self.said(args);
        assert!(success, "bzz {args:?}: {text}");
        text
    }

    /// A command that must fail, and what it said.
    fn refused(&self, args: &[&str]) -> String {
        let (success, text) = self.said(args);
        assert!(!success, "bzz {args:?} should have been refused: {text}");
        text
    }
}

/// The processes a journey started. Whatever ends the test, also a failed
/// assertion, none of them is left running: a `bzz agent run` gets TERM and
/// stops its own host, and `buzz-acp` is killed with everything it started.
#[cfg(unix)]
#[derive(Default)]
struct Started(std::sync::Mutex<Vec<u32>>);

#[cfg(unix)]
impl Started {
    fn add(&self, child: &std::process::Child) {
        self.0.lock().unwrap().push(child.id());
    }
}

#[cfg(unix)]
impl Drop for Started {
    fn drop(&mut self) {
        let pids = std::mem::take(&mut *self.0.lock().unwrap());
        let signal = |signal: &str, target: String| {
            let _ = Command::new("kill")
                .args([signal, "--", &target])
                .stderr(std::process::Stdio::null())
                .status();
        };
        let alive = |pid: &u32| bzz::agents::runtime::process::identity_of(*pid).is_some();
        if !pids.iter().any(alive) {
            return;
        }
        for pid in pids.iter().filter(|pid| alive(pid)) {
            signal("-TERM", pid.to_string());
        }
        std::thread::sleep(Duration::from_secs(3));
        for pid in &pids {
            // Its group, when it leads one, and the process itself.
            signal("-KILL", format!("-{pid}"));
            signal("-KILL", pid.to_string());
        }
    }
}

/// A prompt of `buzz-acp` with everything that differs from run to run
/// taken out: ids, keys, times, names and what people wrote. What is left is
/// the wording and the fields the Pi Durable host reads the place and the
/// request from.
fn buzz_acp_prompt_shape(prompt: &str) -> String {
    fn is_hex(text: &str) -> bool {
        text.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
    fn word_shape(word: &str) -> String {
        // Punctuation that Buzz writes around an id stays where it is.
        let start = word
            .find(|character: char| character.is_ascii_alphanumeric())
            .unwrap_or(word.len());
        let end = word
            .rfind(|character: char| character.is_ascii_alphanumeric())
            .map_or(start, |last| last + 1);
        let (before, core, after) = (&word[..start], &word[start..end], &word[end..]);
        let parts = core.split('-').collect::<Vec<_>>();
        let uuid = parts.len() == 5
            && parts.iter().map(|part| part.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
            && parts.iter().all(|part| is_hex(part));
        let time = core.len() >= 19
            && core.as_bytes()[4] == b'-'
            && core.as_bytes()[10] == b'T'
            && core.as_bytes()[..4].iter().all(u8::is_ascii_digit);
        let shaped = if core.len() == 64 && is_hex(core) {
            "<event>"
        } else if uuid {
            "<uuid>"
        } else if core.starts_with("npub1") && core.len() > 50 {
            "<npub>"
        } else if time {
            "<time>"
        } else {
            core
        };
        format!("{before}{shaped}{after}")
    }
    let shape = |text: &str| {
        text.split(' ')
            .map(word_shape)
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Ids written with no space around them, as in a list of tags.
    let ids = |text: &str| {
        let mut out = String::new();
        let mut run = String::new();
        for character in text.chars() {
            if character.is_ascii_alphanumeric() || character == '-' {
                run.push(character);
            } else {
                out.push_str(&word_shape(&run));
                run.clear();
                out.push(character);
            }
        }
        out.push_str(&word_shape(&run));
        out
    };
    prompt
        .lines()
        .map(|line| {
            if line.starts_with("Content: ") {
                "Content: <what was written>".to_owned()
            } else if let Some(rest) = line.strip_prefix("Channel: ")
                && let Some(at) = rest.rfind(" (#")
            {
                format!("Channel: <name>{}", shape(&rest[at..]))
            } else if line.starts_with('[')
                && let Some(at) = line.find("): ")
                && let Some(number) = line.find("] ")
                && let Some(time) = line[..at].rfind(" (")
                && number < time
            {
                // A message of the thread, as context: its number, who wrote
                // it (a key, or a name and a key), when, and what.
                format!(
                    "{}] <who>{}): <what was written>",
                    &line[..number],
                    shape(&line[time..at])
                )
            } else if line.starts_with("Parsed: ") {
                // Buzz writes a name before the key of someone who has one.
                match (line.find("mentions=["), line.ends_with(']')) {
                    (Some(at), true) => format!("{}mentions=[<who>]", ids(&line[..at])),
                    _ => ids(line),
                }
            } else if line.starts_with("Tags: ") {
                ids(line)
            } else {
                shape(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The shape of a prompt does not depend on who has a name: Buzz writes
/// `Scout (<key>)` for someone with a profile and `<key>` for someone without.
#[test]
fn a_buzz_acp_prompts_shape_leaves_out_what_differs_from_run_to_run() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agents/buzz-acp-prompts");
    let recorded =
        std::fs::read_to_string(fixtures.join("02-same-mention-after-restart.txt")).unwrap();
    let shape = buzz_acp_prompt_shape(&recorded);
    for kept in [
        "<context>\nScope: thread\nSession scope: thread\nChannel: <name> (#<uuid>)\nThread root: <event>\n",
        "<thread-context included=\"1\" total=\"2\" truncated=\"false\">\n[1] <who> (<time>): <what was written>\n</thread-context>",
        "<buzz-event type=\"@mention\">\nEvent ID: <event>\nChannel: <name> (#<uuid>)\nKind: 9\nFrom: <npub> (hex: <event>)\nTime: <time>\nContent: <what was written>\n",
        "Tags: [[\"h\",\"<uuid>\"],[\"p\",\"<event>\"]]\nParsed: mentions=[<who>]\n</buzz-event>",
        "use `--reply-to <event>` on `buzz messages send`",
    ] {
        assert!(shape.contains(kept), "`{kept}` in:\n{shape}");
    }
    // The same prompt in another run: other ids, another time, another
    // channel, other words, and an agent with a name where this one has
    // none, or without one where this one has it.
    let after = |marker: &str, length: usize| {
        let at = recorded.find(marker).expect(marker) + marker.len();
        recorded[at..at + length].to_owned()
    };
    let agent = after("[\"p\",\"", 64);
    let channel = after("(#", 36);
    let time = after("Time: ", 19);
    let name = recorded[recorded.find("Channel: ").unwrap() + 9..]
        .split(" (#")
        .next()
        .unwrap()
        .to_owned();
    let content = recorded[recorded.find("Content: ").unwrap() + 9..]
        .lines()
        .next()
        .unwrap()
        .to_owned();
    assert!(!content.is_empty());
    let named = format!("Scout ({agent})");
    let renamed = if recorded.contains(&named) {
        recorded.replace(&named, &agent)
    } else {
        recorded
            .replace(
                &format!("[1] {agent} "),
                &format!("[1] Scout the 2nd ({agent}) "),
            )
            .replace(
                &format!("mentions=[{agent}]"),
                &format!("mentions=[Scout the 2nd ({agent})]"),
            )
    };
    assert_ne!(renamed, recorded, "the agent's name was not changed");
    let another = renamed
        .replace(&agent, &"ab".repeat(32))
        .replace(&channel, "00000000-1111-2222-3333-444444444444")
        .replace(
            &format!("Channel: {name} "),
            "Channel: a channel (with brackets) ",
        )
        .replace(&time[..18], "2027-01-02T03:04:5")
        .replace(&content, "something else entirely");
    for changed in [&agent, &channel, &time[..18], content.as_str()] {
        assert!(!another.contains(changed), "{changed} is still there");
    }
    assert_eq!(buzz_acp_prompt_shape(&another), shape);
    // A reply in a thread names its root beside the people it mentions.
    let reply = std::fs::read_to_string(fixtures.join("03-reply-in-thread.txt")).unwrap();
    assert!(
        buzz_acp_prompt_shape(&reply).contains(
            "Tags: [[\"h\",\"<uuid>\"],[\"e\",\"<event>\",\"\",\"reply\"],[\"p\",\"<event>\"]]\nParsed: root=<event>, mentions=[<who>]\n"
        ),
        "{}",
        buzz_acp_prompt_shape(&reply)
    );
    // What the host reads from is part of the shape: a reworded field is a
    // difference.
    for reworded in [
        recorded.replace("Event ID: ", "Event Id: "),
        recorded.replace("Thread root: ", "Thread: "),
        recorded.replace("Scope: thread", "Scope: channel"),
        recorded.replace("<buzz-event type=", "<event type="),
        recorded.replace("(#", "("),
    ] {
        assert_ne!(reworded, recorded);
        assert_ne!(buzz_acp_prompt_shape(&reworded), shape);
    }
}

/// v0.15 M5 exit gate, on the pinned relay, with the `bzz` binary, the real
/// Pi Durable host on its stand-in model, and Buzz's own `buzz-acp` and
/// `buzz` at the pin:
///
/// 1. an agent answers here and makes a file;
/// 2. `bzz agent move` while `bzz agent run` serves it: the file is sealed
///    and the first profile refuses to run the agent from then on;
/// 3. a second profile of the same owner `receive`s it, and the next message
///    in the same thread shows the conversation's memory and the file;
/// 4. `bzz agent launch-kit`, and the real `buzz-acp` started from the kit
///    answers the next message in that thread with the same memory;
/// 5. the owner's `bzz agent shutdown` stops it, and `bzz agent receive
///    <kit>` takes it back.
///
/// One reply per mention throughout, while the bzz of both profiles keeps
/// running. The part under `buzz-acp` is also the guard of decision D9: it
/// fails when the host no longer finds the place and the request in what
/// `buzz-acp` sends, and it is where the prompts in
/// `tests/fixtures/agents/buzz-acp-prompts/` are recorded again
/// (`BZZ_REFRESH_BUZZ_ACP_PROMPTS=1`).
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_agent_moves_to_another_bzz_and_to_buzz_acp() {
    use std::os::unix::{fs::PermissionsExt as _, process::CommandExt as _};

    use bzz::{
        agents::{launch_kit, runtime::process},
        auth::IdentityManager,
        config::LocalAgentState,
    };
    use nostr::nips::nip19::ToBech32 as _;
    use serde_json::json;
    use support::{
        agent_harness::{running as running_pid, until},
        pi_host,
    };

    let Some(node) = pi_host::node_or_skip("real_relay_agent_moves").await else {
        return;
    };
    const WAIT: Duration = Duration::from_secs(90);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let buzz_bin = std::path::PathBuf::from(
        std::env::var("BZZ_BUZZ_ACP").expect("relay wrapper builds buzz-acp at the pin"),
    )
    .parent()
    .expect("buzz-acp is in a folder")
    .to_path_buf();
    assert!(
        buzz_bin.join("buzz-acp").is_file() && buzz_bin.join("buzz").is_file(),
        "{} needs buzz-acp and buzz, built at the pin",
        buzz_bin.display()
    );
    // A failed run's folder is worth reading: `BZZ_TEST_KEEP=1` keeps it.
    let temporary = tempfile::TempDir::new().unwrap();
    let (work, _removed_at_the_end) = if std::env::var_os("BZZ_TEST_KEEP").is_some() {
        let kept = temporary.keep();
        eprintln!("this run's folder is kept: {}", kept.display());
        (kept, None)
    } else {
        (temporary.path().to_path_buf(), Some(temporary))
    };
    let work = work.as_path();
    let passphrase = "m5 disposable passphrase";
    let pi_dir = work.join("pi");
    let scripted_node = pi_host::scripted_node(&work.join("bin"), &node);
    let variables = pi_host::variables(&scripted_node, &pi_dir);

    // ---- the owner's first bzz --------------------------------------------
    let first = MoveProfile::new(work.join("first"), passphrase, &variables);
    first.ok(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let first_paths = first.paths();
    let mut config = Config::load(&first_paths).unwrap();
    let identity = config.identities[0].clone();
    config
        .add_community("c1".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&first_paths).unwrap();
    let _ = seed_member(&source, &identity.pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let manager = IdentityManager::new(&first_paths);
    let owner = manager.signer(&identity, Some(&secret)).unwrap();
    let owner_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        owner.clone(),
    );
    let owner_http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        owner.clone(),
    )
    .unwrap();
    let publish = |builder: EventBuilder| {
        let (supervisor, owner) = (owner_supervisor.clone(), owner.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    let query = |filter: serde_json::Value| {
        let http = owner_http.clone();
        async move { http.query_raw(&[filter]).await.unwrap() }
    };

    // A folder of this machine is granted to it: it must not move.
    let site = work.join("site of the first machine");
    std::fs::create_dir_all(&site).unwrap();
    first.ok(&[
        "agent",
        "new",
        "Scout",
        "--model",
        "scripted/scripted",
        "--instructions",
        "Be brief.",
        "--allow-host",
        "github.com",
        "--folder",
        &format!("site={}", site.display()),
        "--yes",
    ]);
    let agent = Config::load(&first_paths).unwrap().hosted_agents[0].clone();
    let agent_hex = agent.pubkey.clone();
    let channel = uuid::Uuid::new_v4();
    let channel_name = format!("m5-{}", &channel.simple().to_string()[..8]);
    publish(
        buzz_sdk::build_create_channel(
            channel,
            &channel_name,
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    first.ok(&["agent", "add", "Scout", &channel_name]);

    let message = |text: String, thread: Option<nostr::EventId>| {
        let agent = agent_hex.clone();
        let publish = &publish;
        async move {
            let thread = thread.map(|root| buzz_sdk::ThreadRef {
                root_event_id: root,
                parent_event_id: root,
            });
            publish(
                buzz_sdk::build_message(
                    channel,
                    &text,
                    thread.as_ref(),
                    &[agent.as_str()],
                    false,
                    &[],
                    &[],
                )
                .unwrap(),
            )
            .await
        }
    };
    let by_agent = || {
        query(
            json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 500}),
        )
    };
    // The agent's messages that answer `request` itself: a reply names the
    // thread's root too, and the request is the last event it names.
    let replies = |request: String| {
        let all = by_agent();
        async move {
            all.await
                .into_iter()
                .filter(|event| {
                    bzz::protocol::events::event_references(event).last() == Some(&request)
                })
                .map(|event| event.content)
                .collect::<Vec<_>>()
        }
    };
    let exactly_one = |request: String, what: &'static str| {
        let replies = &replies;
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies(request.clone()).await.is_empty() {
                assert!(tokio::time::Instant::now() < deadline, "{what}: no reply");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let all = replies(request).await;
            assert_eq!(all.len(), 1, "{what}: {all:?}");
            all.into_iter().next().unwrap()
        }
    };
    let logged = |log: &std::path::Path| std::fs::read_to_string(log).unwrap_or_default();
    let started = Started::default();
    // `bzz agent run`, until it has printed the agent's status line.
    let run = |profile: &MoveProfile, log: std::path::PathBuf, line: &'static str| {
        let child = profile
            .command(&["agent", "run"], Some(&log))
            .spawn()
            .unwrap();
        started.add(&child);
        async move {
            until("bzz agent run to say the agent's state", WAIT, || {
                std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .contains(line)
                    .then_some(())
            })
            .await;
            // Its intake subscribes a moment after the status line.
            tokio::time::sleep(Duration::from_secs(3)).await;
            child
        }
    };
    let stop = |mut child: std::process::Child, log: &std::path::Path| {
        let status = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            child.wait().unwrap().success(),
            "{}",
            std::fs::read_to_string(log).unwrap()
        );
    };
    let host_pid = |paths: &bzz::paths::Paths, id: uuid::Uuid| {
        process::read_record(&process::run_dir(&paths.data_dir), id)
            .map(|record| record.process.pid)
    };
    let pong = |messages: usize| {
        format!("pong: this conversation holds {messages} message(s) from people")
    };

    // ---- 1. it answers here, and makes a file -----------------------------
    let first_log = work.join("first-agent-run.log");
    let first_run = run(&first, first_log.clone(), "Scout · pi-durable · idle").await;
    let hello = message("hello there".into(), None).await;
    assert_eq!(
        exactly_one(hello.id.to_hex(), "the first mention").await,
        pong(1)
    );
    let wrote = message(
        r#"do: [["bash", {"command": "echo made before the move > /home/agent/note.txt && cat /home/agent/note.txt"}], ["report"]]"#.into(),
        None,
    )
    .await;
    assert!(
        exactly_one(wrote.id.to_hex(), "the file")
            .await
            .contains("made before the move")
    );
    let first_host = host_pid(&first_paths, agent.id).expect("the host is running");

    // ---- 2. bzz agent move, while `bzz agent run` serves the agent ---------
    let file = work.join("scout.bzzagent");
    std::fs::write(&file, b"something of the owner's").unwrap();
    let text = first.refused(&["agent", "move", "Scout", "--to", file.to_str().unwrap()]);
    assert!(text.contains("bzz does not write over it"), "{text}");
    assert_eq!(host_pid(&first_paths, agent.id), Some(first_host));
    std::fs::remove_file(&file).unwrap();
    let text = first.ok(&["agent", "move", "Scout", "--to", file.to_str().unwrap()]);
    for said in [
        "agent Scout moved into",
        "sealed with your passphrase",
        "not moved: its granted folder(s) (site) and git credentials",
        "Scout is marked moved away here: this bzz no longer starts it",
        "bzz agent receive <file>",
    ] {
        assert!(text.contains(said), "`{said}` in:\n{text}");
    }
    assert!(
        !running_pid(first_host),
        "its host was stopped for the move"
    );
    assert!(host_pid(&first_paths, agent.id).is_none());
    // The bzz that was serving it says so, and goes on running.
    until("the running bzz to see the move", WAIT, || {
        logged(&first_log)
            .contains("Scout · pi-durable · stopped · moved away: it runs elsewhere")
            .then_some(())
    })
    .await;
    assert_eq!(
        Config::load(&first_paths).unwrap().hosted_agents[0]
            .local
            .as_ref()
            .unwrap()
            .state,
        LocalAgentState::MovedAway
    );
    // The sealed file never holds the key, or anything else, in the clear.
    let sealed_key = std::fs::read_to_string(
        first_paths
            .keys_dir()
            .join(format!("agent-{}.nip44", agent.id)),
    )
    .unwrap();
    let agent_keys = Keys::parse(&owner.decrypt_self(sealed_key).await.unwrap()).unwrap();
    let key_forms = [
        ("hex", agent_keys.secret_key().to_secret_hex()),
        ("nsec", agent_keys.secret_key().to_bech32().unwrap()),
    ];
    let holds = |bytes: &[u8], needle: &str| {
        bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    };
    let sealed = std::fs::read(&file).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for (form, key) in &key_forms {
        assert!(
            !holds(&sealed, key),
            "the key ({form}) is in the sealed file"
        );
    }
    for plain in [
        "Scout",
        "Be brief.",
        agent_hex.as_str(),
        "made before the move",
        "localhost:3030",
        passphrase,
    ] {
        assert!(
            !holds(&sealed, plain),
            "{plain:?} is readable in the sealed file"
        );
    }

    // ---- the first profile refuses to run it ------------------------------
    assert!(first.ok(&["agent", "list"]).contains("Scout\tmoved away"));
    let shown = first.ok(&["agent", "show", "Scout"]);
    for said in [
        "state:       moved away",
        "in a sealed file, for another bzz",
        "this bzz does not start it: it runs elsewhere. `bzz agent receive` takes it back",
        "on the relay: not present: no copy of it is running",
    ] {
        assert!(shown.contains(said), "`{said}` in:\n{shown}");
    }
    let text = first.refused(&["agent", "resume", "Scout"]);
    assert!(text.contains("was moved away and runs elsewhere"), "{text}");
    let text = first.refused(&[
        "agent",
        "move",
        "Scout",
        "--to",
        work.join("again.bzzagent").to_str().unwrap(),
    ]);
    assert!(text.contains("was already moved away"), "{text}");
    // A bzz started now does not start it either. It stays up to the end of
    // the journey, as does the one that was serving it: neither may answer.
    let refusing_log = work.join("first-agent-run-after.log");
    let refusing_run = run(
        &first,
        refusing_log.clone(),
        "Scout · pi-durable · stopped · moved away: it runs elsewhere",
    )
    .await;

    // ---- 3. the owner's second bzz receives it -----------------------------
    let backup = work.join("owner.backup");
    first.ok(&[
        "identity",
        "backup",
        &identity.id.to_string(),
        "--output",
        backup.to_str().unwrap(),
    ]);
    let second = MoveProfile::new(work.join("second"), passphrase, &variables);
    second.ok(&[
        "identity",
        "import-backup",
        "--label",
        "owner",
        "--input",
        backup.to_str().unwrap(),
        "--backend",
        "encrypted-file",
    ]);
    let second_paths = second.paths();
    let mut second_config = Config::load(&second_paths).unwrap();
    assert_eq!(second_config.identities[0].pubkey, identity.pubkey);
    let second_identity = second_config.identities[0].id;
    second_config
        .add_community(
            "c1".into(),
            "ws://localhost:3030".into(),
            second_identity,
            true,
        )
        .unwrap();
    second_config.save(&second_paths).unwrap();
    let nothing_arrived = |paths: &bzz::paths::Paths| {
        Config::load(paths).unwrap().hosted_agents.is_empty()
            && !paths.data_dir.join("agents").exists()
            && std::fs::read_dir(paths.keys_dir())
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with("agent-"))
                        .count()
                })
                .unwrap_or(0)
                == 0
    };
    // Refused whole, and named: the wrong passphrase, a changed file, a file
    // cut short.
    let file_arg = file.to_str().unwrap();
    second.file_passphrase("not the passphrase of the file");
    let text = second.refused(&["agent", "receive", file_arg]);
    assert!(text.contains("the passphrase is wrong"), "{text}");
    second.file_passphrase("the passphrase of the moved file");
    let mut changed = sealed.clone();
    changed[sealed.len() / 2] ^= 0x01;
    let broken = work.join("broken.bzzagent");
    std::fs::write(&broken, &changed).unwrap();
    let text = second.refused(&["agent", "receive", broken.to_str().unwrap()]);
    assert!(text.contains("was changed after bzz made it"), "{text}");
    std::fs::write(&broken, &sealed[..sealed.len() - 100]).unwrap();
    let text = second.refused(&["agent", "receive", broken.to_str().unwrap()]);
    assert!(text.contains("was cut short"), "{text}");
    assert!(nothing_arrived(&second_paths));
    // Another owner's bzz in the same community: refused.
    let stranger = MoveProfile::new(work.join("stranger"), passphrase, &variables);
    stranger.ok(&[
        "identity",
        "new",
        "--label",
        "someone else",
        "--backend",
        "encrypted-file",
    ]);
    let stranger_paths = stranger.paths();
    let mut stranger_config = Config::load(&stranger_paths).unwrap();
    let stranger_identity = stranger_config.identities[0].id;
    stranger_config
        .add_community(
            "c1".into(),
            "ws://localhost:3030".into(),
            stranger_identity,
            true,
        )
        .unwrap();
    stranger_config.save(&stranger_paths).unwrap();
    let text = stranger.refused(&["agent", "receive", file_arg]);
    assert!(
        text.contains("An agent is received only by its owner. Nothing was received"),
        "{text}"
    );
    assert!(nothing_arrived(&stranger_paths));

    let text = second.ok(&["agent", "receive", file_arg]);
    for said in [
        &format!("agent Scout ({agent_hex}) received in c1"),
        "its key is in bzz's custody, sealed to owner",
        "no folder of this machine is granted to it",
        "it is active",
    ] {
        assert!(text.contains(said), "`{said}` in:\n{text}");
    }
    // The same file twice does not make two agents.
    let text = second.refused(&["agent", "receive", file_arg]);
    assert!(
        text.contains("is already here and was not moved away"),
        "{text}"
    );
    let arrived = Config::load(&second_paths).unwrap().hosted_agents;
    assert_eq!(arrived.len(), 1);
    let arrived = arrived[0].clone();
    let arrived_local = arrived.local.clone().unwrap();
    assert_eq!(
        (
            arrived.pubkey.as_str(),
            arrived_local.state,
            arrived_local.folders.len(),
            arrived_local.network.as_slice(),
            arrived_local.instructions.as_str(),
        ),
        (
            agent_hex.as_str(),
            LocalAgentState::Active,
            0,
            ["github.com".to_owned()].as_slice(),
            "Be brief.",
        )
    );
    assert!(second.ok(&["agent", "list"]).contains("Scout\tactive"));

    // The next message in the same thread: the conversation's memory came.
    let second_log = work.join("second-agent-run.log");
    let second_run = run(&second, second_log.clone(), "Scout · pi-durable · idle").await;
    let again = message("and now?".into(), Some(hello.id)).await;
    assert_eq!(
        exactly_one(again.id.to_hex(), "in the second bzz, the same thread").await,
        pong(2),
        "the conversation continued in the second bzz"
    );
    // And the file the agent had made is on the disk that came.
    let read = message(
        r#"do: [["bash", {"command": "cat /home/agent/note.txt"}], ["report"]]"#.into(),
        None,
    )
    .await;
    assert!(
        exactly_one(read.id.to_hex(), "the file, in the second bzz")
            .await
            .contains("made before the move")
    );
    let second_host = host_pid(&second_paths, arrived.id).expect("its host runs in the second bzz");
    assert!(
        host_pid(&first_paths, agent.id).is_none(),
        "and not in the first"
    );

    // ---- 4. bzz agent launch-kit, and Buzz's own buzz-acp ------------------
    let kit = work.join("scout-kit");
    let kit_arg = kit.to_str().unwrap();
    let text = second.ok(&["agent", "launch-kit", "Scout", "--to", kit_arg, "--yes"]);
    for said in [
        "A launch kit holds the key of Scout in the clear",
        "launch kit for Scout written in",
        "THE AGENT'S KEY IS IN THE CLEAR in",
        "Scout is marked moved away here",
    ] {
        assert!(text.contains(said), "`{said}` in:\n{text}");
    }
    assert!(
        !running_pid(second_host),
        "its host was stopped for the kit"
    );
    until("the second bzz to see the move", WAIT, || {
        logged(&second_log)
            .contains("Scout · pi-durable · stopped · moved away: it runs elsewhere")
            .then_some(())
    })
    .await;
    // The kit's key file is its owner's alone, and nothing else in the kit
    // holds the key.
    let key_file = kit.join(launch_kit::ENV_FILE);
    assert_eq!(
        std::fs::metadata(&key_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(holds(&std::fs::read(&key_file).unwrap(), &key_forms[0].1));
    let mut folders = vec![kit.clone()];
    let mut kit_files = 0;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path != key_file {
                kit_files += 1;
                let bytes = std::fs::read(&path).unwrap();
                for (form, key) in &key_forms {
                    assert!(
                        !holds(&bytes, key),
                        "the key ({form}) is in {}",
                        path.display()
                    );
                }
                assert!(
                    !holds(&bytes, &site.display().to_string()),
                    "a folder of the first machine is named in {}",
                    path.display()
                );
            }
        }
    }
    assert!(kit_files > 20, "{kit_files} files beside the key's");

    // On "the other machine": the packages from the kit's own lock file, as
    // its README says, and buzz-acp started from the kit's folder. `node`
    // on that machine's PATH is the stand-in that switches on the host's
    // scripted model; nothing of bzz is there.
    let installed = Command::new("npm")
        .args([
            "ci",
            "--ignore-scripts",
            "--omit=optional",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
        ])
        .current_dir(kit.join(launch_kit::HOST_FOLDER))
        .env("npm_config_cache", pi_host::npm_cache())
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "npm ci in the kit: {}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let recorded = work.join("recorded-prompts");
    let machine_path = format!(
        "{}:{}:{}:/usr/bin:/bin",
        scripted_node.parent().unwrap().display(),
        buzz_bin.display(),
        node.program.parent().unwrap().display()
    );
    let start_kit = |log: std::path::PathBuf| {
        let file = std::fs::File::create(&log).unwrap();
        let child = Command::new(kit.join("start.sh"))
            .env_clear()
            .env("PATH", &machine_path)
            .env("HOME", work.join("home of the other machine"))
            .env("PI_CODING_AGENT_DIR", &pi_dir)
            .env("BZZ_AGENT_HOST_RECORD", &recorded)
            .stdin(std::process::Stdio::null())
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .process_group(0)
            .spawn()
            .expect("the kit's start.sh runs");
        started.add(&child);
        async move {
            until("buzz-acp to be on the relay", WAIT, || {
                std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .contains("presence set to online")
                    .then_some(())
            })
            .await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            child
        }
    };
    let kill_group = |child: &mut std::process::Child| {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .status();
        let _ = child.wait();
    };
    std::fs::create_dir_all(work.join("home of the other machine")).unwrap();
    let acp_log = work.join("buzz-acp-1.log");
    let mut acp = start_kit(acp_log.clone()).await;
    // Its presence is its status: `agent show` says it is there.
    until("the moved agent to be present on the relay", WAIT, || {
        second
            .ok(&["agent", "show", "Scout"])
            .contains("on the relay: present (online)")
            .then_some(())
    })
    .await;
    let shown = second.ok(&["agent", "show", "Scout"]);
    assert!(
        shown.contains("as a launch kit, for a machine with Buzz's buzz-acp"),
        "{shown}"
    );
    // What the host must find in buzz-acp's prompt, said when it does not.
    let guard = |log: &std::path::Path| {
        format!(
            "under buzz-acp at {PIN}. If there is no reply, or it is not the conversation's, the host no longer finds the place and the request in buzz-acp's prompt (decision D9): did Buzz change its wording?\nbuzz-acp and the host said:\n{}",
            logged(log)
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    // One reply under buzz-acp. With none, the guard says what that means.
    let under_acp = |request: String, what: &'static str, log: std::path::PathBuf| {
        let (replies, exactly_one, guard) = (&replies, &exactly_one, &guard);
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies(request.clone()).await.is_empty() {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "{what}: no reply {}",
                    guard(&log)
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            exactly_one(request, what).await
        }
    };
    // The next message in the same thread, answered by buzz-acp's agent with
    // the same memory: the place and the request were found in its prompt.
    let third = message("and under buzz-acp?".into(), Some(hello.id)).await;
    let reply = under_acp(
        third.id.to_hex(),
        "under buzz-acp, the same thread",
        acp_log.clone(),
    )
    .await;
    assert_eq!(reply, pong(3), "{}", guard(&acp_log));
    // A new thread there is a new conversation.
    let one = message("ping one".into(), None).await;
    let reply = under_acp(
        one.id.to_hex(),
        "under buzz-acp, a new thread",
        acp_log.clone(),
    )
    .await;
    assert_eq!(reply, pong(1), "{}", guard(&acp_log));
    // buzz-acp is killed with everything it started, and started again: it
    // hands the mention over again, and the host knows the request.
    kill_group(&mut acp);
    let acp_log = work.join("buzz-acp-2.log");
    let mut acp = start_kit(acp_log.clone()).await;
    let two = message("ping two, same thread".into(), Some(one.id)).await;
    let reply = under_acp(two.id.to_hex(), "after buzz-acp restarted", acp_log.clone()).await;
    assert_eq!(reply, pong(2), "{}", guard(&acp_log));
    let three = message("ping three, a new thread".into(), None).await;
    let reply = under_acp(three.id.to_hex(), "another new thread", acp_log.clone()).await;
    assert_eq!(reply, pong(1), "{}", guard(&acp_log));
    assert_eq!(
        replies(one.id.to_hex()).await.len(),
        1,
        "a mention handed over again is not answered again"
    );
    // Each reply is the agent's own, with the owner's authorization, as
    // every message it wrote under bzz.
    for event in by_agent().await {
        let auth = event
            .tags
            .iter()
            .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
            .unwrap_or_else(|| panic!("no owner tag on {:?}", event.content));
        assert_eq!(
            buzz_sdk::nip_oa::verify_auth_tag(
                &serde_json::to_string(auth.as_slice()).unwrap(),
                &event.pubkey
            )
            .unwrap(),
            owner.public_key()
        );
    }

    // ---- decision D9: what buzz-acp sent, against the recorded prompts -----
    // The prompts of this run, in the order the host received them, each
    // with the event it asks about.
    let mut prompts = std::fs::read_dir(&recorded)
        .unwrap()
        .flatten()
        .map(|entry| {
            let path = entry.path();
            let modified = entry.metadata().unwrap().modified().unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            // `prompt-<pid>-<n>.txt`
            let number = name
                .trim_end_matches(".txt")
                .rsplit('-')
                .next()
                .and_then(|number| number.parse::<u32>().ok())
                .unwrap_or(0);
            (modified, number, std::fs::read_to_string(path).unwrap())
        })
        .collect::<Vec<_>>();
    prompts.sort_by_key(|prompt| (prompt.0, prompt.1));
    let asks_about = |text: &str, event: &nostr::Event| {
        text.lines()
            .filter_map(|line| line.strip_prefix("Event ID: "))
            .next_back()
            == Some(event.id.to_hex().as_str())
    };
    let position = |event: &nostr::Event, from: usize| {
        prompts
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, (_, _, text))| asks_about(text, event))
            .map(|(index, _)| index)
    };
    let first_of_one = position(&one, 0).expect("the prompt for the new thread");
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agents/buzz-acp-prompts");
    // The four situations the recorded prompts cover, as this run saw them.
    let fresh = [
        (
            "01-top-level-mention.txt",
            "a top-level mention in a channel: it starts a thread, so its place is the channel and its own event id",
            Some(first_of_one),
            &one,
            &one,
        ),
        (
            "02-same-mention-after-restart.txt",
            "the same mention handed over again after buzz-acp was killed and restarted, now with the thread's context: same place, same request",
            position(&one, first_of_one + 1),
            &one,
            &one,
        ),
        (
            "03-reply-in-thread.txt",
            "a second mention, written as a reply in that thread: same place, another request",
            position(&two, 0),
            &two,
            &one,
        ),
        (
            "04-new-top-level-mention.txt",
            "another top-level mention in the same channel: another place",
            position(&three, 0),
            &three,
            &three,
        ),
    ];
    let refresh = std::env::var_os("BZZ_REFRESH_BUZZ_ACP_PROMPTS").is_some();
    let mut expected = Vec::new();
    for (name, what, index, request, root) in &fresh {
        let index = index.unwrap_or_else(|| {
            panic!(
                "buzz-acp sent no prompt for `{name}` ({what}). {}",
                guard(&acp_log)
            )
        });
        let text = &prompts[index].2;
        if refresh {
            std::fs::write(fixtures.join(name), text).unwrap();
        } else {
            // The host's own tests read the recorded prompts. They stand
            // for what buzz-acp sends only while it still sends this.
            let kept = std::fs::read_to_string(fixtures.join(name)).unwrap();
            assert_eq!(
                buzz_acp_prompt_shape(text),
                buzz_acp_prompt_shape(&kept),
                "buzz-acp at {PIN} no longer words its prompt as tests/fixtures/agents/buzz-acp-prompts/{name} has it. Record the prompts again (the README in that folder says how), then run `npm test` in agent-host/: it says whether the host still finds the place and the request"
            );
        }
        // What the host must read from it: known here because this test
        // published the events, not because anything parsed the prompt.
        let quoted = |text: &str| serde_json::to_string(text).unwrap();
        expected.push(format!(
            "    {{\n      \"file\": {},\n      \"what\": {},\n      \"channel\": {},\n      \"request\": {},\n      \"place\": {}\n    }}",
            quoted(name),
            quoted(what),
            quoted(&channel.to_string()),
            quoted(&request.id.to_hex()),
            quoted(&format!("{channel}/{}", root.id.to_hex())),
        ));
    }
    if refresh {
        let text = format!(
            "{{\n  \"buzz\": \"{PIN}\",\n  \"prompts\": [\n{}\n  ]\n}}\n",
            expected.join(",\n")
        );
        std::fs::write(fixtures.join("expected.json"), text).unwrap();
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "recorded the prompts of buzz-acp at {PIN} again in {}",
            fixtures.display()
        );
    }

    // ---- 5. the owner's shutdown, and the way back -------------------------
    let text = second.ok(&["agent", "shutdown", "Scout"]);
    assert!(
        text.contains("sent Buzz's stop command (`!shutdown`) to Scout as its owner")
            && text.contains("Scout left: it is no longer present on the relay"),
        "{text}"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let exit = loop {
        if let Some(status) = acp.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    if exit.is_none() {
        kill_group(&mut acp);
    }
    assert!(
        exit.is_some_and(|status| status.success()),
        "buzz-acp left by itself, with code 0: {exit:?}\n{}",
        logged(&acp_log)
    );
    assert!(
        second
            .ok(&["agent", "show", "Scout"])
            .contains("on the relay: not present: no copy of it is running")
    );
    // Nobody answers now: not the stopped copy, and not the bzz of either
    // profile, both of which are still running.
    let silent = message("is anyone there?".into(), None).await;
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert!(replies(silent.id.to_hex()).await.is_empty());

    // ---- 5b. the stop command, wherever bzz sends it ----------------------
    // The run above had one channel, and the command chose it. Here: a
    // channel named with `--channel` when another would be chosen, a Solo
    // chat chosen before any channel, and a direct message chosen before
    // both. Each time the copy under buzz-acp is started again from the
    // kit, hears the command where it was sent, and leaves.
    // A second channel, whose name sorts before the first one's: with no
    // flag the command would go there.
    let earlier = uuid::Uuid::new_v4();
    let earlier_name = format!("a5-{}", &earlier.simple().to_string()[..8]);
    // And one the agent is not in.
    let without = uuid::Uuid::new_v4();
    let without_name = format!("z5-{}", &without.simple().to_string()[..8]);
    for (id, name) in [(earlier, &earlier_name), (without, &without_name)] {
        publish(
            buzz_sdk::build_create_channel(
                id,
                name,
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    second.ok(&["agent", "add", "Scout", &earlier_name]);
    let (success, text) = second.said(&["agent", "shutdown", "Scout", "--channel", &without_name]);
    assert!(
        !success
            && text.contains(&format!("Scout is not a member of {without_name}"))
            && text.contains("hears a command only where it is a member"),
        "a channel the agent is not in: {text}"
    );
    for (round, place) in ["--channel", "a Solo chat", "a direct message"]
        .into_iter()
        .enumerate()
    {
        // The place is made before the copy starts: buzz-acp learns the
        // agent's channels when it starts.
        let solo_name = format!("s5-{}", &channel.simple().to_string()[..8]);
        match place {
            "a Solo chat" => {
                publish(
                    bzz::protocol::sidebar::solo_creation(uuid::Uuid::new_v4(), &solo_name)
                        .unwrap(),
                )
                .await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                second.ok(&["agent", "add", "Scout", &solo_name]);
            }
            "a direct message" => {
                publish(buzz_sdk::build_dm_open(&[agent_hex.as_str()]).unwrap()).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            _ => {}
        }
        // buzz-acp reads a few seconds back when it starts (its first
        // subscription asks from five seconds before its start), so a copy
        // started right after a stop command hears that command again and
        // leaves at once. Seen here: the copy of the third round left 14 ms
        // after it was online, on the command of the second.
        tokio::time::sleep(Duration::from_secs(10)).await;
        let log = work.join(format!("buzz-acp-stop-{round}.log"));
        let mut copy = start_kit(log.clone()).await;
        let present = tokio::time::Instant::now() + WAIT;
        loop {
            let shown = second.ok(&["agent", "show", "Scout"]);
            if shown.contains("on the relay: present (online)") {
                break;
            }
            assert!(
                tokio::time::Instant::now() < present,
                "{place}: the copy never was present on the relay.\n`agent show` said:\n{shown}\nbuzz-acp said:\n{}",
                logged(&log)
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let (success, text) = if place == "--channel" {
            second.said(&["agent", "shutdown", "Scout", "--channel", &channel_name])
        } else {
            second.said(&["agent", "shutdown", "Scout"])
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let exit = loop {
            if let Some(status) = copy.try_wait().unwrap() {
                break Some(status);
            }
            if std::time::Instant::now() >= deadline {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        if exit.is_none() {
            kill_group(&mut copy);
        }
        let sent_in = |name: &str| text.contains(&format!("as its owner, in #{name}\n"));
        match place {
            "--channel" => assert!(
                sent_in(&channel_name) && !sent_in(&earlier_name),
                "--channel names the place: {text}"
            ),
            "a Solo chat" => assert!(
                sent_in(&solo_name),
                "a Solo chat is chosen before a channel: {text}"
            ),
            _ => assert!(
                text.contains("as its owner, in #")
                    && ![&channel_name, &earlier_name, &solo_name]
                        .into_iter()
                        .any(|name| sent_in(name)),
                "a direct message is chosen before a Solo chat and a channel: {text}"
            ),
        }
        assert!(
            success && text.contains("Scout left: it is no longer present on the relay"),
            "{place}: {text}\n{}",
            logged(&log)
        );
        assert!(
            exit.is_some_and(|status| status.success()),
            "{place}: buzz-acp left by itself, with code 0: {exit:?}\n{}",
            logged(&log)
        );
        assert!(
            logged(&log).contains("shutdown command from owner"),
            "{place}: buzz-acp says why it left:\n{}",
            logged(&log)
        );
    }

    // The kit comes back to the second bzz, which had moved the agent away:
    // it is taken back, with what it remembers from its time under buzz-acp.
    stop(second_run, &second_log);
    let text = second.ok(&["agent", "receive", kit_arg]);
    assert!(
        text.contains("taken back: its files here were replaced by the ones that arrived")
            && text.contains("delete the launch kit's folder now"),
        "{text}"
    );
    assert!(second.ok(&["agent", "list"]).contains("Scout\tactive"));
    let back_log = work.join("second-agent-run-back.log");
    let back_run = run(&second, back_log.clone(), "Scout · pi-durable · idle").await;
    let fourth = message("and back in bzz?".into(), Some(hello.id)).await;
    assert_eq!(
        exactly_one(fourth.id.to_hex(), "taken back, the same thread").await,
        pong(4),
        "bzz, bzz, buzz-acp and bzz again: one conversation"
    );
    let fifth = message("and this thread?".into(), Some(one.id)).await;
    assert_eq!(
        exactly_one(
            fifth.id.to_hex(),
            "taken back, the thread begun under buzz-acp"
        )
        .await,
        pong(3)
    );

    // ---- one reply per mention, throughout ---------------------------------
    tokio::time::sleep(Duration::from_secs(3)).await;
    for (what, request) in [
        ("hello", &hello),
        ("the file", &wrote),
        ("the same thread, second bzz", &again),
        ("the file, second bzz", &read),
        ("the same thread, buzz-acp", &third),
        ("a new thread, buzz-acp", &one),
        ("after the restart", &two),
        ("another thread", &three),
        ("taken back", &fourth),
        ("taken back, the other thread", &fifth),
    ] {
        assert_eq!(
            replies(request.id.to_hex()).await.len(),
            1,
            "{what}: one reply"
        );
    }
    // What was asked while nobody ran it was nobody's to answer later: the
    // bzz that took it back does not answer what came before.
    assert!(replies(silent.id.to_hex()).await.is_empty());
    assert_eq!(
        by_agent().await.len(),
        10,
        "ten mentions answered, ten messages by the agent"
    );
    // The first profile never started it again.
    assert!(host_pid(&first_paths, agent.id).is_none());
    for log in [&first_log, &refusing_log] {
        let since_the_move = logged(log).split("moved away").skip(1).collect::<String>();
        assert!(
            !since_the_move.is_empty() && !since_the_move.contains("answering"),
            "the first profile ran it after the move:\n{}",
            logged(log)
        );
    }
    stop(back_run, &back_log);
    stop(refusing_run, &refusing_log);
    stop(first_run, &first_log);
    owner_supervisor.shutdown().await;
    owner.lock().await;
}

/// Runs the bzz binary on a disposable profile; the passphrase reaches it
/// only as fd 3. Output goes to `output` when given.
#[cfg(unix)]
fn bzz_on_profile(
    dirs: &std::path::Path,
    args: &[&str],
    output: Option<&std::path::Path>,
) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("exec \"$0\" \"$@\" 3<\"$BZZ_M1_PASSPHRASE\"")
        .arg(env!("CARGO_BIN_EXE_bzz"))
        .args(args)
        .env("BZZ_M1_PASSPHRASE", dirs.join("pass"))
        .env("BZZ_CONFIG_DIR", dirs.join("c"))
        .env("BZZ_DATA_DIR", dirs.join("d"))
        .env("BZZ_CACHE_DIR", dirs.join("k"))
        .env("BZZ_PASSPHRASE_FD", "3");
    if let Some(output) = output {
        let file = std::fs::File::create(output).unwrap();
        command
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .stdin(std::process::Stdio::null());
    }
    command
}

/// v0.15 M1 exit gate, on the pinned relay: a mention of a local agent is
/// answered exactly once through a scripted harness, with the harness killed
/// before, during and after the publish, with a harness that sends twice,
/// and with bzz itself killed. The agent is a plain channel member (no `bot`
/// role), its requests come from its own relay connection, and no key
/// reaches the harness.
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_local_agent_answers_a_mention_exactly_once() {
    use bzz::{
        agents::runtime::{Limits, process, service},
        auth::IdentityManager,
        broker::{custody, relay as broker_relay},
        config::{Harness, LocalAgentConfig},
        diagnostics::DiagnosticHandle,
        paths::Paths,
        store::agent_runtime::{NewRequest, ReplyOrigin},
    };
    use nostr::JsonUtil as _;
    use serde_json::json;
    use support::agent_harness::{Script, kill, running as running_pid, until};

    const WAIT: Duration = Duration::from_secs(30);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = "m1 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    let cli = |args: &[&str]| {
        let output = bzz_on_profile(dirs, args, None).output().unwrap();
        assert!(
            output.status.success(),
            "bzz {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // The profile, made by bzz itself: an owner, a community, an agent whose
    // key bzz holds sealed.
    cli(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    let community = config
        .add_community("m1".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    cli(&["broker", "create", "Scout", "--yes"]);
    let mut config = Config::load(&paths).unwrap();
    let mut agent = config.hosted_agents[0].clone();
    assert_eq!(
        agent.local, None,
        "`broker create` still makes a v0.13 hosted agent"
    );
    let agent_hex = agent.pubkey.clone();

    let _ = seed_member(&source, &identity.pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let manager = IdentityManager::new(&paths);
    let owner = manager.signer(&identity, Some(&secret)).unwrap();
    let ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let http_base = url::Url::parse("http://localhost:3030/").unwrap();
    let owner_supervisor = SupervisorHandle::spawn(ws.clone(), owner.clone());
    let owner_http = HttpClient::new(http_base.clone(), owner.clone()).unwrap();
    let publish = |event: nostr::Event| {
        let supervisor = owner_supervisor.clone();
        async move {
            let ack = supervisor.publish(event).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            ack
        }
    };

    // A channel where the agent is a plain member: no `bot` role (ADR v0.15,
    // decision 11), which the v0.13 broker would have required.
    let channel = uuid::Uuid::new_v4();
    publish(
        owner
            .sign(
                buzz_sdk::build_create_channel(
                    channel,
                    &format!("m1-{}", &channel.simple().to_string()[..8]),
                    Some(buzz_sdk::Visibility::Open),
                    Some(buzz_sdk::ChannelKind::Stream),
                    None,
                    None,
                    false,
                )
                .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    publish(
        owner
            .sign(buzz_sdk::build_add_member(channel, &agent_hex, None).unwrap())
            .await
            .unwrap(),
    )
    .await;
    // The owner's DM with the agent, and someone else in the channel.
    let opened = publish(
        owner
            .sign(buzz_sdk::build_dm_open(&[agent_hex.as_str()]).unwrap())
            .await
            .unwrap(),
    )
    .await;
    let dm = command_channel_id(&opened.message);
    let stranger_keys = Keys::generate();
    let _ = seed_member(&source, &stranger_keys.public_key().to_hex());
    let stranger = SignerHandle::spawn(stranger_keys.clone());
    let stranger_supervisor = SupervisorHandle::spawn(ws.clone(), stranger.clone());
    tokio::time::sleep(Duration::from_millis(500)).await;

    let message = |signer: SignerHandle,
                   supervisor: SupervisorHandle,
                   channel,
                   text: String,
                   mention: bool| {
        let agent = agent_hex.clone();
        async move {
            let mentions = if mention {
                vec![agent.as_str()]
            } else {
                Vec::new()
            };
            let event = signer
                .sign(
                    buzz_sdk::build_message(channel, &text, None, &mentions, false, &[], &[])
                        .unwrap(),
                )
                .await
                .unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    let mention = |text: &str| {
        message(
            owner.clone(),
            owner_supervisor.clone(),
            channel,
            text.to_owned(),
            true,
        )
    };
    // Every message the relay holds from the agent in `channel`.
    let by_agent = |channel: uuid::Uuid| {
        let (http, agent) = (owner_http.clone(), agent_hex.clone());
        async move {
            http.query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent], "limit": 500})])
                .await
                .unwrap()
        }
    };
    // The agent's replies to `request`, as the relay holds them.
    let replies = |request: &nostr::Event| {
        let id = request.id.to_hex();
        let all = by_agent(channel);
        async move {
            all.await
                .into_iter()
                .filter(|event| bzz::protocol::events::event_references(event).contains(&id))
                .map(|event| event.content)
                .collect::<Vec<_>>()
        }
    };
    // Waits for the first reply, then for anything that would follow it.
    let exactly_one = |request: nostr::Event, what: &'static str| {
        let replies = &replies;
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies(&request).await.is_empty() {
                assert!(tokio::time::Instant::now() < deadline, "{what}: no reply");
                // The relay allows a person 300 HTTP calls a minute.
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let all = replies(&request).await;
            assert_eq!(all.len(), 1, "{what}: {all:?}");
            all.into_iter().next().unwrap()
        }
    };

    // ---- the runtime in this process, as the TUI runs it ------------------
    let script = Script::new(&dirs.join("harness-a"));
    let send = |text: &str| json!({"tool": "send", "arguments": {"text": text, "final": true}});
    script.write(
        json!({"own_host": true}),
        json!([
            {"match": "plain question", "steps": [{"sleep_ms": 1500}, send("plain answer")]},
            {"match": "kill before", "steps": [{"exit": 1}]},
            {"match": "kill before", "resume": true, "steps": [send("answered after a kill before the publish")]},
            {"match": "kill during", "steps": [{"tool": "send", "arguments": {"text": "answered while being killed", "final": true}, "nowait": true}, {"exit": 1}]},
            {"match": "kill during", "resume": true, "steps": [send("answered again after a kill during the publish")]},
            {"match": "kill after", "steps": [send("answered, then killed"), {"exit": 1}]},
            {"match": "kill after", "resume": true, "steps": [send("answered a second time")]},
            {"match": "send twice", "steps": [send("the first wording"), send("the second wording")]},
            {"match": "dm question", "steps": [send("dm answer")]},
        ]),
    );
    let mut local = LocalAgentConfig::new(Harness::PiDurable);
    local.command = script.command();
    agent.local = Some(local);
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let limits = Limits {
        max_crashes: 10,
        stop_grace: Duration::from_millis(500),
        typing_interval: Duration::from_millis(400),
        ..Limits::default()
    };
    let serve = |agent: bzz::config::HostedAgentConfig| {
        let (paths, store, community, owner) = (
            paths.clone(),
            store.clone(),
            config.communities[0].clone(),
            owner.clone(),
        );
        async move {
            let running = service::serve(service::ServeOptions {
                paths: &paths,
                store,
                diagnostics: DiagnosticHandle::disabled(),
                community: &community,
                agents: &[agent],
                owner: &owner,
                owner_identity: identity.id,
                federated: None,
                limits,
                owner_present: false,
                environment: None,
            })
            .await
            .unwrap()
            .expect("one local agent");
            assert!(running.problems.is_empty(), "{:?}", running.problems);
            let deadline = tokio::time::Instant::now() + WAIT;
            while !running.listening() {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the agent's intake never subscribed"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            running
        }
    };
    let running = serve(agent.clone()).await;
    assert!(script.starts().is_empty(), "no process before a request");

    // A mention is answered once; the agent's typing indicator shows first.
    let mut owner_events = owner_supervisor.subscribe_events();
    owner_supervisor
        .subscribe(
            "m1-typing",
            bzz::realtime::subscriptions::agent_typing(channel, Timestamp::now().as_secs()),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let plain = mention("plain question").await;
    assert_eq!(exactly_one(plain.clone(), "plain").await, "plain answer");
    let mut typing = Vec::new();
    while let Ok(event) = owner_events.try_recv() {
        if let bzz::realtime::supervisor::SupervisorEvent::Session(SessionEvent::Event {
            event,
            ..
        }) = event
            && event.kind.as_u16() == 20_002
        {
            typing.push(event);
        }
    }
    assert!(
        !typing.is_empty(),
        "the relay delivered the agent's typing indicator"
    );
    assert_eq!(typing[0].pubkey.to_hex(), agent_hex);
    assert!(
        bzz::agents::typing::parse(&typing[0], channel, typing[0].created_at.as_secs()).is_some(),
        "{:?}",
        typing[0].tags
    );
    // The reply is the agent's, in the mention's thread, with the owner's tag.
    let stored = by_agent(channel)
        .await
        .into_iter()
        .find(|event| event.content == "plain answer")
        .unwrap();
    let auth = stored
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
        .expect("NIP-OA tag");
    assert_eq!(
        buzz_sdk::nip_oa::verify_auth_tag(
            &serde_json::to_string(auth.as_slice()).unwrap(),
            &stored.pubkey
        )
        .unwrap(),
        owner.public_key()
    );
    assert_eq!(
        bzz::protocol::events::thread_coordinates(&stored).0,
        Some(plain.id.to_hex())
    );

    // Killed before the publish: handed over again, answered once.
    let before = mention("kill before the publish").await;
    assert_eq!(
        exactly_one(before, "killed before").await,
        "answered after a kill before the publish"
    );
    // Killed during the publish: the harness sends and dies without waiting.
    let during = mention("kill during the publish").await;
    let answer = exactly_one(during, "killed during").await;
    assert!(
        [
            "answered while being killed",
            "answered again after a kill during the publish"
        ]
        .contains(&answer.as_str()),
        "{answer}"
    );
    // Killed after the publish: nothing is handed over or published again.
    let after = mention("kill after the publish").await;
    assert_eq!(
        exactly_one(after, "killed after").await,
        "answered, then killed"
    );
    // A harness that sends twice.
    let twice = mention("send twice please").await;
    assert_eq!(exactly_one(twice, "sent twice").await, "the first wording");
    assert!(
        script.starts().len() >= 4,
        "the harness died three times: {:?}",
        script.starts()
    );

    // The owner's DM needs no mention and is answered flat; someone else's
    // mention in the channel is not answered at all (owner only).
    let in_dm = message(
        owner.clone(),
        owner_supervisor.clone(),
        dm,
        "dm question".into(),
        false,
    )
    .await;
    let ignored = message(
        stranger.clone(),
        stranger_supervisor.clone(),
        channel,
        "plain question from a stranger".into(),
        true,
    )
    .await;
    let deadline = tokio::time::Instant::now() + WAIT;
    while by_agent(dm).await.is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "no reply in the DM");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    let in_the_dm = by_agent(dm).await;
    assert_eq!(in_the_dm.len(), 1);
    assert_eq!(in_the_dm[0].content, "dm answer");
    assert!(bzz::protocol::events::event_references(&in_the_dm[0]).is_empty());
    let _ = in_dm;
    assert!(
        replies(&ignored).await.is_empty(),
        "a stranger does not wake an owner-only agent"
    );
    let unanswered = store
        .call({
            let (agent, id) = (agent.id, ignored.id.to_hex());
            move |store| store.agent_inbox_get(agent, &id)
        })
        .await
        .unwrap();
    assert!(unanswered.is_none());
    assert_eq!(
        by_agent(channel).await.len(),
        5,
        "five mentions, five replies"
    );
    // The relay's own log (written by scripts/test-relay.sh): the agent's
    // intake subscribed a few times, not twice a second. A stored membership
    // notice comes back with every subscription and is acted on once.
    if let Ok(log) = std::fs::read_to_string("/tmp/dawn-relay-run.log") {
        let subscriptions = log
            .lines()
            .filter(|line| line.contains(&agent_hex) && line.contains("\"event\":\"req\""))
            .count();
        assert!(
            (1..=4).contains(&subscriptions),
            "the agent sent {subscriptions} REQ frames"
        );
    }
    running.stop().await;
    for pid in script.starts() {
        assert!(!running_pid(pid), "no harness outlives the runtime");
    }

    // ---- a crash of bzz around the publish --------------------------------
    // What a bzz that died leaves in its ledger: a reply stored and already
    // on the relay but not recorded as published, and one stored and never
    // sent. The next start publishes both again, unchanged.
    let agent_signer = custody::unlock(&paths.keys_dir(), &owner, &agent)
        .await
        .unwrap();
    let agent_supervisor = SupervisorHandle::spawn(ws.clone(), agent_signer.clone());
    let auth_tag = broker_relay::auth_tag(&agent.auth_tag).unwrap();
    let mut stored_replies = Vec::new();
    for (text, already_on_the_relay) in [
        ("stored and sent before the crash", true),
        ("stored before the crash, never sent", false),
    ] {
        let request = mention(&format!("asked: {text}")).await;
        let thread = buzz_sdk::ThreadRef {
            root_event_id: request.id,
            parent_event_id: request.id,
        };
        let reply = agent_signer
            .sign(
                buzz_sdk::build_message(channel, text, Some(&thread), &[], false, &[], &[])
                    .unwrap()
                    .tag(auth_tag.clone()),
            )
            .await
            .unwrap();
        if already_on_the_relay {
            let ack = agent_supervisor.publish(reply.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
        }
        store
            .call({
                let new = NewRequest {
                    agent_id: agent.id,
                    request_id: request.id.to_hex(),
                    community_id: community,
                    channel_id: channel,
                    place: format!("{channel}/{}", request.id.to_hex()),
                    author: request.pubkey.to_hex(),
                    created_at: request.created_at.as_secs(),
                    event_json: request.as_json(),
                };
                let (reply_id, reply_json) = (reply.id.to_hex(), reply.as_json());
                move |store| {
                    store.agent_inbox_offer(&new)?;
                    store.agent_inbox_delivered(new.agent_id, &new.request_id)?;
                    store.agent_reply_claim(
                        new.agent_id,
                        &new.request_id,
                        &reply_id,
                        &reply_json,
                        ReplyOrigin::Send,
                        None,
                    )
                }
            })
            .await
            .unwrap();
        stored_replies.push((request, reply));
    }
    agent_supervisor.shutdown().await;
    agent_signer.lock().await;
    let starts_before = script.starts().len();
    let running = serve(agent.clone()).await;
    for (request, reply) in &stored_replies {
        let held = owner_http
            .query_raw(&[json!({"ids": [reply.id.to_hex()], "limit": 2})])
            .await
            .unwrap();
        assert_eq!(held.len(), 1, "the relay holds the stored reply once");
        assert_eq!(held[0].as_json(), reply.as_json(), "unchanged");
        let ledger = store
            .call({
                let (agent, id) = (agent.id, request.id.to_hex());
                move |store| store.agent_reply_get(agent, &id)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            ledger.published,
            "recorded as published after the relay took it"
        );
    }
    // The two requests reach the intake again (they are newer than its
    // cursor): they are known, so nothing answers them a second time.
    tokio::time::sleep(Duration::from_secs(3)).await;
    for (request, reply) in &stored_replies {
        assert_eq!(replies(request).await, std::slice::from_ref(&reply.content));
    }
    assert_eq!(
        script.starts().len(),
        starts_before,
        "recovery started no harness"
    );
    running.stop().await;

    // ---- without the TUI: `bzz agent run`, and bzz itself killed ----------
    let script_b = Script::new(&dirs.join("harness-b"));
    let live = script_b.marker("live");
    script_b.write(
        json!({"own_host": true, "ignore_eof": true}),
        json!([
            {"match": "agent run question", "steps": [send("answered by bzz agent run")]},
            {"match": "bzz dies", "steps": [{"touch": live}, {"stall": true, "deaf": true}]},
            {"match": "bzz dies", "resume": true, "steps": [send("answered after bzz itself was killed")]},
        ]),
    );
    agent.local.as_mut().unwrap().command = script_b.command();
    config.hosted_agents[0] = agent.clone();
    config.save(&paths).unwrap();
    let output = dirs.join("agent-run.log");
    let said = |text: &str| {
        std::fs::read_to_string(&output)
            .unwrap_or_default()
            .contains(text)
    };
    let mut first_bzz = bzz_on_profile(dirs, &["agent", "run"], Some(&output))
        .spawn()
        .unwrap();
    until("bzz agent run to be running", WAIT, || {
        said("running in m1").then_some(())
    })
    .await;
    // One status line per agent, printed when its state changes.
    until("the agent's status line", WAIT, || {
        said("Scout · pi-durable · idle").then_some(())
    })
    .await;
    let asked = mention("agent run question").await;
    assert_eq!(
        exactly_one(asked, "agent run").await,
        "answered by bzz agent run"
    );

    let doomed = mention("bzz dies now").await;
    until("the turn to be live", WAIT, || live.exists().then_some(())).await;
    let harness_pid = *script_b.starts().last().unwrap();
    let record = process::read_record(&process::run_dir(&paths.data_dir), agent.id).unwrap();
    assert_eq!(
        (record.process.pid, record.owner.pid),
        (harness_pid, first_bzz.id())
    );
    kill(first_bzz.id());
    first_bzz.wait().unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        running_pid(harness_pid),
        "the harness outlived the bzz that was killed"
    );
    assert!(replies(&doomed).await.is_empty());

    let output = dirs.join("agent-run-2.log");
    let said = |text: &str| {
        std::fs::read_to_string(&output)
            .unwrap_or_default()
            .contains(text)
    };
    let mut second_bzz = bzz_on_profile(dirs, &["agent", "run"], Some(&output))
        .spawn()
        .unwrap();
    let swept = format!("stopped a harness left by an earlier bzz (process {harness_pid})");
    until("the leftover to be swept", WAIT, || {
        said(&swept).then_some(())
    })
    .await;
    assert!(!running_pid(harness_pid), "stopped before anything else");
    assert_eq!(
        exactly_one(doomed, "bzz killed").await,
        "answered after bzz itself was killed"
    );
    // Interrupted, bzz stops its harness (deaf to its closed input: TERM).
    let stop = Command::new("kill")
        .args(["-TERM", &second_bzz.id().to_string()])
        .status()
        .unwrap();
    assert!(stop.success());
    assert!(
        second_bzz.wait().unwrap().success(),
        "{}",
        std::fs::read_to_string(&output).unwrap()
    );
    for pid in script_b.starts() {
        assert!(!running_pid(pid), "no harness outlives bzz agent run");
    }
    assert!(process::read_record(&process::run_dir(&paths.data_dir), agent.id).is_none());

    // ---- rule 10: nothing a harness saw holds a key -----------------------
    let owner_keys = manager.unlock(&identity, Some(&secret)).unwrap();
    let sealed =
        std::fs::read_to_string(paths.keys_dir().join(format!("agent-{}.nip44", agent.id)))
            .unwrap();
    let agent_secret = owner.decrypt_self(sealed).await.unwrap();
    let agent_keys = Keys::parse(&agent_secret).unwrap();
    use nostr::ToBech32 as _;
    let secrets = [
        ("agent key (hex)", agent_keys.secret_key().to_secret_hex()),
        (
            "agent key (nsec)",
            agent_keys.secret_key().to_bech32().unwrap(),
        ),
        ("owner key (hex)", owner_keys.secret_key().to_secret_hex()),
        (
            "owner key (nsec)",
            owner_keys.secret_key().to_bech32().unwrap(),
        ),
        ("owner passphrase", passphrase.to_owned()),
        ("relay address", "localhost:3030".to_owned()),
    ];
    let folder = process::agent_dir(&paths.data_dir, agent.id);
    let mut seen = vec![
        std::fs::read_to_string(script.log_path()).unwrap(),
        std::fs::read_to_string(script_b.log_path()).unwrap(),
    ];
    for entry in std::fs::read_dir(&folder).unwrap().flatten() {
        if entry.path().is_file() {
            seen.push(String::from_utf8_lossy(&std::fs::read(entry.path()).unwrap()).into_owned());
        }
    }
    assert!(seen.len() >= 3 && seen[0].contains("\"start\"") && seen[1].contains("\"env\""));
    for text in &seen {
        for (what, secret) in &secrets {
            assert!(
                !secret.is_empty() && !text.contains(secret.as_str()),
                "{what} reached a harness"
            );
        }
    }
    owner_supervisor.shutdown().await;
    stranger_supervisor.shutdown().await;
    owner.lock().await;
    stranger.lock().await;
}

/// v0.15 M6 exit gate, on the pinned relay, with **the owner's own installed
/// programs and sign-ins**: for each of Grok, Codex and Claude Code, an
/// agent made by `bzz agent new` answers a mention exactly once; a write
/// outside its folders does not happen, and one inside a folder granted for
/// writing does; after bzz and the program have stopped and started again,
/// the next message in the thread is answered from the program's own saved
/// session; and a version outside the tested range is refused before a key
/// is made.
///
/// It runs only when `BZZ_REAL_HARNESSES` names the harnesses to try
/// (`grok,codex,claude-code`): it spends three short prompts of each on the
/// cheapest model that program offers. It is never part of a default run or
/// of CI, which has no sign-in. With `BZZ_AGENT_RECORD` it also writes the
/// transcripts that `tests/agent_coding_test.rs` replays.
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh; runs only with BZZ_REAL_HARNESSES"]
async fn real_relay_coding_harnesses_answer_once_remember_and_stay_in_their_folders() {
    use bzz::{
        agents::{
            manage,
            runtime::{Limits, coding, process, service},
        },
        auth::IdentityManager,
        config::Harness,
        diagnostics::DiagnosticHandle,
        paths::Paths,
    };
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt as _;

    let wanted = std::env::var("BZZ_REAL_HARNESSES").unwrap_or_default();
    if wanted.trim().is_empty() {
        // Written to the terminal itself: the test harness hides what a
        // passing test prints, and a skip must be seen.
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED real_relay_coding_harnesses_answer_once_remember_and_stay_in_their_folders: it uses your own installed grok, codex and claude with your sign-ins, so it runs only when asked: BZZ_REAL_HARNESSES=grok,codex,claude-code"
        );
        return;
    }
    // A reply of a real model takes seconds, sometimes a minute. A turn
    // that runs away is stopped by bzz well before the test gives up: it
    // is somebody's own account that pays for it.
    const WAIT: Duration = Duration::from_secs(200);
    const TURN: Duration = Duration::from_secs(150);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    // Where the transcripts go: where the caller says, or with the test.
    let kept = std::env::var("BZZ_AGENT_RECORD")
        .ok()
        .filter(|folder| !folder.trim().is_empty())
        .map(std::path::PathBuf::from);

    for name in wanted
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let harness = manage::parse_harness(name).unwrap();
        let spec = coding::spec(harness).expect("a coding harness");
        // The cheapest model each program offered when this was written.
        let (model, thinking) = match harness {
            Harness::Grok => ("grok-4.5", Some("low")),
            Harness::Codex => ("gpt-5.6-luna", Some("low")),
            _ => ("haiku", None),
        };
        eprintln!("--- {name}: {} with {model}", spec.title);
        let temporary = tempfile::TempDir::new().unwrap();
        // The real place: a program names a folder as it really is.
        let dirs = std::fs::canonicalize(temporary.path()).unwrap();
        let dirs = dirs.as_path();
        let passphrase = "m6 disposable passphrase";
        std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
        let records = kept
            .clone()
            .map_or_else(|| dirs.join("transcripts"), |folder| folder.join(name));
        let said = |args: &[&str], variables: &[(&str, String)]| {
            let mut command = bzz_on_profile(dirs, args, None);
            command.envs(variables.iter().map(|(name, value)| (*name, value.clone())));
            let output = command.output().unwrap();
            (
                output.status.success(),
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            )
        };
        let ok = |args: &[&str]| {
            let (success, text) = said(args, &[]);
            assert!(success, "bzz {args:?}: {text}");
            text
        };
        ok(&[
            "identity",
            "new",
            "--label",
            "owner",
            "--backend",
            "encrypted-file",
        ]);
        let paths = Paths {
            config_dir: dirs.join("c"),
            data_dir: dirs.join("d"),
            cache_dir: dirs.join("k"),
        };
        let mut config = Config::load(&paths).unwrap();
        let identity = config.identities[0].clone();
        config
            .add_community("m6".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        config.save(&paths).unwrap();
        let _ = seed_member(&source, &identity.pubkey);
        tokio::time::sleep(Duration::from_secs(2)).await;
        let on_this_machine = || {
            std::fs::read_dir(paths.keys_dir())
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with("agent-"))
                        .count()
                })
                .unwrap_or(0)
        };

        // ---- doctor: the program, its version, its adapter; no session ----
        let report = ok(&["agent", "doctor"]);
        let line = report
            .lines()
            .find(|line| line.starts_with(&format!("{}:", spec.command)))
            .unwrap_or_else(|| panic!("no line for {} in:\n{report}", spec.command));
        assert!(line.contains(": ok)"), "{line}");
        assert!(
            report.contains(&format!("made with `{}`", spec.sign_in)),
            "{report}"
        );

        // ---- a version outside the range: refused before a key is made ----
        let real = coding::find_program(spec, |name: &str| std::env::var(name).ok())
            .await
            .unwrap();
        let shim = dirs.join(format!("{}-of-another-version", spec.command));
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{} 99.0.0'; exit 0; fi\nexec '{}' \"$@\"\n",
                spec.command,
                real.path.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (created, text) = said(
            &["agent", "new", "Old", "--harness", name, "--yes"],
            &[(spec.variable, shim.display().to_string())],
        );
        assert!(!created, "{text}");
        for stated in [
            "99.0.0",
            "outside the versions bzz knows to work",
            &real.version_text(),
        ] {
            assert!(text.contains(stated), "`{stated}` in: {text}");
        }
        assert_eq!(on_this_machine(), 0, "no key was made");

        // ---- bzz agent new ------------------------------------------------
        let work = dirs.join("work");
        // Not under the machine's temporary folders: Grok's and Codex's own
        // sandboxes let a program write there, whatever bzz grants.
        let outside = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("m6-not-granted-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let folder = format!("work={}:rw", work.display());
        let mut arguments = vec![
            "agent",
            "new",
            "Scout",
            "--harness",
            name,
            "--model",
            model,
            "--instructions",
            "Answer in one short sentence.",
            "--folder",
            folder.as_str(),
            "--yes",
        ];
        if let Some(level) = thinking {
            arguments.extend(["--thinking", level]);
        }
        let text = ok(&arguments);
        for stated in [
            format!("{} {} at", spec.title, real.version_text()),
            "created in m6".to_owned(),
            format!("capability record: {}", manage::harness_label(harness)),
            bzz::agents::capability::CODING_HARNESS_RUNS.to_owned(),
        ] {
            assert!(text.contains(&stated), "`{stated}` in:\n{text}");
        }
        // D18: the short warning, where the program has no sandbox of its own.
        assert_eq!(
            text.contains(bzz::agents::capability::NO_SANDBOX_WARNING),
            harness == Harness::ClaudeCode,
            "{text}"
        );
        assert_eq!(on_this_machine(), 1);
        let config = Config::load(&paths).unwrap();
        let agent = config.hosted_agents[0].clone();
        let agent_hex = agent.pubkey.clone();
        assert!(
            agent.local.as_ref().unwrap().command.is_empty(),
            "its own launch"
        );

        // ---- the owner, a channel, the agent in it ------------------------
        let secret = secrecy::SecretString::from(passphrase.to_owned());
        let owner = IdentityManager::new(&paths)
            .signer(&identity, Some(&secret))
            .unwrap();
        let ws = url::Url::parse("ws://localhost:3030/").unwrap();
        let owner_supervisor = SupervisorHandle::spawn(ws.clone(), owner.clone());
        let owner_http = HttpClient::new(
            url::Url::parse("http://localhost:3030/").unwrap(),
            owner.clone(),
        )
        .unwrap();
        let publish = |builder: EventBuilder| {
            let (supervisor, owner) = (owner_supervisor.clone(), owner.clone());
            async move {
                let event = owner.sign(builder).await.unwrap();
                let ack = supervisor.publish(event.clone()).await.unwrap();
                assert!(ack.accepted, "{}", ack.message);
                event
            }
        };
        let channel = uuid::Uuid::new_v4();
        publish(
            buzz_sdk::build_create_channel(
                channel,
                &format!("m6-{}", &channel.simple().to_string()[..8]),
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await;
        publish(buzz_sdk::build_add_member(channel, &agent_hex, None).unwrap()).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        // A message of the owner's: a mention, or not; in a thread, or not.
        let message = |text: String, mention: bool, thread: Option<nostr::Event>| {
            let (agent, publish) = (agent_hex.clone(), &publish);
            async move {
                let mentions = if mention {
                    vec![agent.as_str()]
                } else {
                    Vec::new()
                };
                let thread = thread.map(|root| buzz_sdk::ThreadRef {
                    root_event_id: root.id,
                    parent_event_id: root.id,
                });
                publish(
                    buzz_sdk::build_message(
                        channel,
                        &text,
                        thread.as_ref(),
                        &mentions,
                        false,
                        &[],
                        &[],
                    )
                    .unwrap(),
                )
                .await
            }
        };
        // The agent's messages that answer `request`, as the relay holds them.
        let replies = |request: &nostr::Event| {
            let (http, agent, id) = (owner_http.clone(), agent_hex.clone(), request.id.to_hex());
            async move {
                http.query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent], "limit": 500})])
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        bzz::protocol::events::thread_coordinates(event).1.as_deref() == Some(&id)
                    })
                    .map(|event| event.content)
                    .collect::<Vec<_>>()
            }
        };
        // Waits for the first reply, then for anything that would follow it.
        let exactly_one = |request: nostr::Event, what: &'static str| {
            let replies = &replies;
            async move {
                let deadline = tokio::time::Instant::now() + WAIT;
                while replies(&request).await.is_empty() {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "{name}, {what}: no reply"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                let all = replies(&request).await;
                assert_eq!(all.len(), 1, "{name}, {what}: {all:?}");
                eprintln!("{name}, {what}: {:?}", all[0]);
                all.into_iter().next().unwrap()
            }
        };

        // ---- the runtime in this process, as the TUI runs it --------------
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let store = StoreHandle::spawn(store).unwrap();
        let environment: bzz::agents::runtime::agent::Environment = {
            let records = records.display().to_string();
            std::sync::Arc::new(move |name: &str| {
                if name == "BZZ_AGENT_RECORD" {
                    Some(records.clone())
                } else {
                    std::env::var(name).ok()
                }
            })
        };
        let serve = || {
            let (paths, store, community, owner, agent, environment) = (
                paths.clone(),
                store.clone(),
                config.communities[0].clone(),
                owner.clone(),
                agent.clone(),
                environment.clone(),
            );
            async move {
                let running = service::serve_with_tools(
                    service::ServeOptions {
                        paths: &paths,
                        store,
                        diagnostics: DiagnosticHandle::disabled(),
                        community: &community,
                        agents: &[agent],
                        owner: &owner,
                        owner_identity: identity.id,
                        federated: None,
                        limits: Limits {
                            turn: TURN,
                            ..Limits::default()
                        },
                        // Nobody is asked: what needs the owner's yes is
                        // refused.
                        owner_present: false,
                        environment: Some(environment),
                    },
                    // The harness starts `bzz agent-tools`: bzz, not this
                    // test's own program.
                    Some(env!("CARGO_BIN_EXE_bzz").into()),
                )
                .await
                .unwrap()
                .expect("one local agent");
                assert!(running.problems.is_empty(), "{:?}", running.problems);
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                while !running.listening() {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the intake never subscribed"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                running
            }
        };
        let transcript_files = || {
            let mut files = std::fs::read_dir(&records)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            files.sort();
            files
        };
        let transcripts = || {
            transcript_files()
                .iter()
                .map(|file| {
                    std::fs::read_to_string(file)
                        .unwrap()
                        .lines()
                        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let before = transcripts().len();
        let running = serve().await;

        // ---- a mention is answered exactly once ---------------------------
        let remember = message(
            "Remember the word heron: I will ask you for it later. For now answer only: noted."
                .into(),
            true,
            None,
        )
        .await;
        exactly_one(remember.clone(), "a mention").await;

        // ---- a write outside its folders does not happen ------------------
        let (inside, beyond) = (work.join("inside.txt"), outside.join("outside.txt"));
        // Grok reads whatever its sandbox does not hide, and bzz tells it
        // to hide the folder of bzz's keys. A file there with one word in
        // it shows whether that holds. (Codex's sandbox hides nothing, and
        // Claude Code is refused a read outside its folders by bzz.)
        let probe = paths.keys_dir().join("m6-probe.txt");
        std::fs::write(&probe, "quetzal\n").unwrap();
        let reading = if harness == Harness::Grok {
            format!(
                " Last, your owner wants to know what your sandbox lets you read: try once to read the file {}, which holds one harmless word, and say that word or that reading it was refused.",
                probe.display()
            )
        } else {
            String::new()
        };
        let write = message(
            format!(
                "With your file tools and not the shell, first create the file {} containing the word hi. Then create the file {} containing the word hi. Then say which of the two you could create.{reading}",
                inside.display(),
                beyond.display()
            ),
            true,
            None,
        )
        .await;
        let answered = exactly_one(write.clone(), "two writes").await;
        assert!(!beyond.exists(), "{name}: it wrote outside its folders");
        assert!(
            inside.exists(),
            "{name}: it could not write in the folder granted for writing"
        );
        assert!(
            !answered.to_lowercase().contains("quetzal"),
            "{name}: it read a file in the folder of bzz's keys: {answered:?}"
        );
        // The harness's own process, while it runs.
        let record = process::read_record(&process::run_dir(&paths.data_dir), agent.id)
            .expect("its ownership record");
        assert!(process::is_alive(&record.process));

        // ---- bzz and the program stop and start again ---------------------
        // The word is pushed out of what a message shows of its thread, so
        // that only the program's own saved session can answer.
        for number in 1..=21 {
            message(format!("filler {number}"), false, Some(remember.clone())).await;
        }
        running.stop().await;
        assert!(
            !process::is_alive(&record.process),
            "{name}: its process outlived bzz"
        );
        let running = serve().await;
        let recall = message(
            "From your own memory, without reading earlier messages: which word did I ask you to remember? Answer with that word.".into(),
            true,
            Some(remember.clone()),
        )
        .await;
        let recalled = exactly_one(recall.clone(), "after a restart").await;
        assert!(
            recalled.to_lowercase().contains("heron"),
            "{name}: it did not remember: {recalled:?}"
        );
        // Still one reply to each of the earlier two.
        assert_eq!(replies(&remember).await.len(), 1);
        assert_eq!(replies(&write).await.len(), 1);
        running.stop().await;

        // ---- what went over the pipe --------------------------------------
        let runs = transcripts();
        assert_eq!(runs.len(), before + 2, "one transcript per start");
        let methods = |run: &[serde_json::Value]| {
            run.iter()
                .filter_map(|entry| entry["out"]["method"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let (first, second) = (&runs[before], &runs[before + 1]);
        // The saved session was loaded, not a new one opened; and the word
        // was not read back from the chat.
        let again = methods(second);
        assert!(
            again.contains(&"session/load".to_owned())
                && !again.contains(&"session/new".to_owned()),
            "{name}: {again:?}"
        );
        assert!(
            !second.iter().any(|entry| entry["tool"]["name"] == "read"),
            "{name}: it read the thread again, so its memory was not shown"
        );
        // The mode bzz sets, before the first prompt of each session.
        if let Some(mode) = coding::required_mode(harness) {
            for run in [first, second] {
                let sent = methods(run);
                let set = sent.iter().position(|method| method == "session/set_mode");
                let prompted = sent.iter().position(|method| method == "session/prompt");
                assert!(set.is_some() && set < prompted, "{name}: {sent:?}");
                assert!(
                    run.iter()
                        .any(|entry| entry["out"]["method"] == "session/set_mode"
                            && entry["out"]["params"]["modeId"] == mode),
                    "{name}"
                );
            }
        }
        // No key and no credential in what was recorded.
        let recorded = runs
            .iter()
            .flatten()
            .map(ToString::to_string)
            .collect::<String>();
        assert!(
            !recorded.contains("bzzt_") && !recorded.contains("nsec1"),
            "{name}"
        );
        // The folder that was not granted is a path of this checkout: in a
        // transcript that is kept it is written as a name.
        if kept.is_some() {
            let home = directories::BaseDirs::new()
                .map(|dirs| dirs.home_dir().display().to_string())
                .unwrap_or_default();
            let there = outside.display().to_string();
            for file in &transcript_files()[before..] {
                let text = std::fs::read_to_string(file)
                    .unwrap()
                    .replace(&there.replacen(&home, "<home>", 1), "<not-granted>")
                    .replace(&there, "<not-granted>");
                std::fs::write(file, text).unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&outside);
        owner_supervisor.shutdown().await;
        owner.lock().await;
    }
}

/// v0.15 M2 and M4 on the pinned relay, with the release's own commands and
/// bzz's real Pi Durable host on its stand-in model: `bzz agent new` makes
/// an agent Buzz can see, `bzz agent add` puts it in a channel as Buzz 1.0
/// does, a mention is answered once (also one written by Buzz's own CLI),
/// the host is killed in the middle of a turn and the turn ends once, the
/// conversation and the agent's disk survive a restart of bzz and of the
/// host, and `edit`, `pause`, `resume` and `delete` do what they say.
///
/// Needs Node.js 22.19 or newer; without it the test says so and skips
/// (`BZZ_TEST_REQUIRE_NODE=1`, set by the relay job in CI, makes that a
/// failure).
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_agent_new_to_delete_on_the_pi_durable_host() {
    use bzz::{
        agents::runtime::process, auth::IdentityManager, config::LocalAgentState, paths::Paths,
    };
    use serde_json::json;
    use support::{
        agent_harness::{kill, running as running_pid, until},
        pi_host,
    };

    let Some(node) = pi_host::node_or_skip("real_relay_agent_new_to_delete").await else {
        return;
    };
    const WAIT: Duration = Duration::from_secs(60);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let buzz_cli = std::env::var("BZZ_BUZZ_CLI").expect("relay wrapper builds the buzz CLI");
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = "c1 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    // What a test adds to bzz's environment: a stand-in for `node` that
    // switches on the host's scripted model, an empty folder as Pi's own,
    // and a cache for npm that is not the owner's.
    let pi_dir = dirs.join("pi");
    let variables = pi_host::variables(&pi_host::scripted_node(&dirs.join("bin"), &node), &pi_dir);
    let bzz = |args: &[&str], output: Option<&std::path::Path>| {
        let mut command = bzz_on_profile(dirs, args, output);
        command.envs(variables.iter().cloned());
        command
    };
    // Runs one command to its end: whether it succeeded, and what it said.
    let said = |args: &[&str]| {
        let output = bzz(args, None).output().unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    let ok = |args: &[&str]| {
        let (success, text) = said(args);
        assert!(success, "bzz {args:?}: {text}");
        text
    };

    ok(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    config
        .add_community("c1".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    let _ = seed_member(&source, &identity.pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let manager = IdentityManager::new(&paths);
    let owner = manager.signer(&identity, Some(&secret)).unwrap();
    let owner_hex = owner.public_key().to_hex();
    let ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let owner_supervisor = SupervisorHandle::spawn(ws.clone(), owner.clone());
    let owner_http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        owner.clone(),
    )
    .unwrap();
    let publish = |builder: EventBuilder| {
        let (supervisor, owner) = (owner_supervisor.clone(), owner.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    let query = |filter: serde_json::Value| {
        let http = owner_http.clone();
        async move { http.query_raw(&[filter]).await.unwrap() }
    };
    let on_this_machine = || {
        let count = |folder: std::path::PathBuf, prefix: &str| {
            std::fs::read_dir(folder)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                        .count()
                })
                .unwrap_or(0)
        };
        (
            count(paths.keys_dir(), "agent-"),
            std::fs::read_dir(paths.data_dir.join("agents"))
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| {
                            uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_ok()
                        })
                        .count()
                })
                .unwrap_or(0),
            Config::load(&paths).unwrap().hosted_agents.len(),
        )
    };

    // ---- the checks that fail before anything exists ----------------------
    let report = ok(&["agent", "doctor"]);
    assert!(
        report.contains("Node.js:") && report.contains("not installed yet"),
        "{report}"
    );
    assert!(
        !bzz::agents::runtime::install::is_installed(&paths.data_dir),
        "doctor installs nothing"
    );
    // What a harness does not have is refused where it would be set, on
    // any machine: a coding harness takes no list of hosts.
    let (created, text) = said(&[
        "agent",
        "new",
        "Grok",
        "--harness",
        "grok",
        "--allow-host",
        "github.com",
        "--yes",
    ]);
    assert!(
        !created && text.contains("a list of hosts is for a pi-durable agent"),
        "{text}"
    );
    // A model Pi does not have: refused by the host's own check, after the
    // install of first use and before any key is made.
    let (created, text) = said(&["agent", "new", "Nope", "--model", "nope/none", "--yes"]);
    assert!(
        !created && text.contains("cannot be used (provider_unavailable)"),
        "{text}"
    );
    assert!(bzz::agents::runtime::install::is_installed(&paths.data_dir));
    assert_eq!(on_this_machine(), (0, 0, 0));
    assert!(
        query(json!({"kinds": [30_177], "authors": [owner_hex], "limit": 10}))
            .await
            .is_empty(),
        "nothing was published"
    );

    // ---- bzz agent new ----------------------------------------------------
    let text = ok(&[
        "agent",
        "new",
        "Scout",
        "--model",
        "scripted/scripted",
        "--instructions",
        "Be brief.",
        "--allow-host",
        "github.com",
        "--yes",
    ]);
    assert!(
        text.contains("model scripted/scripted") && text.contains("created in c1"),
        "{text}"
    );
    assert!(
        text.contains("capability record: pi-durable")
            && text.contains("one final reply per mention: bzz")
            && text.contains("bzz agent add Scout <channel>"),
        "{text}"
    );
    assert_eq!(on_this_machine(), (1, 1, 1));
    let config = Config::load(&paths).unwrap();
    let agent = config.hosted_agents[0].clone();
    let agent_hex = agent.pubkey.clone();
    let local = agent.local.clone().expect("a local agent");
    assert_eq!(local.network, ["github.com"]);
    assert!(local.command.is_empty(), "the default launch");
    // What Buzz reads: the agent's profile, marked as a bot, with the
    // owner's tag; and the owner's record of it.
    let profiles = query(json!({"kinds": [0], "authors": [agent_hex], "limit": 5})).await;
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&profiles[0].content).unwrap(),
        json!({"name": "Scout", "bot": true})
    );
    let records =
        query(json!({"kinds": [30_177], "authors": [owner_hex], "#d": [agent_hex]})).await;
    assert_eq!(records.len(), 1);
    assert_eq!(
        bzz::protocol::events::first_tag(&records[0], "runtime").as_deref(),
        Some("bzz/pi-durable")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&records[0].content).unwrap(),
        json!({"parallelism": 1, "respond_to": "owner-only", "name": "Scout"})
    );
    let verified =
        bzz::agents::protocol::verify_public_agent(&profiles[0], None, Some(&records[0])).unwrap();
    assert_eq!(verified.owner_pubkey, owner_hex);
    let (again, text) = said(&["agent", "new", "scout", "--yes"]);
    assert!(!again && text.contains("exists"), "{text}");
    let listed = ok(&["agent", "list"]);
    assert!(
        listed.contains("Scout\tactive\tpi-durable\tscripted/scripted"),
        "{listed}"
    );
    let report = ok(&["agent", "doctor", "--models"]);
    assert!(
        report.contains("installed in") && report.contains("Scout · pi-durable · active · ready"),
        "{report}"
    );

    // ---- bzz agent add ----------------------------------------------------
    let channel = uuid::Uuid::new_v4();
    let channel_name = format!("c1-{}", &channel.simple().to_string()[..8]);
    publish(
        buzz_sdk::build_create_channel(
            channel,
            &channel_name,
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    let solo = uuid::Uuid::new_v4();
    publish(bzz::protocol::sidebar::solo_creation(solo, "Plans").unwrap()).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let role_in = |channel: uuid::Uuid| {
        let (list, agent) = (
            query(json!({"kinds": [39_002], "#d": [channel.to_string()], "limit": 2})),
            agent_hex.clone(),
        );
        async move {
            list.await.iter().find_map(|event| {
                event.tags.iter().find_map(|tag| {
                    let values = tag.as_slice();
                    (values.first().map(String::as_str) == Some("p")
                        && values.get(1) == Some(&agent))
                    // `["p", pubkey, relay, role]`.
                    .then(|| values.get(3).cloned().unwrap_or_default())
                })
            })
        }
    };
    assert_eq!(role_in(channel).await, None);
    // A plain member first, in a channel of its own: what Buzz 1.0 makes
    // of the agent a Solo chat is created with, and what bzz made of every
    // agent it added before the run against Buzz 1.0.1.
    let plain = uuid::Uuid::new_v4();
    publish(
        buzz_sdk::build_create_channel(
            plain,
            &format!("p1-{}", &plain.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    publish(buzz_sdk::build_add_member(plain, &agent_hex, None).unwrap()).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(role_in(plain).await.as_deref(), Some("member"));

    // bzz's own directory lists it now, while it has the `bot` role nowhere:
    // its owner's proof is what counts (D13). Until v0.14 it was not listed.
    assert_eq!(role_in(solo).await, None);
    assert_eq!(role_in(channel).await, None);
    let refreshed = ok(&["agents", "refresh"]);
    assert!(refreshed.contains("verified=1"), "{refreshed}");
    let directory: serde_json::Value =
        serde_json::from_str(&ok(&["agents", "list", "--json"])).unwrap();
    let listed = directory["agents"].as_array().unwrap();
    assert_eq!(listed.len(), 1, "{directory}");
    assert_eq!(
        (
            listed[0]["name"].as_str(),
            listed[0]["pubkey"].as_str(),
            listed[0]["owner_pubkey"].as_str(),
            listed[0]["eligibility"].as_str(),
            listed[0]["respond_to"].as_str(),
        ),
        (
            Some("Scout"),
            Some(agent_hex.as_str()),
            Some(owner_hex.as_str()),
            Some("eligible"),
            Some("owner-only"),
        ),
        "{directory}"
    );
    assert_eq!(
        listed[0]["channel_ids"],
        json!([plain.to_string()]),
        "the channel it is a plain member of: {directory}"
    );
    // It is the owner's own agent, and the directory says so: until the
    // run against Buzz 1.0 it was listed as a remote agent.
    assert_eq!(directory["own"][&agent_hex], "runs-here", "{directory}");
    let listing = ok(&["agents", "list"]);
    assert!(
        listing.contains("yours · run by this bzz") && !listing.contains("remote agents"),
        "{listing}"
    );
    let shown = ok(&["agents", "show", &agent_hex]);
    assert!(
        shown.contains("your own agent") && !shown.contains("not controlled by bzz"),
        "{shown}"
    );

    // `bzz agent add`, by the channel's name as the sidebar shows it: this
    // profile never synced that channel, so bzz asks the relay. The agent
    // gets the `bot` role, in a channel and in a Solo chat, as Buzz 1.0.1
    // gives it to an agent it adds to either.
    let text = ok(&["agent", "add", "Scout", &channel_name]);
    assert!(
        text.contains(&format!(
            "Scout added to #{channel_name} with the bot role, as Buzz adds an agent"
        )),
        "{text}"
    );
    assert_eq!(
        role_in(channel).await.as_deref(),
        Some("bot"),
        "the bot role, as Buzz 1.0.1 adds an agent to a channel that exists"
    );
    let text = ok(&["agent", "add", "Scout", &channel.to_string()]);
    assert!(text.contains("was already a member"), "{text}");
    let text = ok(&["agent", "add", "Scout", &solo.to_string()]);
    assert!(text.contains("with the bot role"), "{text}");
    assert_eq!(role_in(solo).await.as_deref(), Some("bot"));
    let (added, text) = said(&["agent", "add", "Scout", "no-such-channel"]);
    assert!(!added && text.contains("no channel is called"), "{text}");

    // ---- mentions ---------------------------------------------------------
    let message = |channel: uuid::Uuid, text: String, thread: Option<nostr::EventId>| {
        let agent = agent_hex.clone();
        let publish = &publish;
        async move {
            let thread = thread.map(|root| buzz_sdk::ThreadRef {
                root_event_id: root,
                parent_event_id: root,
            });
            publish(
                buzz_sdk::build_message(
                    channel,
                    &text,
                    thread.as_ref(),
                    &[agent.as_str()],
                    false,
                    &[],
                    &[],
                )
                .unwrap(),
            )
            .await
        }
    };
    let by_agent = |channel: uuid::Uuid| {
        query(
            json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 500}),
        )
    };
    let replies = |request: String| {
        let all = by_agent(channel);
        async move {
            all.await
                .into_iter()
                .filter(|event| bzz::protocol::events::event_references(event).contains(&request))
                .map(|event| event.content)
                .collect::<Vec<_>>()
        }
    };
    let exactly_one = |request: String, what: &'static str| {
        let replies = &replies;
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies(request.clone()).await.is_empty() {
                assert!(tokio::time::Instant::now() < deadline, "{what}: no reply");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let all = replies(request).await;
            assert_eq!(all.len(), 1, "{what}: {all:?}");
            all.into_iter().next().unwrap()
        }
    };
    // bzz's own bookkeeping for a request, read beside the running bzz.
    let database = rusqlite::Connection::open_with_flags(
        paths.database_file(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    );
    let turn = |request: &str| {
        database.as_ref().ok().and_then(|database| {
            database
                .query_row(
                    "SELECT state,attempts FROM agent_inbox WHERE request_id=?1",
                    [request],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
                )
                .ok()
        })
    };
    let start = |log: &std::path::Path| {
        let child = bzz(&["agent", "run"], Some(log)).spawn().unwrap();
        let log = log.to_path_buf();
        async move {
            until("bzz agent run to be running", WAIT, || {
                std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .contains("Scout · pi-durable · idle")
                    .then_some(())
            })
            .await;
            // Its intake subscribes a moment after the status line.
            tokio::time::sleep(Duration::from_secs(3)).await;
            child
        }
    };
    let stop = |mut child: std::process::Child, log: &std::path::Path| {
        let status = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            child.wait().unwrap().success(),
            "{}",
            std::fs::read_to_string(log).unwrap()
        );
    };
    let host_pid = || {
        process::read_record(&process::run_dir(&paths.data_dir), agent.id)
            .map(|record| record.process.pid)
    };

    let first_log = dirs.join("agent-run-1.log");
    let first_bzz = start(&first_log).await;
    assert!(host_pid().is_none(), "no process before a request");

    // A mention is answered once, by the real host.
    let hello = message(channel, "hello there".into(), None).await;
    assert_eq!(
        exactly_one(hello.id.to_hex(), "the first mention").await,
        "pong: this conversation holds 1 message(s) from people"
    );
    let stored = by_agent(channel).await.pop().unwrap();
    let auth = stored
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
        .expect("the owner's tag");
    assert_eq!(
        buzz_sdk::nip_oa::verify_auth_tag(
            &serde_json::to_string(auth.as_slice()).unwrap(),
            &stored.pubkey
        )
        .unwrap(),
        owner.public_key()
    );
    let first_pid = host_pid().expect("the host is running");
    // What runs is Node on the installed host and the agent's folder.
    #[cfg(target_os = "linux")]
    {
        let arguments = std::fs::read(format!("/proc/{first_pid}/cmdline")).unwrap();
        let arguments = String::from_utf8_lossy(&arguments);
        let host = bzz::agents::runtime::install::host_dir(&paths.data_dir).join("main.mjs");
        assert!(
            arguments.contains(&host.display().to_string())
                && arguments.contains(&agent.id.to_string()),
            "{arguments}"
        );
    }

    // A mention as Buzz's own CLI writes it, by the agent's name: Buzz
    // resolves the name from the profile bzz published.
    use nostr::nips::nip19::ToBech32 as _;
    let owner_keys = manager.unlock(&identity, Some(&secret)).unwrap();
    let sent = Command::new(&buzz_cli)
        .args([
            "messages",
            "send",
            "--channel",
            &channel.to_string(),
            "--content",
            "@Scout are you there?",
        ])
        .env("BUZZ_RELAY_URL", "http://localhost:3030")
        .env(
            "BUZZ_PRIVATE_KEY",
            owner_keys.secret_key().to_bech32().unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        sent.status.success(),
        "buzz messages send: {}",
        String::from_utf8_lossy(&sent.stderr)
    );
    let from_buzz = query(
        json!({"kinds": [9], "#h": [channel.to_string()], "authors": [owner_hex], "limit": 50}),
    )
    .await
    .into_iter()
    .find(|event| event.content.contains("are you there?"))
    .expect("the message Buzz's CLI sent");
    assert!(
        bzz::protocol::events::tag_values(&from_buzz, "p").contains(&agent_hex),
        "Buzz resolved the agent's name to a mention: {:?}",
        from_buzz.tags
    );
    assert_eq!(
        exactly_one(from_buzz.id.to_hex(), "a mention from Buzz's CLI").await,
        "pong: this conversation holds 1 message(s) from people"
    );

    // In a Solo chat the answer is flat, in the chat itself.
    message(solo, "anyone home?".into(), None).await;
    let deadline = tokio::time::Instant::now() + WAIT;
    while by_agent(solo).await.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no reply in the Solo chat"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    let in_solo = by_agent(solo).await;
    assert_eq!(in_solo.len(), 1);
    assert!(bzz::protocol::events::event_references(&in_solo[0]).is_empty());

    // A file on the agent's own disk, written through its own shell.
    let wrote = message(
        channel,
        r#"do: [["bash", {"command": "echo kept across restarts > /home/agent/note.txt && cat /home/agent/note.txt"}], ["report"]]"#.into(),
        None,
    )
    .await;
    assert!(
        exactly_one(wrote.id.to_hex(), "the file")
            .await
            .contains("kept across restarts")
    );
    let folder = process::agent_dir(&paths.data_dir, agent.id);
    assert!(folder.join("memory.sqlite").is_file() && folder.join("files.sqlite").is_file());
    assert!(
        !folder.join("note.txt").exists(),
        "on its disk, not on the machine"
    );

    // The host is killed in the middle of a turn: the turn ends once.
    let cut = message(
        channel,
        r#"do: [["sleep", 6000], ["buzz_send", {"text": "answered after the host was killed", "final": true}]]"#.into(),
        None,
    )
    .await;
    let cut_id = cut.id.to_hex();
    until("the turn to be running", WAIT, || {
        (turn(&cut_id).map(|(state, _)| state).as_deref() == Some("delivered")).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(replies(cut_id.clone()).await.is_empty());
    assert_eq!(host_pid(), Some(first_pid));
    kill(first_pid);
    assert_eq!(
        exactly_one(cut_id.clone(), "killed during the turn").await,
        "answered after the host was killed"
    );
    assert_eq!(
        turn(&cut_id),
        Some(("replied".to_owned(), 2)),
        "handed over again to a new host"
    );
    let second_pid = host_pid().expect("a new host");
    assert_ne!(second_pid, first_pid);

    // `agent show` while it runs: state, channels, last turns, the record.
    let shown = ok(&["agent", "show", "Scout"]);
    for expected in [
        "state:       active · its process is running",
        "harness:     pi-durable",
        "model:       scripted/scripted",
        "wakes for:   its owner only",
        "may reach:   github.com",
        "folders:     none of this machine",
        &format!("#{channel_name} (bot)"),
        "#Plans (bot)",
        "last turns:",
        "answered · attempt 2",
        "capability record: pi-durable",
        "its own disk:        yes",
    ] {
        assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
    }
    // bzz and the host both restart.
    stop(first_bzz, &first_log);
    assert!(!running_pid(second_pid), "no host outlives bzz agent run");
    assert!(host_pid().is_none());
    let second_log = dirs.join("agent-run-2.log");
    let second_bzz = start(&second_log).await;
    // The same thread remembers what was said before the restart.
    let again = message(channel, "and now?".into(), Some(hello.id)).await;
    assert_eq!(
        exactly_one(again.id.to_hex(), "after the restart").await,
        "pong: this conversation holds 2 message(s) from people",
        "the conversation survived a restart of bzz and of the host"
    );
    // The file is still on the agent's disk.
    let read = message(
        channel,
        r#"do: [["bash", {"command": "cat /home/agent/note.txt"}], ["report"]]"#.into(),
        None,
    )
    .await;
    assert!(
        exactly_one(read.id.to_hex(), "the file after the restart")
            .await
            .contains("kept across restarts")
    );
    assert_eq!(
        by_agent(channel).await.len(),
        6,
        "six mentions in the channel, six replies"
    );
    stop(second_bzz, &second_log);

    // ---- pause, resume, edit ----------------------------------------------
    ok(&["agent", "pause", "Scout"]);
    assert_eq!(
        Config::load(&paths).unwrap().hosted_agents[0]
            .local
            .as_ref()
            .unwrap()
            .state,
        LocalAgentState::Paused
    );
    assert!(ok(&["agent", "list"]).contains("Scout\tpaused"));
    let paused_log = dirs.join("agent-run-3.log");
    let paused_bzz = bzz(&["agent", "run"], Some(&paused_log)).spawn().unwrap();
    until("the paused agent's status line", WAIT, || {
        std::fs::read_to_string(&paused_log)
            .unwrap_or_default()
            .contains("Scout · pi-durable · paused")
            .then_some(())
    })
    .await;
    stop(paused_bzz, &paused_log);
    ok(&["agent", "resume", "Scout"]);
    assert!(Config::load(&paths).unwrap().hosted_agents[0].runs_locally());

    let text = ok(&[
        "agent",
        "edit",
        "Scout",
        "--name",
        "Ranger",
        "--wake",
        "anyone",
        "--picture",
        "https://example.org/ranger.png",
    ]);
    assert!(
        text.contains("published again: its profile and your record of it"),
        "{text}"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    let profiles = query(json!({"kinds": [0], "authors": [agent_hex], "limit": 5})).await;
    assert_eq!(profiles.len(), 1, "a profile replaces the one before");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&profiles[0].content).unwrap(),
        json!({"name": "Ranger", "bot": true, "picture": "https://example.org/ranger.png"})
    );
    let records =
        query(json!({"kinds": [30_177], "authors": [owner_hex], "#d": [agent_hex]})).await;
    assert_eq!(records.len(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&records[0].content).unwrap(),
        json!({
            "parallelism": 1, "respond_to": "anyone", "name": "Ranger",
            "picture": "https://example.org/ranger.png",
        })
    );
    // Instructions and hosts are bzz's own: nothing is published for them.
    let text = ok(&[
        "agent",
        "edit",
        "Ranger",
        "--no-hosts",
        "--instructions",
        "Be kind.",
    ]);
    assert!(!text.contains("published again"), "{text}");
    let (edited, text) = said(&["agent", "edit", "Ranger", "--model", "nope/none"]);
    assert!(!edited && text.contains("cannot be used"), "{text}");
    assert_eq!(
        Config::load(&paths).unwrap().hosted_agents[0]
            .local
            .as_ref()
            .unwrap()
            .model
            .as_ref()
            .map(|model| model.provider.as_str()),
        Some("scripted"),
        "a model that cannot be used is not saved"
    );

    // ---- rule 10: nothing the host saw or wrote holds a key ---------------
    let sealed =
        std::fs::read_to_string(paths.keys_dir().join(format!("agent-{}.nip44", agent.id)))
            .unwrap();
    let agent_keys = Keys::parse(&owner.decrypt_self(sealed).await.unwrap()).unwrap();
    let secrets = [
        ("agent key (hex)", agent_keys.secret_key().to_secret_hex()),
        (
            "agent key (nsec)",
            agent_keys.secret_key().to_bech32().unwrap(),
        ),
        ("owner key (hex)", owner_keys.secret_key().to_secret_hex()),
        (
            "owner key (nsec)",
            owner_keys.secret_key().to_bech32().unwrap(),
        ),
        ("owner passphrase", passphrase.to_owned()),
        ("relay address", "localhost:3030".to_owned()),
    ];
    let mut seen = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap().flatten() {
        if entry.path().is_file() {
            seen.push((
                entry.file_name().to_string_lossy().into_owned(),
                String::from_utf8_lossy(&std::fs::read(entry.path()).unwrap()).into_owned(),
            ));
        }
    }
    assert!(
        seen.iter().any(|(name, _)| name == "harness.log")
            && seen.iter().any(|(name, _)| name == "memory.sqlite"),
        "{:?}",
        seen.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    for (file, text) in &seen {
        for (what, secret) in &secrets {
            assert!(
                !secret.is_empty() && !text.contains(secret.as_str()),
                "{what} is in {file}"
            );
        }
    }

    // ---- bzz agent delete -------------------------------------------------
    let text = ok(&["agent", "delete", "Ranger", "--yes"]);
    for removed in [
        "agent Ranger deleted",
        "removed: your record of it on the relay",
        &format!("removed: its place in channel {channel}"),
        "removed: its settings",
        "removed: its key",
        "removed: its folder, with its memory and its disk",
        "row(s) in bzz's database",
    ] {
        assert!(text.contains(removed), "`{removed}` in:\n{text}");
    }
    assert!(!text.contains("could not remove"), "{text}");
    assert_eq!(on_this_machine(), (0, 0, 0));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        query(json!({"kinds": [30_177], "authors": [owner_hex], "#d": [agent_hex]}))
            .await
            .is_empty(),
        "the owner's record is gone from the relay, so Buzz lists no agent nobody runs"
    );
    assert_eq!(role_in(channel).await, None);
    assert_eq!(role_in(solo).await, None);
    assert_eq!(role_in(plain).await, None);
    drop(database);
    let rows = rusqlite::Connection::open(paths.database_file())
        .unwrap()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM agent_inbox)+(SELECT COUNT(*) FROM agent_replies)+(SELECT COUNT(*) FROM agent_cursors)",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(rows, 0, "its rows in bzz's database are gone");
    assert!(ok(&["agent", "list"]).contains("no local agents"));
    owner_supervisor.shutdown().await;
    owner.lock().await;
}

/// v0.15 M7 exit gate and the credentials line of M3, on the pinned relay,
/// with bzz's real Pi Durable host on its stand-in model. The agent is made
/// by `bzz agent new` with a folder granted for commands.
///
/// With the owner there (the runtime as the TUI runs it): a real command
/// waits for him, is shown as it will run, runs after his yes, and its
/// output reaches the chat; a refused one does not run. A clone from a
/// private remote that demands a credential fails with a clear message
/// without one and works once `bzz agent credential set` has stored it.
/// Without him (`bzz agent run`): a command that needs his approval is
/// refused with a reason for the people in the chat, and one in a folder
/// allowed without asking runs. The secret is nowhere on disk in the clear.
///
/// Needs Node.js 22.19 or newer and a git with `http-backend`; without
/// them the test says so and skips (`BZZ_TEST_REQUIRE_NODE=1` makes that a
/// failure).
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_agent_runs_a_real_command_and_clones_a_private_remote() {
    use bzz::{
        agents::runtime::{Limits, approval::Answer, service},
        auth::IdentityManager,
        diagnostics::DiagnosticHandle,
        paths::Paths,
    };
    use serde_json::json;
    use std::io::Write as _;
    use support::{agent_harness::until, pi_host};

    let test = "real_relay_agent_runs_a_real_command";
    let Some(node) = pi_host::node_or_skip(test).await else {
        return;
    };
    const WAIT: Duration = Duration::from_secs(60);
    const USER: &str = "scout-bot";
    const TOKEN: &str = "ghp_NotARealTokenOnlyAMarkerForTheRelayTest000";
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let Some(remote) = pi_host::git_remote(test, &node, &dirs.join("remote"), USER, TOKEN) else {
        return;
    };
    let passphrase = "c4 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    let pi_dir = dirs.join("pi");
    let stand_in = pi_host::scripted_node(&dirs.join("bin"), &node);
    let variables = pi_host::variables(&stand_in, &pi_dir);
    let bzz = |args: &[&str], output: Option<&std::path::Path>| {
        let mut command = bzz_on_profile(dirs, args, output);
        command.envs(variables.iter().cloned());
        command
    };
    let said = |args: &[&str]| {
        let output = bzz(args, None).output().unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    let ok = |args: &[&str]| {
        let (success, text) = said(args);
        assert!(success, "bzz {args:?}: {text}");
        text
    };

    // ---- the profile, and an agent with hands -----------------------------
    ok(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    config
        .add_community("c4".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    let _ = seed_member(&source, &identity.pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let owner = IdentityManager::new(&paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    let owner_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        owner.clone(),
    );
    let owner_http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        owner.clone(),
    )
    .unwrap();
    let publish = |builder: EventBuilder| {
        let (supervisor, owner) = (owner_supervisor.clone(), owner.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    let (work, ops) = (dirs.join("work"), dirs.join("ops"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&ops).unwrap();
    let text = ok(&[
        "agent",
        "new",
        "Scout",
        "--model",
        "scripted/scripted",
        "--folder",
        &format!("work={}:exec", work.display()),
        "--folder",
        &format!("ops={}:exec-without-asking", ops.display()),
        "--allow-host",
        &remote.host,
        "--yes",
    ]);
    // The capability record printed at creation says where it has hands.
    for expected in [
        "created in c4",
        "real commands of this machine, as your user, started in /mnt/work, /mnt/ops",
        "In /mnt/work each command waits for your yes in the TUI.",
        "Warning: in /mnt/ops commands run WITHOUT asking you",
        "The folder is where it starts, not a wall",
        "Pi packages: on.",
    ] {
        assert!(text.contains(expected), "`{expected}` in:\n{text}");
    }
    let config = Config::load(&paths).unwrap();
    let agent = config.hosted_agents[0].clone();
    let agent_hex = agent.pubkey.clone();
    let shown = ok(&["agent", "show", "Scout"]);
    for expected in [
        &format!(
            "folder:      /mnt/work = {} (read-only; real commands, each asked of you first)",
            work.display()
        ),
        &format!(
            "folder:      /mnt/ops = {} (read-only; real commands, WITHOUT asking you)",
            ops.display()
        ),
        "git credentials: none stored",
        "its host: not started yet on this machine",
        "activity:    no command asked for and no credential asked for yet",
    ] {
        assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
    }
    let channel = uuid::Uuid::new_v4();
    let channel_name = format!("c4-{}", &channel.simple().to_string()[..8]);
    publish(
        buzz_sdk::build_create_channel(
            channel,
            &channel_name,
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    ok(&["agent", "add", "Scout", &channel_name]);

    let mention = |text: String| {
        let (agent, publish) = (agent_hex.clone(), &publish);
        async move {
            publish(
                buzz_sdk::build_message(channel, &text, None, &[agent.as_str()], false, &[], &[])
                    .unwrap(),
            )
            .await
        }
    };
    let run = |folder: &str, command: &str| {
        mention(format!(
            "do: {}",
            json!([["run", {"folder": folder, "command": command}], ["report"]])
        ))
    };
    // The agent's one reply to `request`, as the relay holds it.
    let reply_to = |request: nostr::Event, what: &'static str| {
        let (http, agent) = (owner_http.clone(), agent_hex.clone());
        async move {
            let id = request.id.to_hex();
            let replies = || {
                let (http, agent, id) = (http.clone(), agent.clone(), id.clone());
                async move {
                    http.query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent], "limit": 500})])
                        .await
                        .unwrap()
                        .into_iter()
                        .filter(|event| bzz::protocol::events::event_references(event).contains(&id))
                        .map(|event| event.content)
                        .collect::<Vec<_>>()
                }
            };
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies().await.is_empty() {
                assert!(tokio::time::Instant::now() < deadline, "{what}: no reply");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            let all = replies().await;
            assert_eq!(all.len(), 1, "{what}: {all:?}");
            all.into_iter().next().unwrap()
        }
    };

    // ---- with the owner there: the runtime as the TUI runs it -------------
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let running = service::serve(service::ServeOptions {
        paths: &paths,
        store,
        diagnostics: DiagnosticHandle::disabled(),
        community: &config.communities[0],
        agents: &config.hosted_agents,
        owner: &owner,
        owner_identity: identity.id,
        federated: None,
        limits: Limits {
            stop_grace: Duration::from_millis(500),
            ..Limits::default()
        },
        owner_present: true,
        environment: Some(std::sync::Arc::new(pi_host::environment(
            &stand_in, &pi_dir,
        ))),
    })
    .await
    .unwrap()
    .expect("one local agent");
    assert!(running.problems.is_empty(), "{:?}", running.problems);
    until("the agent's intake to subscribe", WAIT, || {
        running.listening().then_some(())
    })
    .await;
    let one_question = || {
        let host = running.host.clone();
        async move {
            until("a command to wait for the owner", WAIT, move || {
                let mut waiting = host.approvals();
                (waiting.len() == 1).then(|| waiting.remove(0))
            })
            .await
        }
    };

    // Allowed: it runs as the owner saw it, and its output reaches the chat.
    let command = "printf 'made by a real %s\\n' command > made.txt &&\n  cat made.txt && pwd";
    let request = run("work", command).await;
    let asked = one_question().await;
    assert_eq!(asked.command, command);
    assert_eq!(
        (asked.agent, asked.folder.as_str(), asked.channel),
        (agent.id, "work", channel)
    );
    assert_eq!(asked.requester, identity.pubkey);
    assert!(
        !work.join("made.txt").exists(),
        "nothing ran while it waited"
    );
    assert!(running.host.answer_approval(asked.id, Answer::Allow));
    let reply = reply_to(request, "an approved command").await;
    assert!(
        reply.contains("made by a real command") && reply.contains(&work.display().to_string()),
        "the output of the command reached the chat: {reply}"
    );
    assert_eq!(
        std::fs::read_to_string(work.join("made.txt")).unwrap(),
        "made by a real command\n"
    );

    // Refused: it does not run, and the agent says why in the chat.
    let request = run("work", "touch refused.txt").await;
    let asked = one_question().await;
    assert!(
        running
            .host
            .answer_approval(asked.id, Answer::Refuse(Some("not in this folder".into())))
    );
    let reply = reply_to(request, "a refused command").await;
    assert!(
        reply.contains("Your owner refused this command, and says: not in this folder"),
        "{reply}"
    );
    assert!(
        !work.join("refused.txt").exists(),
        "a refused command does not run"
    );

    // A private remote, with no credential stored: a clear message.
    let clone = |into: &str| {
        mention(format!(
            "do: {}",
            json!([
                ["bash", {"command": format!("{{ git clone {} {into} && cat {into}/notes.txt; }} 2>&1; echo \"exit $?\"", remote.origin)}],
                ["report"],
            ])
        ))
    };
    let request = clone("first").await;
    let reply = reply_to(request, "a clone without a credential").await;
    assert!(
        reply.contains(&format!(
            "No credential is stored for {host}. Your owner can store one with `bzz agent credential set Scout {host}`.",
            host = remote.host
        )) && reply.contains("exit 128"),
        "{reply}"
    );
    // The owner stores one, with the command, while the agent runs.
    let stored = {
        let mut child = bzz(&["agent", "credential", "set", "Scout", &remote.host], None)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{USER}\n{TOKEN}\n").as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    assert!(
        stored.status.success(),
        "{}",
        String::from_utf8_lossy(&stored.stderr)
    );
    let said_on_set = format!(
        "{}{}",
        String::from_utf8_lossy(&stored.stdout),
        String::from_utf8_lossy(&stored.stderr)
    );
    assert!(said_on_set.contains("sealed to your key"), "{said_on_set}");
    assert_eq!(
        ok(&["agent", "credential", "list", "Scout"]).trim(),
        remote.host
    );
    let request = clone("second").await;
    let reply = reply_to(request, "a clone with the stored credential").await;
    assert!(
        reply.contains("Cloned into")
            && reply.contains("alpha\nbeta\ngamma")
            && reply.contains("exit 0"),
        "{reply}"
    );
    running.stop().await;

    // ---- without the owner: `bzz agent run` -------------------------------
    let log = dirs.join("agent-run.log");
    let mut headless = bzz(&["agent", "run"], Some(&log)).spawn().unwrap();
    until("bzz agent run to be running", WAIT, || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("Scout · pi-durable · idle")
            .then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let request = run("work", "touch headless.txt").await;
    let reply = reply_to(request, "a command with nobody to ask").await;
    assert!(
        reply.contains("needs your owner's approval")
            && reply.contains("your owner must open bzz to approve the command"),
        "the people in the chat are told what is needed: {reply}"
    );
    assert!(!work.join("headless.txt").exists());
    let request = run("ops", "echo restarted > done.txt && cat done.txt").await;
    let reply = reply_to(request, "a command allowed without asking").await;
    assert!(reply.contains("restarted"), "{reply}");
    assert!(ops.join("done.txt").exists());

    // `agent show`, beside the running bzz: what the host reported, the
    // host a credential is stored for, and every question with its answer.
    let shown = ok(&["agent", "show", "Scout"]);
    for expected in [
        "git credentials stored for (the host only is shown):",
        &format!("  {}", remote.host),
        "its host, at its last start (",
        "real commands: the `run` tool, in /mnt/work, /mnt/ops",
        "Pi packages: none named in Pi's settings",
        "· command in /mnt/work · printf 'made by a real %s\\n' command > made.txt && (and 1 more line(s)) · asked of you",
        "· allowed by you",
        "· command in /mnt/work · touch refused.txt · refused by you: not in this folder",
        &format!("· git credential for {} · none is stored", remote.host),
        &format!("· git credential for {} · given", remote.host),
        "· command in /mnt/work · touch headless.txt · refused: nobody was at bzz to ask",
        "· command in /mnt/ops · echo restarted > done.txt && cat done.txt · allowed without asking",
    ] {
        assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
    }
    let report = ok(&["agent", "doctor"]);
    assert!(
        report.contains("real commands: the `run` tool, in /mnt/work, /mnt/ops")
            && report.contains(&format!("git credential stored for {}", remote.host)),
        "{report}"
    );
    let status = Command::new("kill")
        .args(["-TERM", &headless.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        headless.wait().unwrap().success(),
        "{}",
        std::fs::read_to_string(&log).unwrap()
    );

    // ---- the secret scan ---------------------------------------------------
    // Every file of the profile: the settings, bzz's database, the agent's
    // two SQLite files, its host's log, its activity, the sealed credential.
    let mut files = Vec::new();
    for folder in ["c", "d", "k"] {
        files.extend(pi_host::files_under(&dirs.join(folder)));
    }
    files.push(log.clone());
    for expected in [
        "config.toml",
        "bzz.db",
        "memory.sqlite",
        "files.sqlite",
        "harness.log",
        "activity.jsonl",
    ] {
        assert!(
            files.iter().any(|file| file.ends_with(expected)),
            "{expected} was searched"
        );
    }
    for file in &files {
        for (form, secret) in pi_host::secret_forms(USER, TOKEN) {
            assert!(
                !pi_host::file_holds(file, &secret),
                "{} holds {form}",
                file.display()
            );
        }
        assert!(!pi_host::file_holds(file, passphrase), "{}", file.display());
    }
    for (what, text) in [("agent show", &shown), ("credential set", &said_on_set)] {
        assert!(!text.contains(TOKEN) && !text.contains(USER), "{what}");
    }
    let agent_folder = bzz::agents::runtime::process::agent_dir(&paths.data_dir, agent.id);
    assert!(
        pi_host::file_holds(&agent_folder.join("files.sqlite"), "alpha\nbeta\ngamma"),
        "the search sees what is on the agent's disk"
    );

    // ---- delete ------------------------------------------------------------
    let text = ok(&["agent", "delete", "Scout", "--yes"]);
    assert!(
        text.contains("removed: its 1 stored git credential(s)"),
        "{text}"
    );
    let left = std::fs::read_dir(paths.keys_dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("agent-"))
        .collect::<Vec<_>>();
    assert!(left.is_empty(), "{left:?}");
    owner_supervisor.shutdown().await;
    owner.lock().await;
}

/// What the run against the real Buzz 1.0.1 found
/// (`runs/experiments/buzz1/agents/README.md`), on the pinned relay, with
/// bzz's real Pi Durable host on its stand-in model and every step of the
/// owner's the `bzz` binary or an event of the owner's own key:
///
/// - **presence**: a running agent is present on the relay by its own key,
///   and gone when it is paused or deleted; the owner's key says nothing;
/// - **the observed case**: the owner makes the agent read-only (kind 9000
///   with `["role","guest"]`) and mentions it. Nothing is handed to the
///   harness and nothing is signed, where five different replies were
///   signed for one mention and `agent show` said "answered";
/// - **the ledger when the relay refuses**: the relay refuses the agent's
///   reply while its records say the agent may write (a moderator's
///   timeout). One signed reply, the request fails at once with its kind,
///   and it is never published later;
/// - **a turn that outlives the agent's membership** publishes nothing;
/// - **`bzz agent pause` and `resume`** in another process reach the bzz
///   that runs the agent;
/// - **`bzz agent delete`** reaches it too: an agent it serves, and one
///   that is answering, are let go before their files go.
///
/// The claims are collected and asserted together at the end, so that one
/// run against the code as it was shows each of them failing.
#[cfg(unix)]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_agent_keeps_to_its_access_and_a_delete_or_a_pause_reaches_the_bzz_that_runs_it()
{
    use bzz::{auth::IdentityManager, paths::Paths, protocol::presence};
    use serde_json::json;
    use support::{agent_harness::until, pi_host};

    let Some(node) = pi_host::node_or_skip("real_relay_agent_keeps_to_its_access").await else {
        return;
    };
    const WAIT: Duration = Duration::from_secs(60);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = "c7 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    let pi_dir = dirs.join("pi");
    let variables = pi_host::variables(&pi_host::scripted_node(&dirs.join("bin"), &node), &pi_dir);
    let bzz = |args: &[&str], output: Option<&std::path::Path>| {
        let mut command = bzz_on_profile(dirs, args, output);
        command.envs(variables.iter().cloned());
        command
    };
    let said = |args: &[&str]| {
        let output = bzz(args, None).output().unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    let ok = |args: &[&str]| {
        let (success, text) = said(args);
        assert!(success, "bzz {args:?}: {text}");
        text
    };
    // What the run found, as this test sees it: each claim that does not
    // hold, in the order it was looked at.
    let faults = std::cell::RefCell::new(Vec::<String>::new());
    let claim = |holds: bool, what: String| {
        if !holds {
            faults.borrow_mut().push(what);
        }
    };

    ok(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    config
        .add_community("c7".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    // An owner of the relay: he may also put a member in time-out.
    let _ = seed_member(&source, &identity.pubkey);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let owner = IdentityManager::new(&paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    let owner_hex = owner.public_key().to_hex();
    let owner_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        owner.clone(),
    );
    let owner_http = HttpClient::new(
        url::Url::parse("http://localhost:3030/").unwrap(),
        owner.clone(),
    )
    .unwrap();
    let relay_key = bzz::protocol::http::relay_signing_pubkey(&owner_http.nip11().await.unwrap())
        .expect("the relay names its key")
        .to_owned();
    // The owner's own connection is between two attempts now and then: a
    // `bzz` command of the same owner has just used the relay too.
    let publish = |builder: EventBuilder| {
        let (supervisor, owner) = (owner_supervisor.clone(), owner.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let deadline = tokio::time::Instant::now() + WAIT;
            loop {
                match supervisor.publish(event.clone()).await {
                    Ok(ack) => {
                        assert!(
                            ack.accepted || ack.message.starts_with("duplicate:"),
                            "{}",
                            ack.message
                        );
                        return event;
                    }
                    Err(error) => {
                        assert!(
                            tokio::time::Instant::now() < deadline,
                            "the owner could not publish: {error}"
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }
    };
    let query = |filter: serde_json::Value| {
        let http = owner_http.clone();
        async move { http.query_raw(&[filter]).await.unwrap() }
    };
    let tagged = |kind: u16, tags: Vec<Vec<String>>| {
        EventBuilder::new(Kind::Custom(kind), "")
            .tags(tags.into_iter().map(|tag| nostr::Tag::parse(tag).unwrap()))
    };

    // ---- two agents, one channel ------------------------------------------
    for name in ["Scout", "Echo"] {
        ok(&[
            "agent",
            "new",
            name,
            "--model",
            "scripted/scripted",
            "--yes",
        ]);
    }
    let config = Config::load(&paths).unwrap();
    let agent_of = |name: &str| {
        config
            .hosted_agents
            .iter()
            .find(|agent| agent.label == name)
            .unwrap()
            .clone()
    };
    let (scout, echo) = (agent_of("Scout"), agent_of("Echo"));
    let lab = uuid::Uuid::new_v4();
    let lab_name = format!("c7-{}", &lab.simple().to_string()[..8]);
    publish(
        buzz_sdk::build_create_channel(
            lab,
            &lab_name,
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    // The role the relay records for an agent in the channel.
    let role_of = |agent: &str| {
        let (list, agent) = (
            query(json!({"kinds": [39_002], "#d": [lab.to_string()], "limit": 2})),
            agent.to_owned(),
        );
        async move {
            list.await.iter().find_map(|event| {
                event.tags.iter().find_map(|tag| {
                    let values = tag.as_slice();
                    (values.first().map(String::as_str) == Some("p")
                        && values.get(1) == Some(&agent))
                    .then(|| values.get(3).cloned().unwrap_or_default())
                })
            })
        }
    };
    let role_becomes = |agent: &str, role: Option<&'static str>| {
        let (agent, role_of) = (agent.to_owned(), &role_of);
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while role_of(&agent).await.as_deref() != role {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the relay never listed the role {role:?}"
                );
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };
    // The owner changes a member's role as Buzz 1.0 does: kind 9000 for a
    // member that is there, with the role.
    let set_role = |agent: &str, role: &'static str| {
        let event = tagged(
            9000,
            vec![
                vec!["h".into(), lab.to_string()],
                vec!["p".into(), agent.to_owned()],
                vec!["role".into(), role.into()],
            ],
        );
        let (publish, role_becomes, agent) = (&publish, &role_becomes, agent.to_owned());
        async move {
            publish(event).await;
            role_becomes(&agent, Some(role)).await;
        }
    };
    for name in ["Scout", "Echo"] {
        let text = ok(&["agent", "add", name, &lab_name]);
        claim(
            text.contains("with the bot role"),
            format!("role: `agent add` says it gives the bot role: {text}"),
        );
    }
    claim(
        role_of(&scout.pubkey).await.as_deref() == Some("bot"),
        format!(
            "role: `agent add` gives the bot role in a channel, as Buzz 1.0.1 does: {:?}",
            role_of(&scout.pubkey).await
        ),
    );

    // ---- what the test reads ----------------------------------------------
    let mention = |agent: &str, text: String| {
        let (agent, publish) = (agent.to_owned(), &publish);
        async move {
            publish(
                buzz_sdk::build_message(lab, &text, None, &[agent.as_str()], false, &[], &[])
                    .unwrap(),
            )
            .await
        }
    };
    // The replies the relay holds to a request, by whoever.
    let replies = |request: String| {
        let all = query(json!({"kinds": [9], "#h": [lab.to_string()], "limit": 500}));
        let owner_hex = owner_hex.clone();
        async move {
            all.await
                .into_iter()
                .filter(|event| {
                    event.pubkey.to_hex() != owner_hex
                        && bzz::protocol::events::event_references(event).contains(&request)
                })
                .map(|event| event.content)
                .collect::<Vec<_>>()
        }
    };
    let exactly_one = |request: String, what: &'static str| {
        let replies = &replies;
        async move {
            let deadline = tokio::time::Instant::now() + WAIT;
            while replies(request.clone()).await.is_empty() {
                assert!(tokio::time::Instant::now() < deadline, "{what}: no reply");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let all = replies(request).await;
            assert_eq!(all.len(), 1, "{what}: {all:?}");
            all.into_iter().next().unwrap()
        }
    };
    // bzz's own books, read beside the running bzz: the request's state,
    // its failure and its attempts; and the id of its stored reply.
    let database = paths.database_file();
    let turn = |request: &str| {
        rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|database| {
            database
                .query_row(
                    "SELECT state,COALESCE(failure,''),attempts FROM agent_inbox WHERE request_id=?1",
                    [request],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u32>(2)?,
                        ))
                    },
                )
                .ok()
        })
    };
    let stored_reply = |request: &str| {
        rusqlite::Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()
            .and_then(|database| {
                database
                    .query_row(
                        "SELECT reply_event_id FROM agent_replies WHERE request_id=?1",
                        [request],
                        |row| row.get::<_, String>(0),
                    )
                    .ok()
            })
    };
    // Watches one request for a while, as the tester did: every reply bzz
    // stored for it, and what `agent show` called it meanwhile.
    let watch = |request: String, for_seconds: u64| {
        let (stored_reply, said) = (&stored_reply, &said);
        async move {
            let short = request[..12].to_owned();
            let (mut stored, mut called) = (Vec::<String>::new(), Vec::<String>::new());
            let until = tokio::time::Instant::now() + Duration::from_secs(for_seconds);
            let mut looked = 0_u32;
            while tokio::time::Instant::now() < until {
                if let Some(id) = stored_reply(&request)
                    && !stored.contains(&id)
                {
                    stored.push(id);
                }
                // `agent show` is a whole bzz: asked now and then.
                if looked.is_multiple_of(10) {
                    let shown = said(&["agent", "show", "Scout"]).1;
                    if let Some(line) = shown.lines().find(|line| line.contains(&short)) {
                        let line = line.trim().to_owned();
                        if !called.contains(&line) {
                            called.push(line);
                        }
                    }
                }
                looked += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            (stored, called)
        }
    };
    // Whether the relay holds a key as present: its own snapshot.
    let present = |agent: &str| {
        let (snapshot, agent, relay_key) = (
            query(json!({"kinds": [presence::KIND_PRESENCE], "authors": [agent], "limit": 1})),
            agent.to_owned(),
            relay_key.clone(),
        );
        async move {
            snapshot
                .await
                .iter()
                .filter_map(|event| presence::observe(event, Some(&relay_key), |_| None))
                .find(|observed| observed.subjects.contains(&agent))
                .is_some_and(|observed| observed.presence != presence::Presence::Offline)
        }
    };
    let presence_becomes = |agent: &str, wanted: bool, what: &'static str| {
        let (agent, present, claim) = (agent.to_owned(), &present, &claim);
        async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            let mut now = present(&agent).await;
            while now != wanted && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(500)).await;
                now = present(&agent).await;
            }
            claim(
                now == wanted,
                format!(
                    "presence: {what}: the relay holds it as {}",
                    if now { "present" } else { "not present" }
                ),
            );
        }
    };
    let run_log = dirs.join("agent-run.log");
    let logged = |text: &str| {
        std::fs::read_to_string(&run_log)
            .unwrap_or_default()
            .contains(text)
    };
    let log_says = |text: &'static str, what: &'static str| {
        let (logged, claim) = (&logged, &claim);
        async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while !logged(text) && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            claim(
                logged(text),
                format!("{what}: `bzz agent run` never printed `{text}`"),
            );
        }
    };

    // ---- bzz agent run ----------------------------------------------------
    let mut running = bzz(&["agent", "run"], Some(&run_log)).spawn().unwrap();
    until("bzz agent run to be running", WAIT, || {
        (logged("Scout · pi-durable · idle") && logged("Echo · pi-durable · idle")).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // ---- presence ---------------------------------------------------------
    presence_becomes(&scout.pubkey, true, "a running agent says it is present").await;
    presence_becomes(&echo.pubkey, true, "a second running agent says so too").await;
    claim(
        !present(&owner_hex).await,
        "presence: the owner's own key signs nothing: the relay holds the owner as present".into(),
    );
    let hello = mention(&scout.pubkey, "hello there".into()).await;
    assert_eq!(
        exactly_one(hello.id.to_hex(), "the first mention").await,
        "pong: this conversation holds 1 message(s) from people"
    );

    // ---- the observed case: read-only, then a mention ---------------------
    set_role(&scout.pubkey, "guest").await;
    let viewing = mention(&scout.pubkey, "are you there?".into()).await;
    let viewing_id = viewing.id.to_hex();
    let (stored, called) = watch(viewing_id.clone(), 12).await;
    claim(
        stored.is_empty(),
        format!(
            "read-only: nothing is signed for a mention where the agent may only read: {} different stored replies {:?}",
            stored.len(),
            stored.iter().map(|id| &id[..8]).collect::<Vec<_>>()
        ),
    );
    claim(
        !called.iter().any(|line| line.contains("answered")),
        format!("read-only: `agent show` never says answered of it: {called:?}"),
    );
    let state = turn(&viewing_id);
    claim(
        state == Some(("failed".into(), "read-only".into(), 0)),
        format!("read-only: the request fails as read-only, and is never handed over: {state:?}"),
    );
    claim(
        replies(viewing_id.clone()).await.is_empty(),
        "read-only: the relay holds a reply to it".into(),
    );
    let shown = said(&["agent", "show", "Scout"]).1;
    claim(
        shown.contains(&format!("#{lab_name} (guest)"))
            && shown.contains("read-only: it cannot answer there"),
        format!("read-only: `agent show` says it is read-only in the channel:\n{shown}"),
    );
    claim(
        shown.contains("failed: read-only"),
        format!("read-only: `agent show` says why the mention was not answered:\n{shown}"),
    );
    // "Participating" again: the next mention is answered.
    set_role(&scout.pubkey, "member").await;
    let back = mention(&scout.pubkey, "and now?".into()).await;
    assert_eq!(
        exactly_one(back.id.to_hex(), "a writing member again").await,
        "pong: this conversation holds 1 message(s) from people"
    );
    claim(
        replies(viewing_id.clone()).await.is_empty(),
        "read-only: the mention of that time was answered afterwards".into(),
    );

    // ---- the relay refuses the reply --------------------------------------
    // A moderator's time-out: the relay's records still list the agent as
    // a member who may write, and the relay refuses what it writes.
    let until_then = (Timestamp::now().as_secs() + 600).to_string();
    publish(tagged(
        9042,
        vec![
            vec!["p".into(), scout.pubkey.clone()],
            vec!["expiration".into(), until_then],
        ],
    ))
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let refused = mention(&scout.pubkey, "can you write?".into()).await;
    let refused_id = refused.id.to_hex();
    let (stored, called) = watch(refused_id.clone(), 12).await;
    claim(
        stored.len() == 1,
        format!(
            "refused: one signed reply for one mention, whatever the relay answers: {} different stored replies {:?}",
            stored.len(),
            stored.iter().map(|id| &id[..8]).collect::<Vec<_>>()
        ),
    );
    claim(
        !called.iter().any(|line| line.contains("answered")),
        format!(
            "refused: `agent show` never says answered of a reply the relay refused: {called:?}"
        ),
    );
    let state = turn(&refused_id);
    claim(
        state == Some(("failed".into(), "reply-refused".into(), 1)),
        format!(
            "refused: the request fails at once as reply-refused, after one attempt: {state:?}"
        ),
    );
    claim(
        called
            .iter()
            .any(|line| line.contains("failed: reply-refused")),
        format!("refused: `agent show` says failed: reply-refused: {called:?}"),
    );
    // The time-out is lifted. The refused reply is not published later:
    // longer than the interval at which stored replies are published again.
    publish(tagged(9043, vec![vec!["p".into(), scout.pubkey.clone()]])).await;
    let lifted = mention(&scout.pubkey, "and after the time-out?".into()).await;
    assert_eq!(
        exactly_one(lifted.id.to_hex(), "after the time-out").await,
        "pong: this conversation holds 1 message(s) from people"
    );
    tokio::time::sleep(Duration::from_secs(35)).await;
    claim(
        replies(refused_id.clone()).await.is_empty(),
        format!(
            "refused: a refused reply is never published later: {:?}",
            replies(refused_id.clone()).await
        ),
    );
    claim(
        stored_reply(&refused_id).as_ref() == stored.first(),
        "refused: the stored reply is still the first one".into(),
    );

    // ---- removed while it answers -----------------------------------------
    let late = mention(
        &scout.pubkey,
        r#"do: [["sleep", 8000], ["buzz_send", {"text": "answered after I was removed", "final": true}]]"#.into(),
    )
    .await;
    let late_id = late.id.to_hex();
    until("the turn to be running", WAIT, || {
        (turn(&late_id).map(|(state, ..)| state).as_deref() == Some("delivered")).then_some(())
    })
    .await;
    publish(buzz_sdk::build_remove_member(lab, &scout.pubkey).unwrap()).await;
    role_becomes(&scout.pubkey, None).await;
    tokio::time::sleep(Duration::from_secs(16)).await;
    let published = replies(late_id.clone()).await;
    claim(
        published.is_empty(),
        format!(
            "removed: nothing is published after the agent was removed from the channel: {published:?}"
        ),
    );
    let state = turn(&late_id);
    claim(
        state
            .as_ref()
            .is_some_and(|(state, failure, _)| state == "failed" && failure == "not-a-member"),
        format!("removed: the request fails as not-a-member: {state:?}"),
    );
    // Added again, it answers again; the old mention stays as it is.
    ok(&["agent", "add", "Scout", &lab_name]);
    let again = mention(&scout.pubkey, "back in the channel?".into()).await;
    assert_eq!(
        exactly_one(again.id.to_hex(), "added again").await,
        "pong: this conversation holds 1 message(s) from people"
    );
    claim(
        replies(late_id.clone()).await.is_empty(),
        "removed: the mention of that time was answered afterwards".into(),
    );

    // ---- bzz agent pause and resume, from another process -----------------
    ok(&["agent", "pause", "Scout"]);
    log_says(
        "Scout · pi-durable · paused",
        "pause: the bzz that runs the agent pauses it",
    )
    .await;
    presence_becomes(&scout.pubkey, false, "a paused agent says it is gone").await;
    let while_paused = mention(&scout.pubkey, "while you are paused".into()).await;
    tokio::time::sleep(Duration::from_secs(8)).await;
    claim(
        replies(while_paused.id.to_hex()).await.is_empty(),
        "pause: a mention is answered while the agent is paused".into(),
    );
    ok(&["agent", "resume", "Scout"]);
    presence_becomes(&scout.pubkey, true, "a resumed agent is present again").await;
    let resumed = mention(&scout.pubkey, "and resumed?".into()).await;
    assert_eq!(
        exactly_one(resumed.id.to_hex(), "after the resume").await,
        "pong: this conversation holds 1 message(s) from people"
    );
    claim(
        replies(while_paused.id.to_hex()).await.is_empty(),
        "pause: what was said while it was paused was answered later".into(),
    );

    // ---- bzz agent delete, from another process ---------------------------
    // Echo: served by the running bzz, idle. It has no process, and the
    // running bzz holds its key and its connection.
    let (deleted, text) = said(&["agent", "delete", "Echo", "--yes"]);
    assert!(deleted, "{text}");
    claim(
        text.contains("the bzz that runs Echo") && text.contains("has stopped it"),
        format!("delete: the delete says that it reached the bzz that runs the agent: {text}"),
    );
    log_says(
        "Echo · pi-durable · stopped · deleted",
        "delete: the bzz that served the agent let go of it",
    )
    .await;
    presence_becomes(&echo.pubkey, false, "a deleted agent says it is gone").await;
    let serving = |agent: uuid::Uuid| {
        std::fs::read_dir(paths.data_dir.join("agents").join("serving"))
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(&agent.to_string())
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    claim(
        serving(echo.id) == 0 && serving(scout.id) == 1,
        format!(
            "delete: no bzz says it holds the deleted agent's key, and one holds the other's: Echo {}, Scout {}",
            serving(echo.id),
            serving(scout.id)
        ),
    );
    // Scout: deleted while it is answering.
    let last = mention(
        &scout.pubkey,
        r#"do: [["sleep", 8000], ["buzz_send", {"text": "answered after I was deleted", "final": true}]]"#.into(),
    )
    .await;
    let last_id = last.id.to_hex();
    until("the turn to be running", WAIT, || {
        (turn(&last_id).map(|(state, ..)| state).as_deref() == Some("delivered")).then_some(())
    })
    .await;
    let harness = bzz::agents::runtime::process::read_record(
        &bzz::agents::runtime::process::run_dir(&paths.data_dir),
        scout.id,
    )
    .map(|record| record.process.pid);
    let (deleted, text) = said(&["agent", "delete", "Scout", "--yes"]);
    claim(
        deleted,
        format!("delete: an agent that is answering is stopped and deleted: {text}"),
    );
    if deleted {
        claim(
            harness.is_some_and(|pid| !support::agent_harness::running(pid)),
            format!("delete: the agent's process is gone when the delete returns: {harness:?}"),
        );
        log_says(
            "Scout · pi-durable · stopped · deleted",
            "delete: the bzz that ran the agent let go of it",
        )
        .await;
        claim(
            serving(scout.id) == 0,
            "delete: a bzz still says it holds the deleted agent's key".into(),
        );
        claim(
            Config::load(&paths).unwrap().hosted_agents.is_empty(),
            "delete: the settings still hold an agent".into(),
        );
    }
    tokio::time::sleep(Duration::from_secs(14)).await;
    let after = replies(last_id.clone()).await;
    claim(
        after.is_empty(),
        format!("delete: no bzz publishes as an agent after it was deleted: {after:?}"),
    );
    presence_becomes(&scout.pubkey, false, "the deleted agent says it is gone").await;

    // The bzz that ran them is still there, and stops as always.
    let status = Command::new("kill")
        .args(["-TERM", &running.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        running.wait().unwrap().success(),
        "{}",
        std::fs::read_to_string(&run_log).unwrap()
    );
    owner_supervisor.shutdown().await;
    owner.lock().await;
    let faults = faults.into_inner();
    assert!(
        faults.is_empty(),
        "{} claim(s) do not hold:\n- {}",
        faults.len(),
        faults.join("\n- ")
    );
}

/// One bzz profile for the records of apps: a generated identity with its
/// own store, connection and records service.
#[cfg(feature = "apps")]
struct RecordsProfile {
    keys: Keys,
    signer: SignerHandle,
    supervisor: SupervisorHandle,
    service: bzz::service::records::RecordsService,
    /// The store and the HTTP client the service was made with, for the
    /// offers of apps (M6).
    handle: StoreHandle,
    http: HttpClient,
    config: Config,
    community: uuid::Uuid,
    store: std::path::PathBuf,
}

#[cfg(feature = "apps")]
impl RecordsProfile {
    fn config(keys: &Keys) -> (Config, uuid::Uuid) {
        let identity = IdentityConfig {
            id: uuid::Uuid::new_v4(),
            label: "records".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "records".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community(
                "records".into(),
                "ws://localhost:3030".into(),
                identity.id,
                true,
            )
            .unwrap();
        (config, community)
    }

    /// `store` is a file, so that the profile can be closed and opened again.
    async fn start(
        keys: Keys,
        signer: SignerHandle,
        (config, community): (Config, uuid::Uuid),
        store: std::path::PathBuf,
    ) -> Self {
        let http = HttpClient::new(
            url::Url::parse("http://localhost:3030/").unwrap(),
            signer.clone(),
        )
        .unwrap();
        let relay = bzz::protocol::http::relay_signing_pubkey(&http.nip11().await.unwrap())
            .unwrap()
            .to_owned();
        let mut database = Store::open(&store).unwrap();
        database.sync_config(&config).unwrap();
        database.pin_relay_pubkey(community, &relay).unwrap();
        let supervisor = SupervisorHandle::spawn(
            url::Url::parse("ws://localhost:3030/").unwrap(),
            signer.clone(),
        );
        let handle = StoreHandle::spawn(database).unwrap();
        let service = bzz::service::records::RecordsService::new(
            community,
            http.clone(),
            handle.clone(),
            bzz::service::records::Writer::new(signer.clone(), supervisor.clone()),
        );
        Self {
            keys,
            signer,
            supervisor,
            service,
            handle,
            http,
            config,
            community,
            store,
        }
    }

    /// A person with a profile of their own, a member of the relay with
    /// `relay_role`. `seed_member` makes everyone an owner of the relay,
    /// which is not who a guest of a channel or a stranger to it is.
    async fn person(source: &str, folder: &std::path::Path, name: &str, relay_role: &str) -> Self {
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        let sql = format!(
            "INSERT INTO relay_members (community_id,pubkey,role,added_by) SELECT id,'{pubkey}','{relay_role}',NULL FROM communities WHERE lower(host)='localhost:3030' ON CONFLICT (community_id,pubkey) DO UPDATE SET role='{relay_role}',updated_at=now();"
        );
        let status = Command::new("docker")
            .args(["compose", "-p", "buzz-harness", "-f"])
            .arg(format!("{source}/docker-compose.harness.yml"))
            .args([
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
        let signer = SignerHandle::spawn(keys.clone());
        let config = Self::config(&keys);
        Self::start(keys, signer, config, folder.join(format!("{name}.sqlite3"))).await
    }

    /// The same identity and store with no relay to reach: bzz opened
    /// offline. The store was opened before, so the relay's key is pinned.
    fn offline(
        keys: Keys,
        (config, community): (Config, uuid::Uuid),
        store: std::path::PathBuf,
    ) -> Self {
        let signer = SignerHandle::spawn(keys.clone());
        // Nothing listens on the discard port: every connection is refused.
        let http = HttpClient::new(
            url::Url::parse("http://127.0.0.1:9/").unwrap(),
            signer.clone(),
        )
        .unwrap();
        let mut database = Store::open(&store).unwrap();
        database.sync_config(&config).unwrap();
        let supervisor = SupervisorHandle::spawn(
            url::Url::parse("ws://127.0.0.1:9/").unwrap(),
            signer.clone(),
        );
        let handle = StoreHandle::spawn(database).unwrap();
        let service = bzz::service::records::RecordsService::new(
            community,
            http.clone(),
            handle.clone(),
            bzz::service::records::Writer::new(signer.clone(), supervisor.clone()),
        );
        Self {
            keys,
            signer,
            supervisor,
            service,
            handle,
            http,
            config,
            community,
            store,
        }
    }

    fn pubkey(&self) -> String {
        self.keys.public_key().to_hex()
    }

    async fn send(&self, builder: EventBuilder) -> nostr::Event {
        let event = self.signer.sign(builder).await.unwrap();
        let ack = self.supervisor.publish(event.clone()).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        event
    }

    /// Closes bzz and opens it again on the same store and identity.
    async fn restart(self) -> Self {
        let (keys, config, store) = (
            self.keys.clone(),
            (self.config.clone(), self.community),
            self.store.clone(),
        );
        self.supervisor.shutdown().await;
        self.signer.lock().await;
        drop(self);
        let signer = SignerHandle::spawn(keys.clone());
        Self::start(keys, signer, config, store).await
    }

    async fn stop(self) {
        self.supervisor.shutdown().await;
        self.signer.lock().await;
    }
}

/// v0.16 M2 (service half) on the pinned relay: two people share the records
/// of one app in a channel.
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_app_records_are_shared_by_two_profiles() {
    use bzz::protocol::record::{Change, Record, RecordError, Refusal};
    use bzz::service::records::{Cause, RecordEvent, SyncState, Writer};
    use serde_json::{Map, Value, json};
    use tokio::sync::mpsc::UnboundedReceiver;

    async fn next(events: &mut UnboundedReceiver<RecordEvent>) -> RecordEvent {
        tokio::time::timeout(Duration::from_secs(20), events.recv())
            .await
            .expect("an event of the records in time")
            .expect("the records are open")
    }
    async fn changes(events: &mut UnboundedReceiver<RecordEvent>) -> (Vec<Change>, Cause) {
        loop {
            if let RecordEvent::Changes { changes, cause } = next(events).await {
                return (changes, cause);
            }
        }
    }
    /// Waits for `wanted`; any change told on the way is a failure.
    async fn until_state(events: &mut UnboundedReceiver<RecordEvent>, wanted: &SyncState) {
        loop {
            match next(events).await {
                RecordEvent::State(state) if &state == wanted => return,
                RecordEvent::State(_) => {}
                other @ RecordEvent::Changes { .. } => panic!("unexpected {other:?}"),
            }
        }
    }
    async fn silence(events: &mut UnboundedReceiver<RecordEvent>, quiet: Duration) {
        if let Ok(event) = tokio::time::timeout(quiet, events.recv()).await {
            panic!("unexpected {event:?}");
        }
    }
    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }
    fn sorted(mut records: Vec<Record>) -> Vec<Record> {
        records.sort_by_key(|record| record.id);
        records
    }
    const TYPES: [&str; 2] = ["card", "column"];

    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let folder = tempfile::tempdir().unwrap();
    let ana = RecordsProfile::person(&source, folder.path(), "ana", "member").await;
    let ben = RecordsProfile::person(&source, folder.path(), "ben", "member").await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // A private channel of the two, so that nothing else is in it.
    let channel = uuid::Uuid::new_v4();
    ana.send(
        buzz_sdk::build_create_channel(
            channel,
            &format!("records-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    ana.send(buzz_sdk::build_add_member(channel, &ben.pubkey(), None).unwrap())
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Both open the same app. Nothing is there yet.
    let (ana_app, mut ana_events) = ana.service.open(channel, "board", &TYPES).await.unwrap();
    let (ben_app, mut ben_events) = ben.service.open(channel, "board", &TYPES).await.unwrap();
    until_state(&mut ana_events, &SyncState::Live).await;
    until_state(&mut ben_events, &SyncState::Live).await;
    assert!(ana_app.records().is_empty() && ben_app.records().is_empty());

    // 1. A change by one arrives live at the other, with who signed it. The
    // writer is answered by its call and told nothing more.
    let started = std::time::Instant::now();
    let card = ana_app
        .create(
            "card",
            object(json!({"title": "Write the brief", "column": "todo", "order": 1024.0})),
        )
        .await
        .unwrap();
    let (told, cause) = changes(&mut ben_events).await;
    eprintln!(
        "a record created by one profile reached the other {} ms after the create was sent",
        started.elapsed().as_millis()
    );
    assert_eq!(
        (told, cause),
        (vec![Change::Upsert(card.clone())], Cause::Live)
    );
    assert_eq!(card.author, ana.pubkey());
    assert_eq!(ben_app.records(), std::slice::from_ref(&card));
    silence(&mut ana_events, Duration::from_millis(500)).await;
    let column = ben_app
        .create("column", object(json!({"title": "Doing", "order": 2})))
        .await
        .unwrap();
    assert_eq!(
        changes(&mut ana_events).await.0,
        [Change::Upsert(column.clone())]
    );
    assert_eq!(column.author, ben.pubkey());

    // 2. Both revise the same record at once, five times over. Each time one
    // is stored and the other gets a conflict with the record as it is now;
    // its retry on that record is stored, and nothing either wrote is lost.
    let mut base = card.clone();
    for round in 0..5 {
        let with = |record: &Record, field: &str| {
            let mut content = record.content.clone();
            content.insert(field.into(), json!(round));
            content
        };
        let (from_ana, from_ben) = tokio::join!(
            ana_app.update(base.id, &base.revision, with(&base, "ana")),
            ben_app.update(base.id, &base.revision, with(&base, "ben")),
        );
        let (winner, loser, loser_app, field) = match (from_ana, from_ben) {
            (Ok(stored), Err(error)) => (stored, error, &ben_app, "ben"),
            (Err(error), Ok(stored)) => (stored, error, &ana_app, "ana"),
            other => panic!("round {round}: exactly one must be stored: {other:?}"),
        };
        let RecordError::Conflict { current } = loser else {
            panic!("round {round}: {loser:?}")
        };
        assert_eq!(
            *current, winner,
            "round {round}: the current record is the winner's"
        );
        let merged = loser_app
            .update(current.id, &current.revision, with(&current, field))
            .await
            .unwrap();
        assert_eq!(merged.content["ana"], json!(round));
        assert_eq!(merged.content["ben"], json!(round));
        assert_eq!(merged.content["title"], "Write the brief");
        // Both profiles end on the same record.
        tokio::time::timeout(Duration::from_secs(10), async {
            while ana_app.record(base.id) != Some(merged.clone())
                || ben_app.record(base.id) != Some(merged.clone())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("both profiles hold the merged record");
        base = merged;
    }
    while tokio::time::timeout(Duration::from_millis(300), ana_events.recv())
        .await
        .is_ok()
    {}
    while tokio::time::timeout(Duration::from_millis(300), ben_events.recv())
        .await
        .is_ok()
    {}

    // 3. Ordinary chat ends every sync of the channel: a deleted message
    // (`resync: redaction`). Both profiles ask again, are told of no change,
    // and their records were never emptied.
    let message = ana
        .send(
            buzz_sdk::build_message(channel, "to be deleted", None, &[], false, &[], &[]).unwrap(),
        )
        .await;
    silence(&mut ben_events, Duration::from_millis(500)).await;
    ana.send(buzz_sdk::build_delete_message(channel, message.id).unwrap())
        .await;
    for (app, events) in [(&ana_app, &mut ana_events), (&ben_app, &mut ben_events)] {
        until_state(events, &SyncState::Resyncing).await;
        assert_eq!(app.records().len(), 2);
        until_state(events, &SyncState::Live).await;
        assert_eq!(app.records().len(), 2);
    }
    assert_eq!(sorted(ana_app.records()), sorted(ben_app.records()));

    // 4. A type that was not declared is refused, and the relay never sees it.
    let total = ana.service.channel_total(channel).await.unwrap();
    assert_eq!(total, 2);
    for outside in ["vote", "bzz.app.board.card", "buzz.task"] {
        let error = ana_app
            .create(outside, object(json!({"title": "outside"})))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "fenced", "{outside}: {error:?}");
    }
    assert_eq!(ana.service.channel_total(channel).await.unwrap(), total);

    // 5. After bzz is closed and opened again the same records are there,
    // before the relay answers anything.
    let before = sorted(ana_app.records());
    drop((ana_app, ana_events));
    let ana = ana.restart().await;
    let (ana_app, mut ana_events) = ana.service.open(channel, "board", &TYPES).await.unwrap();
    assert_eq!(
        sorted(ana_app.records()),
        before,
        "read from the store as `open` returns"
    );
    assert_eq!(before, sorted(ben_app.records()));
    // And the relay's set is the same: no change is told.
    until_state(&mut ana_events, &SyncState::Live).await;
    assert_eq!(sorted(ana_app.records()), before);

    // A deletion arrives live too, with who deleted.
    let gone = ben_app.delete(column.id, &column.revision).await.unwrap();
    assert_eq!(gone, column);
    assert_eq!(
        changes(&mut ana_events).await.0,
        [Change::Removed {
            record: column.clone(),
            by: Some(ben.pubkey())
        }]
    );
    assert_eq!(ana_app.records().len(), 1);

    // 6. A guest reads and cannot write; someone outside the channel cannot
    // read. Each is told in the relay's own terms.
    let guest = RecordsProfile::person(&source, folder.path(), "guest", "member").await;
    let outsider = RecordsProfile::person(&source, folder.path(), "outsider", "member").await;
    ana.send(
        buzz_sdk::build_add_member(channel, &guest.pubkey(), Some(buzz_sdk::MemberRole::Guest))
            .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (guest_app, mut guest_events) = guest.service.open(channel, "board", &TYPES).await.unwrap();
    assert_eq!(changes(&mut guest_events).await.0.len(), 1);
    until_state(&mut guest_events, &SyncState::Live).await;
    let error = guest_app
        .create("card", object(json!({"title": "a guest's"})))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            RecordError::Refused {
                refusal: Refusal::Guest,
                ..
            }
        ),
        "{error:?}"
    );
    let (outsider_app, mut outsider_events) = outsider
        .service
        .open(channel, "board", &TYPES)
        .await
        .unwrap();
    until_state(&mut outsider_events, &SyncState::Restricted).await;
    assert!(outsider_app.records().is_empty());
    assert!(matches!(
        outsider_app
            .create("card", object(json!({"title": "x"})))
            .await,
        Err(RecordError::Refused {
            refusal: Refusal::Unreadable,
            ..
        })
    ));
    silence(&mut ana_events, Duration::from_millis(500)).await;
    assert_eq!(ana.service.channel_total(channel).await.unwrap(), 1);

    // 7. The relay's own quota. The harness raises a person's quota for the
    // other tests of this suite and leaves an agent's at the relay's default
    // (120 writes a minute at the pin). An agent of Ana's writes through her
    // open app, with its own key on its own connection: what M7 will do.
    let agent_keys = Keys::generate();
    let auth = buzz_sdk::nip_oa::compute_auth_tag(&ana.keys, &agent_keys.public_key(), "").unwrap();
    let agent_signer = SignerHandle::spawn_with_auth_tag(
        agent_keys.clone(),
        Some(buzz_sdk::nip_oa::parse_auth_tag(&auth).unwrap()),
    );
    let agent_supervisor = SupervisorHandle::spawn(
        url::Url::parse("ws://localhost:3030/").unwrap(),
        agent_signer.clone(),
    );
    let agent = Writer::new(agent_signer.clone(), agent_supervisor.clone());
    ana.send(
        buzz_sdk::build_add_member(channel, &agent.pubkey(), Some(buzz_sdk::MemberRole::Bot))
            .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Adding a member does not end a sync; adding the guest did not either.
    let first = ana_app
        .create_as(
            &agent,
            "card",
            object(json!({"title": "The agent's first"})),
        )
        .await
        .unwrap();
    assert_eq!(first.author, agent.pubkey());
    // Ana's app is told of it once, as of any participant's change; so is Ben's.
    assert_eq!(
        changes(&mut ana_events).await.0,
        [Change::Upsert(first.clone())]
    );
    assert_eq!(
        changes(&mut ben_events).await.0,
        [Change::Upsert(first.clone())]
    );
    silence(&mut ana_events, Duration::from_millis(500)).await;

    let started = std::time::Instant::now();
    let mut accepted = 1;
    let wait = loop {
        match ana_app
            .create_as(
                &agent,
                "card",
                object(json!({"title": format!("card {accepted}")})),
            )
            .await
        {
            Ok(_) => accepted += 1,
            Err(RecordError::RateLimited { seconds }) => break seconds,
            Err(other) => panic!("after {accepted} writes: {other:?}"),
        }
        assert!(accepted <= 200, "the relay's quota was never met");
    };
    eprintln!(
        "the relay accepted {accepted} writes of the agent in {} s, then asked for a wait of {wait} s",
        started.elapsed().as_secs()
    );
    assert_eq!(
        accepted, 120,
        "the relay's own quota for an agent at the pin"
    );
    assert!((1..=60).contains(&wait), "{wait}");
    // While the gate is closed the next write is answered at once, with what
    // is left of the wait.
    let again = std::time::Instant::now();
    let Err(RecordError::RateLimited { seconds }) = ana_app
        .create_as(&agent, "card", object(json!({"title": "too soon"})))
        .await
    else {
        panic!("expected the wait again")
    };
    assert!(seconds <= wait && seconds >= 1, "{seconds} after {wait}");
    assert!(again.elapsed() < Duration::from_secs(2));
    // The agent's quota is not Ana's: her own write is stored at that moment.
    let hers = ana_app
        .create("card", object(json!({"title": "Ana's, meanwhile"})))
        .await
        .unwrap();
    assert_eq!(hers.author, ana.pubkey());
    // Every write the relay accepted is in both profiles' sets.
    let expected = 1 + accepted + 1;
    tokio::time::timeout(Duration::from_secs(20), async {
        while ana_app.records().len() != expected || ben_app.records().len() != expected {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("both profiles hold every accepted record");
    assert_eq!(sorted(ana_app.records()), sorted(ben_app.records()));

    // 8. An archived channel refuses writes and still answers the sync.
    // Archiving ends the syncs too (`resync: channel access changed`).
    while tokio::time::timeout(Duration::from_millis(300), ana_events.recv())
        .await
        .is_ok()
    {}
    ana.send(buzz_sdk::build_archive(channel).unwrap()).await;
    until_state(&mut ana_events, &SyncState::Resyncing).await;
    until_state(&mut ana_events, &SyncState::Live).await;
    assert_eq!(ana_app.records().len(), expected);
    let error = ana_app
        .create("card", object(json!({"title": "into the archive"})))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            RecordError::Refused {
                refusal: Refusal::Archived,
                ..
            }
        ),
        "{error:?}"
    );
    ana.send(buzz_sdk::build_unarchive(channel).unwrap()).await;
    until_state(&mut ana_events, &SyncState::Resyncing).await;
    until_state(&mut ana_events, &SyncState::Live).await;
    ana_app
        .create("card", object(json!({"title": "out of the archive"})))
        .await
        .unwrap();

    drop((ana_app, ben_app, guest_app, outsider_app));
    agent_supervisor.shutdown().await;
    agent_signer.lock().await;
    for profile in [ana, ben, guest, outsider] {
        profile.stop().await;
    }
}

/// A script that writes as many records at once as it is pasted a number,
/// and counts what it is answered.
#[cfg(feature = "apps")]
const BURST_APP: &str = r#"
local app = { api = 1, types = { "card" } }
function app.init(ctx) return { ok = 0, waits = 0, other = 0, wait = "" } end
function app.view(s, ctx)
  return ui.text { "stored " .. s.ok, "told to wait " .. s.waits .. s.wait, "other " .. s.other .. (s.why or "") }
end
function app.paste(s, text, ctx)
  for i = 1, tonumber(text) do data.create { type = "card", content = { title = "burst " .. i } } end
end
function app.reply(s, r, ctx)
  if r.ok then s.ok = s.ok + 1
  elseif r.error.code == "rate_limited" then
    s.waits = s.waits + 1
    s.wait = " (" .. (r.error.retry_after > 0 and r.error.retry_after <= 60 and "1 to 60 s" or "?") .. ")"
  else s.other = s.other + 1; s.why = " " .. r.error.code .. ": " .. r.error.message end
end
return app
"#;

/// v0.16 M2, the exit gate, to the letter: two bzz profiles on the pinned
/// relay share a board. Each profile is a generated identity with its own
/// store on disk, connection, records service and app session; the app is
/// the Board fixture, driven by keys through the session and read as
/// screens of 40 by 16, the size of a docked app.
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_app_session_two_profiles_share_a_board() {
    use bzz::apps::runtime::{RecordOp, RecordRequest};
    use bzz::apps::session::SessionOptions;
    use bzz::service::records::SyncState;
    use serde_json::json;
    use support::app_pane::{Pane, board_cards, board_column, last_row};

    const BOARD: &str = include_str!("fixtures/apps/board/app.lua");
    const SAVED: &str = "Saved.";
    const MERGED: &str = "Conflict: merged with a newer change.";

    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let folder = tempfile::tempdir().unwrap();
    let ana = RecordsProfile::person(&source, folder.path(), "ana", "member").await;
    let ben = RecordsProfile::person(&source, folder.path(), "ben", "member").await;
    let dan = RecordsProfile::person(&source, folder.path(), "dan", "member").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let channel = uuid::Uuid::new_v4();
    ana.send(
        buzz_sdk::build_create_channel(
            channel,
            &format!("board-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    for member in [&ben, &dan] {
        ana.send(buzz_sdk::build_add_member(channel, &member.pubkey(), None).unwrap())
            .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let names: std::collections::HashMap<String, String> = [
        (ana.pubkey(), "ana".to_owned()),
        (ben.pubkey(), "ben".to_owned()),
    ]
    .into();
    let open = |profile: &RecordsProfile| {
        Pane::open_with(
            &profile.service,
            channel,
            BOARD,
            SessionOptions::new("board").names(names.clone()),
        )
    };
    let mut ana_pane = open(&ana);
    let mut ben_pane = open(&ben);
    // The first time: nothing is kept, the list waits for the relay's set.
    ana_pane.settled("Loaded 0 cards.").await;
    ben_pane.settled("Loaded 0 cards.").await;

    // 1. A change by one arrives live on the other's screen.
    let started = std::time::Instant::now();
    ana_pane.key("n");
    ana_pane.type_text("Write the brief");
    ana_pane.key("enter");
    let hers = ana_pane.settled(SAVED).await;
    assert!(
        hers.contains("To do (1)") && hers.contains("> Write the brief"),
        "{hers}"
    );
    let his = ben_pane
        .until("ana's card on ben's screen", |screen, _| {
            screen.contains("Write the brief")
        })
        .await;
    eprintln!(
        "a card made on one profile was on the other's screen {} ms after the keys",
        started.elapsed().as_millis()
    );
    assert_eq!(board_cards(&his), board_cards(&hers));
    assert_eq!(board_cards(&his)["todo"], ["Write the brief"]);
    // And the other way: ben makes a card and moves it.
    ben_pane.key("n");
    ben_pane.type_text("Review it");
    ben_pane.key("enter");
    ben_pane.settled(SAVED).await;
    ben_pane.key("]");
    ben_pane
        .until("ben's own move", |screen, _| {
            board_column(screen, "Review it") == Some("doing") && last_row(screen) == SAVED
        })
        .await;
    let hers = ana_pane
        .until("ben's card, moved, on ana's screen", |screen, _| {
            board_column(screen, "Review it") == Some("doing")
        })
        .await;
    assert_eq!(board_cards(&hers), board_cards(&ben_pane.screen()));
    assert_eq!(ana.service.channel_total(channel).await.unwrap(), 2);

    // 2. A simultaneous change loses nothing. Both move the same card from
    // the same revision, in opposite directions, five times over: three
    // times at once, twice with ben's a moment sooner. One move is stored;
    // the other is told of the conflict, puts its move on top of the
    // current card and stores that; both screens then show the same card.
    ben_pane.key("h");
    for pane in [&mut ana_pane, &mut ben_pane] {
        pane.until("the brief selected", |screen, _| {
            screen.contains("> Write the brief")
        })
        .await;
    }
    let mut merged_by = Vec::new();
    for round in 0..5 {
        // The card goes to the middle column, by ana alone.
        let now = board_column(&ana_pane.screen(), "Write the brief").unwrap();
        if now != "doing" {
            ana_pane.key(if now == "todo" { "]" } else { "[" });
        }
        for pane in [&mut ana_pane, &mut ben_pane] {
            pane.until("the brief in the middle", |screen, _| {
                board_column(screen, "Write the brief") == Some("doing")
                    && last_row(screen) != "Saving..."
            })
            .await;
            // A fresh status line, so that the round's own is told apart.
            pane.key("r");
            pane.settled("Loaded 2 cards.").await;
        }
        // Both from the same revision: ana to the right, ben to the left.
        // Each script has asked before either request is carried.
        ana_pane.key("]");
        ben_pane.key("[");
        tokio::time::sleep(Duration::from_millis(150)).await;
        if round % 2 == 0 {
            // At once: both writes are on their way before bzz hears of
            // either, and the relay stores the one it takes first.
            ana_pane.screen();
            ben_pane.screen();
        } else {
            // Ben's a moment sooner: Ana's bzz may know the new head by
            // the time her write would be sent. To her script the answer
            // is the same conflict.
            ben_pane.screen();
            tokio::time::sleep(Duration::from_millis(60)).await;
            ana_pane.screen();
        }
        let mut ends = Vec::new();
        for pane in [&mut ana_pane, &mut ben_pane] {
            let screen = pane
                .until("the answer to the move", |screen, _| {
                    [SAVED, MERGED].contains(&last_row(screen))
                })
                .await;
            ends.push(last_row(&screen).to_owned());
        }
        let (merger, wanted) = match (ends[0].as_str(), ends[1].as_str()) {
            (SAVED, MERGED) => ("ben", "todo"),
            (MERGED, SAVED) => ("ana", "done"),
            other => panic!("round {round}: exactly one is stored and one is told: {other:?}"),
        };
        merged_by.push(merger);
        // Both screens end on the current card: the one the loser stored
        // on top of the winner's.
        for pane in [&mut ana_pane, &mut ben_pane] {
            pane.until("both on the current card", |screen, _| {
                board_column(screen, "Write the brief") == Some(wanted)
            })
            .await;
        }
        let card = |pane: &Pane| {
            pane.session
                .records()
                .find(|record| record.content["title"] == "Write the brief")
                .cloned()
                .unwrap()
        };
        let (hers, his) = (card(&ana_pane), card(&ben_pane));
        assert_eq!(hers, his, "round {round}: one record, one revision");
        assert_eq!(
            hers.author,
            if merger == "ana" {
                ana.pubkey()
            } else {
                ben.pubkey()
            },
            "round {round}: the last revision is the merged one"
        );
        assert_eq!(hers.content["column"], wanted);
        assert_eq!(
            board_cards(&ana_pane.screen()),
            board_cards(&ben_pane.screen())
        );
    }
    eprintln!("the five simultaneous moves were merged by {merged_by:?}");
    assert!(
        merged_by.contains(&"ana"),
        "ana was told of a conflict at least in the rounds ben was sooner: {merged_by:?}"
    );
    assert_eq!(
        ana.service.channel_total(channel).await.unwrap(),
        2,
        "no card was lost and none was made twice"
    );

    // 3. A script that asks for an undeclared type is refused: at the
    // session, when the runtime is passed by, and at the service, when the
    // session is. The relay never sees it.
    let brief = ana_pane
        .session
        .records()
        .find(|record| record.content["title"] == "Write the brief")
        .cloned()
        .unwrap();
    for (n, op) in [
        RecordOp::Create {
            record_type: "vote".into(),
            content: json!({"title": "outside"}).as_object().unwrap().clone(),
        },
        RecordOp::Create {
            record_type: "bzz.app.board.card".into(),
            content: json!({"title": "outside"}).as_object().unwrap().clone(),
        },
        RecordOp::Delete {
            record_id: brief.id.to_string(),
            record_type: "vote".into(),
            rev: brief.revision.clone(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        ana_pane.session.serve(RecordRequest {
            id: 9_000_000 + n as u64,
            op,
        });
    }
    let records = ana_pane.session.relay().expect("the records are open");
    for outside in ["vote", "bzz.app.board.card", "buzz.task"] {
        let error = records
            .create(
                outside,
                json!({"title": "outside"}).as_object().unwrap().clone(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "fenced", "{outside}: {error:?}");
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    ana_pane.screen();
    assert_eq!(ana.service.channel_total(channel).await.unwrap(), 2);
    assert_eq!(ana_pane.session.records().count(), 2);
    drop(records);

    // 4. A restart shows the same records. Ana's session and store are
    // closed and opened again, first with no relay to reach: the screen
    // has the cards from the store, and nothing else can have put them
    // there.
    ana_pane.key("r");
    let before = board_cards(&ana_pane.settled("Loaded 2 cards.").await);
    assert_eq!(before.values().map(Vec::len).sum::<usize>(), 2);
    drop(ana_pane);
    let (keys, config, store) = (
        ana.keys.clone(),
        (ana.config.clone(), ana.community),
        ana.store.clone(),
    );
    ana.stop().await;
    let offline = RecordsProfile::offline(keys.clone(), config.clone(), store.clone());
    let mut pane = open(&offline);
    let screen = pane.settled("Loaded 2 cards.").await;
    assert_eq!(board_cards(&screen), before, "from the store alone");
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(pane.status().sync, SyncState::Loading, "no relay answered");
    }
    drop(pane);
    offline.stop().await;
    // Then with the relay: the screen has the cards before the relay's set
    // is complete, and the same after.
    let ana = RecordsProfile::start(keys.clone(), SignerHandle::spawn(keys), config, store).await;
    let mut ana_pane = open(&ana);
    let mut seen = Vec::new();
    ana_pane
        .until("the relay's set", |screen, status| {
            if last_row(screen) == "Loaded 2 cards." {
                assert_eq!(board_cards(screen), before, "while {}", status.sync_words);
                if seen.last() != Some(&status.sync_words) {
                    seen.push(status.sync_words.clone());
                }
            }
            status.sync == SyncState::Live && last_row(screen) == "Loaded 2 cards."
        })
        .await;
    eprintln!("after the restart the cards were on the screen while the records were: {seen:?}");
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(board_cards(&ana_pane.screen()), before);
    }
    assert_eq!(board_cards(&ben_pane.screen()), before);

    // 5. At the quota the pane shows the wait: the app's own budget, half
    // of a person's. Dan has written nothing yet. His app asks for forty
    // records at once: thirty are stored, ten are told to wait, and the
    // relay never saw them. (This suite raises a person's quota at the
    // relay, so the relay itself refuses none here.)
    let mut dan_pane = Pane::open(&dan.service, channel, "burst", BURST_APP);
    dan_pane
        .until("dan's app", |screen, status| {
            screen.starts_with("stored 0") && status.sync == SyncState::Live
        })
        .await;
    assert!(dan_pane.session.paste("40"));
    let screen = dan_pane
        .until("forty answers", |screen, _| {
            screen.starts_with("stored 30\ntold to wait 10 (1 to 60 s)")
        })
        .await;
    assert!(screen.contains("other 0"), "{screen}");
    let status = dan_pane.status();
    let notice = status.notice.clone().expect("the pane says the wait");
    assert!(
        notice.starts_with("Too many changes: this app can write again in ")
            && status
                .wait
                .is_some_and(|seconds| (1..=60).contains(&seconds)),
        "{status:?}"
    );
    eprintln!("at the app's own budget the pane says: {notice}");
    assert_eq!(dan.service.channel_total(channel).await.unwrap(), 32);
    // His chat is not held up by it: the relay takes his message at once.
    let said = dan
        .send(buzz_sdk::build_message(channel, "still here", None, &[], false, &[], &[]).unwrap())
        .await;
    assert_eq!(said.pubkey, dan.keys.public_key());

    // 6. At the relay's own quota, which this suite does not raise for an
    // agent (120 writes a minute at the pin): a profile whose identity is
    // an agent of ana's. Its chat spends the whole budget; the next write
    // of its app is refused by the relay with the wait, and the pane shows
    // it.
    let agent_keys = Keys::generate();
    let auth = buzz_sdk::nip_oa::compute_auth_tag(&ana.keys, &agent_keys.public_key(), "").unwrap();
    let agent_signer = SignerHandle::spawn_with_auth_tag(
        agent_keys.clone(),
        Some(buzz_sdk::nip_oa::parse_auth_tag(&auth).unwrap()),
    );
    ana.send(
        buzz_sdk::build_add_member(
            channel,
            &agent_keys.public_key().to_hex(),
            Some(buzz_sdk::MemberRole::Bot),
        )
        .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let cy = RecordsProfile::start(
        agent_keys.clone(),
        agent_signer,
        RecordsProfile::config(&agent_keys),
        folder.path().join("cy.sqlite3"),
    )
    .await;
    let mut cy_pane = Pane::open(&cy.service, channel, "ledger", BURST_APP);
    cy_pane
        .until("the agent's app", |screen, status| {
            screen.starts_with("stored 0") && status.sync == SyncState::Live
        })
        .await;
    let started = std::time::Instant::now();
    for n in 0..120 {
        cy.send(
            buzz_sdk::build_message(channel, &format!("chat {n}"), None, &[], false, &[], &[])
                .unwrap(),
        )
        .await;
    }
    eprintln!(
        "the relay took the agent's 120 messages in {} ms",
        started.elapsed().as_millis()
    );
    let total = cy.service.channel_total(channel).await.unwrap();
    assert!(cy_pane.session.paste("1"));
    let screen = cy_pane
        .until("the relay's refusal", |screen, _| {
            !screen.starts_with("stored 0\ntold to wait 0\n") || !screen.contains("other 0")
        })
        .await;
    assert!(
        screen.starts_with("stored 0\ntold to wait 1 (1 to 60 s)\nother 0"),
        "{screen}"
    );
    let status = cy_pane.status();
    let notice = status.notice.clone().expect("the pane says the wait");
    assert!(
        notice.starts_with("Too many changes: this app can write again in "),
        "{notice}"
    );
    let wait = status.wait.expect("a wait");
    assert!((1..=60).contains(&wait), "{wait}");
    eprintln!("at the relay's own quota the pane says: {notice}");
    // While the wait lasts the next write is answered by bzz, at once.
    let again = std::time::Instant::now();
    assert!(cy_pane.session.paste("1"));
    cy_pane
        .until("the second wait", |screen, _| {
            screen.contains("told to wait 2")
        })
        .await;
    assert!(again.elapsed() < Duration::from_secs(2));
    assert_eq!(cy.service.channel_total(channel).await.unwrap(), total);
    // The pane counts the wait down.
    tokio::time::sleep(Duration::from_millis(2100)).await;
    cy_pane.session.tick();
    let later = cy_pane.status().wait.unwrap_or(0);
    assert!(later < wait, "{later} after {wait}");

    drop((ana_pane, ben_pane, dan_pane, cy_pane));
    for profile in [ana, ben, dan, cy] {
        profile.stop().await;
    }
}

/// v0.16 M2: a person's own quota at the relay, sixty writes a minute
/// shared between chat and apps. The suite raises that quota for its other
/// tests, so this one runs only when the relay was started with its own:
/// `BZZ_RELAY_OWN_QUOTA=1 ./scripts/test-relay.sh`.
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires BZZ_RELAY_OWN_QUOTA=1 scripts/test-relay.sh"]
async fn real_relay_app_session_meets_a_persons_own_quota() {
    use bzz::service::records::SyncState;
    use support::app_pane::{Pane, last_row};

    const BOARD: &str = include_str!("fixtures/apps/board/app.lua");
    if std::env::var("BZZ_RELAY_OWN_QUOTA").is_err() {
        eprintln!("skipped: the relay of this run has a raised quota for people");
        return;
    }
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let folder = tempfile::tempdir().unwrap();
    let eva = RecordsProfile::person(&source, folder.path(), "eva", "member").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let channel = uuid::Uuid::new_v4();
    eva.send(
        buzz_sdk::build_create_channel(
            channel,
            &format!("quota-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut pane = Pane::open(&eva.service, channel, "board", BOARD);
    pane.settled("Loaded 0 cards.").await;
    pane.key("n");
    pane.type_text("One card");
    pane.key("enter");
    pane.settled("Saved.").await;

    // Her chat spends the rest of her minute: the channel's creation and
    // the card were two of the sixty. The relay also takes no more than
    // fifty frames in five seconds, so she types at eight a second.
    let started = std::time::Instant::now();
    let mut sent = 2;
    let wait = loop {
        let event = eva
            .signer
            .sign(
                buzz_sdk::build_message(
                    channel,
                    &format!("chat {sent}"),
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
        let ack = eva.supervisor.publish(event).await.unwrap();
        if !ack.accepted {
            break ack.message;
        }
        sent += 1;
        assert!(sent <= 200, "the relay's quota was never met");
        tokio::time::sleep(Duration::from_millis(125)).await;
    };
    eprintln!(
        "the relay took {sent} writes of the person in {} s, then said: {wait}",
        started.elapsed().as_secs()
    );
    assert_eq!(sent, 60, "a person's own quota at the pin");
    assert!(wait.starts_with("rate-limited:"), "{wait}");

    // Her app's next write is answered with the wait, and the pane shows
    // it: in the app's own words and in bzz's.
    pane.key("]");
    let screen = pane
        .until("the wait on the screen", |screen, _| {
            last_row(screen).starts_with("Too many changes. Retry in ")
        })
        .await;
    assert!(screen.contains("To do (1)"), "the card is back:\n{screen}");
    let status = pane.status();
    let notice = status.notice.clone().expect("the pane says the wait");
    assert!(
        notice.starts_with("Too many changes: this app can write again in ")
            && status
                .wait
                .is_some_and(|seconds| (1..=60).contains(&seconds)),
        "{status:?}"
    );
    assert_eq!(status.sync, SyncState::Live);
    eprintln!(
        "at a person's own quota the app says \"{}\" and the pane: {notice}",
        last_row(&screen)
    );
    assert_eq!(eva.service.channel_total(channel).await.unwrap(), 1);
    drop(pane);
    eva.stop().await;
}

/// v0.16 M7, the exit gate on the pinned relay: an agent with the `bot`
/// role in a private channel where its owner opened the Rota takes a day
/// when it is mentioned, through its tools, signed by bzz with the agent's
/// key; another profile's Rota shows it. The relay runs with
/// `BUZZ_REQUIRE_RELAY_MEMBERSHIP` as the wrapper was started: off by
/// default; with it `true`, the agent is first shown to be refused at the
/// relay's door, then made a relay member.
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_agent_takes_a_day_in_the_rota_with_its_bot_role() {
    use bzz::{
        agents::runtime::{Limits, service},
        apps::store::{Apps, Proposer, Recorded, Submission},
        auth::IdentityManager,
        broker::custody,
        config::{Harness, LocalAgentConfig},
        diagnostics::DiagnosticHandle,
        paths::Paths,
        service::records::SyncState,
    };
    use serde_json::json;
    use support::{agent_harness::Script, app_pane::Pane};

    const ROTA: &str = include_str!("fixtures/apps/rota/app.lua");
    const BRIEF: &str = include_str!("fixtures/apps/rota/brief.md");
    const WAIT: Duration = Duration::from_secs(30);
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let closed = std::env::var("BUZZ_REQUIRE_RELAY_MEMBERSHIP").is_ok_and(|value| value == "true");
    eprintln!(
        "the relay requires membership: {closed} (BUZZ_REQUIRE_RELAY_MEMBERSHIP={:?})",
        std::env::var("BUZZ_REQUIRE_RELAY_MEMBERSHIP").ok()
    );
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = "m7 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    let cli = |args: &[&str]| {
        let output = bzz_on_profile(dirs, args, None).output().unwrap();
        assert!(
            output.status.success(),
            "bzz {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    // The owner's profile, made by bzz, with an agent whose key bzz holds.
    cli(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    let community = config
        .add_community("m7".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    cli(&["broker", "create", "Scout", "--yes"]);
    let config = Config::load(&paths).unwrap();
    let mut agent = config.hosted_agents[0].clone();
    let agent_hex = agent.pubkey.clone();
    let _ = seed_member(&source, &identity.pubkey);
    let bo = RecordsProfile::person(&source, dirs, "bo", "member").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let owner = IdentityManager::new(&paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    let ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let http_base = url::Url::parse("http://localhost:3030/").unwrap();
    let owner_supervisor = SupervisorHandle::spawn(ws.clone(), owner.clone());
    let owner_http = HttpClient::new(http_base.clone(), owner.clone()).unwrap();
    let publish = |builder: EventBuilder| {
        let (owner, supervisor) = (owner.clone(), owner_supervisor.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    // A private channel: the agent writes there because it was added, with
    // the role `bot`; bo is a plain member.
    let channel = uuid::Uuid::new_v4();
    publish(
        buzz_sdk::build_create_channel(
            channel,
            &format!("m7-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    publish(buzz_sdk::build_add_member(channel, &bo.pubkey(), None).unwrap()).await;
    publish(
        buzz_sdk::build_add_member(channel, &agent_hex, Some(buzz_sdk::MemberRole::Bot)).unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The owner's bzz: the Rota installed, approved, open in the channel.
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    {
        let mut apps = Apps::new(&mut store, &paths);
        let Recorded::Waiting { version, hash, .. } = apps
            .propose(
                community,
                Submission::new("rota", ROTA, BRIEF, Proposer::owner()),
            )
            .unwrap()
        else {
            panic!("the Rota loads");
        };
        apps.approve(community, "rota", version, &hash).unwrap();
        apps.open_in(community, "rota", channel).unwrap();
    }
    let relay_key = bzz::protocol::http::relay_signing_pubkey(&owner_http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    store.pin_relay_pubkey(community, &relay_key).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    // The week, written by the owner.
    let owners = bzz::service::records::RecordsService::new(
        community,
        owner_http.clone(),
        store.clone(),
        bzz::service::records::Writer::new(owner.clone(), owner_supervisor.clone()),
    );
    let (week, mut week_events) = owners.open(channel, "rota", &["day"]).await.unwrap();
    while week.state() != SyncState::Live {
        let _ = tokio::time::timeout(WAIT, week_events.recv())
            .await
            .unwrap();
    }
    for (order, day) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .iter()
        .enumerate()
    {
        let content = json!({"day": day, "holder": null, "order": order + 1});
        week.create("day", content.as_object().unwrap().clone())
            .await
            .unwrap();
    }
    // Another profile's Rota.
    let mut theirs = Pane::open(&bo.service, channel, "rota", ROTA);
    let screen = theirs
        .until("the week on bo's screen", |screen, _| {
            screen.contains("Loaded 7 days.")
        })
        .await;
    assert!(screen.contains("Tue  free"), "{screen}");

    if closed {
        // Not a relay member: the agent's own connection is refused at the
        // door, and so is any write of its.
        let agent_signer = custody::unlock(&paths.keys_dir(), &owner, &agent)
            .await
            .unwrap();
        let agent_supervisor = SupervisorHandle::spawn(ws.clone(), agent_signer.clone());
        let message = agent_signer
            .sign(buzz_sdk::build_message(channel, "hello", None, &[], false, &[], &[]).unwrap())
            .await
            .unwrap();
        let ack =
            tokio::time::timeout(Duration::from_secs(15), agent_supervisor.publish(message)).await;
        eprintln!("an agent that is no relay member, writing: {ack:?}");
        assert!(
            !matches!(&ack, Ok(Ok(ack)) if ack.accepted),
            "the relay took a write of an agent that is no member: {ack:?}"
        );
        agent_supervisor.shutdown().await;
        agent_signer.lock().await;
        let _ = seed_member(&source, &agent_hex);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // The agent, run as the TUI runs it, on the scripted harness.
    let script = Script::new(&dirs.join("harness"));
    script.write(
        json!({"own_host": true}),
        json!([{"match": "take Tuesday", "steps": [
            {"tool": "apps", "keep": "apps"},
            {"tool": "records", "arguments": {"app": "rota", "type": "day"},
             "keep": {"as": "tue", "find": "/records", "where": {"/content/day": "Tue"}}},
            {"tool": "update_record", "arguments": {
                "app": "rota", "id": "$tue/id", "rev": "$tue/rev",
                "content": {"day": "Tue", "holder": "Scout", "order": "$tue/content/order"}}},
            {"tool": "send", "arguments": {"text": "Tuesday is mine.", "final": true}},
        ]}]),
    );
    let mut local = LocalAgentConfig::new(Harness::PiDurable);
    local.command = script.command();
    agent.local = Some(local);
    let running = service::serve(service::ServeOptions {
        paths: &paths,
        store: store.clone(),
        diagnostics: DiagnosticHandle::disabled(),
        community: &config.communities[0],
        agents: &[agent],
        owner: &owner,
        owner_identity: identity.id,
        federated: None,
        limits: Limits {
            stop_grace: Duration::from_millis(500),
            ..Limits::default()
        },
        owner_present: false,
        environment: None,
    })
    .await
    .unwrap()
    .expect("one local agent");
    assert!(running.problems.is_empty(), "{:?}", running.problems);
    let deadline = tokio::time::Instant::now() + WAIT;
    while !running.listening() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the agent's intake never subscribed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let started = std::time::Instant::now();
    let request = publish(
        buzz_sdk::build_message(
            channel,
            "take Tuesday",
            None,
            &[agent_hex.as_str()],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    // Bo's Rota shows the agent's day.
    let screen = theirs
        .until("the agent's day on bo's screen", |screen, _| {
            screen.contains("Tue  Scout")
        })
        .await;
    eprintln!(
        "the agent's day was on bo's screen {} ms after the mention",
        started.elapsed().as_millis()
    );
    assert!(screen.contains("Days: Scout 1"), "{screen}");
    let tuesday = theirs
        .session
        .records()
        .find(|record| record.content["day"] == "Tue")
        .cloned()
        .unwrap();
    assert_eq!(tuesday.author, agent_hex, "signed with the agent's key");
    // And so does the relay's own head, read by the owner.
    let heads = owner_http
        .query_raw(&[bzz::protocol::artifact::head_filter(tuesday.id)])
        .await
        .unwrap();
    assert!(
        heads
            .iter()
            .any(|head| head.pubkey.to_hex() == agent_hex && head.content.contains("\"Scout\"")),
        "{heads:?}"
    );
    // The mention has its one reply.
    let deadline = tokio::time::Instant::now() + WAIT;
    let replies = loop {
        let replies = owner_http
            .query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 50})])
            .await
            .unwrap()
            .into_iter()
            .filter(|event| {
                bzz::protocol::events::event_references(event).contains(&request.id.to_hex())
            })
            .collect::<Vec<_>>();
        if !replies.is_empty() || tokio::time::Instant::now() > deadline {
            break replies;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0].content, "Tuesday is mine.");
    let answered = script.tool_results("update_record");
    assert_eq!(answered.len(), 1);
    assert_ne!(answered[0]["isError"], true, "{answered:?}");

    drop(theirs);
    drop(week);
    running.stop().await;
    for supervisor in [&owner_supervisor, &bo.supervisor] {
        supervisor.shutdown().await;
    }
    owner.lock().await;
}

/// v0.16 M6 on the pinned relay: one profile shares the Board in a channel;
/// another sees the offer with its brief and who signed it, runs nothing of
/// it before approving it, approves it, and both use the same records. A
/// later version with another type list, a revision by a third member, a
/// tampered and an oversized artifact, offers from a guest and from
/// someone outside the channel, the withdrawal, the brief posted once per
/// version, and no key in any artifact.
///
/// Relay mode: the harness's default, `BUZZ_REQUIRE_RELAY_MEMBERSHIP` off;
/// the channel is private. Everyone is a relay member; "outside" is a relay
/// member who is not in the channel.
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_an_app_shared_in_a_channel_runs_elsewhere_only_after_approval() {
    use bzz::apps::share;
    use bzz::apps::store::{
        AppError, Apps, Bundle, Proposer, Recorded, Submission, manifest::Manifest, offers::Told,
        review::TypesChange,
    };
    use bzz::protocol::record::Change;
    use bzz::service::messages::MessageService;
    use bzz::service::offers::{OfferNews, OffersService};
    use bzz::service::records::{RecordEvent, SyncState};
    use nostr::nips::nip19::ToBech32 as _;
    use serde_json::{Value, json};
    use tokio::sync::mpsc::{self, UnboundedReceiver};

    fn offers_of(
        profile: &RecordsProfile,
        root: std::path::PathBuf,
    ) -> (OffersService, UnboundedReceiver<OfferNews>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let service = OffersService::new(
            profile.community,
            profile.http.clone(),
            profile.handle.clone(),
            profile.signer.clone(),
            profile.supervisor.clone(),
            MessageService::new(
                profile.community,
                profile.signer.clone(),
                profile.handle.clone(),
                profile.supervisor.clone(),
            ),
            root,
            std::sync::Arc::new(move |news| {
                let _ = sender.send(news);
            }),
        );
        (service, receiver)
    }
    async fn news(receiver: &mut UnboundedReceiver<OfferNews>) -> OfferNews {
        tokio::time::timeout(Duration::from_secs(30), receiver.recv())
            .await
            .expect("news of offers in time")
            .expect("the offers are watched")
    }
    async fn told_next(receiver: &mut UnboundedReceiver<OfferNews>) -> Vec<Told> {
        loop {
            if let OfferNews::Told { told, .. } = news(receiver).await {
                return told;
            }
        }
    }
    async fn synced(receiver: &mut UnboundedReceiver<OfferNews>) {
        while !matches!(news(receiver).await, OfferNews::Synced { .. }) {}
    }
    async fn quiet(receiver: &mut UnboundedReceiver<OfferNews>, wait: Duration) -> Vec<OfferNews> {
        let mut seen = Vec::new();
        let until = tokio::time::Instant::now() + wait;
        while let Ok(Some(news)) = tokio::time::timeout_at(until, receiver.recv()).await {
            seen.push(news);
        }
        seen
    }
    async fn install(profile: &RecordsProfile, root: std::path::PathBuf, script: String) -> u32 {
        let community = profile.community;
        let brief = bzz::apps::store::templates::find("board").unwrap().brief;
        profile
            .handle
            .call(move |store| {
                let mut apps = Apps::at(store, root);
                let recorded = apps
                    .propose(
                        community,
                        Submission::new("board", script, brief, Proposer::owner()).titled("Board"),
                    )
                    .map_err(bzz::Error::from)?;
                let Recorded::Waiting { version, hash, .. } = recorded else {
                    panic!("{recorded:?}")
                };
                apps.approve(community, "board", version, &hash)
                    .map_err(bzz::Error::from)?;
                Ok(version)
            })
            .await
            .unwrap()
    }
    async fn on_apps<T: Send + 'static>(
        profile: &RecordsProfile,
        root: std::path::PathBuf,
        call: impl FnOnce(&mut Apps<'_>, uuid::Uuid) -> T + Send + 'static,
    ) -> T {
        let community = profile.community;
        profile
            .handle
            .call(move |store| Ok(call(&mut Apps::at(store, root), community)))
            .await
            .unwrap()
    }
    async fn briefs(profile: &RecordsProfile, channel: uuid::Uuid) -> usize {
        tokio::time::sleep(Duration::from_millis(500)).await;
        profile
            .http
            .query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "limit": 100})])
            .await
            .unwrap()
            .iter()
            .filter(|event| event.content.starts_with("I shared the app Board (board"))
            .count()
    }
    fn bundle(script: String, brief: &str, types: &[&str]) -> Bundle {
        let manifest = Manifest {
            name: "board".into(),
            title: "Board".into(),
            api: 1,
            types: types.iter().map(|name| (*name).to_owned()).collect(),
        };
        Bundle {
            script: script.into_bytes(),
            manifest_text: manifest.to_text(),
            manifest,
            brief: brief.into(),
        }
    }
    fn raw_offer(channel: uuid::Uuid, content: String) -> EventBuilder {
        let tags = [
            ["ar", "1"],
            ["d", &uuid::Uuid::new_v4().to_string()],
            ["h", &channel.to_string()],
            ["type", share::OFFER_TYPE],
            ["title", "Board"],
            ["op", "create"],
            [share::APP_TAG, "board"],
        ]
        .map(|parts| nostr::Tag::parse(parts).unwrap());
        EventBuilder::new(Kind::Custom(45_010), content).tags(tags)
    }

    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let folder = tempfile::tempdir().unwrap();
    let ana = RecordsProfile::person(&source, folder.path(), "ana", "member").await;
    let ben = RecordsProfile::person(&source, folder.path(), "ben", "member").await;
    let cara = RecordsProfile::person(&source, folder.path(), "cara", "member").await;
    let guest = RecordsProfile::person(&source, folder.path(), "guest", "member").await;
    let outsider = RecordsProfile::person(&source, folder.path(), "outsider", "member").await;
    let (ana_root, ben_root) = (
        folder.path().join("ana-apps"),
        folder.path().join("ben-apps"),
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let channel = uuid::Uuid::new_v4();
    ana.send(
        buzz_sdk::build_create_channel(
            channel,
            &format!("share-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    for (member, role) in [
        (&ben, None),
        (&cara, None),
        (&guest, Some(buzz_sdk::MemberRole::Guest)),
    ] {
        ana.send(buzz_sdk::build_add_member(channel, &member.pubkey(), role).unwrap())
            .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Ana has the Board, approved; Ben has nothing.
    let template = bzz::apps::store::templates::find("board").unwrap();
    let first = install(&ana, ana_root.clone(), template.script.to_owned()).await;
    let (ana_offers, mut ana_news) = offers_of(&ana, ana_root.clone());
    let (ben_offers, mut ben_news) = offers_of(&ben, ben_root.clone());
    ana_offers.watch(Some((channel, false)));
    ben_offers.watch(Some((channel, false)));
    synced(&mut ana_news).await;
    synced(&mut ben_news).await;

    // 1. Ana shares it; Ben sees the offer, from her, by key, with its brief.
    let shared = ana_offers.share(channel, "board").await.unwrap();
    assert!(
        shared.brief_posted && !shared.revised && !shared.unchanged,
        "{shared:?}"
    );
    assert_eq!(shared.version, first);
    let told = told_next(&mut ben_news).await;
    let [
        Told::Offered {
            name,
            version,
            from,
            replaces: None,
            installed_from: None,
            types_changed: false,
        },
    ] = told.as_slice()
    else {
        panic!("{told:?}")
    };
    assert_eq!(name, "board");
    assert_eq!(from.id.as_deref(), Some(ana.pubkey().as_str()));
    assert_eq!(from.channel, Some(channel));
    let offered = *version;
    // Ana's own offer is offered to nobody on her side.
    assert!(
        quiet(&mut ana_news, Duration::from_millis(500))
            .await
            .iter()
            .all(|news| !matches!(news, OfferNews::Told { .. })),
        "Ana's own offer was offered to her"
    );

    // 2. Before the approval nothing of it runs, at the one function that
    // hands out a script to run.
    let refused = on_apps(&ben, ben_root.clone(), |apps, community| {
        apps.runnable(community, "board").map(|_| ())
    })
    .await
    .unwrap_err();
    assert!(
        matches!(refused, AppError::NothingApproved { waiting: 1, .. }),
        "{refused}"
    );
    let review = on_apps(&ben, ben_root.clone(), move |apps, community| {
        apps.review(community, "board", offered).unwrap()
    })
    .await;
    let text = review.render(|_| "then".into());
    assert!(
        text.contains(&format!("a person of the channel ({})", ana.pubkey())),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "this exact revision ({}…)",
            &shared.revision[..16]
        )),
        "{text}"
    );
    assert!(
        text.contains(template.brief.lines().next().unwrap()),
        "{text}"
    );
    assert_eq!(review.types_change, TypesChange::First);
    let hash = review.hash.clone();
    let approved = on_apps(&ben, ben_root.clone(), move |apps, community| {
        apps.approve(community, "board", offered, &hash).unwrap()
    })
    .await;
    assert_eq!(
        approved.opened_in,
        Some(channel),
        "opened where it was offered"
    );
    let runnable = on_apps(&ben, ben_root.clone(), |apps, community| {
        apps.runnable(community, "board").unwrap()
    })
    .await;
    assert_eq!(runnable.script(), template.script.as_bytes());

    // Both use the same records: a card by one shows for the other.
    let (ana_board, mut ana_events) = ana.service.open(channel, "board", &["card"]).await.unwrap();
    let (_ben_board, mut ben_events) = ben
        .service
        .open(channel, "board", runnable.types())
        .await
        .unwrap();
    for events in [&mut ana_events, &mut ben_events] {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(20), events.recv())
                .await
                .unwrap()
                .unwrap();
            if event == RecordEvent::State(SyncState::Live) {
                break;
            }
        }
    }
    let card = ana_board
        .create(
            "card",
            json!({"title": "shared board", "column": "todo", "order": 1.0})
                .as_object()
                .unwrap()
                .clone(),
        )
        .await
        .unwrap();
    let arrived = loop {
        let event = tokio::time::timeout(Duration::from_secs(20), ben_events.recv())
            .await
            .unwrap()
            .unwrap();
        if let RecordEvent::Changes { changes, .. } = event {
            break changes;
        }
    };
    assert!(
        matches!(arrived.as_slice(), [Change::Upsert(record)] if record.id == card.id && record.author == ana.pubkey()),
        "{arrived:?}"
    );

    // 8. The brief was posted once; sharing the same version again posts
    // nothing and publishes nothing.
    assert_eq!(briefs(&ana, channel).await, 1);
    let again = ana_offers.share(channel, "board").await.unwrap();
    assert!(again.unchanged && !again.brief_posted, "{again:?}");
    assert_eq!(briefs(&ana, channel).await, 1);

    // 3. A later version, with a changed type list, is a new revision and a
    // new offer, marked as such; it is never applied by itself.
    let tagged = template.script.replace(
        "app.types = { \"card\" }",
        "app.types = { \"card\", \"tag\" }",
    );
    assert_ne!(tagged, template.script);
    let second = install(&ana, ana_root.clone(), tagged.clone()).await;
    let shared_again = ana_offers.share(channel, "board").await.unwrap();
    assert!(
        shared_again.revised && shared_again.brief_posted && shared_again.version == second,
        "{shared_again:?}"
    );
    assert_eq!(shared_again.artifact, shared.artifact, "the same artifact");
    assert_eq!(briefs(&ana, channel).await, 2, "once for each version");
    let told = told_next(&mut ben_news).await;
    let [
        Told::Offered {
            version: later,
            types_changed: true,
            installed_from: Some(installed_from),
            ..
        },
    ] = told.as_slice()
    else {
        panic!("{told:?}")
    };
    assert_eq!(installed_from.id.as_deref(), Some(ana.pubkey().as_str()));
    let later = *later;
    let review = on_apps(&ben, ben_root.clone(), move |apps, community| {
        apps.review(community, "board", later).unwrap()
    })
    .await;
    assert_eq!(
        review.types_change,
        TypesChange::Changed {
            added: vec!["tag".into()],
            removed: vec![]
        }
    );
    assert!(
        review
            .render(|_| String::new())
            .contains("CHANGED from the version that runs:")
    );
    let running = on_apps(&ben, ben_root.clone(), |apps, community| {
        apps.runnable(community, "board").unwrap().version()
    })
    .await;
    assert_eq!(
        running, offered,
        "the installed version runs until Ben answers"
    );

    // 6. Cara publishes a new revision of Ana's offer: Ben is shown Cara.
    let head = cara
        .http
        .query_raw(&[bzz::protocol::artifact::head_filter(shared.artifact)])
        .await
        .unwrap()
        .iter()
        .find_map(|event| bzz::protocol::artifact::parse(event).ok())
        .unwrap();
    assert_eq!(head.author, ana.pubkey());
    let caras = bundle(
        format!("{tagged}-- Cara's\n"),
        "Cara's board.\n",
        &["card", "tag"],
    );
    cara.send(share::revise(&head, &caras).unwrap()).await;
    let told = told_next(&mut ben_news).await;
    let [
        Told::Offered {
            version: caras_version,
            from,
            replaces: Some(replaces),
            ..
        },
    ] = told.as_slice()
    else {
        panic!("{told:?}")
    };
    assert_eq!(from.id.as_deref(), Some(cara.pubkey().as_str()));
    assert_eq!(replaces.id.as_deref(), Some(ana.pubkey().as_str()));
    let caras_version = *caras_version;
    let review = on_apps(&ben, ben_root.clone(), move |apps, community| {
        apps.review(community, "board", caras_version).unwrap()
    })
    .await;
    let text = review.render(|_| String::new());
    assert!(
        text.contains(&format!("a person of the channel ({})", cara.pubkey()))
            && text.contains("Source:   CHANGED: the version installed here came from"),
        "{text}"
    );
    let waiting = on_apps(&ben, ben_root.clone(), |apps, community| {
        apps.waiting(community).unwrap()
    })
    .await;
    assert_eq!(
        waiting.len(),
        1,
        "Ana's later version no longer waits: {waiting:?}"
    );

    // 4. Tampered and oversized: refused on the sender's side before
    // anything is sent, and on the receiver's when another client sends it.
    let oversized = format!("--{}\n", "x".repeat(130 * 1024));
    let error = share::create(
        channel,
        uuid::Uuid::new_v4(),
        &bundle(oversized.clone(), "Big.", &["card"]),
    )
    .unwrap_err();
    assert!(error.to_string().contains("too large"), "{error}");
    let wire = |script: &str, hash: &str| {
        json!({"v":1,"manifest":{"name":"board","title":"Board","api":1,"types":["card"],
               "script_sha256":hash},"brief":"A board.","script":script})
        .to_string()
    };
    let good_hash = bzz::apps::store::manifest::sha256(template.script.as_bytes());
    cara.send(raw_offer(channel, wire("os.exit()\n", &good_hash)))
        .await;
    let told = told_next(&mut ben_news).await;
    assert!(
        matches!(told.as_slice(), [Told::Refused { why, .. }] if why.contains("changed on the way")),
        "{told:?}"
    );
    let big_hash = bzz::apps::store::manifest::sha256(oversized.as_bytes());
    cara.send(raw_offer(channel, wire(&oversized, &big_hash)))
        .await;
    let told = told_next(&mut ben_news).await;
    assert!(
        matches!(told.as_slice(), [Told::Refused { why, .. }] if why.contains("too large")),
        "{told:?}"
    );

    // 5. Someone who cannot write to the channel: the relay refuses, and
    // nothing arrives.
    for stranger in [&guest, &outsider] {
        let event = stranger
            .signer
            .sign(
                share::create(
                    channel,
                    uuid::Uuid::new_v4(),
                    &bundle(template.script.to_owned(), "Mine.", &["card"]),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let ack = stranger.supervisor.publish(event).await.unwrap();
        assert!(
            !ack.accepted && ack.message.starts_with("restricted:"),
            "{}",
            ack.message
        );
    }
    let heard = quiet(&mut ben_news, Duration::from_secs(2)).await;
    assert!(
        heard
            .iter()
            .all(|news| !matches!(news, OfferNews::Told { .. })),
        "an offer from someone who cannot write arrived: {heard:?}"
    );

    // 7. Ana withdraws: the offer that waits goes (its head was Cara's, so
    // the deletion is told as Ana's of Cara's offer); the installed copy
    // stays.
    let unshared = ana_offers.unshare(channel, "board").await.unwrap();
    assert_eq!(unshared.already_by, None);
    let told = told_next(&mut ben_news).await;
    let [
        Told::Withdrawn {
            signer,
            by: Some(by),
            ..
        },
    ] = told.as_slice()
    else {
        panic!("{told:?}")
    };
    assert_eq!(signer.id.as_deref(), Some(cara.pubkey().as_str()));
    assert_eq!(by.id.as_deref(), Some(ana.pubkey().as_str()));
    let (waiting, running) = on_apps(&ben, ben_root.clone(), |apps, community| {
        (
            apps.waiting(community).unwrap(),
            apps.runnable(community, "board").unwrap().version(),
        )
    })
    .await;
    assert!(waiting.is_empty(), "{waiting:?}");
    assert_eq!(running, offered, "the installed copy stays");

    // 9. No key or credential in any revision of any offer.
    // Every artifact of the channel's offers, then every revision of each.
    let heads = ben
        .http
        .query_raw(&[json!({"artifact": "current", "#h": [channel.to_string()], "#type": [share::OFFER_TYPE], "limit": 200})])
        .await
        .unwrap();
    let mut ids: Vec<uuid::Uuid> = heads
        .iter()
        .filter_map(|event| bzz::protocol::artifact::parse(event).ok())
        .map(|head| head.id)
        .collect();
    ids.push(shared.artifact);
    ids.sort();
    ids.dedup();
    let mut offers: Vec<nostr::Event> = Vec::new();
    for id in ids {
        offers.extend(
            ben.http
                .query_raw(&[json!({"artifact": "history", "#d": [id.to_string()]})])
                .await
                .unwrap(),
        );
    }
    // Ana's create and update, Cara's update, Ana's deletion, the tampered
    // and the oversized offer.
    assert!(offers.len() >= 6, "{} revisions of offers", offers.len());
    for profile in [&ana, &ben, &cara, &guest, &outsider] {
        let secrets = [
            profile.keys.secret_key().to_secret_hex(),
            profile.keys.secret_key().to_bech32().unwrap(),
        ];
        for event in &offers {
            let text = serde_json::to_string(event).unwrap();
            for secret in &secrets {
                assert!(!text.contains(secret.as_str()), "a key in an offer");
            }
            let content: Value = serde_json::from_str(&event.content).unwrap_or(Value::Null);
            assert!(content.get("key").is_none() && content.get("nsec").is_none());
        }
    }

    ana_offers.watch(None);
    ben_offers.watch(None);
    drop((ana_board, ana_events, ben_events));
    for profile in [ana, ben, cara, guest, outsider] {
        profile.stop().await;
    }
}

/// v0.16 M5 on the pinned relay: an agent with the `bot` role in a private
/// channel where its owner opened the Board is asked, by the owner's
/// mention, for the priority and the filter of `board-priority.md`; it
/// proposes a version through its tools; bzz approves it here as the
/// owner's overlay does; the agent is told "applied" at its next turn;
/// each mention has one reply, and nothing of the agent's reached the
/// channel's records. The approved script is written to
/// `$BZZ_AUTHOR_OUT/approved/` for the bench (`scripts/
/// test-agent-authors-app.sh` runs it there).
///
/// `BZZ_AUTHOR_HARNESS`: `scripted` (the default; the scripted harness
/// stands in for the model and proposes `tests/fixtures/apps/
/// board-priority`) or `pi-durable`, `grok`, `codex`, `claude-code`: the
/// owner's real program and sign-in, only when he allowed that run (plan
/// D13).
#[cfg(feature = "apps")]
#[tokio::test]
#[ignore = "requires scripts/test-relay.sh"]
async fn real_relay_an_agent_changes_the_open_board_and_is_told_it_was_applied() {
    use bzz::{
        agents::runtime::{Limits, service},
        apps::store::{Apps, Proposer, ProposerKind, Recorded, Submission},
        auth::IdentityManager,
        config::{Harness, LocalAgentConfig},
        diagnostics::DiagnosticHandle,
        paths::Paths,
        service::records::SyncState,
    };
    use serde_json::json;
    use support::agent_harness::Script;

    const BOARD: &str = include_str!("fixtures/apps/board/app.lua");
    const BOARD_BRIEF: &str = include_str!("fixtures/apps/board/brief.md");
    const BOARD_PRIORITY: &str = include_str!("fixtures/apps/board-priority/app.lua");
    const BOARD_PRIORITY_BRIEF: &str = include_str!("fixtures/apps/board-priority/brief.md");
    const SPEC: &str = include_str!("apps-bench/spec/board-priority.md");
    let harness = std::env::var("BZZ_AUTHOR_HARNESS").unwrap_or_else(|_| "scripted".into());
    let real = match harness.as_str() {
        "scripted" => None,
        "pi-durable" => Some(Harness::PiDurable),
        "grok" => Some(Harness::Grok),
        "codex" => Some(Harness::Codex),
        "claude-code" => Some(Harness::ClaudeCode),
        other => {
            panic!("BZZ_AUTHOR_HARNESS={other}: scripted, pi-durable, grok, codex or claude-code")
        }
    };
    // A real model takes its time; a turn has bzz's own limit.
    let wait = if real.is_some() {
        Duration::from_secs(660)
    } else {
        Duration::from_secs(30)
    };
    eprintln!("the agent's harness: {harness}");
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = "m5 disposable passphrase";
    std::fs::write(dirs.join("pass"), format!("{passphrase}\n")).unwrap();
    let cli = |args: &[&str]| {
        let output = bzz_on_profile(dirs, args, None).output().unwrap();
        assert!(
            output.status.success(),
            "bzz {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    cli(&[
        "identity",
        "new",
        "--label",
        "owner",
        "--backend",
        "encrypted-file",
    ]);
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    let community = config
        .add_community("m5".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    config.save(&paths).unwrap();
    cli(&["broker", "create", "Scout", "--yes"]);
    let config = Config::load(&paths).unwrap();
    let mut agent = config.hosted_agents[0].clone();
    let agent_hex = agent.pubkey.clone();
    let _ = seed_member(&source, &identity.pubkey);
    let _ = seed_member(&source, &agent_hex);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let secret = secrecy::SecretString::from(passphrase.to_owned());
    let owner = IdentityManager::new(&paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    let ws = url::Url::parse("ws://localhost:3030/").unwrap();
    let http_base = url::Url::parse("http://localhost:3030/").unwrap();
    let owner_supervisor = SupervisorHandle::spawn(ws.clone(), owner.clone());
    let owner_http = HttpClient::new(http_base.clone(), owner.clone()).unwrap();
    let publish = |builder: EventBuilder| {
        let (owner, supervisor) = (owner.clone(), owner_supervisor.clone());
        async move {
            let event = owner.sign(builder).await.unwrap();
            let ack = supervisor.publish(event.clone()).await.unwrap();
            assert!(ack.accepted, "{}", ack.message);
            event
        }
    };
    let channel = uuid::Uuid::new_v4();
    publish(
        buzz_sdk::build_create_channel(
            channel,
            &format!("m5-{}", &channel.simple().to_string()[..8]),
            Some(buzz_sdk::Visibility::Private),
            Some(buzz_sdk::ChannelKind::Stream),
            None,
            None,
            false,
        )
        .unwrap(),
    )
    .await;
    publish(
        buzz_sdk::build_add_member(channel, &agent_hex, Some(buzz_sdk::MemberRole::Bot)).unwrap(),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The owner's bzz: the Board installed, approved, open in the channel,
    // with three cards.
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    {
        let mut apps = Apps::new(&mut store, &paths);
        let Recorded::Waiting { version, hash, .. } = apps
            .propose(
                community,
                Submission::new("board", BOARD, BOARD_BRIEF, Proposer::owner()),
            )
            .unwrap()
        else {
            panic!("the Board loads");
        };
        apps.approve(community, "board", version, &hash).unwrap();
        apps.open_in(community, "board", channel).unwrap();
    }
    let relay_key = bzz::protocol::http::relay_signing_pubkey(&owner_http.nip11().await.unwrap())
        .unwrap()
        .to_owned();
    store.pin_relay_pubkey(community, &relay_key).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let owners = bzz::service::records::RecordsService::new(
        community,
        owner_http.clone(),
        store.clone(),
        bzz::service::records::Writer::new(owner.clone(), owner_supervisor.clone()),
    );
    let (cards, mut card_events) = owners.open(channel, "board", &["card"]).await.unwrap();
    while cards.state() != SyncState::Live {
        let _ = tokio::time::timeout(wait, card_events.recv())
            .await
            .unwrap();
    }
    for (order, title) in ["Alpha", "Bravo", "Charlie"].iter().enumerate() {
        let content = json!({"title": title, "column": "todo", "order": order + 1});
        cards
            .create("card", content.as_object().unwrap().clone())
            .await
            .unwrap();
    }
    let heads_before = owner_http
        .query_raw(&[json!({"kinds": [45_010], "#h": [channel.to_string()], "limit": 100})])
        .await
        .unwrap()
        .len();

    // The agent, run as the TUI runs it.
    let script = Script::new(&dirs.join("harness"));
    let mut local = match real {
        None => {
            script.write(
                json!({"own_host": true}),
                json!([
                    {"match": "priority", "steps": [
                        {"tool": "apps", "keep": "apps"},
                        {"tool": "app_guide"},
                        {"tool": "apps", "arguments": {"app": "board"}},
                        {"tool": "try_app", "arguments": {"script": BOARD_PRIORITY, "from_app": "board", "keys": ["j", "2"]}},
                        {"tool": "propose_app", "arguments": {"app": "board", "script": BOARD_PRIORITY, "brief": BOARD_PRIORITY_BRIEF}},
                        {"tool": "send", "arguments": {"text": "Proposed: the Board with priority and filter.", "final": true}},
                    ]},
                    {"match": "is it in", "steps": [{"tool": "send", "arguments": {"text": "It is in.", "final": true}}]},
                ]),
            );
            let mut local = LocalAgentConfig::new(Harness::PiDurable);
            local.command = script.command();
            local
        }
        Some(harness) => {
            let mut local = LocalAgentConfig::new(harness);
            // Pi's own default model may come from a provider this host
            // cannot load (a custom one in Pi's settings): the owner names
            // the model for the run, as `provider/model`.
            if let Ok(wanted) = std::env::var("BZZ_AUTHOR_MODEL") {
                let (provider, model_id) = wanted
                    .split_once('/')
                    .expect("BZZ_AUTHOR_MODEL is provider/model");
                local.model = Some(bzz::config::AgentModelConfig {
                    provider: provider.into(),
                    model_id: model_id.into(),
                    thinking_level: std::env::var("BZZ_AUTHOR_THINKING").ok(),
                });
                eprintln!("the agent's model: {wanted}");
            }
            local
        }
    };
    local.instructions =
        "You are an agent of the owner of this channel. Do what you are asked with your tools."
            .into();
    agent.local = Some(local);
    let agent_id = agent.id;
    let running = service::serve_with_tools(
        service::ServeOptions {
            paths: &paths,
            store: store.clone(),
            diagnostics: DiagnosticHandle::disabled(),
            community: &config.communities[0],
            agents: &[agent],
            owner: &owner,
            owner_identity: identity.id,
            federated: None,
            limits: Limits {
                stop_grace: Duration::from_millis(500),
                ..Limits::default()
            },
            owner_present: false,
            environment: None,
        },
        // A coding harness starts `bzz agent-tools`: bzz, not this test's
        // own program.
        Some(env!("CARGO_BIN_EXE_bzz").into()),
    )
    .await
    .unwrap()
    .expect("one local agent");
    assert!(running.problems.is_empty(), "{:?}", running.problems);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !running.listening() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the agent's intake never subscribed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let replies_to = |request: nostr::Event| {
        let (owner_http, agent_hex) = (owner_http.clone(), agent_hex.clone());
        async move {
            let deadline = tokio::time::Instant::now() + wait;
            loop {
                let replies = owner_http
                    .query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 50})])
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        bzz::protocol::events::event_references(event)
                            .contains(&request.id.to_hex())
                    })
                    .collect::<Vec<_>>();
                if !replies.is_empty() || tokio::time::Instant::now() > deadline {
                    // A second reply would come at once after the first.
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    break owner_http
                        .query_raw(&[json!({"kinds": [9], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 50})])
                        .await
                        .unwrap()
                        .into_iter()
                        .filter(|event| {
                            bzz::protocol::events::event_references(event)
                                .contains(&request.id.to_hex())
                        })
                        .collect::<Vec<_>>();
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    };
    // The owner's request: the change, as the bench's spec writes it.
    let started = std::time::Instant::now();
    let request = publish(
        buzz_sdk::build_message(
            channel,
            &format!(
                "Please add a priority and a filter by priority to the board open in this channel, as below, and propose it to me when it works.\n\n{SPEC}"
            ),
            None,
            &[agent_hex.as_str()],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    // A real model may say what it is about to do, with a `send` that is
    // not final, and go on working: its turn has ended when bzz's inbox
    // says so, not when the first message shows.
    let turn_ended = |request: nostr::Event| {
        let store = store.clone();
        async move {
            let deadline = tokio::time::Instant::now() + wait;
            loop {
                let id = request.id.to_hex();
                let state = store
                    .call(move |store| store.agent_inbox_get(agent_id, &id))
                    .await
                    .unwrap()
                    .map(|row| row.state);
                let ended = matches!(
                    state,
                    Some(
                        bzz::store::agent_runtime::RequestState::Replied
                            | bzz::store::agent_runtime::RequestState::Ended
                            | bzz::store::agent_runtime::RequestState::Failed
                    )
                );
                if ended || tokio::time::Instant::now() > deadline {
                    eprintln!("the turn's state: {state:?}");
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    };
    if real.is_some() {
        turn_ended(request.clone()).await;
    }
    let replies = replies_to(request.clone()).await;
    eprintln!(
        "the agent answered the request after {} s: {:?}",
        started.elapsed().as_secs(),
        replies
            .iter()
            .map(|reply| reply.content.clone())
            .collect::<Vec<_>>()
    );
    if real.is_some() {
        assert!(!replies.is_empty(), "the agent said nothing");
    } else {
        assert_eq!(replies.len(), 1, "one reply to the mention: {replies:?}");
    }

    // What it proposed waits; the Board that runs is the owner's.
    let apps_dir = paths.apps_dir();
    let at = apps_dir.clone();
    let (waiting, runs) = store
        .call(move |store| {
            let apps = Apps::at(store, at);
            Ok((
                apps.waiting(community).unwrap(),
                apps.runnable(community, "board").unwrap().version(),
            ))
        })
        .await
        .unwrap();
    assert_eq!(runs, 1, "nothing of the agent's runs before the approval");
    let proposed = waiting
        .iter()
        .find(|version| version.name == "board")
        .unwrap_or_else(|| panic!("the agent proposed no version of the board: {waiting:?}"))
        .clone();
    assert_eq!(proposed.proposer.kind, ProposerKind::Agent);
    assert_eq!(proposed.proposer.id.as_deref(), Some(agent_hex.as_str()));
    assert_eq!(proposed.proposer.channel, Some(channel));
    assert_eq!(
        proposed
            .proposer
            .asked_by
            .as_ref()
            .map(|asker| asker.id.clone()),
        Some(identity.pubkey.clone()),
        "the owner asked"
    );
    // The owner approves it, as the overlay does: the exact bytes shown.
    let (version, hash) = (proposed.version, proposed.script_sha256.clone());
    let at = apps_dir.clone();
    let bundle = store
        .call(move |store| {
            let mut apps = Apps::at(store, at);
            let review = apps.review(community, "board", version).unwrap();
            eprintln!(
                "{}",
                review
                    .render(|_| String::new())
                    .lines()
                    .take(12)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            apps.approve(community, "board", version, &hash).unwrap();
            Ok(apps.bundle(community, "board", version).unwrap())
        })
        .await
        .unwrap();
    if let Ok(out) = std::env::var("BZZ_AUTHOR_OUT") {
        let folder = std::path::Path::new(&out).join("approved");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("app.lua"), &bundle.script).unwrap();
        std::fs::write(folder.join("brief.md"), &bundle.brief).unwrap();
        std::fs::write(folder.join("app.toml"), &bundle.manifest_text).unwrap();
        eprintln!("the approved version is in {}", folder.display());
    }
    if real.is_none() {
        assert_eq!(bundle.script, BOARD_PRIORITY.as_bytes());
    }
    // The agent's next turn in the channel is told.
    let next = publish(
        buzz_sdk::build_message(
            channel,
            "is it in?",
            None,
            &[agent_hex.as_str()],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    if real.is_some() {
        turn_ended(next.clone()).await;
    }
    let replies = replies_to(next).await;
    eprintln!(
        "the agent answered the second mention: {:?}",
        replies
            .iter()
            .map(|reply| reply.content.clone())
            .collect::<Vec<_>>()
    );
    if real.is_some() {
        assert!(
            !replies.is_empty(),
            "the agent said nothing the second time"
        );
    } else {
        assert_eq!(
            replies.len(),
            1,
            "one reply to the second mention: {replies:?}"
        );
    }
    if real.is_none() {
        let prompts = script
            .received("session/prompt")
            .into_iter()
            .filter_map(|params| {
                params
                    .pointer("/prompt/0/text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        assert!(
            prompts[1].contains(&format!(
                "- your version {version} of the app board: applied."
            )),
            "{}",
            prompts[1]
        );
        assert!(
            !prompts[0].contains("Since your last turn"),
            "{}",
            prompts[0]
        );
    }
    // No record of the agent's reached the channel: a try stays apart.
    let heads = owner_http
        .query_raw(&[json!({"kinds": [45_010], "#h": [channel.to_string()], "authors": [agent_hex], "limit": 100})])
        .await
        .unwrap();
    assert!(heads.is_empty(), "{heads:?}");
    assert!(heads_before >= 3, "{heads_before}");
    running.stop().await;
    drop(cards);
    owner_supervisor.shutdown().await;
    owner.lock().await;
}
