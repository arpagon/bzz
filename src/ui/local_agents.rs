//! `:agents` — the local agents of the open community: each one's state,
//! what it is doing now, how its last turn ended, its settings and its
//! capability record, with keys to pause, resume, stop the running turn and
//! read its log. And `:agent new`, the form that asks what `bzz agent new`
//! asks.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use uuid::Uuid;

use crate::{
    agents::manage::{self, Answers, NewAgent},
    config::Harness,
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// One local agent as the panel shows it. No content of any message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRow {
    pub id: Uuid,
    pub name: String,
    pub harness: Harness,
    /// Its state in a few words: idle, answering 1, paused, not running.
    pub state: String,
    /// What it is doing now, or what is wrong. `None`: nothing.
    pub doing: Option<String>,
    /// How its last turn ended, and when. `None`: no turn yet.
    pub last: Option<String>,
    /// Its settings, as `bzz agent show` prints them.
    pub settings: Vec<String>,
    /// Its capability record, with what its own settings add to it.
    pub record: Vec<String>,
}

/// What the panel remembers between frames.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PanelState {
    pub selected: usize,
    /// The log of the selected agent is shown instead of its record.
    pub show_log: bool,
    /// The end of that log, oldest line first.
    pub log: Vec<String>,
}

impl PanelState {
    pub fn move_by(&mut self, delta: isize, rows: usize) {
        self.selected = match rows {
            0 => 0,
            rows => self.selected.saturating_add_signed(delta).min(rows - 1),
        };
        self.show_log = false;
        self.log.clear();
    }
}

/// The end of a harness's log: at most `lines` lines, each made safe to
/// print and cut to `width`. A harness writes what it likes to its log.
pub fn log_tail(text: &str, lines: usize, width: usize) -> Vec<String> {
    let mut tail = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(lines)
        .map(|line| {
            sanitize::single_line(line)
                .chars()
                .take(width)
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    tail.reverse();
    tail
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    rows: &[AgentRow],
    state: &PanelState,
    // Whether this bzz is running the community's agents now (unlocked).
    running: bool,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(format!(
            " local agents{} · j/k select · space pause/resume · x stop its turn · i add to this channel · l log · n new · Tab directory · Esc ",
            if running { "" } else { " · not running: unlock to run them" }
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No local agent in this community. Press n, or run `:agent new`, to create one: bzz holds its key and starts its process.",
            )
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::try_from(rows.len().min(8) + 1).unwrap_or(9)),
            Constraint::Min(1),
        ])
        .split(inner);
    let mut list = vec![Line::from(Span::styled(
        "agent · harness · state · doing now",
        theme.style(HighlightGroup::Muted),
    ))];
    list.extend(rows.iter().enumerate().take(8).map(|(index, row)| {
        Line::from(format!(
            "{} {} · {} · {}{}",
            if index == state.selected { "›" } else { " " },
            sanitize::single_line(&crate::protocol::artifact::display(&row.name, 32)),
            row.harness.as_str(),
            row.state,
            row.doing
                .as_deref()
                .map(|doing| format!(
                    " · {}",
                    sanitize::single_line(&crate::protocol::artifact::display(doing, 60))
                ))
                .unwrap_or_default()
        ))
    }));
    frame.render_widget(
        Paragraph::new(list).style(theme.style(HighlightGroup::Normal)),
        panes[0],
    );
    let Some(row) = rows.get(state.selected) else {
        return;
    };
    let muted = |text: &str| {
        Line::from(Span::styled(
            text.to_owned(),
            theme.style(HighlightGroup::Muted),
        ))
    };
    let mut detail = vec![
        muted(&format!(
            "{} · l {}",
            sanitize::single_line(&row.name),
            if state.show_log {
                "back to its record"
            } else {
                "its log"
            }
        )),
        Line::from(format!("state:        {}", row.state)),
        Line::from(format!(
            "doing now:    {}",
            row.doing
                .as_deref()
                .map_or_else(|| "nothing".to_owned(), sanitize::single_line)
        )),
        Line::from(format!(
            "last outcome: {}",
            row.last.as_deref().unwrap_or("no turn yet")
        )),
    ];
    if state.show_log {
        detail.push(muted(
            "log (what its harness wrote, newest last; no key is ever in it)",
        ));
        if state.log.is_empty() {
            detail.push(Line::from("  empty: it has not been started yet"));
        }
        // The newest lines are the ones that fit.
        let room = usize::from(panes[1].height).saturating_sub(detail.len());
        let skip = state.log.len().saturating_sub(room);
        detail.extend(
            state
                .log
                .iter()
                .skip(skip)
                .map(|line| Line::from(format!("  {line}"))),
        );
        frame.render_widget(
            Paragraph::new(detail).style(theme.style(HighlightGroup::Normal)),
            panes[1],
        );
        return;
    }
    detail.extend(
        row.settings
            .iter()
            .map(|line| Line::from(sanitize::single_line(line))),
    );
    detail.extend(
        row.record
            .iter()
            .map(|line| Line::from(sanitize::single_line(line))),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

/// Where the form of `:agent new` stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormStage {
    /// The owner is answering.
    Editing,
    /// Node, the install of the host and the model are being checked.
    Checking,
    /// The checks passed: the owner confirms the authorization.
    Confirm {
        new: Box<NewAgent>,
        notes: Vec<String>,
    },
    /// Created: what was made, and the agent's capability record.
    Done(Vec<String>),
}

/// The answers so far, one text per question of [`manage::QUESTIONS`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAgentForm {
    pub fields: [String; manage::QUESTIONS.len()],
    pub focus: usize,
    pub stage: FormStage,
    /// Why the answers were refused, or why the creation failed.
    pub error: Option<String>,
}

