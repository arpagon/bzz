use bzz::service::messages::MessageService;
use nostr::{EventId, Keys};

#[test]
fn upstream_builder_emits_direct_and_nested_markers() {
    let keys = Keys::generate();
    let channel = uuid::Uuid::new_v4();
    let root = EventId::from_hex(&"a".repeat(64)).unwrap();
    let parent = EventId::from_hex(&"b".repeat(64)).unwrap();
    let direct = buzz_sdk::build_message(
        channel,
        "direct",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: root,
            parent_event_id: root,
        }),
        &[],
        false,
        &[],
    )
    .unwrap()
    .sign_with_keys(&keys)
    .unwrap();
    assert!(
        direct
            .tags
            .iter()
            .any(|tag| tag.as_slice().get(3).map(String::as_str) == Some("reply"))
    );
    let nested = buzz_sdk::build_message(
        channel,
        "nested",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: root,
            parent_event_id: parent,
        }),
        &[],
        false,
        &[],
    )
    .unwrap()
    .sign_with_keys(&keys)
    .unwrap();
    assert!(
        nested
            .tags
            .iter()
            .any(|tag| tag.as_slice().get(3).map(String::as_str) == Some("root"))
    );
    assert!(
        nested
            .tags
            .iter()
            .any(|tag| tag.as_slice().get(3).map(String::as_str) == Some("reply"))
    );
    let _ = std::mem::size_of::<MessageService>();
}
