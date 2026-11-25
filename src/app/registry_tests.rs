//! v0.17 C8, C10 and C11 at the level of the App: the `:` line completes
//! with Tab, the footer follows the focus, and the help, the leader panel
//! and the actions menu read the registry and the live key map.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;

use crate::{
    config::Config, paths::Paths, store::Store, store::writer::StoreHandle, ui::state::Overlay,
};

async fn app(temporary: &TempDir) -> super::App {
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
    super::App::new(config, paths, handle).await.unwrap()
}

/// An App with a community, so the workspace and its status line are drawn;
/// nothing connects.
async fn app_with_community(temporary: &TempDir) -> super::App {
    let mut app = app(temporary).await;
    let identity = crate::config::IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "registry".into(),
        pubkey: "a".repeat(64),
        backend: crate::config::KeyBackend::Keychain,
        key_ref: "identity:registry".into(),
    };
    app.config.identities.push(identity.clone());
    app.config
        .add_community(
            "registry".into(),
            "wss://registry.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn screen(app: &mut super::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn type_on_the_line(app: &mut super::App, text: &str) {
    for character in text.chars() {
        app.text_overlay_key(key(KeyCode::Char(character)), false)
            .await
            .unwrap();
    }
}

/// C11: `:hu`, Tab, Tab: the line reads `:huddle start`, the verbs are
/// listed under it with the one written in brackets, and the usage.
#[tokio::test]
async fn tab_completes_the_command_line_and_shows_what_it_offers() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    app.presentation.open_overlay(Overlay::Command);
    type_on_the_line(&mut app, "hu").await;
    let typing = screen(&mut app, 80, 24);
    assert!(typing.contains(":hu"), "{typing}");
    assert!(typing.contains("Tab: huddle"), "{typing}");
    app.text_overlay_key(key(KeyCode::Tab), false)
        .await
        .unwrap();
    assert_eq!(app.command, "huddle ");
    app.text_overlay_key(key(KeyCode::Tab), false)
        .await
        .unwrap();
    assert_eq!(app.command, "huddle start");
    let shown = screen(&mut app, 80, 24);
    assert!(shown.contains(":huddle start"), "{shown}");
    assert!(shown.contains("Tab: [start]  join"), "{shown}");
    assert!(shown.contains("usage: :huddle [start"), "{shown}");
    app.text_overlay_key(key(KeyCode::Tab), false)
        .await
        .unwrap();
    assert_eq!(app.command, "huddle join");
    app.text_overlay_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), false)
        .await
        .unwrap();
    assert_eq!(app.command, "huddle start");
    // Esc closes the line and forgets the cycle.
    app.text_overlay_key(key(KeyCode::Esc), false)
        .await
        .unwrap();
    assert_eq!(app.presentation.overlay, None);
    assert_eq!(
        app.command_completion,
        crate::ui::command::Completion::default()
    );
}

/// C8: a key a person binds in `keymap.toml` shows in the help, the leader
/// panel, the palette and the footer, which all read the registry and the
/// live key map.
#[test]
fn a_users_binding_shows_in_the_help_the_leader_panel_the_palette_and_the_footer() {
    use crate::ui::{
        action_table::FooterContext,
        keymap::{KeyChord, KeyMap, KeyScope, UiAction},
    };
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("keymap.toml");
    std::fs::write(
        &path,
        "[[binding]]\nkeys = ['r']\ndisabled = true\n\
         [[binding]]\nkeys = ['x']\naction = 'react'\n\
         [[binding]]\nkeys = ['space', 'x']\naction = 'react'\n",
    )
    .unwrap();
    let keymap = KeyMap::load_from(&path).unwrap();
    let scope = KeyScope::Workspace;
    // The help: the row "React to message" with both keys, `r` disabled.
    let help = crate::ui::help::help_lines(&keymap, scope, &[], 120).join("\n");
    assert!(
        help.lines()
            .any(|line| line.contains("x, Space x") && line.ends_with("React to message")),
        "{help}"
    );
    assert!(
        !help
            .lines()
            .any(|line| line.trim_start().starts_with("r ") && line.contains("React")),
        "{help}"
    );
    // The leader panel: Space x is there with the row's title.
    let space = KeyChord::new(KeyCode::Char(' '), KeyModifiers::NONE);
    let (_, lines) = crate::ui::help::leader_panel(
        &keymap,
        scope,
        &[space],
        ratatui::layout::Rect::new(0, 0, 80, 23),
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("x       React to message")),
        "{lines:?}"
    );
    // The palette's key column.
    assert_eq!(
        crate::ui::palette::key_label(&keymap, scope, UiAction::React),
        "x"
    );
    // The footer.
    let keys = crate::ui::status::footer_keys(&keymap, FooterContext::Message, |_| true);
    assert!(keys.contains(&("x".to_owned(), "react")), "{keys:?}");
    assert!(!keys.iter().any(|(key, _)| key == "r"), "{keys:?}");
}

/// C10: the status line's hint follows the focus: the channels, the empty
/// timeline, the Inbox, an overlay.
#[tokio::test]
async fn the_status_line_hint_follows_the_focus() {
    use crate::ui::state::{FocusSurface, Route};
    let temporary = TempDir::new().unwrap();
    let mut app = app_with_community(&temporary).await;
    let last = |app: &mut super::App, width| {
        screen(app, width, 24)
            .lines()
            .last()
            .unwrap_or_default()
            .to_owned()
    };
    app.presentation.set_workspace_focus(FocusSurface::Channels);
    let channels = last(&mut app, 160);
    assert!(channels.contains("Space Space find"), "{channels}");
    assert!(
        channels.trim_end().ends_with("Ctrl-K actions"),
        "{channels}"
    );
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    let timeline = last(&mut app, 160);
    assert_ne!(channels, timeline);
    assert!(timeline.contains("Ctrl-K actions"), "{timeline}");
    app.presentation.route = Route::Inbox;
    let inbox = last(&mut app, 160);
    assert!(inbox.contains("Ctrl-K actions"), "{inbox}");
    app.presentation.route = Route::Workspace;
    app.presentation.open_overlay(Overlay::Command);
    let overlay = last(&mut app, 160);
    assert!(overlay.contains("Esc close"), "{overlay}");
    assert!(!overlay.contains("Ctrl-K"), "{overlay}");
}

/// C11: `:theme n`, Tab completes a theme id bzz has; a wrong word
/// completes nothing and says what is near.
#[tokio::test]
async fn tab_completes_theme_names_and_a_wrong_word_says_so() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    app.presentation.open_overlay(Overlay::Command);
    type_on_the_line(&mut app, "theme n").await;
    app.text_overlay_key(key(KeyCode::Tab), false)
        .await
        .unwrap();
    assert!(app.command.starts_with("theme n"), "{}", app.command);
    let theme = app.command.trim().trim_start_matches("theme ").to_owned();
    assert!(
        crate::ui::theme::ThemeRegistry::entries().any(|entry| entry.id == theme),
        "{theme}"
    );
    app.command.clear();
    type_on_the_line(&mut app, "hudle").await;
    app.text_overlay_key(key(KeyCode::Tab), false)
        .await
        .unwrap();
    assert_eq!(app.command, "hudle");
    let shown = screen(&mut app, 80, 24);
    assert!(
        shown.contains("no command starts with hudle · did you mean :huddle?"),
        "{shown}"
    );
}
