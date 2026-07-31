use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{domain::Channel, render::sanitize};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    channels: &[Channel],
    selected: usize,
    unread: &std::collections::HashSet<uuid::Uuid>,
) {
    let joined = channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.is_member)
        .collect::<Vec<_>>();
    let items = joined.iter().map(|(_, channel)| {
        let badge = if unread.contains(&channel.id) {
            "●"
        } else {
            " "
        };
        let privacy = if matches!(channel.visibility, crate::domain::Visibility::Private) {
            "🔒"
        } else {
            "#"
        };
        ListItem::new(Line::from(format!(
            "{badge} {privacy}{}",
            sanitize::single_line(&channel.name)
        )))
    });
    let mut state =
        ListState::default().with_selected(joined.iter().position(|(index, _)| *index == selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" channels "))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}
