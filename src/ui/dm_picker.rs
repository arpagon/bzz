use std::collections::{BTreeSet, HashMap};

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use uuid::Uuid;

use crate::{
    domain::Profile,
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

#[derive(Clone, Debug, Default)]
pub struct DmPickerState {
    pub query: String,
    pub selected_pubkey: Option<String>,
    pub recipients: BTreeSet<String>,
    pub add_to: Option<Uuid>,
    pub submitting: bool,
}

impl DmPickerState {
    pub fn candidates<'a>(
        &self,
        profiles: &'a HashMap<String, Profile>,
        self_pubkey: &str,
    ) -> Vec<&'a Profile> {
        let mut candidates = profiles
            .values()
            .filter(|profile| profile.pubkey != self_pubkey)
            .collect::<Vec<_>>();
        if self.query.is_empty() {
            candidates.sort_by_key(|profile| profile.label().to_ascii_lowercase());
            return candidates;
        }
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut ranked = candidates
            .into_iter()
            .filter_map(|profile| {
                let label = profile.label();
                let mut buffer = Vec::new();
                pattern
                    .score(Utf32Str::new(&label, &mut buffer), &mut matcher)
                    .map(|score| (score, label.to_ascii_lowercase(), profile))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        ranked.into_iter().map(|(_, _, profile)| profile).collect()
    }

    pub fn reconcile(&mut self, profiles: &HashMap<String, Profile>, self_pubkey: &str) {
        let candidates = self.candidates(profiles, self_pubkey);
        if !candidates
            .iter()
            .any(|profile| Some(&profile.pubkey) == self.selected_pubkey.as_ref())
        {
            self.selected_pubkey = candidates.first().map(|profile| profile.pubkey.clone());
        }
    }

    pub fn move_by(
        &mut self,
        profiles: &HashMap<String, Profile>,
        self_pubkey: &str,
        delta: isize,
    ) {
        let candidates = self.candidates(profiles, self_pubkey);
        if candidates.is_empty() {
            self.selected_pubkey = None;
            return;
        }
        let current = self
            .selected_pubkey
            .as_ref()
            .and_then(|pubkey| {
                candidates
                    .iter()
                    .position(|profile| profile.pubkey == *pubkey)
            })
            .unwrap_or_default();
        let next = current
            .saturating_add_signed(delta)
            .min(candidates.len() - 1);
        self.selected_pubkey = Some(candidates[next].pubkey.clone());
    }

    pub fn toggle_selected(&mut self) -> Result<(), &'static str> {
        let Some(pubkey) = self.selected_pubkey.clone() else {
            return Ok(());
        };
        if self.recipients.remove(&pubkey) {
            return Ok(());
        }
        if self.add_to.is_some() && !self.recipients.is_empty() {
            return Err("Add one participant at a time; the relay opens a new conversation.");
        }
        if self.recipients.len() >= 8 {
            return Err("A workspace DM supports at most eight other participants.");
        }
        self.recipients.insert(pubkey);
        Ok(())
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &DmPickerState,
    profiles: &HashMap<String, Profile>,
    self_pubkey: &str,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let candidates = state.candidates(profiles, self_pubkey);
    let action = if state.add_to.is_some() {
        "Add participant opens a new immutable conversation"
    } else {
        "Private workspace DM · relay-readable, not end-to-end encrypted"
    };
    let title = format!(
        " {action} · Space select · Enter open · Esc cancel{} ",
        if state.submitting {
            " · submitting…"
        } else {
            ""
        }
    );
    let selected_labels = state
        .recipients
        .iter()
        .filter_map(|pubkey| profiles.get(pubkey))
        .map(Profile::label)
        .collect::<Vec<_>>()
        .join(", ");
    let mut items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("> ", theme.style(HighlightGroup::SelectionMarker)),
            Span::styled(
                sanitize::single_line(&state.query),
                theme.style(HighlightGroup::Normal),
            ),
        ])),
        ListItem::new(format!(
            "selected: {}",
            if selected_labels.is_empty() {
                "none".into()
            } else {
                sanitize::single_line(&selected_labels)
            }
        )),
    ];
    items.extend(candidates.iter().take(100).map(|profile| {
        let selected = state.recipients.contains(&profile.pubkey);
        ListItem::new(format!(
            "{} {}  {}",
            if selected { "[x]" } else { "[ ]" },
            sanitize::single_line(&profile.label()),
            crate::domain::abbreviated_pubkey(&profile.pubkey)
        ))
        .style(theme.style(if selected {
            HighlightGroup::ChannelUnread
        } else {
            HighlightGroup::Normal
        }))
    }));
    let selected = state.selected_pubkey.as_ref().and_then(|pubkey| {
        candidates
            .iter()
            .take(100)
            .position(|profile| profile.pubkey == *pubkey)
            .map(|index| index + 2)
    });
    let mut list_state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(theme.style(HighlightGroup::SelectedRow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme.border_type(BorderSurface::Picker))
                    .border_style(theme.style(HighlightGroup::ModalBorder))
                    .title_style(theme.style(HighlightGroup::ModalTitle))
                    .title(title),
            ),
        area,
        &mut list_state,
    );
}
