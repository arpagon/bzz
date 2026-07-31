use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    domain::{Message, Profile, Reaction},
    render::{markdown, sanitize},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TimelineState {
    pub selected_event: Option<String>,
    pub at_live_bottom: bool,
    pub newer: usize,
}
impl TimelineState {
    pub fn reconcile(&mut self, messages: &[Message]) {
        if self.at_live_bottom {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
            self.newer = 0;
        } else if let Some(selected) = &self.selected_event
            && !messages.iter().any(|message| &message.event_id == selected)
        {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
        }
    }
    pub fn selected_index(&self, messages: &[Message]) -> Option<usize> {
        self.selected_event
            .as_ref()
            .and_then(|id| messages.iter().position(|message| &message.event_id == id))
    }
    pub fn move_by(&mut self, messages: &[Message], delta: isize) {
        if messages.is_empty() {
            return;
        }
        let current = self.selected_index(messages).unwrap_or(messages.len() - 1);
        let next = current.saturating_add_signed(delta).min(messages.len() - 1);
        self.selected_event = Some(messages[next].event_id.clone());
        self.at_live_bottom = next == messages.len() - 1;
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &std::collections::HashMap<String, Profile>,
    reactions: &std::collections::HashMap<String, Vec<Reaction>>,
    state: &TimelineState,
    title: &str,
) {
    let items = messages.iter().map(|message| {
        let author = profiles
            .get(&message.pubkey)
            .map(Profile::label)
            .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&message.pubkey));
        let marker = if message.deleted {
            "[deleted]"
        } else if message.pending {
            "[pending]"
        } else if message.rejected.is_some() {
            "[rejected]"
        } else {
            ""
        };
        let mut lines = vec![Line::from(vec![
            Span::styled(
                sanitize::single_line(&author),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {} {marker}", format_time(message.created_at))),
        ])];
        if message.deleted {
            lines.push(Line::styled(
                "  message deleted",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            for line in markdown::render(&message.content).lines {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
            let active = reactions
                .get(&message.event_id)
                .into_iter()
                .flatten()
                .filter(|reaction| !reaction.deleted)
                .map(|reaction| (reaction.emoji.as_str(), reaction.pubkey.as_str()))
                .collect::<std::collections::BTreeSet<_>>();
            let mut aggregate = std::collections::BTreeMap::<&str, usize>::new();
            for (emoji, _) in active {
                *aggregate.entry(emoji).or_default() += 1;
            }
            if !aggregate.is_empty() {
                lines.push(Line::styled(
                    format!(
                        "  {}",
                        aggregate
                            .into_iter()
                            .map(|(emoji, count)| format!("{emoji} {count}"))
                            .collect::<Vec<_>>()
                            .join("  ")
                    ),
                    Style::default().fg(Color::Magenta),
                ));
            }
        }
        ListItem::new(lines)
    });
    let mut list_state = ListState::default().with_selected(state.selected_index(messages));
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", sanitize::single_line(title))),
            )
            .highlight_style(Style::default().bg(Color::Rgb(38, 38, 48)))
            .highlight_symbol("▌"),
        area,
        &mut list_state,
    );
}

fn format_time(timestamp: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(timestamp).unwrap_or(0))
        .ok()
        .map(|value| {
            let local = value
                .to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
            format!("{:02}:{:02}", local.hour(), local.minute())
        })
        .unwrap_or_else(|| "--:--".into())
}
