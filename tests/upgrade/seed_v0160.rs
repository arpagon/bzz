//! Upgrade fixture writer. This file is compiled inside a bzz **v0.16.0**
//! checkout (copied to its `examples/`), never by v0.17.0. It writes a whole
//! v0.16.0 profile with disposable keys, made by v0.16.0's own code:
//!
//! - the settings: an identity in an encrypted file, a community, a **local
//!   agent** made by `agents::manage::create` (the code of `bzz agent new`)
//!   against a relay that accepts everything, the theme `nord`, and saved
//!   **huddle devices under their ALSA names**, as v0.16.0's device list
//!   (`cpal` on ALSA) gave them;
//! - a **theme of the user's own** (`theme.toml`) that sets only
//!   `[highlight.Border]`, and a **key map of the user's own**
//!   (`keymap.toml`) that binds `u` and `!` in the workspace and `ctrl-g`
//!   everywhere, and turns `Space o` off;
//! - the database at **schema 23**: messages, an applied edit, a mention, a
//!   draft; the **Board installed and approved** (`bzz app add`'s and
//!   `approve`'s code), **open in `#general`**, a second version proposed
//!   by the agent at a person's request and still waiting, and two of the
//!   Board's cards as the relay's sync stored them.
//!
//! It records what v0.16.0 itself reads back (`expected.json`), so the
//! v0.17.0 test compares against the old code's view, and the exact bytes of
//! the settings it wrote.
//!
//! Usage: `cargo run --locked --example seed_v0160 -- <empty directory>`

use async_trait::async_trait;
use bzz::{
    agents::{
        manage::{self, NewAgent, OwnerContext, Publisher},
        runtime::relay::Published,
    },
    apps::store::{Apps, Recorded, Submission, templates},
    auth::{IdentityManager, signer::SignerHandle},
    config::{Config, HostedAgentConfig, KeyBackend, WakeRule},
    paths::Paths,
    protocol::artifact,
    store::{Store, apps::Proposer, models::MessageSearchQuery},
    ui::{
        keymap::{KeyChord, KeyLookup, KeyMap, KeyScope},
        theme::{self, HighlightGroup},
    },
};
use crossterm::event::{KeyCode, KeyModifiers};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

/// Disposable: it protects a key generated for this fixture only.
const PASSPHRASE: &str = "upgrade fixture passphrase";

/// ALSA names as v0.16.0 listed and saved them (`:huddle input`).
const INPUT_DEVICE: &str = "hw:CARD=PCH,DEV=0";
const OUTPUT_DEVICE: &str = "front:CARD=PCH,DEV=0";

/// A theme of the user's own that colours only the border, as many did.
const USER_THEME: &str = "# my own colours\n[highlight.Border]\nforeground = \"#ff8800\"\n";

/// A key map of the user's own. `u` was free in v0.16.0's workspace, and
/// v0.17.0 gives it a meaning of its own (plan D5): the user's stays.
const USER_KEYMAP: &str = r#"# my own keys
[[binding]]
scope = "workspace"
keys = ["u"]
action = "open-inbox"

[[binding]]
scope = "workspace"
keys = ["!"]
action = "open-help"

[[binding]]
keys = ["ctrl-g"]
action = "channel-switcher"

[[binding]]
scope = "workspace"
keys = ["space", "o"]
disabled = true
"#;

/// A relay that takes every record of an agent's creation.
struct Accepting;

#[async_trait]
impl Publisher for Accepting {
    async fn as_owner(&self, _event: nostr::Event) -> Published {
        Published::Accepted
    }
    async fn as_agent(
        &self,
        _agent: &HostedAgentConfig,
        _signer: &SignerHandle,
        _event: nostr::Event,
    ) -> Published {
        Published::Accepted
    }
}

