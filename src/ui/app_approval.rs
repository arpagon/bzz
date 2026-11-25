//! A version of an app put to its owner (plan v0.16.0, M4; rule 16), on
//! v0.15's overlay and in its manner (`ui::approval`): who it comes from,
//! the brief, the record types it declares **with any change marked as a
//! change**, the sizes, and its difference from the version that runs.
//!
//! - **What is approved is what was shown.** The head (who, the hash, the
//!   sizes, the types) is always on screen; the brief and the difference
//!   scroll below it, and a yes is taken only after their last row has
//!   been on screen. The approval names the hash the head shows.
//! - **A version is announced from anywhere.** While one waits, a line at
//!   the top of the screen says so ([`banner_text`]), as for an agent's
//!   command; `Ctrl+Y` or `:approvals` opens this.
//! - **Nothing answers by accident.** It opens only when the owner asks.
//!
//! Everything a proposer wrote has gone through `visible` in
//! [`Review::render`] before it gets here, and is drawn as one line each.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    agents::runtime::approval::MAX_REASON_CHARS,
    apps::store::{
        AppVersion,
        review::{Review, from_whom_short},
    },
    render::sanitize,
    ui::{
        approval::command_rows,
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

/// What the overlay remembers between frames.
#[derive(Clone, Debug, Default)]
pub struct AppApprovalState {
    /// The version shown: the app's name and the version's number. A
    /// version that arrives later never takes its place.
    pub selected: Option<(String, u32)>,
    /// What the owner is shown of it; None once it no longer waits.
    pub review: Option<Review>,
    /// Why it cannot be shown, when it cannot.
    pub unreadable: Option<String>,
    /// The first row of the brief and the difference that is shown.
    pub scroll: usize,
    /// The last row has been on screen.
    pub seen_end: bool,
    /// The owner is refusing and writes why: the reason so far.
    pub reason: Option<String>,
    /// What the last key could not do.
    pub notice: Option<String>,
    /// Rows that fit, and rows there are, as of the last frame.
    pub page: usize,
    pub rows: usize,
}

impl AppApprovalState {
    /// Shows a version from its start.
    pub fn show(&mut self, name: &str, version: u32, review: Result<Review, String>) {
        let (review, unreadable) = match review {
            Ok(review) => (Some(review), None),
            Err(why) => (None, Some(why)),
        };
        *self = Self {
            selected: Some((name.to_owned(), version)),
            review,
            unreadable,
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

/// When a version was proposed, as the overlay writes it.
pub fn when(seconds: u64) -> String {
    crate::agents::manage::when(seconds)
}

/// The review as the overlay shows it: the head, always on screen, and the
/// body (the brief and the difference), which scrolls.
pub fn split(review: &Review) -> (Vec<String>, Vec<String>) {
    let text = review.render(when);
    let mut lines = text.lines().map(str::to_owned);
    let head: Vec<String> = lines.by_ref().take_while(|line| !line.is_empty()).collect();
    (head, lines.collect())
}

/// The line that says a version waits, for the top of the screen.
pub fn banner_text(first: &AppVersion, waiting: usize) -> String {
    // An agent proposes in a channel too; only a person's offer comes from
    // the channel itself.
    let changed = if first.proposer.channel.is_some()
        && first.proposer.kind == crate::apps::store::ProposerKind::Member
    {
        " from the channel"
    } else {
        ""
    };
    format!(
        " {}: version {} from {}{changed} waits for your approval{} · Ctrl-Y to see and answer it ",
        sanitize::single_line(&first.name),
        first.version,
        sanitize::single_line(&crate::protocol::artifact::display(
            &from_whom_short(&first.proposer),
            48
        )),
        match waiting.saturating_sub(1) {
            0 => String::new(),
            more => format!(" · and {more} more"),
        },
    )
}

fn head_style(line: &str, theme: &Theme) -> Style {
    if line.contains("CHANGED")
        || line.starts_with("WARNING")
        || line.starts_with("    + ")
        || line.starts_with("    - ")
    {
        theme.style(HighlightGroup::Warning)
    } else if line.starts_with("Hash:") {
        theme.style(HighlightGroup::Strong)
    } else {
        theme.style(HighlightGroup::Normal)
    }
}

fn body_style(line: &str, theme: &Theme) -> Style {
    if line.starts_with("@@") {
        theme.style(HighlightGroup::Muted)
    } else if line.starts_with('+') {
        theme.style(HighlightGroup::Success)
    } else if line.starts_with('-') {
        theme.style(HighlightGroup::Error)
    } else {
        theme.style(HighlightGroup::Normal)
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut AppApprovalState,
    others: usize,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let keys = if state.reason.is_some() {
        "Enter refuse · Esc back"
    } else {
        "a approve · r refuse · j/k PgUp/PgDn scroll · Tab the next one · Esc decide later"
    };
    let block = crate::ui::frame::block(
        theme,
        BorderSurface::Modal,
        "a version of an app waits for your answer",
        true,
    )
    .title_bottom(crate::ui::frame::hint_line(theme, keys));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(review) = &state.review else {
        let text = state.unreadable.clone().unwrap_or_else(|| {
            "That version no longer waits: it was answered, or replaced by a later one. Press Esc."
                .to_owned()
        });
        frame.render_widget(
            Paragraph::new(sanitize::single_line(&text))
                .style(theme.style(HighlightGroup::Normal))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let (head, body) = split(review);
    let width = usize::from(inner.width);
    let mut shown: Vec<Line> = head
        .iter()
        .map(|line| {
            Line::from(Span::styled(
                sanitize::single_line(line),
                head_style(line, theme),
            ))
        })
        .collect();
    if others > 0 {
        shown.push(Line::from(Span::styled(
            format!("{others} more wait (Tab)"),
            theme.style(HighlightGroup::Muted),
        )));
    }
    shown.push(Line::from(Span::styled(
        "─".repeat(width),
        theme.style(HighlightGroup::Muted),
    )));
    // The brief and the difference, wrapped: a line goes on in rows marked
    // so, and nothing is cut.
    let gutter = 2;
    let mut rows: Vec<(bool, String, Style)> = Vec::new();
    for line in &body {
        let style = body_style(line, theme);
        for row in command_rows(line, width.saturating_sub(gutter)) {
            let text = row
                .parts
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>();
            rows.push((row.line.is_some(), text, style));
        }
    }
    let foot = 1 + usize::from(state.reason.is_some()) + usize::from(state.notice.is_some());
    let page = usize::from(inner.height)
        .saturating_sub(shown.len() + foot)
        .max(1);
    state.page = page;
    state.rows = rows.len();
    state.scroll = state.scroll.min(rows.len().saturating_sub(page));
    let end = (state.scroll + page).min(rows.len());
    if end == rows.len() && usize::from(inner.height) > shown.len() + foot {
        state.seen_end = true;
    }
    for (starts, text, style) in &rows[state.scroll..end] {
        shown.push(Line::from(vec![
            Span::styled(
                if *starts { "  " } else { "┆ " },
                theme.style(HighlightGroup::Muted),
            ),
            Span::styled(text.clone(), *style),
        ]));
    }
    let muted = theme.style(HighlightGroup::Muted);
    shown.push(Line::from(Span::styled(
        if rows.len() <= page {
            "that is the whole version".to_owned()
        } else if state.seen_end && end == rows.len() {
            format!("rows {}–{end} of {}: the end", state.scroll + 1, rows.len())
        } else {
            format!(
                "rows {}–{end} of {} · scroll to the end before you approve (j, PgDn)",
                state.scroll + 1,
                rows.len()
            )
        },
        muted,
    )));
    if let Some(reason) = &state.reason {
        shown.push(Line::from(format!(
            "refuse, and say why (optional): {}▏",
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
    use ratatui::{Terminal, backend::TestBackend};
    use uuid::Uuid;

    use super::*;
    use crate::apps::store::{
        Proposer,
        diff::difference,
        review::{Running, TypesChange},
    };

    fn review(lines: usize) -> Review {
        let old = "local app = { api = 1, types = { \"card\" } }\nreturn app\n";
        let mut new = String::from("local app = { api = 1, types = { \"card\", \"vote\" } }\n");
        for line in 0..lines {
            new.push_str(&format!("-- line {line}\n"));
        }
        new.push_str("return app\n");
        Review {
            installed_from: Some(Proposer::owner()),
            offer: None,
            community: Uuid::nil(),
            name: "tasks".into(),
            title: "Tasks".into(),
            version: 2,
            hash: "58e1".repeat(16),
            from: Proposer::agent("e5".repeat(32), "Scout"),
            proposed_at: 1_700_000_000,
            script_bytes: new.len() as u64,
            brief_bytes: 20,
            brief: "# Tasks\n\nCards and votes.\n".into(),
            brief_changed: true,
            types: vec!["card".into(), "vote".into()],
            types_change: TypesChange::Changed {
                added: vec!["vote".into()],
                removed: vec![],
            },
            running: Some(Running {
                version: 1,
                hash: "9f0e".repeat(16),
                title: "Tasks".into(),
                script_bytes: old.len() as u64,
                brief_bytes: 20,
                types: vec!["card".into()],
            }),
            running_unreadable: None,
            difference: difference(old, &new),
            hidden_characters: 0,
        }
    }

    fn screen(state: &mut AppApprovalState, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state, 1, &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The head (who, the hash, the types with their change) is always on
    /// screen; the brief and the difference scroll below it, and the end
    /// must have been seen before a yes is taken.
    #[test]
    fn the_head_stays_and_the_end_must_be_seen() {
        let mut state = AppApprovalState::default();
        state.show("tasks", 2, Ok(review(60)));
        let shown = screen(&mut state, 100, 30);
        for expected in [
            "┌ a version of an app waits for your answer ─",
            " a approve · r refuse ",
            "tasks (Tasks), version 2",
            "From:     your agent Scout (e5e5",
            &format!("Hash:     {}", "58e1".repeat(16)),
            "Brief:    20 bytes, CHANGED from the version that runs",
            "Record types: card, vote",
            "CHANGED from the version that runs:",
            "+ vote   new: the app could not read or write these records before",
            "1 more wait (Tab)",
            "scroll to the end before you approve (j, PgDn)",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(!state.seen_end, "{shown}");
        assert!(!shown.contains("+return app"), "{shown}");
        state.scroll_by(isize::MAX);
        let shown = screen(&mut state, 100, 30);
        assert!(state.seen_end, "{shown}");
        assert!(shown.contains("+-- line 59"), "{shown}");
        assert!(shown.contains(": the end"), "{shown}");
        // The head is still there at the end.
        assert!(
            shown.contains("CHANGED from the version that runs:"),
            "{shown}"
        );
        assert!(shown.contains("Hash:"), "{shown}");
        // Back up, it stays seen.
        state.scroll_by(isize::MIN);
        screen(&mut state, 100, 30);
        assert!(state.seen_end);
        // A short version is seen whole at once.
        let mut short = AppApprovalState::default();
        short.show("tasks", 2, Ok(review(1)));
        let shown = screen(&mut short, 100, 40);
        assert!(
            short.seen_end && shown.contains("that is the whole version"),
            "{shown}"
        );
        // One that no longer waits takes no answer and says so.
        short.review = None;
        let shown = screen(&mut short, 100, 20);
        assert!(shown.contains("That version no longer waits"), "{shown}");
    }

    /// An offer from the channel (M6): who signed this exact revision, by
    /// name and key, and a source other than the one the installed
    /// version came from, marked as a change in the head.
    #[test]
    fn an_offer_says_who_signed_this_revision_and_a_new_source_plainly() {
        let mut offered = review(1);
        let channel = Uuid::from_u128(5);
        offered.from = Proposer::member("b0".repeat(32), "Bob").in_channel(channel);
        offered.installed_from = Some(Proposer::member("a0".repeat(32), "Ana").in_channel(channel));
        offered.offer = Some(crate::apps::store::review::OfferFacts {
            artifact: Uuid::from_u128(9),
            revision: "7e".repeat(32),
        });
        let mut state = AppApprovalState::default();
        state.show("tasks", 2, Ok(offered.clone()));
        let shown = screen(&mut state, 200, 40);
        for expected in [
            &format!(
                "From:     Bob, a person of the channel ({})",
                "b0".repeat(32)
            ),
            &format!(
                "Offer:    shared in the channel; this exact revision ({}…) is signed by Bob",
                "7e".repeat(8)
            ),
            &format!(
                "Source:   CHANGED: the version installed here came from Ana, a person of the channel ({})",
                "a0".repeat(32)
            ),
        ] {
            assert!(
                shown.contains(expected.as_str()),
                "`{expected}` in:\n{shown}"
            );
        }
        // The same person as the installed version: no change of source.
        offered.installed_from = Some(offered.from.clone());
        state.show("tasks", 2, Ok(offered));
        let shown = screen(&mut state, 200, 40);
        assert!(!shown.contains("Source:"), "{shown}");
    }

    #[test]
    fn the_banner_says_which_app_which_version_and_from_whom() {
        let version = crate::store::apps::AppVersion {
            community: Uuid::nil(),
            name: "tasks".into(),
            version: 2,
            state: crate::store::apps::VersionState::Waiting,
            detail: None,
            title: Some("Tasks".into()),
            api: Some(1),
            types: vec!["card".into()],
            script_sha256: "0".repeat(64),
            manifest_sha256: None,
            brief_sha256: "0".repeat(64),
            script_bytes: 1,
            brief_bytes: 1,
            proposer: Proposer::agent("e5".repeat(32), "Sc\u{1b}[2Jout"),
            proposed_at: 1,
            decided_at: None,
            kept: true,
            told_at: None,
        };
        let text = banner_text(&version, 3);
        assert!(
            text.starts_with(" tasks: version 2 from your agent Sc"),
            "{text}"
        );
        assert!(
            text.contains("waits for your approval · and 2 more · Ctrl-Y to see and answer it"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'), "{text}");
    }
}
