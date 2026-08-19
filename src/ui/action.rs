//! Pure workspace interaction reducer and typed application effects.
//!
//! This is deliberately limited to presentation state plus named effects. It
//! cannot hold or reach the signer, store, relay, HTTP client, process runner,
//! terminal, or secret-bearing configuration. `App` is the sole executor of
//! the effects after applying this reducer's state transition.

use crate::ui::{
    keymap::UiAction,
    state::{FocusSurface, Overlay, PresentationState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceState {
    pub presentation: PresentationState,
    pub community_cursor: usize,
    pub community_count: usize,
    pub communities_visible: bool,
    pub channels_visible: bool,
    pub context_visible: bool,
}

impl WorkspaceState {
    pub fn new(
        presentation: PresentationState,
        community_cursor: usize,
        community_count: usize,
        communities_visible: bool,
        channels_visible: bool,
        context_visible: bool,
    ) -> Self {
        Self {
            presentation,
            community_cursor: community_cursor.min(community_count.saturating_sub(1)),
            community_count,
            communities_visible,
            channels_visible,
            context_visible,
        }
    }

    fn focus_workspace(&mut self, focus: FocusSurface) {
        self.presentation.set_workspace_focus(focus);
    }

    fn cycle_focus(&mut self, delta: isize) {
        let mut surfaces = Vec::with_capacity(4);
        if self.communities_visible && self.community_count > 0 {
            surfaces.push(FocusSurface::Communities);
        }
        if self.channels_visible {
            surfaces.push(FocusSurface::Channels);
        }
        surfaces.push(FocusSurface::Timeline);
        if self.context_visible {
            surfaces.push(FocusSurface::Context);
        }
        let current = surfaces
            .iter()
            .position(|surface| *surface == self.presentation.focus)
            .unwrap_or_default();
        let len = surfaces.len();
        let steps = delta.unsigned_abs() % len;
        let next = if delta.is_negative() {
            (current + len - steps) % len
        } else {
            (current + steps) % len
        };
        if let Some(focus) = surfaces.get(next).copied() {
            self.focus_workspace(focus);
        }
    }

    fn move_community_cursor(&mut self, delta: isize) {
        self.community_cursor = self
            .community_cursor
            .saturating_add_signed(delta)
            .min(self.community_count.saturating_sub(1));
    }
}

/// A detached viewport movement. It deliberately carries no row identity, so
/// selecting a message and scrolling its rendered rows are separate actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewportScroll {
    Lines(isize),
    HalfPage(isize),
}

/// Named effects emitted by [`reduce_workspace`]. None of these values holds a
/// capability; the application adapter independently validates availability
/// before performing an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEffect {
    None,
    RequestQuitConfirmation,
    CloseContext,
    EnsureContext,
    MoveSelection(isize),
    MoveSelectionToEdge { last: bool },
    ScrollViewport(ViewportScroll),
    ResizeFocusedSidePane(isize),
    ActivateFocused,
    ActivateCommunity(usize),
    OpenComposer,
    OpenSearch,
    OpenInbox,
    OpenFinder,
    OpenContextActions,
    Refresh,
    OpenOptions,
    OpenCommand,
    OpenDmPicker,
    ToggleThread,
    OpenMediaPreview,
    OpenReaction,
    ConfirmDelete,
    MarkUnread,
    Unavailable(UiAction),
}

