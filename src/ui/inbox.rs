//! Inbox route presentation.
//!
//! The Inbox is a persistent workspace: its list and detail each retain their
//! own viewport state. This module only renders already-authorized local data;
//! it never queries a service, advances read state, or publishes a message.

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    domain::{InboxCategory, InboxItem, Message, Profile},
    render::sanitize,
    ui::{
        state::{FocusSurface, ViewportState},
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

const WIDE_INBOX_WIDTH: u16 = 88;
const MAX_DETAIL_MESSAGE_CHARS: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InboxFilter {
    #[default]
    All,
    Mentions,
    Threads,
    Dms,
    NeedsAction,
    Unread,
    Drafts,
}

impl InboxFilter {
    pub const fn next(self) -> Self {
        match self {
            Self::All => Self::Mentions,
            Self::Mentions => Self::Threads,
            Self::Threads => Self::Dms,
            Self::Dms => Self::NeedsAction,
            Self::NeedsAction => Self::Unread,
            Self::Unread => Self::Drafts,
            Self::Drafts => Self::All,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Mentions => "Mentions",
            Self::Threads => "Threads",
            Self::Dms => "DMs",
            Self::NeedsAction => "Needs action",
            Self::Unread => "Unread",
            Self::Drafts => "Drafts",
        }
    }

    pub fn matches(self, item: &InboxItem) -> bool {
        match self {
            Self::All => !item.draft_only(),
            Self::Mentions => item.categories.contains(&InboxCategory::Mention),
            Self::Threads => item.categories.contains(&InboxCategory::Thread),
            Self::Dms => item.categories.contains(&InboxCategory::Dm),
            Self::NeedsAction => item.categories.contains(&InboxCategory::NeedsAction),
            Self::Unread => item.unread(),
            Self::Drafts => item.categories.contains(&InboxCategory::Draft),
        }
    }
}

/// Layout measured once by the route renderer and reused by App when it emits
/// semantic hit regions. No event handler reconstructs Inbox geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InboxLayout {
    pub list: Option<Rect>,
    pub detail: Option<Rect>,
    pub narrow: bool,
}

