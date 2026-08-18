use super::theme::{ThemeEntry, ThemeRegistry, ThemeScope};

#[derive(Clone, Debug)]
pub struct ThemePicker {
    entries: Vec<ThemeEntry>,
    filtered: Vec<usize>,
    query: String,
    selected: usize,
    scope: ThemeScope,
}

impl ThemePicker {
    pub fn open(selected_id: &str, scope: ThemeScope) -> Self {
        let entries = ThemeRegistry::entries().collect::<Vec<_>>();
        let selected_entry = entries
            .iter()
            .position(|entry| entry.id == selected_id)
            .unwrap_or_default();
        let mut picker = Self {
            entries,
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
            scope,
        };
        picker.filter();
        picker.selected = picker
            .filtered
            .iter()
            .position(|index| *index == selected_entry)
            .unwrap_or_default();
        picker
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn scope(&self) -> ThemeScope {
        self.scope
    }

    pub fn toggle_scope(&mut self) {
        self.scope = match self.scope {
            ThemeScope::Global => ThemeScope::Community,
            ThemeScope::Community => ThemeScope::Global,
        };
    }

    pub fn push(&mut self, character: char) {
        if !character.is_control() && self.query.chars().count() < 80 {
            self.query.push(character);
            self.selected = 0;
            self.filter();
        }
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.filter();
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.filtered.len() - 1);
    }

    pub fn selected(&self) -> Option<ThemeEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
            .copied()
    }

    pub fn select_id(&mut self, id: &str) -> bool {
        let Some(selected) = self
            .filtered
            .iter()
            .position(|index| self.entries.get(*index).is_some_and(|entry| entry.id == id))
        else {
            return false;
        };
        self.selected = selected;
        true
    }

    pub fn visible(&self, limit: usize) -> Vec<(ThemeEntry, bool)> {
        if self.filtered.is_empty() || limit == 0 {
            return Vec::new();
        }
        let start = self
            .selected
            .saturating_sub(limit.saturating_sub(1) / 2)
            .min(self.filtered.len().saturating_sub(limit));
        self.filtered[start..self.filtered.len().min(start + limit)]
            .iter()
            .enumerate()
            .filter_map(|(offset, index)| {
                self.entries
                    .get(*index)
                    .copied()
                    .map(|entry| (entry, start + offset == self.selected))
            })
            .collect()
    }

    fn filter(&mut self) {
        let query = fold(&self.query);
        let mut prefix = Vec::new();
        let mut substring = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let id = fold(entry.id);
            let name = fold(entry.name);
            if query.is_empty() || id.starts_with(&query) || name.starts_with(&query) {
                prefix.push(index);
            } else if id.contains(&query) || name.contains(&query) {
                substring.push(index);
            }
        }
        prefix.extend(substring);
        self.filtered = prefix;
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }
}

fn fold(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::ThemePicker;
    use crate::ui::theme::ThemeScope;

    #[test]
    fn filters_moves_and_toggles_scope() {
        let mut picker = ThemePicker::open("bzz", ThemeScope::Global);
        for character in "nord".chars() {
            picker.push(character);
        }
        assert_eq!(picker.selected().unwrap().id, "nord");
        picker.toggle_scope();
        assert_eq!(picker.scope(), ThemeScope::Community);
        picker.backspace();
        assert!(!picker.visible(12).is_empty());
    }
}
