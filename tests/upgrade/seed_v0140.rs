//! Upgrade fixture writer. This file is compiled inside a bzz **v0.14.0**
//! checkout (copied to its `examples/`), never by v0.15.0. It writes a whole
//! v0.14.0 profile with disposable keys: the settings as v0.14.0 saves them,
//! with an identity in an encrypted file and a **hosted agent of v0.13**
//! (made by v0.14.0's own `broker create` code: a sealed key, an owner
//! authorization, a credential), and a database with messages, an applied
//! edit, a draft and a synced sidebar section. It records what v0.14.0
//! itself reads back, so the v0.15.0 test compares against the old code's
//! view.
//!
//! Usage: `cargo run --locked --example seed_v0140 -- <empty directory>`

use bzz::{
    auth::IdentityManager,
    broker::custody,
    config::{Config, KeyBackend},
    paths::Paths,
    protocol::{sidebar::Lane, sidebar_doc},
    store::{Store, models::MessageSearchQuery},
};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use uuid::Uuid;

/// Disposable: it protects a key generated for this fixture only.
const PASSPHRASE: &str = "upgrade fixture passphrase";

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

    // The settings, as v0.14.0 writes them.
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
    // A hosted agent of v0.13: bzz holds its key for a process the user
    // runs. Made as `bzz broker create` makes one, then given one write.
    let owner = manager.signer(&identity, Some(&secret)).unwrap();
    let (mut agent, credential) = custody::create(
        &paths.keys_dir(),
        &owner,
        identity.id,
        community,
        "hosted-v013",
    )
    .await
    .unwrap();
    custody::set_allowed(
        &mut agent,
        &["channel.read".to_owned(), "message.reply".to_owned()],
    )
    .unwrap();
    owner.lock().await;
    config.hosted_agents.push(agent.clone());
    config.save(&paths).unwrap();

    let database = paths.database_file();
    let mut store = Store::open(&database).unwrap();
    store.sync_config(&config).unwrap();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();

    let stream = Uuid::new_v4();
    let dm = Uuid::new_v4();
    let secret_channel = Uuid::new_v4();
    for (channel, is_dm) in [(stream, false), (dm, true), (secret_channel, false)] {
        let mut tags = vec![
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse([
                "name",
                if is_dm {
                    "DM"
                } else if channel == secret_channel {
                    "secret"
                } else {
                    "general"
                },
            ])
            .unwrap(),
            Tag::parse(["t", if is_dm { "dm" } else { "stream" }]).unwrap(),
            Tag::parse(["closed"]).unwrap(),
        ];
        if is_dm {
            tags.push(Tag::parse(["private"]).unwrap());
            tags.push(Tag::parse(["hidden"]).unwrap());
        } else if channel == secret_channel {
            tags.push(Tag::parse(["private"]).unwrap());
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
                Tag::parse(["p", &own.public_key().to_hex()]).unwrap(),
                Tag::parse(["p", &author.public_key().to_hex()]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &roster).unwrap();
    }
    let message =
        |keys: &Keys, channel: Uuid, content: &str, root: Option<&str>, mention: bool, at: u64| {
            let mut tags = vec![Tag::parse(["h", &channel.to_string()]).unwrap()];
            if let Some(root) = root {
                tags.push(Tag::parse(["e", root, "", "reply"]).unwrap());
            }
            if mention {
                tags.push(Tag::parse(["p", &own.public_key().to_hex()]).unwrap());
            }
            EventBuilder::new(Kind::Custom(9), content)
                .tags(tags)
                .custom_created_at(Timestamp::from(at))
                .sign_with_keys(keys)
                .unwrap()
        };
    let root = message(&own, stream, "upgrade root", None, false, 1_000);
    let plain = message(&author, stream, "plain history row", None, false, 1_003);
    let plain_id = plain.id.to_hex();
    // An edit, which v0.14.0 applies.
    let edit = EventBuilder::new(Kind::Custom(40_003), "plain history row, editedneedle")
        .tags([
            Tag::parse(["h", &stream.to_string()]).unwrap(),
            Tag::parse(["e", &plain_id]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(1_010))
        .sign_with_keys(&author)
        .unwrap();
    store.apply_event(community, &root).unwrap();
    let root_id = root.id.to_hex();
    for (index, event) in [
        message(
            &author,
            stream,
            "upgradeneedle thread mention",
            Some(&root_id),
            true,
            1_001,
        ),
        message(
            &author,
            dm,
            "private upgradeneedle hello",
            None,
            false,
            1_002,
        ),
        plain,
        edit,
        // The hosted agent's own message: its history must stay too.
        EventBuilder::new(Kind::Custom(9), "posted by the hosted agent")
            .tags([Tag::parse(["h", &stream.to_string()]).unwrap()])
            .custom_created_at(Timestamp::from(1_020))
            .sign_with_keys(&Keys::generate())
            .unwrap(),
    ]
    .iter()
    .enumerate()
    {
        assert!(
            store.apply_event(community, event).unwrap(),
            "event {index}"
        );
    }
    store
        .save_draft(community, stream, Some(&root_id), "unsent upgrade draft")
        .unwrap();
    // A synced sidebar section holding the stream (v0.14, migration 17).
    let own_hex = own.public_key().to_hex();
    let mut tree = sidebar_doc::Tree::new();
    {
        let stamp = sidebar_doc::Stamp {
            now_ms: 1,
            dev: "0123456789abcdef".into(),
        };
        let mut edit = sidebar_doc::Edit::new(&mut tree, &stamp);
        edit.create_section("s1", "Ops Team", None);
        edit.set(&["a", &stream.to_string()], json!("s1"));
    }
    assert!(
        store
            .record_sidebar_document(
                community,
                &own_hex,
                Lane::Sections,
                &"5".repeat(64),
                30,
                &tree
            )
            .unwrap()
    );

    // What v0.14.0 itself reads back.
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
    let channels = store
        .channels(community)
        .unwrap()
        .into_iter()
        .map(|channel| {
            json!({
                "id": channel.id,
                "name": channel.name,
                "private": channel.visibility == bzz::domain::Visibility::Private,
                "member": channel.is_member,
            })
        })
        .collect::<Vec<_>>();
    let layout = store.sidebar_layout(community, &own_hex).unwrap();
    let sections = layout
        .sections
        .iter()
        .map(|section| json!({"id": section.id, "name": section.name}))
        .collect::<Vec<_>>();
    let assigned = layout.assignments.get(&stream).cloned();
    let version: u32 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    drop(store);
    let expected = json!({
        "schema_version": version,
        "passphrase": PASSPHRASE,
        "community": community,
        "identity": identity.id,
        "own": own_hex,
        "stream": stream,
        "root": root_id,
        "inbox": inbox,
        "search_hits": hits,
        "edited_hits": edited_hits,
        "messages": messages,
        "channels": channels,
        "draft": "unsent upgrade draft",
        "sections": sections,
        "stream_section": assigned,
        // The hosted agent as v0.14.0 holds it in its settings, and the
        // credential it printed once (disposable, like every key here).
        "hosted_agent": serde_json::to_value(&agent).unwrap(),
        "hosted_agent_credential": credential,
    });
    std::fs::write(
        directory.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    println!(
        "v0.14.0 profile at {} (schema {version}, hosted agent {})",
        directory.display(),
        agent.label
    );
}
