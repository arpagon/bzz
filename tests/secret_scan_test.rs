//! T8 (v0.13): drive the new secret-bearing surfaces with real secrets, then
//! scan every file bzz wrote, its reports and JSON status, and the Debug and
//! Display forms of the types that hold them. Nothing secret may appear.
mod support;

use std::{path::Path, process::Command};

use bzz::{
    auth::{
        federated::{AdapterClient, AgentEvidence, EvidenceSource as _},
        signer::SignerHandle,
    },
    clock,
    diagnostics::{DiagnosticEvent, DiagnosticHandle, ErrorClass},
    paths::Paths,
};
use nostr::Keys;
use secrecy::ExposeSecret as _;
use support::fake_adapter::{Config, FakeAdapter};

fn all_files(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            all_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn assert_absent(haystacks: &[(String, String)], secrets: &[(&str, &str)]) {
    for (where_, text) in haystacks {
        for (what, secret) in secrets {
            assert!(
                !secret.is_empty() && !text.contains(secret),
                "{what} leaked into {where_}"
            );
        }
    }
}

fn bzz(dirs: &Path, args: &[&str], passphrase: &Path) -> std::process::Output {
    // The passphrase reaches the child only as fd 3, never in arguments.
    Command::new("sh")
        .arg("-c")
        .arg("exec \"$0\" \"$@\" 3<\"$BZZ_T8_PASSPHRASE\"")
        .arg(env!("CARGO_BIN_EXE_bzz"))
        .args(args)
        .env("BZZ_T8_PASSPHRASE", passphrase)
        .env("BZZ_CONFIG_DIR", dirs.join("c"))
        .env("BZZ_DATA_DIR", dirs.join("d"))
        .env("BZZ_CACHE_DIR", dirs.join("k"))
        .env("BZZ_PASSPHRASE_FD", "3")
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn broker_credentials_never_reach_disk_reports_or_status() {
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase = dirs.join("pass");
    std::fs::write(&passphrase, "sentinel passphrase for t8\n").unwrap();
    let created = bzz(
        dirs,
        &[
            "identity",
            "new",
            "--label",
            "owner",
            "--backend",
            "encrypted-file",
        ],
        &passphrase,
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let stdout = String::from_utf8_lossy(&created.stdout).into_owned();
    let identity = stdout
        .lines()
        .find_map(|line| line.strip_prefix("identity: "))
        .unwrap()
        .trim()
        .to_owned();
    // A community entry without probing a relay.
    let config_file = dirs.join("c").join("config.toml");
    let community = uuid::Uuid::new_v4();
    let config = std::fs::read_to_string(&config_file)
        .unwrap()
        .replace("communities = []\n", "");
    std::fs::write(
        &config_file,
        format!(
            "default_community = \"{community}\"\n{config}\n[[communities]]\nid = \"{community}\"\nlabel = \"t8\"\nrelay_url = \"ws://localhost:3030\"\nidentity_id = \"{identity}\"\nallow_insecure_localhost = true\n"
        ),
    )
    .unwrap();
    let credential = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .find(|word| word.starts_with("bzzb_"))
            .unwrap()
            .to_owned()
    };
    let create = bzz(dirs, &["broker", "create", "helper", "--yes"], &passphrase);
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let first = credential(&create);
    let rotate = bzz(dirs, &["broker", "rotate", "helper"], &passphrase);
    let second = credential(&rotate);
    let list = bzz(dirs, &["broker", "list"], &passphrase);
    let status = bzz(dirs, &["diagnostics", "status", "--json"], &passphrase);
    let report = dirs.join("report.json");
    let wrote = bzz(
        dirs,
        &[
            "diagnostics",
            "report",
            "--output",
            report.to_str().unwrap(),
        ],
        &passphrase,
    );
    assert!(
        wrote.status.success(),
        "{}",
        String::from_utf8_lossy(&wrote.stderr)
    );

    let mut haystacks = vec![
        (
            "broker list".to_owned(),
            String::from_utf8_lossy(&list.stdout).into_owned(),
        ),
        (
            "diagnostics status".to_owned(),
            String::from_utf8_lossy(&status.stdout).into_owned(),
        ),
        (
            "stderr".to_owned(),
            String::from_utf8_lossy(&create.stderr).into_owned(),
        ),
    ];
    let mut files = Vec::new();
    all_files(dirs, &mut files);
    for file in files.iter().filter(|file| *file != &passphrase) {
        let bytes = std::fs::read(file).unwrap();
        haystacks.push((
            file.display().to_string(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    assert!(
        haystacks
            .iter()
            .any(|(path, _)| path.ends_with("report.json"))
    );
    assert_absent(
        &haystacks,
        &[
            ("first credential", &first),
            ("rotated credential", &second),
            ("owner passphrase", "sentinel passphrase for t8"),
        ],
    );
}

#[tokio::test]
async fn adapter_session_grant_and_assertions_never_print_or_journal() {
    let adapter = FakeAdapter::start(Config::default()).await;
    let client = AdapterClient::new(adapter.base()).unwrap();
    let pending = client.begin_login().await.unwrap();
    let login = pending.url.clone();
    let browser = tokio::spawn(async move { FakeAdapter::browse(&login).await });
    let session = client
        .complete_login(pending, std::time::Duration::from_secs(10))
        .await
        .unwrap();
    browser.await.unwrap();
    let relay = url::Url::parse("wss://community.example").unwrap();
    let owner_keys = Keys::generate();
    let owner = SignerHandle::spawn(owner_keys.clone());
    let assertion = client.assertion(&session, &relay, &owner).await.unwrap();
    let agent_keys = Keys::generate();
    let agent = SignerHandle::spawn(agent_keys.clone());
    let tag = owner
        .owner_auth_tag(agent_keys.public_key(), "")
        .await
        .unwrap();
    let grant = client
        .agent_grant(&session, &agent, &tag, &owner_keys.public_key().to_hex())
        .await
        .unwrap();
    let grant_token = grant.token.expose_secret().to_owned();
    let evidence = AgentEvidence::new(
        client.clone(),
        grant.clone(),
        agent.clone(),
        clock::system(),
    );
    let agent_assertion = evidence.evidence(&relay).await.unwrap().unwrap();
    // Failures carry fixed text only, never the credential that failed.
    client
        .agent_revoke(&session, &agent_keys.public_key().to_hex())
        .await
        .unwrap();
    evidence.invalidate(&relay);
    let failure = evidence.evidence(&relay).await.unwrap_err();

    // The journal records the failure as a fixed class.
    let temporary = tempfile::TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let journal = DiagnosticHandle::start(&paths, bzz::config::LocalJournalMode::On, None).unwrap();
    journal.emit(DiagnosticEvent::AgentTypingSubscriptionClosed {
        error_class: ErrorClass::from_error(&failure),
    });
    journal.shutdown().await;

    let mut haystacks = vec![
        ("Session debug".to_owned(), format!("{session:?}")),
        ("AgentGrant debug".to_owned(), format!("{grant:?}")),
        ("Assertion debug".to_owned(), format!("{assertion:?}")),
        ("error display".to_owned(), failure.to_string()),
        ("error debug".to_owned(), format!("{failure:?}")),
    ];
    let mut files = Vec::new();
    all_files(temporary.path(), &mut files);
    assert!(!files.is_empty(), "the journal wrote a file");
    for file in files {
        haystacks.push((
            file.display().to_string(),
            String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned(),
        ));
    }
    let session_token = session.token.expose_secret().to_owned();
    let jws = assertion.jws.expose_secret().to_owned();
    let agent_jws = agent_assertion.trim_start_matches("Bearer ").to_owned();
    assert_absent(
        &haystacks,
        &[
            ("adapter session token", &session_token),
            ("agent grant", &grant_token),
            ("owner assertion", &jws),
            ("agent assertion", &agent_jws),
            ("owner secret", &owner_keys.secret_key().to_secret_hex()),
            ("agent secret", &agent_keys.secret_key().to_secret_hex()),
        ],
    );
    owner.lock().await;
    agent.lock().await;
}

/// v0.15 M5: an agent's key leaves bzz only as the owner asked. `bzz agent
/// move` seals it in a file, `bzz agent receive` takes it into custody,
/// sealed to the owner, and `bzz agent launch-kit` writes it in the clear into one
/// file of mode 600, after saying so and asking. Every command is the `bzz`
/// binary, with the scripted harness as the agent's host; then everything
/// bzz printed and every file it wrote is searched for the key, for both
/// passphrases and for what the agent remembers.
#[cfg(unix)]
#[tokio::test]
async fn a_moved_agents_key_is_sealed_in_its_file_and_in_the_clear_in_one_file_of_a_kit() {
    use std::os::unix::fs::PermissionsExt as _;

    use bzz::{
        agents::runtime::process,
        auth::IdentityManager,
        broker::custody,
        config::{Config, Harness, LocalAgentConfig, LocalAgentState},
    };
    use nostr::ToBech32 as _;
    use serde_json::json;
    use support::agent_harness::Script;

    let temporary = tempfile::TempDir::new().unwrap();
    let work = temporary.path();
    let owner_passphrase = "the owner's disposable passphrase";
    let file_passphrase = "the moved file's own passphrase";
    std::fs::write(work.join("pass"), format!("{owner_passphrase}\n")).unwrap();
    std::fs::write(work.join("file-pass"), format!("{file_passphrase}\n")).unwrap();
    // The owner's passphrase on descriptor 3, the file's on descriptor 4:
    // neither is ever an argument or a variable's value.
    let run = |dirs: &Path, args: &[&str]| {
        let output = Command::new("sh")
            .arg("-c")
            .arg("exec \"$0\" \"$@\" 3<\"$BZZ_T8_PASSPHRASE\" 4<\"$BZZ_T8_FILE_PASSPHRASE\"")
            .arg(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_T8_PASSPHRASE", work.join("pass"))
            .env("BZZ_T8_FILE_PASSPHRASE", work.join("file-pass"))
            .env("BZZ_CONFIG_DIR", dirs.join("c"))
            .env("BZZ_DATA_DIR", dirs.join("d"))
            .env("BZZ_CACHE_DIR", dirs.join("k"))
            .env("BZZ_PASSPHRASE_FD", "3")
            .env("BZZ_MOVE_PASSPHRASE_FD", "4")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    let paths_of = |dirs: &Path| Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut said = Vec::<(String, String)>::new();
    let mut ok = |dirs: &Path, args: &[&str]| {
        let (success, text) = run(dirs, args);
        assert!(success, "bzz {args:?}: {text}");
        said.push((
            format!("what `bzz {}` printed", args.join(" ")),
            text.clone(),
        ));
        text
    };

    // The owner's first bzz, with two agents: one goes to another bzz, one
    // to a machine with buzz-acp.
    let first = work.join("first");
    ok(
        &first,
        &[
            "identity",
            "new",
            "--label",
            "owner",
            "--backend",
            "encrypted-file",
        ],
    );
    let paths = paths_of(&first);
    let mut config = Config::load(&paths).unwrap();
    let identity = config.identities[0].clone();
    let community = config
        .add_community(
            "c1".into(),
            "wss://relay.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let secret = secrecy::SecretString::from(owner_passphrase.to_owned());
    let owner_keys = IdentityManager::new(&paths)
        .unlock(&identity, Some(&secret))
        .unwrap();
    let owner = SignerHandle::spawn(owner_keys.clone());
    let script = Script::new(&work.join("harness"));
    script.write(json!({"own_host": true}), json!([]));
    let remembered = "what the agent remembers and nobody else should read";
    let mut keys = Vec::new();
    for name in ["Scout", "Courier"] {
        let (mut agent, _) =
            custody::create(&paths.keys_dir(), &owner, identity.id, community, name)
                .await
                .unwrap();
        agent.allowed.clear();
        agent.local = Some(LocalAgentConfig {
            instructions: format!("Private instructions of {name}."),
            // The agent's host is run from a checkout: here, the scripted one.
            command: script.command(),
            ..LocalAgentConfig::new(Harness::PiDurable)
        });
        let folder = process::agent_dir(&paths.data_dir, agent.id);
        process::create_private_dir(&folder).unwrap();
        for file in ["memory.sqlite", "files.sqlite"] {
            std::fs::write(folder.join(file), format!("SQLite format 3 {remembered} ")).unwrap();
        }
        let sealed =
            std::fs::read_to_string(paths.keys_dir().join(format!("agent-{}.nip44", agent.id)))
                .unwrap();
        keys.push(Keys::parse(&owner.decrypt_self(sealed).await.unwrap()).unwrap());
        config.hosted_agents.push(agent);
    }
    config.save(&paths).unwrap();
    owner.lock().await;

    // ---- bzz agent move ----------------------------------------------------
    let file = work.join("scout.bzzagent");
    let text = ok(
        &first,
        &["agent", "move", "Scout", "--to", file.to_str().unwrap()],
    );
    assert!(text.contains("sealed with your passphrase"), "{text}");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // ---- bzz agent receive ---------------------------------------------------
    // Here, where it was moved away from: it is taken back. (Another bzz of
    // the owner's receiving it is `tests/agent_move_test.rs`.)
    let text = ok(&first, &["agent", "receive", file.to_str().unwrap()]);
    assert!(
        text.contains("taken back") && text.contains("its key is in bzz's custody"),
        "{text}"
    );
    assert!(
        Config::load(&paths)
            .unwrap()
            .hosted_agents
            .iter()
            .find(|agent| agent.label == "Scout")
            .unwrap()
            .runs_locally()
    );

    // ---- bzz agent launch-kit ----------------------------------------------
    let kit = work.join("courier-kit");
    // Not without the owner's yes: asked, and with no answer nothing is
    // written.
    let (written, text) = run(
        &first,
        &[
            "agent",
            "launch-kit",
            "Courier",
            "--to",
            kit.to_str().unwrap(),
        ],
    );
    assert!(
        !written
            && text.contains("holds the key of Courier in the clear")
            && text.contains("no launch kit was written"),
        "{text}"
    );
    assert!(!kit.exists());
    said.push(("what a refused `bzz agent launch-kit` printed".into(), text));
    let mut ok = |dirs: &Path, args: &[&str]| {
        let (success, text) = run(dirs, args);
        assert!(success, "bzz {args:?}: {text}");
        said.push((
            format!("what `bzz {}` printed", args.join(" ")),
            text.clone(),
        ));
        text
    };
    let text = ok(
        &first,
        &[
            "agent",
            "launch-kit",
            "Courier",
            "--to",
            kit.to_str().unwrap(),
            "--yes",
        ],
    );
    assert!(
        text.contains("A launch kit holds the key of Courier in the clear")
            && text.contains("THE AGENT'S KEY IS IN THE CLEAR in"),
        "{text}"
    );
    assert_eq!(
        Config::load(&paths)
            .unwrap()
            .hosted_agents
            .iter()
            .find(|agent| agent.label == "Courier")
            .and_then(|agent| agent.local.as_ref().map(|local| local.state)),
        Some(LocalAgentState::MovedAway)
    );
    let listed = ok(&first, &["agent", "list"]);
    assert!(
        listed.contains("Scout\tactive") && listed.contains("Courier\tmoved away"),
        "{listed}"
    );

    // ---- the scan ----------------------------------------------------------
    let (scout, courier) = (&keys[0], &keys[1]);
    let forms = |keys: &Keys| {
        [
            keys.secret_key().to_secret_hex(),
            keys.secret_key().to_bech32().unwrap(),
        ]
    };
    let (scout_forms, courier_forms, owner_forms) =
        (forms(scout), forms(courier), forms(&owner_keys));
    let secrets = [
        ("Scout's key (hex)", scout_forms[0].as_str()),
        ("Scout's key (nsec)", scout_forms[1].as_str()),
        ("Courier's key (hex)", courier_forms[0].as_str()),
        ("Courier's key (nsec)", courier_forms[1].as_str()),
        ("the owner's key (hex)", owner_forms[0].as_str()),
        ("the owner's key (nsec)", owner_forms[1].as_str()),
        ("the owner's passphrase", owner_passphrase),
        ("the file's passphrase", file_passphrase),
    ];
    // The one file that may hold a key, and holds exactly one.
    let key_file = kit.join("buzz-acp.env");
    assert_eq!(
        std::fs::metadata(&key_file).unwrap().permissions().mode() & 0o777,
        0o600,
        "the kit's key file is its owner's alone"
    );
    let environment = std::fs::read_to_string(&key_file).unwrap();
    assert!(
        environment.contains(&format!("BUZZ_PRIVATE_KEY='{}'", courier_forms[0])),
        "the kit holds the key of the agent it is for"
    );
    assert_absent(
        &[("the kit's key file".into(), environment)],
        &secrets
            .iter()
            .filter(|(what, _)| *what != "Courier's key (hex)")
            .copied()
            .collect::<Vec<_>>(),
    );
    // Everything else: what bzz printed, every file bzz keeps, the sealed
    // file, and the other files of the kit.
    let mut files = Vec::new();
    for root in [&first, &kit] {
        all_files(root, &mut files);
    }
    files.push(file.clone());
    let mut haystacks = said;
    let mut scanned = Vec::new();
    for path in files.iter().filter(|path| **path != key_file) {
        let name = path.strip_prefix(work).unwrap().display().to_string();
        haystacks.push((
            name.clone(),
            String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned(),
        ));
        scanned.push(name);
    }
    // The scan sees what is there: the settings, the database, both sealed
    // keys, the sealed file and the kit's files are all among what it read.
    for seen in [
        "first/c/config.toml",
        "first/d/bzz.db",
        "scout.bzzagent",
        "courier-kit/bzz-agent.json",
        "courier-kit/agent/agent.json",
        "courier-kit/README.md",
        "courier-kit/start.sh",
    ] {
        assert!(
            scanned.iter().any(|name| name == seen),
            "{seen}: {scanned:?}"
        );
    }
    assert_eq!(
        scanned
            .iter()
            .filter(|name| name.contains("/d/keys/agent-"))
            .count(),
        2,
        "both agents' sealed keys: {scanned:?}"
    );
    assert_absent(&haystacks, &secrets);
    // What the agent remembers is sealed in the file too. In the kit it is
    // not: the two files are there as they are.
    let sealed = std::fs::read(&file).unwrap();
    for plain in [remembered, "Private instructions of Scout.", "Scout"] {
        assert!(
            !sealed
                .windows(plain.len())
                .any(|window| window == plain.as_bytes()),
            "{plain:?} is readable in the sealed file"
        );
    }
    assert!(
        std::fs::read_to_string(kit.join("agent/memory.sqlite"))
            .unwrap()
            .contains(remembered)
    );
}

/// v0.15 M1 and M6, rule 10 (the key stays in bzz): run a local agent whose
/// key is sealed on disk through whole turns, on bzz's own host and on each
/// of the three coding harnesses, then scan what its harness could see (its
/// environment, its arguments, every message bzz sent it, every tool result)
/// and what bzz wrote for it (its log, its folder, the ownership record, the
/// database, a recorded transcript). No agent key, owner key or broker
/// credential may appear. The session credential of `bzz agent-tools` appears
/// in one place only: the server entry of `session/new`.
///
/// A coding harness is started here as bzz starts the owner's own program:
/// found on `PATH`, asked for its version, and for Codex and Claude Code run
/// by Node through an adapter that is installed first. Stand-ins take the
/// place of the four programs and of npm.
#[cfg(unix)]
#[tokio::test]
async fn no_key_reaches_a_harness_environment_arguments_or_logs() {
    use bzz::{
        agents::runtime::{
            Limits,
            admission::{ChannelFacts, PlaceKind},
            agent::AgentSpec,
            host::{AgentHost, HostOptions, Offer},
            process,
        },
        broker::custody,
        config::{Harness, LocalAgentConfig},
        store::{Store, writer::StoreHandle},
    };
    use nostr::ToBech32 as _;
    use serde_json::json;
    use support::agent_harness::{RecordedRelay, Script};

    let temporary = tempfile::TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let owner_keys = Keys::generate();
    let owner = SignerHandle::spawn(owner_keys.clone());
    // The owner's community, in the database as bzz keeps it.
    let mut config = bzz::config::Config::default();
    let identity = bzz::config::IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "owner".into(),
        pubkey: owner_keys.public_key().to_hex(),
        backend: bzz::config::KeyBackend::EncryptedFile,
        key_ref: "owner".into(),
    };
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "scan".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    // The agent as bzz makes one: its key sealed to the owner on disk.
    let (configured, broker_credential) = custody::create(
        &paths.keys_dir(),
        &owner,
        uuid::Uuid::new_v4(),
        community,
        "Scout",
    )
    .await
    .unwrap();
    let sealed = std::fs::read_to_string(
        paths
            .keys_dir()
            .join(format!("agent-{}.nip44", configured.id)),
    )
    .unwrap();
    let agent_keys = Keys::parse(&owner.decrypt_self(sealed).await.unwrap()).unwrap();
    assert_eq!(agent_keys.public_key().to_hex(), configured.pubkey);
    let relay = RecordedRelay::new(
        agent_keys.clone(),
        bzz::broker::relay::auth_tag(&configured.auth_tag).unwrap(),
    );
    let channel = uuid::Uuid::new_v4();
    relay.set_channel(ChannelFacts {
        id: channel,
        name: "general".into(),
        read_only: Vec::new(),
        kind: PlaceKind::Channel,
        members: vec![owner_keys.public_key().to_hex(), configured.pubkey.clone()],
    });
    let mut opened_store = Store::open(paths.database_file()).unwrap();
    opened_store.sync_config(&config).unwrap();
    // v0.16 M7: the Rota open in the channel, and a relay that keeps its
    // records, which the agent reads and writes through its tools.
    #[cfg(feature = "apps")]
    let artifacts = {
        let artifacts = support::artifact_relay::ArtifactRelay::start().await;
        opened_store
            .pin_relay_pubkey(community, &artifacts.keys.public_key().to_hex())
            .unwrap();
        let mut apps = bzz::apps::store::Apps::new(&mut opened_store, &paths);
        let bzz::apps::store::Recorded::Waiting { version, hash, .. } = apps
            .propose(
                community,
                bzz::apps::store::Submission::new(
                    "rota",
                    include_str!("fixtures/apps/rota/app.lua"),
                    include_str!("fixtures/apps/rota/brief.md"),
                    bzz::apps::store::Proposer::owner(),
                ),
            )
            .unwrap()
        else {
            panic!("the Rota loads");
        };
        apps.approve(community, "rota", version, &hash).unwrap();
        apps.open_in(community, "rota", channel).unwrap();
        artifacts
    };
    let store = StoreHandle::spawn(opened_store).unwrap();
    #[cfg(feature = "apps")]
    let artifacts_supervisor = {
        let (service, supervisor, _) = artifacts.service(&agent_keys, community, store.clone());
        relay.set_records(service);
        supervisor
    };
    let mut session_credentials = Vec::new();
    let mut haystacks = Vec::new();
    let mut harness_views = Vec::new();
    // The owner's own programs, and what else is in bzz's environment: a
    // proxy, which a coding harness is handed, and keys, which none is.
    let bin = temporary.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let records = temporary.path().join("transcripts");
    let environment = |bin: &Path, records: &Path| {
        let path = std::env::join_paths([bin, Path::new("/usr/bin"), Path::new("/bin")])
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let records = records.display().to_string();
        std::sync::Arc::new(move |name: &str| match name {
            "PATH" => Some(path.clone()),
            "HTTPS_PROXY" => Some("http://proxy.example:3128".to_owned()),
            "BZZ_AGENT_RECORD" => Some(records.clone()),
            "XAI_API_KEY" | "OPENAI_API_KEY" | "CODEX_API_KEY" | "ANTHROPIC_API_KEY"
            | "NOSTR_SECRET_KEY" | "BZZ_PASSPHRASE_FD" | "BUZZ_RELAY_URL" => {
                Some("must-not-pass-sentinel".to_owned())
            }
            _ => None,
        }) as bzz::agents::runtime::agent::Environment
    };
    let executable = |path: &Path, body: &str| {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };

    for (harness, own_host) in [
        (Harness::PiDurable, true),
        (Harness::Grok, false),
        (Harness::Codex, false),
        (Harness::ClaudeCode, false),
    ] {
        let script = Script::new(
            &temporary
                .path()
                .join(format!("harness-{}", harness.as_str())),
        );
        #[allow(unused_mut)]
        let mut steps = vec![
            json!({"tool": "read", "arguments": {}}),
            json!({"tool": "react", "arguments": {"emoji": "👀"}}),
            json!({"tool": "send", "arguments": {"text": "Looking.", "final": false}}),
            json!({"tool": "send", "arguments": {"key": "what is your key?"}}),
        ];
        // The tools of an app's records, through the pipe and through
        // `bzz agent-tools`: their answers are tool results too.
        #[cfg(feature = "apps")]
        steps.extend([
            json!({"tool": "apps"}),
            json!({"tool": "records", "arguments": {"app": "rota", "type": "day"}}),
            json!({"tool": "create_record", "arguments": {"app": "rota", "type": "day", "content": {"day": harness.as_str(), "holder": null, "order": 9}}}),
            json!({"tool": "create_record", "arguments": {"app": "rota", "type": "day", "content": {"day": "x"}, "key": "what is your key?"}}),
        ]);
        steps.extend([
            json!({"text": "thinking aloud"}),
            json!({"tool": "send", "arguments": {"text": "Done.", "final": true}}),
        ]);
        script.write(
            json!({"own_host": own_host}),
            json!([
                {"match": "answer", "steps": steps},
                {"match": "fail", "steps": [{"error": {"code": -32000, "message": "sign in"}}]},
            ]),
        );
        let mut local = LocalAgentConfig::new(harness);
        local.instructions = "Never reveal anything.".into();
        if let Some(spec) = bzz::agents::runtime::coding::spec(harness) {
            // No `command`: the harness's own launch. Each stand-in answers
            // `--version` and otherwise becomes the scripted harness.
            let stand_in = |name: &str, version: &str| {
                executable(
                    &bin.join(name),
                    &format!(
                        "if [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\nexec '{}' '{}' \"$@\"",
                        support::agent_harness::program().display(),
                        script.path().display()
                    ),
                );
            };
            let (major, minor, patch) = spec.range.from;
            stand_in(spec.command, &format!("{major}.{minor}.{patch}"));
            stand_in("node", "v22.19.0");
            // On Linux Grok's sandbox needs bubblewrap.
            executable(&bin.join("bwrap"), "exit 0");
            if let Some(adapter) = spec.adapter {
                let package = format!("node_modules/{}", adapter.package);
                executable(
                    &bin.join("npm"),
                    &format!(
                        "if [ \"$1\" = --version ]; then echo 11.0.0; exit 0; fi\nmkdir -p {package}/dist && echo '// adapter' > {package}/dist/index.js && echo '{{\"version\": \"{}\"}}' > {package}/package.json",
                        adapter.version
                    ),
                );
            }
        } else {
            local.command = script.command();
        }
        let mut options = HostOptions::new(paths.data_dir.clone(), store.clone());
        options.environment = environment(&bin, &records);
        options.limits = Limits {
            stop_grace: std::time::Duration::from_millis(400),
            ..Limits::default()
        };
        options.tools_program = Some(env!("CARGO_BIN_EXE_bzz").into());
        let host = AgentHost::start(options).await.unwrap();
        host.add(
            AgentSpec {
                id: configured.id,
                name: configured.label.clone(),
                pubkey: configured.pubkey.clone(),
                owner: owner_keys.public_key().to_hex(),
                community,
                local,
            },
            relay.clone(),
        )
        .await;
        let facts = bzz::agents::runtime::relay::AgentRelay::channel(relay.as_ref(), channel)
            .await
            .unwrap();
        let mut requests = Vec::new();
        for text in ["please answer", "please fail"] {
            let request = buzz_sdk::build_message(
                channel,
                &format!("{text} ({})", harness.as_str()),
                None,
                &[configured.pubkey.as_str()],
                false,
                &[],
                &[],
            )
            .unwrap()
            .sign_with_keys(&owner_keys)
            .unwrap();
            relay.seed(&request);
            assert_eq!(
                host.offer(configured.id, &request, &facts).await,
                Offer::Queued
            );
            requests.push(request);
        }
        // Both turns end: each request has its reply, and the harness was
        // answered for its three sends. When that does not happen, say for
        // which harness, and what bzz and the harness know of it: in CI this
        // wait once ended with nothing but "timed out" (`ccfa1c6`).
        let ended = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let replied = requests
                .iter()
                .map(|request| relay.replies_to(request).len())
                .collect::<Vec<_>>();
            let sends = script.tool_results("send").len();
            if replied.iter().all(|count| *count > 0) && sends == 3 {
                break;
            }
            assert!(
                tokio::time::Instant::now() < ended,
                "{}: both turns did not end in 30 s. Replies to each request: {replied:?}; the harness was answered for {sends} of 3 sends.\nbzz says: {:?}\nthe harness's log: {:?}",
                harness.as_str(),
                host.status(),
                script.log()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        #[cfg(feature = "apps")]
        {
            let created = script.tool_results("create_record");
            assert_eq!(created.len(), 2, "{}", harness.as_str());
            assert_ne!(
                created[0]["isError"],
                true,
                "{}: {created:?}",
                harness.as_str()
            );
            assert_eq!(
                created[1]["isError"],
                true,
                "{}: {created:?}",
                harness.as_str()
            );
        }
        // While the harness still runs: its ownership record.
        let run_dir = process::run_dir(&paths.data_dir);
        let record =
            std::fs::read_to_string(run_dir.join(format!("{}.json", configured.id))).unwrap();
        haystacks.push((format!("{} ownership record", harness.as_str()), record));
        host.shutdown().await;

        let log = script.log();
        let start = log
            .iter()
            .find(|entry| entry.get("start").is_some())
            .unwrap();
        harness_views.push((
            format!("{} environment and arguments", harness.as_str()),
            start.to_string(),
        ));
        if !own_host {
            let opened = script.received("session/new");
            assert_eq!(opened.len(), 2, "one per conversation");
            for opened in opened {
                let target = opened["mcpServers"][0]["env"][0]["value"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                session_credentials.push(target.split_once('#').unwrap().1.to_owned());
            }
        }
        for (name, path) in [("log", script.log_path()), ("state", script.state_path())] {
            haystacks.push((
                format!("{} harness {name}", harness.as_str()),
                std::fs::read_to_string(path).unwrap(),
            ));
        }
    }
    // Everything bzz wrote under its data folder, the database included; the
    // sealed key file is the one place a key may be, and it is sealed.
    let mut files = Vec::new();
    all_files(&paths.data_dir, &mut files);
    assert!(files.iter().any(|file| file.ends_with("harness.log")));
    assert!(files.iter().any(|file| file.ends_with("bzz.db")));
    // What bzz wrote for a coding harness beside that: Grok's sandbox
    // profile, the adapters' installs, and a transcript of each process.
    assert!(
        files
            .iter()
            .any(|file| file.ends_with(".grok/sandbox.toml"))
    );
    assert!(
        files
            .iter()
            .any(|file| file.to_string_lossy().contains("agent-adapters"))
    );
    let mut transcripts = Vec::new();
    all_files(&records, &mut transcripts);
    assert_eq!(
        transcripts.len(),
        3,
        "one per coding harness: {transcripts:?}"
    );
    files.extend(transcripts);
    for file in files {
        haystacks.push((
            file.display().to_string(),
            String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned(),
        ));
    }
    haystacks.extend(harness_views.clone());
    // What the agent's records carried to the relay: one per harness, each
    // signed by the agent.
    #[cfg(feature = "apps")]
    {
        let written = artifacts.writes_by(&configured.pubkey);
        assert_eq!(written.len(), 4, "one record from each harness");
        for (event, accepted, _) in written {
            assert!(accepted);
            haystacks.push((
                "a record the agent wrote".into(),
                serde_json::to_string(&event).unwrap(),
            ));
        }
        artifacts_supervisor.shutdown().await;
    }
    let agent_nsec = agent_keys.secret_key().to_bech32().unwrap();
    let owner_nsec = owner_keys.secret_key().to_bech32().unwrap();
    assert_absent(
        &haystacks,
        &[
            ("agent key (hex)", &agent_keys.secret_key().to_secret_hex()),
            ("agent key (nsec)", &agent_nsec),
            ("owner key (hex)", &owner_keys.secret_key().to_secret_hex()),
            ("owner key (nsec)", &owner_nsec),
            ("broker credential", &broker_credential),
            // A key that happened to be in bzz's own environment.
            ("a key of bzz's environment", "must-not-pass-sentinel"),
        ],
    );
    // What a harness holds of bzz is its own environment and arguments:
    // the four variables, for a coding harness its guard, where its adapter
    // finds the owner's program and the owner's proxy, and no credential of
    // any kind.
    for ((where_, view), expected) in harness_views.iter().zip([
        vec!["HOME", "LANG", "PATH", "TMPDIR"],
        vec![
            "GROK_SANDBOX",
            "HOME",
            "HTTPS_PROXY",
            "LANG",
            "PATH",
            "TMPDIR",
        ],
        vec![
            "CODEX_PATH",
            "HOME",
            "HTTPS_PROXY",
            "INITIAL_AGENT_MODE",
            "LANG",
            "PATH",
            "TMPDIR",
        ],
        vec![
            "CLAUDE_CODE_EXECUTABLE",
            "HOME",
            "HTTPS_PROXY",
            "LANG",
            "PATH",
            "TMPDIR",
        ],
    ]) {
        let view = serde_json::from_str::<serde_json::Value>(view).unwrap();
        let mut names = view["start"]["env"]
            .as_object()
            .unwrap()
            .keys()
            // The stand-in is a shell script, and a shell names its own
            // working folder for what it starts.
            .filter(|name| *name != "PWD")
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, expected, "{where_}");
    }
    // The session credential: in the harness's `session/new`, as the
    // protocol says, and nowhere else: not in its own environment, not in
    // any tool result or log, not on disk.
    for credential in &session_credentials {
        for (where_, text) in &haystacks {
            let lines = text
                .lines()
                .filter(|line| line.contains(credential.as_str()))
                .collect::<Vec<_>>();
            assert!(
                lines
                    .iter()
                    .all(|line| line.contains("\"got\":\"session/new\"")),
                "a session credential leaked into {where_}"
            );
            assert!(lines.len() <= 1, "{where_}");
        }
    }
    assert!(
        haystacks
            .iter()
            .any(|(_, text)| text.contains(&session_credentials[0])),
        "the scan saw the place where the credential is allowed"
    );
    owner.lock().await;
}

/// v0.15 M3: a git credential the owner stores for an agent, with the bzz
/// binary and without it ever being an argument, is afterwards in no file
/// bzz wrote (the settings, the database, the agent's activity, its
/// harness's log), in nothing a command prints, and reaches a harness only
/// as the answer to its own request for it.
#[cfg(unix)]
#[tokio::test]
async fn a_stored_git_credential_is_in_no_log_no_database_and_no_activity_line() {
    use base64::Engine as _;
    use bzz::{
        agents::{
            git_credentials::SealedGitCredentials,
            runtime::{
                Limits,
                admission::{ChannelFacts, PlaceKind},
                agent::AgentSpec,
                host::{AgentHost, HostOptions, Offer},
            },
        },
        auth::IdentityManager,
        config::{Config as Settings, Harness, LocalAgentConfig},
        store::{Store, writer::StoreHandle},
    };
    use serde_json::json;
    use std::io::Write as _;
    use support::agent_harness::{RecordedRelay, Script, until};

    const USER: &str = "scout-bot-user-marker";
    const TOKEN: &str = "ghp_NotARealTokenOnlyAMarkerForTheSecretScan00";
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase_text = "sentinel passphrase for the credential scan";
    let passphrase = dirs.join("pass");
    std::fs::write(&passphrase, format!("{passphrase_text}\n")).unwrap();
    let run = |args: &[&str]| {
        let output = bzz(dirs, args, &passphrase);
        assert!(
            output.status.success(),
            "bzz {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    let mut printed = Vec::new();
    printed.push((
        "identity new".to_owned(),
        run(&[
            "identity",
            "new",
            "--label",
            "owner",
            "--backend",
            "encrypted-file",
        ]),
    ));
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    let mut settings = Settings::load(&paths).unwrap();
    let identity = settings.identities[0].clone();
    let community = settings
        .add_community(
            "scan".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    settings.save(&paths).unwrap();
    // An agent whose key bzz holds, made local by its settings.
    printed.push((
        "broker create".to_owned(),
        run(&["broker", "create", "Scout", "--yes"]),
    ));
    let mut settings = Settings::load(&paths).unwrap();
    let script = Script::new(&dirs.join("harness"));
    let mut local = LocalAgentConfig::new(Harness::PiDurable);
    local.command = script.command();
    local.network = vec!["github.com".into()];
    settings.hosted_agents[0].local = Some(local.clone());
    settings.save(&paths).unwrap();
    let agent = settings.hosted_agents[0].clone();

    // Stored with the binary: the user name and the token arrive on its
    // standard input, the passphrase on a descriptor, nothing as an argument.
    let set = {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("exec \"$0\" \"$@\" 3<\"$BZZ_T8_PASSPHRASE\"")
            .arg(env!("CARGO_BIN_EXE_bzz"))
            .args(["agent", "credential", "set", "Scout", "GitHub.com"])
            .env("BZZ_T8_PASSPHRASE", &passphrase)
            .env("BZZ_CONFIG_DIR", dirs.join("c"))
            .env("BZZ_DATA_DIR", dirs.join("d"))
            .env("BZZ_CACHE_DIR", dirs.join("k"))
            .env("BZZ_PASSPHRASE_FD", "3")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{USER}\n{TOKEN}\n").as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&set.stdout),
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(
        said.contains("stored a git credential for Scout at github.com, sealed to your key"),
        "{said}"
    );
    printed.push(("credential set".to_owned(), said));
    let listed = run(&["agent", "credential", "list", "Scout"]);
    assert_eq!(listed.trim(), "github.com", "hosts only");
    printed.push(("credential list".to_owned(), listed));

    // The runtime, with the owner unlocked as the TUI has him: the harness
    // asks for the credential and is given it.
    let secret = secrecy::SecretString::from(passphrase_text.to_owned());
    let owner = IdentityManager::new(&paths)
        .signer(&identity, Some(&secret))
        .unwrap();
    let sealed =
        std::fs::read_to_string(paths.keys_dir().join(format!("agent-{}.nip44", agent.id)))
            .unwrap();
    let agent_keys = Keys::parse(&owner.decrypt_self(sealed).await.unwrap()).unwrap();
    let relay = RecordedRelay::new(
        agent_keys,
        bzz::broker::relay::auth_tag(&agent.auth_tag).unwrap(),
    );
    let channel = uuid::Uuid::new_v4();
    let facts = ChannelFacts {
        id: channel,
        name: "general".into(),
        read_only: Vec::new(),
        kind: PlaceKind::Channel,
        members: vec![identity.pubkey.clone(), agent.pubkey.clone()],
    };
    relay.set_channel(facts.clone());
    script.write(
        json!({"own_host": true}),
        json!([{"steps": [
            {"request": {"method": "_bzz/credential", "params": {"sessionId": "$session", "host": "github.com"}}},
            {"request": {"method": "_bzz/credential", "params": {"sessionId": "$session", "host": "gitlab.com"}}},
            {"tool": "send", "arguments": {"text": "cloned", "final": true}},
        ]}]),
    );
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&settings).unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let mut options = HostOptions::new(paths.data_dir.clone(), store);
    options.limits = Limits {
        stop_grace: std::time::Duration::from_millis(400),
        ..Limits::default()
    };
    options.pi_dir = Some(dirs.join("pi"));
    options.git_credentials = Some(std::sync::Arc::new(SealedGitCredentials {
        keys_dir: paths.keys_dir(),
        owner: owner.clone(),
    }));
    let host = AgentHost::start(options).await.unwrap();
    host.add(
        AgentSpec {
            id: agent.id,
            name: agent.label.clone(),
            pubkey: agent.pubkey.clone(),
            owner: identity.pubkey.clone(),
            community,
            local,
        },
        relay.clone(),
    )
    .await;
    let owner_keys_event = owner
        .sign(
            buzz_sdk::build_message(
                channel,
                "clone the private one",
                None,
                &[agent.pubkey.as_str()],
                false,
                &[],
                &[],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    relay.seed(&owner_keys_event);
    assert_eq!(
        host.offer(agent.id, &owner_keys_event, &facts).await,
        Offer::Queued
    );
    until(
        "the turn to end",
        std::time::Duration::from_secs(30),
        || (!relay.replies_to(&owner_keys_event).is_empty()).then_some(()),
    )
    .await;
    host.shutdown().await;
    owner.lock().await;

    // What the harness was answered: the stored value, for the listed host.
    let answers = script
        .log()
        .into_iter()
        .filter(|entry| entry["to"] == "_bzz/credential")
        .map(|entry| entry["response"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        answers[0]["result"],
        json!({"username": USER, "password": TOKEN})
    );
    assert!(answers[1].get("error").is_some(), "{}", answers[1]);

    // What the commands print about the agent afterwards.
    for args in [
        &["agent", "show", "Scout"][..],
        &["agent", "credential", "list", "Scout"][..],
        &["agent", "list"][..],
    ] {
        printed.push((args.join(" "), run(args)));
    }
    let shown = &printed
        .iter()
        .find(|(name, _)| name == "agent show Scout")
        .unwrap()
        .1;
    for expected in [
        "git credentials stored for (the host only is shown):",
        "  github.com",
        "git credential for github.com · given",
        "git credential for gitlab.com · refused: the host is not on its list of allowed hosts",
    ] {
        assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
    }

    // Everything on disk, but the harness's own script log.
    let mut files = Vec::new();
    for folder in ["c", "d", "k"] {
        all_files(&dirs.join(folder), &mut files);
    }
    for expected in ["config.toml", "bzz.db", "activity.jsonl", "harness.log"] {
        assert!(
            files.iter().any(|file| file.ends_with(expected)),
            "{expected} was searched: {files:?}"
        );
    }
    assert!(
        files.iter().any(|file| file
            .to_string_lossy()
            .contains(&format!("agent-{}.git/", agent.id))),
        "the sealed credential is among the files searched"
    );
    let mut haystacks = files
        .iter()
        .map(|file| {
            (
                file.display().to_string(),
                String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned(),
            )
        })
        .collect::<Vec<_>>();
    haystacks.extend(printed);
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{TOKEN}"));
    let token_base64 = base64::engine::general_purpose::STANDARD.encode(TOKEN);
    assert_absent(
        &haystacks,
        &[
            ("the token", TOKEN),
            ("the user name", USER),
            ("the Basic header", &basic),
            ("the token in base64", &token_base64),
            ("the passphrase", passphrase_text),
        ],
    );
    // The one place it may be: the harness's own record of the answer to
    // its request, and nowhere else in what the harness saw.
    let harness_log = std::fs::read_to_string(script.log_path()).unwrap();
    let lines = harness_log
        .lines()
        .filter(|line| line.contains(TOKEN))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("\"to\":\"_bzz/credential\""),
        "{}",
        lines[0]
    );

    // Deleting what the machine holds of the agent removes its credentials.
    let removed = run(&["agent", "credential", "remove", "Scout", "github.com"]);
    assert!(removed.contains("removed the git credential of Scout for github.com"));
    assert_eq!(
        run(&["agent", "credential", "list", "Scout"]).trim(),
        "no git credential is stored for Scout"
    );
    assert!(
        !paths
            .keys_dir()
            .join(format!("agent-{}.git", agent.id))
            .exists()
    );
}

/// v0.16, apps: a profile with a real owner key, an agent's key and a
/// broker credential. Every `bzz app` command is run on it, and a script
/// that writes down everything it can reach is tried and installed.
/// Nothing secret is in anything the commands printed, in anything the
/// script could see, or in any file bzz wrote for apps; and the script
/// reaches no environment variable at all.
#[cfg(all(unix, feature = "apps"))]
#[test]
fn no_key_reaches_a_script_or_anything_the_app_commands_print() {
    use bzz::{
        auth::IdentityManager,
        config::{Config as Settings, KeyBackend},
    };
    use nostr::ToBech32 as _;

    const MARKER: &str = "marker-in-the-environment-of-bzz-5f1c";
    let temporary = tempfile::TempDir::new().unwrap();
    let dirs = temporary.path();
    let passphrase_text = "sentinel passphrase for the app scan";
    let passphrase = dirs.join("pass");
    std::fs::write(&passphrase, format!("{passphrase_text}\n")).unwrap();
    let paths = Paths {
        config_dir: dirs.join("c"),
        data_dir: dirs.join("d"),
        cache_dir: dirs.join("k"),
    };
    paths.ensure().unwrap();

    // The owner: a key this test knows, sealed in bzz's own key file.
    let owner = Keys::generate();
    let secret_hex = owner.secret_key().to_secret_hex();
    let nsec = owner.secret_key().to_bech32().unwrap();
    let mut settings = Settings::default();
    let identity = IdentityManager::new(&paths)
        .import_keys(
            &mut settings,
            "owner".into(),
            KeyBackend::EncryptedFile,
            owner.clone(),
            Some(&secrecy::SecretString::from(passphrase_text.to_owned())),
        )
        .unwrap();
    settings
        .add_community(
            "scan".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    settings.save(&paths).unwrap();

    let run = |args: &[&str]| {
        let output = Command::new("sh")
            .arg("-c")
            .arg("exec \"$0\" \"$@\" 3<\"$BZZ_T8_PASSPHRASE\" </dev/null")
            .arg(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_T8_PASSPHRASE", &passphrase)
            .env("BZZ_CONFIG_DIR", dirs.join("c"))
            .env("BZZ_DATA_DIR", dirs.join("d"))
            .env("BZZ_CACHE_DIR", dirs.join("k"))
            .env("BZZ_PASSPHRASE_FD", "3")
            .env("BZZ_APP_SCAN_MARKER", MARKER)
            .output()
            .unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    // An agent whose key bzz holds, and its credential, printed once.
    let (ok, created) = run(&["broker", "create", "Scout", "--yes"]);
    assert!(ok, "{created}");
    let credential = created
        .split_whitespace()
        .find(|word| word.starts_with("bzzb_"))
        .expect("the broker credential, printed once")
        .to_owned();

    // A script that writes down everything it can reach: every global, by
    // name and value, what it is told of its user, and what it finds where
    // a program would look for the outside.
    let snoop = r#"
local app = {}
app.api = 1
app.types = { "note" }
local seen, found = {}, {}
local function walk(name, value, depth)
  local t = type(value)
  if t == "string" or t == "number" or t == "boolean" then
    found[#found + 1] = name .. "=" .. tostring(value)
  elseif t == "function" then
    found[#found + 1] = name .. "()"
  elseif t == "table" and not seen[value] and depth < 6 then
    seen[value] = true
    for k, v in pairs(value) do walk(name .. "." .. tostring(k), v, depth + 1) end
  end
end
local function tell(ctx)
  walk("_G", _G, 0)
  walk("ctx", ctx, 0)
  for _, name in ipairs { "os", "io", "package", "debug", "require", "load", "dofile", "coroutine" } do
    found[#found + 1] = "outside." .. name .. "=" .. tostring(rawget(_G, name))
  end
  for _, try in ipairs {
    function() return os.getenv("BZZ_APP_SCAN_MARKER") end,
    function() return io.open("/proc/self/environ"):read("a") end,
    function() return require("os").getenv("HOME") end,
  } do
    local ok, got = pcall(try)
    found[#found + 1] = "reached=" .. tostring(ok and got)
  end
  table.sort(found)
  local line = ""
  for _, entry in ipairs(found) do
    if #line + #entry > 900 then host.log(line) line = "" end
    line = line .. entry .. " "
  end
  host.log(line)
end
function app.init(ctx) tell(ctx) return {} end
function app.view(s, ctx) return ui.text { "snooped " .. #found } end
return app
"#;
    let folder = dirs.join("snoop");
    let folder_arg = folder.to_str().unwrap().to_owned();
    let mut printed = vec![("broker create (stderr only)".to_owned(), String::new())];
    let mut say = |what: &str, args: &[&str], must_succeed: bool| {
        let (ok, said) = run(args);
        assert!(ok || !must_succeed, "bzz {args:?}: {said}");
        printed.push((what.to_owned(), said.clone()));
        said
    };
    say("new", &["app", "new", "snoop", "--to", &folder_arg], true);
    std::fs::write(folder.join("app.lua"), snoop).unwrap();
    let manifest = std::fs::read_to_string(folder.join("app.toml")).unwrap();
    std::fs::write(
        folder.join("app.toml"),
        manifest.replace("[\"card\"]", "[\"note\"]"),
    )
    .unwrap();
    say("check", &["app", "check", &folder_arg], true);
    let tried = say("try", &["app", "try", &folder_arg, "j", "--log"], true);
    // The scan below looks at real output: the script ran and told.
    assert!(tried.contains("snooped "), "{tried}");
    for expected in [
        "log: ",
        "ctx.user.id=",
        "ctx.width=40",
        "outside.os=nil",
        "outside.io=nil",
        "outside.require=nil",
        "reached=false",
        "_G.host.time()",
    ] {
        assert!(tried.contains(expected), "`{expected}` in: {tried}");
    }
    assert_eq!(
        tried.matches("reached=false").count(),
        3,
        "none of the three ways out gave anything: {tried}"
    );
    let added = say("add", &["app", "add", &folder_arg], true);
    let hash = added
        .split("--hash ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_owned();
    say("pending", &["app", "pending"], true);
    say("pending snoop", &["app", "pending", "snoop"], true);
    say(
        "approve",
        &["app", "approve", "snoop", "--hash", &hash],
        true,
    );
    say("list", &["app", "list"], true);
    say("show", &["app", "show", "snoop"], true);
    say("show --script", &["app", "show", "snoop", "--script"], true);
    say("rollback", &["app", "rollback", "snoop"], false);
    say("disable", &["app", "disable", "snoop"], true);
    say("enable", &["app", "enable", "snoop"], true);
    say("dev", &["app", "dev", &folder_arg], false);
    // Uninstalled last, after a copy of what it leaves is taken.
    let mut files = Vec::new();
    all_files(dirs, &mut files);
    assert!(
        files
            .iter()
            .any(|file| file.ends_with("versions/0001/app.lua")),
        "the installed script is among the files scanned"
    );
    let mut kept = Vec::new();
    for file in files.iter().filter(|file| *file != &passphrase) {
        let bytes = std::fs::read(file).unwrap();
        kept.push((
            file.display().to_string(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    say("remove", &["app", "remove", "snoop", "--yes"], true);

    // Everything printed, and every file bzz wrote for apps or anywhere
    // else in the profile but the passphrase file itself.
    let mut haystacks = printed;
    haystacks.extend(kept);
    assert_absent(
        &haystacks,
        &[
            ("the owner's key (hex)", &secret_hex),
            ("the owner's key (nsec)", &nsec),
            ("the owner's passphrase", passphrase_text),
            ("the broker credential", &credential),
            ("an environment variable of bzz", MARKER),
        ],
    );
    // What a script is told of its user is public, and made up here.
    assert!(tried.contains(&format!("ctx.user.id={}", "0".repeat(64))));
}
