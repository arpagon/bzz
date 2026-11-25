//! The app pane in bzz's loop (v0.16 M3): one session for each open app,
//! the commands that open and close one, its keys, its paste, and its
//! drawing. The rules that need no app are in `ui::app_pane`; the layout is
//! `ui::layout::panes_with_app`.
//!
//! **An app follows its channel (plan D4).** `:app open <name>` opens an
//! installed, enabled and approved app in the selected channel, and the
//! store remembers it there (`Apps::open_in`). Selecting another channel
//! shows that channel's app, or none. One app is shown in a channel:
//! opening another there closes the first.
//!
//! **A session stays alive while its channel is not selected,** so the
//! app's own state (its selection, an open form) is there when the owner
//! comes back, and its records keep syncing. At most `LIVE_SESSIONS` are
//! kept; past that the one shown least recently is closed, and opened
//! again from the store's records when its channel is selected. Every
//! session ends when bzz switches community or locks, and when it quits.
//!
//! **Nothing of an app costs the conversation anything.** The session never
//! blocks this loop: its waker sends one message on the background channel
//! and never waits; the loop polls every session on that message and on
//! every tick (a wake lost to a full channel is made up a tenth of a second
//! later), and the pane polls again just before it draws. A script that
//! loops is stopped by its own thread's budget; one stuck in a call costs a
//! stale screen and "not responding" in its pane.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use uuid::Uuid;

use super::App;
use crate::{
    error::Result,
    ui::{
        app_pane::{
            self, KeyContext, KeyOwner, PaneView, PasteContext, PasteTarget, key_owner,
            paste_target,
        },
        hit_map::{HitMap, HitTarget},
        layout::{self, AppPlacement, AppRequest},
        state::{FocusSurface, Overlay, Route},
    },
};

/// Sessions kept alive at once, the shown one among them.
#[cfg(feature = "apps")]
pub const LIVE_SESSIONS: usize = 4;

/// One line of `:apps`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppRow {
    pub name: String,
    pub line: String,
}

/// The `:apps` panel.
#[derive(Clone, Debug, Default)]
pub struct AppList {
    pub rows: Vec<AppRow>,
    pub selected: usize,
    /// A word under the list.
    pub note: Option<String>,
}

/// What bzz holds of apps in its window.
#[derive(Default)]
pub struct AppPanes {
    /// Of the thread and the app, the app was asked for last.
    pub front: bool,
    /// The owner asked for the whole window while the app has the focus.
    pub zoom: bool,
    /// Where the last layout put the pane.
    pub placement: AppPlacement,
    /// Where apps are open in the selected community, as the store keeps
    /// it: channel, app's name.
    pub open: HashMap<Uuid, String>,
    /// The community `open` was read for.
    pub open_of: Option<Uuid>,
    /// The `:apps` panel, while it is open.
    pub list: Option<AppList>,
    /// A message for the loop has been sent and not yet taken: further
    /// wakes need not send another.
    wake: std::sync::Arc<AtomicBool>,
    /// Counts what was shown, to close the session shown least recently.
    #[cfg(feature = "apps")]
    clock: u64,
    #[cfg(feature = "apps")]
    live: Vec<live::Live>,
    /// Failures and closings found while polling, for the loop to record
    /// in the store.
    #[cfg(feature = "apps")]
    pending: Vec<live::Pending>,
    /// The versions that wait for the owner's answer in the selected
    /// community, the one that has waited longest first.
    pub waiting: Vec<crate::store::apps::AppVersion>,
    /// When the store was last asked what runs, what waits and where apps
    /// are open; None: ask at the next tick.
    #[cfg(feature = "apps")]
    pub checked: Option<std::time::Instant>,
    /// The approval of a version, while its overlay is open.
    #[cfg(feature = "apps")]
    pub review: crate::ui::app_approval::AppApprovalState,
    /// A test's records in memory, in place of the relay's.
    #[cfg(all(test, feature = "apps"))]
    pub memory: Option<crate::apps::session::SharedStore>,
    /// The offers of apps in the channel shown, for the community it was
    /// made for (M6).
    #[cfg(feature = "apps")]
    pub offers: Option<(Uuid, crate::service::offers::OffersService)>,
    /// What came of offers lately, for `:apps`.
    pub offer_notes: Vec<String>,
    /// `bzz demo`'s Board: its records in memory, and the teammates who
    /// move its cards (`crate::demo::board`).
    #[cfg(feature = "apps")]
    pub demo: Option<crate::demo::board::DemoBoard>,
}

/// How often the store is asked again: an answer given at the shell, or by
/// another bzz of the owner's, reaches an open app within this.
#[cfg(feature = "apps")]
pub const STORE_CHECK: std::time::Duration = std::time::Duration::from_secs(1);

impl App {
    /// The selected channel of the workspace, if an app is open there.
    pub(super) fn shown_app(&self) -> Option<(Uuid, &str)> {
        if self.presentation.route != Route::Workspace {
            return None;
        }
        let channel = self.current_channel()?.id;
        self.apps
            .open
            .get(&channel)
            .map(|name| (channel, name.as_str()))
    }

    /// The app of the selected channel has the keyboard.
    pub(super) fn app_has_focus(&self) -> bool {
        self.presentation.focus == FocusSurface::App && self.shown_app().is_some()
    }

    pub(super) fn app_request(&self) -> AppRequest {
        AppRequest {
            open: self.shown_app().is_some(),
            focused: self.app_has_focus(),
            zoom: self.apps.zoom,
            front: self.apps.front,
        }
    }

    /// Gives the app of the selected channel the keyboard and the right
    /// side. Says so when there is none.
    pub(super) fn focus_app(&mut self) {
        if self.shown_app().is_none() {
            self.status_error = Some(self.no_app_here());
            return;
        }
        self.apps.front = true;
        self.presentation.set_workspace_focus(FocusSurface::App);
    }

    fn no_app_here(&self) -> String {
        match self.current_channel() {
            Some(channel) if channel.kind.is_dm() => {
                "no app here: an app cannot be opened in a direct message".into()
            }
            Some(_) => "no app is open in this channel; :apps lists the installed ones".into(),
            None => "select a channel first".into(),
        }
    }

    /// The thread a new message replies into: the open thread, only while it
    /// is drawn. Never one the owner cannot see because it does not fit or
    /// waits behind the app.
    pub(super) fn reply_root(&self) -> Option<String> {
        self.thread_root.clone().filter(|_| self.thread_drawn)
    }

    /// `Ctrl+C`: the keyboard goes back to the conversation, and the
    /// workspace is shown again.
    pub(super) fn leave_app(&mut self) {
        self.apps.zoom = false;
        self.presentation
            .set_workspace_focus(FocusSurface::Timeline);
    }

