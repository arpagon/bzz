//! Release acceptance 2 of v0.16.0: a real v0.15.0 profile opened by v0.16.0.
//!
//! `scripts/upgrade-test.sh` (`BZZ_UPGRADE_FROM_VERSION=0150`, the default)
//! builds bzz v0.15.0, lets it write a whole profile with its own code
//! (`tests/upgrade/seed_v0150.rs`: local agents made by `bzz agent new`'s
//! code, their inbox and reply ledger, what they asked of the owner, a
//! hosted agent of v0.13, artifacts, messages), records what v0.15.0 reads
//! back, then runs this test with `BZZ_UPGRADE_FIXTURE` pointing at that
//! directory.
//!
//! What must hold: migrations 21, 22 and 23 run (schema 20 to 23), the
//! backup made before them is the database as v0.15.0 left it, everything
//! v0.15.0 showed is there unchanged, the agents' replies are not sent
//! twice (the one stored and never published goes out once, byte for byte;
//! the published and the refused ones never), and bzz works with no app.

use std::path::{Path, PathBuf};

use bzz::{
    agents::activity,
    auth::IdentityManager,
    broker::custody,
    config::{Config, LocalAgentState},
    paths::Paths,
    store::{Store, models::MessageSearchQuery},
};
use serde_json::{Value, json};
use uuid::Uuid;

#[cfg(unix)]
mod support;

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

fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

/// What the ledger holds for one request, in the seed's words.
fn ledger(store: &Store, agent: Uuid, request: &str, place: &str) -> Value {
    let row = store.agent_inbox_get(agent, request).unwrap().unwrap();
    let reply = store.agent_reply_get(agent, request).unwrap();
    json!({
        "request": request,
        "place": place,
        "state": row.state.as_str(),
        "attempts": row.attempts,
        "effects": row.effects,
        "failure": row.failure,
        "session": store.agent_session(agent, place).unwrap(),
        "reply": reply.map(|reply| json!({
            "event_id": reply.event_id,
            "event_json": reply.event_json,
            "origin": reply.origin.as_str(),
            "published": reply.published,
            "refused": reply.refused,
        })),
    })
}

