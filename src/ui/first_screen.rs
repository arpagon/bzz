//! The first screen, when bzz starts with no community: what bzz is, and
//! four ways on. `d` opens the demo in this same process; the other three
//! show the exact commands to copy until joining inside bzz exists (plan
//! A9, which replaces this text).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};

use crate::{
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// One way on from the first screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    Demo,
    Join,
    Pair,
    Import,
}

impl Choice {
    pub const ALL: [Self; 4] = [Self::Demo, Self::Join, Self::Pair, Self::Import];

    pub const fn key(self) -> char {
        match self {
            Self::Demo => 'd',
            Self::Join => 'j',
            Self::Pair => 'p',
            Self::Import => 'k',
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Demo => "Try the demo",
            Self::Join => "Join a community",
            Self::Pair => "Pair with Buzz",
            Self::Import => "Import a key",
        }
    }

    pub const fn hint(self) -> &'static str {
        match self {
            Self::Demo => "a generated team, offline; nothing is saved",
            Self::Join => "a new identity, then the community's address",
            Self::Pair => "use the account of Buzz desktop on this machine",
            Self::Import => "a Buzz key or a backup file you already have",
        }
    }

    /// What to run, line by line, for the three that are commands still.
    /// `<…>` is what the person fills in.
    pub const fn commands(self) -> &'static [&'static str] {
        match self {
            Self::Demo => &[],
            Self::Join => &[
                "bzz identity new --label me",
                "bzz community add my-team wss://<community address> <identity id>",
            ],
            Self::Pair => &[
                "bzz identity pair-desktop --label buzz --relay wss://<community address>",
                "bzz community add my-team wss://<community address> <identity id>",
            ],
            Self::Import => &[
                "bzz identity import --label me",
                "bzz identity import-backup --label me --input <backup file>",
                "bzz community add my-team wss://<community address> <identity id>",
            ],
        }
    }

    /// What to know after the commands, line by line.
    pub const fn note(self) -> &'static [&'static str] {
        match self {
            Self::Demo => &[],
            Self::Join => &[
                "The first prints your identity id and its pubkey. Give the pubkey",
                "to the community's admin, and once you are added, start bzz again.",
            ],
            Self::Pair => &[
                "Approve the request in Buzz desktop; the first prints the identity id.",
                "Then add the community and start bzz again.",
            ],
            Self::Import => &[
                "bzz asks for the key itself, never on the command line.",
                "Then add the community and start bzz again.",
            ],
        }
    }
}

/// What the first screen shows: the cursor, and the commands of a choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstScreen {
    selected: usize,
    shown: Option<Choice>,
}

/// What a key did on the first screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Not a key of this screen: bzz handles it as before (`?`, `q`, `:`).
    Ignored,
    Handled,
    OpenDemo,
}

impl FirstScreen {
    pub fn selected(&self) -> Choice {
        Choice::ALL[self.selected.min(Choice::ALL.len() - 1)]
    }

