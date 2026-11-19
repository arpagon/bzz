//! The element vocabulary: what a `ui.*` constructor accepts, and the
//! conversion of the tree `app.view` returned into [`Node`]s that
//! [`super::draw`] renders with Ratatui. Every element is a thin mapping to
//! one Ratatui widget or layout.
//!
//! A tree is the script's own tables. It is read raw, and the reading stops
//! at the limits of a screen: bzz's thread draws what comes out of here.

use std::str::FromStr;

use mlua::{Table, Value};
use ratatui::layout::{Alignment, Constraint, Direction, Flex};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{BorderType, Borders, Padding, ScrollbarOrientation};

use super::limits::Limits;
use crate::ui::theme::{HighlightGroup, Theme};

/// The style properties every element, span and line accepts, and the shape
/// of every `*_style` and `highlight` table.
pub const STYLE: &[&str] = &[
    "fg",
    "bg",
    "bold",
    "dim",
    "italic",
    "underline",
    "reversed",
    "crossed_out",
];

/// One element of the contract.
pub struct Element {
    pub name: &'static str,
    /// Named properties besides the style ones and `size`.
    pub properties: &'static [&'static str],
    /// Does it take positional items (children or content)?
    pub positional: bool,
}

const fn element(
    name: &'static str,
    properties: &'static [&'static str],
    positional: bool,
) -> Element {
    Element {
        name,
        properties,
        positional,
    }
}

/// The eighteen elements. `docs/app-api.md` and `docs/app-api.d.lua` list
/// the same names and properties; a test compares the three.
pub const ELEMENTS: &[Element] = &[
    element("text", &["align", "wrap", "scroll"], true),
    element("span", &[], true),
    element("line", &["align"], true),
    element(
        "block",
        &[
            "title",
            "title_bottom",
            "title_align",
            "borders",
            "border_type",
            "border_style",
            "padding",
            "spacing",
        ],
        true,
    ),
    element("column", &["spacing", "flex", "margin"], true),
    element("row", &["spacing", "flex", "margin"], true),
    element(
        "list",
        &[
            "items",
            "selected",
            "highlight_symbol",
            "highlight",
            "id",
            "direction",
            "scroll_padding",
        ],
        false,
    ),
    element(
        "table",
        &[
            "header",
            "rows",
            "footer",
            "widths",
            "selected",
            "highlight",
            "highlight_symbol",
            "column_spacing",
            "header_style",
            "id",
        ],
        false,
    ),
    element(
        "tabs",
        &["titles", "selected", "divider", "highlight"],
        false,
    ),
    element(
        "gauge",
        &["ratio", "percent", "label", "gauge_style", "use_unicode"],
        false,
    ),
    element(
        "line_gauge",
        &[
            "ratio",
            "percent",
            "label",
            "filled_style",
            "unfilled_style",
        ],
        false,
    ),
    element("sparkline", &["data", "max", "direction"], false),
    element(
        "bar_chart",
        &[
            "bars",
            "bar_width",
            "bar_gap",
            "max",
            "horizontal",
            "value_style",
            "label_style",
        ],
        false,
    ),
    element(
        "scrollbar",
        &[
            "orientation",
            "length",
            "position",
            "viewport",
            "thumb_style",
            "track_style",
        ],
        false,
    ),
    element(
        "input",
        &["value", "cursor", "placeholder", "focused"],
        false,
    ),
    element("layers", &[], true),
    element(
        "popup",
        &["width", "height", "title", "border_type", "spacing"],
        true,
    ),
    element("spacer", &[], false),
];

pub fn element_named(name: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|e| e.name == name)
}

/// Does the element `name` accept the property `key`?
pub fn accepts(element: &Element, key: &str) -> bool {
    element.properties.contains(&key)
        || STYLE.contains(&key)
        || (key == "size" && element.name != "span" && element.name != "line")
}

pub fn known_properties(element: &Element) -> String {
    let mut all: Vec<&str> = element.properties.to_vec();
    all.extend_from_slice(STYLE);
    if element.name != "span" && element.name != "line" {
        all.push("size");
    }
    all.join(", ")
}

// ------------------------------------------------------------------ colours

/// A colour or a style named by its meaning. bzz's theme says what it looks
/// like, so an app that uses these follows the user's theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeName {
    Accent,
    Muted,
    Error,
    Warning,
    Success,
    Selection,
    Border,
    Title,
}

/// The names of meaning, as a script writes them.
pub const THEME_NAMES: &[(&str, ThemeName)] = &[
    ("accent", ThemeName::Accent),
    ("muted", ThemeName::Muted),
    ("error", ThemeName::Error),
    ("warning", ThemeName::Warning),
    ("success", ThemeName::Success),
    ("selection", ThemeName::Selection),
    ("border", ThemeName::Border),
    ("title", ThemeName::Title),
];

/// The colours with a fixed name, besides `#rrggbb` and an index 0 to 255.
pub const COLOR_NAMES: &[&str] = &[
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "gray",
    "dark_gray",
    "light_red",
    "light_green",
    "light_yellow",
    "light_blue",
    "light_magenta",
    "light_cyan",
    "white",
    "reset",
];

fn theme_name(name: &str) -> Option<ThemeName> {
    THEME_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

/// What an app is drawn with: the styles of bzz's theme that the names of
/// meaning stand for. The owner of a runtime passes it to every draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppTheme {
    /// The pane's own text and background.
    pub normal: Style,
    pub accent: Style,
    pub muted: Style,
    pub error: Style,
    pub warning: Style,
    pub success: Style,
    pub selection: Style,
    pub border: Style,
    pub title: Style,
}

