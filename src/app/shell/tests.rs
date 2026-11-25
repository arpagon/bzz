//! The shell inside bzz's own `App`, drawn to Ratatui's test backend: what
//! the frame says of a hidden pane, `z`, `Esc`, and each mouse gesture
//! against the hit map of the frame drawn (v0.17 L10 to L12, F7).

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{Channel, ChannelFacts, ChannelKind, DeliveryState, Message, Visibility},
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::{
        action::{WorkspaceEffect, WorkspaceState, reduce_workspace},
        hit_map::HitTarget,
        keymap::{KeyLookup, KeyMap, KeyScope, UiAction},
        shell::Pane,
        state::{ComposerTarget, FocusSurface, Overlay, PresentationState},
    },
};

use super::super::App;

fn channel(name: &str, kind: ChannelKind, visibility: Visibility) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        name: name.into(),
        about: String::new(),
        kind,
        visibility,
        is_member: true,
        is_hidden: false,
        member_count: 2,
        last_event_at: None,
        facts: ChannelFacts::default(),
    }
}

fn message(index: u64, channel: Uuid) -> Message {
    Message {
        event_id: format!("event-{index}"),
        channel_id: channel,
        pubkey: format!("{:064}", index % 3),
        created_at: 1_700_000_000 + index * 600,
        content: format!("message number {index}"),
        attachments: vec![],
        root_event_id: None,
        parent_event_id: None,
        deleted: false,
        delivery: DeliveryState::Delivered,
        system: None,
        sent_from_thread: None,
        broadcast: false,
        emoji: Vec::new(),
        edited: false,
    }
}

/// One community, `#general` with sixty messages, thirty more channels,
/// a DM with Scout, and a huddle's backing channel.
async fn fixture(temporary: &TempDir) -> App {
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let config = Config::default();
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let handle = StoreHandle::spawn(store).unwrap();
    let mut app = App::new(config, paths, handle).await.unwrap();
    let identity = crate::config::IdentityConfig {
        id: Uuid::new_v4(),
        label: "shell".into(),
        pubkey: "a".repeat(64),
        backend: crate::config::KeyBackend::Keychain,
        key_ref: "identity:shell".into(),
    };
    app.config.identities.push(identity.clone());
    app.config
        .add_community(
            "shell".into(),
            "wss://shell.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let general = channel("general", ChannelKind::Stream, Visibility::Public);
    let mut channels = vec![general.clone()];
    for index in 0..30 {
        channels.push(channel(
            &format!("room-{index:02}"),
            ChannelKind::Stream,
            Visibility::Public,
        ));
    }
    channels.push(channel("Scout", ChannelKind::Dm, Visibility::Private));
    channels.push(channel(
        "huddle-0a1b2c3d",
        ChannelKind::Stream,
        Visibility::Private,
    ));
    app.channels = channels;
    app.selected_channel = 0;
    app.messages = (0..60).map(|index| message(index, general.id)).collect();
    app.timeline.reconcile(&app.messages);
    app.timeline.at_live_bottom = true;
    let synced = app.config.clone();
    app.store
        .call(move |store| store.sync_config(&synced))
        .await
        .unwrap();
    app
}

fn draw(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn hit(app: &App, column: u16, row: u16) -> Option<HitTarget> {
    app.last_hit_map
        .as_ref()
        .and_then(|map| map.hit(column, row))
        .cloned()
}

#[tokio::test]
async fn the_frame_names_what_is_hidden_and_z_draws_the_conversation_alone() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    // 80 columns, a thread open and the focus back on the conversation:
    // the thread at 42 beside 38, the sidebar hidden and said so.
    app.thread_root = Some("event-59".into());
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    let rows = draw(&mut app, 80, 24);
    assert!(rows[0].contains("hidden · channels 2"), "{rows:#?}");
    assert!(rows[0].contains("context"), "{rows:#?}");
    let panes = app.shell.last_panes.unwrap();
    assert_eq!(panes.timeline.width, 38);
    // 60 columns, the thread with the focus takes the conversation's place.
    app.presentation.set_workspace_focus(FocusSurface::Context);
    let rows = draw(&mut app, 60, 20);
    assert!(
        rows[0].starts_with("┏ context") || rows[0].contains("context"),
        "{rows:#?}"
    );
    assert!(rows[0].contains("3"), "{rows:#?}");
    assert_eq!(app.shell.last_panes.unwrap().timeline.width, 0);
    // `z`: only the conversation and the dock, and the frame says how back.
    let mut state = WorkspaceState::new(PresentationState::default(), 0, 1, true, true, true);
    assert_eq!(
        reduce_workspace(&mut state, UiAction::ConversationOnly),
        WorkspaceEffect::ToggleConversationOnly
    );
    assert_eq!(state.presentation.focus, FocusSurface::Timeline);
    app.shell.conversation_only = true;
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    let rows = draw(&mut app, 160, 45);
    assert!(rows[0].contains("hidden · the other panes z"), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("room-01")), "{rows:#?}");
    assert!(
        !rows.iter().any(|row| row.contains("context ·")),
        "{rows:#?}"
    );
    app.shell.conversation_only = false;
    let rows = draw(&mut app, 160, 45);
    assert!(rows.iter().any(|row| row.contains("room-01")), "{rows:#?}");
}

