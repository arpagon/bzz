//! Small deterministic TUI journey harness.
//!
//! It deliberately joins only the typed key router, pure Inbox reducer, and
//! TestBackend renderer. It has no App, store, signer, relay, process, or
//! terminal capability, so recorded effects are the authority for interaction
//! semantics rather than a network side effect.

use std::collections::HashMap;

use bzz::{
    domain::{InboxItem, Message, Profile},
    ui::{
        action::{InboxEffect, InboxWorkspaceState, reduce_inbox},
        inbox::{self, InboxState, InboxView},
        input::{InputContext, InputDispatch, InputRouter},
        keymap::{KeyMap, KeyScope, UiAction},
        state::{FocusSurface, PresentationState},
        theme::Theme,
    },
};
use crossterm::event::KeyEvent;
use ratatui::{Terminal, backend::TestBackend};

/// A recording-only Inbox route. Effects are intentionally not executed: no
/// test journey can read, publish, or make an external request by accident.
pub struct InboxHarness {
    keymap: KeyMap,
    router: InputRouter,
    pub presentation: PresentationState,
    pub inbox: InboxState,
    pub items: Vec<InboxItem>,
    pub messages: Vec<Message>,
    pub effects: Vec<InboxEffect>,
    terminal: Terminal<TestBackend>,
    theme: Theme,
}

impl InboxHarness {
    pub fn new(width: u16, height: u16, items: Vec<InboxItem>, messages: Vec<Message>) -> Self {
        let mut presentation = PresentationState::default();
        presentation.enter_inbox();
        let mut inbox = InboxState::default();
        inbox.reconcile(&items);
        Self {
            keymap: KeyMap::builtin(),
            router: InputRouter::default(),
            presentation,
            inbox,
            items,
            messages,
            effects: Vec::new(),
            terminal: Terminal::new(TestBackend::new(width, height))
                .expect("TestBackend terminal is available"),
            theme: Theme::default(),
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) {
        let context = InputContext {
            overlay_open: false,
            composer_completion_open: false,
            composer_open: false,
            filter_open: false,
            route_scope: KeyScope::Inbox,
        };
        if let InputDispatch::Action(action) = self.router.dispatch(&self.keymap, context, key) {
            self.reduce(action);
        }
    }

    pub fn render(&mut self) {
        let view = InboxView {
            items: &self.items,
            messages: &self.messages,
            profiles: &HashMap::<String, Profile>::new(),
            focus: self.presentation.focus,
            theme: &self.theme,
            loading: false,
        };
        self.terminal
            .draw(|frame| {
                inbox::render(frame, frame.area(), &mut self.inbox, view);
            })
            .expect("TestBackend draw succeeds");
    }

    pub fn screen_text(&self) -> String {
        self.terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn reduce(&mut self, action: UiAction) {
        let mut state = InboxWorkspaceState {
            presentation: self.presentation.clone(),
            narrow_layout: self.inbox.narrow_layout,
            narrow_detail: self.inbox.narrow_detail,
            detail_available: self.inbox.selected(&self.items).is_some(),
        };
        let effect = reduce_inbox(&mut state, action);
        self.presentation = state.presentation;
        self.inbox.narrow_detail = state.narrow_detail;
        match effect {
            InboxEffect::MoveSelection(delta) => self.inbox.move_by(&self.items, delta),
            InboxEffect::MoveSelectionToEdge { last } => self.inbox.move_to_edge(&self.items, last),
            InboxEffect::CycleFilter => {
                self.inbox.filter = self.inbox.filter.next();
                self.inbox.reconcile(&self.items);
            }
            _ => {}
        }
        self.effects.push(effect);
    }

    pub fn inbox_focus(&self) -> FocusSurface {
        self.presentation.focus
    }
}
