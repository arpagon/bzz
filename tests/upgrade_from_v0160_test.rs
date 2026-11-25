//! The upgrade of v0.17.0: a real v0.16.0 profile opened by v0.17.0.
//!
//! `scripts/upgrade-test.sh` (`BZZ_UPGRADE_FROM_VERSION=0160`, the default)
//! builds bzz v0.16.0, lets it write a whole profile with its own code
//! (`tests/upgrade/seed_v0160.rs`: a local agent, the Board installed,
//! approved and open in a channel with a second version waiting, its cards,
//! huddle devices saved under their ALSA names, a theme and a key map of the
//! user's own, messages and a draft), records what v0.16.0 reads back, then
//! runs this test with `BZZ_UPGRADE_FIXTURE` pointing at that directory.
//!
//! What must hold (plan v0.17.0, D11 and rule 19): no migration runs (the
//! schema stays 23 and no backup is made), every table holds what it held,
//! everything v0.16.0 showed is there unchanged, the settings saved again by
//! v0.17.0 are the bytes v0.16.0 wrote (so v0.16.0 can still read them), the
//! user's theme that sets only `[highlight.Border]` loads, with the change
//! the release notes name (unfocused frames are muted now), and the user's
//! key map binds as it did, `u` included.
//!
//! The saved ALSA device names are opened against a sound server only when
//! `BZZ_UPGRADE_AUDIO=1` and `PULSE_SERVER` names a server of a test (a
//! container: `scripts/test-audio-containers.sh serve`), never the
//! session's own.
//!
//! The seed has an installed app and saved huddle devices: this test is of
//! the default build, as the released binary is.

#![cfg(all(feature = "apps", feature = "huddles-audio"))]

use std::path::{Path, PathBuf};

