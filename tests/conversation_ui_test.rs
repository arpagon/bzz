use bzz::domain::{Channel, Visibility};
use bzz::ui::{composer::Composer, finder};
#[test]
fn composer_preserves_multiline_and_clears_only_on_send() {
    let mut composer = Composer::default();
    for character in "hello".chars() {
        composer.insert(character);
    }
    composer.newline();
    for character in "world".chars() {
        composer.insert(character);
    }
    assert_eq!(composer.take_for_send().as_deref(), Some("hello\nworld"));
    assert!(composer.body.is_empty());
}
#[test]
fn finder_ranks_joined_channels_before_open_when_query_empty() {
    let channels = vec![
        Channel {
            id: uuid::Uuid::new_v4(),
            name: "open".into(),
            about: String::new(),
            visibility: Visibility::Public,
            is_member: false,
            is_hidden: false,
            member_count: 0,
            last_event_at: None,
        },
        Channel {
            id: uuid::Uuid::new_v4(),
            name: "joined".into(),
            about: String::new(),
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 0,
            last_event_at: Some(1),
        },
        Channel {
            id: uuid::Uuid::new_v4(),
            name: "recent joined".into(),
            about: String::new(),
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 0,
            last_event_at: Some(2),
        },
    ];
    let ranked = finder::rank("", &channels);
    assert_eq!(ranked[0].name, "recent joined");
    assert_eq!(ranked[1].name, "joined");
    assert_eq!(ranked[2].name, "open");
}
