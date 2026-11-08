//! v0.13 M3 against an FI-enforcing pinned relay (T4): `scripts/test-relay.sh
//! --nip-fi` starts the fake NIP-FA issuer on localhost:3093 and the relay in
//! `BUZZ_NIP_FI_MODE=enforce` with a 20-second connection lifetime.
mod support;

use std::{process::Command, sync::Arc, time::Duration};

use bzz::{
    auth::{
        federated::{AdapterClient, AdapterEvidence, EvidenceSlot, RelayDenial},
        signer::SignerHandle,
    },
    clock,
    diagnostics::DiagnosticHandle,
    error::Error,
    protocol::{http::HttpClient, types::QueryFilter},
    realtime::{
        session::{self, SessionEvent},
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
};
use nostr::Keys;
use support::fake_adapter::FakeAdapter;

const RELAY_WS: &str = "ws://localhost:3030/";
const RELAY_HTTP: &str = "http://localhost:3030/";
const ADAPTER: &str = "http://localhost:3093/";

fn seed_member(pubkey: &str) {
    let source = std::env::var("BZZ_BUZZ_SOURCE").expect("relay wrapper sets source");
    let sql = format!(
        "INSERT INTO relay_members (community_id,pubkey,role,added_by) SELECT id,'{pubkey}','owner',NULL FROM communities WHERE lower(host)='localhost:3030' ON CONFLICT (community_id,pubkey) DO UPDATE SET role='owner',updated_at=now();"
    );
    let status = Command::new("docker")
        .args([
            "compose",
            "-p",
            "buzz-harness",
            "-f",
            &format!("{source}/docker-compose.harness.yml"),
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
        .unwrap();
    assert!(status.success());
}

async fn signed_in_slot(signer: &SignerHandle) -> EvidenceSlot {
    let adapter = AdapterClient::new(url::Url::parse(ADAPTER).unwrap()).unwrap();
    let pending = adapter.begin_login().await.unwrap();
    let login = pending.url.clone();
    let browser = tokio::spawn(async move { FakeAdapter::browse(&login).await });
    let session = adapter
        .complete_login(pending, Duration::from_secs(10))
        .await
        .unwrap();
    browser.await.unwrap();
    let slot = EvidenceSlot::default();
    slot.set(AdapterEvidence::new(adapter, session, signer.clone(), clock::system()) as Arc<_>);
    slot
}

#[tokio::test]
#[ignore = "requires scripts/test-relay.sh --nip-fi"]
async fn fi_relay_denies_without_evidence_and_admits_with_nip_fa_evidence() {
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    seed_member(&pubkey);
    let signer = SignerHandle::spawn(keys);
    let http_info = HttpClient::new(url::Url::parse(RELAY_HTTP).unwrap(), signer.clone()).unwrap();
    let info = http_info.nip11().await.unwrap();
    assert_eq!(
        bzz::auth::federated::requirement(&info),
        bzz::auth::federated::Requirement::Required
    );

    // No evidence: typed, text-free denials on both transports.
    let error = session::connect(url::Url::parse(RELAY_WS).unwrap(), signer.clone())
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&error, Error::Auth(message) if message == RelayDenial::EvidenceRequired.notice()),
        "{error:?}"
    );
    let error = http_info
        .query(&[QueryFilter {
            kinds: vec![9],
            limit: Some(1),
            ..QueryFilter::default()
        }])
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Auth(message) if message == RelayDenial::EvidenceRequired.notice()),
        "{error:?}"
    );

    // NIP-FA sign-in; evidence on the upgrade, HTTP bridge and Blossom.
    let slot = signed_in_slot(&signer).await;
    let header = slot
        .header(&url::Url::parse(RELAY_WS).unwrap())
        .await
        .unwrap();
    let (handle, mut events) = session::connect_with_evidence(
        url::Url::parse(RELAY_WS).unwrap(),
        signer.clone(),
        DiagnosticHandle::disabled(),
        header,
    )
    .await
    .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    handle.shutdown().await;
    let http = HttpClient::new(url::Url::parse(RELAY_HTTP).unwrap(), signer.clone())
        .unwrap()
        .with_evidence(slot.clone());
    http.query(&[QueryFilter {
        kinds: vec![9],
        limit: Some(1),
        ..QueryFilter::default()
    }])
    .await
    .unwrap();
    let media = bzz::media::client::MediaClient::new(
        url::Url::parse(RELAY_HTTP).unwrap(),
        "localhost:3030".into(),
        signer.clone(),
        1,
    )
    .unwrap()
    .with_evidence(slot.clone());
    let directory = tempfile::TempDir::new().unwrap();
    let png = directory.path().join("fi.png");
    let mut bytes = Vec::new();
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    std::fs::write(&png, &bytes).unwrap();
    let uploaded = media
        .upload(&png, "image/png", Some("fi.png".into()))
        .await
        .unwrap();
    assert_eq!(uploaded.size, bytes.len() as u64);
    signer.lock().await;
}

#[tokio::test]
#[ignore = "requires scripts/test-relay.sh --nip-fi"]
async fn fi_session_deadline_reconnects_with_fresh_evidence() {
    let keys = Keys::generate();
    seed_member(&keys.public_key().to_hex());
    let signer = SignerHandle::spawn(keys);
    let slot = signed_in_slot(&signer).await;
    let supervisor = SupervisorHandle::spawn_with_evidence(
        url::Url::parse(RELAY_WS).unwrap(),
        signer.clone(),
        DiagnosticHandle::disabled(),
        slot,
    );
    let mut events = supervisor.subscribe_events();
    supervisor
        .subscribe("deadline", vec![serde_json::json!({"kinds":[9],"limit":1})])
        .await
        .unwrap();
    // The relay ends every FI session at its 20 s lifetime; the supervisor
    // must come back online on its own, never terminally.
    let mut authenticated = 0;
    let mut disconnected = false;
    tokio::time::timeout(Duration::from_secs(60), async {
        while authenticated < 2 {
            match events.recv().await.unwrap() {
                SupervisorEvent::Session(SessionEvent::Authenticated) => authenticated += 1,
                SupervisorEvent::Session(SessionEvent::Disconnected(_)) => disconnected = true,
                SupervisorEvent::Terminal(message) => panic!("deadline became terminal: {message}"),
                _ => {}
            }
        }
    })
    .await
    .expect("reconnected after the FI session deadline");
    assert!(disconnected);
    supervisor.shutdown().await;
    signer.lock().await;
}
