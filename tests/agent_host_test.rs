//! v0.15 M2, bzz's side: the agent runtime with the real Pi Durable host
//! (`agent-host/`), installed on first use from the files inside the bzz
//! binary, against a recorded relay. The model is the host's scripted
//! stand-in; nothing here reads Pi's real folder or calls a model.
//!
//! These tests need Node.js 22.19 or newer and, the first time, the npm
//! registry. Without Node they skip and say so.
#![cfg(unix)]

mod support;

use std::{sync::Arc, time::Duration};

use bzz::{
    agents::runtime::{
        Limits,
        admission::{ChannelFacts, PlaceKind},
        agent::{AgentSpec, Phase},
        host::{AgentHost, HostOptions, Offer},
        install, process,
    },
    config::{AgentModelConfig, Harness, LocalAgentConfig},
    store::{
        Store,
        agent_runtime::{InboxRow, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, Keys};
use support::{
    agent_harness::{RecordedRelay, kill, running, until},
    pi_host,
};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(60);

struct World {
    temporary: TempDir,
    owner: Keys,
    relay: Arc<RecordedRelay>,
    store: StoreHandle,
    agent: Uuid,
    channel: Uuid,
    node: std::path::PathBuf,
}

impl World {
    async fn new(node: &install::Node) -> Self {
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
            node: pi_host::scripted_node(&temporary.path().join("bin"), node),
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
            members: vec![
                world.owner.public_key().to_hex(),
                world.relay.agent.public_key().to_hex(),
            ],
        });
        world
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.temporary.path().join("data")
    }

    fn pi_dir(&self) -> std::path::PathBuf {
        self.temporary.path().join("pi")
    }

    /// The agent as `bzz agent new` would write it: the pi-durable harness
    /// with no launch override, on the stand-in model.
    fn settings(&self) -> LocalAgentConfig {
        let mut local = LocalAgentConfig::new(Harness::PiDurable);
        local.model = Some(AgentModelConfig {
            provider: "scripted".into(),
            model_id: "scripted".into(),
            thinking_level: None,
        });
        local.instructions = "Be brief.".into();
        local
    }

    async fn host(&self) -> AgentHost {
        self.host_with(self.settings()).await
    }

    async fn host_with(&self, local: LocalAgentConfig) -> AgentHost {
        let mut options = HostOptions::new(self.data_dir(), self.store.clone());
        options.limits = Limits {
            stop_grace: Duration::from_millis(500),
            restart_backoff: Duration::from_millis(100),
            typing_interval: Duration::from_millis(200),
            ..Limits::default()
        };
        options.pi_dir = Some(self.pi_dir());
        let environment = pi_host::environment(&self.node, &self.pi_dir());
        options.environment = Arc::new(environment);
        let host = AgentHost::start(options).await.unwrap();
        host.add(
            AgentSpec {
                id: self.agent,
                name: "Scout".into(),
                pubkey: self.relay.agent.public_key().to_hex(),
                owner: self.owner.public_key().to_hex(),
                community: Uuid::nil(),
                local,
            },
            self.relay.clone(),
        )
        .await;
        host
    }

    /// A mention of the agent by its owner: top level, or in `thread`.
    async fn ask(&self, host: &AgentHost, text: &str, thread: Option<&Event>) -> Event {
        let agent = self.relay.agent.public_key().to_hex();
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
        let facts =
            bzz::agents::runtime::relay::AgentRelay::channel(self.relay.as_ref(), self.channel)
                .await
                .unwrap();
        assert_eq!(
            host.offer(self.agent, &event, &facts).await,
            Offer::Queued,
            "{text}"
        );
        event
    }

    async fn row(&self, request: &Event) -> Option<InboxRow> {
        let (agent, id) = (self.agent, request.id.to_hex());
        self.store
            .call(move |store| store.agent_inbox_get(agent, &id))
            .await
            .unwrap()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(
            process::agent_dir(&self.data_dir(), self.agent).join("harness.log"),
        )
        .unwrap_or_default()
    }

    /// Waits for the request's turn to be over and its reply acknowledged.
    async fn settled(&self, host: &AgentHost, request: &Event) -> InboxRow {
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
                "the request never settled: {:?}\nstatus: {:?}\nhost log:\n{}",
                self.row(request).await,
                host.status(),
                self.log()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn replies(&self, request: &Event) -> Vec<String> {
        self.relay
            .replies_to(request)
            .into_iter()
            .map(|event| event.content)
            .collect()
    }

    fn pid(&self) -> Option<u32> {
        process::read_record(&process::run_dir(&self.data_dir()), self.agent)
            .map(|record| record.process.pid)
    }
}

/// The plan's M2 exit, on bzz's side, against a recorded relay: the host is
/// installed on first use and started by its default launch; a mention is
/// answered once; the host is killed in the middle of a turn and that turn
/// ends with one reply; after bzz and the host have both restarted, the same
/// thread remembers and a file on the agent's disk is still there.
#[tokio::test]
async fn the_real_host_is_installed_started_and_answers_once_across_kills_and_restarts() {
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    let world = World::new(&node).await;
    assert!(!install::is_installed(&world.data_dir()));
    let host = world.host().await;
    assert!(
        !install::is_installed(&world.data_dir()),
        "nothing is installed before an agent needs it"
    );

    // A mention is answered once. The first one installs the host.
    let first = world.ask(&host, "hello there", None).await;
    let row = world.settled(&host, &first).await;
    assert_eq!(
        (row.state, row.attempts),
        (RequestState::Replied, 1),
        "{}",
        world.log()
    );
    assert_eq!(
        world.replies(&first),
        ["pong: this conversation holds 1 message(s) from people"]
    );
    assert!(install::is_installed(&world.data_dir()));
    let folder = install::host_dir(&world.data_dir());
    assert!(folder.join("node_modules").is_dir() && folder.join("main.mjs").is_file());
    // Rule 13: no package brought an install script's leftovers, and the
    // optional native bindings were left out.
    assert!(!folder.join("node_modules/node-liblzma").exists());
    // The agent's two files are in its own folder, nowhere else.
    let agent_folder = process::agent_dir(&world.data_dir(), world.agent);
    assert!(agent_folder.join("memory.sqlite").is_file());
    let status = host.status();
    assert_eq!(status[0].phase, Phase::Running);
    let first_pid = world.pid().expect("the host is running");
    assert!(running(first_pid));

    // A file written on the agent's disk through its own shell.
    let wrote = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "echo kept across restarts > /home/agent/note.txt && cat /home/agent/note.txt"}], ["report"]]"#,
            None,
        )
        .await;
    world.settled(&host, &wrote).await;
    let reported = world.replies(&wrote);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("kept across restarts"), "{reported:?}");
    assert!(agent_folder.join("files.sqlite").is_file());
    assert!(
        !agent_folder.join("note.txt").exists(),
        "the agent's file is on its disk, not on the machine"
    );

    // Killed in the middle of a turn: the turn ends with one reply.
    let cut = world
        .ask(
            &host,
            r#"do: [["sleep", 4000], ["buzz_send", {"text": "answered after the host was killed", "final": true}]]"#,
            None,
        )
        .await;
    until("the turn to be running", WAIT, || {
        (host.status()[0].turns == 1).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(world.replies(&cut).is_empty());
    kill(first_pid);
    let row = world.settled(&host, &cut).await;
    assert_eq!(
        (row.state, row.attempts),
        (RequestState::Replied, 2),
        "handed over again to the new process\n{}",
        world.log()
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        world.replies(&cut),
        ["answered after the host was killed"],
        "one reply, whatever was killed"
    );
    let second_pid = world.pid().expect("a new host is running");
    assert_ne!(second_pid, first_pid);
    assert!(!running(first_pid));

    // bzz and the host both restart.
    host.shutdown().await;
    assert!(!running(second_pid), "no host outlives the runtime");
    assert!(world.pid().is_none());
    let host = world.host().await;

    // The same thread remembers what was said before the restart.
    let again = world.ask(&host, "and now?", Some(&first)).await;
    world.settled(&host, &again).await;
    assert_eq!(
        world.replies(&again),
        ["pong: this conversation holds 2 message(s) from people"],
        "the conversation of the thread survived both restarts"
    );
    // The file is still on the agent's disk.
    let read = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "cat /home/agent/note.txt"}], ["report"]]"#,
            None,
        )
        .await;
    world.settled(&host, &read).await;
    let reported = world.replies(&read);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("kept across restarts"), "{reported:?}");
    // Every mention has exactly one reply on the relay.
    assert_eq!(world.relay.by_agent(9).len(), 5);
    // What the host was started with: Node, its main file, the agent's
    // folder; an environment built from nothing.
    let third_pid = world.pid().unwrap();
    assert!(![first_pid, second_pid].contains(&third_pid));
    #[cfg(target_os = "linux")]
    {
        let read = |name: &str| {
            String::from_utf8_lossy(&std::fs::read(format!("/proc/{third_pid}/{name}")).unwrap())
                .split('\0')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let arguments = read("cmdline");
        assert_eq!(
            arguments[arguments.len() - 2..],
            [
                folder.join("main.mjs").display().to_string(),
                agent_folder.display().to_string()
            ]
        );
        let mut names = read("environ")
            .into_iter()
            .filter_map(|entry| entry.split_once('=').map(|(name, _)| name.to_owned()))
            // The shell that the stand-in for `node` is written in adds it.
            .filter(|name| name != "PWD")
            .collect::<Vec<_>>();
        names.sort();
        // The stand-in adds the first one; bzz gave the other four.
        assert_eq!(
            names,
            [
                "BZZ_AGENT_HOST_SCRIPTED_MODEL",
                "HOME",
                "LANG",
                "PATH",
                "TMPDIR"
            ]
        );
    }
    host.shutdown().await;
    assert!(!running(third_pid));
}

