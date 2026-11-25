//! Motion: one timing scale for all of bzz, one switch, and bzz's clock
//! (plan v0.17.0 D9, L7, L8).
//!
//! An animation is an [`Anim`]: a value that runs from 0 to 1 between two
//! instants. Nothing here holds a timer. The run loop asks each running
//! thing when it next needs a frame ([`Anim::wake`]), sleeps until the
//! earliest answer, and draws nothing in between: while no animation runs,
//! none asks for a frame (plan rule 20).
//!
//! Time is `tokio::time::Instant`, so tests move it by hand with paused
//! Tokio time. With `motion = off` (or `BZZ_NO_MOTION`) every animation is
//! at its end from the start and asks for no frame; `calm` keeps what
//! informs (a message that arrives, a toast, a count that rises) and drops
//! what decorates (the focus sweep, an overlay that unfolds, the shimmer).

use std::time::Duration;

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Modifier, Style},
};
use tokio::time::Instant;

use crate::config::Motion;

/// The only lengths a movement may have.
pub mod scale {
    use std::time::Duration;

    /// Feedback to a key, something that closes: an overlay folds away.
    pub const FAST: Duration = Duration::from_millis(80);
    /// Something enters: an overlay opens, a toast slides in, focus moves.
    pub const BASE: Duration = Duration::from_millis(150);
    /// Something leaves or settles: a toast slides out, a new message's
    /// tint fades, an unread count flashes.
    pub const SLOW: Duration = Duration::from_millis(250);
    /// How long a toast stays. Not motion: nothing is drawn while it holds.
    pub const HOLD: Duration = Duration::from_millis(3_000);
    /// One sweep of the loading shimmer.
    pub const LOOP: Duration = Duration::from_millis(1_200);
    /// The longest the shimmer runs; then the plain loading line.
    pub const LOADING: Duration = Duration::from_secs(10);
    /// The time between two frames while something moves.
    pub const FRAME: Duration = Duration::from_millis(33);
}

/// What an animation is for: `calm` keeps what informs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Says that something happened: a toast, a message that arrives.
    Informs,
    /// Only pleasant: a sweep, an unfolding, a shimmer.
    Decorates,
}

/// Whether an animation of `role` plays under `motion`.
pub const fn plays(motion: Motion, role: Role) -> bool {
    match (motion, role) {
        (Motion::Off, _) | (Motion::Calm, Role::Decorates) => false,
        (Motion::Full | Motion::Calm, _) => true,
    }
}

/// How a value runs from 0 to 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ease {
    Linear,
    /// Fast, then settling: for what enters.
    OutCubic,
    /// Slow, then gone: for what leaves.
    InCubic,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::OutCubic => 1.0 - (1.0 - t).powi(3),
            Self::InCubic => t * t * t,
        }
    }
}

/// A value from 0 to 1 between two instants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Anim {
    start: Instant,
    length: Duration,
    ease: Ease,
    /// A step holds one look for its whole length (no blend is possible):
    /// it asks for a frame at its end only.
    step: bool,
}

impl Anim {
    /// An animation of `length` from `now`, or of none when `role` does not
    /// play under `motion`: then it is at its end at once.
    pub fn new(now: Instant, length: Duration, ease: Ease, motion: Motion, role: Role) -> Self {
        Self {
            start: now,
            length: if plays(motion, role) {
                length
            } else {
                Duration::ZERO
            },
            ease,
            step: false,
        }
    }

    /// A look held for `length` with no frame in between: the start and the
    /// end are drawn, nothing else.
    pub fn step(now: Instant, length: Duration, motion: Motion, role: Role) -> Self {
        Self {
            step: true,
            ..Self::new(now, length, Ease::Linear, motion, role)
        }
    }

    /// When the animation is over.
    pub fn finish(&self) -> Instant {
        self.start + self.length
    }

    /// Where the value is at `now`: 0 at the start, 1 at the end and after.
    pub fn progress(&self, now: Instant) -> f32 {
        if self.length.is_zero() || now >= self.finish() {
            return 1.0;
        }
        if self.step {
            return 0.0;
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32() / self.length.as_secs_f32();
        self.ease.apply(t)
    }

    /// Whether the animation still shows something other than its end.
    pub fn running(&self, now: Instant) -> bool {
        now < self.finish()
    }

    /// When the animation needs its next frame: one frame away while it
    /// runs, its end for the last frame, never once it is over.
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let end = self.finish();
        if now >= end {
            None
        } else if self.step {
            Some(end)
        } else {
            Some((now + scale::FRAME).min(end))
        }
    }
}

