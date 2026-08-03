use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    domain::{InboxCategory, InboxItem, Profile},
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

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
            Self::All => true,
            Self::Mentions => item.categories.contains(&InboxCategory::Mention),
            Self::Threads => item.categories.contains(&InboxCategory::Thread),
            Self::Dms => item.categories.contains(&InboxCategory::Dm),
            Self::NeedsAction => item.categories.contains(&InboxCategory::NeedsAction),
            Self::Unread => item.unread(),
            Self::Drafts => item.categories.contains(&InboxCategory::Draft),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct InboxState {
    pub selected_id: Option<String>,
    pub filter: InboxFilter,
    pub narrow_detail: bool,
}

impl InboxState {
    pub fn visible<'a>(&self, items: &'a [InboxItem]) -> Vec<&'a InboxItem> {
        items
            .iter()
            .filter(|item| self.filter.matches(item))
            .collect()
    }

    pub fn selected<'a>(&self, items: &'a [InboxItem]) -> Option<&'a InboxItem> {
        let visible = self.visible(items);
        self.selected_id
            .as_ref()
            .and_then(|id| visible.iter().find(|item| item.conversation_id == *id))
            .copied()
            .or_else(|| visible.first().copied())
    }

    pub fn reconcile(&mut self, items: &[InboxItem]) {
        let visible = self.visible(items);
        if !visible
            .iter()
            .any(|item| Some(&item.conversation_id) == self.selected_id.as_ref())
        {
            self.selected_id = visible.first().map(|item| item.conversation_id.clone());
        }
    }

    pub fn move_by(&mut self, items: &[InboxItem], delta: isize) {
        let visible = self.visible(items);
        if visible.is_empty() {
            self.selected_id = None;
            return;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|id| visible.iter().position(|item| item.conversation_id == *id))
            .unwrap_or_default();
        let next = current.saturating_add_signed(delta).min(visible.len() - 1);
        self.selected_id = Some(visible[next].conversation_id.clone());
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    items: &[InboxItem],
    profiles: &HashMap<String, Profile>,
    state: &InboxState,
    theme: &Theme,
    loading: bool,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Modal))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(format!(
            " Inbox · {} · f filter · Enter detail · o open · i reply · m/U read/unread · a all read · Esc close ",
            state.filter.label()
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let visible = state.visible(items);
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(if loading {
                "Loading Inbox…"
            } else {
                "No conversations match this filter."
            })
            .style(theme.style(HighlightGroup::Normal)),
            inner,
        );
        return;
    }
    let narrow = inner.width < 88;
    let selected = state.selected(items);
    if narrow && state.narrow_detail {
        render_detail(frame, inner, selected, profiles, theme);
        return;
    }
    let columns = if narrow {
        vec![inner]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(inner)
            .to_vec()
    };
    let list_items = visible.iter().map(|item| {
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
    });
    let mut list_state = ListState::default().with_selected(
        state
            .selected_id
            .as_ref()
            .and_then(|id| visible.iter().position(|item| item.conversation_id == *id)),
    );
    frame.render_stateful_widget(
        List::new(list_items)
            .highlight_symbol("› ")
            .highlight_style(theme.style(HighlightGroup::SelectedRow))
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(theme.style(HighlightGroup::PaneBorder)),
            ),
        columns[0],
        &mut list_state,
    );
    if !narrow {
        render_detail(frame, columns[1], selected, profiles, theme);
    }
}

fn render_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: Option<&InboxItem>,
    profiles: &HashMap<String, Profile>,
    theme: &Theme,
) {
    let Some(item) = selected else {
        return;
    };
    let sender = item
        .sender_pubkey
        .as_ref()
        .map(|pubkey| {
            profiles
                .get(pubkey)
                .map_or_else(|| crate::domain::abbreviated_pubkey(pubkey), Profile::label)
        })
        .unwrap_or_else(|| "Local draft".into());
    let categories = item
        .categories
        .iter()
        .map(category_label)
        .collect::<Vec<_>>()
        .join(", ");
    let body = format!(
        "{}\n{}\n\n{}\n\n{}\n\n{}",
        sanitize::single_line(&sender),
        categories,
        sanitize::text(&item.preview),
        if item.unread() { "Unread" } else { "Read" },
        if item.categories.contains(&InboxCategory::NeedsAction) {
            "Needs-action cards are read-only in this release. Open the source context for details."
        } else {
            "Press o to open the source context or i to open it and reply."
        }
    );
    frame.render_widget(
        Paragraph::new(body)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(theme.style(HighlightGroup::PaneBorder))
                    .title(" detail "),
            ),
        area,
    );
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
