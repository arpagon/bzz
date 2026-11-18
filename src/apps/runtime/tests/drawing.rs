//! Drawing: every element of the contract, inside the area it is given,
//! in the colours of the user's theme, and in a time that the limits of a
//! screen bound.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use super::*;
use crate::apps::runtime::draw::text_of;
use crate::apps::runtime::{AppTheme, Event, Options, Runtime, Signal};
use crate::ui::theme::Theme;

/// Every element of the contract on one screen, each with a word to find.
const GALLERY: &str = r##"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { page = 1, field = { value = "typed", cursor = 2 } } end
function app.key(s, key)
  if key.name == "tab" then s.page = s.page % 3 + 1 else ui.edit(s.field, key) end
end
function app.view(s, ctx)
  local page1 = ui.column {
    ui.tabs { titles = { "One", "Two", ui.span { "Three", fg = "accent" } }, selected = s.page, highlight = "selection", divider = "/" },
    ui.row {
      size = 5,
      ui.block {
        title = "Block", title_bottom = ui.line { "bottom", align = "right" }, border_type = "rounded",
        border_style = "border", padding = { 1, 0 },
        ui.text { "plain", ui.line { "a ", ui.span { "span", bold = true, fg = "error" } }, align = "left" },
      },
      ui.list { items = { "first", "second", ui.line { "third" } }, selected = 2, highlight_symbol = "* ", highlight = { reversed = true } },
    },
    ui.table {
      size = 4,
      header = { "Name", "Qty" }, rows = { { "apples", 3 }, { "pears", ui.span { "12", fg = "warning" } } },
      footer = { "total", "15" }, widths = { "fill", 5 }, selected = 2, header_style = "title", highlight = "selection",
    },
    ui.gauge { size = 1, ratio = 0.5, label = "half", gauge_style = { fg = "success" } },
    ui.line_gauge { percent = 25, label = "quarter", filled_style = { fg = "accent" } },
    ui.sparkline { size = 2, data = { 1, 4, 2, 8, 5 }, max = 8, fg = "muted" },
    ui.bar_chart { size = 4, bars = { { label = "a", value = 3 }, { label = "b", value = 7, text = "seven" } }, bar_width = 5 },
    ui.row { size = 1, ui.text { "Field: ", size = 7 }, ui.input { value = s.field.value, cursor = s.field.cursor, focused = true } },
    ui.row { ui.spacer { size = 3 }, ui.text { "after the spacer" }, ui.scrollbar { size = 1, length = 30, position = 10, viewport = 5 } },
  }
  if s.page == 1 then return page1 end
  return ui.layers {
    page1,
    ui.popup { title = " Popup ", width = "50%", height = 5, border_type = "double", ui.text { "in the popup", fg = "success" } },
  }
end
return app
"##;

fn gallery(size: (u16, u16)) -> Headless {
    Headless::start(
        GALLERY,
        HeadlessOptions::new("gallery.lua")
            .size(size.0, size.1)
            .limits(Limits::patient()),
    )
    .expect("starts")
}

#[test]
fn every_element_of_the_contract_draws() {
    let mut app = gallery((60, 26));
    let screen = app.screen();
    assert_eq!(app.error(), None, "{screen}");
    for drawn in [
        " One / Two / Three",  // tabs
        "╭Block",              // block, rounded
        "bottom╯",             // its bottom title, to the right
        "│ plain",             // text, inside the padding
        "a span",              // line and span
        "* second",            // list, with the symbol on the selected item
        "  first",             // and the room for it on the others
        "  Name",              // table header
        "> pears",             // table selection
        "  total",             // table footer
        "half",                // gauge label
        "quarter",             // line gauge
        "seven",               // bar chart, a bar's own text
        "Field: typed",        // input
        "   after the spacer", // spacer
    ] {
        assert!(screen.contains(drawn), "no {drawn:?} in\n{screen}");
    }
    // The sparkline and the scrollbar draw with block characters.
    assert!(screen.contains('█') && screen.contains('▄'), "{screen}");
    // A focused input says where the cursor goes: after "Field: ty".
    let row = screen
        .lines()
        .position(|l| l.contains("Field: typed"))
        .expect("the field") as u16;
    assert_eq!(app.cursor(), Some(Position::new(9, row)));

    // A popup is drawn over what is under it, in the middle.
    app.key("tab");
    let screen = app.screen();
    assert!(
        screen.contains("╔ Popup ") && screen.contains("║in the popup"),
        "{screen}"
    );
    let top = screen
        .lines()
        .position(|l| l.contains("╔ Popup "))
        .expect("the popup");
    assert_eq!(top, (26 - 5) / 2 + 1, "{screen}");
    let left = screen
        .lines()
        .nth(top)
        .and_then(|l| l.find('╔'))
        .map(|byte| {
            screen.lines().nth(top).unwrap_or("")[..byte]
                .chars()
                .count()
        });
    assert_eq!(left, Some((60 - 30) / 2), "{screen}");
}

