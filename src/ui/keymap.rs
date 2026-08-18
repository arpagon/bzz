use std::{fs, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

use crate::{
    error::{Error, Result},
    paths::Paths,
};

const MAX_KEYMAP_BYTES: u64 = 64 * 1024;
const MAX_BINDINGS: usize = 128;
const MAX_SEQUENCE_CHORDS: usize = 4;

/// A named UI intent. The keyboard/mouse router resolves configuration into one
/// of these values before dispatch, so input handling never evaluates an
/// arbitrary configuration string at runtime.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum UiAction {
    BackOrQuit,
    OpenHelp,
    ToggleCommunities,
    ToggleChannels,
    ToggleContext,
    FocusCommunities,
    FocusChannels,
    FocusTimeline,
    FocusContext,
    NextFocus,
    PreviousFocus,
    SelectPrevious,
    SelectNext,
    ScrollViewportUp,
    ScrollViewportDown,
    HalfPageUp,
    HalfPageDown,
    JumpTop,
    JumpBottom,
    ActivateFocused,
    Compose,
    Filter,
    Search,
    OpenInbox,
    ChannelSwitcher,
    OpenContextActions,
    Refresh,
    OpenOptions,
    OpenCommand,
    NewDm,
    ToggleThread,
    React,
    Delete,
    MarkUnread,
    MarkRead,
    Preview,
    Submit,
    InsertNewline,
    Complete,
    DeletePreviousWord,
    DeleteToStart,
    DeleteToEnd,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
}

/// The part of the TUI that owns a binding. Scopes deliberately distinguish
/// persistent routes from text-owning states, so a user keymap cannot make a
/// printable composer character execute a workspace action.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyScope {
    #[default]
    Global,
    Workspace,
    Inbox,
    Composer,
    Filter,
    Overlay,
}

impl KeyScope {
    const ALL: [Self; 6] = [
        Self::Global,
        Self::Workspace,
        Self::Inbox,
        Self::Composer,
        Self::Filter,
        Self::Overlay,
    ];

    fn owns_text(self) -> bool {
        matches!(self, Self::Composer | Self::Filter)
    }
}

/// One terminal chord, represented independently of Crossterm's event type so
/// it can be parsed and checked before raw mode is entered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyChord {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn from_event(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }

    pub fn matches(self, event: KeyEvent) -> bool {
        self.code == event.code && self.modifiers == event.modifiers
    }

    fn parse(value: &str) -> std::result::Result<Self, ()> {
        let value = value.trim();
        if value.is_empty() || value.len() > 32 || value.chars().any(char::is_control) {
            return Err(());
        }
        let mut modifiers = KeyModifiers::NONE;
        let mut pieces = value.split('-').peekable();
        let mut key = None;
        while let Some(piece) = pieces.next() {
            let piece = piece.trim();
            if piece.is_empty() {
                return Err(());
            }
            let is_last = pieces.peek().is_none();
            if !is_last {
                let modifier = match piece.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => KeyModifiers::CONTROL,
                    "alt" => KeyModifiers::ALT,
                    "shift" => KeyModifiers::SHIFT,
                    _ => return Err(()),
                };
                if modifiers.contains(modifier) {
                    return Err(());
                }
                modifiers.insert(modifier);
                continue;
            }
            let lower = piece.to_ascii_lowercase();
            let code = match lower.as_str() {
                "space" => KeyCode::Char(' '),
                "tab" => KeyCode::Tab,
                "backtab" => KeyCode::BackTab,
                "enter" | "return" => KeyCode::Enter,
                "esc" | "escape" => KeyCode::Esc,
                "backspace" => KeyCode::Backspace,
                "delete" | "del" => KeyCode::Delete,
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                "pageup" | "page-up" => KeyCode::PageUp,
                "pagedown" | "page-down" => KeyCode::PageDown,
                _ => {
                    let mut characters = piece.chars();
                    let Some(character) = characters.next() else {
                        return Err(());
                    };
                    if characters.next().is_some() || character.is_control() {
                        return Err(());
                    }
                    if character.is_ascii_uppercase() {
                        modifiers.insert(KeyModifiers::SHIFT);
                    }
                    KeyCode::Char(character)
                }
            };
            key = Some(code);
        }
        key.map(|code| Self { code, modifiers }).ok_or(())
    }

    fn is_plain_printable(self) -> bool {
        matches!(self.code, KeyCode::Char(character) if !character.is_control())
            && self.modifiers.is_empty()
    }
}

/// A short, bounded key sequence. All sequence matching happens through this
/// typed value, rather than through strings held by the application state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeySequence(Vec<KeyChord>);

