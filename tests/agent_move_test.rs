//! v0.15 M5: moving an agent, against a recorded relay and two data folders
//! that stand for two machines.
//!
//! All tests but the last use a scripted harness
//! (`examples/scripted_harness.rs`) in the place of bzz's own host and need
//! no Node, model or network. The last one runs the real Pi Durable host on
//! its stand-in model: it needs Node.js 22.19 or newer and, the first time,
//! the npm registry, and says so when it skips.
//!
//! The same journey on the pinned relay, with the `bzz` binary and Buzz's
//! own `buzz-acp`, is `real_relay_agent_moves_to_another_bzz_and_to_buzz_acp`
//! in `tests/relay_integration.rs`.
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
        bundle::Kdf,
        launch_kit,
        moving::{self, Mover},
        runtime::{
            Limits,
            admission::{ChannelFacts, PlaceKind},
            agent::AgentSpec,
            hold::{self, Destination, Hold},
            host::{AgentHost, HostOptions, Offer},
            process,
            settle::{HostAccess, SettleLimits},
        },
    },
    auth::signer::SignerHandle,
    broker::custody,
    config::{
        AgentModelConfig, Config, FolderGrant, Harness, HostedAgentConfig, IdentityConfig,
        KeyBackend, LocalAgentConfig, LocalAgentState, WakeRule,
    },
    paths::Paths,
    store::{
        Store,
        agent_runtime::{NewRequest, RequestState},
        writer::StoreHandle,
    },
};
use nostr::{Event, JsonUtil as _, Keys, ToBech32 as _};
use secrecy::SecretString;
use serde_json::{Value, json};
use support::{
    agent_harness::{RecordedRelay, Script, kill, running, until},
    pi_host,
};
use tempfile::TempDir;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);
const RELAY: &str = "wss://relay.example/";
const PASSPHRASE: &str = "a disposable passphrase";
/// Argon2id at its cheapest: these tests are not about its cost.
const QUICK: Kdf = Kdf {
    memory_kib: 8,
    iterations: 1,
    parallelism: 1,
};

fn passphrase() -> SecretString {
    SecretString::from(PASSPHRASE.to_owned())
}

fn limits() -> Limits {
    Limits {
        start: Duration::from_secs(5),
        session: Duration::from_secs(5),
        turn: Duration::from_secs(15),
        cancel_grace: Duration::from_millis(400),
        stop_grace: Duration::from_millis(400),
        typing_interval: Duration::from_millis(50),
        restart_backoff: Duration::from_millis(30),
        publish: Duration::from_secs(3),
        checkpoint_idle: Duration::from_secs(600),
        ..Limits::default()
    }
}

/// One bzz: its settings, its data folder, its database, and its owner's
/// identity unlocked. Two of them with the same owner stand for the owner's
/// two machines.
struct Home {
    temporary: TempDir,
    paths: Paths,
    config: Config,
    store: StoreHandle,
    owner: SignerHandle,
    script: Script,
}

impl Home {
    fn new(owner: &Keys, relay: &str) -> Self {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let mut config = Config::default();
        let identity = Uuid::new_v4();
        config.identities.push(IdentityConfig {
            id: identity,
            label: "owner".into(),
            pubkey: owner.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "owner.key".into(),
        });
        config
            .add_community("c1".into(), relay.into(), identity, false)
            .unwrap();
        config.save(&paths).unwrap();
        let store = StoreHandle::spawn(Store::open(paths.database_file()).unwrap()).unwrap();
        Self {
            script: Script::new(&temporary.path().join("harness")),
            temporary,
            paths,
            config,
            store,
            owner: SignerHandle::spawn(owner.clone()),
        }
    }

    /// An agent as `bzz agent new` leaves one, with a memory and a disk, and
    /// with everything an owner can set, set.
    async fn agent(&mut self, name: &str) -> HostedAgentConfig {
        let community = self.config.communities[0].id;
        let identity = self.config.identities[0].id;
        let (mut agent, _) = custody::create(
            &self.paths.keys_dir(),
            &self.owner,
            identity,
            community,
            name,
        )
        .await
        .unwrap();
        agent.allowed.clear();
        let site = self.temporary.path().join("site");
        std::fs::create_dir_all(&site).unwrap();
        agent.local = Some(LocalAgentConfig {
            wake: WakeRule::Allowlist,
            wake_allowlist: vec![Keys::generate().public_key().to_hex()],
            model: Some(AgentModelConfig {
                provider: "scripted".into(),
                model_id: "scripted".into(),
                thinking_level: Some("low".into()),
            }),
            instructions: "Be brief.".into(),
            folders: vec![FolderGrant {
                name: "site".into(),
                path: site,
                write: true,
                exec: bzz::config::ExecGrant::Ask,
            }],
            network: vec!["git.example.org:8443".into(), "github.com".into()],
            command: self.script.command(),
            picture: Some("https://example.org/scout.png".into()),
            packages: false,
            ..LocalAgentConfig::new(Harness::PiDurable)
        });
        let folder = self.folder(agent.id);
        process::create_private_dir(&folder).unwrap();
        std::fs::write(folder.join("memory.sqlite"), memory_of(name)).unwrap();
        std::fs::write(folder.join("files.sqlite"), disk_of(name)).unwrap();
        self.config.hosted_agents.push(agent.clone());
        self.config.save(&self.paths).unwrap();
        // The scripted harness answers as bzz's own host does, also to the
        // checkpoint a move asks for.
        self.script.write(json!({"own_host": true}), json!([]));
        agent
    }

    fn folder(&self, agent: Uuid) -> PathBuf {
        process::agent_dir(&self.paths.data_dir, agent)
    }

    /// The settings as another bzz process would read them now.
    fn on_disk(&self) -> Config {
        Config::load(&self.paths).unwrap()
    }

    fn state_on_disk(&self, agent: Uuid) -> Option<LocalAgentState> {
        self.on_disk()
            .hosted_agents
            .iter()
            .find(|entry| entry.id == agent)
            .and_then(|entry| entry.local.as_ref().map(|local| local.state))
    }

    /// The agent's key, read out of this bzz's custody.
    async fn key_of(&self, agent: &HostedAgentConfig) -> Keys {
        let sealed = std::fs::read_to_string(
            self.paths
                .keys_dir()
                .join(format!("agent-{}.nip44", agent.id)),
        )
        .unwrap();
        Keys::parse(&self.owner.decrypt_self(sealed).await.unwrap()).unwrap()
    }
}

/// What a move needs of a home, with the scripted harness as the agent's
/// host. A macro, so that the home's settings can be borrowed beside it.
macro_rules! mover {
    ($home:expr) => {
        mover_of(&$home.paths, &$home.store, &$home.owner, &$home.script)
    };
}