impl AppTheme {
    pub fn style(&self, name: ThemeName) -> Style {
        match name {
            ThemeName::Accent => self.accent,
            ThemeName::Muted => self.muted,
            ThemeName::Error => self.error,
            ThemeName::Warning => self.warning,
            ThemeName::Success => self.success,
            ThemeName::Selection => self.selection,
            ThemeName::Border => self.border,
            ThemeName::Title => self.title,
        }
    }
}

impl From<&Theme> for AppTheme {
    /// `accent` is the theme's primary colour, which bzz uses for links, the
    /// focused border and information; `selection` is a selected row.
    fn from(theme: &Theme) -> Self {
        Self {
            normal: theme.style(HighlightGroup::Normal),
            accent: theme.style(HighlightGroup::Info),
            muted: theme.style(HighlightGroup::Muted),
            error: theme.style(HighlightGroup::Error),
            warning: theme.style(HighlightGroup::Warning),
            success: theme.style(HighlightGroup::Success),
            selection: theme.style(HighlightGroup::SelectedRow),
            border: theme.style(HighlightGroup::Border),
            title: theme.style(HighlightGroup::Title),
        }
    }
}

impl Default for AppTheme {
    fn default() -> Self {
        Self::from(&Theme::default())
    }
}

/// A colour as a script named it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Fixed(Color),
    Theme(ThemeName),
}

/// A style as a script wrote it. The theme is applied when it is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// A whole style of the theme, under the rest.
    pub whole: Option<ThemeName>,
    pub fg: Option<Paint>,
    pub bg: Option<Paint>,
    pub add: Modifier,
}

impl Look {
    pub fn is_plain(&self) -> bool {
        *self == Look::default()
    }

    /// As a foreground a name of meaning is that style's foreground; as a
    /// background it is that style's background, or its foreground when it
    /// has none (so that `bg = "error"` is the error colour).
    pub fn resolve(&self, theme: &AppTheme) -> Style {
        let mut style = match self.whole {
            Some(name) => theme.style(name),
            None => Style::new(),
        };
        match self.fg {
            Some(Paint::Fixed(color)) => style.fg = Some(color),
            Some(Paint::Theme(name)) => {
                if let Some(color) = theme.style(name).fg {
                    style.fg = Some(color);
                }
            }
            None => {}
        }
        match self.bg {
            Some(Paint::Fixed(color)) => style.bg = Some(color),
            Some(Paint::Theme(name)) => {
                let named = theme.style(name);
                if let Some(color) = named.bg.or(named.fg) {
                    style.bg = Some(color);
                }
            }
            None => {}
        }
        style.add_modifier(self.add)
    }
}

// --------------------------------------------------------------------- text

/// A piece of text with its own style.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub look: Look,
}

/// One line of pieces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLine {
    pub pieces: Vec<Piece>,
    pub look: Look,
    pub align: Option<Alignment>,
}

impl TextLine {
    fn raw(text: String) -> Self {
        Self {
            pieces: vec![Piece {
                text,
                look: Look::default(),
            }],
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------- the tree

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Len(u16),
    Pct(u16),
    Min(u16),
    Max(u16),
    Fill(u16),
    Ratio(u32, u32),
}

impl Size {
    pub fn constraint(self) -> Constraint {
        match self {
            Size::Len(n) => Constraint::Length(n),
            Size::Pct(n) => Constraint::Percentage(n),
            Size::Min(n) => Constraint::Min(n),
            Size::Max(n) => Constraint::Max(n),
            Size::Fill(n) => Constraint::Fill(n),
            Size::Ratio(a, b) => Constraint::Ratio(a, b),
        }
    }
}

/// One element of a screen, checked and ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub size: Option<Size>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub label: TextLine,
    pub value: u64,
    pub text: Option<String>,
    pub look: Look,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Text {
        lines: Vec<TextLine>,
        look: Look,
        align: Alignment,
        wrap: bool,
        scroll: u16,
    },
    Block {
        title: Option<TextLine>,
        title_bottom: Option<TextLine>,
        title_align: Alignment,
        borders: Borders,
        border_type: BorderType,
        border_look: Look,
        padding: Padding,
        look: Look,
        spacing: u16,
        children: Vec<Node>,
    },
    Stack {
        direction: Direction,
        spacing: u16,
        flex: Flex,
        margin: u16,
        look: Look,
        children: Vec<Node>,
    },
    List {
        items: Vec<TextLine>,
        selected: Option<usize>,
        symbol: String,
        highlight: Look,
        look: Look,
        id: Option<String>,
        bottom_to_top: bool,
        scroll_padding: usize,
    },
    Table {
        header: Option<Vec<TextLine>>,
        rows: Vec<Vec<TextLine>>,
        footer: Option<Vec<TextLine>>,
        widths: Vec<Constraint>,
        selected: Option<usize>,
        highlight: Look,
        symbol: String,
        column_spacing: u16,
        header_look: Look,
        look: Look,
        id: Option<String>,
    },
    Tabs {
        titles: Vec<TextLine>,
        selected: Option<usize>,
        divider: String,
        highlight: Look,
        look: Look,
    },
    Gauge {
        ratio: f64,
        label: Option<String>,
        look: Look,
        gauge_look: Look,
        unicode: bool,
    },
    LineGauge {
        ratio: f64,
        label: Option<String>,
        look: Look,
        filled: Look,
        unfilled: Look,
    },
    Sparkline {
        data: Vec<u64>,
        max: Option<u64>,
        look: Look,
        right_to_left: bool,
    },
    BarChart {
        bars: Vec<Bar>,
        bar_width: u16,
        bar_gap: u16,
        max: Option<u64>,
        horizontal: bool,
        look: Look,
        value_look: Look,
        label_look: Look,
    },
    Scrollbar {
        orientation: ScrollbarOrientation,
        length: usize,
        position: usize,
        viewport: usize,
        look: Look,
        thumb: Look,
        track: Look,
    },
    Input {
        value: String,
        cursor: usize,
        placeholder: String,
        focused: bool,
        look: Look,
    },
    Layers {
        children: Vec<Node>,
    },
    Popup {
        width: Size,
        height: Size,
        title: Option<TextLine>,
        border_type: BorderType,
        look: Look,
        spacing: u16,
        children: Vec<Node>,
    },
    Spacer,
}

/// What is left of the limits of one screen while it is read.
struct Budget {
    elements: usize,
    text: usize,
    max_elements: usize,
    max_text: usize,
    max_depth: usize,
}

impl Budget {
    fn element(&mut self, n: usize) -> Result<(), String> {
        if n > self.elements {
            return Err(format!(
                "the screen has more than {} elements (every element, line, span and cell counts)",
                self.max_elements
            ));
        }
        self.elements -= n;
        Ok(())
    }

