//! v0.13 M3 against the fake NIP-FA adapter (T3).
mod support;

use std::time::Duration;

use bzz::{
    auth::{
        federated::{
            AdapterClient, AdapterError, AdapterEvidence, Denial, EvidenceSource as _,
            RefusalReason, SessionStore,
        },
        signer::SignerHandle,
    },
    clock::{self, ManualClock, WallClock as _},
};
use nostr::Keys;
use secrecy::ExposeSecret as _;
use support::fake_adapter::{Config, FakeAdapter};

async fn sign_in(adapter: &FakeAdapter) -> bzz::auth::federated::Session {
    let client = AdapterClient::new(adapter.base()).unwrap();
    let pending = client.begin_login().await.unwrap();
    let login = pending.url.clone();
    assert_eq!(login.host_str(), adapter.base().host_str());
    let pairs: std::collections::HashMap<_, _> = login.query_pairs().into_owned().collect();
    assert_eq!(pairs["handoff_challenge_method"], "S256");
    assert!(pairs["return_to"].starts_with("http://127.0.0.1:"));
    let browser = tokio::spawn(async move { FakeAdapter::browse(&login).await });
    let session = client
        .complete_login(pending, Duration::from_secs(10))
        .await
        .unwrap();
    browser.await.unwrap();
    session
}

#[tokio::test]
async fn browser_login_exchanges_once_and_verifies_the_same_session() {
    let adapter = FakeAdapter::start(Config::default()).await;
    let session = sign_in(&adapter).await;
    assert!(!session.token.expose_secret().is_empty());
    let paths = adapter
        .requests()
        .into_iter()
        .map(|request| request.path)
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec!["/v1/login/start", "/v1/login/exchange", "/v1/session"]
    );
    // The handoff secret travels only in the exchange body, never the URL.
    let start = &adapter.requests()[0];
    let exchange = &adapter.requests()[1];
    let secret = exchange.json()["handoff_secret"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!start.query.contains(&secret));
}

#[tokio::test]
async fn mismatched_session_check_rejects_the_login() {
    let adapter = FakeAdapter::start(Config {
        mismatched_session_check: true,
        ..Config::default()
    })
    .await;
    let client = AdapterClient::new(adapter.base()).unwrap();
    let pending = client.begin_login().await.unwrap();
    let login = pending.url.clone();
    tokio::spawn(async move { FakeAdapter::browse(&login).await });
    let error = client
        .complete_login(pending, Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("did not match"), "{error}");
}

#[tokio::test]
async fn assertion_is_bound_to_key_and_canonical_relay_then_cached_and_reminted() {
    let adapter = FakeAdapter::start(Config {
        assertion_ttl: 120,
        ..Config::default()
    })
    .await;
    let session = sign_in(&adapter).await;
    let keys = Keys::generate();
    let signer = SignerHandle::spawn(keys.clone());
    let clock = ManualClock::new(clock::SystemClock.now_unix());
    let evidence = AdapterEvidence::new(
        AdapterClient::new(adapter.base()).unwrap(),
        session,
        signer.clone(),
        clock.clone(),
    );
    let relay = url::Url::parse("wss://Community.Example:8443/ignored/path").unwrap();
    let first = evidence.evidence(&relay).await.unwrap().unwrap();
    assert!(first.starts_with("Bearer ey"));
    let claims = &adapter.issued()[0];
    assert_eq!(claims["nostr_pubkey"], keys.public_key().to_hex());
    assert_eq!(claims["aud"], "https://community.example:8443");
    let request = adapter
        .requests()
        .into_iter()
        .find(|request| request.path == "/v1/identity/assertions")
        .unwrap();
    assert_eq!(request.json()["relay_url"], "wss://community.example:8443");
    assert!(
        request
            .header("authorization")
            .unwrap()
            .starts_with("Bearer ")
    );
    assert!(
        request
            .header("nostr-authorization")
            .unwrap()
            .starts_with("Nostr ")
    );

    // Cached while fresh, re-minted inside the margin.
    assert_eq!(evidence.evidence(&relay).await.unwrap().unwrap(), first);
    assert_eq!(adapter.issued().len(), 1);
    // The adapter stamps `exp` from the real clock; allow a few seconds of
    // real time between creating the manual clock and minting (still well
    // before expiry), so a loaded machine does not miss the boundary.
    clock.advance(120 - bzz::auth::federated::REMINT_MARGIN_SECONDS + 5);
    evidence.evidence(&relay).await.unwrap();
    assert_eq!(adapter.issued().len(), 2, "re-mint before expiry");
    signer.lock().await;
}

#[tokio::test]
async fn denials_follow_the_nip_fa_client_actions() {
    let adapter = FakeAdapter::start(Config::default()).await;
    let session = sign_in(&adapter).await;
    let signer = SignerHandle::spawn(Keys::generate());
    let client = AdapterClient::new(adapter.base()).unwrap();
    let relay = url::Url::parse("wss://community.example").unwrap();
    for (status, code, expected) in [
        (
            403,
            "authorization_denied",
            Denial::Refused(RefusalReason::AuthorizationDenied),
        ),
        (
            403,
            "binding_mismatch",
            Denial::Refused(RefusalReason::BindingMismatch),
        ),
        (
            403,
            "invalid_proof",
            Denial::Refused(RefusalReason::InvalidProof),
        ),
        (429, "rate_limited", Denial::Retry { after: None }),
        (503, "issuance_unavailable", Denial::Retry { after: None }),
        (418, "teapot", Denial::Refused(RefusalReason::Undefined)),
    ] {
        adapter.deny("/v1/identity/assertions", status, code);
        let error = client
            .assertion(&session, &relay, &signer)
            .await
            .unwrap_err();
        assert_eq!(error, AdapterError::Denied(expected), "{status} {code}");
    }
    adapter.clear_denials();
    // A revoked session clears the session in the evidence source.
    adapter.revoke_sessions();
    let evidence = AdapterEvidence::new(client, session, signer.clone(), clock::system());
    let error = evidence.evidence(&relay).await.unwrap_err();
    assert!(error.to_string().contains("session ended"), "{error}");
    assert!(!evidence.has_session());
    signer.lock().await;
}