#[tokio::test]
#[ignore = "requires a v0.15.0 fixture from scripts/upgrade-test.sh"]
async fn v0150_profile_upgrades_with_its_agents_ledger_and_artifacts_and_no_app() {
    let fixture = PathBuf::from(
        std::env::var("BZZ_UPGRADE_FIXTURE")
            .expect("BZZ_UPGRADE_FIXTURE names the v0.15.0 fixture"),
    );
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    // Work on a copy: the fixture stays a pristine v0.15.0 profile.
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
    assert_eq!(before, 20, "v0.15.0 wrote schema 20");

    // ---- the settings: every agent loads as v0.15.0 saved it ------------
    let config = Config::load(&paths).expect("v0.16.0 reads the settings v0.15.0 wrote");
    let agents = config
        .hosted_agents
        .iter()
        .map(|agent| serde_json::to_value(agent).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(agents.len(), 3);
    assert_eq!(
        Value::Array(agents),
        expected["agents"],
        "the hosted agent, Scout and Rover are what v0.15.0 saved, field for field"
    );
    config.save(&paths).unwrap();
    assert_eq!(
        Config::load(&paths).unwrap(),
        config,
        "saved by v0.16.0, the same"
    );
    let agent = |key: &str| {
        let id = uuid(&expected[key]);
        config
            .hosted_agents
            .iter()
            .find(|agent| agent.id == id)
            .unwrap()
            .clone()
    };
    let (hosted, scout, rover) = (agent("hosted"), agent("scout"), agent("rover"));
    assert!(hosted.local.is_none(), "a hosted agent of v0.13 stays one");
    assert_eq!(scout.local.as_ref().unwrap().state, LocalAgentState::Active);
    assert_eq!(
        rover.local.as_ref().unwrap().state,
        LocalAgentState::MovedAway,
        "an agent moved away stays moved away"
    );

    // ---- the database: migrations 21 to 23, and what v0.15.0 showed -----
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
        "the history is what v0.15.0 showed, edits and the agent's reply included"
    );
    assert_eq!(
        store
            .draft(community, stream, Some(expected["root"].as_str().unwrap()))
            .unwrap(),
        expected["draft"].as_str().unwrap(),
        "the unsent draft is still there"
    );

    // The agents' inbox and reply ledger, request by request.
    for row in expected["ledger"].as_array().unwrap() {
        let request = row["request"].as_str().unwrap();
        let place = row["place"].as_str().unwrap();
        assert_eq!(
            &ledger(&store, scout.id, request, place),
            row,
            "the ledger of request {request} is what v0.15.0 held"
        );
    }
    let unpublished = store
        .agent_replies_unpublished(scout.id, 0)
        .unwrap()
        .into_iter()
        .map(|reply| json!(reply.event_id))
        .collect::<Vec<_>>();
    assert_eq!(Value::Array(unpublished), expected["unpublished"]);
    assert_eq!(
        store.agent_cursor(scout.id).unwrap(),
        expected["cursor"].as_u64()
    );

    // The channel's artifacts, and the task assigned to the owner.
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
    assert_eq!(
        Value::Array(artifacts),
        expected["artifacts"],
        "the artifact panel lists what v0.15.0 listed"
    );
    let assigned = store
        .assigned_artifacts(community, own)
        .unwrap()
        .into_iter()
        .map(|stored| json!(stored.head.id.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(Value::Array(assigned), expected["assigned"], "My tasks");
    drop(store);

    // What the agent asked of its owner, and was answered.
    let activity = activity::recent(&paths.data_dir, scout.id, 50)
        .into_iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(activity),
        expected["activity"],
        "the approvals and the credential given are in the activity as v0.15.0 wrote them"
    );

    let upgraded = rusqlite::Connection::open(&database).unwrap();
    let after: u32 = upgraded
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after, 23, "migrations 21 to 23 ran ({before} -> {after})");
    let integrity: String = upgraded
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    // The tables of apps exist and hold nothing: no app was installed.
    for table in [
        "apps",
        "app_versions",
        "app_channels",
        "app_shares",
        "app_offers",
    ] {
        let rows: i64 = upgraded
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|error| panic!("{table}: {error}"));
        assert_eq!(rows, 0, "{table}");
    }
    let asked_by: bool = upgraded
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_versions') WHERE name='asked_by_id')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        asked_by,
        "migration 23 added who asked an agent for a version"
    );
    drop(upgraded);
    // The backup made before the migrations is the database as v0.15.0 left
    // it: the newest backup, at schema 20, without the tables of apps.
    let backup = std::fs::read_dir(&paths.data_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".bak"))
        .max_by_key(|path| path.file_name().map(std::ffi::OsStr::to_os_string))
        .expect("a backup made before the migration");
    assert_eq!(schema_version(&backup), before);
    let backed_up = rusqlite::Connection::open(&backup).unwrap();
    let integrity: String = backed_up
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let (events, replies, no_apps): (i64, i64, bool) = backed_up
        .query_row(
            "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM agent_replies),NOT EXISTS(SELECT 1 FROM sqlite_master WHERE name='apps')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(events >= 9, "the backup holds the events: {events}");
    assert_eq!(replies, 3, "and the ledger");
    assert!(no_apps, "the backup is the database as v0.15.0 left it");
    drop(backed_up);

    // ---- the agents' keys, sealed by v0.15.0, open with the owner --------
    let identity = config
        .identities
        .iter()
        .find(|identity| identity.id == scout.owner_identity)
        .unwrap();
    let passphrase = secrecy::SecretString::from(expected["passphrase"].as_str().unwrap());
    let owner = IdentityManager::new(&paths)
        .signer(identity, Some(&passphrase))
        .expect("the owner's identity unlocks");
    for agent in [&hosted, &scout, &rover] {
        let signer = custody::unlock(&paths.keys_dir(), &owner, agent)
            .await
            .unwrap_or_else(|error| panic!("{}'s key: {error}", agent.label));
        assert_eq!(signer.public_key().to_hex(), agent.pubkey);
        signer.lock().await;
    }
    owner.lock().await;

    // ---- no reply is sent twice ------------------------------------------
    #[cfg(unix)]
    replies_are_not_sent_twice(&paths, &expected, &config).await;

    // ---- bzz works with no app ---------------------------------------------
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
    let apps = bzz(&["app", "list"]);
    assert!(apps.contains("no app"), "`bzz app list`: {apps}");
    let local = bzz(&["agent", "list"]);
    assert!(
        local.contains("Scout") && local.contains("Rover") && !local.contains(&hosted.pubkey),
        "`bzz agent list` lists the two local agents: {local}"
    );
    assert!(
        local.contains("moved"),
        "Rover is shown moved away: {local}"
    );
    let shown = bzz(&["agent", "show", "Scout"]);
    for line in [
        "command in /mnt/work · ls -la (and 1 more line(s)) · allowed by you",
        "command in /mnt/work · rm -rf build · refused by you: not today",
        "git credential for github.com · given",
    ] {
        assert!(
            shown.contains(line),
            "`bzz agent show Scout` has `{line}`: {shown}"
        );
    }
    let brokered = bzz(&["broker", "list"]);
    assert!(
        brokered.contains(&hosted.pubkey),
        "`bzz broker list` still lists the hosted agent: {brokered}"
    );
    bzz(&["check"]);
}

