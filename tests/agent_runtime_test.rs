//! v0.15 M1: the agent runtime against a scripted harness
//! (`examples/scripted_harness.rs`) and a recorded relay. Each test names the
//! rule of ADR v0.15 local agents or of `docs/agent-host-protocol.md` that it
//! shows. Nothing here needs Node, a model or a network.
#![cfg(unix)]

mod support;

use std::{sync::Arc, time::Duration};

use bzz::{
    agents::{
        activity, git_credentials,
        runtime::{
            Limits,
            admission::{ChannelFacts, PlaceKind, Refusal},
            agent::{AgentSpec, Phase},
            approval::Answer,
            host::{AgentHost, HostOptions, Offer},
            install::StartReport,
            process::{self, Leftover},
        },
    },
    config::{
        AgentModelConfig, ExecGrant, FolderGrant, Harness, HostedAgentConfig, LocalAgentConfig,
        LocalAgentState, WakeRule,
    },
    store::{
        Store,
        agent_runtime::{InboxRow, NewRequest, ReplyOrigin, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, JsonUtil as _, Keys};
use serde_json::{Value, json};
use support::agent_harness::{Mode, RecordedRelay, Script, kill, running, until};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);

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
        // Longer than any test, but for the one that waits for it.
        checkpoint_idle: Duration::from_secs(600),
        agent_exchanges: 3,
        agent_exchange_window: Duration::from_secs(3600),
        prune_interval: Duration::from_secs(24 * 3600),
        // Longer than any test, but for the one that waits for it.
        approval: Duration::from_secs(600),
    }
}

/// One owner, one agent, one channel, one recorded relay, one database.
struct World {
    temporary: TempDir,
    owner: Keys,
    stranger: Keys,
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
            stranger: Keys::generate(),
            relay,
            store,
            agent: Uuid::new_v4(),
            channel: Uuid::new_v4(),
        };
        world.channel_is(PlaceKind::Channel, world.channel, true);
        world
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.temporary.path().join("data")
    }

    fn agent_pubkey(&self) -> String {
        self.relay.agent.public_key().to_hex()
    }

    fn channel_is(&self, kind: PlaceKind, channel: Uuid, with_stranger: bool) -> ChannelFacts {
        let mut members = vec![self.owner.public_key().to_hex(), self.agent_pubkey()];
        if with_stranger {
            members.push(self.stranger.public_key().to_hex());
        }
        let facts = ChannelFacts {
            id: channel,
            name: "general".into(),
            kind,
            members,
        };
        self.relay.set_channel(facts.clone());
        facts
    }

    fn settings(&self, harness: Harness) -> LocalAgentConfig {
        let mut local = LocalAgentConfig::new(harness);
        local.command = self.script.command();
        local.instructions = "Be brief.".into();
        local
    }

    fn spec(&self, local: LocalAgentConfig) -> AgentSpec {
        AgentSpec {
            id: self.agent,
            name: "Scout".into(),
            pubkey: self.agent_pubkey(),
            owner: self.owner.public_key().to_hex(),
            community: Uuid::nil(),
            local,
        }
    }

    async fn host_with(&self, local: LocalAgentConfig, limits: Limits) -> AgentHost {
        self.host_configured(local, limits, |_| {}).await
    }

    /// A host whose options `configure` may change: whether the owner is
    /// there to be asked, and what he stored for git.
    async fn host_configured(
        &self,
        local: LocalAgentConfig,
        limits: Limits,
        configure: impl FnOnce(&mut HostOptions),
    ) -> AgentHost {
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = limits;
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        configure(&mut options);
        let host = AgentHost::start(options).await.unwrap();
        host.add(self.spec(local), self.relay.clone()).await;
        host
    }

    /// A host with the owner at the TUI: a command may be put to him.
    async fn host_with_owner(&self, local: LocalAgentConfig, limits: Limits) -> AgentHost {
        self.host_configured(local, limits, |options| options.owner_present = true)
            .await
    }

    /// The settings of bzz's own host with folders granted as `grants`
    /// says: `(name, exec)`. The folders are made under the test's own.
    fn hands(&self, grants: &[(&str, ExecGrant)]) -> LocalAgentConfig {
        let mut local = self.settings(Harness::PiDurable);
        local.folders = grants
            .iter()
            .map(|(name, exec)| {
                let path = self.temporary.path().join("granted").join(name);
                std::fs::create_dir_all(&path).unwrap();
                FolderGrant {
                    name: (*name).to_owned(),
                    path,
                    write: false,
                    exec: *exec,
                }
            })
            .collect();
        local
    }

    /// What the harness was answered for each request of `method` it made
    /// with a `request` step, once there are `count`.
    async fn answers(&self, method: &str, count: usize) -> Vec<Value> {
        until(&format!("{count} answers to `{method}`"), WAIT, || {
            Some(
                self.script
                    .log()
                    .into_iter()
                    .filter(|entry| entry["to"] == method && entry.get("response").is_some())
                    .map(|entry| entry["response"].clone())
                    .collect::<Vec<_>>(),
            )
            .filter(|answers| answers.len() >= count)
        })
        .await
    }

    /// The agent's activity, as `bzz agent show` prints it, without the
    /// time each line starts with.
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

    /// A host running the agent on `harness` with the script as written.
    async fn host(&self, harness: Harness) -> AgentHost {
        self.host_with(self.settings(harness), limits()).await
    }

    /// A message in `channel`, by `author`, kept on the relay.
    fn message_in(
        &self,
        channel: Uuid,
        author: &Keys,
        text: &str,
        mention: bool,
        thread: Option<&Event>,
    ) -> Event {
        let agent = self.agent_pubkey();
        let mentions = if mention {
            vec![agent.as_str()]
        } else {
            Vec::new()
        };
        let thread = thread.map(|root| buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        });
        let event =
            buzz_sdk::build_message(channel, text, thread.as_ref(), &mentions, false, &[], &[])
                .unwrap()
                .sign_with_keys(author)
                .unwrap();
        self.relay.seed(&event);
        event
    }

    /// A top-level mention of the agent by its owner.
    fn mention(&self, text: &str) -> Event {
        self.message_in(self.channel, &self.owner, text, true, None)
    }

    async fn offer(&self, host: &AgentHost, event: &Event) -> Offer {
        let channel = bzz::protocol::events::channel_id(event).unwrap();
        let facts = bzz::agents::runtime::relay::AgentRelay::channel(self.relay.as_ref(), channel)
            .await
            .unwrap();
        host.offer(self.agent, event, &facts).await
    }

    /// Offers a mention and expects it to be admitted.
    async fn ask(&self, host: &AgentHost, text: &str) -> Event {
        let event = self.mention(text);
        assert_eq!(self.offer(host, &event).await, Offer::Queued);
        event
    }

    async fn row(&self, request: &Event) -> Option<InboxRow> {
        let (agent, id) = (self.agent, request.id.to_hex());
        self.store
            .call(move |store| store.agent_inbox_get(agent, &id))
            .await
            .unwrap()
    }

    /// Waits until the request has left the queue, its turn is over, and
    /// the relay has acknowledged its reply if it has one. (A request counts
    /// as replied from the moment its reply is stored, before the publish.)
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

    async fn state_is(&self, request: &Event, state: RequestState) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while self.row(request).await.map(|row| row.state) != Some(state) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "request never became {state:?}: {:?}",
                self.row(request).await
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The results of the tool `name`, once the harness has logged `count`
    /// of them. A request is settled when its reply is stored, which is
    /// before the harness reads the tool's answer.
    async fn results(&self, name: &str, count: usize) -> Vec<Value> {
        until(&format!("{count} results of `{name}`"), WAIT, || {
            Some(self.script.tool_results(name)).filter(|results| results.len() >= count)
        })
        .await
    }

    fn contents(&self, events: &[Event]) -> Vec<String> {
        events.iter().map(|event| event.content.clone()).collect()
    }
}

fn send(text: &str, is_final: bool) -> Value {
    json!({"tool": "send", "arguments": {"text": text, "final": is_final}})
}

fn has_tag(event: &Event, expected: &[&str]) -> bool {
    event.tags.iter().any(|tag| {
        tag.as_slice()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            == expected
    })
}

// ---- the protocol ---------------------------------------------------------

/// ADR decisions 4 and 5, protocol sections 3 and 5: a mention reaches bzz's
/// own host with the place and the request as fields, the host answers
/// through `_bzz/tool`, and bzz publishes one reply signed by the agent with
/// the owner's tag. The typing indicator shows while the turn runs.
#[tokio::test]
async fn a_mention_is_answered_once_through_the_tool_over_the_pipe() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [{"sleep_ms": 200}, send("17 times 23 is 391.", true)]}]),
    );
    let mut local = world.settings(Harness::PiDurable);
    local.model = Some(AgentModelConfig {
        provider: "openai-codex".into(),
        model_id: "gpt-6-luna".into(),
        thinking_level: Some("low".into()),
    });
    local.folders = vec![FolderGrant::read_only("site", "/home/ann/site")];
    local.network = vec!["github.com".into()];
    let host = world.host_with(local, limits()).await;
    let earlier = world.message_in(world.channel, &world.owner, "unrelated", false, None);
    let request = world.ask(&host, "what is 17 times 23?").await;
    let row = world.settled(&request).await;
    assert_eq!(
        (row.state, row.attempts, row.effects),
        (RequestState::Replied, 1, 0)
    );

    // One reply on the relay: the agent's, threaded under the mention.
    let replies = world.relay.replies_to(&request);
    assert_eq!(world.contents(&replies), ["17 times 23 is 391."]);
    let reply = &replies[0];
    let request_id = request.id.to_hex();
    assert!(has_tag(reply, &["h", &world.channel.to_string()]));
    assert!(
        has_tag(reply, &["e", &request_id, "", "reply"]),
        "{:?}",
        reply.tags
    );
    let auth = reply
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
        .expect("the owner's tag");
    assert_eq!(
        buzz_sdk::nip_oa::verify_auth_tag(
            &serde_json::to_string(auth.as_slice()).unwrap(),
            &reply.pubkey
        )
        .unwrap(),
        world.owner.public_key()
    );
    assert_eq!(world.relay.calls(9).len(), 1, "published once");
    let stored = world
        .store
        .call({
            let agent = world.agent;
            let id = request_id.clone();
            move |store| store.agent_reply_get(agent, &id)
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (stored.event_id.as_str(), stored.origin, stored.published),
        (reply.id.to_hex().as_str(), ReplyOrigin::Send, true)
    );
    assert_eq!(
        world.results("send", 1).await,
        [json!({"text": format!("sent as {}", reply.id.to_hex()), "final": true})]
    );

    // What the host was told, as the protocol document has it.
    let initialize = &world.script.received("initialize")[0];
    assert_eq!(initialize["protocolVersion"], 1);
    assert_eq!(initialize["clientCapabilities"], json!({}));
    assert_eq!(
        initialize["clientInfo"],
        json!({"name": "bzz", "version": env!("CARGO_PKG_VERSION")})
    );
    assert_eq!(
        initialize["_meta"]["bzz"],
        json!({
            "agent": {"id": world.agent, "pubkey": world.agent_pubkey(), "name": "Scout"},
            "model": {"provider": "openai-codex", "modelId": "gpt-6-luna", "thinkingLevel": "low"},
            "instructions": "Be brief.",
            "mounts": [{"name": "site", "path": "/home/ann/site", "write": false, "exec": false}],
            "network": ["github.com"],
            "packages": true,
            "piDir": "/nonexistent/pi/agent",
        })
    );
    let place = format!("{}/{request_id}", world.channel);
    assert_eq!(
        world.script.received("session/new"),
        [json!({"cwd": "/home/agent", "mcpServers": [], "_meta": {"bzz/place": place}})]
    );
    let prompts = world.script.received("session/prompt");
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["_meta"], json!({"bzz/request": request_id}));
    let text = prompts[0]["prompt"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("what is 17 times 23?") && text.contains("(your owner)"),
        "{text}"
    );
    assert!(text.contains("a thread in the channel #general"), "{text}");
    assert!(
        !text.contains("unrelated"),
        "another thread's messages are not this conversation"
    );
    assert!(
        !text.contains("Be brief."),
        "bzz's own host has the instructions already"
    );
    let _ = earlier;

    // The typing indicator, in the mention's thread, while the turn ran.
    let typing = world.relay.calls(20_002);
    assert!(!typing.is_empty(), "a typing indicator was published");
    let now = nostr::Timestamp::now().as_secs();
    let signal = bzz::agents::typing::parse(&typing[0], world.channel, now).unwrap();
    assert_eq!(
        signal.scope,
        bzz::agents::typing::TypingScope::Thread(request_id)
    );
    // It stops with the final reply.
    let count = world.relay.calls(20_002).len();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(world.relay.calls(20_002).len(), count);
    host.shutdown().await;
}

