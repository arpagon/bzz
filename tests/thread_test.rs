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

#[test]
fn sent_from_thread_copies_render_their_provenance() {
    use bzz::{
        config::{Config, IdentityConfig, KeyBackend},
        store::Store,
    };
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "me".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "k".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("one".into(), "wss://one.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    let channel = uuid::Uuid::new_v4();
    let root = "b".repeat(64);
    let event = buzz_sdk::build_message(channel, "copy", None, &[], false, &[], &[])
        .unwrap()
        .tag(bzz::protocol::sent_from_thread::build_tag(
            &root,
            Some("Release plan…"),
        ))
        .sign_with_keys(&nostr::Keys::generate())
        .unwrap();
    store.apply_event(community, &event).unwrap();
    let messages = store.messages(community, channel, 10).unwrap();
    let reference = messages[0].sent_from_thread.as_ref().unwrap();
    assert_eq!(reference.root_event_id, root);
    assert_eq!(reference.excerpt.as_deref(), Some("Release plan…"));
    let theme = bzz::ui::theme::Theme::default();
    let mut state = bzz::ui::timeline::TimelineState::default();
    state.reconcile(&messages);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 12)).unwrap();
    terminal
        .draw(|frame| {
            bzz::ui::timeline::render(
                frame,
                frame.area(),
                &messages,
                &Default::default(),
                &Default::default(),
                &Default::default(),
                &Default::default(),
                &mut state,
                "timeline",
                &theme,
                true,
                None,
            );
        })
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        screen.contains("↪ sent from thread · Release plan…"),
        "{screen}"
    );
}
