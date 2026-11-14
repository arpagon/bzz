//! v0.15 M6: the three coding harnesses under bzz's guard, against a
//! scripted harness (`examples/scripted_harness.rs`) that stands in for the
//! owner's own program, and against transcripts recorded from the real
//! programs (`tests/fixtures/agents/transcripts/`). Each test names the rule
//! of the plan (M6, D16 to D19) or of ADR v0.15 local agents, decision 8,
//! that it shows. Nothing here needs Grok, Codex, Claude Code, Node, a
//! sign-in or a network.
#![cfg(unix)]

mod support;

use std::{
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bzz::{
    agents::{
        activity,
        runtime::{
            Limits,
            admission::{ChannelFacts, PlaceKind},
            agent::{AgentSpec, Environment},
            approval::Answer,
            coding,
            host::{AgentHost, HostOptions, Offer},
            process,
        },
    },
    config::{
        AgentModelConfig, ExecGrant, FolderGrant, Harness, HostedAgentConfig, LocalAgentConfig,
        WakeRule,
    },
    store::{
        Store,
        agent_runtime::{InboxRow, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, Keys};
use serde_json::{Value, json};
use support::agent_harness::{RecordedRelay, Script, program as scripted_harness, until};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);
const CODING: [Harness; 3] = [Harness::Grok, Harness::Codex, Harness::ClaudeCode];

fn limits() -> Limits {
    Limits {
        start: Duration::from_secs(5),
        session: Duration::from_secs(5),
        turn: Duration::from_secs(15),
        cancel_grace: Duration::from_millis(400),
        stop_grace: Duration::from_millis(400),
        typing_interval: Duration::from_millis(50),
        crash_window: Duration::from_secs(120),
        max_crashes: 3,
        max_attempts: 3,
        restart_backoff: Duration::from_millis(30),
        publish: Duration::from_secs(3),
        concurrent_turns: 4,
        effects_per_turn: 3,
        checkpoint_idle: Duration::from_secs(600),
        agent_exchanges: 3,
        agent_exchange_window: Duration::from_secs(3600),
        prune_interval: Duration::from_secs(24 * 3600),
        approval: Duration::from_secs(600),
    }
}

/// One owner, one agent, one channel, one recorded relay, one database.
struct World {
    temporary: TempDir,
    owner: Keys,
    relay: Arc<RecordedRelay>,
    store: StoreHandle,
    agent: Uuid,
    channel: Uuid,
    script: Script,
}

impl World {
    async fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let (owner, agent_keys) = (Keys::generate(), Keys::generate());
        let auth_tag = bzz::auth::signer::SignerHandle::spawn(owner.clone())
            .owner_auth_tag(agent_keys.public_key(), "")
            .await
            .unwrap();
        let relay =
            RecordedRelay::new(agent_keys, bzz::broker::relay::auth_tag(&auth_tag).unwrap());
        let store =
            StoreHandle::spawn(Store::open(temporary.path().join("data").join("bzz.db")).unwrap())
                .unwrap();
        let world = Self {
            script: Script::new(&temporary.path().join("harness")),
            temporary,
            owner,
            relay,
            store,
            agent: Uuid::new_v4(),
            channel: Uuid::new_v4(),
        };
        world.relay.set_channel(ChannelFacts {
            id: world.channel,
            name: "general".into(),
            kind: PlaceKind::Channel,
            members: vec![world.owner.public_key().to_hex(), world.agent_pubkey()],
        });
        world
    }

    /// The real place of the test's own folder: `/tmp` may be a link.
    fn root(&self) -> PathBuf {
        std::fs::canonicalize(self.temporary.path()).unwrap()
    }

    /// As it really is: a program names its working folder that way.
    fn data_dir(&self) -> PathBuf {
        self.root().join("data")
    }

    /// The agent's working folder, as bzz makes it.
    fn workspace(&self) -> PathBuf {
        process::agent_dir(&self.data_dir(), self.agent).join("workspace")
    }

    fn agent_pubkey(&self) -> String {
        self.relay.agent.public_key().to_hex()
    }

    /// A folder of the test's own, made now.
    fn folder(&self, name: &str) -> PathBuf {
        let path = self.root().join("granted").join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn grant(&self, name: &str, write: bool, exec: ExecGrant) -> FolderGrant {
        FolderGrant {
            name: name.into(),
            path: self.folder(name),
            write,
            exec,
        }
    }

    /// The settings of an agent on `harness`, started from the script.
    fn settings(&self, harness: Harness) -> LocalAgentConfig {
        let mut local = LocalAgentConfig::new(harness);
        local.command = self.script.command();
        local.instructions = "Be brief.".into();
        local
    }

    async fn host_configured(
        &self,
        local: LocalAgentConfig,
        configure: impl FnOnce(&mut HostOptions),
    ) -> AgentHost {
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = limits();
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        // bzz's own environment, as far as a test knows it: only `PATH`.
        // What this machine has beyond that (a proxy, certificates) would
        // be handed on to a coding harness.
        options.environment = Arc::new(|name| {
            (name == "PATH")
                .then(|| std::env::var("PATH").ok())
                .flatten()
        });
        configure(&mut options);
        let host = AgentHost::start(options).await.unwrap();
        host.add(
            AgentSpec {
                id: self.agent,
                name: "Scout".into(),
                pubkey: self.agent_pubkey(),
                owner: self.owner.public_key().to_hex(),
                community: Uuid::nil(),
                local,
            },
            self.relay.clone(),
        )
        .await;
        host
    }

    /// A host with nobody at bzz to answer, as `bzz agent run`.
    async fn host(&self, local: LocalAgentConfig) -> AgentHost {
        self.host_configured(local, |_| {}).await
    }

    /// A host with the owner at the TUI.
    async fn host_with_owner(&self, local: LocalAgentConfig) -> AgentHost {
        self.host_configured(local, |options| options.owner_present = true)
            .await
    }

    fn message(&self, text: &str, thread: Option<&Event>) -> Event {
        let agent = self.agent_pubkey();
        let thread = thread.map(|root| buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        });
        let event = buzz_sdk::build_message(
            self.channel,
            text,
            thread.as_ref(),
            &[agent.as_str()],
            false,
            &[],
            &[],
        )
        .unwrap()
        .sign_with_keys(&self.owner)
        .unwrap();
        self.relay.seed(&event);
        event
    }

    async fn offer(&self, host: &AgentHost, event: &Event) {
        let facts =
            bzz::agents::runtime::relay::AgentRelay::channel(self.relay.as_ref(), self.channel)
                .await
                .unwrap();
        assert_eq!(host.offer(self.agent, event, &facts).await, Offer::Queued);
    }

    /// A top-level mention of the agent by its owner, admitted.
    async fn ask(&self, host: &AgentHost, text: &str) -> Event {
        let event = self.message(text, None);
        self.offer(host, &event).await;
        event
    }

    async fn row(&self, request: &Event) -> Option<InboxRow> {
        let (agent, id) = (self.agent, request.id.to_hex());
        self.store
            .call(move |store| store.agent_inbox_get(agent, &id))
            .await
            .unwrap()
    }

    /// Waits until the request's turn is over and its reply acknowledged.
    async fn settled(&self, request: &Event) -> InboxRow {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            if let Some(row) = self.row(request).await
                && !matches!(row.state, RequestState::Queued | RequestState::Delivered)
            {
                let (agent, id) = (self.agent, request.id.to_hex());
                let reply = self
                    .store
                    .call(move |store| store.agent_reply_get(agent, &id))
                    .await
                    .unwrap();
                if reply.is_none_or(|reply| reply.published) {
                    return row;
                }
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

    /// What bzz answered each permission request, once there are `count`:
    /// the id of the option it selected, or `cancelled`.
    async fn leave(&self, count: usize) -> Vec<String> {
        until(
            &format!("{count} answers to permission requests"),
            WAIT,
            || {
                Some(
                    self.script
                        .log()
                        .into_iter()
                        .filter(|entry| entry["to"] == "session/request_permission")
                        .map(|entry| {
                            let outcome = &entry["response"]["result"]["outcome"];
                            outcome["optionId"]
                                .as_str()
                                .or(outcome["outcome"].as_str())
                                .unwrap_or("no answer")
                                .to_owned()
                        })
                        .collect::<Vec<_>>(),
                )
                .filter(|answers| answers.len() >= count)
            },
        )
        .await
    }

    /// The agent's activity, without the time each line starts with.
    fn activity(&self) -> Vec<String> {
        activity::recent(&self.data_dir(), self.agent, 100)
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

    fn replies(&self, request: &Event) -> Vec<String> {
        self.relay
            .replies_to(request)
            .iter()
            .map(|event| event.content.clone())
            .collect()
    }

    /// The methods the harness received, in their order.
    fn methods(&self) -> Vec<String> {
        self.script
            .log()
            .into_iter()
            .filter_map(|entry| entry["got"].as_str().map(str::to_owned))
            .collect()
    }
}

fn send(text: &str) -> Value {
    json!({"tool": "send", "arguments": {"text": text, "final": true}})
}

/// The options Claude Code offers with a request: once, always, no.
fn options() -> Value {
    json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ])
}

/// A step: the harness asks leave for `tool`, as the three programs do.
fn leave(tool: Value) -> Value {
    json!({"request": {"method": "session/request_permission", "params": {
        "sessionId": "$session", "toolCall": tool, "options": options(),
    }}})
}

/// A file edit, as Claude Code asks leave for one.
fn edit(path: &Path) -> Value {
    json!({
        "toolCallId": "t", "name": "Write", "status": "pending", "kind": "edit",
        "title": format!("Write {}", path.display()),
        "rawInput": {"file_path": path, "content": "hi\n"},
        "content": [{"type": "diff", "path": path, "oldText": null, "newText": "hi\n"}],
        "locations": [{"path": path}],
    })
}

/// A shell command, as Claude Code asks leave for one; with `cwd`, as Codex
/// does.
fn command(text: &str, cwd: Option<&Path>) -> Value {
    let mut tool = json!({
        "toolCallId": "c", "status": "pending", "kind": "execute", "title": text,
        "rawInput": {"command": text},
    });
    if let Some(cwd) = cwd {
        tool["rawInput"]["cwd"] = json!(cwd);
    }
    tool
}

fn index_of(methods: &[String], method: &str, nth: usize) -> usize {
    methods
        .iter()
        .enumerate()
        .filter(|(_, got)| *got == method)
        .nth(nth)
        .unwrap_or_else(|| panic!("no {method} number {nth} in {methods:?}"))
        .0
}

// ---- the guard, before a session is prompted -------------------------------

/// Plan M6, point 3: the mode is set by bzz on every session before its
/// first prompt, on a new session and on a loaded one, whatever mode the
/// owner's own settings start it in. Grok has no modes: its sandbox profile
/// is its guard, written before it starts and named in its environment.
#[tokio::test]
async fn the_mode_is_set_before_the_first_prompt_on_a_new_and_on_a_loaded_session() {
    for (harness, mode, load_lists) in [
        (Harness::ClaudeCode, "default", true),
        (Harness::Codex, "workspace-write", true),
        // A harness that answers `session/load` without its modes.
        (Harness::Codex, "workspace-write", false),
    ] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": false, "load_session": true, "load_lists": load_lists}),
            json!([{"steps": [send("first")]}, {"steps": [send("second")]}]),
        );
        let host = world.host(world.settings(harness)).await;
        let first = world.ask(&host, "one").await;
        world.settled(&first).await;
        assert_eq!(world.replies(&first), ["first"]);
        let methods = world.methods();
        let (opened, set, prompted) = (
            index_of(&methods, "session/new", 0),
            index_of(&methods, "session/set_mode", 0),
            index_of(&methods, "session/prompt", 0),
        );
        assert!(opened < set && set < prompted, "{harness:?}: {methods:?}");
        let session = world.script.received("session/prompt")[0]["sessionId"].clone();
        assert_eq!(
            world.script.received("session/set_mode")[0],
            json!({"sessionId": session, "modeId": mode}),
            "{harness:?}"
        );
        host.shutdown().await;

        // bzz and the program start again: the conversation is loaded, and
        // the mode is set on it again before it is asked anything.
        let host = world.host(world.settings(harness)).await;
        let second = world.message("two", Some(&first));
        world.offer(&host, &second).await;
        world.settled(&second).await;
        assert_eq!(world.replies(&second), ["second"]);
        let methods = world.methods();
        assert_eq!(world.script.received("session/new").len(), 1, "{methods:?}");
        let (loaded, set, prompted) = (
            index_of(&methods, "session/load", 0),
            index_of(&methods, "session/set_mode", 1),
            index_of(&methods, "session/prompt", 1),
        );
        assert!(loaded < set && set < prompted, "{harness:?}: {methods:?}");
        assert_eq!(
            world.script.received("session/set_mode")[1],
            json!({"sessionId": session, "modeId": mode})
        );
        host.shutdown().await;
    }

    // Grok: no mode is sent. Its guard is the profile and the variable.
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false, "modes": []}),
        json!([{"steps": [send("done")]}]),
    );
    let mut local = world.settings(Harness::Grok);
    local.folders = vec![
        world.grant("work", true, ExecGrant::No),
        world.grant("docs", false, ExecGrant::No),
    ];
    let host = world.host(local.clone()).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    assert_eq!(world.replies(&request), ["done"]);
    assert!(!world.methods().contains(&"session/set_mode".to_owned()));
    let start = world
        .script
        .log()
        .into_iter()
        .find(|entry| entry.get("start").is_some())
        .unwrap();
    assert_eq!(start["start"]["env"]["GROK_SANDBOX"], coding::GROK_PROFILE);
    assert_eq!(
        start["start"]["cwd"],
        json!(std::fs::canonicalize(world.workspace()).unwrap())
    );
    let profile = std::fs::read_to_string(world.workspace().join(".grok/sandbox.toml")).unwrap();
    // Its folders for writing, and bzz's keys hidden from it.
    let hidden = coding::hidden_from_grok(&world.data_dir());
    assert_eq!(profile, coding::grok_profile(&local, &hidden));
    assert!(profile.contains(&world.folder("work").display().to_string()));
    assert!(!profile.contains(&world.folder("docs").display().to_string()));
    host.shutdown().await;
}

