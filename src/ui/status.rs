//! The status line: the mode as a label, the channel, what is happening,
//! the transient notice, whether bzz is online, and one hint.
//!
//! Every part has a priority. When the window is narrow the lowest
//! priority drops first, whole: no part wraps or is cut in the middle of a
//! word, except the notice, which takes the room that is left. Two parts are
//! slots that other parts of bzz fill: [`StatusBarView::huddle_slot`] (left
//! of centre: who speaks in a huddle) and [`StatusBarView::mark_slot`] (at
//! the right: a mark such as `DEMO · generated, offline`).

use std::time::Duration;

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use tokio::time::Instant;
use unicode_width::UnicodeWidthStr;

use crate::{
    domain::ConnectionState,
    ui::{
        action_table::{ActionSpec, FooterContext, TABLE},
        keymap::{KeyMap, KeyScope, UiAction},
        theme::{HighlightGroup, Theme},
        typing::truncate_cells,
    },
};

pub fn connection_label(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Locked => "identity locked",
        ConnectionState::IdentityMissing => "identity missing",
        ConnectionState::IdentityCorrupt => "identity corrupt",
        ConnectionState::Offline => "offline",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Authenticating => "authenticating",
        ConnectionState::Online => "online",
        ConnectionState::Backfilling => "backfilling",
        ConnectionState::AccessDenied => "access denied",
        ConnectionState::ClockSkew => "clock skew",
    }
}

/// The colour of the dot before the connection: online, on its way, or
/// stopped until someone acts.
pub const fn connection_group(state: ConnectionState) -> HighlightGroup {
    match state {
        ConnectionState::Online | ConnectionState::Backfilling => HighlightGroup::StatusOnline,
        ConnectionState::Offline
        | ConnectionState::Connecting
        | ConnectionState::Authenticating => HighlightGroup::StatusOffline,
        ConnectionState::Locked
        | ConnectionState::IdentityMissing
        | ConnectionState::IdentityCorrupt
        | ConnectionState::AccessDenied
        | ConnectionState::ClockSkew => HighlightGroup::Error,
    }
}

/// Priorities of the parts of the status line: the lowest drops first.
pub mod priority {
    /// The mode label never drops; it is cut last.
    pub const MODE: u8 = u8::MAX;
    /// What a slot that must always show asks for (the demo's mark).
    pub const ALWAYS: u8 = 250;
    /// How to leave an app that has the keyboard.
    pub const APP_LEAVE: u8 = 210;
    pub const CONNECTION: u8 = 200;
    /// An agent typing, an app's own line.
    pub const ACTIVITY: u8 = 150;
    /// A notice: parts below it make room for it while it is shown.
    pub const NOTICE: u8 = 145;
    /// The default of the huddle slot.
    pub const HUDDLE: u8 = 140;
    pub const CHANNEL: u8 = 100;
    pub const HINT: u8 = 80;
}

/// A part of the status line that another part of bzz fills.
#[derive(Clone, Debug, Default)]
pub struct Slot<'a> {
    pub line: Option<Line<'a>>,
    pub priority: u8,
    /// Shorter forms of the line, longest first: taken in turn before the
    /// slot is dropped.
    pub shorter: Vec<Line<'a>>,
}

