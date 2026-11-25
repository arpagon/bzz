//! The workspace shell: which panes are drawn, and where.
//!
//! Columns are drawn in this order: community rail, channel sidebar,
//! conversation, thread, app. While anything else is on screen the
//! conversation keeps [`MIN_CONVERSATION`] columns; the other panes are
//! admitted while there is room for them, in this order, so the lowest
//! drops first as the window narrows and comes back as soon as it widens:
//!
//! 0. the pane with the focus, whatever it is: a pane the owner went to is
//!    never left undrawn. If it cannot sit beside the conversation even at
//!    its least width it takes the conversation's place (an app takes the
//!    whole window, as in v0.16);
//! 1. the pane of the right side that was asked for last (the thread or the
//!    app);
//! 2. the channel sidebar;
//! 3. the other pane of the right side;
//! 4. the community rail, and only when every pane before it that is asked
//!    for is drawn (v0.16 drew it in the room a thread did not fit in, and
//!    where the sidebar would not fit even alone). With one community the
//!    rail only shows from [`SINGLE_COMMUNITY_RAIL_FROM`] columns.
//!
//! The pane with the focus and the pane of the right side asked for last
//! narrow to their least width (the sidebar to 18, the thread to 30) before
//! they give way; the others keep their widths. A pane that is asked for
//! and not drawn is only hidden: [`Hidden`] says which, and the conversation's
//! frame names it with the key that shows it.
//!
//! [`Shell::conversation_only`] (`z`) draws the conversation and the writing
//! dock alone.
//!
//! The table of what shows at each width is in `docs/validation-v0.17.0/layout.md`
//! and pinned by `the_table_of_breakpoints`. What changed from v0.16:
//! the conversation keeps 38 columns instead of 30, so the app docks from
//! 80 columns (was 72), the thread from 68 at 30 columns wide (was 74 at its
//! own width), thread and app together from 124 (was 116), with the sidebar
//! from 152 (was 144). The rule for the app pane comes from M0's trial
//! (`docs/adr-v0.16-apps.md`, "The pane and its keys"): its 42 columns, its
//! 16 rows, and zoom while focused where it cannot dock all hold.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// The conversation's columns while anything else is on screen.
pub const MIN_CONVERSATION: u16 = 38;
/// v0.16's name for [`MIN_CONVERSATION`].
pub const MIN_TIMELINE_WIDTH: u16 = MIN_CONVERSATION;
pub const COMMUNITY_WIDTH: u16 = 22;
/// The least width of the thread pane (`ui.thread_width`'s least value).
pub const THREAD_MIN_WIDTH: u16 = 30;
/// The least width of the sidebar (`ui.sidebar_width`'s least value).
pub const SIDEBAR_MIN_WIDTH: u16 = 18;
/// With one community its rail shows only from this many columns.
pub const SINGLE_COMMUNITY_RAIL_FROM: u16 = 100;

/// The region an app is promised: under it bzz says so and the script's
/// `view` is not called.
pub const APP_MIN_COLUMNS: u16 = 40;
pub const APP_MIN_ROWS: u16 = 16;
/// The app pane is bordered like every pane of the workspace.
pub const APP_BORDER: u16 = 2;
/// The outer width of the docked app pane.
pub const APP_PANE_WIDTH: u16 = APP_MIN_COLUMNS + APP_BORDER;

/// What the caller asks of the layout for the app pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppRequest {
    /// An app is open in the selected channel.
    pub open: bool,
    /// The app has the keyboard.
    pub focused: bool,
    /// The owner asked for the whole window while the app has the focus.
    pub zoom: bool,
    /// Of the two panes of the right side, the app was asked for last.
    pub front: bool,
}

impl AppRequest {
    pub const CLOSED: Self = Self {
        open: false,
        focused: false,
        zoom: false,
        front: false,
    };
}

/// Where the app pane ended up: for the status line, the focus order and
/// the pane's own words.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppPlacement {
    /// No app is open in the selected channel.
    #[default]
    Closed,
    /// Beside the conversation.
    Docked,
    /// The whole window above the status line.
    Zoomed,
    /// Open and running, not drawn: there is no room beside the
    /// conversation, and the app does not have the focus.
    HiddenNarrow,
    /// Open and running, not drawn: the thread has the right side.
    HiddenBehindThread,
}

impl AppPlacement {
    pub const fn is_drawn(self) -> bool {
        matches!(self, Self::Docked | Self::Zoomed)
    }
}

/// A pane of the workspace other than the conversation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidePane {
    #[default]
    None,
    Communities,
    Channels,
    Thread,
    App,
}

/// Panes asked for and not drawn: hidden, never closed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Hidden {
    pub communities: bool,
    pub channels: bool,
    /// The pane with the focus took the conversation's place.
    pub conversation: bool,
    pub thread: bool,
    pub app: bool,
    /// `z`: only the conversation is drawn.
    pub conversation_only: bool,
}

