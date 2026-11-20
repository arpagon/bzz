//! v0.16 M5: agents write and change apps. The scripted harness
//! (`examples/scripted_harness.rs`) stands in for the model, through the
//! pipe as bzz's own host and through `bzz agent-tools` as a coding
//! harness; a recorded relay carries its chat, and a scripted relay that
//! keeps records (`support::artifact_relay`) the channel's records. The
//! tools, the store of apps, the approval's rule and the outcome told to
//! the agent are bzz's own code.
//!
//! The owner's answer is given here as the overlay gives it, by
//! `Apps::approve` and `Apps::refuse`; the overlay itself, with the open
//! Board changing in place, is `app::pane::tests::managed::
//! an_agents_board_with_priority_changes_the_open_board_in_place`.
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
    apps::store::{
        Apps, Outcome, Proposer, ProposerKind, Recorded, Submission, VersionState,
        review::from_whom,
    },
    config::{Harness, LocalAgentConfig, WakeRule},
    protocol::record::Fence,
    realtime::supervisor::SupervisorHandle,
    store::{
        Store,
        agent_runtime::{InboxRow, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, Keys};
use serde_json::{Value, json};
use support::{
    agent_harness::{RecordedRelay, Script},
    artifact_relay::ArtifactRelay,
};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(30);
const BOARD: &str = include_str!("fixtures/apps/board/app.lua");
const BOARD_BRIEF: &str = include_str!("fixtures/apps/board/brief.md");
const BOARD_PRIORITY: &str = include_str!("fixtures/apps/board-priority/app.lua");
const BOARD_PRIORITY_BRIEF: &str = include_str!("fixtures/apps/board-priority/brief.md");
const HOSTILE: &str = include_str!("apps-bench/hostile-app/app.lua");

fn limits() -> Limits {
    Limits {
        start: Duration::from_secs(5),
        session: Duration::from_secs(5),
        turn: Duration::from_secs(25),
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

/// The owner's bzz with an agent, the Board installed by the owner and
/// open in a channel, with three cards on a relay of records.
struct World {
    temporary: TempDir,
    owner: Keys,
    agent_keys: Keys,
    /// A person of the channel, who may wake the agent.
    ana: Keys,
    chat: Arc<RecordedRelay>,
    records: ArtifactRelay,
    store: StoreHandle,
    community: Uuid,
    agent: Uuid,
    channel: Uuid,
    script: Script,
    supervisors: Vec<SupervisorHandle>,
}

impl World {
    async fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let (owner, agent_keys, ana) = (Keys::generate(), Keys::generate(), Keys::generate());
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
            ana,
            chat,
            records,
            store,
            community,
            agent: Uuid::new_v4(),
            channel: Uuid::new_v4(),
            supervisors: Vec::new(),
        };
        world.chat.set_channel(ChannelFacts {
            id: world.channel,
            name: "general".into(),
            kind: PlaceKind::Channel,
            members: vec![
                world.owner.public_key().to_hex(),
                world.agent_pubkey(),
                world.ana.public_key().to_hex(),
            ],
            read_only: Vec::new(),
        });
        let (service, supervisor, _signer) = world.records.service(
            &world.agent_keys.clone(),
            world.community,
            world.store.clone(),
        );
        world.supervisors.push(supervisor);
        world.chat.set_records(service);
        let fence = Fence::new("board", world.channel, &["card"]).unwrap();
        for (order, title) in ["Alpha", "Bravo", "Charlie"].iter().enumerate() {
            let content = json!({"title": title, "column": "todo", "order": order + 1});
            world.records.seed(
                &fence
                    .create("card", Uuid::new_v4(), content.as_object().unwrap())
                    .unwrap()
                    .sign_with_keys(&world.owner)
                    .unwrap(),
            );
        }
        world.install_board().await;
        world
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.temporary.path().join("data")
    }

    fn agent_pubkey(&self) -> String {
        self.agent_keys.public_key().to_hex()
    }

    async fn apps<T: Send + 'static>(
        &self,
        call: impl FnOnce(&mut Apps<'_>, Uuid) -> T + Send + 'static,
    ) -> T {
        let (community, root) = (self.community, self.data_dir().join("apps"));
        self.store
            .call(move |store| Ok(call(&mut Apps::at(store, root), community)))
            .await
            .unwrap()
    }

    /// The owner installs the Board and opens it in the channel.
    async fn install_board(&self) {
        let channel = self.channel;
        self.apps(move |apps, community| {
            let recorded = apps
                .propose(
                    community,
                    Submission::new("board", BOARD, BOARD_BRIEF, Proposer::owner()),
                )
                .unwrap();
            let Recorded::Waiting { version, hash, .. } = recorded else {
                panic!("the Board did not load: {recorded:?}");
            };
            apps.approve(community, "board", version, &hash).unwrap();
            apps.open_in(community, "board", channel).unwrap();
        })
        .await;
    }

    async fn host(&self, own_host: bool) -> AgentHost {
        let harness = if own_host {
            Harness::PiDurable
        } else {
            Harness::ClaudeCode
        };
        let mut local = LocalAgentConfig::new(harness);
        local.command = self.script.command();
        // Ana may wake it (plan D7: a person who may wake the agent asks).
        local.wake = WakeRule::Allowlist;
        local.wake_allowlist = vec![self.ana.public_key().to_hex()];
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = limits();
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        let host = AgentHost::start(options).await.unwrap();
        host.add(
            AgentSpec {
                id: self.agent,
                name: "Scout".into(),
                pubkey: self.agent_pubkey(),
                owner: self.owner.public_key().to_hex(),
                community: self.community,
                local,
            },
            self.chat.clone(),
        )
        .await;
        host
    }

    /// A mention of the agent in the channel by `who`.
    async fn ask(&self, host: &AgentHost, who: &Keys, text: &str) -> Event {
        let event = buzz_sdk::build_message(
            self.channel,
            text,
            None,
            &[self.agent_pubkey().as_str()],
            false,
            &[],
            &[],
        )
        .unwrap()
        .sign_with_keys(who)
        .unwrap();
        self.chat.seed(&event);
        let facts =
            bzz::agents::runtime::relay::AgentRelay::channel(self.chat.as_ref(), self.channel)
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

    /// What the tool `name` answered, each call: (is an error, text).
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

    /// The text of every prompt the harness was given, oldest first.
    fn prompts(&self) -> Vec<String> {
        self.script
            .received("session/prompt")
            .into_iter()
            .map(|params| {
                params
                    .pointer("/prompt/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

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

    /// The bytes of the Board that bzz hands out to run, and its version.
    async fn runnable(&self) -> (u32, Vec<u8>) {
        self.apps(|apps, community| {
            let runnable = apps.runnable(community, "board").unwrap();
            (runnable.version(), runnable.into_script())
        })
        .await
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
        .add_community("m5".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    (config, community)
}

/// The JSON a tool answered: its text, or its last line for a failure.
fn json_of(text: &str) -> Value {
    serde_json::from_str(text.lines().last().unwrap_or(text))
        .unwrap_or_else(|_| panic!("not JSON: {text}"))
}

/// How a model changes the Board, step by step: it reads the guide and
/// the Board, tries the new version on the channel's cards, proposes it,
/// and answers.
fn change_the_board(script: &str, brief: &str) -> Value {
    json!([
        {"tool": "apps", "keep": "apps"},
        {"tool": "app_guide"},
        {"tool": "apps", "arguments": {"app": "board"}},
        {"tool": "try_app", "arguments": {"script": script, "from_app": "board", "keys": ["j", "2"]}},
        {"tool": "propose_app", "arguments": {"app": "board", "script": script, "brief": brief}},
        {"tool": "send", "arguments": {"text": "I proposed the Board with priority; the owner decides.", "final": true}},
    ])
}

/// M5's exit gate, scripted: Ana asks the agent for the priority and the
/// filter; it reads the Board, tries its version on the channel's cards
/// and proposes it; nothing of it runs until the owner approves; the
/// approval shows the agent and Ana; at its next turn in the channel the
/// agent is told "applied", once. Through the pipe and through
/// `bzz agent-tools`.
#[tokio::test]
async fn an_agent_proposes_the_board_with_priority_and_is_told_it_was_applied() {
    for own_host in [true, false] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": own_host}),
            json!([
                {"match": "add a priority", "steps": change_the_board(BOARD_PRIORITY, BOARD_PRIORITY_BRIEF)},
                {"match": "is it in", "steps": [{"tool": "send", "arguments": {"text": "Yes.", "final": true}}]},
                {"match": "and now", "steps": [{"tool": "send", "arguments": {"text": "Still yes.", "final": true}}]},
            ]),
        );
        let host = world.host(own_host).await;
        let ana = world.ana.clone();
        let request = world
            .ask(
                &host,
                &ana,
                "please add a priority and a filter to the board",
            )
            .await;
        let row = world.settled(&request).await;
        assert_eq!(
            row.state,
            RequestState::Replied,
            "{:#?}",
            world.script.log()
        );

        // The tools' descriptions are how it learns of apps: nothing is in
        // the first prompt beforehand (D17).
        let first = world.prompts().remove(0);
        assert!(
            !first.contains("app_guide") && !first.contains("Since your last turn"),
            "{first}"
        );
        // What it was told: the Board, the guide, the script, the try.
        let listed = json_of(&world.answers("apps")[0].1);
        assert_eq!(listed["apps"][0]["name"], "board");
        assert_eq!(listed["apps"][0]["version"], 1);
        let (failed, guide) = world.answers("app_guide").remove(0);
        assert!(!failed && guide.starts_with("# Writing an app for bzz"));
        assert!(guide.contains("# The app contract, `api = 1`"));
        let read = json_of(&world.answers("apps")[1].1);
        assert_eq!(read["version"], 1);
        assert_eq!(read["script"], BOARD);
        assert_eq!(read["brief"], BOARD_BRIEF);
        assert!(
            read["manifest"]
                .as_str()
                .unwrap()
                .contains("types = [\"card\"]"),
            "{read}"
        );
        let (failed, tried) = world.answers("try_app").remove(0);
        assert!(!failed, "{tried}");
        let tried = json_of(&tried);
        assert_eq!(tried["size"], "40x16");
        assert_eq!(tried["error"], Value::Null, "{tried}");
        let screen = tried["screen"].as_str().unwrap();
        assert!(screen.contains("> Bravo P2"), "{screen}");
        assert!(screen.contains("Priority: all"), "{screen}");
        // The try wrote a card's priority in memory only.
        assert!(world.records.writes_by(&world.agent_pubkey()).is_empty());

        // Proposed, and waiting: the Board that runs is still the owner's.
        let (failed, proposed) = world.answers("propose_app").remove(0);
        assert!(!failed, "{proposed}");
        let proposed = json_of(&proposed);
        assert_eq!(proposed["status"], "waiting");
        assert_eq!(proposed["version"], 2);
        assert!(
            proposed["message"]
                .as_str()
                .unwrap()
                .starts_with("Waiting for the owner."),
            "{proposed}"
        );
        assert_eq!(world.runnable().await, (1, BOARD.as_bytes().to_vec()));
        // What the owner is shown: the agent, and Ana who asked.
        let (agent_key, ana_key) = (world.agent_pubkey(), world.ana.public_key().to_hex());
        let review = world
            .apps(|apps, community| apps.review(community, "board", 2).unwrap())
            .await;
        assert_eq!(review.from.kind, ProposerKind::Agent);
        assert_eq!(review.from.id.as_deref(), Some(agent_key.as_str()));
        assert_eq!(review.from.channel, Some(world.channel));
        assert_eq!(
            review.from.asked_by.as_ref().map(|asker| asker.id.as_str()),
            Some(ana_key.as_str())
        );
        assert!(from_whom(&review.from).starts_with("your agent Scout ("));
        assert!(
            review
                .render(|_| String::new())
                .contains(&format!("Asked by: ")),
            "{}",
            review.render(|_| String::new())
        );
        assert_eq!(world.chat.replies_to(&request).len(), 1);

        // The owner approves it, as the overlay does.
        let hash = review.hash.clone();
        world
            .apps(move |apps, community| apps.approve(community, "board", 2, &hash).unwrap())
            .await;
        assert_eq!(
            world.runnable().await,
            (2, BOARD_PRIORITY.as_bytes().to_vec())
        );
        // The agent's next turn in the channel is told, once.
        let owner = world.owner.clone();
        let next = world.ask(&host, &owner, "is it in?").await;
        world.settled(&next).await;
        let later = world.ask(&host, &owner, "and now?").await;
        world.settled(&later).await;
        let prompts = world.prompts();
        assert_eq!(prompts.len(), 3, "{prompts:#?}");
        assert!(
            prompts[1].contains("Since your last turn in this channel, of what you proposed here:\n- your version 2 of the app board: applied."),
            "{}",
            prompts[1]
        );
        assert!(
            !prompts[2].contains("Since your last turn"),
            "told once: {}",
            prompts[2]
        );
        // One final reply to each mention, and the proposal in the activity.
        for asked in [&request, &next, &later] {
            assert_eq!(world.chat.replies_to(asked).len(), 1);
        }
        let ana_name = &ana_key[..8];
        assert!(
            world.activity().iter().any(|line| line
                .starts_with("version 2 of the app board proposed, asked by ")
                && line.contains(ana_name)
                && line.ends_with("· waiting for the owner")),
            "{:?}",
            world.activity()
        );
        world.stop(host).await;
    }
}

/// Refused with a reason, nothing changes: the Board that runs, the store's
/// versions and the records are as before, and the agent gets the reason
/// word for word at its next turn.
#[tokio::test]
async fn a_refused_version_changes_nothing_and_the_agent_gets_the_reason() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "add a priority", "steps": change_the_board(BOARD_PRIORITY, BOARD_PRIORITY_BRIEF)},
            {"match": "what happened", "steps": [{"tool": "send", "arguments": {"text": "It was refused.", "final": true}}]},
        ]),
    );
    let host = world.host(true).await;
    let ana = world.ana.clone();
    let request = world.ask(&host, &ana, "add a priority, please").await;
    world.settled(&request).await;
    let heads_before = world.records.kept().heads.clone();
    let reason = "Not now: \"P1\" means something else here; ask me first.";
    world
        .apps(move |apps, community| {
            apps.refuse(community, "board", 2, Some(reason)).unwrap();
        })
        .await;
    assert_eq!(world.runnable().await, (1, BOARD.as_bytes().to_vec()));
    let versions = world
        .apps(|apps, community| {
            apps.versions(community, "board")
                .unwrap()
                .into_iter()
                .map(|version| (version.version, version.state, version.kept))
                .collect::<Vec<_>>()
        })
        .await;
    assert_eq!(
        versions,
        [
            (2, VersionState::Refused, false),
            (1, VersionState::Approved, true)
        ]
    );
    let next = world.ask(&host, &ana, "what happened?").await;
    world.settled(&next).await;
    let prompt = world.prompts().remove(1);
    assert!(
        prompt.contains(&format!(
            "- your version 2 of the app board: refused by the owner, who said: \"{reason}\". Nothing of it runs."
        )),
        "{prompt}"
    );
    // The same answer whenever it asks.
    assert_eq!(
        world
            .apps(|apps, community| apps.outcome(community, "board", 2).unwrap())
            .await,
        Outcome::Refused {
            reason: Some(reason.into())
        }
    );
    assert_eq!(
        world.records.kept().heads,
        heads_before,
        "no record changed"
    );
    assert!(world.records.writes_by(&world.agent_pubkey()).is_empty());
    world.stop(host).await;
}

/// A script with an error comes back to the agent at once, with file and
/// line, and the owner is never asked: nothing is recorded, nothing waits.
/// So does a brief that does not name the script's types, and none at all.
#[tokio::test]
async fn a_script_with_an_error_comes_back_with_file_and_line_and_the_owner_is_never_asked() {
    for own_host in [true, false] {
        let world = World::new().await;
        let broken = BOARD_PRIORITY.replacen("local COLS", "local COLS = = 1\nlocal COLS_", 1);
        let line = BOARD_PRIORITY
            .lines()
            .position(|text| text.starts_with("local COLS"))
            .unwrap()
            + 1;
        world.script.write(
            json!({"own_host": own_host}),
            json!([{"match": "break it", "steps": [
                {"tool": "propose_app", "arguments": {"app": "board", "script": broken, "brief": BOARD_PRIORITY_BRIEF}},
                {"tool": "try_app", "arguments": {"script": broken}},
                {"tool": "propose_app", "arguments": {"app": "board", "script": BOARD_PRIORITY, "brief": "A board with priorities."}},
                {"tool": "propose_app", "arguments": {"app": "board", "script": BOARD_PRIORITY}},
                {"tool": "propose_app", "arguments": {"app": "board", "script": "return 1", "brief": "x", "approve": true}},
                {"tool": "send", "arguments": {"text": "It does not load.", "final": true}},
            ]}]),
        );
        let host = world.host(own_host).await;
        let owner = world.owner.clone();
        let request = world.ask(&host, &owner, "break it").await;
        world.settled(&request).await;
        let answers = world.answers("propose_app");
        assert_eq!(answers.len(), 4, "{answers:?}");
        assert!(answers.iter().all(|(failed, _)| *failed), "{answers:?}");
        assert!(
            answers[0]
                .1
                .starts_with(&format!("failed: board.lua:{line}:")),
            "{}",
            answers[0].1
        );
        assert!(
            answers[0].1.contains("the owner was not asked"),
            "{}",
            answers[0].1
        );
        assert_eq!(json_of(&answers[0].1)["error"]["code"], "failed");
        assert!(
            answers[1]
                .1
                .contains("the brief does not name the record type card"),
            "{}",
            answers[1].1
        );
        assert!(answers[2].1.contains("needs `brief`"), "{}", answers[2].1);
        assert!(
            answers[3].1.contains("no argument `approve`"),
            "{}",
            answers[3].1
        );
        // The try says the same error, at the same place.
        let tried = json_of(&world.answers("try_app")[0].1);
        assert!(
            tried["error"]
                .as_str()
                .unwrap()
                .starts_with(&format!("app.lua:{line}:")),
            "{tried}"
        );
        // Nothing waits, nothing was recorded, the Board runs as it was.
        let (waiting, versions) = world
            .apps(|apps, community| {
                (
                    apps.waiting(community).unwrap().len(),
                    apps.versions(community, "board").unwrap().len(),
                )
            })
            .await;
        assert_eq!((waiting, versions), (0, 1));
        assert_eq!(world.runnable().await, (1, BOARD.as_bytes().to_vec()));
        world.stop(host).await;
    }
}

/// The screen an agent gets for the Board at 40×16 is the one the Board
/// draws there, and a hostile script tried by an agent reaches nothing and
/// costs bzz nothing: the try answers in its time, the agent's next call
/// is answered at once, and no frame reaches the relay.
#[tokio::test]
async fn a_try_draws_the_boards_screen_and_a_hostile_script_reaches_nothing() {
    let world = World::new().await;
    let records = json!([
        {"type": "card", "content": {"title": "Alpha", "column": "todo", "order": 1}},
        {"type": "card", "content": {"title": "Bravo", "column": "todo", "order": 2}, "by": "bo"},
        {"type": "card", "content": {"title": "Charlie", "column": "doing", "order": 3}},
    ]);
    let marker = world.script.marker("after-hostile");
    world.script.write(
        json!({"own_host": true}),
        json!([{"match": "try them", "steps": [
            {"tool": "try_app", "arguments": {"script": BOARD, "records": records, "keys": ["j"]}},
            {"tool": "try_app", "arguments": {"script": HOSTILE, "width": 200, "height": 80,
                "keys": ["x", "y", "p", "m", "s", "c", "l", "u", "d"]}},
            {"touch": marker},
            {"tool": "apps"},
            {"tool": "try_app", "arguments": {"script": BOARD, "from_app": "board", "keys": ["n", "type:Delta", "enter"]}},
            {"tool": "send", "arguments": {"text": "Tried.", "final": true}},
        ]}]),
    );
    let host = world.host(true).await;
    let owner = world.owner.clone();
    let started = std::time::Instant::now();
    let request = world.ask(&host, &owner, "try them").await;
    world.settled(&request).await;
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
    let tries = world.answers("try_app");
    assert_eq!(tries.len(), 3);
    // The Board at the pane's size, as `bzz app try` draws it.
    let board = json_of(&tries[0].1);
    let expected = bzz::apps::dev::try_script(bzz::apps::dev::Trial {
        script: BOARD.as_bytes().to_vec(),
        script_name: "app.lua".into(),
        width: 40,
        height: 16,
        steps: vec![bzz::apps::session::headless::Step::parse("j").unwrap()],
        seeds: bzz::apps::dev::parse_seed(&records.to_string()).unwrap(),
    })
    .unwrap()
    .tried;
    let trimmed = |screen: &str| {
        screen
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(board["screen"].as_str().unwrap(), trimmed(&expected.screen));
    assert_eq!(board["screen"].as_str().unwrap().lines().count(), 16);
    assert!(
        board["screen"].as_str().unwrap().contains("> Bravo"),
        "{board}"
    );
    // The hostile script: an answer with what stopped it, in its time.
    let (failed, hostile) = &tries[1];
    assert!(!failed, "{hostile}");
    let hostile = json_of(hostile);
    let screen = hostile["screen"].as_str().unwrap();
    assert!(screen.contains("blocked ("), "{screen}");
    assert!(!screen.contains("REACHED"), "{screen}");
    assert!(marker.exists());
    // The `apps` after it was answered, and the rest of the turn went on.
    assert!(!world.answers("apps")[0].0);
    // A try on a copy of the channel's cards: the new card is in the copy,
    // not on the relay.
    let copied = json_of(&tries[2].1);
    let screen = copied["screen"].as_str().unwrap();
    assert!(
        screen.contains("To do (4)") && screen.contains("Delta"),
        "{screen}"
    );
    assert!(world.records.writes_by(&world.agent_pubkey()).is_empty());
    assert!(
        world
            .records
            .kept()
            .heads
            .values()
            .all(|head| !head.content.contains("Delta"))
    );
    world.stop(host).await;
}

/// An agent's proposal cannot run unapproved by any path, and it cannot
/// bury the owner: three versions waiting at most, of all apps; no tool
/// approves, opens, enables or removes; every answer leaves the Board the
/// owner approved as the only bytes bzz hands out.
#[tokio::test]
async fn an_agents_proposals_never_run_unapproved_and_are_bounded() {
    let world = World::new().await;
    let new_app = |name: &str| {
        json!({"tool": "propose_app", "arguments": {
            "app": name,
            "script": format!("local app = {{ api = 1, types = {{ \"note\" }} }}\nfunction app.view(s, ctx) return ui.text {{ \"{name}\" }} end\nreturn app\n"),
            "brief": "Notes: records of type `note`, each {text}."}})
    };
    world.script.write(
        json!({"own_host": true}),
        json!([{"match": "make apps", "steps": [
            {"tool": "propose_app", "arguments": {"app": "board", "script": BOARD_PRIORITY, "brief": BOARD_PRIORITY_BRIEF}},
            new_app("notes"),
            new_app("poll"),
            new_app("tally"),
            {"tool": "propose_app", "arguments": {"app": "board", "script": BOARD_PRIORITY.replace("Priority: ", "Prio: "), "brief": BOARD_PRIORITY_BRIEF}},
            {"tool": "apps", "arguments": {"app": "notes"}},
            {"tool": "approve", "arguments": {"app": "board"}},
            {"tool": "open_app", "arguments": {"app": "notes"}},
            {"tool": "send", "arguments": {"text": "Proposed.", "final": true}},
        ]}]),
    );
    let host = world.host(true).await;
    let owner = world.owner.clone();
    let request = world.ask(&host, &owner, "make apps").await;
    world.settled(&request).await;
    let answers = world.answers("propose_app");
    let failed = answers
        .iter()
        .map(|(failed, _)| *failed)
        .collect::<Vec<_>>();
    assert_eq!(failed, [false, false, false, true, false], "{answers:#?}");
    assert!(
        answers[3]
            .1
            .starts_with("3 versions proposed by this agent already wait"),
        "{}",
        answers[3].1
    );
    // The fifth replaced its own waiting Board, which does not count twice.
    assert_eq!(json_of(&answers[4].1)["replaces"], json!([2]));
    // An app not open here is shown only as what became of its versions.
    let notes = json_of(&world.answers("apps")[0].1);
    assert_eq!(notes["open_here"], false);
    assert_eq!(notes["your_versions"][0]["state"], "waiting");
    assert!(notes.get("script").is_none(), "{notes}");
    // There is no tool to approve or open.
    for name in ["approve", "open_app"] {
        let (failed, text) = world.answers(name).remove(0);
        assert!(
            failed && text.contains(&format!("There is no tool `{name}`")),
            "{text}"
        );
    }
    // Only what the owner approved runs; nothing of the agent's is open.
    assert_eq!(world.runnable().await, (1, BOARD.as_bytes().to_vec()));
    let (waiting, open, notes_runs) = world
        .apps(|apps, community| {
            (
                apps.waiting(community)
                    .unwrap()
                    .iter()
                    .map(|version| (version.name.clone(), version.version))
                    .collect::<Vec<_>>(),
                apps.list(community)
                    .unwrap()
                    .iter()
                    .filter(|listed| !listed.channels.is_empty())
                    .map(|listed| listed.app.name.clone())
                    .collect::<Vec<_>>(),
                apps.runnable(community, "notes").is_ok(),
            )
        })
        .await;
    let mut waiting = waiting;
    waiting.sort();
    assert_eq!(
        waiting,
        [
            ("board".to_owned(), 3),
            ("notes".to_owned(), 1),
            ("poll".to_owned(), 1)
        ]
    );
    assert_eq!(open, ["board"]);
    assert!(!notes_runs);
    world.stop(host).await;
}