    fn text(&mut self, s: &mlua::LuaString) -> Result<String, String> {
        let bytes = s.as_bytes();
        if bytes.len() > self.text {
            return Err(format!(
                "the screen has more than {} bytes of text",
                self.max_text
            ));
        }
        self.text -= bytes.len();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The items 1..n of a list of the script, each counted as an element.
    fn seq(&mut self, t: &Table) -> Result<Vec<Value>, String> {
        let n = t.raw_len();
        self.element(n)?;
        Ok((1..=n)
            .map(|i| t.raw_get(i).unwrap_or(Value::Nil))
            .collect())
    }
}

/// Converts what `app.view` returned. An error names the element, the
/// property and, when the element was made by a `ui.*` call, its
/// `file:line`.
pub fn parse(v: &Value, limits: &Limits) -> Result<Node, String> {
    let mut budget = Budget {
        elements: limits.screen_elements,
        text: limits.screen_text_bytes,
        max_elements: limits.screen_elements,
        max_text: limits.screen_text_bytes,
        max_depth: limits.screen_depth,
    };
    budget.element(1)?;
    node(v, "view()", 0, &mut budget)
}

struct P<'a> {
    t: &'a Table,
    kind: String,
    at: String,
}

impl P<'_> {
    fn err(&self, message: impl std::fmt::Display) -> String {
        format!("{}ui.{}: {message}", self.at, self.kind)
    }
    fn get(&self, k: &str) -> Value {
        self.t.raw_get(k).unwrap_or(Value::Nil)
    }
    fn int(&self, k: &str) -> Result<Option<i64>, String> {
        match self.get(k) {
            Value::Nil => Ok(None),
            Value::Integer(i) => Ok(Some(i)),
            Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Ok(Some(n as i64)),
            other => Err(self.err(format!("'{k}' must be an integer, got {}", show(&other)))),
        }
    }
    fn uint(&self, k: &str, default: u64) -> Result<u64, String> {
        match self.int(k)? {
            None => Ok(default),
            Some(i) if i >= 0 => Ok(i as u64),
            Some(i) => Err(self.err(format!("'{k}' must be 0 or more, got {i}"))),
        }
    }
    fn u16(&self, k: &str, default: u16) -> Result<u16, String> {
        Ok(self.uint(k, default as u64)?.min(u16::MAX as u64) as u16)
    }
    fn num(&self, k: &str) -> Result<Option<f64>, String> {
        match self.get(k) {
            Value::Nil => Ok(None),
            Value::Integer(i) => Ok(Some(i as f64)),
            Value::Number(n) => Ok(Some(n)),
            other => Err(self.err(format!("'{k}' must be a number, got {}", show(&other)))),
        }
    }
    fn boolean(&self, k: &str) -> Result<bool, String> {
        match self.get(k) {
            Value::Nil => Ok(false),
            Value::Boolean(b) => Ok(b),
            other => Err(self.err(format!("'{k}' must be true or false, got {}", show(&other)))),
        }
    }
    fn string(&self, k: &str, budget: &mut Budget) -> Result<Option<String>, String> {
        match self.get(k) {
            Value::Nil => Ok(None),
            Value::String(s) => Ok(Some(budget.text(&s).map_err(|e| self.err(e))?)),
            Value::Integer(i) => Ok(Some(i.to_string())),
            Value::Number(n) => Ok(Some(n.to_string())),
            other => Err(self.err(format!("'{k}' must be a string, got {}", show(&other)))),
        }
    }
    fn choice<T: Clone>(&self, k: &str, options: &[(&str, T)], default: T) -> Result<T, String> {
        let chosen = match self.get(k) {
            Value::Nil => return Ok(default),
            Value::String(s) => s.to_string_lossy(),
            other => show(&other),
        };
        options
            .iter()
            .find(|(n, _)| *n == chosen)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
                self.err(format!(
                    "'{k}' must be one of {}, got {}",
                    names.join(", "),
                    brief(&chosen)
                ))
            })
    }
    /// 1-based index in Lua, 0-based in Ratatui. 0 or nil is none.
    fn selected(&self) -> Result<Option<usize>, String> {
        Ok(match self.int("selected")? {
            Some(i) if i >= 1 => Some(i as usize - 1),
            _ => None,
        })
    }
    fn look_here(&self) -> Result<Look, String> {
        look_of(self.t).map_err(|e| self.err(e))
    }
    /// A style property: a table of style fields, or a name of meaning.
    fn look(&self, k: &str) -> Result<Look, String> {
        match self.get(k) {
            Value::Nil => Ok(Look::default()),
            Value::Table(t) => look_of(&t).map_err(|e| self.err(format!("'{k}': {e}"))),
            Value::String(s) => {
                let name = s.to_string_lossy();
                match theme_name(&name) {
                    Some(whole) => Ok(Look {
                        whole: Some(whole),
                        ..Look::default()
                    }),
                    None => Err(self.err(format!(
                        "'{k}' = {} is not a style of the theme; use {} or a table like {{fg = \"red\", bold = true}}",
                        brief(&name),
                        theme_list()
                    ))),
                }
            }
            other => Err(self.err(format!(
                "'{k}' must be a style table like {{fg = \"red\", bold = true}} or a name of the theme, got {}",
                show(&other)
            ))),
        }
    }
    fn line_opt(&self, k: &str, budget: &mut Budget) -> Result<Option<TextLine>, String> {
        match self.get(k) {
            Value::Nil => Ok(None),
            v => line_of(&v, budget)
                .map(Some)
                .map_err(|e| self.err(format!("'{k}': {e}"))),
        }
    }
    fn lines_list(&self, k: &str, budget: &mut Budget) -> Result<Vec<TextLine>, String> {
        match self.get(k) {
            Value::Nil => Ok(Vec::new()),
            Value::Table(t) => {
                let mut out = Vec::new();
                for (i, v) in budget
                    .seq(&t)
                    .map_err(|e| self.err(e))?
                    .into_iter()
                    .enumerate()
                {
                    out.push(
                        line_of(&v, budget)
                            .map_err(|e| self.err(format!("'{k}'[{}]: {e}", i + 1)))?,
                    );
                }
                Ok(out)
            }
            other => Err(self.err(format!("'{k}' must be a list, got {}", show(&other)))),
        }
    }
    fn children(&self, path: &str, depth: usize, budget: &mut Budget) -> Result<Vec<Node>, String> {
        budget
            .seq(self.t)
            .map_err(|e| self.err(e))?
            .iter()
            .enumerate()
            .map(|(i, v)| node(v, &format!("{path}[{}]", i + 1), depth + 1, budget))
            .collect()
    }
}

