//! Toasts (plan v0.17.0 L7): a short confirmation near where the eye is,
//! at the bottom right of the panes, never over the composer, never the
//! focus. One at a time, with a short queue.
//!
//! A toast enters in [`scale::BASE`], stays [`scale::HOLD`] without a frame,
//! and leaves in [`scale::SLOW`]. With motion off it is simply there and then
//! gone: one frame when it comes, one when it goes, none between.

use std::collections::VecDeque;

use ratatui::{buffer::Buffer, layout::Rect, text::Line, widgets::Widget as _};
use tokio::time::Instant;
use unicode_width::UnicodeWidthStr as _;

use crate::{
    config::Motion,
    render::sanitize,
    ui::{
        motion::{Anim, Ease, Role, scale},
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

/// How many toasts may wait behind the one shown; the oldest waiting gives
/// way to a newer one.
pub const QUEUE: usize = 3;

/// The widest a toast is drawn, frame included.
pub const MAX_WIDTH: u16 = 48;

#[derive(Clone, Debug)]
struct Shown {
    text: String,
    enter: Anim,
    /// When it starts to leave.
    leaves_at: Instant,
    leave: Option<Anim>,
    motion: Motion,
}

/// Where a toast is in its life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Sliding in: 0 out of sight, 1 in place.
    Entering(f32),
    Holding,
    /// Sliding out: 0 in place, 1 out of sight.
    Leaving(f32),
}

#[derive(Clone, Debug, Default)]
pub struct Toasts {
    shown: Option<Shown>,
    waiting: VecDeque<String>,
}

impl Toasts {
    /// Says `text` in a toast: now when none shows, after the one shown
    /// otherwise. The same text shown or waiting is not said twice.
    pub fn push(&mut self, text: &str, now: Instant, motion: Motion) {
        let text = sanitize::single_line(text).trim().to_owned();
        if text.is_empty()
            || self.shown.as_ref().is_some_and(|shown| shown.text == text)
            || self.waiting.contains(&text)
        {
            return;
        }
        if self.shown.is_none() {
            self.shown = Some(Self::start(text, now, motion));
        } else {
            if self.waiting.len() >= QUEUE {
                self.waiting.pop_front();
            }
            self.waiting.push_back(text);
        }
    }

    fn start(text: String, now: Instant, motion: Motion) -> Shown {
        let enter = Anim::new(now, scale::BASE, Ease::OutCubic, motion, Role::Informs);
        Shown {
            text,
            leaves_at: enter.finish() + scale::HOLD,
            enter,
            leave: None,
            motion,
        }
    }

    /// Moves the toasts to `now`: one that is gone makes way for the next.
    /// Answers whether what is drawn changed.
    pub fn advance(&mut self, now: Instant) -> bool {
        let mut changed = false;
        loop {
            let Some(shown) = &mut self.shown else {
                return changed;
            };
            if shown.leave.is_none() && now >= shown.leaves_at {
                shown.leave = Some(Anim::new(
                    shown.leaves_at,
                    scale::SLOW,
                    Ease::InCubic,
                    shown.motion,
                    Role::Informs,
                ));
                changed = true;
            }
            match shown.leave {
                Some(leave) if !leave.running(now) => {
                    let motion = shown.motion;
                    self.shown = self
                        .waiting
                        .pop_front()
                        .map(|text| Self::start(text, now, motion));
                    changed = true;
                }
                _ => return changed,
            }
        }
    }

    /// When the toasts need their next frame: every frame while one moves,
    /// the instant it starts to leave while it holds, none without one.
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let shown = self.shown.as_ref()?;
        if let Some(leave) = shown.leave {
            // The frame after its end removes it (or brings the next).
            return Some(leave.wake(now).unwrap_or(now));
        }
        shown.enter.wake(now).or(Some(shown.leaves_at))
    }

    /// The text shown and where it is, at `now`.
    pub fn current(&self, now: Instant) -> Option<(&str, Phase)> {
        let shown = self.shown.as_ref()?;
        let phase = match shown.leave {
            Some(leave) => Phase::Leaving(leave.progress(now)),
            None if shown.enter.running(now) => Phase::Entering(shown.enter.progress(now)),
            None => Phase::Holding,
        };
        Some((&shown.text, phase))
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_none()
    }

    /// Draws the toast at the bottom right of `room`: the panes' area above
    /// the composer and the status line. It slides in from the right edge
    /// and back out to it.
    pub fn render(&self, buf: &mut Buffer, room: Rect, theme: &Theme, now: Instant) {
        let Some((text, phase)) = self.current(now) else {
            return;
        };
        let room = room.intersection(buf.area);
        if room.width < 12 || room.height < 3 {
            return;
        }
        let width = u16::try_from(text.width())
            .unwrap_or(u16::MAX)
            .saturating_add(4)
            .min(MAX_WIDTH)
            .min(room.width);
        let place = Rect::new(room.right() - width, room.bottom() - 3, width, 3);
        let hidden = match phase {
            Phase::Entering(progress) => 1.0 - progress,
            Phase::Holding => 0.0,
            Phase::Leaving(progress) => progress,
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let shift = (f32::from(width) * hidden.clamp(0.0, 1.0)).round() as u16;
        if shift >= width {
            return;
        }
        // Drawn whole aside, then the part still in sight is copied in.
        let mut card = Buffer::empty(Rect::new(0, 0, width, 3));
        let block = crate::ui::frame::block(theme, BorderSurface::Pane, "", false)
            .style(theme.style(HighlightGroup::Normal));
        let inner = block.inner(card.area);
        block.render(card.area, &mut card);
        let line = Line::styled(
            crate::ui::typing::truncate_cells(text, usize::from(inner.width.saturating_sub(2))),
            theme.style(HighlightGroup::Normal),
        );
        card.set_line(inner.x + 1, inner.y, &line, inner.width.saturating_sub(1));
        for y in 0..3 {
            for x in 0..width - shift {
                let cell = card[(x, y)].clone();
                buf[(place.x + shift + x, place.y + y)] = cell;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{buffer::Buffer, layout::Rect};
    use tokio::time::Instant;

    use super::{Phase, Toasts};
    use crate::{config::Motion, ui::theme::Theme};

    /// Runs the loop's way with the toasts alone: sleep until the wake time,
    /// draw a frame, ask again. Answers the frames' instants in ms from
    /// `start`.
    fn frames(toasts: &mut Toasts, start: Instant, until: Duration) -> Vec<u64> {
        let mut now = start;
        let mut drawn = Vec::new();
        while let Some(next) = toasts.wake(now) {
            if next - start > until {
                break;
            }
            now = next;
            toasts.advance(now);
            drawn.push(u64::try_from((now - start).as_millis()).unwrap());
        }
        drawn
    }

    #[tokio::test(start_paused = true)]
    async fn a_toast_asks_for_frames_for_150_ms_none_for_3_s_then_for_250_ms() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        toasts.push("Saved.", start, Motion::Full);
        let drawn = frames(&mut toasts, start, Duration::from_secs(10));
        let entering = drawn.iter().filter(|&&t| t <= 150).count();
        let holding = drawn.iter().filter(|&&t| t > 150 && t < 3_150).count();
        let leaving = drawn.iter().filter(|&&t| t >= 3_150).count();
        assert_eq!(&drawn[..5], &[33, 66, 99, 132, 150], "{drawn:?}");
        assert_eq!(entering, 5, "{drawn:?}");
        assert_eq!(holding, 0, "nothing is drawn while it holds: {drawn:?}");
        // Leaving: 3150 (it starts), then every 33 ms to 3400.
        assert_eq!(drawn[5], 3_150, "{drawn:?}");
        assert_eq!(drawn.last(), Some(&3_400), "{drawn:?}");
        assert_eq!(leaving, 9, "{drawn:?}");
        assert!(toasts.is_empty());
        assert_eq!(toasts.wake(start + Duration::from_secs(4)), None);
    }

    #[tokio::test(start_paused = true)]
    async fn with_motion_off_a_toast_is_there_and_gone_with_no_frame_between() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        toasts.push("Saved.", start, Motion::Off);
        assert_eq!(toasts.current(start).map(|(_, phase)| phase), Some(Phase::Holding));
        let drawn = frames(&mut toasts, start, Duration::from_secs(10));
        // The frame that takes it away, at 3 s, and nothing else.
        assert_eq!(drawn, vec![3_000]);
        assert!(toasts.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn one_toast_at_a_time_with_a_short_queue() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        for text in ["one", "two", "three", "four", "five", "four"] {
            toasts.push(text, start, Motion::Off);
        }
        assert_eq!(toasts.current(start).map(|(text, _)| text), Some("one"));
        assert_eq!(toasts.waiting.len(), super::QUEUE);
        assert_eq!(toasts.waiting.front().map(String::as_str), Some("three"));
        let later = start + Duration::from_secs(3);
        assert!(toasts.advance(later));
        assert_eq!(toasts.current(later).map(|(text, _)| text), Some("three"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_toast_slides_in_from_the_right_and_sits_at_the_bottom_right() {
        let start = Instant::now();
        let theme = Theme::default();
        let mut toasts = Toasts::default();
        toasts.push("Saved.", start, Motion::Full);
        let room = Rect::new(0, 0, 40, 10);
        let row = |buf: &Buffer, y: u16| -> String {
            (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>()
        };
        let mut settled = Buffer::empty(room);
        toasts.render(&mut settled, room, &theme, start + Duration::from_millis(200));
        assert_eq!(row(&settled, 8).trim_end(), format!("{}│ Saved. │", " ".repeat(30)));
        let mut entering = Buffer::empty(room);
        toasts.render(&mut entering, room, &theme, start + Duration::from_millis(20));
        assert!(row(&entering, 8).trim().len() < row(&settled, 8).trim().len());
    }
}
