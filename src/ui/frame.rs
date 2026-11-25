//! How a pane, a modal and the whole frame look: the one function that draws
//! a pane's frame, the key hints of a footer, the backdrop behind an overlay,
//! and the last pass of a frame that takes colour away where the terminal or
//! the person asks for less of it (`NO_COLOR`, no true colour).
//!
//! Every pane and overlay draws its frame through [`block`], so focus looks
//! the same everywhere: an unfocused frame is quiet, in the theme's muted
//! colour, and the focused one is in its primary colour with its title as a
//! label. Under `NO_COLOR` the focused frame is heavy and the label reversed,
//! so focus never depends on colour (plan rule 21).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

use crate::{
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// How much colour the terminal shows, decided once at start.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorMode {
    /// 24-bit colour: what the theme says is drawn.
    #[default]
    True,
    /// The 256-colour palette: every RGB value goes to the nearest entry.
    Ansi256,
    /// Sixteen colours: every RGB or indexed value goes to the nearest one.
    Ansi16,
    /// `NO_COLOR`: no colour at all; bold, dim, reverse and underline stay.
    None,
}

impl ColorMode {
    /// The mode of this process, read from the environment once.
    pub fn detected() -> Self {
        static MODE: std::sync::OnceLock<ColorMode> = std::sync::OnceLock::new();
        *MODE.get_or_init(Self::from_env)
    }

    /// Reads `NO_COLOR` (any non-empty value), `COLORTERM` and `TERM`.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        Self::from_values(
            var("NO_COLOR").as_deref(),
            var("COLORTERM").as_deref(),
            var("TERM").as_deref(),
            cfg!(windows),
        )
    }

    /// The decision, apart from the process environment so tests can make it.
    pub fn from_values(
        no_color: Option<&str>,
        colorterm: Option<&str>,
        term: Option<&str>,
        windows: bool,
    ) -> Self {
        if no_color.is_some_and(|value| !value.is_empty()) {
            return Self::None;
        }
        let colorterm = colorterm.unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            return Self::True;
        }
        let term = term.unwrap_or_default().to_ascii_lowercase();
        if term == "dumb" {
            return Self::None;
        }
        if term.contains("direct") || term.contains("truecolor") {
            return Self::True;
        }
        // Windows terminals rarely set either variable and draw true colour.
        if term.is_empty() && windows {
            return Self::True;
        }
        if term.contains("256") {
            return Self::Ansi256;
        }
        if term.is_empty() {
            // Nothing is known: keep what the theme says.
            return Self::True;
        }
        Self::Ansi16
    }
}

/// The frame of a pane or an overlay, with its title.
///
/// `focused` frames are in the primary colour with the title as a label;
/// the others are muted. A modal or picker is always the focus. Under
/// `NO_COLOR` the focused frame is heavy.
pub fn block<'a>(theme: &Theme, surface: BorderSurface, title: &str, focused: bool) -> Block<'a> {
    let modal = matches!(surface, BorderSurface::Modal | BorderSurface::Picker);
    let focused = focused || modal;
    let border_group = match (modal, focused, surface) {
        (true, _, _) => HighlightGroup::ModalBorder,
        (false, true, BorderSurface::Composer) => HighlightGroup::ActiveComposerBorder,
        (false, false, BorderSurface::Composer) => HighlightGroup::ComposerBorder,
        (false, true, _) => HighlightGroup::FocusedPaneBorder,
        (false, false, _) => HighlightGroup::PaneBorder,
    };
    let border_type = if focused && theme.color_mode() == ColorMode::None {
        BorderType::Thick
    } else {
        theme.border_type(surface)
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(theme.style(border_group));
    let title = sanitize::single_line(title);
    let title = title.trim();
    if !title.is_empty() {
        block = block.title(title_label(theme, title, focused));
    }
    block
}

/// The frame of an overlay: a modal or picker, always the focus, with its
/// keys at its foot.
pub fn modal<'a>(
    theme: &Theme,
    surface: BorderSurface,
    title: &str,
    keys: &[(&str, &str)],
) -> Block<'a> {
    let block = block(theme, surface, title, true);
    if keys.is_empty() {
        block
    } else {
        block.title_bottom(hints(theme, keys))
    }
}

