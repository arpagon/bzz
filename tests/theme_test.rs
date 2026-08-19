use std::collections::{HashMap, HashSet};

use bzz::{
    domain::{Channel, ChannelKind, Message, Profile, Reaction, Visibility},
    ui::{
        sidebar,
        state::ViewportState,
        theme::{Theme, ThemeRegistry},
        timeline::{self, TimelineState},
    },
};
use ratatui::{Terminal, backend::TestBackend};
use uuid::Uuid;

#[test]
fn every_builtin_theme_renders_core_surfaces_at_supported_sizes() {
    let channel_id = Uuid::new_v4();
    let self_pubkey = "a".repeat(64);
    let event_id = "b".repeat(64);
    let channels = vec![Channel {
        id: channel_id,
        name: "theme-test".into(),
        about: String::new(),
        kind: ChannelKind::Stream,
        visibility: Visibility::Public,
        is_member: true,
        is_hidden: false,
        member_count: 1,
        last_event_at: Some(1),
    }];
    let messages = vec![Message {
        event_id: event_id.clone(),
        channel_id,
        pubkey: self_pubkey.clone(),
        created_at: 1,
        content: "**bold** [safe](https://example.test) `code`".into(),
        attachments: Vec::new(),
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        pending: true,
        rejected: None,
    }];
    let profiles = HashMap::from([(
        self_pubkey.clone(),
        Profile {
            pubkey: self_pubkey.clone(),
            display_name: Some("tester".into()),
            name: None,
            picture: None,
            nip05: None,
            about: None,
            event_id: "d".repeat(64),
            created_at: 1,
        },
    )]);
    let reactions = HashMap::from([(
        event_id.clone(),
        vec![Reaction {
            event_id: "c".repeat(64),
            target_event_id: event_id.clone(),
            pubkey: self_pubkey.clone(),
            emoji: "+".into(),
            created_at: 2,
            deleted: false,
        }],
    )]);
    let mut state = TimelineState {
        selected_event: Some(event_id),
        at_live_bottom: true,
        newer: 0,
        ..TimelineState::default()
    };

    for entry in ThemeRegistry::entries() {
        let theme = Theme::builtin(entry.id).unwrap();
        for (width, height) in [(50, 12), (80, 24), (120, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let [left, right] = ratatui::layout::Layout::horizontal([
                        ratatui::layout::Constraint::Length(20),
                        ratatui::layout::Constraint::Fill(1),
                    ])
                    .areas(frame.area());
                    sidebar::render(
                        frame,
                        left,
                        &channels,
                        &ViewportState {
                            selected_id: Some(channel_id.to_string()),
                            ..ViewportState::default()
                        },
                        &HashSet::from([channel_id]),
                        &theme,
                        true,
                    );
                    timeline::render(
                        frame,
                        right,
                        &messages,
                        &profiles,
                        &reactions,
                        &mut state,
                        "theme-test",
                        &theme,
                        false,
                        Some(&self_pubkey),
                    );
                })
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                text.contains("theme-test"),
                "theme {} at {width}x{height}",
                entry.id
            );
            assert!(!text.contains('\x1b'), "theme {} emitted ESC", entry.id);
            assert!(!text.contains('\u{7}'), "theme {} emitted BEL", entry.id);
        }
    }
}
