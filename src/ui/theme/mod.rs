mod builtin;
mod parse;

use std::{
    collections::BTreeMap,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use ratatui::{
    style::{Color, Modifier, Style},
    widgets::BorderType,
};

use crate::{
    error::{Error, Result},
    paths::{Paths, set_private_permissions},
};

pub const DEFAULT_THEME_ID: &str = "bzz";
const MAX_THEME_BYTES: u64 = 256 * 1024;

macro_rules! define_groups {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr(usize)]
        pub enum HighlightGroup {
            $($variant),+
        }

        impl HighlightGroup {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const COUNT: usize = Self::ALL.len();

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

define_groups! {
    Normal => "Normal",
    Strong => "Strong",
    Emphasis => "Emphasis",
    Muted => "Muted",
    Title => "Title",
    Border => "Border",
    FocusBorder => "FocusBorder",
    Selection => "Selection",
    SelectionBorder => "SelectionBorder",
    Error => "Error",
    Warning => "Warning",
    Success => "Success",
    Info => "Info",
    Pending => "Pending",
    Rejected => "Rejected",
    PaneTitle => "PaneTitle",
    PaneBorder => "PaneBorder",
    FocusedPaneBorder => "FocusedPaneBorder",
    ModalTitle => "ModalTitle",
    ModalBorder => "ModalBorder",
    CommunityRail => "CommunityRail",
    CommunitySelected => "CommunitySelected",
    Sidebar => "Sidebar",
    SidebarText => "SidebarText",
    SidebarMuted => "SidebarMuted",
    ChannelUnread => "ChannelUnread",
    SelectedRow => "SelectedRow",
    SelectionMarker => "SelectionMarker",
    UnreadNotice => "UnreadNotice",
    MessageAuthor => "MessageAuthor",
    MessageTimestamp => "MessageTimestamp",
    MessageBody => "MessageBody",
    MessageDeleted => "MessageDeleted",
    Reaction => "Reaction",
    SelfReaction => "SelfReaction",
    Composer => "Composer",
    ComposerTitle => "ComposerTitle",
    ComposerBorder => "ComposerBorder",
    ActiveComposerBorder => "ActiveComposerBorder",
    StatusBar => "StatusBar",
    StatusMode => "StatusMode",
    StatusModeInsert => "StatusModeInsert",
    StatusModeCommand => "StatusModeCommand",
    MarkdownLink => "MarkdownLink",
    MarkdownCode => "MarkdownCode",
    MarkdownMarker => "MarkdownMarker",
    Placeholder => "Placeholder",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum BorderSurface {
    Pane,
    Composer,
    Modal,
    Picker,
    Message,
}

impl BorderSurface {
    const COUNT: usize = 5;

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "pane" => Some(Self::Pane),
            "composer" => Some(Self::Composer),
            "modal" => Some(Self::Modal),
            "picker" => Some(Self::Picker),
            "message" => Some(Self::Message),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeScope {
    Global,
    Community,
}

#[derive(Clone, Debug)]
pub struct Theme {
    id: String,
    name: String,
    highlights: [ResolvedHighlight; HighlightGroup::COUNT],
    borders: [BorderType; BorderSurface::COUNT],
}

impl Theme {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn style(&self, group: HighlightGroup) -> Style {
        self.highlights[group as usize].style
    }

    pub fn apply(&self, group: HighlightGroup, base: Style) -> Style {
        let highlight = self.highlights[group as usize];
        let mut resolved = base.patch(highlight.style);
        if highlight.clear_foreground {
            resolved.fg = None;
        }
        if highlight.clear_background {
            resolved.bg = None;
        }
        resolved
    }

    pub const fn border_type(&self, surface: BorderSurface) -> BorderType {
        self.borders[surface as usize]
    }

    pub fn builtin(name: &str) -> Option<Self> {
        let palette = builtin::lookup(name)?;
        Some(Self::from_palette(
            palette,
            &ThemeOptions::default(),
            &mut Vec::new(),
        ))
    }

    fn from_palette(
        palette: &'static Palette,
        options: &ThemeOptions,
        warnings: &mut Vec<String>,
    ) -> Self {
        let mut definitions = default_definitions();
        if palette.id != DEFAULT_THEME_ID {
            apply_palette(&mut definitions, palette);
        }
        apply_overrides(&mut definitions, options, warnings);
        Self {
            id: palette.id.to_owned(),
            name: palette.name.to_owned(),
            highlights: resolve_definitions(&definitions, warnings),
            borders: resolve_borders(&options.borders),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::builtin(DEFAULT_THEME_ID).expect("the bzz theme is built in")
    }
}

#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub theme: Theme,
    pub warnings: Vec<String>,
}

pub struct ThemeRegistry;

impl ThemeRegistry {
    pub fn entries() -> impl Iterator<Item = ThemeEntry> {
        builtin::PALETTES.iter().map(|palette| ThemeEntry {
            id: palette.id,
            name: palette.name,
        })
    }

    pub fn contains(name: &str) -> bool {
        builtin::lookup(name).is_some()
    }

    pub fn canonical_id(name: &str) -> Option<&'static str> {
        builtin::lookup(name).map(|palette| palette.id)
    }

    pub fn export(name: &str) -> Result<String> {
        let palette =
            builtin::lookup(name).ok_or_else(|| Error::Config(format!("unknown theme: {name}")))?;
        Ok(export_palette(palette))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeEntry {
    pub id: &'static str,
    pub name: &'static str,
}

pub fn load(paths: &Paths, selected: &str) -> Result<LoadedTheme> {
    let mut warnings = Vec::new();
    let palette = match builtin::lookup(selected) {
        Some(palette) => palette,
        None => {
            warnings.push(format!(
                "unknown theme {selected:?}; using {DEFAULT_THEME_ID}"
            ));
            builtin::lookup(DEFAULT_THEME_ID).expect("the bzz theme is built in")
        }
    };
    let path = paths.theme_file();
    let options = match fs::metadata(&path) {
        Ok(metadata) => {
            if metadata.len() > MAX_THEME_BYTES {
                return Err(Error::Config(format!(
                    "{} exceeds the 256 KiB theme size limit",
                    path.display()
                )));
            }
            let text = fs::read_to_string(&path).map_err(|error| Error::io(&path, error))?;
            let (options, mut parser_warnings) = parse::parse(&text).map_err(|error| {
                Error::Config(format!("{} is not valid TOML: {error}", path.display()))
            })?;
            warnings.append(&mut parser_warnings);
            options
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ThemeOptions::default(),
        Err(error) => return Err(Error::io(&path, error)),
    };
    let theme = Theme::from_palette(palette, &options, &mut warnings);
    warnings.extend(contrast_warnings(&theme));
    Ok(LoadedTheme { theme, warnings })
}

pub fn check(paths: &Paths, names: impl IntoIterator<Item = String>) -> Result<Vec<String>> {
    let mut names = names.into_iter().collect::<std::collections::BTreeSet<_>>();
    if names.is_empty() {
        names.insert(DEFAULT_THEME_ID.into());
    }
    let mut warnings = Vec::new();
    for name in names {
        if !ThemeRegistry::contains(&name) {
            return Err(Error::Config(format!("unknown theme: {name}")));
        }
        for warning in load(paths, &name)?.warnings {
            let warning = format!("{name}: {warning}");
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }
    Ok(warnings)
}

pub fn export_to(name: &str, output: &Path) -> Result<()> {
    let content = ThemeRegistry::export(name)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| Error::io(output, error))?;
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = fs::remove_file(output);
        return Err(Error::io(output, error));
    }
    if let Err(error) = set_private_permissions(output) {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    Ok(())
}

pub fn theme_path(paths: &Paths) -> PathBuf {
    paths.theme_file()
}

#[derive(Clone, Copy, Debug, Default)]
struct HighlightDefinition {
    link: Option<HighlightGroup>,
    style: Style,
    clear_foreground: bool,
    clear_background: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct ResolvedHighlight {
    style: Style,
    clear_foreground: bool,
    clear_background: bool,
}

#[derive(Clone, Copy, Debug)]
enum ColorOverride {
    Set(Color),
    Clear,
}

#[derive(Clone, Debug, Default)]
pub(super) struct HighlightOptions {
    link: Option<Option<HighlightGroup>>,
    foreground: Option<String>,
    background: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    dim: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
}

#[derive(Clone, Debug)]
pub(super) struct BorderOptions {
    default: Option<BorderType>,
    surfaces: [Option<BorderType>; BorderSurface::COUNT],
}

impl Default for BorderOptions {
    fn default() -> Self {
        Self {
            default: None,
            surfaces: [None; BorderSurface::COUNT],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct ThemeOptions {
    highlights: BTreeMap<HighlightGroup, HighlightOptions>,
    borders: BorderOptions,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Palette {
    id: &'static str,
    name: &'static str,
    primary: &'static str,
    accent: &'static str,
    warning: &'static str,
    error: &'static str,
    background: &'static str,
    surface: &'static str,
    surface_dark: &'static str,
    text: &'static str,
    text_muted: &'static str,
    border: &'static str,
    sidebar_background: &'static str,
    sidebar_text: &'static str,
    sidebar_text_muted: &'static str,
    rail_background: &'static str,
    selection_background: &'static str,
    selection_foreground: &'static str,
}

fn default_definitions() -> [HighlightDefinition; HighlightGroup::COUNT] {
    use HighlightGroup as H;

    let mut definitions = [HighlightDefinition::default(); HighlightGroup::COUNT];
    let mut set = |group: H, link: Option<H>, style: Style| {
        definitions[group as usize] = HighlightDefinition {
            link,
            style,
            ..HighlightDefinition::default()
        };
    };

    set(H::Normal, None, Style::default());
    set(
        H::Strong,
        None,
        Style::default().add_modifier(Modifier::BOLD),
    );
    set(
        H::Emphasis,
        None,
        Style::default().add_modifier(Modifier::ITALIC),
    );
    set(H::Muted, None, Style::default().fg(Color::DarkGray));
    set(H::Title, Some(H::Strong), Style::default());
    set(H::Border, None, Style::default());
    set(H::FocusBorder, None, Style::default());
    set(
        H::Selection,
        None,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    set(
        H::SelectionBorder,
        None,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    set(H::Error, None, Style::default().fg(Color::Red));
    set(H::Warning, None, Style::default().fg(Color::Yellow));
    set(H::Success, None, Style::default().fg(Color::Green));
    set(H::Info, None, Style::default().fg(Color::Cyan));
    set(H::Pending, Some(H::Warning), Style::default());
    set(H::Rejected, Some(H::Error), Style::default());
    set(H::PaneTitle, Some(H::Title), Style::default());
    set(H::PaneBorder, Some(H::Border), Style::default());
    set(H::FocusedPaneBorder, Some(H::FocusBorder), Style::default());
    set(H::ModalTitle, Some(H::Title), Style::default());
    set(H::ModalBorder, Some(H::FocusBorder), Style::default());
    set(H::CommunityRail, Some(H::Normal), Style::default());
    set(H::CommunitySelected, Some(H::Selection), Style::default());
    set(H::Sidebar, Some(H::Normal), Style::default());
    set(H::SidebarText, Some(H::Normal), Style::default());
    set(H::SidebarMuted, Some(H::Muted), Style::default());
    set(
        H::ChannelUnread,
        Some(H::SidebarText),
        Style::default().add_modifier(Modifier::BOLD),
    );
    set(
        H::SelectedRow,
        None,
        Style::default().bg(Color::Rgb(38, 38, 48)),
    );
    set(H::SelectionMarker, Some(H::Selection), Style::default());
    set(
        H::UnreadNotice,
        Some(H::Warning),
        Style::default().add_modifier(Modifier::BOLD),
    );
    set(
        H::MessageAuthor,
        Some(H::Info),
        Style::default().add_modifier(Modifier::BOLD),
    );
    set(H::MessageTimestamp, Some(H::Muted), Style::default());
    set(H::MessageBody, Some(H::Normal), Style::default());
    set(
        H::MessageDeleted,
        Some(H::Muted),
        Style::default().add_modifier(Modifier::ITALIC),
    );
    set(H::Reaction, None, Style::default().fg(Color::Magenta));
    set(H::SelfReaction, Some(H::Success), Style::default());
    set(H::Composer, Some(H::Normal), Style::default());
    set(H::ComposerTitle, Some(H::Title), Style::default());
    set(H::ComposerBorder, Some(H::Border), Style::default());
    set(
        H::ActiveComposerBorder,
        Some(H::FocusBorder),
        Style::default(),
    );
    set(
        H::StatusBar,
        None,
        Style::default().bg(Color::DarkGray).fg(Color::White),
    );
    set(
        H::StatusMode,
        None,
        Style::default().bg(Color::DarkGray).fg(Color::White),
    );
    set(
        H::StatusModeInsert,
        None,
        Style::default().bg(Color::DarkGray).fg(Color::White),
    );
    set(
        H::StatusModeCommand,
        None,
        Style::default().bg(Color::DarkGray).fg(Color::White),
    );
    set(
        H::MarkdownLink,
        None,
        Style::default().add_modifier(Modifier::UNDERLINED),
    );
    set(
        H::MarkdownCode,
        Some(H::Muted),
        Style::default().add_modifier(Modifier::DIM),
    );
    set(H::MarkdownMarker, Some(H::Muted), Style::default());
    set(H::Placeholder, Some(H::Muted), Style::default());

    definitions
}

fn apply_palette(
    definitions: &mut [HighlightDefinition; HighlightGroup::COUNT],
    palette: &Palette,
) {
    use HighlightGroup as H;

    set_colors(definitions, H::Normal, palette.text, palette.background);
    set_foreground(definitions, H::Muted, palette.text_muted);
    set_foreground(definitions, H::Border, palette.border);
    set_foreground(definitions, H::FocusBorder, palette.primary);
    set_colors(
        definitions,
        H::Selection,
        value_or(palette.selection_foreground, palette.background),
        value_or(palette.selection_background, palette.primary),
    );
    set_foreground(definitions, H::SelectionBorder, palette.accent);
    set_foreground(definitions, H::Error, palette.error);
    set_foreground(definitions, H::Warning, palette.warning);
    set_foreground(definitions, H::Success, palette.accent);
    set_foreground(definitions, H::Info, palette.primary);
    set_foreground(definitions, H::MarkdownLink, palette.primary);
    set_colors(
        definitions,
        H::CommunityRail,
        value_or(palette.sidebar_text, palette.text),
        value_or(palette.rail_background, palette.surface_dark),
    );
    set_colors(
        definitions,
        H::Sidebar,
        value_or(palette.sidebar_text, palette.text),
        value_or(palette.sidebar_background, palette.background),
    );
    set_foreground(
        definitions,
        H::SidebarText,
        value_or(palette.sidebar_text, palette.text),
    );
    set_foreground(
        definitions,
        H::SidebarMuted,
        value_or(palette.sidebar_text_muted, palette.text_muted),
    );
    set_colors(
        definitions,
        H::SelectedRow,
        value_or(palette.selection_foreground, palette.background),
        value_or(palette.selection_background, palette.primary),
    );
    set_colors(definitions, H::Composer, palette.text, palette.surface);
    set_colors(
        definitions,
        H::StatusBar,
        palette.text,
        palette.surface_dark,
    );
    set_colors(
        definitions,
        H::StatusMode,
        palette.background,
        palette.primary,
    );
    set_colors(
        definitions,
        H::StatusModeInsert,
        palette.background,
        palette.accent,
    );
    set_colors(
        definitions,
        H::StatusModeCommand,
        palette.background,
        palette.warning,
    );
}

fn value_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

fn set_foreground(
    definitions: &mut [HighlightDefinition; HighlightGroup::COUNT],
    group: HighlightGroup,
    color: &str,
) {
    if let Some(color) = parse_color(color) {
        definitions[group as usize].style.fg = Some(color);
    }
}

fn set_colors(
    definitions: &mut [HighlightDefinition; HighlightGroup::COUNT],
    group: HighlightGroup,
    foreground: &str,
    background: &str,
) {
    if let Some(color) = parse_color(foreground) {
        definitions[group as usize].style.fg = Some(color);
    }
    if let Some(color) = parse_color(background) {
        definitions[group as usize].style.bg = Some(color);
    }
}

fn apply_overrides(
    definitions: &mut [HighlightDefinition; HighlightGroup::COUNT],
    options: &ThemeOptions,
    warnings: &mut Vec<String>,
) {
    for (&group, options) in &options.highlights {
        let definition = &mut definitions[group as usize];
        if let Some(link) = options.link {
            definition.link = link;
        }
        apply_color(
            &mut definition.style.fg,
            &mut definition.clear_foreground,
            options
                .foreground
                .as_deref()
                .and_then(|value| parse_color_override(group, "foreground", value, warnings)),
        );
        apply_color(
            &mut definition.style.bg,
            &mut definition.clear_background,
            options
                .background
                .as_deref()
                .and_then(|value| parse_color_override(group, "background", value, warnings)),
        );
        for (enabled, modifier) in [
            (options.bold, Modifier::BOLD),
            (options.italic, Modifier::ITALIC),
            (options.dim, Modifier::DIM),
            (options.underline, Modifier::UNDERLINED),
            (options.strikethrough, Modifier::CROSSED_OUT),
        ] {
            if let Some(enabled) = enabled {
                definition.style = if enabled {
                    definition.style.add_modifier(modifier)
                } else {
                    definition.style.remove_modifier(modifier)
                };
            }
        }
    }
}

fn apply_color(channel: &mut Option<Color>, clear: &mut bool, value: Option<ColorOverride>) {
    match value {
        Some(ColorOverride::Set(color)) => {
            *channel = Some(color);
            *clear = false;
        }
        Some(ColorOverride::Clear) => {
            *channel = None;
            *clear = true;
        }
        None => {}
    }
}

fn parse_color_override(
    group: HighlightGroup,
    field: &str,
    value: &str,
    warnings: &mut Vec<String>,
) -> Option<ColorOverride> {
    if value == "none" {
        return Some(ColorOverride::Clear);
    }
    match parse_color(value) {
        Some(color) => Some(ColorOverride::Set(color)),
        None => {
            warnings.push(format!(
                "[highlight.{}] {field} = {value:?} is not a supported color and was ignored",
                group.name()
            ));
            None
        }
    }
}

fn parse_color(value: &str) -> Option<Color> {
    match value {
        "terminal_default" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" => Some(Color::Gray),
        "dark_gray" => Some(Color::DarkGray),
        "light_red" => Some(Color::LightRed),
        "light_green" => Some(Color::LightGreen),
        "light_yellow" => Some(Color::LightYellow),
        "light_blue" => Some(Color::LightBlue),
        "light_magenta" => Some(Color::LightMagenta),
        "light_cyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        raw => parse_hex(raw),
    }
}

fn parse_hex(value: &str) -> Option<Color> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    let channel = |range| u8::from_str_radix(&value[range], 16).ok();
    Some(Color::Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ResolveState {
    Unvisited,
    Visiting,
    Resolved,
}

fn resolve_definitions(
    definitions: &[HighlightDefinition; HighlightGroup::COUNT],
    warnings: &mut Vec<String>,
) -> [ResolvedHighlight; HighlightGroup::COUNT] {
    let mut resolved = [ResolvedHighlight::default(); HighlightGroup::COUNT];
    let mut states = [ResolveState::Unvisited; HighlightGroup::COUNT];
    for &group in HighlightGroup::ALL {
        let _ = resolve_group(group, definitions, &mut resolved, &mut states, warnings);
    }
    resolved
}

fn resolve_group(
    group: HighlightGroup,
    definitions: &[HighlightDefinition; HighlightGroup::COUNT],
    resolved: &mut [ResolvedHighlight; HighlightGroup::COUNT],
    states: &mut [ResolveState; HighlightGroup::COUNT],
    warnings: &mut Vec<String>,
) -> std::result::Result<ResolvedHighlight, ()> {
    let index = group as usize;
    match states[index] {
        ResolveState::Resolved => return Ok(resolved[index]),
        ResolveState::Visiting => {
            warnings.push(format!(
                "[highlight.{}] link cycle detected; cyclic inheritance was ignored",
                group.name()
            ));
            return Err(());
        }
        ResolveState::Unvisited => {}
    }
    states[index] = ResolveState::Visiting;
    let definition = definitions[index];
    let inherited = match definition.link {
        Some(link) => {
            resolve_group(link, definitions, resolved, states, warnings).unwrap_or_default()
        }
        None => ResolvedHighlight::default(),
    };
    let mut value = ResolvedHighlight {
        style: inherited.style.patch(definition.style),
        clear_foreground: inherited.clear_foreground,
        clear_background: inherited.clear_background,
    };
    if definition.style.fg.is_some() {
        value.clear_foreground = false;
    }
    if definition.style.bg.is_some() {
        value.clear_background = false;
    }
    if definition.clear_foreground {
        value.style.fg = None;
        value.clear_foreground = true;
    }
    if definition.clear_background {
        value.style.bg = None;
        value.clear_background = true;
    }
    resolved[index] = value;
    states[index] = ResolveState::Resolved;
    Ok(value)
}

fn resolve_borders(options: &BorderOptions) -> [BorderType; BorderSurface::COUNT] {
    let configured_default = options.default;
    let default = configured_default.unwrap_or(BorderType::Plain);
    std::array::from_fn(|index| options.surfaces[index].unwrap_or(default))
}

fn contrast_warnings(theme: &Theme) -> Vec<String> {
    [
        (HighlightGroup::Normal, 3.0_f64),
        (HighlightGroup::SelectedRow, 3.0_f64),
        (HighlightGroup::StatusBar, 3.0_f64),
    ]
    .into_iter()
    .filter_map(|(group, minimum)| {
        let style = theme.style(group);
        let ratio = contrast_ratio(style.fg?, style.bg?)?;
        (ratio < minimum).then(|| {
            format!(
                "theme {} has {:.2}:1 contrast for {} (minimum {:.1}:1)",
                theme.id(),
                ratio,
                group.name(),
                minimum
            )
        })
    })
    .collect()
}

fn contrast_ratio(foreground: Color, background: Color) -> Option<f64> {
    let foreground = relative_luminance(rgb(foreground)?);
    let background = relative_luminance(rgb(background)?);
    let (lighter, darker) = if foreground > background {
        (foreground, background)
    } else {
        (background, foreground)
    };
    Some((lighter + 0.05) / (darker + 0.05))
}

fn rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(red, green, blue) => Some((red, green, blue)),
        _ => None,
    }
}

fn relative_luminance((red, green, blue): (u8, u8, u8)) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
}

fn export_palette(palette: &Palette) -> String {
    let mut output = format!(
        "# Exported from the built-in {} theme.\n\n[highlight.Normal]\nforeground = {:?}\nbackground = {:?}\n\n[highlight.FocusBorder]\nforeground = {:?}\n\n[highlight.Selection]\nforeground = {:?}\nbackground = {:?}\nbold = true\n\n[highlight.SelectionBorder]\nforeground = {:?}\nbold = true\n\n[highlight.Muted]\nforeground = {:?}\n\n[highlight.Border]\nforeground = {:?}\n\n[highlight.Error]\nforeground = {:?}\n\n[highlight.Warning]\nforeground = {:?}\n\n[highlight.Success]\nforeground = {:?}\n\n[highlight.Info]\nforeground = {:?}\n\n[highlight.StatusBar]\nforeground = {:?}\nbackground = {:?}\n\n[ui.border]\ndefault = \"plain\"\n",
        palette.name,
        palette.text,
        palette.background,
        palette.primary,
        value_or(palette.selection_foreground, palette.background),
        value_or(palette.selection_background, palette.primary),
        palette.accent,
        palette.text_muted,
        palette.border,
        palette.error,
        palette.warning,
        palette.accent,
        palette.primary,
        palette.text,
        palette.surface_dark,
    );
    if palette.id == DEFAULT_THEME_ID {
        output = output.replace("\"\"", "\"terminal_default\"");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::style::{Color, Modifier, Style};
    use tempfile::TempDir;

    use super::{
        BorderSurface, HighlightGroup, Theme, ThemeRegistry, export_to, load, parse::parse,
    };
    use crate::paths::Paths;

    fn paths(temporary: &TempDir) -> Paths {
        Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        }
    }

    #[test]
    fn registry_has_bzz_plus_all_audited_slk_palettes() {
        let entries = ThemeRegistry::entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 60);
        assert_eq!(entries[0].id, "bzz");
        let unique = entries
            .iter()
            .map(|entry| entry.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), entries.len());
        for entry in entries {
            assert_eq!(ThemeRegistry::canonical_id(entry.name), Some(entry.id));
            let theme = Theme::builtin(entry.id).unwrap();
            assert!(
                super::contrast_warnings(&theme).is_empty(),
                "{}: {:?}",
                entry.id,
                super::contrast_warnings(&theme)
            );
        }
    }

    #[test]
    fn bzz_default_keeps_plain_borders_and_original_focus_color() {
        let theme = Theme::default();
        for surface in [
            BorderSurface::Pane,
            BorderSurface::Composer,
            BorderSurface::Modal,
            BorderSurface::Picker,
            BorderSurface::Message,
        ] {
            assert_eq!(
                theme.border_type(surface),
                ratatui::widgets::BorderType::Plain
            );
        }
        assert_eq!(theme.style(HighlightGroup::FocusedPaneBorder).fg, None);
        assert_eq!(
            theme.style(HighlightGroup::StatusBar),
            theme.style(HighlightGroup::StatusMode)
        );
    }

    #[test]
    fn parser_resolves_links_clears_channels_and_ignores_one_bad_leaf() {
        let (options, parser_warnings) = parse(
            r##"
[highlight.Info]
foreground = "#112233"

[highlight.MessageAuthor]
link = "Info"
bold = false
background = "none"
future = true

[ui.border]
default = "double"
modal = "thick"
"##,
        )
        .unwrap();
        assert_eq!(parser_warnings.len(), 1);
        let palette = super::builtin::lookup("bzz").unwrap();
        let mut warnings = parser_warnings;
        let theme = Theme::from_palette(palette, &options, &mut warnings);
        let author = theme.apply(
            HighlightGroup::MessageAuthor,
            Style::default().bg(Color::Red),
        );
        assert_eq!(author.fg, Some(Color::Rgb(0x11, 0x22, 0x33)));
        assert_eq!(author.bg, None);
        assert!(!author.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            theme.border_type(BorderSurface::Pane),
            ratatui::widgets::BorderType::Double
        );
        assert_eq!(
            theme.border_type(BorderSurface::Modal),
            ratatui::widgets::BorderType::Thick
        );
    }

    #[test]
    fn cyclic_links_warn_and_keep_the_theme_usable() {
        let (options, mut warnings) = parse(
            r#"
[highlight.Info]
link = "Warning"
foreground = "cyan"

[highlight.Warning]
link = "Info"
foreground = "yellow"
"#,
        )
        .unwrap();
        let palette = super::builtin::lookup("bzz").unwrap();
        let theme = Theme::from_palette(palette, &options, &mut warnings);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("link cycle"))
        );
        assert!(theme.style(HighlightGroup::Info).fg.is_some());
        assert!(theme.style(HighlightGroup::Warning).fg.is_some());
    }

    #[test]
    fn invalid_theme_does_not_modify_or_create_files() {
        let temporary = TempDir::new().unwrap();
        let paths = paths(&temporary);
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::write(paths.theme_file(), "[highlight.Normal\n").unwrap();
        assert!(load(&paths, "bzz").is_err());
        assert_eq!(
            fs::read_to_string(paths.theme_file()).unwrap(),
            "[highlight.Normal\n"
        );
    }

    #[test]
    fn export_is_owner_only_and_refuses_overwrite() {
        let temporary = TempDir::new().unwrap();
        let output = temporary.path().join("nord.toml");
        export_to("nord", &output).unwrap();
        let before = fs::read(&output).unwrap();
        assert!(export_to("nord", &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