/// A pane's title: a label when the pane has the focus, quiet otherwise.
pub fn title_label(theme: &Theme, title: &str, focused: bool) -> Line<'static> {
    let style = if focused {
        theme.style(HighlightGroup::FocusedPaneTitle)
    } else {
        theme.style(HighlightGroup::PaneTitle)
    };
    Line::from(Span::styled(format!(" {title} "), style))
}

/// Quiet state shown at the right of a frame's top edge (a sort order, a
/// count): what the pane is, never what to press.
pub fn info(theme: &Theme, text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {} ", sanitize::single_line(text)),
        theme.style(HighlightGroup::PaneTitle),
    ))
    .right_aligned()
}

/// The keys of a footer: each key in the hint colour, what it does muted.
pub fn hints(theme: &Theme, pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", theme.style(HighlightGroup::Muted)));
        }
        spans.push(Span::styled(
            (*key).to_owned(),
            theme.style(HighlightGroup::HintKey),
        ));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                theme.style(HighlightGroup::HintText),
            ));
        }
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// The keys of a footer written as one sentence, `key what · key what`: the
/// first word of each part is the key.
pub fn hint_line(theme: &Theme, text: &str) -> Line<'static> {
    let text = sanitize::single_line(text);
    let pairs = text
        .split(" · ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.split_once(' ').unwrap_or((part, "")))
        .collect::<Vec<_>>();
    hints(theme, &pairs)
}

/// Dims every cell of `area`: the screen behind an overlay. The colour moves
/// to the muted one and the DIM attribute is added, so the effect stays
/// without colour. Drawn before the overlay, which clears its own cells.
pub fn dim(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let muted = theme
        .style(HighlightGroup::Muted)
        .fg
        .unwrap_or(Color::DarkGray);
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.diff_option == ratatui::buffer::CellDiffOption::Skip {
                    // An image protocol owns this cell.
                    continue;
                }
                cell.fg = muted;
                if let Color::Rgb(red, green, blue) = cell.bg {
                    cell.bg = Color::Rgb(red / 2, green / 2, blue / 2);
                }
                cell.modifier.insert(Modifier::DIM);
                cell.modifier
                    .remove(Modifier::BOLD | Modifier::REVERSED | Modifier::UNDERLINED);
            }
        }
    }
}

/// The last pass of a frame: removes or reduces colour for the terminal.
pub fn finish(buf: &mut Buffer, mode: ColorMode) {
    match mode {
        ColorMode::True => {}
        ColorMode::Ansi256 => {
            for cell in &mut buf.content {
                cell.fg = to_256(cell.fg);
                cell.bg = to_256(cell.bg);
            }
        }
        ColorMode::Ansi16 => {
            for cell in &mut buf.content {
                cell.fg = to_16(cell.fg);
                cell.bg = to_16(cell.bg);
            }
        }
        ColorMode::None => {
            for cell in &mut buf.content {
                cell.fg = Color::Reset;
                cell.bg = Color::Reset;
                cell.underline_color = Color::Reset;
            }
        }
    }
}

/// The style of a plain style under a colour mode, for code that writes to
/// the terminal outside a frame.
pub fn reduce(style: Style, mode: ColorMode) -> Style {
    let map = |color: Option<Color>| {
        color.map(|color| match mode {
            ColorMode::True => color,
            ColorMode::Ansi256 => to_256(color),
            ColorMode::Ansi16 => to_16(color),
            ColorMode::None => Color::Reset,
        })
    };
    Style {
        fg: map(style.fg),
        bg: map(style.bg),
        ..style
    }
}

const ANSI16: [(Color, (i32, i32, i32)); 16] = [
    (Color::Black, (0, 0, 0)),
    (Color::Red, (205, 0, 0)),
    (Color::Green, (0, 205, 0)),
    (Color::Yellow, (205, 205, 0)),
    (Color::Blue, (0, 0, 238)),
    (Color::Magenta, (205, 0, 205)),
    (Color::Cyan, (0, 205, 205)),
    (Color::Gray, (229, 229, 229)),
    (Color::DarkGray, (127, 127, 127)),
    (Color::LightRed, (255, 0, 0)),
    (Color::LightGreen, (0, 255, 0)),
    (Color::LightYellow, (255, 255, 0)),
    (Color::LightBlue, (92, 92, 255)),
    (Color::LightMagenta, (255, 0, 255)),
    (Color::LightCyan, (0, 255, 255)),
    (Color::White, (255, 255, 255)),
];