impl Hidden {
    /// The key that shows each hidden pane, and its name, in the order of
    /// the columns. The app is left out: the status line says it
    /// (`app hidden · 5`).
    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.conversation_only {
            return vec![("z", "the other panes")];
        }
        let mut keys = Vec::new();
        if self.communities {
            keys.push(("1", "communities"));
        }
        if self.channels {
            keys.push(("2", "channels"));
        }
        if self.conversation {
            keys.push(("3", "conversation"));
        }
        if self.thread {
            keys.push(("4", "thread"));
        }
        keys
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Panes {
    pub community: Option<Rect>,
    pub sidebar: Option<Rect>,
    /// Empty when the pane with the focus took its place.
    pub timeline: Rect,
    pub thread: Option<Rect>,
    /// The app pane, docked or zoomed, with its border.
    pub app: Option<Rect>,
    pub app_placement: AppPlacement,
    /// A workspace-local writing dock. It is measured with the rest of the
    /// shell so it can never overlap messages or invalidate pointer geometry.
    pub composer: Option<Rect>,
    pub status: Rect,
    pub hidden: Hidden,
}

/// Everything the layout is asked: which panes are open, their widths, the
/// focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shell {
    /// The community rail is switched on.
    pub community: bool,
    /// How many communities there are: with one, the rail waits for
    /// [`SINGLE_COMMUNITY_RAIL_FROM`] columns.
    pub communities: usize,
    pub sidebar: bool,
    /// A thread is open.
    pub thread: bool,
    pub sidebar_width: u16,
    pub thread_width: u16,
    pub composer_height: u16,
    pub app: AppRequest,
    /// The pane with the focus, if it is not the conversation.
    pub focus: SidePane,
    /// `z`: the conversation and the writing dock alone.
    pub conversation_only: bool,
}

impl Shell {
    /// The rail, the sidebar, no thread, no app, the default widths.
    pub const fn new(sidebar_width: u16, thread_width: u16) -> Self {
        Self {
            community: true,
            communities: 2,
            sidebar: true,
            thread: false,
            sidebar_width,
            thread_width,
            composer_height: 0,
            app: AppRequest::CLOSED,
            focus: SidePane::None,
            conversation_only: false,
        }
    }
}

/// Resolves the ordinary workspace without reserving a writing dock.
///
/// Kept as the compact layout entry point for pure callers and tests. The app
/// uses [`workspace`] so the visible composer is part of the same
/// render/hit-map generation as every other surface.
pub fn panes(
    area: Rect,
    community: bool,
    sidebar: bool,
    thread: bool,
    sidebar_width: u16,
    thread_width: u16,
) -> Panes {
    panes_with_composer(
        area,
        community,
        sidebar,
        thread,
        sidebar_width,
        thread_width,
        0,
    )
}

/// Resolves the workspace shell with no app open, reserving
/// `composer_height` terminal rows above the one-row status line. A
/// caller-controlled height is clamped to the available area so a small
/// terminal remains recoverable.
pub fn panes_with_composer(
    area: Rect,
    community: bool,
    sidebar: bool,
    thread: bool,
    sidebar_width: u16,
    thread_width: u16,
    composer_height: u16,
) -> Panes {
    panes_with_app(
        area,
        community,
        sidebar,
        thread,
        sidebar_width,
        thread_width,
        composer_height,
        AppRequest::CLOSED,
    )
}

/// v0.16's entry point: several communities, the focus on the app if it has
/// it and otherwise on the conversation.
#[allow(clippy::too_many_arguments)]
pub fn panes_with_app(
    area: Rect,
    community: bool,
    sidebar: bool,
    thread: bool,
    sidebar_width: u16,
    thread_width: u16,
    composer_height: u16,
    app: AppRequest,
) -> Panes {
    workspace(
        area,
        Shell {
            community,
            sidebar,
            thread,
            composer_height,
            app,
            focus: if app.open && app.focused {
                SidePane::App
            } else {
                SidePane::None
            },
            ..Shell::new(sidebar_width, thread_width)
        },
    )
}

