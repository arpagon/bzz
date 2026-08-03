use bzz::ui::keymap::{KeyAction, map_insert, map_normal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn normal_keymap_has_safe_quit_and_modal_navigation() {
    assert_eq!(
        map_normal(
            KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT),
            false
        ),
        KeyAction::Quit
    );
    assert_eq!(
        map_normal(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), false),
        KeyAction::Down
    );
    assert_eq!(
        map_normal(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), true),
        KeyAction::First
    );
    assert_eq!(
        map_normal(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            false
        ),
        KeyAction::Theme
    );
    assert_eq!(
        map_normal(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), false),
        KeyAction::Search
    );
    assert_eq!(
        map_normal(
            KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT),
            false
        ),
        KeyAction::Inbox
    );
    assert_eq!(
        map_normal(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            false
        ),
        KeyAction::NewDm
    );
}

#[test]
fn composer_distinguishes_send_and_newline() {
    assert_eq!(
        map_insert(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        KeyAction::Submit
    );
    assert_eq!(
        map_insert(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        KeyAction::Newline
    );
    assert_eq!(
        map_insert(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
        KeyAction::Newline
    );
}