/// Protocol section 6: another harness gets its tools from `bzz agent-tools`,
/// started with a credential made for that session. A call is bound to the
/// calling session's live turn; a credential of a process that exited, or one
/// bzz never made, is refused.
#[tokio::test]
async fn another_harness_calls_bzz_agent_tools_with_a_credential_per_session() {
    let world = World::new().await;
    let gate = world.script.marker("gate");
    world.script.write(
        json!({"own_host": false}),
        json!([
            {"match": "first", "steps": [send("one", true)]},
            {"match": "second", "steps": [{"touch": world.script.marker("second-live")}, {"wait_for": gate}, send("two", true)]},
        ]),
    );
    let host = world.host(Harness::Codex).await;
    let first = world.ask(&host, "first question").await;
    world.settled(&first).await;
    assert_eq!(world.contents(&world.relay.replies_to(&first)), ["one"]);

    // The server bzz listed, and what it lists.
    let opened = &world.script.received("session/new")[0];
    let server = &opened["mcpServers"][0];
    assert_eq!(server["name"], "buzz");
    assert_eq!(server["command"], env!("CARGO_BIN_EXE_bzz"));
    assert_eq!(server["args"], json!(["agent-tools"]));
    assert_eq!(server["env"][0]["name"], "BZZ_AGENT_TOOLS");
    let credential_of = |opened: &Value| {
        let target = opened["mcpServers"][0]["env"][0]["value"]
            .as_str()
            .unwrap()
            .to_owned();
        let (address, credential) = target.split_once('#').unwrap();
        assert_eq!(address, host.tools_address().to_string());
        credential.to_owned()
    };
    let first_credential = credential_of(opened);
    let listed = world
        .script
        .log()
        .into_iter()
        .find(|entry| entry["mcp"] == "tools/list")
        .expect("the harness listed the tools");
    let names = listed["answer"]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, ["send", "read", "react"]);
    // A coding harness never received the instructions, so the prompt has them.
    let text = world.script.received("session/prompt")[0]["prompt"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        text.contains("Be brief.") && text.contains("`send` tool"),
        "{text}"
    );
    // Its session id is kept for a later `session/load`.
    let place = format!("{}/{}", world.channel, first.id.to_hex());
    let stored = world
        .store
        .call({
            let agent = world.agent;
            move |store| store.agent_session(agent, &place)
        })
        .await
        .unwrap();
    assert!(stored.is_some());

    // A second conversation, live and waiting.
    let second = world.ask(&host, "second question").await;
    let live = world.script.marker("second-live");
    until("the second turn to be live", WAIT, || {
        live.exists().then_some(())
    })
    .await;
    let sessions = world.script.received("session/new");
    assert_eq!(sessions.len(), 2);
    let second_credential = credential_of(&sessions[1]);
    assert_ne!(
        first_credential, second_credential,
        "one credential per session"
    );

    let call = |credential: String, arguments: Value| {
        let url = format!("http://{}/v1/tool", host.tools_address());
        async move {
            let response = reqwest::Client::new()
                .post(url)
                .bearer_auth(credential)
                .json(&json!({"name": "send", "arguments": arguments}))
                .send()
                .await
                .unwrap();
            (
                response.status().as_u16(),
                response.json::<Value>().await.unwrap(),
            )
        }
    };
    // The first session's credential, while only the second has a live turn:
    // it cannot answer the second session's mention.
    let (status, body) = call(
        first_credential.clone(),
        json!({"text": "sneak", "final": true}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["isError"], true, "{body}");
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .contains("No message is being answered in this session")
    );
    // Nor can any argument say which mention is answered.
    let (_, body) = call(
        first_credential.clone(),
        json!({"text": "sneak", "final": true, "request": second.id.to_hex()}),
    )
    .await;
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .contains("no argument `request`"),
        "{body}"
    );
    // A credential bzz never made, and a v0.13 broker credential.
    for foreign in [
        format!("bzzt_{}", "0".repeat(64)),
        bzz::broker::host::new_credential(),
    ] {
        let (status, body) = call(foreign, json!({"text": "sneak", "final": true})).await;
        assert_eq!((status, body["isError"].clone()), (401, json!(true)));
    }
    assert!(
        world.relay.replies_to(&second).is_empty(),
        "nothing was sneaked in"
    );
    assert_eq!(world.relay.calls(9).len(), 1);

    std::fs::write(&gate, "open").unwrap();
    world.settled(&second).await;
    assert_eq!(world.contents(&world.relay.replies_to(&second)), ["two"]);

    // The process exits: both credentials are revoked at once.
    let pid = world.script.starts()[0];
    host.shutdown().await;
    assert!(!running(pid));
    assert!(
        host.credentials().is_empty(),
        "no credential outlives the process"
    );
    for revoked in [first_credential, second_credential] {
        let (status, body) = call(revoked, json!({"text": "late", "final": true})).await;
        assert_eq!(status, 401, "{body}");
    }
    assert_eq!(world.relay.calls(9).len(), 2);
}

/// D12 and the protocol's `session/new`: several conversations run at once
/// as sessions of one process, and each answer lands in its own thread.
#[tokio::test]
async fn several_sessions_run_at_once_in_one_process() {
    let world = World::new().await;
    let (alpha, beta) = (world.script.marker("alpha"), world.script.marker("beta"));
    // Each turn waits for the other to be live before it answers.
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "alpha", "steps": [{"touch": alpha}, {"wait_for": beta}, send("answer alpha", true)]},
            {"match": "beta", "steps": [{"touch": beta}, {"wait_for": alpha}, send("answer beta", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let first = world.ask(&host, "question alpha").await;
    let second = world.ask(&host, "question beta").await;
    world.settled(&first).await;
    world.settled(&second).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&first)),
        ["answer alpha"]
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&second)),
        ["answer beta"]
    );
    assert_eq!(world.script.starts().len(), 1, "one process for both");
    let sessions = world.script.received("session/new");
    assert_eq!(sessions.len(), 2);
    assert_ne!(
        sessions[0]["_meta"]["bzz/place"],
        sessions[1]["_meta"]["bzz/place"]
    );
    // The replies are out; the harness ends its two turns a moment later.
    let status = until("both turns to end", WAIT, || {
        Some(host.status().remove(0)).filter(|status| status.turns == 0)
    })
    .await;
    assert_eq!(status.phase, Phase::Running);
    assert!(
        status.line().contains("Scout · pi-durable · ready"),
        "{}",
        status.line()
    );
    host.shutdown().await;
}

/// Task rule 7: a message for a busy conversation waits in bzz's inbox and
/// is handed to the harness as a new prompt in the same session when the
/// first ends.
#[tokio::test]
async fn a_message_for_a_busy_conversation_is_the_next_prompt_of_the_same_session() {
    let world = World::new().await;
    let gate = world.script.marker("gate");
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"steps": [{"touch": world.script.marker("live")}, {"wait_for": gate}, send("first answer", true)]},
            {"steps": [send("second answer", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let first = world.ask(&host, "first").await;
    let live = world.script.marker("live");
    until("the first turn to be live", WAIT, || {
        live.exists().then_some(())
    })
    .await;
    // In the same thread, while the first turn runs.
    let second = world.message_in(world.channel, &world.owner, "and also", true, Some(&first));
    assert_eq!(world.offer(&host, &second).await, Offer::Queued);
    assert_eq!(
        world.offer(&host, &second).await,
        Offer::Duplicate,
        "offered twice, admitted once"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        world.row(&second).await.unwrap().state,
        RequestState::Queued
    );
    assert_eq!(
        world.script.received("session/prompt").len(),
        1,
        "one prompt in flight per session"
    );

    std::fs::write(&gate, "open").unwrap();
    world.settled(&first).await;
    world.settled(&second).await;
    assert_eq!(world.script.received("session/new").len(), 1);
    let prompts = world.script.received("session/prompt");
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0]["sessionId"], prompts[1]["sessionId"]);
    assert_eq!(prompts[1]["_meta"]["bzz/request"], second.id.to_hex());
    // The second prompt shows the conversation so far, the first reply too.
    let text = prompts[1]["prompt"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("first answer") && text.contains("- you ("),
        "{text}"
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&second)),
        ["second answer"]
    );
    host.shutdown().await;
}

/// Protocol section 4: a request bzz does not know gets -32601, a
/// notification in an unknown namespace and an update of an unknown kind are
/// ignored, and a permission request is allowed only for bzz's chat tools.
#[tokio::test]
async fn unknown_requests_are_refused_unknown_notifications_ignored_permissions_ruled() {
    let world = World::new().await;
    let options = json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "always", "name": "Always", "kind": "allow_always"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ]);
    let ask = |title: &str, kind: &str| {
        json!({"request": {"method": "session/request_permission", "params": {
            "sessionId": "$session",
            "toolCall": {"toolCallId": "t1", "title": title, "kind": kind},
            "options": options,
        }}})
    };
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            {"notify": {"method": "_x.ai/session_state", "params": {"busy": true}}},
            {"update": {"sessionUpdate": "usage_update", "used": 12}},
            {"request": {"method": "fs/read_text_file", "params": {"path": "/etc/passwd"}}},
            {"request": {"method": "_bzz/unknown", "params": {}}},
            ask("mcp__buzz__send", "other"),
            ask("rm -rf /", "execute"),
            {"request": {"method": "_bzz/tool", "params": {"name": "send"}}},
            send("still here", true),
        ]}]),
    );
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["still here"]
    );
    let responses = world
        .script
        .log()
        .into_iter()
        .filter(|entry| entry.get("response").is_some())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 5);
    for unknown in &responses[..2] {
        assert_eq!(unknown["response"]["error"]["code"], -32601, "{unknown}");
    }
    assert_eq!(
        responses[2]["response"]["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "allow"}})
    );
    assert_eq!(
        responses[3]["response"]["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "reject"}})
    );
    assert_eq!(responses[4]["response"]["result"]["isError"], true);
    host.shutdown().await;
}

// ---- tools ----------------------------------------------------------------

/// Protocol section 5: a wrong call is refused with the right name and
/// nothing is sent; `message` is accepted for `text`; `read` returns the
/// conversation newest last; `react` reacts to the request by default and
/// only to messages of the place.
#[tokio::test]
async fn tools_check_their_arguments_and_act_only_in_the_place() {
    let world = World::new().await;
    let elsewhere = Uuid::new_v4();
    world.channel_is(PlaceKind::Channel, elsewhere, false);
    let outside = world.message_in(elsewhere, &world.owner, "in another channel", false, None);
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            {"tool": "send", "arguments": {"msg": "guessed", "final": true}},
            {"tool": "send", "arguments": {"text": 5}},
            {"tool": "post", "arguments": {"text": "x"}},
            {"tool": "read", "arguments": {"limit": 500}},
            {"tool": "read", "arguments": {}},
            {"tool": "react", "arguments": {"emoji": "👀"}},
            {"tool": "react", "arguments": {"emoji": "👍", "event": outside.id.to_hex()}},
            {"tool": "read", "arguments": {"before": outside.id.to_hex()}},
            {"tool": "send", "arguments": {"message": "On it.", "final": true}},
        ]}]),
    );
    let host = world.host(Harness::PiDurable).await;
    let request = world.mention("please look");
    // A later second, so that the order of the two does not rest on ids.
    let earlier = buzz_sdk::build_message(
        world.channel,
        "an earlier reply",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: request.id,
            parent_event_id: request.id,
        }),
        &[],
        false,
        &[],
        &[],
    )
    .unwrap()
    .custom_created_at(nostr::Timestamp::from_secs(
        request.created_at.as_secs() + 1,
    ))
    .sign_with_keys(&world.stranger)
    .unwrap();
    world.relay.seed(&earlier);
    assert_eq!(world.offer(&host, &request).await, Offer::Queued);
    world.settled(&request).await;

    let sends = world.results("send", 3).await;
    let text = |result: &Value| result["text"].as_str().unwrap().to_owned();
    assert!(sends[0]["isError"] == true && text(&sends[0]).contains("no argument `msg`"));
    assert!(
        text(&sends[0]).contains("`text` (string, required)")
            && text(&sends[0]).ends_with("Nothing was sent.")
    );
    assert!(text(&sends[1]).contains("`text` of `send` must be a string"));
    assert_eq!(
        sends[2]["final"], true,
        "`message` is another name for `text`"
    );
    assert!(text(&world.script.tool_results("post")[0]).contains("no tool `post`"));
    let reads = world.script.tool_results("read");
    assert!(text(&reads[0]).contains("1 to 100"));
    let transcript = text(&reads[1]);
    let lines = transcript.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{transcript}");
    assert!(
        lines[0].contains("please look") && lines[1].contains("an earlier reply"),
        "newest last"
    );
    assert!(lines[1].contains(&earlier.id.to_hex()));
    assert!(
        text(&reads[2]).contains("no message") || text(&reads[2]).contains("There is no message")
    );
    let reacts = world.script.tool_results("react");
    assert!(reacts[0].get("isError").is_none(), "{}", reacts[0]);
    assert!(reacts[1]["isError"] == true && text(&reacts[1]).contains("No reaction was added."));

    // One message and one reaction, both to the request, nothing else.
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["On it."]
    );
    assert_eq!(world.relay.calls(9).len(), 1);
    let reactions = world.relay.by_agent(7);
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].content, "👀");
    assert!(bzz::protocol::events::event_references(&reactions[0]).contains(&request.id.to_hex()));
    let row = world.row(&request).await.unwrap();
    assert_eq!(
        (row.state, row.effects),
        (RequestState::Replied, 1),
        "the reaction is counted"
    );
    host.shutdown().await;
}

/// ADR decision 5: a second final `send` for the same mention returns the
/// stored reply and publishes nothing; after the final reply the turn posts
/// nothing more.
#[tokio::test]
async fn a_harness_that_sends_twice_yields_one_reply() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            send("first wording", true),
            send("second wording", true),
            send("an afterthought", false),
            {"tool": "react", "arguments": {"emoji": "🎉"}},
            {"text": "I answered in the chat."},
        ]}]),
    );
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let replies = world.relay.replies_to(&request);
    assert_eq!(world.contents(&replies), ["first wording"]);
    assert_eq!(
        world.relay.calls(9).len(),
        1,
        "the second was never published"
    );
    assert!(world.relay.by_agent(7).is_empty());
    let results = world.results("send", 3).await;
    let id = replies[0].id.to_hex();
    assert_eq!(
        results[0],
        json!({"text": format!("sent as {id}"), "final": true})
    );
    assert_eq!(results[1]["final"], true);
    assert!(
        results[1]["text"]
            .as_str()
            .unwrap()
            .starts_with(&format!("already sent as {id}")),
        "{}",
        results[1]
    );
    assert_eq!(results[2]["isError"], true, "{}", results[2]);
    assert_eq!(world.results("react", 1).await[0]["isError"], true);
    host.shutdown().await;
}

/// Rule 11: bzz enforces. A turn posts only so much besides its final reply.
#[tokio::test]
async fn a_turn_posts_only_so_much_besides_its_final_reply() {
    let world = World::new().await;
    let steps = (0..6)
        .map(|index| send(&format!("progress {index}"), false))
        .collect::<Vec<_>>();
    world
        .script
        .write(json!({"own_host": true}), json!([{"steps": steps}]));
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "hello").await;
    let row = world.settled(&request).await;
    assert_eq!(
        world.relay.calls(9).len(),
        3,
        "the limit of the test's settings"
    );
    assert!(
        world.script.tool_results("send")[3]["text"]
            .as_str()
            .unwrap()
            .contains("as much as one turn may")
    );
    assert_eq!(row.state, RequestState::Ended);
    host.shutdown().await;
}