#[test]
fn nothing_is_drawn_outside_the_area() {
    // The buffer is bzz's whole window; the app has one region of it.
    let window = Rect::new(0, 0, 70, 34);
    let untouched = Style::new().fg(Color::Indexed(201)).bg(Color::Indexed(17));
    let theme = AppTheme::from(&Theme::builtin("dracula").expect("a theme"));
    for script in [GALLERY, BOARD] {
        let signal = Signal::new();
        let mut runtime = Runtime::start(
            script,
            Options::new("app.lua", ana())
                .limits(Limits::patient())
                .waker(signal.waker()),
        )
        .expect("starts");
        let settle = |runtime: &mut Runtime| {
            while !runtime.is_idle() {
                signal.wait(Duration::from_secs(20));
            }
            runtime.poll();
        };
        settle(&mut runtime);
        let mut areas = vec![
            Rect::new(0, 0, 70, 34),
            Rect::new(28, 1, 42, 30),
            Rect::new(5, 5, 40, 16),
            Rect::new(69, 33, 1, 1),
            Rect::new(10, 10, 1, 20),
            Rect::new(10, 10, 50, 1),
            Rect::new(3, 3, 0, 10),
            Rect::new(3, 3, 10, 0),
            // Areas that reach past the window: cut to it, not drawn past it.
            Rect::new(60, 30, 40, 16),
            Rect::new(200, 200, 40, 16),
        ];
        // And every small size, where an element has less room than it
        // wants.
        for w in 0..12 {
            for h in 0..8 {
                areas.push(Rect::new(20, 12, w, h));
            }
        }
        for page in 0..3 {
            for area in &areas {
                runtime.send(Event::Resize {
                    width: area.width,
                    height: area.height,
                });
                settle(&mut runtime);
                let mut buffer = Buffer::filled(window, ratatui::buffer::Cell::new("#"));
                buffer.set_style(window, untouched);
                let cursor = runtime.draw(*area, &mut buffer, &theme);
                let inside = area.intersection(window);
                for y in window.top()..window.bottom() {
                    for x in window.left()..window.right() {
                        if !inside.contains(Position::new(x, y)) {
                            let cell = &buffer[(x, y)];
                            assert!(
                                cell.symbol() == "#"
                                    && Some(cell.fg) == untouched.fg
                                    && Some(cell.bg) == untouched.bg,
                                "page {page}, area {area:?}: the cell at {x},{y} was drawn on"
                            );
                        }
                    }
                }
                if let Some(cursor) = cursor {
                    assert!(
                        inside.contains(cursor),
                        "the cursor {cursor:?} is outside {area:?}"
                    );
                }
            }
            runtime.send(Event::Key(crate::apps::runtime::Key::named("tab")));
            runtime.send(Event::Key(crate::apps::runtime::Key::named("n")));
        }
    }
}

