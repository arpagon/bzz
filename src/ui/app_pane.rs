//! The app pane's rules that need no app (v0.16 M3): who owns a key, where
//! a paste goes, and what the pane draws around an app. The app itself is
//! drawn by its session (`apps::session`); the session in bzz's loop is in
//! `app/pane.rs`.
//!
//! **One key is bzz's while an app has the focus: `Ctrl+C`,** which gives
//! the keyboard back to the conversation (M0, question 5). Every other key
//! is the app's. Overlays and the writing dock come before the app, and
//! `Ctrl+Y` before everything while an agent's command waits for the owner.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::ui::{
    layout::{APP_MIN_COLUMNS, APP_MIN_ROWS},
    state::Overlay,
    theme::{BorderSurface, HighlightGroup, Theme},
    typing::truncate_cells,
};

/// How the key bzz keeps is written in the pane's border.
pub const LEAVE_KEY_LABEL: &str = "Ctrl-c";

/// `Ctrl+C`, the one key bzz keeps while an app has the focus.
pub fn is_leave_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c' | 'C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

/// What decides who owns a key.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyContext {
    /// An agent's command waits for the owner's answer.
    pub approval_waiting: bool,
    /// The dialog that answers it is open.
    pub approval_open: bool,
    /// Any overlay is open: help, the command line, a panel, a question.
    pub overlay_open: bool,
    /// The writing dock is open.
    pub composer_open: bool,
    /// An app is drawn and has the focus.
    pub app_focused: bool,
}

/// Who a key goes to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyOwner {
    /// `Ctrl+Y` opens the command that waits for the owner (v0.15).
    Approval,
    /// The open overlay.
    Overlay,
    /// The writing dock.
    Composer,
    /// `Ctrl+C` with the app focused: the keyboard goes back to the
    /// conversation. The app never sees it.
    LeaveApp,
    /// The app.
    App,
    /// The workspace or the Inbox, as without an app.
    Workspace,
}

/// Who owns `key`, first match wins:
///
/// 1. `Ctrl+Y` while an agent's command waits and its dialog is closed;
/// 2. an open overlay;
/// 3. the writing dock, when open;
/// 4. the app, when its pane has the focus: `Ctrl+C` leaves it, every
///    other key is the app's;
/// 5. the workspace or the Inbox.
pub fn key_owner(key: KeyEvent, context: KeyContext) -> KeyOwner {
    let approval_key = key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::CONTROL;
    if approval_key && context.approval_waiting && !context.approval_open {
        KeyOwner::Approval
    } else if context.overlay_open {
        KeyOwner::Overlay
    } else if context.composer_open {
        KeyOwner::Composer
    } else if context.app_focused {
        if is_leave_key(key) {
            KeyOwner::LeaveApp
        } else {
            KeyOwner::App
        }
    } else {
        KeyOwner::Workspace
    }
}

/// What decides where a paste goes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PasteContext {
    pub overlay: Option<Overlay>,
    pub composer_open: bool,
    pub app_focused: bool,
}

/// Where a bracketed paste goes. It is always one piece of text: never
/// keys, so a line break in it sends nothing and runs nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasteTarget {
    /// The answer in focus of `:agent new`'s form.
    AgentForm,
    /// The reason of a refusal being written (it never answers).
    ApprovalReason,
    /// The writing dock.
    Composer,
    /// The app with the focus.
    App,
    /// Nowhere: another overlay is open, or nothing takes text.
    Dropped,
    /// The field of the join form being typed (v0.17).
    Join,
}

pub fn paste_target(context: PasteContext) -> PasteTarget {
    match context.overlay {
        Some(Overlay::AgentNew) => PasteTarget::AgentForm,
        Some(Overlay::Approval | Overlay::AppApproval) => PasteTarget::ApprovalReason,
        Some(Overlay::Join) => PasteTarget::Join,
        Some(_) => PasteTarget::Dropped,
        None if context.composer_open => PasteTarget::Composer,
        None if context.app_focused => PasteTarget::App,
        None => PasteTarget::Dropped,
    }
}

/// What an app is given of a paste: the composer's rule. Terminals send a
/// pasted line break as CR; CRLF and a lone CR become LF, a tab four
/// spaces, and control and direction characters are dropped, so a paste
/// cannot write an escape sequence through an app's screen. None when the
/// paste is too large.
pub fn app_paste_text(text: &str) -> Option<String> {
    crate::media::clipboard::sanitize_pasted_text(text)
        .ok()
        .filter(|text| !text.is_empty())
}