fn mover_of<'a>(
    paths: &'a Paths,
    store: &'a StoreHandle,
    owner: &'a SignerHandle,
    script: &Script,
) -> Mover<'a> {
    Mover {
        kdf: QUICK,
        host: scripted_host(script),
        release_limit: Duration::from_secs(10),
        ..Mover::new(paths, store, owner)
    }
}

/// The scripted harness in the place of the agent's host.
fn scripted_host(script: &Script) -> HostAccess {
    HostAccess {
        command: Some(script.command()),
        pi_dir: Some("/nonexistent/pi/agent".into()),
        limits: SettleLimits {
            start: Duration::from_secs(10),
            checkpoint: Duration::from_secs(10),
            stop_grace: Duration::from_millis(400),
        },
        ..HostAccess::of_this_process()
    }
}

fn memory_of(name: &str) -> Vec<u8> {
    format!("SQLite format 3\0 the memory of {name} ")
        .into_bytes()
        .repeat(700)
}

fn disk_of(name: &str) -> Vec<u8> {
    format!("SQLite format 3\0 the disk of {name} ")
        .into_bytes()
        .repeat(300)
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Every file below `root`, with its bytes.
fn files_below(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                found.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

fn holds(haystack: &[u8], needle: &str) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
}

/// The forms an agent's key is written in.
fn forms_of(keys: &Keys) -> Vec<(&'static str, String)> {
    vec![
        ("hex", keys.secret_key().to_secret_hex()),
        ("nsec", keys.secret_key().to_bech32().unwrap()),
    ]
}

#[tokio::test]
async fn an_agent_moves_to_another_bzz_with_its_key_its_files_and_its_settings() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    let settings = agent.local.clone().unwrap();
    let key = here.key_of(&agent).await;
    let file = here.temporary.path().join("scout.bzzagent");

    let moved = moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap();
    assert_eq!((moved.name.as_str(), &moved.file), ("Scout", &file));
    assert_eq!(moved.bytes, std::fs::metadata(&file).unwrap().len());
    assert_eq!(mode(&file), 0o600);
    // The host was asked to make the files whole before anything was read.
    assert_eq!(here.script.received("_bzz/checkpoint").len(), 1);
    let asked = &here.script.received("initialize")[0]["_meta"]["bzz"];
    assert_eq!(
        (&asked["mounts"], &asked["network"], &asked["packages"]),
        (&json!([]), &json!([]), &json!(false)),
        "for the checkpoint the host gets no folder, no host and no package"
    );

    // Here: moved away, in the settings as they are in memory and on disk,
    // and in the agent's own folder, which a running bzz reads.
    assert_eq!(
        here.config.hosted_agents[0].local.as_ref().unwrap().state,
        LocalAgentState::MovedAway
    );
    assert_eq!(
        here.state_on_disk(agent.id),
        Some(LocalAgentState::MovedAway)
    );
    assert!(matches!(
        hold::read(&here.paths.data_dir, agent.id),
        Some(Hold::MovedAway {
            at: Some(_),
            to: Some(Destination::File)
        })
    ));
    assert!(
        moving::moved_line(&here.paths.data_dir, agent.id)
            .unwrap()
            .contains("in a sealed file, for another bzz")
    );
    // A second move of what is not here is refused.
    let again = moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &here.temporary.path().join("again.bzzagent"),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(again.contains("was already moved away"), "{again}");

    // The file: sealed. Nothing of the agent is readable in it.
    let sealed = std::fs::read(&file).unwrap();
    for (form, secret) in forms_of(&key) {
        assert!(!holds(&sealed, &secret), "the key ({form}) is in the file");
    }
    for plain in [
        "Scout",
        "Be brief.",
        agent.pubkey.as_str(),
        "git.example.org",
        "the memory of Scout",
        "the disk of Scout",
        "memory.sqlite",
    ] {
        assert!(!holds(&sealed, plain), "{plain:?} is readable in the file");
    }

    // Another bzz of the same owner, in the same community.
    let mut there = Home::new(&owner, RELAY);
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let carried = arrival.carried.clone();
    assert_eq!(
        (
            carried.name.as_str(),
            carried.pubkey.as_str(),
            carried.relay.as_str(),
            carried.harness,
            carried.packages,
        ),
        (
            "Scout",
            agent.pubkey.as_str(),
            RELAY,
            Harness::PiDurable,
            false
        )
    );
    assert_eq!(carried.owner().unwrap(), owner.public_key().to_hex());
    let received = moving::receive(
        &there.paths,
        &there.store,
        &there.owner,
        &mut there.config,
        arrival,
        &scripted_host(&there.script),
    )
    .await
    .unwrap();
    assert_eq!(
        (received.id, received.name.as_str(), received.taken_back),
        (agent.id, "Scout", false)
    );
    let arrived = there.on_disk().hosted_agents;
    assert_eq!(arrived.len(), 1);
    let arrived = arrived[0].clone();
    assert_eq!(
        (
            arrived.id,
            arrived.label.as_str(),
            &arrived.pubkey,
            &arrived.auth_tag,
            arrived.community,
            arrived.owner_identity,
            arrived.paused,
        ),
        (
            agent.id,
            "Scout",
            &agent.pubkey,
            &agent.auth_tag,
            there.config.communities[0].id,
            there.config.identities[0].id,
            false,
        ),
        "it belongs to this bzz's own community and identity"
    );
    assert!(arrived.allowed.is_empty(), "the broker serves it nothing");
    // Its settings, but for what belongs to the machine it left: no folder
    // is granted, and the launch is the default one.
    assert_eq!(
        arrived.local.clone().unwrap(),
        LocalAgentConfig {
            folders: Vec::new(),
            command: Vec::new(),
            state: LocalAgentState::Active,
            ..settings.clone()
        }
    );
    assert!(arrived.runs_locally());
    // Its key is in this bzz's custody, sealed: the same key.
    assert_eq!(
        there.key_of(&arrived).await.secret_key().to_secret_hex(),
        key.secret_key().to_secret_hex()
    );
    assert_eq!(
        custody::unlock(&there.paths.keys_dir(), &there.owner, &arrived)
            .await
            .unwrap()
            .public_key()
            .to_hex(),
        agent.pubkey
    );
    // Its two files, byte for byte, private, and nothing else.
    let folder = there.folder(agent.id);
    assert_eq!(
        std::fs::read(folder.join("memory.sqlite")).unwrap(),
        memory_of("Scout")
    );
    assert_eq!(
        std::fs::read(folder.join("files.sqlite")).unwrap(),
        disk_of("Scout")
    );
    assert_eq!(mode(&folder.join("memory.sqlite")), 0o600);
    assert_eq!(mode(&folder.join("files.sqlite")), 0o600);
    assert_eq!(mode(&folder), 0o700);
    let mut names = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["files.sqlite", "memory.sqlite"]);
    assert_eq!(hold::read(&there.paths.data_dir, agent.id), None);

    // The same file a second time does not make a second agent.
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let again = moving::receive(
        &there.paths,
        &there.store,
        &there.owner,
        &mut there.config,
        arrival,
        &scripted_host(&there.script),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        again.contains("is already here and was not moved away"),
        "{again}"
    );
    assert_eq!(there.config.hosted_agents.len(), 1);
    assert_eq!(there.on_disk().hosted_agents, vec![arrived]);
}

