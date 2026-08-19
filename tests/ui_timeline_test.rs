use std::collections::HashMap;

use bzz::{
    domain::Message,
    ui::{
        theme::Theme,
        timeline::{self, TimelineState},
    },
};
use ratatui::{Terminal, backend::TestBackend};
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
        ..TimelineState::default()
    };
    let messages = vec![message("a"), message("b")];
    state.reconcile(&messages);
    assert_eq!(state.selected_event.as_deref(), Some("a"));
}
#[test]
fn detached_scroll_does_not_change_selected_event() {
    let mut state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: false,
        viewport_height: 4,
        content_height: 20,
        ..TimelineState::default()
    };
    state.scroll_by(5);
    assert_eq!(state.selected_event.as_deref(), Some("a"));
    assert_eq!(state.scroll, 5);
    assert!(!state.at_live_bottom);
}

#[test]
fn wrapped_rows_are_measured_for_detached_scroll() {
    let mut state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: true,
        ..TimelineState::default()
    };
    let message = Message {
        content: "a deliberately long message that wraps across a narrow terminal viewport".into(),
        ..message("a")
    };
    let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
    terminal
        .draw(|frame| {
            timeline::render(
                frame,
                frame.area(),
                &[message],
                &HashMap::new(),
                &HashMap::new(),
                &mut state,
                "timeline",
                &Theme::default(),
                true,
                None,
            );
        })
        .unwrap();
    assert!(state.content_height > state.viewport_height);
    let selected = state.selected_event.clone();
    state.scroll_by(-1);
    assert_eq!(state.selected_event, selected);
    assert!(!state.at_live_bottom);
}

#[test]
fn live_bottom_tracks_latest() {
    let mut state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: true,
        newer: 0,
        ..TimelineState::default()
    };
    let messages = vec![message("a"), message("b")];
    state.reconcile(&messages);
    assert_eq!(state.selected_event.as_deref(), Some("b"));
}
