use bzz::{domain::Message, ui::timeline::TimelineState};
use uuid::Uuid;
fn message(id: &str) -> Message {
    Message {
        event_id: id.into(),
        channel_id: Uuid::nil(),
        pubkey: "a".repeat(64),
        created_at: 1,
        content: id.into(),
        attachments: Vec::new(),
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        pending: false,
        rejected: None,
    }
}
#[test]
fn history_anchor_does_not_jump_on_new_message() {
    let mut state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: false,
        newer: 0,
    };
    let messages = vec![message("a"), message("b")];
    state.reconcile(&messages);
    assert_eq!(state.selected_event.as_deref(), Some("a"));
}
#[test]
fn live_bottom_tracks_latest() {
    let mut state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: true,
        newer: 0,
    };
    let messages = vec![message("a"), message("b")];
    state.reconcile(&messages);
    assert_eq!(state.selected_event.as_deref(), Some("b"));
}
