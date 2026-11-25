//! The workspace's panes, and what the shell draws and measures around them
//! (v0.17 L10, L11): the words a hidden pane leaves on the conversation's
//! frame, the borders a drag resizes, the status line's hint.
//!
//! `draw_hidden` is a small frame helper of general use (a quiet line at
//! the right of a frame's top edge, made of key hints); it lives here so
//! the frame module of another task is not touched.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr as _;

use crate::ui::{
    layout::{MIN_CONVERSATION, Panes, SIDEBAR_MIN_WIDTH, THREAD_MIN_WIDTH},
    theme::{HighlightGroup, Theme},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    Communities,
    Channels,
    Timeline,
    Thread,
}

impl Pane {
    pub const fn next(self) -> Self {
        match self {
            Self::Communities => Self::Channels,
            Self::Channels => Self::Timeline,
            Self::Timeline => Self::Thread,
            Self::Thread => Self::Communities,
        }
    }
}

/// The widest `ui.sidebar_width` and `ui.thread_width` take (`Config`).
const SIDEBAR_MAX_WIDTH: u16 = 60;
const THREAD_MAX_WIDTH: u16 = 80;

/// The columns of text a title takes on screen.
pub fn text_width(text: &str) -> u16 {
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}

/// The borders a drag resizes: between the sidebar and the conversation,
/// and between the conversation and the thread. Each is the two columns of
/// the two frames that meet there. The rail and the app keep their widths.
pub fn splitters(panes: &Panes) -> Vec<(Pane, Rect)> {
    let mut borders = Vec::new();
    if panes.timeline.width == 0 {
        return borders;
    }
    if let Some(sidebar) = panes.sidebar
        && sidebar.right() == panes.timeline.x
    {
        borders.push((
            Pane::Channels,
            Rect::new(
                sidebar.right().saturating_sub(1),
                sidebar.y,
                2,
                sidebar.height,
            ),
        ));
    }
    if let Some(thread) = panes.thread
        && thread.x == panes.timeline.right()
    {
        borders.push((
            Pane::Thread,
            Rect::new(thread.x.saturating_sub(1), thread.y, 2, thread.height),
        ));
    }
    borders
}

/// The width a pane takes when its border is dragged to `column`, inside
/// its own limits (those of `ui.sidebar_width` and `ui.thread_width`) and
/// the layout's: the conversation keeps its [`MIN_CONVERSATION`] columns.
pub fn dragged_width(panes: &Panes, pane: Pane, column: u16) -> Option<u16> {
    let spare = panes.timeline.width.checked_sub(MIN_CONVERSATION)?;
    let (wanted, current, least, most) = match pane {
        Pane::Channels => {
            let sidebar = panes.sidebar?;
            (
                column.saturating_add(1).saturating_sub(sidebar.x),
                sidebar.width,
                SIDEBAR_MIN_WIDTH,
                SIDEBAR_MAX_WIDTH,
            )
        }
        Pane::Thread => {
            let thread = panes.thread?;
            (
                thread.right().saturating_sub(column),
                thread.width,
                THREAD_MIN_WIDTH,
                THREAD_MAX_WIDTH,
            )
        }
        Pane::Communities | Pane::Timeline => return None,
    };
    let most = most.min(current.saturating_add(spare));
    (most >= least).then(|| wanted.clamp(least, most))
}

/// Where the status line's `Ctrl-K actions · ? keys` hint was drawn, read
/// back from the frame, so the hint's place stays the status line's own.
pub fn find_hint(buffer: &Buffer, status: Rect) -> Option<Rect> {
    let row = status.y;
    let symbols = (status.x..status.right())
        .map(|x| buffer.cell((x, row)).map_or("", |cell| cell.symbol()))
        .collect::<Vec<_>>();
    let matches_at = |start: usize, word: &str| {
        word.chars().enumerate().all(|(offset, character)| {
            symbols
                .get(start + offset)
                .is_some_and(|symbol| symbol.chars().eq(std::iter::once(character)))
        })
    };
    let start = (0..symbols.len()).find(|start| matches_at(*start, "Ctrl-K"))?;
    let end = (start..symbols.len())
        .find(|end| matches_at(*end, "keys"))
        .map_or(start + "Ctrl-K actions".len(), |end| end + "keys".len())
        .min(symbols.len());
    let x = status.x.saturating_add(u16::try_from(start).ok()?);
    let width = u16::try_from(end - start).ok()?;
    Some(Rect::new(x, row, width, 1))
}

