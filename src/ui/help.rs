//! Generated, effective-keymap help for the interaction foundation.
//!
//! Help is derived from the validated typed map rather than a handwritten
//! duplicate of defaults, so scoped user overrides and disabled bindings stay
//! truthful without exposing the contents of `keymap.toml`. After the keys it
//! lists the `:` commands of [`crate::ui::command::COMMANDS`] (v0.17 C3).
//!
//! The help scrolls: [`help_lines`] wraps it to the window's width, so the
//! caller knows exactly how many rows there are, and [`HelpScroll`] keeps the
//! position.

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::ui::{
    actions::ContextAction,
    keymap::{KeyChord, KeyMap, KeyScope},
};

/// The keys the help itself answers, as its first lines say.
pub const HELP_KEYS_HINT: &str =
    "j/k or Down/Up scroll · PageDown/PageUp a page · g/G top and bottom · Esc or q closes help.";

/// One line of the help before wrapping: its text and, when it wraps, how
/// far its continuation is indented.
struct HelpLine {
    text: String,
    hang: usize,
}

impl HelpLine {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            hang: 0,
        }
    }

    /// `  key                  what it does`, its continuation under the
    /// description.
    fn entry(keys: &str, description: &str) -> Self {
        if keys.width() <= 20 {
            Self {
                text: format!("  {keys:<20} {description}"),
                hang: 23,
            }
        } else {
            Self {
                text: format!("  {keys} \u{2014} {description}"),
                hang: 6,
            }
        }
    }
}

fn help_content(
    keymap: &KeyMap,
    scope: KeyScope,
    contextual_actions: &[ContextAction],
) -> Vec<HelpLine> {
    let mut bindings = keymap
        .effective_bindings(scope)
        .into_iter()
        .map(|binding| (binding.sequence.label(), binding.action.label()))
        .collect::<Vec<_>>();
    bindings.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut lines = vec![
        HelpLine::plain("bzz effective keymap"),
        HelpLine::plain(String::new()),
        HelpLine::plain("The entries below include your active scoped overrides."),
        HelpLine::plain(HELP_KEYS_HINT),
        HelpLine::plain(String::new()),
    ];
    lines.extend(
        bindings
            .into_iter()
            .map(|(keys, action)| HelpLine::entry(&keys, action)),
    );
    let mut disabled = keymap
        .disabled_bindings(scope)
        .into_iter()
        .map(|sequence| sequence.label())
        .collect::<Vec<_>>();
    disabled.sort_unstable();
    if !disabled.is_empty() {
        lines.push(HelpLine::plain(String::new()));
        lines.push(HelpLine::plain("Disabled in this scope"));
        lines.extend(
            disabled
                .into_iter()
                .map(|keys| HelpLine::entry(&keys, "disabled")),
        );
    }
    if !contextual_actions.is_empty() {
        lines.push(HelpLine::plain(String::new()));
        lines.push(HelpLine::plain("Actions for focused item"));
        lines.extend(contextual_actions.iter().map(|entry| match entry.reason {
            Some(reason) => HelpLine::entry(entry.label, &format!("unavailable \u{2014} {reason}")),
            None => HelpLine::entry(entry.label, "available"),
        }));
    }
    lines.push(HelpLine::plain(String::new()));
    lines.push(HelpLine::plain(
        "Commands: press :, type one, Enter runs it",
    ));
    for spec in crate::ui::command::COMMANDS {
        let aliases = spec
            .aliases
            .iter()
            .map(|alias| format!(", :{alias}"))
            .collect::<String>();
        let usage = format!("{}{aliases}", spec.usage());
        if usage.width() <= 20 {
            lines.push(HelpLine::entry(&usage, spec.summary));
        } else {
            lines.push(HelpLine {
                text: format!("  {usage}"),
                hang: 4,
            });
            lines.push(HelpLine {
                text: format!("  {:<20} {}", "", spec.summary),
                hang: 23,
            });
        }
    }
    lines
}

pub fn effective_keymap(keymap: &KeyMap, scope: KeyScope) -> String {
    effective_keymap_with_actions(keymap, scope, &[])
}

