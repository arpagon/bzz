//! Upgrade fixture writer. This file is compiled inside a bzz **v0.15.0**
//! checkout (copied to its `examples/`), never by v0.16.0. It writes a whole
//! v0.15.0 profile with disposable keys, made by v0.15.0's own code:
//!
//! - the settings, with an identity in an encrypted file, a hosted agent of
//!   v0.13 (`broker create`'s code), and two **local agents** made by
//!   `agents::manage::create` (the code of `bzz agent new`) against a relay
//!   that accepts everything: `Scout` on `pi-durable`, with a folder granted
//!   for commands that ask, and `Rover`, moved away;
//! - Scout's **inbox and reply ledger** (migrations 18 and 20): a mention
//!   answered and published, one answered and stored but never published
//!   (bzz stopped between the two), one whose reply the relay refused, one
//!   that ended without a reply; a session and a cursor;
//! - Scout's **activity**: a command allowed, a command refused with a
//!   reason, a git credential given (the approvals of v0.15's M7 and M3);
//! - **artifacts** of the channel: a task assigned to the owner and a note
//!   of a type of another client;
//! - messages, an applied edit, a draft, as the earlier seeds.
//!
//! It records what v0.15.0 itself reads back, so the v0.16.0 test compares
//! against the old code's view.
//!
//! Usage: `cargo run --locked --example seed_v0150 -- <empty directory>`

use async_trait::async_trait;
use bzz::{
    agents::{
        activity,
        manage::{self, NewAgent, OwnerContext, Publisher},
        runtime::relay::Published,
    },
    auth::{IdentityManager, signer::SignerHandle},
    broker::custody,
    config::{
        Config, ExecGrant, FolderGrant, HostedAgentConfig, KeyBackend, LocalAgentState, WakeRule,
    },
    paths::Paths,
    protocol::artifact,
    store::{
        Store,
        agent_runtime::{NewRequest, ReplyOrigin, RequestState},
        models::MessageSearchQuery,
    },
};
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, Kind, Tag, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

/// Disposable: it protects a key generated for this fixture only.
const PASSPHRASE: &str = "upgrade fixture passphrase";

/// A relay that takes every record of an agent's creation.
struct Accepting;

#[async_trait]
impl Publisher for Accepting {
    async fn as_owner(&self, _event: Event) -> Published {
        Published::Accepted
    }
    async fn as_agent(
        &self,
        _agent: &HostedAgentConfig,
        _signer: &SignerHandle,
        _event: Event,
    ) -> Published {
        Published::Accepted
    }
}