/// Plan M6, point 3: a session that does not take bzz's mode is never
/// prompted. The people in the chat are told, and the owner sees why.
#[tokio::test]
async fn a_session_that_does_not_take_the_mode_is_never_prompted() {
    for (what, settings, said) in [
        (
            "the mode is not on its list",
            json!({"own_host": false, "modes": ["auto", "plan"]}),
            "does not offer the mode `default`",
        ),
        (
            "it has no modes at all",
            json!({"own_host": false, "modes": []}),
            "refused the mode `default`",
        ),
        (
            "it refuses the mode",
            json!({"own_host": false, "refuse_mode": true}),
            "refused the mode `default`",
        ),
    ] {
        let world = World::new().await;
        world
            .script
            .write(settings, json!([{"steps": [send("never said")]}]));
        let host = world.host(world.settings(Harness::ClaudeCode)).await;
        let request = world.ask(&host, "hello").await;
        let row = world.settled(&request).await;
        assert_eq!(row.failure.as_deref(), Some("not-guarded"), "{what}");
        assert_eq!(
            world.replies(&request),
            [
                "I could not answer: my program did not accept the limits bzz sets on it, so nothing was sent to it."
            ],
            "{what}"
        );
        assert!(
            !world.methods().contains(&"session/prompt".to_owned()),
            "{what}: {:?}",
            world.methods()
        );
        let problem = host.status()[0].problem.clone().unwrap_or_default();
        assert!(problem.contains(said), "{what}: {problem}");
        assert!(problem.contains("nothing was sent to it"), "{problem}");
        // No credential of a session that was never used stays behind.
        assert!(host.credentials().is_empty(), "{what}");
        host.shutdown().await;
    }
}

