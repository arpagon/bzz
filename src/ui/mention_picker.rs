use std::ops::Range;

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::{
    domain::MentionCandidate,
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentionPicker {
    pub range: Range<usize>,
    pub query: String,
    pub candidates: Vec<MentionCandidate>,
    pub selected: usize,
}

impl MentionPicker {
    pub fn new(range: Range<usize>, query: String, candidates: Vec<MentionCandidate>) -> Self {
        Self {
            range,
            query,
            candidates,
            selected: 0,
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.candidates.len() - 1);
    }

    pub fn selected(&self) -> Option<&MentionCandidate> {
        self.candidates.get(self.selected)
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, picker: &MentionPicker, theme: &Theme) {
    frame.render_widget(Clear, area);
    let mut items = Vec::new();
    if picker.candidates.is_empty() {
        items.push(ListItem::new("No cached members match this mention."));
    } else {
        items.extend(picker.candidates.iter().map(|candidate| {
            ListItem::new(Line::from(format!(
                "@{}  {}",
                sanitize::single_line(&candidate.label),
                crate::domain::abbreviated_pubkey(&candidate.pubkey)
            )))
        }));
    }
    let selected = (!picker.candidates.is_empty()).then_some(picker.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(theme.style(HighlightGroup::SelectedRow))
            .style(theme.style(HighlightGroup::Normal))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme.border_type(BorderSurface::Picker))
                    .border_style(theme.style(HighlightGroup::ModalBorder))
                    .title_style(theme.style(HighlightGroup::ModalTitle))
                    .title(" mention · Up/Down select · Tab/Enter accept · Esc close "),
            ),
        area,
        &mut state,
    );
}