#[tokio::test]
async fn a_file_that_is_not_whole_or_not_the_owners_is_refused_and_leaves_nothing() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    let file = here.temporary.path().join("scout.bzzagent");
    moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap();
    let whole = std::fs::read(&file).unwrap();
    let nothing_of_it = |home: &Home| {
        (
            home.on_disk().hosted_agents.len(),
            std::fs::read_dir(home.paths.keys_dir()).unwrap().count(),
            home.paths.data_dir.join("agents").exists(),
        )
    };

    // Changed, cut short, the wrong passphrase: each named, each refused
    // before anything is used.
    let broken = here.temporary.path().join("broken.bzzagent");
    let refusal = |bytes: Vec<u8>, with: &'static str| {
        std::fs::write(&broken, bytes).unwrap();
        let broken = broken.clone();
        async move {
            moving::open_file(&broken, &SecretString::from(with.to_owned()))
                .await
                .unwrap_err()
                .to_string()
        }
    };
    let mut changed = whole.clone();
    changed[whole.len() / 2] ^= 0x01;
    let said = refusal(changed, PASSPHRASE).await;
    assert!(said.contains("was changed after bzz made it"), "{said}");
    let said = refusal(whole[..whole.len() - 1].to_vec(), PASSPHRASE).await;
    assert!(said.contains("was cut short"), "{said}");
    let said = refusal(whole.clone(), "not the passphrase at all").await;
    assert!(said.contains("the passphrase is wrong"), "{said}");
    let said = refusal(b"just some text".to_vec(), PASSPHRASE).await;
    assert!(
        said.contains("not a file made by `bzz agent move`"),
        "{said}"
    );

    // Another owner's bzz, in the same community: refused, and told whose
    // it is. Nothing of the agent is made there.
    let stranger_keys = Keys::generate();
    let mut stranger = Home::new(&stranger_keys, RELAY);
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let said = moving::plan(&stranger.config, &arrival.carried)
        .unwrap_err()
        .to_string();
    assert!(
        said.contains(&format!(
            "owned by {}",
            owner.public_key().to_bech32().unwrap()
        )) && said.contains("An agent is received only by its owner"),
        "{said}"
    );
    let said = moving::receive(
        &stranger.paths,
        &stranger.store,
        &stranger.owner,
        &mut stranger.config,
        arrival,
        &scripted_host(&stranger.script),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("received only by its owner"), "{said}");
    assert_eq!(nothing_of_it(&stranger), (0, 0, false));

    // The owner's bzz without that community: refused, with what to do.
    let mut elsewhere = Home::new(&owner, "wss://other.example/");
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let said = moving::receive(
        &elsewhere.paths,
        &elsewhere.store,
        &elsewhere.owner,
        &mut elsewhere.config,
        arrival,
        &scripted_host(&elsewhere.script),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        said.contains("belongs to the community at wss://relay.example/")
            && said.contains("bzz community add"),
        "{said}"
    );
    assert_eq!(nothing_of_it(&elsewhere), (0, 0, false));

    // The owner's bzz, where another agent has that name: refused.
    let mut taken = Home::new(&owner, RELAY);
    let namesake = taken.agent("scout").await;
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let said = moving::receive(
        &taken.paths,
        &taken.store,
        &taken.owner,
        &mut taken.config,
        arrival,
        &scripted_host(&taken.script),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        said.contains("another agent here is called Scout"),
        "{said}"
    );
    assert_eq!(taken.on_disk().hosted_agents, vec![namesake]);

    // A receive that fails at its last step leaves nothing of the agent.
    let mut failing = Home::new(&owner, RELAY);
    std::fs::create_dir(failing.paths.config_file().with_extension("toml.tmp")).unwrap();
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    assert!(
        moving::receive(
            &failing.paths,
            &failing.store,
            &failing.owner,
            &mut failing.config,
            arrival,
            &scripted_host(&failing.script),
        )
        .await
        .is_err()
    );
    assert!(failing.config.hosted_agents.is_empty());
    assert_eq!(
        (
            failing.on_disk().hosted_agents.len(),
            std::fs::read_dir(failing.paths.keys_dir()).unwrap().count(),
            failing.folder(agent.id).exists(),
        ),
        (0, 0, false),
        "no settings, no key, no folder"
    );
}

/// What a running bzz does with an agent: the runtime, a recorded relay, and
/// the scripted harness as the agent's process.
struct Running {
    host: AgentHost,
    relay: Arc<RecordedRelay>,
    owner: Keys,
    agent: HostedAgentConfig,
    channel: Uuid,
    /// How long an answer may take: the real host is installed on first use.
    wait: Duration,
}

impl Running {
    async fn start(home: &Home, owner: &Keys, agent: &HostedAgentConfig) -> Self {
        let relay = RecordedRelay::new(
            home.key_of(agent).await,
            bzz::broker::relay::auth_tag(&agent.auth_tag).unwrap(),
        );
        let channel = Uuid::new_v4();
        relay.set_channel(ChannelFacts {
            id: channel,
            name: "general".into(),
            kind: PlaceKind::Channel,
            members: vec![owner.public_key().to_hex(), agent.pubkey.clone()],
        });
        Self::start_on(home, owner, agent, relay, channel, None).await
    }

