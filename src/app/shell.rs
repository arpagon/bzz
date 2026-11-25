//! The workspace's shell in bzz's loop (v0.17 L10 to L12, task `layout`):
//! what the layout is asked (`ui::layout::workspace`), the conversation-only
//! mode on `z`, what the frame says of a hidden pane, `Esc` with nothing to
//! close, and the mouse gestures the hit map does not answer alone: the
//! wheel over a pane that does not have the focus, a border dragged, the
//! status line's hint clicked.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Frame, layout::Rect};

use super::App;
use crate::{
    error::Result,
    ui::{
        hit_map::{HitMap, HitTarget},
        layout::{self, Panes, SidePane},
        shell::Pane,
        state::FocusSurface,
    },
};

/// What the layout keeps between frames.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ShellState {
    /// `z`: the conversation and the writing dock alone.
    pub(super) conversation_only: bool,
    /// The border being dragged, if one is.
    pub(super) drag: Option<Pane>,
    /// The panes of the last frame drawn: a drag measures against them.
    pub(super) last_panes: Option<Panes>,
}

impl App {
    /// What the layout is asked this frame.
    pub(super) fn workspace_shell(&self) -> layout::Shell {
        let focus = match self.presentation.focus {
            FocusSurface::Communities => SidePane::Communities,
            FocusSurface::Channels => SidePane::Channels,
            FocusSurface::Context => SidePane::Thread,
            FocusSurface::App if self.app_has_focus() => SidePane::App,
            _ => SidePane::None,
        };
        layout::Shell {
            community: self.community_rail,
            communities: self.config.communities.len(),
            sidebar: self.sidebar,
            thread: self.thread_root.is_some(),
            sidebar_width: self.config.ui.sidebar_width,
            thread_width: self.config.ui.thread_width,
            composer_height: self.composer_dock_height(),
            app: self.app_request(),
            focus,
            conversation_only: self.shell.conversation_only,
        }
    }

    /// The panes of the workspace for a frame of `area`.
    pub(super) fn workspace_panes(&self, area: Rect) -> Panes {
        layout::workspace(area, self.workspace_shell())
    }

    /// `Esc` in the workspace or the Inbox with nothing to close (L12).
    pub(super) fn say_nothing_to_close(&mut self) {
        self.status_error = Some("nothing to close · q quits".into());
    }

    /// The borders a drag resizes, and the status line's hint; pushed after
    /// the panes so they win over the pane under them.
    pub(super) fn push_shell_targets(
        &self,
        frame: &mut Frame<'_>,
        panes: &Panes,
        map: &mut HitMap,
    ) {
        for (pane, area) in crate::ui::shell::splitters(panes) {
            map.push(area, HitTarget::Splitter(pane));
        }
        if let Some(area) = crate::ui::shell::find_hint(frame.buffer_mut(), panes.status) {
            map.push(area, HitTarget::StatusHint);
        }
    }

    /// The conversation's frame (or the frame in its place) names what is
    /// hidden and the key that shows it.
    pub(super) fn draw_hidden_panes(&self, frame: &mut Frame<'_>, panes: &Panes, title: &str) {
        let keys = panes.hidden.keys();
        if keys.is_empty() {
            return;
        }
        let (pane, title_width) = if panes.timeline.width > 0 {
            (panes.timeline, crate::ui::shell::text_width(title))
        } else if let Some(pane) = panes.thread.or(panes.sidebar).or(panes.community) {
            // "context · 12 replies", "channels · smart": room for a title.
            (pane, 22)
        } else {
            return;
        };
        crate::ui::shell::draw_hidden(frame.buffer_mut(), pane, title_width, &keys, &self.theme);
    }

    /// A border pressed, dragged and let go. Answers whether the event was
    /// a drag's.
    pub(super) fn drag_border(
        &mut self,
        mouse: MouseEvent,
        target: Option<&HitTarget>,
    ) -> Result<bool> {
        match (mouse.kind, self.shell.drag) {
            (MouseEventKind::Down(MouseButton::Left), _) => {
                if let Some(HitTarget::Splitter(pane)) = target {
                    self.shell.drag = Some(*pane);
                    return Ok(true);
                }
                self.shell.drag = None;
                Ok(false)
            }
            (MouseEventKind::Drag(MouseButton::Left), Some(pane)) => {
                if let Some(panes) = self.shell.last_panes
                    && let Some(width) = crate::ui::shell::dragged_width(&panes, pane, mouse.column)
                {
                    match pane {
                        Pane::Channels => self.config.ui.sidebar_width = width,
                        Pane::Thread => self.config.ui.thread_width = width,
                        Pane::Communities | Pane::Timeline => {}
                    }
                }
                Ok(true)
            }
            (MouseEventKind::Up(MouseButton::Left), Some(pane)) => {
                self.shell.drag = None;
                let (label, width) = match pane {
                    Pane::Channels => ("channel pane", self.config.ui.sidebar_width),
                    _ => ("context pane", self.config.ui.thread_width),
                };
                self.config.save(&self.paths)?;
                self.status_error = Some(format!("{label} width saved as {width}"));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// The wheel scrolls the pane under the pointer and leaves the focus
    /// where it is, while writing too (L11).
    pub(super) fn wheel_without_focus(&mut self, target: HitTarget, delta: isize) {
        let focus = self.presentation.focus;
        if matches!(target, HitTarget::Composer | HitTarget::MentionCandidate(_)) {
            return;
        }
        self.wheel_over(target, delta);
        self.presentation.focus = focus;
    }

    /// A double click on a message opens its thread: the clicked one, even
    /// if another thread is open, and it stays open if it is that one.
    pub(super) async fn open_thread_of_selected(&mut self) -> Result<()> {
        if self.thread_of_selected_is_open() {
            self.presentation.set_workspace_focus(FocusSurface::Context);
            return Ok(());
        }
        self.thread_root = None;
        self.toggle_thread().await
    }

    /// The selected message's thread is the one open (`toggle_thread`
    /// finds the root the same way).
    pub(super) fn thread_of_selected_is_open(&self) -> bool {
        let root = self.selected_message().map(|message| {
            message
                .sent_from_thread
                .as_ref()
                .map(|reference| reference.root_event_id.clone())
                .or_else(|| message.root_event_id.clone())
                .unwrap_or_else(|| message.event_id.clone())
        });
        root.is_some() && root == self.thread_root
    }

    /// The status line's hint was clicked: the palette.
    pub(super) async fn click_status_hint(&mut self) {
        if self.presentation.overlay.is_none() && self.presentation.composer_target.is_none() {
            self.open_palette().await;
        }
    }
}

#[cfg(test)]
mod tests;
