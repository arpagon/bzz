//! v0.17 C3 to C7 at the level of the App: the help scrolls to its end, the
//! `:` line answers a mistake under its prompt, the finder's selection, the
//! leader panel's delay, and `d`, `u` and `:status`.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{Channel, ChannelFacts, ChannelKind, Message, Visibility},
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::{
        keymap::UiAction,
        state::{ConfirmationKind, FocusSurface, Overlay},
    },
};

const OWN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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

/// An App with a community whose identity is `OWN`; nothing connects.
async fn app_with_identity(temporary: &TempDir) -> super::App {
    let mut app = app(temporary).await;
    let identity = crate::config::IdentityConfig {
        id: Uuid::new_v4(),
        label: "find".into(),
        pubkey: OWN.into(),
        backend: crate::config::KeyBackend::Keychain,
        key_ref: "identity:find".into(),
    };
    app.config.identities.push(identity.clone());
    app.config
        .add_community(
            "find".into(),
            "wss://find.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    app
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

fn message(channel: Uuid, id: &str, author: &str) -> Message {
    serde_json::from_value(serde_json::json!({
        "event_id": id,
        "channel_id": channel,
        "pubkey": author,
        "created_at": 1_800_000_000_u64,
        "content": "generated message",
        "root_event_id": null,
        "parent_event_id": null,
        "deleted": false,
    }))
    .unwrap()
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

async fn command(app: &mut super::App, text: &str) {
    app.command = text.into();
    app.presentation.open_overlay(Overlay::Command);
    app.text_overlay_key(key(KeyCode::Enter), false)
        .await
        .unwrap();
}

/// C3: at 80x24 the help shows where it is, and `j`, PageDown and `G` reach
/// its last row (the last `:` command), which v0.16.0 never showed.
#[tokio::test]
async fn the_help_scrolls_to_its_last_row_at_80x24() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    command(&mut app, "help").await;
    assert_eq!(app.presentation.overlay, Some(Overlay::Help));
    let first = rows(&mut app, 80, 24).join("\n");
    assert!(first.contains("bzz effective keymap"), "{first}");
    assert!(first.contains("rows 1-"), "{first}");
    assert!(!first.contains("local cache"), "{first}");
    let total = app.help_scroll.total;
    // One row at a time: j reaches the end and stops there.
    for _ in 0..total {
        app.help_key(key(KeyCode::Char('j')));
        rows(&mut app, 80, 24);
    }
    let end = rows(&mut app, 80, 24).join("\n");
    assert!(end.contains(":purge-cache"), "{end}");
    assert!(end.contains("local cache"), "{end}");
    assert!(end.contains(&format!("of {total}")), "{end}");
    app.help_key(key(KeyCode::Char('g')));
    assert_eq!(app.help_scroll.offset, 0);
    // A page at a time.
    let mut pages = 0;
    while app.help_scroll.offset < app.help_scroll.max_offset() {
        app.help_key(key(KeyCode::PageDown));
        pages += 1;
        assert!(pages < 50);
    }
    assert!(rows(&mut app, 80, 24).join("\n").contains("local cache"));
    app.help_key(key(KeyCode::Char('g')));
    app.help_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(app.help_scroll.offset, app.help_scroll.max_offset());
    assert!(total > 22, "the help is longer than one 80x24 screen");
}

/// C4: a mistyped command keeps its prompt, and the nearest command is
/// named under it.
#[tokio::test]
async fn a_mistyped_command_names_the_nearest_under_its_prompt() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    command(&mut app, "hudle").await;
    assert_eq!(app.presentation.overlay, Some(Overlay::Command));
    assert_eq!(app.command, "hudle");
    let screen = rows(&mut app, 80, 24).join("\n");
    assert!(screen.contains(":hudle"), "{screen}");
    assert!(screen.contains("did you mean :huddle?"), "{screen}");
    assert!(!screen.contains("unknown command:"), "{screen}");
    // Typing clears the answer; Esc leaves.
    app.text_overlay_key(key(KeyCode::Backspace), false)
        .await
        .unwrap();
    assert!(app.command_answer.is_none());
    app.text_overlay_key(key(KeyCode::Esc), false)
        .await
        .unwrap();
    assert_eq!(app.presentation.overlay, None);
}

/// C4: a usage line longer than the window wraps under the prompt, whole.
#[tokio::test]
async fn a_long_usage_wraps_and_is_not_cut() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    command(&mut app, "agents nowhere").await;
    let answer = app.command_answer.clone().unwrap();
    assert!(answer.starts_with("usage: :agents"), "{answer}");
    let screen = rows(&mut app, 80, 24);
    assert!(
        squeeze(&screen).contains(&answer),
        "{answer}\n{}",
        screen.join("\n")
    );
    // :community add names the shell command, also whole.
    app.presentation.close_overlay();
    app.command_answer = None;
    command(&mut app, "community add").await;
    assert_eq!(
        app.command_answer.as_deref(),
        Some(crate::ui::command::COMMUNITY_ANSWER)
    );
    let screen = rows(&mut app, 80, 24);
    assert!(
        squeeze(&screen).contains(crate::ui::command::COMMUNITY_ANSWER),
        "{}",
        screen.join("\n")
    );
}