/// The earliest of several instants.
pub fn earliest(times: impl IntoIterator<Item = Option<Instant>>) -> Option<Instant> {
    times.into_iter().flatten().min()
}

/// A colour `t` of the way from `from` to `to`, when both are true colours.
/// Anything else cannot be blended (bzz does not know the terminal's own
/// background): `None`, and the caller steps instead.
pub fn blend(from: Color, to: Color, t: f32) -> Option<Color> {
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (from, to) else {
        return None;
    };
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| -> u8 {
        let value = f32::from(a) + (f32::from(b) - f32::from(a)) * t;
        // Clamped to 0..=255 above; the cast cannot lose more than the
        // fraction.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = value.round().clamp(0.0, 255.0) as u8;
        value
    };
    Some(Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2)))
}

/// Whether a symbol is part of a frame's line (not a title's letter).
fn is_frame_line(symbol: &str) -> bool {
    matches!(
        symbol,
        "─" | "│"
            | "┌"
            | "┐"
            | "└"
            | "┘"
            | "╭"
            | "╮"
            | "╰"
            | "╯"
            | "━"
            | "┃"
            | "┏"
            | "┓"
            | "┗"
            | "┛"
            | "═"
            | "║"
            | "╔"
            | "╗"
            | "╚"
            | "╝"
    )
}

/// The cells of a frame's edge in the order the focus colour runs round
/// it: from the top-left corner, right along the top, down the right side,
/// back along the bottom and up the left side.
fn perimeter(area: Rect) -> Vec<(u16, u16)> {
    if area.width < 2 || area.height < 2 {
        return Vec::new();
    }
    let (left, top) = (area.left(), area.top());
    let (right, bottom) = (area.right() - 1, area.bottom() - 1);
    let mut cells = Vec::with_capacity(usize::from(area.width) * 2 + usize::from(area.height) * 2);
    cells.extend((left..=right).map(|x| (x, top)));
    cells.extend((top + 1..=bottom).map(|y| (right, y)));
    cells.extend((left..right).rev().map(|x| (x, bottom)));
    cells.extend((top + 1..bottom).rev().map(|y| (left, y)));
    cells
}

/// The focus colour running round a frame (L8): the part of the edge it has
/// not reached yet keeps the quiet frame's style. Letters on the edge (a
/// title, a hint) are left as drawn.
pub fn sweep(buf: &mut Buffer, area: Rect, progress: f32, quiet: Style) {
    let area = area.intersection(buf.area);
    let cells = perimeter(area);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let reached = ((cells.len() as f32) * progress.clamp(0.0, 1.0)).ceil() as usize;
    for &(x, y) in cells.iter().skip(reached) {
        if let Some(cell) = buf.cell_mut((x, y))
            && is_frame_line(cell.symbol())
        {
            cell.set_style(quiet);
        }
    }
}

/// The part of `rect` shown when it unfolds from its middle: `progress` 0
/// shows a line in the middle, 1 the whole of it.
pub fn unfolded(rect: Rect, progress: f32) -> Rect {
    let progress = progress.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = |length: u16| -> u16 { ((f32::from(length) * progress).round() as u16).max(1) };
    let width = scaled(rect.width).min(rect.width);
    let height = scaled(rect.height).min(rect.height);
    Rect::new(
        rect.x + (rect.width - width) / 2,
        rect.y + (rect.height - height) / 2,
        width,
        height,
    )
}

/// The smallest rectangle of `area` whose cells differ between `before` and
/// `after`: where an overlay drew itself.
pub fn changed_rect(before: &Buffer, after: &Buffer, area: Rect) -> Option<Rect> {
    let area = area.intersection(before.area).intersection(after.area);
    let (mut left, mut top, mut right, mut bottom) = (u16::MAX, u16::MAX, 0_u16, 0_u16);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if before[(x, y)] != after[(x, y)] {
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    (left <= right && top <= bottom)
        .then(|| Rect::new(left, top, right - left + 1, bottom - top + 1))
}

/// The cells of `rect`, row by row.
pub fn copy_cells(buf: &Buffer, rect: Rect) -> Vec<Cell> {
    let rect = rect.intersection(buf.area);
    let mut cells = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            cells.push(buf[(x, y)].clone());
        }
    }
    cells
}

/// Puts the cells of `rect` (as [`copy_cells`] took them) back into `buf`,
/// only those inside `shown`.
pub fn paste_cells(buf: &mut Buffer, rect: Rect, cells: &[Cell], shown: Rect) {
    let rect = rect.intersection(buf.area);
    let shown = shown.intersection(rect);
    for y in shown.top()..shown.bottom() {
        for x in shown.left()..shown.right() {
            let index = usize::from(y - rect.y) * usize::from(rect.width) + usize::from(x - rect.x);
            if let Some(cell) = cells.get(index) {
                buf[(x, y)] = cell.clone();
            }
        }
    }
}

