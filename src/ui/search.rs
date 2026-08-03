use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::{
    domain::{SearchResult, SearchResultKind},
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected_id: Option<String>,
    pub generation: u64,
    pub loading: bool,
    pub local_only: bool,
    pub notice: Option<String>,
}

impl SearchState {
    pub fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.notice = None;
    }

    pub fn reconcile(&mut self) {
        if !self
            .results
            .iter()
            .any(|result| Some(&result.stable_id) == self.selected_id.as_ref())
        {
            self.selected_id = self.results.first().map(|result| result.stable_id.clone());
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.results.is_empty() {
            self.selected_id = None;
            return;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|id| {
                self.results
                    .iter()
                    .position(|result| result.stable_id == *id)
            })
            .unwrap_or_default();
        let next = current
            .saturating_add_signed(delta)
            .min(self.results.len() - 1);
        self.selected_id = Some(self.results[next].stable_id.clone());
    }

    pub fn selected(&self) -> Option<&SearchResult> {
        self.selected_id
            .as_ref()
            .and_then(|id| self.results.iter().find(|result| result.stable_id == *id))
            .or_else(|| self.results.first())
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &SearchState, theme: &Theme) {
    frame.render_widget(Clear, area);
    let status = if state.loading {
        "searching…"
    } else if state.local_only {
        "offline · local results"
    } else {
        "local + relay"
    };
    let title = format!(" Search · {status} · from: in: after: before: · Enter open · Esc close ");
    let mut items = vec![ListItem::new(Line::from(vec![
        Span::styled("> ", theme.style(HighlightGroup::SelectionMarker)),
        Span::styled(
            sanitize::single_line(&state.query),
            theme.style(HighlightGroup::Normal),
        ),
    ]))];
    if let Some(notice) = &state.notice {
        items.push(
            ListItem::new(format!("  {}", sanitize::single_line(notice)))
                .style(theme.style(HighlightGroup::StatusMode)),
        );
    }
    if state.results.is_empty() && !state.loading && state.notice.is_none() {
        items.push(ListItem::new("  No visible results."));
    }
    items.extend(state.results.iter().map(|result| {
        ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", kind_label(result.kind)),
                    theme.style(HighlightGroup::MessageTimestamp),
                ),
                Span::styled(
                    sanitize::single_line(&result.label),
                    theme.style(HighlightGroup::Normal),
                ),
            ]),
            Line::from(format!("    {}", sanitize::single_line(&result.detail))),
        ])
    }));
    let selected = state.selected_id.as_ref().and_then(|id| {
        state
            .results
            .iter()
            .position(|result| result.stable_id == *id)
            .map(|index| index + 1 + usize::from(state.notice.is_some()))
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

const fn kind_label(kind: SearchResultKind) -> &'static str {
    match kind {
        SearchResultKind::Channel => "channel",
        SearchResultKind::Dm => "DM",
        SearchResultKind::Person => "person",
        SearchResultKind::Message => "message",
    }
}