fn theme_list() -> String {
    THEME_NAMES
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A value of the script in an error message, cut short.
fn brief(text: &str) -> String {
    let mut shown: String = text.chars().take(40).collect();
    if shown.len() < text.len() {
        shown.push('…');
    }
    format!("\"{shown}\"")
}

fn show(v: &Value) -> String {
    match v {
        Value::String(s) => brief(&s.to_string_lossy()),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        other => other.type_name().to_string(),
    }
}

fn node(v: &Value, path: &str, depth: usize, budget: &mut Budget) -> Result<Node, String> {
    if depth > budget.max_depth {
        let at = match v {
            Value::Table(t) => match t.raw_get::<Value>("_at") {
                Ok(Value::String(at)) => at.to_string_lossy(),
                _ => "view()".to_string(),
            },
            _ => "view()".to_string(),
        };
        return Err(format!(
            "{at}: elements nest deeper than {} levels",
            budget.max_depth
        ));
    }
    let t = match v {
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            let lines = text_lines(v, budget).map_err(|e| format!("{path}: {e}"))?;
            return Ok(Node {
                size: None,
                kind: Kind::Text {
                    lines,
                    look: Look::default(),
                    align: Alignment::Left,
                    wrap: false,
                    scroll: 0,
                },
            });
        }
        Value::Table(t) => t,
        other => {
            return Err(format!(
                "{path}: expected a ui element or a string, got {}",
                other.type_name()
            ));
        }
    };
    let Some(kind) = kind_of(t) else {
        return Err(format!(
            "{path}: a plain table is not an element; build it with ui.text{{...}}, ui.column{{...}}, ..."
        ));
    };
    let at = match t.raw_get::<Value>("_at") {
        Ok(Value::String(at)) => format!("{}: ", at.to_string_lossy()),
        _ => format!("{path}: "),
    };
    let p = P {
        t,
        kind: kind.clone(),
        at,
    };
    let size = match p.get("size") {
        Value::Nil => None,
        v => Some(parse_size(&v).map_err(|e| p.err(format!("'size': {e}")))?),
    };
    let look = p.look_here()?;
    let kind = match kind.as_str() {
        "text" => {
            let mut lines = Vec::new();
            for (i, item) in budget.seq(t).map_err(|e| p.err(e))?.iter().enumerate() {
                lines.extend(
                    text_lines(item, budget)
                        .map_err(|e| p.err(format!("item [{}]: {e}", i + 1)))?,
                );
            }
            Kind::Text {
                lines,
                look,
                align: align(&p, "align")?,
                wrap: p.boolean("wrap")?,
                scroll: p.u16("scroll", 0)?,
            }
        }
        "span" | "line" => {
            let line = line_of(v, budget).map_err(|e| p.err(e))?;
            Kind::Text {
                lines: vec![line],
                look: Look::default(),
                align: Alignment::Left,
                wrap: false,
                scroll: 0,
            }
        }
        "block" => Kind::Block {
            title: p.line_opt("title", budget)?,
            title_bottom: p.line_opt("title_bottom", budget)?,
            title_align: align(&p, "title_align")?,
            borders: borders(&p, budget)?,
            border_type: border_type(&p)?,
            border_look: p.look("border_style")?,
            padding: padding(&p)?,
            look,
            spacing: p.u16("spacing", 0)?,
            children: p.children(&format!("{path}.block"), depth, budget)?,
        },
        "column" | "row" => Kind::Stack {
            direction: if kind == "row" {
                Direction::Horizontal
            } else {
                Direction::Vertical
            },
            spacing: p.u16("spacing", 0)?,
            flex: p.choice(
                "flex",
                &[
                    ("legacy", Flex::Legacy),
                    ("start", Flex::Start),
                    ("center", Flex::Center),
                    ("end", Flex::End),
                    ("space_between", Flex::SpaceBetween),
                    ("space_around", Flex::SpaceAround),
                    ("space_evenly", Flex::SpaceEvenly),
                ],
                Flex::Legacy,
            )?,
            margin: p.u16("margin", 0)?,
            look,
            children: p.children(&format!("{path}.{kind}"), depth, budget)?,
        },
        "list" => Kind::List {
            items: p.lines_list("items", budget)?,
            selected: p.selected()?,
            symbol: p
                .string("highlight_symbol", budget)?
                .unwrap_or_else(|| "> ".into()),
            highlight: p.look("highlight")?,
            look,
            id: p.string("id", budget)?,
            bottom_to_top: p.choice(
                "direction",
                &[("top_to_bottom", false), ("bottom_to_top", true)],
                false,
            )?,
            scroll_padding: p.uint("scroll_padding", 0)?.min(u16::MAX as u64) as usize,
        },
        "table" => table(&p, look, budget)?,
        "tabs" => {
            let titles = match p.get("titles") {
                Value::Nil => Vec::new(),
                Value::Table(titles) => budget
                    .seq(&titles)
                    .map_err(|e| p.err(e))?
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        line_of(v, budget).map_err(|e| p.err(format!("'titles'[{}]: {e}", i + 1)))
                    })
                    .collect::<Result<_, _>>()?,
                other => {
                    return Err(p.err(format!("'titles' must be a list, got {}", show(&other))));
                }
            };
            Kind::Tabs {
                titles,
                selected: p.selected()?,
                divider: p.string("divider", budget)?.unwrap_or_else(|| "|".into()),
                highlight: p.look("highlight")?,
                look,
            }
        }
        "gauge" => Kind::Gauge {
            ratio: ratio(&p)?,
            label: p.string("label", budget)?,
            look,
            gauge_look: p.look("gauge_style")?,
            unicode: p.boolean("use_unicode")?,
        },
        "line_gauge" => Kind::LineGauge {
            ratio: ratio(&p)?,
            label: p.string("label", budget)?,
            look,
            filled: p.look("filled_style")?,
            unfilled: p.look("unfilled_style")?,
        },
        "sparkline" => {
            let data = match p.get("data") {
                Value::Nil => Vec::new(),
                Value::Table(d) => budget
                    .seq(&d)
                    .map_err(|e| p.err(e))?
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        amount(v).ok_or_else(|| {
                            p.err(format!(
                                "'data'[{}] must be a number 0 or more, got {}",
                                i + 1,
                                show(v)
                            ))
                        })
                    })
                    .collect::<Result<_, _>>()?,
                other => {
                    return Err(p.err(format!(
                        "'data' must be a list of numbers, got {}",
                        show(&other)
                    )));
                }
            };
            Kind::Sparkline {
                data,
                max: p.int("max")?.map(|m| m.max(0) as u64),
                look,
                right_to_left: p.choice(
                    "direction",
                    &[("left_to_right", false), ("right_to_left", true)],
                    false,
                )?,
            }
        }
        "bar_chart" => {
            let bars = match p.get("bars") {
                Value::Nil => Vec::new(),
                Value::Table(bs) => budget
                    .seq(&bs)
                    .map_err(|e| p.err(e))?
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        bar_of(b, budget).map_err(|e| p.err(format!("'bars'[{}]: {e}", i + 1)))
                    })
                    .collect::<Result<_, _>>()?,
                other => {
                    return Err(p.err(format!("'bars' must be a list, got {}", show(&other))));
                }
            };
            Kind::BarChart {
                bars,
                bar_width: p.u16("bar_width", 3)?.max(1),
                bar_gap: p.u16("bar_gap", 1)?,
                max: p.int("max")?.map(|m| m.max(0) as u64),
                horizontal: p.boolean("horizontal")?,
                look,
                value_look: p.look("value_style")?,
                label_look: p.look("label_style")?,
            }
        }
        "scrollbar" => Kind::Scrollbar {
            orientation: p.choice(
                "orientation",
                &[
                    ("vertical_right", ScrollbarOrientation::VerticalRight),
                    ("vertical_left", ScrollbarOrientation::VerticalLeft),
                    ("horizontal_bottom", ScrollbarOrientation::HorizontalBottom),
                    ("horizontal_top", ScrollbarOrientation::HorizontalTop),
                ],
                ScrollbarOrientation::VerticalRight,
            )?,
            length: p.uint("length", 0)?.min(u32::MAX as u64) as usize,
            position: (p.uint("position", 1)?.clamp(1, u32::MAX as u64) - 1) as usize,
            viewport: p.uint("viewport", 0)?.min(u32::MAX as u64) as usize,
            look,
            thumb: p.look("thumb_style")?,
            track: p.look("track_style")?,
        },
        "input" => {
            let value = p.string("value", budget)?.unwrap_or_default();
            let len = value.chars().count();
            let cursor = p
                .int("cursor")?
                .map(|c| (c.max(0) as u64).min(len as u64) as usize)
                .unwrap_or(len);
            Kind::Input {
                value,
                cursor,
                placeholder: p.string("placeholder", budget)?.unwrap_or_default(),
                focused: p.boolean("focused")?,
                look,
            }
        }
        "layers" => Kind::Layers {
            children: p.children(&format!("{path}.layers"), depth, budget)?,
        },
        "popup" => Kind::Popup {
            width: match p.get("width") {
                Value::Nil => Size::Pct(60),
                v => parse_size(&v).map_err(|e| p.err(format!("'width': {e}")))?,
            },
            height: match p.get("height") {
                Value::Nil => Size::Pct(50),
                v => parse_size(&v).map_err(|e| p.err(format!("'height': {e}")))?,
            },
            title: p.line_opt("title", budget)?,
            border_type: border_type(&p)?,
            look,
            spacing: p.u16("spacing", 0)?,
            children: p.children(&format!("{path}.popup"), depth, budget)?,
        },
        "spacer" => Kind::Spacer,
        other => return Err(format!("{path}: unknown element kind {}", brief(other))),
    };
    Ok(Node { size, kind })
}

