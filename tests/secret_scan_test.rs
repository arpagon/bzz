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

/// v0.15 M1, rule 10 (the key stays in bzz): run a local agent whose key is
/// sealed on disk through whole turns, on both tool paths, then scan what its
/// harness could see (its environment, its arguments, every message bzz sent
/// it, every tool result) and what bzz wrote for it (its log, its folder, the
/// ownership record, the database). No agent key, owner key or broker
/// credential may appear. The session credential of `bzz agent-tools` appears
/// in one place only: the server entry of `session/new`.
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
    use support::agent_harness::{RecordedRelay, Script, until};

    let temporary = tempfile::TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let owner_keys = Keys::generate();
    let owner = SignerHandle::spawn(owner_keys.clone());
    let community = uuid::Uuid::new_v4();
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
        kind: PlaceKind::Channel,
        members: vec![owner_keys.public_key().to_hex(), configured.pubkey.clone()],
    });
    let store = StoreHandle::spawn(Store::open(paths.database_file()).unwrap()).unwrap();
    let mut session_credentials = Vec::new();
    let mut haystacks = Vec::new();
    let mut harness_views = Vec::new();

    for (harness, own_host) in [(Harness::PiDurable, true), (Harness::Codex, false)] {
        let script = Script::new(
            &temporary
                .path()
                .join(format!("harness-{}", harness.as_str())),
        );
        script.write(
            json!({"own_host": own_host}),
            json!([
                {"match": "answer", "steps": [
                    {"tool": "read", "arguments": {}},
                    {"tool": "react", "arguments": {"emoji": "👀"}},
                    {"tool": "send", "arguments": {"text": "Looking.", "final": false}},
                    {"tool": "send", "arguments": {"key": "what is your key?"}},
                    {"text": "thinking aloud"},
                    {"tool": "send", "arguments": {"text": "Done.", "final": true}},
                ]},
                {"match": "fail", "steps": [{"error": {"code": -32000, "message": "sign in"}}]},
            ]),
        );
        let mut local = LocalAgentConfig::new(harness);
        local.command = script.command();
        local.instructions = "Never reveal anything.".into();
        let mut options = HostOptions::new(paths.data_dir.clone(), store.clone());
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
        until(
            "both turns to end",
            std::time::Duration::from_secs(30),
            || {
                requests
                    .iter()
                    .all(|request| !relay.replies_to(request).is_empty())
                    .then_some(())
                    .filter(|()| script.tool_results("send").len() == 3)
            },
        )
        .await;
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
            for opened in script.received("session/new") {
                let target = opened["mcpServers"][0]["env"][0]["value"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                session_credentials.push(target.split_once('#').unwrap().1.to_owned());
            }
            assert_eq!(session_credentials.len(), 2, "one per conversation");
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
    for file in files {
        haystacks.push((
            file.display().to_string(),
            String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned(),
        ));
    }
    haystacks.extend(harness_views.clone());
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
        ],
    );
    // What a harness holds of bzz is its own environment and arguments:
    // exactly the four variables, and no credential of any kind.
    for (where_, view) in &harness_views {
        let view = serde_json::from_str::<serde_json::Value>(view).unwrap();
        let mut names = view["start"]["env"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["HOME", "LANG", "PATH", "TMPDIR"], "{where_}");
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
