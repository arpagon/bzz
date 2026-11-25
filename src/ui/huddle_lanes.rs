//! The huddle visualizer: one lane per person in the `:huddle` panel, your
//! microphone last, and a one-line strip for the status line (plan v0.17.0
//! B2, B3; the style the owner chose from research R3).
//!
//! A lane is two rows: a badge in the person's colour (ringed while they
//! speak), the name, a state in words ("● live", "6s ago", "muted"), and
//! their own level over the last 6 to 10 seconds, mirrored about the middle,
//! the right edge being now and the past fading to grey. Colour is never the
//! only signal (rule 21): speaking is a ring, bold and "● live"; talking
//! while muted is "muted!" and a line of words. Lanes use Block Elements
//! only; the strip uses braille unless [`Glyphs::Blocks`].
//!
//! Everything is drawn from a [`Meter`] and forgotten; nothing here keeps a
//! level.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    config::Motion,
    huddle::meter::{HISTORY_HZ, MUTED_TALK_SECONDS, Meter, Track},
    ui::theme::{HighlightGroup, Theme},
};

/// How many colours the terminal shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Depth {
    TrueColor,
    Ansi256,
    /// `NO_COLOR`: no cell of the visualizer carries a colour.
    None,
}

/// Which glyphs the font is trusted with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Glyphs {
    /// Block Elements and braille.
    Unicode,
    /// Block Elements only.
    Blocks,
}

/// How the visualizer may draw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Look {
    pub depth: Depth,
    pub glyphs: Glyphs,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            depth: Depth::TrueColor,
            glyphs: Glyphs::Unicode,
        }
    }
}

impl Look {
    /// SEAM (agent `look`, plan L1): until bzz has one colour-depth setting
    /// for the whole interface, the visualizer reads the environment itself:
    /// `NO_COLOR` (any value) for no colour, `COLORTERM` for true colour,
    /// else 256 colours; `TERM=linux` or `BZZ_BLOCKS_ONLY` for block glyphs.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        Self::from_vars(
            var("NO_COLOR").as_deref(),
            var("COLORTERM").as_deref(),
            var("TERM").as_deref(),
            var("BZZ_BLOCKS_ONLY").as_deref(),
        )
    }

    pub fn from_vars(
        no_color: Option<&str>,
        colorterm: Option<&str>,
        term: Option<&str>,
        blocks_only: Option<&str>,
    ) -> Self {
        let depth = if no_color.is_some_and(|value| !value.is_empty()) {
            Depth::None
        } else if colorterm.is_some_and(|value| {
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        }) {
            Depth::TrueColor
        } else {
            Depth::Ansi256
        };
        let glyphs = if term == Some("linux") || blocks_only.is_some_and(|value| !value.is_empty())
        {
            Glyphs::Blocks
        } else {
            Glyphs::Unicode
        };
        Self { depth, glyphs }
    }
}

pub(crate) type Rgb = (u8, u8, u8);

/// Person colours for dark and light backgrounds. None is amber: amber
/// means "you're muted".
const PEOPLE_DARK: [Rgb; 6] = [
    (0x2E, 0xC4, 0xC4),
    (0xD0, 0x7C, 0xE0),
    (0x8A, 0xD6, 0x4A),
    (0x5C, 0x9D, 0xFF),
    (0xFF, 0x7B, 0x72),
    (0xA9, 0x9C, 0xFF),
];
const PEOPLE_LIGHT: [Rgb; 6] = [
    (0x00, 0x83, 0x8F),
    (0x9C, 0x3F, 0xB0),
    (0x2E, 0x8B, 0x2E),
    (0x1F, 0x5F, 0xD1),
    (0xC8, 0x3E, 0x35),
    (0x6A, 0x50, 0xD8),
];

pub(crate) fn named_rgb(color: Color) -> Option<Rgb> {
    Some(match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (0xCD, 0x31, 0x31),
        Color::Green => (0x0D, 0xBC, 0x79),
        Color::Yellow => (0xE5, 0xC0, 0x10),
        Color::Blue => (0x24, 0x72, 0xC8),
        Color::Magenta => (0xBC, 0x3F, 0xBC),
        Color::Cyan => (0x11, 0xA8, 0xCD),
        Color::Gray => (0xC0, 0xC0, 0xC0),
        Color::DarkGray => (0x76, 0x76, 0x76),
        Color::LightRed => (0xF1, 0x4C, 0x4C),
        Color::LightGreen => (0x23, 0xD1, 0x8B),
        Color::LightYellow => (0xF5, 0xF5, 0x43),
        Color::LightBlue => (0x3B, 0x8E, 0xEA),
        Color::LightMagenta => (0xD6, 0x70, 0xD6),
        Color::LightCyan => (0x29, 0xB8, 0xDB),
        Color::White => (0xE5, 0xE5, 0xE5),
        _ => return None,
    })
}

