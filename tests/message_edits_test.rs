//! v0.14 M1: message edits (kind 40003), following upstream
//! `docs/effective-message-edits.md`.
use bzz::{
    config::{CommunityConfig, Config, IdentityConfig, KeyBackend},
    store::Store,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use uuid::Uuid;

struct Fixture {
    store: Store,
    community: Uuid,
    channel: Uuid,
}

fn fixture() -> Fixture {
    let viewer = Keys::generate();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity,
            label: "edits".into(),
            pubkey: viewer.public_key().to_hex(),
            backend: KeyBackend::Keychain,
            key_ref: "edits".into(),
        }],
        communities: vec![CommunityConfig {
            id: community,
            label: "edits".into(),
            relay_url: "wss://buzz.example/".into(),
            identity_id: identity,
            allow_insecure_localhost: false,
            theme: None,
            identity_adapter: None,
        }],
        default_community: Some(community),
        ..Config::default()
    };
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    Fixture {
        store,
        community,
        channel: Uuid::new_v4(),
    }
}

fn imeta(hash: &str) -> Tag {
    Tag::parse(vec![
        "imeta".to_owned(),
        format!("url https://buzz.example/media/{hash}.png"),
        "m image/png".into(),
        format!("x {hash}"),
        "size 68".into(),
        "dim 1x1".into(),
    ])
    .unwrap()
}

fn message(keys: &Keys, channel: Uuid, text: &str, media: &[&str], at: u64) -> Event {
    let mut tags = vec![Tag::parse(["h", &channel.to_string()]).unwrap()];
    tags.extend(media.iter().map(|hash| imeta(hash)));
    let body = media.iter().fold(text.to_owned(), |body, hash| {
        format!("{body}\n![image](https://buzz.example/media/{hash}.png)")
    });
    EventBuilder::new(Kind::Custom(9), body)
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .unwrap()
}

fn edit(keys: &Keys, channel: Uuid, target: &str, text: &str, media: &[&str], at: u64) -> Event {
    let mut tags = vec![
        Tag::parse(["h", &channel.to_string()]).unwrap(),
        Tag::parse(["e", target]).unwrap(),
    ];
    tags.extend(media.iter().map(|hash| imeta(hash)));
    let body = media.iter().fold(text.to_owned(), |body, hash| {
        format!("{body}\n![image](https://buzz.example/media/{hash}.png)")
    });
    EventBuilder::new(Kind::Custom(40_003), body)
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .unwrap()
}

fn shown(fixture: &Fixture) -> bzz::domain::Message {
    let mut messages = fixture
        .store
        .messages(fixture.community, fixture.channel, 10)
        .unwrap();
    assert_eq!(messages.len(), 1, "edits never appear as rows");
    messages.remove(0)
}

#[test]
fn the_latest_edit_governs_text_and_marks_the_message() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let original = message(&alice, fixture.channel, "first", &[], 10);
    let id = original.id.to_hex();
    let (community, channel) = (fixture.community, fixture.channel);
    fixture.store.apply_event(community, &original).unwrap();
    assert!(!shown(&fixture).edited);
    let later = edit(&alice, channel, &id, "third", &[], 30);
    let earlier = edit(&alice, channel, &id, "second", &[], 20);
    // Arrival order does not matter: the greatest created_at wins.
    fixture.store.apply_event(community, &later).unwrap();
    fixture.store.apply_event(community, &earlier).unwrap();
    let message = shown(&fixture);
    assert_eq!(message.content, "third");
    assert!(message.edited);
}