// ---- how a turn ends ------------------------------------------------------

/// ADR decision 5: a turn that ends with no `send` publishes the harness's
/// last plain text once, through the ledger.
#[tokio::test]
async fn a_turn_without_a_send_publishes_the_last_plain_text_once() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [
            {"text": "Let me think."},
            {"update": {"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Searching", "kind": "search"}},
            {"text": "17 times 23 "},
            {"text": "is 391."},
        ]}]),
    );
    let host = world.host(Harness::Codex).await;
    let request = world.ask(&host, "what is 17 times 23?").await;
    let row = world.settled(&request).await;
    assert_eq!(row.state, RequestState::Replied);
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["17 times 23 is 391."]
    );
    assert_eq!(world.relay.calls(9).len(), 1);
    let stored = world
        .store
        .call({
            let (agent, id) = (world.agent, request.id.to_hex());
            move |store| store.agent_reply_get(agent, &id)
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!((stored.origin, stored.published), (ReplyOrigin::Text, true));
    host.shutdown().await;
}

/// ADR decision 5 and M0's finding with Claude Code: plain text is never
/// published when a `send` happened in the turn, final or not.
#[tokio::test]
async fn plain_text_is_not_published_when_a_send_happened() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "progress", "steps": [send("On it.", false), {"text": "I told them I am on it."}]},
            {"match": "final", "steps": [send("Done.", true), {"text": "I answered ann in the team chat."}]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let progress = world.ask(&host, "progress only").await;
    let done = world.ask(&host, "final then text").await;
    let row = world.settled(&progress).await;
    world.settled(&done).await;
    assert_eq!(
        (row.state, row.failure, row.effects),
        (RequestState::Ended, None, 1)
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&progress)),
        ["On it."]
    );
    assert_eq!(world.contents(&world.relay.replies_to(&done)), ["Done."]);
    assert_eq!(
        world.relay.calls(9).len(),
        2,
        "no plain text and no failure notice"
    );
    host.shutdown().await;
}

/// Task rule 8: with no `send` and no text the turn fails with a notice, and
/// every failure is named by its kind.
#[tokio::test]
async fn a_turn_that_fails_posts_one_notice_named_by_kind() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "silence", "steps": []},
            {"match": "refusal", "steps": [{"stop": "refusal"}]},
            {"match": "sign-in", "steps": [{"error": {"code": -32000, "message": "token sk-secret-1234 has expired"}}]},
            {"match": "limit", "steps": [{"text": "partial"}, {"error": {"code": -32603, "message": "You have hit your usage limit"}}]},
            {"match": "provider", "steps": [send("working on it", false), {"error": {"code": -32603, "message": "upstream connect error at 10.1.2.3"}}]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let expected = [
        ("silence", "no-reply", "I finished without an answer."),
        ("refusal", "refused", "I could not answer this request."),
        (
            "sign-in",
            "sign-in-needed",
            "I could not answer: my owner needs to sign in to my model again.",
        ),
        (
            "limit",
            "limit-reached",
            "I could not answer: my model's usage limit was reached. Try again later.",
        ),
        (
            "provider",
            "provider-error",
            "I could not answer: my model returned an error.",
        ),
    ];
    let mut requests = Vec::new();
    for (word, ..) in expected {
        requests.push(world.ask(&host, &format!("please test {word}")).await);
    }
    for (request, (word, kind, notice)) in requests.iter().zip(expected) {
        let row = world.settled(request).await;
        assert_eq!(
            (row.state, row.failure.as_deref()),
            (RequestState::Failed, Some(kind)),
            "{word}"
        );
        let replies = world.contents(&world.relay.replies_to(request));
        assert_eq!(replies.last().map(String::as_str), Some(notice), "{word}");
        assert_eq!(
            replies.len(),
            if word == "provider" { 2 } else { 1 },
            "{word}: {replies:?}"
        );
    }
    // The harness's own words never leave bzz.
    let published = world.contents(&world.relay.by_agent(9)).join("\n");
    assert!(!published.contains("sk-secret-1234") && !published.contains("10.1.2.3"));
    assert!(
        !published.contains("partial"),
        "text of a failed turn is not an answer"
    );
    host.shutdown().await;
}

/// Task rule 8: a turn has a time limit. A harness that honours the cancel
/// keeps its process and its session; one that does not is stopped.
#[tokio::test]
async fn a_turn_has_a_time_limit() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "slow", "steps": [{"text": "thinking"}, {"stall": true}, send("too late", true)]},
            {"match": "quick", "steps": [send("quick answer", true)]},
            {"match": "deaf", "steps": [{"stall": true, "deaf": true}]},
            {"match": "after", "steps": [send("back again", true)]},
        ]),
    );
    let mut short = limits();
    short.turn = Duration::from_millis(600);
    let host = world
        .host_with(world.settings(Harness::PiDurable), short)
        .await;
    let slow = world.ask(&host, "slow question").await;
    let row = world.settled(&slow).await;
    assert_eq!(
        (row.state, row.failure.as_deref()),
        (RequestState::Failed, Some("timeout"))
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&slow)),
        ["I could not answer in time, and stopped."]
    );
    assert_eq!(world.script.received("session/cancel").len(), 1);
    // The process and the session survived: the next message uses both.
    let quick = world.message_in(world.channel, &world.owner, "quick one", true, Some(&slow));
    assert_eq!(world.offer(&host, &quick).await, Offer::Queued);
    world.settled(&quick).await;
    assert_eq!(world.script.starts().len(), 1);
    assert_eq!(world.script.received("session/new").len(), 1);

    // A harness that ignores the cancel is stopped, and started again later.
    let deaf = world.ask(&host, "deaf question").await;
    let row = world.settled(&deaf).await;
    assert_eq!(row.failure.as_deref(), Some("timeout"));
    let first_pid = world.script.starts()[0];
    until("the deaf harness to be stopped", WAIT, || {
        (!running(first_pid)).then_some(())
    })
    .await;
    let after = world.ask(&host, "after that").await;
    world.settled(&after).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&after)),
        ["back again"]
    );
    assert_eq!(world.script.starts().len(), 2);
    host.shutdown().await;
}

// ---- a harness that misbehaves --------------------------------------------

/// Task rule 3: a harness that floods its output is read without blocking
/// bzz or keeping what it sent, and its turn still ends.
#[tokio::test]
async fn a_flooding_harness_does_not_block_bzz() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "flood", "steps": [
                {"touch": world.script.marker("flooding")},
                {"flood": {"chunks": 60_000, "garbage": 500, "long_line_bytes": 6_000_000}},
                {"flood": {"chunks": 60_000}},
                {"stop": "end_turn"},
            ]},
            {"match": "other", "steps": [send("answered meanwhile", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let flood = world.ask(&host, "flood please").await;
    let flooding = world.script.marker("flooding");
    until("the flood to start", WAIT, || {
        flooding.exists().then_some(())
    })
    .await;
    // Another conversation of the same process is served during the flood,
    // and bzz itself stays responsive.
    let other = world.ask(&host, "other question").await;
    let status = tokio::time::timeout(Duration::from_secs(1), async { host.status() }).await;
    assert!(status.is_ok());
    world.settled(&other).await;
    let row = world.settled(&flood).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&other)),
        ["answered meanwhile"]
    );
    // The flood was plain text and no `send`: its last text is the reply,
    // capped, not 720 kB of it.
    assert_eq!(row.state, RequestState::Replied);
    let reply = &world.relay.replies_to(&flood)[0];
    assert!(
        reply.content.starts_with("flood flood") && reply.content.len() <= 16_000,
        "{}",
        reply.content.len()
    );
    assert_eq!(
        world.script.starts().len(),
        1,
        "the harness was not stopped for it"
    );
    host.shutdown().await;
}

/// Task rule 9: a harness that keeps dying is not restarted forever, and the
/// requests it could not take get one notice each.
#[tokio::test]
async fn a_harness_that_keeps_dying_is_not_restarted_forever() {
    let world = World::new().await;
    world
        .script
        .write(json!({"own_host": true, "initialize": "exit"}), json!([]));
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "anyone there?").await;
    let row = world.settled(&request).await;
    assert_eq!(
        (row.state, row.failure.as_deref(), row.attempts),
        (RequestState::Failed, Some("unavailable"), 0)
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["I am not available right now: my process keeps stopping."]
    );
    assert_eq!(world.script.starts().len(), 3, "three starts, then no more");
    assert_eq!(host.status()[0].phase, Phase::Unavailable);
    // A later request does not start it a fourth time.
    let later = world.ask(&host, "and now?").await;
    world.settled(&later).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(world.script.starts().len(), 3);
    assert_eq!(world.relay.replies_to(&later).len(), 1);
    assert!(host.status()[0].line().contains("unavailable"));
    host.shutdown().await;
}

/// Task rule 3: a harness that never answers `initialize`, or answers with
/// another protocol version, is stopped and counted, not waited for.
#[tokio::test]
async fn a_harness_that_stalls_at_start_is_stopped_and_counted() {
    for (initialize, problem) in [
        ("stall", "did not answer in time"),
        ("version", "another version"),
    ] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": true, "initialize": initialize}),
            json!([]),
        );
        let mut short = limits();
        short.start = Duration::from_millis(300);
        short.max_crashes = 2;
        let host = world
            .host_with(world.settings(Harness::PiDurable), short)
            .await;
        let request = world.ask(&host, "hello").await;
        let row = world.settled(&request).await;
        assert_eq!(row.failure.as_deref(), Some("unavailable"), "{initialize}");
        let starts = world.script.starts();
        assert_eq!(starts.len(), 2, "{initialize}");
        for pid in starts {
            assert!(
                !running(pid),
                "{initialize}: a harness that did not start is not left running"
            );
        }
        assert!(
            host.status()[0]
                .problem
                .as_deref()
                .unwrap()
                .contains(problem),
            "{:?}",
            host.status()[0]
        );
        host.shutdown().await;
    }
}

/// Protocol section 2 and rule 10: the harness runs in its own process
/// group, with an environment built from nothing and an ownership record,
/// and stopping it stops everything it started.
#[tokio::test]
async fn the_harness_gets_a_built_environment_a_record_and_a_group_stop() {
    let world = World::new().await;
    let child_file = world.script.marker("child");
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [{"spawn": ["sleep", "600"], "pid_file": child_file}, send("started one", true)]}]),
    );
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "start something").await;
    world.settled(&request).await;
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
    assert_eq!(
        names,
        ["HOME", "LANG", "PATH", "TMPDIR"],
        "nothing of bzz's own environment"
    );
    let folder = process::agent_dir(&world.data_dir(), world.agent);
    assert_eq!(
        start["start"]["env"]["HOME"],
        json!(folder),
        "bzz's own host lives in the agent's folder"
    );
    assert_eq!(start["start"]["cwd"], json!(folder));
    assert_eq!(
        start["start"]["argv"].as_array().unwrap().last().unwrap(),
        &json!(folder),
        "`<host> <agent folder>`"
    );

    let pid = world.script.starts()[0];
    let record = process::read_record(&process::run_dir(&world.data_dir()), world.agent).unwrap();
    assert_eq!(
        (record.process.pid, record.owner.pid),
        (pid, std::process::id())
    );
    assert_eq!(host.status()[0].pid, Some(pid));
    let child = std::fs::read_to_string(&child_file)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert!(running(pid) && running(child));

    host.shutdown().await;
    assert!(!running(pid), "the harness left when its input closed");
    until("what it started to be gone", WAIT, || {
        (!running(child)).then_some(())
    })
    .await;
    assert!(process::read_record(&process::run_dir(&world.data_dir()), world.agent).is_none());
    assert!(
        world.script.log().iter().any(|entry| entry["eof"] == true),
        "stopped by closing its input"
    );
    assert!(
        std::fs::read_to_string(folder.join("harness.log"))
            .unwrap()
            .contains("scripted harness")
    );
}

// ---- after a crash --------------------------------------------------------

/// Task rule 9, killed before the publish: nothing of the turn was
/// published, so the request is handed over again, with a warning, and is
/// answered once. This holds for a harness that cannot tell a request too.
#[tokio::test]
async fn killed_before_the_publish_the_request_is_handed_over_again() {
    for harness in [Harness::PiDurable, Harness::ClaudeCode] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": harness == Harness::PiDurable}),
            json!([
                {"steps": [{"text": "starting"}, {"exit": 1}]},
                {"resume": true, "steps": [send("answered after the crash", true)]},
                // An unseen request after a crash would take this one.
                {"steps": [send("answered after the crash", true)]},
            ]),
        );
        let host = world.host(harness).await;
        let request = world.ask(&host, "hello").await;
        let row = world.settled(&request).await;
        assert_eq!(
            (row.state, row.attempts),
            (RequestState::Replied, 2),
            "{harness:?}"
        );
        assert_eq!(
            world.contents(&world.relay.replies_to(&request)),
            ["answered after the crash"]
        );
        assert_eq!(world.relay.calls(9).len(), 1);
        assert_eq!(world.script.starts().len(), 2);
        let prompts = world.script.received("session/prompt");
        assert_eq!(prompts.len(), 2);
        assert_eq!(
            prompts[0]["_meta"], prompts[1]["_meta"],
            "the same request id"
        );
        let (first, second) = (
            prompts[0]["prompt"][0]["text"].as_str().unwrap(),
            prompts[1]["prompt"][0]["text"].as_str().unwrap(),
        );
        assert!(
            !first.contains("cut short")
                && second.starts_with("Note: your last turn on this request was cut short")
        );
        host.shutdown().await;
    }
}