/// The nearest colour of the xterm 256-colour cube or grey ramp.
pub fn to_ansi256((r, g, b): Rgb) -> u8 {
    let level = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let value = |l: u8| if l == 0 { 0 } else { 55 + 40 * l };
    let (lr, lg, lb) = (level(r), level(g), level(b));
    let cube = (value(lr), value(lg), value(lb));
    let average = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let grey_index = if average > 238 {
        23
    } else {
        (average.saturating_sub(3) / 10).min(23) as u8
    };
    let grey = 8 + 10 * grey_index;
    let distance = |(a, b2, c): Rgb| {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a, r) + d(b2, g) + d(c, b)
    };
    if distance((grey, grey, grey)) < distance(cube) {
        232 + grey_index
    } else {
        16 + 36 * lr + 6 * lg + lb
    }
}

pub(crate) fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The colours of one frame, resolved from the theme and the look.
pub(crate) struct Paint {
    pub(crate) look: Look,
    people: [Rgb; 6],
    pub(crate) you: Rgb,
    pub(crate) amber: Rgb,
    pub(crate) grey: Rgb,
    pub(crate) text: Style,
    /// Whether the theme's background is light.
    pub(crate) light: bool,
}

impl Paint {
    pub(crate) fn new(theme: &Theme, look: Look) -> Self {
        let normal = theme.style(HighlightGroup::Normal);
        let light = normal
            .bg
            .and_then(named_rgb)
            .is_some_and(|(r, g, b)| u32::from(r) + u32::from(g) + u32::from(b) > 3 * 140);
        let fg = |group: HighlightGroup, fallback: Rgb| {
            theme
                .style(group)
                .fg
                .and_then(named_rgb)
                .unwrap_or(fallback)
        };
        Self {
            look,
            people: if light { PEOPLE_LIGHT } else { PEOPLE_DARK },
            you: fg(HighlightGroup::Success, (0x50, 0xC8, 0x78)),
            amber: fg(HighlightGroup::Warning, (0xE0, 0xA0, 0x30)),
            grey: if light {
                (0x9A, 0xA0, 0xA6)
            } else {
                (0x6C, 0x6C, 0x6C)
            },
            text: if look.depth == Depth::None {
                Style::default()
            } else {
                normal
            },
            light,
        }
    }

    pub(crate) fn color(&self, rgb: Rgb) -> Option<Color> {
        match self.look.depth {
            Depth::TrueColor => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
            Depth::Ansi256 => Some(Color::Indexed(to_ansi256(rgb))),
            Depth::None => None,
        }
    }

    pub(crate) fn fg(&self, rgb: Rgb) -> Style {
        match self.color(rgb) {
            Some(color) => self.text.fg(color),
            None => Style::default(),
        }
    }

    pub(crate) fn person(&self, slot: usize) -> Rgb {
        self.people[slot % self.people.len()]
    }

    /// Older history greys out; the right edge is the colour itself.
    pub(crate) fn aged(&self, rgb: Rgb, age: f32) -> Rgb {
        lerp(rgb, self.grey, (age * 1.4).min(0.85))
    }
}

/// A person in the call, in lane order.
#[derive(Clone)]
pub struct Person {
    pub index: u8,
    pub name: String,
}

impl std::fmt::Debug for Person {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Person { .. }")
    }
}

/// What the visualizer draws from.
pub struct Lanes<'a> {
    pub meter: &'a Meter,
    /// The other people, in roster order (you are not among them).
    pub people: &'a [Person],
    pub muted: bool,
    pub motion: Motion,
    pub look: Look,
}

impl std::fmt::Debug for Lanes<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Lanes { .. }")
    }
}

/// Talking while muted, long enough to say so.
pub(crate) fn talking_muted(lanes: &Lanes<'_>) -> bool {
    lanes.muted
        && lanes.meter.mic().speaking()
        && lanes.meter.mic().open_for() >= MUTED_TALK_SECONDS
}

pub(crate) fn speaking_names<'a>(lanes: &Lanes<'a>) -> Vec<&'a str> {
    lanes
        .people
        .iter()
        .filter(|person| lanes.meter.peer(person.index).is_some_and(Track::speaking))
        .map(|person| person.name.as_str())
        .collect()
}

