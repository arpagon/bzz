mod support;

use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    realtime::session,
    store::{Store, models::OutboxState},
    sync::read_state::{ReadStateBlob, split},
};
use nostr::Keys;
use std::collections::BTreeMap;
use support::fake_relay::FakeRelay;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn cached_send_ack_duplicate_and_restart_form_one_message() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "human".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "team".into(),
            "wss://team.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let channel = Uuid::new_v4();
    let keys = Keys::generate();
    let event = buzz_sdk::build_message(channel, "hello", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    {
        let mut store = Store::open(&path).unwrap();
        store.sync_config(&config).unwrap();
        store.insert_outbox(community, &event).unwrap();
        store
            .save_draft(community, channel, None, "restart-safe draft")
            .unwrap();
        assert_eq!(store.pending_outbox(community).unwrap().len(), 1);
        let relay = FakeRelay::start().await;
        let signer = SignerHandle::spawn(Keys::generate());
        let (session, _) = session::connect(relay.url.clone(), signer.clone())
            .await
            .unwrap();
        let ack = session.publish(event.clone()).await.unwrap();
        assert!(ack.accepted);
        store
            .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
            .unwrap();
        store.apply_event(community, &event).unwrap();
        store.apply_event(community, &event).unwrap();
        session.shutdown().await;
        signer.lock().await;
        relay.stop();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.messages(community, channel, 100).unwrap().len(), 1);
    assert_eq!(
        store.draft(community, channel, None).unwrap(),
        "restart-safe draft"
    );
    assert!(store.pending_outbox(community).unwrap().is_empty());
}

#[test]
fn two_community_cache_and_read_state_are_isolated() {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "human".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let a = config
        .add_community("a".into(), "wss://a.example".into(), identity.id, false)
        .unwrap();
    let b = config
        .add_community("b".into(), "wss://b.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    let channel = Uuid::new_v4();
    let keys = Keys::generate();
    let event_a = buzz_sdk::build_message(channel, "community a", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    let event_b = buzz_sdk::build_message(channel, "community b", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(a, &event_a).unwrap();
    store.apply_event(b, &event_b).unwrap();
    assert_eq!(
        store.messages(a, channel, 10).unwrap()[0].content,
        "community a"
    );
    assert_eq!(
        store.messages(b, channel, 10).unwrap()[0].content,
        "community b"
    );
    store
        .advance_read(a, &identity.pubkey, "same", 10, true)
        .unwrap();
    store
        .advance_read(b, &identity.pubkey, "same", 20, true)
        .unwrap();
    assert_eq!(
        store.read_contexts(a, &identity.pubkey, false).unwrap()["same"],
        10
    );
    assert_eq!(
        store.read_contexts(b, &identity.pubkey, false).unwrap()["same"],
        20
    );
    let blobs = split(BTreeMap::from([("same".into(), 20)]), "client").unwrap();
    assert_eq!(
        blobs[0],
        ReadStateBlob {
            v: 1,
            client_id: "client".into(),
            contexts: BTreeMap::from([("same".into(), 20)])
        }
    );
    store.purge_community(a).unwrap();
    assert!(store.messages(a, channel, 10).unwrap().is_empty());
    assert_eq!(store.messages(b, channel, 10).unwrap().len(), 1);
}
