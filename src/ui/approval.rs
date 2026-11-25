//! A real command an agent wants to run, put to its owner (plan v0.15.0,
//! M7): which agent asks, in which conversation, for which folder, and the
//! whole command exactly as the shell will get it.
//!
//! - **What is approved is what was shown.** The command is shown whole:
//!   every line, with what would not show on a terminal written out as an
//!   escape, and a yes is taken only after its last row has been on screen.
//! - **A question is seen from anywhere.** While one waits, a line at the
//!   top of the screen says so, whatever else is open ([`render_banner`]).
//! - **Nothing answers by accident.** The dialog opens only when the owner
//!   asks for it, never over what he is typing.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    agents::runtime::approval::{Asked, MAX_REASON_CHARS},
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// What the dialog remembers between frames.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApprovalState {
    /// The question that is shown, by its id. A question that arrives
    /// later never takes its place.
    pub selected: Option<u64>,
    /// The first row of the command that is shown.
    pub scroll: usize,
    /// The last row of the command has been on screen.
    pub seen_end: bool,
    /// The owner is refusing and writes why: the reason so far.
    pub reason: Option<String>,
    /// What the last key could not do.
    pub notice: Option<String>,
    /// Rows of the command that fit, as of the last frame.
    pub page: usize,
    /// Rows the command takes at the width of the last frame.
    pub rows: usize,
}