pub fn effective_keymap_with_actions(
    keymap: &KeyMap,
    scope: KeyScope,
    contextual_actions: &[ContextAction],
) -> String {
    help_content(keymap, scope, contextual_actions)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The help as rows of at most `width` cells: what is drawn, one row each,
/// so that scrolling reaches exactly the last row.
pub fn help_lines(
    keymap: &KeyMap,
    scope: KeyScope,
    contextual_actions: &[ContextAction],
    width: usize,
) -> Vec<String> {
    help_content(keymap, scope, contextual_actions)
        .into_iter()
        .flat_map(|line| wrap(&line.text, width, line.hang))
        .collect()
}

/// Word-wrap a line of prose to `width` cells, continuation rows indented
/// by two (the `:` prompt's answer, v0.17 C4).
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap(text, width, 2)
}

/// Word-wrap one line to `width` cells; continuation rows start `hang`
/// cells in. A word longer than a row is cut.
fn wrap(text: &str, width: usize, hang: usize) -> Vec<String> {
    let width = width.max(1);
    if text.width() <= width {
        return vec![text.to_owned()];
    }
    let hang = if hang * 2 >= width { 0 } else { hang };
    let mut rows = Vec::new();
    let indent = text.len() - text.trim_start().len();
    let mut row = text[..indent].to_owned();
    let mut row_width = row.width();
    let mut row_has_word = false;
    for word in text[indent..].split(' ') {
        let word_width = word.width();
        let needed = word_width + usize::from(row_has_word);
        if row_has_word && row_width + needed > width {
            rows.push(std::mem::take(&mut row));
            row = " ".repeat(hang);
            row_width = hang;
            row_has_word = false;
        }
        if row_has_word {
            row.push(' ');
            row_width += 1;
        }
        // A word wider than a whole row: cut it.
        for character in word.chars() {
            let cells = character.width().unwrap_or(0);
            if row_width + cells > width && row_width > hang {
                rows.push(std::mem::take(&mut row));
                row = " ".repeat(hang);
                row_width = hang;
            }
            row.push(character);
            row_width += cells;
        }
        row_has_word = true;
    }
    rows.push(row);
    rows
}

/// Where the help is scrolled to. The caller sets the page from what it
/// drew; the keys move within it (v0.17 C3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HelpScroll {
    /// The first row shown.
    pub offset: usize,
    /// How many rows the help has, wrapped to the last width drawn.
    pub total: usize,
    /// How many rows the window shows.
    pub page: usize,
}

impl HelpScroll {
    pub const fn max_offset(&self) -> usize {
        self.total.saturating_sub(self.page)
    }

    /// Remember what was drawn; the offset stays within it.
    pub fn set_view(&mut self, total: usize, page: usize) {
        self.total = total;
        self.page = page;
        self.offset = self.offset.min(self.max_offset());
    }

    pub fn by(&mut self, rows: isize) {
        self.offset = self
            .offset
            .saturating_add_signed(rows)
            .min(self.max_offset());
    }

    pub fn page_by(&mut self, pages: isize) {
        let rows = isize::try_from(self.page.saturating_sub(1).max(1)).unwrap_or(1);
        self.by(rows.saturating_mul(pages));
    }

    pub const fn top(&mut self) {
        self.offset = 0;
    }

    pub const fn bottom(&mut self) {
        self.offset = self.max_offset();
    }

    /// ` effective keymap · rows 1-22 of 120 `, for the frame's title.
    pub fn title(&self) -> String {
        if self.total <= self.page {
            " effective keymap ".to_owned()
        } else {
            format!(
                " effective keymap \u{b7} rows {}-{} of {} \u{b7} j/k PageDown/PageUp ",
                self.offset + 1,
                (self.offset + self.page).min(self.total),
                self.total
            )
        }
    }
}

