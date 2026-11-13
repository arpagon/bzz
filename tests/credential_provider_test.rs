//! v0.13 M3: Buzz Desktop as identity and evidence provider (T5).
#![cfg(not(windows))]
mod support;

use std::sync::Arc;

use bzz::{
    auth::{
        federated::EvidenceSource as _,
        provider::{Endpoint, ProviderClient, ProviderError, ProviderEvidence},
        signer::SignerHandle,
    },
    clock::{self, WallClock as _},
};
use secrecy::SecretString;
use support::fake_provider::FakeProvider;

fn client(provider: &FakeProvider, token: &str) -> Arc<ProviderClient> {
    Arc::new(
        ProviderClient::new(
            Endpoint::Advertised(provider.url.clone()),
            Some(SecretString::from(token.to_owned())),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn remote_signer_signs_exactly_what_was_requested() {
    let provider = FakeProvider::start(&["wss://team.example"]).await;
    let client = client(&provider, &provider.token);
    let signer = SignerHandle::spawn_remote(client.clone(), provider.keys.public_key());

    let channel = uuid::Uuid::new_v4();
    let message = signer
        .sign(buzz_sdk::build_message(channel, "generated", None, &[], false, &[], &[]).unwrap())
        .await
        .unwrap();
    assert_eq!(message.pubkey, provider.keys.public_key());
    message.verify().unwrap();

    let auth = signer
        .auth(
            "challenge-1",
            nostr::RelayUrl::parse("wss://team.example").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth.kind.as_u16(), 22_242);

    let header = signer
        .nip98_header("POST", "https://team.example/query", Some(b"[]"))
        .await
        .unwrap();
    assert!(header.starts_with("Nostr "));
    // NIP-98 to an unbound origin (e.g. an adapter) is refused by policy.
    assert!(
        signer
            .nip98_header(
                "POST",
                "https://adapter.example/v1/identity/assertions",
                Some(b"{}")
            )
            .await
            .is_err()
    );

    let ciphertext = signer.encrypt_self("read-state".into()).await.unwrap();
    assert_eq!(signer.decrypt_self(ciphertext).await.unwrap(), "read-state");

    // Sensitive kinds surface the approval state; unknown kinds are denied.
    let profile = signer
        .sign(nostr::EventBuilder::new(nostr::Kind::Metadata, "{}"))
        .await;
    assert!(profile.unwrap_err().to_string().contains("approve"));
    assert!(provider.calls().iter().all(|method| method != "pair"));
    signer.lock().await;
}

#[tokio::test]
async fn tampered_signatures_and_revoked_tokens_fail_closed() {
    let provider = FakeProvider::start(&["wss://team.example"]).await;
    provider.behavior.lock().unwrap().tamper = true;
    let signer = SignerHandle::spawn_remote(
        client(&provider, &provider.token),
        provider.keys.public_key(),
    );
    let error = signer
        .sign(
            buzz_sdk::build_message(uuid::Uuid::new_v4(), "x", None, &[], false, &[], &[]).unwrap(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("different event"), "{error}");
    signer.lock().await;

    // The configured identity must match the provider's key.
    provider.behavior.lock().unwrap().tamper = false;
    let impostor = SignerHandle::spawn_remote(
        client(&provider, &provider.token),
        nostr::Keys::generate().public_key(),
    );
    assert!(
        impostor
            .sign(
                buzz_sdk::build_message(uuid::Uuid::new_v4(), "x", None, &[], false, &[], &[])
                    .unwrap()
            )
            .await
            .is_err()
    );
    impostor.lock().await;

    let wrong_token = client(&provider, "not-the-token");
    assert_eq!(
        wrong_token.describe().await.unwrap_err(),
        ProviderError::Revoked
    );
}

#[tokio::test]
async fn describe_binds_relays_and_evidence_comes_from_federated_identity() {
    let provider = FakeProvider::start(&["https://Team.Example"]).await;
    let client = client(&provider, &provider.token);
    let description = client.describe().await.unwrap();
    assert_eq!(description.pubkey, provider.keys.public_key().to_hex());
    assert!(description.binds("wss://team.example"));
    assert!(!description.binds("wss://other.example"));

    let evidence = ProviderEvidence::new(client.clone(), clock::system());
    let relay = url::Url::parse("wss://team.example").unwrap();
    // No enterprise login on the desktop: send no evidence.
    assert_eq!(evidence.evidence(&relay).await.unwrap(), None);
    let exp = clock::SystemClock.now_unix() + 600;
    use base64::Engine as _;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::json!({"exp": exp}).to_string());
    provider.behavior.lock().unwrap().assertion = format!("eyJhbGciOiJFUzI1NiJ9.{payload}.sig");
    let header = evidence.evidence(&relay).await.unwrap().unwrap();
    assert!(header.starts_with("Bearer eyJ"));
    let calls_before = provider.calls().len();
    evidence.evidence(&relay).await.unwrap();
    assert_eq!(
        provider.calls().len(),
        calls_before,
        "cached until near exp"
    );
    evidence.invalidate(&relay);
    evidence.evidence(&relay).await.unwrap();
    assert_eq!(provider.calls().len(), calls_before + 1);
}

#[tokio::test]
async fn pairing_needs_no_token_and_returns_one() {
    let provider = FakeProvider::start(&[]).await;
    let unpaired = ProviderClient::new(Endpoint::Advertised(provider.url.clone()), None).unwrap();
    let pairing = unpaired.pair("bzz", "wss://team.example").await.unwrap();
    assert_eq!(pairing.token, provider.token);
    assert_eq!(pairing.pubkey, provider.keys.public_key().to_hex());
    assert_eq!(
        unpaired.describe().await.unwrap_err(),
        ProviderError::Revoked
    );
}

#[test]
fn inherited_provider_rejects_non_loopback_urls() {
    assert!(
        ProviderClient::from_inherited_value(r#"{"url":"http://localhost:4100/","token":"t"}"#)
            .is_err()
    );
    assert!(
        ProviderClient::from_inherited_value(r#"{"url":"http://127.0.0.1:4100/rpc"}"#).is_err()
    );
    assert!(
        ProviderClient::from_inherited_value(r#"{"url":"http://127.0.0.1:4100/rpc","token":"t"}"#)
            .unwrap()
            .is_inherited()
    );
}

/// v0.14 M2: Buzz 1.0 signs only ordinary kinds and auth proofs for a local
/// tool. bzz refuses kinds the contract never signs without asking, and stops
/// offering a sensitive kind once Buzz refused it.
#[tokio::test]
async fn buzz_1_0_refusals_are_known_before_the_person_acts() {
    let provider = FakeProvider::start(&["wss://team.example"]).await;
    provider.behavior.lock().unwrap().buzz_1_0 = true;
    let client = client(&provider, &provider.token);
    let signer = SignerHandle::spawn_remote(client.clone(), provider.keys.public_key());
    let channel = uuid::Uuid::new_v4();
    let target = nostr::EventId::all_zeros();
    let sign_calls = || {
        provider
            .calls()
            .iter()
            .filter(|method| *method == "sign_event")
            .count()
    };

    // Ordinary work signs.
    signer
        .sign(buzz_sdk::build_message(channel, "generated", None, &[], false, &[], &[]).unwrap())
        .await
        .unwrap();
    assert_eq!(signer.refusal(9), None);

    // Never signed for local tools: refused before Buzz is asked.
    assert_eq!(
        signer.refusal(1984),
        Some(bzz::auth::operation_policy::REFUSED)
    );
    let before = sign_calls();
    let report = signer
        .sign(
            buzz_sdk::build_report(
                buzz_sdk::ReportTarget::Pubkey(&provider.keys.public_key().to_hex()),
                "spam",
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(report, bzz::error::Error::SignerRefused(_)),
        "{report:?}"
    );
    assert_eq!(sign_calls(), before, "nothing was sent to Buzz");

    // Sensitive: Buzz may ask a person; Buzz 1.0 refuses. Then bzz knows.
    assert_eq!(signer.refusal(5), None);
    let delete = signer
        .sign(buzz_sdk::build_delete_compat(channel, target).unwrap())
        .await
        .unwrap_err();
    assert!(delete.to_string().contains("do it in Buzz"), "{delete}");
    assert_eq!(
        signer.refusal(5),
        Some(bzz::auth::operation_policy::REFUSED)
    );
    let before = sign_calls();
    assert!(
        signer
            .sign(buzz_sdk::build_delete_compat(channel, target).unwrap())
            .await
            .is_err()
    );
    assert_eq!(sign_calls(), before, "a known refusal is not asked again");
}

#[tokio::test]
async fn approval_required_keeps_the_kind_available() {
    let provider = FakeProvider::start(&["wss://team.example"]).await;
    let client = client(&provider, &provider.token);
    let signer = SignerHandle::spawn_remote(client.clone(), provider.keys.public_key());
    let error = signer
        .sign(nostr::EventBuilder::new(nostr::Kind::Metadata, "{}"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("approve it in Buzz"), "{error}");
    assert_eq!(
        signer.refusal(0),
        None,
        "a pending approval is not a refusal"
    );
    // Keys bzz holds are never refused.
    let local = SignerHandle::spawn(nostr::Keys::generate());
    assert_eq!(local.refusal(1984), None);
    assert_eq!(local.refusal(5), None);
}