pub fn layout(area: Rect, narrow_detail: bool) -> InboxLayout {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let narrow = inner.width < WIDE_INBOX_WIDTH;
    if narrow && narrow_detail {
        return InboxLayout {
            list: None,
            detail: Some(inner),
            narrow,
        };
    }
    if narrow {
        return InboxLayout {
            list: Some(inner),
            detail: None,
            narrow,
        };
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(inner);
    InboxLayout {
        list: Some(columns[0]),
        detail: Some(columns[1]),
        narrow,
    }
}

#[derive(Clone, Debug, Default)]
pub struct InboxState {
    pub list_viewport: ViewportState,
    pub detail_viewport: ViewportState,
    pub filter: InboxFilter,
    /// Updated from the measured route layout; input uses this to distinguish
    /// a wide pane focus from a narrow route-local detail screen.
    pub narrow_layout: bool,
    /// In a narrow terminal this selects the route-local detail screen. Wide
    /// terminals always draw both panes but retain focus independently.
    pub narrow_detail: bool,
}

impl InboxState {
    pub fn visible<'a>(&self, items: &'a [InboxItem]) -> Vec<&'a InboxItem> {
        items
            .iter()
            .filter(|item| self.filter.matches(item))
            .collect()
    }

    pub fn visible_ids(&self, items: &[InboxItem]) -> Vec<String> {
        self.visible(items)
            .into_iter()
            .map(|item| item.conversation_id.clone())
            .collect()
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.list_viewport.selected_id.as_deref()
    }

    pub fn selected<'a>(&self, items: &'a [InboxItem]) -> Option<&'a InboxItem> {
        let visible = self.visible(items);
        self.list_viewport
            .selected_id
            .as_ref()
            .and_then(|id| visible.iter().find(|item| item.conversation_id == *id))
            .copied()
            .or_else(|| visible.first().copied())
    }

    pub fn reconcile(&mut self, items: &[InboxItem]) {
        let selected_before = self.list_viewport.selected_id.clone();
        self.list_viewport.reconcile(self.visible_ids(items));
        if self.list_viewport.selected_id != selected_before {
            self.detail_viewport = ViewportState::default();
        }
    }

    pub fn select(&mut self, conversation_id: String) {
        if self.list_viewport.selected_id.as_deref() != Some(conversation_id.as_str()) {
            self.detail_viewport = ViewportState::default();
        }
        self.list_viewport.select(Some(conversation_id));
    }

    pub fn move_by(&mut self, items: &[InboxItem], delta: isize) {
        let visible = self.visible(items);
        if visible.is_empty() {
            self.list_viewport.select(None);
            return;
        }
        let current = self
            .list_viewport
            .selected_id
            .as_ref()
            .and_then(|id| visible.iter().position(|item| item.conversation_id == *id))
            .unwrap_or_default();
        let next = current.saturating_add_signed(delta).min(visible.len() - 1);
        self.select(visible[next].conversation_id.clone());
    }

    pub fn move_to_edge(&mut self, items: &[InboxItem], last: bool) {
        let visible = self.visible(items);
        if let Some(item) = visible.get(if last {
            visible.len().saturating_sub(1)
        } else {
            0
        }) {
            self.select(item.conversation_id.clone());
        }
    }

    pub fn scroll_list(&mut self, delta: isize, items: &[InboxItem]) {
        self.list_viewport
            .scroll_by(delta, self.visible(items).len());
    }

    pub fn scroll_detail(&mut self, delta: isize, messages: &[Message]) {
        self.detail_viewport
            .scroll_by(delta, detail_line_count(messages));
    }

    /// Keep a stable event anchor while a background detail refresh completes.
    pub fn reconcile_detail(&mut self, messages: &[Message], unread_anchor: Option<&str>) {
        let ids = messages
            .iter()
            .map(|message| message.event_id.clone())
            .collect::<Vec<_>>();
        if self.detail_viewport.selected_id.is_none() {
            let anchor = unread_anchor
                .map(str::to_owned)
                .or_else(|| ids.last().cloned());
            if let Some(anchor_id) = anchor.as_deref()
                && let Some(index) = messages
                    .iter()
                    .position(|message| message.event_id == anchor_id)
            {
                self.detail_viewport.scroll = detail_line_count(&messages[..index]);
            }
            self.detail_viewport.select(anchor);
        }
        if !self
            .detail_viewport
            .selected_id
            .as_ref()
            .is_some_and(|selected| ids.iter().any(|id| id == selected))
        {
            self.detail_viewport.select(ids.first().cloned());
        }
    }

    pub fn set_detail_viewport_height(&mut self, height: usize, messages: &[Message]) {
        self.detail_viewport.viewport_height = height.max(1);
        let max_scroll =
            detail_line_count(messages).saturating_sub(self.detail_viewport.viewport_height);
        self.detail_viewport.scroll = self.detail_viewport.scroll.min(max_scroll);
    }
}