    /// The same on a relay and a channel that exist: another bzz of the
    /// owner's sees the same relay. `real_host` is the Node and Pi's folder
    /// of a test that runs bzz's real host.
    async fn start_on(
        home: &Home,
        owner: &Keys,
        agent: &HostedAgentConfig,
        relay: Arc<RecordedRelay>,
        channel: Uuid,
        real_host: Option<(&Path, &Path)>,
    ) -> Self {
        let mut options = HostOptions::new(home.paths.data_dir.clone(), home.store.clone());
        options.limits = limits();
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        options.pi_dir = Some("/nonexistent/pi/agent".into());
        if let Some((node, pi_dir)) = real_host {
            options.limits = Limits {
                stop_grace: Duration::from_millis(500),
                restart_backoff: Duration::from_millis(100),
                typing_interval: Duration::from_millis(200),
                ..Limits::default()
            };
            options.pi_dir = Some(pi_dir.to_path_buf());
            options.environment = Arc::new(pi_host::environment(node, pi_dir));
        }
        let host = AgentHost::start(options).await.unwrap();
        assert!(host.runnable(agent));
        host.add(
            AgentSpec {
                id: agent.id,
                name: agent.label.clone(),
                pubkey: agent.pubkey.clone(),
                owner: owner.public_key().to_hex(),
                community: agent.community,
                local: LocalAgentConfig {
                    wake: WakeRule::OwnerOnly,
                    wake_allowlist: Vec::new(),
                    ..agent.local.clone().unwrap()
                },
            },
            relay.clone(),
        )
        .await;
        Self {
            host,
            relay,
            owner: owner.clone(),
            agent: agent.clone(),
            channel,
            wait: if real_host.is_some() {
                Duration::from_secs(120)
            } else {
                WAIT
            },
        }
    }

    fn mention(&self, text: &str) -> Event {
        self.mention_in(text, None)
    }

    /// A mention by the owner: top level, or in the thread `root` started.
    fn mention_in(&self, text: &str, root: Option<&Event>) -> Event {
        let thread = root.map(|root| buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        });
        let event = buzz_sdk::build_message(
            self.channel,
            text,
            thread.as_ref(),
            &[self.agent.pubkey.as_str()],
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

    async fn offer(&self, event: &Event) -> Offer {
        let facts =
            bzz::agents::runtime::relay::AgentRelay::channel(self.relay.as_ref(), self.channel)
                .await
                .unwrap();
        self.host.offer(self.agent.id, event, &facts).await
    }

    /// Asks, and returns the one reply.
    async fn answered(&self, home: &Home, text: &str) -> String {
        self.answered_in(home, text, None).await.1
    }

    /// Asks at the top level or in a thread, and returns the request with
    /// its one reply.
    async fn answered_in(&self, home: &Home, text: &str, root: Option<&Event>) -> (Event, String) {
        // After a move that failed the agent listens again within a moment.
        let event = self.mention_in(text, root);
        let deadline = tokio::time::Instant::now() + self.wait;
        while self.offer(&event).await != Offer::Queued {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the agent does not listen: {:?}",
                self.line()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let (agent, id) = (self.agent.id, event.id.to_hex());
        until("the request to be answered", self.wait, || {
            let replies = self.relay.replies_to(&event);
            (!replies.is_empty()).then_some(replies)
        })
        .await;
        let state = home
            .store
            .call(move |store| store.agent_inbox_get(agent, &id))
            .await
            .unwrap()
            .map(|row| row.state);
        assert_eq!(state, Some(RequestState::Replied));
        let replies = self.relay.replies_to(&event);
        assert_eq!(replies.len(), 1, "one reply per mention");
        let reply = replies[0].content.clone();
        (event, reply)
    }

    fn line(&self) -> String {
        self.host
            .status()
            .iter()
            .find(|status| status.id == self.agent.id)
            .map(|status| status.line())
            .unwrap_or_default()
    }

    fn pid(&self) -> Option<u32> {
        self.host
            .status()
            .iter()
            .find(|status| status.id == self.agent.id)
            .and_then(|status| status.pid)
            .filter(|pid| running(*pid))
    }
}

fn send(text: &str) -> Value {
    json!({"steps": [{"tool": "send", "arguments": {"text": text, "final": true}}]})
}

#[tokio::test]
async fn a_move_that_fails_before_its_last_step_leaves_the_agent_running_as_it_was() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    here.script.write(
        json!({"own_host": true}),
        json!([
            send("one"),
            send("two"),
            send("three"),
            send("four"),
            send("five")
        ]),
    );
    let live = Running::start(&here, &owner, &agent).await;
    assert_eq!(live.answered(&here, "hello").await, "one");
    let first_pid = live.pid().expect("its process is running");
    let file = here.temporary.path().join("scout.bzzagent");
    // Nothing of an attempt is left: no file, no partial file, the agent
    // active in memory and on disk, and nothing holding it.
    let as_it_was = |home: &Home, what: &str| {
        let beside = std::fs::read_dir(home.temporary.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("bzzagent"))
            .collect::<Vec<_>>();
        assert!(beside.is_empty(), "{what}: {beside:?}");
        assert_eq!(
            home.config.hosted_agents[0].local.as_ref().unwrap().state,
            LocalAgentState::Active,
            "{what}"
        );
        assert_eq!(
            home.state_on_disk(agent.id),
            Some(LocalAgentState::Active),
            "{what}"
        );
        assert_eq!(hold::read(&home.paths.data_dir, agent.id), None, "{what}");
    };

    // Refused before anything is touched: the agent's process is not even
    // stopped.
    let weak = moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &SecretString::from("too short".to_owned()),
        &file,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(weak.contains("at least 12 characters"), "{weak}");
    std::fs::write(&file, b"mine").unwrap();
    let taken = moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(taken.contains("bzz does not write over it"), "{taken}");
    assert_eq!(std::fs::read(&file).unwrap(), b"mine");
    std::fs::remove_file(&file).unwrap();
    assert_eq!(live.pid(), Some(first_pid), "not stopped for a refusal");
    as_it_was(&here, "refused early");

    // Its files cannot be made whole: the host does not start.
    let mut mover = mover!(here);
    mover.host.command = Some(vec!["/bin/sh".into(), "-c".into(), "exit 3".into()]);
    let failed = moving::move_to_file(&mover, &mut here.config, agent.id, &passphrase(), &file)
        .await
        .unwrap_err()
        .to_string();
    drop(mover);
    assert!(
        failed.contains("could not be made ready for a move"),
        "{failed}"
    );
    as_it_was(&here, "the files could not be made whole");
    // The bzz that ran it had stopped it for the move, and runs it again.
    assert!(!running(first_pid), "it was stopped for the move");
    assert_eq!(live.answered(&here, "still there?").await, "two");
    let second_pid = live.pid().expect("it runs again");
    assert_ne!(second_pid, first_pid);

    // The file cannot be written.
    let unwritable = here
        .temporary
        .path()
        .join(format!("{}.bzzagent", "n".repeat(300)));
    assert!(
        moving::move_to_file(
            &mover!(here),
            &mut here.config,
            agent.id,
            &passphrase(),
            &unwritable,
        )
        .await
        .is_err()
    );
    as_it_was(&here, "the file could not be written");
    assert_eq!(live.answered(&here, "and now?").await, "three");

    // The last step itself cannot be written down: the move did not happen,
    // and the sealed copy that was made is removed.
    let blocked = here.paths.config_file().with_extension("toml.tmp");
    std::fs::create_dir(&blocked).unwrap();
    assert!(
        moving::move_to_file(
            &mover!(here),
            &mut here.config,
            agent.id,
            &passphrase(),
            &file,
        )
        .await
        .is_err()
    );
    std::fs::remove_dir(&blocked).unwrap();
    as_it_was(&here, "the settings could not be saved");
    assert_eq!(live.answered(&here, "once more?").await, "four");
    let before_the_move = live.pid().expect("it runs");

    // And the move that succeeds: the bzz that runs the agent stops it, and
    // never starts it again, although its own copy of the settings still
    // says "active".
    moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap();
    assert!(!running(before_the_move), "stopped for the move");
    assert!(
        !live.relay.closed(),
        "it kept its connection while the move could fail"
    );
    until("the running bzz to see the move", WAIT, || {
        live.line().contains("stopped · moved away").then_some(())
    })
    .await;
    // It is another home's agent now: the bzz it left keeps no connection
    // as the agent, so the relay does not count the agent present for it.
    until("the agent's connection to be closed", WAIT, || {
        live.relay.closed().then_some(())
    })
    .await;
    let starts = here.script.starts().len();
    let late = live.mention("anyone?");
    assert_eq!(live.offer(&late).await, Offer::NotRunning);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(here.script.starts().len(), starts, "nothing was started");
    assert!(live.relay.replies_to(&late).is_empty());
    assert_eq!(live.pid(), None);
    assert!(
        process::read_record(&process::run_dir(&here.paths.data_dir), agent.id).is_none(),
        "no process of it is recorded"
    );
    live.host.shutdown().await;

    // A bzz that starts afterwards with an older copy of the settings does
    // not run it either, and says why. The settings are then corrected.
    let mut stale = here.on_disk();
    stale.hosted_agents[0].local.as_mut().unwrap().state = LocalAgentState::Active;
    let mut options = HostOptions::new(here.paths.data_dir.clone(), here.store.clone());
    options.limits = limits();
    let host = AgentHost::start(options).await.unwrap();
    assert!(!host.runnable(&stale.hosted_agents[0]));
    let line = host.status()[0].line();
    assert!(
        line.contains("stopped · moved away: it runs elsewhere"),
        "{line}"
    );
    host.shutdown().await;
    assert_eq!(moving::reconcile(&here.paths, &mut stale), ["Scout"]);
    assert_eq!(
        stale.hosted_agents[0].local.as_ref().unwrap().state,
        LocalAgentState::MovedAway
    );
    assert!(moving::reconcile(&here.paths, &mut stale).is_empty());
}

#[tokio::test]
async fn an_agent_that_is_answering_is_not_moved_and_its_turn_is_not_cut() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    let gate = here.script.marker("gate");
    here.script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            {"touch": here.script.marker("live")},
            {"wait_for": gate},
            {"tool": "send", "arguments": {"text": "the whole answer", "final": true}},
        ]}]),
    );
    let live = Running::start(&here, &owner, &agent).await;
    let question = live.mention("a long question");
    assert_eq!(live.offer(&question).await, Offer::Queued);
    until("the turn to be running", WAIT, || {
        here.script.marker("live").exists().then_some(())
    })
    .await;
    let pid = live.pid().expect("its process is running");
    let file = here.temporary.path().join("scout.bzzagent");
    let refused = moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        refused.contains("has 1 request(s) it has not finished answering"),
        "{refused}"
    );
    assert!(!file.exists());
    assert_eq!(live.pid(), Some(pid), "its process was left alone");
    assert_eq!(hold::read(&here.paths.data_dir, agent.id), None);
    std::fs::write(&gate, b"").unwrap();
    until("the answer", WAIT, || {
        (live.relay.replies_to(&question).len() == 1).then_some(())
    })
    .await;
    assert_eq!(
        live.relay.replies_to(&question)[0].content,
        "the whole answer"
    );
    live.host.shutdown().await;

    // A request that was never handed over holds a move back too.
    let waiting = live.mention("never handed over");
    let request = NewRequest {
        agent_id: agent.id,
        request_id: waiting.id.to_hex(),
        community_id: agent.community,
        channel_id: live.channel,
        place: format!("{}/{}", live.channel, waiting.id.to_hex()),
        author: owner.public_key().to_hex(),
        created_at: waiting.created_at.as_secs(),
        event_json: waiting.as_json(),
    };
    here.store
        .call(move |store| store.agent_inbox_offer(&request))
        .await
        .unwrap();
    assert!(
        moving::move_to_file(
            &mover!(here),
            &mut here.config,
            agent.id,
            &passphrase(),
            &file,
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("has 1 request(s)")
    );
    assert_eq!(here.state_on_disk(agent.id), Some(LocalAgentState::Active));
}