pub fn which_key(keymap: &KeyMap, scope: KeyScope, prefix: &[KeyChord]) -> String {
    let mut hints = keymap
        .next_chords(scope, prefix)
        .into_iter()
        .map(|chord| {
            let mut sequence = prefix.to_vec();
            sequence.push(chord);
            let label = match keymap.lookup(scope, &sequence) {
                crate::ui::keymap::KeyLookup::Action(action) => action.label(),
                crate::ui::keymap::KeyLookup::Pending => "continue sequence",
                crate::ui::keymap::KeyLookup::NoMatch => "",
            };
            (chord.label(), label)
        })
        .collect::<Vec<_>>();
    hints.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let prefix = prefix
        .iter()
        .map(|chord| chord.label())
        .collect::<Vec<_>>()
        .join(" ");
    let mut lines = vec![format!(" {prefix} · next key "), String::new()];
    lines.extend(
        hints
            .into_iter()
            .map(|(key, action)| format!("  {key:<12} {action}")),
    );
    lines.push(String::new());
    lines.push("Esc cancels".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::ui::{
        actions::ContextAction,
        keymap::{KeyChord, KeyMap, KeyScope, UiAction},
    };

    use super::{
        HelpScroll, effective_keymap, effective_keymap_with_actions, help_lines, which_key,
    };

    #[test]
    fn help_is_derived_from_the_effective_scoped_map() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'workspace'\nkeys = ['?']\ndisabled = true\n\
             [[binding]]\nscope = 'workspace'\nkeys = ['x']\naction = 'open-inbox'\n",
        )
        .unwrap();
        let keymap = KeyMap::load_from(&path).unwrap();
        let help = effective_keymap(&keymap, KeyScope::Workspace);
        assert!(help.contains("x                    open Inbox"));
        assert!(!help.contains("show keymap help"));
        assert!(help.contains("Disabled in this scope"));
        assert!(help.contains("?                    disabled"));
    }

    #[test]
    fn help_lists_enabled_and_unavailable_context_actions() {
        let output = effective_keymap_with_actions(
            &KeyMap::builtin(),
            KeyScope::Workspace,
            &[
                ContextAction {
                    action: UiAction::Compose,
                    label: "reply",
                    enabled: true,
                    reason: None,
                },
                ContextAction {
                    action: UiAction::Delete,
                    label: "delete own message",
                    enabled: false,
                    reason: Some("only your own message can be deleted"),
                },
            ],
        );
        assert!(output.contains("Actions for focused item"));
        assert!(output.contains("reply                available"));
        assert!(output.contains("unavailable — only your own message can be deleted"));
    }

    #[test]
    fn which_key_labels_only_valid_continuations() {
        let keymap = KeyMap::builtin();
        let output = which_key(
            &keymap,
            KeyScope::Workspace,
            &[KeyChord::new(
                crossterm::event::KeyCode::Char(' '),
                crossterm::event::KeyModifiers::NONE,
            )],
        );
        assert!(output.contains("n            open Inbox"));
        assert!(output.contains("Space        channel / DM switcher"));
        assert!(!output.contains("refresh\n  z"));
    }

    /// v0.17 C3: the help lists the `:` commands with their arguments after
    /// the keys, wraps to the window, and scrolls to its last row.
    #[test]
    fn help_lists_commands_after_the_keys_and_scrolls_to_the_end() {
        let keymap = KeyMap::builtin();
        let text = effective_keymap(&keymap, KeyScope::Workspace);
        let keys = text.find("jump to last").unwrap();
        let commands = text.find("Commands: press :").unwrap();
        assert!(keys < commands);
        for usage in [
            ":help",
            ":quit, :q",
            ":huddle [start | join",
            ":status [[--expires",
        ] {
            assert!(text.contains(usage), "{usage}");
        }
        // 80 columns less the frame's two.
        let lines = help_lines(&keymap, KeyScope::Workspace, &[], 78);
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 78)
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("y ") && line.contains("copy"))
        );
        let mut scroll = HelpScroll::default();
        // 24 rows: the status line, the frame's two borders.
        scroll.set_view(lines.len(), 21);
        assert!(scroll.total > scroll.page, "the help is longer than 80x24");
        for _ in 0..lines.len() {
            scroll.by(1);
        }
        let shown = &lines[scroll.offset..scroll.offset + scroll.page];
        assert_eq!(shown.last(), lines.last());
        assert!(shown.last().unwrap().contains("local cache"));
        scroll.top();
        scroll.page_by(1);
        assert_eq!(scroll.offset, 20);
        scroll.bottom();
        assert_eq!(scroll.offset, lines.len() - 21);
        assert!(scroll.title().contains(&format!("of {}", lines.len())));
    }
}
