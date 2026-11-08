//! `:broker` — hosted agents, their policy and the content-free activity log.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    broker::host::Activity,
    config::HostedAgentConfig,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    agents: &[HostedAgentConfig],
    activity: &[Activity],
    serving: Option<u16>,
    paused_all: bool,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let state = match (serving, paused_all) {
        (None, _) => "not serving".to_owned(),
        (Some(port), true) => format!("127.0.0.1:{port} · ALL PAUSED"),
        (Some(port), false) => format!("127.0.0.1:{port}"),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(format!(
            " agent broker · {state} · p pause/resume all · Esc "
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::try_from(agents.len().clamp(1, 8) + 1).unwrap_or(9)),
            Constraint::Min(1),
        ])
        .split(inner);
    let mut lines = vec![Line::from(Span::styled(
        "hosted agents (manage with `bzz broker …`)",
        theme.style(HighlightGroup::Muted),
    ))];
    if agents.is_empty() {
        lines.push(Line::from(
            "none — create one with `bzz broker create <label>`",
        ));
    }
    for agent in agents.iter().take(8) {
        lines.push(Line::from(format!(
            "{} · {} · {}",
            crate::protocol::artifact::display(&agent.label, 32),
            if agent.paused || paused_all {
                "paused"
            } else {
                "active"
            },
            agent.allowed.join(", ")
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme.style(HighlightGroup::Normal)),
        panes[0],
    );
    let mut log = vec![Line::from(Span::styled(
        "recent activity (no content)",
        theme.style(HighlightGroup::Muted),
    ))];
    log.extend(activity.iter().rev().take(200).map(|row| {
        Line::from(format!(
            "{} · {} · {} · {}{}",
            time::OffsetDateTime::from_unix_timestamp(row.at as i64)
                .ok()
                .and_then(|at| at
                    .format(&time::format_description::well_known::Rfc3339)
                    .ok())
                .unwrap_or_default(),
            crate::protocol::artifact::display(&row.agent, 32),
            row.action,
            row.outcome,
            row.target
                .as_deref()
                .map(|target| format!(" · {target}"))
                .unwrap_or_default()
        ))
    }));
    frame.render_widget(
        Paragraph::new(log)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
        panes[1],
    );
}