/// Task rule 9: once something of the turn was published, a harness that
/// cannot tell it has seen the request is not handed it again; the turn
/// fails with a notice rather than answer twice. bzz's own host, which
/// knows the request, continues.
#[tokio::test]
async fn killed_after_a_progress_message_only_a_harness_that_knows_the_request_continues() {
    for harness in [Harness::Codex, Harness::PiDurable] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": harness == Harness::PiDurable}),
            json!([
                {"steps": [send("Working on it.", false), {"exit": 1}]},
                {"resume": true, "steps": [send("Here is the answer.", true)]},
                {"steps": [send("Working on it.", false), send("Here is the answer.", true)]},
            ]),
        );
        let host = world.host(harness).await;
        let request = world.ask(&host, "hello").await;
        let row = world.settled(&request).await;
        let replies = world.contents(&world.relay.replies_to(&request));
        if harness == Harness::PiDurable {
            assert_eq!((row.state, row.attempts), (RequestState::Replied, 2));
            assert_eq!(replies, ["Working on it.", "Here is the answer."]);
        } else {
            assert_eq!(
                (row.state, row.failure.as_deref(), row.attempts),
                (RequestState::Failed, Some("interrupted"), 1)
            );
            assert_eq!(
                replies,
                [
                    "Working on it.",
                    "I was interrupted while answering and may have left this unfinished. Ask again if you still need it."
                ]
            );
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(
                world.script.received("session/prompt").len(),
                1,
                "never handed over again"
            );
            assert_eq!(
                world.script.starts().len(),
                1,
                "and nothing restarted for it"
            );
        }
        host.shutdown().await;
    }
}

/// Task rule 12, killed during the publish: the harness dies while its final
/// `send` is on its way to the relay. The reply is published once and the
/// request is not handed over again.
#[tokio::test]
async fn killed_during_the_publish_the_reply_is_published_once() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false}),
        json!([
            {"steps": [send("the answer", true)]},
            {"steps": [send("a second answer", true)]},
        ]),
    );
    // The relay holds the publish, so the kill lands while it is under way.
    world.relay.set_mode(Mode::Held);
    let host = world.host(Harness::Codex).await;
    let request = world.ask(&host, "hello").await;
    until("the publish to be on its way", WAIT, || {
        (!world.relay.calls(9).is_empty()).then_some(())
    })
    .await;
    assert!(
        world.relay.replies_to(&request).is_empty(),
        "held by the relay"
    );
    let pid = world.script.starts()[0];
    kill(pid);
    until("the harness to be gone", WAIT, || {
        (!running(pid)).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    world.relay.set_mode(Mode::Accept);
    world.relay.release();
    let row = world.settled(&request).await;
    assert_eq!(row.state, RequestState::Replied);
    until("a reply to reach the relay", WAIT, || {
        (!world.relay.replies_to(&request).is_empty()).then_some(())
    })
    .await;
    // And then whatever a second attempt would add.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["the answer"]
    );
    assert_eq!(world.relay.calls(9).len(), 1);
    assert_eq!(
        world.script.received("session/prompt").len(),
        1,
        "not handed over again"
    );
    host.shutdown().await;
}

/// Task rule 12, killed after the publish: the reply is on the relay and the
/// harness dies before it ends its turn. The request is settled; nothing is
/// handed over or published again.
#[tokio::test]
async fn killed_after_the_publish_nothing_is_published_again() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"steps": [send("the answer", true), {"exit": 1}]},
            {"resume": true, "steps": [send("the answer, again", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;
    let pid = world.script.starts()[0];
    until("the harness to be gone", WAIT, || {
        (!running(pid)).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["the answer"]
    );
    assert_eq!(world.relay.calls(9).len(), 1);
    assert_eq!(world.script.received("session/prompt").len(), 1);
    assert_eq!(
        world.row(&request).await.unwrap().state,
        RequestState::Replied
    );
    host.shutdown().await;
}

/// ADR decision 5: after a crash of bzz the stored reply is published again,
/// unchanged, before any harness starts; a relay that already has it keeps
/// one copy.
#[tokio::test]
async fn after_a_crash_of_bzz_the_stored_reply_is_published_again_unchanged() {
    for mode in [Mode::Lost, Mode::Silent] {
        let world = World::new().await;
        world.script.write(
            json!({"own_host": true}),
            json!([{"steps": [send("the answer", true)]}]),
        );
        world.relay.set_mode(mode.clone());
        let host = world.host(Harness::PiDurable).await;
        let request = world.ask(&host, "hello").await;
        world.state_is(&request, RequestState::Replied).await;
        until("the tool to answer", WAIT, || {
            (!world.script.tool_results("send").is_empty()).then_some(())
        })
        .await;
        let result = &world.script.tool_results("send")[0];
        assert!(
            result["text"]
                .as_str()
                .unwrap()
                .contains("has not confirmed"),
            "{result}"
        );
        let pending = world
            .store
            .call({
                let agent = world.agent;
                move |store| store.agent_replies_unpublished(agent, 0)
            })
            .await
            .unwrap();
        assert_eq!(pending.len(), 1, "stored before it was published");
        // bzz goes away with the reply unacknowledged, and comes back.
        host.shutdown().await;
        world.relay.set_mode(Mode::Accept);
        let again = world.host(Harness::PiDurable).await;
        let replies = world.relay.replies_to(&request);
        assert_eq!(
            world.contents(&replies),
            ["the answer"],
            "{mode:?}: exactly one on the relay"
        );
        assert_eq!(replies[0].id.to_hex(), pending[0].event_id);
        let calls = world.relay.calls(9);
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].as_json(),
            calls[1].as_json(),
            "the same signed event, byte for byte"
        );
        let stored = world
            .store
            .call({
                let (agent, id) = (world.agent, request.id.to_hex());
                move |store| store.agent_reply_get(agent, &id)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(stored.published);
        assert_eq!(
            world.script.starts().len(),
            1,
            "recovery published it without starting a harness"
        );
        again.shutdown().await;
    }
}

/// Task rules 2 and 9: at startup the leftovers of a bzz that is gone are
/// stopped before recovery, and recovery settles the open request before
/// anything starts again.
#[tokio::test]
async fn at_startup_leftovers_are_stopped_before_recovery_and_recovery_before_any_start() {
    let world = World::new().await;
    // What a killed bzz leaves: a harness still running, deaf to its closed
    // input, with its record; and a request it had been handed.
    world.script.write(
        json!({"own_host": true, "ignore_eof": true}),
        json!([{"steps": [send("answered by the new process", true)]}]),
    );
    let run_dir = process::run_dir(&world.data_dir());
    let folder = process::agent_dir(&world.data_dir(), world.agent);
    let (leftover, stdin, stdout) = process::Harness::spawn(
        &process::Launch {
            program: world.script.command()[0].clone(),
            args: vec![world.script.command()[1].clone()],
            env: process::base_environment(&folder, |name| std::env::var(name).ok()),
            cwd: folder.clone(),
            log: folder.join("harness.log"),
        },
        &run_dir,
        world.agent,
    )
    .unwrap();
    let left_pid = leftover.pid().unwrap();
    // Its bzz is gone: nobody holds it, and its record names a dead owner.
    std::mem::forget(leftover);
    drop((stdin, stdout));
    let mut record = process::read_record(&run_dir, world.agent).unwrap();
    record.owner.pid = u32::MAX - 11;
    record.owner.start_time = "0".into();
    std::fs::write(
        run_dir.join(format!("{}.json", world.agent)),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let request = world.mention("asked before the crash");
    let place = format!("{}/{}", world.channel, request.id.to_hex());
    world
        .store
        .call({
            let new = NewRequest {
                agent_id: world.agent,
                request_id: request.id.to_hex(),
                community_id: Uuid::nil(),
                channel_id: world.channel,
                place,
                author: request.pubkey.to_hex(),
                created_at: request.created_at.as_secs(),
                event_json: request.as_json(),
            };
            move |store| {
                store.agent_inbox_offer(&new)?;
                store.agent_inbox_delivered(new.agent_id, &new.request_id)
            }
        })
        .await
        .unwrap();
    until("the leftover to be running", WAIT, || {
        (world.script.starts().len() == 1).then_some(())
    })
    .await;
    assert!(running(left_pid));

    let mut options = HostOptions::new(world.data_dir(), world.store.clone());
    options.limits = limits();
    let host = AgentHost::start(options).await.unwrap();
    // Swept by `start`, before any agent is added or recovered.
    assert_eq!(
        host.leftovers(),
        [Leftover::Stopped {
            agent: world.agent,
            pid: left_pid
        }]
    );
    assert!(!running(left_pid), "the leftover was stopped at startup");
    assert_eq!(
        world.row(&request).await.unwrap().state,
        RequestState::Delivered
    );
    assert!(world.relay.calls(9).is_empty());

    host.add(
        world.spec(world.settings(Harness::PiDurable)),
        world.relay.clone(),
    )
    .await;
    let row = world.settled(&request).await;
    assert_eq!((row.state, row.attempts), (RequestState::Replied, 2));
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["answered by the new process"]
    );
    let starts = world.script.starts();
    assert_eq!(starts.len(), 2);
    assert_ne!(starts[1], left_pid);
    host.shutdown().await;
}

/// Task rule 9: a request that a dead bzz had handed to a harness that
/// cannot tell requests apart, with something already published, is never
/// handed over again: recovery ends it with a notice and starts nothing.
#[tokio::test]
async fn recovery_ends_a_doubtful_request_with_a_notice_and_starts_nothing() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [send("a second answer", true)]}]),
    );
    let request = world.mention("asked before the crash");
    world
        .store
        .call({
            let new = NewRequest {
                agent_id: world.agent,
                request_id: request.id.to_hex(),
                community_id: Uuid::nil(),
                channel_id: world.channel,
                place: format!("{}/{}", world.channel, request.id.to_hex()),
                author: request.pubkey.to_hex(),
                created_at: request.created_at.as_secs(),
                event_json: request.as_json(),
            };
            move |store| {
                store.agent_inbox_offer(&new)?;
                store.agent_inbox_delivered(new.agent_id, &new.request_id)?;
                store.agent_inbox_effect(new.agent_id, &new.request_id)
            }
        })
        .await
        .unwrap();
    let host = world.host(Harness::Grok).await;
    // `add` returns after recovery: the notice is already out.
    let row = world.row(&request).await.unwrap();
    assert_eq!(
        (row.state, row.failure.as_deref()),
        (RequestState::Failed, Some("interrupted"))
    );
    assert_eq!(world.relay.replies_to(&request).len(), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(world.script.starts().is_empty(), "no harness was started");
    host.shutdown().await;
}

// ---- who wakes an agent, and which agents run -----------------------------

/// ADR decision 4 and D3: only an author who passes the agent's rule wakes
/// it; a DM is the owner's alone, needs no mention and is answered flat.
#[tokio::test]
async fn only_an_author_who_passes_the_rule_wakes_the_agent() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "from the owner", "steps": [send("to the owner", true)]},
            {"match": "in the dm", "steps": [send("in the dm too", true)]},
            {"match": "from a listed", "steps": [send("to a listed person", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    // Owner only, the default: a stranger's mention is not delivered.
    let stranger = world.message_in(
        world.channel,
        &world.stranger,
        "from a stranger",
        true,
        None,
    );
    assert_eq!(
        world.offer(&host, &stranger).await,
        Offer::Refused(Refusal::AuthorNotAllowed)
    );
    let unmentioned = world.message_in(
        world.channel,
        &world.owner,
        "not for the agent",
        false,
        None,
    );
    assert_eq!(
        world.offer(&host, &unmentioned).await,
        Offer::Refused(Refusal::NotMentioned)
    );
    assert!(world.row(&stranger).await.is_none() && world.row(&unmentioned).await.is_none());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        world.script.starts().is_empty(),
        "a refused message starts nothing"
    );

    let owner = world.ask(&host, "from the owner").await;
    world.settled(&owner).await;
    // The owner's DM: no mention, and the reply is a flat message.
    let dm = Uuid::new_v4();
    world.channel_is(PlaceKind::Dm, dm, false);
    let direct = world.message_in(dm, &world.owner, "in the dm", false, None);
    assert_eq!(world.offer(&host, &direct).await, Offer::Queued);
    world.settled(&direct).await;
    let flat = world
        .relay
        .by_agent(9)
        .into_iter()
        .find(|event| event.content == "in the dm too")
        .expect("the DM reply");
    assert!(has_tag(&flat, &["h", &dm.to_string()]));
    assert!(
        bzz::protocol::events::event_references(&flat).is_empty(),
        "flat, as a DM is"
    );
    assert_eq!(
        world.script.received("session/new")[1]["_meta"]["bzz/place"],
        dm.to_string()
    );
    // A DM with someone else never wakes it, whatever the rule.
    let theirs = Uuid::new_v4();
    let mut facts = world.channel_is(PlaceKind::Dm, theirs, false);
    facts.members = vec![world.stranger.public_key().to_hex(), world.agent_pubkey()];
    world.relay.set_channel(facts);
    let other_dm = world.message_in(theirs, &world.stranger, "psst", true, None);
    host.shutdown().await;

    let mut open = world.settings(Harness::PiDurable);
    open.wake = WakeRule::Allowlist;
    open.wake_allowlist = vec![world.stranger.public_key().to_hex()];
    let host = world.host_with(open, limits()).await;
    assert_eq!(
        world.offer(&host, &other_dm).await,
        Offer::Refused(Refusal::DmNotOwnerOnly)
    );
    let listed = world.message_in(
        world.channel,
        &world.stranger,
        "from a listed person",
        true,
        None,
    );
    assert_eq!(world.offer(&host, &listed).await, Offer::Queued);
    world.settled(&listed).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&listed)),
        ["to a listed person"]
    );
    host.shutdown().await;
}

