//! v0.16 M7: an agent uses an app's records, without a screen and without
//! the app's script. The scripted harness (`examples/scripted_harness.rs`)
//! stands in for the model, through the pipe as bzz's own host and through
//! `bzz agent-tools` as a coding harness; a recorded relay carries its chat;
//! a scripted relay that keeps records (`support::artifact_relay`) carries
//! its records, its owner's and another profile's, on real sockets. The
//! fence, the signer, the quota and the answers are bzz's own code.
#![cfg(all(unix, feature = "apps"))]

mod support;

use std::{sync::Arc, time::Duration};

use bzz::{
    agents::{
        activity,
        runtime::{
            Limits,
            admission::{ChannelFacts, PlaceKind},
            agent::AgentSpec,
            host::{AgentHost, HostOptions, Offer},
        },
    },
    apps::store::{Apps, Proposer, Recorded, Submission},
    config::{Harness, LocalAgentConfig},
    protocol::record::Fence,
    realtime::supervisor::SupervisorHandle,
    service::records::{RecordsService, SyncState},
    store::{
        Store,
        agent_runtime::{InboxRow, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, Keys};
use serde_json::{Value, json};
use support::{
    agent_harness::{RecordedRelay, Script, until},
    app_pane::Pane,
    artifact_relay::ArtifactRelay,
};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);
const ROTA: &str = include_str!("fixtures/apps/rota/app.lua");
const ROTA_BRIEF: &str = include_str!("fixtures/apps/rota/brief.md");
const BOARD: &str = include_str!("fixtures/apps/board/app.lua");
const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn limits() -> Limits {
    Limits {
        start: Duration::from_secs(5),
        session: Duration::from_secs(5),
        turn: Duration::from_secs(15),
        cancel_grace: Duration::from_millis(400),
        stop_grace: Duration::from_millis(400),
        typing_interval: Duration::from_millis(200),
        restart_backoff: Duration::from_millis(30),
        publish: Duration::from_secs(3),
        checkpoint_idle: Duration::from_secs(600),
        approval: Duration::from_secs(600),
        ..Limits::default()
    }
}

/// The owner's bzz with an agent, the Rota open in a channel, another
/// profile in the same channel, and one relay of records for all three.
struct World {
    temporary: TempDir,
    owner: Keys,
    agent_keys: Keys,
    other: Keys,
    chat: Arc<RecordedRelay>,
    records: ArtifactRelay,
    store: StoreHandle,
    community: Uuid,
    agent: Uuid,
    /// Where the Rota is open.
    channel: Uuid,
    /// Where it is not.
    elsewhere: Uuid,
    script: Script,
    supervisors: Vec<SupervisorHandle>,
}

impl World {
    async fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let (owner, agent_keys, other) = (Keys::generate(), Keys::generate(), Keys::generate());
        let auth_tag = bzz::auth::signer::SignerHandle::spawn(owner.clone())
            .owner_auth_tag(agent_keys.public_key(), "")
            .await
            .unwrap();
        let chat = RecordedRelay::new(
            agent_keys.clone(),
            bzz::broker::relay::auth_tag(&auth_tag).unwrap(),
        );
        let records = ArtifactRelay::start().await;
        let (config, community) = support_config(&owner);
        let mut store = Store::open(temporary.path().join("data").join("bzz.db")).unwrap();
        store.sync_config(&config).unwrap();
        store
            .pin_relay_pubkey(community, &records.keys.public_key().to_hex())
            .unwrap();
        let store = StoreHandle::spawn(store).unwrap();
        let mut world = Self {
            script: Script::new(&temporary.path().join("harness")),
            temporary,
            owner,
            agent_keys,
            other,
            chat,
            records,
            store,
            community,
            agent: Uuid::new_v4(),
            channel: Uuid::new_v4(),
            elsewhere: Uuid::new_v4(),
            supervisors: Vec::new(),
        };
        for channel in [world.channel, world.elsewhere] {
            world.chat.set_channel(ChannelFacts {
                id: channel,
                name: "general".into(),
                kind: PlaceKind::Channel,
                members: vec![
                    world.owner.public_key().to_hex(),
                    world.agent_pubkey(),
                    world.other.public_key().to_hex(),
                ],
                read_only: Vec::new(),
            });
        }
        // The agent's own records service: its key, its connection.
        let (service, supervisor) =
            world.service_for(&world.agent_keys.clone(), world.store.clone());
        world.supervisors.push(supervisor);
        world.chat.set_records(service);
        // The week, written by the owner before the agent was asked.
        let fence = Fence::new("rota", world.channel, &["day"]).unwrap();
        for (order, day) in DAYS.iter().enumerate() {
            let content = json!({"day": day, "holder": null, "order": order + 1});
            world.records.seed(
                &fence
                    .create("day", Uuid::new_v4(), content.as_object().unwrap())
                    .unwrap()
                    .sign_with_keys(&world.owner)
                    .unwrap(),
            );
        }
        world
    }

    fn service_for(&self, keys: &Keys, store: StoreHandle) -> (RecordsService, SupervisorHandle) {
        let (service, supervisor, _signer) = self.records.service(keys, self.community, store);
        (service, supervisor)
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.temporary.path().join("data")
    }

    fn agent_pubkey(&self) -> String {
        self.agent_keys.public_key().to_hex()
    }

    /// Installs `name` from `script` in the owner's bzz and approves it, as
    /// `bzz app add` and `bzz app approve` do.
    async fn install(&self, name: &'static str, script: &'static str) {
        let (community, root) = (self.community, self.data_dir().join("apps"));
        self.store
            .call(move |store| {
                let mut apps = Apps::at(store, root);
                let recorded = apps
                    .propose(
                        community,
                        Submission::new(name, script, ROTA_BRIEF, Proposer::owner()),
                    )
                    .unwrap();
                let Recorded::Waiting { version, hash, .. } = recorded else {
                    panic!("{name} did not load: {recorded:?}");
                };
                apps.approve(community, name, version, &hash).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn open_in(&self, name: &'static str, channel: Uuid) {
        let (community, root) = (self.community, self.data_dir().join("apps"));
        self.store
            .call(move |store| {
                Apps::at(store, root)
                    .open_in(community, name, channel)
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn set_enabled(&self, name: &'static str, enabled: bool) {
        let (community, root) = (self.community, self.data_dir().join("apps"));
        self.store
            .call(move |store| {
                Apps::at(store, root)
                    .set_enabled(community, name, enabled)
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    fn spec(&self, local: LocalAgentConfig) -> AgentSpec {
        AgentSpec {
            id: self.agent,
            name: "Scout".into(),
            pubkey: self.agent_pubkey(),
            owner: self.owner.public_key().to_hex(),
            community: self.community,
            local,
        }
    }

    async fn host(&self, own_host: bool) -> AgentHost {
        let harness = if own_host {
            Harness::PiDurable
        } else {
            Harness::ClaudeCode
        };
        let mut local = LocalAgentConfig::new(harness);
        local.command = self.script.command();
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = limits();
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        let host = AgentHost::start(options).await.unwrap();
        host.add(self.spec(local), self.chat.clone()).await;
        host
    }

    /// A mention of the agent by its owner in `channel`.
    async fn ask(&self, host: &AgentHost, channel: Uuid, text: &str) -> Event {
        let event = buzz_sdk::build_message(
            channel,
            text,
            None,
            &[self.agent_pubkey().as_str()],
            false,
            &[],
            &[],
        )
        .unwrap()
        .sign_with_keys(&self.owner)
        .unwrap();
        self.chat.seed(&event);
        let facts = bzz::agents::runtime::relay::AgentRelay::channel(self.chat.as_ref(), channel)
            .await
            .unwrap();
        assert_eq!(host.offer(self.agent, &event, &facts).await, Offer::Queued);
        event
    }

    async fn row(&self, request: &Event) -> Option<InboxRow> {
        let (agent, id) = (self.agent, request.id.to_hex());
        self.store
            .call(move |store| store.agent_inbox_get(agent, &id))
            .await
            .unwrap()
    }

    async fn settled(&self, request: &Event) -> InboxRow {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            if let Some(row) = self.row(request).await
                && !matches!(row.state, RequestState::Queued | RequestState::Delivered)
            {
                return row;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "request never settled: {:?}\nharness log: {:#?}",
                self.row(request).await,
                self.script.log()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// What the tool `name` answered, each call, as the harness read it.
    fn answers(&self, name: &str) -> Vec<(bool, String)> {
        self.script
            .tool_results(name)
            .into_iter()
            .map(|result| {
                (
                    result["isError"].as_bool().unwrap_or(false),
                    result["text"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    /// The agent's activity, as `bzz agent show` prints it, without the time.
    fn activity(&self) -> Vec<String> {
        activity::recent(&self.data_dir(), self.agent, 50)
            .iter()
            .map(|entry| {
                entry
                    .line()
                    .split_once(" · ")
                    .map(|(_, rest)| rest.to_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    async fn stop(self, host: AgentHost) {
        host.shutdown().await;
        for supervisor in &self.supervisors {
            supervisor.shutdown().await;
        }
    }
}

fn support_config(owner: &Keys) -> (bzz::config::Config, Uuid) {
    let identity = bzz::config::IdentityConfig {
        id: Uuid::new_v4(),
        label: "owner".into(),
        pubkey: owner.public_key().to_hex(),
        backend: bzz::config::KeyBackend::EncryptedFile,
        key_ref: "owner".into(),
    };
    let mut config = bzz::config::Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("m7".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    (config, community)
}

/// The JSON a tool answered: its text, or its last line for a failure.
fn json_of(text: &str) -> Value {
    serde_json::from_str(text.lines().last().unwrap_or(text))
        .unwrap_or_else(|_| panic!("not JSON: {text}"))
}

/// The steps with which an agent takes Tuesday: list the apps, list the
/// days, find Tuesday, and update it with its rev.
fn take_tuesday() -> Value {
    json!([
        {"tool": "apps", "keep": "apps"},
        {"tool": "records", "arguments": {"app": "rota", "type": "day"},
         "keep": {"as": "tue", "find": "/records", "where": {"/content/day": "Tue"}}},
        {"tool": "update_record", "arguments": {
            "app": "rota", "id": "$tue/id", "rev": "$tue/rev",
            "content": {"day": "Tue", "holder": "Scout", "order": "$tue/content/order"}}},
        {"tool": "send", "arguments": {"text": "Tuesday is mine.", "final": true}},
    ])
}

/// The exit gate of M7, scripted: an agent mentioned in a channel where the
/// Rota is open takes a day in it; the owner's open Rota and another
/// profile's show it at once. Through the pipe (bzz's own host) and
/// through `bzz agent-tools` (a coding harness).
#[tokio::test]
async fn an_agent_takes_a_day_in_the_rota_and_another_profile_sees_it() {
    for own_host in [true, false] {
        let mut world = World::new().await;
        world.install("rota", ROTA).await;
        world.open_in("rota", world.channel).await;
        // Another profile, with a bzz and a store of its own.
        let mut other_store = Store::open_memory().unwrap();
        other_store
            .sync_config(&support_config(&world.other).0)
            .unwrap();
        let other_store = StoreHandle::spawn(other_store).unwrap();
        let other_keys = world.other.clone();
        let (other, other_supervisor) = world.service_for(&other_keys, other_store);
        world.supervisors.push(other_supervisor);
        let mut theirs = Pane::open(&other, world.channel, "rota", ROTA);
        // And the owner's own open Rota, beside the agent, on the same store.
        let owner_keys = world.owner.clone();
        let (owners, owner_supervisor) = world.service_for(&owner_keys, world.store.clone());
        world.supervisors.push(owner_supervisor);
        let mut mine = Pane::open(&owners, world.channel, "rota", ROTA);
        for pane in [&mut theirs, &mut mine] {
            let screen = pane
                .until("the week", |screen, _| screen.contains("Loaded 7 days."))
                .await;
            assert!(screen.contains("Tue  free"), "{screen}");
        }

        world.script.write(
            json!({"own_host": own_host}),
            json!([{"match": "take Tuesday", "steps": take_tuesday()}]),
        );
        let host = world.host(own_host).await;
        let request = world.ask(&host, world.channel, "take Tuesday").await;
        let row = world.settled(&request).await;
        assert_eq!(
            row.state,
            RequestState::Replied,
            "{:#?}",
            world.script.log()
        );

        // What the agent was told: the app with its brief and types, the
        // seven days, and the record as stored, signed by itself.
        let apps = json_of(&world.answers("apps")[0].1);
        assert_eq!(apps["apps"][0]["name"], "rota");
        assert_eq!(apps["apps"][0]["types"], json!(["day"]));
        assert!(
            apps["apps"][0]["brief"]
                .as_str()
                .unwrap()
                .contains("Who covers each day")
        );
        let listed = json_of(&world.answers("records")[0].1);
        assert_eq!(listed["count"], 7, "{listed}");
        let (failed, updated) = world.answers("update_record").remove(0);
        assert!(!failed, "{updated}");
        let stored = json_of(&updated)["record"].clone();
        assert_eq!(stored["content"]["holder"], "Scout");
        assert_eq!(
            stored["by"],
            json!({"id": world.agent_pubkey(), "name": "Scout"})
        );

        // On the relay: one revision, signed by the agent's key.
        let writes = world.records.writes_by(&world.agent_pubkey());
        assert_eq!(writes.len(), 1, "own_host {own_host}");
        assert!(writes[0].1, "{}", writes[0].2);
        assert!(
            world
                .records
                .writes_by(&world.owner.public_key().to_hex())
                .is_empty(),
            "nothing was signed with the owner's key"
        );

        // Another profile's Rota shows it, and so does the owner's own.
        for (who, pane) in [("another profile", &mut theirs), ("the owner", &mut mine)] {
            let screen = pane
                .until(who, |screen, _| screen.contains("Tue  Scout"))
                .await;
            assert!(screen.contains("Days: Scout 1"), "{who}: {screen}");
        }
        // One final reply to the mention, and the write in the activity.
        assert_eq!(world.chat.replies_to(&request).len(), 1);
        assert_eq!(
            world.activity(),
            [format!(
                "update a day of the app rota ({}) · updated",
                &stored["id"].as_str().unwrap()[..8]
            )]
        );
        drop((theirs, mine));
        world.stop(host).await;
    }
}

/// Rule 17 for agents: only the declared types, only an app open in the
/// conversation's channel, nothing of another channel, nothing of a
/// disabled app. Each is refused where bzz serves the tool, and no frame
/// reaches the relay.
#[tokio::test]
async fn the_fence_holds_whatever_the_harness_sends() {
    let world = World::new().await;
    world.install("rota", ROTA).await;
    world.install("board", BOARD).await;
    world.open_in("rota", world.channel).await;
    // The board is open, but somewhere else; the Rota too, elsewhere, so a
    // record of this channel's Rota is named from another channel.
    world.open_in("board", world.elsewhere).await;
    world.open_in("rota", world.elsewhere).await;
    let tuesday = {
        let kept = world.records.kept();
        kept.heads
            .values()
            .find(|head| head.content.contains("\"Tue\""))
            .cloned()
            .unwrap()
    };
    let (id, rev) = (
        bzz::protocol::artifact::parse(&tuesday)
            .unwrap()
            .id
            .to_string(),
        tuesday.id.to_hex(),
    );
    let content = json!({"day": "Tue", "holder": "Scout", "order": 2});
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "fence here", "steps": [
                // An undeclared type, and the long form of a declared one.
                {"tool": "create_record", "arguments": {"app": "rota", "type": "card", "content": {"title": "x"}}},
                {"tool": "create_record", "arguments": {"app": "rota", "type": "bzz.app.rota.day", "content": content}},
                // An app installed and approved, but open in another channel.
                {"tool": "create_record", "arguments": {"app": "board", "type": "card", "content": {"title": "x"}}},
                {"tool": "records", "arguments": {"app": "board", "type": "card"}},
                // A channel named in the arguments is no argument at all.
                {"tool": "records", "arguments": {"app": "rota", "type": "day", "channel": world.elsewhere.to_string()}},
                {"tool": "send", "arguments": {"text": "fenced", "final": true}},
            ]},
            {"match": "fence elsewhere", "steps": [
                // Woken in the other channel: the first channel's record is
                // not there to be found.
                {"tool": "update_record", "arguments": {"app": "rota", "id": id, "rev": rev, "content": content}},
                {"tool": "send", "arguments": {"text": "not here", "final": true}},
            ]},
            {"match": "fence disabled", "steps": [
                {"tool": "apps", "keep": "apps"},
                {"tool": "update_record", "arguments": {"app": "rota", "id": id, "rev": rev, "content": content}},
                {"tool": "send", "arguments": {"text": "disabled", "final": true}},
            ]},
        ]),
    );
    let host = world.host(true).await;
    let here = world.ask(&host, world.channel, "fence here").await;
    world.settled(&here).await;
    let creates = world.answers("create_record");
    assert!(creates.iter().all(|(failed, _)| *failed), "{creates:?}");
    assert!(
        creates[0]
            .1
            .starts_with("fenced: the app rota did not declare the record type card"),
        "{}",
        creates[0].1
    );
    assert!(
        creates[0].1.contains("The app's record types are day"),
        "{}",
        creates[0].1
    );
    assert!(
        creates[1]
            .1
            .contains("not a name an app or a record type can have"),
        "{}",
        creates[1].1
    );
    assert!(
        creates[2]
            .1
            .starts_with("The app board is not open in this channel. The apps open here: rota."),
        "{}",
        creates[2].1
    );
    let lists = world.answers("records");
    assert!(
        lists[0].0 && lists[0].1.contains("board is not open in this channel"),
        "{lists:?}"
    );
    assert!(
        lists[1].0 && lists[1].1.contains("no argument `channel`"),
        "{lists:?}"
    );

    let elsewhere = world.ask(&host, world.elsewhere, "fence elsewhere").await;
    world.settled(&elsewhere).await;
    let (failed, text) = world.answers("update_record").remove(0);
    assert!(failed && text.starts_with("not_found: "), "{text}");

    world.set_enabled("rota", false).await;
    let disabled = world.ask(&host, world.channel, "fence disabled").await;
    world.settled(&disabled).await;
    let apps = json_of(&world.answers("apps")[0].1);
    assert_eq!(apps["apps"], json!([]));
    assert_eq!(
        apps["unusable"],
        json!([{"name": "rota", "why": "its owner disabled it"}])
    );
    let (failed, text) = world.answers("update_record").remove(1);
    assert!(
        failed && text.starts_with("The app rota is open in this channel but cannot be used now: its owner disabled it."),
        "{text}"
    );

    // Not one frame of the agent's reached the relay, and the week is as
    // it was.
    assert_eq!(world.records.writes_by(&world.agent_pubkey()), Vec::new());
    assert!(
        world
            .records
            .kept()
            .heads
            .values()
            .all(|head| !head.content.contains("Scout"))
    );
    world.stop(host).await;
}

/// A conflict gives the agent the record as it is now; at the relay's
/// quota it gets the wait, and bzz sends nothing more until it has passed.
#[tokio::test]
async fn a_conflict_gives_the_current_record_and_the_quota_gives_the_wait() {
    let mut world = World::new().await;
    world.install("rota", ROTA).await;
    world.open_in("rota", world.channel).await;
    let listed = world.script.marker("listed");
    let changed = world.script.marker("changed");
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "conflict", "steps": [
                {"tool": "records", "arguments": {"app": "rota", "type": "day"},
                 "keep": {"as": "tue", "find": "/records", "where": {"/content/day": "Tue"}}},
                {"touch": listed},
                {"wait_for": changed},
                {"tool": "update_record", "arguments": {
                    "app": "rota", "id": "$tue/id", "rev": "$tue/rev",
                    "content": {"day": "Tue", "holder": "Scout", "order": 2}}},
                {"tool": "send", "arguments": {"text": "Tuesday was taken.", "final": true}},
            ]},
            {"match": "quota", "steps": [
                {"tool": "records", "arguments": {"app": "rota", "type": "day"},
                 "keep": {"as": "wed", "find": "/records", "where": {"/content/day": "Wed"}}},
                {"tool": "update_record", "arguments": {
                    "app": "rota", "id": "$wed/id", "rev": "$wed/rev",
                    "content": {"day": "Wed", "holder": "Scout", "order": 3}}},
                {"tool": "update_record", "arguments": {
                    "app": "rota", "id": "$wed/id", "rev": "$wed/rev",
                    "content": {"day": "Wed", "holder": "Scout", "order": 3}}},
                {"tool": "send", "arguments": {"text": "Later.", "final": true}},
            ]},
        ]),
    );
    let host = world.host(true).await;
    let request = world.ask(&host, world.channel, "conflict").await;
    until("the agent to list the days", WAIT, || {
        listed.exists().then_some(())
    })
    .await;
    // Another profile takes Tuesday first.
    let mut other_store = Store::open_memory().unwrap();
    other_store
        .sync_config(&support_config(&world.other).0)
        .unwrap();
    let other_keys = world.other.clone();
    let (other, other_supervisor) =
        world.service_for(&other_keys, StoreHandle::spawn(other_store).unwrap());
    world.supervisors.push(other_supervisor);
    let (records, mut events) = other.open(world.channel, "rota", &["day"]).await.unwrap();
    while records.state() != SyncState::Live {
        let _ = tokio::time::timeout(WAIT, events.recv()).await.unwrap();
    }
    let tuesday = records
        .records()
        .into_iter()
        .find(|record| record.content["day"] == "Tue")
        .unwrap();
    let mut content = tuesday.content.clone();
    content.insert("holder".into(), json!("bo"));
    records
        .update(tuesday.id, &tuesday.revision, content)
        .await
        .unwrap();
    std::fs::write(&changed, "").unwrap();
    world.settled(&request).await;
    let (failed, text) = world.answers("update_record").remove(0);
    assert!(failed && text.starts_with("conflict: "), "{text}");
    let detail = json_of(&text);
    assert_eq!(detail["error"]["code"], "conflict");
    assert_eq!(detail["current"]["content"]["holder"], "bo");
    assert_eq!(
        detail["current"]["by"]["id"],
        world.other.public_key().to_hex()
    );
    assert_ne!(detail["current"]["rev"], json!(tuesday.revision));
    assert!(
        world
            .records
            .kept()
            .heads
            .values()
            .any(|head| head.content.contains("\"bo\""))
    );

    // The relay's quota for the agent is used up: the first write is told
    // the wait; the second is answered at once, and not sent.
    world.records.kept().quota.insert(world.agent_pubkey(), 0);
    let sent_before = world.records.writes_by(&world.agent_pubkey()).len();
    let request = world.ask(&host, world.channel, "quota").await;
    world.settled(&request).await;
    let answers = world.answers("update_record");
    for (failed, text) in &answers[1..] {
        assert!(*failed && text.starts_with("rate_limited: "), "{text}");
        let detail = json_of(text);
        let wait = detail["error"]["retry_after"].as_u64().unwrap();
        assert!((1..=19).contains(&wait), "{detail}");
        assert!(text.contains(&format!("Try again in {wait} s")), "{text}");
    }
    assert_eq!(answers.len(), 3);
    assert_eq!(
        world.records.writes_by(&world.agent_pubkey()).len(),
        sent_before + 1,
        "the second write waited for the relay's wait, and sent nothing"
    );
    drop(records);
    world.stop(host).await;
}

/// bzz's own cap on an agent's record writes: 60 in any minute, then
/// `rate_limited` with the wait, and no frame.
#[tokio::test]
async fn an_agents_record_writes_stop_at_its_own_budget() {
    let world = World::new().await;
    world.install("rota", ROTA).await;
    world.open_in("rota", world.channel).await;
    let mut steps = (0..61)
        .map(|order| {
            json!({"tool": "create_record", "arguments": {
                "app": "rota", "type": "day",
                "content": {"day": format!("Extra {order}"), "holder": null, "order": 100 + order}}})
        })
        .collect::<Vec<_>>();
    steps.push(json!({"tool": "send", "arguments": {"text": "Done.", "final": true}}));
    world.script.write(
        json!({"own_host": true}),
        json!([{"match": "many", "steps": steps}]),
    );
    let host = world.host(true).await;
    let request = world.ask(&host, world.channel, "many").await;
    world.settled(&request).await;
    let answers = world.answers("create_record");
    assert_eq!(answers.len(), 61);
    assert!(
        answers[..60].iter().all(|(failed, _)| !failed),
        "{:?}",
        answers.iter().find(|(failed, _)| *failed)
    );
    let (failed, text) = &answers[60];
    assert!(*failed && text.starts_with("rate_limited: "), "{text}");
    let wait = json_of(text)["error"]["retry_after"].as_u64().unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    assert_eq!(world.records.writes_by(&world.agent_pubkey()).len(), 60);
    // A record is no message: the turn could have posted its messages.
    assert_eq!(world.chat.replies_to(&request).len(), 1);
    world.stop(host).await;
}
