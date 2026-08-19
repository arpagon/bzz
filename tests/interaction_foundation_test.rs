//! Deterministic interaction journeys: terminal events enter the typed router,
//! then only a presentation reducer and named effects are observed.

use bzz::ui::{
    action::{WorkspaceEffect, WorkspaceState, reduce_workspace},
    actions::{ActionContext, ActionMenu, derive as derive_actions},
    input::{InputContext, InputDispatch, InputOwner, InputRouter},
    keymap::{KeyMap, KeyScope, UiAction},
    state::{PresentationState, Route},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use proptest::prelude::*;
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
        has_inbox_selection: false,
        inbox_has_context: false,
        inbox_can_reply: false,
        inbox_visible_count: 0,
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
fn inbox_scope_routes_only_explicit_triage_effects() {
    let keymap = KeyMap::builtin();
    let context = InputContext {
        overlay_open: false,
        composer_completion_open: false,
        composer_open: false,
        filter_open: false,
        route_scope: KeyScope::Inbox,
    };
    let mut router = InputRouter::default();
    assert_eq!(
        router.dispatch(&keymap, context, key('m')),
        InputDispatch::Action(UiAction::MarkRead)
    );
    assert_eq!(
        router.dispatch(&keymap, context, key('o')),
        InputDispatch::Action(UiAction::OpenCanonicalContext)
    );
    assert_eq!(
        router.dispatch(&keymap, context, key('a')),
        InputDispatch::Action(UiAction::MarkVisibleRead)
    );
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

proptest! {
    #[test]
    fn printable_composer_streams_never_reach_route_actions(
        input in proptest::collection::vec("[ -~]", 0..128),
    ) {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        let composer = InputContext {
            composer_open: true,
            ..InputContext::workspace()
        };
        for fragment in input {
            for character in fragment.chars() {
                prop_assert_eq!(
                    router.dispatch(&keymap, composer, key(character)),
                    InputDispatch::Owned(InputOwner::Composer),
                );
            }
        }
    }

    #[test]
    fn arbitrary_workspace_key_streams_keep_sequences_bounded(
        input in proptest::collection::vec("[ -~]", 0..256),
    ) {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        for fragment in input {
            for character in fragment.chars() {
                let _ = router.dispatch(&keymap, InputContext::workspace(), key(character));
                prop_assert!(router.sequence().len() <= 4);
            }
        }
    }
}