/// The words inside the `:` prompt's box, its frame and line breaks taken
/// out. Only the box is read: whatever screen lies behind it (the first
/// screen, a conversation) shows to its left and right.
fn squeeze(screen: &[String]) -> String {
    let rows = screen
        .iter()
        .map(|row| row.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let title = "┌ command".chars().collect::<Vec<_>>();
    let Some((top, left)) = rows.iter().enumerate().find_map(|(y, row)| {
        row.windows(title.len())
            .position(|window| window == title.as_slice())
            .map(|x| (y, x))
    }) else {
        return String::new();
    };
    let right = rows[top]
        .iter()
        .rposition(|character| *character == '┐')
        .unwrap_or(rows[top].len() - 1);
    rows[top + 1..]
        .iter()
        .take_while(|row| row.get(left) == Some(&'│'))
        .map(|row| row[left + 1..right].iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// C4: `:q` asks as `q` does; `:theme` opens the picker; `:theme <name>`
/// sets it; a theme that does not exist is named.
#[tokio::test]
async fn quit_and_theme_commands() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    command(&mut app, "q").await;
    assert_eq!(app.presentation.overlay, Some(Overlay::Confirmation));
    assert_eq!(app.presentation.confirmation, Some(ConfirmationKind::Quit));
    app.presentation.close_overlay();
    command(&mut app, "quit").await;
    assert_eq!(app.presentation.confirmation, Some(ConfirmationKind::Quit));
    app.presentation.close_overlay();
    command(&mut app, "theme").await;
    assert_eq!(app.presentation.overlay, Some(Overlay::Theme));
    app.theme_picker_key(key(KeyCode::Esc));
    command(&mut app, "theme nord").await;
    assert_eq!(app.presentation.overlay, None);
    assert_eq!(app.config.ui.theme, "nord");
    assert_eq!(app.theme.id(), "nord");
    command(&mut app, "theme plaid").await;
    assert_eq!(app.presentation.overlay, Some(Overlay::Command));
    assert!(
        app.command_answer
            .as_deref()
            .is_some_and(|answer| answer.starts_with("no theme named plaid"))
    );
    assert_eq!(app.theme.id(), "nord");
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

/// C6: the leader panel appears 300 ms after the key, on bzz's clock, asks
/// for one frame then, and waits for the next key without timing out.
#[tokio::test]
async fn the_leader_panel_waits_300_ms_and_asks_for_one_frame() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    let _ = app.input_router.dispatch(
        &app.keymap,
        crate::ui::input::InputContext::workspace(),
        key(KeyCode::Char(' ')),
    );
    let now = tokio::time::Instant::now();
    app.leader_started_at = Some(now - Duration::from_millis(150));
    assert!(!app.on_tick().await.unwrap());
    assert_eq!(app.presentation.overlay, None);
    app.leader_started_at = Some(now - Duration::from_millis(300));
    let mut frames = 0;
    for _ in 0..5 {
        frames += usize::from(app.on_tick().await.unwrap());
    }
    assert_eq!(frames, 1);
    assert_eq!(app.presentation.overlay, Some(Overlay::WhichKey));
    // Long after v0.16.0's 750 ms the panel is still there, for reading.
    app.leader_started_at = Some(now - Duration::from_secs(5));
    assert!(!app.on_tick().await.unwrap());
    assert!(app.input_router.sequence_active());
    assert_eq!(app.presentation.overlay, Some(Overlay::WhichKey));
    for (width, height) in [(80, 24), (120, 32)] {
        let screen = rows(&mut app, width, height).join("\n");
        for label in [
            // v0.17 C8: the registry's titles.
            "Go to channel or DM",
            "Actions for the selected item",
            "Cycle channel sort",
            "Open Inbox",
            "Change theme…",
            "View author profile",
            "Refresh",
            "Zoom the app",
            "Show or hide communities",
            "Show or hide channels",
            "Show or hide thread pane",
            "Find any action",
            "Esc cancels",
        ] {
            assert!(
                screen.contains(label),
                "{width}x{height}: {label}\n{screen}"
            );
        }
    }
}

/// A sequence whose panel cannot show (something else holds the screen)
/// still ends after 750 ms, as in v0.16.0.
#[tokio::test]
async fn a_sequence_behind_another_overlay_still_expires() {
    let temporary = TempDir::new().unwrap();
    let mut app = app(&temporary).await;
    let _ = app.input_router.dispatch(
        &app.keymap,
        crate::ui::input::InputContext::workspace(),
        key(KeyCode::Char(' ')),
    );
    app.presentation.open_overlay(Overlay::Broker);
    app.leader_started_at = Some(tokio::time::Instant::now() - super::LEADER_TIMEOUT);
    app.on_tick().await.unwrap();
    assert!(!app.input_router.sequence_active());
    assert_eq!(app.presentation.overlay, Some(Overlay::Broker));
}

/// C7: `d` and `u` are the workspace's, and `d` on another person's message
/// says why not in the words of the actions menu.
#[tokio::test]
async fn d_on_another_persons_message_says_why_not() {
    let temporary = TempDir::new().unwrap();
    let mut app = app_with_identity(&temporary).await;
    let keymap = &app.keymap;
    for (character, action) in [('d', UiAction::Delete), ('u', UiAction::MarkUnread)] {
        let chord = crate::ui::keymap::KeyChord::new(KeyCode::Char(character), KeyModifiers::NONE);
        assert_eq!(
            keymap.lookup(crate::ui::keymap::KeyScope::Workspace, &[chord]),
            crate::ui::keymap::KeyLookup::Action(action)
        );
        // The Inbox keeps its own: `u` is not `U` there.
        assert_eq!(
            keymap.lookup(crate::ui::keymap::KeyScope::Inbox, &[chord]),
            crate::ui::keymap::KeyLookup::NoMatch
        );
    }
    let general = channel("general", ChannelKind::Stream);
    let id = general.id;
    app.channels = vec![general];
    app.messages = vec![message(id, "e1", OTHER), message(id, "e2", OWN)];
    app.presentation.set_workspace_focus(FocusSurface::Timeline);
    app.timeline.selected_event = Some("e1".into());
    assert_eq!(
        app.context_refusal(UiAction::Delete).as_deref(),
        Some("Delete my message: only your own message can be deleted")
    );
    // Without a selected message.
    app.timeline.selected_event = None;
    assert_eq!(
        app.context_refusal(UiAction::Delete).as_deref(),
        Some("Delete my message: select a message first")
    );
    // Not in the conversation.
    app.presentation.set_workspace_focus(FocusSurface::Channels);
    assert!(
        app.context_refusal(UiAction::MarkUnread)
            .is_some_and(|refusal| refusal.contains("select a message in the conversation"))
    );
}

/// C7, D6: a bare `:status` shows the status (v0.16.0 cleared it).
#[test]
fn bare_status_shows() {
    let lunch = crate::protocol::presence::UserStatus {
        text: "lunch".into(),
        emoji: "🍕".into(),
        expires_at: Some(1_000 + 2 * 3_600),
    };
    assert_eq!(
        super::status_notice(Some(&lunch), 1_000),
        "your status: 🍕 lunch · ends in 2h · :status clear clears it"
    );
    assert_eq!(
        super::status_notice(Some(&lunch), 1_000 + 3 * 3_600),
        "you have no status · :status <text> sets one"
    );
    assert_eq!(
        super::status_notice(None, 0),
        "you have no status · :status <text> sets one"
    );
}

#[tokio::test]
async fn bare_status_does_not_clear() {
    let temporary = TempDir::new().unwrap();
    let mut app = app_with_identity(&temporary).await;
    command(&mut app, "status").await;
    // Without a connection v0.16.0 answered "cached read-only mode" and,
    // connected, published an empty status; v0.17.0 reads it.
    assert_eq!(
        app.status_error.as_deref(),
        Some("you have no status · :status <text> sets one")
    );
    command(&mut app, "status clear").await;
    assert!(
        app.status_error
            .as_deref()
            .is_some_and(|error| error.starts_with("cached read-only mode"))
    );
}