#[test]
fn ties_break_on_the_greatest_event_id_and_uppercase_targets_match() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let original = message(&alice, fixture.channel, "first", &[], 10);
    let id = original.id.to_hex();
    let (community, channel) = (fixture.community, fixture.channel);
    fixture.store.apply_event(community, &original).unwrap();
    let a = edit(&alice, channel, &id.to_ascii_uppercase(), "a", &[], 20);
    let b = edit(&alice, channel, &id, "b", &[], 20);
    fixture.store.apply_event(community, &a).unwrap();
    fixture.store.apply_event(community, &b).unwrap();
    let winner = if a.id.to_hex() > b.id.to_hex() {
        "a"
    } else {
        "b"
    };
    assert_eq!(shown(&fixture).content, winner);
}

#[test]
fn an_edit_replaces_the_attachment_set_and_an_empty_set_removes_them() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let original = message(&alice, fixture.channel, "photo", &[&first], 10);
    let id = original.id.to_hex();
    let (community, channel) = (fixture.community, fixture.channel);
    fixture.store.apply_event(community, &original).unwrap();
    assert_eq!(shown(&fixture).attachments.len(), 1);

    let swap = edit(&alice, channel, &id, "other photo", &[&second], 20);
    fixture.store.apply_event(community, &swap).unwrap();
    let message = shown(&fixture);
    assert_eq!(message.content, "other photo");
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].sha256, second);

    let cleared = edit(&alice, channel, &id, "no photo", &[], 30);
    fixture.store.apply_event(community, &cleared).unwrap();
    let message = shown(&fixture);
    assert_eq!(message.content, "no photo");
    assert!(
        message.attachments.is_empty(),
        "an empty imeta set never falls back to the original's"
    );
}

#[test]
fn foreign_cross_channel_and_deleted_edits_do_not_apply() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let mallory = Keys::generate();
    let original = message(&alice, fixture.channel, "first", &[], 10);
    let id = original.id.to_hex();
    let (community, channel) = (fixture.community, fixture.channel);
    fixture.store.apply_event(community, &original).unwrap();
    let foreign = edit(&mallory, channel, &id, "forged", &[], 40);
    let elsewhere = edit(&alice, Uuid::new_v4(), &id, "elsewhere", &[], 50);
    let live = edit(&alice, channel, &id, "live", &[], 20);
    let retracted = edit(&alice, channel, &id, "retracted", &[], 30);
    for event in [&foreign, &elsewhere, &live, &retracted] {
        fixture.store.apply_event(community, event).unwrap();
    }
    assert_eq!(shown(&fixture).content, "retracted");
    let deletion = EventBuilder::new(Kind::Custom(5), "")
        .tags([
            Tag::parse(["e", &retracted.id.to_hex()]).unwrap(),
            Tag::parse(["h", &channel.to_string()]).unwrap(),
        ])
        .custom_created_at(nostr::Timestamp::from(60))
        .sign_with_keys(&alice)
        .unwrap();
    fixture.store.apply_event(community, &deletion).unwrap();
    assert_eq!(shown(&fixture).content, "live", "a deleted edit drops out");
}

#[test]
fn an_edit_that_arrives_before_its_original_still_applies() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let original = message(&alice, fixture.channel, "first", &[], 10);
    let (community, channel) = (fixture.community, fixture.channel);
    let early = edit(&alice, channel, &original.id.to_hex(), "edited", &[], 20);
    fixture.store.apply_event(community, &early).unwrap();
    fixture.store.apply_event(community, &original).unwrap();
    assert_eq!(shown(&fixture).content, "edited");
}

#[test]
fn threads_show_the_edited_text_too() {
    let mut fixture = fixture();
    let alice = Keys::generate();
    let root = message(&alice, fixture.channel, "root", &[], 10);
    let root_id = root.id.to_hex();
    let (community, channel) = (fixture.community, fixture.channel);
    fixture.store.apply_event(community, &root).unwrap();
    let fix = edit(&alice, channel, &root_id, "root, fixed", &[], 20);
    fixture.store.apply_event(community, &fix).unwrap();
    let thread = fixture.store.thread(community, &root_id, 10).unwrap();
    assert_eq!(thread[0].content, "root, fixed");
    assert!(thread[0].edited);
}
