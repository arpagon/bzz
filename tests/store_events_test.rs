use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    domain::SystemEventKind,
    store::{
        Store,
        models::{OutboxState, SyncCursor},
    },
};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
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
fn relay_thread_summaries_are_validated_but_never_stored_as_messages() {
    let (mut store, community, channel, relay) = fixture();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    let root = "ab".repeat(32);
    let summary = EventBuilder::new(
        Kind::Custom(39_005),
        serde_json::json!({
            "reply_count":1,
            "descendant_count":1,
            "last_reply_at":20,
            "participants":[relay.public_key().to_hex()]
        })
        .to_string(),
    )
    .tags([
        Tag::parse(["e", &root]).unwrap(),
        Tag::parse(["d", &root]).unwrap(),
        Tag::parse(["h", &channel.to_string()]).unwrap(),
    ])
    .sign_with_keys(&relay)
    .unwrap();
    assert!(
        store
            .live_thread_summary(community, &summary)
            .unwrap()
            .is_some()
    );
    assert!(store.apply_event(community, &summary).is_err());
    assert!(store.messages(community, channel, 100).unwrap().is_empty());
}

#[test]
fn relay_membership_preserves_only_the_exact_bot_role_for_agent_discovery() {
    let (mut store, community, channel, relay) = fixture();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    let agent = Keys::generate().public_key().to_hex();
    let human = Keys::generate().public_key().to_hex();
    let lookalike = Keys::generate().public_key().to_hex();
    let metadata = EventBuilder::new(Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", "agents"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &metadata).unwrap();
    let membership = EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &agent, "", "bot"]).unwrap(),
            Tag::parse(["p", &human, "", "admin"]).unwrap(),
            Tag::parse(["p", &lookalike, "", "bot", "extra"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &membership).unwrap();

    assert_eq!(
        store.remote_agent_candidate_pubkeys(community).unwrap(),
        vec![agent.clone()]
    );
    let candidates = store
        .mention_candidates(community, channel, &"a".repeat(64), "")
        .unwrap();
    assert_eq!(candidates.len(), 3);
    assert!(candidates.iter().all(|candidate| !candidate.is_agent));
    assert!(
        store
            .agent_mentions_need_validation(community, channel, &[agent])
            .unwrap()
            == 1
    );
    assert!(
        store
            .agent_mentions_need_validation(community, channel, &[human])
            .unwrap()
            == 0
    );
    assert!(
        store
            .agent_mentions_need_validation(community, channel, &[lookalike])
            .unwrap()
            == 0
    );
}

#[test]
fn relay_system_events_are_semantic_content_free_and_not_unread_activity() {
    let (mut store, community, channel, relay) = fixture();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    let other = Keys::generate().public_key().to_hex();
    let metadata = EventBuilder::new(Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", "DM"]).unwrap(),
            Tag::parse(["t", "dm"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &metadata).unwrap();
    let membership = EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &"a".repeat(64)]).unwrap(),
            Tag::parse(["p", &other]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &membership).unwrap();
    let system = EventBuilder::new(
        Kind::Custom(40_099),
        serde_json::json!({
            "type":"dm_created",
            "actor":"a".repeat(64),
            "participants":["a".repeat(64),other]
        })
        .to_string(),
    )
    .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
    .sign_with_keys(&relay)
    .unwrap();
    store.apply_event(community, &system).unwrap();

    let messages = store.messages(community, channel, 10).unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.is_empty());
    assert_eq!(
        messages[0].system.as_ref().map(|event| event.kind),
        Some(SystemEventKind::DmCreated)
    );
    assert!(
        store
            .channel_content_ids(community, channel)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .latest_channel_activity_at(community, channel)
            .unwrap(),
        None
    );
    assert!(
        store
            .unread_channels(community, &"a".repeat(64))
            .unwrap()
            .is_empty()
    );

    let unknown = EventBuilder::new(
        Kind::Custom(40_099),
        r#"{"type":"future","secret":"never render"}"#,
    )
    .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
    .sign_with_keys(&relay)
    .unwrap();
    store.apply_event(community, &unknown).unwrap();
    let messages = store.messages(community, channel, 10).unwrap();
    assert_eq!(messages.len(), 2);
    let unknown_message = messages
        .iter()
        .find(|message| message.event_id == unknown.id.to_hex())
        .unwrap();
    assert_eq!(
        unknown_message.system.as_ref().map(|event| event.kind),
        Some(SystemEventKind::Unsupported)
    );
    assert!(unknown_message.content.is_empty());

    let attacker = Keys::generate();
    let forged = EventBuilder::new(Kind::Custom(40_099), r#"{"type":"channel_created"}"#)
        .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
        .sign_with_keys(&attacker)
        .unwrap();
    assert!(store.apply_event(community, &forged).is_err());
}

