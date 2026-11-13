use std::{cmp::Ordering, collections::HashMap};

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    config::ChannelSort,
    domain::{Channel, SidebarLayout},
    render::sanitize,
    store::agents::RemoteAgentView,
    ui::{
        state::ViewportState,
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

/// One sidebar row: a group heading or a joined channel (by store index).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarRow {
    Header(String),
    Channel(usize),
}

/// The Buzz group a channel falls in, in display order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Group {
    Solo,
    Section(usize),
    Channels,
    Forums,
    Dms,
    System,
}

const SOLO_LABEL: &str = "buzz.solo";

fn group_of(channel: &Channel, layout: &SidebarLayout) -> Group {
    if channel.kind.is_system() {
        return Group::System;
    }
    if let Some(section) = layout.assignments.get(&channel.id)
        && let Some(index) = layout.sections.iter().position(|live| live.id == *section)
    {
        return Group::Section(index);
    }
    if channel.facts.labels.iter().any(|label| label == SOLO_LABEL) {
        Group::Solo
    } else if channel.kind.is_dm() {
        Group::Dms
    } else if matches!(channel.kind, crate::domain::ChannelKind::Forum) {
        Group::Forums
    } else {
        Group::Channels
    }
}

/// The synced sort key of a group (`channels`, `forums`, `dms`,
/// `section:<id>`); Solo and System have none.
fn sort_key(group: Group, layout: &SidebarLayout) -> Option<String> {
    match group {
        Group::Section(index) => layout
            .sections
            .get(index)
            .map(|section| format!("{}{}", crate::protocol::sidebar::SECTION_PREFIX, section.id)),
        Group::Channels => Some("channels".into()),
        Group::Forums => Some("forums".into()),
        Group::Dms => Some("dms".into()),
        Group::Solo | Group::System => None,
    }
}

/// The synced sort key of the group a channel is in, if it has one.
pub fn channel_sort_key(channel: &Channel, layout: &SidebarLayout) -> Option<String> {
    sort_key(group_of(channel, layout), layout)
}

/// The synced section a channel is in, if any.
pub fn channel_section<'a>(
    channel: &Channel,
    layout: &'a SidebarLayout,
) -> Option<&'a crate::domain::SidebarSection> {
    match group_of(channel, layout) {
        Group::Section(index) => layout.sections.get(index),
        _ => None,
    }
}

fn heading(group: Group, layout: &SidebarLayout) -> String {
    match group {
        Group::Solo => "Solo".into(),
        Group::Section(index) => layout
            .sections
            .get(index)
            .map_or_else(String::new, |section| match &section.icon {
                Some(icon) => format!(
                    "{} {}",
                    sanitize::single_line(icon),
                    sanitize::single_line(&section.name)
                ),
                None => sanitize::single_line(&section.name),
            }),
        Group::Channels => "Channels".into(),
        Group::Forums => "Forums".into(),
        Group::Dms => "Direct messages".into(),
        Group::System => "System".into(),
    }
}

/// Joined channels grouped as Buzz shows them: Solo chats, the user's synced
/// sections in their order, channels, forums, direct messages, then System
/// (collapsed unless `show_system`, as `:system`). A group's synced sort
/// mode (`alpha`/`recent`) wins over the local one. Headings appear only
/// when more than one group has channels.
pub fn rows(
    channels: &[Channel],
    unread: &std::collections::HashSet<uuid::Uuid>,
    sort: ChannelSort,
    show_system: bool,
    layout: &SidebarLayout,
) -> Vec<SidebarRow> {
    let mut grouped = std::collections::BTreeMap::<Group, Vec<usize>>::new();
    for (index, channel) in channels.iter().enumerate() {
        if channel.is_member && (show_system || !channel.kind.is_system()) {
            grouped
                .entry(group_of(channel, layout))
                .or_default()
                .push(index);
        }
    }
    let headings = grouped.len() > 1;
    let mut rows = Vec::new();
    for (group, mut members) in grouped {
        let mode = sort_key(group, layout)
            .and_then(|key| layout.sort.get(&key))
            .map(|mode| match mode.as_str() {
                "alpha" => ChannelSort::Alphabetical,
                _ => ChannelSort::Recent,
            })
            .unwrap_or(sort);
        members.sort_by(|left, right| {
            let (left, right) = (&channels[*left], &channels[*right]);
            match mode {
                ChannelSort::Smart => unread
                    .contains(&right.id)
                    .cmp(&unread.contains(&left.id))
                    .then_with(|| recent_then_label(left, right)),
                ChannelSort::Recent => recent_then_label(left, right),
                ChannelSort::Alphabetical => compare_label(left, right),
            }
        });
        if headings {
            rows.push(SidebarRow::Header(heading(group, layout)));
        }
        rows.extend(members.into_iter().map(SidebarRow::Channel));
    }
    rows
}

/// Joined channel indexes in the one presentation order used by drawing,
/// keyboard movement, viewport reconciliation and mouse hit maps (the
/// channel rows of [`rows`]). The channel list itself stays in store order.
pub fn ordered_indexes(
    channels: &[Channel],
    unread: &std::collections::HashSet<uuid::Uuid>,
    sort: ChannelSort,
    show_system: bool,
    layout: &SidebarLayout,
) -> Vec<usize> {
    rows(channels, unread, sort, show_system, layout)
        .into_iter()
        .filter_map(|row| match row {
            SidebarRow::Channel(index) => Some(index),
            SidebarRow::Header(_) => None,
        })
        .collect()
}

