use bzz::{
    domain::{
        Channel, ChannelKind, InboxCategory, InboxItem, Message, Profile, Reaction, SearchResult,
        SearchResultKind, Visibility,
    },
    media::{Attachment, MediaKind},
    ui::{
        dm_picker::{self, DmPickerState},
        inbox::{self, InboxState},
        search::{self, SearchState},
        sidebar,
        state::ViewportState,
        theme::Theme,
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
            kind: ChannelKind::Stream,
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
            kind: ChannelKind::Stream,
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
    let mut timeline_state = TimelineState {
        selected_event: Some("a".repeat(64)),
        at_live_bottom: true,
        newer: 0,
        ..TimelineState::default()
    };
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
                &ViewportState {
                    selected_id: Some(channel.to_string()),
                    ..ViewportState::default()
                },
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
                &mut timeline_state,
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
    let mut timeline_state = TimelineState::default();
    terminal
        .draw(|frame| {
            timeline::render(
                frame,
                frame.area(),
                &[message],
                &HashMap::new(),
                &HashMap::new(),
                &mut timeline_state,
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
        true,
        false,
        18,
        60,
    );
    assert_eq!(configured.sidebar.unwrap().width, 18);
    let hidden_community = bzz::ui::layout::panes(
        ratatui::layout::Rect::new(0, 0, 120, 25),
        false,
        true,
        false,
        18,
        60,
    );
    assert!(hidden_community.community.is_none());
    for (width, height) in [(50, 12), (69, 15), (100, 25), (140, 40)] {
        let panes = bzz::ui::layout::panes(
            ratatui::layout::Rect::new(0, 0, width, height),
            true,
            true,
            true,
            28,
            44,
        );
        assert!(panes.timeline.right() <= width);
        assert!(panes.status.bottom() <= height);
    }
}

#[test]
fn inbox_search_and_dm_picker_render_safe_wide_and_narrow_states() {
    let channel = Uuid::new_v4();
    let pubkey = "a".repeat(64);
    let profile = Profile {
        pubkey: pubkey.clone(),
        display_name: Some("Generic Person".into()),
        name: None,
        picture: None,
        nip05: None,
        about: None,
        event_id: "b".repeat(64),
        created_at: 1,
    };
    let profiles = HashMap::from([(pubkey.clone(), profile)]);
    let items = vec![InboxItem {
        conversation_id: format!("dm:{channel}"),
        categories: vec![InboxCategory::Dm, InboxCategory::Mention],
        event_id: Some("c".repeat(64)),
        channel_id: Some(channel),
        thread_root: None,
        sender_pubkey: Some(pubkey.clone()),
        created_at: 1,
        preview: "safe preview\u{1b}]52;bad".into(),
        unread_count: 1,
        draft_count: 0,
        forced_unread: false,
    }];
    let mut inbox_state = InboxState::default();
    inbox_state.reconcile(&items);
    let theme = Theme::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            inbox::render(frame, area, &items, &profiles, &inbox_state, &theme, false);
        })
        .unwrap();
    let wide = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(wide.contains("Inbox"));
    assert!(wide.contains("Generic Person"));
    assert!(!wide.contains('\u{1b}'));

    let search_state = SearchState {
        query: "generic".into(),
        results: vec![SearchResult {
            stable_id: format!("channel:{channel}"),
            kind: SearchResultKind::Channel,
            label: "general".into(),
            detail: "workspace channel".into(),
            channel_id: Some(channel),
            event_id: None,
            thread_root: None,
            pubkey: None,
            created_at: 1,
            remote_rank: None,
        }],
        selected_id: Some(format!("channel:{channel}")),
        ..SearchState::default()
    };
    terminal
        .draw(|frame| search::render(frame, frame.area(), &search_state, &theme))
        .unwrap();
    let search_text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(search_text.contains("Search"));
    assert!(search_text.contains("general"));

    let mut dm_state = DmPickerState::default();
    dm_state.reconcile(&profiles, &"f".repeat(64));
    terminal
        .draw(|frame| {
            dm_picker::render(
                frame,
                frame.area(),
                &dm_state,
                &profiles,
                &"f".repeat(64),
                &theme,
            );
        })
        .unwrap();
    let dm_text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(dm_text.contains("not end-to-end encrypted"));
    assert!(dm_text.contains("Generic Person"));
}
