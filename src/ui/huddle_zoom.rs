//! The huddle's zoomed view (plan v0.17.0 B5, B6): the visualizer over the
//! whole window, in one of three styles, and the picker that previews each.
//!
//! - **Lanes**: one tall lane per person, your microphone last (the panel's
//!   lanes, taller).
//! - **Bars**: a spectrum of the call's mix in three tones of the theme's
//!   colour by height, eighth-block resolution, and peak marks that are
//!   thrown up and fall.
//! - **Orb**: a sphere of liquid in the colour of whoever speaks, two
//!   colours when two speak, calm and grey in silence; half-block cells,
//!   its size capped because it is the costliest style to write.
//!
//! Every style names who speaks (the header and, for bars and the orb, a
//! line of names), keeps its meaning without colour (rule 21: `●`, bold and
//! words), draws Block Elements only when asked, and holds still with
//! motion off. A spectrum is of the mix: the bars are not coloured by
//! person, because the mix cannot say who made which sound.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    config::{HuddleStyle, Motion},
    huddle::{meter::Track, spectrum::Spectrum},
    ui::{
        huddle_lanes::{
            Depth, Glyphs, Lanes, Paint, Rgb, UP, draw_badge, lerp, named_rgb, put, speaking_names,
            state_of, talking_muted, who_speaks,
        },
        theme::{HighlightGroup, Theme},
    },
};

/// The most rows the orb takes, whatever the window: it is the costliest
/// style to write (every cell two colours), so it stays this small.
pub const ORB_MAX_ROWS: u16 = 18;

/// What the zoomed view draws from.
pub struct Zoom<'a> {
    pub lanes: Lanes<'a>,
    pub spectrum: &'a Spectrum,
    pub style: HuddleStyle,
    pub channel: &'a str,
    /// The picker's selected row, while it is open.
    pub picker: Option<usize>,
}

impl std::fmt::Debug for Zoom<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Zoom { .. }")
    }
}

/// One line in the picker for each style.
pub const fn describe(style: HuddleStyle) -> &'static str {
    match style {
        HuddleStyle::Lanes => "one lane per person",
        HuddleStyle::Bars => "the call's spectrum",
        HuddleStyle::Orb => "the speaker's colour",
    }
}

/// The base of the bars: the theme's own colour (its focus colour).
fn theme_rgb(theme: &Theme) -> Rgb {
    theme
        .style(HighlightGroup::FocusBorder)
        .fg
        .and_then(named_rgb)
        .unwrap_or((0x11, 0xA8, 0xCD))
}

/// The zoomed view, over `area`.
pub fn render(buf: &mut Buffer, area: Rect, zoom: &Zoom<'_>, theme: &Theme) {
    if area.width < 20 || area.height < 6 {
        return;
    }
    let paint = Paint::new(theme, zoom.lanes.look);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_style(paint.text);
        }
    }
    header(buf, area, zoom, &paint);
    let mut body = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if let Some(selected) = zoom.picker {
        let width = picker(buf, body, selected, &paint);
        body.x += width + 1;
        body.width = body.width.saturating_sub(width + 1);
    }
    let footer = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..body
    };
    match zoom.style {
        HuddleStyle::Lanes => tall_lanes(buf, body, &zoom.lanes, &paint),
        HuddleStyle::Bars => {
            bars(buf, body, zoom, &paint, theme);
            names(buf, footer, &zoom.lanes, &paint);
        }
        HuddleStyle::Orb => {
            orb(buf, body, zoom, &paint);
            names(buf, footer, &zoom.lanes, &paint);
        }
    }
    if zoom.style == HuddleStyle::Lanes {
        mic_line(buf, footer, &zoom.lanes, &paint);
    }
    let keys = if zoom.picker.is_some() {
        "↑↓ preview · Enter keep · Esc back"
    } else {
        "v style · z back · m mute"
    };
    let at = area.right().saturating_sub(keys.width() as u16 + 1);
    let row = Rect {
        x: area.x,
        y: area.bottom() - 1,
        width: area.width,
        height: 1,
    };
    let used = (row.left()..row.right())
        .rev()
        .find(|&x| buf[(x, row.y)].symbol() != " ")
        .map_or(row.x, |x| x + 1);
    if at > used + 1 {
        put(buf, row, at - row.x, 0, keys, paint.fg(paint.grey));
    }
}

fn header(buf: &mut Buffer, area: Rect, zoom: &Zoom<'_>, paint: &Paint) {
    let lanes = &zoom.lanes;
    let names = speaking_names(lanes);
    let channel = crate::protocol::artifact::display(zoom.channel, 30);
    let place = match (area.width >= 60, channel.is_empty()) {
        (false, _) => String::new(),
        (true, true) => "huddle · ".to_owned(),
        (true, false) => format!("huddle in #{channel} · "),
    };
    let text = format!(
        "{place}{} in the call · {}",
        lanes.people.len() + 1,
        who_speaks(&names)
    );
    let style = match names.first() {
        Some(first) => {
            let slot = lanes
                .people
                .iter()
                .position(|person| person.name == *first)
                .unwrap_or(0);
            paint.fg(paint.person(slot)).add_modifier(Modifier::BOLD)
        }
        None => paint.text,
    };
    let row = Rect { height: 1, ..area };
    let used = put(buf, row, 1, 0, &text, style);
    let label = format!("{} · {}", zoom.style.name(), describe(zoom.style));
    let at = area.width.saturating_sub(label.width() as u16 + 1);
    if at > used + 3 {
        put(buf, row, at, 0, &label, paint.fg(paint.grey));
    }
}