#[test]
fn outbox_and_delete_are_reduced_deterministically() {
    let (mut store, community, channel, keys) = fixture();
    let event = buzz_sdk::build_message(channel, "pending", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.insert_outbox(community, &event).unwrap();
    assert_eq!(
        store.messages(community, channel, 10).unwrap()[0].delivery,
        bzz::domain::DeliveryState::Pending
    );
    store
        .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();
    assert_eq!(
        store.messages(community, channel, 10).unwrap()[0].delivery,
        bzz::domain::DeliveryState::Delivered
    );
    let deletion = buzz_sdk::build_delete_compat(channel, event.id)
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(community, &deletion).unwrap();
    assert!(store.messages(community, channel, 10).unwrap()[0].deleted);
}

#[test]
fn exact_unknown_and_rejected_states_are_not_collapsed_into_pending() {
    let (mut store, community, channel, keys) = fixture();
    let event = buzz_sdk::build_message(channel, "delivery", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    let event_id = event.id.to_hex();
    store.insert_outbox(community, &event).unwrap();
    store
        .set_outbox_state(
            community,
            &event_id,
            OutboxState::Unknown,
            Some("ack timeout"),
        )
        .unwrap();
    assert_eq!(
        store.messages(community, channel, 10).unwrap()[0].delivery,
        bzz::domain::DeliveryState::Unknown
    );
    store
        .set_outbox_state(
            community,
            &event_id,
            OutboxState::Rejected,
            Some("access denied"),
        )
        .unwrap();
    assert_eq!(
        store.messages(community, channel, 10).unwrap()[0].delivery,
        bzz::domain::DeliveryState::Rejected
    );
}

#[test]
fn repeated_relay_echo_is_a_noop_after_exact_outbox_delivery() {
    let (mut store, community, channel, keys) = fixture();
    let event = buzz_sdk::build_message(channel, "pending", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.insert_outbox(community, &event).unwrap();

    assert!(store.apply_event(community, &event).unwrap());
    assert!(!store.apply_event(community, &event).unwrap());
    assert_eq!(
        store.messages(community, channel, 10).unwrap()[0].delivery,
        bzz::domain::DeliveryState::Delivered
    );
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
fn thread_read_markers_clear_their_sidebar_unread_indicator() {
    let (mut store, community, channel, other) = fixture();
    let self_pubkey = "a".repeat(64);
    let root = buzz_sdk::build_message(channel, "root", None, &[], false, &[])
        .unwrap()
        .custom_created_at(Timestamp::from(10))
        .sign_with_keys(&other)
        .unwrap();
    let reply = buzz_sdk::build_message(
        channel,
        "reply",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        }),
        &[],
        false,
        &[],
    )
    .unwrap()
    .custom_created_at(Timestamp::from(20))
    .sign_with_keys(&other)
    .unwrap();
    store.apply_event(community, &root).unwrap();
    store.apply_event(community, &reply).unwrap();
    assert_eq!(
        store
            .latest_channel_activity_at(community, channel)
            .unwrap(),
        Some(20)
    );
    store
        .advance_read(community, &self_pubkey, &channel.to_string(), 10, true)
        .unwrap();

    assert!(
        store
            .unread_channels(community, &self_pubkey)
            .unwrap()
            .contains(&channel)
    );

    store
        .advance_read(
            community,
            &self_pubkey,
            &format!("thread:{}", root.id.to_hex()),
            20,
            true,
        )
        .unwrap();

    assert!(
        !store
            .unread_channels(community, &self_pubkey)
            .unwrap()
            .contains(&channel)
    );
}

#[test]
fn thread_summaries_count_delivered_non_deleted_descendants() {
    let (mut store, community, channel, author) = fixture();
    let root = buzz_sdk::build_message(channel, "root", None, &[], false, &[])
        .unwrap()
        .custom_created_at(Timestamp::from(10))
        .sign_with_keys(&author)
        .unwrap();
    let direct = buzz_sdk::build_message(
        channel,
        "direct",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        }),
        &[],
        false,
        &[],
    )
    .unwrap()
    .custom_created_at(Timestamp::from(20))
    .sign_with_keys(&author)
    .unwrap();
    let nested = buzz_sdk::build_message(
        channel,
        "nested",
        Some(&buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: direct.id,
        }),
        &[],
        false,
        &[],
    )
    .unwrap()
    .custom_created_at(Timestamp::from(30))
    .sign_with_keys(&author)
    .unwrap();
    store.apply_event(community, &root).unwrap();
    store.apply_event(community, &direct).unwrap();
    store.insert_outbox(community, &nested).unwrap();

    let roots = vec![root.id.to_hex()];
    let summary = store.thread_summaries(community, channel, &roots).unwrap();
    assert_eq!(summary[&root.id.to_hex()].descendant_count, 1);
    assert_eq!(summary[&root.id.to_hex()].last_reply_at, Some(20));

    store
        .set_outbox_state(community, &nested.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();
    let summary = store.thread_summaries(community, channel, &roots).unwrap();
    assert_eq!(summary[&root.id.to_hex()].descendant_count, 2);
    assert_eq!(summary[&root.id.to_hex()].last_reply_at, Some(30));

    let deletion = EventBuilder::new(Kind::EventDeletion, "")
        .tags([Tag::event(nested.id)])
        .custom_created_at(Timestamp::from(40))
        .sign_with_keys(&author)
        .unwrap();
    store.apply_event(community, &deletion).unwrap();
    let summary = store.thread_summaries(community, channel, &roots).unwrap();
    assert_eq!(summary[&root.id.to_hex()].descendant_count, 1);
    assert_eq!(summary[&root.id.to_hex()].last_reply_at, Some(20));

    assert!(
        store
            .thread_summaries(community, Uuid::new_v4(), &roots)
            .unwrap()
            .is_empty()
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
