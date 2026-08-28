mod support;

use bzz::{
    Error,
    auth::signer::SignerHandle,
    config::LocalJournalMode,
    diagnostics::{DiagnosticEvent, DiagnosticHandle, report},
    paths::Paths,
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

#[tokio::test]
async fn session_diagnostics_explain_auth_and_ack_without_message_content() {
    let temporary = tempfile::TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let diagnostics = DiagnosticHandle::start(&paths, LocalJournalMode::On, None).unwrap();
    let relay = FakeRelay::start().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let (session, mut events) =
        session::connect_with_diagnostics(relay.url.clone(), signer.clone(), diagnostics.clone())
            .await
            .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let sentinel = "SENTINEL-MESSAGE-CONTENT";
    let event = buzz_sdk::build_message(uuid::Uuid::new_v4(), sentinel, None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap();
    session.publish(event.clone()).await.unwrap();
    session.shutdown().await;
    diagnostics.shutdown().await;
    signer.lock().await;
    relay.stop();

    let records = report::load_records(&paths);
    assert!(
        records
            .iter()
            .any(|record| matches!(record.event, DiagnosticEvent::Authenticated { .. }))
    );
    assert!(records.iter().any(|record| matches!(
        &record.event,
        DiagnosticEvent::PublishAcknowledged { event_id, accepted: true, .. }
            if event_id == &event.id.to_hex()
    )));
    let encoded = serde_json::to_string(&records).unwrap();
    assert!(!encoded.contains(sentinel));
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
    let personal = subscriptions::personal(&"a".repeat(64), 1_000);
    assert_eq!(personal[0]["#p"], json!(["a".repeat(64)]));
    assert_eq!(
        personal[0]["kinds"],
        json!([30_622, 46_010, 46_011, 46_012])
    );
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
async fn client_close_acknowledgement_is_not_forwarded_as_relay_failure() {
    let (relay, close_frames) = FakeRelay::start_acknowledging_closes().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    supervisor
        .subscribe("leaving", vec![json!({"kinds":[20_002]})])
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if matches!(
                events.recv().await,
                Ok(bzz::realtime::supervisor::SupervisorEvent::Session(
                    SessionEvent::Eose(id)
                )) if id == "leaving"
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();

    supervisor.close("leaving").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while close_frames.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), events.recv())
            .await
            .is_err()
    );

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn relay_closed_subscription_is_forgotten_without_close_feedback() {
    let (relay, close_frames) = FakeRelay::start_closing_subscriptions().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    supervisor
        .subscribe("rejected", vec![json!({"kinds":[20_002]})])
        .await
        .unwrap();
    let observed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if matches!(
                events.recv().await,
                Ok(bzz::realtime::supervisor::SupervisorEvent::Session(
                    SessionEvent::Closed { subscription, .. }
                )) if subscription == "rejected"
            ) {
                break;
            }
        }
    })
    .await;
    assert!(observed.is_ok());

    // App-level quarantine may defensively request closure after the relay has
    // already declared the subscription closed. That request must remain local.
    supervisor.close("rejected").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert_eq!(close_frames.load(std::sync::atomic::Ordering::SeqCst), 0);

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn supervisor_paces_subscription_startup() {
    let (relay, request_times) = FakeRelay::start_recording_requests().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    for index in 0..12 {
        supervisor
            .subscribe(format!("paced-{index:02}"), vec![json!({"kinds":[9]})])
            .await
            .unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        let mut eose = 0;
        while eose < 12 {
            if matches!(
                events.recv().await,
                Ok(bzz::realtime::supervisor::SupervisorEvent::Session(
                    SessionEvent::Eose(_)
                ))
            ) {
                eose += 1;
            }
        }
    })
    .await
    .unwrap();

    let times = request_times.lock().unwrap().clone();
    assert_eq!(times.len(), 12);
    for pair in times.windows(2) {
        assert!(
            pair[1].duration_since(pair[0]) >= std::time::Duration::from_millis(90),
            "REQ frames were not paced: {times:?}"
        );
    }
    assert!(times[8].duration_since(times[0]) >= std::time::Duration::from_millis(900));

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn rate_limited_subscription_remains_desired_and_recovers() {
    let (relay, request_frames) = FakeRelay::start_rate_limiting_first_request().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    supervisor
        .subscribe("recovering", vec![json!({"kinds":[9]})])
        .await
        .unwrap();

    let mut limited = false;
    let mut recovered = false;
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        while !recovered {
            match events.recv().await.unwrap() {
                bzz::realtime::supervisor::SupervisorEvent::RateLimited { retry_after } => {
                    assert_eq!(retry_after, std::time::Duration::from_secs(1));
                    limited = true;
                }
                bzz::realtime::supervisor::SupervisorEvent::Session(SessionEvent::Eose(id))
                    if id == "recovering" =>
                {
                    recovered = true;
                }
                bzz::realtime::supervisor::SupervisorEvent::Session(SessionEvent::Closed {
                    subscription,
                    ..
                }) if subscription == "recovering" => {
                    panic!("temporary quota closure became terminal")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(limited);
    assert_eq!(request_frames.load(std::sync::atomic::Ordering::SeqCst), 2);

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn explicit_rate_limited_publication_is_not_republished() {
    let (relay, event_frames) =
        FakeRelay::start_counting_event_ack(false, "rate-limited: quota exceeded; retry in 1s")
            .await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let event = buzz_sdk::build_message(uuid::Uuid::new_v4(), "generated", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap();
    let ack = supervisor.publish(event).await.unwrap();
    assert!(!ack.accepted);
    assert_eq!(ack.message, "rate-limited: retry in 1s");
    tokio::time::sleep(std::time::Duration::from_millis(1_250)).await;
    assert_eq!(event_frames.load(std::sync::atomic::Ordering::SeqCst), 1);

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn legacy_uncorrelated_notice_is_uncertain_and_not_republished() {
    let (relay, event_frames) = FakeRelay::start_legacy_rate_limit_notice().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    let mut events = supervisor.subscribe_events();
    let event = buzz_sdk::build_message(uuid::Uuid::new_v4(), "generated", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap();
    let error = supervisor.publish(event).await.unwrap_err();
    assert!(matches!(
        error,
        Error::Network(message) if message == "rate-limited: retry in 1s"
    ));
    let limited = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if matches!(
                events.recv().await,
                Ok(bzz::realtime::supervisor::SupervisorEvent::RateLimited { retry_after })
                    if retry_after == std::time::Duration::from_secs(1)
            ) {
                break;
            }
        }
    })
    .await;
    assert!(limited.is_ok());
    tokio::time::sleep(std::time::Duration::from_millis(1_250)).await;
    assert_eq!(event_frames.load(std::sync::atomic::Ordering::SeqCst), 1);

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
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