/// The workspace shell. See the module's documentation for the order of
/// admission. Besides it:
///
/// - While the app has the focus it takes the whole window above the status
///   line (the writing dock too) when zoom is asked for, when the window is
///   too narrow to dock it, or when docked it would have fewer than
///   [`APP_MIN_ROWS`] rows and zoomed it would not.
/// - An app that is neither drawn nor focused keeps running; the placement
///   says why it is not drawn.
pub fn workspace(area: Rect, shell: Shell) -> Panes {
    let app = shell.app;
    let composer_height = shell.composer_height.min(area.height.saturating_sub(2));
    let mut vertical_constraints = vec![Constraint::Min(1)];
    if composer_height > 0 {
        vertical_constraints.push(Constraint::Length(composer_height));
    }
    vertical_constraints.push(Constraint::Length(1));
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vertical_constraints)
        .split(area);
    let body = vertical[0];
    let composer = (composer_height > 0).then(|| vertical[1]);
    let status = *vertical.last().unwrap_or(&area);

    let app_width = APP_PANE_WIDTH;
    // Zoom: everything above the status line, the writing dock included.
    let zoomed = Rect::new(area.x, area.y, area.width, status.y.saturating_sub(area.y));
    let docks_wide = body.width >= MIN_CONVERSATION.saturating_add(app_width);
    let docks_tall = body.height >= APP_MIN_ROWS + APP_BORDER;
    let must_zoom =
        !docks_wide || (!docks_tall && zoomed.height >= APP_MIN_ROWS.saturating_add(APP_BORDER));
    if app.open && app.focused && (app.zoom || must_zoom) {
        return Panes {
            community: None,
            sidebar: None,
            timeline: Rect::new(area.x, area.y, 0, 0),
            thread: None,
            app: Some(zoomed),
            app_placement: AppPlacement::Zoomed,
            composer: None,
            status,
            hidden: Hidden::default(),
        };
    }
    if shell.conversation_only {
        return Panes {
            community: None,
            sidebar: None,
            timeline: body,
            thread: None,
            app: None,
            app_placement: if app.open {
                AppPlacement::HiddenNarrow
            } else {
                AppPlacement::Closed
            },
            composer,
            status,
            hidden: Hidden {
                conversation_only: true,
                ..Hidden::default()
            },
        };
    }

    let focus = shell.focus;
    let rail_wanted = shell.community
        && (shell.communities > 1
            || body.width >= SINGLE_COMMUNITY_RAIL_FROM
            || focus == SidePane::Communities);
    let asked = |pane: SidePane| match pane {
        SidePane::None => false,
        SidePane::Communities => rail_wanted,
        SidePane::Channels => shell.sidebar,
        SidePane::Thread => shell.thread,
        SidePane::App => app.open,
    };
    let width_of = |pane: SidePane| match pane {
        SidePane::None => 0,
        SidePane::Communities => COMMUNITY_WIDTH,
        SidePane::Channels => shell.sidebar_width,
        SidePane::Thread => shell.thread_width,
        SidePane::App => app_width,
    };
    let least_of = |pane: SidePane| match pane {
        SidePane::Channels => SIDEBAR_MIN_WIDTH.min(shell.sidebar_width),
        SidePane::Thread => THREAD_MIN_WIDTH.min(shell.thread_width),
        other => width_of(other),
    };
    // The pane of the right side asked for last; the focused app is always
    // in front.
    let app_first = app.open && (app.front || app.focused || !shell.thread);
    let (first_right, second_right) = if app_first {
        (SidePane::App, SidePane::Thread)
    } else {
        (SidePane::Thread, SidePane::App)
    };

    let mut widths = [0_u16; 5];
    let slot = |pane: SidePane| match pane {
        SidePane::None => 0,
        SidePane::Communities => 1,
        SidePane::Channels => 2,
        SidePane::Thread => 3,
        SidePane::App => 4,
    };
    let mut used = MIN_CONVERSATION;
    let mut replaced = false;
    // Admit `pane` at its width or, when `may_narrow` and nothing is beside
    // the conversation yet, at what is left down to its least width.
    let admit = |pane: SidePane, may_narrow: bool, widths: &mut [u16; 5], used: &mut u16| {
        if !asked(pane) || widths[slot(pane)] > 0 {
            return widths[slot(pane)] > 0;
        }
        let wanted = width_of(pane);
        let room = body.width.saturating_sub(*used);
        let width = if room >= wanted {
            wanted
        } else if may_narrow
            && *used == MIN_CONVERSATION
            && room >= least_of(pane)
            && pane != SidePane::App
        {
            room
        } else {
            return false;
        };
        widths[slot(pane)] = width;
        *used = used.saturating_add(width);
        true
    };
    // 0. The pane with the focus.
    if asked(focus) && !admit(focus, true, &mut widths, &mut used) && focus != SidePane::App {
        // It takes the conversation's place.
        replaced = true;
    }
    if !replaced {
        // 1. The pane of the right side asked for last; if it does not fit,
        // the other.
        if !admit(first_right, true, &mut widths, &mut used) {
            admit(second_right, true, &mut widths, &mut used);
        }
        // 2. The sidebar.
        admit(SidePane::Channels, false, &mut widths, &mut used);
        // 3. The other pane of the right side.
        admit(second_right, false, &mut widths, &mut used);
        // 4. The rail, last: never in the room a pane before it did not
        // fit in (communities without their channels help nobody), except
        // an app, which cannot dock at all below its width.
        let gave_way = |pane: SidePane| asked(pane) && widths[slot(pane)] == 0;
        let before_gave_way = gave_way(SidePane::Channels)
            || gave_way(SidePane::Thread)
            || (gave_way(SidePane::App) && docks_wide);
        if !before_gave_way || focus == SidePane::Communities {
            admit(SidePane::Communities, false, &mut widths, &mut used);
        }
    }
    let shown = |pane: SidePane| widths[slot(pane)] > 0;
    let show_app = shown(SidePane::App);
    let app_placement = if !app.open {
        AppPlacement::Closed
    } else if show_app {
        AppPlacement::Docked
    } else if !docks_wide || replaced {
        AppPlacement::HiddenNarrow
    } else {
        AppPlacement::HiddenBehindThread
    };
    let hidden = Hidden {
        communities: rail_wanted && !shown(SidePane::Communities) && focus != SidePane::Communities,
        channels: shell.sidebar && !shown(SidePane::Channels) && focus != SidePane::Channels,
        conversation: replaced,
        thread: shell.thread && !shown(SidePane::Thread) && focus != SidePane::Thread,
        app: app.open && !show_app,
        conversation_only: false,
    };
    if replaced {
        let at = |side: SidePane| (focus == side).then_some(body);
        return Panes {
            community: at(SidePane::Communities),
            sidebar: at(SidePane::Channels),
            timeline: Rect::new(body.x, body.y, 0, body.height),
            thread: at(SidePane::Thread),
            app: None,
            app_placement,
            composer,
            status,
            hidden,
        };
    }
    let mut constraints = vec![];
    for pane in [SidePane::Communities, SidePane::Channels] {
        if shown(pane) {
            constraints.push(Constraint::Length(widths[slot(pane)]));
        }
    }
    constraints.push(Constraint::Min(MIN_CONVERSATION.min(body.width)));
    for pane in [SidePane::Thread, SidePane::App] {
        if shown(pane) {
            constraints.push(Constraint::Length(widths[slot(pane)]));
        }
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(body);
    let mut index = 0;
    let mut next = || {
        let rect = columns[index];
        index += 1;
        rect
    };
    let community = shown(SidePane::Communities).then(&mut next);
    let sidebar = shown(SidePane::Channels).then(&mut next);
    let timeline = next();
    let thread = shown(SidePane::Thread).then(&mut next);
    let app = show_app.then(&mut next);
    Panes {
        community,
        sidebar,
        timeline,
        thread,
        app,
        app_placement,
        composer,
        status,
        hidden,
    }
}