#[tokio::main]
async fn main() {
    let directory = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    let paths = Paths {
        config_dir: directory.join("config"),
        data_dir: directory.join("data"),
        cache_dir: directory.join("cache"),
    };
    assert!(!paths.config_dir.exists(), "use an empty directory");
    paths.ensure().unwrap();
    let own = Keys::generate();
    let relay = Keys::generate();
    let author = Keys::generate();
    let own_hex = own.public_key().to_hex();

    // ---- the settings, as v0.15.0 writes them ---------------------------
    let secret = SecretString::from(PASSPHRASE.to_owned());
    let mut config = Config::default();
    let manager = IdentityManager::new(&paths);
    let identity = manager
        .import_keys(
            &mut config,
            "upgrade".into(),
            KeyBackend::EncryptedFile,
            own.clone(),
            Some(&secret),
        )
        .unwrap();
    let community = config
        .add_community(
            "upgrade".into(),
            "wss://upgrade.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    config.save(&paths).unwrap();
    let owner = manager.signer(&identity, Some(&secret)).unwrap();

    // A hosted agent of v0.13, as in the v0.14.0 seed.
    let (mut hosted, _credential) = custody::create(
        &paths.keys_dir(),
        &owner,
        identity.id,
        community,
        "hosted-v013",
    )
    .await
    .unwrap();
    custody::set_allowed(
        &mut hosted,
        &["channel.read".to_owned(), "message.reply".to_owned()],
    )
    .unwrap();
    config.hosted_agents.push(hosted.clone());
    config.save(&paths).unwrap();

    // Two local agents, made by the code of `bzz agent new`.
    let granted = directory.join("granted-work");
    std::fs::create_dir_all(&granted).unwrap();
    let context = OwnerContext {
        paths: &paths,
        owner: &owner,
        owner_identity: identity.id,
        community,
        federated: None,
    };
    let mut scout = NewAgent::named("Scout");
    scout.instructions = "Answer briefly.".into();
    scout.wake = WakeRule::Allowlist;
    scout.wake_allowlist = vec![author.public_key().to_hex()];
    scout.folders = vec![FolderGrant {
        name: "work".into(),
        path: granted.clone(),
        write: true,
        exec: ExecGrant::Ask,
    }];
    scout.network = vec!["github.com".into()];
    let scout = manage::create(&context, &mut config, &scout, &Accepting)
        .await
        .map_err(|failure| failure.error)
        .unwrap()
        .agent;
    let rover = manage::create(&context, &mut config, &NewAgent::named("Rover"), &Accepting)
        .await
        .map_err(|failure| failure.error)
        .unwrap()
        .agent;
    // Rover was moved to another machine: bzz refuses to run it here.
    for agent in &mut config.hosted_agents {
        if agent.id == rover.id {
            agent.local.as_mut().unwrap().state = LocalAgentState::MovedAway;
        }
    }
    config.save(&paths).unwrap();
    let config = Config::load(&paths).unwrap();
    let scout = config
        .hosted_agents
        .iter()
        .find(|agent| agent.id == scout.id)
        .unwrap()
        .clone();

    // ---- the database ----------------------------------------------------
    let database = paths.database_file();
    let mut store = Store::open(&database).unwrap();
    store.sync_config(&config).unwrap();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();

    let stream = Uuid::new_v4();
    let dm = Uuid::new_v4();
    for (channel, is_dm) in [(stream, false), (dm, true)] {
        let mut tags = vec![
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", if is_dm { "DM" } else { "general" }]).unwrap(),
            Tag::parse(["t", if is_dm { "dm" } else { "stream" }]).unwrap(),
            Tag::parse(["closed"]).unwrap(),
        ];
        if is_dm {
            tags.push(Tag::parse(["private"]).unwrap());
            tags.push(Tag::parse(["hidden"]).unwrap());
        } else {
            tags.push(Tag::parse(["public"]).unwrap());
        }
        let metadata = EventBuilder::new(Kind::Custom(39_000), "")
            .tags(tags)
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &metadata).unwrap();
        let roster = EventBuilder::new(Kind::Custom(39_002), "")
            .tags([
                Tag::parse(["d", &channel.to_string()]).unwrap(),
                Tag::parse(["p", &own_hex]).unwrap(),
                Tag::parse(["p", &author.public_key().to_hex()]).unwrap(),
                Tag::parse(["p", &scout.pubkey]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &roster).unwrap();
    }
    let message = |keys: &Keys, channel: Uuid, content: &str, mention: Option<&str>, at: u64| {
        let mut tags = vec![Tag::parse(["h", &channel.to_string()]).unwrap()];
        if let Some(mentioned) = mention {
            tags.push(Tag::parse(["p", mentioned]).unwrap());
        }
        EventBuilder::new(Kind::Custom(9), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    };
    let root = message(&own, stream, "upgrade root", None, 1_000);
    let plain = message(&author, stream, "plain history row", None, 1_003);
    let plain_id = plain.id.to_hex();
    let edit = EventBuilder::new(Kind::Custom(40_003), "plain history row, editedneedle")
        .tags([
            Tag::parse(["h", &stream.to_string()]).unwrap(),
            Tag::parse(["e", &plain_id]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(1_010))
        .sign_with_keys(&author)
        .unwrap();
    for (index, event) in [
        root.clone(),
        message(
            &author,
            stream,
            "upgradeneedle a mention of me",
            Some(&own_hex),
            1_001,
        ),
        message(&author, dm, "private upgradeneedle hello", None, 1_002),
        plain,
        edit,
    ]
    .iter()
    .enumerate()
    {
        assert!(
            store.apply_event(community, event).unwrap(),
            "event {index}"
        );
    }
    let root_id = root.id.to_hex();
    store
        .save_draft(community, stream, Some(&root_id), "unsent upgrade draft")
        .unwrap();

    // ---- Scout's inbox and ledger -------------------------------------
    // Four mentions of Scout, by the person on its list, and what became of
    // each. Its replies are signed with its own key, as v0.15.0 signs them.
    let agent_signer = custody::unlock(&paths.keys_dir(), &owner, &scout)
        .await
        .unwrap();
    let agent_tag = bzz::broker::relay::auth_tag(&scout.auth_tag).unwrap();
    let mut requests = Vec::new();
    for (index, text) in [
        "@Scout answered and published",
        "@Scout answered, stored, never published",
        "@Scout answered, refused by the relay",
        "@Scout ended without a reply",
    ]
    .iter()
    .enumerate()
    {
        let mention = message(&author, stream, text, Some(&scout.pubkey), 1_100 + index as u64);
        assert!(store.apply_event(community, &mention).unwrap());
        let request = mention.id.to_hex();
        let place = format!("{stream}/{request}");
        assert!(
            store
                .agent_inbox_offer(&NewRequest {
                    agent_id: scout.id,
                    request_id: request.clone(),
                    community_id: community,
                    channel_id: stream,
                    place: place.clone(),
                    author: author.public_key().to_hex(),
                    created_at: mention.created_at.as_secs(),
                    event_json: mention.as_json(),
                })
                .unwrap()
        );
        assert!(store.agent_inbox_delivered(scout.id, &request).unwrap());
        store
            .set_agent_session(scout.id, &place, &format!("session-{index}"))
            .unwrap();
        requests.push((mention, request, place));
    }
    let reply_to = |request: &Event, text: &str| {
        EventBuilder::new(Kind::Custom(9), text)
            .tags([
                Tag::parse(["h", &stream.to_string()]).unwrap(),
                Tag::parse(["e", &request.id.to_hex(), "", "reply"]).unwrap(),
                Tag::parse(["p", &request.pubkey.to_hex()]).unwrap(),
                agent_tag.clone(),
            ])
            .custom_created_at(Timestamp::from(request.created_at.as_secs() + 1))
    };
    for (index, (mention, request, _)) in requests.iter().take(3).enumerate() {
        let reply = agent_signer
            .sign(reply_to(mention, &format!("reply {index} of Scout")))
            .await
            .unwrap();
        let claim = store
            .agent_reply_claim(
                scout.id,
                request,
                &reply.id.to_hex(),
                &reply.as_json(),
                ReplyOrigin::Send,
                None,
            )
            .unwrap();
        assert!(claim.fresh);
        match index {
            0 => store.agent_reply_published(scout.id, request).unwrap(),
            1 => {} // bzz stopped before the relay answered
            _ => assert!(
                store
                    .agent_reply_refused(scout.id, request, &reply.id.to_hex(), "reply-refused")
                    .unwrap()
            ),
        }
        if index == 0 {
            // The published reply is on the relay, and so in the history.
            assert!(store.apply_event(community, &reply).unwrap());
        }
    }
    assert!(
        store
            .agent_inbox_settle(scout.id, &requests[3].1, RequestState::Ended, None)
            .unwrap()
    );
    store.advance_agent_cursor(scout.id, 1_103).unwrap();
    agent_signer.lock().await;
    owner.lock().await;

    // ---- Scout's activity: what it asked of its owner ------------------
    for entry in [
        activity::Entry::command("work", "ls -la\necho done", "allowed by you"),
        activity::Entry::command("work", "rm -rf build", "refused by you: not today"),
        activity::Entry::credential("github.com", "given"),
    ] {
        activity::record(&paths.data_dir, scout.id, &entry);
    }

    // ---- artifacts of the channel --------------------------------------
    let artifact_of = |kind: &str, title: &str, extra: Option<[&str; 2]>, content: &str| {
        let id = Uuid::new_v4().to_string();
        let home = stream.to_string();
        let mut tags = vec![
            Tag::parse(["ar", "1"]).unwrap(),
            Tag::parse(["d", &id]).unwrap(),
            Tag::parse(["h", &home]).unwrap(),
            Tag::parse(["type", kind]).unwrap(),
            Tag::parse(["title", title]).unwrap(),
            Tag::parse(["op", "create"]).unwrap(),
        ];
        if let Some(extra) = extra {
            tags.push(Tag::parse(extra).unwrap());
        }
        artifact::parse(
            &EventBuilder::new(Kind::Custom(artifact::KIND_ARTIFACT), content)
                .tags(tags)
                .sign_with_keys(&author)
                .unwrap(),
        )
        .unwrap()
    };
    let task = artifact_of(
        "buzz.task",
        "Upgrade task",
        Some(["assignee", &own_hex]),
        "{}",
    );
    let note = artifact_of(
        "bzz.apple.note",
        "A note of another client",
        None,
        r#"{"text":"kept as it is"}"#,
    );
    store
        .replace_channel_artifacts(community, stream, &[task, note])
        .unwrap();

    // ---- what v0.15.0 itself reads back --------------------------------
    let inbox = store
        .inbox_items(community, &own_hex)
        .unwrap()
        .into_iter()
        .map(|item| json!({"conversation": item.conversation_id, "unread": item.unread_count}))
        .collect::<Vec<_>>();
    let search = |word: &str| {
        store
            .search_messages(
                community,
                &own_hex,
                &MessageSearchQuery {
                    fts_query: format!(r#""{word}"*"#),
                    author: None,
                    channel_id: None,
                    since: None,
                    until: None,
                    limit: 20,
                },
            )
            .unwrap()
            .into_iter()
            .filter_map(|hit| hit.event_id)
            .collect::<Vec<_>>()
    };
    let hits = search("upgradeneedle");
    let edited_hits = search("editedneedle");
    let messages = store
        .messages(community, stream, 100)
        .unwrap()
        .into_iter()
        .map(|message| json!({"id": message.event_id, "text": message.content, "edited": message.edited}))
        .collect::<Vec<_>>();
    let ledger = requests
        .iter()
        .map(|(_, request, place)| {
            let row = store.agent_inbox_get(scout.id, request).unwrap().unwrap();
            let reply = store.agent_reply_get(scout.id, request).unwrap();
            json!({
                "request": request,
                "place": place,
                "state": row.state.as_str(),
                "attempts": row.attempts,
                "effects": row.effects,
                "failure": row.failure,
                "session": store.agent_session(scout.id, place).unwrap(),
                "reply": reply.map(|reply| json!({
                    "event_id": reply.event_id,
                    "event_json": reply.event_json,
                    "origin": reply.origin.as_str(),
                    "published": reply.published,
                    "refused": reply.refused,
                })),
            })
        })
        .collect::<Vec<_>>();
    let unpublished = store
        .agent_replies_unpublished(scout.id, 0)
        .unwrap()
        .into_iter()
        .map(|reply| reply.event_id)
        .collect::<Vec<_>>();
    assert_eq!(unpublished.len(), 1);
    let cursor = store.agent_cursor(scout.id).unwrap();
    let artifacts = store
        .channel_artifacts(community, stream, None)
        .unwrap()
        .into_iter()
        .map(|stored| {
            json!({
                "id": stored.head.id,
                "type": stored.head.artifact_type,
                "title": stored.head.title,
                "head": stored.head.head,
                "content": stored.head.event.content,
            })
        })
        .collect::<Vec<_>>();
    let assigned = store
        .assigned_artifacts(community, &own_hex)
        .unwrap()
        .into_iter()
        .map(|stored| stored.head.id.to_string())
        .collect::<Vec<_>>();
    let activity = activity::recent(&paths.data_dir, scout.id, 50)
        .into_iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(activity.len(), 3);
    let version: u32 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    drop(store);
    let agents = config
        .hosted_agents
        .iter()
        .map(|agent| serde_json::to_value(agent).unwrap())
        .collect::<Vec<Value>>();
    let expected = json!({
        "schema_version": version,
        "passphrase": PASSPHRASE,
        "community": community,
        "identity": identity.id,
        "own": own_hex,
        "author": author.public_key().to_hex(),
        "stream": stream,
        "root": root_id,
        "inbox": inbox,
        "search_hits": hits,
        "edited_hits": edited_hits,
        "messages": messages,
        "draft": "unsent upgrade draft",
        // Every agent of the settings as v0.15.0 saved it: the hosted agent
        // of v0.13, Scout and Rover.
        "agents": agents,
        "hosted": hosted.id,
        "scout": scout.id,
        "scout_pubkey": scout.pubkey,
        "rover": rover.id,
        "ledger": ledger,
        "unpublished": unpublished,
        "cursor": cursor,
        "artifacts": artifacts,
        "assigned": assigned,
        "activity": activity,
    });
    std::fs::write(
        directory.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    println!(
        "v0.15.0 profile at {} (schema {version}; agents Scout, Rover and {})",
        directory.display(),
        hosted.label
    );
}
