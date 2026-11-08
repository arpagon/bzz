//! Artifacts panel: the selected channel's (or open thread's) NIP-AR set, or
//! the cross-channel "My tasks" view. Content schemas are unpublished, so
//! every type renders the safe fallback: title, type, home and its
//! client-defined tags, never interpreted content.

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use uuid::Uuid;

use crate::{
    domain::Profile,
    protocol::artifact::display,
    store::artifacts::StoredArtifact,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactScope {
    Channel(Uuid),
    Thread(Uuid, String),
    Assigned,
}

#[derive(Clone, Debug)]
pub struct ArtifactPanelState {
    pub scope: ArtifactScope,
    pub items: Vec<StoredArtifact>,
    pub selected: Option<Uuid>,
    pub loading: bool,
    /// Sync state of the selected channel ("synced", "stale", "unsupported").
    pub sync_note: Option<String>,
}

impl Default for ArtifactPanelState {
    fn default() -> Self {
        Self {
            scope: ArtifactScope::Assigned,
            items: Vec::new(),
            selected: None,
            loading: false,
            sync_note: None,
        }
    }
}

impl ArtifactPanelState {
    pub fn set_items(&mut self, items: Vec<StoredArtifact>) {
        self.items = items;
        if !self
            .items
            .iter()
            .any(|item| Some(item.head.id) == self.selected)
        {
            self.selected = self.items.first().map(|item| item.head.id);
        }
    }

    pub fn selected(&self) -> Option<&StoredArtifact> {
        let id = self.selected?;
        self.items.iter().find(|item| item.head.id == id)
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|id| self.items.iter().position(|item| item.head.id == id))
            .unwrap_or_default();
        let next = current
            .saturating_add_signed(delta)
            .min(self.items.len() - 1);
        self.selected = Some(self.items[next].head.id);
    }
}

fn short(pubkey: &str, profiles: &HashMap<String, Profile>) -> String {
    profiles
        .get(pubkey)
        .and_then(|profile| {
            profile
                .display_name
                .clone()
                .or_else(|| profile.name.clone())
        })
        .map_or_else(
            || pubkey.chars().take(8).collect(),
            |name| display(&name, 24),
        )
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &ArtifactPanelState,
    channel_names: &HashMap<Uuid, String>,
    profiles: &HashMap<String, Profile>,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let scope = match &state.scope {
        ArtifactScope::Channel(channel) => format!(
            "artifacts in #{}",
            channel_names.get(channel).map_or("channel", String::as_str)
        ),
        ArtifactScope::Thread(..) => "artifacts in this thread".into(),
        ArtifactScope::Assigned => "my tasks".into(),
    };
    let note = state
        .sync_note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    let loading = if state.loading { " · loading…" } else { "" };
    let title = format!(
        " {scope}{note}{loading} · ↑/↓ · Enter open · d delete · u restore · :artifact rename|move · Esc "
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if state.items.is_empty() {
        let empty = match (&state.scope, state.loading) {
            (_, true) => "Loading artifacts…",
            (ArtifactScope::Assigned, false) => "No open tasks are assigned to you.",
            _ => "No artifacts here.",
        };
        frame.render_widget(
            Paragraph::new(empty)
                .style(theme.style(HighlightGroup::Normal))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let panes = Layout::default()
        .direction(if inner.width >= 88 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(inner);
    let items = state
        .items
        .iter()
        .map(|item| {
            let head = &item.head;
            let title = if head.is_deleted() {
                let previous = item
                    .live
                    .as_ref()
                    .and_then(|live| live.title.as_deref())
                    .unwrap_or("untitled");
                format!("✗ {} (deleted)", display(previous, 60))
            } else {
                display(head.title.as_deref().unwrap_or("untitled"), 60)
            };
            ListItem::new(Line::from(vec![
                Span::styled(title, theme.style(HighlightGroup::Normal)),
                Span::styled(
                    format!("  {}", display(&head.artifact_type, 32)),
                    theme.style(HighlightGroup::Muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let mut list_state = ListState::default().with_selected(
        state
            .selected
            .and_then(|id| state.items.iter().position(|item| item.head.id == id)),
    );
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(HighlightGroup::Selection)),
        panes[0],
        &mut list_state,
    );
    let Some(selected) = state.selected() else {
        return;
    };
    let head = &selected.head;
    let mut lines = vec![
        Line::from(display(
            head.title
                .as_deref()
                .or(selected
                    .live
                    .as_ref()
                    .and_then(|live| live.title.as_deref()))
                .unwrap_or("untitled"),
            120,
        )),
        Line::from(Span::styled(
            format!(
                "type {} · in #{} · {} by {}",
                display(&head.artifact_type, 64),
                channel_names
                    .get(&head.home)
                    .map_or_else(|| head.home.to_string(), |name| display(name, 40)),
                format!("{:?}", head.op).to_lowercase(),
                short(&head.author, profiles),
            ),
            theme.style(HighlightGroup::Muted),
        )),
    ];
    if head.root.is_some() {
        lines.push(Line::from(Span::styled(
            "attached to a thread",
            theme.style(HighlightGroup::Muted),
        )));
    }
    for (name, value) in selected
        .live
        .as_ref()
        .unwrap_or(head)
        .client_tags()
        .into_iter()
        .take(12)
    {
        let value = if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            short(&value, profiles)
        } else {
            display(&value, 64)
        };
        lines.push(Line::from(format!("{}: {value}", display(&name, 24))));
    }
    lines.push(Line::from(Span::styled(
        "Content uses a schema bzz does not edit; it is kept unchanged.",
        theme.style(HighlightGroup::Muted),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
        panes[1],
    );
}