/// The picker: each style with a line about it; the selected one is drawn
/// live behind it.
fn picker(buf: &mut Buffer, body: Rect, selected: usize, paint: &Paint) -> u16 {
    let width = 30.min(body.width / 2);
    let box_area = Rect {
        width,
        height: (HuddleStyle::ALL.len() as u16 + 2).min(body.height),
        ..body
    };
    let border = paint.fg(paint.grey);
    let (h, v, corners) = ('─', '│', ['╭', '╮', '╰', '╯']);
    let (right, bottom) = (box_area.right() - 1, box_area.bottom() - 1);
    for x in box_area.left()..=right {
        buf[(x, box_area.y)].set_char(h).set_style(border);
        buf[(x, bottom)].set_char(h).set_style(border);
    }
    for y in box_area.top()..=bottom {
        buf[(box_area.x, y)].set_char(v).set_style(border);
        buf[(right, y)].set_char(v).set_style(border);
    }
    buf[(box_area.x, box_area.y)].set_char(corners[0]);
    buf[(right, box_area.y)].set_char(corners[1]);
    buf[(box_area.x, bottom)].set_char(corners[2]);
    buf[(right, bottom)].set_char(corners[3]);
    put(
        buf,
        box_area,
        2,
        0,
        " huddle style ",
        paint.text.add_modifier(Modifier::BOLD),
    );
    for (row, style) in HuddleStyle::ALL.into_iter().enumerate() {
        let y = 1 + row as u16;
        if y + 1 >= box_area.height {
            break;
        }
        let chosen = row == selected;
        let text = format!(
            "{} {:<5} {}",
            if chosen { '▶' } else { ' ' },
            style.name(),
            describe(style)
        );
        let style = if chosen {
            paint.text.add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            paint.text
        };
        let inner = Rect {
            x: box_area.x + 1,
            width: box_area.width.saturating_sub(2),
            ..box_area
        };
        put(buf, inner, 0, y, &text, style);
    }
    width
}

/// The line of names under bars and the orb: who speaks, in their colour,
/// with `●` and bold; the others grey; your microphone last.
fn names(buf: &mut Buffer, row: Rect, lanes: &Lanes<'_>, paint: &Paint) {
    let mut x = 0;
    for (slot, person) in lanes.people.iter().enumerate() {
        let speaking = lanes.meter.peer(person.index).is_some_and(Track::speaking);
        let name = crate::ui::typing::truncate_cells(&person.name, 16);
        let (text, style) = if speaking {
            (
                format!("● {name}"),
                paint.fg(paint.person(slot)).add_modifier(Modifier::BOLD),
            )
        } else {
            (format!("· {name}"), paint.fg(paint.grey))
        };
        if x + text.width() as u16 + 2 > row.width {
            break;
        }
        x += put(buf, row, x, 0, &text, style) + 2;
    }
    let (text, style) = you(lanes, paint);
    if x + text.width() as u16 <= row.width {
        put(buf, row, x, 0, &text, style);
    }
}