/// The region inside a pane's border.
pub const fn inside(pane: Rect) -> Rect {
    Rect::new(
        pane.x.saturating_add(1),
        pane.y.saturating_add(1),
        pane.width.saturating_sub(APP_BORDER),
        pane.height.saturating_sub(APP_BORDER),
    )
}

/// Whether a region is large enough for an app.
pub const fn fits_an_app(region: Rect) -> bool {
    region.width >= APP_MIN_COLUMNS && region.height >= APP_MIN_ROWS
}

#[cfg(test)]
mod tests {
    use super::{
        APP_BORDER, APP_PANE_WIDTH, AppPlacement, AppRequest, COMMUNITY_WIDTH, MIN_CONVERSATION,
        Panes, Shell, SidePane, fits_an_app, inside, panes_with_app, panes_with_composer,
        workspace,
    };
    use ratatui::layout::Rect;

    fn with_app(
        width: u16,
        height: u16,
        community: bool,
        sidebar: bool,
        thread: bool,
        app: AppRequest,
    ) -> Panes {
        panes_with_app(
            Rect::new(0, 0, width, height),
            community,
            sidebar,
            thread,
            28,
            44,
            3,
            app,
        )
    }

    fn open(focused: bool, zoom: bool, front: bool) -> AppRequest {
        AppRequest {
            open: true,
            focused,
            zoom,
            front,
        }
    }

    /// What is drawn, left to right, with its width: `R` the rail, `S` the
    /// sidebar, `C` the conversation, `T` the thread, `A` the app; then the
    /// keys of what is hidden.
    fn describe(panes: &Panes) -> String {
        if panes.app_placement == AppPlacement::Zoomed {
            return "A zoomed".into();
        }
        let mut parts = Vec::new();
        for (letter, pane) in [
            ("R", panes.community),
            ("S", panes.sidebar),
            ("C", Some(panes.timeline).filter(|pane| pane.width > 0)),
            ("T", panes.thread),
            ("A", panes.app),
        ] {
            if let Some(pane) = pane {
                parts.push(format!("{letter}{}", pane.width));
            }
        }
        let hidden = panes
            .hidden
            .keys()
            .iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>()
            .join("");
        if hidden.is_empty() {
            parts.join(" ")
        } else {
            format!("{} | {hidden}", parts.join(" "))
        }
    }

