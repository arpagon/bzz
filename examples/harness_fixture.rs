//! Disposable fixture for Herdr scenarios and captures on a *fresh* isolated
//! pinned harness relay (`ws://localhost:3030`; channel names are fixed).
//! Never point it at a real community.
//!
//! Usage: `harness_fixture <empty fixture dir> <pinned Buzz checkout>`
//!
//! Writes `config/`, `data/` and `cache/` for a fresh viewer identity
//! (encrypted file; its passphrase in `passphrase`, mode 0600) and fills the
//! relay with generated content: `t9-general` (history, a thread, a reaction,
//! a task artifact and custom emoji), `t9-announcements` where the viewer is a
//! guest, and the viewer's own `t9-agent-config` system channel.
//!
//! For v0.15 it also gives the viewer a **local agent**, `Scout`, on bzz's
//! real Pi Durable host with the host's stand-in model, as a member of the
//! viewer's own `#t15-agents`, with one folder granted for commands. That
//! needs Node.js 22.19 or newer and, the first time, the npm registry; with
//! no such Node the agent is left out and the fixture says so. The stand-in
//! is switched on as the tests do it (`tests/support/pi_host.rs`): bzz is
//! given, as `BZZ_NODE`, a small program that sets the host's variable and
//! runs the real Node. Those paths are in the fixture's `env` file, which
//! `scripts/test-tui-herdr.sh` adds to bzz's environment. `second/` is a
//! second profile of the same owner with no agent, to receive one in.
//! Three more agents, one on each coding harness, have no program to run
//! on: the same file names `grok`, `codex` and `claude` as a path that does
//! not exist, so that no program of the user's own is ever started.