#[tokio::test]
async fn a_bzz_takes_back_the_agent_it_moved_away() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    let settings = agent.local.clone().unwrap();
    let file = here.temporary.path().join("scout.bzzagent");
    moving::move_to_file(
        &mover!(here),
        &mut here.config,
        agent.id,
        &passphrase(),
        &file,
    )
    .await
    .unwrap();
    // What stays here is the copy from before the move: it is what the
    // file replaces.
    let folder = here.folder(agent.id);
    std::fs::write(folder.join("memory.sqlite"), b"a stale copy").unwrap();
    std::fs::write(folder.join("memory.sqlite-wal"), b"and its leftovers").unwrap();
    // A cursor from when it ran here.
    let id = agent.id;
    here.store
        .call(move |store| store.advance_agent_cursor(id, 1000))
        .await
        .unwrap();

    // A take-back that fails halfway leaves it moved away.
    let blocked = here.paths.config_file().with_extension("toml.tmp");
    std::fs::create_dir(&blocked).unwrap();
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    assert!(
        moving::receive(
            &here.paths,
            &here.store,
            &here.owner,
            &mut here.config,
            arrival,
            &scripted_host(&here.script),
        )
        .await
        .is_err()
    );
    std::fs::remove_dir(&blocked).unwrap();
    assert_eq!(
        here.state_on_disk(agent.id),
        Some(LocalAgentState::MovedAway)
    );
    assert!(hold::moved_away(&here.paths.data_dir, agent.id));
    assert_eq!(
        here.config.hosted_agents[0].local.as_ref().unwrap().state,
        LocalAgentState::MovedAway
    );

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    let received = moving::receive(
        &here.paths,
        &here.store,
        &here.owner,
        &mut here.config,
        arrival,
        &scripted_host(&here.script),
    )
    .await
    .unwrap();
    assert_eq!((received.id, received.taken_back), (agent.id, true));
    // Active again, with the file's memory and disk, and with what this
    // machine had granted it.
    let back = here.on_disk().hosted_agents;
    assert_eq!(back.len(), 1, "taken back, not received as a second agent");
    assert_eq!(
        back[0].local.clone().unwrap(),
        LocalAgentConfig {
            state: LocalAgentState::Active,
            ..settings
        },
        "its folders and its launch here are as they were"
    );
    assert!(back[0].runs_locally());
    assert_eq!(hold::read(&here.paths.data_dir, agent.id), None);
    assert_eq!(
        std::fs::read(folder.join("memory.sqlite")).unwrap(),
        memory_of("Scout")
    );
    assert_eq!(
        std::fs::read(folder.join("files.sqlite")).unwrap(),
        disk_of("Scout")
    );
    assert!(!folder.join("memory.sqlite-wal").exists());
    assert_eq!(
        custody::unlock(&here.paths.keys_dir(), &here.owner, &back[0])
            .await
            .unwrap()
            .public_key()
            .to_hex(),
        agent.pubkey
    );
    // What was said to it while it was away is not a request here.
    let cursor = here
        .store
        .call(move |store| store.agent_cursor(id))
        .await
        .unwrap()
        .unwrap();
    assert!(cursor >= before + 30, "{cursor} < {before} + 30");
    // And a bzz runs it again.
    let mut options = HostOptions::new(here.paths.data_dir.clone(), here.store.clone());
    options.limits = limits();
    let host = AgentHost::start(options).await.unwrap();
    assert!(host.runnable(&back[0]));
    host.shutdown().await;
}