fn table(p: &P, look: Look, budget: &mut Budget) -> Result<Kind, String> {
    let row_of = |v: &Value, what: &str, budget: &mut Budget| -> Result<Vec<TextLine>, String> {
        let Value::Table(r) = v else {
            return Err(p.err(format!("{what} must be a list of cells, got {}", show(v))));
        };
        budget
            .seq(r)
            .map_err(|e| p.err(e))?
            .iter()
            .enumerate()
            .map(|(j, c)| line_of(c, budget).map_err(|e| p.err(format!("{what}[{}]: {e}", j + 1))))
            .collect()
    };
    let header = match p.get("header") {
        Value::Nil => None,
        v => Some(row_of(&v, "'header'", budget)?),
    };
    let footer = match p.get("footer") {
        Value::Nil => None,
        v => Some(row_of(&v, "'footer'", budget)?),
    };
    let rows: Vec<Vec<TextLine>> = match p.get("rows") {
        Value::Nil => Vec::new(),
        Value::Table(rs) => budget
            .seq(&rs)
            .map_err(|e| p.err(e))?
            .iter()
            .enumerate()
            .map(|(i, r)| row_of(r, &format!("'rows'[{}]", i + 1), budget))
            .collect::<Result<_, _>>()?,
        other => {
            return Err(p.err(format!(
                "'rows' must be a list of rows, got {}",
                show(&other)
            )));
        }
    };
    let columns = header
        .as_ref()
        .map_or(0, Vec::len)
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let widths = match p.get("widths") {
        Value::Nil => vec![Constraint::Fill(1); columns],
        Value::Table(ws) => budget
            .seq(&ws)
            .map_err(|e| p.err(e))?
            .iter()
            .enumerate()
            .map(|(i, w)| {
                parse_size(w)
                    .map(Size::constraint)
                    .map_err(|e| p.err(format!("'widths'[{}]: {e}", i + 1)))
            })
            .collect::<Result<_, _>>()?,
        other => {
            return Err(p.err(format!(
                "'widths' must be a list of sizes, got {}",
                show(&other)
            )));
        }
    };
    Ok(Kind::Table {
        header,
        rows,
        footer,
        widths,
        selected: p.selected()?,
        highlight: p.look("highlight")?,
        symbol: p
            .string("highlight_symbol", budget)?
            .unwrap_or_else(|| "> ".into()),
        column_spacing: p.u16("column_spacing", 1)?,
        header_look: p.look("header_style")?,
        look,
        id: p.string("id", budget)?,
    })
}

