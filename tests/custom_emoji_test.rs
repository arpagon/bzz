//! v0.13 M5: custom emoji palette (30030 sets, 30180 entries) and NIP-30 tags.
use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
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

fn set(keys: &Keys, emoji: &[(&str, &str)], at: u64) -> nostr::Event {
    buzz_sdk::build_custom_emoji_set(
        &emoji
            .iter()
            .map(|(shortcode, url)| buzz_sdk::CustomEmoji {
                shortcode: (*shortcode).into(),
                url: (*url).into(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
    .custom_created_at(nostr::Timestamp::from(at))
    .sign_with_keys(keys)
    .unwrap()
}

#[test]
fn sets_replace_their_own_rows_and_newest_contribution_wins() {
    let (mut store, community) = store();
    let alice = Keys::generate();
    let bob = Keys::generate();
    store
        .apply_event(
            community,
            &set(
                &alice,
                &[
                    ("wave", "https://x.io/a-wave.png"),
                    ("old", "https://x.io/old.png"),
                ],
                10,
            ),
        )
        .unwrap();
    store
        .apply_event(
            community,
            &set(&bob, &[("wave", "https://x.io/b-wave.png")], 20),
        )
        .unwrap();
    let palette = store.emoji_palette(community).unwrap();
    assert_eq!(
        palette["wave"], "https://x.io/b-wave.png",
        "newest set wins"
    );
    assert_eq!(palette["old"], "https://x.io/old.png");
    // Alice replaces her set without `old`; an older replay changes nothing.
    store
        .apply_event(
            community,
            &set(&alice, &[("wave", "https://x.io/a2.png")], 30),
        )
        .unwrap();
    store
        .apply_event(
            community,
            &set(&alice, &[("old", "https://x.io/zombie.png")], 5),
        )
        .unwrap();
    let palette = store.emoji_palette(community).unwrap();
    assert!(
        !palette.contains_key("old"),
        "removed from the replaced set"
    );
    assert_eq!(palette["wave"], "https://x.io/a2.png");

    let entry = buzz_sdk::build_emoji_entry("party_parrot", "https://x.io/pp.gif")
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(40))
        .sign_with_keys(&bob)
        .unwrap();
    store.apply_event(community, &entry).unwrap();
    assert_eq!(
        store.emoji_palette(community).unwrap()["party_parrot"],
        "https://x.io/pp.gif"
    );
}

#[test]
fn messages_and_reactions_carry_their_own_emoji() {
    let (mut store, community) = store();
    let keys = Keys::generate();
    let channel = Uuid::new_v4();
    let message = buzz_sdk::build_message(
        channel,
        "hi :wave:",
        None,
        &[],
        false,
        &[],
        &[vec![
            "emoji".into(),
            "wave".into(),
            "https://x.io/w.png".into(),
        ]],
    )
    .unwrap()
    .sign_with_keys(&keys)
    .unwrap();
    store.apply_event(community, &message).unwrap();
    let stored = &store.messages(community, channel, 10).unwrap()[0];
    assert_eq!(stored.emoji.len(), 1);
    assert_eq!(stored.emoji[0].url, "https://x.io/w.png");

    let reaction = buzz_sdk::build_custom_emoji_reaction(message.id, "wave", "https://x.io/w.png")
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(community, &reaction).unwrap();
    let reactions = store.reactions(community, &message.id.to_hex()).unwrap();
    assert_eq!(reactions[0].emoji, ":wave:");
    assert_eq!(
        reactions[0].emoji_url.as_deref(),
        Some("https://x.io/w.png")
    );
}

#[test]
fn channel_scoped_or_mismatched_entries_are_ignored() {
    let (mut store, community) = store();
    let keys = Keys::generate();
    let scoped = EventBuilder::new(Kind::Custom(30_180), "x")
        .tags([
            Tag::parse(["d", "x"]).unwrap(),
            Tag::parse(["emoji", "x", "https://x.io/x.png"]).unwrap(),
            Tag::parse(["h", &Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    store.apply_event(community, &scoped).unwrap();
    assert!(store.emoji_palette(community).unwrap().is_empty());
}