impl<'a> Slot<'a> {
    pub fn new(line: Line<'a>, priority: u8) -> Self {
        Self {
            line: Some(line),
            priority,
            shorter: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StatusBarView<'a> {
    pub mode: &'a str,
    pub mode_group: HighlightGroup,
    /// The conversation in view: `#general`, `@ada`.
    pub channel: Option<&'a str>,
    pub activity: Option<&'a str>,
    pub notice: &'a str,
    pub connection: ConnectionState,
    /// The mark says what the connection is (the demo's `generated,
    /// offline`): the connection is then not shown beside it.
    pub mark_is_connection: bool,
    /// An app has the keyboard: `?` and `q` are its keys, so the hint
    /// names the one key bzz keeps, and that hint stays when the row is
    /// short.
    pub app_focused: bool,
    /// Left of centre: the huddle strip.
    pub huddle: Slot<'a>,
    /// At the right, before the connection: a mark such as
    /// `DEMO · generated, offline`.
    pub mark: Slot<'a>,
    /// The hint for where the focus is ([`footer`], v0.17 C10); without one
    /// the line shows [`hint`].
    pub hint: Option<Slot<'a>>,
}

impl<'a> StatusBarView<'a> {
    /// A view with nothing in its slots.
    pub fn new(mode: &'a str, connection: ConnectionState) -> Self {
        Self {
            mode,
            mode_group: HighlightGroup::StatusMode,
            channel: None,
            activity: None,
            notice: "",
            connection,
            mark_is_connection: false,
            app_focused: false,
            huddle: Slot::default(),
            mark: Slot::default(),
            hint: None,
        }
    }

    /// Fills the slot left of centre (the huddle strip).
    pub fn huddle_slot(&mut self, line: Line<'a>, priority: u8) {
        self.huddle = Slot::new(line, priority);
    }

    /// Fills the slot left of centre with forms of the strip from longest
    /// to shortest; a narrow row takes a shorter form before dropping it.
    pub fn huddle_slot_shortening(&mut self, mut lines: Vec<Line<'a>>, priority: u8) {
        if lines.is_empty() {
            return;
        }
        let line = lines.remove(0);
        self.huddle = Slot {
            line: Some(line),
            priority,
            shorter: lines,
        };
    }

    /// Fills the slot at the right (a mark such as the demo's).
    pub fn mark_slot(&mut self, line: Line<'a>, priority: u8) {
        self.mark = Slot::new(line, priority);
    }
}

/// The status line's hint while an app has the keyboard.
pub const APP_LEAVE_HINT: &str = " Ctrl-c leaves the app ";

/// The hint at the right: the two keys that lead to everything else.
pub fn hint(theme: &Theme) -> Line<'static> {
    crate::ui::frame::hints(theme, &[("Ctrl-K", "actions"), ("?", "keys")])
}

/// How many keys the hint names before the palette's.
const FOOTER_KEYS: usize = 3;

/// A key as the status line writes it: `Ctrl-K`, `Space p`, `Enter`.
fn footer_key(keymap: &KeyMap, scope: KeyScope, action: UiAction) -> String {
    crate::ui::palette::key_label(keymap, scope, action).replace('+', "-")
}

/// The keys of the hint where the focus is `context` (v0.17 C10): the rows
/// of the registry that are a hint there and can run now (`can_run`), lowest
/// rank first, three at most, each with the key the live key map gives it;
/// then the palette's key, where the palette opens.
pub fn footer_keys(
    keymap: &KeyMap,
    context: FooterContext,
    can_run: impl Fn(&ActionSpec) -> bool,
) -> Vec<(String, &'static str)> {
    let scope = context.scope();
    let mut rows = TABLE
        .iter()
        .flat_map(|spec| {
            spec.hints
                .iter()
                .filter(move |hint| hint.context == context)
                .map(move |hint| (hint, spec))
        })
        .filter(|(_, spec)| can_run(spec))
        .collect::<Vec<_>>();
    rows.sort_by_key(|(hint, _)| hint.rank);
    let mut keys: Vec<(String, &'static str)> = Vec::new();
    for (hint, spec) in rows {
        let key = match (hint.key, spec.key_action()) {
            (Some(key), _) => key.to_owned(),
            (None, Some(action)) => footer_key(keymap, scope, action),
            (None, None) => String::new(),
        };
        if key.is_empty() || keys.iter().any(|(known, _)| *known == key) {
            continue;
        }
        keys.push((key, hint.label));
        if keys.len() == FOOTER_KEYS {
            break;
        }
    }
    keys
}

/// The hint where the focus is: [`footer_keys`] and the palette's key, and
/// shorter forms down to `Ctrl-K actions · ? keys` for a narrow window.
/// Read from the registry and the live key map, so a user's binding shows.
/// It is drawn with the frame that drew the rest: it asks for none.
pub fn footer(
    theme: &Theme,
    keymap: &KeyMap,
    context: FooterContext,
    can_run: impl Fn(&ActionSpec) -> bool,
) -> Slot<'static> {
    let scope = context.scope();
    let keys = footer_keys(keymap, context, can_run);
    let palette = footer_key(keymap, scope, UiAction::OpenPalette);
    let help = footer_key(keymap, scope, UiAction::OpenHelp);
    let mut tail = Vec::new();
    if !palette.is_empty() {
        tail.push((palette.clone(), "actions"));
    }
    let mut base = tail.clone();
    if !help.is_empty() {
        base.push((help, "keys"));
    }
    let line = |pairs: &[(String, &'static str)]| {
        let pairs = pairs
            .iter()
            .map(|(key, label)| (key.as_str(), *label))
            .collect::<Vec<_>>();
        crate::ui::frame::hints(theme, &pairs)
    };
    // A shorter form loses the padding the status line keeps from the first.
    let bare = |pairs: &[(String, &'static str)]| {
        let mut shorter = line(pairs);
        shorter.spans.remove(0);
        shorter.spans.pop();
        shorter
    };
    if keys.is_empty() {
        return Slot::new(line(&base), priority::HINT);
    }
    let with = |count: usize| {
        keys.iter()
            .take(count)
            .cloned()
            .chain(tail.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut shorter = (1..keys.len())
        .rev()
        .map(|count| bare(&with(count)))
        .collect::<Vec<_>>();
    if !base.is_empty() {
        shorter.push(bare(&base));
    }
    Slot {
        line: Some(line(&with(keys.len()))),
        priority: priority::HINT,
        shorter,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Left,
    Right,
}

struct Part<'a> {
    line: Line<'a>,
    priority: u8,
    side: Side,
    /// Shorter forms, taken before the part is dropped.
    shorter: Vec<Line<'a>>,
}

/// Drops the part at `index`, or puts its next shorter form in its place.
fn shorten_or_drop(parts: &mut Vec<Part<'_>>, index: usize) {
    let part = &mut parts[index];
    if part.shorter.is_empty() {
        parts.remove(index);
        return;
    }
    let mut line = part.shorter.remove(0);
    if let Some(first) = part.line.spans.first().cloned()
        && first.content == " "
    {
        // Keep the padding the part was given.
        line.spans.insert(0, first);
        if let Some(last) = part.line.spans.last().cloned() {
            line.spans.push(last);
        }
    }
    part.line = line;
}

/// Renders the status line on one row.
pub fn render(frame: &mut Frame<'_>, area: Rect, theme: &Theme, view: StatusBarView<'_>) {
    let total = usize::from(area.width);
    if total == 0 {
        return;
    }
    let line = layout(theme, view, total);
    frame.render_widget(
        Paragraph::new(line).style(theme.style(HighlightGroup::StatusBar)),
        area,
    );
}

fn layout<'a>(theme: &Theme, view: StatusBarView<'a>, total: usize) -> Line<'a> {
    let bar = theme.style(HighlightGroup::StatusBar);
    let mut parts = Vec::new();
    let mode = truncate_cells(&format!(" {} ", view.mode), total.min(16));
    let mode_width = mode.as_str().width();
    if let Some(channel) = view.channel.filter(|channel| !channel.is_empty()) {
        parts.push(Part {
            line: Line::from(Span::styled(
                format!(" {} ", truncate_cells(channel, 24)),
                theme.style(HighlightGroup::StatusChannel),
            )),
            priority: priority::CHANNEL,
            side: Side::Left,
            shorter: Vec::new(),
        });
    }
    if let Some(line) = view.huddle.line {
        let mut spans = vec![Span::styled(" ", bar)];
        spans.extend(line.spans);
        spans.push(Span::styled(" ", bar));
        parts.push(Part {
            line: Line::from(spans),
            priority: view.huddle.priority,
            side: Side::Left,
            shorter: view.huddle.shorter,
        });
    }
    if let Some(activity) = view.activity.filter(|activity| !activity.is_empty()) {
        parts.push(Part {
            line: Line::from(Span::styled(
                format!(" {} ", truncate_cells(activity, 32)),
                theme.style(HighlightGroup::StatusAgent),
            )),
            priority: priority::ACTIVITY,
            side: Side::Left,
            shorter: Vec::new(),
        });
    }
    if let Some(line) = view.mark.line {
        let mut spans = line.spans;
        spans.push(Span::styled(" ", bar));
        parts.push(Part {
            line: Line::from(spans),
            priority: view.mark.priority,
            side: Side::Right,
            shorter: Vec::new(),
        });
    }
    if !view.mark_is_connection {
        parts.push(Part {
            line: Line::from(vec![
                Span::styled(" ● ", theme.style(connection_group(view.connection))),
                Span::styled(format!("{} ", connection_label(view.connection)), bar),
            ]),
            priority: priority::CONNECTION,
            side: Side::Right,
            shorter: Vec::new(),
        });
    }
    parts.push(if view.app_focused {
        Part {
            line: Line::from(Span::styled(APP_LEAVE_HINT, bar)),
            priority: priority::APP_LEAVE,
            side: Side::Right,
            shorter: Vec::new(),
        }
    } else {
        match view.hint {
            Some(Slot {
                line: Some(line),
                priority,
                shorter,
            }) => Part {
                line,
                priority,
                side: Side::Right,
                shorter,
            },
            _ => Part {
                line: hint(theme),
                priority: priority::HINT,
                side: Side::Right,
                shorter: Vec::new(),
            },
        }
    });

    // A notice is shown whole while parts of lower priority can make room
    // for it; then it is cut to what is left, down to a few cells; then it
    // goes. Parts drop lowest priority first while the row is too wide.
    let notice_full = if view.notice.is_empty() {
        0
    } else {
        view.notice.width().saturating_add(1)
    };
    let mut notice_kept = !view.notice.is_empty();
    let lowest = |parts: &[Part<'_>], below: u8| {
        parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.priority < below)
            .min_by_key(|(index, part)| (part.priority, usize::MAX - index))
            .map(|(index, _)| index)
    };
    loop {
        let fixed =
            mode_width.saturating_add(parts.iter().map(|part| part.line.width()).sum::<usize>());
        if !notice_kept || fixed.saturating_add(notice_full) <= total {
            if fixed <= total {
                break;
            }
            match lowest(&parts, u8::MAX) {
                Some(index) => shorten_or_drop(&mut parts, index),
                None => break,
            }
            continue;
        }
        if let Some(index) = lowest(&parts, priority::NOTICE) {
            shorten_or_drop(&mut parts, index);
            continue;
        }
        if fixed.saturating_add(notice_full.min(12)) <= total {
            break;
        }
        notice_kept = false;
    }

    let fixed = mode_width.saturating_add(parts.iter().map(|part| part.line.width()).sum());
    let middle = total.saturating_sub(fixed);
    let mut spans = vec![Span::styled(mode, theme.style(view.mode_group))];
    for part in parts.iter_mut().filter(|part| part.side == Side::Left) {
        spans.append(&mut part.line.spans);
    }
    let notice = if notice_kept && middle > 1 {
        let mut text = truncate_cells(&format!(" {}", view.notice), middle);
        let padding = middle.saturating_sub(text.as_str().width());
        text.push_str(&" ".repeat(padding));
        text
    } else {
        " ".repeat(middle)
    };
    spans.push(Span::styled(notice, bar));
    for part in parts.iter_mut().filter(|part| part.side == Side::Right) {
        spans.append(&mut part.line.spans);
    }
    Line::from(spans)
}

/// How long a notice stays on the status line.
pub const NOTICE_LIFETIME: Duration = Duration::from_secs(5);

/// Expires the transient notice about five seconds after it appears.
///
/// Monotonic time from bzz's clock (`tokio::time::Instant`, see
/// `src/clock.rs`), so tests drive it with paused Tokio time. Nothing runs
/// while a notice waits: the loop sleeps until [`NoticeTimer::deadline`],
/// and asks for one frame when it passes.
#[derive(Clone, Debug, Default)]
pub struct NoticeTimer {
    shown: Option<(String, Instant)>,
}

impl NoticeTimer {
    /// Notes the notice now shown; a new text starts a new lifetime.
    pub fn observe(&mut self, notice: Option<&str>, now: Instant) {
        match notice {
            None => self.shown = None,
            Some(text) => {
                if self.shown.as_ref().is_none_or(|(shown, _)| shown != text) {
                    self.shown = Some((text.to_owned(), now));
                }
            }
        }
    }

    /// When the notice now shown expires.
    pub fn deadline(&self) -> Option<Instant> {
        self.shown
            .as_ref()
            .map(|(_, since)| *since + NOTICE_LIFETIME)
    }

    /// Whether `notice` is the one that expired at `now`; the timer forgets
    /// it then.
    pub fn expire(&mut self, notice: Option<&str>, now: Instant) -> bool {
        let expired = match (&self.shown, notice) {
            (Some((shown, since)), Some(current)) => {
                shown == current && now >= *since + NOTICE_LIFETIME
            }
            _ => false,
        };
        if expired || notice.is_none() {
            self.shown = None;
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{
        Terminal,
        backend::TestBackend,
        text::{Line, Span},
    };
    use tokio::time::Instant;

    use super::{NOTICE_LIFETIME, NoticeTimer, StatusBarView, priority, render};
    use crate::{
        domain::ConnectionState,
        ui::{
            redraw_gate::RedrawGate,
            theme::{HighlightGroup, Theme},
        },
    };

    fn row(view: &StatusBarView<'_>, width: u16) -> String {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &theme, view.clone()))
            .unwrap();
        let row = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(row.chars().count(), usize::from(width), "{row}");
        row
    }

    fn normal() -> StatusBarView<'static> {
        let mut view = StatusBarView::new("NORMAL", ConnectionState::Online);
        view.channel = Some("#general");
        view
    }

    #[test]
    fn at_60_80_and_160_columns_the_line_says_mode_channel_and_online() {
        for width in [60_u16, 80, 160] {
            let row = row(&normal(), width);
            assert!(row.starts_with(" NORMAL "), "{width}: {row}");
            assert!(row.contains(" #general "), "{width}: {row}");
            assert!(row.contains(" ● online "), "{width}: {row}");
            assert!(row.contains("Ctrl-K actions · ? keys"), "{width}: {row}");
            for word in ["HALFBLOCKS", "SIXEL", "KITTY", "q quit"] {
                assert!(!row.contains(word), "{width}: {row}");
            }
        }
    }

    #[test]
    fn the_mode_is_a_label_and_the_dot_is_coloured() {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &theme, normal()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            Some(buffer[(1, 0)].bg),
            theme.style(HighlightGroup::StatusMode).bg
        );
        let dot = (0..80).find(|x| buffer[(*x, 0)].symbol() == "●").unwrap();
        assert_eq!(
            Some(buffer[(dot, 0)].fg),
            theme.style(HighlightGroup::StatusOnline).fg
        );
        let mut offline = normal();
        offline.connection = ConnectionState::Offline;
        assert!(row(&offline, 80).contains(" ● offline "));
    }

    #[test]
    fn when_narrow_the_lowest_priority_drops_first() {
        let mut view = normal();
        view.activity = Some("◆ Fizz ⠹");
        let narrow = row(&view, 40);
        assert!(narrow.contains("NORMAL"), "{narrow}");
        assert!(narrow.contains("Fizz"), "{narrow}");
        assert!(narrow.contains("online"), "{narrow}");
        assert!(narrow.contains("#general"), "{narrow}");
        assert!(!narrow.contains("? keys"), "{narrow}");
        let narrower = row(&view, 30);
        assert!(narrower.contains("Fizz"), "{narrower}");
        assert!(narrower.contains("online"), "{narrower}");
        assert!(!narrower.contains("#general"), "{narrower}");
        let tiny = row(&view, 12);
        assert!(tiny.contains("NORMAL"), "{tiny}");
    }

    #[test]
    fn the_two_slots_take_a_line_and_a_priority() {
        let mut view = normal();
        view.huddle_slot(Line::from(Span::raw("🎧 Ada speaks")), priority::HUDDLE);
        view.mark_slot(
            Line::from(Span::raw("DEMO · generated, offline")),
            priority::ALWAYS,
        );
        let wide = row(&view, 160);
        let huddle = wide.find("Ada speaks").unwrap();
        let mark = wide.find("DEMO").unwrap();
        let online = wide.find("online").unwrap();
        assert!(wide.find("#general").unwrap() < huddle, "{wide}");
        assert!(huddle < mark && mark < online, "{wide}");
        // Narrow: the mark stays, the hint and the channel go first.
        let narrow = row(&view, 60);
        assert!(narrow.contains("DEMO · generated, offline"), "{narrow}");
        assert!(!narrow.contains("? keys"), "{narrow}");
        let narrower = row(&view, 44);
        assert!(narrower.contains("DEMO · generated, offline"), "{narrower}");
        assert!(!narrower.contains("Ada"), "{narrower}");
    }

    /// A narrow row takes the huddle strip's shorter forms before it drops
    /// it.
    #[test]
    fn the_huddle_slot_shortens_before_it_drops() {
        let mut view = normal();
        view.huddle_slot_shortening(
            vec![
                Line::from(Span::raw("🎧 ⣀⣤⣶⣿⣿⣶⣤⣀⣀⣤⣶⣿ Ada · mic muted")),
                Line::from(Span::raw("🎧 ⣀⣤⣶⣿⣿⣶ Ada · mic muted")),
                Line::from(Span::raw("🎧 Ada · mic muted")),
            ],
            priority::HUDDLE,
        );
        let wide = row(&view, 160);
        assert!(wide.contains("⣀⣤⣶⣿⣿⣶⣤⣀⣀⣤⣶⣿ Ada"), "{wide}");
        let narrow = row(&view, 45);
        assert!(narrow.contains(" ⣀⣤⣶⣿⣿⣶ Ada · mic muted"), "{narrow}");
        assert!(!narrow.contains("⣀⣤⣶⣿⣿⣶⣤"), "{narrow}");
        let narrower = row(&view, 40);
        assert!(narrower.contains("Ada · mic muted"), "{narrower}");
        assert!(!narrower.contains('⣀'), "{narrower}");
        let narrowest = row(&view, 30);
        assert!(!narrowest.contains("Ada"), "{narrowest}");
        assert!(narrowest.contains("online"), "{narrowest}");
    }

    /// While an app has the keyboard the status line never offers `?` or
    /// `q`, which are the app's, and says how to leave, down to 40 columns.
    #[test]
    fn with_an_app_focused_the_status_line_says_how_to_leave_it() {
        for width in [40_u16, 60, 80, 160] {
            let mut view = StatusBarView::new("APP", ConnectionState::Online);
            view.notice = "a notice";
            view.app_focused = true;
            let row = row(&view, width);
            assert!(row.contains("Ctrl-c leaves the app"), "{width}: {row}");
            assert!(!row.contains("? keys"), "{width}: {row}");
            assert!(!row.contains("q quit"), "{width}: {row}");
        }
    }

    #[test]
    fn notice_and_activity_share_the_row_without_overflow() {
        let mut view = StatusBarView::new("INSERT", ConnectionState::Online);
        view.mode_group = HighlightGroup::StatusModeInsert;
        view.activity = Some("◆ A very long generated agent label ⠋");
        view.notice = "generated notice without private content";
        let row = row(&view, 60);
        assert!(row.contains('◆'));
        assert!(row.contains("generated"));
        let wide = row_160(&view);
        assert!(wide.contains("generated notice without private content"));
    }

    /// A notice is not cut while the channel and the hint can make room.
    #[test]
    fn a_notice_is_shown_whole_before_the_channel_and_the_hint() {
        let mut view = normal();
        view.notice = "board uninstalled; 2 records of bzz.app.board.* stay in the channel";
        let row = row(&view, 120);
        assert!(row.contains(view.notice), "{row}");
        assert!(row.contains("● online"), "{row}");
        assert!(!row.contains("? keys"), "{row}");
        let wide = row_160(&view);
        assert!(wide.contains(view.notice), "{wide}");
        assert!(
            wide.contains("#general") && wide.contains("? keys"),
            "{wide}"
        );
    }

    fn row_160(view: &StatusBarView<'_>) -> String {
        row(view, 160)
    }

    fn with_footer(
        mode: &'static str,
        keymap: &crate::ui::keymap::KeyMap,
        context: crate::ui::action_table::FooterContext,
    ) -> StatusBarView<'static> {
        let mut view = StatusBarView::new(mode, ConnectionState::Online);
        view.channel = Some("#general");
        view.hint = Some(super::footer(&Theme::default(), keymap, context, |_| true));
        view
    }

    /// v0.17 C10: the hint names the keys that matter where the focus is,
    /// read from the registry and the live key map, ending in the palette's
    /// key; a narrow row takes fewer, down to `Ctrl-K actions · ? keys`.
    #[test]
    fn the_hint_follows_the_focus_at_60_80_and_160_columns() {
        use crate::ui::action_table::FooterContext as F;
        let keymap = crate::ui::keymap::KeyMap::builtin();
        for (context, mode, wide) in [
            (
                F::Message,
                "NORMAL",
                "Enter thread · r react · i reply · Ctrl-K actions",
            ),
            (
                F::Timeline,
                "NORMAL",
                "i write · Space Space find · Ctrl-K actions",
            ),
            (
                F::Thread,
                "NORMAL",
                "i reply · r react · Space 4 hide · Ctrl-K actions",
            ),
            (
                F::Channels,
                "NORMAL",
                "Enter open · Space Space find · i write · Ctrl-K actions",
            ),
            (
                F::Inbox,
                "INBOX",
                "o open · i reply · m read · Ctrl-K actions",
            ),
            (
                F::Composer,
                "INSERT",
                "Enter send · Ctrl-J newline · Esc close",
            ),
            (F::Huddle, "HUDDLE", "s start · j join · m mute"),
            (F::Overlay, "PALETTE", "Esc close"),
        ] {
            let view = with_footer(mode, &keymap, context);
            let wide_row = row(&view, 160);
            assert!(wide_row.contains(wide), "{context:?} 160: {wide_row}");
            let middle = row(&view, 80);
            assert!(middle.contains("● online"), "{context:?} 80: {middle}");
            let narrow = row(&view, 60);
            assert!(narrow.contains("● online"), "{context:?} 60: {narrow}");
            if wide.contains("Ctrl-K actions") {
                // Wherever the palette opens, its key is the last word.
                for shown in [&wide_row, &middle, &narrow] {
                    if shown.contains("actions") {
                        let end = shown.trim_end();
                        assert!(
                            end.ends_with("Ctrl-K actions")
                                || end.ends_with("Ctrl-K actions · ? keys"),
                            "{context:?}: {shown}"
                        );
                    }
                }
            }
        }
        // At 80 columns the message hint is whole; at 60 it takes fewer.
        let view = with_footer("NORMAL", &keymap, F::Message);
        assert!(row(&view, 80).contains("Enter thread · r react · i reply · Ctrl-K actions"));
        let narrow = row(&view, 60);
        assert!(narrow.contains("Enter thread"), "{narrow}");
        assert!(!narrow.contains("i reply"), "{narrow}");
        // Narrower still: the two keys that lead to everything else.
        let mut tight = with_footer("NORMAL", &keymap, F::Message);
        tight.activity = Some("◆ Scout is typing ⠹");
        let tight_row = row(&tight, 80);
        assert!(tight_row.contains("Ctrl-K actions"), "{tight_row}");
    }

    /// A key a person binds in `keymap.toml` is the one the hint shows.
    #[test]
    fn the_hint_shows_a_users_binding() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        std::fs::write(
            &path,
            "[[binding]]\nkeys = ['r']\ndisabled = true\n\
             [[binding]]\nkeys = ['x']\naction = 'react'\n\
             [[binding]]\nkeys = ['ctrl-k']\ndisabled = true\n",
        )
        .unwrap();
        let keymap = crate::ui::keymap::KeyMap::load_from(&path).unwrap();
        let view = with_footer(
            "NORMAL",
            &keymap,
            crate::ui::action_table::FooterContext::Message,
        );
        let shown = row(&view, 160);
        assert!(
            shown.contains("Enter thread · x react · i reply · Space k actions"),
            "{shown}"
        );
    }