    /// Who owns `key` now (`ui::app_pane::key_owner`).
    pub(super) fn key_owner(&self, key: KeyEvent) -> KeyOwner {
        key_owner(
            key,
            KeyContext {
                // An agent's command, or a version of an app: Ctrl+Y opens
                // it even while the app has the keyboard.
                approval_waiting: !self.approvals.is_empty() || !self.apps.waiting.is_empty(),
                approval_open: matches!(
                    self.presentation.overlay,
                    Some(Overlay::Approval | Overlay::AppApproval)
                ),
                overlay_open: self.presentation.overlay.is_some(),
                composer_open: self.presentation.composer_target.is_some(),
                app_focused: self.app_has_focus(),
            },
        )
    }

    /// A key while the app has the focus. Answers whether it was the app's
    /// (or the one that leaves it).
    pub(super) fn app_key(&mut self, key: KeyEvent) -> bool {
        match self.key_owner(key) {
            KeyOwner::LeaveApp => {
                self.leave_app();
                true
            }
            KeyOwner::App => {
                self.send_to_app(key);
                true
            }
            _ => false,
        }
    }

    #[cfg(feature = "apps")]
    fn send_to_app(&mut self, key: KeyEvent) {
        let Some((channel, _)) = self.shown_app() else {
            return;
        };
        if let Some(board) = &mut self.apps.demo {
            board.typed(std::time::Instant::now());
        }
        if let Some(key) = crate::apps::runtime::Key::from_crossterm(key)
            && let Some(session) = self.apps.session_mut(channel)
        {
            session.key(key);
        }
    }

    #[cfg(not(feature = "apps"))]
    fn send_to_app(&mut self, _key: KeyEvent) {}

    /// Where a bracketed paste goes now.
    pub(super) fn paste_target(&self) -> PasteTarget {
        paste_target(PasteContext {
            overlay: self.presentation.overlay,
            composer_open: self.presentation.composer_target.is_some(),
            app_focused: self.app_has_focus(),
        })
    }

    /// A paste for the app with the focus: one piece of text.
    pub(super) fn paste_to_app(&mut self, text: &str) {
        let Some(text) = app_pane::app_paste_text(text) else {
            return;
        };
        #[cfg(feature = "apps")]
        if let Some(board) = &mut self.apps.demo {
            board.typed(std::time::Instant::now());
        }
        #[cfg(feature = "apps")]
        if let Some((channel, _)) = self.shown_app()
            && let Some(session) = self.apps.session_mut(channel)
        {
            session.paste(text);
        }
        #[cfg(not(feature = "apps"))]
        let _ = text;
    }

    /// What the status line says of the app, if anything: an app that is
    /// open and not drawn, an app's error, an app that does not respond.
    pub(super) fn app_status_line(&self) -> Option<String> {
        let (channel, _) = self.shown_app()?;
        match self.apps.placement {
            AppPlacement::HiddenNarrow => return Some("app hidden · 5".into()),
            AppPlacement::HiddenBehindThread => return Some("app behind thread · 5".into()),
            _ => {}
        }
        #[cfg(feature = "apps")]
        {
            self.apps.trouble(channel)
        }
        #[cfg(not(feature = "apps"))]
        {
            let _ = channel;
            None
        }
    }

    /// Draws the pane of the selected channel's app into `area`.
    pub(super) fn render_app_pane(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        zoomed: bool,
        hit_map: &mut HitMap,
    ) {
        hit_map.push(area, HitTarget::App);
        let focused = self.app_has_focus();
        let Some((channel, name)) = self.shown_app() else {
            return;
        };
        let name = name.to_owned();
        let takes_cursor = focused
            && self.presentation.overlay.is_none()
            && self.presentation.composer_target.is_none();
        #[cfg(feature = "apps")]
        {
            let theme = &self.theme;
            let live = self
                .apps
                .live
                .iter_mut()
                .find(|live| live.channel == channel);
            let title = live
                .as_ref()
                .map_or_else(|| name.clone(), |live| live.title.clone());
            let notice = live.as_ref().and_then(|live| live.notice());
            let region = app_pane::render_frame(
                frame,
                area,
                PaneView {
                    title: &title,
                    focused,
                    zoomed,
                    notice: notice.as_deref(),
                },
                theme,
            );
            if !layout::fits_an_app(region) {
                app_pane::render_words(
                    frame,
                    region,
                    &app_pane::too_small_text(region, zoomed),
                    false,
                    theme,
                );
                return;
            }
            match live {
                Some(live) => {
                    if let Some(cursor) = live.draw(frame, region, theme)
                        && takes_cursor
                    {
                        frame.set_cursor_position(cursor);
                    }
                }
                None => app_pane::render_words(frame, region, "Opening...", false, theme),
            }
        }
        #[cfg(not(feature = "apps"))]
        {
            let _ = (channel, takes_cursor);
            let region = app_pane::render_frame(
                frame,
                area,
                PaneView {
                    title: &name,
                    focused,
                    zoomed,
                    notice: None,
                },
                &self.theme,
            );
            if layout::fits_an_app(region) {
                app_pane::render_words(
                    frame,
                    region,
                    "This bzz was built without apps.",
                    true,
                    &self.theme,
                );
            }
        }
    }

    /// Whether an app session runs: its timers and records are looked at
    /// on the loop's busy housekeeping (v0.17 L9).
    pub(super) fn apps_running(&self) -> bool {
        #[cfg(feature = "apps")]
        {
            !self.apps.live.is_empty()
        }
        #[cfg(not(feature = "apps"))]
        {
            false
        }
    }

    /// The loop took the wake message.
    pub(super) fn app_woken(&mut self) {
        self.apps.wake.store(false, Ordering::SeqCst);
    }

    /// Polls every session: what the scripts and the records produced is
    /// carried between them. Answers whether the shown app has something
    /// new to draw. Never waits.
    pub(super) fn poll_apps(&mut self) -> bool {
        #[cfg(feature = "apps")]
        {
            let shown = self.shown_app().map(|(channel, _)| channel);
            self.apps.poll(shown)
        }
        #[cfg(not(feature = "apps"))]
        {
            false
        }
    }

    /// Every tick: time for the sessions (timers fire only in an app that
    /// has them), names from bzz's profiles, the store's word on where apps
    /// are open, the session of the selected channel's app started, and
    /// what polling found recorded. Answers whether to draw again.
    pub(super) async fn tick_apps(&mut self) -> Result<bool> {
        #[cfg(feature = "apps")]
        {
            self.watch_offers();
            let mut changed = self.reconcile_apps().await?;
            if self
                .apps
                .checked
                .is_none_or(|checked| checked.elapsed() >= STORE_CHECK)
            {
                changed |= self.sync_apps().await?;
            }
            for live in &mut self.apps.live {
                changed |= live.watch();
            }
            // The demo's teammates move a card now and then, while the
            // Board is on screen; its session hears of it as it is polled.
            let board_shown = self.apps.placement.is_drawn()
                && self
                    .shown_app()
                    .is_some_and(|(_, name)| name == crate::demo::board::NAME);
            if let Some(board) = &mut self.apps.demo {
                board.tick(board_shown, std::time::Instant::now());
            }
            let shown = self.shown_app().map(|(channel, _)| channel);
            let profiles = &self.profiles;
            changed |= self
                .apps
                .tick(shown, |id| profiles.get(id).map(|profile| profile.label()));
            self.look_up_app_names();
            changed |= self.poll_apps();
            changed |= self.settle_apps().await?;
            Ok(changed)
        }
        #[cfg(not(feature = "apps"))]
        {
            Ok(false)
        }
    }