/// Copies back from `from` the cells of `rect` that lie outside `shown`.
pub fn restore_outside(buf: &mut Buffer, from: &Buffer, rect: Rect, shown: Rect) {
    let rect = rect.intersection(buf.area).intersection(from.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let inside =
                x >= shown.left() && x < shown.right() && y >= shown.top() && y < shown.bottom();
            if !inside {
                buf[(x, y)] = from[(x, y)].clone();
            }
        }
    }
}

/// Lays a tint on the cells of `area`: their background takes `tint`.
pub fn tint(buf: &mut Buffer, area: Rect, tint: Color) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y))
                && cell.diff_option != ratatui::buffer::CellDiffOption::Skip
            {
                cell.bg = tint;
            }
        }
    }
}

/// Reverses the count at the right end of a row (the unread count's flash):
/// the last run of digits and `+` before the row's trailing blanks.
pub fn flash_count(buf: &mut Buffer, row: Rect) {
    let row = row.intersection(buf.area);
    if row.is_empty() {
        return;
    }
    let y = row.y;
    let mut x = row.right();
    while x > row.left() && buf[(x - 1, y)].symbol() == " " {
        x -= 1;
    }
    while x > row.left() {
        let symbol = buf[(x - 1, y)].symbol();
        if !(symbol.chars().all(|c| c.is_ascii_digit() || c == '+') && !symbol.is_empty()) {
            break;
        }
        buf[(x - 1, y)].modifier.insert(Modifier::REVERSED);
        x -= 1;
    }
}