/// Plan M6, point 1: the model the owner chose goes to the harness in the
/// way it takes one, before the first prompt. A model its program does not
/// offer ends the turn with a notice instead of an answer from another
/// model.
#[tokio::test]
async fn the_owners_model_is_set_on_the_session_or_the_turn_says_it_is_not_offered() {
    let offered = json!({
        "own_host": false,
        "models": ["grok-4.7", "grok-4.5"],
        "thinking": ["high", "low"],
    });
    let model = |id: &str, level: Option<&str>| AgentModelConfig {
        provider: "grok".into(),
        model_id: id.into(),
        thinking_level: level.map(str::to_owned),
    };
    let world = World::new().await;
    world
        .script
        .write(offered.clone(), json!([{"steps": [send("ok")]}]));
    let mut local = world.settings(Harness::Grok);
    local.model = Some(model("grok-4.5", Some("low")));
    let host = world.host(local).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    assert_eq!(world.replies(&request), ["ok"]);
    let session = world.script.received("session/prompt")[0]["sessionId"].clone();
    assert_eq!(
        world.script.received("session/set_config_option"),
        [
            json!({"sessionId": session, "configId": "model", "value": "grok-4.5"}),
            json!({"sessionId": session, "configId": "effort", "value": "low"}),
        ]
    );
    let methods = world.methods();
    assert!(
        index_of(&methods, "session/set_config_option", 1)
            < index_of(&methods, "session/prompt", 0)
    );
    host.shutdown().await;

    // A model the program does not offer: no prompt, and a notice.
    let world = World::new().await;
    world
        .script
        .write(offered.clone(), json!([{"steps": [send("never said")]}]));
    let mut local = world.settings(Harness::Grok);
    local.model = Some(model("gpt-9", None));
    let host = world.host(local).await;
    let request = world.ask(&host, "hello").await;
    let row = world.settled(&request).await;
    assert_eq!(row.failure.as_deref(), Some("model-unavailable"));
    assert_eq!(
        world.replies(&request),
        ["I could not answer: the model my owner chose for me is not available."]
    );
    assert!(!world.methods().contains(&"session/prompt".to_owned()));
    let problem = host.status()[0].problem.clone().unwrap_or_default();
    assert!(
        problem.contains("does not offer the model `gpt-9`")
            && problem.contains("grok-4.7, grok-4.5"),
        "{problem}"
    );
    host.shutdown().await;

    // A thinking level the model does not have is no reason to stay
    // silent: the owner is told, and the agent answers.
    let world = World::new().await;
    world
        .script
        .write(offered, json!([{"steps": [send("ok")]}]));
    let mut local = world.settings(Harness::Grok);
    local.model = Some(model("grok-4.7", Some("ultra")));
    let host = world.host(local).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    assert_eq!(world.replies(&request), ["ok"]);
    assert_eq!(world.script.received("session/set_config_option").len(), 1);
    host.shutdown().await;
}

