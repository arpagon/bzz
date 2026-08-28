use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    domain::ConnectionState,
    ui::{
        theme::{HighlightGroup, Theme},
        typing::truncate_cells,
    },
};

pub fn connection_label(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Locked => "identity locked",
        ConnectionState::IdentityMissing => "identity missing",
        ConnectionState::IdentityCorrupt => "identity corrupt",
        ConnectionState::Offline => "offline cache",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Authenticating => "authenticating",
        ConnectionState::Online => "online",
        ConnectionState::Backfilling => "backfilling",
        ConnectionState::AccessDenied => "access denied",
        ConnectionState::ClockSkew => "clock skew",
    }
}

#[derive(Clone, Copy)]
pub struct StatusBarView<'a> {
    pub mode: &'a str,
    pub mode_group: HighlightGroup,
    pub activity: Option<&'a str>,
    pub notice: &'a str,
    pub connection: &'a str,
    pub media: &'a str,
}

/// Renders one responsive row of adjacent, theme-owned status nuggets.
///
/// The mode and active-agent segment take priority. Connection, graphics, and
/// help remain right-aligned when they fit; no value wraps into a second row.
pub fn render(frame: &mut Frame<'_>, area: Rect, theme: &Theme, view: StatusBarView<'_>) {
    let total_width = usize::from(area.width);
    if total_width == 0 {
        return;
    }
    let fit = |value: &str, width: usize| {
        let mut output = truncate_cells(value, width);
        let padding = width.saturating_sub(output.as_str().width());
        output.push_str(&" ".repeat(padding));
        output
    };

    let mode_text = truncate_cells(&format!(" {} ", view.mode), total_width.min(16));
    let mode_width = mode_text.as_str().width();
    let minimum_middle = view
        .activity
        .map(|activity| activity.width().saturating_add(2).min(18))
        .unwrap_or_default()
        .max(usize::from(!view.notice.is_empty()));

    let connection = format!(" {} ", view.connection);
    let media = format!(" {} ", view.media.to_uppercase());
    let help = " ? help · q quit ".to_owned();
    let mut right = Vec::new();
    let mut right_width = 0_usize;
    for (minimum_terminal_width, text, group) in [
        (0_usize, connection, HighlightGroup::StatusConnection),
        (44, media, HighlightGroup::StatusMedia),
        (72, help, HighlightGroup::StatusBar),
    ] {
        let width = text.as_str().width();
        if total_width >= minimum_terminal_width
            && mode_width
                .saturating_add(right_width)
                .saturating_add(width)
                .saturating_add(minimum_middle)
                <= total_width
        {
            right_width = right_width.saturating_add(width);
            right.push((text, group));
        }
    }

    let middle_width = total_width.saturating_sub(mode_width.saturating_add(right_width));
    let activity_cap = if view.notice.is_empty() {
        middle_width
    } else if middle_width >= 18 {
        middle_width / 2
    } else {
        0
    };
    let activity = view.activity.and_then(|activity| {
        let width = activity_cap.checked_sub(2)?;
        let activity = truncate_cells(activity, width);
        (!activity.is_empty()).then(|| format!(" {activity} "))
    });
    let activity_width = activity.as_deref().map_or(0, UnicodeWidthStr::width);
    let notice_width = middle_width.saturating_sub(activity_width);
    let notice_text = if view.notice.is_empty() {
        " ".repeat(notice_width)
    } else {
        fit(&format!(" {}", view.notice), notice_width)
    };

    let mut spans = Vec::with_capacity(3 + right.len());
    spans.push(Span::styled(mode_text, theme.style(view.mode_group)));
    if let Some(activity) = activity {
        spans.push(Span::styled(
            activity,
            theme.style(HighlightGroup::StatusAgent),
        ));
    }
    spans.push(Span::styled(
        notice_text,
        theme.style(HighlightGroup::StatusBar),
    ));
    spans.extend(
        right
            .into_iter()
            .map(|(text, group)| Span::styled(text, theme.style(group))),
    );
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme.style(HighlightGroup::StatusBar)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::{StatusBarView, render};
    use crate::ui::theme::{HighlightGroup, Theme};

    fn row(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn wide_and_narrow_status_stay_on_one_row_with_priority_segments() {
        let theme = Theme::default();
        let view = StatusBarView {
            mode: "NORMAL",
            mode_group: HighlightGroup::StatusMode,
            activity: Some("◆ Fizz ⠹"),
            notice: "",
            connection: "online",
            media: "kitty",
        };
        let mut wide = Terminal::new(TestBackend::new(100, 1)).unwrap();
        wide.draw(|frame| render(frame, frame.area(), &theme, view))
            .unwrap();
        let wide = row(&wide);
        assert!(wide.contains(" NORMAL "));
        assert!(wide.contains(" ◆ Fizz ⠹ "));
        assert!(wide.contains(" online "));
        assert!(wide.contains(" KITTY "));
        assert!(wide.contains("? help · q quit"));

        let mut narrow = Terminal::new(TestBackend::new(32, 1)).unwrap();
        narrow
            .draw(|frame| render(frame, frame.area(), &theme, view))
            .unwrap();
        let narrow = row(&narrow);
        assert!(narrow.contains("NORMAL"));
        assert!(narrow.contains("Fizz"));
        assert!(narrow.contains("online"));
        assert!(!narrow.contains("KITTY"));
        assert!(!narrow.contains("help"));
    }

    #[test]
    fn notice_and_activity_share_the_flexible_segment_without_overflow() {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &theme,
                    StatusBarView {
                        mode: "INSERT",
                        mode_group: HighlightGroup::StatusModeInsert,
                        activity: Some("◆ A very long generated agent label ⠋"),
                        notice: "generated notice without private content",
                        connection: "online",
                        media: "kitty",
                    },
                );
            })
            .unwrap();
        let row = row(&terminal);
        assert_eq!(row.chars().count(), 60);
        assert!(row.contains('◆'));
        assert!(row.contains("generated"));
    }
}