/// Placeholder rows while a channel's history loads (L8): four messages'
/// shapes, and a brighter band that crosses them once a [`scale::LOOP`].
/// Without colour the band is `▒` on `░`.
pub fn shimmer(buf: &mut Buffer, area: Rect, phase: f32, quiet: Style, bright: Style) {
    let area = area.intersection(buf.area);
    if area.width < 8 || area.height < 2 {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
        }
    }
    // Each placeholder: an author's width, then two lines of body.
    let shapes: [(u16, [u16; 2]); 4] = [(6, [70, 45]), (8, [55, 0]), (5, [80, 62]), (7, [40, 0])];
    let width = area.width.saturating_sub(2);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let band = (f32::from(width + 16) * phase.clamp(0.0, 1.0)) as u16;
    let mut y = area.y;
    for (author, lines) in shapes {
        let rows = [author, lines[0] * width / 100, lines[1] * width / 100];
        for (index, length) in rows.into_iter().enumerate() {
            if y >= area.bottom() {
                return;
            }
            if length > 0 {
                for offset in 0..length.min(width) {
                    let x = area.x + 1 + offset;
                    let lit = offset + 8 >= band && offset < band;
                    let (symbol, style) = if lit { ("▒", bright) } else { ("░", quiet) };
                    let style = if index == 0 {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };
                    buf[(x, y)].set_symbol(symbol).set_style(style);
                }
                y += 1;
            }
        }
        y += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    /// Frames an animation asks for, from its start until it rests: the
    /// loop draws when a wake time comes, then asks again.
    fn frames(anim: &Anim, start: Instant) -> Vec<u64> {
        let mut now = start;
        let mut drawn = Vec::new();
        while let Some(next) = anim.wake(now) {
            now = next;
            drawn.push(u64::try_from((now - start).as_millis()).unwrap());
        }
        drawn
    }

    #[tokio::test(start_paused = true)]
    async fn an_animation_asks_for_frames_only_while_it_runs() {
        let start = Instant::now();
        let anim = Anim::new(
            start,
            scale::BASE,
            Ease::OutCubic,
            Motion::Full,
            Role::Informs,
        );
        assert_eq!(anim.wake(start), Some(at(start, 33)));
        assert_eq!(anim.wake(at(start, 140)), Some(at(start, 150)));
        assert_eq!(anim.progress(at(start, 150)), 1.0);
        assert_eq!(anim.wake(at(start, 150)), None);
        assert_eq!(anim.wake(at(start, 10_000)), None);
        assert!(anim.progress(at(start, 75)) > 0.5);
    }

    #[tokio::test(start_paused = true)]
    async fn each_length_of_the_scale_gives_its_frame_count() {
        let start = Instant::now();
        let count = |length| {
            frames(
                &Anim::new(start, length, Ease::Linear, Motion::Full, Role::Decorates),
                start,
            )
        };
        // 33 ms apart, and the end: 80 ms is 3 frames, 150 ms 5, 250 ms 8.
        assert_eq!(count(scale::FAST), vec![33, 66, 80]);
        assert_eq!(count(scale::BASE), vec![33, 66, 99, 132, 150]);
        assert_eq!(count(scale::SLOW).len(), 8);
        assert_eq!(count(scale::SLOW).last(), Some(&250));
    }

    #[tokio::test(start_paused = true)]
    async fn with_motion_off_the_end_shows_at_once_and_no_frame_is_asked() {
        let start = Instant::now();
        for role in [Role::Informs, Role::Decorates] {
            let anim = Anim::new(start, scale::SLOW, Ease::Linear, Motion::Off, role);
            assert_eq!(anim.progress(start), 1.0);
            assert_eq!(anim.wake(start), None);
            assert!(!anim.running(start));
            let step = Anim::step(start, scale::SLOW, Motion::Off, role);
            assert_eq!(step.wake(start), None);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn calm_keeps_what_informs_and_drops_what_decorates() {
        let start = Instant::now();
        let informs = Anim::new(
            start,
            scale::SLOW,
            Ease::Linear,
            Motion::Calm,
            Role::Informs,
        );
        let decorates = Anim::new(
            start,
            scale::SLOW,
            Ease::Linear,
            Motion::Calm,
            Role::Decorates,
        );
        assert!(informs.running(start));
        assert!(!decorates.running(start));
        assert_eq!(decorates.wake(start), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_asks_for_its_end_only() {
        let start = Instant::now();
        let step = Anim::step(start, scale::SLOW, Motion::Full, Role::Informs);
        assert_eq!(frames(&step, start), vec![250]);
        assert_eq!(step.progress(at(start, 100)), 0.0);
        assert_eq!(step.progress(at(start, 250)), 1.0);
    }

    #[test]
    fn blending_needs_two_true_colours() {
        assert_eq!(
            blend(Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50), 0.5),
            Some(Color::Rgb(100, 50, 25))
        );
        assert_eq!(blend(Color::Rgb(0, 0, 0), Color::Reset, 0.5), None);
        assert_eq!(blend(Color::Cyan, Color::Rgb(1, 1, 1), 0.5), None);
    }

    #[test]
    fn the_sweep_leaves_the_part_not_reached_quiet() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = Buffer::empty(area);
        let focus = Style::default().fg(Color::Cyan);
        let quiet = Style::default().fg(Color::DarkGray);
        ratatui::widgets::Widget::render(
            ratatui::widgets::Block::bordered()
                .border_style(focus)
                .title(" ab "),
            area,
            &mut buf,
        );
        sweep(&mut buf, area, 0.25, quiet);
        // The top row is reached; the bottom-left corner is not.
        assert_eq!(buf[(5, 0)].fg, Color::Cyan);
        assert_eq!(buf[(0, 3)].fg, Color::DarkGray);
        // The title's letters are not touched.
        assert_eq!(buf[(2, 0)].symbol(), "a");
        sweep(&mut buf, area, 1.0, quiet);
        assert_eq!(buf[(0, 3)].fg, Color::DarkGray, "a sweep only quiets");
    }

    #[test]
    fn an_overlay_unfolds_from_its_middle() {
        let rect = Rect::new(10, 5, 40, 10);
        assert_eq!(unfolded(rect, 1.0), rect);
        let half = unfolded(rect, 0.5);
        assert_eq!((half.width, half.height), (20, 5));
        assert_eq!((half.x, half.y), (20, 7));
        assert_eq!(unfolded(rect, 0.0).width, 1);
    }

    #[test]
    fn the_changed_rect_is_where_cells_differ() {
        let area = Rect::new(0, 0, 20, 6);
        let before = Buffer::empty(area);
        let mut after = before.clone();
        after[(4, 2)].set_symbol("x");
        after[(9, 3)].set_symbol("y");
        assert_eq!(
            changed_rect(&before, &after, area),
            Some(Rect::new(4, 2, 6, 2))
        );
        assert_eq!(changed_rect(&before, &before, area), None);
        let cells = copy_cells(&after, Rect::new(4, 2, 6, 2));
        let mut empty = Buffer::empty(area);
        paste_cells(
            &mut empty,
            Rect::new(4, 2, 6, 2),
            &cells,
            Rect::new(4, 2, 6, 2),
        );
        assert_eq!(empty[(9, 3)].symbol(), "y");
    }

    #[test]
    fn a_rising_count_flashes_reversed() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "# design 12", Style::default());
        flash_count(&mut buf, area);
        assert!(buf[(9, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(10, 0)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(7, 0)].modifier.contains(Modifier::REVERSED));
    }
}