/// Task rule 10: paused and moved-away agents never start; pausing a running
/// agent stops its process, and resuming settles what was open.
#[tokio::test]
async fn paused_and_moved_away_agents_never_start() {
    let world = World::new().await;
    let gate = world.script.marker("gate");
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"steps": [{"touch": world.script.marker("live")}, {"wait_for": gate}, send("too late", true)]},
            {"resume": true, "steps": [send("after the pause", true)]},
        ]),
    );
    let mut options = HostOptions::new(world.data_dir(), world.store.clone());
    options.limits = limits();
    let host = AgentHost::start(options).await.unwrap();
    let configured = |state, paused| HostedAgentConfig {
        id: Uuid::new_v4(),
        label: format!("{state:?}-{paused}"),
        pubkey: world.agent_pubkey(),
        community: Uuid::nil(),
        owner_identity: Uuid::nil(),
        auth_tag: "[]".into(),
        credential_hash: "c".repeat(64),
        allowed: Vec::new(),
        paused,
        local: Some(LocalAgentConfig {
            state,
            ..world.settings(Harness::PiDurable)
        }),
    };
    let (moved, paused, broker_paused, active) = (
        configured(LocalAgentState::MovedAway, false),
        configured(LocalAgentState::Paused, false),
        configured(LocalAgentState::Active, true),
        configured(LocalAgentState::Active, false),
    );
    assert!(!host.runnable(&moved) && !host.runnable(&paused) && !host.runnable(&broker_paused));
    assert!(host.runnable(&active));
    let mut hosted = active.clone();
    hosted.local = None;
    assert!(
        !host.runnable(&hosted),
        "a v0.13 hosted agent is never run by bzz"
    );
    let lines = host
        .status()
        .iter()
        .map(|status| status.line())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("stopped · moved away")),
        "{lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("paused · paused by its owner"))
            .count(),
        2
    );
    // An agent the host does not run takes no request and starts nothing.
    let request = world.mention("hello");
    assert_eq!(world.offer(&host, &request).await, Offer::NotRunning);
    assert!(world.script.starts().is_empty());

    // Pausing a running agent stops its process; nothing is published.
    host.add(
        world.spec(world.settings(Harness::PiDurable)),
        world.relay.clone(),
    )
    .await;
    assert_eq!(world.offer(&host, &request).await, Offer::Queued);
    let live = world.script.marker("live");
    until("the turn to be live", WAIT, || live.exists().then_some(())).await;
    let pid = world.script.starts()[0];
    host.set_paused(world.agent, true).await;
    assert!(!running(pid), "a paused agent has no process");
    assert!(
        host.status()
            .iter()
            .any(|status| status.id == world.agent && status.phase == Phase::Paused)
    );
    std::fs::write(&gate, "open").unwrap();
    let late = world.mention("while paused");
    assert_eq!(
        world.offer(&host, &late).await,
        Offer::NotRunning,
        "a paused agent does not listen"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(world.relay.calls(9).is_empty());
    assert_eq!(
        world.script.starts().len(),
        1,
        "nothing starts while paused"
    );
    // Resumed: the open request is settled and answered once.
    host.set_paused(world.agent, false).await;
    world.settled(&request).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&request)),
        ["after the pause"]
    );
    host.shutdown().await;
}

/// ADR decision 9: a second bzz on the same data does not start an agent
/// whose process a live bzz owns.
#[tokio::test]
async fn an_agent_run_by_another_live_bzz_is_not_started() {
    let world = World::new().await;
    world.script.write(json!({"own_host": true}), json!([]));
    let run_dir = process::run_dir(&world.data_dir());
    std::fs::create_dir_all(&run_dir).unwrap();
    // Another bzz, alive, owns the agent's process.
    let mut other_bzz = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let mut its_harness = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let record = process::OwnershipRecord {
        agent: world.agent,
        process: process::identity_of(its_harness.id()).unwrap(),
        owner: process::identity_of(other_bzz.id()).unwrap(),
    };
    std::fs::write(
        run_dir.join(format!("{}.json", world.agent)),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let mut options = HostOptions::new(world.data_dir(), world.store.clone());
    options.limits = limits();
    let host = AgentHost::start(options).await.unwrap();
    assert_eq!(
        host.leftovers(),
        [Leftover::OwnedElsewhere {
            agent: world.agent,
            owner_pid: other_bzz.id()
        }]
    );
    assert!(
        running(its_harness.id()),
        "the other bzz's harness is left alone"
    );
    let configured = HostedAgentConfig {
        id: world.agent,
        label: "Scout".into(),
        pubkey: world.agent_pubkey(),
        community: Uuid::nil(),
        owner_identity: Uuid::nil(),
        auth_tag: "[]".into(),
        credential_hash: "c".repeat(64),
        allowed: Vec::new(),
        paused: false,
        local: Some(world.settings(Harness::PiDurable)),
    };
    assert!(!host.runnable(&configured));
    assert!(
        host.status()[0].line().contains("another bzz runs it"),
        "{}",
        host.status()[0].line()
    );
    host.shutdown().await;
    let _ = (other_bzz.kill(), its_harness.kill());
    let _ = (other_bzz.wait(), its_harness.wait());
}

// ---- sessions across restarts, and the idle checkpoint --------------------

/// Protocol, `session/new`: for a harness that is not bzz's own, bzz keeps
/// the session id it returned for a place and uses `session/load` after a
/// restart when the harness offers it; a harness that refuses the load gets
/// a new session.
#[tokio::test]
async fn a_stored_session_is_loaded_after_a_restart_when_the_harness_offers_it() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": false, "load_session": true}),
        json!([
            {"steps": [send("first answer", true)]},
            {"steps": [send("second answer", true)]},
            {"steps": [send("third answer", true)]},
        ]),
    );
    let host = world.host(Harness::ClaudeCode).await;
    let first = world.ask(&host, "first").await;
    world.settled(&first).await;
    let session = world.script.received("session/prompt")[0]["sessionId"].clone();
    host.shutdown().await;

    // A new process: the same conversation is loaded, not opened anew, and
    // its tools work with the credential made for this start.
    let host = world.host(Harness::ClaudeCode).await;
    let second = world.message_in(world.channel, &world.owner, "second", true, Some(&first));
    assert_eq!(world.offer(&host, &second).await, Offer::Queued);
    world.settled(&second).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&second)),
        ["second answer"]
    );
    assert_eq!(world.script.received("session/new").len(), 1);
    let loads = world.script.received("session/load");
    assert_eq!(loads.len(), 1);
    assert_eq!(loads[0]["sessionId"], session);
    assert_eq!(loads[0]["mcpServers"][0]["name"], "buzz");
    assert_eq!(
        world.script.received("session/prompt")[1]["sessionId"],
        session
    );
    assert_ne!(
        loads[0]["mcpServers"][0]["env"][0]["value"],
        world.script.received("session/new")[0]["mcpServers"][0]["env"][0]["value"],
        "a credential is never reused across starts"
    );
    host.shutdown().await;

    // The harness no longer knows the session: bzz opens a new one.
    let mut state =
        serde_json::from_str::<Value>(&std::fs::read_to_string(world.script.state_path()).unwrap())
            .unwrap();
    state["sessions"] = json!([]);
    std::fs::write(world.script.state_path(), state.to_string()).unwrap();
    let host = world.host(Harness::ClaudeCode).await;
    let third = world.message_in(world.channel, &world.owner, "third", true, Some(&first));
    assert_eq!(world.offer(&host, &third).await, Offer::Queued);
    world.settled(&third).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&third)),
        ["third answer"]
    );
    assert_eq!(world.script.received("session/load").len(), 2);
    assert_eq!(world.script.received("session/new").len(), 2);
    host.shutdown().await;
}

/// Protocol, `_bzz/checkpoint`: bzz's own host is asked to checkpoint its
/// files when the agent has been idle for a while, never during a turn, and
/// on request (before a move). Another harness is never asked.
#[tokio::test]
async fn bzzs_own_host_is_asked_to_checkpoint_when_idle() {
    for harness in [Harness::PiDurable, Harness::Grok] {
        let world = World::new().await;
        let gate = world.script.marker("gate");
        world.script.write(
            json!({"own_host": harness == Harness::PiDurable}),
            json!([
                {"steps": [{"touch": world.script.marker("live")}, {"wait_for": gate}, send("answer", true)]},
            ]),
        );
        let mut idle = limits();
        idle.checkpoint_idle = Duration::from_millis(300);
        let host = world.host_with(world.settings(harness), idle).await;
        let request = world.ask(&host, "hello").await;
        let live = world.script.marker("live");
        until("the turn to be live", WAIT, || live.exists().then_some(())).await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!host.checkpoint(world.agent).await, "not while a turn runs");
        assert!(world.script.received("_bzz/checkpoint").is_empty());
        std::fs::write(&gate, "open").unwrap();
        world.settled(&request).await;
        tokio::time::sleep(Duration::from_millis(900)).await;
        let asked = world.script.received("_bzz/checkpoint").len();
        if harness == Harness::PiDurable {
            assert_eq!(asked, 1, "once, after it went idle");
            assert!(host.checkpoint(world.agent).await, "and on request");
            assert_eq!(world.script.received("_bzz/checkpoint").len(), 2);
        } else {
            assert_eq!(asked, 0);
            assert!(!host.checkpoint(world.agent).await);
        }
        host.shutdown().await;
    }
}

// ---- what M1 left open (v0.15 M4) ------------------------------------------

/// bzz's own host answers a failed turn with one code, -32000, and names the
/// failure in `error.data.kind`. That code is also ACP's "sign in", so the
/// kind decides.
#[tokio::test]
async fn a_failure_bzzs_own_host_names_is_read_from_its_kind() {
    let world = World::new().await;
    let failed = |kind: &str, message: &str| json!({"error": {"code": -32000, "message": message, "data": {"kind": kind}}});
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "provider", "steps": [failed("error", "The turn failed: upstream exploded at 10.1.2.3")]},
            {"match": "no such model", "steps": [failed("model_unavailable", "openai-codex has no model gpt-0")]},
            {"match": "extension", "steps": [failed("provider_unavailable", "cursor is not one of Pi's providers")]},
            {"match": "signed out", "steps": [failed("sign_in_needed", "run pi and sign in to openai-codex")]},
            {"match": "busy", "steps": [failed("error", "The turn failed: 429 rate limit")]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    let unavailable = "I could not answer: the model my owner chose for me is not available.";
    for (word, kind, notice) in [
        (
            "provider",
            "provider-error",
            "I could not answer: my model returned an error.",
        ),
        ("no such model", "model-unavailable", unavailable),
        ("extension", "model-unavailable", unavailable),
        (
            "signed out",
            "sign-in-needed",
            "I could not answer: my owner needs to sign in to my model again.",
        ),
        (
            "busy",
            "limit-reached",
            "I could not answer: my model's usage limit was reached. Try again later.",
        ),
    ] {
        let request = world.ask(&host, &format!("please test {word}")).await;
        let row = world.settled(&request).await;
        assert_eq!(
            (row.state, row.failure.as_deref()),
            (RequestState::Failed, Some(kind)),
            "{word}"
        );
        assert_eq!(
            world.contents(&world.relay.replies_to(&request)),
            [notice],
            "{word}"
        );
    }
    let published = world.contents(&world.relay.by_agent(9)).join("\n");
    assert!(!published.contains("10.1.2.3") && !published.contains("gpt-0"));
    host.shutdown().await;
}

/// The agents panel's key: the owner stops a running turn. It ends once,
/// with a notice that says so; a harness that honours the cancel keeps its
/// process, and one that does not is stopped and the turn is not run again.
#[tokio::test]
async fn the_owner_stops_a_running_turn_and_it_ends_once() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([
            {"match": "long", "steps": [{"text": "thinking"}, {"stall": true}, send("too late", true)]},
            {"match": "deaf", "steps": [{"stall": true, "deaf": true}]},
            {"match": "deaf", "resume": true, "steps": [send("ran again", true)]},
            {"match": "after", "steps": [send("back again", true)]},
        ]),
    );
    let host = world.host(Harness::PiDurable).await;
    assert_eq!(host.stop_turns(world.agent).await, 0, "nothing runs yet");
    let running_turn = |host: &AgentHost| {
        let host = host.clone();
        async move {
            until("the turn to be running", WAIT, || {
                (host.status()[0].turns == 1).then_some(())
            })
            .await;
        }
    };
    let long = world.ask(&host, "a long question").await;
    running_turn(&host).await;
    until("the harness to be thinking", WAIT, || {
        (!world.script.received("session/prompt").is_empty()).then_some(())
    })
    .await;
    assert_eq!(host.stop_turns(world.agent).await, 1);
    let row = world.settled(&long).await;
    assert_eq!(
        (row.state, row.failure.as_deref(), row.attempts),
        (RequestState::Failed, Some("stopped"), 1)
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&long)),
        ["My owner stopped this answer."],
        "neither the text so far nor what the harness would have sent"
    );
    assert_eq!(world.script.received("session/cancel").len(), 1);
    assert_eq!(host.status()[0].phase, Phase::Running, "its process stays");

    // A harness deaf to the cancel is stopped; the turn is not run again.
    let deaf = world.ask(&host, "a deaf question").await;
    until("the second prompt to reach the harness", WAIT, || {
        (world.script.received("session/prompt").len() == 2).then_some(())
    })
    .await;
    running_turn(&host).await;
    assert_eq!(host.stop_turns(world.agent).await, 1);
    let row = world.settled(&deaf).await;
    assert_eq!(
        (row.state, row.failure.as_deref(), row.attempts),
        (RequestState::Failed, Some("stopped"), 1)
    );
    let first_pid = world.script.starts()[0];
    until("the deaf harness to be stopped", WAIT, || {
        (!running(first_pid)).then_some(())
    })
    .await;
    let after = world.ask(&host, "after that").await;
    world.settled(&after).await;
    assert_eq!(
        world.contents(&world.relay.replies_to(&after)),
        ["back again"]
    );
    assert_eq!(
        world.contents(&world.relay.replies_to(&deaf)),
        ["My owner stopped this answer."]
    );
    assert_eq!(world.script.starts().len(), 2);
    host.shutdown().await;
}