#[test]
fn esc_with_nothing_open_does_not_ask_to_quit_and_q_does() {
    let keymap = KeyMap::builtin();
    let esc = [crate::ui::keymap::KeyChord::new(
        crossterm::event::KeyCode::Esc,
        KeyModifiers::NONE,
    )];
    for scope in [KeyScope::Workspace, KeyScope::Inbox] {
        assert_eq!(
            keymap.lookup(scope, &esc),
            KeyLookup::Action(UiAction::Back)
        );
    }
    // The overlays and the composer keep theirs.
    assert_eq!(
        keymap.lookup(KeyScope::Overlay, &esc),
        KeyLookup::Action(UiAction::BackOrQuit)
    );
    assert_eq!(
        keymap.lookup(KeyScope::Composer, &esc),
        KeyLookup::Action(UiAction::BackOrQuit)
    );
    let fresh = || WorkspaceState::new(PresentationState::default(), 0, 1, true, true, false);
    assert_eq!(
        reduce_workspace(&mut fresh(), UiAction::Back),
        WorkspaceEffect::NothingToClose
    );
    assert_eq!(
        reduce_workspace(&mut fresh(), UiAction::BackOrQuit),
        WorkspaceEffect::RequestQuitConfirmation
    );
    // Esc still closes what is open: an overlay, the composer, a thread.
    let mut open = fresh();
    open.presentation.open_overlay(Overlay::Help);
    assert_eq!(
        reduce_workspace(&mut open, UiAction::Back),
        WorkspaceEffect::None
    );
    assert_eq!(open.presentation.overlay, None);
    let mut writing = fresh();
    writing.presentation.composer_target = Some(ComposerTarget {
        community_id: Uuid::nil(),
        channel_id: Uuid::nil(),
        thread_root_id: None,
        parent_event_id: None,
    });
    assert_eq!(
        reduce_workspace(&mut writing, UiAction::Back),
        WorkspaceEffect::None
    );
    assert_eq!(writing.presentation.composer_target, None);
    let mut thread = WorkspaceState::new(PresentationState::default(), 0, 1, true, true, true);
    assert_eq!(
        reduce_workspace(&mut thread, UiAction::Back),
        WorkspaceEffect::CloseContext
    );
    // And the Inbox: back to the workspace, then nothing more.
    let mut inbox = crate::ui::action::InboxWorkspaceState {
        presentation: PresentationState::default(),
        narrow_layout: false,
        narrow_detail: false,
        detail_available: true,
    };
    inbox.presentation.enter_inbox();
    assert_eq!(
        crate::ui::action::reduce_inbox(&mut inbox, UiAction::Back),
        crate::ui::action::InboxEffect::None
    );
    assert_eq!(
        crate::ui::action::reduce_inbox(&mut inbox, UiAction::Back),
        crate::ui::action::InboxEffect::NothingToClose
    );
}