/// A number 0 or more, as a bar's or a sparkline's value.
fn amount(v: &Value) -> Option<u64> {
    match v {
        Value::Integer(n) if *n >= 0 => Some(*n as u64),
        Value::Number(n) if *n >= 0.0 && n.is_finite() => Some(n.round().min(1.0e18) as u64),
        _ => None,
    }
}

fn ratio(p: &P) -> Result<f64, String> {
    let r = match (p.num("ratio")?, p.num("percent")?) {
        (Some(r), _) => r,
        (None, Some(percent)) => percent / 100.0,
        (None, None) => 0.0,
    };
    Ok(if r.is_finite() {
        r.clamp(0.0, 1.0)
    } else {
        0.0
    })
}

fn align(p: &P, k: &str) -> Result<Alignment, String> {
    p.choice(
        k,
        &[
            ("left", Alignment::Left),
            ("center", Alignment::Center),
            ("right", Alignment::Right),
        ],
        Alignment::Left,
    )
}

fn border_type(p: &P) -> Result<BorderType, String> {
    p.choice(
        "border_type",
        &[
            ("plain", BorderType::Plain),
            ("rounded", BorderType::Rounded),
            ("double", BorderType::Double),
            ("thick", BorderType::Thick),
        ],
        BorderType::Plain,
    )
}

