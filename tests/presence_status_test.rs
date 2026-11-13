//! v0.13 M6: NIP-38 status projection and presence/status presentation.
use std::collections::HashMap;

use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    domain::{Message, Profile},
    protocol::presence::Presence,
    store::Store,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use uuid::Uuid;

fn store() -> (Store, Uuid) {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
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
    (store, community)
}

fn status(keys: &Keys, text: &str, extra: &[&[&str]], at: u64) -> nostr::Event {
    let mut tags = vec![Tag::parse(["d", "general"]).unwrap()];
    tags.extend(
        extra
            .iter()
            .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
    );
    EventBuilder::new(Kind::Custom(30_315), text)
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .unwrap()
}

#[test]
fn newest_status_wins_and_expiry_hides_it() {
    let (mut store, community) = store();
    let keys = Keys::generate();
    let profile = EventBuilder::new(Kind::Metadata, r#"{"name":"Ana"}"#)
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(community, &profile).unwrap();
    store
        .apply_event(
            community,
            &status(&keys, "newer", &[&["expiration", "2000"]], 200),
        )
        .unwrap();
    store
        .apply_event(community, &status(&keys, "older", &[], 100))
        .unwrap();
    let pubkey = keys.public_key().to_hex();
    let profile = store.profiles(community).unwrap().remove(&pubkey).unwrap();
    assert_eq!(profile.status_text(1_999).as_deref(), Some("newer"));
    assert_eq!(profile.status_text(2_000), None, "expired at 2000");
    // An empty newer status clears; the older one never resurfaces.
    store
        .apply_event(community, &status(&keys, "", &[], 300))
        .unwrap();
    let profile = store.profiles(community).unwrap().remove(&pubkey).unwrap();
    assert_eq!(profile.status_text(0), None);
}

#[test]
fn ephemeral_presence_never_enters_the_durable_store() {
    let (mut store, community) = store();
    let event = buzz_sdk::build_presence_update("online")
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap();
    assert!(store.apply_event(community, &event).is_err());
}

#[test]
fn timeline_shows_presence_marker_and_status() {
    let pubkey = "a".repeat(64);
    let channel = Uuid::new_v4();
    let profile = Profile {
        pubkey: pubkey.clone(),
        display_name: Some("Ana".into()),
        name: None,
        picture: None,
        nip05: None,
        about: None,
        event_id: "b".repeat(64),
        created_at: 1,
        verified: None,
        status: Some(bzz::protocol::presence::UserStatus {
            text: "in a meeting".into(),
            emoji: "📅".into(),
            expires_at: None,
        }),
        presence: Some(Presence::Online),
        in_huddle: false,
    };
    let profiles = HashMap::from([(pubkey.clone(), profile)]);
    let messages = vec![Message {
        event_id: "c".repeat(64),
        channel_id: channel,
        pubkey,
        created_at: 1_790_000_000,
        content: "hello".into(),
        attachments: vec![],
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        delivery: bzz::domain::DeliveryState::Delivered,
        system: None,
        sent_from_thread: None,
        broadcast: false,
        emoji: Vec::new(),
        edited: false,
    }];
    let theme = bzz::ui::theme::Theme::default();
    let mut state = bzz::ui::timeline::TimelineState::default();
    state.reconcile(&messages);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 8)).unwrap();
    terminal
        .draw(|frame| {
            bzz::ui::timeline::render(
                frame,
                frame.area(),
                &messages,
                &Default::default(),
                &profiles,
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
    assert!(screen.contains("● Ana"), "{screen}");
    // The avatar keeps its own shape glyph and the bare initial.
    assert!(screen.contains("A] ● Ana"), "{screen}");
    assert!(
        screen.contains("📅") && screen.contains("in a meeting"),
        "{screen}"
    );
}