    /// The sets of open panes of the table, with the default widths.
    fn request(name: &str) -> Shell {
        let base = Shell {
            composer_height: 3,
            ..Shell::new(28, 44)
        };
        let thread = Shell {
            thread: true,
            focus: SidePane::Thread,
            ..base
        };
        let app = Shell {
            app: open(false, false, true),
            ..base
        };
        match name {
            "sidebar" => base,
            "one community" => Shell {
                communities: 1,
                ..base
            },
            "thread" => thread,
            "thread, focus back" => Shell {
                focus: SidePane::None,
                ..thread
            },
            "thread, focus channels" => Shell {
                focus: SidePane::Channels,
                ..thread
            },
            "app" => app,
            "app focused" => Shell {
                app: open(true, false, true),
                focus: SidePane::App,
                ..base
            },
            "thread and app" => Shell {
                thread: true,
                ..app
            },
            "app and thread" => Shell {
                thread: true,
                focus: SidePane::Thread,
                app: open(false, false, false),
                ..base
            },
            "z" => Shell {
                thread: true,
                conversation_only: true,
                ..app
            },
            other => panic!("{other}"),
        }
    }

    const WIDTHS: [u16; 7] = [60, 72, 80, 100, 120, 160, 200];

    /// The whole table of breakpoints (docs/validation-v0.17.0/layout.md),
    /// at 24 rows with a writing dock of 3. Each line: the set of open
    /// panes, then what is drawn at 60, 72, 80, 100, 120, 160 and 200
    /// columns.
    const TABLE: &str = "\
sidebar: C60 | 12 · S28 C44 | 1 · S28 C52 | 1 · R22 S28 C50 · R22 S28 C70 · R22 S28 C110 · R22 S28 C150
one community: C60 | 2 · S28 C44 · S28 C52 · R22 S28 C50 · R22 S28 C70 · R22 S28 C110 · R22 S28 C150
thread: T60 | 123 · C38 T34 | 12 · C38 T42 | 12 · C56 T44 | 12 · S28 C48 T44 | 1 · R22 S28 C66 T44 · R22 S28 C106 T44
thread, focus back: C60 | 124 · C38 T34 | 12 · C38 T42 | 12 · C56 T44 | 12 · S28 C48 T44 | 1 · R22 S28 C66 T44 · R22 S28 C106 T44
thread, focus channels: S22 C38 | 14 · S28 C44 | 14 · S28 C52 | 14 · S28 C72 | 14 · S28 C48 T44 | 1 · R22 S28 C66 T44 · R22 S28 C106 T44
app: C60 | 12 · S28 C44 | 1 · C38 A42 | 12 · C58 A42 | 12 · S28 C50 A42 | 1 · R22 S28 C68 A42 · R22 S28 C108 A42
app focused: A zoomed · A zoomed · C38 A42 | 12 · C58 A42 | 12 · S28 C50 A42 | 1 · R22 S28 C68 A42 · R22 S28 C108 A42
thread and app: C60 | 124 · C38 T34 | 12 · C38 A42 | 124 · C58 A42 | 124 · S28 C50 A42 | 14 · S28 C46 T44 A42 | 1 · R22 S28 C64 T44 A42
app and thread: T60 | 123 · C38 T34 | 12 · C38 T42 | 12 · C56 T44 | 12 · S28 C48 T44 | 1 · S28 C46 T44 A42 | 1 · R22 S28 C64 T44 A42
z: C60 | z · C72 | z · C80 | z · C100 | z · C120 | z · C160 | z · C200 | z
";

    #[test]
    fn the_table_of_breakpoints() {
        let mut drawn = String::new();
        for line in TABLE.lines() {
            let (name, _) = line.split_once(": ").unwrap();
            let row = WIDTHS
                .iter()
                .map(|width| describe(&workspace(Rect::new(0, 0, *width, 24), request(name))))
                .collect::<Vec<_>>()
                .join(" · ");
            drawn.push_str(&format!("{name}: {row}\n"));
        }
        assert_eq!(drawn, TABLE, "\n{drawn}");
    }

    fn shells() -> Vec<(String, Shell)> {
        let mut shells = Vec::new();
        for flags in 0..32_u8 {
            for focus in [
                SidePane::None,
                SidePane::Communities,
                SidePane::Channels,
                SidePane::Thread,
                SidePane::App,
            ] {
                for communities in [1, 3] {
                    for (sidebar_width, thread_width) in [(28, 44), (18, 30), (60, 80)] {
                        let app_focused = focus == SidePane::App;
                        let shell = Shell {
                            community: flags & 1 != 0,
                            communities,
                            sidebar: flags & 2 != 0,
                            thread: flags & 4 != 0,
                            sidebar_width,
                            thread_width,
                            composer_height: 3,
                            app: AppRequest {
                                open: flags & 8 != 0 || app_focused,
                                focused: app_focused,
                                zoom: false,
                                front: flags & 16 != 0,
                            },
                            focus,
                            conversation_only: false,
                        };
                        let label = format!(
                            "flags {flags:05b} focus {focus:?} communities {communities} widths {sidebar_width}/{thread_width}"
                        );
                        shells.push((label, shell));
                    }
                }
            }
        }
        shells
    }