pub(crate) fn who_speaks(names: &[&str]) -> String {
    match names {
        [] => "nobody is speaking".into(),
        [one] => format!("{one} is speaking"),
        [a, b] => format!("{a} and {b} at once"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more at once", rest.len()),
    }
}

pub(crate) fn put(buf: &mut Buffer, area: Rect, x: u16, y: u16, text: &str, style: Style) -> u16 {
    if y >= area.height || x >= area.width {
        return 0;
    }
    let room = usize::from(area.width - x);
    let text = crate::ui::typing::truncate_cells(text, room);
    let (end, _) = buf.set_stringn(area.x + x, area.y + y, &text, room, style);
    end - (area.x + x)
}

pub(crate) fn state_of(track: &Track, now: f64, motion: Motion) -> String {
    if track.speaking() {
        return "● live".into();
    }
    match track.since_spoke(now) {
        // A still meter shows no clock.
        Some(_) if motion == Motion::Off => "quiet".into(),
        Some(seconds) if seconds < 60.0 => format!("{}s ago", seconds.floor().max(1.0) as u64),
        Some(seconds) if seconds < 3600.0 => format!("{}m ago", (seconds / 60.0).floor() as u64),
        _ => "quiet".into(),
    }
}

/// Lower eighths, the upper half of a lane growing up from the middle.
pub(crate) const UP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The lower half mirrors it, as near as Block Elements go.
fn down(eighths: usize) -> char {
    match eighths {
        0 => ' ',
        1..=3 => '▔',
        4..=7 => '▀',
        _ => '█',
    }
}

/// One person's level over the last `seconds`, newest at the right edge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_lane(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    width: u16,
    track: &Track,
    rgb: Rgb,
    paint: &Paint,
    motion: Motion,
    seconds: f32,
) {
    if width == 0 || y + 1 >= area.height {
        return;
    }
    let columns = (seconds * HISTORY_HZ) as usize;
    let per = columns as f32 / f32::from(width);
    let quiet = paint.fg(paint.grey).add_modifier(Modifier::DIM);
    for cell in 0..width {
        let age_cells = width - 1 - cell;
        let (value, age) = if motion == Motion::Off {
            // A still meter: full while speaking, flat otherwise.
            (if track.speaking() { 0.75 } else { 0.0 }, 0.0)
        } else {
            let from = (f32::from(age_cells) * per) as usize;
            let to = ((f32::from(age_cells + 1) * per) as usize).max(from + 1);
            (
                track.columns_max(from, to - from),
                f32::from(age_cells) / f32::from(width),
            )
        };
        let eighths = (value.powf(1.6) * 8.0).round() as usize;
        let (cx, top, bottom) = (area.x + x + cell, area.y + y, area.y + y + 1);
        if eighths == 0 {
            let symbol = if cell % 2 == 0 { "▁" } else { " " };
            buf[(cx, top)].set_symbol(symbol).set_style(quiet);
            buf[(cx, bottom)].set_symbol(" ").set_style(paint.text);
            continue;
        }
        let mut style = paint.fg(paint.aged(rgb, age));
        if paint.look.depth == Depth::None && age > 0.5 {
            style = style.add_modifier(Modifier::DIM);
        }
        buf[(cx, top)].set_char(UP[eighths.min(8)]).set_style(style);
        buf[(cx, bottom)].set_char(down(eighths)).set_style(style);
    }
}

/// A two-row badge: the initial on the person's colour, ringed while they
/// speak (the ring reads without colour too).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_badge(
    buf: &mut Buffer,
    area: Rect,
    x: u16,
    y: u16,
    name: &str,
    rgb: Rgb,
    ring: bool,
    paint: &Paint,
) {
    if x + 3 > area.width || y + 1 >= area.height {
        return;
    }
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or('?', |c| c.to_uppercase().next().unwrap_or(c));
    let ink = if u32::from(rgb.0) * 3 + u32::from(rgb.1) * 6 + u32::from(rgb.2) > 1_400 {
        (0x14, 0x11, 0x0D)
    } else {
        (0xFF, 0xFF, 0xFF)
    };
    let fill = match paint.color(if ring {
        rgb
    } else {
        lerp(rgb, paint.grey, 0.55)
    }) {
        Some(bg) => paint.fg(ink).bg(bg),
        None => Style::default(),
    };
    for row in 0..2 {
        let (bx, by) = (area.x + x, area.y + y + row);
        let side = if ring { paint.fg(rgb) } else { paint.text };
        buf[(bx, by)]
            .set_char(if ring { '▐' } else { ' ' })
            .set_style(side);
        buf[(bx + 2, by)]
            .set_char(if ring { '▌' } else { ' ' })
            .set_style(side);
        buf[(bx + 1, by)]
            .set_char(if row == 0 { initial } else { ' ' })
            .set_style(fill.add_modifier(Modifier::BOLD));
    }
}

