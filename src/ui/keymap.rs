use std::{fs, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
    FocusApp,
    ZoomApp,
    NextFocus,
    PreviousFocus,
    ResizeFocusedNarrow,
    ResizeFocusedWide,
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
    CycleChannelSort,
    ChannelSwitcher,
    OpenContextActions,
    Refresh,
    OpenOptions,
    OpenCommand,
    NewDm,
    ToggleThread,
    ToggleCopySelection,
    CopyMessages,
    React,
    Delete,
    Edit,
    MarkUnread,
    ViewProfile,
    MarkRead,
    MarkVisibleRead,
    OpenCanonicalContext,
    Preview,
    Submit,
    InsertNewline,
    Complete,
    DeletePreviousWord,
    DeleteToStart,
    DeleteToEnd,
    ClearComposer,
    PasteClipboard,
    AttachFile,
    AttachPath,
    RemoveLastAttachment,
    RetryAttachments,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
    /// The palette: find and run any action (v0.17, D4).
    OpenPalette,
}

impl UiAction {
    /// Short bzz-owned label used by generated keymap help and which-key.
    pub const fn label(self) -> &'static str {
        match self {
            Self::BackOrQuit => "back / quit",
            Self::OpenHelp => "show keymap help",
            Self::ToggleCommunities => "toggle communities",
            Self::ToggleChannels => "toggle channels",
            Self::ToggleContext => "toggle context",
            Self::FocusCommunities => "focus communities",
            Self::FocusChannels => "focus channels",
            Self::FocusTimeline => "focus timeline",
            Self::FocusContext => "focus context",
            Self::FocusApp => "focus the app (Ctrl-c leaves it)",
            Self::ZoomApp => "zoom the app",
            Self::NextFocus => "next focus",
            Self::PreviousFocus => "previous focus",
            Self::ResizeFocusedNarrow => "narrow focused side pane",
            Self::ResizeFocusedWide => "widen focused side pane",
            Self::SelectPrevious => "previous selection",
            Self::SelectNext => "next selection",
            Self::ScrollViewportUp => "scroll viewport up",
            Self::ScrollViewportDown => "scroll viewport down",
            Self::HalfPageUp => "half page up",
            Self::HalfPageDown => "half page down",
            Self::JumpTop => "jump to first",
            Self::JumpBottom => "jump to last",
            Self::ActivateFocused => "activate focused item",
            Self::Compose => "compose",
            Self::Filter => "filter",
            Self::Search => "search",
            Self::OpenInbox => "open Inbox",
            Self::CycleChannelSort => "cycle channel sort",
            Self::ChannelSwitcher => "channel / DM switcher",
            Self::OpenContextActions => "contextual actions",
            Self::Refresh => "refresh",
            Self::OpenOptions => "options / theme",
            Self::OpenCommand => "command prompt",
            Self::NewDm => "new DM",
            Self::ToggleThread => "toggle context",
            Self::ToggleCopySelection => "start/cancel message selection",
            Self::CopyMessages => "copy selected message(s)",
            Self::React => "react",
            Self::Delete => "delete",
            Self::Edit => "edit own message",
            Self::MarkUnread => "mark unread",
            Self::ViewProfile => "view author profile",
            Self::MarkRead => "mark read",
            Self::MarkVisibleRead => "mark visible rows read",
            Self::OpenCanonicalContext => "open canonical context",
            Self::Preview => "preview media",
            Self::Submit => "send",
            Self::InsertNewline => "insert newline",
            Self::Complete => "complete",
            Self::DeletePreviousWord => "delete previous word",
            Self::DeleteToStart => "delete to line start",
            Self::DeleteToEnd => "delete to line end",
            Self::ClearComposer => "clear draft",
            Self::PasteClipboard => "paste text or attachments",
            Self::AttachFile => "choose local file(s)",
            Self::AttachPath => "attach by local path",
            Self::RemoveLastAttachment => "remove last attachment",
            Self::RetryAttachments => "retry failed attachments",
            Self::MoveWordLeft => "previous word",
            Self::MoveWordRight => "next word",
            Self::MoveLineStart => "line start",
            Self::MoveLineEnd => "line end",
            Self::OpenPalette => "find any action",
        }
    }
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

    pub fn from_event(event: KeyEvent) -> Self {
        let event = normalize_key_event(event);
        Self::new(event.code, event.modifiers)
    }

    /// A terminal-independent, bounded label for generated help. This is not
    /// parsed at dispatch time and never includes user configuration text.
    pub fn label(self) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift".to_owned());
        }
        let key = match self.code {
            KeyCode::Backspace => "Backspace".into(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Left => "Left".into(),
            KeyCode::Right => "Right".into(),
            KeyCode::Up => "Up".into(),
            KeyCode::Down => "Down".into(),
            KeyCode::Home => "Home".into(),
            KeyCode::End => "End".into(),
            KeyCode::PageUp => "PageUp".into(),
            KeyCode::PageDown => "PageDown".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::BackTab => "BackTab".into(),
            KeyCode::Delete => "Delete".into(),
            KeyCode::Insert => "Insert".into(),
            KeyCode::Esc => "Esc".into(),
            KeyCode::F(number) => format!("F{number}"),
            KeyCode::Char(' ') => "Space".into(),
            KeyCode::Char(character) => character.to_string(),
            _ => "key".into(),
        };
        parts.push(key);
        parts.join("+")
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
        key.map(|code| Self::written(code, modifiers)).ok_or(())
    }

    /// A chord as a key map writes it, in the form [`normalize_key_event`]
    /// gives a key: `shift-?` is `?`, `shift-a` is `A`, `shift-tab` is
    /// `backtab`. So a map written with or without `shift-` matches the key
    /// every terminal sends. A chord with Ctrl is kept as written.
    fn written(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let code = match code {
            KeyCode::Char(character)
                if modifiers.contains(KeyModifiers::SHIFT)
                    && !modifiers.contains(KeyModifiers::CONTROL)
                    && !character.is_uppercase() =>
            {
                let mut upper = character.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(upper), None) if upper != character => KeyCode::Char(upper),
                    _ => KeyCode::Char(character),
                }
            }
            KeyCode::Tab if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            other => other,
        };
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char(_)) {
            return Self::new(code, modifiers);
        }
        let event = normalize_key_event_on(KeyEvent::new(code, modifiers), false);
        Self::new(event.code, event.modifiers)
    }

    fn is_text_input(self) -> bool {
        matches!(self.code, KeyCode::Char(character) if !character.is_control())
            && !self
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
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

    pub fn label(&self) -> String {
        self.0
            .iter()
            .map(|chord| chord.label())
            .collect::<Vec<_>>()
            .join(" ")
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
    // Scope-local suppressions are retained separately so disabling a global
    // default in (for example) Inbox does not accidentally leave that global
    // binding effective in the Inbox scope.
    disabled: Vec<(KeyScope, KeySequence)>,
}

impl KeyMap {
    /// The built-in map, generated from the keys of the rows of
    /// [`crate::ui::action_table::TABLE`] (v0.17 C8, plan rule 23): each
    /// row's keys bind its [`ActionSpec::key_action`](crate::ui::action_table::ActionSpec::key_action).
    pub fn generated() -> Self {
        let mut bindings = Vec::new();
        for spec in crate::ui::action_table::TABLE {
            let Some(action) = spec.key_action() else {
                continue;
            };
            for (scope, keys) in spec.keys {
                bindings.push(KeyBinding {
                    scope: *scope,
                    sequence: KeySequence(
                        keys.iter()
                            .map(|key| KeyChord::parse(key).expect("builtin key chord is valid"))
                            .collect(),
                    ),
                    action,
                });
            }
        }
        let keymap = Self {
            bindings,
            disabled: Vec::new(),
        };
        keymap
            .validate()
            .expect("builtin keymap must be internally valid");
        keymap
    }

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
        add(KeyScope::Global, &["5"], UiAction::FocusApp);
        add(KeyScope::Global, &["tab"], UiAction::NextFocus);
        add(KeyScope::Global, &["backtab"], UiAction::PreviousFocus);
        add(KeyScope::Global, &["h"], UiAction::PreviousFocus);
        add(KeyScope::Global, &["l"], UiAction::NextFocus);
        add(KeyScope::Global, &["alt-h"], UiAction::ResizeFocusedNarrow);
        add(KeyScope::Global, &["alt-l"], UiAction::ResizeFocusedWide);
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
        add(KeyScope::Global, &["v"], UiAction::ToggleCopySelection);
        add(KeyScope::Global, &["y"], UiAction::CopyMessages);
        add(KeyScope::Global, &["r"], UiAction::React);
        add(KeyScope::Global, &["e"], UiAction::Edit);
        add(
            KeyScope::Global,
            &["space", "space"],
            UiAction::ChannelSwitcher,
        );
        add(KeyScope::Global, &["space", "n"], UiAction::OpenInbox);
        add(
            KeyScope::Global,
            &["space", "s"],
            UiAction::CycleChannelSort,
        );
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
        add(KeyScope::Global, &["space", "5"], UiAction::ZoomApp);
        add(KeyScope::Global, &["space", "r"], UiAction::Refresh);
        add(KeyScope::Global, &["space", "p"], UiAction::ViewProfile);
        add(KeyScope::Global, &["space", "o"], UiAction::OpenOptions);
        add(KeyScope::Global, &[":"], UiAction::OpenCommand);
        // v0.17 D4: the palette. Text-owning scopes do not inherit these,
        // so Ctrl-K in the composer keeps deleting to the line end.
        add(KeyScope::Global, &["ctrl-k"], UiAction::OpenPalette);
        add(KeyScope::Global, &["space", "k"], UiAction::OpenPalette);

        // Overlays never inherit workspace actions: their owner explicitly
        // decides which small set of close/confirm keys it accepts.
        add(KeyScope::Overlay, &["esc"], UiAction::BackOrQuit);
        add(KeyScope::Overlay, &["q"], UiAction::BackOrQuit);
        add(KeyScope::Overlay, &["?"], UiAction::BackOrQuit);

        // Inbox owns its personal-work operations. These scoped bindings do
        // not apply to a workspace timeline or any text-owning input.
        add(KeyScope::Inbox, &["f"], UiAction::Filter);
        add(KeyScope::Inbox, &["o"], UiAction::OpenCanonicalContext);
        add(KeyScope::Inbox, &["m"], UiAction::MarkRead);
        add(KeyScope::Inbox, &["U"], UiAction::MarkUnread);
        add(KeyScope::Inbox, &["a"], UiAction::MarkVisibleRead);

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
        add(KeyScope::Composer, &["ctrl-c"], UiAction::ClearComposer);
        add(KeyScope::Composer, &["ctrl-v"], UiAction::PasteClipboard);
        add(KeyScope::Composer, &["ctrl-o"], UiAction::AttachFile);
        add(KeyScope::Composer, &["alt-o"], UiAction::AttachPath);
        add(
            KeyScope::Composer,
            &["delete"],
            UiAction::RemoveLastAttachment,
        );
        add(KeyScope::Composer, &["ctrl-r"], UiAction::RetryAttachments);
        add(KeyScope::Composer, &["ctrl-left"], UiAction::MoveWordLeft);
        add(KeyScope::Composer, &["ctrl-right"], UiAction::MoveWordRight);
        add(KeyScope::Composer, &["home"], UiAction::MoveLineStart);
        add(KeyScope::Composer, &["end"], UiAction::MoveLineEnd);

        // v0.17 D5: two keys of the actions menu get their own in the
        // workspace; the Inbox keeps `U` and does not inherit these.
        add(KeyScope::Workspace, &["d"], UiAction::Delete);
        add(KeyScope::Workspace, &["u"], UiAction::MarkUnread);

        let keymap = Self {
            bindings,
            disabled: Vec::new(),
        };
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
                keymap
                    .disabled
                    .retain(|(scope, existing)| *scope != binding.scope || existing != &sequence);
                keymap.disabled.push((binding.scope, sequence));
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
            keymap
                .disabled
                .retain(|(scope, existing)| *scope != binding.scope || existing != &sequence);
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

    pub fn disabled_bindings(&self, scope: KeyScope) -> Vec<&KeySequence> {
        self.disabled
            .iter()
            .filter(|(disabled_scope, _)| *disabled_scope == scope)
            .map(|(_, sequence)| sequence)
            .collect()
    }

    pub fn effective_bindings(&self, scope: KeyScope) -> Vec<&KeyBinding> {
        // Text-owning states deliberately do not inherit global bindings:
        // literal `i`, `Space`, `j`, etc. must remain text. Overlay owners
        // likewise never inherit background workspace actions. Both carry
        // their own tightly constrained bindings instead.
        let mut effective = if scope.owns_text() || scope == KeyScope::Overlay {
            Vec::new()
        } else {
            self.bindings
                .iter()
                .filter(|binding| {
                    binding.scope == KeyScope::Global
                        && !self.disabled.iter().any(|(disabled_scope, sequence)| {
                            *disabled_scope == scope && *sequence == binding.sequence
                        })
                })
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
        if self.bindings.len().saturating_add(self.disabled.len())
            > MAX_BINDINGS.saturating_mul(KeyScope::ALL.len())
        {
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
                    && (binding.sequence.chords().len() != 1
                        || binding
                            .sequence
                            .chords()
                            .first()
                            .is_some_and(|chord| chord.is_text_input()))
                {
                    return Err(Error::Config(
                        "keymap cannot bind a printable character or sequence in a text-owning scope"
                            .into(),
                    ));
                }
                if !crate::ui::actions::keymap_allows(scope, binding.action) {
                    return Err(Error::Config(
                        "keymap action is not valid in this input scope".into(),
                    ));
                }
                if scope == KeyScope::Composer && !composer_action(binding.action) {
                    return Err(Error::Config(
                        "keymap composer scope permits only composer editing actions".into(),
                    ));
                }
                if scope == KeyScope::Filter && !filter_action(binding.action) {
                    return Err(Error::Config(
                        "keymap filter scope permits only filter input actions".into(),
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
            | UiAction::ClearComposer
            | UiAction::PasteClipboard
            | UiAction::AttachFile
            | UiAction::AttachPath
            | UiAction::RemoveLastAttachment
            | UiAction::RetryAttachments
            | UiAction::MoveWordLeft
            | UiAction::MoveWordRight
            | UiAction::MoveLineStart
            | UiAction::MoveLineEnd
    )
}

fn filter_action(action: UiAction) -> bool {
    matches!(
        action,
        UiAction::BackOrQuit
            | UiAction::SelectPrevious
            | UiAction::SelectNext
            | UiAction::ActivateFocused
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

// The composer receives this adapter only after `InputRouter` grants its
// typed composer owner the key. It converts owned printable/cursor/attachment
// input into bounded local edits; route actions are resolved separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAction {
    Up,
    Down,
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

pub fn map_insert(key: KeyEvent) -> KeyAction {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => KeyAction::Escape,
        (KeyModifiers::ALT, KeyCode::Enter) => KeyAction::Newline,
        (KeyModifiers::CONTROL, KeyCode::Char('j')) => KeyAction::Newline,
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Enter) => KeyAction::Submit,
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

/// Where keys enter bzz. A terminal reports one key in different ways on
/// different platforms; this gives it the one form the key map, the composer
/// and the apps match, the form a Linux or macOS terminal sends:
///
/// - a character carries Shift exactly when it is a capital (Windows reports
///   Shift with `?`, `:`, `!` and every shifted symbol, and no Shift with a
///   capital typed under Caps Lock);
/// - with Ctrl a letter is small and carries no Shift (Windows reports
///   Ctrl+C under Caps Lock as Ctrl+`C`);
/// - Shift+Tab is `BackTab` with no further modifier;
/// - on Windows a character typed with AltGr, which Windows reports as
///   Ctrl+Alt, is text (`@`, `{`, `€` on many European layouts);
/// - on Windows Ctrl+Enter is Ctrl+J: a console fed by a terminal (ConPTY,
///   as over SSH) reports the byte of Ctrl+J as Ctrl+Enter.
///
/// It changes nothing a Linux or macOS terminal sends but `BackTab`, which
/// crossterm reports with Shift on every platform.
pub fn normalize_key_event(event: KeyEvent) -> KeyEvent {
    normalize_key_event_on(event, cfg!(windows))
}

/// [`normalize_key_event`] for a chosen platform, so tests on any machine
/// can feed keys exactly as Windows delivers them.
pub fn normalize_key_event_on(mut event: KeyEvent, windows: bool) -> KeyEvent {
    let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
    match event.code {
        KeyCode::Char(character) => {
            if windows && event.modifiers.contains(ctrl_alt) && is_altgr_text(character) {
                event.modifiers.remove(ctrl_alt);
            }
            if event.modifiers.contains(KeyModifiers::CONTROL) {
                if character.is_ascii_uppercase() {
                    event.code = KeyCode::Char(character.to_ascii_lowercase());
                    event.modifiers.remove(KeyModifiers::SHIFT);
                }
            } else {
                event
                    .modifiers
                    .set(KeyModifiers::SHIFT, character.is_uppercase());
            }
        }
        KeyCode::BackTab => event.modifiers.remove(KeyModifiers::SHIFT),
        KeyCode::Enter if windows && event.modifiers == KeyModifiers::CONTROL => {
            event.code = KeyCode::Char('j');
        }
        _ => {}
    }
    event
}

/// What a Windows console sends that is not a key press but must be one.
///
/// A character the keyboard layout cannot type (`¿`, `¡`, `€`, `°` on a US
/// layout, from a terminal or a paste) reaches a console as an Alt code,
/// and crossterm reports it as the *release* of that character with no
/// press before it. Every other key is a press, then its release. So a
/// release of a character that was not pressed is the press of it. The
/// door keeps the characters pressed and not yet released (a few: more
/// than that are keys held from before bzz started). Only a character
/// outside ASCII is taken so: every layout types ASCII, and every key of
/// bzz's key map is ASCII, so a stray release can never become a command.
#[derive(Clone, Debug, Default)]
pub struct KeyDoor {
    held: Vec<char>,
}

const HELD_KEYS: usize = 16;

impl KeyDoor {
    /// A key as it enters bzz: [`normalize_key_event`], and on Windows an
    /// Alt code as the press it stands for. Other releases stay releases.
    pub fn admit(&mut self, event: KeyEvent) -> KeyEvent {
        self.admit_on(event, cfg!(windows))
    }

    pub fn admit_on(&mut self, event: KeyEvent, windows: bool) -> KeyEvent {
        let mut event = normalize_key_event_on(event, windows);
        let KeyCode::Char(character) = event.code else {
            return event;
        };
        if !windows {
            return event;
        }
        match event.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                if !self.held.contains(&character) {
                    if self.held.len() == HELD_KEYS {
                        self.held.remove(0);
                    }
                    self.held.push(character);
                }
            }
            KeyEventKind::Release => {
                if let Some(index) = self.held.iter().position(|held| *held == character) {
                    self.held.remove(index);
                } else if !character.is_ascii() {
                    event.kind = KeyEventKind::Press;
                }
            }
        }
        event
    }
}

/// The line break of a paste on Windows, where a paste arrives as keys
/// (docs/e2e-windows.md): in the composer, an Enter with more key presses
/// already waiting behind it is a new line, not "send". A person who types
/// Enter and the next key before bzz has read the Enter gets a line break
/// too, and sends with the next Enter; nothing is sent that was not meant.
pub fn paste_line_break(event: KeyEvent, more_waiting: bool, windows: bool) -> KeyEvent {
    if windows
        && more_waiting
        && event.code == KeyCode::Enter
        && event.modifiers.is_empty()
        && event.kind == KeyEventKind::Press
    {
        return KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
    }
    event
}

/// A character Windows reports with Ctrl+Alt that can only be AltGr text: a
/// real Ctrl+Alt chord reports the key's own letter or digit.
fn is_altgr_text(character: char) -> bool {
    !character.is_ascii_alphanumeric() && character != ' ' && !character.is_control()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tempfile::TempDir;

    use super::{
        KeyChord, KeyLookup, KeyMap, KeyScope, UiAction, normalize_key_event,
        normalize_key_event_on,
    };

    /// v0.17 C8: the map generated from the registry's rows is the
    /// hand-written one, binding for binding in every scope, and each
    /// action's keys come in the same order (the first is the one the
    /// palette and the footer show).
    #[test]
    fn the_generated_map_is_the_hand_written_one() {
        let generated = KeyMap::generated();
        let written = KeyMap::builtin();
        let mut compared = 0;
        for scope in KeyScope::ALL {
            let rows = |map: &KeyMap| {
                map.effective_bindings(scope)
                    .into_iter()
                    .map(|binding| (binding.sequence.label(), format!("{:?}", binding.action)))
                    .collect::<Vec<_>>()
            };
            let (mut left, mut right) = (rows(&generated), rows(&written));
            // Per action, the order of its keys.
            let order = |rows: &[(String, String)], action: &str| {
                rows.iter()
                    .filter(|(_, name)| name == action)
                    .map(|(keys, _)| keys.clone())
                    .collect::<Vec<_>>()
            };
            for (_, action) in &right {
                assert_eq!(
                    order(&left, action),
                    order(&right, action),
                    "{scope:?} {action}"
                );
            }
            left.sort();
            right.sort();
            assert_eq!(left, right, "{scope:?}");
            compared += right.len();
        }
        assert_eq!(generated.disabled, written.disabled);
        // Global 46 + Workspace 48 + Inbox 51 + Composer 18 + Overlay 3.
        assert!(compared > 150, "{compared}");
        println!("generated == hand-written: {compared} effective bindings in 6 scopes");
    }

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
    fn composer_has_a_confirmable_clear_draft_binding() {
        let keymap = KeyMap::builtin();
        let clear = KeyChord::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            keymap.lookup(KeyScope::Composer, &[clear]),
            KeyLookup::Action(UiAction::ClearComposer)
        );
    }

    #[test]
    fn composer_clipboard_and_attachment_bindings_are_typed() {
        let keymap = KeyMap::builtin();
        for (key, action) in [
            (
                KeyChord::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
                UiAction::PasteClipboard,
            ),
            (
                KeyChord::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
                UiAction::AttachFile,
            ),
            (
                KeyChord::new(KeyCode::Char('o'), KeyModifiers::ALT),
                UiAction::AttachPath,
            ),
            (
                KeyChord::new(KeyCode::Delete, KeyModifiers::NONE),
                UiAction::RemoveLastAttachment,
            ),
            (
                KeyChord::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
                UiAction::RetryAttachments,
            ),
        ] {
            assert_eq!(
                keymap.lookup(KeyScope::Composer, &[key]),
                KeyLookup::Action(action)
            );
        }
        let legacy = KeyChord::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            keymap.lookup(KeyScope::Composer, &[legacy]),
            KeyLookup::NoMatch
        );
    }

    #[test]
    fn defaults_use_modal_navigation_without_legacy_conflicts() {
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
        for (key, action) in [
            ('r', UiAction::React),
            ('v', UiAction::ToggleCopySelection),
            ('y', UiAction::CopyMessages),
        ] {
            assert_eq!(
                keymap.lookup(
                    KeyScope::Workspace,
                    &[KeyChord::new(KeyCode::Char(key), KeyModifiers::NONE)],
                ),
                KeyLookup::Action(action)
            );
        }
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
    fn scoped_disable_suppresses_a_global_default_only_in_that_scope() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'inbox'\nkeys = ['space', 'n']\ndisabled = true\n",
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
            KeyLookup::NoMatch
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
    fn text_scopes_reject_shifted_characters_and_sequences() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'composer'\nkeys = ['X']\naction = 'submit'\n",
        )
        .unwrap();
        assert!(KeyMap::load_from(&path).is_err());
        fs::write(
            &path,
            "[[binding]]\nscope = 'filter'\nkeys = ['ctrl-k', 'x']\naction = 'delete-to-end'\n",
        )
        .unwrap();
        assert!(KeyMap::load_from(&path).is_err());
    }

    #[test]
    fn filter_scope_rejects_workspace_actions() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'filter'\nkeys = ['ctrl-r']\naction = 'refresh'\n",
        )
        .unwrap();
        assert!(KeyMap::load_from(&path).is_err());
    }

    #[test]
    fn registry_rejects_actions_outside_their_scope() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'overlay'\nkeys = ['enter']\naction = 'open-inbox'\n",
        )
        .unwrap();
        assert!(KeyMap::load_from(&path).is_err());
        fs::write(&path, "[[binding]]\nkeys = ['m']\naction = 'mark-read'\n").unwrap();
        assert!(KeyMap::load_from(&path).is_err());
    }

    #[test]
    fn action_prefix_ambiguity_is_rejected() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(&path, "[[binding]]\nkeys = ['g']\naction = 'refresh'\n").unwrap();
        assert!(KeyMap::load_from(&path).is_err());
    }

    // Keys exactly as crossterm delivers them in a Windows console (ConPTY
    // over SSH, Windows 11 26100) and in tmux on Linux: examples/key_probe.rs,
    // docs/e2e-windows.md.
    fn on_windows(code: KeyCode, modifiers: KeyModifiers) -> KeyChord {
        KeyChord::from_event(normalize_key_event_on(KeyEvent::new(code, modifiers), true))
    }

    #[test]
    fn windows_shifted_symbols_reach_their_bindings() {
        let keymap = KeyMap::builtin();
        let shift = KeyModifiers::SHIFT;
        for (character, action) in [('?', UiAction::OpenHelp), (':', UiAction::OpenCommand)] {
            assert_eq!(
                keymap.lookup(
                    KeyScope::Workspace,
                    &[on_windows(KeyCode::Char(character), shift)]
                ),
                KeyLookup::Action(action),
                "{character}"
            );
        }
        assert_eq!(
            keymap.lookup(KeyScope::Overlay, &[on_windows(KeyCode::Char('?'), shift)]),
            KeyLookup::Action(UiAction::BackOrQuit)
        );
        for character in "?:@!#$%^&*()_+{}|<>\"~".chars() {
            assert_eq!(
                on_windows(KeyCode::Char(character), shift),
                KeyChord::new(KeyCode::Char(character), KeyModifiers::NONE),
                "{character}"
            );
        }
    }

    #[test]
    fn windows_capitals_and_ctrl_keys_under_caps_lock() {
        let keymap = KeyMap::builtin();
        // Shift+J, and J under Caps Lock, which Windows reports without Shift.
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
            assert_eq!(
                keymap.lookup(
                    KeyScope::Workspace,
                    &[on_windows(KeyCode::Char('J'), modifiers)]
                ),
                KeyLookup::Action(UiAction::ScrollViewportDown)
            );
        }
        // Ctrl+C under Caps Lock arrives as Ctrl+`C`; Ctrl+Shift+C too.
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ] {
            assert_eq!(
                keymap.lookup(
                    KeyScope::Composer,
                    &[on_windows(KeyCode::Char('C'), modifiers)]
                ),
                KeyLookup::Action(UiAction::ClearComposer)
            );
        }
        // A capital typed with Caps Lock and Shift is small on Windows.
        assert_eq!(
            on_windows(KeyCode::Char('a'), KeyModifiers::SHIFT),
            KeyChord::new(KeyCode::Char('a'), KeyModifiers::NONE)
        );
    }

    #[test]
    fn windows_altgr_characters_are_text() {
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        for character in ['@', '{', '[', '\\', '|', '~', '€', 'ą'] {
            let key = normalize_key_event_on(KeyEvent::new(KeyCode::Char(character), altgr), true);
            assert_eq!(
                super::map_insert(key),
                super::KeyAction::Character(character)
            );
            // Elsewhere Ctrl+Alt is never AltGr, and is left alone.
            let key = KeyEvent::new(KeyCode::Char(character), altgr);
            assert_eq!(normalize_key_event_on(key, false), key);
        }
        // A real Ctrl+Alt chord reports the key's own letter or digit.
        for character in ['a', '2', ' '] {
            let key = KeyEvent::new(KeyCode::Char(character), altgr);
            assert_eq!(normalize_key_event_on(key, true), key);
        }
    }

    #[test]
    fn windows_ctrl_enter_is_the_composer_newline() {
        let keymap = KeyMap::builtin();
        assert_eq!(
            keymap.lookup(
                KeyScope::Composer,
                &[on_windows(KeyCode::Enter, KeyModifiers::CONTROL)]
            ),
            KeyLookup::Action(UiAction::InsertNewline)
        );
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(normalize_key_event_on(key, false), key);
    }

    #[test]
    fn shift_tab_reaches_previous_focus() {
        // Every platform reports Shift+Tab as BackTab with Shift.
        let keymap = KeyMap::builtin();
        let back = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        for windows in [false, true] {
            assert_eq!(
                keymap.lookup(
                    KeyScope::Workspace,
                    &[KeyChord::from_event(normalize_key_event_on(back, windows))]
                ),
                KeyLookup::Action(UiAction::PreviousFocus)
            );
        }
    }

    #[test]
    fn keys_from_a_linux_terminal_are_unchanged() {
        let none = KeyModifiers::NONE;
        let mut keys = "?:@!#$%^&*()_+{}|<>\"~a/éñ😀 "
            .chars()
            .map(|character| KeyEvent::new(KeyCode::Char(character), none))
            .collect::<Vec<_>>();
        for character in "JKGU".chars() {
            keys.push(KeyEvent::new(KeyCode::Char(character), KeyModifiers::SHIFT));
        }
        for character in "kypnjdu c".chars() {
            keys.push(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL,
            ));
        }
        for character in "hlo".chars() {
            keys.push(KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT));
        }
        keys.push(KeyEvent::new(
            KeyCode::Char('J'),
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        for code in [
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::F(1),
            KeyCode::F(5),
            KeyCode::Up,
            KeyCode::Home,
            KeyCode::PageUp,
            KeyCode::Delete,
            KeyCode::Esc,
        ] {
            keys.push(KeyEvent::new(code, none));
        }
        keys.push(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        keys.push(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        for key in keys {
            assert_eq!(normalize_key_event_on(key, false), key, "{key:?}");
            assert_eq!(normalize_key_event(key), key, "{key:?}");
        }
    }

    #[test]
    fn user_maps_match_with_or_without_shift() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        for written in ["!", "shift-!"] {
            fs::write(
                &path,
                format!("[[binding]]\nkeys = ['{written}']\naction = 'refresh'\n"),
            )
            .unwrap();
            let keymap = KeyMap::load_from(&path).unwrap();
            for key in [
                KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('!'), KeyModifiers::SHIFT),
            ] {
                for windows in [false, true] {
                    let chord = KeyChord::from_event(normalize_key_event_on(key, windows));
                    assert_eq!(
                        keymap.lookup(KeyScope::Workspace, &[chord]),
                        KeyLookup::Action(UiAction::Refresh),
                        "{written} {key:?}"
                    );
                }
            }
        }
        for (written, key) in [
            ("X", KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            (
                "shift-x",
                KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT),
            ),
            (
                "shift-X",
                KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT),
            ),
            (
                "shift-tab",
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            ),
        ] {
            fs::write(
                &path,
                format!("[[binding]]\nkeys = ['{written}']\naction = 'refresh'\n"),
            )
            .unwrap();
            let keymap = KeyMap::load_from(&path).unwrap();
            assert_eq!(
                keymap.lookup(KeyScope::Workspace, &[KeyChord::from_event(key)]),
                KeyLookup::Action(UiAction::Refresh),
                "{written}"
            );
        }
    }

    #[test]
    fn windows_alt_codes_are_presses_and_releases_stay_releases() {
        use crossterm::event::KeyEventKind;
        let mut door = super::KeyDoor::default();
        let release = |character| {
            KeyEvent::new_with_kind(
                KeyCode::Char(character),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )
        };
        // `¿` from a terminal or a paste, exactly as crossterm gives it.
        assert_eq!(door.admit_on(release('¿'), true).kind, KeyEventKind::Press);
        // A typed key: its release after its press stays a release.
        let press = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(door.admit_on(press, true).kind, KeyEventKind::Press);
        assert_eq!(
            door.admit_on(release('a'), true).kind,
            KeyEventKind::Release
        );
        // `?` is pressed with Shift and released without it: one key.
        let shifted = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT);
        assert_eq!(door.admit_on(shifted, true).kind, KeyEventKind::Press);
        assert_eq!(
            door.admit_on(release('?'), true).kind,
            KeyEventKind::Release
        );
        // A release of an ASCII key never pressed (held from before bzz
        // started) is not a key.
        assert_eq!(
            door.admit_on(release('q'), true).kind,
            KeyEventKind::Release
        );
        // Elsewhere a release is a release.
        assert_eq!(
            door.admit_on(release('¿'), false).kind,
            KeyEventKind::Release
        );
        // Keys held from before do not grow the door without bound.
        for character in ('a'..='z').chain('A'..='Z') {
            door.admit_on(
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                true,
            );
        }
        assert!(door.held.len() <= super::HELD_KEYS);
    }

    #[test]
    fn windows_paste_line_break_is_a_composer_newline_only_with_keys_behind() {
        let keymap = KeyMap::builtin();
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let broken = super::paste_line_break(enter, true, true);
        assert_eq!(
            keymap.lookup(KeyScope::Composer, &[KeyChord::from_event(broken)]),
            KeyLookup::Action(UiAction::InsertNewline)
        );
        assert_eq!(super::paste_line_break(enter, false, true), enter);
        assert_eq!(super::paste_line_break(enter, true, false), enter);
        let alt = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(super::paste_line_break(alt, true, true), alt);
    }

    #[test]
    fn chord_matching_requires_exact_modifiers() {
        let chord = KeyChord::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert!(chord.matches(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)));
        assert!(!chord.matches(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
    }
}