#[tokio::test]
async fn a_border_dragged_resizes_its_pane_within_the_limits_and_is_saved() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    app.thread_root = Some("event-59".into());
    draw(&mut app, 160, 40);
    let panes = app.shell.last_panes.unwrap();
    let sidebar = panes.sidebar.unwrap();
    let border = sidebar.right() - 1;
    assert_eq!(
        hit(&app, border, 5),
        Some(HitTarget::Splitter(Pane::Channels))
    );
    assert_eq!(
        hit(&app, border + 1, 5),
        Some(HitTarget::Splitter(Pane::Channels))
    );
    let target = hit(&app, border, 5);
    assert!(
        app.drag_border(
            mouse(MouseEventKind::Down(MouseButton::Left), border, 5),
            target.as_ref()
        )
        .unwrap()
    );
    assert!(
        app.drag_border(
            mouse(MouseEventKind::Drag(MouseButton::Left), border + 8, 6),
            None
        )
        .unwrap()
    );
    assert_eq!(app.config.ui.sidebar_width, sidebar.width + 8);
    // Past the conversation's 38 columns it stops there.
    assert!(
        app.drag_border(mouse(MouseEventKind::Drag(MouseButton::Left), 150, 6), None)
            .unwrap()
    );
    let most = sidebar.width + panes.timeline.width - 38;
    assert_eq!(app.config.ui.sidebar_width, most.min(60));
    assert!(
        app.drag_border(mouse(MouseEventKind::Drag(MouseButton::Left), 2, 6), None)
            .unwrap()
    );
    assert_eq!(app.config.ui.sidebar_width, 18);
    assert!(
        app.drag_border(mouse(MouseEventKind::Up(MouseButton::Left), 2, 6), None)
            .unwrap()
    );
    assert_eq!(
        app.status_error.as_deref(),
        Some("channel pane width saved as 18")
    );
    let saved = Config::load(&app.paths).unwrap();
    assert_eq!(saved.ui.sidebar_width, 18);
    draw(&mut app, 160, 40);
    assert_eq!(app.shell.last_panes.unwrap().sidebar.unwrap().width, 18);
    // The thread's border, from its side.
    let thread = app.shell.last_panes.unwrap().thread.unwrap();
    let target = hit(&app, thread.x, 5);
    assert_eq!(target, Some(HitTarget::Splitter(Pane::Thread)));
    app.drag_border(
        mouse(MouseEventKind::Down(MouseButton::Left), thread.x, 5),
        target.as_ref(),
    )
    .unwrap();
    app.drag_border(
        mouse(MouseEventKind::Drag(MouseButton::Left), thread.x - 6, 5),
        None,
    )
    .unwrap();
    app.drag_border(
        mouse(MouseEventKind::Up(MouseButton::Left), thread.x - 6, 5),
        None,
    )
    .unwrap();
    assert_eq!(app.config.ui.thread_width, thread.width + 6);
    // A press anywhere else is not a drag.
    assert!(
        !app.drag_border(
            mouse(MouseEventKind::Down(MouseButton::Left), 80, 10),
            Some(&HitTarget::Timeline)
        )
        .unwrap()
    );
    assert!(
        !app.drag_border(mouse(MouseEventKind::Drag(MouseButton::Left), 81, 10), None)
            .unwrap()
    );
}

#[tokio::test]
async fn the_wheel_scrolls_the_pane_under_it_and_leaves_the_focus() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    draw(&mut app, 120, 30);
    let sidebar = app.shell.last_panes.unwrap().sidebar.unwrap();
    let before = app.channel_viewport.scroll;
    let target = hit(&app, sidebar.x + 3, sidebar.y + 3).unwrap();
    app.wheel_without_focus(target, 3);
    assert!(app.channel_viewport.scroll > before);
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    // While writing too, over the conversation.
    app.presentation.set_workspace_focus(FocusSurface::Channels);
    app.presentation.composer_target = Some(ComposerTarget {
        community_id: app.config.communities[0].id,
        channel_id: app.channels[0].id,
        thread_root_id: None,
        parent_event_id: None,
    });
    draw(&mut app, 120, 30);
    let timeline = app.shell.last_panes.unwrap().timeline;
    let before = app.timeline.scroll;
    let target = hit(&app, timeline.x + 10, timeline.y + 5).unwrap();
    assert_eq!(target, HitTarget::Timeline);
    app.wheel_without_focus(target, -3);
    assert!(
        app.timeline.scroll < before,
        "{} {before}",
        app.timeline.scroll
    );
    assert_eq!(app.presentation.focus, FocusSurface::Channels);
    assert!(app.presentation.composer_target.is_some());
    // Over the composer the wheel does nothing.
    let scroll = app.timeline.scroll;
    app.wheel_without_focus(HitTarget::Composer, 3);
    assert_eq!(app.timeline.scroll, scroll);
}

#[tokio::test]
async fn a_double_click_opens_the_thread_of_the_message_clicked() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    draw(&mut app, 120, 30);
    let timeline = app.shell.last_panes.unwrap().timeline;
    // The newest message is at the bottom of the conversation.
    let target = (timeline.y..timeline.bottom())
        .rev()
        .find_map(|row| match hit(&app, timeline.x + 8, row) {
            Some(HitTarget::TimelineMessage(id)) => Some(id),
            _ => None,
        })
        .unwrap();
    assert!(!app.record_primary_click(&HitTarget::TimelineMessage(target.clone())));
    assert!(app.record_primary_click(&HitTarget::TimelineMessage(target.clone())));
    // What the click did: the focus and the selection on the message. The
    // thread opens (`toggle_thread`, which also reloads from the store: the
    // relay scenario `t17-layout-mouse` sees it drawn).
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    app.timeline.selected_event = Some(target.clone());
    assert!(!app.thread_of_selected_is_open());
    // Open on it: a second double click keeps it open and focuses it.
    app.thread_root = Some(target.clone());
    assert!(app.thread_of_selected_is_open());
    app.open_thread_of_selected().await.unwrap();
    assert_eq!(app.thread_root.as_deref(), Some(target.as_str()));
    assert_eq!(app.presentation.focus, FocusSurface::Context);
    // Another message's thread open: the clicked one is not, so it opens
    // in its place rather than closing the other (a toggle would).
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    app.timeline.selected_event = Some("event-3".into());
    assert!(!app.thread_of_selected_is_open());
}