/// What bzz tells the real host at `initialize` is what the host enforces,
/// and what the host says of a failure is what bzz posts: a granted folder
/// is read under `/mnt/<name>` and not written; a provider's error and a
/// model that cannot be used are named as such, never as "sign in"; and the
/// check of a model before an agent is created is the host's own.
#[tokio::test]
async fn the_real_host_keeps_to_the_grants_and_names_its_failures() {
    use bzz::{agents::runtime::install::ModelCheck, config::FolderGrant};
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    let world = World::new(&node).await;
    let site = world.temporary.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("hello.txt"), "hello from the machine\n").unwrap();
    let mut local = world.settings();
    local.folders = vec![FolderGrant {
        name: "site".into(),
        path: site.clone(),
        write: false,
    }];
    let host = world.host_with(local).await;

    // The granted folder, read-only, as the owner granted it.
    let read = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "cat /mnt/site/hello.txt"}], ["report"]]"#,
            None,
        )
        .await;
    world.settled(&host, &read).await;
    let reported = world.replies(&read);
    assert!(
        reported.len() == 1 && reported[0].contains("hello from the machine"),
        "{reported:?}\n{}",
        world.log()
    );
    let write = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "echo changed > /mnt/site/hello.txt"}], ["bash", {"command": "echo new > /mnt/site/new.txt"}], ["report"]]"#,
            None,
        )
        .await;
    world.settled(&host, &write).await;
    assert_eq!(world.replies(&write).len(), 1);
    assert_eq!(
        std::fs::read_to_string(site.join("hello.txt")).unwrap(),
        "hello from the machine\n",
        "a folder granted read-only is not written"
    );
    assert!(!site.join("new.txt").exists());

    // A provider's error: named as that, with none of its words.
    let failed = world
        .ask(
            &host,
            r#"do: [["error", "upstream exploded at 10.1.2.3"]]"#,
            None,
        )
        .await;
    let row = world.settled(&host, &failed).await;
    assert_eq!(
        (row.state, row.failure.as_deref()),
        (RequestState::Failed, Some("provider-error")),
        "{}",
        world.log()
    );
    assert_eq!(
        world.replies(&failed),
        ["I could not answer: my model returned an error."]
    );
    // The agent goes on: the next mention is answered.
    let after = world.ask(&host, "still there?", None).await;
    let row = world.settled(&host, &after).await;
    assert_eq!(row.state, RequestState::Replied, "{}", world.log());
    host.shutdown().await;

    // The check `agent new` makes before anything exists, asked of the
    // installed host itself. Pi's folder here is empty: no sign-in at all.
    let installed = install::host_dir(&world.data_dir());
    let stand_in = install::Node {
        program: world.node.clone(),
        version: node.version,
    };
    let environment = pi_host::environment(&world.node, &world.pi_dir());
    let model = |provider: &str, model_id: &str| AgentModelConfig {
        provider: provider.into(),
        model_id: model_id.into(),
        thinking_level: None,
    };
    let check = async |wanted: Option<AgentModelConfig>| {
        install::check_model(
            &stand_in,
            &installed,
            &world.pi_dir(),
            wanted.as_ref(),
            environment.clone(),
        )
        .await
    };
    assert_eq!(
        check(Some(model("scripted", "scripted"))).await,
        ModelCheck::Available {
            provider: "scripted".into(),
            model_id: "scripted".into(),
            // The level Pi would use when none is named.
            thinking_level: Some("off".into()),
        }
    );
    for (wanted, kind) in [
        (Some(model("nope", "none")), "provider_unavailable"),
        (
            Some(model("scripted", "no-such-model")),
            "model_unavailable",
        ),
        (Some(model("openai-codex", "gpt-6-luna")), "sign_in_needed"),
    ] {
        match check(wanted.clone()).await {
            ModelCheck::Unavailable {
                kind: found,
                message,
            } => {
                assert_eq!(found, kind, "{wanted:?}: {message}");
                assert!(!message.is_empty());
            }
            other => panic!("{wanted:?}: {other:?}"),
        }
    }
    assert!(
        !world.pi_dir().exists(),
        "asking leaves Pi's folder as it was: not even created"
    );

    // An agent whose model cannot be used says so when it is asked.
    let mut unusable = world.settings();
    unusable.model = Some(model("nope", "none"));
    let host = world.host_with(unusable).await;
    let asked = world.ask(&host, "anyone?", None).await;
    let row = world.settled(&host, &asked).await;
    assert_eq!(
        (row.state, row.failure.as_deref()),
        (RequestState::Failed, Some("model-unavailable")),
        "{}",
        world.log()
    );
    assert_eq!(
        world.replies(&asked),
        ["I could not answer: the model my owner chose for me is not available."]
    );
    let published = world
        .relay
        .by_agent(9)
        .into_iter()
        .map(|event| event.content)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!published.contains("10.1.2.3"), "{published}");
    host.shutdown().await;
}

