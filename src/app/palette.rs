//! The palette in bzz's loop (v0.17 C1): opening it with the facts of the
//! moment, its keys, and running what was chosen through the paths a key
//! or a typed `:` command already take. The ranking and drawing are in
//! `ui::palette`; the rows in `ui::action_table`.

use crossterm::event::KeyEvent;

use super::App;
use crate::{
    config::ClipboardMode,
    domain::ChannelKind,
    error::Result,
    ui::{
        action_table::{Needs, Run, TABLE},
        palette::{self, Facts, Outcome, Palette, PaletteChannel, Target},
        state::{ConfirmationKind, FocusSurface, Overlay, Route},
        terminal::{TerminalGuard, Tui, copy_osc52},
    },
};

impl App {
    /// The facts the palette judges its rows by, read once as it opens.
    fn palette_facts(&self) -> Facts {
        Facts {
            actions: self.action_context(),
            channel_is_dm: self
                .current_channel()
                .is_some_and(|channel| channel.kind == ChannelKind::Dm),
            huddle_running: self.active_huddle().is_some(),
            in_call: self.huddle_call_state().is_some(),
            app_here: self.shown_app().is_some(),
            artifact_selected: self.artifact_panel.selected.is_some(),
            communities: self.config.communities.len(),
        }
    }