use bzz::{
    apps::store::Apps,
    auth::IdentityManager,
    broker::custody,
    config::Config,
    paths::Paths,
    store::{Store, models::MessageSearchQuery},
    ui::{
        keymap::{self, KeyChord, KeyLookup, KeyMap, KeyScope},
        theme::{self, HighlightGroup},
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use uuid::Uuid;

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

/// Every table and its row count.
fn table_counts(database: &Path) -> Value {
    let connection = rusqlite::Connection::open(database).unwrap();
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
    Value::Object(counts)
}

fn schema_version(database: &Path) -> u32 {
    rusqlite::Connection::open(database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn backups(data_dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(data_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".bak"))
        .collect()
}

/// The keys of the seed, as a Linux terminal sends them, and what v0.17.0
/// does with each.
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
        out.insert(name.into(), json!(found(keymap, scope, &keys)));
    }
    Value::Object(out)
}

fn found(keymap: &KeyMap, scope: KeyScope, keys: &[KeyChord]) -> String {
    match keymap.lookup(scope, keys) {
        KeyLookup::Action(action) => format!("{action:?}"),
        KeyLookup::Pending => "pending".into(),
        KeyLookup::NoMatch => "none".into(),
    }
}

#[tokio::test]
#[ignore = "requires a v0.16.0 fixture from scripts/upgrade-test.sh"]
async fn v0160_profile_opens_without_a_migration_and_loses_nothing() {
    let fixture = PathBuf::from(
        std::env::var("BZZ_UPGRADE_FIXTURE")
            .expect("BZZ_UPGRADE_FIXTURE names the v0.16.0 fixture"),
    );
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    // Work on a copy: the fixture stays a pristine v0.16.0 profile.
    let temporary = tempfile::TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    copy_tree(&fixture.join("config"), &paths.config_dir);
    copy_tree(&fixture.join("data"), &paths.data_dir);
    std::fs::create_dir_all(&paths.cache_dir).unwrap();
    let database = paths.database_file();
    let before = schema_version(&database);
    assert_eq!(
        u64::from(before),
        expected["schema_version"].as_u64().unwrap()
    );
    assert_eq!(before, 23, "v0.16.0 wrote schema 23");
    assert_eq!(
        table_counts(&database),
        expected["table_counts"],
        "the copy is what v0.16.0 left"
    );
    let backups_before = backups(&paths.data_dir);

    // ---- the settings ------------------------------------------------------
    let config = Config::load(&paths).expect("v0.17.0 reads the settings v0.16.0 wrote");
    let agents = config
        .hosted_agents
        .iter()
        .map(|agent| serde_json::to_value(agent).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(agents),
        expected["agents"],
        "the local agent is what v0.16.0 saved, field for field"
    );
    assert_eq!(json!(config.ui.theme), expected["theme_name"]);
    assert_eq!(
        json!(config.ui.huddle_input_device),
        expected["input_device"],
        "the saved microphone is kept as v0.16.0 named it"
    );
    assert_eq!(
        json!(config.ui.huddle_output_device),
        expected["output_device"]
    );
    config.save(&paths).unwrap();
    assert_eq!(
        std::fs::read_to_string(paths.config_file()).unwrap(),
        expected["config_toml"].as_str().unwrap(),
        "saved again by v0.17.0, the settings are the bytes v0.16.0 wrote: v0.16.0 can read them"
    );
    assert_eq!(Config::load(&paths).unwrap(), config);

    // ---- the user's own theme --------------------------------------------
    assert_eq!(
        std::fs::read_to_string(paths.theme_file()).unwrap(),
        expected["theme_toml"].as_str().unwrap()
    );
    let loaded = theme::load(&paths, &config.ui.theme).unwrap();
    assert!(
        loaded.warnings.is_empty(),
        "a theme that sets only [highlight.Border] loads without a warning: {:?}",
        loaded.warnings
    );
    let fg = |group| format!("{:?}", loaded.theme.style(group).fg);
    let old = &expected["theme"];
    assert_eq!(
        json!(fg(HighlightGroup::Border)),
        old["Border"],
        "the user's border colour is still the Border group's"
    );
    assert_eq!(json!(fg(HighlightGroup::FocusBorder)), old["FocusBorder"]);
    // What changed, on purpose (plan L2), and the release notes say: an
    // unfocused frame takes the theme's muted colour, not the border's.
    assert_eq!(old["PaneBorder"], old["Border"], "v0.16.0 coloured them");
    assert_eq!(
        fg(HighlightGroup::PaneBorder),
        fg(HighlightGroup::Muted),
        "v0.17.0 draws an unfocused frame in the muted colour"
    );
    assert_ne!(json!(fg(HighlightGroup::PaneBorder)), old["Border"]);
    assert_eq!(
        fg(HighlightGroup::FocusedPaneBorder),
        fg(HighlightGroup::FocusBorder),
        "the focused frame is still the FocusBorder colour"
    );

    // ---- the user's own keys ----------------------------------------------
    let keymap = KeyMap::load(&paths).expect("v0.17.0 accepts the key map v0.16.0 accepted");
    assert_eq!(
        lookups(&keymap),
        expected["keys"],
        "every key of the user's map does what it did in v0.16.0, u (which v0.17.0 binds by default) included"
    );
    // The same keys as Windows sends them (Shift with a shifted symbol):
    // v0.17.0 takes them in one form on every platform.
    let windows = |code, modifiers| {
        KeyChord::from_event(keymap::normalize_key_event_on(
            KeyEvent::new(code, modifiers),
            true,
        ))
    };
    assert_eq!(
        found(
            &keymap,
            KeyScope::Workspace,
            &[windows(KeyCode::Char('!'), KeyModifiers::SHIFT)]
        ),
        expected["keys"]["workspace !"].as_str().unwrap()
    );
    // And v0.17.0's own new keys are there where the user did not bind them.
    assert_eq!(
        found(
            &KeyMap::builtin(),
            KeyScope::Workspace,
            &[KeyChord::new(KeyCode::Char('u'), KeyModifiers::NONE)]
        ),
        "MarkUnread"
    );
    assert_eq!(
        found(
            &keymap,
            KeyScope::Workspace,
            &[KeyChord::new(KeyCode::Char('d'), KeyModifiers::NONE)]
        ),
        "Delete"
    );

    // ---- the database: no migration, nothing lost --------------------------
    let mut store = Store::open(&database).unwrap();
    store.sync_config(&config).unwrap();
    store.search_integrity().unwrap();
    let community = uuid(&expected["community"]);
    let stream = uuid(&expected["stream"]);
    let own = expected["own"].as_str().unwrap();

    let by_text = |left: &Value, right: &Value| left.to_string().cmp(&right.to_string());
    let mut inbox = store
        .inbox_items(community, own)
        .unwrap()
        .into_iter()
        .map(|item| json!({"conversation": item.conversation_id, "unread": item.unread_count}))
        .collect::<Vec<_>>();
    inbox.sort_by(by_text);
    let mut old_inbox = expected["inbox"].as_array().unwrap().clone();
    old_inbox.sort_by(by_text);
    assert!(!old_inbox.is_empty());
    assert_eq!(inbox, old_inbox, "the Inbox is what v0.16.0 showed");
    let search = |word: &str| {
        let mut hits = store
            .search_messages(
                community,
                own,
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
            .collect::<Vec<_>>();
        hits.sort();
        hits
    };
    let sorted = |value: &Value| {
        let mut list = value
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        list.sort();
        list
    };
    assert_eq!(sorted(&expected["search_hits"]).len(), 2);
    assert_eq!(search("upgradeneedle"), sorted(&expected["search_hits"]));
    assert_eq!(search("editedneedle"), sorted(&expected["edited_hits"]));
    let messages = store
        .messages(community, stream, 100)
        .unwrap()
        .into_iter()
        .map(|message| json!({"id": message.event_id, "text": message.content, "edited": message.edited}))
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(messages),
        expected["messages"],
        "the history is what v0.16.0 showed, the edit included"
    );
    assert_eq!(
        store
            .draft(community, stream, Some(expected["root"].as_str().unwrap()))
            .unwrap(),
        expected["draft"].as_str().unwrap(),
        "the unsent draft is still there"
    );

    // The Board's cards, byte for byte as the relay's sync stored them.
    let card_type = "bzz.app.board.card".to_owned();
    let cards = store
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
    assert_eq!(Value::Array(cards), expected["cards"]);
    let count = store.app_record_count(community, "board").unwrap();
    assert_eq!(json!([count.0, count.1]), expected["record_count"]);

    // The Board: installed, approved, open in #general, the agent's version
    // still waiting and its agent still to be told.
    let apps_expected = &expected["apps"];
    {
        let apps = Apps::new(&mut store, &paths);
        let listed = apps.get(community, "board").unwrap();
        let runnable = apps
            .runnable(community, "board")
            .expect("the approved Board runs, its files the bytes v0.16.0 approved");
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
        let seen = json!({
            "title": listed.title(),
            "enabled": listed.app.enabled,
            "current": listed.app.current,
            "channels": listed.channels,
            "running_hash": runnable.hash(),
            "running_version": runnable.version(),
            "versions": versions,
            "open_in_general": apps.open_in_channel(community, stream).unwrap(),
        });
        assert_eq!(&seen, apps_expected, "the Board is as v0.16.0 left it");
        assert_eq!(apps.waiting(community).unwrap().len(), 1);
        let untold = apps
            .untold(
                community,
                expected["scout_pubkey"].as_str().unwrap(),
                stream,
            )
            .unwrap();
        assert!(
            untold.is_empty(),
            "nothing was decided, so the agent has nothing to be told yet"
        );
    }
    drop(store);

    // No migration ran: the schema, every table's rows, no backup made.
    assert_eq!(
        schema_version(&database),
        23,
        "plan D11: no migration in v0.17.0"
    );
    assert_eq!(
        table_counts(&database),
        expected["table_counts"],
        "every table holds what v0.16.0 left in it"
    );
    assert_eq!(
        backups(&paths.data_dir),
        backups_before,
        "no backup: there was nothing to migrate"
    );
    let integrity: String = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");

    // ---- the agent's key, sealed by v0.16.0, opens with the owner ---------
    let scout = config
        .hosted_agents
        .iter()
        .find(|agent| agent.id == uuid(&expected["scout"]))
        .unwrap();
    let identity = config
        .identities
        .iter()
        .find(|identity| identity.id == scout.owner_identity)
        .unwrap();
    let passphrase = secrecy::SecretString::from(expected["passphrase"].as_str().unwrap());
    let owner = IdentityManager::new(&paths)
        .signer(identity, Some(&passphrase))
        .expect("the owner's identity unlocks");
    let signer = custody::unlock(&paths.keys_dir(), &owner, scout)
        .await
        .expect("Scout's key");
    assert_eq!(signer.public_key().to_hex(), scout.pubkey);
    signer.lock().await;
    owner.lock().await;

    // ---- the saved ALSA names, against a sound server of a test -------------
    #[cfg(all(target_os = "linux", feature = "huddles-audio"))]
    saved_alsa_names_open_the_default_and_say_so_once(&expected);

    // ---- the binary on the upgraded profile ---------------------------------
    let bzz = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_CONFIG_DIR", &paths.config_dir)
            .env("BZZ_DATA_DIR", &paths.data_dir)
            .env("BZZ_CACHE_DIR", &paths.cache_dir)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "bzz {args:?}: {text}");
        text
    };
    let checked = bzz(&["check"]);
    assert!(
        !checked.to_lowercase().contains("warning"),
        "`bzz check` finds nothing wrong with the theme and the key map: {checked}"
    );
    let apps = bzz(&["app", "list"]);
    assert!(
        apps.contains("board") && apps.contains("version 1"),
        "`bzz app list` shows the Board: {apps}"
    );
    let local = bzz(&["agent", "list"]);
    assert!(local.contains("Scout"), "`bzz agent list`: {local}");
    assert_eq!(
        std::fs::read_to_string(paths.config_file()).unwrap(),
        expected["config_toml"].as_str().unwrap(),
        "the commands left the settings as v0.16.0 wrote them"
    );
    assert_eq!(schema_version(&database), 23);
}

