use bzz::ui::{
    input::{InputContext, InputDispatch, InputRouter},
    keymap::{KeyAction, KeyMap, UiAction, map_insert},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn workspace_keymap_uses_typed_navigation_and_leader_sequences() {
    let keymap = KeyMap::builtin();
    let mut router = InputRouter::default();
    let workspace = InputContext::workspace();

    assert_eq!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char('j'), KeyModifiers::NONE)
        ),
        InputDispatch::Action(UiAction::SelectNext)
    );
    assert!(matches!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char('g'), KeyModifiers::NONE)
        ),
        InputDispatch::Pending { .. }
    ));
    assert_eq!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char('g'), KeyModifiers::NONE)
        ),
        InputDispatch::Action(UiAction::JumpTop)
    );
    assert!(matches!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char(' '), KeyModifiers::NONE)
        ),
        InputDispatch::Pending { .. }
    ));
    assert_eq!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char('n'), KeyModifiers::NONE)
        ),
        InputDispatch::Action(UiAction::OpenInbox)
    );
    assert_eq!(
        router.dispatch(
            &keymap,
            workspace,
            key(KeyCode::Char('Q'), KeyModifiers::SHIFT)
        ),
        InputDispatch::Owned(bzz::ui::input::InputOwner::Route)
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
