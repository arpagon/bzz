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
    /// `bzz demo` (v0.17): the huddle is a generated recording to look at;
    /// there is nothing to join, and no device is opened.
    pub recording: bool,
}

pub fn render(frame: &mut Frame<'_>, area: Rect, view: &HuddleView, theme: &Theme) {
    render_with_lanes(frame, area, view, theme, None);
}

/// The panel, with the lanes of the call above the text while you are in it.
pub fn render_with_lanes(
    frame: &mut Frame<'_>,
    area: Rect,
    view: &HuddleView,
    theme: &Theme,
    lanes: Option<&crate::ui::huddle_lanes::Lanes<'_>>,
) {
    frame.render_widget(Clear, area);
    let keys = match (&view.roster, view.call) {
        _ if view.recording => "a generated recording · Esc",
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
            // In the call, the lanes name everyone.
            let listed = if lanes.is_some() { 0 } else { 24 };
            for name in roster.iter().take(listed) {
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
            _ if view.recording => Span::styled(
                "the voices are generated: a recording to look at, with nothing to join",
                muted,
            ),
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
    let paragraph = Paragraph::new(lines)
        .style(theme.style(HighlightGroup::Normal))
        .wrap(Wrap { trim: true });
    let inner = block.inner(area);
    // The lanes go above the text, which keeps at least two rows.
    let lanes_height = lanes.map_or(0, |lanes| {
        crate::ui::huddle_lanes::height_for(lanes.people.len()).min(inner.height.saturating_sub(2))
    });
    match lanes {
        Some(lanes) if lanes_height >= 3 => {
            frame.render_widget(
                Block::default().style(theme.style(HighlightGroup::Normal)),
                area,
            );
            frame.render_widget(block, area);
            let lanes_area = Rect {
                height: lanes_height,
                ..inner
            };
            crate::ui::huddle_lanes::render(frame.buffer_mut(), lanes_area, lanes, theme);
            let text_area = Rect {
                y: inner.y + lanes_height,
                height: inner.height - lanes_height,
                ..inner
            };
            frame.render_widget(paragraph, text_area);
        }
        _ => frame.render_widget(paragraph.block(block), area),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        config::Motion,
        huddle::{meter::Meter, synth},
        ui::huddle_lanes::{Lanes, Look, Person},
    };

    fn screen(width: u16, height: u16, lanes: Option<&Lanes<'_>>) -> String {
        let theme = Theme::builtin("bzz").unwrap();
        let view = HuddleView {
            channel: "team".into(),
            roster: Some(vec!["Ada".into(), "Lin".into(), "Sam".into(), "You".into()]),
            started_minutes_ago: 4,
            call: Some(true),
            audio_available: true,
            ..HuddleView::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_with_lanes(frame, frame.area(), &view, &theme, lanes))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// In a call, the panel shows the lanes above its text, at 80×24 (the
    /// modal is 80×22 there) and at 42×12.
    #[test]
    fn the_panel_shows_the_lanes_above_its_text() {
        let mut source = synth::Synthetic::new(&synth::DEMO_SCRIPT, None);
        let levels = source.levels();
        let mut meter = Meter::default();
        let mut t = 0.0;
        while t <= 5.5 {
            source.advance_to(t);
            meter.update(levels.as_ref(), &[1, 2, 3], t);
            t += 1.0 / 30.0;
        }
        let people = ["Ada", "Lin", "Sam"]
            .iter()
            .zip(1..)
            .map(|(name, index)| Person {
                index,
                name: (*name).into(),
            })
            .collect::<Vec<_>>();
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: Look::default(),
            recording: false,
        };
        let wide = screen(80, 22, Some(&lanes));
        assert!(wide.contains("Ada and Lin at once"), "{wide}");
        assert!(wide.contains("You · your microphone"), "{wide}");
        assert!(wide.contains("you are in the call · muted"), "{wide}");
        let small = screen(42, 12, Some(&lanes));
        assert!(small.contains("Ada and Lin at once"), "{small}");
        assert!(small.contains("muted"), "{small}");
        // Without a call the panel is as it was.
        let plain = screen(80, 22, None);
        assert!(plain.contains("active for 4 min · 4 in it"), "{plain}");
        assert!(!plain.contains("at once"), "{plain}");
        println!("{wide}\n\n{small}");
    }
}