impl NewAgentForm {
    pub fn new(name: &str) -> Self {
        let mut fields: [String; manage::QUESTIONS.len()] = Default::default();
        fields[0] = name.trim().to_owned();
        fields[1] = "pi-durable".into();
        Self {
            // A name given with the command is answered already.
            focus: usize::from(!fields[0].is_empty()) * 2,
            fields,
            stage: FormStage::Editing,
            error: None,
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        self.focus = self
            .focus
            .saturating_add_signed(delta)
            .min(self.fields.len() - 1);
    }

    /// Adds typed or pasted text to the answer in focus, on one line.
    pub fn insert(&mut self, text: &str) {
        if self.stage != FormStage::Editing {
            return;
        }
        let field = &mut self.fields[self.focus];
        let room = 2048usize.saturating_sub(field.chars().count());
        field.extend(
            text.chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .take(room),
        );
        self.error = None;
    }

    pub fn backspace(&mut self) {
        if self.stage == FormStage::Editing {
            self.fields[self.focus].pop();
            self.error = None;
        }
    }

    /// The answers as written, in the shape the command gathers them.
    pub fn answers(&self) -> Answers {
        let list = |text: &str| {
            manage::split_list(text)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        Answers {
            name: self.fields[0].clone(),
            harness: self.fields[1].clone(),
            model: self.fields[2].clone(),
            thinking: None,
            instructions: self.fields[3].clone(),
            wake: self.fields[4].clone(),
            allow: list(&self.fields[5]),
            folders: manage::split_commas(&self.fields[6]),
            hosts: list(&self.fields[7]),
            picture: self.fields[8].clone(),
            packages: self.fields[9].clone(),
        }
    }
}

pub fn render_form(
    frame: &mut Frame<'_>,
    area: Rect,
    form: &NewAgentForm,
    community: &str,
    owner: &str,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let keys = match form.stage {
        FormStage::Editing => "Tab/↑/↓ move · Enter next · Ctrl-S check and continue · Esc cancel",
        FormStage::Checking => "checking…",
        FormStage::Confirm { .. } => "Enter sign and create · e edit the answers · Esc cancel",
        FormStage::Done(_) => "Enter or Esc close",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(format!(" new local agent · {keys} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let muted = |text: String| Line::from(Span::styled(text, theme.style(HighlightGroup::Muted)));
    let mut lines = Vec::new();
    match &form.stage {
        FormStage::Done(done) => {
            lines.extend(done.iter().map(|line| Line::from(line.clone())));
        }
        stage => {
            lines.push(muted(format!(
                "Community: {} (the one that is open; bzz holds the agent's key and starts its process)",
                sanitize::single_line(community)
            )));
            for (index, ((label, _), value)) in
                manage::QUESTIONS.iter().zip(&form.fields).enumerate()
            {
                let focused = *stage == FormStage::Editing && index == form.focus;
                lines.push(Line::from(format!(
                    "{} {label}: {}{}",
                    if focused { "›" } else { " " },
                    sanitize::single_line(value),
                    if focused { "▏" } else { "" }
                )));
            }
            lines.push(Line::default());
            match stage {
                FormStage::Editing => {
                    lines.push(muted(manage::QUESTIONS[form.focus].1.to_owned()));
                }
                FormStage::Checking => lines.push(Line::from(
                    "Checking Node.js, the agent host (installed now when this is its first use; that needs the npm registry) and the model…",
                )),
                FormStage::Confirm { new, notes } => {
                    lines.extend(notes.iter().map(|note| Line::from(note.clone())));
                    lines.push(Line::from(format!(
                        "Enter signs an owner authorization (NIP-OA) letting agent \"{}\" act for {}, and publishes its profile and your record of it in {}.",
                        sanitize::single_line(&new.name),
                        sanitize::single_line(owner),
                        sanitize::single_line(community)
                    )));
                }
                FormStage::Done(_) => {}
            }
        }
    }
    if let Some(error) = &form.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            sanitize::single_line(error),
            theme.style(HighlightGroup::Error),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn screen(width: u16, height: u16, draw: impl FnOnce(&mut Frame<'_>)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn row(name: &str, state: &str, doing: Option<&str>, last: Option<&str>) -> AgentRow {
        AgentRow {
            id: Uuid::new_v4(),
            name: name.into(),
            harness: Harness::PiDurable,
            state: state.into(),
            doing: doing.map(str::to_owned),
            last: last.map(str::to_owned),
            settings: vec![
                "model:       scripted/scripted".into(),
                "wakes for:   its owner only".into(),
            ],
            record: crate::agents::capability::record(Harness::PiDurable).lines(),
        }
    }

    #[test]
    fn the_panel_shows_each_agents_state_doing_last_outcome_and_record() {
        let rows = [
            row(
                "Scout",
                "answering 1",
                Some("bash"),
                Some("answered · 2026-11-14T10:00:00Z"),
            ),
            row("Ranger", "paused", Some("paused by its owner"), None),
        ];
        let theme = Theme::default();
        let mut state = PanelState::default();
        let shown = screen(150, 40, |frame| {
            render(frame, frame.area(), &rows, &state, true, &theme)
        });
        for expected in [
            "local agents · j/k select · space pause/resume · x stop its turn · i add to this channel · l log · n new",
            "› Scout · pi-durable · answering 1 · bash",
            "  Ranger · pi-durable · paused · paused by its owner",
            "state:        answering 1",
            "doing now:    bash",
            "last outcome: answered · 2026-11-14T10:00:00Z",
            "model:       scripted/scripted",
            "capability record: pi-durable (everything in v0.15.0)",
            "its own disk:        yes",
            "one final reply per mention: bzz",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(!shown.contains("not running"), "{shown}");

        // The second agent, with nothing done yet; and bzz locked.
        state.move_by(1, rows.len());
        state.move_by(1, rows.len());
        assert_eq!(state.selected, 1, "the selection stays on the list");
        let shown = screen(150, 40, |frame| {
            render(frame, frame.area(), &rows, &state, false, &theme)
        });
        for expected in [
            "› Ranger · pi-durable · paused",
            "last outcome: no turn yet",
            "not running: unlock to run them",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }

        // Its log, in place of its record: the newest lines, made safe.
        state.show_log = true;
        state.log = log_tail("first\n\nsecond \u{1b}[31mred\u{1b}[0m\nthird\n", 2, 40);
        assert_eq!(state.log.len(), 2);
        assert!(!state.log.join("").contains('\u{1b}'), "{:?}", state.log);
        let shown = screen(150, 40, |frame| {
            render(frame, frame.area(), &rows, &state, true, &theme)
        });
        assert!(
            shown.contains("log (what its harness wrote, newest last"),
            "{shown}"
        );
        assert!(
            shown.contains("third") && !shown.contains("first"),
            "{shown}"
        );
        assert!(!shown.contains("capability record"), "{shown}");

        // No agent: how to make one.
        let shown = screen(100, 10, |frame| {
            render(
                frame,
                frame.area(),
                &[],
                &PanelState::default(),
                true,
                &theme,
            )
        });
        assert!(shown.contains(":agent new"), "{shown}");
    }

    #[test]
    fn the_form_asks_what_the_command_asks_and_reads_it_the_same_way() {
        let theme = Theme::default();
        let mut form = NewAgentForm::new("");
        assert_eq!(form.focus, 0);
        let shown = screen(130, 24, |frame| {
            render_form(frame, frame.area(), &form, "rikuy", "ann", &theme)
        });
        for (label, _) in manage::QUESTIONS {
            assert!(
                shown.contains(&format!("{label}:")),
                "`{label}` in:\n{shown}"
            );
        }
        assert!(shown.contains("Community: rikuy"), "{shown}");
        assert!(shown.contains("› Name: ▏"), "{shown}");
        assert!(shown.contains("as Buzz will show it"), "{shown}");

        // Typing and pasting go to the answer in focus, on one line.
        form.insert("Sc");
        form.insert("out\nRanger");
        form.backspace();
        assert_eq!(form.fields[0], "Scout Range");
        form.fields[0] = "Scout".into();
        form.move_by(2);
        form.insert("scripted/scripted");
        form.move_by(5);
        form.insert("GitHub.com, example.org:8443");
        form.move_by(99);
        assert_eq!(form.focus, manage::QUESTIONS.len() - 1);
        let new = form.answers().read().unwrap();
        assert_eq!(new.name, "Scout");
        assert_eq!(new.harness, Harness::PiDurable);
        assert_eq!(
            new.model.as_ref().map(|model| model.model_id.as_str()),
            Some("scripted")
        );
        assert_eq!(new.network, ["example.org:8443", "github.com"]);
        // What the command refuses, the form refuses, in the same words.
        let mut wrong = form.clone();
        wrong.fields[1] = "gemini".into();
        assert!(
            wrong
                .answers()
                .read()
                .unwrap_err()
                .to_string()
                .contains("choose pi-durable, grok, codex or claude-code")
        );

        // The confirmation says what will be signed and published.
        form.stage = FormStage::Confirm {
            new: Box::new(new),
            notes: vec!["Node.js 22.19.0 at /usr/bin/node".into()],
        };
        form.insert("ignored while confirming");
        let shown = screen(130, 24, |frame| {
            render_form(frame, frame.area(), &form, "rikuy", "ann", &theme)
        });
        for expected in [
            "Enter sign and create",
            "Node.js 22.19.0 at /usr/bin/node",
            "Enter signs an owner authorization (NIP-OA) letting agent \"Scout\" act for ann",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(!shown.contains("ignored while confirming"), "{shown}");

        // A name given with `:agent new <name>` is answered already.
        let named = NewAgentForm::new(" Ranger ");
        assert_eq!((named.fields[0].as_str(), named.focus), ("Ranger", 2));
    }
}
