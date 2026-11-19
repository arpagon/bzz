//! The app pane inside bzz's own `App`, drawn to Ratatui's test backend:
//! an app on records in memory, keys, a paste, the layout and the mouse,
//! with no terminal and no relay.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{Channel, ChannelFacts, ChannelKind, Visibility},
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::{
        hit_map::HitTarget,
        layout::AppPlacement,
        state::{ComposerTarget, FocusSurface, Overlay},
    },
};

use super::super::App;

/// An app that writes down every key and paste it is given.
const RECORDER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { keys = {}, pastes = {} } end
function app.view(s, ctx)
  local pasted = table.concat(s.pastes, "|"):gsub("\n", "/")
  return ui.text { "keys " .. table.concat(s.keys, " "), "pasted " .. pasted, "size " .. ctx.width .. "x" .. ctx.height }
end
function app.key(s, key, ctx) s.keys[#s.keys + 1] = key.name end
function app.paste(s, text, ctx) s.pastes[#s.pastes + 1] = text end
return app
"#;

/// An app whose every key runs until it is stopped.
const LOOPER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { n = 0 } end
function app.view(s, ctx) return ui.text { "loops " .. s.n } end
function app.key(s, key, ctx) s.n = s.n + 1 while true do end end
return app
"#;

async fn fixture(temporary: &TempDir) -> (App, Uuid) {
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
        label: "pane".into(),
        pubkey: "a".repeat(64),
        backend: crate::config::KeyBackend::Keychain,
        key_ref: "identity:pane".into(),
    };
    app.config.identities.push(identity.clone());
    app.config
        .add_community(
            "pane".into(),
            "wss://pane.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let channel = Uuid::new_v4();
    app.channels = vec![
        Channel {
            id: channel,
            name: "general".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: ChannelFacts::default(),
        },
        Channel {
            id: Uuid::new_v4(),
            name: "random".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: ChannelFacts::default(),
        },
    ];
    app.selected_channel = 0;
    let synced = app.config.clone();
    app.store
        .call(move |store| store.sync_config(&synced))
        .await
        .unwrap();
    (app, channel)
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Polls and draws until the screen shows `wanted`.
async fn until(app: &mut App, width: u16, height: u16, wanted: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_apps();
        let shown = screen(app, width, height);
        if shown.contains(wanted) {
            return shown;
        }
        assert!(Instant::now() < deadline, "no {wanted:?} on:\n{shown}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

#[tokio::test]
async fn keys_and_a_paste_reach_the_focused_app_and_nothing_else() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.focus_app();
    let shown = until(&mut app, 120, 30, "keys").await;
    assert!(shown.contains("recorder · Ctrl-c leave"), "{shown}");
    assert!(shown.contains(" APP "), "{shown}");
    assert!(shown.contains("Ctrl-c leaves the app"), "{shown}");
    for code in [
        KeyCode::Char('q'),
        KeyCode::Char(':'),
        KeyCode::Char('1'),
        KeyCode::Char('5'),
        KeyCode::Char('i'),
        KeyCode::Char('?'),
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::Enter,
    ] {
        assert!(app.app_key(key(code)), "{code:?} is the app's");
    }
    let shown = until(&mut app, 120, 30, "keys q : 1 5 i ? esc tab enter").await;
    // None of them opened anything of bzz's, or asked to quit.
    assert_eq!(app.presentation.overlay, None);
    assert_eq!(app.presentation.composer_target, None);
    assert!(!app.should_quit);
    assert_eq!(app.presentation.focus, FocusSurface::App);
    assert!(!shown.contains("quit bzz?"));
    // A paste of two lines is one piece of text, with LF.
    app.handle_terminal_paste("line one\rline two".into())
        .await
        .unwrap();
    until(&mut app, 120, 30, "pasted line one/line two").await;
    // Ctrl+C leaves, and is not the app's.
    assert!(app.app_key(ctrl_c()));
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    assert!(!app.app_has_focus());
    let shown = screen(&mut app, 120, 30);
    assert!(shown.contains("recorder · 5 focus"), "{shown}");
    assert!(!shown.contains("Ctrl-c leaves the app"), "{shown}");
    assert!(shown.contains("? help · q quit"), "{shown}");
    // Now keys are bzz's again, and the app is given none.
    assert!(!app.app_key(key(KeyCode::Char('x'))));
    // The writing dock owns the keyboard before the app, even focused.
    app.focus_app();
    app.presentation.composer_target = Some(ComposerTarget {
        community_id: app.active_community_id().unwrap(),
        channel_id: channel,
        thread_root_id: None,
        parent_event_id: None,
    });
    assert!(!app.app_key(key(KeyCode::Char('h'))));
    assert!(!app.app_key(ctrl_c()));
    app.handle_terminal_paste("to the\rcomposer".into())
        .await
        .unwrap();
    assert_eq!(app.composer.body, "to the\ncomposer");
    // An overlay owns it too; a paste there goes nowhere.
    app.presentation.composer_target = None;
    app.presentation.open_overlay(Overlay::Help);
    assert!(!app.app_key(key(KeyCode::Char('j'))));
    app.handle_terminal_paste("dropped".into()).await.unwrap();
    app.presentation.close_overlay();
    let shown = until(&mut app, 120, 30, "pasted line one/line two").await;
    assert!(!shown.contains("dropped"), "{shown}");
    assert!(
        shown.contains("keys q : 1 5 i ? esc tab enter\n")
            || shown.contains("keys q : 1 5 i ? esc tab enter "),
        "{shown}"
    );
    assert!(
        !shown.contains("enter h") && !shown.contains("enter x") && !shown.contains("enter j"),
        "{shown}"
    );
}

#[tokio::test]
async fn the_pane_follows_the_window_and_its_channel() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.focus_app();
    // 80 by 24: beside the conversation, 40 by 18 inside.
    let shown = until(&mut app, 80, 24, "size 40x18").await;
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(!shown.contains("communities"), "{shown}");
    // 60 columns: the app takes the window while it has the focus.
    let shown = until(&mut app, 60, 24, "size 58x21").await;
    assert_eq!(app.apps.placement, AppPlacement::Zoomed);
    assert!(!shown.contains("general"), "{shown}");
    // Without the focus it is hidden, and the status line says so.
    app.leave_app();
    let shown = screen(&mut app, 60, 24);
    assert_eq!(app.apps.placement, AppPlacement::HiddenNarrow);
    assert!(shown.contains("app hidden · 5"), "{shown}");
    // Zoom on request, at any size.
    app.zoom_app();
    until(&mut app, 160, 40, "size 158x37").await;
    assert_eq!(app.apps.placement, AppPlacement::Zoomed);
    app.app_key(ctrl_c());
    let shown = screen(&mut app, 160, 40);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(shown.contains("communities"), "{shown}");
    // Under 40 by 16 inside bzz says so, and the script draws nothing.
    app.focus_app();
    let shown = screen(&mut app, 120, 18);
    assert!(shown.contains("This app needs 40×16"), "{shown}");
    assert!(!shown.contains("size 40x"), "{shown}");
    // Another channel selected: its app, or none, and the focus comes back.
    app.selected_channel = 1;
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::Closed);
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    assert!(!shown.contains("recorder"), "{shown}");
    // And the session kept its state for when the owner comes back.
    app.selected_channel = 0;
    until(&mut app, 120, 30, "recorder · 5 focus").await;
}

#[tokio::test]
async fn the_thread_and_the_app_share_the_right_side() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.thread_root = Some("e".repeat(64));
    app.apps.front = false;
    // Asked for last, the thread has the right side under 144 columns, and
    // a reply goes into it.
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::HiddenBehindThread);
    assert!(shown.contains("app behind thread · 5"), "{shown}");
    assert!(app.thread_drawn);
    assert_eq!(app.reply_root(), app.thread_root);
    assert!(shown.contains("reply in #general"), "{shown}");
    // `5` brings the app to the front; the thread waits, and the composer
    // no longer replies into it.
    app.focus_app();
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(!app.thread_drawn);
    assert_eq!(app.reply_root(), None);
    assert!(shown.contains("message #general"), "{shown}");
    // From 144 both show.
    screen(&mut app, 144, 30);
    assert!(app.thread_drawn);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
}