fn you(lanes: &Lanes<'_>, paint: &Paint) -> (String, Style) {
    if talking_muted(lanes) {
        (
            "You · you're muted".into(),
            paint
                .fg(paint.amber)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
    } else if lanes.muted {
        ("You · muted".into(), paint.fg(paint.grey))
    } else if lanes.meter.mic().speaking() {
        (
            "● You · live".into(),
            paint.fg(paint.you).add_modifier(Modifier::BOLD),
        )
    } else {
        ("You · live".into(), paint.fg(paint.you))
    }
}

/// Under the lanes: what your microphone does.
fn mic_line(buf: &mut Buffer, row: Rect, lanes: &Lanes<'_>, paint: &Paint) {
    let (text, style) = match (lanes.muted, talking_muted(lanes)) {
        (true, true) => (
            "you're muted · nobody hears you · m unmutes",
            paint.fg(paint.amber).add_modifier(Modifier::BOLD),
        ),
        (true, false) => ("mic muted · nobody hears you", paint.fg(paint.grey)),
        (false, _) => ("mic live · everyone hears you", paint.fg(paint.you)),
    };
    put(buf, row, 0, 0, text, style);
}

/// The lower half of a tall lane, as near as Block Elements go.
fn down(eighths: usize) -> char {
    match eighths {
        0 => ' ',
        1..=3 => '▔',
        4..=7 => '▀',
        _ => '█',
    }
}

/// Lanes as tall as the window allows: up to six rows each, mirrored about
/// their middle, the right edge being now.
fn tall_lanes(buf: &mut Buffer, body: Rect, lanes: &Lanes<'_>, paint: &Paint) {
    let count = lanes.people.len() + 1;
    // One row for the scale, one for the microphone's line below.
    let room = body.height.saturating_sub(2);
    let lane_rows = (room / count as u16).clamp(2, 6) & !1;
    let shown = usize::from(room / lane_rows.max(1)).min(count);
    if shown == 0 {
        return;
    }
    let name_width: u16 = if body.width >= 80 { 24 } else { 10 };
    let lane_x = 4 + 1 + name_width;
    let lane_width = body.width.saturating_sub(lane_x + 1);
    let seconds = if lane_width >= 60 { 12.0 } else { 6.0 };
    if lane_width > 20 {
        put(
            buf,
            body,
            lane_x,
            0,
            &format!("{seconds:.0}s ago"),
            paint.fg(paint.grey),
        );
        put(
            buf,
            body,
            body.width.saturating_sub(4),
            0,
            "now",
            paint.fg(paint.grey),
        );
    }
    let now = lanes.meter.now();
    let warn = talking_muted(lanes);
    // Keep whoever spoke last when not everyone fits; you are always last.
    let mut people = lanes.people.iter().enumerate().collect::<Vec<_>>();
    if people.len() + 1 > shown {
        people.sort_by(|(_, a), (_, b)| {
            let last = |index: u8| {
                lanes
                    .meter
                    .peer(index)
                    .and_then(|track| track.since_spoke(now))
                    .unwrap_or(f64::MAX)
            };
            last(a.index).total_cmp(&last(b.index))
        });
        people.truncate(shown - 1);
        people.sort_by_key(|(slot, _)| *slot);
    }
    let mut y = 1;
    for (slot, person) in people {
        let Some(track) = lanes.meter.peer(person.index) else {
            continue;
        };
        let rgb = paint.person(slot);
        let top = y + (lane_rows - 2) / 2;
        draw_badge(
            buf,
            body,
            0,
            top,
            &person.name,
            rgb,
            track.speaking(),
            paint,
        );
        let name_style = if track.speaking() {
            paint.fg(rgb).add_modifier(Modifier::BOLD)
        } else {
            paint.text
        };
        let name = crate::ui::typing::truncate_cells(&person.name, usize::from(name_width));
        put(buf, body, 4, top, &name, name_style);
        let state_style = if track.speaking() {
            paint.fg(rgb)
        } else {
            paint.fg(paint.grey)
        };
        put(
            buf,
            body,
            4,
            top + 1,
            &state_of(track, now, lanes.motion),
            state_style,
        );
        let lane = Rect::new(body.x + lane_x, body.y + y, lane_width, lane_rows);
        tall_lane(buf, lane, track, rgb, paint, lanes.motion, seconds);
        y += lane_rows;
    }
    let mic = lanes.meter.mic();
    let mic_rgb = if warn {
        paint.amber
    } else if lanes.muted {
        paint.grey
    } else {
        paint.you
    };
    let top = y + (lane_rows - 2) / 2;
    draw_badge(buf, body, 0, top, "You", mic_rgb, mic.speaking(), paint);
    let name = if body.width >= 80 {
        "You · your microphone"
    } else {
        "You"
    };
    let name_style = if warn || (mic.speaking() && !lanes.muted) {
        paint.fg(mic_rgb).add_modifier(Modifier::BOLD)
    } else {
        paint.text
    };
    put(buf, body, 4, top, name, name_style);
    let (state, state_style) = match (lanes.muted, warn) {
        (true, true) => (
            "muted!",
            paint
                .fg(paint.amber)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        (true, false) => ("muted", paint.fg(paint.grey)),
        (false, _) if mic.speaking() => ("● live", paint.fg(paint.you)),
        (false, _) => ("live", paint.fg(paint.you)),
    };
    put(buf, body, 4, top + 1, state, state_style);
    let lane = Rect::new(body.x + lane_x, body.y + y, lane_width, lane_rows);
    tall_lane(buf, lane, mic, mic_rgb, paint, lanes.motion, seconds);
}

/// A person's level over the last `seconds` in `area` (an even number of
/// rows), mirrored about the middle, newest at the right edge.
fn tall_lane(
    buf: &mut Buffer,
    area: Rect,
    track: &Track,
    rgb: Rgb,
    paint: &Paint,
    motion: Motion,
    seconds: f32,
) {
    if area.width == 0 || area.height < 2 || area.bottom() > buf.area.bottom() {
        return;
    }
    let half = area.height / 2;
    let columns = (seconds * crate::huddle::meter::HISTORY_HZ) as usize;
    let per = columns as f32 / f32::from(area.width);
    let quiet = paint.fg(paint.grey).add_modifier(Modifier::DIM);
    let middle = area.y + half;
    for cell in 0..area.width {
        let age_cells = area.width - 1 - cell;
        let (value, age) = if motion == Motion::Off {
            (if track.speaking() { 0.75 } else { 0.0 }, 0.0)
        } else {
            let from = (f32::from(age_cells) * per) as usize;
            let to = ((f32::from(age_cells + 1) * per) as usize).max(from + 1);
            (
                track.columns_max(from, to - from),
                f32::from(age_cells) / f32::from(area.width),
            )
        };
        let eighths = (value.powf(1.6) * f32::from(half) * 8.0).round() as usize;
        let x = area.x + cell;
        if eighths == 0 {
            let symbol = if cell % 2 == 0 { "▁" } else { " " };
            buf[(x, middle - 1)].set_symbol(symbol).set_style(quiet);
            continue;
        }
        let mut style = paint.fg(paint.aged(rgb, age));
        if paint.look.depth == Depth::None && age > 0.5 {
            style = style.add_modifier(Modifier::DIM);
        }
        for row in 0..half {
            let fill = eighths.saturating_sub(usize::from(row) * 8).min(8);
            if fill == 0 {
                break;
            }
            buf[(x, middle - 1 - row)]
                .set_char(UP[fill])
                .set_style(style);
            buf[(x, middle + row)].set_char(down(fill)).set_style(style);
        }
    }
}

const EIGHTHS: [char; 9] = UP;

/// Spectrum bars: three tones by height, a baseline where a bar is empty,
/// and peak marks at quarter-cell steps.
fn bars(buf: &mut Buffer, body: Rect, zoom: &Zoom<'_>, paint: &Paint, theme: &Theme) {
    // The last row is the names.
    let area = Rect {
        height: body.height.saturating_sub(1),
        ..body
    };
    if area.height < 2 || area.width < 8 {
        return;
    }
    let (bar, gap) = if area.width >= 80 { (2, 1) } else { (1, 1) };
    let count = (area.width + gap) / (bar + gap);
    let left = (area.width - (count * (bar + gap) - gap)) / 2;
    let base = theme_rgb(theme);
    let (tones, mark) = if paint.light {
        (
            [
                lerp(base, (0xFF, 0xFF, 0xFF), 0.5),
                base,
                lerp(base, (0, 0, 0), 0.35),
            ],
            (0x24, 0x29, 0x2E),
        )
    } else {
        (
            [
                lerp(base, (0, 0, 0), 0.4),
                base,
                lerp(base, (0xFF, 0xFF, 0xFF), 0.45),
            ],
            (0xF0, 0xF0, 0xF0),
        )
    };
    let height = f32::from(area.height);
    let tone = |row: u16| {
        let at = f32::from(row) / height;
        let rgb = if at >= 0.55 {
            tones[2]
        } else if at >= 0.28 {
            tones[1]
        } else {
            tones[0]
        };
        paint.fg(rgb)
    };
    let marks: [char; 4] = match zoom.lanes.look.glyphs {
        Glyphs::Unicode => ['⎽', '⎼', '⎻', '⎺'],
        Glyphs::Blocks => ['▁', '▁', '▔', '▔'],
    };
    let baseline = paint.fg(paint.grey).add_modifier(Modifier::DIM);
    let spectrum = zoom.spectrum;
    for i in 0..count {
        let x0 = area.x + left + i * (bar + gap);
        let from = f32::from(i) / f32::from(count);
        let to = f32::from(i + 1) / f32::from(count);
        // A gentle curve: speech fills the middle, a shout the top.
        let value = spectrum.level(from, to).powf(1.35);
        let eighths = (value * height * 8.0).round() as i32;
        for row in 0..area.height {
            let y = area.bottom() - 1 - row;
            let filled = eighths - i32::from(row) * 8;
            let (symbol, style) = if filled >= 8 {
                ('█', tone(row))
            } else if filled > 0 {
                (EIGHTHS[filled as usize], tone(row))
            } else if row == 0 {
                ('▁', baseline)
            } else {
                break;
            };
            for dx in 0..bar {
                buf[(x0 + dx, y)].set_char(symbol).set_style(style);
            }
        }
        if zoom.lanes.motion == Motion::Off {
            continue;
        }
        let peak = spectrum.peak(from, to).powf(1.35);
        let quarters = (peak * height * 4.0).round() as i32;
        let row = quarters / 4;
        if peak > value + 0.03 && row >= 0 && (row as u16) < area.height {
            let y = area.bottom() - 1 - row as u16;
            let style = paint.fg(mark).add_modifier(Modifier::BOLD);
            for dx in 0..bar {
                buf[(x0 + dx, y)]
                    .set_char(marks[(quarters % 4) as usize])
                    .set_style(style);
            }
        }
    }
}

/// What a pixel of the orb is, for drawing without colour.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Matter {
    Liquid,
    Glass,
}

/// The orb: a sphere whose liquid rises with the level of the call and
/// ripples with its spectrum, in the colour of whoever speaks.
fn orb(buf: &mut Buffer, body: Rect, zoom: &Zoom<'_>, paint: &Paint) {
    let area = Rect {
        height: body.height.saturating_sub(1),
        ..body
    };
    let rows = area.height.min(ORB_MAX_ROWS).min(area.width / 2);
    if rows < 3 {
        return;
    }
    let cols = rows * 2;
    let origin_x = area.x + (area.width - cols) / 2;
    let origin_y = area.y + (area.height - rows) / 2;
    let lanes = &zoom.lanes;
    let speaking = lanes
        .people
        .iter()
        .enumerate()
        .filter_map(|(slot, person)| {
            lanes
                .meter
                .peer(person.index)
                .filter(|track| track.speaking())
                .map(|track| (paint.person(slot), track.level()))
        })
        .collect::<Vec<_>>();
    let liquid_a = speaking.first().map_or(paint.grey, |(rgb, _)| *rgb);
    let liquid_b = speaking.get(1).map_or(liquid_a, |(rgb, _)| *rgb);
    let level = speaking.iter().map(|(_, level)| *level).fold(0.0, f32::max);
    let (level, ripple, t) = match lanes.motion {
        Motion::Off => (if speaking.is_empty() { 0.0 } else { 0.6 }, 0.0, 0.0),
        Motion::Calm => (level, 0.5, lanes.meter.now() as f32),
        Motion::Full => (level, 1.0, lanes.meter.now() as f32),
    };
    let glass: Rgb = if paint.light {
        (0xDD, 0xE2, 0xE8)
    } else {
        (0xD8, 0xDC, 0xE4)
    };
    let shade: Rgb = if paint.light {
        (0x8A, 0x90, 0x98)
    } else {
        (0x20, 0x1E, 0x1C)
    };
    let surface = |x: f32| {
        let mut y = 0.26 + 0.44 * level;
        for k in 0..6 {
            let band = k as f32 * 0.15 + 0.05;
            let b = zoom.spectrum.level(band, band + 0.12);
            let frequency = 1.5 + k as f32 * 1.3;
            y += ripple
                * 0.08
                * b
                * (frequency * x * 3.1 + t * (1.6 + k as f32 * 0.7) + k as f32).sin();
        }
        y
    };
    let (w, h) = (f32::from(cols), f32::from(rows) * 2.0);
    let radius = (h / 2.0 - 0.5).min(w / 2.0 - 0.5);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let pixel = |px: u16, py: u16| -> Option<(Rgb, Matter)> {
        let dx = (f32::from(px) + 0.5 - cx) / radius;
        let dy = (f32::from(py) + 0.5 - cy) / radius;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > 1.0 {
            return None;
        }
        let height = 1.0 - (dy + 1.0) / 2.0;
        // Two people at once: the liquid goes from one colour to the other.
        let liquid = lerp(liquid_a, liquid_b, (dx + 1.0) / 2.0 * 1.6 - 0.3);
        let s = surface(dx);
        let (rgb, matter) = if height < s {
            let depth = s - height;
            (
                lerp(lerp(liquid, glass, 0.35), liquid, (depth * 7.0).min(1.0)),
                Matter::Liquid,
            )
        } else {
            let above = height - s;
            (
                lerp(lerp(liquid, glass, 0.6), glass, (above * 14.0).min(1.0)),
                Matter::Glass,
            )
        };
        // Darker towards the rim, so it reads as a sphere.
        let rgb = lerp(rgb, shade, (distance - 0.72).max(0.0) * 1.6);
        // Coarse steps: neighbouring frames repeat more cells, which the
        // terminal is then not sent again.
        let step = |v: u8| v & 0xF8;
        Some(((step(rgb.0), step(rgb.1), step(rgb.2)), matter))
    };
    let colour = paint.look.depth != Depth::None;
    for row in 0..rows {
        for col in 0..cols {
            let top = pixel(col, row * 2);
            let bottom = pixel(col, row * 2 + 1);
            let cell = &mut buf[(origin_x + col, origin_y + row)];
            if !colour {
                let symbol = match (top.map(|p| p.1), bottom.map(|p| p.1)) {
                    (None, None) => continue,
                    (Some(Matter::Liquid), Some(Matter::Liquid)) => '█',
                    (_, Some(Matter::Liquid)) => '▄',
                    (Some(Matter::Liquid), _) => '▀',
                    _ => '░',
                };
                cell.set_char(symbol);
                continue;
            }
            match (top, bottom) {
                (Some((t, _)), Some((b, _))) => {
                    cell.set_char('▀');
                    cell.fg = paint.color(t).unwrap_or_default();
                    cell.bg = paint.color(b).unwrap_or_default();
                }
                (Some((t, _)), None) => {
                    cell.set_char('▀');
                    cell.fg = paint.color(t).unwrap_or_default();
                }
                (None, Some((b, _))) => {
                    cell.set_char('▄');
                    cell.fg = paint.color(b).unwrap_or_default();
                }
                (None, None) => {}
            }
        }
    }
    // The name of whoever the orb is coloured by, under it.
    let names = speaking_names(lanes);
    let label = match names.as_slice() {
        [] => "quiet".to_owned(),
        [one] => (*one).to_owned(),
        [a, b, ..] => format!("{a} + {b}"),
    };
    if origin_y + rows < area.bottom() {
        let x = (area.width / 2).saturating_sub(label.width() as u16 / 2);
        let style = match speaking.first() {
            Some((rgb, _)) => paint.fg(*rgb).add_modifier(Modifier::BOLD),
            None => paint.fg(paint.grey),
        };
        put(buf, area, x, origin_y + rows - area.y, &label, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        huddle::{levels::Levels, meter::Meter, synth},
        ui::huddle_lanes::{Look, Person},
    };
    use ratatui::style::Color;

    /// The generated passage up to `at` seconds, at 30 frames a second, with
    /// the spectrum read from its mix.
    fn passage(at: f64) -> (Meter, Spectrum) {
        let mut source = synth::Synthetic::new(&synth::DEMO_SCRIPT, None);
        let levels: std::sync::Arc<Levels> = source.levels();
        levels.mix().want(true);
        let mut meter = Meter::default();
        let mut spectrum = Spectrum::default();
        let mut t = 0.0;
        while t <= at + 1e-9 {
            source.advance_to(t);
            meter.update(levels.as_ref(), &[1, 2, 3], t);
            spectrum.update(levels.mix(), t);
            t += 1.0 / 30.0;
        }
        (meter, spectrum)
    }

    fn people() -> Vec<Person> {
        ["Ada", "Lin", "Sam"]
            .iter()
            .enumerate()
            .map(|(i, name)| Person {
                index: i as u8 + 1,
                name: (*name).into(),
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        meter: &Meter,
        spectrum: &Spectrum,
        style: HuddleStyle,
        (width, height): (u16, u16),
        look: Look,
        theme: &str,
        motion: Motion,
        picker: Option<usize>,
    ) -> Buffer {
        let theme = Theme::builtin(theme).unwrap();
        let people = people();
        let zoom = Zoom {
            lanes: Lanes {
                meter,
                people: &people,
                muted: true,
                motion,
                look,
                recording: false,
            },
            spectrum,
            style,
            channel: "team",
            picker,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        let area = buf.area;
        render(&mut buf, area, &zoom, &theme);
        buf
    }

    pub(crate) fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Text and the style of every run of cells.
    fn golden(buf: &Buffer) -> String {
        let mut out = text(buf);
        out.push_str("\n--- styles ---\n");
        for y in 0..buf.area.height {
            let mut row = Vec::new();
            let mut x = 0;
            while x < buf.area.width {
                let style = |x: u16| {
                    let cell = &buf[(x, y)];
                    (cell.fg, cell.bg, cell.modifier)
                };
                let first = style(x);
                let mut end = x;
                while end + 1 < buf.area.width && style(end + 1) == first {
                    end += 1;
                }
                if first != (Color::Reset, Color::Reset, Modifier::empty()) {
                    row.push(format!(
                        "{x}-{end} {:?}/{:?}/{:?}",
                        first.0, first.1, first.2
                    ));
                }
                x = end + 1;
            }
            if !row.is_empty() {
                out.push_str(&format!("{y}: {}\n", row.join(" ")));
            }
        }
        out
    }

    fn check_golden(name: &str, actual: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/huddle_zoom")
            .join(format!("{name}.txt"));
        if std::env::var_os("BZZ_UPDATE_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; run with BZZ_UPDATE_GOLDENS=1",
                path.display()
            )
        });
        assert_eq!(actual, expected, "golden {name} differs");
    }

    const TRUE: Look = Look {
        depth: Depth::TrueColor,
        glyphs: Glyphs::Unicode,
    };
    const C256: Look = Look {
        depth: Depth::Ansi256,
        glyphs: Glyphs::Unicode,
    };
    const BLOCKS: Look = Look {
        depth: Depth::Ansi256,
        glyphs: Glyphs::Blocks,
    };
    const NONE: Look = Look {
        depth: Depth::None,
        glyphs: Glyphs::Blocks,
    };

    /// Each style at 42×12 and 120×30, in true colour, 256 colours, block
    /// characters only, `NO_COLOR` and a light theme, at the moment Ada and
    /// Lin speak at once (5.5 s of the generated passage).
    #[test]
    fn zoom_goldens() {
        let (meter, spectrum) = passage(5.5);
        for style in HuddleStyle::ALL {
            for (size, dims) in [("42x12", (42, 12)), ("120x30", (120, 30))] {
                for (look_name, look, theme) in [
                    ("bzz", TRUE, "bzz"),
                    ("light", TRUE, "light"),
                    ("ansi256", C256, "bzz"),
                    ("blocks", BLOCKS, "bzz"),
                    ("no-color", NONE, "bzz"),
                ] {
                    let buf = draw(
                        &meter,
                        &spectrum,
                        style,
                        dims,
                        look,
                        theme,
                        Motion::Full,
                        None,
                    );
                    check_golden(
                        &format!("{}-{size}-{look_name}", style.name()),
                        &golden(&buf),
                    );
                }
            }
        }
        // The picker over the bars, live behind it.
        let buf = draw(
            &meter,
            &spectrum,
            HuddleStyle::Bars,
            (120, 30),
            TRUE,
            "bzz",
            Motion::Full,
            Some(1),
        );
        check_golden("picker-bars-120x30-bzz", &golden(&buf));
    }

    /// Who speaks is on screen in every style, in words.
    #[test]
    fn every_style_names_who_speaks() {
        let (meter, spectrum) = passage(5.5);
        for style in HuddleStyle::ALL {
            for dims in [(42, 12), (120, 30)] {
                for look in [TRUE, NONE] {
                    let screen = text(&draw(
                        &meter,
                        &spectrum,
                        style,
                        dims,
                        look,
                        "bzz",
                        Motion::Full,
                        None,
                    ));
                    assert!(screen.contains("Ada and Lin at once"), "{screen}");
                    if style != HuddleStyle::Lanes {
                        assert!(screen.contains("● Ada"), "{screen}");
                        assert!(screen.contains("● Lin"), "{screen}");
                        assert!(screen.contains("· Sam"), "{screen}");
                        assert!(screen.contains("You · muted"), "{screen}");
                    }
                }
            }
        }
        let (meter, spectrum) = passage(7.1);
        let screen = text(&draw(
            &meter,
            &spectrum,
            HuddleStyle::Orb,
            (120, 30),
            NONE,
            "bzz",
            Motion::Full,
            None,
        ));
        assert!(screen.contains("You · you're muted"), "{screen}");
    }

    #[test]
    fn no_color_and_256_colours_and_blocks_hold_in_every_style() {
        let (meter, spectrum) = passage(5.5);
        for style in HuddleStyle::ALL {
            let buf = draw(
                &meter,
                &spectrum,
                style,
                (120, 30),
                NONE,
                "bzz",
                Motion::Full,
                None,
            );
            for cell in buf.content() {
                assert_eq!(
                    (cell.fg, cell.bg),
                    (Color::Reset, Color::Reset),
                    "{style:?}"
                );
            }
            let buf = draw(
                &meter,
                &spectrum,
                style,
                (120, 30),
                C256,
                "bzz",
                Motion::Full,
                None,
            );
            for cell in buf.content() {
                for color in [cell.fg, cell.bg] {
                    assert!(!matches!(color, Color::Rgb(..)), "{style:?} {color:?}");
                }
            }
            let buf = draw(
                &meter,
                &spectrum,
                style,
                (120, 30),
                BLOCKS,
                "bzz",
                Motion::Full,
                None,
            );
            for c in text(&buf).chars() {
                assert!(
                    c.is_ascii()
                        || ('\u{2580}'..='\u{259F}').contains(&c)
                        || "●·…▶↑↓╭╮╰╯─│".contains(c),
                    "{style:?}: {c:?} in blocks-only"
                );
            }
        }
    }

    /// The bars are the call's spectrum: lit while people speak, a flat
    /// baseline in silence.
    #[test]
    fn the_bars_rise_with_speech_and_rest_in_silence() {
        let lit = |buf: &Buffer| {
            buf.content()
                .iter()
                .filter(|cell| UP[2..].contains(&cell.symbol().chars().next().unwrap_or(' ')))
                .count()
        };
        let (meter, spectrum) = passage(1.5);
        let speaking = draw(
            &meter,
            &spectrum,
            HuddleStyle::Bars,
            (120, 30),
            TRUE,
            "bzz",
            Motion::Full,
            None,
        );
        let (meter, spectrum) = passage(2.95);
        let quiet = draw(
            &meter,
            &spectrum,
            HuddleStyle::Bars,
            (120, 30),
            TRUE,
            "bzz",
            Motion::Full,
            None,
        );
        assert!(lit(&speaking) > 60, "{}", text(&speaking));
        assert_eq!(lit(&quiet), 0, "{}", text(&quiet));
        assert!(text(&quiet).contains("nobody is speaking"));
    }

    /// The orb takes the colour of who speaks, two colours for two, grey in
    /// silence, and never more than [`ORB_MAX_ROWS`] rows.
    #[test]
    fn the_orb_is_in_the_speakers_colour_and_capped() {
        let colours = |at: f64| {
            let (meter, spectrum) = passage(at);
            let buf = draw(
                &meter,
                &spectrum,
                HuddleStyle::Orb,
                (120, 30),
                TRUE,
                "bzz",
                Motion::Full,
                None,
            );
            let rows = (0..30)
                .filter(|&y| (0..120).any(|x| "▀▄".contains(buf[(x, y)].symbol())))
                .count();
            assert!(rows <= usize::from(ORB_MAX_ROWS), "{rows}");
            buf.content()
                .iter()
                .filter(|cell| cell.symbol() == "▀")
                .filter_map(|cell| match cell.bg {
                    Color::Rgb(r, g, b) => Some((r, g, b)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let paint = Paint::new(&Theme::builtin("bzz").unwrap(), TRUE);
        let near = |pixels: &[Rgb], rgb: Rgb| {
            pixels.iter().any(|p| {
                (i32::from(p.0) - i32::from(rgb.0)).abs() < 12
                    && (i32::from(p.1) - i32::from(rgb.1)).abs() < 12
                    && (i32::from(p.2) - i32::from(rgb.2)).abs() < 12
            })
        };
        let ada = colours(1.5);
        assert!(near(&ada, paint.person(0)) && !near(&ada, paint.person(1)));
        let both = colours(5.5);
        assert!(near(&both, paint.person(0)) && near(&both, paint.person(1)));
        let quiet = colours(2.95);
        assert!(!near(&quiet, paint.person(0)) && near(&quiet, paint.grey));
    }

    /// A terminal writer that takes `delay` to flush, as over a slow link.
    struct SlowWriter {
        delay_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
        bytes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        /// Time spent writing, in microseconds.
        spent: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    impl std::io::Write for SlowWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.bytes
                .fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            let started = std::time::Instant::now();
            let ms = self.delay_ms.load(std::sync::atomic::Ordering::Relaxed);
            // A sleep of zero still gives up the time slice, which on
            // Windows is long enough to look like a slow writer.
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            self.spent.fetch_add(
                started.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            Ok(())
        }
    }

    /// The orb, drawn through a real terminal backend to a writer that
    /// sleeps: the rate steps down to five frames a second; when the writer
    /// is quick again it climbs back to twenty.
    #[test]
    fn the_orb_drops_its_rate_on_a_slow_writer() {
        use crate::huddle::meter::Throttle;
        let theme = Theme::builtin("bzz").unwrap();
        let people = people();
        let base = 1.0 / 20.0;
        let mut throttle = Throttle::default();
        let delay_ms = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(30));
        let bytes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let spent = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let size = Rect::new(0, 0, 120, 30);
        let mut terminal = ratatui::Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(SlowWriter {
                delay_ms: delay_ms.clone(),
                bytes: bytes.clone(),
                spent: spent.clone(),
            }),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(size),
            },
        )
        .unwrap();
        let mut source = synth::Synthetic::new(&synth::DEMO_SCRIPT, None);
        let levels = source.levels();
        levels.mix().want(true);
        let mut meter = Meter::default();
        let mut spectrum = Spectrum::default();
        let mut t = 0.0;
        let mut intervals = Vec::new();
        for frame in 0..60 {
            if frame == 30 {
                delay_ms.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            source.advance_to(t);
            meter.update(levels.as_ref(), &[1, 2, 3], t);
            spectrum.update(levels.mix(), t);
            let zoom = Zoom {
                lanes: Lanes {
                    meter: &meter,
                    people: &people,
                    muted: true,
                    motion: Motion::Full,
                    look: TRUE,
                    recording: false,
                },
                spectrum: &spectrum,
                style: HuddleStyle::Orb,
                channel: "team",
                picker: None,
            };
            // The time the frame spent being written (in bzz, the whole
            // draw is timed; here only the writer, so a busy test machine
            // does not make a quick writer look slow).
            let before = spent.load(std::sync::atomic::Ordering::Relaxed);
            terminal
                .draw(|frame| render(frame.buffer_mut(), size, &zoom, &theme))
                .unwrap();
            let micros = spent.load(std::sync::atomic::Ordering::Relaxed) - before;
            throttle.record(micros as f64 / 1e6, base);
            intervals.push(throttle.interval(base));
            t += throttle.interval(base);
        }
        // Slow: doubled every three frames, to 0.1 then 0.2 s, and held.
        assert!((intervals[2] - 0.1).abs() < 1e-9, "{intervals:?}");
        assert!((intervals[5] - 0.2).abs() < 1e-9, "{intervals:?}");
        assert!(
            (intervals[29] - Throttle::SLOWEST).abs() < 1e-9,
            "{intervals:?}"
        );
        // Quick again: back up a step every thirty quick frames.
        assert!(intervals[59] < Throttle::SLOWEST, "{intervals:?}");
        assert!(bytes.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    /// Motion off: a still picture that changes only with the speaker.
    #[test]
    fn motion_off_holds_still_until_the_speaker_changes() {
        for style in HuddleStyle::ALL {
            let at = |t: f64| {
                let (meter, spectrum) = passage(t);
                text(&draw(
                    &meter,
                    &spectrum,
                    style,
                    (120, 30),
                    TRUE,
                    "bzz",
                    Motion::Off,
                    None,
                ))
            };
            if style != HuddleStyle::Bars {
                assert_eq!(at(1.2), at(1.6), "{style:?}: Ada speaks throughout");
            }
            assert_ne!(at(1.2), at(4.0), "{style:?}: Lin took over");
        }
    }
}
