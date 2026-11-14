//! v0.15 M1: the agent runtime against a scripted harness
//! (`examples/scripted_harness.rs`) and a recorded relay. Each test names the
//! rule of ADR v0.15 local agents or of `docs/agent-host-protocol.md` that it
//! shows. Nothing here needs Node, a model or a network.
#![cfg(unix)]

mod support;

use std::{sync::Arc, time::Duration};

use bzz::{
    agents::runtime::{
        Limits,
        admission::{ChannelFacts, PlaceKind, Refusal},
        agent::{AgentSpec, Phase},
        host::{AgentHost, HostOptions, Offer},
        process::{self, Leftover},
    },
    config::{
        AgentModelConfig, FolderGrant, Harness, HostedAgentConfig, LocalAgentConfig,
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
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = limits;
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        let host = AgentHost::start(options).await.unwrap();
        host.add(self.spec(local), self.relay.clone()).await;
        host
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
    local.folders = vec![FolderGrant {
        name: "site".into(),
        path: "/home/ann/site".into(),
        write: false,
    }];
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
            "mounts": [{"name": "site", "path": "/home/ann/site", "write": false}],
            "network": ["github.com"],
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
    until("the reply to be acknowledged", WAIT, || {
        (world.relay.replies_to(&request).len() == 1).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
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