fn borders(p: &P, budget: &mut Budget) -> Result<Borders, String> {
    let one = |s: &str| -> Result<Borders, String> {
        Ok(match s {
            "all" => Borders::ALL,
            "none" => Borders::NONE,
            "top" => Borders::TOP,
            "bottom" => Borders::BOTTOM,
            "left" => Borders::LEFT,
            "right" => Borders::RIGHT,
            _ => {
                return Err(p.err(format!(
                    "'borders' items are all, none, top, bottom, left, right; got {}",
                    brief(s)
                )));
            }
        })
    };
    match p.get("borders") {
        Value::Nil => Ok(Borders::ALL),
        Value::String(s) => one(&s.to_string_lossy()),
        Value::Table(t) => {
            let mut b = Borders::NONE;
            for v in budget.seq(&t).map_err(|e| p.err(e))? {
                let Value::String(s) = v else {
                    return Err(p.err("'borders' list items must be strings"));
                };
                b |= one(&s.to_string_lossy())?;
            }
            Ok(b)
        }
        other => Err(p.err(format!(
            "'borders' must be a string or a list, got {}",
            show(&other)
        ))),
    }
}

fn padding(p: &P) -> Result<Padding, String> {
    let cell = |v: &Value| match v {
        Value::Integer(n) if *n >= 0 => (*n).min(u16::MAX as i64) as u16,
        _ => 0,
    };
    match p.get("padding") {
        Value::Nil => Ok(Padding::ZERO),
        v @ Value::Integer(_) => Ok(Padding::uniform(cell(&v))),
        Value::Table(t) => {
            let get = |i: usize| cell(&t.raw_get(i).unwrap_or(Value::Nil));
            match t.raw_len() {
                2 => Ok(Padding::symmetric(get(1), get(2))),
                4 => Ok(Padding::new(get(1), get(2), get(3), get(4))),
                _ => Err(p.err(
                    "'padding' is a number, {horizontal, vertical} or {left, right, top, bottom}",
                )),
            }
        }
        other => Err(p.err(format!(
            "'padding' must be a number or a list, got {}",
            show(&other)
        ))),
    }
}

/// A size: number of cells, "50%", "min:10", "max:10", "fill", "fill:2" or
/// "1/3".
pub fn parse_size(v: &Value) -> Result<Size, String> {
    let bad = |s: &str| {
        format!(
            "bad size {}; use 10, \"50%\", \"min:10\", \"max:10\", \"fill\", \"fill:2\" or \"1/3\"",
            brief(s)
        )
    };
    let n16 = |s: &str, all: &str| s.trim().parse::<u16>().map_err(|_| bad(all));
    match v {
        Value::Integer(n) if *n >= 0 => Ok(Size::Len((*n).min(u16::MAX as i64) as u16)),
        Value::Number(n) if *n >= 0.0 => Ok(Size::Len(n.round().min(u16::MAX as f64) as u16)),
        Value::String(s) => {
            let s = s.to_string_lossy();
            let s = s.trim();
            if let Some(x) = s.strip_suffix('%') {
                return Ok(Size::Pct(n16(x, s)?.min(100)));
            }
            if let Some(x) = s.strip_prefix("min:") {
                return Ok(Size::Min(n16(x, s)?));
            }
            if let Some(x) = s.strip_prefix("max:") {
                return Ok(Size::Max(n16(x, s)?));
            }
            if s == "fill" {
                return Ok(Size::Fill(1));
            }
            if let Some(x) = s.strip_prefix("fill:") {
                return Ok(Size::Fill(n16(x, s)?));
            }
            if let Some((a, b)) = s.split_once('/') {
                let a = a.trim().parse::<u16>().map_err(|_| bad(s))?;
                let b = b.trim().parse::<u16>().map_err(|_| bad(s))?;
                if b == 0 {
                    return Err(bad(s));
                }
                return Ok(Size::Ratio(a as u32, b as u32));
            }
            Err(bad(s))
        }
        other => Err(format!(
            "a size must be a number or a string, got {}",
            show(other)
        )),
    }
}