#[tokio::test]
async fn a_launch_kit_holds_what_buzz_acp_needs_and_the_key_in_one_file_only() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Scout").await;
    let key = here.key_of(&agent).await;
    let kit = here.temporary.path().join("scout-kit");

    // Never into a folder that is there.
    std::fs::create_dir(&kit).unwrap();
    let refused = moving::launch_kit(&mover!(here), &mut here.config, agent.id, &kit)
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("only into a folder it makes"), "{refused}");
    std::fs::remove_dir(&kit).unwrap();
    // A kit whose last step fails is removed: the key is not left in the
    // clear for an agent that did not move.
    let blocked = here.paths.config_file().with_extension("toml.tmp");
    std::fs::create_dir(&blocked).unwrap();
    assert!(
        moving::launch_kit(&mover!(here), &mut here.config, agent.id, &kit)
            .await
            .is_err()
    );
    std::fs::remove_dir(&blocked).unwrap();
    assert!(
        !kit.exists(),
        "no kit is left of a move that did not happen"
    );
    assert_eq!(here.state_on_disk(agent.id), Some(LocalAgentState::Active));
    assert_eq!(hold::read(&here.paths.data_dir, agent.id), None);

    let made = moving::launch_kit(&mover!(here), &mut here.config, agent.id, &kit)
        .await
        .unwrap();
    assert_eq!(made.folder, kit);
    assert_eq!(made.key_file, kit.join("buzz-acp.env"));
    assert_eq!(
        here.state_on_disk(agent.id),
        Some(LocalAgentState::MovedAway),
        "moved away, as with `move`"
    );
    assert!(matches!(
        hold::read(&here.paths.data_dir, agent.id),
        Some(Hold::MovedAway {
            to: Some(Destination::LaunchKit),
            ..
        })
    ));

    // What the folder holds.
    let files = files_below(&kit);
    let names = files
        .iter()
        .map(|(path, _)| path.strip_prefix(&kit).unwrap().display().to_string())
        .collect::<Vec<_>>();
    for expected in [
        "README.md",
        "buzz-acp.env",
        "start.sh",
        "bzz-agent.service",
        "bzz-agent.json",
        "agent/memory.sqlite",
        "agent/files.sqlite",
        "agent/agent.json",
        "host/main.mjs",
        "host/package.json",
        "host/package-lock.json",
        "host/src/host.mjs",
        "host/src/remote.mjs",
        "host/src/run-child.mjs",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "{expected}: {names:?}"
        );
    }
    // The host's files are the ones inside this bzz, every one of them.
    for (path, content) in bzz::agents::runtime::install::HOST_FILES {
        assert_eq!(
            std::fs::read_to_string(kit.join("host").join(path)).unwrap(),
            *content,
            "{path}"
        );
    }
    assert_eq!(
        std::fs::read(kit.join("agent/memory.sqlite")).unwrap(),
        memory_of("Scout")
    );
    assert_eq!(
        std::fs::read(kit.join("agent/files.sqlite")).unwrap(),
        disk_of("Scout")
    );
    // Modes: the key's file and the agent's files are their owner's alone.
    assert_eq!(mode(&kit), 0o700);
    assert_eq!(mode(&kit.join("buzz-acp.env")), 0o600);
    assert_eq!(mode(&kit.join("bzz-agent.json")), 0o600);
    assert_eq!(mode(&kit.join("agent")), 0o700);
    assert_eq!(mode(&kit.join("agent/memory.sqlite")), 0o600);
    assert_eq!(mode(&kit.join("agent/files.sqlite")), 0o600);
    assert_eq!(mode(&kit.join("agent/agent.json")), 0o600);
    assert_eq!(mode(&kit.join("start.sh")) & 0o100, 0o100);

    // The key: in the environment file, in the clear, and in no other file.
    let environment = std::fs::read_to_string(kit.join("buzz-acp.env")).unwrap();
    let hex = key.secret_key().to_secret_hex();
    assert!(environment.contains(&format!("BUZZ_PRIVATE_KEY='{hex}'\n")));
    for (path, bytes) in &files {
        if path.file_name().unwrap() == "buzz-acp.env" {
            continue;
        }
        for (form, secret) in forms_of(&key) {
            assert!(
                !holds(bytes, &secret),
                "the key ({form}) is in {}",
                path.display()
            );
        }
    }
    // What buzz-acp reads: the owner's authorization, the relay, how to
    // start the host from the kit's own folder, and who may wake it.
    let allowed = agent.local.as_ref().unwrap().wake_allowlist.join(",");
    for line in [
        format!("BUZZ_AUTH_TAG='{}'", agent.auth_tag),
        "BUZZ_RELAY_URL='wss://relay.example'".to_owned(),
        "BUZZ_ACP_AGENT_COMMAND='node'".to_owned(),
        "BUZZ_ACP_AGENT_ARGS='host/main.mjs,agent'".to_owned(),
        "BUZZ_ACP_SESSION_POLICY='thread'".to_owned(),
        "BUZZ_ACP_MULTIPLE_EVENT_HANDLING='queue'".to_owned(),
        "BUZZ_ACP_NO_BASE_PROMPT='true'".to_owned(),
        "BUZZ_ACP_NO_MEMORY='true'".to_owned(),
        "BUZZ_ACP_RESPOND_TO='allowlist'".to_owned(),
        format!("BUZZ_ACP_RESPOND_TO_ALLOWLIST='{allowed}'"),
    ] {
        assert!(
            environment.lines().any(|written| written == line),
            "{line}\n{environment}"
        );
    }
    // A shell reads the file as it stands.
    let sourced = std::process::Command::new("sh")
        .arg("-c")
        .arg(". ./buzz-acp.env && printf '%s|%s' \"$BUZZ_PRIVATE_KEY\" \"$BUZZ_AUTH_TAG\"")
        .current_dir(&kit)
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&sourced.stdout),
        format!("{hex}|{}", agent.auth_tag)
    );
    // What the host reads there: its settings, and no folder of any machine.
    let agent_json: Value =
        serde_json::from_slice(&std::fs::read(kit.join("agent/agent.json")).unwrap()).unwrap();
    assert_eq!(
        agent_json,
        json!({
            "name": "Scout",
            "model": {"provider": "scripted", "modelId": "scripted", "thinkingLevel": "low"},
            "instructions": "Be brief.",
            "network": ["git.example.org:8443", "github.com"],
            "packages": false,
        })
    );
    let everything = files
        .iter()
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .collect::<String>();
    let granted = here.temporary.path().join("site").display().to_string();
    assert!(
        !everything.contains(&granted),
        "a folder of this machine is named in the kit"
    );
    // The steps on the other machine.
    let readme = std::fs::read_to_string(kit.join("README.md")).unwrap();
    for said in [
        "# Launch kit for the agent Scout",
        "`buzz-acp.env` holds the agent's key in the clear",
        "Node.js 22.19 or newer",
        "npm ci --ignore-scripts --omit=optional --omit=dev",
        &format!("git checkout {}", launch_kit::BUZZ_PIN),
        "cargo build --release -p buzz-acp -p buzz-cli",
        "`scripted/scripted`",
        "./start.sh",
        "`bzz-agent.service` is an example of a systemd user unit",
        "bzz agent shutdown Scout",
        "bzz agent receive <this folder>",
        "the folders you granted it",
    ] {
        assert!(readme.contains(said), "{said}\n{readme}");
    }
    assert!(
        !readme.contains('@'),
        "a place was left unfilled:\n{readme}"
    );
    let unit = std::fs::read_to_string(kit.join("bzz-agent.service")).unwrap();
    assert!(
        unit.contains("WorkingDirectory=%h/scout-kit")
            && unit.contains("EnvironmentFile=%h/scout-kit/buzz-acp.env")
            && unit.contains("Restart=on-failure"),
        "{unit}"
    );

    // The kit comes back: `receive` takes the agent in from the folder, in
    // another bzz of the owner's, with the key from the kit's one file.
    let mut there = Home::new(&owner, RELAY);
    there.script.write(json!({"own_host": true}), json!([]));
    // buzz-acp does not give the host time to close its files: a kit comes
    // back with write-ahead files beside them. The agent's host makes a copy
    // whole before anything is taken. The stand-in host here does nothing,
    // so the copy is not whole, and nothing is received.
    std::fs::write(kit.join("agent/memory.sqlite-wal"), b"not yet in the file").unwrap();
    let arrival = moving::open_kit(&kit).unwrap();
    let not_whole = moving::receive(
        &there.paths,
        &there.store,
        &there.owner,
        &mut there.config,
        arrival,
        &scripted_host(&there.script),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        not_whole.contains("a write-ahead file is still beside the agent's files"),
        "{not_whole}"
    );
    assert_eq!(
        there.script.received("_bzz/checkpoint").len(),
        1,
        "the host was asked to make them whole"
    );
    assert_eq!(
        (
            there.on_disk().hosted_agents.len(),
            std::fs::read_dir(there.paths.keys_dir()).unwrap().count(),
            there.folder(agent.id).exists(),
        ),
        (0, 0, false),
        "nothing of it arrived"
    );
    // The kit itself is not touched.
    assert_eq!(
        std::fs::read(kit.join("agent/memory.sqlite-wal")).unwrap(),
        b"not yet in the file"
    );
    assert_eq!(
        std::fs::read(kit.join("agent/memory.sqlite")).unwrap(),
        memory_of("Scout")
    );
    std::fs::remove_file(kit.join("agent/memory.sqlite-wal")).unwrap();
    let arrival = moving::open_kit(&kit).unwrap();
    let received = moving::receive(
        &there.paths,
        &there.store,
        &there.owner,
        &mut there.config,
        arrival,
        &scripted_host(&there.script),
    )
    .await
    .unwrap();
    assert_eq!((received.id, received.taken_back), (agent.id, false));
    let arrived = there.on_disk().hosted_agents[0].clone();
    assert!(arrived.runs_locally());
    assert_eq!(arrived.local.as_ref().unwrap().folders, Vec::new());
    assert_eq!(
        there.key_of(&arrived).await.secret_key().to_secret_hex(),
        hex
    );
    assert_eq!(
        std::fs::read(there.folder(agent.id).join("memory.sqlite")).unwrap(),
        memory_of("Scout")
    );
    // A folder that is no kit says so.
    let said = moving::open_kit(&here.temporary.path().join("site"))
        .unwrap_err()
        .to_string();
    assert!(said.contains("is not a launch kit"), "{said}");
}

