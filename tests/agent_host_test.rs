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
            read_only: Vec::new(),
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
        self.host_configured(local, |_| {}).await
    }

    /// A host whose options `configure` may change: whether the owner is
    /// there to be asked, and what he stored for git.
    async fn host_configured(
        &self,
        local: LocalAgentConfig,
        configure: impl FnOnce(&mut HostOptions),
    ) -> AgentHost {
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
        configure(&mut options);
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

    // Killed in the middle of a turn: the turn ends with one reply. The
    // host reports this turn's first tool call (by its own words: the turn
    // before may still be listed a moment), and is then in the turn's
    // sleep: the kill comes then, not after a fixed time that a loaded
    // machine can stretch past the sleep.
    let cut = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "echo before the kill"}], ["sleep", 4000], ["buzz_send", {"text": "answered after the host was killed", "final": true}]]"#,
            None,
        )
        .await;
    until("the turn to be past its first tool", WAIT, || {
        host.status()[0]
            .activity
            .as_deref()
            .is_some_and(|activity| activity.contains("echo before the kill"))
            .then_some(())
    })
    .await;
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
        exec: bzz::config::ExecGrant::No,
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
            true,
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
                ..
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

/// The write-ahead files beside an agent's two files, when they are there.
fn write_ahead_files(folder: &std::path::Path) -> Vec<String> {
    let mut found = ["memory.sqlite", "files.sqlite"]
        .iter()
        .flat_map(|file| [format!("{file}-wal"), format!("{file}-shm")])
        .filter(|name| folder.join(name).exists())
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// When the real host has been idle for the stated time, bzz asks it to
/// checkpoint (`_bzz/checkpoint`): its two files are then whole, with no
/// write-ahead file beside them, the host goes on running, and the next
/// message of the conversation finds its memory. Not before that time, and
/// not while a turn runs.
#[tokio::test]
async fn the_real_host_checkpoints_its_files_when_it_has_been_idle() {
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    const IDLE: Duration = Duration::from_secs(4);
    let world = World::new(&node).await;
    let host = world
        .host_configured(world.settings(), |options| {
            options.limits.checkpoint_idle = IDLE;
        })
        .await;
    // A turn that writes to both files: its conversation and its disk.
    let first = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "echo kept > /home/agent/note.txt && cat /home/agent/note.txt"}], ["report"]]"#,
            None,
        )
        .await;
    world.settled(&host, &first).await;
    let answered = tokio::time::Instant::now();
    assert_eq!(world.replies(&first).len(), 1, "{}", world.log());
    let folder = process::agent_dir(&world.data_dir(), world.agent);
    let pid = world.pid().expect("the host is running");
    assert_eq!(
        write_ahead_files(&folder),
        [
            "files.sqlite-shm",
            "files.sqlite-wal",
            "memory.sqlite-shm",
            "memory.sqlite-wal"
        ],
        "a host that runs keeps its newest writes beside its files"
    );
    // Not before the time is up: looked at half way, and counted from when
    // the test looked, which a loaded machine can make later than planned.
    tokio::time::sleep(IDLE / 2).await;
    let looked = answered.elapsed();
    assert!(
        !write_ahead_files(&folder).is_empty() || looked + Duration::from_secs(1) >= IDLE,
        "checkpointed early, {looked:?} after the answer"
    );
    until("the idle host to checkpoint its files", WAIT, || {
        write_ahead_files(&folder).is_empty().then_some(())
    })
    .await;
    // The host went idle a moment before this test saw the answer (`settled`
    // looks now and then), so the time is counted with that moment allowed
    // for. On a busy machine it was 7 ms.
    const LATE: Duration = Duration::from_secs(1);
    assert!(
        answered.elapsed() + LATE >= IDLE,
        "the files were made whole {:?} after the answer, before the host had been idle for {IDLE:?}",
        answered.elapsed()
    );
    assert_eq!(world.pid(), Some(pid), "the host goes on running");
    assert!(running(pid));
    for file in ["memory.sqlite", "files.sqlite"] {
        assert!(folder.join(file).is_file(), "{file}");
    }

    // The conversation goes on from the files as they were left.
    let second = world
        .ask(
            &host,
            r#"do: [["bash", {"command": "cat /home/agent/note.txt"}], ["report"]]"#,
            Some(&first),
        )
        .await;
    world.settled(&host, &second).await;
    let reported = world.replies(&second);
    assert!(
        reported.len() == 1 && reported[0].contains("kept"),
        "{reported:?}\n{}",
        world.log()
    );
    assert_eq!(world.pid(), Some(pid), "in the same process");

    // While a turn runs nothing is checkpointed: a turn longer than the
    // idle time ends with its answer, and the files are whole only after.
    let long = world
        .ask(
            &host,
            &format!(
                r#"do: [["sleep", {}], ["buzz_send", {{"text": "after a long turn", "final": true}}]]"#,
                (IDLE * 2).as_millis()
            ),
            None,
        )
        .await;
    until("the long turn to be running", WAIT, || {
        (host.status()[0].turns == 1).then_some(())
    })
    .await;
    // Looked at for as long as the turn runs, rather than once after a
    // fixed time that a loaded machine can stretch past the turn's end.
    let running_since = tokio::time::Instant::now();
    while host.status()[0].turns == 1 {
        assert!(
            !write_ahead_files(&folder).is_empty(),
            "checkpointed while a turn ran, {:?} into it",
            running_since.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        running_since.elapsed() + Duration::from_secs(1) >= IDLE + IDLE / 2,
        "the turn ran {:?}, not past the idle time",
        running_since.elapsed()
    );
    world.settled(&host, &long).await;
    assert_eq!(world.replies(&long), ["after a long turn"]);
    until("the idle host to checkpoint again", WAIT, || {
        write_ahead_files(&folder).is_empty().then_some(())
    })
    .await;
    assert_eq!(world.pid(), Some(pid));
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

/// One step of the stand-in model that starts a real command and one that
/// reports what came back, as the text of a mention.
fn run_and_report(folder: &str, command: &str) -> String {
    format!(
        "do: {}",
        serde_json::json!([["run", {"folder": folder, "command": command}], ["report"]])
    )
}

/// The agent's activity without the time each line starts with.
fn activity_of(world: &World) -> Vec<String> {
    bzz::agents::activity::recent(&world.data_dir(), world.agent, 50)
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

/// The plan's M7 exit, on bzz's side, with the real host: a real command in
/// a folder granted for commands runs after the owner allows it, exactly as
/// he saw it, and its output reaches the chat; a refused one does not run,
/// and the agent is told why; a folder allowed without asking is not asked
/// about; without the TUI a command that needs approval is refused with a
/// reason for the people in the chat.
#[tokio::test]
async fn the_real_host_runs_a_command_only_with_the_owners_yes() {
    use bzz::{
        agents::runtime::{approval::Answer, install::StartReport},
        config::{ExecGrant, FolderGrant},
    };
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    let world = World::new(&node).await;
    let grant = |name: &str, exec| {
        let path = world.temporary.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        FolderGrant {
            name: name.into(),
            path,
            write: false,
            exec,
        }
    };
    let mut local = world.settings();
    local.folders = vec![
        grant("work", ExecGrant::Ask),
        grant("ops", ExecGrant::WithoutAsking),
        grant("docs", ExecGrant::No),
    ];
    let (work, ops, docs) = (
        local.folders[0].path.clone(),
        local.folders[1].path.clone(),
        local.folders[2].path.clone(),
    );
    let host = world
        .host_configured(local.clone(), |options| options.owner_present = true)
        .await;
    let one_question = |host: &AgentHost| {
        let host = host.clone();
        async move {
            until("a command to wait for the owner", WAIT, move || {
                let mut waiting = host.approvals();
                (waiting.len() == 1).then(|| waiting.remove(0))
            })
            .await
        }
    };

    // ---- allowed --------------------------------------------------------
    // Two lines, a quote and a redirection: what the owner sees is what the
    // shell gets.
    let command = "printf 'made by a real %s\\n' command > made.txt &&\n  cat made.txt && pwd";
    let request = world
        .ask(&host, &run_and_report("work", command), None)
        .await;
    let asked = one_question(&host).await;
    assert_eq!(asked.command, command, "the command, untouched");
    assert_eq!((asked.folder.as_str(), &asked.path), ("work", &work));
    assert_eq!(asked.agent_name, "Scout");
    assert!(
        !work.join("made.txt").exists(),
        "nothing ran while it waited"
    );
    assert!(host.answer_approval(asked.id, Answer::Allow));
    world.settled(&host, &request).await;
    let reply = world.replies(&request);
    assert_eq!(reply.len(), 1, "{reply:?}\n{}", world.log());
    assert!(
        reply[0].contains("made by a real command")
            && reply[0].contains(&work.display().to_string()),
        "its output reached the chat: {reply:?}\n{}",
        world.log()
    );
    assert_eq!(
        std::fs::read_to_string(work.join("made.txt")).unwrap(),
        "made by a real command\n",
        "it ran on this machine, in the granted folder"
    );
    // What the host reported at its start, kept for `agent show`.
    let report = StartReport::read(&world.data_dir(), world.agent).expect("the start report");
    assert_eq!(report.run, ["work", "ops"]);
    let package =
        serde_json::from_str::<serde_json::Value>(include_str!("../agent-host/package.json"))
            .unwrap();
    assert_eq!(Some(report.host.as_str()), package["version"].as_str());

    // ---- refused, with a reason -------------------------------------------
    let request = world
        .ask(&host, &run_and_report("work", "touch refused.txt"), None)
        .await;
    let asked = one_question(&host).await;
    assert!(host.answer_approval(asked.id, Answer::Refuse(Some("not in this folder".into()))));
    world.settled(&host, &request).await;
    let reply = world.replies(&request);
    assert!(
        reply.len() == 1
            && reply[0].contains("Your owner refused this command, and says: not in this folder"),
        "the agent was told why: {reply:?}\n{}",
        world.log()
    );
    assert!(
        !work.join("refused.txt").exists(),
        "a refused command does not run"
    );

    // ---- allowed without asking, and a folder with no commands -------------
    let request = world
        .ask(
            &host,
            &run_and_report("ops", "echo restarted > done.txt && cat done.txt"),
            None,
        )
        .await;
    world.settled(&host, &request).await;
    assert!(host.approvals().is_empty(), "the owner was not asked");
    assert!(world.replies(&request)[0].contains("restarted"));
    assert!(ops.join("done.txt").exists());
    let request = world
        .ask(&host, &run_and_report("docs", "touch never.txt"), None)
        .await;
    world.settled(&host, &request).await;
    assert!(!docs.join("never.txt").exists());
    assert!(host.approvals().is_empty());

    assert_eq!(
        activity_of(&world),
        [
            "command in /mnt/work · printf 'made by a real %s\\n' command > made.txt && (and 1 more line(s)) · asked of you",
            "command in /mnt/work · printf 'made by a real %s\\n' command > made.txt && (and 1 more line(s)) · allowed by you",
            "command in /mnt/work · touch refused.txt · asked of you",
            "command in /mnt/work · touch refused.txt · refused by you: not in this folder",
            "command in /mnt/ops · echo restarted > done.txt && cat done.txt · allowed without asking",
        ],
        "the host does not even ask about a folder without `exec`"
    );
    host.shutdown().await;

    // ---- without the TUI ---------------------------------------------------
    let host = world.host_with(local).await;
    let request = world
        .ask(&host, &run_and_report("work", "touch headless.txt"), None)
        .await;
    world.settled(&host, &request).await;
    let reply = world.replies(&request);
    assert!(
        reply.len() == 1
            && reply[0].contains("needs your owner's approval")
            && reply[0].contains("your owner must open bzz to approve the command"),
        "the people in the chat can be told what to do: {reply:?}\n{}",
        world.log()
    );
    assert!(!work.join("headless.txt").exists());
    // The folder allowed without asking still works with nobody there.
    let request = world
        .ask(
            &host,
            &run_and_report("ops", "echo again >> done.txt && cat done.txt"),
            None,
        )
        .await;
    world.settled(&host, &request).await;
    assert!(world.replies(&request)[0].contains("again"));
    host.shutdown().await;
}

/// The plan's M3 line on credentials, on bzz's side, with the real host and
/// a private remote that demands one (`git http-backend` behind a server
/// that answers 401): without a stored credential the clone fails with a
/// message that says what the owner can do; with one it works; and the
/// secret is in no log, no SQLite file and no activity line.
#[tokio::test]
async fn the_real_host_clones_a_private_remote_with_a_stored_credential_and_not_without() {
    use bzz::agents::git_credentials::{self, SealedGitCredentials};
    const USER: &str = "scout-bot";
    const TOKEN: &str = "ghp_NotARealTokenOnlyAMarkerForThisTest0000";
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    let world = World::new(&node).await;
    let Some(remote) = pi_host::git_remote(
        "agent_host_test",
        &node,
        &world.temporary.path().join("remote"),
        USER,
        TOKEN,
    ) else {
        return;
    };
    let owner = bzz::auth::signer::SignerHandle::spawn(world.owner.clone());
    let keys = world.data_dir().join("keys");
    let mut local = world.settings();
    local.network = vec![remote.host.clone()];
    let stored = |options: &mut HostOptions| {
        options.git_credentials = Some(Arc::new(SealedGitCredentials {
            keys_dir: keys.clone(),
            owner: owner.clone(),
        }));
    };
    let host = world.host_configured(local.clone(), stored).await;
    let clone = |into: &str| {
        format!(
            "do: {}",
            serde_json::json!([
                ["bash", {"command": format!("{{ git clone {} {into} && cat {into}/notes.txt; }} 2>&1; echo \"exit $?\"", remote.origin)}],
                ["report"],
            ])
        )
    };

    // Nothing stored: the clone fails, and the agent is told what its
    // owner can do.
    let request = world.ask(&host, &clone("first"), None).await;
    world.settled(&host, &request).await;
    let reply = world.replies(&request);
    assert_eq!(reply.len(), 1, "{reply:?}\n{}", world.log());
    assert!(
        reply[0].contains(&format!(
            "No credential is stored for {host}. Your owner can store one with `bzz agent credential set Scout {host}`.",
            host = remote.host
        )) && reply[0].contains("exit 128"),
        "a clear message without a credential: {reply:?}"
    );

    // Stored for another agent only: still none for this one.
    git_credentials::set(&keys, &owner, Uuid::new_v4(), &remote.host, USER, TOKEN)
        .await
        .unwrap();
    let request = world.ask(&host, &clone("second"), None).await;
    world.settled(&host, &request).await;
    assert!(
        world.replies(&request)[0].contains("No credential is stored for"),
        "another agent's credential is not this agent's"
    );

    // Stored for this agent: bzz answers at the moment git asks, with no
    // restart, and the clone works.
    git_credentials::set(&keys, &owner, world.agent, &remote.host, USER, TOKEN)
        .await
        .unwrap();
    let request = world.ask(&host, &clone("third"), None).await;
    world.settled(&host, &request).await;
    let reply = world.replies(&request);
    assert!(
        reply.len() == 1
            && reply[0].contains("Cloned into")
            && reply[0].contains("alpha\nbeta\ngamma")
            && reply[0].contains("exit 0"),
        "the clone works with the stored credential: {reply:?}\n{}",
        world.log()
    );
    assert_eq!(
        activity_of(&world),
        [
            format!("git credential for {} · none is stored", remote.host),
            format!("git credential for {} · none is stored", remote.host),
            format!("git credential for {} · given", remote.host),
        ]
    );
    host.shutdown().await;

    // A host that is no longer on the agent's list: the host itself refuses
    // before any request leaves, and bzz is not even asked.
    local.network.clear();
    let host = world.host_configured(local, stored).await;
    let request = world.ask(&host, &clone("fourth"), None).await;
    world.settled(&host, &request).await;
    assert!(
        world.replies(&request)[0].contains("allow-list"),
        "{:?}",
        world.replies(&request)
    );
    assert_eq!(activity_of(&world).len(), 3, "bzz was not asked");
    host.shutdown().await;

    // The secret scan: every file bzz and the host wrote, the database, both
    // SQLite files of the agent with what they had not yet merged, the
    // host's log and the agent's activity.
    let files = pi_host::files_under(&world.data_dir());
    let named = |end: &str| {
        files
            .iter()
            .any(|file| file.to_string_lossy().ends_with(end))
    };
    for expected in [
        "bzz.db",
        "memory.sqlite",
        "files.sqlite",
        "harness.log",
        "activity.jsonl",
        "started.json",
    ] {
        assert!(named(expected), "{expected} was searched: {files:?}");
    }
    for file in &files {
        for (form, secret) in pi_host::secret_forms(USER, TOKEN) {
            assert!(
                !pi_host::file_holds(file, &secret),
                "{} holds {form}",
                file.display()
            );
        }
    }
    // The search sees what is there: what the clone brought is on the
    // agent's disk, and what git printed is in its memory.
    let agent_folder = process::agent_dir(&world.data_dir(), world.agent);
    assert!(pi_host::file_holds(
        &agent_folder.join("files.sqlite"),
        "alpha\nbeta\ngamma"
    ));
    assert!(pi_host::file_holds(
        &agent_folder.join("memory.sqlite"),
        "Cloned into"
    ));
    // The stored credential is on disk only sealed.
    assert!(
        files
            .iter()
            .any(|file| file.to_string_lossy().contains(".git/"))
    );
    owner.lock().await;
}

/// The plan's M2 line for `agent new`: when the chosen model's provider is
/// missing, bzz says whether a Pi package of Pi's settings names it without
/// a Pi Durable entry. The host reports it, through its own entry for this
/// question (`main.mjs --check-model`), with the packages loaded as an
/// agent's start would load them, or left alone when they are off.
#[tokio::test]
async fn agent_new_says_which_pi_package_names_a_missing_provider() {
    use bzz::agents::manage::{self, NewAgent};
    let Some(node) = pi_host::node_or_skip("agent_host_test").await else {
        return;
    };
    let world = World::new(&node).await;
    // Pi's settings name two packages: one written for the `pi` command
    // only, which registers a provider there, and one for this host too,
    // which brings a provider of its own.
    let pi = world.pi_dir();
    let package = |name: &str, manifest: serde_json::Value, file: &str, text: &str| {
        let folder = world.temporary.path().join("packages").join(name);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("package.json"), manifest.to_string()).unwrap();
        std::fs::write(folder.join(file), text).unwrap();
        folder
    };
    let pi_only = package(
        "pi-acme-cloud",
        serde_json::json!({"name": "pi-acme-cloud", "type": "module", "pi": {"extensions": ["./src/index.ts"]}}),
        "src/index.ts",
        "export default (pi) => { pi.registerProvider(\"acme-cloud\", { baseUrl: \"https://llm.example.invalid/v1\", models: [] }); };\n",
    );
    let durable = package(
        "acme",
        serde_json::json!({"name": "acme", "type": "module", "pi": {"durable": ["./src/durable.mjs"]}}),
        "src/durable.mjs",
        "import { fauxProvider } from \"@earendil-works/pi-ai/providers/faux\";\n\
         const faux = fauxProvider({ provider: \"acme\", models: [{ id: \"acme-1\" }] });\n\
         export default async () => ({ providers: [faux.provider] });\n",
    );
    std::fs::create_dir_all(&pi).unwrap();
    std::fs::write(
        pi.join("settings.json"),
        serde_json::json!({"packages": [pi_only, durable]}).to_string(),
    )
    .unwrap();
    let environment: bzz::agents::runtime::agent::Environment =
        Arc::new(pi_host::environment(&world.node, &pi));
    let check = async |model: &str, packages: bool| {
        let mut new = NewAgent::named("Scout");
        new.model = manage::parse_model(model, None).unwrap();
        new.packages = packages;
        manage::preflight(&world.data_dir(), &new, Some(&pi), &environment, &|_| {})
            .await
            .map(|preflight| preflight.notes.join("\n"))
            .map_err(|error| error.to_string())
    };

    // The provider a package names without an entry for this host.
    let missing = check("acme-cloud/big", true).await.unwrap_err();
    for said in [
        "the model acme-cloud/big cannot be used (provider_unavailable)",
        &format!(
            "bzz found the Pi package {} in Pi's settings",
            pi_only.display()
        ),
        "names this provider but has no Pi Durable entry",
        "the `pi` command can use it, bzz's host cannot",
    ] {
        assert!(missing.contains(said), "`{said}` in: {missing}");
    }
    // A provider nothing names.
    let nobody = check("nobody-has-this/big", true).await.unwrap_err();
    assert!(
        nobody.contains("bzz found no Pi package in Pi's settings that names this provider"),
        "{nobody}"
    );
    // A provider that a package with an entry brings: it can be used, which
    // the check did not know before it loaded the packages.
    let found = check("acme/acme-1", true).await.unwrap();
    assert!(found.contains("model acme/acme-1"), "{found}");
    // With packages off for the agent, no package is looked at, and bzz
    // says so.
    let off = check("acme/acme-1", false).await.unwrap_err();
    assert!(
        off.contains("(provider_unavailable)")
            && off.contains("Pi packages are switched off for this agent"),
        "{off}"
    );
    assert!(
        !off.contains("bzz found the Pi package"),
        "nothing was looked for: {off}"
    );
    // The stand-in model, whatever the packages.
    assert!(
        check("scripted/scripted", false)
            .await
            .unwrap()
            .contains("model scripted/scripted")
    );
    // What the answers give the agent on this machine is said before the
    // owner confirms: a folder for commands, with whether he is asked, and
    // Pi packages.
    let mut hands = NewAgent::named("Scout");
    hands.folders = vec![bzz::config::FolderGrant {
        name: "work".into(),
        path: world.temporary.path().to_path_buf(),
        write: false,
        exec: bzz::config::ExecGrant::Ask,
    }];
    let said = manage::preflight(&world.data_dir(), &hands, Some(&pi), &environment, &|_| {})
        .await
        .unwrap()
        .notes
        .join("\n");
    for expected in [
        "In /mnt/work each command waits for your yes in the TUI.",
        "The folder is where it starts, not a wall",
        "Pi packages: on. A package named in Pi's settings runs inside its host, with your rights",
    ] {
        assert!(said.contains(expected), "`{expected}` in:\n{said}");
    }
}
