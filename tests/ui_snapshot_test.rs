use bzz::{
    domain::{Channel, Message, Reaction, Visibility},
    media::{Attachment, MediaKind},
    ui::{
        sidebar,
        timeline::{self, TimelineState},
    },
};
use ratatui::{Terminal, backend::TestBackend};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[test]
fn timeline_and_sidebar_render_deterministically_without_control_bytes() {
    let channel = Uuid::new_v4();
    let channels = vec![
        Channel {
            id: channel,
            name: "general".into(),
            about: "topic".into(),
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 3,
            last_event_at: Some(1),
        },
        Channel {
            id: Uuid::new_v4(),
            name: "discover-only".into(),
            about: String::new(),
            visibility: Visibility::Public,
            is_member: false,
            is_hidden: false,
            member_count: 1,
            last_event_at: None,
        },
    ];
    let messages = vec![Message {
        event_id: "a".repeat(64),
        channel_id: channel,
        pubkey: "b".repeat(64),
        created_at: 1,
        content: "hello **world**\x1b]52;c;bad\x07".into(),
        attachments: Vec::new(),
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        pending: true,
        rejected: None,
    }];
    let reactions = HashMap::from([(
        "a".repeat(64),
        vec![
            Reaction {
                event_id: "c".repeat(64),
                target_event_id: "a".repeat(64),
                pubkey: "b".repeat(64),
                emoji: "+".into(),
                created_at: 2,
                deleted: false,
            },
            Reaction {
                event_id: "d".repeat(64),
                target_event_id: "a".repeat(64),
                pubkey: "e".repeat(64),
                emoji: "+".into(),
                created_at: 3,
                deleted: true,
            },
        ],
    )]);
    let theme = bzz::ui::theme::Theme::default();
    let self_pubkey = "b".repeat(64);
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let [left, right] = ratatui::layout::Layout::horizontal([
                ratatui::layout::Constraint::Length(25),
                ratatui::layout::Constraint::Fill(1),
            ])
            .areas(frame.area());
            sidebar::render(
                frame,
                left,
                &channels,
                0,
                &HashSet::from([channel]),
                &theme,
                true,
            );
            timeline::render(
                frame,
                right,
                &messages,
                &HashMap::new(),
                &reactions,
                &TimelineState {
                    selected_event: Some("a".repeat(64)),
                    at_live_bottom: true,
                    newer: 0,
                },
                "general",
                &theme,
                true,
                Some(&self_pubkey),
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("general"));
    assert!(!text.contains("discover-only"));
    assert!(text.contains("hello"));
    assert!(text.contains("+ 1"));
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x07'));
}

#[test]
fn attachment_cards_remain_visible_without_a_graphics_protocol() {
    let channel = Uuid::new_v4();
    let message = Message {
        event_id: "a".repeat(64),
        channel_id: channel,
        pubkey: "b".repeat(64),
        created_at: 1,
        content: "with media".into(),
        attachments: vec![Attachment {
            index: 0,
            url: format!("https://buzz.example/media/{}.png", "c".repeat(64)),
            mime: "image/png".into(),
            sha256: "c".repeat(64),
            size: 1024,
            width: Some(1),
            height: Some(1),
            alt: Some("safe\x1b]52;bad\x07 image".into()),
            blurhash: None,
            thumb: None,
            poster: None,
            filename: None,
            duration_millis: None,
            kind: MediaKind::Image,
            spoiler: false,
            error: None,
        }],
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        pending: false,
        rejected: None,
    };
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            timeline::render(
                frame,
                frame.area(),
                &[message],
                &HashMap::new(),
                &HashMap::new(),
                &TimelineState::default(),
                "media",
                &bzz::ui::theme::Theme::default(),
                true,
                None,
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
    assert!(text.contains("safe"));
    assert!(text.contains("image/png"));
    assert!(text.contains("1.0 KiB"));
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x07'));
}

#[test]
fn narrow_terminal_layout_does_not_overlap() {
    let configured = bzz::ui::layout::panes(
        ratatui::layout::Rect::new(0, 0, 120, 25),
        true,
        false,
        18,
        60,
    );
    assert_eq!(configured.sidebar.unwrap().width, 18);
    for (width, height) in [(50, 12), (69, 15), (100, 25), (140, 40)] {
        let panes = bzz::ui::layout::panes(
            ratatui::layout::Rect::new(0, 0, width, height),
            true,
            true,
            28,
            44,
        );
        assert!(panes.timeline.right() <= width);
        assert!(panes.status.bottom() <= height);
    }
}