fn distance((r1, g1, b1): (i32, i32, i32), (r2, g2, b2): (i32, i32, i32)) -> i32 {
    (r1 - r2).pow(2) + (g1 - g2).pow(2) + (b1 - b2).pow(2)
}

fn to_16(color: Color) -> Color {
    let rgb = match color {
        Color::Rgb(red, green, blue) => (i32::from(red), i32::from(green), i32::from(blue)),
        Color::Indexed(index) => indexed_rgb(index),
        other => return other,
    };
    ANSI16
        .iter()
        .min_by_key(|(_, reference)| distance(*reference, rgb))
        .map_or(Color::Reset, |(color, _)| *color)
}

fn to_256(color: Color) -> Color {
    let Color::Rgb(red, green, blue) = color else {
        return color;
    };
    let rgb = (i32::from(red), i32::from(green), i32::from(blue));
    // The 6×6×6 cube and the grey ramp; the nearer of the two wins.
    let level = |value: i32| -> i32 {
        if value < 48 {
            0
        } else if value < 115 {
            1
        } else {
            (value - 35) / 40
        }
    };
    let (cr, cg, cb) = (level(rgb.0), level(rgb.1), level(rgb.2));
    let cube_index = 16 + 36 * cr + 6 * cg + cb;
    let cube = indexed_rgb(u8::try_from(cube_index).unwrap_or(16));
    let average = (rgb.0 + rgb.1 + rgb.2) / 3;
    let grey_step = ((average - 8) / 10).clamp(0, 23);
    let grey_index = 232 + grey_step;
    let grey = indexed_rgb(u8::try_from(grey_index).unwrap_or(232));
    let index = if distance(grey, rgb) < distance(cube, rgb) {
        grey_index
    } else {
        cube_index
    };
    Color::Indexed(u8::try_from(index).unwrap_or(16))
}