/// With a sound server of a test only. v0.16.0 saved ALSA's names; v0.17.0
/// speaks to the sound server, which has no such name: the default devices
/// open, and the join says so once in a run.
#[cfg(all(target_os = "linux", feature = "huddles-audio"))]
fn saved_alsa_names_open_the_default_and_say_so_once(expected: &Value) {
    if std::env::var("BZZ_UPGRADE_AUDIO").as_deref() != Ok("1") {
        eprintln!(
            "the saved ALSA names were not opened: BZZ_UPGRADE_AUDIO=1 and PULSE_SERVER of a test server ask for it"
        );
        return;
    }
    let server = std::env::var("PULSE_SERVER").unwrap_or_default();
    assert!(
        server.starts_with("unix:") && !server.contains("/run/user/"),
        "BZZ_UPGRADE_AUDIO=1 needs PULSE_SERVER naming a test server, not the session's: {server:?}"
    );
    let input = expected["input_device"].as_str().unwrap();
    let output = expected["output_device"].as_str().unwrap();
    let (devices, _capture, _playback) =
        bzz::huddle::audio::open_devices(Some(input), Some(output))
            .expect("the default devices open in place of the saved ones");
    assert_eq!(
        devices.notice().map(str::to_owned),
        Some(format!(
            "saved input \"{input}\" not found; using the default; saved output \"{output}\" not found; using the default"
        )),
        "the one notice"
    );
    drop(devices);
    let (again, _capture, _playback) =
        bzz::huddle::audio::open_devices(Some(input), Some(output)).unwrap();
    assert_eq!(
        again.notice(),
        None,
        "told once in a run, not at every join"
    );
}
