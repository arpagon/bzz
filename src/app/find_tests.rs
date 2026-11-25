//! v0.17 C3 to C7 at the level of the App: the help scrolls to its end, the
//! `:` line answers a mistake under its prompt, the finder's selection, the
//! leader panel's delay, and `d`, `u` and `:status`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{Channel, ChannelFacts, ChannelKind, Visibility},
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::state::Overlay,
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

fn channel(name: &str, kind: ChannelKind) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        name: name.into(),
        about: String::new(),
        kind,
        visibility: Visibility::Public,
        is_member: true,
        is_hidden: false,
        member_count: 1,
        last_event_at: None,
        facts: ChannelFacts::default(),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn rows(app: &mut super::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// C5: ↓, Tab, Ctrl-n move the finder's selection, ↑, Shift-Tab, Ctrl-p
/// move it back, a new letter starts at the top, and the rows end with the
/// way to a new DM.
#[tokio::test]
async fn the_finder_selection_moves() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    app.channels = vec![
        channel("general", ChannelKind::Stream),
        channel("t9-general", ChannelKind::Stream),
        channel("Scout", ChannelKind::Dm),
    ];
    app.presentation.open_overlay(Overlay::Finder);
    for character in "gen".chars() {
        app.text_overlay_key(key(KeyCode::Char(character)), true)
            .await
            .unwrap();
    }
    assert_eq!(app.finder.selected, 0);
    app.text_overlay_key(key(KeyCode::Down), true)
        .await
        .unwrap();
    assert_eq!(app.finder.selected, 1);
    let screen = rows(&mut app, 120, 40).join("\n");
    assert!(screen.contains("› #t9-general"), "{screen}");
    app.text_overlay_key(key(KeyCode::Tab), true).await.unwrap();
    app.text_overlay_key(
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        true,
    )
    .await
    .unwrap();
    // general, t9-general, the new-DM row: wrapped to the top.
    assert_eq!(app.finder.selected, 0);
    app.text_overlay_key(key(KeyCode::BackTab), true)
        .await
        .unwrap();
    assert_eq!(app.finder.selected, 2);
    app.text_overlay_key(
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        true,
    )
    .await
    .unwrap();
    app.text_overlay_key(key(KeyCode::Up), true).await.unwrap();
    assert_eq!(app.finder.selected, 0);
    app.text_overlay_key(key(KeyCode::Down), true)
        .await
        .unwrap();
    app.text_overlay_key(key(KeyCode::Char('e')), true)
        .await
        .unwrap();
    assert_eq!(app.finder.selected, 0);
    // A DM is found by the person's name and marked as a DM.
    app.finder.clear();
    for character in "sco".chars() {
        app.finder.push(character);
    }
    let screen = rows(&mut app, 120, 40).join("\n");
    assert!(screen.contains("› @Scout"), "{screen}");
    assert!(screen.contains("start a DM"), "{screen}");
}
