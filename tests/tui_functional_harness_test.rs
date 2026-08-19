//! Functional journeys for the pure, recording-only TestBackend harness.

#[path = "support/tui_harness.rs"]
mod tui_harness;

use bzz::{
    domain::{InboxCategory, InboxItem, Message},
    ui::{action::InboxEffect, state::FocusSurface},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use uuid::Uuid;

use tui_harness::InboxHarness;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fixture() -> (Vec<InboxItem>, Vec<Message>) {
    let channel = Uuid::new_v4();
    let event_id = "a".repeat(64);
    (
        vec![InboxItem {
            conversation_id: format!("event:{event_id}"),
            categories: vec![InboxCategory::Mention],
            event_id: Some(event_id.clone()),
            channel_id: Some(channel),
            thread_root: None,
            sender_pubkey: Some("b".repeat(64)),
            created_at: 1,
            preview: "generated Inbox work".into(),
            unread_count: 1,
            first_unread_event_id: Some(event_id.clone()),
            first_unread_at: Some(1),
            draft_count: 0,
            latest_draft_at: None,
            forced_unread: false,
        }],
        vec![Message {
            event_id,
            channel_id: channel,
            pubkey: "b".repeat(64),
            created_at: 1,
            content: "bounded generated context".into(),
            attachments: Vec::new(),
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            pending: false,
            rejected: None,
        }],
    )
}

#[test]
fn inbox_detail_and_reply_journey_emit_effects_without_acknowledgement() {
    let (items, messages) = fixture();
    let mut harness = InboxHarness::new(120, 32, items, messages);
    harness.render();
    assert!(
        harness
            .screen_text()
            .contains("opening does not acknowledge")
    );

    harness.send_key(key(KeyCode::Enter));
    assert_eq!(harness.inbox_focus(), FocusSurface::InboxDetail);
    assert_eq!(harness.effects.last(), Some(&InboxEffect::LoadDetail));

    harness.send_key(key(KeyCode::Char('i')));
    assert_eq!(harness.effects.last(), Some(&InboxEffect::OpenComposer));
    assert!(
        harness
            .effects
            .iter()
            .all(|effect| !matches!(effect, InboxEffect::MarkRead))
    );
    assert_eq!(harness.items[0].unread_count, 1);

    harness.send_key(key(KeyCode::Char('o')));
    assert_eq!(
        harness.effects.last(),
        Some(&InboxEffect::OpenCanonicalContext)
    );
}

#[test]
fn narrow_detail_back_returns_to_the_same_list_selection() {
    let (items, messages) = fixture();
    let expected = items[0].conversation_id.clone();
    let mut harness = InboxHarness::new(60, 20, items, messages);
    harness.render();
    assert!(harness.inbox.narrow_layout);

    harness.send_key(key(KeyCode::Enter));
    assert!(harness.inbox.narrow_detail);
    harness.render();
    assert!(harness.screen_text().contains("first unread"));

    harness.send_key(key(KeyCode::Esc));
    assert!(!harness.inbox.narrow_detail);
    assert_eq!(harness.inbox.selected_id(), Some(expected.as_str()));
    assert_eq!(harness.inbox_focus(), FocusSurface::InboxList);
}