/// Stable viewport ids for every row: channel ids, and `§<n>` for headings.
pub fn row_ids(channels: &[Channel], rows: &[SidebarRow]) -> Vec<String> {
    rows.iter()
        .enumerate()
        .map(|(position, row)| match row {
            SidebarRow::Channel(index) => channels[*index].id.to_string(),
            SidebarRow::Header(_) => format!("§{position}"),
        })
        .collect()
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
    agents: &HashMap<String, RemoteAgentView>,
    viewport: &ViewportState,
    unread: &std::collections::HashSet<uuid::Uuid>,
    sort: ChannelSort,
    show_system: bool,
    layout: &SidebarLayout,
    dm_presence: &HashMap<uuid::Uuid, crate::protocol::presence::Presence>,
    huddle_channel: Option<uuid::Uuid>,
    theme: &Theme,
    focused: bool,
) {
    let rows = rows(channels, unread, sort, show_system, layout);
    let system_count = channels
        .iter()
        .filter(|channel| channel.is_member && channel.kind.is_system())
        .count();
    let items = rows.iter().map(|row| {
        let index = match row {
            SidebarRow::Header(label) => {
                return ListItem::new(Line::from(format!("  {label}")))
                    .style(theme.style(HighlightGroup::PaneTitle));
            }
            SidebarRow::Channel(index) => index,
        };
        let channel = &channels[*index];
        let is_unread = unread.contains(&channel.id);
        let badge = if is_unread { "●" } else { " " };
        let privacy = if channel.kind.is_dm() {
            if agents
                .values()
                .any(|agent| !agent.stale && agent.channel_ids.contains(&channel.id))
            {
                "◆ @"
            } else {
                "@"
            }
        } else if channel.kind.is_system() {
            "⚙"
        } else if matches!(channel.visibility, crate::domain::Visibility::Private) {
            "🔒"
        } else {
            "#"
        };
        // The unread badge is the leading dot; presence trails the name so the
        // two never read as one another.
        let presence = match dm_presence.get(&channel.id) {
            Some(crate::protocol::presence::Presence::Online) => " ●",
            Some(crate::protocol::presence::Presence::Away) => " ◐",
            _ => "",
        };
        let huddle = if huddle_channel == Some(channel.id) {
            " 🎧"
        } else {
            ""
        };
        ListItem::new(Line::from(format!(
            "{badge} {privacy}{}{presence}{huddle}",
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
            rows.iter().position(|row| {
                matches!(row, SidebarRow::Channel(index) if channels[*index].id.to_string() == *selected)
            })
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
                    .title(match (system_count, show_system) {
                        (0, _) => format!(" channels · {} ", sort.label()),
                        (count, false) => {
                            format!(" channels · {} · ⚙{count} ", sort.label())
                        }
                        (_, true) => format!(" channels · {} · ⚙ open ", sort.label()),
                    }),
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
            facts: Default::default(),
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
            ordered_indexes(
                &channels,
                &unread,
                ChannelSort::Smart,
                false,
                &Default::default()
            ),
            vec![0, 1, 2]
        );
        assert_eq!(
            ordered_indexes(
                &channels,
                &unread,
                ChannelSort::Recent,
                false,
                &Default::default()
            ),
            vec![1, 0, 2]
        );
        assert_eq!(
            ordered_indexes(
                &channels,
                &unread,
                ChannelSort::Alphabetical,
                false,
                &Default::default()
            ),
            vec![1, 2, 0]
        );
    }

    /// v0.14 M4: Solo chats, the synced sections in their order, channels,
    /// forums and DMs, each sorted by its synced mode when it has one.
    #[test]
    fn channels_group_like_buzz_with_synced_sections_and_sort() {
        use super::{SidebarRow, row_ids, rows};
        let mut solo = channel("plans", Some(1));
        solo.visibility = Visibility::Private;
        solo.facts.labels = vec!["buzz.solo".into()];
        let mut dm = channel("ada", Some(50));
        dm.kind = ChannelKind::Dm;
        let mut forum = channel("ideas", Some(5));
        forum.kind = ChannelKind::Forum;
        let channels = vec![
            channel("zeta", Some(30)),
            channel("alpha", Some(10)),
            solo,
            dm,
            forum,
            channel("design", Some(40)),
            channel("eng", Some(20)),
        ];
        let layout = crate::domain::SidebarLayout {
            sections: vec![crate::domain::SidebarSection {
                id: "s1".into(),
                name: "Team".into(),
                icon: Some("🚀".into()),
            }],
            assignments: [
                (channels[5].id, "s1".to_owned()),
                (channels[6].id, "s1".to_owned()),
            ]
            .into(),
            sort: [
                ("channels".to_owned(), "alpha".to_owned()),
                ("section:s1".to_owned(), "recent".to_owned()),
            ]
            .into(),
        };
        let got = rows(
            &channels,
            &HashSet::new(),
            ChannelSort::Recent,
            false,
            &layout,
        );
        use SidebarRow::{Channel as C, Header as H};
        assert_eq!(
            got,
            vec![
                H("Solo".into()),
                C(2),
                H("🚀 Team".into()),
                C(5),
                C(6),
                H("Channels".into()),
                C(1),
                C(0),
                H("Forums".into()),
                C(4),
                H("Direct messages".into()),
                C(3),
            ]
        );
        assert_eq!(
            ordered_indexes(
                &channels,
                &HashSet::new(),
                ChannelSort::Recent,
                false,
                &layout
            ),
            vec![2, 5, 6, 1, 0, 4, 3]
        );
        let ids = row_ids(&channels, &got);
        assert_eq!(ids[0], "§0");
        assert_eq!(ids[1], channels[2].id.to_string());
        // A single group needs no heading, as before v0.14.
        let only = vec![channel("a", None), channel("b", None)];
        assert!(
            rows(
                &only,
                &HashSet::new(),
                ChannelSort::Recent,
                false,
                &Default::default()
            )
            .iter()
            .all(|row| matches!(row, SidebarRow::Channel(_)))
        );
    }
}