/// The lanes: a header, a lane per person, your microphone last, and a
/// line about your microphone when there is room. Fits 42×12 and up.
pub fn render(buf: &mut Buffer, area: Rect, lanes: &Lanes<'_>, theme: &Theme) {
    if area.width < 20 || area.height < 3 {
        return;
    }
    let paint = Paint::new(theme, lanes.look);
    let now = lanes.meter.now();
    let wide = area.width >= 64;
    let name_width: u16 = if wide { 22 } else { 9 };
    let lane_x = 1 + 3 + 1 + name_width + 1;
    let lane_width = area.width.saturating_sub(lane_x + 1);
    let seconds = if lane_width >= 40 { 10.0 } else { 6.0 };
    let warn = talking_muted(lanes);

    // Rows: the header, two a lane, your microphone, a line about it.
    let footer = u16::from(area.height >= 6);
    let room = (area.height.saturating_sub(1 + 2 + footer) / 2) as usize;
    let mut shown = lanes.people.iter().enumerate().collect::<Vec<_>>();
    let hidden = shown.len().saturating_sub(room);
    if hidden > 0 {
        // Keep whoever spoke last.
        shown.sort_by(|(_, a), (_, b)| {
            let last = |person: &Person| {
                lanes
                    .meter
                    .peer(person.index)
                    .and_then(|track| track.since_spoke(now))
                    .unwrap_or(f64::MAX)
            };
            last(a).total_cmp(&last(b))
        });
        shown.truncate(room);
        shown.sort_by_key(|(slot, _)| *slot);
    }

    let names = speaking_names(lanes);
    let mut header = format!(
        "{} in the call · {}",
        lanes.people.len() + 1,
        who_speaks(&names)
    );
    if hidden > 0 {
        header.push_str(&format!(" · +{hidden} more"));
    }
    let header_style = match names.as_slice() {
        [] => paint.fg(paint.grey),
        _ => {
            let slot = lanes
                .people
                .iter()
                .position(|person| person.name == names[0])
                .unwrap_or(0);
            paint.fg(paint.person(slot)).add_modifier(Modifier::BOLD)
        }
    };
    let used = put(buf, area, 1, 0, &header, header_style);
    if wide && lane_x + lane_width > used + 12 {
        let scale = format!("{seconds:.0}s ago … now");
        let at = area.width.saturating_sub(scale.width() as u16 + 1);
        if at > used + 1 {
            put(buf, area, at, 0, &scale, paint.fg(paint.grey));
        }
    }

    let mut y = 1;
    for (slot, person) in &shown {
        let Some(track) = lanes.meter.peer(person.index) else {
            continue;
        };
        let rgb = paint.person(*slot);
        draw_badge(buf, area, 1, y, &person.name, rgb, track.speaking(), &paint);
        let name_style = if track.speaking() {
            paint.fg(rgb).add_modifier(Modifier::BOLD)
        } else {
            paint.text
        };
        let name = crate::ui::typing::truncate_cells(&person.name, usize::from(name_width));
        put(buf, area, 5, y, &name, name_style);
        let state = state_of(track, now, lanes.motion);
        let state_style = if track.speaking() {
            paint.fg(rgb)
        } else {
            paint.fg(paint.grey)
        };
        put(buf, area, 5, y + 1, &state, state_style);
        draw_lane(
            buf,
            area,
            lane_x,
            y,
            lane_width,
            track,
            rgb,
            &paint,
            lanes.motion,
            seconds,
        );
        y += 2;
    }

    // Your microphone, last: it moves while muted, so you see it works.
    let mic = lanes.meter.mic();
    let mic_rgb = if warn {
        paint.amber
    } else if lanes.muted {
        paint.grey
    } else {
        paint.you
    };
    draw_badge(buf, area, 1, y, "You", mic_rgb, mic.speaking(), &paint);
    let name = if wide {
        "You · your microphone"
    } else {
        "You"
    };
    let name_style = if warn || (mic.speaking() && !lanes.muted) {
        paint.fg(mic_rgb).add_modifier(Modifier::BOLD)
    } else {
        paint.text
    };
    put(buf, area, 5, y, name, name_style);
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
    put(buf, area, 5, y + 1, state, state_style);
    draw_lane(
        buf,
        area,
        lane_x,
        y,
        lane_width,
        mic,
        mic_rgb,
        &paint,
        lanes.motion,
        seconds,
    );
    y += 2;
    if footer == 1 && y < area.height {
        let (line, style) = match (lanes.muted, warn) {
            (true, true) if area.width < 46 => (
                "you're muted · nobody hears you",
                paint.fg(paint.amber).add_modifier(Modifier::BOLD),
            ),
            (true, true) => (
                "you're muted · nobody hears you · m unmutes",
                paint.fg(paint.amber).add_modifier(Modifier::BOLD),
            ),
            (true, false) if area.width < 46 => {
                ("mic muted · nobody hears you", paint.fg(paint.grey))
            }
            (true, false) => (
                "mic muted · nobody hears you · m unmutes",
                paint.fg(paint.grey),
            ),
            (false, _) => (
                "mic live · everyone hears you · m mutes",
                paint.fg(paint.you),
            ),
        };
        put(buf, area, 1, y, line, style);
    }
}

