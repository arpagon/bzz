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
