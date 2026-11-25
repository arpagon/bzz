//! The status line's hint follows the focus (v0.17 C10): which context the
//! focus is in, and which of the registry's hint rows can run there. The
//! keys and their order are in `ui::status::footer` and the rows'
//! `hints` in `ui::action_table`.

use super::App;
use crate::ui::{
    action_table::{ActionSpec, FooterContext},
    palette::{self, Status},
    state::{FocusSurface, Overlay, Route},
    status::Slot,
};

impl App {
    /// Where the focus is, for the hint; `None` while an app has the
    /// keyboard (the status line then says how to leave it).
    pub(super) fn footer_context(&self) -> Option<FooterContext> {
        match self.presentation.overlay {
            Some(Overlay::Huddle) => return Some(FooterContext::Huddle),
            // The help, the leader panel and the actions menu are read
            // over the workspace: the hint stays the workspace's.
            Some(Overlay::Help | Overlay::WhichKey | Overlay::Actions) | None => {}
            Some(_) => return Some(FooterContext::Overlay),
        }
        if self.presentation.composer_target.is_some() {
            return Some(FooterContext::Composer);
        }
        if self.presentation.route == Route::Inbox {
            return Some(FooterContext::Inbox);
        }
        Some(match self.presentation.focus {
            FocusSurface::App => return None,
            FocusSurface::InboxList | FocusSurface::InboxDetail => FooterContext::Inbox,
            FocusSurface::Communities | FocusSurface::Channels => FooterContext::Channels,
            FocusSurface::Context => FooterContext::Thread,
            FocusSurface::Timeline if self.selected_message().is_some() => FooterContext::Message,
            FocusSurface::Timeline => FooterContext::Timeline,
        })
    }

    /// The hint for the status line, from the registry and the live key
    /// map. A row is offered only where the palette would run it.
    pub(super) fn footer_slot(&self) -> Option<Slot<'static>> {
        let context = self.footer_context()?;
        let facts = self.palette_facts();
        let huddle_running = self.active_huddle().is_some();
        let in_call = self.huddle_call_state().is_some();
        let can_run = |spec: &ActionSpec| match context {
            // The huddle panel's own keys: start or join while out of the
            // call, mute and leave while in it.
            FooterContext::Huddle => match spec.id {
                "huddle.start" => !huddle_running,
                "huddle.join" => huddle_running && !in_call,
                "huddle.mute" | "huddle.leave" => in_call,
                _ => true,
            },
            // Writing and overlays: their keys always work.
            FooterContext::Composer | FooterContext::Overlay => true,
            _ => palette::status(spec, &facts) == Status::Ready,
        };
        Some(crate::ui::status::footer(
            &self.theme,
            &self.keymap,
            context,
            can_run,
        ))
    }
}
