use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    domain::{Message, Profile, Reaction},
    render::{markdown, sanitize},
    ui::theme::{BorderSurface, HighlightGroup, Theme},
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

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &std::collections::HashMap<String, Profile>,
    reactions: &std::collections::HashMap<String, Vec<Reaction>>,
    state: &TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
) {
    let items = messages.iter().map(|message| {
        let author = profiles
            .get(&message.pubkey)
            .map(Profile::label)
            .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&message.pubkey));
        let mut header = vec![
            Span::styled(
                sanitize::single_line(&author),
                theme.style(HighlightGroup::MessageAuthor),
            ),
            Span::styled(
                format!("  {}", format_time(message.created_at)),
                theme.style(HighlightGroup::MessageTimestamp),
            ),
        ];
        if message.deleted {
            header.push(Span::styled(
                " [deleted]",
                theme.style(HighlightGroup::MessageDeleted),
            ));
        } else if message.pending {
            header.push(Span::styled(
                " [pending]",
                theme.style(HighlightGroup::Pending),
            ));
        } else if message.rejected.is_some() {
            header.push(Span::styled(
                " [rejected]",
                theme.style(HighlightGroup::Rejected),
            ));
        }
        let mut lines = vec![Line::from(header)];
        if message.deleted {
            lines.push(Line::styled(
                "  message deleted",
                theme.style(HighlightGroup::MessageDeleted),
            ));
        } else {
            for line in markdown::render(&message.content, theme).lines {
                let mut spans = vec![Span::styled("  ", theme.style(HighlightGroup::MessageBody))];
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
            let mut aggregate = std::collections::BTreeMap::<&str, (usize, bool)>::new();
            for (emoji, author) in active {
                let value = aggregate.entry(emoji).or_default();
                value.0 += 1;
                value.1 |= self_pubkey == Some(author);
            }
            if !aggregate.is_empty() {
                let mut spans = vec![Span::raw("  ")];
                for (index, (emoji, (count, own))) in aggregate.into_iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::raw("  "));
                    }
                    spans.push(Span::styled(
                        format!("{emoji} {count}"),
                        theme.style(if own {
                            HighlightGroup::SelfReaction
                        } else {
                            HighlightGroup::Reaction
                        }),
                    ));
                }
                lines.push(Line::from(spans));
            }
        }
        ListItem::new(lines)
    });
    let mut list_state = ListState::default().with_selected(state.selected_index(messages));
    let border_group = if focused {
        HighlightGroup::FocusedPaneBorder
    } else {
        HighlightGroup::PaneBorder
    };
    frame.render_stateful_widget(
        List::new(items)
            .style(theme.style(HighlightGroup::Normal))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme.border_type(BorderSurface::Pane))
                    .border_style(theme.style(border_group))
                    .title_style(theme.style(HighlightGroup::PaneTitle))
                    .title(format!(" {} ", sanitize::single_line(title))),
            )
            .highlight_style(theme.style(HighlightGroup::SelectedRow))
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
