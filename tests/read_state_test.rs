use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    store::{Store, models::ReadSlotRecord},
    sync::read_state::{ReadStateBlob, build_events, decrypt_event},
};
use nostr::Keys;
use std::collections::BTreeMap;

#[tokio::test]
async fn encrypted_read_state_round_trips_to_self() {
    let signer = SignerHandle::spawn(Keys::generate());
    let contexts = BTreeMap::from([("channel".into(), 42)]);
    let events = build_events(contexts.clone(), "client", &["slot".into()], &signer, 0)
        .await
        .unwrap();
    let blob = decrypt_event(&events[0], &signer).await.unwrap();
    assert_eq!(
        blob,
        ReadStateBlob {
            v: 1,
            client_id: "client".into(),
            contexts
        }
    );
    signer.lock().await;
}

#[test]
fn remote_slot_collision_rotates_the_local_slot_without_regressing_time() {
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "me".into(),
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
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let (client, slots, _) = store
        .ensure_local_read_slots(community, &identity.pubkey, "local", "slot-a")
        .unwrap();
    assert_eq!(client, "local");
    assert_eq!(slots, ["slot-a"]);
    store
        .record_read_slot(&ReadSlotRecord {
            community_id: community,
            pubkey: identity.pubkey.clone(),
            slot_id: "slot-a".into(),
            client_id: "remote".into(),
            event_id: "b".repeat(64),
            event_created_at: 50,
            local: false,
        })
        .unwrap();
    let (client, slots, max_seen) = store
        .ensure_local_read_slots(community, &identity.pubkey, "replacement", "slot-b")
        .unwrap();
    assert_eq!(client, "replacement");
    assert_eq!(slots, ["slot-b"]);
    assert_eq!(max_seen, 50);
}

#[test]
fn merge_order_does_not_reduce_markers() {
    let sources = [
        ReadStateBlob {
            v: 1,
            client_id: "a".into(),
            contexts: BTreeMap::from([("x".into(), 1)]),
        },
        ReadStateBlob {
            v: 1,
            client_id: "b".into(),
            contexts: BTreeMap::from([("x".into(), 99)]),
        },
    ];
    for order in [[0, 1], [1, 0]] {
        let mut merged = ReadStateBlob {
            v: 1,
            client_id: "m".into(),
            contexts: BTreeMap::new(),
        };
        for index in order {
            merged.merge(&sources[index]);
        }
        assert_eq!(merged.contexts["x"], 99);
    }
}
