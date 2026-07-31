use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Panes {
    pub community: Option<Rect>,
    pub sidebar: Option<Rect>,
    pub timeline: Rect,
    pub thread: Option<Rect>,
    pub status: Rect,
}

pub fn panes(
    area: Rect,
    sidebar: bool,
    thread: bool,
    sidebar_width: u16,
    thread_width: u16,
) -> Panes {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let body = vertical[0];
    if body.width < 70 {
        return Panes {
            community: None,
            sidebar: None,
            timeline: body,
            thread: None,
            status: vertical[1],
        };
    }
    let show_sidebar = sidebar && body.width >= 30_u16.saturating_add(sidebar_width);
    let sidebar_space = if show_sidebar { sidebar_width } else { 0 };
    let show_thread = thread
        && body.width
            >= 30_u16
                .saturating_add(sidebar_space)
                .saturating_add(thread_width);
    let thread_space = if show_thread { thread_width } else { 0 };
    let show_community = body.width
        >= 34_u16
            .saturating_add(sidebar_space)
            .saturating_add(thread_space);
    let mut constraints = vec![];
    if show_community {
        constraints.push(Constraint::Length(4));
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
    Panes {
        community,
        sidebar,
        timeline,
        thread,
        status: vertical[1],
    }
}