/// Plan M6, point 1: the rules of the chat, the agent's instructions and
/// its folders go to each harness in the way that program takes them, and
/// Claude Code is told to read none of its owner's settings.
#[tokio::test]
async fn a_session_is_told_its_prompt_and_its_folders_as_its_program_takes_them() {
    for harness in CODING {
        let world = World::new().await;
        let (work, docs) = (world.folder("work"), world.folder("docs"));
        // As the two adapters answer `initialize`; Grok offers neither.
        let capabilities = if harness == Harness::Grok {
            json!({"list": {}})
        } else {
            json!({"systemPrompt": {}, "additionalDirectories": {}})
        };
        world.script.write(
            json!({"own_host": false, "load_session": true, "capabilities": capabilities}),
            json!([{"steps": [send("ok")]}, {"steps": [send("again")]}]),
        );
        let mut local = world.settings(harness);
        local.folders = vec![
            world.grant("work", true, ExecGrant::No),
            world.grant("docs", false, ExecGrant::No),
        ];
        let host = world.host(local.clone()).await;
        let request = world.ask(&host, "hello").await;
        world.settled(&request).await;
        let opened = world.script.received("session/new")[0].clone();
        let workspace = world.workspace();
        assert_eq!(opened["cwd"], json!(workspace), "{harness:?}");
        assert_eq!(opened["mcpServers"][0]["name"], "buzz");
        let told = match harness {
            Harness::Grok => opened["_meta"]["rules"].as_str(),
            _ => opened["systemPrompt"]["content"][0]["text"].as_str(),
        }
        .unwrap_or_else(|| panic!("{harness:?}: no system prompt in {opened}"))
        .to_owned();
        for stated in [
            "You are Scout".to_owned(),
            "`send` tool of the `buzz` server".to_owned(),
            format!("Your working folder is {}.", workspace.display()),
            format!("read and write in: {} (work)", work.display()),
            format!("read, and not change: {} (docs)", docs.display()),
            "Your instructions:\nBe brief.".to_owned(),
        ] {
            assert!(
                told.contains(&stated),
                "{harness:?}: `{stated}` in:\n{told}"
            );
        }
        match harness {
            Harness::ClaudeCode => {
                assert_eq!(opened["systemPrompt"]["mode"], "append");
                assert_eq!(opened["additionalDirectories"], json!([work, docs]));
                assert_eq!(
                    opened["_meta"]["claudeCode"]["options"],
                    json!({"settingSources": [], "strictMcpConfig": true})
                );
            }
            // What Codex is told of becomes writable in its sandbox.
            Harness::Codex => {
                assert_eq!(opened["additionalDirectories"], json!([work]));
                assert!(opened["_meta"].get("claudeCode").is_none());
            }
            _ => {
                assert!(opened.get("additionalDirectories").is_none());
                assert!(opened.get("systemPrompt").is_none());
            }
        }
        // The instructions travelled once, as the system prompt: a message
        // says only how to answer.
        let text = world.script.received("session/prompt")[0]["prompt"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            text.contains("`send` tool") && !text.contains("Be brief."),
            "{text}"
        );
        // Its environment: the four variables, its guard, and nothing else.
        let start = world
            .script
            .log()
            .into_iter()
            .find(|entry| entry.get("start").is_some())
            .unwrap();
        let mut names = start["start"]["env"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        let guard = match harness {
            Harness::Grok => Some("GROK_SANDBOX"),
            Harness::Codex => Some("INITIAL_AGENT_MODE"),
            _ => None,
        };
        let mut expected = vec!["HOME", "LANG", "PATH", "TMPDIR"];
        expected.extend(guard);
        expected.sort_unstable();
        assert_eq!(names, expected, "{harness:?}");
        host.shutdown().await;

        // A loaded session is told the same.
        let host = world.host(local).await;
        let second = world.message("again", Some(&request));
        world.offer(&host, &second).await;
        world.settled(&second).await;
        let loaded = world.script.received("session/load")[0].clone();
        for member in ["cwd", "additionalDirectories", "systemPrompt"] {
            assert_eq!(
                loaded.get(member),
                opened.get(member),
                "{harness:?}: {member}"
            );
        }
        assert_eq!(loaded["_meta"]["rules"], opened["_meta"]["rules"]);
        assert_eq!(loaded["_meta"]["claudeCode"], opened["_meta"]["claudeCode"]);
        host.shutdown().await;
    }
}

// ---- what a harness asks leave for ------------------------------------------

/// Plan M6, point 3: every permission request is answered by rule. bzz's own
/// chat tools are allowed; a file edit only inside the agent's working
/// folder and folders granted for writing; anything else, and anything bzz
/// cannot read a path or a kind from, is refused. Never "always".
#[tokio::test]
async fn each_thing_a_coding_harness_asks_leave_for_is_answered_by_rule() {
    let world = World::new().await;
    let (work, docs, outside) = (
        world.folder("work"),
        world.folder("docs"),
        world.folder("not-granted"),
    );
    // bzz makes the working folder at the agent's start; the links an
    // agent could leave there are made by the script, in its first step.
    let workspace = world
        .root()
        .join(format!("data/agents/{}/workspace", world.agent));
    std::fs::write(outside.join("secret"), "s").unwrap();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "bzz's chat tool, as Claude Code names it",
            json!({"title": "mcp__buzz__send", "kind": "other", "name": "mcp__buzz__send"}),
            "allow-once",
        ),
        (
            "bzz's chat tool, as Grok names it",
            json!({"title": "buzz__react", "kind": "other"}),
            "allow-once",
        ),
        (
            "an edit in its working folder",
            edit(&workspace.join("notes.md")),
            "allow-once",
        ),
        (
            "a new file in a new folder of it",
            edit(&workspace.join("src/deep/new.rs")),
            "allow-once",
        ),
        (
            "an edit in a folder granted for writing",
            edit(&work.join("a.txt")),
            "allow-once",
        ),
        (
            "an edit in a folder granted for reading",
            edit(&docs.join("a.md")),
            "reject",
        ),
        (
            "an edit outside every folder",
            edit(&outside.join("a.txt")),
            "reject",
        ),
        (
            "an edit in its own folder's parent",
            edit(&workspace.parent().unwrap().join("activity.jsonl")),
            "reject",
        ),
        (
            "an edit through a link that leaves",
            edit(&workspace.join("link-out/a.txt")),
            "reject",
        ),
        (
            "an edit of a link to a file outside",
            edit(&workspace.join("link-file")),
            "reject",
        ),
        (
            "an edit with `..` in its path",
            edit(&workspace.join("../../../../not-granted/a.txt")),
            "reject",
        ),
        (
            "an edit of a path that is not absolute",
            edit(Path::new("notes.md")),
            "reject",
        ),
        (
            "an edit that names no path",
            json!({"kind": "edit", "title": "Edit a file"}),
            "reject",
        ),
        (
            "an edit that names two files, one of them outside",
            json!({"kind": "edit", "title": "Write", "rawInput": {"file_path": workspace.join("a")},
                "content": [{"type": "diff", "path": outside.join("b"), "newText": "x"}]}),
            "reject",
        ),
        (
            "a read in a folder granted for reading",
            json!({"kind": "read", "title": "Read", "locations": [{"path": docs.join("a.md")}]}),
            "allow-once",
        ),
        (
            "a read outside every folder",
            json!({"kind": "read", "title": "Read", "rawInput": {"file_path": outside.join("secret")}}),
            "reject",
        ),
        (
            "a shell command, with no folder granted for commands",
            command("ls -la", None),
            "reject",
        ),
        (
            "a command bzz cannot read",
            json!({"kind": "execute", "title": "rm -rf ~"}),
            "reject",
        ),
        (
            "a fetch",
            json!({"kind": "fetch", "title": "Fetch https://example.org"}),
            "reject",
        ),
        (
            "a deletion",
            json!({"kind": "delete", "title": "Delete", "locations": [{"path": workspace.join("notes.md")}]}),
            "reject",
        ),
        (
            "a tool of another server",
            json!({"kind": "other", "title": "mcp__postgres__query"}),
            "reject",
        ),
        (
            "a shell command dressed as the chat tool",
            json!({"kind": "execute", "title": "mcp__buzz__send"}),
            "reject",
        ),
        (
            "a request that names no kind",
            json!({"title": "Do something"}),
            "reject",
        ),
    ];
    let mut steps = vec![
        json!({"spawn": ["ln", "-s", outside.display().to_string(), workspace.join("link-out").display().to_string()]}),
        json!({"spawn": ["ln", "-s", outside.join("secret").display().to_string(), workspace.join("link-file").display().to_string()]}),
        json!({"sleep_ms": 300}),
    ];
    steps.extend(cases.iter().map(|(_, tool, _)| leave(tool.clone())));
    // An edit bzz would allow, with only "always" on offer: nothing is
    // selected.
    steps.push(json!({"request": {"method": "session/request_permission", "params": {
        "sessionId": "$session", "toolCall": edit(&workspace.join("notes.md")),
        "options": [{"optionId": "always", "kind": "allow_always"}, {"optionId": "never", "kind": "reject_always"}],
    }}}));
    steps.push(send("done"));
    world
        .script
        .write(json!({"own_host": false}), json!([{"steps": steps}]));
    let mut local = world.settings(Harness::ClaudeCode);
    local.folders = vec![
        world.grant("work", true, ExecGrant::No),
        world.grant("docs", false, ExecGrant::No),
    ];
    let host = world.host(local).await;
    let request = world.ask(&host, "work").await;
    world.settled(&request).await;
    assert_eq!(world.replies(&request), ["done"]);
    let answers = world.leave(cases.len() + 1).await;
    for ((what, _, expected), answer) in cases.iter().zip(&answers) {
        assert_eq!(answer, expected, "{what}");
    }
    assert_eq!(answers[cases.len()], "cancelled", "only `always` on offer");
    assert!(
        !answers.iter().any(|answer| answer == "allow-with-updates"),
        "never `always`: {answers:?}"
    );
    // What was refused is in the agent's activity, with its path and why.
    let activity = world.activity().join("\n");
    for stated in [
        format!(
            "a file edit: {} · refused: outside its working folder and the folders granted for writing",
            docs.join("a.md").display()
        ),
        format!("a file edit: {}", outside.join("a.txt").display()),
        format!(
            "a read: {} · refused: outside its working folder and its granted folders",
            outside.join("secret").display()
        ),
        "· ls -la · refused: no folder is granted for commands".to_owned(),
        "a request of kind `fetch` · refused: bzz has no rule that allows it".to_owned(),
        "a request of kind `delete` · refused".to_owned(),
        "a request that names no kind · refused".to_owned(),
    ] {
        assert!(activity.contains(&stated), "`{stated}` in:\n{activity}");
    }
    // What was allowed is not noted: only what was refused or asked.
    assert!(!activity.contains(&workspace.join("notes.md").display().to_string()));
    host.shutdown().await;
}