/// Rows the lanes take for this many other people (header, lanes, footer).
pub const fn height_for(people: usize) -> u16 {
    1 + 2 * (people as u16 + 1) + 1
}

/// Braille bars, bottom-up dots of the left and right columns of a cell.
const LEFT_DOTS: [u32; 4] = [0x40, 0x04, 0x02, 0x01];
const RIGHT_DOTS: [u32; 4] = [0x80, 0x20, 0x10, 0x08];

/// The status line's strip: `🎧 ⣀⣤⣶⣿… Ada · mic muted`. Twelve cells for the
/// last three seconds, each in the colour of whoever was loudest then; the
/// name of who speaks now; your microphone, amber when you talk muted. With
/// motion off, no moving cells: `🎧 Ada speaking · mic muted`.
pub fn strip(lanes: &Lanes<'_>, theme: &Theme) -> Line<'static> {
    strip_cells(lanes, theme, 12)
}

/// The strip from longest to shortest, for a status line that has less
/// room: the moving cells go before the name.
pub fn strips(lanes: &Lanes<'_>, theme: &Theme) -> [Line<'static>; 3] {
    [
        strip_cells(lanes, theme, 12),
        strip_cells(lanes, theme, 6),
        strip_cells(lanes, theme, 0),
    ]
}

fn strip_cells(lanes: &Lanes<'_>, theme: &Theme, cells: usize) -> Line<'static> {
    let paint = Paint::new(theme, lanes.look);
    let mut spans = vec![Span::styled("🎧 ", paint.text)];
    let tracks = lanes
        .people
        .iter()
        .enumerate()
        .filter_map(|(slot, person)| {
            lanes
                .meter
                .peer(person.index)
                .map(|track| (paint.person(slot), track))
        })
        .chain((!lanes.muted).then(|| (paint.you, lanes.meter.mic())))
        .collect::<Vec<_>>();
    if lanes.motion != Motion::Off && cells > 0 {
        // The last 3 s: with 12 cells, 5 columns a cell, 2.5 a braille
        // column; with 6, the last 1.5 s.
        for cell in 0..cells {
            let age = cells - 1 - cell;
            let base = age * 5;
            let mut loudest = (0.0_f32, paint.grey);
            let (mut left, mut right) = (0.0_f32, 0.0_f32);
            for (rgb, track) in &tracks {
                let older = track.columns_max(base + 3, 2);
                let newer = track.columns_max(base, 3);
                left = left.max(older);
                right = right.max(newer);
                if older.max(newer) > loudest.0 {
                    loudest = (older.max(newer), *rgb);
                }
            }
            let height = |v: f32| ((v.powf(1.4) * 4.0).round() as usize).min(4);
            let (l, r) = (height(left), height(right));
            let symbol = match lanes.look.glyphs {
                Glyphs::Unicode => {
                    let bits = LEFT_DOTS
                        .iter()
                        .take(l.max(1))
                        .chain(RIGHT_DOTS.iter().take(r.max(1)))
                        .fold(0, |bits, dot| bits | dot);
                    char::from_u32(0x2800 + bits).unwrap_or(' ')
                }
                Glyphs::Blocks => UP[(l.max(r) * 2).clamp(1, 8)],
            };
            let style = if l.max(r) == 0 {
                paint.fg(paint.grey).add_modifier(Modifier::DIM)
            } else {
                paint.fg(paint.aged(loudest.1, age as f32 / cells as f32))
            };
            spans.push(Span::styled(symbol.to_string(), style));
        }
        spans.push(Span::styled(" ", paint.text));
    }
    let names = speaking_names(lanes);
    let who = match (names.as_slice(), lanes.motion) {
        ([], _) => "quiet".to_owned(),
        ([one], Motion::Off) => format!("{one} speaking"),
        ([one], _) => (*one).to_owned(),
        ([a, b], _) => format!("{a} and {b}"),
        ([a, rest @ ..], _) => format!("{a} +{}", rest.len()),
    };
    let who_style = match names.first() {
        Some(first) => {
            let slot = lanes
                .people
                .iter()
                .position(|person| person.name == *first)
                .unwrap_or(0);
            paint.fg(paint.person(slot)).add_modifier(Modifier::BOLD)
        }
        None => paint.fg(paint.grey),
    };
    spans.push(Span::styled(who, who_style));
    spans.push(Span::styled(" · ", paint.fg(paint.grey)));
    let (mic, mic_style) = if talking_muted(lanes) {
        (
            "you're muted",
            paint
                .fg(paint.amber)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
    } else if lanes.muted {
        ("mic muted", paint.fg(paint.grey))
    } else {
        ("mic live", paint.fg(paint.you))
    };
    spans.push(Span::styled(mic, mic_style));
    Line::from(spans)
}

/// The strip's plain text, for a status line that takes text.
pub fn strip_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// SEAM (agent `look`, plan L3): the status line takes its activity as
/// text. Until it offers a styled slot, the strip is passed as text and its
/// styles are laid over the cells that show it, as far as they match (the
/// line may have been cut short). Returns whether the whole line was there.
pub fn paint_over(buf: &mut Buffer, area: Rect, line: &Line<'_>) -> bool {
    let Some(first) = line
        .spans
        .first()
        .and_then(|span| span.content.split_whitespace().next())
    else {
        return false;
    };
    let y = area.y;
    let Some(start) = (area.x..area.right()).find(|&x| buf[(x, y)].symbol() == first) else {
        return false;
    };
    let mut x = start;
    for span in &line.spans {
        for grapheme in
            unicode_segmentation::UnicodeSegmentation::graphemes(span.content.as_ref(), true)
        {
            if x >= area.right() || buf[(x, y)].symbol() != grapheme {
                return false;
            }
            let cell = &mut buf[(x, y)];
            let style = cell.style().patch(span.style);
            cell.set_style(style);
            x += (grapheme.width() as u16).max(1);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huddle::{levels::Levels, synth};

    /// The research passage, generated: Ada, Lin and Sam, and you muted.
    /// Returns the meter at `at` seconds, driven at 30 frames a second.
    fn passage(at: f64) -> Meter {
        let mut source = synth::Synthetic::new(&synth::DEMO_SCRIPT, None);
        let levels: std::sync::Arc<Levels> = source.levels();
        let mut meter = Meter::default();
        let mut t = 0.0;
        while t <= at + 1e-9 {
            source.advance_to(t);
            meter.update(levels.as_ref(), &[1, 2, 3], t);
            t += 1.0 / 30.0;
        }
        meter
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

    fn draw(
        meter: &Meter,
        width: u16,
        height: u16,
        look: Look,
        theme: &str,
        motion: Motion,
    ) -> Buffer {
        let theme = Theme::builtin(theme).unwrap();
        let people = people();
        let lanes = Lanes {
            meter,
            people: &people,
            muted: true,
            motion,
            look,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        let area = buf.area;
        render(&mut buf, area, &lanes, &theme);
        buf
    }

    fn text(buf: &Buffer) -> String {
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

    /// Text and style of every cell, as a golden.
    fn golden(buf: &Buffer) -> String {
        let mut out = text(buf);
        out.push_str("\n--- styles ---\n");
        // Runs of cells that share a style: `from-to fg/bg/modifier`.
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
            .join("tests/fixtures/huddle_lanes")
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

    /// Goldens at 42×12 and 80×24 in the default theme, a light theme, 256
    /// colours, block glyphs only and `NO_COLOR`, at three moments of the
    /// generated passage: Ada and Lin at once (5.5 s), you talking muted
    /// (7.1 s) and Sam (8.5 s).
    #[test]
    fn lanes_goldens() {
        for (moment, at) in [("both", 5.5), ("muted-talk", 7.1), ("sam", 8.5)] {
            let meter = passage(at);
            for (size, (w, h)) in [("42x12", (42, 12)), ("80x24", (80, 24))] {
                for (look_name, look, theme) in [
                    ("bzz", TRUE, "bzz"),
                    ("light", TRUE, "light"),
                    ("ansi256", C256, "bzz"),
                    ("blocks", BLOCKS, "bzz"),
                    ("no-color", NONE, "bzz"),
                ] {
                    let buf = draw(&meter, w, h, look, theme, Motion::Full);
                    check_golden(&format!("{moment}-{size}-{look_name}"), &golden(&buf));
                }
            }
        }
    }

    #[test]
    fn two_at_once_both_lanes_move_and_the_header_says_so() {
        let meter = passage(5.5);
        let buf = draw(&meter, 42, 12, TRUE, "bzz", Motion::Full);
        let screen = text(&buf);
        assert!(
            screen.contains("4 in the call · Ada and Lin at once"),
            "{screen}"
        );
        let rows = screen.lines().collect::<Vec<_>>();
        // Ada's and Lin's lanes reach the right edge (now); Sam's does not.
        let edge = |row: usize| rows[row].chars().last().unwrap_or(' ');
        assert!(
            rows[1].contains("Ada") && rows[2].contains("● live"),
            "{screen}"
        );
        assert!(
            rows[3].contains("Lin") && rows[4].contains("● live"),
            "{screen}"
        );
        assert!(UP[2..].contains(&edge(1)), "{screen}");
        assert!(UP[2..].contains(&edge(3)), "{screen}");
        assert!(
            rows[6].contains("3s ago") || rows[6].contains("quiet"),
            "{screen}"
        );
        // Names of speakers are bold; Sam's is not.
        let bold = |row: u16, x: u16| buf[(x, row)].modifier.contains(Modifier::BOLD);
        assert!(bold(1, 5) && bold(3, 5) && !bold(5, 5));
    }

    #[test]
    fn talking_muted_says_so_in_words_and_in_amber() {
        let meter = passage(7.1);
        for look in [TRUE, NONE] {
            let buf = draw(&meter, 42, 12, look, "bzz", Motion::Full);
            let screen = text(&buf);
            assert!(screen.contains("muted!"), "{screen}");
            assert!(
                screen.contains("you're muted · nobody hears you"),
                "{screen}"
            );
            // The microphone lane moves although nothing is sent.
            let mic_row = screen.lines().position(|row| row.contains("You")).unwrap();
            let lane = screen.lines().nth(mic_row).unwrap();
            assert!(
                lane.chars().rev().take(4).any(|c| UP[2..].contains(&c)),
                "{screen}"
            );
        }
        let buf = draw(&meter, 42, 12, TRUE, "bzz", Motion::Full);
        let warning = Theme::builtin("bzz")
            .unwrap()
            .style(HighlightGroup::Warning)
            .fg
            .and_then(named_rgb)
            .unwrap();
        let screen = text(&buf);
        let row = screen
            .lines()
            .position(|row| row.contains("muted!"))
            .unwrap() as u16;
        let x = screen
            .lines()
            .nth(row.into())
            .unwrap()
            .find("muted!")
            .unwrap() as u16;
        assert_eq!(
            buf[(x, row)].fg,
            Color::Rgb(warning.0, warning.1, warning.2)
        );
    }

    #[test]
    fn muted_and_quiet_says_nobody_hears_you() {
        let meter = passage(4.0);
        let screen = text(&draw(&meter, 42, 12, TRUE, "bzz", Motion::Full));
        assert!(screen.contains("mic muted · nobody hears you"), "{screen}");
        assert!(screen.contains("Lin is speaking"), "{screen}");
    }

    #[test]
    fn no_color_draws_no_colour() {
        let meter = passage(5.5);
        for (w, h) in [(42, 12), (80, 24)] {
            let buf = draw(&meter, w, h, NONE, "bzz", Motion::Full);
            for cell in buf.content() {
                assert_eq!(cell.fg, Color::Reset);
                assert_eq!(cell.bg, Color::Reset);
            }
        }
    }

    #[test]
    fn colours_are_256_or_none_when_asked() {
        let meter = passage(5.5);
        let buf = draw(&meter, 80, 24, C256, "bzz", Motion::Full);
        for cell in buf.content() {
            for color in [cell.fg, cell.bg] {
                assert!(!matches!(color, Color::Rgb(..)), "{color:?}");
            }
        }
        assert_eq!(to_ansi256((0, 0, 0)), 16);
        assert_eq!(to_ansi256((255, 255, 255)), 231);
        assert_eq!(to_ansi256((0x80, 0x80, 0x80)), 244);
    }

    #[test]
    fn blocks_only_uses_block_elements_and_words() {
        let meter = passage(5.5);
        let theme = Theme::builtin("bzz").unwrap();
        let people = people();
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: BLOCKS,
        };
        let buf = draw(&meter, 42, 12, BLOCKS, "bzz", Motion::Full);
        let strip = strip_text(&strip(&lanes, &theme));
        for c in text(&buf).chars().chain(strip.chars()) {
            assert!(
                c.is_ascii() || ('\u{2580}'..='\u{259F}').contains(&c) || "●·…🎧!'".contains(c),
                "{c:?} in blocks-only"
            );
        }
    }

    #[test]
    fn many_people_keep_the_last_speakers_and_you() {
        let meter = passage(8.5);
        let theme = Theme::builtin("bzz").unwrap();
        let mut people = people();
        for i in 4..9 {
            people.push(Person {
                index: i,
                name: format!("P{i}"),
            });
        }
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: TRUE,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 42, 12));
        let area = buf.area;
        render(&mut buf, area, &lanes, &theme);
        let screen = text(&buf);
        assert!(screen.contains("+4 more"), "{screen}");
        assert!(screen.contains("Sam") && screen.contains("You"), "{screen}");
    }

    #[test]
    fn motion_off_is_a_still_meter() {
        let meter = passage(5.5);
        let a = text(&draw(&meter, 42, 12, TRUE, "bzz", Motion::Off));
        let later = passage(5.7);
        let b = text(&draw(&later, 42, 12, TRUE, "bzz", Motion::Off));
        assert_eq!(a, b, "nothing moves while the same people speak");
        assert!(a.contains("Ada and Lin at once"));
        assert!(!a.contains("s ago"), "{a}");
    }

    fn status_row(lanes: &Lanes<'_>, theme: &Theme) -> Buffer {
        let line = strip(lanes, theme);
        let text = strip_text(&line);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::status::render(
                    frame,
                    frame.area(),
                    theme,
                    crate::ui::status::StatusBarView {
                        mode: "NORMAL",
                        mode_group: HighlightGroup::StatusMode,
                        activity: Some(&text),
                        notice: "",
                        connection: "online",
                        media: "sixel",
                        app_focused: false,
                    },
                );
                let area = frame.area();
                paint_over(frame.buffer_mut(), area, &line);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The strip in an 80×1 status line: who speaks, and "you're muted" in
    /// amber when you talk muted.
    #[test]
    fn strip_goldens() {
        let theme = Theme::builtin("bzz").unwrap();
        let people = people();
        for (moment, at) in [
            ("ada", 1.5),
            ("both", 5.5),
            ("muted-talk", 7.1),
            ("quiet", 2.9),
        ] {
            let meter = passage(at);
            for (look_name, look) in [("bzz", TRUE), ("blocks", BLOCKS), ("no-color", NONE)] {
                let lanes = Lanes {
                    meter: &meter,
                    people: &people,
                    muted: true,
                    motion: Motion::Full,
                    look,
                };
                let buf = status_row(&lanes, &theme);
                check_golden(&format!("strip-{moment}-80x1-{look_name}"), &golden(&buf));
            }
        }
        let meter = passage(7.1);
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: TRUE,
        };
        let buf = status_row(&lanes, &theme);
        let row = text(&buf);
        assert!(row.contains("🎧") && row.contains("you're muted"), "{row}");
        let x = (0..80)
            .find(|&x| buf[(x, 0)].symbol() == "y" && buf[(x + 1, 0)].symbol() == "o")
            .unwrap();
        let amber = theme
            .style(HighlightGroup::Warning)
            .fg
            .and_then(named_rgb)
            .unwrap();
        assert_eq!(buf[(x, 0)].fg, Color::Rgb(amber.0, amber.1, amber.2));
        let meter = passage(1.5);
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: TRUE,
        };
        let row = text(&status_row(&lanes, &theme));
        assert!(row.contains("Ada · mic muted"), "{row}");
    }

    #[test]
    fn look_reads_the_environment_it_is_given() {
        let look = Look::from_vars(Some("1"), Some("truecolor"), Some("xterm"), None);
        assert_eq!(look.depth, Depth::None);
        let look = Look::from_vars(None, Some("truecolor"), Some("xterm"), None);
        assert_eq!(look, TRUE);
        let look = Look::from_vars(None, None, Some("linux"), None);
        assert_eq!(look, BLOCKS);
    }

    #[test]
    fn debug_prints_no_number() {
        let meter = passage(1.0);
        let people = people();
        let lanes = Lanes {
            meter: &meter,
            people: &people,
            muted: true,
            motion: Motion::Full,
            look: TRUE,
        };
        for debug in [format!("{lanes:?}"), format!("{:?}", people[0])] {
            assert!(!debug.chars().any(|c| c.is_ascii_digit()), "{debug}");
        }
    }
}
