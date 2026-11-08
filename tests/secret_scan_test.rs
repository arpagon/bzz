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
