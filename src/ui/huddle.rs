//! `:huddle` — the selected channel's huddle: roster, guidelines, reactions
//! and your call. Device names show only here, never in logs.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::ui::theme::{BorderSurface, HighlightGroup, Theme};

/// Everything the panel shows, already resolved to display text.
#[derive(Clone, Debug, Default)]
pub struct HuddleView {
    pub channel: String,
    /// `None` when the channel has no active huddle.
    pub roster: Option<Vec<String>>,
    pub started_minutes_ago: u64,
    pub guidelines: Option<String>,
    pub reactions: Vec<String>,
    /// `Some(muted)` while you are in the call.
    pub call: Option<bool>,
    pub joining: bool,
    pub audio_available: bool,
    pub devices: Option<(Vec<String>, Vec<String>)>,
    pub you_created: bool,
}

pub fn render(frame: &mut Frame<'_>, area: Rect, view: &HuddleView, theme: &Theme) {
    frame.render_widget(Clear, area);
    let keys = match (&view.roster, view.call) {
        (None, _) => "s start · Esc",
        (Some(_), None) => "j join (muted) · Esc",
        (Some(_), Some(_)) => "m mute/unmute · space push-to-talk · l leave · Esc",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(format!(
            " huddle in #{} · {keys}{} ",
            crate::protocol::artifact::display(&view.channel, 40),
            if view.you_created && view.roster.is_some() {
                " · e end for everyone"
            } else {
                ""
            }
        ));
    let muted = theme.style(HighlightGroup::Muted);
    let mut lines = Vec::new();
    match &view.roster {
        None => lines.push(Line::from(
            "No huddle is running here. Press s to start one.",
        )),
        Some(roster) => {
            lines.push(Line::from(format!(
                "active for {} min · {} in it",
                view.started_minutes_ago,
                roster.len()
            )));
            for name in roster.iter().take(24) {
                lines.push(Line::from(format!(
                    "  • {}",
                    crate::protocol::artifact::display(name, 48)
                )));
            }
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(
        match (view.call, view.joining, view.audio_available) {
            (_, _, false) => Span::styled(crate::huddle::VIEW_ONLY, muted),
            (_, true, _) => Span::raw("joining…"),
            (Some(true), _, _) => Span::raw("you are in the call · muted"),
            (Some(false), _, _) => Span::raw("you are in the call · LIVE (others hear you)"),
            (None, _, _) => Span::styled("you are not in the call", muted),
        },
    ));
    if let Some(guidelines) = &view.guidelines {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("guidelines", muted)));
        lines.push(Line::from(crate::protocol::artifact::display(
            guidelines, 400,
        )));
    }
    if !view.reactions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "reactions: {}",
            view.reactions.join(" ")
        )));
    }
    if let Some((inputs, outputs)) = &view.devices {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "devices (:huddle input <name> / :huddle output <name>)",
            muted,
        )));
        for name in inputs.iter().take(8) {
            lines.push(Line::from(format!(
                "  mic: {}",
                crate::protocol::artifact::display(name, 60)
            )));
        }
        for name in outputs.iter().take(8) {
            lines.push(Line::from(format!(
                "  speaker: {}",
                crate::protocol::artifact::display(name, 60)
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true })
            .block(block),
        area,
    );
}
