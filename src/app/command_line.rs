//! The `:` line in bzz's loop (v0.17 C11): Tab completes what is typed from
//! the registry and what bzz has (themes, installed apps, channels), and the
//! line shows under it what Tab offers and the usage of the command. The
//! completion itself is in `ui::command`.

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Clear, Paragraph},
};

use super::{App, centered};
use crate::{
    render::sanitize,
    ui::{
        command::Words,
        theme::{BorderSurface, HighlightGroup, ThemeRegistry},
    },
};

impl App {
    /// The installed apps of the open community, by name.
    async fn command_line_apps(&self) -> Vec<String> {
        #[cfg(feature = "apps")]
        if let Some(community) = self.active_community_id() {
            let listed = self
                .store
                .call(move |store| store.apps(community))
                .await
                .unwrap_or_default();
            let mut names = listed.into_iter().map(|app| app.name).collect::<Vec<_>>();
            names.sort();
            return names;
        }
        Vec::new()
    }

    /// What the `:` line completes from: theme ids, apps, joined channels.
    fn with_command_words<T>(&self, then: impl FnOnce(&Words<'_>) -> T) -> T {
        let themes = ThemeRegistry::entries()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let channels = self
            .channels
            .iter()
            .filter(|channel| channel.is_member && !channel.kind.is_dm())
            .map(|channel| sanitize::single_line(&channel.name))
            .collect::<Vec<_>>();
        let apps = self.command_apps.as_deref().unwrap_or_default();
        then(&Words {
            themes: &themes,
            apps,
            channels: &channels,
        })
    }

    /// Tab (or Shift-Tab, `forward` false) on the `:` line.
    pub(super) async fn complete_command(&mut self, forward: bool) {
        if self.command_apps.is_none() && self.command.trim_start().starts_with("app") {
            self.command_apps = Some(self.command_line_apps().await);
        }
        let mut line = std::mem::take(&mut self.command);
        let mut completion = std::mem::take(&mut self.command_completion);
        self.with_command_words(|words| completion.tab(&mut line, words, forward));
        self.command = line;
        self.command_completion = completion;
    }

    /// The `:` line closed: the next one starts fresh.
    pub(super) fn command_line_closed(&mut self) {
        self.command_completion.reset();
        self.command_apps = None;
    }

    /// The `:` line, and under it what Tab offers and the usage.
    pub(super) fn render_command_line(&self, frame: &mut Frame<'_>, area: Rect) {
        let inner = usize::from((area.width * 70 / 100).saturating_sub(4)).max(10);
        let mut lines = vec![sanitize::single_line(&format!(":{}", self.command))];
        let below =
            self.with_command_words(|words| self.command_completion.lines(&self.command, words));
        if !below.is_empty() {
            lines.push(String::new());
        }
        for line in below {
            // At most two rows of candidates: the rest come by Tab.
            let mut wrapped = crate::ui::help::wrap_text(&line, inner);
            wrapped.truncate(2);
            lines.extend(wrapped);
        }
        let height = u16::try_from(lines.len() + 2)
            .unwrap_or(u16::MAX)
            .max(3)
            .min(area.height);
        let popup = centered(area, 70, height);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines.join("\n"))
                .style(self.theme.style(HighlightGroup::Normal))
                .block(
                    Block::bordered()
                        .border_type(self.theme.border_type(BorderSurface::Modal))
                        .border_style(self.theme.style(HighlightGroup::ModalBorder))
                        .title_style(self.theme.style(HighlightGroup::ModalTitle))
                        .title(" command \u{b7} Tab completes \u{b7} Enter runs "),
                ),
            popup,
        );
    }
}
