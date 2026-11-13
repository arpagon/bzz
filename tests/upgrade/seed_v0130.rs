//! Upgrade fixture writer. This file is compiled inside a bzz **v0.13.0**
//! checkout (copied to its `examples/`), never by v0.14.0. It writes a real
//! v0.13.0 database with disposable keys and records what v0.13.0 itself
//! reads back, so the v0.14.0 test compares against the old code's view.
//!
//! Beyond the v0.12.1 seed it stores what v0.13.0 held but misread: an edit
//! (kind 40003) it never applied, and a channel whose bare `private` tag it
//! took for public.
//!
//! Usage: `cargo run --locked --example seed_v0130 -- <empty directory>`

use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::{Store, models::MessageSearchQuery},
};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use serde_json::json;
use uuid::Uuid;

fn main() {
    let directory = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("bzz.db");
    assert!(!database.exists(), "use an empty directory");
    let own = Keys::generate();
    let relay = Keys::generate();
    let author = Keys::generate();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "upgrade".into(),
        pubkey: own.public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "upgrade".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "upgrade".into(),
            "wss://upgrade.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let mut store = Store::open(&database).unwrap();
    store.sync_config(&config).unwrap();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();

    let stream = Uuid::new_v4();
    let dm = Uuid::new_v4();
    let secret = Uuid::new_v4();
    let mut secret_metadata = None;
    for (channel, is_dm) in [(stream, false), (dm, true), (secret, false)] {
        let mut tags = vec![
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse([
                "name",
                if is_dm {
                    "DM"
                } else if channel == secret {
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
        } else if channel == secret {
            // Upstream marks a private channel with a bare `private` tag.
            tags.push(Tag::parse(["private"]).unwrap());
        } else {
            tags.push(Tag::parse(["public"]).unwrap());
        }
        let metadata = EventBuilder::new(Kind::Custom(39_000), "")
            .tags(tags)
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &metadata).unwrap();
        if channel == secret {
            secret_metadata = Some(metadata.as_json());
        }
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
    // An edit v0.13.0 stores but never applies.
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
        plain.clone(),
        edit.clone(),
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

    // What v0.12.1 itself reads back.
    let own_hex = own.public_key().to_hex();
    let inbox = store
        .inbox_items(community, &own_hex)
        .unwrap()
        .into_iter()
        .map(|item| json!({"conversation": item.conversation_id, "unread": item.unread_count}))
        .collect::<Vec<_>>();
    let hits = store
        .search_messages(
            community,
            &own_hex,
            &MessageSearchQuery {
                fts_query: r#""upgradeneedle"*"#.into(),
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
        .collect::<Vec<_>>();
    let edited_hits = store
        .search_messages(
            community,
            &own_hex,
            &MessageSearchQuery {
                fts_query: r#""editedneedle"*"#.into(),
                author: None,
                channel_id: None,
                since: None,
                until: None,
                limit: 20,
            },
        )
        .unwrap()
        .len();
    let shown = store.messages(community, stream, 100).unwrap();
    let messages = shown.len();
    let plain_text = shown
        .iter()
        .find(|message| message.event_id == plain_id)
        .map(|message| message.content.clone())
        .expect("v0.13.0 shows the original row");
    let secret_public = store
        .channels(community)
        .unwrap()
        .iter()
        .find(|channel| channel.id == secret)
        .map(|channel| format!("{:?}", channel.visibility))
        .unwrap();
    let version: u32 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    drop(store);
    let expected = json!({
        "schema_version": version,
        "community": community,
        "own": own_hex,
        "stream": stream,
        "dm": dm,
        "root": root_id,
        "inbox": inbox,
        "search_hits": hits,
        "stream_messages": messages,
        "draft": "unsent upgrade draft",
        "plain": plain_id,
        "plain_text_v013": plain_text,
        "edited_text": "plain history row, editedneedle",
        "edited_hits_v013": edited_hits,
        "secret": secret,
        "secret_visibility_v013": secret_public,
        "secret_metadata": secret_metadata.unwrap(),
    });
    std::fs::write(
        directory.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    println!(
        "v0.13.0 database at {} (schema {version})",
        database.display()
    );
}