/// Plan M6, point 3, as Codex 0.166 asks: a request that names only the id
/// of a call is read from what the harness reported under that id. bzz's
/// own chat tool is allowed, a tool of another server is not, and a file
/// change is held to the files of the diffs that were reported.
#[tokio::test]
async fn a_request_that_names_only_its_call_is_answered_from_what_was_reported() {
    let world = World::new().await;
    let (work, outside) = (world.folder("work"), world.folder("not-granted"));
    // As Codex names its options.
    let options = json!([
        {"optionId": "allow_once", "name": "Allow", "kind": "allow_once"},
        {"optionId": "allow_session", "name": "Allow for this session", "kind": "allow_always"},
        {"optionId": "allow_always", "name": "Always allow", "kind": "allow_always"},
        {"optionId": "cancel", "name": "Cancel", "kind": "reject_once"},
    ]);
    let by_id = |id: &str| {
        json!({"request": {"method": "session/request_permission", "params": {
            "_meta": {"is_mcp_tool_approval": true}, "sessionId": "$session",
            "toolCall": {"kind": "execute", "status": "pending", "toolCallId": id},
            "options": options,
        }}})
    };
    let tool = |id: &str, server: &str, name: &str| {
        json!({"update": {
            "sessionUpdate": "tool_call", "toolCallId": id, "status": "in_progress",
            "_meta": {"is_mcp_tool_call": true}, "kind": "execute",
            "title": format!("mcp.{server}.{name}"),
            "rawInput": {"server": server, "tool": name, "arguments": {"text": "hi", "final": true}},
        }})
    };
    let change = |id: &str, path: &Path| {
        json!({"update": {
            "sessionUpdate": "tool_call", "toolCallId": id, "status": "in_progress",
            "kind": "edit", "title": "Editing files",
            "content": [{"type": "diff", "path": path, "oldText": null, "newText": "hi\n"}],
        }})
    };
    let edit_by_id = |id: &str| {
        json!({"request": {"method": "session/request_permission", "params": {
            "sessionId": "$session",
            "toolCall": {"status": "pending", "toolCallId": id},
            "options": options,
        }}})
    };
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [
            tool("exec-1", "buzz", "send"),
            by_id("exec-1"),
            tool("exec-2", "postgres", "query"),
            by_id("exec-2"),
            // A call that was never reported.
            by_id("exec-unknown"),
            change("exec-3", &work.join("a.txt")),
            edit_by_id("exec-3"),
            change("exec-4", &outside.join("a.txt")),
            edit_by_id("exec-4"),
            send("done"),
        ]}]),
    );
    let mut local = world.settings(Harness::Codex);
    local.folders = vec![world.grant("work", true, ExecGrant::No)];
    let host = world.host(local).await;
    let request = world.ask(&host, "work").await;
    world.settled(&request).await;
    assert_eq!(
        world.leave(5).await,
        ["allow_once", "cancel", "cancel", "allow_once", "cancel"]
    );
    let activity = world.activity().join("\n");
    assert!(
        activity.contains("a tool of another server: postgres.query · refused"),
        "{activity}"
    );
    assert!(
        activity.contains(&format!("a file edit: {}", outside.join("a.txt").display())),
        "{activity}"
    );
    host.shutdown().await;
}