/// Reduce one action using only presentation and local workspace facts.
pub fn reduce_workspace(state: &mut WorkspaceState, action: UiAction) -> WorkspaceEffect {
    match action {
        UiAction::BackOrQuit => {
            if state.presentation.back() {
                WorkspaceEffect::None
            } else if state.context_visible {
                state.context_visible = false;
                state.focus_workspace(FocusSurface::Timeline);
                WorkspaceEffect::CloseContext
            } else {
                WorkspaceEffect::RequestQuitConfirmation
            }
        }
        UiAction::OpenHelp => {
            state.presentation.open_overlay(Overlay::Help);
            WorkspaceEffect::None
        }
        UiAction::ToggleCommunities => {
            state.communities_visible = !state.communities_visible;
            if !state.communities_visible && state.presentation.focus == FocusSurface::Communities {
                state.focus_workspace(FocusSurface::Timeline);
            }
            WorkspaceEffect::None
        }
        UiAction::ToggleChannels => {
            state.channels_visible = !state.channels_visible;
            if !state.channels_visible && state.presentation.focus == FocusSurface::Channels {
                state.focus_workspace(FocusSurface::Timeline);
            }
            WorkspaceEffect::None
        }
        UiAction::ToggleContext => {
            if state.context_visible {
                state.context_visible = false;
                state.focus_workspace(FocusSurface::Timeline);
                WorkspaceEffect::CloseContext
            } else {
                WorkspaceEffect::EnsureContext
            }
        }
        UiAction::FocusCommunities => {
            state.communities_visible = true;
            state.focus_workspace(FocusSurface::Communities);
            WorkspaceEffect::None
        }
        UiAction::FocusChannels => {
            state.channels_visible = true;
            state.focus_workspace(FocusSurface::Channels);
            WorkspaceEffect::None
        }
        UiAction::FocusTimeline => {
            state.focus_workspace(FocusSurface::Timeline);
            WorkspaceEffect::None
        }
        UiAction::FocusContext => {
            if state.context_visible {
                state.focus_workspace(FocusSurface::Context);
                WorkspaceEffect::None
            } else {
                WorkspaceEffect::EnsureContext
            }
        }
        UiAction::NextFocus => {
            state.cycle_focus(1);
            WorkspaceEffect::None
        }
        UiAction::PreviousFocus => {
            state.cycle_focus(-1);
            WorkspaceEffect::None
        }
        UiAction::ResizeFocusedNarrow => WorkspaceEffect::ResizeFocusedSidePane(-1),
        UiAction::ResizeFocusedWide => WorkspaceEffect::ResizeFocusedSidePane(1),
        UiAction::SelectPrevious => {
            if state.presentation.focus == FocusSurface::Communities {
                state.move_community_cursor(-1);
                WorkspaceEffect::None
            } else {
                WorkspaceEffect::MoveSelection(-1)
            }
        }
        UiAction::SelectNext => {
            if state.presentation.focus == FocusSurface::Communities {
                state.move_community_cursor(1);
                WorkspaceEffect::None
            } else {
                WorkspaceEffect::MoveSelection(1)
            }
        }
        UiAction::HalfPageUp => WorkspaceEffect::ScrollViewport(ViewportScroll::HalfPage(-1)),
        UiAction::HalfPageDown => WorkspaceEffect::ScrollViewport(ViewportScroll::HalfPage(1)),
        UiAction::JumpTop if state.presentation.focus == FocusSurface::Communities => {
            state.community_cursor = 0;
            WorkspaceEffect::None
        }
        UiAction::JumpBottom if state.presentation.focus == FocusSurface::Communities => {
            state.community_cursor = state.community_count.saturating_sub(1);
            WorkspaceEffect::None
        }
        UiAction::JumpTop => WorkspaceEffect::MoveSelectionToEdge { last: false },
        UiAction::JumpBottom => WorkspaceEffect::MoveSelectionToEdge { last: true },
        UiAction::ScrollViewportUp => WorkspaceEffect::ScrollViewport(ViewportScroll::Lines(-1)),
        UiAction::ScrollViewportDown => WorkspaceEffect::ScrollViewport(ViewportScroll::Lines(1)),
        UiAction::ActivateFocused => {
            if state.presentation.focus == FocusSurface::Communities {
                WorkspaceEffect::ActivateCommunity(state.community_cursor)
            } else {
                WorkspaceEffect::ActivateFocused
            }
        }
        UiAction::Compose => WorkspaceEffect::OpenComposer,
        UiAction::Filter | UiAction::Search => WorkspaceEffect::OpenSearch,
        UiAction::OpenInbox => {
            state.presentation.enter_inbox();
            WorkspaceEffect::OpenInbox
        }
        UiAction::ChannelSwitcher => WorkspaceEffect::OpenFinder,
        UiAction::OpenContextActions => WorkspaceEffect::OpenContextActions,
        UiAction::Refresh => WorkspaceEffect::Refresh,
        UiAction::OpenOptions => WorkspaceEffect::OpenOptions,
        UiAction::OpenCommand => WorkspaceEffect::OpenCommand,
        UiAction::NewDm => WorkspaceEffect::OpenDmPicker,
        UiAction::ToggleThread => WorkspaceEffect::ToggleThread,
        UiAction::Preview => WorkspaceEffect::OpenMediaPreview,
        UiAction::React => WorkspaceEffect::OpenReaction,
        UiAction::Delete => WorkspaceEffect::ConfirmDelete,
        UiAction::MarkUnread => WorkspaceEffect::MarkUnread,
        unsupported => WorkspaceEffect::Unavailable(unsupported),
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::{
        keymap::UiAction,
        state::{FocusSurface, Overlay, PresentationState, Route},
    };

    use super::{ViewportScroll, WorkspaceEffect, WorkspaceState, reduce_workspace};

    fn state() -> WorkspaceState {
        WorkspaceState::new(PresentationState::default(), 0, 3, true, true, false)
    }

    #[test]
    fn focus_and_selection_are_pure_presentation_transitions() {
        let mut state = state();
        assert_eq!(
            reduce_workspace(&mut state, UiAction::FocusCommunities),
            WorkspaceEffect::None
        );
        assert_eq!(state.presentation.focus, FocusSurface::Communities);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::SelectNext),
            WorkspaceEffect::None
        );
        assert_eq!(state.community_cursor, 1);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::PreviousFocus),
            WorkspaceEffect::None
        );
        assert_eq!(state.presentation.focus, FocusSurface::Timeline);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::NextFocus),
            WorkspaceEffect::None
        );
        assert_eq!(state.presentation.focus, FocusSurface::Communities);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::ActivateFocused),
            WorkspaceEffect::ActivateCommunity(1)
        );
    }

    #[test]
    fn leader_actions_emit_named_effects_without_capabilities() {
        let mut state = state();
        assert_eq!(
            reduce_workspace(&mut state, UiAction::OpenInbox),
            WorkspaceEffect::OpenInbox
        );
        assert_eq!(state.presentation.route, Route::Inbox);
        assert_eq!(state.presentation.focus, FocusSurface::InboxList);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::OpenHelp),
            WorkspaceEffect::None
        );
        assert_eq!(state.presentation.overlay, Some(Overlay::Help));
    }

    #[test]
    fn detached_scroll_effect_never_moves_selection() {
        let mut state = state();
        let selected_before = state.community_cursor;
        assert_eq!(
            reduce_workspace(&mut state, UiAction::HalfPageDown),
            WorkspaceEffect::ScrollViewport(ViewportScroll::HalfPage(1))
        );
        assert_eq!(state.community_cursor, selected_before);
    }

    #[test]
    fn unavailable_actions_remain_typed_and_do_not_fall_through() {
        let mut state = state();
        assert_eq!(
            reduce_workspace(&mut state, UiAction::MarkRead),
            WorkspaceEffect::Unavailable(UiAction::MarkRead)
        );
    }

    #[test]
    fn back_closes_context_before_requesting_quit() {
        let mut state = WorkspaceState::new(PresentationState::default(), 0, 0, true, true, true);
        state
            .presentation
            .set_workspace_focus(FocusSurface::Context);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::BackOrQuit),
            WorkspaceEffect::CloseContext
        );
        assert_eq!(state.presentation.focus, FocusSurface::Timeline);
        assert_eq!(
            reduce_workspace(&mut state, UiAction::BackOrQuit),
            WorkspaceEffect::RequestQuitConfirmation
        );
    }
}