    /// Only what can run is offered: a row the caller refuses is left out.
    #[test]
    fn the_hint_offers_only_what_can_run() {
        let keymap = crate::ui::keymap::KeyMap::builtin();
        let keys = super::footer_keys(
            &keymap,
            crate::ui::action_table::FooterContext::Message,
            |spec| spec.id != "msg.react",
        );
        assert_eq!(
            keys,
            [
                ("Enter".to_owned(), "thread"),
                ("i".to_owned(), "reply"),
                ("e".to_owned(), "edit"),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_notice_expires_after_five_seconds_and_asks_for_one_frame() {
        let mut timer = NoticeTimer::default();
        let mut notice = Some("Saved.".to_owned());
        let start = Instant::now();
        timer.observe(notice.as_deref(), start);
        assert_eq!(timer.deadline(), Some(start + NOTICE_LIFETIME));
        // The loop sleeps to the deadline; count the frames it asks for in
        // ten seconds of rest.
        let mut gate = RedrawGate::default();
        let mut frames = 0;
        let end = start + Duration::from_secs(10);
        loop {
            let next = timer.deadline().unwrap_or(end).min(end);
            tokio::time::sleep_until(next).await;
            if Instant::now() >= end {
                break;
            }
            if timer.expire(notice.as_deref(), Instant::now()) {
                notice = None;
                gate.request();
            }
            timer.observe(notice.as_deref(), Instant::now());
            if gate.take() {
                frames += 1;
            }
        }
        assert_eq!(frames, 1);
        assert_eq!(notice, None);
        assert!(Instant::now() - start >= Duration::from_secs(10));
        // A notice replaced by another starts again.
        let at = Instant::now();
        timer.observe(Some("one"), at);
        tokio::time::advance(Duration::from_secs(3)).await;
        timer.observe(Some("two"), Instant::now());
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(!timer.expire(Some("two"), Instant::now()));
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(timer.expire(Some("two"), Instant::now()));
    }
}