/// Plan M6, point 3: a shell command follows the folders granted for
/// commands and the owner's answer, as a Pi Durable agent's `run` does.
/// The owner is shown the command and where it would start; his yes allows
/// it once, his no refuses it, and with nobody at bzz it is refused.
#[tokio::test]
async fn a_command_runs_only_by_the_folders_granted_and_the_owners_answer() {
    let world = World::new().await;
    let ran = |name: &str| world.script.marker(name);
    let asked = |text: &str, cwd: Option<&Path>, marker: &str| {
        let mut step = leave(command(text, cwd));
        step["then_touch"] = json!(ran(marker));
        step["when_selected"] = json!("allow-once");
        step
    };
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [
            asked("make test", None, "first"),
            asked("rm -rf build", None, "second"),
            send("done"),
        ]}]),
    );
    let mut local = world.settings(Harness::ClaudeCode);
    local.folders = vec![world.grant("work", true, ExecGrant::Ask)];
    let host = world.host_with_owner(local).await;
    let request = world.ask(&host, "build").await;
    let question = async || {
        until("a question for the owner", WAIT, || {
            host.approvals().into_iter().next()
        })
        .await
    };
    // The first command waits: nothing is answered and nothing ran.
    let first = question().await;
    assert_eq!(first.command, "make test");
    assert!(first.coding, "the owner is told whose command it is");
    assert_eq!(first.folder, "work");
    assert_eq!(first.path, world.workspace());
    assert_eq!(first.agent, world.agent);
    assert!(!ran("first").exists());
    assert!(
        host.status()[0]
            .activity
            .as_deref()
            .is_some_and(|doing| doing.starts_with("waiting for your answer: run in")),
        "{:?}",
        host.status()
    );
    assert!(host.answer_approval(first.id, Answer::Allow));
    assert_eq!(world.leave(1).await, ["allow-once"]);
    until("the first command to start", WAIT, || {
        ran("first").exists().then_some(())
    })
    .await;
    // The second is refused by the owner.
    let second = question().await;
    assert_eq!(second.command, "rm -rf build");
    assert!(host.answer_approval(second.id, Answer::Refuse(Some("not that".into()))));
    assert_eq!(world.leave(2).await[1], "reject");
    world.settled(&request).await;
    assert!(!ran("second").exists());
    let workspace = world.workspace().display().to_string();
    assert_eq!(
        world.activity(),
        [
            format!("command in {workspace} · make test · asked of you"),
            format!("command in {workspace} · make test · allowed by you"),
            format!("command in {workspace} · rm -rf build · asked of you"),
            format!("command in {workspace} · rm -rf build · refused by you: not that"),
        ]
    );
    host.shutdown().await;

    // Nobody at bzz (`bzz agent run`): a folder that asks refuses; a folder
    // allowed without asking lets it run; and where a command would start
    // decides which folder counts.
    for (what, grants, wake, cwd, expected, noted) in [
        (
            "a folder that asks, and nobody to ask",
            vec![("work", ExecGrant::Ask)],
            WakeRule::OwnerOnly,
            None,
            "reject",
            "refused: nobody was at bzz to ask",
        ),
        (
            "a folder allowed without asking",
            vec![("ops", ExecGrant::WithoutAsking)],
            WakeRule::OwnerOnly,
            None,
            "allow-once",
            "allowed without asking",
        ),
        (
            "one folder that asks among those granted",
            vec![("ops", ExecGrant::WithoutAsking), ("work", ExecGrant::Ask)],
            WakeRule::OwnerOnly,
            None,
            "reject",
            "refused: nobody was at bzz to ask",
        ),
        (
            "without asking, for an agent anyone may wake",
            vec![("ops", ExecGrant::WithoutAsking)],
            WakeRule::Anyone,
            None,
            "reject",
            "refused: nobody was at bzz to ask",
        ),
        (
            "started inside the folder allowed without asking",
            vec![("ops", ExecGrant::WithoutAsking), ("work", ExecGrant::Ask)],
            WakeRule::OwnerOnly,
            Some("ops"),
            "allow-once",
            "allowed without asking",
        ),
        (
            "started inside a folder not granted for commands",
            vec![("ops", ExecGrant::WithoutAsking), ("docs", ExecGrant::No)],
            WakeRule::OwnerOnly,
            Some("docs"),
            "reject",
            "refused: the folder it would start in is not granted for commands",
        ),
        (
            "started somewhere that is neither its own nor granted",
            vec![("ops", ExecGrant::WithoutAsking)],
            WakeRule::OwnerOnly,
            Some("not-granted"),
            "reject",
            "refused: it would start in a folder that is neither the agent's own nor granted",
        ),
    ] {
        let world = World::new().await;
        let cwd = cwd.map(|name| world.folder(name));
        world.script.write(
            json!({"own_host": false}),
            json!([{"steps": [leave(command("make", cwd.as_deref())), send("done")]}]),
        );
        // As Codex asks, with where the command would start.
        let mut local = world.settings(Harness::Codex);
        local.wake = wake;
        local.folders = grants
            .iter()
            .map(|(name, exec)| world.grant(name, false, *exec))
            .collect();
        let host = world.host(local).await;
        let request = world.ask(&host, "build").await;
        world.settled(&request).await;
        assert_eq!(world.leave(1).await, [expected], "{what}");
        let activity = world.activity();
        assert!(
            activity.last().is_some_and(|line| line.ends_with(noted)),
            "{what}: {activity:?}"
        );
        assert!(host.approvals().is_empty(), "{what}");
        host.shutdown().await;
    }
}

/// Plan M6, point 3: a harness that ends its turn when it is refused (Codex
/// does) leaves nothing to publish. The people in the chat are told that
/// the agent needed something its owner has not allowed, not only that it
/// had no answer.
#[tokio::test]
async fn a_turn_that_ends_with_nothing_after_a_refusal_says_so() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false}),
        json!([
            {"match": "outside", "steps": [
                leave(command("touch /etc/x", None)),
                {"stop": "cancelled"},
            ]},
            {"match": "nothing", "steps": [{"stop": "cancelled"}]},
        ]),
    );
    let host = world.host(world.settings(Harness::Codex)).await;
    let refused = world.ask(&host, "write outside").await;
    let row = world.settled(&refused).await;
    assert_eq!(row.failure.as_deref(), Some("not-allowed"));
    assert_eq!(
        world.replies(&refused),
        [
            "I could not finish: I needed something on my owner's machine that my owner has not allowed."
        ]
    );
    // Without a refusal, a turn that ends with nothing says only that.
    let empty = world.ask(&host, "say nothing").await;
    let row = world.settled(&empty).await;
    assert_eq!(row.failure.as_deref(), Some("no-reply"));
    host.shutdown().await;
}

/// ADR decision 13, for a coding harness: what the owner takes away while
/// bzz runs holds at once. The settings file is read again for every
/// answer.
#[tokio::test]
async fn what_the_owner_takes_away_from_a_coding_harness_is_refused_at_once() {
    let world = World::new().await;
    let work = world.folder("work");
    let gate = world.script.marker("gate");
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [
            leave(edit(&work.join("a.txt"))),
            leave(command("make", None)),
            {"touch": world.script.marker("asked")},
            {"wait_for": gate},
            leave(edit(&work.join("b.txt"))),
            leave(command("make", None)),
            send("done"),
        ]}]),
    );
    let mut local = world.settings(Harness::ClaudeCode);
    local.folders = vec![world.grant("work", true, ExecGrant::WithoutAsking)];
    let settings_file = world.temporary.path().join("config.toml");
    let write = |local: &LocalAgentConfig| {
        let config = bzz::config::Config {
            hosted_agents: vec![HostedAgentConfig {
                id: world.agent,
                label: "Scout".into(),
                pubkey: world.agent_pubkey(),
                community: Uuid::nil(),
                owner_identity: Uuid::nil(),
                auth_tag: "[]".into(),
                credential_hash: "c".repeat(64),
                allowed: Vec::new(),
                paused: false,
                local: Some(local.clone()),
            }],
            ..bzz::config::Config::default()
        };
        std::fs::write(&settings_file, toml::to_string_pretty(&config).unwrap()).unwrap();
    };
    write(&local);
    let host = world
        .host_configured(local.clone(), |options| {
            options.settings_file = Some(settings_file.clone());
        })
        .await;
    let request = world.ask(&host, "work").await;
    let asked = world.script.marker("asked");
    until("the first two answers", WAIT, || {
        asked.exists().then_some(())
    })
    .await;
    assert_eq!(world.leave(2).await, ["allow-once", "allow-once"]);
    // The owner takes the writing and the commands away, as `bzz agent
    // edit` does, while this bzz runs.
    let mut taken = local.clone();
    taken.folders = vec![world.grant("work", false, ExecGrant::No)];
    write(&taken);
    std::fs::write(&gate, "open").unwrap();
    world.settled(&request).await;
    assert_eq!(world.leave(4).await[2..], ["reject", "reject"]);
    host.shutdown().await;
}

