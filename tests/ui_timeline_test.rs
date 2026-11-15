use std::collections::HashMap;

use bzz::{
    domain::Message,
    ui::{
        theme::Theme,
        timeline::{self, TimelineState, avatar_marker},
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
        delivery: bzz::domain::DeliveryState::Delivered,
        system: None,
        sent_from_thread: None,
        broadcast: false,
        emoji: Vec::new(),
        edited: false,
    }
}
#[test]
fn textual_avatar_marker_is_stable_and_control_free() {
    let key = "a".repeat(64);
    assert_eq!(avatar_marker(&key, "Renan"), avatar_marker(&key, "Renan"));
    assert_eq!(avatar_marker(&key, "\u{1b}[31m"), "[●3]");
    assert!(!avatar_marker(&key, "Renan").contains('\u{1b}'));
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
fn fully_visible_detached_timeline_is_still_at_the_live_edge() {
    let state = TimelineState {
        selected_event: Some("a".into()),
        at_live_bottom: false,
        viewport_height: 10,
        content_height: 2,
        ..TimelineState::default()
    };

    assert!(state.visible_at_live_edge());
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

fn rendered(message: Message) -> String {
    let mut state = TimelineState {
        at_live_bottom: true,
        ..TimelineState::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal
        .draw(|frame| {
            timeline::render(
                frame,
                frame.area(),
                &[message],
                &HashMap::new(),
                &HashMap::new(),
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
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_edited_message_shows_the_marker_after_its_body() {
    let edited = rendered(Message {
        content: "fixed typo".into(),
        edited: true,
        ..message("a")
    });
    assert!(edited.contains("fixed typo (edited)"), "{edited}");
    let plain = rendered(Message {
        content: "fixed typo".into(),
        ..message("a")
    });
    assert!(!plain.contains("(edited)"), "{plain}");
    let deleted = rendered(Message {
        deleted: true,
        edited: true,
        ..message("a")
    });
    assert!(!deleted.contains("(edited)"), "{deleted}");
}

/// A relay's notice about a member, as it arrives (kind 40099).
fn notice(id: &str, at: u64, kind: &str, actor: &str, target: &str) -> Message {
    let content = serde_json::json!({"type": kind, "actor": actor, "target": target}).to_string();
    Message {
        created_at: at,
        system: Some(bzz::protocol::system::parse(&content)),
        content,
        ..message(id)
    }
}

fn rendered_all(messages: &[Message]) -> Vec<String> {
    let mut state = TimelineState {
        at_live_bottom: true,
        ..TimelineState::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).unwrap();
    terminal
        .draw(|frame| {
            timeline::render(
                frame,
                frame.area(),
                messages,
                &HashMap::new(),
                &HashMap::new(),
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
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// From the run against Buzz 1.0: making an agent "Viewing only" and then
/// "Participating" again showed in bzz's timeline as "Scout joined · added
/// by Owner Six", twice more. The relay writes the same notice for a change
/// of a member's role as for a join. A notice about someone the timeline
/// shows to be a member already is a change, and is shown as one.
#[test]
fn a_change_of_a_members_role_is_shown_as_a_change_not_as_a_join() {
    let (owner, scout, other) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
    let (owner_name, scout_name, other_name) = ("aaaaaaaa…aaaa", "bbbbbbbb…bbbb", "cccccccc…cccc");
    let messages = [
        // The owner adds Scout: a join.
        notice("n1", 10, "member_joined", &owner, &scout),
        // "Viewing only", then "Participating": two changes.
        notice("n2", 20, "member_joined", &owner, &scout),
        notice("n3", 30, "member_joined", &owner, &scout),
        // Someone else joins by himself meanwhile: a join.
        notice("n4", 40, "member_joined", &other, &other),
        // Scout is removed and added again: a join again, then a change.
        notice("n5", 50, "member_removed", &owner, &scout),
        notice("n6", 60, "member_joined", &owner, &scout),
        notice("n7", 70, "member_joined", &owner, &scout),
    ];
    assert_eq!(
        {
            let mut changes = timeline::role_changes(&messages)
                .into_iter()
                .collect::<Vec<_>>();
            changes.sort_unstable();
            changes
        },
        ["n2", "n3", "n7"]
    );
    let lines = rendered_all(&messages);
    let said = lines
        .iter()
        .filter(|line| line.contains('·'))
        .map(|line| {
            // Between the frame and the time.
            let text = line.trim_matches(|c: char| c == '│' || c.is_whitespace());
            text.trim_start_matches('·')
                .trim()
                .rsplit_once(" · ")
                .map_or(text, |(summary, _)| summary)
                .to_owned()
        })
        .collect::<Vec<_>>();
    let joined = format!("{scout_name} joined · added by {owner_name}");
    let changed = format!("{scout_name}'s access to this channel was changed by {owner_name}");
    assert_eq!(
        said,
        [
            joined.clone(),
            changed.clone(),
            changed.clone(),
            format!("{other_name} joined"),
            format!("{scout_name} was removed by {owner_name}"),
            joined,
            changed,
        ],
        "{lines:#?}"
    );

    // The creator of a channel is a member from its creation.
    let created = Message {
        created_at: 5,
        system: Some(bzz::protocol::system::parse(
            &serde_json::json!({"type": "channel_created", "actor": owner}).to_string(),
        )),
        ..message("n0")
    };
    let about_creator = [created, notice("n8", 80, "member_joined", &other, &owner)];
    assert_eq!(
        timeline::role_changes(&about_creator)
            .into_iter()
            .collect::<Vec<_>>(),
        ["n8"]
    );
    // One whose joining is older than what is loaded still reads as a join.
    let alone = [notice("n9", 90, "member_joined", &owner, &scout)];
    assert!(timeline::role_changes(&alone).is_empty());
    assert!(
        rendered_all(&alone)
            .iter()
            .any(|line| line.contains(&format!("{scout_name} joined · added by {owner_name}")))
    );
}
