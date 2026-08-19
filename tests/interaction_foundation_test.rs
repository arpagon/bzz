//! Deterministic interaction journeys: terminal events enter the typed router,
//! then only a presentation reducer and named effects are observed.

use bzz::ui::{
    action::{WorkspaceEffect, WorkspaceState, reduce_workspace},
    actions::{ActionContext, ActionMenu, derive as derive_actions},
    input::{InputContext, InputDispatch, InputOwner, InputRouter},
    keymap::{KeyMap, UiAction},
    state::{PresentationState, Route},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

fn key(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
}

#[test]
fn leader_inbox_journey_only_changes_presentation_and_emits_a_named_effect() {
    let keymap = KeyMap::builtin();
    let mut router = InputRouter::default();
    let mut state = WorkspaceState::new(PresentationState::default(), 0, 1, true, true, false);

    assert!(matches!(
        router.dispatch(&keymap, InputContext::workspace(), key(' ')),
        InputDispatch::Pending { .. }
    ));
    assert_eq!(
        router.dispatch(&keymap, InputContext::workspace(), key('n')),
        InputDispatch::Action(UiAction::OpenInbox)
    );
    assert_eq!(
        reduce_workspace(&mut state, UiAction::OpenInbox),
        WorkspaceEffect::OpenInbox
    );
    assert_eq!(state.presentation.route, Route::Inbox);
}

#[test]
fn custom_sequence_routes_without_terminal_io() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("keymap.toml");
    std::fs::write(
        &path,
        "[[binding]]\nscope = 'workspace'\nkeys = ['ctrl-x', 'i']\naction = 'open-inbox'\n",
    )
    .unwrap();
    let keymap = KeyMap::load_from(&path).unwrap();
    let mut router = InputRouter::default();

    assert!(matches!(
        router.dispatch(
            &keymap,
            InputContext::workspace(),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        ),
        InputDispatch::Pending { .. }
    ));
    assert_eq!(
        router.dispatch(&keymap, InputContext::workspace(), key('i')),
        InputDispatch::Action(UiAction::OpenInbox)
    );
}

#[test]
fn leader_context_actions_are_derived_before_any_effect_executes() {
    let keymap = KeyMap::builtin();
    let mut router = InputRouter::default();
    assert!(matches!(
        router.dispatch(&keymap, InputContext::workspace(), key(' ')),
        InputDispatch::Pending { .. }
    ));
    assert_eq!(
        router.dispatch(&keymap, InputContext::workspace(), key('a')),
        InputDispatch::Action(UiAction::OpenContextActions)
    );
    let mut menu = ActionMenu::new(derive_actions(ActionContext {
        route: Route::Workspace,
        focus: bzz::ui::state::FocusSurface::Timeline,
        has_channel: true,
        has_selected_event: true,
        selected_event_is_own: false,
        selected_event_has_media: false,
        context_open: false,
        can_publish: true,
    }));
    assert!(
        menu.entries()
            .iter()
            .any(|entry| entry.enabled && entry.action == UiAction::React)
    );
    assert!(
        menu.entries()
            .iter()
            .any(|entry| !entry.enabled && entry.action == UiAction::Delete)
    );
    menu.move_by(1);
    assert!(menu.selected().is_some());
}

#[test]
fn composer_text_is_owned_before_workspace_shortcuts() {
    let keymap = KeyMap::builtin();
    let mut router = InputRouter::default();
    let composer = InputContext {
        composer_open: true,
        ..InputContext::workspace()
    };

    for character in [' ', 'j', 'q'] {
        assert_eq!(
            router.dispatch(&keymap, composer, key(character)),
            InputDispatch::Owned(InputOwner::Composer)
        );
    }
}