    /// The ids an app shows (`host.name`, or a record's signer) that bzz
    /// has no profile of are looked up as the timeline's authors are; when
    /// a profile arrives, the next tick gives the name to the app, which
    /// is drawn again. Only what can be a public key is asked for, each id
    /// once, and a bounded number at a time.
    #[cfg(feature = "apps")]
    fn look_up_app_names(&mut self) {
        {
            const AT_ONCE: usize = 64;
            let is_key =
                |id: &str| id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit());
            let unknown: std::collections::HashSet<String> = self
                .apps
                .unnamed()
                .into_iter()
                .filter(|id| is_key(id) && !self.profile_requested.contains(id))
                .take(AT_ONCE)
                .collect();
            if unknown.is_empty() {
                return;
            }
            let Some(runtime) = &self.runtime else {
                return;
            };
            self.profile_requested.extend(unknown.iter().cloned());
            let profiles = runtime.profiles.clone();
            let tx = self.background_tx.clone();
            tokio::spawn(async move {
                let result = profiles.hydrate(unknown).await;
                let _ = tx
                    .send(match result {
                        Ok(_) => super::Background::Changed,
                        Err(error) => super::Background::Failed(error.to_string()),
                    })
                    .await;
            });
        }
    }

    /// Ends every session: bzz locks, quits, or leaves the community.
    pub(super) fn close_all_apps(&mut self) {
        #[cfg(feature = "apps")]
        self.apps.live.clear();
        #[cfg(feature = "apps")]
        self.stop_offers();
        self.apps.offer_notes.clear();
        self.apps.open.clear();
        self.apps.open_of = None;
        self.apps.zoom = false;
        if self.presentation.focus == FocusSurface::App {
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
        }
    }

    /// `:app open <name> | close | zoom`.
    pub(super) async fn app_command(&mut self, arguments: &str) -> Result<()> {
        let mut words = arguments.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("open"), Some(name), None) => self.open_app_here(name).await,
            (Some("close"), None, None) => self.close_app_here().await,
            (Some("zoom"), None, None) => {
                if self.shown_app().is_some() {
                    self.apps.zoom = !self.apps.zoom;
                    self.focus_app();
                } else {
                    self.status_error = Some(self.no_app_here());
                }
                Ok(())
            }
            (Some(verb), _, _) => {
                let rest = arguments
                    .trim_start()
                    .strip_prefix(verb)
                    .unwrap_or_default();
                if !self.manage_command(verb, rest).await? {
                    self.status_error = Some(super::manage::USAGE.into());
                }
                Ok(())
            }
            (None, _, _) => {
                self.status_error = Some(super::manage::USAGE.into());
                Ok(())
            }
        }
    }

    /// `Space 5`: zoom the app of this channel, or back.
    pub(super) fn zoom_app(&mut self) {
        if self.shown_app().is_some() {
            self.apps.zoom = !self.apps.zoom;
            self.focus_app();
        } else {
            self.status_error = Some(self.no_app_here());
        }
    }

    #[cfg(not(feature = "apps"))]
    async fn open_app_here(&mut self, _name: &str) -> Result<()> {
        self.status_error = Some("this bzz was built without apps".into());
        Ok(())
    }

    #[cfg(not(feature = "apps"))]
    async fn close_app_here(&mut self) -> Result<()> {
        self.status_error = Some("this bzz was built without apps".into());
        Ok(())
    }

    #[cfg(not(feature = "apps"))]
    pub(super) async fn open_apps_panel(&mut self) -> Result<()> {
        self.status_error = Some("this bzz was built without apps".into());
        Ok(())
    }

    /// A key in the `:apps` panel: `j`/`k` move, `Enter` opens the app in
    /// the selected channel, `Esc`/`q` close.
    pub(super) async fn apps_panel_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;
        let Some(list) = self.apps.list.as_mut() else {
            self.presentation.close_overlay();
            return Ok(());
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.apps.list = None;
                self.presentation.close_overlay();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                list.selected = (list.selected + 1).min(list.rows.len().saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => list.selected = list.selected.saturating_sub(1),
            KeyCode::Char('a') => {
                let name = list.rows.get(list.selected).map(|row| row.name.clone());
                self.apps.list = None;
                self.presentation.close_overlay();
                if let Some(name) = name {
                    return self.open_app_review(Some(&name)).await;
                }
            }
            KeyCode::Enter => {
                let name = list.rows.get(list.selected).map(|row| row.name.clone());
                self.apps.list = None;
                self.presentation.close_overlay();
                if let Some(name) = name {
                    return self.open_app_here(&name).await;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The `:apps` panel's lines, and which is selected.
    pub(super) fn apps_panel_lines(&self) -> Vec<(String, bool)> {
        let Some(list) = &self.apps.list else {
            return Vec::new();
        };
        let mut lines: Vec<(String, bool)> = list
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.line.clone(), index == list.selected))
            .collect();
        if let Some(note) = &list.note {
            if !lines.is_empty() {
                lines.push((String::new(), false));
            }
            lines.extend(note.lines().map(|line| (line.to_owned(), false)));
        }
        lines
    }
}

#[cfg(feature = "apps")]
impl App {
    /// A folder in development runs in `channel`: nothing approved is open
    /// there.
    pub(super) fn dev_open_in(&self, channel: Uuid) -> bool {
        self.apps
            .live
            .iter()
            .any(|live| live.channel == channel && live.is_dev())
    }

    /// A waker for a session: one message on the background channel, sent
    /// without waiting, and none while one is still to be taken.
    fn app_waker(&self) -> crate::apps::runtime::Waker {
        let sender = self.background_tx.clone();
        let pending = self.apps.wake.clone();
        std::sync::Arc::new(move || {
            if !pending.swap(true, Ordering::SeqCst)
                && sender.try_send(super::Background::AppWake).is_err()
            {
                // The channel is full: the next tick polls anyway.
                pending.store(false, Ordering::SeqCst);
            }
        })
    }

    /// Reads where apps are open when the community changed, and starts the
    /// session of the selected channel's app if it is not running.
    async fn reconcile_apps(&mut self) -> Result<bool> {
        let community = self.active_community_id();
        if self.records_here().is_none() {
            // Records need the relay: a locked or offline bzz runs no app.
            let before = self.apps.live.len();
            self.apps
                .live
                .retain(|live| matches!(live.run, live::Run::Problem(_)));
            if self.apps.live.len() != before {
                return Ok(true);
            }
        }
        if self.apps.open_of != community {
            self.apps.live.clear();
            self.apps.open.clear();
            self.apps.waiting.clear();
            self.apps.checked = None;
            self.apps.open_of = community;
            if let Some(community) = community {
                let root = self.paths.apps_dir();
                let open = self
                    .store
                    .call(move |store| {
                        let mut open = HashMap::new();
                        if let Ok(listed) =
                            crate::apps::store::Apps::at(store, root).list(community)
                        {
                            // A disabled app is open nowhere until it is
                            // enabled again.
                            for app in listed.into_iter().filter(|app| app.app.enabled) {
                                for channel in app.channels {
                                    open.entry(channel).or_insert_with(|| app.app.name.clone());
                                }
                            }
                        }
                        Ok(open)
                    })
                    .await?;
                self.apps.open = open;
            }
            return Ok(true);
        }
        let Some((channel, name)) = self
            .shown_app()
            .map(|(channel, name)| (channel, name.to_owned()))
        else {
            return Ok(false);
        };
        self.apps.clock += 1;
        let clock = self.apps.clock;
        if let Some(live) = self
            .apps
            .live
            .iter_mut()
            .find(|live| live.channel == channel)
        {
            live.shown_at = clock;
            return Ok(false);
        }
        self.start_app(channel, &name).await?;
        Ok(true)
    }

    /// Where an app's records are: the relay's service, or (in a test) a
    /// store in memory. None while bzz is not connected.
    fn records_here(&self) -> Option<live::Where<'_>> {
        #[cfg(test)]
        if let Some(memory) = &self.apps.memory {
            return Some(live::Where::Memory(memory));
        }
        // `bzz demo`: the Board's cards are held in memory.
        if let Some(board) = &self.apps.demo {
            return Some(live::Where::Memory(board.store()));
        }
        self.runtime
            .as_ref()
            .map(|runtime| live::Where::Relay(&runtime.records))
    }

    /// Starts the session of `name` in `channel`, from the one function
    /// that hands out an approved script (`Apps::runnable`). What keeps it
    /// from running is shown in its pane.
    async fn start_app(&mut self, channel: Uuid, name: &str) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let root = self.paths.apps_dir();
        let wanted = name.to_owned();
        let runnable = self
            .store
            .call(move |store| {
                Ok(crate::apps::store::Apps::at(store, root)
                    .runnable(community, &wanted)
                    .map_err(|error| live::why_not(&wanted, &error)))
            })
            .await?;
        let archived = self
            .channels
            .iter()
            .find(|value| value.id == channel)
            .is_some_and(|value| value.facts.archived);
        let names: HashMap<String, String> = self
            .profiles
            .iter()
            .map(|(id, profile)| (id.clone(), profile.label()))
            .collect();
        let waker = self.app_waker();
        let run = match (runnable, self.records_here()) {
            (Err(why), _) => live::Run::Problem(why),
            (Ok(_), None) => live::Run::Problem(
                "This app's records are on the relay, and bzz is not connected.".into(),
            ),
            (Ok(runnable), Some(records)) => {
                live::Run::start(runnable, waker, names, archived, records, channel)
            }
        };
        self.apps.clock += 1;
        let title = match &run {
            live::Run::Session { runnable, .. } => runnable.title().to_owned(),
            _ => name.to_owned(),
        };
        self.apps.live.push(live::Live {
            community,
            channel,
            name: name.to_owned(),
            title,
            shown_at: self.apps.clock,
            run,
        });
        self.apps.evict(LIVE_SESSIONS);
        Ok(())
    }

    /// Asks the store what runs, what waits and where apps are open, and
    /// brings the open apps to it: one whose current version changed (an
    /// approval here or at the shell, a rollback, a version that failed)
    /// is given that version in place; one that is disabled or removed
    /// closes; one that could not run before and can now is started. The
    /// versions that wait are what the banner and `Ctrl+Y` show.
    pub(super) async fn sync_apps(&mut self) -> Result<bool> {
        use crate::apps::store::{AppError, Apps};
        self.apps.checked = Some(std::time::Instant::now());
        let Some(community) = self.active_community_id() else {
            let changed = !self.apps.waiting.is_empty();
            self.apps.waiting.clear();
            return Ok(changed);
        };
        if self.apps.open_of != Some(community) {
            return Ok(false);
        }
        enum Now {
            Same,
            Gone,
            Disabled,
            Run(Box<crate::apps::store::Runnable>),
            Problem(String),
        }
        let asked: Vec<(Uuid, String, Option<u32>, bool)> = self
            .apps
            .live
            .iter()
            .filter(|live| !live.is_dev() && live.community == community)
            .map(|live| {
                (
                    live.channel,
                    live.name.clone(),
                    live.version(),
                    live.is_problem(),
                )
            })
            .collect();
        let root = self.paths.apps_dir();
        let (open, waiting, now) = self
            .store
            .call(move |store| {
                let apps = Apps::at(store, root);
                let mut open = HashMap::new();
                if let Ok(listed) = apps.list(community) {
                    for app in listed.into_iter().filter(|app| app.app.enabled) {
                        for channel in app.channels {
                            open.entry(channel).or_insert_with(|| app.app.name.clone());
                        }
                    }
                }
                let waiting = apps.waiting(community).unwrap_or_default();
                let mut now = Vec::new();
                for (channel, name, running, problem) in asked {
                    let state = match apps.get(community, &name) {
                        Err(AppError::NotInstalled(_)) => Now::Gone,
                        Err(_) => Now::Same,
                        Ok(listed) if !listed.app.enabled => Now::Disabled,
                        Ok(listed) if listed.app.current == running && !problem => Now::Same,
                        Ok(_) => match apps.runnable(community, &name) {
                            Ok(runnable) => Now::Run(Box::new(runnable)),
                            Err(error) => Now::Problem(live::why_not(&name, &error)),
                        },
                    };
                    now.push((channel, name, state));
                }
                Ok((open, waiting, now))
            })
            .await?;
        let ids = |list: &[crate::store::apps::AppVersion]| {
            list.iter()
                .map(|version| (version.name.clone(), version.version))
                .collect::<Vec<_>>()
        };
        let mut changed = ids(&waiting) != ids(&self.apps.waiting);
        self.apps.waiting = waiting;
        if self.presentation.overlay == Some(Overlay::AppApproval) {
            self.review_still_waits();
        }
        // Where apps are open, as the store says; a folder in development
        // is open only here.
        let mut open = open;
        for live in self.apps.live.iter().filter(|live| live.is_dev()) {
            open.insert(live.channel, live.name.clone());
        }
        if open != self.apps.open {
            self.apps.open = open;
            changed = true;
        }
        for (channel, name, state) in now {
            match state {
                Now::Same => {}
                Now::Gone | Now::Disabled => {
                    let removed = matches!(state, Now::Gone);
                    self.apps.live.retain(|live| live.channel != channel);
                    if self.apps.open.get(&channel) == Some(&name) {
                        self.apps.open.remove(&channel);
                    }
                    self.status_error = Some(if removed {
                        format!(
                            "{name} was removed, and is closed; its records stay in the channel"
                        )
                    } else {
                        format!(
                            "{name} is disabled, and is closed; :app enable {name} lets it run again"
                        )
                    });
                    changed = true;
                }
                Now::Run(runnable) => {
                    let Some(live) = self
                        .apps
                        .live
                        .iter_mut()
                        .find(|live| live.channel == channel)
                    else {
                        continue;
                    };
                    if !live.update(*runnable) {
                        // Nothing runs to give it to: it starts afresh.
                        self.apps.live.retain(|live| live.channel != channel);
                    }
                    changed = true;
                }
                Now::Problem(why) => {
                    if let Some(live) = self
                        .apps
                        .live
                        .iter_mut()
                        .find(|live| live.channel == channel)
                        && !matches!(&live.run, live::Run::Problem(was) if *was == why)
                    {
                        live.run = live::Run::Problem(why);
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Records what polling found: a version that failed as it was put to
    /// run goes back to the one before it (`Apps::failed_to_run`), and an
    /// app that asked to close is closed in its channel.
    async fn settle_apps(&mut self) -> Result<bool> {
        let pending = std::mem::take(&mut self.apps.pending);
        if pending.is_empty() {
            return Ok(false);
        }
        let root = self.paths.apps_dir();
        for item in pending {
            match item {
                live::Pending::Failed {
                    community,
                    channel,
                    name,
                    version,
                    error,
                    kept,
                } => {
                    let (store_root, failed_name, failed_error) =
                        (root.clone(), name.clone(), error.clone());
                    let current = self
                        .store
                        .call(move |store| {
                            Ok(crate::apps::store::Apps::at(store, store_root)
                                .failed_to_run(community, &failed_name, version, &failed_error)
                                .ok()
                                .flatten())
                        })
                        .await?;
                    let first = error.lines().next().unwrap_or_default().to_owned();
                    match (kept, current) {
                        // The version before it never stopped running in
                        // the pane, and is the current one again.
                        (Some(kept), _) => {
                            self.status_error = Some(format!(
                                "{name}: version {version} failed to run; version {kept} goes on: {first}"
                            ));
                        }
                        (None, Some(earlier)) if earlier != version => {
                            // The version that ran before is current again;
                            // it starts at the next tick.
                            self.apps.live.retain(|live| live.channel != channel);
                            self.status_error = Some(format!(
                                "{name}: version {version} failed to run, and version {earlier} runs again"
                            ));
                        }
                        _ => {
                            if let Some(live) = self
                                .apps
                                .live
                                .iter_mut()
                                .find(|live| live.channel == channel)
                            {
                                live.run = live::Run::Problem(format!(
                                    "{error}\n\nNo earlier approved version can run instead."
                                ));
                            }
                        }
                    }
                    // What the store says runs now is checked at once.
                    self.apps.checked = None;
                }
                live::Pending::Updated {
                    name,
                    from,
                    version,
                } => {
                    self.status_error = Some(format!(
                        "{name}: version {version} runs in place of {from}, its state kept"
                    ));
                }
                live::Pending::Closed { channel } => {
                    // The demo's Board closes on its own `q`, as anywhere;
                    // a newcomer is told the way back.
                    let demo_board = self.apps.demo.is_some()
                        && self.apps.open.get(&channel).map(String::as_str)
                            == Some(crate::demo::board::NAME);
                    self.forget_app(channel).await?;
                    if demo_board {
                        self.status_error = Some(crate::demo::BOARD_CLOSED.into());
                    }
                }
            }
        }
        Ok(true)
    }

    /// `:app dev <folder>`: the owner's own folder in the pane of this
    /// channel, on its records, reloaded on save, without approval (plan
    /// D5; `app/pane/dev.rs` says whose records and what ends it).
    pub(super) async fn open_dev_here(&mut self, path: &std::path::Path) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("select a community first".into());
            return Ok(());
        };
        let Some(channel) = self.current_channel().cloned() else {
            self.status_error = Some("select a channel first".into());
            return Ok(());
        };
        if channel.kind.is_dm() {
            self.status_error =
                Some("an app cannot be opened in a direct message; open it in a channel".into());
            return Ok(());
        }
        let opened = match dev::DevFolder::open(path) {
            Ok(opened) => opened,
            Err(error) => {
                self.status_error = Some(error);
                return Ok(());
            }
        };
        let Some(records) = self.records_here() else {
            self.status_error = Some(
                "an app's records are on the relay: unlock the identity and connect first".into(),
            );
            return Ok(());
        };
        let names: HashMap<String, String> = self
            .profiles
            .iter()
            .map(|(id, profile)| (id.clone(), profile.label()))
            .collect();
        let script_name = opened.folder.script.file_name().map_or_else(
            || "app.lua".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let options = crate::apps::session::SessionOptions::new(opened.name.clone())
            .script(script_name)
            .waker(self.app_waker())
            .names(names)
            .archived(channel.facts.archived);
        let session = match records.session(opened.script, options, channel.id) {
            Ok(session) => session,
            Err(error) => {
                self.status_error = Some(format!("the folder's app could not be started: {error}"));
                return Ok(());
            }
        };
        self.apps.live.retain(|live| live.channel != channel.id);
        self.apps.clock += 1;
        let title = opened.folder.label(&opened.title);
        let place = opened.folder.dir.display().to_string();
        self.apps.live.push(live::Live {
            community,
            channel: channel.id,
            name: opened.name.clone(),
            title,
            shown_at: self.apps.clock,
            run: live::Run::Dev {
                session: Box::new(session),
                folder: opened.folder,
            },
        });
        self.apps.evict(LIVE_SESSIONS);
        self.apps.open.insert(channel.id, opened.name.clone());
        self.apps.open_of = Some(community);
        self.apps.zoom = false;
        self.focus_app();
        self.status_error = Some(format!(
            "dev: {} runs in #{} on its records (bzz.app.{}.*), not approved, reloaded on every save · :app close ends it",
            crate::render::sanitize::single_line(&place),
            crate::render::sanitize::single_line(&channel.name),
            opened.name
        ));
        Ok(())
    }

    async fn open_app_here(&mut self, name: &str) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("select a community first".into());
            return Ok(());
        };
        let Some(channel) = self.current_channel().cloned() else {
            self.status_error = Some("select a channel first".into());
            return Ok(());
        };
        if channel.kind.is_dm() {
            self.status_error =
                Some("an app cannot be opened in a direct message; open it in a channel".into());
            return Ok(());
        }
        if self.records_here().is_none() {
            self.status_error = Some(
                "an app's records are on the relay: unlock the identity and connect first".into(),
            );
            return Ok(());
        }
        let root = self.paths.apps_dir();
        let wanted = name.to_owned();
        let channel_id = channel.id;
        let replaced = self.apps.open.get(&channel_id).cloned();
        let opened = self
            .store
            .call(move |store| {
                let mut apps = crate::apps::store::Apps::at(store, root);
                if let Err(error) = apps.runnable(community, &wanted) {
                    return Ok(Err(live::why_not(&wanted, &error)));
                }
                if let Some(previous) = replaced.filter(|previous| *previous != wanted) {
                    let _ = apps.close_in(community, &previous, channel_id);
                }
                Ok(apps
                    .open_in(community, &wanted, channel_id)
                    .map_err(|error| error.to_string()))
            })
            .await?;
        if let Err(why) = opened {
            self.status_error = Some(why);
            return Ok(());
        }
        // A folder in development there ends: what opens is approved.
        self.apps
            .live
            .retain(|live| !(live.channel == channel_id && live.is_dev()));
        if self.apps.open.get(&channel_id).map(String::as_str) != Some(name) {
            self.apps.live.retain(|live| live.channel != channel_id);
        }
        self.apps.open.insert(channel_id, name.to_owned());
        self.apps.open_of = Some(community);
        self.apps.zoom = false;
        if !self.apps.live.iter().any(|live| live.channel == channel_id) {
            self.start_app(channel_id, name).await?;
        }
        self.focus_app();
        self.status_error = Some(format!(
            "{name} is open in #{} · Ctrl-c leaves it, 5 comes back to it",
            crate::render::sanitize::single_line(&channel.name)
        ));
        Ok(())
    }

    async fn close_app_here(&mut self) -> Result<()> {
        let Some((channel, name)) = self
            .shown_app()
            .map(|(channel, name)| (channel, name.to_owned()))
        else {
            self.status_error = Some(self.no_app_here());
            return Ok(());
        };
        self.forget_app(channel).await?;
        self.status_error = Some(format!(
            "{name} is closed here; its records stay in the channel"
        ));
        Ok(())
    }

    /// Closes the app of `channel` and forgets it there. A folder in
    /// development was never remembered: what the store has open there
    /// shows again.
    async fn forget_app(&mut self, channel: Uuid) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let dev = self
            .apps
            .live
            .iter()
            .any(|live| live.channel == channel && live.is_dev());
        self.apps.live.retain(|live| live.channel != channel);
        if dev {
            self.apps.open.remove(&channel);
            self.apps.checked = None;
            if self.presentation.focus == FocusSurface::App {
                self.leave_app();
            }
            return Ok(());
        }
        if let Some(name) = self.apps.open.remove(&channel) {
            let root = self.paths.apps_dir();
            self.store
                .call(move |store| {
                    let _ = crate::apps::store::Apps::at(store, root)
                        .close_in(community, &name, channel);
                    Ok(())
                })
                .await?;
        }
        if self.presentation.focus == FocusSurface::App {
            self.leave_app();
        }
        Ok(())
    }

    /// `:apps`: the installed apps of the community and where each is open.
    pub(super) async fn open_apps_panel(&mut self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("select a community first".into());
            return Ok(());
        };
        let root = self.paths.apps_dir();
        let (listed, shares) = self
            .store
            .call(move |store| {
                let shares = store.app_shares(community)?;
                Ok((
                    crate::apps::store::Apps::at(store, root)
                        .list(community)
                        .map_err(|error| error.to_string()),
                    shares,
                ))
            })
            .await?;
        let listed = match listed {
            Ok(listed) => listed,
            Err(error) => {
                self.status_error = Some(error);
                return Ok(());
            }
        };
        let channel_name = |id: &Uuid| {
            self.channels
                .iter()
                .find(|channel| channel.id == *id)
                .map_or_else(
                    || "a channel not listed here".to_owned(),
                    |channel| format!("#{}", crate::render::sanitize::single_line(&channel.name)),
                )
        };
        let rows = listed
            .iter()
            .map(|app| {
                let state = match (&app.running, app.app.enabled) {
                    (_, false) => "disabled".to_owned(),
                    (Some(version), true) => format!("version {}", version.version),
                    (None, true) => "not approved".to_owned(),
                };
                let offered: Vec<String> = app
                    .waiting
                    .iter()
                    .filter(|version| {
                        version.proposer.kind == crate::store::apps::ProposerKind::Member
                    })
                    .map(|version| {
                        format!(
                            "offered by {}{}",
                            crate::render::sanitize::single_line(
                                version.proposer.name.as_deref().unwrap_or("someone")
                            ),
                            version
                                .proposer
                                .channel
                                .map(|channel| format!(" in {}", channel_name(&channel)))
                                .unwrap_or_default()
                        )
                    })
                    .collect();
                let waiting = match (app.waiting.len(), offered.is_empty()) {
                    (0, _) => String::new(),
                    (n, true) => format!(" · {n} waiting"),
                    (n, false) => format!(" · {n} waiting ({})", offered.join(", ")),
                };
                let shared: Vec<String> = shares
                    .iter()
                    .filter(|share| share.name == app.app.name && share.is_shared())
                    .map(|share| channel_name(&share.channel))
                    .collect();
                let shared = if shared.is_empty() {
                    String::new()
                } else {
                    format!(" · shared in {}", shared.join(", "))
                };
                let open = if app.channels.is_empty() {
                    "open nowhere".to_owned()
                } else {
                    format!(
                        "open in {}",
                        app.channels
                            .iter()
                            .map(channel_name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                AppRow {
                    name: app.app.name.clone(),
                    line: format!(
                        "{} ({}) · {state}{waiting} · {open}{shared}",
                        app.app.name,
                        crate::render::sanitize::single_line(&app.title())
                    ),
                }
            })
            .collect::<Vec<_>>();
        let note = if self.demo {
            "Enter opens it in this channel · Esc closes this list.\nIn the demo the Board is installed and approved."
        } else if rows.is_empty() {
            "No app is installed in this community. At the shell: bzz app new <name>, then bzz app add <folder>."
        } else {
            "Enter opens the selected app in this channel · a shows its waiting version for your answer · :app share offers the open one here · :app close closes it · Esc closes this list. Only an approved app runs."
        };
        let mut note = note.to_owned();
        if !self.apps.offer_notes.is_empty() {
            note.push_str("\n\nOffers lately:");
            for line in &self.apps.offer_notes {
                note.push_str("\n  ");
                note.push_str(line);
            }
        }
        self.apps.list = Some(AppList {
            rows,
            selected: 0,
            note: Some(note),
        });
        self.presentation.open_overlay(Overlay::Apps);
        Ok(())
    }

    /// `bzz demo`'s Board: its cards in memory, where the approved Board
    /// runs (`crate::demo::board`).
    pub fn set_demo_board(&mut self, board: crate::demo::board::DemoBoard) {
        self.apps.demo = Some(board);
    }

    /// Tests open an app on records in memory, with no store behind it.
    #[cfg(test)]
    pub(super) fn open_app_in_memory(&mut self, channel: Uuid, name: &str, source: &str) {
        let community = self.active_community_id().unwrap_or_default();
        let session = crate::apps::session::AppSession::in_memory(
            source.as_bytes().to_vec(),
            crate::apps::session::SessionOptions::new(name).waker(self.app_waker()),
            &crate::apps::session::SharedStore::new(crate::apps::session::MemoryStore::new(
                crate::apps::runtime::Who::new("a".repeat(64), "you"),
                crate::apps::runtime::SystemClock::shared(),
            )),
        )
        .expect("a session in memory");
        self.apps.open.insert(channel, name.to_owned());
        self.apps.open_of = Some(community);
        self.apps.live.retain(|live| live.channel != channel);
        self.apps.live.push(live::Live {
            community,
            channel,
            name: name.to_owned(),
            title: name.to_owned(),
            shown_at: 0,
            run: live::Run::Tested {
                session: Box::new(session),
            },
        });
    }
}

#[cfg(feature = "apps")]
impl AppPanes {
    fn session_mut(&mut self, channel: Uuid) -> Option<&mut crate::apps::session::AppSession> {
        self.live
            .iter_mut()
            .find(|live| live.channel == channel)
            .and_then(live::Live::session_mut)
    }

    /// The app's error, or that it does not respond, for the status line.
    fn trouble(&self, channel: Uuid) -> Option<String> {
        self.live
            .iter()
            .find(|live| live.channel == channel)?
            .trouble()
    }

    /// Closes the sessions shown least recently beyond `keep`.
    fn evict(&mut self, keep: usize) {
        while self.live.len() > keep {
            let Some(oldest) = self
                .live
                .iter()
                .enumerate()
                .min_by_key(|(_, live)| live.shown_at)
                .map(|(index, _)| index)
            else {
                return;
            };
            self.live.remove(oldest);
        }
    }

    fn poll(&mut self, shown: Option<Uuid>) -> bool {
        let mut redraw = false;
        for live in &mut self.live {
            let found = live.poll(&mut self.pending);
            redraw |= found && Some(live.channel) == shown;
        }
        redraw
    }

    /// The ids the open apps show, named or not: each is looked up once a
    /// run, as the timeline's authors are (v0.17 F5).
    fn unnamed(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .live
            .iter()
            .filter_map(live::Live::session_ref)
            .flat_map(crate::apps::session::AppSession::people)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    fn tick(&mut self, shown: Option<Uuid>, name_of: impl Fn(&str) -> Option<String>) -> bool {
        let mut redraw = false;
        for live in &mut self.live {
            let changed = live.tick(&name_of);
            redraw |= changed && Some(live.channel) == shown;
        }
        redraw
    }
}

#[cfg(all(test, feature = "apps"))]
mod tests;

#[cfg(feature = "apps")]
mod dev;

#[cfg(feature = "apps")]
mod live {
    use std::collections::HashMap;

    use ratatui::{
        Frame,
        layout::{Position, Rect},
    };
    use uuid::Uuid;

    use crate::apps::runtime::{AppTheme, Output, Phase, Waker};
    use crate::apps::session::{AppSession, SessionOptions};
    use crate::apps::store::{AppError, Runnable};
    use crate::service::records::RecordsService;
    use crate::ui::{app_pane, theme::Theme};

    /// One open app, in one channel.
    pub struct Live {
        pub community: Uuid,
        pub channel: Uuid,
        pub name: String,
        pub title: String,
        pub shown_at: u64,
        pub run: Run,
    }

    pub enum Run {
        Session {
            session: Box<AppSession>,
            runnable: Box<Runnable>,
            /// The approved bytes have loaded once.
            loaded: bool,
            /// A newer approved version given to the running session, which
            /// has not said yet whether it runs.
            trial: Option<Box<Trial>>,
        },
        /// The owner's own folder, run without approval (plan D5).
        Dev {
            session: Box<AppSession>,
            folder: super::dev::DevFolder,
        },
        /// Why the app does not run here.
        Problem(String),
        /// A session of a test, with no store behind it.
        #[cfg(test)]
        Tested { session: Box<AppSession> },
    }

    /// A version given to a running app in place of the one it runs: an
    /// approval, a rollback, an approval at the shell. It replaces the
    /// running version only if it loads, declares the approved types and
    /// draws without an error; otherwise the version before goes on.
    pub struct Trial {
        pub next: Runnable,
        /// What the new version declared when it loaded, if it did.
        pub types: Option<Vec<String>>,
    }

    /// What the loop is to record in the store, or to say.
    pub enum Pending {
        Failed {
            community: Uuid,
            channel: Uuid,
            name: String,
            version: u32,
            error: String,
            /// The version before it still runs in the pane.
            kept: Option<u32>,
        },
        /// A version given to the running app runs now, with its state.
        Updated {
            name: String,
            from: u32,
            version: u32,
        },
        /// The app asked to close (`host.quit()`).
        Closed { channel: Uuid },
    }

    impl Run {
        pub fn start(
            runnable: Runnable,
            waker: Waker,
            names: HashMap<String, String>,
            archived: bool,
            records: Where<'_>,
            channel: Uuid,
        ) -> Self {
            let options = SessionOptions::new(runnable.name())
                .script(runnable.script_name())
                .waker(waker)
                .names(names)
                .archived(archived);
            match records.session(runnable.script().to_vec(), options, channel) {
                Ok(session) => Self::Session {
                    session: Box::new(session),
                    runnable: Box::new(runnable),
                    loaded: false,
                    trial: None,
                },
                Err(error) => Self::Problem(format!("This app could not be started: {error}")),
            }
        }
    }

    /// Where a session's records are.
    pub enum Where<'a> {
        Relay(&'a RecordsService),
        /// A test's records, or `bzz demo`'s.
        Memory(&'a crate::apps::session::SharedStore),
    }

    impl Where<'_> {
        pub fn session(
            &self,
            script: Vec<u8>,
            options: SessionOptions,
            channel: Uuid,
        ) -> std::io::Result<AppSession> {
            match self {
                Self::Relay(service) => AppSession::on_relay(script, options, service, channel),
                Self::Memory(store) => {
                    let _ = channel;
                    AppSession::in_memory(script, options, store)
                }
            }
        }
    }

    /// What keeps an app from running, in the owner's words, with the way
    /// out at the shell.
    pub fn why_not(name: &str, error: &AppError) -> String {
        match error {
            AppError::NothingApproved { waiting, .. } if *waiting > 0 => format!(
                "{error}. Ctrl-Y or :app pending {name} shows it for your answer (at the shell: bzz app approve {name})"
            ),
            AppError::NothingApproved { .. } => format!(
                "{error}. Propose a version: :app add <folder> (at the shell: bzz app add <folder>)"
            ),
            _ => error.to_string(),
        }
    }

    impl Live {
        pub fn session_mut(&mut self) -> Option<&mut AppSession> {
            match &mut self.run {
                Run::Session { session, .. } | Run::Dev { session, .. } => Some(session),
                #[cfg(test)]
                Run::Tested { session } => Some(session),
                Run::Problem(_) => None,
            }
        }

        pub fn session_ref(&self) -> Option<&AppSession> {
            self.session()
        }

        fn session(&self) -> Option<&AppSession> {
            match &self.run {
                Run::Session { session, .. } | Run::Dev { session, .. } => Some(session),
                #[cfg(test)]
                Run::Tested { session } => Some(session),
                Run::Problem(_) => None,
            }
        }

        /// The session's own sentence, for the pane's bottom border.
        pub fn notice(&self) -> Option<String> {
            self.session()?.status().notice
        }

        /// The script's whole error, for a test.
        #[cfg(test)]
        pub fn error(&self) -> Option<String> {
            self.session()?.status().error
        }

        pub fn trouble(&self) -> Option<String> {
            let first = |text: &str| text.lines().next().unwrap_or_default().to_owned();
            if let Run::Problem(why) = &self.run {
                return Some(format!("app {}: {}", self.name, first(why)));
            }
            if let Run::Dev { folder, .. } = &self.run
                && folder.missing
            {
                return Some(format!(
                    "app {}: {} is missing; the version before goes on running",
                    self.name,
                    folder.script.display()
                ));
            }
            let status = self.session()?.status();
            if !status.responding {
                Some(format!("app {} is not responding", self.name))
            } else {
                status
                    .error
                    .map(|error| format!("app {}: {}", self.name, first(&error)))
            }
        }

        pub fn draw(
            &mut self,
            frame: &mut Frame<'_>,
            region: Rect,
            theme: &Theme,
        ) -> Option<Position> {
            if let Run::Problem(why) = &self.run {
                app_pane::render_words(frame, region, why, true, theme);
                return None;
            }
            let session = self.session_mut()?;
            session.draw(region, frame.buffer_mut(), &AppTheme::from(theme))
        }

        /// Carries what the script and the records produced; notes a version
        /// that failed as it was put to run, and an app that asked to close.
        /// Answers whether there is something new to draw.
        pub fn poll(&mut self, pending: &mut Vec<Pending>) -> bool {
            let (community, channel) = (self.community, self.channel);
            let mut redraw = false;
            let failure = match &mut self.run {
                Run::Problem(_) => return false,
                #[cfg(test)]
                Run::Tested { session } => {
                    for output in session.poll() {
                        if matches!(output, Output::Close) {
                            pending.push(Pending::Closed { channel });
                            return true;
                        }
                        redraw |= matches!(output, Output::Redraw | Output::Loaded { .. });
                    }
                    return redraw;
                }
                Run::Dev { session, .. } => {
                    for output in session.poll() {
                        match output {
                            Output::Close => {
                                pending.push(Pending::Closed { channel });
                                return true;
                            }
                            Output::Redraw | Output::Loaded { .. } => redraw = true,
                            Output::Log(_) | Output::Request(_) => {}
                        }
                    }
                    return redraw;
                }
                Run::Session {
                    session,
                    runnable,
                    loaded,
                    trial,
                } => {
                    // What the script's thread reported before it was idle
                    // is all there is of what it was given.
                    let idle = session.runtime().is_idle();
                    let mut failed = None;
                    for output in session.poll() {
                        match output {
                            Output::Loaded { types } => {
                                redraw = true;
                                if let Some(trial) = trial.as_mut() {
                                    trial.types = Some(types);
                                    continue;
                                }
                                match runnable.check_loaded(&types) {
                                    Ok(()) => *loaded = true,
                                    Err(error) => failed = Some(error),
                                }
                            }
                            Output::Close => {
                                pending.push(Pending::Closed { channel });
                                return true;
                            }
                            Output::Redraw => redraw = true,
                            Output::Log(_) | Output::Request(_) => {}
                        }
                    }
                    if idle && let Some(Trial { next, types }) = trial.take().map(|trial| *trial) {
                        redraw = true;
                        let status = session.status();
                        // The new version is held to what was approved,
                        // and must draw: an error in `app.reload` or in its
                        // first `view` is a failure to run.
                        let verdict = match &types {
                            None => Err((
                                status.error.unwrap_or_else(|| {
                                    format!("version {} did not load", next.version())
                                }),
                                false,
                            )),
                            Some(types) => match next.check_loaded(types) {
                                Err(error) => Err((error, true)),
                                Ok(()) => status.error.map_or(Ok(()), |error| Err((error, true))),
                            },
                        };
                        match verdict {
                            Ok(()) => {
                                pending.push(Pending::Updated {
                                    name: next.name().to_owned(),
                                    from: runnable.version(),
                                    version: next.version(),
                                });
                                self.title = next.title().to_owned();
                                **runnable = next;
                            }
                            Err((error, loaded_new)) => {
                                // A version that loaded replaced the old one
                                // in its thread: the old bytes, which were
                                // approved, are given back, and its state
                                // goes with them. One that did not load
                                // never replaced it.
                                if loaded_new {
                                    session.reload(runnable.script().to_vec());
                                }
                                pending.push(Pending::Failed {
                                    community,
                                    channel,
                                    name: self.name.clone(),
                                    version: next.version(),
                                    error,
                                    kept: Some(runnable.version()),
                                });
                            }
                        }
                        return redraw;
                    }
                    let status = session.status();
                    if failed.is_none() && !*loaded && status.phase == Phase::NotLoaded {
                        failed = Some(status.error.unwrap_or_else(|| {
                            format!("{}: the approved version did not load", runnable.name())
                        }));
                    }
                    failed.map(|error| {
                        session.close();
                        (runnable.version(), error)
                    })
                }
            };
            if let Some((version, error)) = failure {
                pending.push(Pending::Failed {
                    community,
                    channel,
                    name: self.name.clone(),
                    version,
                    error: error.clone(),
                    kept: None,
                });
                self.run = Run::Problem(error);
                return true;
            }
            redraw
        }

        /// The version the pane runs, or tries, of an approved app.
        pub fn version(&self) -> Option<u32> {
            match &self.run {
                Run::Session {
                    runnable, trial, ..
                } => Some(
                    trial
                        .as_ref()
                        .map_or(runnable.version(), |trial| trial.next.version()),
                ),
                _ => None,
            }
        }

        /// Gives a newer approved version to the running app, which keeps
        /// its state if the version runs. False when there is no running
        /// session to give it to: the caller starts one instead.
        pub fn update(&mut self, next: Runnable) -> bool {
            match &mut self.run {
                Run::Session {
                    session,
                    loaded: true,
                    trial,
                    ..
                } if trial.is_none() && session.status().responding => {
                    session.reload(next.script().to_vec());
                    *trial = Some(Box::new(Trial { next, types: None }));
                    true
                }
                _ => false,
            }
        }

        pub fn is_dev(&self) -> bool {
            matches!(self.run, Run::Dev { .. })
        }

        pub fn is_problem(&self) -> bool {
            matches!(self.run, Run::Problem(_))
        }

        /// A folder in development: a save of its script is given to the
        /// running app. Answers whether there is something new to draw.
        pub fn watch(&mut self) -> bool {
            let Run::Dev { session, folder } = &mut self.run else {
                return false;
            };
            match folder.changed() {
                Some(script) => {
                    session.reload(script);
                    true
                }
                None => false,
            }
        }

        /// Time for the session, and the names it lacks from bzz's
        /// profiles. Answers whether there is something new to draw.
        pub fn tick(&mut self, name_of: &impl Fn(&str) -> Option<String>) -> bool {
            let Some(session) = self.session_mut() else {
                return false;
            };
            // Everyone it shows: a name bzz learns, or one that changed.
            let names: Vec<(String, String)> = session
                .people()
                .into_iter()
                .filter_map(|id| name_of(&id).map(|name| (id, name)))
                .collect();
            if !names.is_empty() {
                session.set_names(names);
            }
            session.tick()
        }
    }
}