#[tokio::test]
async fn session_store_round_trips_encrypted_and_scoped_to_origin() {
    let adapter = FakeAdapter::start(Config::default()).await;
    let session = sign_in(&adapter).await;
    let directory = tempfile::TempDir::new().unwrap();
    let store = SessionStore::new(directory.path());
    let signer = SignerHandle::spawn(Keys::generate());
    let identity = uuid::Uuid::new_v4();
    let origin = AdapterClient::new(adapter.base()).unwrap().origin();
    store
        .save(identity, &origin, &session, &signer)
        .await
        .unwrap();
    let file = std::fs::read_dir(directory.path().join("federated"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read_to_string(&file).unwrap();
    assert!(
        !bytes.contains(session.token.expose_secret()),
        "session stored in plaintext"
    );
    let loaded = store
        .load(identity, &origin, &signer, &clock::SystemClock)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.token.expose_secret(), session.token.expose_secret());
    assert!(
        store
            .load(
                identity,
                "https://other.example",
                &signer,
                &clock::SystemClock
            )
            .await
            .unwrap()
            .is_none()
    );
    let other_signer = SignerHandle::spawn(Keys::generate());
    assert!(
        store
            .load(identity, &origin, &other_signer, &clock::SystemClock)
            .await
            .unwrap()
            .is_none(),
        "another identity cannot read it"
    );
    store.clear(identity, &origin).unwrap();
    assert!(
        store
            .load(identity, &origin, &signer, &clock::SystemClock)
            .await
            .unwrap()
            .is_none()
    );
    signer.lock().await;
    other_signer.lock().await;
}

#[tokio::test]
async fn agent_grant_mints_owner_bound_agent_assertions_and_revocation_discards_it() {
    use bzz::auth::federated::{AgentEvidence, EvidenceSource as _};
    let adapter = FakeAdapter::start(Config::default()).await;
    let session = sign_in(&adapter).await;
    let owner_keys = Keys::generate();
    let owner = SignerHandle::spawn(owner_keys.clone());
    let client = AdapterClient::new(adapter.base()).unwrap();
    // The owner's own assertion binds the session to the owner key.
    let relay = url::Url::parse("wss://community.example").unwrap();
    client.assertion(&session, &relay, &owner).await.unwrap();

    let agent_keys = Keys::generate();
    let agent = SignerHandle::spawn(agent_keys.clone());
    let tag = owner
        .owner_auth_tag(agent_keys.public_key(), "")
        .await
        .unwrap();
    let owner_hex = owner_keys.public_key().to_hex();
    let grant = client
        .agent_grant(&session, &agent, &tag, &owner_hex)
        .await
        .unwrap();
    assert!(
        !format!("{grant:?}").contains("grant-"),
        "the grant never prints"
    );
    let request = adapter
        .requests()
        .into_iter()
        .find(|request| request.path == "/v1/nip-fi/agent-grant")
        .unwrap();
    assert!(
        request.header("authorization").is_none(),
        "session only as subject_token"
    );

    let clock = ManualClock::new(clock::SystemClock.now_unix());
    let evidence = AgentEvidence::new(
        AdapterClient::new(adapter.base()).unwrap(),
        grant,
        agent.clone(),
        clock.clone(),
    );
    let header = evidence.evidence(&relay).await.unwrap().unwrap();
    assert!(header.starts_with("Bearer ey"));
    let claims = adapter.issued().last().cloned().unwrap();
    assert_eq!(claims["nostr_pubkey"], agent_keys.public_key().to_hex());
    assert_eq!(claims["nostr_owner_pubkey"], owner_hex);

    // An attestation from a different owner is refused for this session.
    let stranger = SignerHandle::spawn(Keys::generate());
    let other = Keys::generate();
    let foreign = stranger
        .owner_auth_tag(other.public_key(), "")
        .await
        .unwrap();
    let refused = client
        .agent_grant(&session, &SignerHandle::spawn(other), &foreign, &owner_hex)
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        AdapterError::Denied(Denial::Refused(RefusalReason::InvalidGrant))
    ));

    // Revocation is idempotent and ends the grant: the next mint discards it.
    let agent_hex = agent_keys.public_key().to_hex();
    client.agent_revoke(&session, &agent_hex).await.unwrap();
    client.agent_revoke(&session, &agent_hex).await.unwrap();
    evidence.invalidate(&relay);
    assert!(evidence.evidence(&relay).await.is_err());
    assert!(!evidence.has_grant(), "invalid_grant discards the grant");
    let unknown = client
        .agent_revoke(&session, &Keys::generate().public_key().to_hex())
        .await
        .unwrap_err();
    assert!(matches!(
        unknown,
        AdapterError::Denied(Denial::Refused(RefusalReason::AgentNotFound))
    ));
    // A revoked agent cannot enroll again.
    let again = client
        .agent_grant(&session, &agent, &tag, &owner_hex)
        .await
        .unwrap_err();
    assert!(matches!(
        again,
        AdapterError::Denied(Denial::Refused(RefusalReason::InvalidGrant))
    ));
    for signer in [owner, agent, stranger] {
        signer.lock().await;
    }
}
