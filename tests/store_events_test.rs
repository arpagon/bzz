use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::{
        Store,
        models::{OutboxState, SyncCursor},
    },
};
use nostr::{EventBuilder, Keys, Tag};
use proptest::prelude::*;
use uuid::Uuid;

fn fixture() -> (Store, Uuid, Uuid, Keys) {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "me".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "identity:test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("one".into(), "wss://one.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    (store, community, Uuid::new_v4(), Keys::generate())
}

#[test]
fn event_delivery_is_idempotent_and_community_isolated() {
    let (mut store, community, channel, keys) = fixture();
    let event = buzz_sdk::build_message(channel, "hello", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    assert!(store.apply_event(community, &event).unwrap());
    assert!(!store.apply_event(community, &event).unwrap());
    assert_eq!(store.messages(community, channel, 100).unwrap().len(), 1);

    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "other".into(),
        pubkey: "b".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "identity:other".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let other = config
        .add_community("two".into(), "wss://two.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    store.apply_event(other, &event).unwrap();
    assert_eq!(store.messages(other, channel, 100).unwrap().len(), 1);
}

#[test]
fn outbox_and_delete_are_reduced_deterministically() {
    let (mut store, community, channel, keys) = fixture();
    let event = buzz_sdk::build_message(channel, "pending", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.insert_outbox(community, &event).unwrap();
    assert!(store.messages(community, channel, 10).unwrap()[0].pending);
    store
        .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();
    assert!(!store.messages(community, channel, 10).unwrap()[0].pending);
    let deletion = buzz_sdk::build_delete_compat(channel, event.id)
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(community, &deletion).unwrap();
    assert!(store.messages(community, channel, 10).unwrap()[0].deleted);
}

#[test]
fn destructive_outbox_events_wait_for_relay_authority() {
    let (mut store, community, channel, owner) = fixture();
    let target = buzz_sdk::build_message(channel, "target", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
    store.apply_event(community, &target).unwrap();
    let deletion = buzz_sdk::build_delete_compat(channel, target.id)
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
    store.insert_outbox(community, &deletion).unwrap();
    assert!(!store.messages(community, channel, 10).unwrap()[0].deleted);
    store
        .set_outbox_state(
            community,
            &deletion.id.to_hex(),
            OutboxState::Rejected,
            Some("rejected"),
        )
        .unwrap();
    assert!(!store.messages(community, channel, 10).unwrap()[0].deleted);
    store.apply_event(community, &deletion).unwrap();
    assert!(store.messages(community, channel, 10).unwrap()[0].deleted);
}

#[test]
fn deletion_reduction_is_authorized_and_order_independent() {
    let (mut store, community, channel, owner) = fixture();
    let target = buzz_sdk::build_message(channel, "target", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
    let attacker = Keys::generate();
    let unauthorized = buzz_sdk::build_delete_compat(channel, target.id)
        .unwrap()
        .sign_with_keys(&attacker)
        .unwrap();
    store.apply_event(community, &unauthorized).unwrap();
    store.apply_event(community, &target).unwrap();
    assert!(!store.messages(community, channel, 10).unwrap()[0].deleted);

    let late_target = buzz_sdk::build_message(channel, "late", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
    let authorized = buzz_sdk::build_delete_compat(channel, late_target.id)
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
    store.apply_event(community, &authorized).unwrap();
    store.apply_event(community, &late_target).unwrap();
    assert!(
        store
            .messages(community, channel, 10)
            .unwrap()
            .iter()
            .find(|message| message.event_id == late_target.id.to_hex())
            .unwrap()
            .deleted
    );
}

#[test]
fn relay_projection_requires_the_pinned_nip11_key() {
    let (mut store, community, channel, _) = fixture();
    let relay = Keys::generate();
    let metadata = EventBuilder::new(nostr::Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", "general"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    assert!(store.apply_event(community, &metadata).is_err());
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    assert!(store.apply_event(community, &metadata).unwrap());
    assert_eq!(store.channels(community).unwrap()[0].name, "general");
    store
        .reconcile_self_memberships(community, &std::collections::BTreeSet::from([channel]))
        .unwrap();
    assert!(store.channels(community).unwrap()[0].is_member);
    store
        .reconcile_self_memberships(community, &std::collections::BTreeSet::new())
        .unwrap();
    assert!(!store.channels(community).unwrap()[0].is_member);
    assert!(
        store
            .pin_relay_pubkey(community, &Keys::generate().public_key().to_hex())
            .is_err()
    );
}

#[test]
fn read_markers_and_sync_cursors_never_move_backwards() {
    let (store, community, _, _) = fixture();
    assert_eq!(
        store
            .advance_read(community, "self", "channel", 20, true)
            .unwrap(),
        20
    );
    assert_eq!(
        store
            .advance_read(community, "self", "channel", 10, true)
            .unwrap(),
        20
    );
    store
        .save_sync_cursor(
            community,
            "history",
            "c",
            &SyncCursor {
                high_created_at: 50,
                high_event_id: "b".repeat(64),
                complete_through: 40,
            },
        )
        .unwrap();
    store
        .save_sync_cursor(
            community,
            "history",
            "c",
            &SyncCursor {
                high_created_at: 30,
                high_event_id: "a".repeat(64),
                complete_through: 20,
            },
        )
        .unwrap();
    let cursor = store.sync_cursor(community, "history", "c").unwrap();
    assert_eq!(cursor.high_created_at, 50);
    assert_eq!(cursor.complete_through, 40);
    store.reset_sync_cursor(community, "history", "c").unwrap();
    assert_eq!(
        store
            .sync_cursor(community, "history", "c")
            .unwrap()
            .high_created_at,
        0
    );
}

proptest! {
    #[test]
    fn duplicate_application_never_multiplies_rows(deliveries in 1usize..20) {
        let (mut store,community,channel,keys)=fixture();
        let event=EventBuilder::new(nostr::Kind::Custom(9),"same").tags([Tag::parse(["h",&channel.to_string()]).unwrap()]).sign_with_keys(&keys).unwrap();
        for _ in 0..deliveries { store.apply_event(community,&event).unwrap(); }
        prop_assert_eq!(store.messages(community,channel,100).unwrap().len(),1);
    }
}