/// Two agents that may wake each other cannot go on for ever: another
/// agent's message wakes this one only so often in one conversation. People
/// are not counted, and neither is another conversation.
#[tokio::test]
async fn two_agents_that_wake_each_other_stop_after_a_few_exchanges() {
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        // Whatever it is asked, it answers and mentions the other agent.
        json!(
            (0..12)
                .map(|index| json!({"steps": [
                    {"tool": "send", "arguments": {
                        "text": format!("answer {index}"), "final": true,
                        "mentions": [world.stranger.public_key().to_hex()],
                    }},
                ]}))
                .collect::<Vec<_>>()
        ),
    );
    let mut open = world.settings(Harness::PiDurable);
    open.wake = WakeRule::Anyone;
    let host = world.host_with(open, limits()).await;
    // The other agent: its messages carry its owner's authorization, as
    // every agent's do.
    let other_owner = Keys::generate();
    let other_tag = bzz::broker::relay::auth_tag(
        &bzz::auth::signer::SignerHandle::spawn(other_owner)
            .owner_auth_tag(world.stranger.public_key(), "")
            .await
            .unwrap(),
    )
    .unwrap();
    let from_the_other = |text: &str, thread: Option<&Event>| {
        let agent = world.agent_pubkey();
        let thread = thread.map(|root| buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        });
        let event = buzz_sdk::build_message(
            world.channel,
            text,
            thread.as_ref(),
            &[agent.as_str()],
            false,
            &[],
            &[],
        )
        .unwrap()
        .tag(other_tag.clone())
        .sign_with_keys(&world.stranger)
        .unwrap();
        world.relay.seed(&event);
        event
    };
    // The owner starts a thread; from then on the two answer each other:
    // every answer of this agent mentions the other, which "answers" back.
    let root = world.ask(&host, "you two, talk").await;
    world.settled(&root).await;
    let mut exchanges = 0;
    let refused = loop {
        let back = from_the_other(&format!("reply {exchanges}"), Some(&root));
        match world.offer(&host, &back).await {
            Offer::Queued => {
                world.settled(&back).await;
                exchanges += 1;
                assert!(exchanges < 10, "they never stop");
            }
            other => break other,
        }
    };
    assert_eq!(refused, Offer::Refused(Refusal::AgentExchangeLimit));
    assert_eq!(exchanges, 3, "as many as one conversation allows");
    assert_eq!(
        world.relay.by_agent(9).len(),
        4,
        "one answer to the owner, three to the other agent, and no more"
    );
    assert!(
        host.status()[0]
            .problem
            .as_deref()
            .is_some_and(|problem| problem.contains("loop")),
        "{:?}",
        host.status()
    );
    // A person in the same conversation still wakes it, as often as he asks.
    for index in 0..4 {
        let person = world.message_in(
            world.channel,
            &world.stranger,
            &format!("a person asks {index}"),
            true,
            Some(&root),
        );
        assert_eq!(world.offer(&host, &person).await, Offer::Queued, "{index}");
        world.settled(&person).await;
    }
    // So does the other agent in another conversation, and the owner here.
    let elsewhere = from_the_other("somewhere else", None);
    assert_eq!(world.offer(&host, &elsewhere).await, Offer::Queued);
    world.settled(&elsewhere).await;
    let owner = world.message_in(world.channel, &world.owner, "go on", true, Some(&root));
    assert_eq!(world.offer(&host, &owner).await, Offer::Queued);
    world.settled(&owner).await;
    host.shutdown().await;
}

/// Finished requests and idle sessions are pruned when an agent starts, by
/// the rule of `Store::agent_prune`: what is over and older than the
/// retention goes with its stored reply; what is open, unacknowledged or
/// recent stays.
#[tokio::test]
async fn an_agents_finished_requests_are_pruned_when_it_starts() {
    use bzz::store::agent_runtime::{SESSION_RETENTION_SECONDS, SETTLED_RETENTION_SECONDS};
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [send("answered", true)]}, {"steps": [send("answered", true)]}]),
    );
    let host = world.host(Harness::PiDurable).await;
    let old = world.ask(&host, "long ago").await;
    world.settled(&old).await;
    let recent = world.ask(&host, "just now").await;
    world.settled(&recent).await;
    host.shutdown().await;
    let (agent, place) = (
        world.agent,
        format!("{}/{}", world.channel, old.id.to_hex()),
    );
    world
        .store
        .call(move |store| store.set_agent_session(agent, &place, "s1"))
        .await
        .unwrap();
    // Time passes for the first request and for the stored session.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let database = rusqlite::Connection::open(world.data_dir().join("bzz.db")).unwrap();
    database.busy_timeout(Duration::from_secs(5)).unwrap();
    database
        .execute(
            "UPDATE agent_inbox SET updated_at=?2 WHERE request_id=?1",
            rusqlite::params![
                old.id.to_hex(),
                (now - SETTLED_RETENTION_SECONDS - 60) as i64
            ],
        )
        .unwrap();
    database
        .execute(
            "UPDATE agent_sessions SET updated_at=?1",
            [(now - SESSION_RETENTION_SECONDS - 60) as i64],
        )
        .unwrap();
    drop(database);

    let host = world.host(Harness::PiDurable).await;
    assert!(world.row(&old).await.is_none(), "the old request is gone");
    let (agent, id) = (world.agent, old.id.to_hex());
    assert!(
        world
            .store
            .call(move |store| store.agent_reply_get(agent, &id))
            .await
            .unwrap()
            .is_none(),
        "with its stored reply"
    );
    assert_eq!(
        world.row(&recent).await.map(|row| row.state),
        Some(RequestState::Replied)
    );
    let (agent, place) = (
        world.agent,
        format!("{}/{}", world.channel, old.id.to_hex()),
    );
    assert_eq!(
        world
            .store
            .call(move |store| store.agent_session(agent, &place))
            .await
            .unwrap(),
        None
    );
    // What the relay holds is untouched: pruning is bzz's own bookkeeping.
    assert_eq!(world.relay.by_agent(9).len(), 2);
    host.shutdown().await;
}

// ---- hands on this machine, and git credentials (M7 and M3) ---------------

/// A step that puts a command to bzz, as bzz's own host does before it
/// starts one. `started` is written only on a yes: it stands for the
/// command itself.
fn approve(mount: &str, command: &str, started: &std::path::Path) -> Value {
    json!({
        "request": {"method": "_bzz/approve", "params": {
            "sessionId": "$session", "kind": "exec", "mount": mount, "command": command,
        }},
        "then_touch": started,
    })
}

fn credential(host: &str) -> Value {
    json!({"request": {"method": "_bzz/credential", "params": {"sessionId": "$session", "host": host}}})
}

/// Waits for exactly one question to wait for the owner.
async fn one_question(host: &AgentHost) -> bzz::agents::runtime::approval::Asked {
    let host = host.clone();
    until("a command to wait for the owner", WAIT, move || {
        let mut waiting = host.approvals();
        (waiting.len() == 1).then(|| waiting.remove(0))
    })
    .await
}

/// Protocol section 3: `initialize` says for each granted folder whether
/// commands may start there (and never whether the owner is asked), and
/// whether Pi packages are on. What the host answers (where it offers
/// `run`, which packages it loaded or left out) is kept for `agent show`
/// and `agent doctor`.
#[tokio::test]
async fn the_host_is_told_which_folders_take_commands_and_what_it_reports_is_kept() {
    let world = World::new().await;
    world.script.write(
        json!({
            "own_host": true,
            "run": ["work", "ops", "../etc", 7],
            "packages": [
                {"source": "~/pi/guard", "loaded": true, "extensions": ["guard"], "providers": []},
                {"source": "npm:pi-x\u{1b}[31m", "loaded": false, "error": "Only Pi packages in a local folder are loaded"},
                "not a report",
            ],
        }),
        json!([{"steps": [send("ok", true)]}]),
    );
    let mut local = world.hands(&[
        ("docs", ExecGrant::No),
        ("work", ExecGrant::Ask),
        ("ops", ExecGrant::WithoutAsking),
    ]);
    local.folders[1].write = true;
    local.packages = false;
    let granted = local.folders.clone();
    assert!(StartReport::read(&world.data_dir(), world.agent).is_none());
    let host = world.host_with(local, limits()).await;
    let request = world.ask(&host, "hello").await;
    world.settled(&request).await;

    let initialize = &world.script.received("initialize")[0]["_meta"]["bzz"];
    assert_eq!(
        initialize["mounts"],
        json!([
            {"name": "docs", "path": granted[0].path, "write": false, "exec": false},
            {"name": "work", "path": granted[1].path, "write": true, "exec": true},
            {"name": "ops", "path": granted[2].path, "write": false, "exec": true},
        ]),
        "exec is apart from write, and says nothing of asking"
    );
    assert_eq!(initialize["packages"], json!(false));
    assert_eq!(initialize["piDir"], json!("/nonexistent/pi/agent"));

    // What the host reported, kept in the agent's folder.
    let report = StartReport::read(&world.data_dir(), world.agent).expect("a report");
    assert_eq!(report.host, "scripted");
    assert_eq!(report.run, ["work", "ops"], "only what is a folder's name");
    assert!(report.packages_off);
    assert_eq!(report.packages.len(), 2);
    assert_eq!(
        (
            report.packages[0].source.as_str(),
            report.packages[0].loaded
        ),
        ("~/pi/guard", true)
    );
    assert_eq!(report.packages[0].extensions, ["guard"]);
    let shown = report
        .lines(&["work".into(), "ops".into(), "late".into()])
        .join("\n");
    for expected in [
        "version scripted",
        "real commands: the `run` tool, in /mnt/work, /mnt/ops",
        "granted for commands now, but not offered at that start: /mnt/late",
        "Pi packages: switched off for this agent",
        "Pi package: ~/pi/guard · loaded · extensions: guard",
        "· not loaded: Only Pi packages in a local folder are loaded",
    ] {
        assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
    }
    assert!(!shown.contains('\u{1b}'), "a package's words are made safe");
    host.shutdown().await;
}

/// M7: a real command in a folder granted for commands waits for the
/// owner. He is shown which agent asks, in which conversation, for which
/// folder, and the command exactly as it will run. Only his yes starts it;
/// his no, with his reason, starts nothing. Both are in the agent's
/// activity.
#[tokio::test]
async fn a_command_runs_only_after_the_owner_allows_it() {
    let world = World::new().await;
    let (ran, not_run) = (world.script.marker("ran"), world.script.marker("not-run"));
    let command = "cd app &&\n  npm test -- --watch=false\t# \u{1b}[2K";
    world.script.write(
        json!({"own_host": true, "run": ["work"]}),
        json!([
            {"match": "tests", "steps": [approve("work", command, &ran), send("ran them", true)]},
            {"match": "wipe", "steps": [approve("work", "rm -rf .", &not_run), send("refused", true)]},
        ]),
    );
    let local = world.hands(&[("work", ExecGrant::Ask)]);
    let path = local.folders[0].path.clone();
    let host = world.host_with_owner(local, limits()).await;
    assert!(host.approvals().is_empty());

    let request = world.ask(&host, "run the tests").await;
    let asked = one_question(&host).await;
    assert_eq!(
        (asked.agent, asked.agent_name.as_str(), asked.channel),
        (world.agent, "Scout", world.channel)
    );
    assert_eq!(
        asked.place,
        format!("{}/{}", world.channel, request.id.to_hex())
    );
    assert_eq!(asked.requester, world.owner.public_key().to_hex());
    assert_eq!((asked.folder.as_str(), &asked.path), ("work", &path));
    assert_eq!(
        asked.command, command,
        "the command as it will run, untouched"
    );
    // While it waits, nothing has started and the turn says what it waits for.
    assert!(!ran.exists());
    assert!(
        world
            .script
            .log()
            .iter()
            .all(|entry| entry["to"] != "_bzz/approve")
    );
    let doing = host.status()[0].doing().unwrap_or_default();
    assert!(
        doing.starts_with("waiting for your answer: run in /mnt/work: cd app &&"),
        "{doing}"
    );
    assert!(host.answer_approval(asked.id, Answer::Allow));
    world.settled(&request).await;
    assert_eq!(
        world.answers("_bzz/approve", 1).await[0]["result"],
        json!({"allow": true}),
        "bzz sends back a yes and nothing of the command"
    );
    assert!(ran.exists(), "the command started");
    assert!(host.approvals().is_empty());
    assert!(
        !host.answer_approval(asked.id, Answer::Allow),
        "answered once"
    );

    // Refused, with a reason for the agent.
    let request = world.ask(&host, "wipe the folder").await;
    let asked = one_question(&host).await;
    assert_eq!(asked.command, "rm -rf .");
    assert!(host.answer_approval(
        asked.id,
        Answer::Refuse(Some("never in this folder".into()))
    ));
    world.settled(&request).await;
    let answer = world.answers("_bzz/approve", 2).await[1]["result"].clone();
    assert_eq!(answer["allow"], json!(false));
    let reason = answer["reason"].as_str().unwrap();
    assert!(
        reason.contains("Your owner refused this command, and says: never in this folder")
            && reason.contains("Nothing was started"),
        "{reason}"
    );
    assert!(!not_run.exists(), "a refused command does not start");

    // The agent's activity: each question and its answer, with the
    // command's first line and what would not show written out.
    assert_eq!(
        world.activity(),
        [
            "command in /mnt/work · cd app && (and 1 more line(s)) · asked of you",
            "command in /mnt/work · cd app && (and 1 more line(s)) · allowed by you",
            "command in /mnt/work · rm -rf . · asked of you",
            "command in /mnt/work · rm -rf . · refused by you: never in this folder",
        ]
    );
    host.shutdown().await;
}

/// M7: a question waits a stated time and then counts as refused. An
/// answer that comes after that starts nothing.
#[tokio::test]
async fn an_unanswered_command_counts_as_refused_and_a_late_answer_starts_nothing() {
    let world = World::new().await;
    let started = world.script.marker("started");
    world.script.write(
        json!({"own_host": true, "run": ["work"]}),
        json!([{"steps": [approve("work", "make deploy", &started), send("no answer", true)]}]),
    );
    let host = world
        .host_with_owner(
            world.hands(&[("work", ExecGrant::Ask)]),
            Limits {
                approval: Duration::from_secs(1),
                ..limits()
            },
        )
        .await;
    let request = world.ask(&host, "deploy").await;
    let asked = one_question(&host).await;
    assert!(asked.expires > std::time::Instant::now());
    let answer = world.answers("_bzz/approve", 1).await[0]["result"].clone();
    assert_eq!(answer["allow"], json!(false));
    assert!(
        answer["reason"]
            .as_str()
            .unwrap()
            .contains("did not answer within 1 seconds, which counts as a refusal"),
        "{answer}"
    );
    // The owner answers now: too late.
    assert!(host.approvals().is_empty());
    assert!(!host.answer_approval(asked.id, Answer::Allow));
    world.settled(&request).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!started.exists(), "a late answer starts nothing");
    assert_eq!(world.answers("_bzz/approve", 1).await.len(), 1);
    assert_eq!(
        world.activity(),
        [
            "command in /mnt/work · make deploy · asked of you",
            "command in /mnt/work · make deploy · refused: no answer in 1 s",
        ]
    );
    host.shutdown().await;
}

