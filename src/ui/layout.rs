//! The workspace shell: which panes are drawn, and where.
//!
//! Columns are drawn in this order: community rail, channel sidebar,
//! conversation, thread, app. The conversation always keeps
//! [`MIN_TIMELINE_WIDTH`] columns; the other panes are admitted while there
//! is room for them, in this order:
//!
//! 1. the pane of the right side that was asked for last (the thread or the
//!    app);
//! 2. the channel sidebar;
//! 3. the other pane of the right side;
//! 4. the community rail, and only beside its sidebar, unless the sidebar
//!    is switched off or would not fit even alone.
//!
//! This is v0.15's order (sidebar, thread, rail) except that a pane the
//! owner asked for goes before the sidebar: v0.15 left an open thread
//! undrawn between 74 and 101 columns while the writing dock said it
//! replied into it. With no app open and every pane fitting, the result is
//! v0.15's cell for cell (`with_no_app_open_the_workspace_is_laid_out_as_in_v015`).
//!
//! The rule for the app pane comes from M0's trial
//! (`docs/adr-v0.16-apps.md`, "The pane and its keys").

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// The conversation's columns, whatever else is open.
pub const MIN_TIMELINE_WIDTH: u16 = 30;
pub const COMMUNITY_WIDTH: u16 = 22;
/// Below this body width the workspace is the conversation alone.
pub const NARROW_BODY_WIDTH: u16 = 70;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Panes {
    pub community: Option<Rect>,
    pub sidebar: Option<Rect>,
    pub timeline: Rect,
    pub thread: Option<Rect>,
    /// The app pane, docked or zoomed, with its border.
    pub app: Option<Rect>,
    pub app_placement: AppPlacement,
    /// A workspace-local writing dock. It is measured with the rest of the
    /// shell so it can never overlap messages or invalidate pointer geometry.
    pub composer: Option<Rect>,
    pub status: Rect,
}

/// Resolves the ordinary workspace without reserving a writing dock.
///
/// Kept as the compact layout entry point for pure callers and tests. The app
/// uses [`panes_with_app`] so the visible composer is part of the same
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

