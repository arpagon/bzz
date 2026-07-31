mod support;

use bzz::{
    Error,
    auth::signer::SignerHandle,
    realtime::{
        session::{self, SessionEvent},
        subscriptions,
        supervisor::SupervisorHandle,
    },
};
use nostr::Keys;
use serde_json::json;
use support::fake_relay::FakeRelay;

#[tokio::test]
async fn session_authenticates_subscribes_and_correlates_ack() {
    let relay = FakeRelay::start().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let (session, mut events) = session::connect(relay.url.clone(), signer.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    session
        .subscribe("test", vec![json!({"kinds":[9]})])
        .await
        .unwrap();
    assert!(matches!(events.recv().await,Some(SessionEvent::Eose(id)) if id=="test"));
    let event = buzz_sdk::build_message(uuid::Uuid::new_v4(), "hello", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap();
    let ack = session.publish(event.clone()).await.unwrap();
    assert!(ack.accepted);
    assert_eq!(ack.event_id, event.id.to_hex());
    session.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[test]
fn global_aux_and_membership_subscriptions_match_buzz_routing() {
    let global = subscriptions::global_stream(1_000);
    assert_eq!(global.len(), 1);
    assert!(global[0].get("#h").is_none());
    assert!(
        global[0]["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind.as_u64() == Some(7))
    );
    let membership = subscriptions::membership(&"a".repeat(64), 1_000);
    assert_eq!(membership[0]["kinds"], json!([44_100, 44_101]));
    assert_eq!(membership[0]["since"], json!(970));
}

#[test]
fn authentication_failures_are_actionably_classified() {
    assert!(matches!(
        session::classify_auth_failure("restricted: not a relay member"),
        Error::Access(_)
    ));
    assert!(matches!(
        session::classify_auth_failure("banned by operator"),
        Error::Access(_)
    ));
    assert!(matches!(
        session::classify_auth_failure("event timestamp expired"),
        Error::Auth(message) if message.starts_with("clock-skew:")
    ));
    assert!(matches!(
        session::classify_auth_failure("temporarily unavailable; try again"),
        Error::Network(_)
    ));
    assert!(matches!(
        session::classify_auth_failure("invalid auth signature"),
        Error::Auth(_)
    ));
}

#[tokio::test]
async fn supervisor_replays_desired_subscription() {
    let relay = FakeRelay::start().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    supervisor
        .subscribe("persistent", vec![json!({"kinds":[9]})])
        .await
        .unwrap();
    let observed=tokio::time::timeout(std::time::Duration::from_secs(3),async move {
        loop { if matches!(events.recv().await,Ok(bzz::realtime::supervisor::SupervisorEvent::Session(SessionEvent::Eose(id))) if id=="persistent") { break; } }
    }).await;
    assert!(observed.is_ok());
    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}