/// The agent runtime of v0.16.0 on the upgraded ledger: at its start it
/// publishes again the one reply v0.15.0 stored and never saw accepted,
/// exactly as stored, and nothing else; started a second time, nothing.
/// No harness is started for it: nothing waits to be answered.
#[cfg(unix)]
async fn replies_are_not_sent_twice(paths: &Paths, expected: &Value, config: &Config) {
    use bzz::agents::runtime::{
        admission::{ChannelFacts, PlaceKind},
        agent::AgentSpec,
        host::{AgentHost, HostOptions},
    };
    use bzz::store::writer::StoreHandle;
    use nostr::{Event, JsonUtil as _, Keys};
    use support::agent_harness::{RecordedRelay, until};

    let scout_id = uuid(&expected["scout"]);
    let scout = config
        .hosted_agents
        .iter()
        .find(|agent| agent.id == scout_id)
        .unwrap()
        .clone();
    let stream = uuid(&expected["stream"]);
    let unpublished = expected["unpublished"][0].as_str().unwrap().to_owned();
    let stored = expected["ledger"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["reply"]["event_id"] == unpublished.as_str())
        .unwrap()["reply"]["event_json"]
        .as_str()
        .unwrap()
        .to_owned();
    // A program that leaves a mark if anything starts a harness.
    let started = paths.data_dir.join("a-harness-was-started");
    let mut local = scout.local.clone().unwrap();
    local.command = vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("touch '{}'; exit 1", started.display()),
    ];
    let spec = AgentSpec {
        id: scout.id,
        name: scout.label.clone(),
        pubkey: scout.pubkey.clone(),
        owner: expected["own"].as_str().unwrap().to_owned(),
        community: scout.community,
        local,
    };
    // The relay's side: the agent is a member of the channel. It signs
    // nothing here that is looked at; the stored reply is signed already.
    let relay = RecordedRelay::new(
        Keys::generate(),
        bzz::broker::relay::auth_tag(&scout.auth_tag).unwrap(),
    );
    relay.set_channel(ChannelFacts {
        id: stream,
        name: "general".into(),
        kind: PlaceKind::Channel,
        members: vec![
            expected["own"].as_str().unwrap().to_owned(),
            expected["author"].as_str().unwrap().to_owned(),
            scout.pubkey.clone(),
        ],
        read_only: Vec::new(),
    });
    let store = StoreHandle::spawn(Store::open(paths.database_file()).unwrap()).unwrap();
    let start = || {
        let mut options = HostOptions::new(paths.data_dir.clone(), store.clone());
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        AgentHost::start(options)
    };
    let host = start().await.unwrap();
    for agent in &config.hosted_agents {
        if agent.id != scout.id {
            assert!(
                !host.runnable(agent),
                "{} is not run by bzz: a hosted agent, or one moved away",
                agent.label
            );
        }
    }
    host.add(spec.clone(), relay.clone()).await;
    until(
        "the stored reply to be published again",
        std::time::Duration::from_secs(20),
        || (!relay.calls(9).is_empty()).then_some(()),
    )
    .await;
    // And whatever more a second attempt would add.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let calls = relay.calls(9);
    assert_eq!(calls.len(), 1, "one message published: {calls:?}");
    assert_eq!(calls[0].id.to_hex(), unpublished);
    assert_eq!(
        calls[0],
        Event::from_json(&stored).unwrap(),
        "the reply v0.15.0 stored, unchanged"
    );
    let reply = store
        .call({
            let request = expected["ledger"][1]["request"]
                .as_str()
                .unwrap()
                .to_owned();
            move |store| store.agent_reply_get(scout_id, &request)
        })
        .await
        .unwrap()
        .unwrap();
    assert!(reply.published, "the ledger now says it was accepted");
    host.shutdown().await;

    // bzz started again: nothing is published.
    let again = start().await.unwrap();
    again.add(spec, relay.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(relay.calls(9).len(), 1, "nothing is sent twice");
    again.shutdown().await;
    assert!(
        !started.exists(),
        "no harness was started: nothing waited for an answer"
    );
}
