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
    /// Loading the next page of older messages (Ctrl-o).
    pub older_loading: bool,
}

impl SearchState {
    pub fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.older_loading = false;
        self.notice = None;
    }

    /// The creation time of the oldest message shown: the next page's bound.
    pub fn oldest_message(&self) -> Option<u64> {
        self.results
            .iter()
            .filter(|result| result.kind == SearchResultKind::Message)
            .map(|result| result.created_at)
            .min()
    }

    /// Append an older page, keeping every row already shown in place.
    /// Returns how many rows were new.
    pub fn append_older(&mut self, page: Vec<SearchResult>) -> usize {
        let before = self.results.len();
        for result in page {
            if !self
                .results
                .iter()
                .any(|shown| shown.stable_id == result.stable_id)
            {
                self.results.push(result);
            }
        }
        self.results.len() - before
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
    let status = if state.loading || state.older_loading {
        "searching…"
    } else if state.local_only {
        "offline · local results"
    } else {
        "local + relay"
    };
    let title = format!(
        " Search · {status} · from: in: after: before: · Enter open · Ctrl-o older · Esc close "
    );
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

#[cfg(test)]
mod tests {
    use super::SearchState;
    use crate::domain::{SearchResult, SearchResultKind};

    fn result(id: &str, kind: SearchResultKind, created_at: u64) -> SearchResult {
        SearchResult {
            stable_id: id.into(),
            kind,
            label: id.into(),
            detail: String::new(),
            channel_id: None,
            event_id: None,
            thread_root: None,
            pubkey: None,
            created_at,
            remote_rank: None,
        }
    }

    /// v0.14 M5: the next page is bounded by the oldest message shown;
    /// appending keeps every shown row in place and skips duplicates.
    #[test]
    fn older_pages_append_below_without_moving_rows() {
        let mut state = SearchState {
            results: vec![
                result("channel:x", SearchResultKind::Channel, 1),
                result("m1", SearchResultKind::Message, 300),
                result("m2", SearchResultKind::Message, 200),
            ],
            selected_id: Some("m2".into()),
            ..SearchState::default()
        };
        assert_eq!(state.oldest_message(), Some(200), "channels do not count");
        let added = state.append_older(vec![
            result("m2", SearchResultKind::Message, 200),
            result("m3", SearchResultKind::Message, 100),
        ]);
        assert_eq!(added, 1);
        let ids = state
            .results
            .iter()
            .map(|r| r.stable_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["channel:x", "m1", "m2", "m3"]);
        state.reconcile();
        assert_eq!(state.selected_id.as_deref(), Some("m2"), "selection stays");
        assert_eq!(state.oldest_message(), Some(100));
        state.older_loading = true;
        state.changed();
        assert!(!state.older_loading, "a new query cancels the older page");
    }
}