#[test]
fn an_app_follows_the_theme() {
    let source = r##"
        local app = { api = 1, types = { "note" } }
        function app.view(s, ctx)
          return ui.column {
            ui.text { "accent", fg = "accent" },
            ui.text { "muted", fg = "muted" },
            ui.text { "error", fg = "error" },
            ui.text { "warning", fg = "warning" },
            ui.text { "success", fg = "success" },
            ui.text { "border", fg = "border" },
            ui.text { "title", fg = "title" },
            ui.text { "onbg", bg = "error" },
            ui.text { "fixed", fg = "red", bg = "#102030" },
            ui.text { "index", fg = 42 },
            ui.list { size = 1, items = { "picked" }, selected = 1, highlight = "selection", highlight_symbol = "" },
            ui.text { "plain" },
          }
        end
        return app
    "##;
    let mut app = start("theme.lua", source);
    assert_eq!(app.error(), None);
    let fixed = (Some(Color::Red), Some(Color::Rgb(0x10, 0x20, 0x30)));
    for name in ["bzz", "dracula", "light"] {
        let from = Theme::builtin(name).expect("a theme");
        let theme = AppTheme::from(&from);
        let area = Rect::new(0, 0, 20, 12);
        let mut buffer = Buffer::empty(area);
        app.runtime_mut().draw(area, &mut buffer, &theme);
        let at = |row: u16| {
            let cell = &buffer[(0, row)];
            (cell.fg, cell.bg)
        };
        let color = |style: Style| style.fg.unwrap_or(Color::Reset);
        let background = theme.normal.bg.unwrap_or(Color::Reset);
        // A name of meaning is that style of the theme, whatever it is.
        assert_eq!(at(0).0, color(theme.accent), "{name}");
        assert_eq!(at(1).0, color(theme.muted), "{name}");
        assert_eq!(at(2).0, color(theme.error), "{name}");
        assert_eq!(at(3).0, color(theme.warning), "{name}");
        assert_eq!(at(4).0, color(theme.success), "{name}");
        assert_eq!(
            at(5).0,
            theme.border.fg.or(theme.normal.fg).unwrap_or(Color::Reset),
            "{name}"
        );
        // As a background, a style without one gives its own colour.
        assert_eq!(at(7).1, color(theme.error), "{name}");
        // A fixed colour is the same under every theme.
        assert_eq!((Some(at(8).0), Some(at(8).1)), fixed, "{name}");
        assert_eq!(at(9).0, Color::Indexed(42), "{name}");
        // A whole style of the theme: a selected row.
        let picked = &buffer[(0, 10)];
        assert_eq!(
            picked.bg,
            theme.selection.bg.unwrap_or(background),
            "{name}"
        );
        // What an app does not colour has the theme's own text and ground.
        let plain = &buffer[(0, 11)];
        assert_eq!(
            (plain.fg, plain.bg),
            (theme.normal.fg.unwrap_or(Color::Reset), background),
            "{name}"
        );
        // An empty cell of the region too.
        assert_eq!(buffer[(19, 11)].bg, background, "{name}");
    }
    // The themes do differ: the test would pass on any one colour otherwise.
    let accents: Vec<Color> = ["bzz", "dracula", "light"]
        .iter()
        .map(|n| {
            AppTheme::from(&Theme::builtin(n).expect("a theme"))
                .accent
                .fg
                .unwrap_or(Color::Reset)
        })
        .collect();
    assert!(
        accents[0] != accents[1] && accents[1] != accents[2],
        "{accents:?}"
    );
    // The title of the theme is bold where the theme says so.
    assert!(
        AppTheme::default()
            .title
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn a_colour_that_is_no_colour_is_told_with_the_names() {
    let mut app = start(
        "app.lua",
        "local app = { api = 1, types = { 'note' } }\nfunction app.view() return ui.text { 'x', fg = 'acent' } end\nreturn app",
    );
    assert_eq!(
        app.error().as_deref(),
        Some(
            "app.view returned a bad screen: app.lua:2: ui.text: 'fg' = \"acent\" is not a colour; use a name of the theme (accent, muted, error, warning, success, selection, border, title), a colour name (red, light_blue, gray...), \"#rrggbb\" or 0-255"
        )
    );
    app.reload(
        "local app = { api = 1, types = { 'note' } }\nfunction app.view() return ui.list { items = {}, highlight = 'selected' } end\nreturn app",
    );
    let error = app.error().unwrap_or_default();
    assert!(
        error.contains("ui.list: 'highlight' = \"selected\" is not a style of the theme; use accent, muted, error, warning, success, selection, border, title or a table like {fg = \"red\", bold = true}"),
        "{error}"
    );
}

/// A script that draws one kind of screen, as large as it is told.
const LARGE: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { count = 1, shape = "none", selected = 1 } end
function app.paste(s, text) s.count = tonumber(text) end
function app.key(s, key)
  if key.name == "down" then s.selected = s.selected + 1
  elseif key.name == "end" then s.selected = s.count
  else s.shape = key.name end
end
function app.view(s, ctx)
  local n = s.count
  if s.shape == "l" then
    local items = {}
    for i = 1, n do items[i] = "item " .. i end
    return ui.list { id = "l", items = items, selected = s.selected }
  elseif s.shape == "t" then
    local rows = {}
    for i = 1, n do rows[i] = { "row " .. i, "b", "c", "d", "e", "f", "g" } end
    return ui.table { id = "t", header = { "A", "B", "C", "D", "E", "F", "G" }, rows = rows, selected = s.selected }
  elseif s.shape == "x" then
    return ui.text { string.rep("word ", n // 5), wrap = true }
  elseif s.shape == "r" then
    local parts = {}
    for i = 1, n do parts[i] = ui.text { "p" } end
    return ui.row(parts)
  elseif s.shape == "c" then
    local parts = {}
    for i = 1, n do parts[i] = ui.text { "line " .. i } end
    return ui.column(parts)
  elseif s.shape == "g" then
    -- A grid: rows of eight cells, each cell a block around a text.
    local rows = {}
    for i = 1, n // 26 do
      local cells = {}
      for j = 1, 8 do cells[j] = ui.block { ui.text { "c" } } end
      rows[i] = ui.row(cells)
    end
    return ui.column(rows)
  elseif s.shape == "d" then
    local inner = ui.text { "bottom" }
    for i = 1, n do inner = ui.block { inner } end
    return inner
  elseif s.shape == "i" then
    return ui.input { value = string.rep("é", n), focused = true }
  end
  return "none"
end
return app
"#;

fn large(count: usize, shape: &str, size: (u16, u16)) -> Headless {
    let mut app = Headless::start(
        LARGE,
        HeadlessOptions::new("large.lua")
            .size(size.0, size.1)
            .limits(Limits::patient()),
    )
    .expect("starts");
    app.paste(&count.to_string()).key(shape);
    assert_eq!(app.error(), None, "{shape} {count}");
    app
}

#[test]
fn the_largest_screen_a_script_can_return_is_drawn_in_good_time() {
    // The owner draws on its own thread. These are screens at the limits
    // of the contract, each the worst of its kind, in a window larger than
    // most: what a script can make that thread do.
    let size = (200, 60);
    let theme = AppTheme::default();
    let mut worst = (Duration::ZERO, "");
    for (shape, what, count) in [
        ("l", "a list of 9,990 items", 9_990),
        ("t", "a table of 1,240 rows of 7 cells", 1_240),
        ("x", "131,070 bytes of text, wrapped", 131_070),
        ("r", "a row of 4,990 parts", 4_990),
        ("c", "a column of 4,990 parts", 4_990),
        ("g", "a grid of 384 rows of 8 bordered cells", 9_990),
        ("d", "blocks nested 31 deep", 31),
        ("i", "an input of 60,000 characters", 60_000),
    ] {
        let mut app = large(count, shape, size);
        let area = Rect::new(0, 0, size.0, size.1);
        let mut times = Vec::new();
        for _ in 0..5 {
            let mut buffer = Buffer::empty(area);
            let started = Instant::now();
            app.runtime_mut().draw(area, &mut buffer, &theme);
            times.push(started.elapsed());
        }
        times.sort();
        let median = times[2];
        println!("{what}: {median:?}");
        if median > worst.0 {
            worst = (median, what);
        }
    }
    println!("the slowest: {} in {:?}", worst.1, worst.0);
    // A debug build on a busy machine; a release build is some ten times
    // faster (the validation document has both). What this catches is a
    // screen whose cost is not the number of its elements: Ratatui's own
    // layout took minutes for the row of parts.
    assert!(
        worst.0 < Duration::from_millis(500),
        "{} took {:?}",
        worst.1,
        worst.0
    );
}

#[test]
fn a_table_is_drawn_from_its_parts() {
    let source = r#"
        local app = { api = 1, types = { "note" } }
        function app.init() return { selected = 1 } end
        function app.key(s, key)
          if key.name == "down" then s.selected = s.selected + 1 elseif key.name == "up" then s.selected = s.selected - 1
          elseif key.name == "end" then s.selected = 9 elseif key.name == "esc" then s.selected = nil end
        end
        function app.view(s)
          local rows = {}
          for i = 1, 9 do rows[i] = { "row " .. i, ui.line { tostring(i * 11), align = "right" }, "over" } end
          return ui.table {
            id = "t", header = { "Name", ui.line { "Qty", align = "right" } }, footer = { "nine rows" }, rows = rows,
            widths = { "fill", 5 }, column_spacing = 2, selected = s.selected, highlight_symbol = "> ",
          }
        end
        return app
    "#;
    let mut app = Headless::start(
        source,
        HeadlessOptions::new("table.lua")
            .size(20, 6)
            .limits(Limits::patient()),
    )
    .expect("starts");
    // The header, four rows of nine, the footer. The symbol's column is
    // always there; the second column is 5 cells, two cells from the
    // first, which fills. A third cell has no column and is not drawn.
    assert_eq!(
        app.screen(),
        "  Name           Qty\n> row 1           11\n  row 2           22\n  row 3           33\n  row 4           44\n  nine rows\n"
    );
    // The rows follow the selection, and no further than they must.
    app.key("down").key("down").key("down").key("down");
    assert_eq!(
        app.screen().lines().skip(1).take(4).collect::<Vec<_>>(),
        [
            "  row 2           22",
            "  row 3           33",
            "  row 4           44",
            "> row 5           55"
        ]
    );
    app.key("up");
    assert_eq!(app.screen().lines().nth(3), Some("> row 4           44"));
    assert_eq!(app.screen().lines().nth(1), Some("  row 2           22"));
    app.key("end");
    assert_eq!(app.screen().lines().nth(4), Some("> row 9           99"));
    // With nothing selected a table with an id stays where it was.
    app.key("esc");
    assert_eq!(app.screen().lines().nth(4), Some("  row 9           99"));
}

#[test]
fn a_list_with_an_id_keeps_its_place() {
    let mut app = large(50, "l", (20, 5));
    for _ in 0..9 {
        app.key("down");
    }
    // Item 10 is selected, at the bottom of five rows.
    assert_eq!(app.screen().lines().last(), Some("> item 10"));
    assert_eq!(app.screen().lines().next(), Some("  item 6"));
    // The list was scrolled; a reload does not bring it back to the top.
    app.reload(LARGE);
    assert_eq!(app.screen().lines().next(), Some("  item 6"));
}

#[test]
fn an_input_keeps_its_cursor_in_sight() {
    let source = r#"
        local app = { api = 1, types = { "note" } }
        function app.init() return { field = { value = "" } } end
        function app.key(s, key) ui.edit(s.field, key) end
        function app.paste(s, text) ui.paste(s.field, text) end
        function app.view(s)
          return ui.row { ui.text { "> ", size = 2 }, ui.input { value = s.field.value, cursor = s.field.cursor, focused = true, placeholder = "type" } }
        end
        return app
    "#;
    let mut app = Headless::start(
        source,
        HeadlessOptions::new("input.lua")
            .size(10, 1)
            .limits(Limits::patient()),
    )
    .expect("starts");
    // Empty: the placeholder, and the cursor at the start of the field.
    assert_eq!(app.screen(), "> type\n");
    assert_eq!(app.cursor(), Some(Position::new(2, 0)));
    app.type_text("abc");
    assert_eq!(app.screen(), "> abc\n");
    assert_eq!(app.cursor(), Some(Position::new(5, 0)));
    // Longer than the field: the text moves left under the cursor.
    app.type_text("defghijk");
    assert_eq!(app.screen(), "> efghijk\n");
    assert_eq!(app.cursor(), Some(Position::new(9, 0)));
    app.key("home");
    assert_eq!(app.screen(), "> abcdefgh\n");
    assert_eq!(app.cursor(), Some(Position::new(2, 0)));
    // Wide characters take two cells each, and a paste is one line.
    app.key("ctrl+u").key("end").key("ctrl+u").paste("日本\n語");
    assert_eq!(app.screen(), "> 日本 語\n");
    assert_eq!(app.cursor(), Some(Position::new(9, 0)));
    app.key("left").key("backspace");
    assert_eq!(app.screen(), "> 日本語\n");
    assert_eq!(app.cursor(), Some(Position::new(6, 0)));
}

#[test]
fn a_screen_as_text_counts_a_wide_character_once() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
    buffer.set_string(0, 0, "日本 ok", Style::new());
    buffer.set_string(0, 1, "plain   ", Style::new());
    assert_eq!(text_of(&buffer), "日本 ok\nplain\n");
}

#[test]
fn no_value_a_script_can_give_an_element_breaks_the_drawing() {
    // Ratatui's widgets check some of what they are given and panic on the
    // rest: a ratio over one, a division into nothing. A panic here would
    // be on bzz's own thread. Every property that takes a number, with the
    // numbers a script can write.
    let numbers = [
        "0",
        "1",
        "-1",
        "0.5",
        "1.5",
        "65535",
        "65536",
        "2147483648",
        "math.maxinteger",
        "math.mininteger",
        "1e308",
        "-1e308",
        "0/0",
        "1/0",
        "-1/0",
    ];
    let elements = [
        "ui.gauge { ratio = N }",
        "ui.gauge { percent = N, label = 'x' }",
        "ui.line_gauge { ratio = N }",
        "ui.line_gauge { percent = N }",
        "ui.sparkline { data = { 1, 2, 3 }, max = N }",
        "ui.sparkline { data = { N, 2, N } }",
        "ui.bar_chart { bars = { { label = 'a', value = N }, { value = 2 } }, bar_width = N, bar_gap = N, max = N }",
        "ui.bar_chart { bars = { { value = 1 } }, horizontal = true, bar_width = N }",
        "ui.scrollbar { length = N, position = N, viewport = N }",
        "ui.scrollbar { orientation = 'horizontal_top', length = 3, position = N }",
        "ui.list { items = { 'a', 'b' }, selected = N, scroll_padding = N }",
        "ui.table { rows = { { 'a', 'b' } }, selected = N, column_spacing = N, widths = { N, 'fill' } }",
        "ui.tabs { titles = { 'a', 'b' }, selected = N }",
        "ui.input { value = 'abc', cursor = N, focused = true }",
        "ui.text { 'a\\nb', scroll = N, wrap = true }",
        "ui.block { padding = N, spacing = N, ui.text { 'x' } }",
        "ui.block { padding = { N, N, N, N }, ui.text { 'x' } }",
        "ui.column { spacing = N, margin = N, ui.text { 'x', size = N }, ui.text { 'y' } }",
        "ui.row { spacing = N, flex = 'space_around', ui.text { 'x', size = N }, ui.spacer { size = N } }",
        "ui.popup { width = N, height = N, spacing = N, ui.text { 'x' } }",
        "ui.text { 'x', fg = N }",
    ];
    let theme = AppTheme::default();
    let (mut drawn, mut refused) = (0, 0);
    for element in elements {
        for number in numbers {
            let source = format!(
                "local app = {{ api = 1, types = {{ 'note' }} }}\nfunction app.view() return {} end\nreturn app",
                element.replace('N', number)
            );
            let mut app = start("extreme.lua", &source);
            // Either the screen is refused with words, or it draws.
            match app.error() {
                Some(error) => {
                    assert!(
                        error.contains("extreme.lua:2: ui."),
                        "{element} with {number}: {error}"
                    );
                    refused += 1;
                }
                None => drawn += 1,
            }
            for (w, h) in [(0, 0), (1, 1), (3, 2), (40, 16), (200, 60)] {
                let area = Rect::new(0, 0, w, h);
                let mut buffer = Buffer::empty(area);
                app.runtime_mut().draw(area, &mut buffer, &theme);
            }
            assert_eq!(app.phase(), crate::apps::runtime::Phase::Running);
        }
    }
    println!("{drawn} screens drawn, {refused} refused with an error");
    assert!(
        drawn > 150 && refused > 50,
        "{drawn} drawn, {refused} refused"
    );
}