pub struct InboxView<'a> {
    pub items: &'a [InboxItem],
    pub messages: &'a [Message],
    pub profiles: &'a HashMap<String, Profile>,
    pub focus: FocusSurface,
    pub theme: &'a Theme,
    pub loading: bool,
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut InboxState,
    view: InboxView<'_>,
) -> InboxLayout {
    let items = view.items;
    let profiles = view.profiles;
    let focus = view.focus;
    let theme = view.theme;
    let loading = view.loading;
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Pane))
        .border_style(theme.style(HighlightGroup::PaneBorder))
        .title_style(theme.style(HighlightGroup::PaneTitle))
        .title(format!(
            " Inbox · {} · f filter · Enter detail · o source · i reply · m/U read/unread · a visible read ",
            state.filter.label()
        ));
    frame.render_widget(outer, area);
    let route_layout = layout(area, state.narrow_detail);
    state.narrow_layout = route_layout.narrow;
    let visible = state.visible(items);
    if visible.is_empty() {
        let target = route_layout.detail.or(route_layout.list).unwrap_or(area);
        frame.render_widget(
            Paragraph::new(if loading {
                "Loading Inbox…"
            } else {
                "No conversations match this filter."
            })
            .style(theme.style(HighlightGroup::Normal)),
            target,
        );
        return route_layout;
    }

    if let Some(list) = route_layout.list {
        state.list_viewport.set_viewport_height(
            usize::from(list.height.saturating_div(2).max(1)),
            &state.visible_ids(items),
        );
        let list_items = visible.iter().map(|item| list_item(item, profiles, theme));
        let selected = state
            .list_viewport
            .selected_id
            .as_ref()
            .and_then(|id| visible.iter().position(|item| item.conversation_id == *id));
        let mut list_state = ListState::default()
            .with_selected(selected)
            .with_offset(state.list_viewport.scroll);
        frame.render_stateful_widget(
            List::new(list_items)
                .highlight_symbol("› ")
                .highlight_style(theme.style(HighlightGroup::SelectedRow))
                .block(
                    Block::default()
                        .borders(if route_layout.detail.is_some() {
                            Borders::RIGHT
                        } else {
                            Borders::NONE
                        })
                        .border_style(theme.style(if focus == FocusSurface::InboxList {
                            HighlightGroup::FocusedPaneBorder
                        } else {
                            HighlightGroup::PaneBorder
                        })),
                ),
            list,
            &mut list_state,
        );
    }
    if let Some(detail) = route_layout.detail {
        render_detail(frame, detail, state, view);
    }
    route_layout
}

fn list_item(
    item: &InboxItem,
    profiles: &HashMap<String, Profile>,
    theme: &Theme,
) -> ListItem<'static> {
    let unread = if item.unread() { "●" } else { " " };
    let categories = item
        .categories
        .iter()
        .map(category_label)
        .collect::<Vec<_>>()
        .join("+");
    let sender = item
        .sender_pubkey
        .as_ref()
        .map(|pubkey| {
            profiles
                .get(pubkey)
                .map_or_else(|| crate::domain::abbreviated_pubkey(pubkey), Profile::label)
        })
        .unwrap_or_else(|| "draft".into());
    ListItem::new(vec![
        Line::from(vec![
            Span::raw(format!("{unread} [{categories}] ")),
            Span::styled(
                sanitize::single_line(&sender),
                theme.style(HighlightGroup::MessageAuthor),
            ),
            Span::raw(format!("  {}", relative_time(item.created_at))),
        ]),
        Line::from(format!(
            "  {}{}",
            sanitize::single_line(&item.preview),
            if item.draft_count > 0 {
                format!(" · {} draft", item.draft_count)
            } else {
                String::new()
            }
        )),
    ])
    .style(theme.style(if item.unread() {
        HighlightGroup::ChannelUnread
    } else {
        HighlightGroup::Normal
    }))
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, state: &mut InboxState, view: InboxView<'_>) {
    let Some(item) = state.selected(view.items) else {
        return;
    };
    state.reconcile_detail(view.messages, item.first_unread_event_id.as_deref());
    let lines = detail_lines(item, view.messages, view.profiles, view.theme);
    state.set_detail_viewport_height(
        usize::from(area.height.saturating_sub(2).max(1)),
        view.messages,
    );
    frame.render_widget(
        Paragraph::new(lines)
            .style(view.theme.style(HighlightGroup::Normal))
            .scroll((
                u16::try_from(state.detail_viewport.scroll).unwrap_or(u16::MAX),
                0,
            ))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(
                        view.theme
                            .style(if view.focus == FocusSurface::InboxDetail {
                                HighlightGroup::FocusedPaneBorder
                            } else {
                                HighlightGroup::PaneBorder
                            }),
                    )
                    .title(" detail · i reply · o source · m mark read "),
            ),
        area,
    );
}

