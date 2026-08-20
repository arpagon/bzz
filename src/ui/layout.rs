use ratatui::layout::{Constraint, Direction, Layout, Rect};

const MIN_TIMELINE_WIDTH: u16 = 30;
const COMMUNITY_WIDTH: u16 = 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Panes {
    pub community: Option<Rect>,
    pub sidebar: Option<Rect>,
    pub timeline: Rect,
    pub thread: Option<Rect>,
    /// A workspace-local writing dock. It is measured with the rest of the
    /// shell so it can never overlap messages or invalidate pointer geometry.
    pub composer: Option<Rect>,
    pub status: Rect,
}

/// Resolves the ordinary workspace without reserving a writing dock.
///
/// Kept as the compact layout entry point for pure callers and tests. The app
/// uses [`panes_with_composer`] so the visible composer is part of the same
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

/// Resolves the workspace shell, reserving `composer_height` terminal rows
/// above the one-row status line. A caller-controlled height is clamped to the
/// available area so a small terminal remains recoverable.
pub fn panes_with_composer(
    area: Rect,
    community: bool,
    sidebar: bool,
    thread: bool,
    sidebar_width: u16,
    thread_width: u16,
    composer_height: u16,
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
    if body.width < 70 {
        return Panes {
            community: None,
            sidebar: None,
            timeline: body,
            thread: None,
            composer,
            status,
        };
    }
    let show_sidebar = sidebar && body.width >= MIN_TIMELINE_WIDTH.saturating_add(sidebar_width);
    let sidebar_space = if show_sidebar { sidebar_width } else { 0 };
    let show_thread = thread
        && body.width
            >= MIN_TIMELINE_WIDTH
                .saturating_add(sidebar_space)
                .saturating_add(thread_width);
    let thread_space = if show_thread { thread_width } else { 0 };
    let show_community = community
        && body.width
            >= MIN_TIMELINE_WIDTH
                .saturating_add(sidebar_space)
                .saturating_add(thread_space)
                .saturating_add(COMMUNITY_WIDTH);
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
    Panes {
        community,
        sidebar,
        timeline,
        thread,
        composer,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::{COMMUNITY_WIDTH, panes_with_composer};
    use ratatui::layout::Rect;

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