/// What the pane says around an app.
#[derive(Clone, Copy, Debug)]
pub struct PaneView<'a> {
    /// The app's title.
    pub title: &'a str,
    pub focused: bool,
    pub zoomed: bool,
    /// The session's own sentence: the wait at the quota, too many records,
    /// an archived channel, records that cannot be read. It goes in the
    /// bottom border, where it costs the app no row.
    pub notice: Option<&'a str>,
}

/// Draws the pane's border and titles, and answers the region inside it.
pub fn render_frame(frame: &mut Frame<'_>, area: Rect, view: PaneView<'_>, theme: &Theme) -> Rect {
    frame.render_widget(Clear, area);
    let width = usize::from(area.width.saturating_sub(4));
    let title = match (view.focused, view.zoomed) {
        (true, true) => format!(" {} · zoomed · {LEAVE_KEY_LABEL} leave ", view.title),
        (true, false) => format!(" {} · {LEAVE_KEY_LABEL} leave ", view.title),
        (false, _) => format!(" {} · 5 focus ", view.title),
    };
    let mut block = Block::bordered()
        .border_type(theme.border_type(BorderSurface::Pane))
        .border_style(theme.style(if view.focused {
            HighlightGroup::FocusedPaneBorder
        } else {
            HighlightGroup::PaneBorder
        }))
        .title_style(theme.style(HighlightGroup::PaneTitle))
        .title(truncate_cells(&title, width));
    if let Some(notice) = view.notice {
        block = block.title_bottom(
            Line::styled(
                truncate_cells(&format!(" {notice} "), width),
                theme.style(HighlightGroup::Warning),
            )
            .alignment(Alignment::Left),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block.style(theme.style(HighlightGroup::Normal)), area);
    inner
}

/// The sentence bzz shows in place of an app whose region is too small:
/// the script's `view` is never called under 40×16.
pub fn too_small_text(region: Rect, zoomed: bool) -> String {
    let way = if zoomed {
        "Make the window larger."
    } else {
        "Make the window larger, or zoom the app: Space 5, or :app zoom."
    };
    format!(
        "This app needs {APP_MIN_COLUMNS}×{APP_MIN_ROWS} and has {}×{} here. {way}",
        region.width, region.height
    )
}

/// Words in the place of an app: why it is too small, why it does not run.
pub fn render_words(frame: &mut Frame<'_>, region: Rect, text: &str, error: bool, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(text.to_owned())
            .style(theme.style(if error {
                HighlightGroup::Error
            } else {
                HighlightGroup::Muted
            }))
            .wrap(Wrap { trim: true }),
        region,
    );
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    use super::{
        KeyContext, KeyOwner, PaneView, PasteContext, PasteTarget, app_paste_text, is_leave_key,
        key_owner, paste_target, render_frame, too_small_text,
    };
    use crate::ui::{state::Overlay, theme::Theme};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(character: char) -> KeyEvent {
        key(KeyCode::Char(character), KeyModifiers::NONE)
    }

    /// Every key the trial sent to an app, and the keys bzz itself uses.
    fn keys() -> Vec<KeyEvent> {
        let mut keys: Vec<KeyEvent> = "q?:/ijkhl12345[]gGyvre aAZ0".chars().map(plain).collect();
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::F(1),
            KeyCode::F(6),
        ] {
            keys.push(key(code, KeyModifiers::NONE));
        }
        for character in ['u', 'y', 'n', 'd', 'g', 'v', 'o', 'j'] {
            keys.push(key(KeyCode::Char(character), KeyModifiers::CONTROL));
        }
        keys.push(key(KeyCode::Char('x'), KeyModifiers::ALT));
        keys.push(key(KeyCode::Char('c'), KeyModifiers::ALT));
        keys
    }

    #[test]
    fn with_the_app_focused_every_key_but_ctrl_c_is_the_apps() {
        let context = KeyContext {
            app_focused: true,
            ..KeyContext::default()
        };
        for key in keys() {
            assert_eq!(key_owner(key, context), KeyOwner::App, "{key:?}");
        }
        let leave = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_owner(leave, context), KeyOwner::LeaveApp);
        // Some terminals report Ctrl+Shift+C as an upper-case C.
        let shifted = key(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(key_owner(shifted, context), KeyOwner::LeaveApp);
        assert!(!is_leave_key(key(KeyCode::Char('c'), KeyModifiers::ALT)));
    }

    #[test]
    fn overlays_and_the_composer_come_before_the_app() {
        for key in keys()
            .into_iter()
            .chain([key(KeyCode::Char('c'), KeyModifiers::CONTROL)])
        {
            let overlay = KeyContext {
                overlay_open: true,
                composer_open: true,
                app_focused: true,
                ..KeyContext::default()
            };
            assert_eq!(key_owner(key, overlay), KeyOwner::Overlay, "{key:?}");
            let composer = KeyContext {
                composer_open: true,
                app_focused: true,
                ..KeyContext::default()
            };
            assert_eq!(key_owner(key, composer), KeyOwner::Composer, "{key:?}");
            let workspace = KeyContext::default();
            assert_eq!(key_owner(key, workspace), KeyOwner::Workspace, "{key:?}");
        }
    }

    #[test]
    fn ctrl_y_opens_a_waiting_command_before_everything_and_is_the_apps_otherwise() {
        let ctrl_y = key(KeyCode::Char('y'), KeyModifiers::CONTROL);
        let waiting = KeyContext {
            approval_waiting: true,
            app_focused: true,
            overlay_open: true,
            composer_open: true,
            ..KeyContext::default()
        };
        assert_eq!(key_owner(ctrl_y, waiting), KeyOwner::Approval);
        let nothing_waits = KeyContext {
            app_focused: true,
            ..KeyContext::default()
        };
        assert_eq!(key_owner(ctrl_y, nothing_waits), KeyOwner::App);
        let dialog_open = KeyContext {
            approval_waiting: true,
            approval_open: true,
            overlay_open: true,
            app_focused: true,
            ..KeyContext::default()
        };
        assert_eq!(key_owner(ctrl_y, dialog_open), KeyOwner::Overlay);
    }

    #[test]
    fn a_paste_goes_to_the_app_only_when_nothing_else_takes_text() {
        let app = PasteContext {
            overlay: None,
            composer_open: false,
            app_focused: true,
        };
        assert_eq!(paste_target(app), PasteTarget::App);
        assert_eq!(
            paste_target(PasteContext {
                composer_open: true,
                ..app
            }),
            PasteTarget::Composer
        );
        assert_eq!(
            paste_target(PasteContext {
                overlay: Some(Overlay::Command),
                ..app
            }),
            PasteTarget::Dropped
        );
        assert_eq!(
            paste_target(PasteContext {
                overlay: Some(Overlay::Help),
                composer_open: true,
                app_focused: true,
            }),
            PasteTarget::Dropped
        );
        assert_eq!(
            paste_target(PasteContext {
                overlay: Some(Overlay::AgentNew),
                ..app
            }),
            PasteTarget::AgentForm
        );
        assert_eq!(
            paste_target(PasteContext {
                overlay: Some(Overlay::Approval),
                ..app
            }),
            PasteTarget::ApprovalReason
        );
        assert_eq!(
            paste_target(PasteContext {
                app_focused: false,
                ..app
            }),
            PasteTarget::Dropped
        );
    }

    #[test]
    fn a_pasted_line_break_reaches_an_app_as_lf() {
        assert_eq!(
            app_paste_text("a\rb\r\nc\nd\r").as_deref(),
            Some("a\nb\nc\nd\n")
        );
        assert_eq!(app_paste_text("x\u{1b}[2Jy").as_deref(), Some("x[2Jy"));
        assert_eq!(app_paste_text(""), None);
    }

    fn drawn(view: PaneView<'_>, width: u16, height: u16) -> (String, Rect) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut inner = Rect::default();
        terminal
            .draw(|frame| inner = render_frame(frame, frame.area(), view, &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, inner)
    }

    #[test]
    fn the_border_says_how_to_leave_and_the_notice_costs_no_row() {
        let view = PaneView {
            title: "Board",
            focused: true,
            zoomed: false,
            notice: Some("Too many changes: this app can write again in 23 s."),
        };
        let (text, inner) = drawn(view, 42, 20);
        let first = text.lines().next().unwrap();
        let last = text.lines().last().unwrap();
        assert!(first.contains("Board · Ctrl-c leave"), "{first}");
        assert!(last.contains("Too many changes"), "{last}");
        assert_eq!(inner, Rect::new(1, 1, 40, 18));
        let (text, _) = drawn(
            PaneView {
                focused: false,
                notice: None,
                ..view
            },
            42,
            20,
        );
        assert!(text.lines().next().unwrap().contains("Board · 5 focus"));
        assert!(!text.contains("Ctrl-c"));
    }

    #[test]
    fn under_forty_by_sixteen_bzz_says_so() {
        let text = too_small_text(Rect::new(0, 0, 40, 10), false);
        assert!(text.contains("40×16") && text.contains("40×10") && text.contains("zoom"));
        assert!(!too_small_text(Rect::new(0, 0, 58, 13), true).contains("zoom"));
    }
}
