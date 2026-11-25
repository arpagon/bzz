//! Artifacts panel: the selected channel's (or open thread's) NIP-AR set, or
//! the cross-channel "My tasks" view. Content schemas are unpublished, so
//! every type renders the safe fallback: title, type, home and its
//! client-defined tags, never interpreted content.
//!
//! An app offered in the channel (`bzz.app`, v0.16) is listed like any
//! artifact, as Buzz lets any member rename or delete it; the panel says
//! what it is and what a rename or a delete here does to the offer. An
//! app's records (`bzz.app.<app>.<record>`) are not listed.

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use uuid::Uuid;

use crate::{
    domain::Profile,
    protocol::artifact::{APP_OFFER_TYPE, display},
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

/// What the panel says of an app offered in the channel.
const APP_OFFER_LINES: [&str; 3] = [
    "An app offered in this channel: its script, manifest and brief. Read and answer it with Ctrl-Y; :apps lists it.",
    "A rename here publishes a new revision signed by you, which every bzz of the channel then offers as yours. A delete here withdraws the offer for everyone.",
    "Copies people installed, and the app's records, stay either way. To withdraw your own offer, :app unshare.",
];

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
    let title = format!("{scope}{note}{loading}");
    let block = crate::ui::frame::modal(
        theme,
        BorderSurface::Picker,
        &title,
        &[
            ("↑/↓", ""),
            ("Enter", "open"),
            ("d", "delete"),
            ("u", "restore"),
            (":artifact", "rename|move"),
            ("Esc", "close"),
        ],
    );
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
            let kind = if head.artifact_type == APP_OFFER_TYPE {
                format!("  {APP_OFFER_TYPE} · an app offered here")
            } else {
                format!("  {}", display(&head.artifact_type, 32))
            };
            ListItem::new(Line::from(vec![
                Span::styled(title, theme.style(HighlightGroup::Normal)),
                Span::styled(kind, theme.style(HighlightGroup::Muted)),
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
    if head.artifact_type == APP_OFFER_TYPE {
        for line in APP_OFFER_LINES {
            lines.push(Line::from(Span::styled(
                line,
                theme.style(HighlightGroup::Muted),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Content uses a schema bzz does not edit; it is kept unchanged.",
            theme.style(HighlightGroup::Muted),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
        panes[1],
    );
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::protocol::artifact::{KIND_ARTIFACT, parse};

    fn artifact(keys: &Keys, home: Uuid, kind: &str, title: &str) -> StoredArtifact {
        let tags = [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &home.to_string()],
            ["type", kind],
            ["title", title],
            ["op", "create"],
        ]
        .map(|parts| Tag::parse(parts).unwrap());
        let head = parse(
            &EventBuilder::new(Kind::Custom(KIND_ARTIFACT), "{}")
                .tags(tags)
                .sign_with_keys(keys)
                .unwrap(),
        )
        .unwrap();
        StoredArtifact {
            live: Some(head.clone()),
            head,
        }
    }

    fn drawn(state: &ArtifactPanelState) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    state,
                    &HashMap::new(),
                    &HashMap::new(),
                    &Theme::default(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn an_app_offered_in_the_channel_is_listed_and_said_for_what_it_is() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let offer = artifact(&keys, channel, APP_OFFER_TYPE, "Board");
        let task = artifact(&keys, channel, "buzz.task", "A task");
        let mut state = ArtifactPanelState {
            scope: ArtifactScope::Channel(channel),
            ..ArtifactPanelState::default()
        };
        state.set_items(vec![offer, task.clone()]);
        let shown = drawn(&state);
        assert!(shown.contains("bzz.app · an app offered here"), "{shown}");
        for words in [
            "An app offered in this channel",
            "A rename here publishes a new revision signed by you",
            "A delete here withdraws the offer for everyone",
            ":app unshare",
        ] {
            assert!(shown.contains(words), "`{words}` in:\n{shown}");
        }
        assert!(!shown.contains("Content uses a schema"), "{shown}");
        // Any other type is as before.
        state.selected = Some(task.head.id);
        let shown = drawn(&state);
        assert!(
            shown.contains("Content uses a schema bzz does not edit"),
            "{shown}"
        );
        assert!(!shown.contains("An app offered in this channel"), "{shown}");
    }
}