impl KeySequence {
    pub fn chords(&self) -> &[KeyChord] {
        &self.0
    }

    fn parse(values: &[String]) -> std::result::Result<Self, ()> {
        if values.is_empty() || values.len() > MAX_SEQUENCE_CHORDS {
            return Err(());
        }
        let chords = values
            .iter()
            .map(|value| KeyChord::parse(value))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self(chords))
    }

    fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyBinding {
    pub scope: KeyScope,
    pub sequence: KeySequence,
    pub action: UiAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyLookup {
    NoMatch,
    Pending,
    Action(UiAction),
}

/// A validated effective keymap. A scope inherits global bindings, then
/// replaces exact sequences with bindings from that scope. This lets Inbox or
/// overlay bindings intentionally override a workspace default without
/// accidentally leaking to text input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyMap {
    bindings: Vec<KeyBinding>,
}

impl KeyMap {
    pub fn builtin() -> Self {
        let mut bindings = Vec::new();
        let mut add = |scope, keys: &[&str], action| {
            bindings.push(KeyBinding {
                scope,
                sequence: KeySequence(
                    keys.iter()
                        .map(|key| KeyChord::parse(key).expect("builtin key chord is valid"))
                        .collect(),
                ),
                action,
            });
        };

        add(KeyScope::Global, &["?"], UiAction::OpenHelp);
        add(KeyScope::Global, &["q"], UiAction::BackOrQuit);
        add(KeyScope::Global, &["esc"], UiAction::BackOrQuit);
        add(KeyScope::Global, &["1"], UiAction::FocusCommunities);
        add(KeyScope::Global, &["2"], UiAction::FocusChannels);
        add(KeyScope::Global, &["3"], UiAction::FocusTimeline);
        add(KeyScope::Global, &["4"], UiAction::FocusContext);
        add(KeyScope::Global, &["tab"], UiAction::NextFocus);
        add(KeyScope::Global, &["backtab"], UiAction::PreviousFocus);
        add(KeyScope::Global, &["h"], UiAction::PreviousFocus);
        add(KeyScope::Global, &["l"], UiAction::NextFocus);
        add(KeyScope::Global, &["j"], UiAction::SelectNext);
        add(KeyScope::Global, &["down"], UiAction::SelectNext);
        add(KeyScope::Global, &["ctrl-n"], UiAction::SelectNext);
        add(KeyScope::Global, &["k"], UiAction::SelectPrevious);
        add(KeyScope::Global, &["up"], UiAction::SelectPrevious);
        add(KeyScope::Global, &["ctrl-p"], UiAction::SelectPrevious);
        add(KeyScope::Global, &["J"], UiAction::ScrollViewportDown);
        add(KeyScope::Global, &["K"], UiAction::ScrollViewportUp);
        add(KeyScope::Global, &["ctrl-d"], UiAction::HalfPageDown);
        add(KeyScope::Global, &["ctrl-u"], UiAction::HalfPageUp);
        add(KeyScope::Global, &["g", "g"], UiAction::JumpTop);
        add(KeyScope::Global, &["G"], UiAction::JumpBottom);
        add(KeyScope::Global, &["enter"], UiAction::ActivateFocused);
        add(KeyScope::Global, &["i"], UiAction::Compose);
        add(KeyScope::Global, &["/"], UiAction::Filter);
        add(
            KeyScope::Global,
            &["space", "space"],
            UiAction::ChannelSwitcher,
        );
        add(KeyScope::Global, &["space", "n"], UiAction::OpenInbox);
        add(
            KeyScope::Global,
            &["space", "a"],
            UiAction::OpenContextActions,
        );
        add(
            KeyScope::Global,
            &["space", "1"],
            UiAction::ToggleCommunities,
        );
        add(KeyScope::Global, &["space", "2"], UiAction::ToggleChannels);
        add(KeyScope::Global, &["space", "4"], UiAction::ToggleContext);
        add(KeyScope::Global, &["space", "r"], UiAction::Refresh);
        add(KeyScope::Global, &["space", "o"], UiAction::OpenOptions);
        add(KeyScope::Global, &[":"], UiAction::OpenCommand);

        add(KeyScope::Composer, &["esc"], UiAction::BackOrQuit);
        add(KeyScope::Composer, &["enter"], UiAction::Submit);
        add(KeyScope::Composer, &["ctrl-j"], UiAction::InsertNewline);
        add(KeyScope::Composer, &["alt-enter"], UiAction::InsertNewline);
        add(KeyScope::Composer, &["tab"], UiAction::Complete);
        add(
            KeyScope::Composer,
            &["ctrl-w"],
            UiAction::DeletePreviousWord,
        );
        add(KeyScope::Composer, &["ctrl-u"], UiAction::DeleteToStart);
        add(KeyScope::Composer, &["ctrl-k"], UiAction::DeleteToEnd);
        add(KeyScope::Composer, &["ctrl-left"], UiAction::MoveWordLeft);
        add(KeyScope::Composer, &["ctrl-right"], UiAction::MoveWordRight);
        add(KeyScope::Composer, &["home"], UiAction::MoveLineStart);
        add(KeyScope::Composer, &["end"], UiAction::MoveLineEnd);

        let keymap = Self { bindings };
        keymap
            .validate()
            .expect("builtin keymap must be internally valid");
        keymap
    }