/// The style fields of a table of the script.
pub fn look_of(t: &Table) -> Result<Look, String> {
    let mut look = Look::default();
    let color = |k: &str| -> Result<Option<Paint>, String> {
        match t.raw_get::<Value>(k).unwrap_or(Value::Nil) {
            Value::Nil => Ok(None),
            Value::String(c) => {
                let c = c.to_string_lossy();
                if let Some(name) = theme_name(&c) {
                    return Ok(Some(Paint::Theme(name)));
                }
                // Ratatui reads more spellings than the contract lists
                // ("grey", "bright red"); they stay accepted.
                Color::from_str(&c).map(|c| Some(Paint::Fixed(c))).map_err(|_| {
                    format!(
                        "'{k}' = {} is not a colour; use a name of the theme ({}), a colour name (red, light_blue, gray...), \"#rrggbb\" or 0-255",
                        brief(&c),
                        theme_list()
                    )
                })
            }
            Value::Integer(i) if (0..=255).contains(&i) => {
                Ok(Some(Paint::Fixed(Color::Indexed(i as u8))))
            }
            other => Err(format!(
                "'{k}' must be a colour name, \"#rrggbb\" or 0-255, got {}",
                show(&other)
            )),
        }
    };
    look.fg = color("fg")?;
    look.bg = color("bg")?;
    for (k, m) in [
        ("bold", Modifier::BOLD),
        ("dim", Modifier::DIM),
        ("italic", Modifier::ITALIC),
        ("underline", Modifier::UNDERLINED),
        ("reversed", Modifier::REVERSED),
        ("crossed_out", Modifier::CROSSED_OUT),
    ] {
        match t.raw_get::<Value>(k).unwrap_or(Value::Nil) {
            Value::Nil | Value::Boolean(false) => {}
            Value::Boolean(true) => look.add |= m,
            other => return Err(format!("'{k}' must be true or false, got {}", show(&other))),
        }
    }
    Ok(look)
}

fn kind_of(t: &Table) -> Option<String> {
    match t.raw_get::<Value>("kind") {
        Ok(Value::String(kind)) => Some(kind.to_string_lossy()),
        _ => None,
    }
}

fn piece_of(v: &Value, budget: &mut Budget) -> Result<Piece, String> {
    match v {
        Value::String(s) => Ok(Piece {
            text: budget.text(s)?,
            look: Look::default(),
        }),
        Value::Integer(_) | Value::Number(_) => Ok(Piece {
            text: show(v),
            look: Look::default(),
        }),
        Value::Table(t) if kind_of(t).as_deref() == Some("span") => {
            let text = match t.raw_get::<Value>(1).unwrap_or(Value::Nil) {
                Value::String(s) => budget.text(&s)?,
                Value::Nil => String::new(),
                other => show(&other),
            };
            Ok(Piece {
                text,
                look: look_of(t)?,
            })
        }
        other => Err(format!(
            "expected a string or ui.span, got {}",
            describe(other)
        )),
    }
}

fn describe(v: &Value) -> String {
    match v {
        Value::Table(t) => match kind_of(t) {
            Some(k) => format!("ui.{}", k.chars().take(20).collect::<String>()),
            None => "a plain table".into(),
        },
        other => show(other),
    }
}

/// A string, ui.span or ui.line as one line (line breaks become spaces).
fn line_of(v: &Value, budget: &mut Budget) -> Result<TextLine, String> {
    match v {
        Value::Table(t) if kind_of(t).as_deref() == Some("line") => {
            let mut pieces = Vec::new();
            for (i, x) in budget.seq(t)?.iter().enumerate() {
                pieces.push(
                    piece_of(x, budget).map_err(|e| format!("ui.line item [{}]: {e}", i + 1))?,
                );
            }
            let align = match t.raw_get::<Value>("align").unwrap_or(Value::Nil) {
                Value::Nil => None,
                Value::String(a) => match &*a.to_string_lossy() {
                    "left" => Some(Alignment::Left),
                    "center" => Some(Alignment::Center),
                    "right" => Some(Alignment::Right),
                    other => {
                        return Err(format!(
                            "ui.line: 'align' must be one of left, center, right, got {}",
                            brief(other)
                        ));
                    }
                },
                other => {
                    return Err(format!(
                        "ui.line: 'align' must be one of left, center, right, got {}",
                        show(&other)
                    ));
                }
            };
            Ok(TextLine {
                pieces,
                look: look_of(t)?,
                align,
            })
        }
        Value::String(s) => Ok(TextLine::raw(budget.text(s)?.replace(['\n', '\r'], " "))),
        _ => {
            let piece = piece_of(v, budget)?;
            Ok(TextLine {
                pieces: vec![piece],
                ..TextLine::default()
            })
        }
    }
}

/// Items of a ui.text: a string with line breaks gives several lines.
fn text_lines(v: &Value, budget: &mut Budget) -> Result<Vec<TextLine>, String> {
    match v {
        Value::String(s) => {
            let text = budget.text(s)?;
            let lines: Vec<TextLine> = text
                .split('\n')
                .map(|l| TextLine::raw(l.trim_end_matches('\r').to_string()))
                .collect();
            // The first line was counted as the item itself.
            budget.element(lines.len() - 1)?;
            Ok(lines)
        }
        _ => Ok(vec![line_of(v, budget)?]),
    }
}

fn bar_of(v: &Value, budget: &mut Budget) -> Result<Bar, String> {
    let Value::Table(t) = v else {
        return Err(format!(
            "expected {{label = ..., value = ...}}, got {}",
            show(v)
        ));
    };
    let given = t.raw_get::<Value>("value").unwrap_or(Value::Nil);
    let value = amount(&given)
        .ok_or_else(|| format!("'value' must be a number 0 or more, got {}", show(&given)))?;
    let label = match t.raw_get::<Value>("label").unwrap_or(Value::Nil) {
        Value::Nil => TextLine::default(),
        l => line_of(&l, budget)?,
    };
    let text = match t.raw_get::<Value>("text").unwrap_or(Value::Nil) {
        Value::Nil => None,
        Value::String(s) => Some(budget.text(&s)?),
        Value::Integer(i) => Some(i.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => return Err(format!("'text' must be a string, got {}", show(&other))),
    };
    Ok(Bar {
        label,
        value,
        text,
        look: look_of(t)?,
    })
}