fn indexed_rgb(index: u8) -> (i32, i32, i32) {
    match index {
        0..=15 => ANSI16[usize::from(index)].1,
        16..=231 => {
            let value = i32::from(index) - 16;
            let step = |level: i32| if level == 0 { 0 } else { 55 + level * 40 };
            (step(value / 36), step((value / 6) % 6), step(value % 6))
        }
        _ => {
            let grey = 8 + (i32::from(index) - 232) * 10;
            (grey, grey, grey)
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier},
        widgets::Paragraph,
    };

    use super::{ColorMode, block, dim, finish, hints, to_16, to_256};
    use crate::ui::theme::{BorderSurface, HighlightGroup, Theme};

    fn draw(theme: &Theme, focused: bool) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("hi").block(block(
                        theme,
                        BorderSurface::Pane,
                        "design",
                        focused,
                    )),
                    frame.area(),
                );
                finish(frame.buffer_mut(), theme.color_mode());
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn colour_mode_follows_no_color_colorterm_and_term() {
        let mode = ColorMode::from_values;
        assert_eq!(
            mode(Some("1"), Some("truecolor"), Some("xterm"), false),
            ColorMode::None
        );
        assert_eq!(
            mode(Some(""), Some("truecolor"), None, false),
            ColorMode::True
        );
        assert_eq!(
            mode(None, Some("24bit"), Some("xterm"), false),
            ColorMode::True
        );
        assert_eq!(
            mode(None, None, Some("xterm-256color"), false),
            ColorMode::Ansi256
        );
        assert_eq!(mode(None, None, Some("xterm"), false), ColorMode::Ansi16);
        assert_eq!(mode(None, None, Some("linux"), false), ColorMode::Ansi16);
        assert_eq!(mode(None, None, Some("dumb"), false), ColorMode::None);
        assert_eq!(mode(None, None, None, true), ColorMode::True);
        assert_eq!(
            mode(None, None, Some("xterm-direct"), false),
            ColorMode::True
        );
    }

    #[test]
    fn a_focused_pane_is_in_the_primary_colour_with_its_title_as_a_label() {
        let theme = Theme::default();
        let buffer = draw(&theme, true);
        assert!(row(&buffer, 0).starts_with("┌ design ─"));
        let corner = &buffer[(0, 0)];
        assert_eq!(
            Some(corner.fg),
            theme.style(HighlightGroup::FocusBorder).fg,
            "the focused frame is in the primary colour"
        );
        assert_eq!(corner.fg, Color::Cyan);
        let label = &buffer[(2, 0)];
        assert!(label.modifier.contains(Modifier::REVERSED), "{label:?}");
    }

    #[test]
    fn an_unfocused_pane_is_quiet() {
        let theme = Theme::default();
        let buffer = draw(&theme, false);
        assert!(row(&buffer, 0).starts_with("┌ design ─"));
        assert_eq!(buffer[(0, 0)].fg, Color::DarkGray);
        assert_eq!(buffer[(2, 0)].fg, Color::DarkGray);
        assert!(!buffer[(2, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn every_builtin_theme_keeps_focus_apart_from_the_quiet_frames() {
        for entry in crate::ui::theme::ThemeRegistry::entries() {
            let theme = Theme::builtin(entry.id).unwrap();
            assert_ne!(
                theme.style(HighlightGroup::FocusedPaneBorder),
                theme.style(HighlightGroup::PaneBorder),
                "{}",
                entry.id
            );
            assert!(
                theme
                    .style(HighlightGroup::FocusedPaneTitle)
                    .add_modifier
                    .contains(Modifier::REVERSED),
                "{}",
                entry.id
            );
        }
    }

    #[test]
    fn under_no_color_no_cell_has_a_colour_and_focus_is_a_heavy_frame() {
        let theme = Theme::default().with_color_mode(ColorMode::None);
        let focused = draw(&theme, true);
        for cell in &focused.content {
            assert_eq!(cell.fg, Color::Reset);
            assert_eq!(cell.bg, Color::Reset);
        }
        assert!(
            row(&focused, 0).starts_with("┏ design ━"),
            "{}",
            row(&focused, 0)
        );
        assert!(focused[(2, 0)].modifier.contains(Modifier::REVERSED));
        let quiet = draw(&theme, false);
        assert!(
            row(&quiet, 0).starts_with("┌ design ─"),
            "{}",
            row(&quiet, 0)
        );
        assert!(!quiet[(2, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn without_true_colour_rgb_values_are_reduced() {
        assert_eq!(to_16(Color::Rgb(0x26, 0x26, 0x30)), Color::Black);
        assert_eq!(to_16(Color::Rgb(0x4A, 0x9E, 0xFF)), Color::LightBlue);
        assert_eq!(to_16(Color::Cyan), Color::Cyan);
        assert_eq!(to_256(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(to_256(Color::Rgb(255, 255, 255)), Color::Indexed(231));
        assert_eq!(to_256(Color::Rgb(0x26, 0x26, 0x30)), Color::Indexed(235));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer[(0, 0)].set_fg(Color::Rgb(0xE0, 0x40, 0x40));
        finish(&mut buffer, ColorMode::Ansi256);
        assert!(matches!(buffer[(0, 0)].fg, Color::Indexed(_)));
        finish(&mut buffer, ColorMode::Ansi16);
        assert!(!matches!(
            buffer[(0, 0)].fg,
            Color::Indexed(_) | Color::Rgb(..)
        ));
    }

    #[test]
    fn hints_colour_the_keys_and_mute_what_they_do() {
        let theme = Theme::default();
        let line = hints(&theme, &[("Enter", "open"), ("Esc", "close")]);
        let text = line.to_string();
        assert_eq!(text, " Enter open · Esc close ");
        let key = line
            .spans
            .iter()
            .find(|span| span.content == "Enter")
            .unwrap();
        assert_eq!(key.style, theme.style(HighlightGroup::HintKey));
        assert_ne!(
            theme.style(HighlightGroup::HintKey).fg,
            theme.style(HighlightGroup::HintText).fg
        );
    }

    #[test]
    fn the_backdrop_dims_every_cell_even_without_colour() {
        let theme = Theme::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 2));
        buffer[(1, 1)]
            .set_fg(Color::Cyan)
            .set_bg(Color::Rgb(100, 100, 100));
        let area = buffer.area;
        dim(&mut buffer, area, &theme);
        finish(&mut buffer, ColorMode::None);
        for cell in &buffer.content {
            assert!(cell.modifier.contains(Modifier::DIM));
        }
    }
}