#[tokio::test]
async fn only_a_harness_whose_record_says_so_is_moved() {
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let agent = here.agent("Coder").await;
    here.config.hosted_agents[0].local.as_mut().unwrap().harness = Harness::Codex;
    let file = here.temporary.path().join("coder.bzzagent");
    for refused in [
        moving::move_to_file(
            &mover!(here),
            &mut here.config,
            agent.id,
            &passphrase(),
            &file,
        )
        .await
        .unwrap_err()
        .to_string(),
        moving::launch_kit(
            &mover!(here),
            &mut here.config,
            agent.id,
            &here.temporary.path().join("kit"),
        )
        .await
        .unwrap_err()
        .to_string(),
    ] {
        assert!(
            refused.contains("an agent on the codex harness cannot be moved")
                && refused.contains("its memory belongs to its own program"),
            "{refused}"
        );
    }
    assert!(!file.exists() && !here.temporary.path().join("kit").exists());
    assert!(here.script.starts().is_empty(), "nothing was started");
    // An agent whose process the user runs himself is not bzz's to move.
    here.config.hosted_agents[0].local = None;
    assert!(
        moving::move_to_file(
            &mover!(here),
            &mut here.config,
            agent.id,
            &passphrase(),
            &file,
        )
        .await
        .is_err()
    );
}

