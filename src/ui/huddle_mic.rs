//! The join confirmation with the microphone check (plan v0.17.0 B7): your
//! own lane, live before anything is sent, and a line that says whether the
//! microphone is heard, or names it when nothing comes.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    widgets::{Block, Clear, Widget as _},
};

use crate::{
    config::Motion,
    huddle::mic_check::{MicCheck, MicState},
    ui::{
        huddle_lanes::{Look, Paint, draw_badge, draw_lane, put},
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

/// The confirmation's last line, as before the check existed.
pub const KEYS: &str = "you start muted · y join · n/Esc cancel";

/// Rows the confirmation takes.
pub const HEIGHT: u16 = 8;

/// The confirmation, over `area` (a popup [`HEIGHT`] rows high).
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    check: &MicCheck,
    look: Look,
    motion: Motion,
    theme: &Theme,
) {
    Clear.render(area, buf);
    let block = Block::bordered()
        .border_type(theme.border_type(BorderSurface::Modal))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(" join the huddle? ")
        .style(theme.style(HighlightGroup::Normal));
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.width < 24 || inner.height < 5 {
        return;
    }
    let paint = Paint::new(theme, look);
    put(
        buf,
        inner,
        1,
        0,
        "your microphone, heard only here: nothing is sent until you join",
        paint.fg(paint.grey),
    );
    let state = check.state();
    let track = check.meter().mic();
    let rgb = match state {
        MicState::Works | MicState::Listening => paint.you,
        MicState::NoSignal | MicState::Failed(_) => paint.amber,
    };
    draw_badge(buf, inner, 1, 1, "You", rgb, track.speaking(), &paint);
    let name_style = if track.speaking() {
        paint.fg(rgb).add_modifier(Modifier::BOLD)
    } else {
        paint.text
    };
    put(buf, inner, 5, 1, "You", name_style);
    let (word, word_style) = match state {
        MicState::Works if track.speaking() => ("● heard", paint.fg(paint.you)),
        MicState::Works => ("works", paint.fg(paint.you)),
        MicState::Listening => ("listening", paint.fg(paint.grey)),
        MicState::NoSignal | MicState::Failed(_) => (
            "no sound",
            paint
                .fg(paint.amber)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
    };
    put(buf, inner, 5, 2, word, word_style);
    let lane_x = 5 + 10;
    draw_lane(
        buf,
        inner,
        lane_x,
        1,
        inner.width.saturating_sub(lane_x + 1),
        track,
        rgb,
        &paint,
        motion,
        6.0,
    );
    let message_style = match state {
        MicState::Works => paint.fg(paint.you),
        MicState::Listening => paint.fg(paint.grey),
        MicState::NoSignal | MicState::Failed(_) => {
            paint.fg(paint.amber).add_modifier(Modifier::BOLD)
        }
    };
    put(buf, inner, 1, 3, &check.message(), message_style);
    put(buf, inner, 1, inner.height - 1, KEYS, paint.text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huddle::synth;
    use crate::ui::huddle_lanes::{Depth, Glyphs};

    fn screen(check: &MicCheck, look: Look) -> (Buffer, String) {
        let theme = Theme::builtin("bzz").unwrap();
        let mut buf = Buffer::empty(Rect::new(0, 0, 72, HEIGHT));
        let area = buf.area;
        render(&mut buf, area, check, look, Motion::Full, &theme);
        let text = (0..HEIGHT)
            .map(|y| {
                (0..72)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (buf, text)
    }

    const NONE: Look = Look {
        depth: Depth::None,
        glyphs: Glyphs::Blocks,
    };

    #[test]
    fn your_lane_moves_before_you_join_and_silence_names_the_device() {
        let mut check = MicCheck::new(None);
        let levels = check.levels();
        let mut t = 0.0;
        while t < 0.6 {
            levels.publish_mic(&synth::tone(220.0, -22.0, 0, 960));
            check.update(t);
            t += 1.0 / 30.0;
        }
        let (_, text) = screen(&check, Look::default());
        assert!(text.contains("● heard"), "{text}");
        assert!(text.contains("your microphone works"), "{text}");
        assert!(text.contains("y join"), "{text}");
        let lane = text.lines().nth(2).unwrap();
        assert!(
            lane.chars().rev().take(3).any(|c| "▂▃▄▅▆▇█".contains(c)),
            "{text}"
        );
        let mut check = MicCheck::new(Some("USB Mic".into()));
        let mut t = 0.0;
        while t < 3.2 {
            check.update(t);
            t += 1.0 / 30.0;
        }
        let (buf, text) = screen(&check, NONE);
        assert!(text.contains("no sound from “USB Mic”"), "{text}");
        assert!(text.contains(":huddle input"), "{text}");
        // Inside the frame (the theme's own), no colour.
        for y in 1..HEIGHT - 1 {
            for x in 1..71 {
                assert_eq!(buf[(x, y)].fg, ratatui::style::Color::Reset, "{x},{y}");
            }
        }
        println!("{text}");
    }
}