// ---- starting the owner's own program ---------------------------------------

/// A program of the owner's that stands in for `grok`, `claude`, `codex` or
/// `node`: it answers `--version`, and otherwise becomes the scripted
/// harness with the arguments it was given.
fn stand_in(bin: &Path, name: &str, version: &str, script: &Script) -> PathBuf {
    std::fs::create_dir_all(bin).unwrap();
    let path = bin.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\nexec '{}' '{}' \"$@\"\n",
            scripted_harness().display(),
            script.path().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// bzz's own environment for a test: `bin` first on `PATH`. `alone`: and
/// nothing else on it, so that no program of the machine is found.
fn environment_on(bin: &Path, alone: bool) -> Environment {
    let mut folders = vec![bin.to_path_buf()];
    if !alone {
        folders.extend(["/usr/bin".into(), "/bin".into()]);
    }
    let path = std::env::join_paths(folders)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Arc::new(move |name| match name {
        "PATH" => Some(path.clone()),
        // What must never reach a harness.
        "BZZ_PASSPHRASE_FD" | "NOSTR_SECRET_KEY" | "XAI_API_KEY" | "ANTHROPIC_API_KEY"
        | "OPENAI_API_KEY" => Some("must not pass".into()),
        _ => None,
    })
}

/// Plan M6, points 1 and 4, D16 and D17: bzz starts the owner's own
/// installed program, as that program is started, only when its version is
/// one bzz knows to work. A missing program and a version out of range
/// start nothing, and say which version was found and which are known.
#[tokio::test]
async fn bzz_starts_the_owners_own_grok_and_only_a_version_it_knows() {
    let world = World::new().await;
    let bin = world.temporary.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    world.script.write(
        json!({"own_host": false, "modes": []}),
        json!([{"steps": [send("from grok")]}]),
    );
    // No `command` in the settings: the harness's own launch.
    let mut local = world.settings(Harness::Grok);
    local.command.clear();
    let environment = environment_on(&bin, false);
    let start = async |local: &LocalAgentConfig| {
        let environment = environment.clone();
        world
            .host_configured(local.clone(), move |options| {
                options.environment = environment;
            })
            .await
    };
    // No program at all: nothing starts, the owner sees what to do, and
    // the people in the chat are told the agent is not available.
    let host = start(&local).await;
    let request = world.ask(&host, "anyone there?").await;
    let row = world.settled(&request).await;
    assert_eq!(row.failure.as_deref(), Some("unavailable"));
    let problem = host.status()[0].problem.clone().unwrap_or_default();
    for stated in ["your own `grok`", "none was found on PATH", "BZZ_GROK"] {
        assert!(problem.contains(stated), "`{stated}` in: {problem}");
    }
    assert!(world.script.starts().is_empty(), "nothing was started");
    host.shutdown().await;

    // A version outside the range: the same, with both versions.
    stand_in(&bin, "grok", "grok 1.1.0 (abc) [stable]", &world.script);
    let host = start(&local).await;
    let request = world.ask(&host, "and now?").await;
    let row = world.settled(&request).await;
    assert_eq!(row.failure.as_deref(), Some("unavailable"));
    let problem = host.status()[0].problem.clone().unwrap_or_default();
    for stated in ["1.1.0", "1.0.77", "does not start it"] {
        assert!(problem.contains(stated), "`{stated}` in: {problem}");
    }
    assert!(world.script.starts().is_empty(), "nothing was started");
    host.shutdown().await;

    // The tested version: started as Grok is, in its working folder, with
    // its sandbox profile and nothing of bzz's. (On Linux Grok's sandbox
    // needs bubblewrap, which a machine that runs this test may lack.)
    stand_in(&bin, "bwrap", "bubblewrap 0.9.0", &world.script);
    let grok = stand_in(
        &bin,
        "grok",
        "grok 1.0.77 (50ca579fa996) [stable]",
        &world.script,
    );
    let host = start(&local).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    assert_eq!(world.replies(&request), ["from grok"]);
    let log = world.script.log();
    let started = &log
        .iter()
        .find(|entry| entry.get("start").is_some())
        .unwrap()["start"];
    assert_eq!(
        started["argv"],
        json!([
            scripted_harness(),
            world.script.path(),
            "agent",
            "--no-leader",
            "stdio"
        ]),
        "started by {}",
        grok.display()
    );
    assert_eq!(
        started["cwd"],
        json!(std::fs::canonicalize(world.workspace()).unwrap())
    );
    assert_eq!(started["env"]["GROK_SANDBOX"], coding::GROK_PROFILE);
    assert!(!started.to_string().contains("must not pass"), "{started}");
    assert!(world.workspace().join(".grok/sandbox.toml").is_file());
    host.shutdown().await;
}

/// Plan M6, point 2: Codex and Claude Code go through their adapter, which
/// bzz installs on first use and never uses half installed. The adapter is
/// started with Node and pointed at the owner's own program.
#[tokio::test]
async fn an_adapter_is_installed_at_the_first_start_and_pointed_at_the_owners_program() {
    for (harness, command, version, adapter, mode) in [
        (
            Harness::ClaudeCode,
            "claude",
            "2.1.322 (Claude Code)",
            &coding::CLAUDE_ADAPTER,
            "default",
        ),
        (
            Harness::Codex,
            "codex",
            "codex-cli 0.166.0",
            &coding::CODEX_ADAPTER,
            "workspace-write",
        ),
    ] {
        let world = World::new().await;
        let bin = world.temporary.path().join("bin");
        world.script.write(
            json!({"own_host": false}),
            json!([{"steps": [send("through the adapter")]}]),
        );
        let program = stand_in(&bin, command, version, &world.script);
        // Node runs the adapter: here it becomes the scripted harness.
        stand_in(&bin, "node", "v22.19.0", &world.script);
        let mut local = world.settings(harness);
        local.command.clear();
        let start = async |alone: bool| {
            let environment = environment_on(&bin, alone);
            world
                .host_configured(local.clone(), move |options| {
                    options.environment = environment;
                })
                .await
        };
        // The adapter is missing and cannot be installed (no npm anywhere
        // on PATH): nothing starts, and the owner is told what is missing.
        let host = start(true).await;
        let request = world.ask(&host, "anyone there?").await;
        let row = world.settled(&request).await;
        assert_eq!(row.failure.as_deref(), Some("unavailable"), "{harness:?}");
        let problem = host.status()[0].problem.clone().unwrap_or_default();
        assert!(
            problem.contains("npm was not found"),
            "{harness:?}: {problem}"
        );
        assert!(world.script.starts().is_empty() && !adapter.is_installed(&world.data_dir()));
        host.shutdown().await;

        // With npm: installed at the first start, from the lock file inside
        // bzz, without install scripts and without optional packages.
        let package = format!("node_modules/{}", adapter.package);
        std::fs::write(
            bin.join("npm"),
            format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 11.0.0; exit 0; fi\necho \"$@\" > npm-arguments\nmkdir -p {package}/dist && echo '// adapter' > {package}/dist/index.js && echo '{{\"version\": \"{}\"}}' > {package}/package.json\n",
                adapter.version
            ),
        )
        .unwrap();
        std::fs::set_permissions(bin.join("npm"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let host = start(false).await;
        let request = world.ask(&host, "hello").await;
        world.settled(&request).await;
        assert_eq!(
            world.replies(&request),
            ["through the adapter"],
            "{harness:?}"
        );
        assert!(adapter.is_installed(&world.data_dir()));
        let folder = adapter.dir(&world.data_dir());
        assert_eq!(
            std::fs::read_to_string(folder.join("npm-arguments"))
                .unwrap()
                .trim(),
            "ci --ignore-scripts --omit=optional --omit=dev --no-audit --no-fund"
        );
        for (path, content) in adapter.files {
            assert_eq!(
                &std::fs::read_to_string(folder.join(path)).unwrap(),
                content
            );
        }
        let log = world.script.log();
        let started = &log
            .iter()
            .find(|entry| entry.get("start").is_some())
            .unwrap()["start"];
        // `node <the adapter's entry>`, and the owner's own program by the
        // adapter's variable.
        assert_eq!(
            started["argv"],
            json!([
                scripted_harness(),
                world.script.path(),
                adapter.entry(&folder)
            ]),
            "{harness:?}"
        );
        assert_eq!(
            started["env"][adapter.program_variable],
            json!(std::fs::canonicalize(&program).unwrap()),
            "{harness:?}"
        );
        assert!(!started.to_string().contains("must not pass"), "{started}");
        // And its guard, on the session.
        assert_eq!(world.script.received("session/set_mode")[0]["modeId"], mode);
        host.shutdown().await;
    }
}

// ---- what the real programs sent --------------------------------------------

/// Where the transcripts recorded from the real programs are kept.
fn transcript(harness: Harness, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agents/transcripts")
        .join(harness.as_str())
        .join(format!("{name}.ndjson"))
}

/// The final messages a program sent through bzz's tool, in a transcript.
fn sent_in(transcript: &Path) -> Vec<String> {
    std::fs::read_to_string(transcript)
        .unwrap_or_else(|error| panic!("{}: {error}", transcript.display()))
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| {
            entry["tool"]["name"] == "send" && entry["tool"]["arguments"]["final"] == true
        })
        .filter_map(|entry| {
            entry["tool"]["arguments"]["text"]
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

/// Plan M6, "recorded transcripts": bzz's real runtime against what Grok,
/// Codex and Claude Code really sent over ACP, with only the pipe replaced.
/// For each program: a mention answered, what it asked leave for when it
/// was told to write inside and outside its folders, and, in a second
/// process, the session it saved loaded again.
///
/// The transcripts were recorded on the pinned relay with the owner's own
/// programs (`docs/validation-v0.15.0.md`, M6, says how to record them
/// again). The scripted harness plays the program's side: it sends what the
/// program sent, calls bzz's tools where the program did, and ends with
/// status 4 at the first message of bzz that is not what bzz sent then.
#[tokio::test]
async fn bzz_answers_what_each_real_program_sent_as_it_did_when_recorded() {
    for (harness, model, thinking) in [
        (Harness::Grok, "grok-4.5", Some("low")),
        (Harness::Codex, "gpt-5.6-luna", Some("low")),
        (Harness::ClaudeCode, "haiku", None),
    ] {
        let world = World::new().await;
        let (work, outside) = (world.folder("work"), world.folder("not-granted"));
        let (first, second) = (
            transcript(harness, "first-run"),
            transcript(harness, "after-restart"),
        );
        let replaying = |transcript: &Path| {
            world.script.write(
                json!({
                    "own_host": false,
                    "replay": transcript,
                    "folders": {"work": work},
                    "places": {"<not-granted>": outside},
                }),
                json!([]),
            );
        };
        // The settings the recorded agent had.
        let mut local = world.settings(harness);
        local.instructions = "Answer in one short sentence.".into();
        local.model = Some(AgentModelConfig {
            provider: harness.as_str().into(),
            model_id: model.into(),
            thinking_level: thinking.map(str::to_owned),
        });
        local.folders = vec![world.grant("work", true, ExecGrant::No)];
        let replayed = |what: &str| {
            let log = world.script.log();
            let mismatch = log.iter().find(|entry| entry["replay"] == "mismatch");
            assert!(
                mismatch.is_none(),
                "{harness:?}, {what}: bzz did not send what it sent when this was recorded:\n{:#}",
                mismatch.unwrap()
            );
        };
        let complete = |count: usize| {
            world
                .script
                .log()
                .iter()
                .filter(|entry| entry["replay"] == "complete")
                .count()
                == count
        };

        // ---- the first process: a mention, then the two writes ------------
        replaying(&first);
        let said = sent_in(&first);
        assert!(
            !said.is_empty(),
            "{harness:?}: the transcript holds no answer"
        );
        let host = world.host(local.clone()).await;
        let remember = world.ask(&host, "Remember the word heron.").await;
        world.settled(&remember).await;
        replayed("a mention");
        // Exactly the answer the program sent, once.
        assert_eq!(world.replies(&remember), [said[0].clone()], "{harness:?}");
        let write = world
            .ask(&host, "Create one file inside and one outside.")
            .await;
        let row = world.settled(&write).await;
        replayed("the two writes");
        let replies = world.replies(&write);
        assert_eq!(replies.len(), 1, "{harness:?}: {replies:?}");
        match said.get(1) {
            Some(text) => assert_eq!(&replies[0], text, "{harness:?}"),
            // The program ended its turn when it was refused (Codex).
            None => assert_eq!(row.failure.as_deref(), Some("not-allowed"), "{harness:?}"),
        }
        until("the first transcript to be played to its end", WAIT, || {
            complete(1).then_some(())
        })
        .await;
        host.shutdown().await;

        // ---- a second process: the saved session, loaded -------------------
        replaying(&second);
        let said = sent_in(&second);
        let host = world.host(local).await;
        let recall = world.message("Which word?", Some(&remember));
        world.offer(&host, &recall).await;
        world.settled(&recall).await;
        replayed("after a restart");
        assert_eq!(world.replies(&recall), [said[0].clone()], "{harness:?}");
        assert!(
            said[0].to_lowercase().contains("heron"),
            "{harness:?}: {said:?}"
        );
        // The session bzz kept for the conversation, not a new one.
        let loads = world.script.received("session/load");
        assert_eq!(loads.len(), 1, "{harness:?}");
        assert_eq!(
            loads[0]["sessionId"],
            world.script.received("session/prompt")[0]["sessionId"],
            "{harness:?}"
        );
        until(
            "the second transcript to be played to its end",
            WAIT,
            || complete(2).then_some(()),
        )
        .await;
        // Still one reply to each of the earlier two.
        assert_eq!(world.replies(&remember).len(), 1);
        assert_eq!(world.replies(&write).len(), 1);
        host.shutdown().await;
    }
}