    #[test]
    fn at_every_width_the_panes_fill_the_body_and_the_conversation_keeps_its_columns() {
        for width in 50..=220 {
            for (label, shell) in shells() {
                let panes = workspace(Rect::new(0, 0, width, 24), shell);
                let label = format!("{width}: {label}: {}", describe(&panes));
                if panes.app_placement == AppPlacement::Zoomed {
                    assert!(shell.app.focused, "{label}");
                    continue;
                }
                let columns = [
                    panes.community,
                    panes.sidebar,
                    Some(panes.timeline).filter(|pane| pane.width > 0),
                    panes.thread,
                    panes.app,
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                let mut x = 0;
                for column in &columns {
                    assert_eq!(column.x, x, "{label}: adjacent, in order");
                    x += column.width;
                }
                assert_eq!(x, width, "{label}: the columns fill the body");
                if panes.timeline.width > 0 && columns.len() > 1 {
                    assert!(panes.timeline.width >= MIN_CONVERSATION, "{label}");
                }
                if panes.timeline.width == 0 {
                    assert_eq!(columns.len(), 1, "{label}: one pane in its place");
                    assert!(panes.hidden.conversation, "{label}");
                }
                // The pane with the focus is drawn.
                let drawn = match shell.focus {
                    SidePane::None => true,
                    SidePane::Communities => !shell.community || panes.community.is_some(),
                    SidePane::Channels => !shell.sidebar || panes.sidebar.is_some(),
                    SidePane::Thread => !shell.thread || panes.thread.is_some(),
                    SidePane::App => panes.app.is_some(),
                };
                assert!(drawn, "{label}: the focused pane is drawn");
                // What is asked for is drawn or said to be hidden.
                assert_eq!(
                    shell.sidebar && panes.sidebar.is_none(),
                    panes.hidden.channels,
                    "{label}"
                );
                assert_eq!(
                    shell.thread && panes.thread.is_none(),
                    panes.hidden.thread,
                    "{label}"
                );
                assert_eq!(
                    shell.app.open && panes.app.is_none(),
                    panes.hidden.app,
                    "{label}"
                );
                if shell.community && panes.community.is_none() && !panes.hidden.communities {
                    // Only the one-community rule hides the rail unsaid.
                    assert!(
                        shell.communities == 1 && width < 100,
                        "{label}: the rail is hidden unsaid"
                    );
                }
                // No rail beside a sidebar that gave way.
                if panes.community.is_some()
                    && shell.sidebar
                    && shell.focus != SidePane::Communities
                {
                    assert!(panes.sidebar.is_some(), "{label}");
                }
                if let Some(pane) = panes.app {
                    assert_eq!(panes.app_placement, AppPlacement::Docked, "{label}");
                    assert_eq!(pane.width, APP_PANE_WIDTH, "{label}");
                }
            }
        }
    }

    /// What drops as the window narrows comes back as it widens: widening
    /// never draws fewer panes, and a pane drops only for one before it in
    /// the order (with no app, whose place follows the order it was asked
    /// in). At 220 columns every pane asked for is drawn.
    #[test]
    fn what_drops_comes_back_when_the_window_widens() {
        for (label, shell) in shells() {
            if shell.app.open {
                continue;
            }
            let drawn = |panes: &Panes| {
                [panes.community, panes.sidebar, panes.thread]
                    .map(|pane| pane.is_some() && panes.timeline.width > 0)
            };
            let mut before: Option<[bool; 3]> = None;
            for width in 50..=220 {
                let now = drawn(&workspace(Rect::new(0, 0, width, 24), shell));
                if let Some(before) = before {
                    let count = |set: [bool; 3]| set.iter().filter(|shown| **shown).count();
                    assert!(count(now) >= count(before), "{width}: {label}");
                    for (index, name) in ["rail", "sidebar", "thread"].iter().enumerate() {
                        if before[index] && !now[index] {
                            // Something before it in the order came in.
                            let came =
                                (0..3).any(|other| other != index && now[other] && !before[other]);
                            assert!(came, "{width}: {label}: the {name} dropped for nothing");
                        }
                    }
                }
                before = Some(now);
            }
            let wide = workspace(Rect::new(0, 0, 220, 24), shell);
            assert_eq!(wide.hidden, super::Hidden::default(), "{label}");
        }
    }

    #[test]
    fn a_thread_asked_for_is_drawn_from_68_columns_and_takes_the_place_below() {
        let thread = |width| {
            workspace(
                Rect::new(0, 0, width, 24),
                Shell {
                    thread: true,
                    focus: SidePane::Thread,
                    composer_height: 3,
                    ..Shell::new(28, 44)
                },
            )
        };
        for width in 68..=81 {
            let panes = thread(width);
            assert_eq!(panes.thread.unwrap().width, width - 38, "{width}");
            assert_eq!(panes.timeline.width, 38, "{width}");
        }
        for width in 82..=109 {
            let panes = thread(width);
            assert_eq!(panes.thread.unwrap().width, 44, "{width}");
            assert!(panes.sidebar.is_none() && panes.hidden.channels, "{width}");
        }
        assert!(thread(110).sidebar.is_some());
        // Below 68 it takes the conversation's place while it has the focus;
        // `3` gives the focus back and the conversation is drawn again.
        let narrow = thread(60);
        assert_eq!(narrow.thread, Some(Rect::new(0, 0, 60, 20)));
        assert_eq!(narrow.timeline.width, 0);
        assert_eq!(
            narrow.hidden.keys(),
            vec![
                ("1", "communities"),
                ("2", "channels"),
                ("3", "conversation")
            ]
        );
        let back = workspace(
            Rect::new(0, 0, 60, 24),
            Shell {
                thread: true,
                composer_height: 3,
                ..Shell::new(28, 44)
            },
        );
        assert_eq!(back.timeline.width, 60);
        assert!(back.thread.is_none() && back.hidden.thread);
    }

    #[test]
    fn the_rail_of_one_community_waits_for_100_columns_and_for_its_key() {
        let one = |width, focus| {
            workspace(
                Rect::new(0, 0, width, 24),
                Shell {
                    communities: 1,
                    focus,
                    ..Shell::new(28, 44)
                },
            )
        };
        assert!(one(99, SidePane::None).community.is_none());
        assert!(!one(99, SidePane::None).hidden.communities);
        assert_eq!(
            one(100, SidePane::None).community.map(|rail| rail.width),
            Some(COMMUNITY_WIDTH)
        );
        // `1` shows it anyway.
        assert!(one(80, SidePane::Communities).community.is_some());
    }

    #[test]
    fn conversation_only_draws_the_conversation_and_the_dock() {
        let panes = workspace(
            Rect::new(0, 0, 120, 30),
            Shell {
                thread: true,
                app: open(false, false, true),
                composer_height: 4,
                conversation_only: true,
                ..Shell::new(28, 44)
            },
        );
        assert_eq!(panes.timeline, Rect::new(0, 0, 120, 25));
        assert!(panes.community.is_none() && panes.sidebar.is_none());
        assert!(panes.thread.is_none() && panes.app.is_none());
        assert_eq!(panes.composer, Some(Rect::new(0, 25, 120, 4)));
        assert_eq!(panes.hidden.keys(), vec![("z", "the other panes")]);
    }

    #[test]
    fn the_thresholds_of_the_default_widths() {
        let docked = |width, community, sidebar| {
            with_app(
                width,
                24,
                community,
                sidebar,
                false,
                open(false, false, true),
            )
        };
        // conversation 38 + app 42 (v0.16: 30 + 42).
        assert_eq!(docked(79, false, false).app, None);
        assert_eq!(
            docked(79, false, false).app_placement,
            AppPlacement::HiddenNarrow
        );
        assert!(docked(80, false, false).app.is_some());
        // + sidebar 28: at 107 the app is drawn and the sidebar is not.
        let narrow = docked(107, true, true);
        assert!(narrow.app.is_some() && narrow.sidebar.is_none() && narrow.community.is_none());
        let sidebar = docked(108, true, true);
        assert!(sidebar.app.is_some() && sidebar.sidebar.is_some() && sidebar.community.is_none());
        // + rail 22
        assert!(docked(129, true, true).community.is_none());
        assert!(docked(130, true, true).community.is_some());
        // Thread and app together: 124 with no sidebar, 152 with it, 174 with the rail.
        let both = |width, community, sidebar, front| {
            let panes = with_app(
                width,
                24,
                community,
                sidebar,
                true,
                open(false, false, front),
            );
            (panes.thread.is_some(), panes.app.is_some())
        };
        assert_eq!(both(123, false, false, true), (false, true));
        assert_eq!(both(123, false, false, false), (true, false));
        assert_eq!(both(124, false, false, true), (true, true));
        assert_eq!(both(151, true, true, true), (false, true));
        assert_eq!(both(151, true, true, false), (true, false));
        assert_eq!(both(152, true, true, false), (true, true));
        assert_eq!(both(152, true, true, true), (true, true));
        let all = with_app(173, 24, true, true, true, open(false, false, true));
        assert!(all.community.is_none() && all.thread.is_some() && all.app.is_some());
        let all = with_app(174, 24, true, true, true, open(false, false, true));
        assert!(all.community.is_some() && all.thread.is_some() && all.app.is_some());
        // The one asked for last wins the right side; the other waits.
        let behind = with_app(120, 24, true, true, true, open(false, false, false));
        assert_eq!(behind.app_placement, AppPlacement::HiddenBehindThread);
        assert!(behind.thread.is_some());
        // The focused app is in front even if the thread was asked for last.
        let focused = with_app(120, 24, true, true, true, open(true, false, false));
        assert_eq!(focused.app_placement, AppPlacement::Docked);
        assert!(focused.thread.is_none());
        // 80 by 24, as v0.16's trial: the conversation 38, the app 40 by 18 inside.
        let eighty = with_app(80, 24, true, true, false, open(true, false, true));
        assert_eq!(eighty.timeline.width, 38);
        let region = inside(eighty.app.unwrap());
        assert_eq!((region.width, region.height), (40, 18));
        assert!(fits_an_app(region));
    }

    #[test]
    fn a_focused_app_that_cannot_dock_takes_the_window_and_leaves_it_with_the_focus() {
        // Too narrow to dock: zoomed while focused, undrawn otherwise.
        let focused = with_app(60, 24, true, true, true, open(true, false, true));
        assert_eq!(focused.app_placement, AppPlacement::Zoomed);
        let left = with_app(60, 24, true, true, true, open(false, false, true));
        assert_eq!(left.app_placement, AppPlacement::HiddenNarrow);
        assert_eq!(left.timeline.width, 60);
        // Wide enough but too short for 16 rows beside the dock: 22 rows
        // dock (16 inside), 21 do not, and zoomed give 18.
        assert_eq!(
            with_app(120, 22, true, true, false, open(true, false, true)).app_placement,
            AppPlacement::Docked
        );
        let short = with_app(120, 21, true, true, false, open(true, false, true));
        assert_eq!(short.app_placement, AppPlacement::Zoomed);
        let region = inside(short.app.unwrap());
        assert_eq!((region.width, region.height), (118, 18));
        // 19 rows is the least window that gives sixteen, zoomed. Below it
        // zoom would gain nothing, so the pane stays beside the conversation,
        // too small, and says so.
        let least = with_app(120, 19, true, true, false, open(true, false, true));
        assert_eq!(least.app_placement, AppPlacement::Zoomed);
        assert_eq!(least.app.unwrap().height - APP_BORDER, 16);
        let too_short = with_app(120, 18, true, true, false, open(true, false, true));
        assert_eq!(too_short.app_placement, AppPlacement::Docked);
        assert!(!fits_an_app(inside(too_short.app.unwrap())));
        // Too narrow to dock, it is zoomed whatever the height.
        assert_eq!(
            with_app(60, 16, true, true, false, open(true, false, true)).app_placement,
            AppPlacement::Zoomed
        );
        // The same window with the focus elsewhere keeps the pane docked.
        assert_eq!(
            with_app(120, 21, true, true, false, open(false, false, true)).app_placement,
            AppPlacement::Docked
        );
        // Asked for: zoomed at any size, only while focused.
        assert_eq!(
            with_app(200, 40, true, true, true, open(true, true, true)).app_placement,
            AppPlacement::Zoomed
        );
        assert_eq!(
            with_app(200, 40, true, true, true, open(false, true, true)).app_placement,
            AppPlacement::Docked
        );
    }

    /// The widths of v0.16's exit gate, moved by the conversation's eight
    /// more columns.
    #[test]
    fn the_widths_of_the_exit_gate() {
        for height in [24, 40] {
            let at = |width, focused| {
                with_app(width, height, true, true, true, open(focused, false, true))
            };
            // 152: rail? no; sidebar, conversation, thread and app.
            let wide = at(152, true);
            assert!(wide.sidebar.is_some() && wide.thread.is_some() && wide.app.is_some());
            assert_eq!(wide.timeline.width, 38);
            // 108: the app in front of the thread, with the sidebar.
            let hundred = at(108, true);
            assert!(hundred.app.is_some() && hundred.thread.is_none() && hundred.sidebar.is_some());
            // 80: the conversation and the app.
            let eighty = at(80, true);
            assert!(eighty.app.is_some() && eighty.sidebar.is_none());
            assert_eq!(eighty.timeline.width, 38);
            // 72: the app alone while focused; the conversation and the
            // thread if not.
            assert_eq!(at(72, true).app_placement, AppPlacement::Zoomed);
            assert_eq!(at(72, false).app_placement, AppPlacement::HiddenNarrow);
            assert!(at(72, false).thread.is_some());
        }
    }

    #[test]
    fn writing_dock_is_reserved_without_overlapping_the_workspace_or_status() {
        let panes = panes_with_composer(Rect::new(0, 0, 180, 40), true, true, true, 28, 44, 6);
        let composer = panes.composer.expect("writing dock");
        assert_eq!(composer.height, 6);
        assert_eq!(panes.status.height, 1);
        assert_eq!(panes.timeline.bottom(), composer.y);
        assert_eq!(composer.bottom(), panes.status.y);
        assert_eq!(panes.community.expect("community").width, COMMUNITY_WIDTH);
    }

    #[test]
    fn small_terminal_clamps_the_dock_and_hides_sidebars() {
        let panes = panes_with_composer(Rect::new(0, 0, 60, 5), true, true, true, 28, 44, 12);
        assert!(panes.community.is_none());
        assert!(panes.sidebar.is_none());
        assert!(panes.thread.is_none());
        assert_eq!(panes.composer.expect("composer").height, 3);
        assert_eq!(panes.status.height, 1);
        assert!(panes.timeline.height >= 1);
    }
}