    pub fn shown(&self) -> Option<Choice> {
        self.shown
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if !(key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) {
            return Outcome::Ignored;
        }
        match key.code {
            KeyCode::Char(character) => {
                match Choice::ALL
                    .into_iter()
                    .position(|choice| choice.key() == character)
                {
                    Some(index) => {
                        self.selected = index;
                        self.choose()
                    }
                    None => Outcome::Ignored,
                }
            }
            KeyCode::Enter => self.choose(),
            KeyCode::Down | KeyCode::Tab => {
                self.selected = (self.selected + 1) % Choice::ALL.len();
                Outcome::Handled
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.selected = (self.selected + Choice::ALL.len() - 1) % Choice::ALL.len();
                Outcome::Handled
            }
            KeyCode::Esc if self.shown.is_some() => {
                self.shown = None;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }

    fn choose(&mut self) -> Outcome {
        match self.selected() {
            Choice::Demo => Outcome::OpenDemo,
            choice => {
                self.shown = Some(choice);
                Outcome::Handled
            }
        }
    }
}

/// Draws the first screen in `area`; `notice` is the last command's answer.
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    state: &FirstScreen,
    notice: Option<&str>,
) {
    let muted = theme.style(HighlightGroup::Muted);
    let strong = theme.style(HighlightGroup::Strong);
    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            Span::styled(
                "  Welcome to bzz",
                theme.style(HighlightGroup::CommunitySelected),
            ),
            Span::styled(" · Buzz in your terminal", strong),
        ]),
        Line::from("  Your team's chat, threads, huddles and agents, from the keyboard."),
        Line::default(),
        Line::from("  There is no community on this machine yet. Start with one of these:"),
        Line::default(),
    ];
    for choice in Choice::ALL {
        let selected = state.selected() == choice;
        let row = if selected {
            theme.style(HighlightGroup::SelectedRow)
        } else {
            theme.style(HighlightGroup::Normal)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { " › " } else { "   " },
                theme.style(HighlightGroup::SelectionMarker),
            ),
            Span::styled(format!("{}  ", choice.key()), strong),
            Span::styled(format!("{:<18}", choice.label()), row),
            Span::styled(format!(" {}", choice.hint()), muted),
        ]));
    }
    lines.push(Line::default());
    if let Some(choice) = state.shown() {
        lines.push(Line::styled(format!("  {}: run", choice.label()), strong));
        for command in choice.commands() {
            lines.push(Line::styled(
                format!("    {command}"),
                theme.style(HighlightGroup::Emphasis),
            ));
        }
        for note in choice.note() {
            lines.push(Line::styled(format!("  {note}"), muted));
        }
        lines.push(Line::default());
    }
    lines.push(Line::styled(
        "  Enter choose · ↑↓ move · Esc back · ? keys · q quit",
        muted,
    ));
    if let Some(notice) = notice {
        lines.push(Line::default());
        lines.push(Line::styled(
            format!("  {}", sanitize::single_line(notice)),
            muted,
        ));
    }
    // A card of at most 84×24 in the middle of a larger terminal.
    let width = area.width.min(84);
    let height = area
        .height
        .min(u16::try_from(lines.len() + 2).unwrap_or(u16::MAX).max(24));
    let area = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .border_type(theme.border_type(BorderSurface::Pane))
                    .border_style(theme.style(HighlightGroup::PaneBorder))
                    .title_style(theme.style(HighlightGroup::PaneTitle))
                    .title(" bzz ")
                    .title_bottom(Line::styled(
                        format!(" bzz {} ", env!("CARGO_PKG_VERSION")),
                        muted,
                    )),
            ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    use super::{Choice, FirstScreen, Outcome, render};
    use crate::ui::theme::Theme;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn screen(state: &FirstScreen, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &Theme::default(), state, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn d_opens_the_demo_and_the_others_show_their_commands() {
        let mut state = FirstScreen::default();
        assert_eq!(state.key(key(KeyCode::Char('d'))), Outcome::OpenDemo);
        for choice in [Choice::Join, Choice::Pair, Choice::Import] {
            assert_eq!(
                state.key(key(KeyCode::Char(choice.key()))),
                Outcome::Handled
            );
            assert_eq!(state.shown(), Some(choice));
            assert!(!choice.commands().is_empty());
        }
        assert_eq!(state.key(key(KeyCode::Esc)), Outcome::Handled);
        assert_eq!(state.shown(), None);
        // Esc with nothing shown, `q`, `?` and `:` stay bzz's own keys.
        for code in [
            KeyCode::Esc,
            KeyCode::Char('q'),
            KeyCode::Char('?'),
            KeyCode::Char(':'),
        ] {
            assert_eq!(state.key(key(code)), Outcome::Ignored, "{code:?}");
        }
        assert_eq!(
            state.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Outcome::Ignored
        );
    }

    #[test]
    fn arrows_move_and_enter_chooses() {
        let mut state = FirstScreen::default();
        assert_eq!(state.selected(), Choice::Demo);
        state.key(key(KeyCode::Up));
        assert_eq!(state.selected(), Choice::Import);
        state.key(key(KeyCode::Down));
        state.key(key(KeyCode::Down));
        assert_eq!(state.selected(), Choice::Join);
        assert_eq!(state.key(key(KeyCode::Enter)), Outcome::Handled);
        assert_eq!(state.shown(), Some(Choice::Join));
        state.key(key(KeyCode::Up));
        assert_eq!(state.key(key(KeyCode::Enter)), Outcome::OpenDemo);
    }

    /// Every choice, its key and its commands fit at 80×24 and 200×50.
    #[test]
    fn the_first_screen_fits_80_by_24_and_200_by_50() {
        for (width, height) in [(80, 24), (200, 50)] {
            for shown in [None, Some('j'), Some('p'), Some('k')] {
                let mut state = FirstScreen::default();
                if let Some(letter) = shown {
                    state.key(key(KeyCode::Char(letter)));
                }
                let text = screen(&state, width, height);
                for needle in [
                    "Welcome to bzz",
                    "d  Try the demo",
                    "j  Join a community",
                    "p  Pair with Buzz",
                    "k  Import a key",
                    "q quit",
                ] {
                    assert!(text.contains(needle), "{width}x{height} {shown:?}: {text}");
                }
                if let Some(choice) = state.shown() {
                    for command in choice.commands() {
                        // A long command may wrap at 80 columns; its start
                        // is always on screen.
                        let start: String = command.chars().take(40).collect();
                        assert!(text.contains(&start), "{width}x{height}: {text}");
                    }
                }
            }
        }
    }

    /// Goldens of the first screen at 80×24 and 200×50, with and without a
    /// choice's commands, in `tests/fixtures/first-screen/`. `BZZ_BLESS=1`
    /// writes them again.
    #[test]
    fn first_screen_snapshots() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-screen");
        let mut join = FirstScreen::default();
        join.key(key(KeyCode::Char('j')));
        for (name, state, width, height) in [
            ("80x24", FirstScreen::default(), 80, 24),
            ("200x50", FirstScreen::default(), 200, 50),
            ("join-80x24", join, 80, 24),
            ("join-200x50", join, 200, 50),
        ] {
            // The version changes with every release; the golden does not.
            let text = screen(&state, width, height).replace(
                env!("CARGO_PKG_VERSION"),
                &"#".repeat(env!("CARGO_PKG_VERSION").len()),
            ) + "\n";
            let path = directory.join(format!("{name}.txt"));
            if std::env::var_os("BZZ_BLESS").is_some() {
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(&path, &text).unwrap();
            }
            let golden = std::fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(text, golden, "{name}: run with BZZ_BLESS=1 to write it");
        }
    }
}