#[tokio::test]
async fn the_mouse_focuses_the_pane_and_the_wheel_keeps_the_apps_focus() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    screen(&mut app, 120, 30);
    let map = app.last_hit_map.clone().unwrap();
    // The pane is 42 columns at the far right, above the dock.
    assert_eq!(map.hit(100, 10), Some(&HitTarget::App));
    assert_eq!(map.hit(78, 10), Some(&HitTarget::App));
    assert_eq!(map.hit(77, 10), Some(&HitTarget::Timeline));
    // A focused app keeps the keyboard when the wheel scrolls another pane.
    app.focus_app();
    app.wheel_over(HitTarget::Timeline, 3);
    assert_eq!(app.presentation.focus, FocusSurface::App);
    // The wheel over the app does nothing, and moves no focus.
    app.leave_app();
    app.wheel_over(HitTarget::App, 3);
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    // Without an app focused the wheel moves the focus, as in v0.15.
    app.wheel_over(HitTarget::ChannelPane, 3);
    assert_eq!(app.presentation.focus, FocusSurface::Channels);
}

/// A script stuck in a call costs the conversation nothing: keys are taken
/// and frames drawn at once, and the pane says the app does not respond.
#[tokio::test]
async fn a_looping_script_costs_the_window_nothing() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "looper", LOOPER);
    app.focus_app();
    until(&mut app, 120, 30, "loops 0").await;
    let started = Instant::now();
    for _ in 0..20 {
        app.app_key(key(KeyCode::Char('x')));
        app.poll_apps();
        screen(&mut app, 120, 30);
    }
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?} for twenty keys and frames",
        started.elapsed()
    );
    // The composer types while the script loops.
    app.leave_app();
    app.presentation.composer_target = Some(ComposerTarget {
        community_id: app.active_community_id().unwrap(),
        channel_id: channel,
        thread_root_id: None,
        parent_event_id: None,
    });
    let typing = Instant::now();
    for character in "still typing".chars() {
        app.insert_action(crate::ui::keymap::KeyAction::Character(character))
            .await
            .unwrap();
        screen(&mut app, 120, 30);
    }
    assert_eq!(app.composer.body, "still typing");
    assert!(
        typing.elapsed() < Duration::from_millis(1500),
        "{:?}",
        typing.elapsed()
    );
}