    /// The channels and DMs the palette can open: those the sidebar lists,
    /// unread first.
    fn palette_channels(&self) -> Vec<PaletteChannel> {
        let unread = self.unread_channels();
        let mut channels = self
            .channels
            .iter()
            .filter(|channel| channel.listed_for_viewer() && !channel.is_hidden)
            .filter(|channel| self.show_system_channels || channel.kind != ChannelKind::System)
            .map(|channel| PaletteChannel {
                id: channel.id,
                name: channel.name.clone(),
                dm: channel.kind == ChannelKind::Dm,
                unread: unread.contains(&channel.id),
                last_event_at: channel.last_event_at.unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        palette::order_channels(&mut channels);
        channels
    }

    pub(super) fn open_palette(&mut self) {
        let facts = self.palette_facts();
        let candidates = palette::candidates(&self.keymap, &facts, &self.palette_channels());
        self.palette = Some(Palette::open(
            candidates,
            &facts,
            &self.palette_recent,
            (self.presentation.route, self.presentation.focus),
        ));
        self.presentation.open_overlay(Overlay::Palette);
    }

    /// Esc: the palette goes and the focus is where it was.
    fn close_palette(&mut self) {
        if let Some(palette) = self.palette.take() {
            let (route, focus) = palette.return_to;
            self.presentation.route = route;
            self.presentation.focus = focus;
        }
        if self.presentation.overlay == Some(Overlay::Palette) {
            self.presentation.close_overlay();
        }
    }

    /// A key while the palette is open. What does not run a row is the
    /// palette's own (`ui::palette::Palette::key`).
    pub(super) fn palette_input(&mut self, key: KeyEvent) -> Option<Target> {
        let Some(palette) = self.palette.as_mut() else {
            self.presentation.close_overlay();
            return None;
        };
        match palette.key(key) {
            Outcome::Stay => None,
            Outcome::Close => {
                self.close_palette();
                None
            }
            Outcome::Copy(command) => {
                self.copy_shell_command(command);
                None
            }
            Outcome::Run(target) => Some(target),
        }
    }

    pub(super) async fn palette_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        if let Some(target) = self.palette_input(key) {
            self.run_palette_target(target, guard, terminal).await?;
        }
        Ok(())
    }

    /// A shell-only row: its command goes to the clipboard when bzz may
    /// write there, and the palette says so; otherwise it stays open and
    /// shows the command to type.
    fn copy_shell_command(&mut self, command: &'static str) {
        let refused = if self.config.ui.clipboard == ClipboardMode::Disabled {
            Some("the clipboard is off in [ui].clipboard".to_owned())
        } else {
            copy_osc52(command).err().map(|error| error.to_string())
        };
        match refused {
            None => {
                self.close_palette();
                self.status_error = Some(format!(
                    "copied to the clipboard: {command} · run it in a shell"
                ));
            }
            Some(why) => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.set_notice(format!("only in the shell: {command} ({why})"));
                }
            }
        }
    }

    async fn run_palette_target(
        &mut self,
        target: Target,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        // The palette closes first: what runs may open its own overlay.
        self.palette = None;
        self.presentation.close_overlay();
        match target {
            Target::Channel(id) => self.open_palette_channel(id).await?,
            Target::Action(index) => {
                let Some(spec) = TABLE.get(index) else {
                    return Ok(());
                };
                palette::remember(&mut self.palette_recent, spec.id);
                match spec.run {
                    Run::Ui(action) => {
                        // A message action chosen from another pane acts on
                        // the timeline's selected message, as `Space a`
                        // would from the timeline.
                        if spec.needs == Needs::Message
                            && self.presentation.route == Route::Workspace
                            && !matches!(
                                self.presentation.focus,
                                FocusSurface::Timeline | FocusSurface::Context
                            )
                        {
                            self.presentation
                                .set_workspace_focus(FocusSurface::Timeline);
                        }
                        self.dispatch_route_action(action, guard, terminal).await?;
                    }
                    Run::Command(text) => {
                        self.command = text.to_owned();
                        self.execute_command().await?;
                    }
                    Run::Prefill(text) => {
                        // As the artifacts panel's `r`: the `:` line opens
                        // with the command written, the cursor at its end.
                        self.command = text.to_owned();
                        self.presentation.open_overlay(Overlay::Command);
                    }
                    Run::Quit => self.presentation.open_confirmation(ConfirmationKind::Quit),
                    Run::Shell(command) => self.copy_shell_command(command),
                }
            }
        }
        Ok(())
    }

    /// As the channel finder opens a channel.
    async fn open_palette_channel(&mut self, id: uuid::Uuid) -> Result<()> {
        let Some(index) = self.channels.iter().position(|channel| channel.id == id) else {
            self.status_error = Some("that channel is no longer listed".into());
            return Ok(());
        };
        if self.presentation.route == Route::Inbox {
            self.presentation.route = Route::Workspace;
            self.presentation.clear_inbox_return();
        }
        let member = self.channels[index].is_member;
        self.select_channel_index(index);
        self.showing_open_channel = !member;
        self.load_selected_channel().await?;
        self.presentation
            .set_workspace_focus(FocusSurface::Timeline);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::{
        config::Config,
        domain::{Channel, ChannelKind, Message, Visibility},
        paths::Paths,
        store::{Store, writer::StoreHandle},
        ui::{
            palette::Target,
            state::{FocusSurface, Overlay},
        },
    };

    async fn app(temporary: &TempDir) -> super::App {
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let config = Config::default();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = super::App::new(config, paths, handle).await.unwrap();
        let channel = Uuid::from_u128(7);
        app.channels = vec![Channel {
            id: channel,
            name: "t9-general".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: Some(1),
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        let id = "f".repeat(64);
        app.messages = vec![Message {
            event_id: id.clone(),
            channel_id: channel,
            // Another person's message.
            pubkey: "c".repeat(64),
            created_at: 1,
            content: "not yours".into(),
            attachments: Vec::new(),
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            delivery: crate::domain::DeliveryState::Delivered,
            system: None,
            sent_from_thread: None,
            broadcast: false,
            emoji: Vec::new(),
            edited: false,
        }];
        app.timeline.selected_event = Some(id);
        app
    }

    fn typed(app: &mut super::App, text: &str) {
        for character in text.chars() {
            assert_eq!(
                app.palette_input(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
                None
            );
        }
    }

    fn enter(app: &mut super::App) -> Option<Target> {
        app.palette_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    #[tokio::test]
    async fn esc_closes_and_puts_the_focus_back() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.presentation.set_workspace_focus(FocusSurface::Channels);
        app.open_palette();
        assert_eq!(app.presentation.overlay, Some(Overlay::Palette));
        typed(&mut app, "thr");
        assert_eq!(
            app.palette_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            None
        );
        assert_eq!(app.presentation.overlay, None);
        assert!(app.palette.is_none());
        assert_eq!(app.presentation.focus, FocusSurface::Channels);
    }

    #[tokio::test]
    async fn delete_over_another_persons_message_runs_nothing_and_stays_open() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette();
        typed(&mut app, "delete");
        assert_eq!(enter(&mut app), None);
        assert_eq!(app.presentation.overlay, Some(Overlay::Palette));
        let notice = app.palette.as_ref().and_then(|palette| palette.notice());
        assert_eq!(
            notice,
            Some("can't now: only your own message can be deleted")
        );
        assert_eq!(app.presentation.confirmation, None);
    }

    #[tokio::test]
    async fn a_channel_row_is_chosen_with_the_actions() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette();
        typed(&mut app, "#t9");
        assert_eq!(enter(&mut app), Some(Target::Channel(Uuid::from_u128(7))));
    }

    #[tokio::test]
    async fn the_palette_reads_at_80x24_and_160x45() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette();
        for (width, height) in [(80, 24), (160, 45)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("Actions"), "{width}x{height}: {text}");
            assert!(text.contains("Type to search"), "{width}x{height}: {text}");
        }
    }
}