/// The keys the user's map and v0.16.0's own bind, as typed on Linux, and
/// what v0.16.0 does with each.
fn lookups(keymap: &KeyMap) -> Value {
    let chord = |code, modifiers| KeyChord::new(code, modifiers);
    let none = KeyModifiers::NONE;
    let cases: [(&str, KeyScope, Vec<KeyChord>); 6] = [
        (
            "workspace u",
            KeyScope::Workspace,
            vec![chord(KeyCode::Char('u'), none)],
        ),
        (
            "workspace !",
            KeyScope::Workspace,
            vec![chord(KeyCode::Char('!'), none)],
        ),
        (
            "workspace ctrl-g",
            KeyScope::Workspace,
            vec![chord(KeyCode::Char('g'), KeyModifiers::CONTROL)],
        ),
        (
            "workspace space o",
            KeyScope::Workspace,
            vec![
                chord(KeyCode::Char(' '), none),
                chord(KeyCode::Char('o'), none),
            ],
        ),
        (
            "workspace ?",
            KeyScope::Workspace,
            vec![chord(KeyCode::Char('?'), none)],
        ),
        (
            "inbox u",
            KeyScope::Inbox,
            vec![chord(KeyCode::Char('u'), none)],
        ),
    ];
    let mut out = serde_json::Map::new();
    for (name, scope, keys) in cases {
        let found = match keymap.lookup(scope, &keys) {
            KeyLookup::Action(action) => format!("{action:?}"),
            KeyLookup::Pending => "pending".into(),
            KeyLookup::NoMatch => "none".into(),
        };
        out.insert(name.into(), json!(found));
    }
    Value::Object(out)
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
    let author_hex = author.public_key().to_hex();

    // ---- the settings, as v0.16.0 writes them ---------------------------
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
    config.ui.theme = "nord".into();
    config.ui.huddle_input_device = Some(INPUT_DEVICE.into());
    config.ui.huddle_output_device = Some(OUTPUT_DEVICE.into());
    config.save(&paths).unwrap();
    let owner = manager.signer(&identity, Some(&secret)).unwrap();

    // A local agent, made by the code of `bzz agent new`.
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
    scout.wake_allowlist = vec![author_hex.clone()];
    let scout = manage::create(&context, &mut config, &scout, &Accepting)
        .await
        .map_err(|failure| failure.error)
        .unwrap()
        .agent;
    config.save(&paths).unwrap();
    owner.lock().await;
    let config = Config::load(&paths).unwrap();

    // ---- the user's own theme and keys ----------------------------------
    std::fs::write(paths.theme_file(), USER_THEME).unwrap();
    std::fs::write(paths.keymap_file(), USER_KEYMAP).unwrap();
    let loaded = theme::load(&paths, &config.ui.theme).unwrap();
    assert!(
        loaded.warnings.is_empty(),
        "v0.16.0 loads the theme: {:?}",
        loaded.warnings
    );
    let fg = |group| format!("{:?}", loaded.theme.style(group).fg);
    let theme_seen = json!({
        "Border": fg(HighlightGroup::Border),
        "PaneBorder": fg(HighlightGroup::PaneBorder),
        "FocusedPaneBorder": fg(HighlightGroup::FocusedPaneBorder),
        "FocusBorder": fg(HighlightGroup::FocusBorder),
        "Muted": fg(HighlightGroup::Muted),
    });
    assert_eq!(theme_seen["Border"], json!("Some(Rgb(255, 136, 0))"));
    assert_eq!(
        theme_seen["PaneBorder"], theme_seen["Border"],
        "v0.16.0 drew every unfocused frame in the user's border colour"
    );
    let keymap = KeyMap::load(&paths).expect("v0.16.0 accepts the user's key map");
    let keys_seen = lookups(&keymap);
    assert_eq!(keys_seen["workspace u"], json!("OpenInbox"));

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
                Tag::parse(["p", &author_hex]).unwrap(),
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
        message(
            &author,
            stream,
            "@Scout make the board show priorities",
            Some(&scout.pubkey),
            1_020,
        ),
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

    // ---- the Board, installed, approved and open in #general -------------
    let board = templates::find("board").expect("v0.16.0 ships the Board");
    let mut apps = Apps::new(&mut store, &paths);
    let first = Submission::new(
        "board",
        board.script.as_bytes(),
        board.brief.as_bytes(),
        Proposer::owner(),
    )
    .titled(board.title);
    let (version, hash) = match apps.propose(community, first).unwrap() {
        Recorded::Waiting { version, hash, .. } => (version, hash),
        other => panic!("the Board waits for approval: {other:?}"),
    };
    apps.approve(community, "board", version, &hash).unwrap();
    assert!(apps.open_in(community, "board", stream).unwrap());
    // A second version, from the agent at a person's request, not answered.
    let changed = format!("{}\n-- priorities, asked of Scout\n", board.script);
    let second = Submission::new(
        "board",
        changed.as_bytes(),
        board.brief.as_bytes(),
        Proposer::agent(scout.pubkey.clone(), "Scout")
            .in_channel(stream)
            .asked_by(author_hex.clone(), "Ana"),
    );
    match apps.propose(community, second).unwrap() {
        Recorded::Waiting { version: 2, .. } => {}
        other => panic!("the agent's version waits: {other:?}"),
    }
    let runnable = apps.runnable(community, "board").unwrap();
    let listed = apps.get(community, "board").unwrap();
    let versions = apps
        .versions(community, "board")
        .unwrap()
        .into_iter()
        .map(|version| {
            json!({
                "version": version.version,
                "state": format!("{:?}", version.state),
                "title": version.title,
                "types": version.types,
                "script_sha256": version.script_sha256,
                "brief_sha256": version.brief_sha256,
                "proposer": version.proposer.kind.as_str(),
                "proposer_id": version.proposer.id,
                "asked_by": version.proposer.asked_by.as_ref().map(|asker| asker.id.clone()),
                "told": version.told_at.is_some(),
                "kept": version.kept,
            })
        })
        .collect::<Vec<_>>();
    let apps_seen = json!({
        "title": listed.title(),
        "enabled": listed.app.enabled,
        "current": listed.app.current,
        "channels": listed.channels,
        "running_hash": runnable.hash(),
        "running_version": runnable.version(),
        "versions": versions,
        "open_in_general": apps.open_in_channel(community, stream).unwrap(),
    });
    drop(apps);

    // Two cards of the Board, as the relay's sync stored them; one carries
    // what Lua cannot show (a null, an empty object, seventeen digits).
    let card_type = "bzz.app.board.card".to_owned();
    let card = |title: &str, content: &str| {
        let id = Uuid::new_v4().to_string();
        let home = stream.to_string();
        artifact::parse(
            &EventBuilder::new(Kind::Custom(artifact::KIND_ARTIFACT), content)
                .tags([
                    Tag::parse(["ar", "1"]).unwrap(),
                    Tag::parse(["d", &id]).unwrap(),
                    Tag::parse(["h", &home]).unwrap(),
                    Tag::parse(["type", &card_type]).unwrap(),
                    Tag::parse(["title", title]).unwrap(),
                    Tag::parse(["op", "create"]).unwrap(),
                ])
                .sign_with_keys(&author)
                .unwrap(),
        )
        .unwrap()
    };
    let cards = [
        card(
            "Ship the release",
            r#"{"title":"Ship the release","column":"doing","order":1}"#,
        ),
        card(
            "Odd",
            r#"{"title":"Odd","column":"todo","order":1795537221.1991239,"gone":null,"meta":{}}"#,
        ),
    ];
    store
        .replace_artifact_types(community, stream, std::slice::from_ref(&card_type), &cards)
        .unwrap();
    let stored_cards = store
        .typed_artifacts(community, stream, std::slice::from_ref(&card_type))
        .unwrap()
        .into_iter()
        .map(|stored| {
            json!({
                "id": stored.id,
                "title": stored.title,
                "content": stored.event.content,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(stored_cards.len(), 2);
    let record_count = store.app_record_count(community, "board").unwrap();

    // ---- what v0.16.0 itself reads back --------------------------------
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
    drop(store);

    let connection = rusqlite::Connection::open(&database).unwrap();
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let tables = {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let mut counts = serde_json::Map::new();
    for table in tables {
        let rows: i64 = connection
            .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .unwrap();
        counts.insert(table, json!(rows));
    }
    drop(connection);
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
        "author": author_hex,
        "stream": stream,
        "root": root_id,
        "config_toml": std::fs::read_to_string(paths.config_file()).unwrap(),
        "theme_name": config.ui.theme,
        "input_device": config.ui.huddle_input_device,
        "output_device": config.ui.huddle_output_device,
        "theme_toml": USER_THEME,
        "keymap_toml": USER_KEYMAP,
        "theme": theme_seen,
        "keys": keys_seen,
        "inbox": inbox,
        "search_hits": hits,
        "edited_hits": edited_hits,
        "messages": messages,
        "draft": "unsent upgrade draft",
        "agents": agents,
        "scout": scout.id,
        "scout_pubkey": scout.pubkey,
        "apps": apps_seen,
        "cards": stored_cards,
        "record_count": [record_count.0, record_count.1],
        "table_counts": counts,
    });
    std::fs::write(
        directory.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    println!(
        "v0.16.0 profile at {} (schema {version}; agent Scout; the Board v{} open in #general, v2 waiting)",
        directory.display(),
        runnable.version()
    );
}
