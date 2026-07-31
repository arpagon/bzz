use base64::{Engine as _, engine::general_purpose::STANDARD};
use bzz::{
    auth::signer::SignerHandle,
    protocol::{envelope::RelayMessage, events::thread_coordinates},
};
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, RelayUrl, Tag};

#[tokio::test]
async fn auth_and_nip98_are_fresh_and_correctly_scoped() {
    let keys = Keys::generate();
    let signer = SignerHandle::spawn(keys.clone());
    let relay = RelayUrl::parse("wss://buzz.example/").unwrap();
    let auth = signer.auth("challenge", relay).await.unwrap();
    assert_eq!(auth.kind.as_u16(), 22_242);
    assert!(
        auth.tags
            .iter()
            .any(|tag| tag.as_slice() == ["challenge", "challenge"])
    );

    let one = signer
        .nip98_header("POST", "https://buzz.example/query", Some(b"{}"))
        .await
        .unwrap();
    let two = signer
        .nip98_header("POST", "https://buzz.example/query", Some(b"{}"))
        .await
        .unwrap();
    assert_ne!(one, two);
    let encoded = one.strip_prefix("Nostr ").unwrap();
    let json = STANDARD.decode(encoded).unwrap();
    let event = Event::from_json(json).unwrap();
    event.verify().unwrap();
    assert_eq!(event.kind.as_u16(), 27_235);
    assert!(
        event
            .tags
            .iter()
            .any(|tag| tag.as_slice()[..2] == ["u", "https://buzz.example/query"])
    );
    assert!(
        event
            .tags
            .iter()
            .any(|tag| tag.as_slice()[..2] == ["method", "POST"])
    );
    signer.lock().await;
    assert!(
        signer
            .auth("later", RelayUrl::parse("wss://buzz.example").unwrap())
            .await
            .is_err()
    );
}

#[test]
fn relay_envelopes_parse_without_confusing_ok_and_events() {
    assert!(
        matches!(RelayMessage::parse(r#"["AUTH","c"]"#).unwrap(), RelayMessage::Auth(value) if value == "c")
    );
    assert!(matches!(
        RelayMessage::parse(r#"["OK","abc",false,"restricted: no"]"#).unwrap(),
        RelayMessage::Ok {
            accepted: false,
            ..
        }
    ));
    assert!(matches!(
        RelayMessage::parse(r#"["X",{"future":true}]"#).unwrap(),
        RelayMessage::Unknown(_)
    ));
    assert!(RelayMessage::parse("not-json").is_err());
}

#[test]
fn nip10_markers_determine_root_and_parent() {
    let keys = Keys::generate();
    let tags = vec![
        Tag::parse(["e", &"a".repeat(64), "", "root"]).unwrap(),
        Tag::parse(["e", &"b".repeat(64), "", "reply"]).unwrap(),
    ];
    let event = EventBuilder::text_note("reply")
        .tags(tags)
        .sign_with_keys(&keys)
        .unwrap();
    assert_eq!(
        thread_coordinates(&event),
        (Some("a".repeat(64)), Some("b".repeat(64)))
    );
    let json = event.as_json();
    assert!(!json.is_empty());
}
