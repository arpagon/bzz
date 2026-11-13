//! Self-tests for the programmable relay (v0.13 T2): it must record exactly
//! what bzz sends and say exactly what the script says.
mod support;

use base64::Engine as _;
use bzz::{auth::signer::SignerHandle, protocol::http::HttpClient, realtime::session};
use nostr::{JsonUtil as _, Keys};
use serde_json::json;
use support::scripted_relay::{HttpReply, Script, ScriptedRelay, Upgrade, WsAction};

#[tokio::test]
async fn serves_nip11_and_records_nip98_query() {
    let relay =
        ScriptedRelay::start(Script::default().nip11(json!({"self":"ab","requires_nip_fi":false})))
            .await;
    let keys = Keys::generate();
    let signer = SignerHandle::spawn(keys.clone());
    let http = HttpClient::new(relay.http_url.clone(), signer.clone()).unwrap();
    let info = http.nip11().await.unwrap();
    assert_eq!(info["requires_nip_fi"], false);
    let events = http
        .query(&[bzz::protocol::types::QueryFilter {
            kinds: vec![9],
            limit: Some(1),
            ..Default::default()
        }])
        .await
        .unwrap();
    assert!(events.is_empty());

    let requests = relay.http_requests();
    assert_eq!(requests.len(), 2);
    let query = &requests[1];
    assert_eq!(
        (query.method.as_str(), query.path.as_str()),
        ("POST", "/query")
    );
    assert_eq!(query.json()[0]["kinds"], json!([9]));
    let auth = query.header("authorization").unwrap();
    let encoded = auth.strip_prefix("Nostr ").unwrap();
    let event = nostr::Event::from_json(
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    event.verify().unwrap();
    assert_eq!(event.kind.as_u16(), 27_235);
    assert_eq!(event.pubkey, keys.public_key());
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn denies_upgrade_with_scripted_status_and_records_headers() {
    let relay = ScriptedRelay::start(Script::default().upgrade(|_| {
        Upgrade::Deny(
            HttpReply::text(401, "authentication required\n")
                .with_header("WWW-Authenticate", "Nostr"),
        )
    }))
    .await;
    let signer = SignerHandle::spawn(Keys::generate());
    let error = session::connect(relay.ws_url.clone(), signer.clone())
        .await
        .err()
        .expect("denied upgrade must fail");
    // NIP-FI's exact 401 contract becomes a typed, text-free denial.
    assert!(
        matches!(&error, bzz::Error::Auth(message)
            if message == bzz::auth::federated::RelayDenial::EvidenceRequired.notice()),
        "{error:?}"
    );
    assert_eq!(relay.upgrades().len(), 1);
    assert_eq!(relay.connections(), 0);
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn scripted_frames_and_policy_close_are_delivered() {
    let relay = ScriptedRelay::start(Script::default().frames(
        |frame| match frame.value[0].as_str() {
            Some("REQ") => vec![WsAction::Close(1008, "restricted: policy".into())],
            _ => support::scripted_relay::default_frame(frame),
        },
    ))
    .await;
    let signer = SignerHandle::spawn(Keys::generate());
    let (handle, mut events) = session::connect(relay.ws_url.clone(), signer.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(session::SessionEvent::Authenticated)
    ));
    handle
        .subscribe("one", vec![json!({"kinds":[9]})])
        .await
        .unwrap();
    let mut disconnected = false;
    while let Some(event) = events.recv().await {
        if matches!(event, session::SessionEvent::Disconnected(_)) {
            disconnected = true;
            break;
        }
    }
    assert!(disconnected);
    assert_eq!(relay.frames_of("AUTH").len(), 1);
    assert_eq!(relay.frames_of("REQ").len(), 1);
    signer.lock().await;
    relay.stop();
}

/// v0.14 M5: upgrades name bzz (upstream #9246), and a relay that serves no
/// community at this address (single-community mode, #9149) is a clear,
/// non-retried refusal rather than a network error.
#[tokio::test]
async fn upgrades_send_user_agent_and_a_404_names_the_wrong_community() {
    let relay = ScriptedRelay::start(
        Script::default().upgrade(|_| Upgrade::Deny(HttpReply::text(404, "not found\n"))),
    )
    .await;
    let signer = SignerHandle::spawn(Keys::generate());
    let error = session::connect(relay.ws_url.clone(), signer.clone())
        .await
        .err()
        .expect("a 404 upgrade fails");
    assert!(
        matches!(&error, bzz::Error::Access(message) if message == session::NO_COMMUNITY),
        "{error:?}"
    );
    let upgrades = relay.upgrades();
    assert_eq!(
        upgrades[0].header("user-agent"),
        Some(concat!("bzz/", env!("CARGO_PKG_VERSION")))
    );
    signer.lock().await;
    relay.stop();
}

/// v0.14 M5: a filter that constrains ids, authors, kinds or a tag to an
/// empty set selects nothing. bzz never sends one (older relays read it as no
/// constraint, upstream #9342): HTTP queries skip it, and a subscription
/// made only of such filters completes at once without a REQ.
#[tokio::test]
async fn filters_with_empty_sets_are_never_sent() {
    use bzz::protocol::types::{QueryFilter, matches_nothing};
    for empty in [
        json!({"ids": []}),
        json!({"authors": [], "kinds": [9]}),
        json!({"kinds": []}),
        json!({"kinds": [9], "#h": []}),
        json!({"kinds": [9], "#a": []}),
        json!({"kinds": [9], "#e": []}),
    ] {
        assert!(matches_nothing(&empty), "{empty}");
    }
    for real in [
        json!({"kinds": [9]}),
        json!({"kinds": [9], "#h": ["x"]}),
        json!({}),
    ] {
        assert!(!matches_nothing(&real), "{real}");
    }

    let relay = ScriptedRelay::start(Script::default()).await;
    let signer = SignerHandle::spawn(Keys::generate());
    let http = HttpClient::new(relay.http_url.clone(), signer.clone()).unwrap();
    let typed = QueryFilter {
        kinds: vec![9],
        ..Default::default()
    }
    .tag("h", Vec::new());
    assert!(http.query(&[typed]).await.unwrap().is_empty());
    assert!(
        http.query_raw(&[json!({"kinds": [9], "#e": []})])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(relay.http_requests().is_empty(), "no HTTP request was made");

    let (handle, mut events) = session::connect(relay.ws_url.clone(), signer.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(session::SessionEvent::Authenticated)
    ));
    handle
        .subscribe("empty", vec![json!({"kinds": [9], "authors": []})])
        .await
        .unwrap();
    assert!(matches!(
        events.recv().await,
        Some(session::SessionEvent::Eose(id)) if id == "empty"
    ));
    handle
        .subscribe(
            "mixed",
            vec![json!({"ids": []}), json!({"kinds": [9], "limit": 1})],
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let reqs = relay.frames_of("REQ");
    assert_eq!(reqs.len(), 1, "only the subscription with a real filter");
    assert_eq!(
        reqs[0].value.as_array().unwrap().len(),
        3,
        "its empty filter dropped"
    );
    signer.lock().await;
    relay.stop();
}
