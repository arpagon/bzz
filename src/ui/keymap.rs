use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    Theme,
    Command,
    Escape,
    Character(char),
    Backspace,
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
        (_, KeyCode::Backspace) => KeyAction::Backspace,
        (modifiers, KeyCode::Char(character))
            if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
        {
            KeyAction::Character(character)
        }
        _ => KeyAction::Ignore,
    }
}
