use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::Store,
};
use criterion::{Criterion, criterion_group, criterion_main};
use nostr::Keys;
use std::hint::black_box;
use uuid::Uuid;

fn bench_store(c: &mut Criterion) {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "bench".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "bench".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "bench".into(),
            "wss://bench.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    store.sync_config(&config).unwrap();
    let channel = Uuid::new_v4();
    let keys = Keys::generate();
    for index in 0..10_000 {
        let event =
            buzz_sdk::build_message(channel, &format!("message {index}"), None, &[], false, &[])
                .unwrap()
                .sign_with_keys(&keys)
                .unwrap();
        store.apply_event(community, &event).unwrap();
    }
    c.bench_function("query latest 500 of 10k", |bench| {
        bench.iter(|| black_box(store.messages(community, channel, 500).unwrap()))
    });
}
criterion_group!(benches, bench_store);
criterion_main!(benches);
