//! Pure keyboard ownership and sequence routing for the v0.4 TUI.
//!
//! This module has no terminal, signer, network, store, or application-service
//! dependency. Callers turn [`InputDispatch::Action`] into a reducer action and
//! handle [`InputDispatch::Owned`] in the input owner named by the dispatch.

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::keymap::{KeyChord, KeyLookup, KeyMap, KeyScope, UiAction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputOwner {
    Overlay,
    ComposerCompletion,
    Composer,
    Filter,
    Route,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputContext {
    pub overlay_open: bool,
    pub composer_completion_open: bool,
    pub composer_open: bool,
    pub filter_open: bool,
    pub route_scope: KeyScope,
}

impl InputContext {
    pub const fn workspace() -> Self {
        Self {
            overlay_open: false,
            composer_completion_open: false,
            composer_open: false,
            filter_open: false,
            route_scope: KeyScope::Workspace,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputDispatch {
    Action(UiAction),
    Pending { next: Vec<KeyChord> },
    Owned(InputOwner),
    Noop,
}

/// State for an in-progress key sequence. A failed continuation is consumed;
/// it is intentionally not replayed to an overlay or route below it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputRouter {
    sequence_scope: Option<KeyScope>,
    sequence: Vec<KeyChord>,
}

impl InputRouter {
    pub fn sequence(&self) -> &[KeyChord] {
        &self.sequence
    }

    pub fn sequence_active(&self) -> bool {
        !self.sequence.is_empty()
    }

    pub fn cancel_sequence(&mut self) {
        self.sequence_scope = None;
        self.sequence.clear();
    }

    pub fn dispatch(
        &mut self,
        keymap: &KeyMap,
        context: InputContext,
        event: KeyEvent,
    ) -> InputDispatch {
        if self.sequence_active() {
            return self.dispatch_sequence(keymap, event);
        }

        if context.overlay_open {
            return self.dispatch_scope(keymap, KeyScope::Overlay, event, InputOwner::Overlay);
        }
        if context.composer_completion_open {
            return InputDispatch::Owned(InputOwner::ComposerCompletion);
        }
        if context.composer_open {
            return self.dispatch_text_scope(
                keymap,
                KeyScope::Composer,
                event,
                InputOwner::Composer,
            );
        }
        if context.filter_open {
            return self.dispatch_text_scope(keymap, KeyScope::Filter, event, InputOwner::Filter);
        }
        self.dispatch_scope(keymap, context.route_scope, event, InputOwner::Route)
    }

    fn dispatch_sequence(&mut self, keymap: &KeyMap, event: KeyEvent) -> InputDispatch {
        if matches!(event.code, KeyCode::Esc) && event.modifiers.is_empty() {
            self.cancel_sequence();
            return InputDispatch::Noop;
        }
        let Some(scope) = self.sequence_scope else {
            self.cancel_sequence();
            return InputDispatch::Noop;
        };
        self.sequence.push(KeyChord::from_event(event));
        match keymap.lookup(scope, &self.sequence) {
            KeyLookup::Action(action) => {
                self.cancel_sequence();
                InputDispatch::Action(action)
            }
            KeyLookup::Pending => InputDispatch::Pending {
                next: keymap.next_chords(scope, &self.sequence),
            },
            KeyLookup::NoMatch => {
                self.cancel_sequence();
                InputDispatch::Noop
            }
        }
    }

    fn dispatch_text_scope(
        &mut self,
        keymap: &KeyMap,
        scope: KeyScope,
        event: KeyEvent,
        owner: InputOwner,
    ) -> InputDispatch {
        match keymap.lookup(scope, &[KeyChord::from_event(event)]) {
            KeyLookup::Action(action) => InputDispatch::Action(action),
            // Text scopes have no builtin sequences. Treat a user-provided
            // prefix as owned input rather than stealing/replaying its first
            // character; validation currently prohibits printable prefixes.
            KeyLookup::Pending | KeyLookup::NoMatch => InputDispatch::Owned(owner),
        }
    }

    fn dispatch_scope(
        &mut self,
        keymap: &KeyMap,
        scope: KeyScope,
        event: KeyEvent,
        owner: InputOwner,
    ) -> InputDispatch {
        let chord = KeyChord::from_event(event);
        match keymap.lookup(scope, &[chord]) {
            KeyLookup::Action(action) => InputDispatch::Action(action),
            KeyLookup::Pending => {
                self.sequence_scope = Some(scope);
                self.sequence.push(chord);
                InputDispatch::Pending {
                    next: keymap.next_chords(scope, &self.sequence),
                }
            }
            KeyLookup::NoMatch => InputDispatch::Owned(owner),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::ui::keymap::{KeyMap, UiAction};

    use super::{InputContext, InputDispatch, InputOwner, InputRouter};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn active_leader_sequence_owns_the_next_key_before_an_overlay() {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        assert!(matches!(
            router.dispatch(
                &keymap,
                InputContext::workspace(),
                key(KeyCode::Char(' '), KeyModifiers::NONE)
            ),
            InputDispatch::Pending { .. }
        ));
        let result = router.dispatch(
            &keymap,
            InputContext {
                overlay_open: true,
                ..InputContext::workspace()
            },
            key(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        assert_eq!(result, InputDispatch::Action(UiAction::OpenInbox));
        assert!(!router.sequence_active());
    }

    #[test]
    fn overlay_does_not_leak_global_navigation_to_the_workspace() {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        let context = InputContext {
            overlay_open: true,
            ..InputContext::workspace()
        };
        assert_eq!(
            router.dispatch(
                &keymap,
                context,
                key(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            InputDispatch::Owned(InputOwner::Overlay)
        );
        assert_eq!(
            router.dispatch(&keymap, context, key(KeyCode::Esc, KeyModifiers::NONE)),
            InputDispatch::Action(UiAction::BackOrQuit)
        );
    }

    #[test]
    fn composer_keeps_printable_text_out_of_the_leader_router() {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        let context = InputContext {
            composer_open: true,
            ..InputContext::workspace()
        };
        assert_eq!(
            router.dispatch(
                &keymap,
                context,
                key(KeyCode::Char(' '), KeyModifiers::NONE)
            ),
            InputDispatch::Owned(InputOwner::Composer)
        );
        assert_eq!(
            router.dispatch(
                &keymap,
                context,
                key(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            InputDispatch::Owned(InputOwner::Composer)
        );
        assert_eq!(
            router.dispatch(
                &keymap,
                context,
                key(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            InputDispatch::Action(UiAction::ClearComposer)
        );
    }

    #[test]
    fn escape_cancels_a_prefix_without_replaying_it() {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        let context = InputContext::workspace();
        let _ = router.dispatch(
            &keymap,
            context,
            key(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert!(router.sequence_active());
        assert_eq!(
            router.dispatch(&keymap, context, key(KeyCode::Esc, KeyModifiers::NONE)),
            InputDispatch::Noop
        );
        assert!(!router.sequence_active());
    }

    #[test]
    fn a_failed_prefix_is_consumed_not_forwarded_to_a_lower_owner() {
        let keymap = KeyMap::builtin();
        let mut router = InputRouter::default();
        let context = InputContext::workspace();
        let _ = router.dispatch(
            &keymap,
            context,
            key(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert_eq!(
            router.dispatch(
                &keymap,
                context,
                key(KeyCode::Char('z'), KeyModifiers::NONE)
            ),
            InputDispatch::Noop
        );
    }
}
