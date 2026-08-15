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
