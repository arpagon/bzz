use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    domain::Channel,
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    channels: &[Channel],
    selected: usize,
    unread: &std::collections::HashSet<uuid::Uuid>,
    theme: &Theme,
    focused: bool,
) {
    let joined = channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.is_member)
        .collect::<Vec<_>>();
    let items = joined.iter().map(|(_, channel)| {
        let is_unread = unread.contains(&channel.id);
        let badge = if is_unread { "●" } else { " " };
        let privacy = if channel.kind.is_dm() {
            "@"
        } else if matches!(channel.visibility, crate::domain::Visibility::Private) {
            "🔒"
        } else {
            "#"
        };
        ListItem::new(Line::from(format!(
            "{badge} {privacy}{}",
            sanitize::single_line(&channel.name)
        )))
        .style(theme.style(if is_unread {
            HighlightGroup::ChannelUnread
        } else {
            HighlightGroup::SidebarText
        }))
    });
    let mut state =
        ListState::default().with_selected(joined.iter().position(|(index, _)| *index == selected));
    let border_group = if focused {
        HighlightGroup::FocusedPaneBorder
    } else {
        HighlightGroup::PaneBorder
    };
    frame.render_stateful_widget(
        List::new(items)
            .style(theme.style(HighlightGroup::Sidebar))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme.border_type(BorderSurface::Pane))
                    .border_style(theme.style(border_group))
                    .title_style(theme.style(HighlightGroup::PaneTitle))
                    .title(" channels "),
            )
            .highlight_style(theme.style(HighlightGroup::Selection))
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}