use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bzz::{
    agents::{manage, runtime::install},
    auth::{IdentityManager, signer::SignerHandle},
    config::{AgentModelConfig, Config, KeyBackend},
    paths::Paths,
    realtime::session::{self, SessionEvent, SessionHandle},
    store::{Store, writer::StoreHandle},
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use secrecy::SecretString;
use uuid::Uuid;

const RELAY_WS: &str = "ws://localhost:3030/";
/// One second apart per event, so history has a deterministic order.
static CLOCK: AtomicU64 = AtomicU64::new(0);

fn seed_member(source: &str, pubkey: &str) {
    let compose = format!("{source}/docker-compose.harness.yml");
    let sql = format!(
        "INSERT INTO relay_members (community_id,pubkey,role,added_by) SELECT id,'{pubkey}','member',NULL FROM communities WHERE lower(host)='localhost:3030' ON CONFLICT (community_id,pubkey) DO NOTHING;"
    );
    let status = Command::new("docker")
        .args([
            "compose",
            "-p",
            "buzz-harness",
            "-f",
            &compose,
            "exec",
            "-T",
            "postgres",
            "psql",
            "-U",
            "buzz",
            "-d",
            "buzz",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
            &sql,
        ])
        .status()
        .expect("docker compose");
    assert!(status.success(), "seeding a harness member failed");
}

async fn connect(signer: &SignerHandle) -> SessionHandle {
    let (session, mut events) =
        session::connect(url::Url::parse(RELAY_WS).unwrap(), signer.clone())
            .await
            .expect("harness relay reachable");
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    session
}

async fn publish(
    session: &SessionHandle,
    signer: &SignerHandle,
    builder: EventBuilder,
) -> nostr::Event {
    let at = CLOCK.fetch_add(1, Ordering::Relaxed);
    let event = signer
        .sign(builder.custom_created_at(nostr::Timestamp::from(at)))
        .await
        .unwrap();
    let ack = session.publish(event.clone()).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    event
}

fn create(
    id: Uuid,
    name: &str,
    visibility: buzz_sdk::Visibility,
    kind: buzz_sdk::ChannelKind,
) -> EventBuilder {
    buzz_sdk::build_create_channel(id, name, Some(visibility), Some(kind), None, None, false)
        .unwrap()
}

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let root = std::path::PathBuf::from(arguments.next().expect("fixture directory"));
    let source = arguments.next().expect("pinned Buzz checkout");
    assert!(
        !root.exists() || std::fs::read_dir(&root).unwrap().next().is_none(),
        "use an empty fixture directory"
    );
    // BZZ_FIXTURE_HISTORY: generated messages in #t9-general (default 24;
    // more than 100 spans several channel-window pages for scroll captures).
    let history = std::env::var("BZZ_FIXTURE_HISTORY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(24)
        .clamp(1, 2_000);
    CLOCK.store(
        nostr::Timestamp::now().as_secs() - history - 60,
        Ordering::Relaxed,
    );
    let owner = Keys::generate();
    let viewer = Keys::generate();
    seed_member(&source, &owner.public_key().to_hex());
    seed_member(&source, &viewer.public_key().to_hex());
    // Two people named alike, for contextual names in the mention chooser.
    let namesakes = [Keys::generate(), Keys::generate()];
    for keys in &namesakes {
        seed_member(&source, &keys.public_key().to_hex());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    // BZZ_FIXTURE_IDENTITY_ADAPTER: an FI-enforcing relay (`test-relay.sh
    // --nip-fi`) admits nothing without evidence, so write only the viewer's
    // profile with this adapter and no relay content.
    let adapter = std::env::var("BZZ_FIXTURE_IDENTITY_ADAPTER").ok();
    let fi = adapter.is_some();
    if !fi {
        publish_content(&owner, &viewer, &namesakes, history).await;
    }
    let passphrase = hex::encode(rand::random::<[u8; 16]>());
    let (paths, config) = write_profile(&root, &root, &viewer, &passphrase, adapter);
    if fi {
        println!("fixture ready: FI profile (identity_adapter set, no relay content)");
        return;
    }
    println!("fixture ready: #t9-general, #t9-announcements (guest), #t9-agent-config (system)");
    // BZZ_FIXTURE_AGENTS=0: no local agent (a capture that needs none).
    if std::env::var("BZZ_FIXTURE_AGENTS").as_deref() != Ok("0") {
        write_profile(&root.join("second"), &root, &viewer, &passphrase, None);
        local_agent(&root, &paths, config, &passphrase).await;
    }
}

/// The viewer's local agent `Scout` and its channel `#t15-agents`.
async fn local_agent(root: &std::path::Path, paths: &Paths, mut config: Config, passphrase: &str) {
    let lookup = |name: &str| std::env::var(name).ok();
    let node = match install::find_node(lookup).await {
        Ok(node) => node,
        Err(problem) => {
            println!("fixture: no local agent ({problem})");
            return;
        }
    };
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let model = repository.join("agent-host/test/support/scripted-model.mjs");
    assert!(model.is_file(), "{} is missing", model.display());
    // The stand-in for `node`, Pi's own folder (an empty one, never the
    // user's) and a cache for npm that is not the user's either.
    let stand_in = root.join("bin").join("node");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(
        &stand_in,
        format!(
            "#!/bin/sh\nBZZ_AGENT_HOST_SCRIPTED_MODEL='{}'\nexport BZZ_AGENT_HOST_SCRIPTED_MODEL\nexec '{}' \"$@\"\n",
            model.display(),
            node.program.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let pi_dir = root.join("pi");
    // No program of the user's own is ever started from this profile: the
    // three coding programs are named as a file that is not there.
    let absent = root.join("bin").join("not-installed").display().to_string();
    let variables = [
        ("BZZ_NODE", stand_in.display().to_string()),
        ("PI_CODING_AGENT_DIR", pi_dir.display().to_string()),
        ("BZZ_GROK", absent.clone()),
        ("BZZ_CODEX", absent.clone()),
        ("BZZ_CLAUDE", absent),
        (
            "npm_config_cache",
            repository
                .join("target/tmp/npm-cache")
                .display()
                .to_string(),
        ),
    ];
    std::fs::write(
        root.join("env"),
        variables
            .iter()
            .map(|(name, value)| format!("{name}={value}\n"))
            .collect::<String>(),
    )
    .unwrap();
    // The folder Scout may run commands in, each asked of the owner first.
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    // Where a scenario writes the file of a moved agent.
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::write(work.join("README.txt"), "a folder of the fixture\n").unwrap();

    let identity = config.identities[0].clone();
    let community = config.communities[0].clone();
    let secret = SecretString::from(passphrase.to_owned());
    let owner = IdentityManager::new(paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    // The viewer's own channel, so that the viewer may add members to it.
    let session = connect(&owner).await;
    let channel = Uuid::new_v4();
    use buzz_sdk::{ChannelKind, Visibility};
    for builder in [
        create(channel, "t15-agents", Visibility::Open, ChannelKind::Stream),
        buzz_sdk::build_message(
            channel,
            "Generated agents channel",
            None,
            &[],
            false,
            &[],
            &[],
        )
        .unwrap(),
    ] {
        let event = owner.sign(builder).await.unwrap();
        let ack = session.publish(event).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
    }
    session.shutdown().await;

    let mut new = manage::NewAgent::named("Scout");
    new.model = Some(AgentModelConfig {
        provider: "scripted".into(),
        model_id: "scripted".into(),
        thinking_level: None,
    });
    new.instructions = "Be brief.".into();
    new.folders = vec![manage::parse_folder(&format!("work={}:exec", work.display())).unwrap()];
    let variables = variables.map(|(name, value)| (name.to_owned(), value));
    let environment: bzz::agents::runtime::agent::Environment = std::sync::Arc::new(move |name| {
        variables
            .iter()
            .find(|(variable, _)| variable == name)
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
    });
    // Installs the host into the profile on this first use.
    manage::preflight(
        &paths.data_dir,
        &new,
        Some(&pi_dir),
        &environment,
        &|line| {
            println!("fixture: {line}");
        },
    )
    .await
    .expect("the checks before an agent is made");
    let relay = manage::OwnerRelay::connect(paths, &community, &identity, owner.clone())
        .await
        .expect("the owner's connection");
    let created = manage::create(
        &manage::OwnerContext {
            paths,
            owner: &owner,
            owner_identity: identity.id,
            community: community.id,
            federated: None,
        },
        &mut config,
        &new,
        &relay,
    )
    .await
    .unwrap_or_else(|failure| panic!("the agent was not made: {failure}"));
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let channel = relay.find_channel(&store, "t15-agents").await.unwrap();
    manage::add_to_channel(&relay, &store, &created.agent, channel)
        .await
        .expect("the agent is added to its channel");
    // One agent on each coding harness, as an owner has them whose program
    // is no longer on the machine: the panel lists each with its record and
    // says what is missing. Nothing is started for them (see `absent`).
    for (name, harness) in [
        ("Garnet", bzz::config::Harness::Grok),
        ("Cobalt", bzz::config::Harness::Codex),
        ("Coral", bzz::config::Harness::ClaudeCode),
    ] {
        let mut coding = manage::NewAgent::named(name);
        coding.harness = harness;
        manage::create(
            &manage::OwnerContext {
                paths,
                owner: &owner,
                owner_identity: identity.id,
                community: community.id,
                federated: None,
            },
            &mut config,
            &coding,
            &relay,
        )
        .await
        .unwrap_or_else(|failure| panic!("the {name} agent was not made: {failure}"));
    }
    relay.close().await;
    owner.lock().await;
    println!("fixture ready: local agent Scout (pi-durable, stand-in model) in #t15-agents");
    println!(
        "fixture ready: Garnet (grok), Cobalt (codex), Coral (claude-code), their programs absent"
    );
}

async fn publish_content(owner: &Keys, viewer: &Keys, namesakes: &[Keys], history: u64) {
    let owner_signer = SignerHandle::spawn(owner.clone());
    let viewer_signer = SignerHandle::spawn(viewer.clone());
    let owner_session = connect(&owner_signer).await;
    let viewer_session = connect(&viewer_signer).await;
    use buzz_sdk::{ChannelKind, Visibility};

    // general: open stream with generated history, a thread and a reaction.
    let general = Uuid::new_v4();
    publish(
        &owner_session,
        &owner_signer,
        create(general, "t9-general", Visibility::Open, ChannelKind::Stream),
    )
    .await;
    publish(
        &viewer_session,
        &viewer_signer,
        buzz_sdk::build_join(general).unwrap(),
    )
    .await;
    let mut last = None;
    for index in 1..=history {
        let text = format!(
            "Generated fixture message {index:03}: the harness relay keeps this channel busy."
        );
        last = Some(
            publish(
                &owner_session,
                &owner_signer,
                buzz_sdk::build_message(general, &text, None, &[], false, &[], &[]).unwrap(),
            )
            .await,
        );
    }
    let thread_root = last.unwrap();
    let thread = buzz_sdk::ThreadRef {
        root_event_id: thread_root.id,
        parent_event_id: thread_root.id,
    };
    for index in 1..=3 {
        publish(
            &owner_session,
            &owner_signer,
            buzz_sdk::build_message(
                general,
                &format!("Generated thread reply {index}"),
                Some(&thread),
                &[],
                false,
                &[],
                &[],
            )
            .unwrap(),
        )
        .await;
    }
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_reaction(thread_root.id, "👍").unwrap(),
    )
    .await;
    // Custom emoji. The default image URL is deliberately unreachable (text
    // fallback); graphics captures set BZZ_FIXTURE_EMOJI_URL to a public
    // bzz-owned image.
    let emoji_url = std::env::var("BZZ_FIXTURE_EMOJI_URL")
        .unwrap_or_else(|_| "https://example.invalid/bzz/wave.png".into());
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_custom_emoji_set(&[buzz_sdk::CustomEmoji {
            shortcode: "bzzwave".into(),
            url: emoji_url.clone(),
        }])
        .unwrap(),
    )
    .await;
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_message(
            general,
            "Generated custom emoji :bzzwave: fixture",
            None,
            &[],
            false,
            &[],
            &[vec!["emoji".into(), "bzzwave".into(), emoji_url.clone()]],
        )
        .unwrap(),
    )
    .await;
    // A task artifact assigned to the viewer.
    let task = EventBuilder::new(Kind::Custom(45_010), r#"{"status":"open"}"#).tags(
        [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &general.to_string()],
            ["type", "buzz.task"],
            ["title", "Generated fixture task"],
            ["op", "create"],
            ["assignee", &viewer.public_key().to_hex()],
        ]
        .map(|parts| Tag::parse(parts).unwrap()),
    );
    publish(&owner_session, &owner_signer, task).await;

    // announcements: joiners become guests (read-only).
    let announcements = Uuid::new_v4();
    publish(
        &owner_session,
        &owner_signer,
        create(
            announcements,
            "t9-announcements",
            Visibility::Open,
            ChannelKind::Stream,
        ),
    )
    .await;
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_update_channel(
            announcements,
            None,
            None,
            None,
            None,
            Some(buzz_sdk::ChannelJoinRole::Guest),
            None,
            None,
        )
        .unwrap(),
    )
    .await;
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_message(
            announcements,
            "Generated announcement: guests read only.",
            None,
            &[],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    publish(
        &viewer_session,
        &viewer_signer,
        buzz_sdk::build_join(announcements).unwrap(),
    )
    .await;

    // The viewer's own system channel.
    publish(
        &viewer_session,
        &viewer_signer,
        create(
            Uuid::new_v4(),
            "t9-agent-config",
            Visibility::Private,
            ChannelKind::System,
        ),
    )
    .await;
    // t14-names: two members both named "Honey" (v0.14 M3).
    let names = Uuid::new_v4();
    publish(
        &owner_session,
        &owner_signer,
        create(names, "t14-names", Visibility::Open, ChannelKind::Stream),
    )
    .await;
    publish(
        &viewer_session,
        &viewer_signer,
        buzz_sdk::build_join(names).unwrap(),
    )
    .await;
    for keys in namesakes {
        let signer = SignerHandle::spawn(keys.clone());
        let session = connect(&signer).await;
        publish(
            &session,
            &signer,
            EventBuilder::new(Kind::Metadata, r#"{"name":"Honey"}"#),
        )
        .await;
        publish(&session, &signer, buzz_sdk::build_join(names).unwrap()).await;
        session.shutdown().await;
    }
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_message(
            names,
            "Generated namesakes channel",
            None,
            &[],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    owner_session.shutdown().await;
    viewer_session.shutdown().await;
}

/// An isolated bzz profile of the viewer under `root`; its passphrase is in
/// `secrets/passphrase`.
fn write_profile(
    root: &std::path::Path,
    secrets: &std::path::Path,
    viewer: &Keys,
    passphrase: &str,
    adapter: Option<String>,
) -> (Paths, Config) {
    let paths = Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        cache_dir: root.join("cache"),
    };
    paths.ensure().unwrap();
    let passphrase_file = secrets.join("passphrase");
    std::fs::write(&passphrase_file, format!("{passphrase}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&passphrase_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut config = Config::default();
    let identity = IdentityManager::new(&paths)
        .import_keys(
            &mut config,
            "harness-viewer".into(),
            KeyBackend::EncryptedFile,
            viewer.clone(),
            Some(&SecretString::from(passphrase.to_owned())),
        )
        .unwrap();
    let community = config
        .add_community(
            "harness".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    if let Some(adapter) = adapter {
        config
            .communities
            .iter_mut()
            .find(|entry| entry.id == community)
            .unwrap()
            .identity_adapter = Some(adapter);
    }
    config.save(&paths).unwrap();
    (paths, config)
}