/// M7: a question is void when its turn is stopped, when the agent is
/// paused, and when bzz quits. Whatever the owner answers afterwards, the
/// command does not start.
#[tokio::test]
async fn a_waiting_command_is_void_when_its_turn_is_stopped_the_agent_paused_or_bzz_quits() {
    for way in ["stop", "pause", "quit"] {
        let world = World::new().await;
        let started = world.script.marker("started");
        world.script.write(
            json!({"own_host": true, "run": ["work"]}),
            json!([{"steps": [approve("work", "make deploy", &started), {"stall": true}]}]),
        );
        let host = world
            .host_with_owner(world.hands(&[("work", ExecGrant::Ask)]), limits())
            .await;
        world.ask(&host, "deploy").await;
        let asked = one_question(&host).await;
        let why = match way {
            "stop" => {
                assert_eq!(host.stop_turns(world.agent).await, 1);
                "its owner stopped the turn"
            }
            "pause" => {
                host.set_paused(world.agent, true).await;
                "the agent was paused"
            }
            _ => {
                host.shutdown().await;
                // Either is true of a bzz that quits; which comes first is
                // not a promise.
                "bzz"
            }
        };
        assert!(host.approvals().is_empty(), "{way}: it no longer waits");
        assert!(
            !host.answer_approval(asked.id, Answer::Allow),
            "{way}: a late yes finds nothing"
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!started.exists(), "{way}: the command did not start");
        // Whatever the harness was told, it was never told yes.
        for entry in world.script.log() {
            assert_ne!(
                entry.pointer("/response/result/allow"),
                Some(&json!(true)),
                "{way}"
            );
        }
        let activity = world.activity();
        assert_eq!(activity.len(), 2, "{way}: {activity:?}");
        assert!(
            activity[1].starts_with("command in /mnt/work · make deploy · void: ")
                && (activity[1].contains(why)
                    || activity[1].contains("nobody is at bzz to answer any more")),
            "{way}: {activity:?}"
        );
        if way == "stop" {
            // The host is told no, with why, and the turn ends as stopped.
            let answer = world.answers("_bzz/approve", 1).await[0]["result"].clone();
            assert_eq!(answer["allow"], json!(false));
            assert!(
                answer["reason"]
                    .as_str()
                    .unwrap()
                    .contains("was withdrawn: its owner stopped the turn"),
                "{answer}"
            );
        }
        host.shutdown().await;
    }
}