/// Without a Node that is new enough the agent does not start and its
/// status says what to do. Whoever asked is told once that it is not
/// available, and nothing is installed.
#[tokio::test]
async fn without_node_the_agent_says_what_to_do() {
    // No Node is needed for this one: bzz is told of one that is not there.
    let mut world = World::new(&install::Node {
        program: "/bin/true".into(),
        version: (22, 19, 0),
    })
    .await;
    world.node = "/nonexistent/node".into();
    let host = world.host().await;
    let asked = world.ask(&host, "anyone there?", None).await;
    let row = world.settled(&host, &asked).await;
    assert_eq!(
        (row.state, row.failure.as_deref(), row.attempts),
        (RequestState::Failed, Some("unavailable"), 0),
        "never handed to anything"
    );
    assert_eq!(
        world.replies(&asked),
        ["I am not available right now: my process keeps stopping."]
    );
    let status = host.status();
    let problem = status[0].problem.clone().unwrap_or_default();
    assert!(
        problem.contains("BZZ_NODE") && problem.contains("22.19"),
        "the status says what to do: {problem}"
    );
    assert_eq!(status[0].phase, Phase::Unavailable);
    assert!(
        !install::host_root(&world.data_dir()).exists(),
        "nothing was installed"
    );
    host.shutdown().await;
}
