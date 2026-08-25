use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    agents::Eligibility,
    render::sanitize,
    store::agents::RemoteAgentView,
    ui::{
        hit_map::{HitMap, HitTarget},
        theme::{BorderSurface, HighlightGroup, Theme},
    },
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentDirectoryState {
    pub agents: Vec<RemoteAgentView>,
    pub selected_pubkey: Option<String>,
    pub loading: bool,
}

impl AgentDirectoryState {
    pub fn reconcile(&mut self) {
        if !self
            .agents
            .iter()
            .any(|agent| self.selected_pubkey.as_deref() == Some(agent.pubkey.as_str()))
        {
            self.selected_pubkey = self.agents.first().map(|agent| agent.pubkey.clone());
        }
    }

    pub fn selected(&self) -> Option<&RemoteAgentView> {
        let pubkey = self.selected_pubkey.as_deref()?;
        self.agents.iter().find(|agent| agent.pubkey == pubkey)
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.agents.is_empty() {
            self.selected_pubkey = None;
            return;
        }
        let current = self
            .selected_pubkey
            .as_ref()
            .and_then(|pubkey| self.agents.iter().position(|agent| &agent.pubkey == pubkey))
            .unwrap_or_default();
        let next = current
            .saturating_add_signed(delta)
            .min(self.agents.len() - 1);
        self.selected_pubkey = Some(self.agents[next].pubkey.clone());
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AgentDirectoryState,
    theme: &Theme,
    hit_map: &mut HitMap,
) {
    frame.render_widget(Clear, area);
    let title = format!(
        " verified remote agents · ↑/↓ select · m mention · r refresh · Esc close{} ",
        if state.loading {
            " · refreshing…"
        } else {
            ""
        }
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.agents.is_empty() {
        frame.render_widget(
            Paragraph::new(if state.loading {
                "Refreshing public relay records…"
            } else {
                "No verified remote agents are cached for this community. Press r to refresh."
            })
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if inner.width >= 88 {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(43), Constraint::Percentage(57)])
            .split(inner);
        render_list(frame, panes[0], state, theme, hit_map);
        render_detail(frame, panes[1], state.selected(), theme);
    } else {
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(inner);
        render_list(frame, panes[0], state, theme, hit_map);
        render_detail(frame, panes[1], state.selected(), theme);
    }
}

fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AgentDirectoryState,
    theme: &Theme,
    hit_map: &mut HitMap,
) {
    let items = state
        .agents
        .iter()
        .map(|agent| {
            let eligibility = if agent.stale {
                "stale"
            } else {
                match agent.eligibility {
                    Eligibility::Eligible => "eligible",
                    Eligibility::Ineligible => "not eligible",
                    Eligibility::PolicyUnknown => "policy unknown",
                }
            };
            ListItem::new(Line::from(vec![
                Span::styled("◆ ", theme.style(HighlightGroup::SelectionMarker)),
                Span::styled(
                    sanitize::single_line(&agent.name),
                    theme.style(HighlightGroup::Normal),
                ),
                Span::raw(format!("  {eligibility}")),
            ]))
        })
        .collect::<Vec<_>>();
    let selected = state.selected_pubkey.as_ref().and_then(|pubkey| {
        state
            .agents
            .iter()
            .position(|agent| &agent.pubkey == pubkey)
    });
    let mut list_state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(theme.style(HighlightGroup::SelectedRow))
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(theme.style(HighlightGroup::ModalBorder)),
            ),
        area,
        &mut list_state,
    );
    let rows = area.height as usize;
    for (index, agent) in state.agents.iter().take(rows).enumerate() {
        hit_map.push(
            Rect::new(area.x, area.y.saturating_add(index as u16), area.width, 1),
            HitTarget::RemoteAgent(agent.pubkey.clone()),
        );
    }
}

fn render_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: Option<&RemoteAgentView>,
    theme: &Theme,
) {
    let Some(agent) = selected else { return };
    let policy = agent
        .respond_to
        .map_or("unknown", crate::agents::RespondTo::as_str);
    let channels = agent
        .channel_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let capabilities = if agent.capabilities.is_empty() {
        "none declared".to_string()
    } else {
        agent
            .capabilities
            .iter()
            .map(|value| sanitize::single_line(value))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let detail = vec![
        Line::from(Span::styled(
            sanitize::single_line(&agent.name),
            theme.style(HighlightGroup::ModalTitle),
        )),
        Line::from(""),
        Line::from(format!("agent: {}", agent.pubkey)),
        Line::from(format!("owner: {}", agent.owner_pubkey)),
        Line::from(format!("policy: {policy}")),
        Line::from(format!(
            "allowlist entries: {}",
            agent.respond_to_allowlist.len()
        )),
        Line::from(format!("eligibility: {}", agent.eligibility.as_str())),
        Line::from(format!(
            "freshness: {}",
            if agent.stale { "stale" } else { "current" }
        )),
        Line::from(format!(
            "presence: {} (ephemeral, not readiness)",
            agent.presence.as_str()
        )),
        Line::from(format!("capabilities: {capabilities}")),
        Line::from(format!("shared channels: {channels}")),
        Line::from(format!("verified at: {}", agent.last_verified_at)),
        Line::from(""),
        Line::from("This process and its private key are controlled remotely."),
        Line::from("bzz can mention it, but cannot start, stop, inspect, or recover its runtime."),
    ];
    frame.render_widget(
        Paragraph::new(detail)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
            ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_stable_by_pubkey_across_refresh_ordering() {
        let mut state = AgentDirectoryState {
            agents: vec![agent("b"), agent("a")],
            selected_pubkey: Some("b".repeat(64)),
            loading: false,
        };
        state.agents.reverse();
        state.reconcile();
        assert_eq!(state.selected_pubkey, Some("b".repeat(64)));
    }

    fn agent(value: &str) -> RemoteAgentView {
        RemoteAgentView {
            schema_version: 1,
            community_id: uuid::Uuid::nil(),
            pubkey: value.repeat(64),
            owner_pubkey: "c".repeat(64),
            name: value.into(),
            capabilities: Vec::new(),
            presence: crate::agents::Presence::Unknown,
            respond_to: None,
            respond_to_allowlist: Vec::new(),
            eligibility: Eligibility::PolicyUnknown,
            stale: false,
            channel_ids: Vec::new(),
            last_verified_at: 0,
        }
    }
}
