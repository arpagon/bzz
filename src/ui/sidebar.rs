use std::cmp::Ordering;

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    config::ChannelSort,
    domain::Channel,
    render::sanitize,
    ui::{
        state::ViewportState,
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

/// Returns joined channel indexes in the one local presentation order used by
/// drawing, keyboard movement, viewport reconciliation, and mouse hit maps.
/// The underlying channel list remains store order and is never mutated.
pub fn ordered_indexes(
    channels: &[Channel],
    unread: &std::collections::HashSet<uuid::Uuid>,
    sort: ChannelSort,
) -> Vec<usize> {
    let mut ordered = channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.is_member)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        let left_channel = &channels[*left];
        let right_channel = &channels[*right];
        match sort {
            ChannelSort::Smart => unread
                .contains(&right_channel.id)
                .cmp(&unread.contains(&left_channel.id))
                .then_with(|| recent_then_label(left_channel, right_channel)),
            ChannelSort::Recent => recent_then_label(left_channel, right_channel),
            ChannelSort::Alphabetical => compare_label(left_channel, right_channel),
        }
    });
    ordered
}

fn recent_then_label(left: &Channel, right: &Channel) -> Ordering {
    right
        .last_event_at
        .cmp(&left.last_event_at)
        .then_with(|| compare_label(left, right))
}

fn compare_label(left: &Channel, right: &Channel) -> Ordering {
    sanitize::single_line(&left.name)
        .to_lowercase()
        .cmp(&sanitize::single_line(&right.name).to_lowercase())
        .then_with(|| left.id.cmp(&right.id))
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    channels: &[Channel],
    viewport: &ViewportState,
    unread: &std::collections::HashSet<uuid::Uuid>,
    sort: ChannelSort,
    theme: &Theme,
    focused: bool,
) {
    let ordered = ordered_indexes(channels, unread, sort);
    let items = ordered.iter().map(|index| {
        let channel = &channels[*index];
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
    let mut state = ListState::default()
        .with_selected(viewport.selected_id.as_ref().and_then(|selected| {
            ordered
                .iter()
                .position(|index| channels[*index].id.to_string() == *selected)
        }))
        .with_offset(viewport.scroll);
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
                    .title(format!(" channels · {} ", sort.label())),
            )
            .highlight_style(theme.style(HighlightGroup::Selection))
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use uuid::Uuid;

    use super::ordered_indexes;
    use crate::{
        config::ChannelSort,
        domain::{Channel, ChannelKind, Visibility},
    };

    fn channel(name: &str, last_event_at: Option<u64>) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            name: name.into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 0,
            last_event_at,
        }
    }

    #[test]
    fn every_sort_has_stable_local_tie_breaks() {
        let channels = vec![
            channel("zeta", Some(10)),
            channel("Alpha", Some(20)),
            channel("middle", None),
        ];
        let unread = HashSet::from([channels[0].id]);
        assert_eq!(
            ordered_indexes(&channels, &unread, ChannelSort::Smart),
            vec![0, 1, 2]
        );
        assert_eq!(
            ordered_indexes(&channels, &unread, ChannelSort::Recent),
            vec![1, 0, 2]
        );
        assert_eq!(
            ordered_indexes(&channels, &unread, ChannelSort::Alphabetical),
            vec![1, 2, 0]
        );
    }
}