impl ApprovalState {
    /// Shows the question `id` from its start.
    pub fn show(&mut self, id: u64) {
        *self = Self {
            selected: Some(id),
            ..Self::default()
        };
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let last = self.rows.saturating_sub(self.page.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(last);
        self.notice = None;
    }

    /// Adds typed or pasted text to the reason, on one line.
    pub fn type_reason(&mut self, text: &str) {
        if let Some(reason) = &mut self.reason {
            let room = MAX_REASON_CHARS.saturating_sub(reason.chars().count());
            reason.extend(
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
        }
    }
}

/// One row of a command on screen: the number of the line it starts, or
/// `None` for the rest of a line too long for one row; and its text in
/// pieces, each either as written or (`true`) the escape of a character
/// that would not show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRow {
    pub line: Option<usize>,
    pub parts: Vec<(String, bool)>,
}

/// A command as rows of at most `width` characters. A line of the command
/// is a row with its number; what does not fit goes on in rows without one,
/// so a long line cannot pass for two commands, nor two lines for one.
/// Nothing is cut, and nothing is left out.
pub fn command_rows(command: &str, width: usize) -> Vec<CommandRow> {
    let width = width.max(8);
    let mut rows = Vec::new();
    for (index, line) in command.split('\n').enumerate() {
        let mut row = CommandRow {
            line: Some(index + 1),
            parts: Vec::new(),
        };
        let mut used = 0;
        for (text, escaped) in sanitize::visible_parts(line) {
            let mut rest = text.as_str();
            while !rest.is_empty() {
                if used == width {
                    rows.push(std::mem::replace(
                        &mut row,
                        CommandRow {
                            line: None,
                            parts: Vec::new(),
                        },
                    ));
                    used = 0;
                }
                // An escape is kept whole: half of one reads as text.
                let count = rest.chars().count();
                let take = if escaped && count > width - used && count <= width {
                    0
                } else {
                    count.min(width - used)
                };
                if take == 0 {
                    used = width;
                    continue;
                }
                let end = rest
                    .char_indices()
                    .nth(take)
                    .map_or(rest.len(), |(index, _)| index);
                row.parts.push((rest[..end].to_owned(), escaped));
                used += take;
                rest = &rest[end..];
            }
        }
        rows.push(row);
    }
    rows
}

/// What is shown beside the question itself.
pub struct ApprovalView<'a> {
    pub asked: &'a Asked,
    /// The conversation, as the owner knows it: `#general`, `#general (a
    /// thread)`, or a direct message.
    pub conversation: &'a str,
    /// Who wrote the message the agent is answering.
    pub requester: &'a str,
    /// Questions that wait besides this one.
    pub others: usize,
    pub seconds_left: u64,
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The line that says a question waits, for the top of the screen.
pub fn banner_text(first: &Asked, conversation: &str, waiting: usize, seconds_left: u64) -> String {
    // A coding harness's command starts in a real folder of this machine.
    let place = if first.coding {
        crate::protocol::artifact::display(&first.path.display().to_string(), 48)
    } else {
        format!("/mnt/{}", first.folder)
    };
    format!(
        " {} asks to run a command in {} ({}){} · Ctrl-Y to see and answer it · refused in {} ",
        sanitize::single_line(&crate::protocol::artifact::display(&first.agent_name, 32)),
        sanitize::single_line(&place),
        sanitize::single_line(&crate::protocol::artifact::display(conversation, 40)),
        match waiting.saturating_sub(1) {
            0 => String::new(),
            more => format!(" · and {more} more"),
        },
        clock(seconds_left)
    )
}

/// Draws the line that says a question waits, over the first row of `area`.
pub fn render_banner(frame: &mut Frame<'_>, area: Rect, text: &str, theme: &Theme) {
    let row = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    frame.render_widget(Clear, row);
    frame.render_widget(
        Paragraph::new(text.to_owned()).style(theme.style(HighlightGroup::StatusModeCommand)),
        row,
    );
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    view: Option<ApprovalView<'_>>,
    state: &mut ApprovalState,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let keys = if state.reason.is_some() {
        "Enter refuse · Esc back"
    } else {
        "a allow · r refuse · j/k PgUp/PgDn scroll · Tab the next one · Esc decide later"
    };
    let block = crate::ui::frame::block(
        theme,
        BorderSurface::Modal,
        "a real command waits for your answer",
        true,
    )
    .title_bottom(crate::ui::frame::hint_line(theme, keys));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(view) = view else {
        frame.render_widget(
            Paragraph::new(
                "That command no longer waits: it timed out, or its turn ended. It was not started. Press Esc.",
            )
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let asked = view.asked;
    let muted = |text: String| Line::from(Span::styled(text, theme.style(HighlightGroup::Muted)));
    let warning = theme.style(HighlightGroup::Warning);
    let lines = asked.command.split('\n').count();
    let escapes = sanitize::visible_parts(&asked.command)
        .iter()
        .any(|(text, escaped)| *escaped && text != "\\n");
    let mut head = vec![
        Line::from(format!(
            "agent:        {}",
            sanitize::single_line(&asked.agent_name)
        )),
        Line::from(format!(
            "conversation: {} · it is answering {}",
            sanitize::single_line(view.conversation),
            sanitize::single_line(view.requester)
        )),
        Line::from(if asked.coding {
            format!(
                "folder:       {} (commands granted by: {})",
                sanitize::single_line(&asked.path.display().to_string()),
                sanitize::single_line(&asked.folder)
            )
        } else {
            format!(
                "folder:       /mnt/{} = {}",
                sanitize::single_line(&asked.folder),
                sanitize::single_line(&asked.path.display().to_string())
            )
        }),
        Line::from(Span::styled(
            "It runs as your user, starting in that folder, and can reach whatever you can.",
            warning,
        )),
        Line::from(format!(
            "time left:    {}, then it counts as refused{}",
            clock(view.seconds_left),
            match view.others {
                0 => String::new(),
                others => format!(" · {others} more wait (Tab)"),
            }
        )),
        muted(if asked.coding {
            format!(
                "the command, as its program reports it ({lines} line(s), {} bytes):",
                asked.command.len()
            )
        } else {
            format!(
                "the command, exactly as /bin/sh will get it ({lines} line(s), {} bytes):",
                asked.command.len()
            )
        }),
    ];
    if asked.coding {
        // bzz answers the program's question; the program starts the
        // command, and bzz cannot check that it is this one.
        head.push(Line::from(Span::styled(
            "Its own program starts it, not bzz: what runs is that program's to keep to.",
            warning,
        )));
    }
    if escapes {
        head.push(Line::from(Span::styled(
            "It holds characters that would not show; each is written out, like \\x1b or \\u{202e}.",
            warning,
        )));
    }
    if !asked.command.is_ascii() {
        head.push(Line::from(Span::styled(
            "It holds characters outside ASCII: a letter can look like another one.",
            warning,
        )));
    }
    // Below the command: where it stands, and the reason being written.
    let foot_height = 1 + usize::from(state.reason.is_some()) + usize::from(state.notice.is_some());
    let gutter = lines.to_string().len().max(2) + 3;
    let width = usize::from(inner.width).saturating_sub(gutter);
    let rows = command_rows(&asked.command, width);
    let page = usize::from(inner.height)
        .saturating_sub(head.len() + foot_height)
        .max(1);
    state.page = page;
    state.rows = rows.len();
    state.scroll = state.scroll.min(rows.len().saturating_sub(page));
    let end = (state.scroll + page).min(rows.len());
    // Only what was on screen counts as seen.
    if end == rows.len() && usize::from(inner.height) > head.len() + foot_height {
        state.seen_end = true;
    }
    let mut shown = head;
    for row in &rows[state.scroll..end] {
        let mut spans = vec![Span::styled(
            match row.line {
                Some(number) => format!("{number:>width$} │ ", width = gutter - 3),
                None => format!("{:>width$} ┆ ", "", width = gutter - 3),
            },
            theme.style(HighlightGroup::Muted),
        )];
        spans.extend(row.parts.iter().map(|(text, escaped)| {
            if *escaped {
                Span::styled(text.clone(), warning)
            } else {
                Span::styled(text.clone(), theme.style(HighlightGroup::Strong))
            }
        }));
        shown.push(Line::from(spans));
    }
    shown.push(muted(if rows.len() <= page {
        "that is the whole command".to_owned()
    } else if state.seen_end && end == rows.len() {
        format!("rows {}–{end} of {}: the end", state.scroll + 1, rows.len())
    } else {
        format!(
            "rows {}–{end} of {} · scroll to the end before you allow (j, PgDn)",
            state.scroll + 1,
            rows.len()
        )
    }));
    if let Some(reason) = &state.reason {
        shown.push(Line::from(format!(
            "refuse, and tell the agent why (optional): {}▏",
            sanitize::single_line(reason)
        )));
    }
    if let Some(notice) = &state.notice {
        shown.push(Line::from(Span::styled(
            sanitize::single_line(notice),
            theme.style(HighlightGroup::Error),
        )));
    }
    frame.render_widget(
        Paragraph::new(shown).style(theme.style(HighlightGroup::Normal)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::{Terminal, backend::TestBackend};
    use uuid::Uuid;

    use super::*;

    fn asked(command: &str) -> Asked {
        Asked {
            id: 7,
            agent: Uuid::nil(),
            agent_name: "Scout".into(),
            channel: Uuid::nil(),
            place: "place".into(),
            requester: "a".repeat(64),
            folder: "work".into(),
            path: "/home/ann/work".into(),
            command: command.into(),
            coding: false,
            asked_at: 1,
            expires: Instant::now() + Duration::from_secs(272),
        }
    }

    fn screen(
        width: u16,
        height: u16,
        asked: Option<&Asked>,
        state: &mut ApprovalState,
    ) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let theme = Theme::default();
        terminal
            .draw(|frame| {
                let view = asked.map(|asked| ApprovalView {
                    asked,
                    conversation: "#general (a thread)",
                    requester: "ann",
                    others: 1,
                    seconds_left: 272,
                });
                render(frame, frame.area(), view, state, &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_command_is_shown_whole_with_its_lines_and_what_would_not_show() {
        // Every character of a command is on some row, as itself or as its
        // escape; a line starts a numbered row and goes on in rows without
        // a number.
        let command = format!(
            "cd app &&\n  npm test\t# \u{1b}[2K\rrm -rf ~\n{}\n\u{202e}evil",
            "x".repeat(50)
        );
        let rows = command_rows(&command, 20);
        assert_eq!(
            rows.iter().map(|row| row.line).collect::<Vec<_>>(),
            [Some(1), Some(2), None, Some(3), None, None, Some(4)]
        );
        let text = |row: &CommandRow| {
            row.parts
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>()
        };
        assert_eq!(text(&rows[0]), "cd app &&");
        assert_eq!(
            format!("{}{}", text(&rows[1]), text(&rows[2])),
            "  npm test\\t# \\x1b[2K\\rrm -rf ~",
            "what would repaint the line is written out"
        );
        assert!(rows.iter().all(|row| text(row).chars().count() <= 20));
        assert_eq!(text(&rows[6]), "\\u{202e}evil");
        assert!(rows[6].parts[0].1 && !rows[6].parts[1].1);
        // Put together again, the rows are the command, escapes aside.
        let again = rows
            .iter()
            .map(|row| {
                format!(
                    "{}{}",
                    if row.line.is_some_and(|line| line > 1) {
                        "\n"
                    } else {
                        ""
                    },
                    text(row)
                )
            })
            .collect::<String>();
        assert_eq!(
            again,
            command
                .split('\n')
                .map(sanitize::visible)
                .collect::<Vec<_>>()
                .join("\n")
        );
        // An escape is never cut in two.
        for width in 8..30 {
            for row in command_rows("ab\u{1b}\u{1b}\u{1b}\u{202e}\u{202e}cd", width) {
                for (text, escaped) in row.parts {
                    assert!(
                        !escaped || text.starts_with('\\') && text.len() >= 4,
                        "{width}: {text}"
                    );
                }
            }
        }
        assert_eq!(command_rows("", 20).len(), 1);
    }

    #[test]
    fn the_dialog_shows_who_asks_where_and_the_command_and_takes_a_yes_only_at_its_end() {
        let short = asked("cd app &&\n  npm test -- --watch=false\t# \u{1b}[2K");
        let mut state = ApprovalState::default();
        state.show(short.id);
        let shown = screen(110, 20, Some(&short), &mut state).join("\n");
        for expected in [
            "a real command waits for your answer · a allow · r refuse",
            "agent:        Scout",
            "conversation: #general (a thread) · it is answering ann",
            "folder:       /mnt/work = /home/ann/work",
            "It runs as your user, starting in that folder, and can reach whatever you can.",
            "time left:    4:32, then it counts as refused · 1 more wait (Tab)",
            "the command, exactly as /bin/sh will get it (2 line(s), 44 bytes):",
            "It holds characters that would not show; each is written out",
            " 1 │ cd app &&",
            " 2 │   npm test -- --watch=false\\t# \\x1b[2K",
            "that is the whole command",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(state.seen_end, "all of it was on screen");
        assert!(!shown.contains('\u{1b}'));

        // A command longer than the screen: its end has not been seen until
        // it is scrolled to.
        let long = asked(
            &(1..=40)
                .map(|line| format!("echo step {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut state = ApprovalState::default();
        state.show(long.id);
        let shown = screen(110, 20, Some(&long), &mut state).join("\n");
        assert!(shown.contains(" 1 │ echo step 1"), "{shown}");
        assert!(!shown.contains("echo step 40"), "{shown}");
        assert!(
            shown.contains("of 40 · scroll to the end before you allow"),
            "{shown}"
        );
        assert!(!state.seen_end);
        state.scroll_by(5);
        let shown = screen(110, 20, Some(&long), &mut state).join("\n");
        assert!(
            shown.contains(" 6 │ echo step 6") && !state.seen_end,
            "{shown}"
        );
        state.scroll_by(1000);
        let shown = screen(110, 20, Some(&long), &mut state).join("\n");
        assert!(shown.contains("40 │ echo step 40"), "{shown}");
        assert!(shown.contains("of 40: the end"), "{shown}");
        assert!(state.seen_end);
        // Scrolling back up does not unsee it.
        state.scroll_by(-1000);
        screen(110, 20, Some(&long), &mut state);
        assert!(state.seen_end && state.scroll == 0);

        // Refusing: the reason is written on one bounded line.
        state.reason = Some(String::new());
        state.type_reason("not\nnow");
        state.type_reason(&"x".repeat(500));
        assert_eq!(
            state.reason.as_ref().unwrap().chars().count(),
            MAX_REASON_CHARS
        );
        state.reason = Some("not now".into());
        state.notice = Some("scroll to the end of the command first".into());
        let shown = screen(110, 20, Some(&long), &mut state).join("\n");
        assert!(
            shown.contains("refuse, and tell the agent why (optional): not now▏"),
            "{shown}"
        );
        assert!(shown.contains("Enter refuse · Esc back"), "{shown}");
        assert!(shown.contains("scroll to the end of the command first"));

        // The question went away while the dialog was open.
        let shown = screen(110, 20, None, &mut state).join("\n");
        assert!(
            shown.contains("That command no longer waits") && shown.contains("It was not started"),
            "{shown}"
        );
    }

    /// v0.15 M6: a command of a coding harness is shown as what it is. The
    /// folder is a real one of this machine, the command is the program's
    /// own account of it, and bzz does not start it.
    #[test]
    fn a_coding_harness_command_is_shown_with_its_real_folder_and_whose_word_it_is() {
        let mut coding = asked("make test");
        coding.coding = true;
        coding.folder = "work, ops".into();
        coding.path = "/home/ann/.local/share/bzz/agents/a/workspace".into();
        let mut state = ApprovalState::default();
        state.show(coding.id);
        let shown = screen(110, 20, Some(&coding), &mut state).join("\n");
        for expected in [
            "folder:       /home/ann/.local/share/bzz/agents/a/workspace (commands granted by: work, ops)",
            "It runs as your user, starting in that folder, and can reach whatever you can.",
            "the command, as its program reports it (1 line(s), 9 bytes):",
            "Its own program starts it, not bzz: what runs is that program's to keep to.",
            " 1 │ make test",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(!shown.contains("/mnt/"), "{shown}");
        assert!(!shown.contains("exactly as /bin/sh will get it"), "{shown}");
    }

    #[test]
    fn the_banner_says_who_asks_where_and_how_to_answer() {
        let first = asked("npm test");
        assert_eq!(
            banner_text(&first, "#general", 1, 272),
            " Scout asks to run a command in /mnt/work (#general) · Ctrl-Y to see and answer it · refused in 4:32 "
        );
        assert!(banner_text(&first, "#general", 3, 9).contains("· and 2 more · Ctrl-Y"));
        assert!(banner_text(&first, "#general", 3, 9).ends_with("refused in 0:09 "));
        // A coding harness's command: the real folder it starts in.
        let mut coding = asked("make");
        coding.coding = true;
        coding.path = "/home/ann/.local/share/bzz/agents/a/workspace".into();
        assert!(
            banner_text(&coding, "#general", 1, 272)
                .contains("a command in /home/ann/.local/share/bzz/agents/a/workspace (#general)"),
            "{}",
            banner_text(&coding, "#general", 1, 272)
        );
        let mut named = asked("x");
        named.agent_name = "Sc\u{1b}[31mout".into();
        assert!(!banner_text(&named, "a\nb", 1, 1).contains(['\u{1b}', '\n']));
    }
}
