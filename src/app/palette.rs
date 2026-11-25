//! The palette in bzz's loop (v0.17 C1, C9, C12): opening it with the
//! facts of the moment and what it can find, its keys, the theme shown
//! behind its theme step, and running what was chosen through the paths a
//! key or a typed `:` command already take. The ranking, the steps and the
//! drawing are in `ui::palette`; the rows in `ui::action_table`.

use crossterm::event::KeyEvent;

use super::App;
use crate::{
    config::ClipboardMode,
    domain::{ChannelKind, Profile},
    error::Result,
    render::sanitize,
    ui::{
        action_table::{Needs, Run, TABLE},
        palette::{
            self, Chosen, Facts, Outcome, Palette, PaletteChannel, PaletteCommunity, PalettePerson,
            Sources,
        },
        state::{ConfirmationKind, FocusSurface, Overlay, Route},
        terminal::{TerminalGuard, Tui, copy_osc52},
        theme::{self, Theme, ThemeRegistry, ThemeScope},
        theme_picker::ThemePicker,
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

    /// The people and agents bzz knows, without me, by name (C12). An
    /// agent is one the relay verified or one of my local agents.
    fn palette_people(&self) -> Vec<PalettePerson> {
        let me = self.self_pubkey().unwrap_or_default().to_owned();
        let is_agent = |pubkey: &str| {
            self.agents.contains_key(pubkey)
                || self
                    .config
                    .hosted_agents
                    .iter()
                    .any(|agent| agent.pubkey.eq_ignore_ascii_case(pubkey))
        };
        let mut people = self
            .profiles
            .values()
            .filter(|profile| !profile.pubkey.eq_ignore_ascii_case(&me))
            .map(|profile| PalettePerson {
                pubkey: profile.pubkey.clone(),
                name: Profile::label(profile),
                agent: is_agent(&profile.pubkey),
            })
            .collect::<Vec<_>>();
        // A verified agent bzz has no profile of yet is still found.
        for agent in self.agents.values() {
            if agent.stale
                || agent.pubkey.eq_ignore_ascii_case(&me)
                || self.profiles.contains_key(&agent.pubkey)
            {
                continue;
            }
            people.push(PalettePerson {
                pubkey: agent.pubkey.clone(),
                name: agent.name.clone(),
                agent: true,
            });
        }
        people.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then(left.pubkey.cmp(&right.pubkey))
        });
        people
    }

    /// The emoji I reacted with in what is loaded, the newest first.
    fn palette_recent_emoji(&self) -> Vec<String> {
        let me = self.self_pubkey().unwrap_or_default();
        let mut mine = self
            .reactions
            .values()
            .flatten()
            .filter(|reaction| {
                !reaction.deleted
                    && reaction.emoji_url.is_none()
                    && reaction.pubkey.eq_ignore_ascii_case(me)
            })
            .map(|reaction| (reaction.created_at, reaction.emoji.clone()))
            .collect::<Vec<_>>();
        mine.sort_by_key(|reaction| std::cmp::Reverse(reaction.0));
        let mut recent = Vec::new();
        for (_, emoji) in mine {
            if !recent.contains(&emoji) {
                recent.push(emoji);
            }
            if recent.len() == 5 {
                break;
            }
        }
        recent
    }

    /// The installed apps of the open community, by name.
    async fn palette_apps(&self) -> Vec<String> {
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

    /// Everything the palette finds beside the table's actions.
    fn palette_sources(&self, apps: Vec<String>) -> Sources {
        Sources {
            channels: self.palette_channels(),
            people: self.palette_people(),
            apps,
            communities: self
                .config
                .communities
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != self.selected_community)
                .map(|(_, community)| PaletteCommunity {
                    id: community.id,
                    name: community.label.clone(),
                })
                .collect(),
            themes: ThemeRegistry::entries().collect(),
            theme: self
                .theme_before_preview
                .as_ref()
                .unwrap_or(&self.theme)
                .id()
                .to_owned(),
            recent_emoji: self.palette_recent_emoji(),
        }
    }

    /// What the palette's channel and people rows are built from.
    fn palette_source(&self) -> (usize, usize, usize) {
        (
            self.channels.len(),
            self.unread_channels().len(),
            self.profiles.len(),
        )
    }

    pub(super) async fn open_palette(&mut self) {
        let apps = self.palette_apps().await;
        let facts = self.palette_facts();
        let sources = self.palette_sources(apps);
        let candidates = palette::candidates(&self.keymap, &facts, &sources);
        let mut opened = Palette::open(
            candidates,
            sources,
            &facts,
            &self.palette_recent,
            (self.presentation.route, self.presentation.focus),
        );
        opened.source = self.palette_source();
        self.palette = Some(opened);
        self.presentation.open_overlay(Overlay::Palette);
    }

    /// Channels and people that arrive while the palette is open (it was
    /// opened while bzz was still syncing) are listed: the palette gets new
    /// candidates before a key and before it is drawn.
    pub(super) fn refresh_palette(&mut self) {
        let source = self.palette_source();
        if self
            .palette
            .as_ref()
            .is_none_or(|palette| palette.source == source)
        {
            return;
        }
        let facts = self.palette_facts();
        // The apps were read as it opened; they do not change meanwhile.
        let apps = self.palette.as_ref().map(Palette::apps).unwrap_or_default();
        let sources = self.palette_sources(apps);
        let candidates = palette::candidates(&self.keymap, &facts, &sources);
        let recent = self.palette_recent.clone();
        if let Some(palette) = self.palette.as_mut() {
            palette.replace_candidates(candidates, sources, &facts, &recent);
            palette.source = source;
        }
    }

    /// Show `id` behind the palette's theme step, keeping the theme to put
    /// back, as the theme picker previews.
    fn preview_palette_theme(&mut self, id: &str) {
        if self.theme_before_preview.is_none() {
            self.theme_before_preview = Some(self.theme.clone());
        }
        match theme::load(&self.paths, id) {
            Ok(loaded) => self.theme = loaded.theme,
            Err(_) => self.theme = Theme::builtin(id).unwrap_or_default(),
        }
    }

    /// The theme from before the palette's preview, back.
    fn restore_palette_theme(&mut self) {
        if self.theme_picker.is_none()
            && let Some(theme) = self.theme_before_preview.take()
        {
            self.theme = theme;
        }
    }

    /// Esc: the palette goes, the focus is where it was, and a previewed
    /// theme is put back.
    fn close_palette(&mut self) {
        if let Some(palette) = self.palette.take() {
            let (route, focus) = palette.return_to;
            self.presentation.route = route;
            self.presentation.focus = focus;
        }
        self.restore_palette_theme();
        if self.presentation.overlay == Some(Overlay::Palette) {
            self.presentation.close_overlay();
        }
    }

    /// A key while the palette is open. What does not run a choice is the
    /// palette's own (`ui::palette::Palette::key`).
    pub(super) fn palette_input(&mut self, key: KeyEvent) -> Option<Chosen> {
        self.refresh_palette();
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
            Outcome::Preview(id) => {
                self.preview_palette_theme(id);
                None
            }
            Outcome::Restore => {
                self.restore_palette_theme();
                None
            }
            Outcome::Run(chosen) => Some(chosen),
        }
    }

    pub(super) async fn palette_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        if let Some(chosen) = self.palette_input(key) {
            self.run_palette_choice(chosen, guard, terminal).await?;
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

    /// A message action chosen from another pane acts on the timeline's
    /// selected message, as `Space a` would from the timeline.
    fn palette_focus_for(&mut self, row: Option<usize>) {
        if row
            .and_then(|index| TABLE.get(index))
            .is_some_and(|spec| spec.needs == Needs::Message)
            && self.presentation.route == Route::Workspace
            && !matches!(
                self.presentation.focus,
                FocusSurface::Timeline | FocusSurface::Context
            )
        {
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
        }
    }

    async fn run_palette_choice(
        &mut self,
        chosen: Chosen,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        // The palette closes first: what runs may open its own overlay. A
        // theme kept from its step is not put back.
        self.palette = None;
        self.presentation.close_overlay();
        palette::remember(&mut self.palette_recent, chosen.recent());
        match chosen {
            Chosen::Channel(id) => self.open_palette_channel(id).await?,
            Chosen::Action(index) => {
                let Some(spec) = TABLE.get(index) else {
                    return Ok(());
                };
                self.palette_focus_for(Some(index));
                match spec.run {
                    Run::Ui(action) => {
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
                    // The palette asks a row's steps before it runs it.
                    Run::Steps(..) => {}
                }
            }
            Chosen::Command { text, row, .. } => {
                // One executor: what the steps built runs as if typed.
                self.restore_palette_theme();
                self.palette_focus_for(row);
                self.command = text;
                self.execute_command().await?;
            }
            Chosen::React { emoji, row } => {
                self.palette_focus_for(Some(row));
                self.send_reaction_emoji(emoji);
            }
            Chosen::Theme { id, .. } => self.keep_palette_theme(id),
            Chosen::Community(id) => {
                if let Some(index) = self
                    .config
                    .communities
                    .iter()
                    .position(|community| community.id == id)
                {
                    self.switch_community(index, guard, terminal).await?;
                }
            }
            Chosen::Search(text) => {
                self.open_search();
                self.search_state.query = text;
                self.search_state.changed();
                self.spawn_search();
            }
        }
        Ok(())
    }

    /// Keep `id` through the theme picker's own Enter: the same scope it
    /// opens with, saved the same way. Seam: once `:theme <name>` exists
    /// (agent `find`), the theme row runs that command instead.
    fn keep_palette_theme(&mut self, id: &str) {
        if self.theme_before_preview.is_none() {
            self.theme_before_preview = Some(self.theme.clone());
        }
        let scope = if self.config.communities.is_empty() {
            ThemeScope::Global
        } else {
            ThemeScope::Community
        };
        self.theme_picker = Some(ThemePicker::open(id, scope));
        self.confirm_theme_selection();
    }

    /// `:dm <npub|hex>…` opens the DM with these people; `:dm add
    /// <npub|hex>` adds one to the open DM. The DM picker's Enter, typed.
    pub(super) fn dm_command(&mut self, people: &str, add: bool) {
        let usage = if add {
            "usage: :dm add <npub|hex> (in a DM), or :dm add to pick"
        } else {
            "usage: :dm <npub|hex> [npub|hex…], or :dm to pick"
        };
        if self.runtime.is_none() {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return;
        }
        let me = self.self_pubkey().unwrap_or_default().to_owned();
        let mut keys = Vec::new();
        for word in people.split_whitespace() {
            let Ok(key) = nostr::PublicKey::parse(word) else {
                self.status_error = Some(format!(
                    "{} is not a key · {usage}",
                    sanitize::single_line(word)
                ));
                return;
            };
            let key = key.to_hex();
            if !key.eq_ignore_ascii_case(&me) && !keys.contains(&key) {
                keys.push(key);
            }
        }
        if keys.is_empty() {
            self.status_error = Some(usage.into());
            return;
        }
        if add {
            let Some(channel) = self
                .current_channel()
                .filter(|channel| channel.kind.is_dm())
                .map(|channel| channel.id)
            else {
                self.status_error = Some("open a DM first".into());
                return;
            };
            if keys.len() > 1 {
                self.status_error = Some(
                    "Add one participant at a time; the relay opens a new conversation.".into(),
                );
                return;
            }
            if self.refused_by_signer(
                "add someone to a DM",
                &[buzz_core::kind::KIND_DM_ADD_MEMBER as u16],
            ) {
                return;
            }
            self.start_dm_add(channel, keys.remove(0));
        } else if keys.len() > 8 {
            self.status_error =
                Some("A workspace DM supports at most eight other participants.".into());
        } else {
            self.start_dm_open(keys);
        }
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
        domain::{Channel, ChannelKind, Message, Profile, Visibility},
        paths::Paths,
        store::{Store, writer::StoreHandle},
        ui::{
            palette::{Chosen, footer_text},
            state::{FocusSurface, Overlay},
        },
    };

    const SCOUT: &str = "852b14d9852b14d9852b14d9852b14d9852b14d9852b14d9852b14d98520a120";

    fn profile(pubkey: &str, name: &str) -> Profile {
        Profile {
            pubkey: pubkey.into(),
            display_name: Some(name.into()),
            name: None,
            picture: None,
            nip05: None,
            about: None,
            event_id: String::new(),
            created_at: 1,
            verified: None,
            status: None,
            presence: None,
            in_huddle: false,
        }
    }

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
        app.profiles.insert(SCOUT.into(), profile(SCOUT, "Scout"));
        let ada = "a".repeat(64);
        app.profiles.insert(ada.clone(), profile(&ada, "Ada"));
        app
    }

    fn press(app: &mut super::App, code: KeyCode) -> Option<Chosen> {
        app.palette_input(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(app: &mut super::App, text: &str) {
        for character in text.chars() {
            assert_eq!(press(app, KeyCode::Char(character)), None);
        }
    }

    fn enter(app: &mut super::App) -> Option<Chosen> {
        press(app, KeyCode::Enter)
    }

    fn breadcrumb(app: &super::App) -> Option<String> {
        app.palette
            .as_ref()
            .and_then(|palette| palette.breadcrumb())
    }

    fn footer(app: &super::App) -> String {
        footer_text(app.palette.as_ref().unwrap()).0
    }

    fn npub(hex: &str) -> String {
        use nostr::nips::nip19::ToBech32 as _;
        nostr::PublicKey::from_hex(hex)
            .unwrap()
            .to_bech32()
            .unwrap()
    }

    #[tokio::test]
    async fn esc_closes_and_puts_the_focus_back() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.presentation.set_workspace_focus(FocusSurface::Channels);
        app.open_palette().await;
        assert_eq!(app.presentation.overlay, Some(Overlay::Palette));
        typed(&mut app, "thr");
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert_eq!(app.presentation.overlay, None);
        assert!(app.palette.is_none());
        assert_eq!(app.presentation.focus, FocusSurface::Channels);
    }

    #[tokio::test]
    async fn delete_over_another_persons_message_runs_nothing_and_stays_open() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette().await;
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
        app.open_palette().await;
        typed(&mut app, "#t9");
        assert_eq!(enter(&mut app), Some(Chosen::Channel(Uuid::from_u128(7))));
    }

    #[tokio::test]
    async fn the_theme_step_previews_and_esc_puts_the_theme_back() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let before = app.theme.id().to_owned();
        app.open_palette().await;
        typed(&mut app, "theme");
        assert_eq!(enter(&mut app), None);
        assert_eq!(breadcrumb(&app).as_deref(), Some("Change theme › theme"));
        // The step starts on the theme in use.
        assert_eq!(app.theme.id(), before);
        press(&mut app, KeyCode::Down);
        let previewed = app.theme.id().to_owned();
        assert_ne!(previewed, before, "moving shows the next theme");
        typed(&mut app, "nord");
        assert_eq!(app.theme.id(), "nord");
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert_eq!(app.theme.id(), before);
        assert!(app.theme_before_preview.is_none());
    }

    #[tokio::test]
    async fn the_theme_step_keeps_on_enter() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette().await;
        typed(&mut app, "theme");
        enter(&mut app);
        typed(&mut app, "nord");
        let chosen = enter(&mut app).expect("Enter keeps the theme");
        assert!(matches!(chosen, Chosen::Theme { id: "nord", .. }));
        app.palette = None;
        app.presentation.close_overlay();
        app.keep_palette_theme("nord");
        assert_eq!(app.theme.id(), "nord");
        assert_eq!(app.config.ui.theme, "nord");
        assert!(app.theme_before_preview.is_none());
    }

    #[tokio::test]
    async fn a_person_is_found_by_name_and_opens_their_profile_or_a_dm() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette().await;
        typed(&mut app, "@scout");
        assert!(footer(&app).contains("Ctrl+D DM"), "{}", footer(&app));
        // No identity in this test: a DM cannot be opened, and it says why.
        assert_eq!(
            app.palette_input(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            app.palette.as_ref().and_then(|palette| palette.notice()),
            Some("can't now: identity is locked or unavailable")
        );
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        app.open_palette().await;
        typed(&mut app, "scout");
        let Some(Chosen::Command { text, object, .. }) = enter(&mut app) else {
            panic!("Enter on a person opens the profile");
        };
        assert_eq!(text, format!("profile {}", npub(SCOUT)));
        assert_eq!(
            object,
            Some(crate::ui::palette::Recent::Person(SCOUT.into()))
        );
    }

    #[tokio::test]
    async fn the_last_row_searches_messages_for_the_query() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette().await;
        typed(&mut app, "launch plan");
        let palette = app.palette.as_ref().unwrap();
        assert_eq!(palette.item_titles().last(), Some(&"Search messages for"));
        for _ in 0..palette.item_count() {
            press(&mut app, KeyCode::Up);
            if app
                .palette
                .as_ref()
                .unwrap()
                .selected_candidate()
                .is_some_and(|row| row.title == "Search messages for")
            {
                break;
            }
        }
        assert_eq!(enter(&mut app), Some(Chosen::Search("launch plan".into())));
    }

    #[tokio::test]
    async fn the_dm_command_checks_its_keys() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.command = "dm notakey".into();
        app.execute_command().await.unwrap();
        // No identity in this test: the read-only answer, as `:dm` gives.
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|error| error.contains("read-only")),
            "{:?}",
            app.status_error
        );
    }

    #[tokio::test]
    async fn the_palette_reads_at_80x24_and_160x45() {
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        app.open_palette().await;
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