/// The workspace shell with the app pane. See the module's documentation
/// for the order of admission. Besides it:
///
/// - While the app has the focus it takes the whole window above the status
///   line (the writing dock too) when zoom is asked for, when the window is
///   too narrow to dock it, or when docked it would have fewer than
///   [`APP_MIN_ROWS`] rows and zoomed it would not.
/// - The pane with the focus is always the one in front.
/// - An app that is neither drawn nor focused keeps running; the placement
///   says why it is not drawn.
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
    let composer_height = composer_height.min(area.height.saturating_sub(2));
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
    let docks_wide = body.width >= MIN_TIMELINE_WIDTH.saturating_add(app_width);
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
        };
    }
    if body.width < NARROW_BODY_WIDTH {
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
        };
    }
    let fits = |used: u16, width: u16| body.width >= used.saturating_add(width);
    let app_docks = app.open && docks_wide;
    let mut used = MIN_TIMELINE_WIDTH;
    let mut show_app = false;
    let mut show_thread = false;
    // 1. The pane of the right side asked for last; the focused one is
    // always in front. If it does not fit, the other.
    let app_first = app_docks && (app.front || app.focused || !thread);
    if app_first {
        show_app = true;
        used = used.saturating_add(app_width);
    } else if thread && fits(used, thread_width) {
        show_thread = true;
        used = used.saturating_add(thread_width);
    } else if app_docks {
        show_app = true;
        used = used.saturating_add(app_width);
    }
    // 2. The sidebar.
    let show_sidebar = sidebar && fits(used, sidebar_width);
    if show_sidebar {
        used = used.saturating_add(sidebar_width);
    }
    // 3. The other pane of the right side.
    if thread && !show_thread && fits(used, thread_width) {
        show_thread = true;
        used = used.saturating_add(thread_width);
    }
    if app_docks && !show_app && fits(used, app_width) {
        show_app = true;
        used = used.saturating_add(app_width);
    }
    // 4. The rail, not beside a sidebar that gave way to a pane of the
    // right side: communities without their channels help nobody.
    let sidebar_gave_way = sidebar && !show_sidebar && fits(MIN_TIMELINE_WIDTH, sidebar_width);
    let show_community = community && !sidebar_gave_way && fits(used, COMMUNITY_WIDTH);
    let app_placement = if !app.open {
        AppPlacement::Closed
    } else if show_app {
        AppPlacement::Docked
    } else if !app_docks {
        AppPlacement::HiddenNarrow
    } else {
        AppPlacement::HiddenBehindThread
    };
    let mut constraints = vec![];
    if show_community {
        constraints.push(Constraint::Length(COMMUNITY_WIDTH));
    }
    if show_sidebar {
        constraints.push(Constraint::Length(sidebar_width));
    }
    constraints.push(Constraint::Min(MIN_TIMELINE_WIDTH));
    if show_thread {
        constraints.push(Constraint::Length(thread_width));
    }
    if show_app {
        constraints.push(Constraint::Length(app_width));
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
    let community = show_community.then(&mut next);
    let sidebar = show_sidebar.then(&mut next);
    let timeline = next();
    let thread = show_thread.then(&mut next);
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
        APP_BORDER, APP_PANE_WIDTH, AppPlacement, AppRequest, COMMUNITY_WIDTH, MIN_TIMELINE_WIDTH,
        Panes, fits_an_app, inside, panes_with_app, panes_with_composer,
    };
    use ratatui::layout::{Constraint, Direction, Layout, Rect};

    type V015 = (
        Option<Rect>,
        Option<Rect>,
        Rect,
        Option<Rect>,
        Option<Rect>,
        Rect,
    );

    /// The layout of v0.15.0, copied whole.
    fn panes_v015(
        area: Rect,
        community: bool,
        sidebar: bool,
        thread: bool,
        sidebar_width: u16,
        thread_width: u16,
        composer_height: u16,
    ) -> V015 {
        let composer_height = composer_height.min(area.height.saturating_sub(2));
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
        if body.width < 70 {
            return (None, None, body, None, composer, status);
        }
        let show_sidebar = sidebar && body.width >= 30_u16.saturating_add(sidebar_width);
        let sidebar_space = if show_sidebar { sidebar_width } else { 0 };
        let show_thread = thread
            && body.width
                >= 30_u16
                    .saturating_add(sidebar_space)
                    .saturating_add(thread_width);
        let thread_space = if show_thread { thread_width } else { 0 };
        let show_community = community
            && body.width
                >= 30_u16
                    .saturating_add(sidebar_space)
                    .saturating_add(thread_space)
                    .saturating_add(22);
        let mut constraints = vec![];
        if show_community {
            constraints.push(Constraint::Length(22));
        }
        if show_sidebar {
            constraints.push(Constraint::Length(sidebar_width));
        }
        constraints.push(Constraint::Min(30));
        if show_thread {
            constraints.push(Constraint::Length(thread_width));
        }
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(body);
        let mut index = 0;
        let community = show_community.then(|| {
            let rect = columns[index];
            index += 1;
            rect
        });
        let sidebar = show_sidebar.then(|| {
            let rect = columns[index];
            index += 1;
            rect
        });
        let timeline = columns[index];
        index += 1;
        let thread = show_thread.then(|| columns[index]);
        (community, sidebar, timeline, thread, composer, status)
    }

    /// With no app open the layout is v0.15's in every case but one: a
    /// thread v0.15 left undrawn for the sidebar is now drawn and the
    /// sidebar gives way (the composer replied into it). That case is
    /// checked for what it is instead.
    #[test]
    fn with_no_app_open_the_workspace_is_laid_out_as_in_v015() {
        let (mut same, mut thread_first) = (0_u32, 0_u32);
        for width in 40..=220 {
            for height in [5, 12, 16, 24, 40] {
                for flags in 0..8_u8 {
                    for (sidebar_width, thread_width) in [(28, 44), (18, 30), (60, 80)] {
                        for composer in [0, 3, 11] {
                            let area = Rect::new(0, 0, width, height);
                            let (community, sidebar, thread) =
                                (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0);
                            let old = panes_v015(
                                area,
                                community,
                                sidebar,
                                thread,
                                sidebar_width,
                                thread_width,
                                composer,
                            );
                            let new = panes_with_composer(
                                area,
                                community,
                                sidebar,
                                thread,
                                sidebar_width,
                                thread_width,
                                composer,
                            );
                            let label = format!(
                                "{width}x{height} flags {flags:03b} widths {sidebar_width}/{thread_width} dock {composer}"
                            );
                            assert_eq!(new.app, None, "{label}");
                            assert_eq!(new.app_placement, AppPlacement::Closed, "{label}");
                            let body_width = width;
                            let v015_dropped_the_thread = thread
                                && old.3.is_none()
                                && body_width >= 70
                                && body_width >= MIN_TIMELINE_WIDTH + thread_width;
                            if v015_dropped_the_thread {
                                // The thread is drawn, beside the conversation's 30.
                                let drawn = new.thread.expect(&label);
                                assert_eq!(drawn.width, thread_width, "{label}");
                                assert!(new.timeline.width >= MIN_TIMELINE_WIDTH, "{label}");
                                assert_eq!((new.composer, new.status), (old.4, old.5), "{label}");
                                thread_first += 1;
                                continue;
                            }
                            assert_eq!(
                                old,
                                (
                                    new.community,
                                    new.sidebar,
                                    new.timeline,
                                    new.thread,
                                    new.composer,
                                    new.status
                                ),
                                "{label}"
                            );
                            same += 1;
                        }
                    }
                }
            }
        }
        assert!(same > 60_000, "{same}");
        assert!(thread_first > 1_000, "{thread_first}");
    }

    /// v0.15 at 80 columns: Enter on a message opened a thread nothing
    /// showed. Now the thread shows, and the sidebar gives way.
    #[test]
    fn a_thread_asked_for_is_drawn_from_74_columns() {
        for width in 74..=101 {
            let panes =
                panes_with_composer(Rect::new(0, 0, width, 24), true, true, true, 28, 44, 3);
            let thread = panes.thread.expect("thread drawn");
            assert_eq!(thread.width, 44, "{width}");
            assert!(
                panes.sidebar.is_none() && panes.community.is_none(),
                "{width}"
            );
            assert_eq!(panes.timeline.width, width - 44, "{width}");
        }
        let at_102 = panes_with_composer(Rect::new(0, 0, 102, 24), true, true, true, 28, 44, 3);
        assert!(at_102.sidebar.is_some() && at_102.thread.is_some());
        let at_73 = panes_with_composer(Rect::new(0, 0, 73, 24), true, true, true, 28, 44, 3);
        assert!(at_73.thread.is_none() && at_73.sidebar.is_some());
    }

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

    #[test]
    fn the_app_pane_never_overlaps_and_the_conversation_keeps_its_thirty_columns() {
        for width in 50..=220 {
            for height in [12, 16, 19, 22, 24, 40] {
                for flags in 0..64_u8 {
                    let app = open(flags & 8 != 0, flags & 16 != 0, flags & 32 != 0);
                    let panes = with_app(
                        width,
                        height,
                        flags & 1 != 0,
                        flags & 2 != 0,
                        flags & 4 != 0,
                        app,
                    );
                    let label = format!("{width}x{height} flags {flags:06b}");
                    assert_eq!(panes.status, Rect::new(0, height - 1, width, 1), "{label}");
                    if panes.app_placement == AppPlacement::Zoomed {
                        assert!(app.focused, "{label}: only a focused app is zoomed");
                        assert_eq!(
                            panes.app,
                            Some(Rect::new(0, 0, width, height - 1)),
                            "{label}"
                        );
                        assert_eq!(panes.composer, None, "{label}");
                        continue;
                    }
                    // The focused app is never left undrawn.
                    assert!(!app.focused || panes.app.is_some(), "{label}");
                    let mut x = 0;
                    for column in [
                        panes.community,
                        panes.sidebar,
                        Some(panes.timeline),
                        panes.thread,
                        panes.app,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        assert_eq!(column.x, x, "{label}: columns are adjacent, in order");
                        x += column.width;
                    }
                    assert_eq!(x, width, "{label}: the columns fill the body");
                    assert!(
                        panes.timeline.width >= MIN_TIMELINE_WIDTH.min(width),
                        "{label}"
                    );
                    // No rail beside a sidebar that gave way.
                    if panes.community.is_some() && flags & 2 != 0 {
                        assert!(panes.sidebar.is_some(), "{label}");
                    }
                    if let Some(pane) = panes.app {
                        assert_eq!(panes.app_placement, AppPlacement::Docked, "{label}");
                        assert_eq!(pane.width, APP_PANE_WIDTH, "{label}");
                        assert_eq!(pane.height, height - 4, "{label}: the body above the dock");
                    }
                }
            }
        }
    }

    #[test]
    fn the_thresholds_of_the_default_widths_are_the_trials() {
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
        // conversation 30 + app 42
        assert_eq!(docked(71, false, false).app, None);
        assert_eq!(
            docked(71, false, false).app_placement,
            AppPlacement::HiddenNarrow
        );
        assert!(docked(72, false, false).app.is_some());
        // + sidebar 28: at 99 the app is drawn and the sidebar is not.
        let narrow = docked(99, true, true);
        assert!(narrow.app.is_some() && narrow.sidebar.is_none() && narrow.community.is_none());
        let sidebar = docked(100, true, true);
        assert!(sidebar.app.is_some() && sidebar.sidebar.is_some() && sidebar.community.is_none());
        // + rail 22
        assert!(docked(121, true, true).community.is_none());
        assert!(docked(122, true, true).community.is_some());
        // Thread and app together: 116 with no sidebar, 144 with it, 166 with the rail.
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
        assert_eq!(both(115, false, false, true), (false, true));
        assert_eq!(both(115, false, false, false), (true, false));
        assert_eq!(both(116, false, false, true), (true, true));
        assert_eq!(both(143, true, true, true), (false, true));
        assert_eq!(both(143, true, true, false), (true, false));
        assert_eq!(both(144, true, true, false), (true, true));
        assert_eq!(both(144, true, true, true), (true, true));
        let all = with_app(165, 24, true, true, true, open(false, false, true));
        assert!(all.community.is_none() && all.thread.is_some() && all.app.is_some());
        let all = with_app(166, 24, true, true, true, open(false, false, true));
        assert!(all.community.is_some() && all.thread.is_some() && all.app.is_some());
        // The one asked for last wins the right side; the other waits.
        let behind = with_app(120, 24, true, true, true, open(false, false, false));
        assert_eq!(behind.app_placement, AppPlacement::HiddenBehindThread);
        assert!(behind.thread.is_some());
        // The focused app is in front even if the thread was asked for last.
        let focused = with_app(120, 24, true, true, true, open(true, false, false));
        assert_eq!(focused.app_placement, AppPlacement::Docked);
        assert!(focused.thread.is_none());
        // 80 by 24: the conversation 38, the app 40 by 18 inside.
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

    /// The widths of the exit gate, at the heights the trial named.
    #[test]
    fn the_widths_of_the_exit_gate() {
        for height in [24, 40] {
            let at = |width, focused| {
                with_app(width, height, true, true, true, open(focused, false, true))
            };
            // 144: rail? no; sidebar, conversation, thread and app.
            let wide = at(144, true);
            assert!(wide.sidebar.is_some() && wide.thread.is_some() && wide.app.is_some());
            assert_eq!(wide.timeline.width, 30);
            // 100: the app in front of the thread, with the sidebar.
            let hundred = at(100, true);
            assert!(hundred.app.is_some() && hundred.thread.is_none() && hundred.sidebar.is_some());
            // 72: the conversation and the app.
            let seventy_two = at(72, true);
            assert!(seventy_two.app.is_some() && seventy_two.sidebar.is_none());
            assert_eq!(seventy_two.timeline.width, 30);
            // 60: the app alone while focused; the conversation alone if not.
            assert_eq!(at(60, true).app_placement, AppPlacement::Zoomed);
            assert_eq!(at(60, false).app_placement, AppPlacement::HiddenNarrow);
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
