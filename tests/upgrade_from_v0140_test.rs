//! Release acceptance 2: a real v0.14.0 profile opened by v0.15.0.
//!
//! `scripts/upgrade-test.sh` builds bzz v0.14.0, lets it write a whole
//! profile (`tests/upgrade/seed_v0140.rs`: settings with a hosted agent of
//! v0.13, the agent's sealed key, and a database with messages), records
//! what v0.14.0 reads back, then runs this test with `BZZ_UPGRADE_FIXTURE`
//! pointing at that directory.
//!
//! What must hold: migrations 18 to 22 run, the backup made before them is a
//! readable database at the old version, nothing v0.14.0 showed is lost or
//! changed, and the hosted agent is as it was: unchanged in the settings,
//! never started by bzz, and still served by the agent broker.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use buzz_sdk::broker::{
    ActionArgs, ActionOutcome, BrokerErrorCode, BrokerRequest, ChannelReadArgs, EventPublished,
    MessagePostArgs, MessageReplyArgs,
};
use bzz::{
    auth::IdentityManager,
    broker::{
        custody,
        host::{ExecError, Executor, Host, HostedAgent},
    },
    config::Config,
    paths::Paths,
    store::{Store, models::MessageSearchQuery},
};
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

fn schema_version(database: &Path) -> u32 {
    rusqlite::Connection::open(database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

/// Stands where the relay would: it answers what the broker let through.
struct Reached;

#[async_trait]
impl Executor for Reached {
    async fn execute(
        &self,
        _agent: &HostedAgent,
        _args: &ActionArgs,
    ) -> Result<ActionOutcome, ExecError> {
        Ok(ActionOutcome::MessageReply(EventPublished {
            event_id: "c".repeat(64),
            kind: 9,
            created_at: 2,
        }))
    }
    fn target_label(&self, _args: &ActionArgs) -> Option<String> {
        None
    }
}

#[tokio::test]
#[ignore = "requires a v0.14.0 fixture from scripts/upgrade-test.sh"]
async fn v0140_profile_upgrades_with_its_hosted_agent_served_and_nothing_lost() {
    let fixture = PathBuf::from(
        std::env::var("BZZ_UPGRADE_FIXTURE")
            .expect("BZZ_UPGRADE_FIXTURE names the v0.14.0 fixture"),
    );
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    // Work on a copy: the fixture stays a pristine v0.14.0 profile.
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
    assert_eq!(before, 17, "v0.14.0 wrote schema 17");

    // ---- the settings: the hosted agent of v0.13 loads unchanged ---------
    let settings_before = std::fs::read_to_string(paths.config_dir.join("config.toml")).unwrap();
    let config = Config::load(&paths).expect("v0.15.0 reads the settings v0.14.0 wrote");
    assert_eq!(config.hosted_agents.len(), 1);
    let agent = config.hosted_agents[0].clone();
    assert_eq!(
        serde_json::to_value(&agent).unwrap(),
        expected["hosted_agent"],
        "the hosted agent is what v0.14.0 saved, field for field"
    );
    assert!(
        agent.local.is_none() && !agent.runs_locally(),
        "a hosted agent of v0.13 is no local agent: bzz never starts a process for it"
    );
    // Saved by v0.15.0, the settings say the same.
    config.save(&paths).unwrap();
    let saved = Config::load(&paths).unwrap();
    assert_eq!(saved, config);
    let settings_after = std::fs::read_to_string(paths.config_dir.join("config.toml")).unwrap();
    assert!(
        !settings_after.contains("[hosted_agents.local]"),
        "no local table is added to a hosted agent:\n{settings_after}"
    );
    assert_eq!(
        settings_before.parse::<toml::Table>().unwrap()["hosted_agents"],
        settings_after.parse::<toml::Table>().unwrap()["hosted_agents"],
        "the hosted agent's entry is written back as it was"
    );

    // ---- the database: migrations 18 to 22, and what v0.14.0 showed -----
    let mut store = Store::open(&database).unwrap();
    store.sync_config(&config).unwrap();
    store.search_integrity().unwrap();
    let community = Uuid::parse_str(expected["community"].as_str().unwrap()).unwrap();
    let stream = Uuid::parse_str(expected["stream"].as_str().unwrap()).unwrap();
    let own = expected["own"].as_str().unwrap();

    let mut inbox = store
        .inbox_items(community, own)
        .unwrap()
        .into_iter()
        .map(|item| json!({"conversation": item.conversation_id, "unread": item.unread_count}))
        .collect::<Vec<_>>();
    let by_text = |left: &Value, right: &Value| left.to_string().cmp(&right.to_string());
    inbox.sort_by(by_text);
    let mut old_inbox = expected["inbox"].as_array().unwrap().clone();
    old_inbox.sort_by(by_text);
    assert!(!old_inbox.is_empty());
    assert_eq!(inbox, old_inbox, "the Inbox survives the upgrade");

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
    assert_eq!(sorted(&expected["edited_hits"]).len(), 1);
    assert_eq!(
        search("editedneedle"),
        sorted(&expected["edited_hits"]),
        "search still finds a message by its edited text"
    );

    // History: every row v0.14.0 showed, with its text and its edit mark.
    let messages = store
        .messages(community, stream, 100)
        .unwrap()
        .into_iter()
        .map(|message| json!({"id": message.event_id, "text": message.content, "edited": message.edited}))
        .collect::<Vec<_>>();
    assert_eq!(
        messages.len(),
        3,
        "the root, the edited row and the agent's message"
    );
    assert_eq!(
        Value::Array(messages),
        expected["messages"],
        "the history is what v0.14.0 showed, edits applied"
    );
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
    assert_eq!(Value::Array(channels), expected["channels"]);
    assert_eq!(
        store
            .draft(community, stream, Some(expected["root"].as_str().unwrap()))
            .unwrap(),
        expected["draft"].as_str().unwrap(),
        "the unsent draft is still there"
    );
    let layout = store.sidebar_layout(community, own).unwrap();
    let sections = layout
        .sections
        .iter()
        .map(|section| json!({"id": section.id, "name": section.name}))
        .collect::<Vec<_>>();
    assert_eq!(Value::Array(sections), expected["sections"]);
    assert_eq!(
        layout.assignments.get(&stream).map(String::as_str),
        expected["stream_section"].as_str(),
        "the synced sidebar section still holds the channel"
    );
    drop(store);

    let upgraded = rusqlite::Connection::open(&database).unwrap();
    let after: u32 = upgraded
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after, 22, "migrations 18 to 22 ran ({before} -> {after})");
    let integrity: String = upgraded
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let by_member: bool = upgraded
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='memberships_by_member')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        by_member,
        "migration 19 made the index of memberships by member"
    );
    // The tables of the agent runtime exist, and hold nothing: no agent of
    // this profile is run by bzz.
    for table in [
        "agent_inbox",
        "agent_replies",
        "agent_sessions",
        "agent_cursors",
    ] {
        let rows: i64 = upgraded
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|error| panic!("{table}: {error}"));
        assert_eq!(rows, 0, "{table}");
    }
    drop(upgraded);
    // The backup made before the migration is a readable database at the
    // old version, with the old history in it.
    // The newest one: the profile may carry a backup v0.14.0 itself made
    // when it created the database, and a folder is listed in no fixed order.
    let backup = std::fs::read_dir(&paths.data_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".bak"))
        // Its name ends in the second it was made, as bzz itself orders them.
        .max_by_key(|path| path.file_name().map(std::ffi::OsStr::to_os_string))
        .expect("a backup made before the migration");
    let backed_up = rusqlite::Connection::open(&backup).unwrap();
    assert_eq!(schema_version(&backup), before);
    let integrity: String = backed_up
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let rows: i64 = backed_up
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert!(rows >= 5, "the backup holds the events: {rows}");
    let no_runtime: bool = backed_up
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM sqlite_master WHERE name='agent_inbox')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(no_runtime, "the backup is the database as v0.14.0 left it");
    drop(backed_up);

    // ---- the hosted agent is still served by the agent broker ------------
    // Its key, sealed by v0.14.0 to the owner, opens with the owner.
    let identity = config
        .identities
        .iter()
        .find(|identity| identity.id == agent.owner_identity)
        .unwrap();
    let passphrase = secrecy::SecretString::from(expected["passphrase"].as_str().unwrap());
    let owner = IdentityManager::new(&paths)
        .signer(identity, Some(&passphrase))
        .expect("the owner's identity unlocks");
    let signer = custody::unlock(&paths.keys_dir(), &owner, &agent)
        .await
        .expect("the agent's sealed key opens with its owner");
    assert_eq!(signer.public_key().to_hex(), agent.pubkey);
    signer.lock().await;
    owner.lock().await;
    // The broker, built from the settings as the TUI builds it, answers the
    // credential v0.14.0 printed, by the policy v0.14.0 saved.
    let host = Host::new(Arc::new(Reached), vec![custody::hosted(&agent)]);
    let server = bzz::broker::http::serve(host, None).await.unwrap();
    let credential = expected["hosted_agent_credential"].as_str().unwrap();
    let ask = |credential: &str, id: &str, args: ActionArgs| {
        let body = BrokerRequest::new(id, args)
            .unwrap()
            .prepare()
            .unwrap()
            .body()
            .to_vec();
        let request = reqwest::Client::new()
            .post(format!("http://{}/v1/action", server.address))
            .header("authorization", format!("Bearer {credential}"))
            .header("content-type", "application/json")
            .body(body);
        async move {
            let answer = request.send().await.unwrap();
            serde_json::from_slice::<buzz_sdk::broker::BrokerResponse>(
                &answer.bytes().await.unwrap(),
            )
            .unwrap()
        }
    };
    let reply = || {
        ActionArgs::MessageReply(MessageReplyArgs {
            channel_id: stream.to_string(),
            reply_to_event_id: expected["root"].as_str().unwrap().to_owned(),
            content: "still here".into(),
            mentions: Vec::new(),
        })
    };
    let served = ask(credential, "u1", reply()).await;
    assert!(
        served.result.outcome().is_some(),
        "an action v0.14.0 allowed is served: {:?}",
        served.result
    );
    let code = |response: &buzz_sdk::broker::BrokerResponse| {
        response.result.error().map(|error| error.code)
    };
    let post = ask(
        credential,
        "u2",
        ActionArgs::MessagePost(MessagePostArgs {
            channel_id: stream.to_string(),
            content: "not allowed".into(),
            mentions: Vec::new(),
        }),
    )
    .await;
    assert_eq!(
        code(&post),
        Some(BrokerErrorCode::Unauthorized),
        "an action v0.14.0 did not allow is still refused"
    );
    let read = ask(
        credential,
        "u3",
        ActionArgs::ChannelRead(ChannelReadArgs {
            channel_id: stream.to_string(),
            root_event_id: None,
            mentions_only: false,
            cursor: None,
            limit: None,
        }),
    )
    .await;
    assert!(read.result.outcome().is_some(), "{:?}", read.result);
    let stranger = ask("bzzb_not-the-credential", "u4", reply()).await;
    assert_eq!(code(&stranger), Some(BrokerErrorCode::Unauthenticated));
    drop(server);

    // The release's own commands on the upgraded profile.
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
    let listed = bzz(&["broker", "list"]);
    assert!(
        listed.contains(&agent.label) && listed.contains(&agent.pubkey),
        "`bzz broker list` still lists it: {listed}"
    );
    assert!(listed.contains("channel.read") && listed.contains("message.reply"));
    let local = bzz(&["agent", "list"]);
    assert!(
        !local.contains(&agent.pubkey),
        "`bzz agent list` is for agents bzz runs: {local}"
    );
    bzz(&["check"]);
}