    /// Load the optional owner-private `keymap.toml`. A missing file means
    /// builtins; a present but invalid file fails before terminal setup.
    pub fn load(paths: &Paths) -> Result<Self> {
        Self::load_from(&paths.keymap_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::builtin());
        }
        let metadata = fs::metadata(path).map_err(|error| Error::io(path, error))?;
        if metadata.len() > MAX_KEYMAP_BYTES {
            return Err(Error::Config(format!(
                "{} exceeds the 64 KiB keymap limit",
                path.display()
            )));
        }
        let input = fs::read_to_string(path).map_err(|error| Error::io(path, error))?;
        let file: KeymapFile = toml::from_str(&input).map_err(|_| {
            Error::Config(format!(
                "{} contains invalid or unknown keymap settings",
                path.display()
            ))
        })?;
        if file.binding.len() > MAX_BINDINGS {
            return Err(Error::Config(format!(
                "{} exceeds the {}-binding keymap limit",
                path.display(),
                MAX_BINDINGS
            )));
        }
        let mut keymap = Self::builtin();
        for binding in file.binding {
            let sequence = KeySequence::parse(&binding.keys).map_err(|_| {
                Error::Config(format!(
                    "{} contains an invalid key sequence",
                    path.display()
                ))
            })?;
            if binding.disabled {
                if binding.action.is_some() {
                    return Err(Error::Config(format!(
                        "{} cannot disable a sequence and assign it an action",
                        path.display()
                    )));
                }
                keymap.bindings.retain(|candidate| {
                    !(candidate.scope == binding.scope && candidate.sequence == sequence)
                });
                continue;
            }
            let Some(action) = binding.action else {
                return Err(Error::Config(format!(
                    "{} assigns no action to a key sequence",
                    path.display()
                )));
            };
            keymap.bindings.retain(|candidate| {
                !(candidate.scope == binding.scope && candidate.sequence == sequence)
            });
            keymap.bindings.push(KeyBinding {
                scope: binding.scope,
                sequence,
                action,
            });
        }
        keymap.validate()?;
        Ok(keymap)
    }

    pub fn lookup(&self, scope: KeyScope, sequence: &[KeyChord]) -> KeyLookup {
        let bindings = self.effective_bindings(scope);
        let mut has_prefix = false;
        for binding in &bindings {
            if binding.sequence.chords().starts_with(sequence) {
                if binding.sequence.chords().len() == sequence.len() {
                    return KeyLookup::Action(binding.action);
                }
                has_prefix = true;
            }
        }
        if has_prefix {
            KeyLookup::Pending
        } else {
            KeyLookup::NoMatch
        }
    }

    pub fn next_chords(&self, scope: KeyScope, prefix: &[KeyChord]) -> Vec<KeyChord> {
        let mut next = Vec::new();
        for binding in self.effective_bindings(scope) {
            if binding.sequence.chords().starts_with(prefix)
                && binding.sequence.chords().len() > prefix.len()
            {
                let chord = binding.sequence.chords()[prefix.len()];
                if !next.contains(&chord) {
                    next.push(chord);
                }
            }
        }
        next
    }

    pub fn effective_bindings(&self, scope: KeyScope) -> Vec<&KeyBinding> {
        // Text-owning states deliberately do not inherit global bindings:
        // literal `i`, `Space`, `j`, etc. must remain text. They carry their
        // own tightly constrained edit map instead.
        let mut effective = if scope.owns_text() {
            Vec::new()
        } else {
            self.bindings
                .iter()
                .filter(|binding| binding.scope == KeyScope::Global)
                .collect::<Vec<_>>()
        };
        if scope == KeyScope::Global {
            return effective;
        }
        for binding in self
            .bindings
            .iter()
            .filter(|binding| binding.scope == scope)
        {
            effective.retain(|candidate| candidate.sequence != binding.sequence);
            effective.push(binding);
        }
        effective
    }

    pub fn validate(&self) -> Result<()> {
        if self.bindings.len() > MAX_BINDINGS.saturating_mul(KeyScope::ALL.len()) {
            return Err(Error::Config(
                "keymap has too many effective bindings".into(),
            ));
        }
        for scope in KeyScope::ALL {
            let bindings = self.effective_bindings(scope);
            for binding in &bindings {
                if binding.sequence.chords().is_empty()
                    || binding.sequence.chords().len() > MAX_SEQUENCE_CHORDS
                {
                    return Err(Error::Config(
                        "keymap has an invalid sequence length".into(),
                    ));
                }
                if scope.owns_text()
                    && binding
                        .sequence
                        .chords()
                        .first()
                        .is_some_and(|chord| chord.is_plain_printable())
                {
                    return Err(Error::Config(
                        "keymap cannot bind a printable character in a text-owning scope".into(),
                    ));
                }
                if scope == KeyScope::Composer && !composer_action(binding.action) {
                    return Err(Error::Config(
                        "keymap composer scope permits only composer editing actions".into(),
                    ));
                }
            }
            for (index, left) in bindings.iter().enumerate() {
                for right in bindings.iter().skip(index + 1) {
                    if left.sequence == right.sequence {
                        return Err(Error::Config(
                            "keymap assigns multiple actions to one effective sequence".into(),
                        ));
                    }
                    if left.sequence.starts_with(&right.sequence)
                        || right.sequence.starts_with(&left.sequence)
                    {
                        return Err(Error::Config(
                            "keymap has an action/prefix sequence ambiguity".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn composer_action(action: UiAction) -> bool {
    matches!(
        action,
        UiAction::BackOrQuit
            | UiAction::Submit
            | UiAction::InsertNewline
            | UiAction::Complete
            | UiAction::DeletePreviousWord
            | UiAction::DeleteToStart
            | UiAction::DeleteToEnd
            | UiAction::MoveWordLeft
            | UiAction::MoveWordRight
            | UiAction::MoveLineStart
            | UiAction::MoveLineEnd
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    #[serde(default)]
    binding: Vec<KeymapFileBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFileBinding {
    #[serde(default)]
    scope: KeyScope,
    keys: Vec<String>,
    #[serde(default)]
    action: Option<UiAction>,
    #[serde(default)]
    disabled: bool,
}

// The v0.3 normal and insert mappers remain while M1 migrates the event router
// one vertical slice at a time. New paths must use `KeyMap` above; these are
// removed once all old Mode branches are cut over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAction {
    Quit,
    Help,
    Up,
    Down,
    First,
    Last,
    PageUp,
    PageDown,
    Open,
    Compose,
    Thread,
    React,
    Delete,
    MarkUnread,
    ToggleSidebar,
    NextPane,
    PreviousPane,
    Finder,
    Search,
    Inbox,
    NewDm,
    HideDm,
    AddDmMember,
    Theme,
    Preview,
    Attach,
    RemoveAttachment,
    RetryAttachments,
    Command,
    Escape,
    Character(char),
    Backspace,
    ForwardDelete,
    Left,
    Right,
    Complete,
    Submit,
    Newline,
    Ignore,
}

pub fn map_normal(key: KeyEvent, awaiting_g: bool) -> KeyAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => KeyAction::Quit,
            KeyCode::Char('t' | 'p') => KeyAction::Finder,
            KeyCode::Char('b') => KeyAction::ToggleSidebar,
            KeyCode::Char(']') => KeyAction::Thread,
            KeyCode::Char('u') => KeyAction::PageUp,
            KeyCode::Char('d') => KeyAction::PageDown,
            KeyCode::Char('y') => KeyAction::Theme,
            KeyCode::Char('n') => KeyAction::NewDm,
            _ => KeyAction::Ignore,
        };
    }
    match key.code {
        KeyCode::Char('Q') => KeyAction::Quit,
        KeyCode::Char('?') => KeyAction::Help,
        KeyCode::Char('j') | KeyCode::Down => KeyAction::Down,
        KeyCode::Char('k') | KeyCode::Up => KeyAction::Up,
        KeyCode::Char('g') if awaiting_g => KeyAction::First,
        KeyCode::Char('G') => KeyAction::Last,
        KeyCode::PageUp => KeyAction::PageUp,
        KeyCode::PageDown => KeyAction::PageDown,
        KeyCode::Enter => KeyAction::Open,
        KeyCode::Char('i') => KeyAction::Compose,
        KeyCode::Char('r') => KeyAction::React,
        KeyCode::Char('p') => KeyAction::Preview,
        KeyCode::Char('/') => KeyAction::Search,
        KeyCode::Char('I') => KeyAction::Inbox,
        KeyCode::Char('H') => KeyAction::HideDm,
        KeyCode::Char('A') => KeyAction::AddDmMember,
        KeyCode::Char('D') => KeyAction::Delete,
        KeyCode::Char('U') => KeyAction::MarkUnread,
        KeyCode::Tab => KeyAction::NextPane,
        KeyCode::BackTab => KeyAction::PreviousPane,
        KeyCode::Char(':') => KeyAction::Command,
        KeyCode::Esc => KeyAction::Escape,
        _ => KeyAction::Ignore,
    }
}

pub fn map_insert(key: KeyEvent) -> KeyAction {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => KeyAction::Escape,
        (KeyModifiers::ALT, KeyCode::Enter) => KeyAction::Newline,
        (KeyModifiers::CONTROL, KeyCode::Char('j')) => KeyAction::Newline,
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Enter) => KeyAction::Submit,
        (KeyModifiers::CONTROL, KeyCode::Char('a')) => KeyAction::Attach,
        (KeyModifiers::CONTROL, KeyCode::Char('x')) => KeyAction::RemoveAttachment,
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => KeyAction::RetryAttachments,
        (_, KeyCode::Backspace) => KeyAction::Backspace,
        (_, KeyCode::Delete) => KeyAction::ForwardDelete,
        (_, KeyCode::Left) => KeyAction::Left,
        (_, KeyCode::Right) => KeyAction::Right,
        (_, KeyCode::Up) => KeyAction::Up,
        (_, KeyCode::Down) => KeyAction::Down,
        (_, KeyCode::Tab) => KeyAction::Complete,
        (modifiers, KeyCode::Char(character))
            if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
        {
            KeyAction::Character(character)
        }
        _ => KeyAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tempfile::TempDir;

    use super::{KeyChord, KeyLookup, KeyMap, KeyScope, UiAction};

    #[test]
    fn builtin_leader_sequences_are_typed_and_discoverable() {
        let keymap = KeyMap::builtin();
        let space = KeyChord::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let n = KeyChord::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(
            keymap.lookup(KeyScope::Workspace, &[space]),
            KeyLookup::Pending
        );
        assert_eq!(
            keymap.lookup(KeyScope::Workspace, &[space, n]),
            KeyLookup::Action(UiAction::OpenInbox)
        );
        assert!(
            keymap
                .next_chords(KeyScope::Workspace, &[space])
                .contains(&n)
        );
    }

    #[test]
    fn defaults_use_concord_style_navigation_without_legacy_conflicts() {
        let keymap = KeyMap::builtin();
        let previous = KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        let inbox = KeyChord::new(KeyCode::Char('I'), KeyModifiers::SHIFT);
        assert_eq!(
            keymap.lookup(KeyScope::Workspace, &[previous]),
            KeyLookup::Action(UiAction::SelectPrevious)
        );
        assert_eq!(
            keymap.lookup(KeyScope::Workspace, &[inbox]),
            KeyLookup::NoMatch
        );
    }

    #[test]
    fn scoped_user_binding_overrides_an_exact_global_sequence() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'inbox'\nkeys = ['space', 'n']\naction = 'mark-read'\n",
        )
        .unwrap();
        let keymap = KeyMap::load_from(&path).unwrap();
        let space = KeyChord::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let n = KeyChord::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(
            keymap.lookup(KeyScope::Workspace, &[space, n]),
            KeyLookup::Action(UiAction::OpenInbox)
        );
        assert_eq!(
            keymap.lookup(KeyScope::Inbox, &[space, n]),
            KeyLookup::Action(UiAction::MarkRead)
        );
    }

    #[test]
    fn invalid_maps_fail_closed_without_echoing_content() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        let hostile = "nsec1never-echo-this";
        fs::write(
            &path,
            format!(
                "[[binding]]\nscope = 'composer'\nkeys = ['x']\naction = 'submit'\n# {hostile}\n"
            ),
        )
        .unwrap();
        let error = KeyMap::load_from(&path).unwrap_err().to_string();
        assert!(!error.contains(hostile));
    }

    #[test]
    fn action_prefix_ambiguity_is_rejected() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(&path, "[[binding]]\nkeys = ['g']\naction = 'refresh'\n").unwrap();
        assert!(KeyMap::load_from(&path).is_err());
    }

    #[test]
    fn chord_matching_requires_exact_modifiers() {
        let chord = KeyChord::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert!(chord.matches(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)));
        assert!(!chord.matches(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
    }
}