/// M7: without the TUI there is nobody to ask. A command in a folder that
/// is allowed without asking runs; any other is refused with a reason that
/// tells the agent, and through it the people in the chat, that its owner
/// must be at bzz. With the owner there, a folder allowed without asking
/// is still not asked about, and a folder not granted for commands takes
/// none, whatever the host was told.
#[tokio::test]
async fn without_the_tui_a_command_is_refused_and_one_allowed_without_asking_runs() {
    for owner_present in [false, true] {
        let world = World::new().await;
        let marker = |name: &str| world.script.marker(name);
        world.script.write(
            json!({"own_host": true, "run": ["work", "ops", "docs"]}),
            json!([{"steps": [
                approve("ops", "systemctl --user restart site", &marker("ops")),
                approve("docs", "touch here", &marker("docs")),
                approve("nowhere", "touch here", &marker("nowhere")),
                approve("ops", "", &marker("empty")),
                approve("ops", "printf 'a\u{0}b'", &marker("nul")),
                approve("work", "npm test", &marker("work")),
                send("done", true),
            ]}]),
        );
        let local = world.hands(&[
            ("docs", ExecGrant::No),
            ("work", ExecGrant::Ask),
            ("ops", ExecGrant::WithoutAsking),
        ]);
        let host = world
            .host_configured(local, limits(), |options| {
                options.owner_present = owner_present
            })
            .await;
        let request = world.ask(&host, "do things").await;
        if owner_present {
            // Only the folder that asks reaches the owner.
            let asked = one_question(&host).await;
            assert_eq!(
                (asked.folder.as_str(), asked.command.as_str()),
                ("work", "npm test")
            );
            assert!(host.answer_approval(asked.id, Answer::Refuse(None)));
        }
        world.settled(&request).await;
        let answers = world.answers("_bzz/approve", 6).await;
        let result = |index: usize| answers[index]["result"].clone();
        let reason = |index: usize| {
            result(index)["reason"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(result(0), json!({"allow": true}), "allowed without asking");
        assert!(marker("ops").exists());
        for (index, name) in [
            (1, "docs"),
            (2, "nowhere"),
            (3, "empty"),
            (4, "nul"),
            (5, "work"),
        ] {
            assert_eq!(result(index)["allow"], json!(false), "{name}");
            assert!(!marker(name).exists(), "{name} did not start");
        }
        assert!(
            reason(1).contains("has not granted that folder for commands"),
            "{}",
            reason(1)
        );
        assert!(
            reason(2).contains("No real command may run in /mnt/nowhere"),
            "{}",
            reason(2)
        );
        assert!(
            reason(3).contains("A command is not empty"),
            "{}",
            reason(3)
        );
        assert!(
            reason(4).contains("holds no NUL character"),
            "{}",
            reason(4)
        );
        if owner_present {
            assert!(
                reason(5).starts_with("Your owner refused this command."),
                "{}",
                reason(5)
            );
        } else {
            assert!(host.approvals().is_empty(), "nothing waits for nobody");
            for said in [
                "needs your owner's approval",
                "your owner is not at bzz",
                "Tell the people in this conversation that your owner must open bzz",
                "Nothing was started",
            ] {
                assert!(reason(5).contains(said), "`{said}` in: {}", reason(5));
            }
        }
        let activity = world.activity();
        assert_eq!(
            activity[0],
            "command in /mnt/ops · systemctl --user restart site · allowed without asking"
        );
        assert!(
            activity.contains(
                &"command in /mnt/docs · touch here · refused: the folder is not granted for commands"
                    .to_owned()
            ),
            "{activity:?}"
        );
        assert_eq!(
            activity.last().map(String::as_str),
            Some(if owner_present {
                "command in /mnt/work · npm test · refused by you"
            } else {
                "command in /mnt/work · npm test · refused: nobody was at bzz to ask"
            })
        );
        host.shutdown().await;
    }
}

/// Protocol section 4: the host's requests carry string ids, several are
/// open at once, and bzz answers them in whatever order they end. While a
/// command waits for the owner, a credential and a chat tool of the same
/// turn are answered; and a second conversation has a question of its own
/// open. One conversation has one question at a time.
#[tokio::test]
async fn several_requests_of_the_host_are_open_at_once_and_answered_in_any_order() {
    let world = World::new().await;
    let owner = bzz::auth::signer::SignerHandle::spawn(world.owner.clone());
    let keys = world.data_dir().join("keys");
    git_credentials::set(
        &keys,
        &owner,
        world.agent,
        "github.com",
        "scout-bot",
        "t0ken-marker",
    )
    .await
    .unwrap();
    let session = "$session";
    world.script.write(
        json!({"own_host": true, "run": ["work"]}),
        json!([
            {"match": "first", "steps": [
                {"requests": [
                    {"method": "_bzz/approve", "params": {"sessionId": session, "kind": "exec", "mount": "work", "command": "make one"}},
                    {"method": "_bzz/approve", "params": {"sessionId": session, "kind": "exec", "mount": "work", "command": "make again"}},
                    {"method": "_bzz/credential", "params": {"sessionId": session, "host": "github.com"}},
                    {"method": "_bzz/tool", "params": {"sessionId": session, "name": "send", "arguments": {"text": "working on it"}}},
                ]},
                send("first done", true),
            ]},
            {"match": "second", "steps": [
                {"request": {"method": "_bzz/approve", "params": {"sessionId": session, "kind": "exec", "mount": "work", "command": "make two"}}},
                send("second done", true),
            ]},
        ]),
    );
    let mut local = world.hands(&[("work", ExecGrant::Ask)]);
    local.network = vec!["github.com".into()];
    let host = world
        .host_configured(local, limits(), |options| {
            options.owner_present = true;
            options.git_credentials = Some(Arc::new(git_credentials::SealedGitCredentials {
                keys_dir: keys.clone(),
                owner: owner.clone(),
            }));
        })
        .await;
    let first = world.ask(&host, "the first question").await;
    let second = world.ask(&host, "the second question").await;
    // Two conversations, each with one question open.
    let waiting = until("both questions to wait", WAIT, || {
        Some(host.approvals()).filter(|waiting| waiting.len() == 2)
    })
    .await;
    let commands = waiting
        .iter()
        .map(|asked| asked.command.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(commands, ["make one", "make two"].into());
    assert_ne!(waiting[0].place, waiting[1].place);
    // The credential and the chat tool were answered while the command
    // still waits: the progress message is on the relay.
    until("the progress message", WAIT, || {
        world
            .relay
            .by_agent(9)
            .iter()
            .any(|event| event.content == "working on it")
            .then_some(())
    })
    .await;
    assert_eq!(host.approvals().len(), 2);
    // Answered in the other order than they were asked.
    for command in ["make two", "make one"] {
        let asked = waiting
            .iter()
            .find(|asked| asked.command == command)
            .unwrap();
        assert!(host.answer_approval(asked.id, Answer::Allow));
    }
    world.settled(&first).await;
    world.settled(&second).await;
    let all = world
        .script
        .log()
        .into_iter()
        .find(|entry| entry.get("responses").is_some())
        .expect("the answers to the requests sent at once");
    let (sent, answered) = (
        all["sent"].as_array().unwrap().clone(),
        all["answered"].as_array().unwrap().clone(),
    );
    assert!(
        sent.iter()
            .all(|id| id.as_str().is_some_and(|id| id.starts_with('h'))),
        "string ids, as the real host's: {sent:?}"
    );
    assert_eq!(answered.len(), 4, "each answered once: {answered:?}");
    assert_eq!(
        answered.last(),
        Some(&sent[0]),
        "the first one asked was the last one answered: {answered:?}"
    );
    let responses = all["responses"].as_array().unwrap();
    assert_eq!(responses[0]["response"]["result"], json!({"allow": true}));
    assert_eq!(
        responses[0]["response"]["id"], sent[0],
        "answered under its own id"
    );
    // One question at a time in one conversation.
    assert_eq!(responses[1]["response"]["result"]["allow"], json!(false));
    assert!(
        responses[1]["response"]["result"]["reason"]
            .as_str()
            .unwrap()
            .contains("One at a time"),
        "{}",
        responses[1]
    );
    assert_eq!(
        responses[2]["response"]["result"],
        json!({"username": "scout-bot", "password": "t0ken-marker"})
    );
    assert_eq!(
        responses[3]["response"]["result"]["text"]
            .as_str()
            .map(|text| text.starts_with("sent as ")),
        Some(true)
    );
    assert_eq!(
        world
            .contents(&world.relay.replies_to(&first))
            .last()
            .map(String::as_str),
        Some("first done")
    );
    host.shutdown().await;
    owner.lock().await;
}

/// M3, protocol section 8: bzz answers `_bzz/credential` only for a host
/// on that agent's list of allowed hosts and only for that agent, from what
/// the owner stored. Otherwise the host gets an error whose message tells
/// the model what its owner can do. Nothing of the secret is kept in the
/// agent's activity.
#[tokio::test]
async fn a_git_credential_is_given_for_a_listed_host_of_this_agent_only() {
    const TOKEN: &str = "ghp_NotARealTokenOnlyAMarkerForThisTest0000";
    let world = World::new().await;
    let owner = bzz::auth::signer::SignerHandle::spawn(world.owner.clone());
    let keys = world.data_dir().join("keys");
    let other_agent = Uuid::new_v4();
    // Stored: for this agent, a listed host and an unlisted one; for
    // another agent, a host this agent lists too.
    for (agent, host, token) in [
        (world.agent, "github.com", TOKEN),
        (world.agent, "unlisted.example", "tok-unlisted-marker"),
        (other_agent, "gitlab.com", "tok-of-another-agent"),
    ] {
        git_credentials::set(&keys, &owner, agent, host, "scout-bot", token)
            .await
            .unwrap();
    }
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            credential("GitHub.com"),
            credential("unlisted.example"),
            credential("gitlab.com"),
            credential("git.example.org:8443"),
            {"request": {"method": "_bzz/credential", "params": {"sessionId": "not-a-session", "host": "github.com"}}},
            {"request": {"method": "_bzz/credential", "params": {"sessionId": "$session"}}},
            send("done", true),
        ]}]),
    );
    let mut local = world.settings(Harness::PiDurable);
    local.network = vec![
        "git.example.org:8443".into(),
        "github.com".into(),
        "gitlab.com".into(),
    ];
    let host = world
        .host_configured(local, limits(), |options| {
            options.git_credentials = Some(Arc::new(git_credentials::SealedGitCredentials {
                keys_dir: keys.clone(),
                owner: owner.clone(),
            }));
        })
        .await;
    let request = world.ask(&host, "clone it").await;
    world.settled(&request).await;
    let answers = world.answers("_bzz/credential", 6).await;
    // The listed host of this agent: the stored value, and nothing else.
    assert_eq!(
        answers[0]["result"],
        json!({"username": "scout-bot", "password": TOKEN})
    );
    let message = |index: usize| {
        assert!(answers[index].get("result").is_none(), "{}", answers[index]);
        assert_eq!(answers[index]["error"]["code"], json!(-32011));
        answers[index]["error"]["message"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // Stored, but the host is not on the agent's allow-list.
    assert!(
        message(1).contains("not on this agent's list of allowed hosts")
            && message(1).contains("bzz agent edit Scout --allow-host unlisted.example"),
        "{}",
        message(1)
    );
    // Listed, but stored for another agent only.
    assert_eq!(
        message(2),
        "No credential is stored for gitlab.com. Your owner can store one with `bzz agent credential set Scout gitlab.com`."
    );
    // Listed, nothing stored.
    assert!(message(3).contains("No credential is stored for git.example.org:8443"));
    // A session that has no turn running, and a request without a host.
    assert!(message(4).contains("No message is being answered in this session"));
    assert!(message(5).contains("needs `sessionId` and `host`"));
    for answer in &answers[1..6] {
        assert!(!answer.to_string().contains("tok-"), "{answer}");
    }

    // The activity names the host and what happened, never the secret.
    assert_eq!(
        world.activity(),
        [
            "git credential for github.com · given",
            "git credential for unlisted.example · refused: the host is not on its list of allowed hosts",
            "git credential for gitlab.com · none is stored",
            "git credential for git.example.org:8443 · none is stored",
        ]
    );
    let kept = std::fs::read_to_string(activity::path(&world.data_dir(), world.agent)).unwrap();
    for secret in [
        TOKEN,
        "scout-bot",
        "tok-unlisted-marker",
        "tok-of-another-agent",
    ] {
        assert!(!kept.contains(secret), "the activity holds {secret}");
    }
    host.shutdown().await;

    // Without a store, as when nothing can be opened: none is given.
    let world = World::new().await;
    world.script.write(
        json!({"own_host": true}),
        json!([{"steps": [credential("github.com"), send("done", true)]}]),
    );
    let mut local = world.settings(Harness::PiDurable);
    local.network = vec!["github.com".into()];
    let host = world.host_with(local, limits()).await;
    let request = world.ask(&host, "clone it").await;
    world.settled(&request).await;
    assert!(
        world.answers("_bzz/credential", 1).await[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot open stored credentials")
    );
    host.shutdown().await;
    owner.lock().await;
}

/// Another harness is not bzz's own host: its `_bzz/approve` and
/// `_bzz/credential` are requests bzz does not know.
#[tokio::test]
async fn only_bzzs_own_host_may_ask_for_a_command_or_a_credential() {
    let world = World::new().await;
    let started = world.script.marker("started");
    world.script.write(
        json!({"own_host": false}),
        json!([{"steps": [approve("work", "id", &started), credential("github.com"), send("done", true)]}]),
    );
    let mut local = world.hands(&[("work", ExecGrant::WithoutAsking)]);
    local.harness = Harness::Codex;
    local.network = vec!["github.com".into()];
    let host = world.host_with_owner(local, limits()).await;
    let request = world.ask(&host, "try").await;
    world.settled(&request).await;
    for method in ["_bzz/approve", "_bzz/credential"] {
        assert_eq!(
            world.answers(method, 1).await[0]["error"]["code"],
            json!(-32601),
            "{method}"
        );
    }
    assert!(!started.exists());
    assert!(world.activity().is_empty());
    host.shutdown().await;
}

/// M7: two agents may each have a command waiting for the owner. Each is
/// answered by itself, and an answer for one does nothing to the other.
#[tokio::test]
async fn two_agents_may_each_have_a_command_waiting() {
    let world = World::new().await;
    let ran = |name: &str| world.script.marker(name);
    world.script.write(
        json!({"own_host": true, "run": ["work"]}),
        json!([{"steps": [approve("work", "make scout", &ran("scout")), send("scout done", true)]}]),
    );
    let host = world
        .host_with_owner(world.hands(&[("work", ExecGrant::Ask)]), limits())
        .await;
    // A second agent of the same owner, on the same host, with a harness
    // and a relay connection of its own.
    let ranger_keys = Keys::generate();
    let auth_tag = bzz::auth::signer::SignerHandle::spawn(world.owner.clone())
        .owner_auth_tag(ranger_keys.public_key(), "")
        .await
        .unwrap();
    let ranger_relay = RecordedRelay::new(
        ranger_keys.clone(),
        bzz::broker::relay::auth_tag(&auth_tag).unwrap(),
    );
    let facts = ChannelFacts {
        id: world.channel,
        name: "general".into(),
        kind: PlaceKind::Channel,
        members: vec![
            world.owner.public_key().to_hex(),
            ranger_keys.public_key().to_hex(),
        ],
    };
    ranger_relay.set_channel(facts.clone());
    let ranger_script = Script::new(&world.temporary.path().join("harness-ranger"));
    ranger_script.write(
        json!({"own_host": true, "run": ["work"]}),
        json!([{"steps": [approve("work", "make ranger", &ran("ranger")), send("ranger done", true)]}]),
    );
    let ranger = Uuid::new_v4();
    let mut local = world.hands(&[("work", ExecGrant::Ask)]);
    local.command = ranger_script.command();
    host.add(
        AgentSpec {
            id: ranger,
            name: "Ranger".into(),
            pubkey: ranger_keys.public_key().to_hex(),
            owner: world.owner.public_key().to_hex(),
            community: Uuid::nil(),
            local,
        },
        ranger_relay.clone(),
    )
    .await;
    world.ask(&host, "scout, build").await;
    let to_ranger = buzz_sdk::build_message(
        world.channel,
        "ranger, build",
        None,
        &[ranger_keys.public_key().to_hex().as_str()],
        false,
        &[],
        &[],
    )
    .unwrap()
    .sign_with_keys(&world.owner)
    .unwrap();
    ranger_relay.seed(&to_ranger);
    assert_eq!(host.offer(ranger, &to_ranger, &facts).await, Offer::Queued);

    let waiting = until("both agents to ask", WAIT, || {
        Some(host.approvals()).filter(|waiting| waiting.len() == 2)
    })
    .await;
    let of = |name: &str| {
        waiting
            .iter()
            .find(|asked| asked.agent_name == name)
            .unwrap()
    };
    assert_eq!(
        (of("Scout").agent, of("Scout").command.as_str()),
        (world.agent, "make scout")
    );
    assert_eq!(
        (of("Ranger").agent, of("Ranger").command.as_str()),
        (ranger, "make ranger")
    );
    // Ranger's is refused: Scout's still waits, and nothing has started.
    assert!(host.answer_approval(of("Ranger").id, Answer::Refuse(None)));
    until("ranger's turn to end", WAIT, || {
        (!ranger_relay.replies_to(&to_ranger).is_empty()).then_some(())
    })
    .await;
    assert_eq!(host.approvals().len(), 1);
    assert_eq!(host.approvals()[0].agent, world.agent);
    assert!(!ran("scout").exists() && !ran("ranger").exists());
    assert!(host.answer_approval(of("Scout").id, Answer::Allow));
    until("scout's command to start", WAIT, || {
        ran("scout").exists().then_some(())
    })
    .await;
    assert!(
        !ran("ranger").exists(),
        "a yes for one is not a yes for the other"
    );
    // Each agent's activity is its own.
    assert!(
        world
            .activity()
            .iter()
            .all(|line| line.contains("make scout"))
    );
    let rangers = activity::recent(&world.data_dir(), ranger, 10);
    assert_eq!(rangers.len(), 2);
    assert!(rangers[1].line().ends_with("make ranger · refused by you"));
    host.shutdown().await;
}

/// M7 and M3: an agent runs on the settings bzz started with, but what its
/// owner takes away afterwards is refused at once, by the bzz that is
/// running: a folder that is no longer granted for commands, a folder that
/// now asks, a host that is no longer allowed, and an agent that is paused
/// or gone from the settings. In doubt (a settings file that cannot be
/// read) nothing is allowed and nothing is given.
#[tokio::test]
async fn what_the_owner_takes_away_is_refused_at_once_by_a_running_bzz() {
    let world = World::new().await;
    let owner = bzz::auth::signer::SignerHandle::spawn(world.owner.clone());
    let keys = world.data_dir().join("keys");
    git_credentials::set(
        &keys,
        &owner,
        world.agent,
        "github.com",
        "bot",
        "t0ken-marker",
    )
    .await
    .unwrap();
    let marker = |name: &str| world.script.marker(name);
    let steps = (1..=6)
        .flat_map(|round| {
            vec![
                approve(
                    "ops",
                    &format!("make {round}"),
                    &marker(&format!("ran-{round}")),
                ),
                credential("github.com"),
                json!({"touch": marker(&format!("asked-{round}"))}),
                json!({"wait_for": marker(&format!("go-{round}"))}),
            ]
        })
        .chain([send("done", true)])
        .collect::<Vec<_>>();
    world.script.write(
        json!({"own_host": true, "run": ["ops"]}),
        json!([{"steps": steps}]),
    );
    let mut local = world.hands(&[("ops", ExecGrant::WithoutAsking)]);
    local.network = vec!["github.com".into()];
    // The settings file, as `bzz agent edit` writes it.
    let settings_file = world.temporary.path().join("config.toml");
    let write = |local: Option<&LocalAgentConfig>, paused: bool| {
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
                paused,
                local: local.cloned(),
            }],
            ..bzz::config::Config::default()
        };
        std::fs::write(&settings_file, toml::to_string_pretty(&config).unwrap()).unwrap();
    };
    write(Some(&local), false);
    let host = world
        .host_configured(local.clone(), limits(), |options| {
            options.settings_file = Some(settings_file.clone());
            options.git_credentials = Some(Arc::new(git_credentials::SealedGitCredentials {
                keys_dir: keys.clone(),
                owner: owner.clone(),
            }));
        })
        .await;
    let request = world.ask(&host, "build").await;
    // One round: what the harness was answered for its command and for its
    // credential, then the next change of the settings.
    let round = async |number: usize| {
        let asked = marker(&format!("asked-{number}"));
        until(&format!("round {number}"), WAIT, || {
            asked.exists().then_some(())
        })
        .await;
        let command = world.answers("_bzz/approve", number).await[number - 1].clone();
        let credential = world.answers("_bzz/credential", number).await[number - 1].clone();
        (
            command["result"]["allow"].as_bool().unwrap(),
            command["result"]["reason"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            credential.get("result").is_some(),
        )
    };
    let next = |number: usize| std::fs::write(marker(&format!("go-{number}")), "go\n").unwrap();

    // 1. As bzz started: the command runs, the credential is given.
    let (allowed, _, given) = round(1).await;
    assert!(allowed && given && marker("ran-1").exists());

    // 2. The owner takes the host off the list, and makes the folder ask.
    //    Nobody is at this bzz to ask, so the command is refused.
    let mut edited = local.clone();
    edited.network.clear();
    edited.folders[0].exec = ExecGrant::Ask;
    write(Some(&edited), false);
    next(1);
    let (allowed, reason, given) = round(2).await;
    assert!(!allowed && !given, "{reason}");
    assert!(reason.contains("your owner is not at bzz"), "{reason}");
    assert!(!marker("ran-2").exists());

    // 3. The grant for commands is taken away; the host is allowed again.
    edited.network = vec!["github.com".into()];
    edited.folders[0].exec = ExecGrant::No;
    write(Some(&edited), false);
    next(2);
    let (allowed, reason, given) = round(3).await;
    assert!(!allowed && given, "{reason}");
    assert!(
        reason.contains("your owner has taken that grant away"),
        "{reason}"
    );

    // 4. The folder is granted again, but it is another folder of the
    //    machine under the same name: not the one this bzz told the host of.
    edited.folders[0].exec = ExecGrant::WithoutAsking;
    edited.folders[0].path = world.temporary.path().join("elsewhere");
    write(Some(&edited), false);
    next(3);
    let (allowed, reason, _) = round(4).await;
    assert!(!allowed, "{reason}");

    // 5. The settings cannot be read: in doubt, nothing.
    std::fs::write(&settings_file, "this is [not toml").unwrap();
    next(4);
    let (allowed, reason, given) = round(5).await;
    assert!(!allowed && !given, "{reason}");
    assert!(reason.contains("could not read its settings"), "{reason}");

    // 6. The agent is paused in the settings, by a command.
    write(Some(&local), true);
    next(5);
    let (allowed, reason, given) = round(6).await;
    assert!(!allowed && !given, "{reason}");
    assert!(reason.contains("no longer one that bzz runs"), "{reason}");
    for number in 2..=6 {
        assert!(!marker(&format!("ran-{number}")).exists(), "round {number}");
    }
    next(6);
    world.settled(&request).await;
    let activity = world.activity();
    assert!(
        activity.contains(
            &"command in /mnt/ops · make 3 · refused: the folder is no longer granted for commands"
                .to_owned()
        ),
        "{activity:?}"
    );
    host.shutdown().await;
    owner.lock().await;
}

/// M7: "without asking" holds only while the people who may wake the agent
/// are ones its owner named. An agent that anyone may wake would otherwise
/// run commands for anyone, as its owner: there the owner is asked, whatever
/// the folder's grant says, and with nobody to ask the answer is no.
#[tokio::test]
async fn a_folder_allowed_without_asking_still_asks_when_anyone_may_wake_the_agent() {
    for owner_present in [false, true] {
        let world = World::new().await;
        let started = world.script.marker("started");
        world.script.write(
            json!({"own_host": true, "run": ["ops"]}),
            json!([{"steps": [approve("ops", "systemctl restart site", &started), send("done", true)]}]),
        );
        let mut local = world.hands(&[("ops", ExecGrant::WithoutAsking)]);
        local.wake = WakeRule::Anyone;
        let host = world
            .host_configured(local, limits(), |options| {
                options.owner_present = owner_present
            })
            .await;
        // Someone who is not the owner wakes it.
        let request = world.message_in(
            world.channel,
            &world.stranger,
            "restart the site",
            true,
            None,
        );
        assert_eq!(world.offer(&host, &request).await, Offer::Queued);
        if owner_present {
            let asked = one_question(&host).await;
            assert_eq!(asked.requester, world.stranger.public_key().to_hex());
            assert!(!started.exists(), "it waits for the owner");
            assert!(host.answer_approval(asked.id, Answer::Refuse(None)));
        }
        world.settled(&request).await;
        let answer = world.answers("_bzz/approve", 1).await[0]["result"].clone();
        assert_eq!(answer["allow"], json!(false), "{answer}");
        assert!(!started.exists());
        assert_eq!(
            world.activity().last().map(String::as_str),
            Some(if owner_present {
                "command in /mnt/ops · systemctl restart site · refused by you"
            } else {
                "command in /mnt/ops · systemctl restart site · refused: nobody was at bzz to ask"
            })
        );
        host.shutdown().await;
    }
}