/// Writes what is hidden at the right of `pane`'s top edge, after a title
/// `title_width` wide: `hidden · channels 2 · thread 4`, each name then the
/// key that shows it (a digit before a name would read as a count). When
/// it no longer fits `hidden ·` goes, then the names, then the keys from
/// the last.
pub fn draw_hidden(
    buffer: &mut Buffer,
    pane: Rect,
    title_width: u16,
    keys: &[(&str, &str)],
    theme: &Theme,
) {
    if pane.height == 0 || pane.width < 4 {
        return;
    }
    // ` title ` starts one column in; keep a column of the edge either side.
    let left = pane
        .x
        .saturating_add(1)
        .saturating_add(title_width.saturating_add(2).min(pane.width / 2))
        .saturating_add(1);
    let right = pane.right().saturating_sub(1);
    let room = usize::from(right.saturating_sub(left));
    let line = |pairs: &[(&str, &str)], named: bool, said: bool| {
        let muted = theme.style(HighlightGroup::HintText);
        let mut spans = vec![Span::styled(if said { " hidden · " } else { " " }, muted)];
        for (index, (key, label)) in pairs.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" · ", theme.style(HighlightGroup::Muted)));
            }
            if named {
                spans.push(Span::styled(format!("{label} "), muted));
            }
            spans.push(Span::styled(
                (*key).to_owned(),
                theme.style(HighlightGroup::HintKey),
            ));
        }
        spans.push(Span::raw(" "));
        Line::from(spans)
    };
    let mut pairs = keys.to_vec();
    let mut shown = line(&pairs, true, true);
    if shown.width() > room {
        shown = line(&pairs, true, false);
    }
    if shown.width() > room {
        shown = line(&pairs, false, false);
    }
    while shown.width() > room && !pairs.is_empty() {
        pairs.pop();
        shown = line(&pairs, false, false);
    }
    if pairs.is_empty() {
        return;
    }
    let width = u16::try_from(shown.width()).unwrap_or(u16::MAX);
    buffer.set_line(right.saturating_sub(width), pane.y, &shown, width);
}

#[cfg(test)]
mod tests {
    use super::{Pane, dragged_width, draw_hidden, find_hint, splitters};
    use crate::ui::{
        layout::{Shell, SidePane, workspace},
        theme::Theme,
    };
    use ratatui::{buffer::Buffer, layout::Rect};

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn the_borders_between_panes_are_where_the_frames_meet() {
        let panes = workspace(
            Rect::new(0, 0, 160, 40),
            Shell {
                thread: true,
                ..Shell::new(28, 44)
            },
        );
        let borders = splitters(&panes);
        let sidebar = panes.sidebar.unwrap();
        let thread = panes.thread.unwrap();
        assert_eq!(
            borders,
            vec![
                (
                    Pane::Channels,
                    Rect::new(sidebar.right() - 1, 0, 2, sidebar.height)
                ),
                (Pane::Thread, Rect::new(thread.x - 1, 0, 2, thread.height)),
            ]
        );
        // Dragged, inside the limits: 18 to 60 for the sidebar, and the
        // conversation keeps 38.
        assert_eq!(
            dragged_width(&panes, Pane::Channels, sidebar.x + 33),
            Some(34)
        );
        assert_eq!(dragged_width(&panes, Pane::Channels, sidebar.x), Some(18));
        assert_eq!(
            dragged_width(&panes, Pane::Channels, 150),
            Some(28 + panes.timeline.width - 38)
        );
        assert_eq!(dragged_width(&panes, Pane::Thread, thread.x - 10), Some(54));
        assert_eq!(dragged_width(&panes, Pane::Thread, 159), Some(30));
        assert_eq!(
            dragged_width(&panes, Pane::Thread, 0),
            Some(44 + panes.timeline.width - 38)
        );
        assert_eq!(dragged_width(&panes, Pane::Communities, 10), None);
        // The pane in the conversation's place has no border to drag.
        let narrow = workspace(
            Rect::new(0, 0, 60, 24),
            Shell {
                thread: true,
                focus: SidePane::Thread,
                ..Shell::new(28, 44)
            },
        );
        assert!(splitters(&narrow).is_empty());
    }

    #[test]
    fn the_hint_is_found_where_the_status_line_drew_it() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 3));
        buffer.set_string(
            0,
            2,
            "NORMAL  #general  ● online   Ctrl-K actions · ? keys",
            ratatui::style::Style::default(),
        );
        let found = find_hint(&buffer, Rect::new(0, 2, 80, 1)).unwrap();
        assert_eq!(
            &row(&buffer, 2)[..]
                .chars()
                .skip(usize::from(found.x))
                .take(usize::from(found.width))
                .collect::<String>(),
            "Ctrl-K actions · ? keys"
        );
        assert_eq!(find_hint(&buffer, Rect::new(0, 1, 80, 1)), None);
    }

    #[test]
    fn what_is_hidden_is_named_at_the_right_of_the_frame_and_shortens() {
        let theme = Theme::default();
        let keys = [("2", "channels"), ("4", "thread")];
        let mut wide = Buffer::empty(Rect::new(0, 0, 60, 3));
        draw_hidden(&mut wide, Rect::new(0, 0, 60, 3), 8, &keys, &theme);
        assert!(
            row(&wide, 0).ends_with(" hidden · channels 2 · thread 4  "),
            "{}",
            row(&wide, 0)
        );
        let mut middle = Buffer::empty(Rect::new(0, 0, 44, 3));
        draw_hidden(&mut middle, Rect::new(0, 0, 44, 3), 8, &keys, &theme);
        let shown = row(&middle, 0);
        assert!(
            shown.ends_with(" channels 2 · thread 4  ") && !shown.contains("hidden"),
            "{shown}"
        );
        let mut narrow = Buffer::empty(Rect::new(0, 0, 30, 3));
        draw_hidden(&mut narrow, Rect::new(0, 0, 30, 3), 14, &keys, &theme);
        assert!(row(&narrow, 0).ends_with(" 2 · 4  "), "{}", row(&narrow, 0));
        let mut least = Buffer::empty(Rect::new(0, 0, 18, 3));
        draw_hidden(&mut least, Rect::new(0, 0, 18, 3), 14, &keys, &theme);
        assert!(row(&least, 0).ends_with(" 2  "), "{}", row(&least, 0));
    }
}
