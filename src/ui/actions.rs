//! Typed contextual actions and availability for the workspace.
//!
//! The registry only consumes already-validated presentation facts. It owns no
//! service, signer, relay, store, or terminal capability; `App` performs the
//! corresponding typed effect only after the user activates an enabled entry.

use crate::ui::{
    keymap::{KeyScope, UiAction},
    state::{FocusSurface, Route},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionContext {
    pub route: Route,
    pub focus: FocusSurface,
    pub has_inbox_selection: bool,
    pub inbox_has_context: bool,
    pub inbox_can_reply: bool,
    pub inbox_visible_count: usize,
    pub has_channel: bool,
    pub has_selected_event: bool,
    pub selected_event_is_own: bool,
    pub selected_event_has_media: bool,
    pub context_open: bool,
    pub can_publish: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextAction {
    pub action: UiAction,
    pub label: &'static str,
    pub enabled: bool,
    pub reason: Option<&'static str>,
}

impl ContextAction {
    const fn enabled(action: UiAction, label: &'static str) -> Self {
        Self {
            action,
            label,
            enabled: true,
            reason: None,
        }
    }

    const fn unavailable(action: UiAction, label: &'static str, reason: &'static str) -> Self {
        Self {
            action,
            label,
            enabled: false,
            reason: Some(reason),
        }
    }
}

/// The action registry is also the keymap authority for non-text scopes. It
/// keeps text-editing intents out of normal routes and prevents an overlay
/// binding from becoming a background shortcut.
pub const fn keymap_allows(scope: KeyScope, action: UiAction) -> bool {
    match scope {
        KeyScope::Composer | KeyScope::Filter => true,
        KeyScope::Overlay => matches!(action, UiAction::BackOrQuit),
        KeyScope::Inbox => !matches!(
            action,
            UiAction::Submit
                | UiAction::InsertNewline
                | UiAction::Complete
                | UiAction::DeletePreviousWord
                | UiAction::DeleteToStart
                | UiAction::DeleteToEnd
                | UiAction::MoveWordLeft
                | UiAction::MoveWordRight
                | UiAction::MoveLineStart
                | UiAction::MoveLineEnd
        ),
        KeyScope::Global | KeyScope::Workspace => !matches!(
            action,
            UiAction::MarkRead
                | UiAction::MarkVisibleRead
                | UiAction::OpenCanonicalContext
                | UiAction::Submit
                | UiAction::InsertNewline
                | UiAction::Complete
                | UiAction::DeletePreviousWord
                | UiAction::DeleteToStart
                | UiAction::DeleteToEnd
                | UiAction::MoveWordLeft
                | UiAction::MoveWordRight
                | UiAction::MoveLineStart
                | UiAction::MoveLineEnd
        ),
    }
}

/// A small, bzz-owned action registry. Enabled entries are deliberately listed
/// before unavailable ones, which leaves unavailable operations discoverable
/// without making selection execute or broaden an operation.
pub fn derive(context: ActionContext) -> Vec<ContextAction> {
    let mut enabled = Vec::new();
    let mut unavailable = Vec::new();
    let mut add = |entry: ContextAction| {
        if entry.enabled {
            enabled.push(entry);
        } else {
            unavailable.push(entry);
        }
    };

    match (context.route, context.focus) {
        (Route::Inbox, FocusSurface::InboxList | FocusSurface::InboxDetail) => {
            add(inbox_action(
                context,
                UiAction::Compose,
                "reply in place",
                context.inbox_can_reply,
                "selected Inbox work has no reply target",
            ));
            add(inbox_action(
                context,
                UiAction::OpenCanonicalContext,
                "open source context",
                context.inbox_has_context,
                "selected Inbox work has no source context",
            ));
            add(inbox_action(
                context,
                UiAction::MarkRead,
                "mark read",
                context.has_inbox_selection,
                "select Inbox work first",
            ));
            add(inbox_action(
                context,
                UiAction::MarkUnread,
                "mark unread",
                context.has_inbox_selection,
                "select Inbox work first",
            ));
            add(inbox_action(
                context,
                UiAction::MarkVisibleRead,
                "mark visible rows read",
                context.inbox_visible_count > 0,
                "no Inbox rows are visible",
            ));
        }
        (Route::Workspace, FocusSurface::Communities) => {
            add(ContextAction::enabled(
                UiAction::ActivateFocused,
                "open community",
            ));
            add(ContextAction::enabled(
                UiAction::OpenOptions,
                "workspace options",
            ));
        }
        (Route::Workspace, FocusSurface::Channels) => {
            add(ContextAction::enabled(
                UiAction::ActivateFocused,
                "open channel",
            ));
            add(channel_action(
                context,
                UiAction::Compose,
                "compose message",
            ));
            add(ContextAction::enabled(
                UiAction::ChannelSwitcher,
                "find channel or DM",
            ));
        }
        (Route::Workspace, FocusSurface::Timeline | FocusSurface::Context) => {
            add(channel_action(context, UiAction::Compose, "reply"));
            add(event_action(
                context,
                UiAction::ToggleThread,
                if context.context_open {
                    "close context"
                } else {
                    "open context"
                },
            ));
            add(event_publish_action(context, UiAction::React, "react"));
            add(own_event_action(
                context,
                UiAction::Delete,
                "delete own message",
            ));
            add(event_publish_action(
                context,
                UiAction::MarkUnread,
                "mark unread",
            ));
            add(media_action(context));
        }
        _ => {}
    }

    enabled.extend(unavailable);
    enabled
}

const fn inbox_action(
    context: ActionContext,
    action: UiAction,
    label: &'static str,
    available: bool,
    missing_reason: &'static str,
) -> ContextAction {
    if !context.has_inbox_selection {
        ContextAction::unavailable(action, label, "select Inbox work first")
    } else if !available {
        ContextAction::unavailable(action, label, missing_reason)
    } else if matches!(action, UiAction::Compose) && !context.can_publish {
        ContextAction::unavailable(action, label, "identity is locked or unavailable")
    } else {
        ContextAction::enabled(action, label)
    }
}

const fn channel_action(
    context: ActionContext,
    action: UiAction,
    label: &'static str,
) -> ContextAction {
    if !context.has_channel {
        ContextAction::unavailable(action, label, "select a channel first")
    } else if !context.can_publish {
        ContextAction::unavailable(action, label, "identity is locked or unavailable")
    } else {
        ContextAction::enabled(action, label)
    }
}

const fn event_action(
    context: ActionContext,
    action: UiAction,
    label: &'static str,
) -> ContextAction {
    if context.has_selected_event {
        ContextAction::enabled(action, label)
    } else {
        ContextAction::unavailable(action, label, "select a message first")
    }
}

const fn event_publish_action(
    context: ActionContext,
    action: UiAction,
    label: &'static str,
) -> ContextAction {
    if !context.has_selected_event {
        ContextAction::unavailable(action, label, "select a message first")
    } else if !context.can_publish {
        ContextAction::unavailable(action, label, "identity is locked or unavailable")
    } else {
        ContextAction::enabled(action, label)
    }
}

const fn own_event_action(
    context: ActionContext,
    action: UiAction,
    label: &'static str,
) -> ContextAction {
    if !context.has_selected_event {
        ContextAction::unavailable(action, label, "select a message first")
    } else if !context.selected_event_is_own {
        ContextAction::unavailable(action, label, "only your own message can be deleted")
    } else if !context.can_publish {
        ContextAction::unavailable(action, label, "identity is locked or unavailable")
    } else {
        ContextAction::enabled(action, label)
    }
}

const fn media_action(context: ActionContext) -> ContextAction {
    if !context.has_selected_event {
        ContextAction::unavailable(UiAction::Preview, "preview media", "select a message first")
    } else if !context.selected_event_has_media {
        ContextAction::unavailable(
            UiAction::Preview,
            "preview media",
            "selected message has no media",
        )
    } else {
        ContextAction::enabled(UiAction::Preview, "preview media")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionMenu {
    entries: Vec<ContextAction>,
    selected: usize,
}

impl ActionMenu {
    pub fn new(entries: Vec<ContextAction>) -> Self {
        Self {
            entries,
            selected: 0,
        }
    }

    pub fn entries(&self) -> &[ContextAction] {
        &self.entries
    }

    pub fn selected(&self) -> Option<ContextAction> {
        self.entries.get(self.selected).copied()
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len();
        let steps = delta.unsigned_abs() % len;
        self.selected = if delta.is_negative() {
            (self.selected + len - steps) % len
        } else {
            (self.selected + steps) % len
        };
    }

    pub fn select_action(&mut self, action: UiAction) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.action == action) else {
            return false;
        };
        self.selected = index;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::{
        keymap::UiAction,
        state::{FocusSurface, Route},
    };

    use super::{ActionContext, ActionMenu, derive};

    fn context() -> ActionContext {
        ActionContext {
            route: Route::Workspace,
            focus: FocusSurface::Timeline,
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
        }
    }

    #[test]
    fn enabled_actions_precede_unavailable_ones_with_reasons() {
        let actions = derive(context());
        let react = actions
            .iter()
            .position(|entry| entry.action == UiAction::React)
            .unwrap();
        let delete = actions
            .iter()
            .position(|entry| entry.action == UiAction::Delete)
            .unwrap();
        assert!(actions[react].enabled);
        assert!(!actions[delete].enabled);
        assert_eq!(
            actions[delete].reason,
            Some("only your own message can be deleted")
        );
        assert!(react < delete);
    }

    #[test]
    fn inbox_actions_keep_read_and_context_explicit() {
        let actions = derive(ActionContext {
            route: Route::Inbox,
            focus: FocusSurface::InboxList,
            has_inbox_selection: true,
            inbox_has_context: true,
            inbox_can_reply: true,
            inbox_visible_count: 2,
            has_channel: false,
            has_selected_event: false,
            selected_event_is_own: false,
            selected_event_has_media: false,
            context_open: false,
            can_publish: true,
        });
        assert!(
            actions
                .iter()
                .any(|entry| { entry.enabled && entry.action == UiAction::OpenCanonicalContext })
        );
        assert!(
            actions
                .iter()
                .any(|entry| entry.enabled && entry.action == UiAction::MarkRead)
        );
        assert!(
            actions
                .iter()
                .any(|entry| entry.enabled && entry.action == UiAction::MarkVisibleRead)
        );
    }

    #[test]
    fn menu_navigation_is_pure_and_wraps() {
        let mut menu = ActionMenu::new(derive(context()));
        let first = menu.selected().unwrap().action;
        menu.move_by(-1);
        assert_ne!(menu.selected().unwrap().action, first);
        assert!(menu.select_action(UiAction::Delete));
        assert_eq!(menu.selected().unwrap().action, UiAction::Delete);
    }
}