fn detail_lines(
    item: &InboxItem,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            if item.unread() {
                format!("{} unread", item.unread_count)
            } else {
                "read".into()
            },
            theme.style(if item.unread() {
                HighlightGroup::ChannelUnread
            } else {
                HighlightGroup::SidebarText
            }),
        ),
        Span::raw(" · opening does not acknowledge"),
    ])];
    if messages.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(sanitize::text(&item.preview)));
        lines.push(Line::default());
        lines.push(Line::from(if item.draft_count > 0 {
            "Local draft available. Press i to restore it in the ordinary composer."
        } else {
            "The bounded local context is unavailable. Press o for canonical source context."
        }));
        return lines;
    }
    for message in messages {
        if item.first_unread_event_id.as_deref() == Some(message.event_id.as_str()) {
            lines.push(Line::default());
            lines.push(Line::styled(
                "── first unread ──",
                theme.style(HighlightGroup::ChannelUnread),
            ));
        }
        let author = profiles.get(&message.pubkey).map_or_else(
            || crate::domain::abbreviated_pubkey(&message.pubkey),
            Profile::label,
        );
        lines.push(Line::default());
        lines.push(Line::styled(
            format!(
                "{} · {}",
                sanitize::single_line(&author),
                relative_time(message.created_at)
            ),
            theme.style(HighlightGroup::MessageAuthor),
        ));
        let content = sanitize::text(&message.content)
            .chars()
            .take(MAX_DETAIL_MESSAGE_CHARS)
            .collect::<String>();
        lines.extend(content.split('\n').map(|line| Line::from(line.to_owned())));
        if !message.attachments.is_empty() {
            lines.push(Line::styled(
                format!("[{} attachment(s)]", message.attachments.len()),
                theme.style(HighlightGroup::SidebarText),
            ));
        }
    }
    lines
}

fn detail_line_count(messages: &[Message]) -> usize {
    messages.iter().fold(2, |count, message| {
        count
            .saturating_add(3)
            .saturating_add(
                message
                    .content
                    .lines()
                    .count()
                    .min(MAX_DETAIL_MESSAGE_CHARS),
            )
            .saturating_add(usize::from(!message.attachments.is_empty()))
    })
}

const fn category_label(category: &InboxCategory) -> &'static str {
    match category {
        InboxCategory::Mention => "mention",
        InboxCategory::Thread => "thread",
        InboxCategory::Dm => "DM",
        InboxCategory::NeedsAction => "action",
        InboxCategory::Draft => "draft",
    }
}

fn relative_time(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let age = now.saturating_sub(timestamp);
    if age < 60 {
        "now".into()
    } else if age < 3_600 {
        format!("{}m", age / 60)
    } else if age < 86_400 {
        format!("{}h", age / 3_600)
    } else {
        format!("{}d", age / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::domain::{InboxCategory, InboxItem};

    use super::{InboxFilter, InboxState};

    fn draft() -> InboxItem {
        InboxItem {
            conversation_id: format!("draft:{}", Uuid::nil()),
            categories: vec![InboxCategory::Draft],
            event_id: None,
            channel_id: Some(Uuid::nil()),
            thread_root: None,
            sender_pubkey: None,
            created_at: 1,
            preview: "draft".into(),
            unread_count: 0,
            first_unread_event_id: None,
            first_unread_at: None,
            draft_count: 1,
            latest_draft_at: Some(1),
            forced_unread: false,
        }
    }

    #[test]
    fn all_excludes_a_draft_only_conversation() {
        assert!(!InboxFilter::All.matches(&draft()));
        assert!(InboxFilter::Drafts.matches(&draft()));
    }

    #[test]
    fn list_selection_is_stable_when_new_work_arrives() {
        let mut first = draft();
        first.categories.push(InboxCategory::Dm);
        let mut second = draft();
        second.categories.push(InboxCategory::Dm);
        second.conversation_id = format!("draft:{}", Uuid::new_v4());
        let mut state = InboxState::default();
        state.reconcile(&[first.clone(), second.clone()]);
        state.select(second.conversation_id.clone());
        let mut newer = draft();
        newer.categories.push(InboxCategory::Dm);
        newer.conversation_id = format!("draft:{}", Uuid::new_v4());
        state.reconcile(&[newer, first, second.clone()]);
        assert_eq!(
            state.list_viewport.selected_id,
            Some(second.conversation_id)
        );
    }
}
