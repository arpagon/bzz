//! Presentation-only state shared by the v0.4 interaction reducer.
//!
//! These types intentionally carry no network handles, signer, store, task, or
//! render cache. They can therefore be reduced and tested without terminal I/O
//! or a live Buzz community.

use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Route {
    #[default]
    Workspace,
    Inbox,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FocusSurface {
    Communities,
    Channels,
    #[default]
    Timeline,
    Context,
    InboxList,
    InboxDetail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overlay {
    Help,
    WhichKey,
    Actions,
    Finder,
    Command,
    Search,
    Theme,
    Reaction,
    Confirmation,
    MediaPreview,
    Attachment,
    DmPicker,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfirmationKind {
    Quit,
    Delete,
    InboxRead,
    ClearDraft,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentPrompt {
    Upload,
    Save,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposerTarget {
    pub community_id: Uuid,
    pub channel_id: Uuid,
    pub thread_root_id: Option<String>,
    /// The validated parent event for an ordinary reply. Root-only replies use
    /// the root as their parent; a top-level target has no parent.
    pub parent_event_id: Option<String>,
}

/// Identity-based selection is independent from pixel/row scrolling. Lists may
/// insert, filter, rewrap, or evict items without turning a selected stable ID
/// into an accidental different row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewportState {
    pub selected_id: Option<String>,
    pub scroll: usize,
    pub horizontal_scroll: usize,
    pub keep_selection_visible: bool,
    pub viewport_height: usize,
}

impl ViewportState {
    pub fn select(&mut self, id: Option<String>) {
        self.selected_id = id;
        self.keep_selection_visible = true;
    }

    pub fn scroll_by(&mut self, delta: isize, content_len: usize) {
        let max_scroll = content_len.saturating_sub(self.viewport_height.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);
        self.keep_selection_visible = false;
    }

    pub fn reconcile<I>(&mut self, visible_ids: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let ids = visible_ids
            .into_iter()
            .map(|id| id.as_ref().to_owned())
            .collect::<Vec<_>>();
        if !self
            .selected_id
            .as_ref()
            .is_some_and(|selected| ids.iter().any(|id| id == selected))
        {
            self.selected_id = ids.first().cloned();
            self.keep_selection_visible = true;
        }
        self.reconcile_scroll(&ids);
    }

    /// Clamp scrolling and, after an explicit selection change, bring the
    /// stable selected ID into view without translating it through a row index.
    pub fn reconcile_scroll(&mut self, visible_ids: &[String]) {
        let height = self.viewport_height.max(1);
        let max_scroll = visible_ids.len().saturating_sub(height);
        self.scroll = self.scroll.min(max_scroll);
        if !self.keep_selection_visible {
            return;
        }
        let Some(selected) = &self.selected_id else {
            return;
        };
        let Some(index) = visible_ids.iter().position(|id| id == selected) else {
            return;
        };
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll.saturating_add(height) {
            self.scroll = index.saturating_add(1).saturating_sub(height);
        }
        self.keep_selection_visible = false;
    }

    pub fn set_viewport_height(&mut self, height: usize, visible_ids: &[String]) {
        self.viewport_height = height.max(1);
        self.reconcile_scroll(visible_ids);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PresentationState {
    pub route: Route,
    pub focus: FocusSurface,
    pub overlay: Option<Overlay>,
    pub composer_target: Option<ComposerTarget>,
    pub confirmation: Option<ConfirmationKind>,
    pub attachment_prompt: Option<AttachmentPrompt>,
    /// Canonical context opened from Inbox returns here on the next back
    /// action. It is presentation-only and never changes message authority.
    pub inbox_return: bool,
}

impl PresentationState {
    pub fn enter_inbox(&mut self) {
        self.route = Route::Inbox;
        self.focus = FocusSurface::InboxList;
        self.close_overlay();
        self.inbox_return = false;
    }

    pub fn open_inbox_context(&mut self, detail: bool) {
        self.route = Route::Workspace;
        self.focus = if detail {
            FocusSurface::Context
        } else {
            FocusSurface::Timeline
        };
        self.close_overlay();
        self.inbox_return = true;
    }

    pub fn clear_inbox_return(&mut self) {
        self.inbox_return = false;
    }

    /// Returns true when this call consumed a route transition. The caller may
    /// then decide whether a second back action should request quit.
    pub fn back(&mut self) -> bool {
        if self.overlay.is_some() {
            self.close_overlay();
            return true;
        }
        if self.composer_target.take().is_some() {
            return true;
        }
        if self.route == Route::Inbox {
            self.route = Route::Workspace;
            self.focus = FocusSurface::Timeline;
            return true;
        }
        if self.inbox_return {
            self.enter_inbox();
            return true;
        }
        false
    }

    pub fn open_overlay(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
        self.confirmation = None;
        self.attachment_prompt = None;
    }

    pub fn open_confirmation(&mut self, kind: ConfirmationKind) {
        self.overlay = Some(Overlay::Confirmation);
        self.confirmation = Some(kind);
    }

    pub fn open_attachment_prompt(&mut self, prompt: AttachmentPrompt) {
        self.overlay = Some(Overlay::Attachment);
        self.attachment_prompt = Some(prompt);
    }

    pub fn set_workspace_focus(&mut self, focus: FocusSurface) {
        if self.route != Route::Workspace
            || !matches!(
                focus,
                FocusSurface::Communities
                    | FocusSurface::Channels
                    | FocusSurface::Timeline
                    | FocusSurface::Context
            )
        {
            return;
        }
        self.focus = focus;
    }

    pub fn set_inbox_focus(&mut self, detail: bool) {
        if self.route == Route::Inbox {
            self.focus = if detail {
                FocusSurface::InboxDetail
            } else {
                FocusSurface::InboxList
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{FocusSurface, Overlay, PresentationState, Route, ViewportState};

    #[test]
    fn viewport_keeps_selection_identity_when_new_items_arrive() {
        let mut viewport = ViewportState {
            selected_id: Some("conversation-b".into()),
            scroll: 1,
            viewport_height: 2,
            ..ViewportState::default()
        };
        viewport.reconcile(["conversation-new", "conversation-b", "conversation-a"]);
        assert_eq!(viewport.selected_id.as_deref(), Some("conversation-b"));
        assert_eq!(viewport.scroll, 1);
    }

    #[test]
    fn detached_scroll_does_not_change_the_selected_id() {
        let mut viewport = ViewportState {
            selected_id: Some("event-2".into()),
            viewport_height: 2,
            ..ViewportState::default()
        };
        viewport.scroll_by(4, 10);
        assert_eq!(viewport.selected_id.as_deref(), Some("event-2"));
        assert_eq!(viewport.scroll, 4);
        assert!(!viewport.keep_selection_visible);
    }

    #[test]
    fn selection_scrolls_into_view_without_changing_selection_identity() {
        let mut viewport = ViewportState {
            viewport_height: 2,
            scroll: 0,
            ..ViewportState::default()
        };
        let ids = ["a", "b", "c", "d"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        viewport.select(Some("d".into()));
        viewport.reconcile_scroll(&ids);
        assert_eq!(viewport.selected_id.as_deref(), Some("d"));
        assert_eq!(viewport.scroll, 2);
    }

    #[test]
    fn back_unwinds_overlay_composer_then_inbox_route() {
        let mut state = PresentationState::default();
        state.enter_inbox();
        state.open_overlay(Overlay::Actions);
        assert!(state.back());
        assert_eq!(state.route, Route::Inbox);
        state.composer_target = Some(super::ComposerTarget {
            community_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            thread_root_id: None,
            parent_event_id: None,
        });
        assert!(state.back());
        assert!(state.back());
        assert_eq!(state.route, Route::Workspace);
        assert_eq!(state.focus, FocusSurface::Timeline);
        assert!(!state.back());
    }

    #[test]
    fn canonical_inbox_context_returns_without_reselecting_work() {
        let mut state = PresentationState::default();
        state.enter_inbox();
        state.open_inbox_context(true);
        assert_eq!(state.route, Route::Workspace);
        assert_eq!(state.focus, FocusSurface::Context);
        assert!(state.inbox_return);
        assert!(state.back());
        assert_eq!(state.route, Route::Inbox);
        assert_eq!(state.focus, FocusSurface::InboxList);
        assert!(!state.inbox_return);
    }

    #[test]
    fn focus_cannot_cross_route_boundaries() {
        let mut state = PresentationState::default();
        state.set_workspace_focus(FocusSurface::Context);
        assert_eq!(state.focus, FocusSurface::Context);
        state.enter_inbox();
        state.set_workspace_focus(FocusSurface::Timeline);
        assert_eq!(state.focus, FocusSurface::InboxList);
        state.set_inbox_focus(true);
        assert_eq!(state.focus, FocusSurface::InboxDetail);
    }

    #[test]
    fn typed_prompts_clear_with_their_overlay() {
        let mut state = PresentationState::default();
        state.open_confirmation(super::ConfirmationKind::InboxRead);
        assert_eq!(state.overlay, Some(Overlay::Confirmation));
        assert!(state.back());
        assert_eq!(state.overlay, None);
        assert_eq!(state.confirmation, None);
        state.open_attachment_prompt(super::AttachmentPrompt::Save);
        state.close_overlay();
        assert_eq!(state.attachment_prompt, None);
    }
}