#[tokio::test]
async fn a_click_on_the_status_lines_hint_opens_the_palette() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    let rows = draw(&mut app, 120, 30);
    let column = rows[29]
        .find("Ctrl-K")
        .map(|byte| rows[29][..byte].chars().count());
    let column = u16::try_from(column.expect("the hint")).unwrap();
    assert_eq!(hit(&app, column + 2, 29), Some(HitTarget::StatusHint));
    app.click_status_hint();
    assert_eq!(app.presentation.overlay, Some(Overlay::Palette));
}

#[tokio::test]
async fn a_dm_composer_names_the_person_and_the_backing_channel_is_not_listed() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    let rows = draw(&mut app, 120, 30).join("\n");
    assert!(rows.contains("message #general"), "{rows}");
    assert!(!rows.contains("huddle-0a1b2c3d"), "{rows}");
    let backing = app
        .channels
        .iter()
        .find(|channel| channel.name.starts_with("huddle-"))
        .unwrap();
    assert!(backing.is_huddle_backing());
    assert!(!backing.listed_for_viewer());
    assert!(!app.channels[0].is_huddle_backing());
    app.selected_channel = app
        .channels
        .iter()
        .position(|channel| channel.name == "Scout")
        .unwrap();
    app.messages.clear();
    let rows = draw(&mut app, 120, 30).join("\n");
    assert!(rows.contains("message @Scout"), "{rows}");
    assert!(!rows.contains("message #Scout"), "{rows}");
    // `:system` shows it.
    app.show_system_channels = true;
    app.selected_channel = 0;
    let rows = draw(&mut app, 120, 60).join("\n");
    assert!(rows.contains("huddle-0a1b2c3d"), "{rows}");
}

/// An app that says how much room it has.
#[cfg(feature = "apps")]
const SIZE_APP: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return {} end
function app.view(s, ctx)
  return ui.text { "board for the release", "room " .. ctx.width .. "x" .. ctx.height }
end
return app
"#;

/// The main screens at 60, 80, 120 and 160 columns with the sidebar, a
/// thread, an app and both, in `tests/fixtures/layout/`. `BZZ_BLESS=1`
/// writes them.
#[cfg(feature = "apps")]
#[tokio::test]
async fn goldens_of_the_main_screens_at_four_widths() {
    let temporary = TempDir::new().unwrap();
    let mut app = fixture(&temporary).await;
    let general = app.channels[0].id;
    app.thread_messages = (40..43).map(|index| message(index, general)).collect();
    app.thread_timeline.reconcile(&app.thread_messages);
    app.thread_timeline.at_live_bottom = true;
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layout");
    for (state, thread, app_open) in [
        ("sidebar", false, false),
        ("thread", true, false),
        ("app", false, true),
        ("both", true, true),
    ] {
        app.thread_root = thread.then(|| "event-40".to_owned());
        if app_open {
            app.open_app_in_memory(general, "board", SIZE_APP);
            app.apps.front = true;
        } else {
            app.apps.open.clear();
        }
        // The focus is on the conversation, so nothing takes its place.
        app.presentation.set_workspace_focus(FocusSurface::Timeline);
        for (width, height) in [(60, 24), (80, 24), (120, 32), (160, 45)] {
            // The app's session answers on its own thread: wait for its
            // view (a loaded machine takes seconds), never compare before.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let rows = loop {
                app.poll_apps();
                let rows = draw(&mut app, width, height);
                // The view of this size, not the last size's.
                let room = app.shell.last_panes.unwrap().app.map(|pane| {
                    let inside = crate::ui::layout::inside(pane);
                    format!("room {}x{}", inside.width, inside.height)
                });
                if !app_open || room.is_none_or(|room| rows.join("\n").contains(&room)) {
                    break rows;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{state} at {width}x{height}: the app drew nothing in 30 s"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            };
            let text = rows
                .iter()
                .map(|row| row.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            let path = folder.join(format!("{state}-{width}x{height}.txt"));
            if std::env::var_os("BZZ_BLESS").is_some() {
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(&path, &text).unwrap();
            }
            let golden = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("{}: run with BZZ_BLESS=1 to write it", path.display()));
            assert_eq!(
                text, golden,
                "{state} at {width}x{height}: run with BZZ_BLESS=1"
            );
        }
    }
}
