use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::{Store, models::MessageSearchQuery},
};
use criterion::{Criterion, criterion_group, criterion_main};
use nostr::{EventBuilder, Keys, Kind, Tag};
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
    let relay = Keys::generate();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    let metadata = EventBuilder::new(Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["name", "bench"]).unwrap(),
            Tag::parse(["t", "stream"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &metadata).unwrap();
    let mut membership_tags = vec![
        Tag::parse(["d", &channel.to_string()]).unwrap(),
        Tag::parse(["p", &identity.pubkey]).unwrap(),
    ];
    for index in 0..1_000 {
        membership_tags.push(Tag::parse(["p", &format!("{index:064x}")]).unwrap());
    }
    let membership = EventBuilder::new(Kind::Custom(39_002), "")
        .tags(membership_tags)
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community, &membership).unwrap();
    for index in 0..100_000 {
        let token = if index % 1_000 == 0 {
            " benchmarkneedle"
        } else {
            ""
        };
        let event = buzz_sdk::build_message(
            channel,
            &format!("message {index}{token}"),
            None,
            &[],
            false,
            &[],
        )
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
        store.apply_event(community, &event).unwrap();
    }
    c.bench_function("query latest 500 of 100k", |bench| {
        bench.iter(|| black_box(store.messages(community, channel, 500).unwrap()))
    });
    let search = MessageSearchQuery {
        fts_query: r#""benchmarkneedle"*"#.into(),
        author: None,
        channel_id: None,
        since: None,
        until: None,
        limit: 100,
    };
    c.bench_function("search FTS5 over 100k", |bench| {
        bench.iter(|| {
            black_box(
                store
                    .search_messages(community, &identity.pubkey, &search)
                    .unwrap(),
            )
        })
    });
    c.bench_function("project Inbox over 100k", |bench| {
        bench.iter(|| black_box(store.inbox_items(community, &identity.pubkey).unwrap()))
    });
    c.bench_function("mention candidates over 1k cached members", |bench| {
        bench.iter(|| {
            black_box(
                store
                    .mention_candidates(community, channel, &identity.pubkey, "000")
                    .unwrap(),
            )
        })
    });
}
criterion_group!(benches, bench_store);
criterion_main!(benches);