/// The plan's M5 exit, with the real host and without the pinned relay: an
/// agent answers here and makes a file; it is moved while the bzz that runs
/// it is open; another bzz of the owner's receives it, and the next message
/// in the same thread shows the conversation's memory and the file. The
/// first bzz no longer runs it.
#[tokio::test]
async fn the_real_host_continues_a_conversation_in_another_bzz_after_a_move() {
    let Some(node) = pi_host::node_or_skip("agent_move_test (the real host)").await else {
        return;
    };
    let owner = Keys::generate();
    let mut here = Home::new(&owner, RELAY);
    let scripted = pi_host::scripted_node(&here.temporary.path().join("bin"), &node);
    let pi_dir = here.temporary.path().join("pi");
    // The agent as `bzz agent new` leaves one: the default launch, the
    // stand-in model, no file yet.
    let mut agent = here.agent("Scout").await;
    {
        let local = agent.local.as_mut().unwrap();
        local.command = Vec::new();
        local.model.as_mut().unwrap().thinking_level = None;
        local.wake = WakeRule::OwnerOnly;
        local.wake_allowlist = Vec::new();
        local.packages = true;
    }
    here.config.hosted_agents[0] = agent.clone();
    here.config.save(&here.paths).unwrap();
    let folder = here.folder(agent.id);
    for file in ["memory.sqlite", "files.sqlite"] {
        std::fs::remove_file(folder.join(file)).unwrap();
    }
    let real = Some((scripted.as_path(), pi_dir.as_path()));
    let relay = RecordedRelay::new(
        here.key_of(&agent).await,
        bzz::broker::relay::auth_tag(&agent.auth_tag).unwrap(),
    );
    let channel = Uuid::new_v4();
    relay.set_channel(ChannelFacts {
        id: channel,
        name: "general".into(),
        kind: PlaceKind::Channel,
        members: vec![owner.public_key().to_hex(), agent.pubkey.clone()],
    });
    let live = Running::start_on(&here, &owner, &agent, relay.clone(), channel, real).await;

    let (hello, reply) = live.answered_in(&here, "hello there", None).await;
    assert_eq!(
        reply,
        "pong: this conversation holds 1 message(s) from people"
    );
    assert_eq!(
        live.answered(
            &here,
            r#"do: [["bash", {"command": "echo made before the move > /home/agent/note.txt && cat /home/agent/note.txt"}], ["report"]]"#,
        )
        .await
        .trim(),
        "made before the move"
    );
    let pid = live.pid().expect("the host is running");
    // The host is killed outright, as when the machine loses power. What it
    // had not yet folded into its two files is in the write-ahead files
    // beside them: the two files alone are not the agent's memory and disk.
    kill(pid);
    until("the host to be gone", WAIT, || {
        (!running(pid)).then_some(())
    })
    .await;
    let write_ahead = || bzz::agents::runtime::settle::write_ahead_files(&folder);
    assert!(
        write_ahead()
            .iter()
            .any(|file| std::fs::metadata(file).is_ok_and(|file| file.len() > 0)),
        "a killed host leaves write-ahead files with content: {:?}",
        write_ahead()
    );

    // The move, while that bzz is open. It makes the files whole first.
    let file = here.temporary.path().join("scout.bzzagent");
    let real_host = HostAccess {
        environment: Arc::new(pi_host::environment(&scripted, &pi_dir)),
        pi_dir: Some(pi_dir.clone()),
        ..HostAccess::of_this_process()
    };
    let mover = Mover {
        kdf: QUICK,
        host: real_host.clone(),
        ..Mover::new(&here.paths, &here.store, &here.owner)
    };
    moving::move_to_file(&mover, &mut here.config, agent.id, &passphrase(), &file)
        .await
        .unwrap();
    drop(mover);
    assert_eq!(
        write_ahead(),
        Vec::<PathBuf>::new(),
        "the files were made whole"
    );
    assert!(
        !folder.join("move.log").exists(),
        "the log of a checkpoint that worked is not kept"
    );
    assert_eq!(
        here.state_on_disk(agent.id),
        Some(LocalAgentState::MovedAway)
    );
    until("the running bzz to see the move", WAIT, || {
        live.line().contains("stopped · moved away").then_some(())
    })
    .await;

    // Another bzz of the owner's, on the same relay.
    let mut there = Home::new(&owner, RELAY);
    let arrival = moving::open_file(&file, &passphrase()).await.unwrap();
    moving::receive(
        &there.paths,
        &there.store,
        &there.owner,
        &mut there.config,
        arrival,
        &scripted_host(&there.script),
    )
    .await
    .unwrap();
    let arrived = there.on_disk().hosted_agents[0].clone();
    assert!(arrived.runs_locally() && arrived.local.as_ref().unwrap().folders.is_empty());
    let moved = Running::start_on(&there, &owner, &arrived, relay.clone(), channel, real).await;
    // The next message in the same thread: the conversation's memory came.
    let (_, reply) = moved.answered_in(&there, "and now?", Some(&hello)).await;
    assert_eq!(
        reply, "pong: this conversation holds 2 message(s) from people",
        "the conversation continued where it was"
    );
    // The file the agent had made is on the disk that came.
    assert_eq!(
        moved
            .answered(
                &there,
                r#"do: [["bash", {"command": "cat /home/agent/note.txt"}], ["report"]]"#,
            )
            .await
            .trim(),
        "made before the move"
    );
    assert!(moved.pid().is_some_and(|there_pid| there_pid != pid));
    // Throughout: one reply to each of the four mentions, and none from the
    // bzz it left, which takes no request for it.
    assert_eq!(relay.by_agent(9).len(), 4);
    let late = live.mention_in("are you still here?", Some(&hello));
    assert_eq!(live.offer(&late).await, Offer::NotRunning);
    assert_eq!(live.pid(), None);
    // Nothing of the agent's key is in what the host wrote in either home.
    let key = there.key_of(&arrived).await;
    for home in [&here, &there] {
        for (path, bytes) in files_below(&home.folder(agent.id)) {
            for (form, secret) in forms_of(&key) {
                assert!(
                    !holds(&bytes, &secret),
                    "the key ({form}) is in {}",
                    path.display()
                );
            }
        }
    }
    moved.host.shutdown().await;
    live.host.shutdown().await;
}
