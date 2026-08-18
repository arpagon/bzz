use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use crossterm::event::{
    Event as TerminalEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use futures_util::StreamExt as _;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::{
    auth::{IdentityManager, read_passphrase, signer::SignerHandle},
    config::{Config, KeyBackend, validate_relay_url},
    domain::{
        Channel, ConnectionState, InboxCategory, InboxItem, Message, Profile, Reaction,
        SearchResultKind,
    },
    error::{Error, Result},
    media::{client::MediaClient, runtime::MediaRuntime},
    paths::Paths,
    protocol::http::HttpClient,
    realtime::{
        session::SessionEvent,
        subscriptions,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    render::sanitize,
    service::{
        channels::ChannelService, dms::DmService, inbox::InboxService, messages::MessageService,
        profiles::ProfileService, read_state::ReadStateService, search::SearchService,
    },
    store::writer::StoreHandle,
    sync::{outbox, read_state},
    ui::{
        composer::Composer,
        dm_picker::DmPickerState,
        hit_map::{HitMap, HitTarget},
        inbox::InboxState,
        keymap::{KeyAction, map_insert, map_normal},
        layout,
        mention_picker::MentionPicker,
        search::SearchState,
        terminal::{TerminalGuard, Tui},
        theme::{self, BorderSurface, HighlightGroup, Theme, ThemeScope},
        theme_picker::ThemePicker,
        timeline::{self, TimelineState},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Normal,
    Insert,
    Finder,
    Reaction,
    ConfirmDelete,
    Command,
    Theme,
    Help,
    MediaPreview,
    Attachment,
    SaveAttachment,
    Inbox,
    Search,
    DmPicker,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pane {
    Channels,
    Timeline,
    Thread,
}

struct Runtime {
    community_id: Uuid,
    identity_id: Uuid,
    signer: SignerHandle,
    supervisor: SupervisorHandle,
    events: broadcast::Receiver<SupervisorEvent>,
    http: HttpClient,
    channels: ChannelService,
    dms: DmService,
    inbox: InboxService,
    profiles: ProfileService,
    search: SearchService,
    messages: MessageService,
    media: MediaClient,
    read_state: ReadStateService,
}

impl Runtime {
    fn build(
        config: &Config,
        index: usize,
        paths: &Paths,
        store: StoreHandle,
        signer: Option<SignerHandle>,
    ) -> Result<Self> {
        let community = config
            .communities
            .get(index)
            .ok_or_else(|| Error::Config("community selection is out of range".into()))?;
        let identity = config
            .identities
            .iter()
            .find(|identity| identity.id == community.identity_id)
            .ok_or_else(|| Error::Config("community identity does not exist".into()))?;
        let signer = if let Some(signer) = signer {
            signer
        } else {
            let passphrase = matches!(identity.backend, KeyBackend::EncryptedFile)
                .then(|| read_passphrase("Identity passphrase: ", false))
                .transpose()?;
            let keys = IdentityManager::new(paths).unlock(identity, passphrase.as_ref())?;
            SignerHandle::spawn(keys)
        };
        let endpoint =
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
        let supervisor = SupervisorHandle::spawn(endpoint.websocket, signer.clone());
        let events = supervisor.subscribe_events();
        let http = HttpClient::new(endpoint.http_base.clone(), signer.clone())?;
        let media = MediaClient::new(
            endpoint.http_base,
            endpoint.authority,
            signer.clone(),
            config.media.download_concurrency,
        )?;
        Ok(Self {
            community_id: community.id,
            identity_id: identity.id,
            signer: signer.clone(),
            supervisor: supervisor.clone(),
            events,
            http: http.clone(),
            channels: ChannelService::new(community.id, http.clone(), store.clone()),
            dms: DmService::new(
                community.id,
                signer.clone(),
                http.clone(),
                store.clone(),
                supervisor.clone(),
            ),
            inbox: InboxService::new(community.id, http.clone(), store.clone()),
            profiles: ProfileService::new(community.id, http.clone(), store.clone()),
            search: SearchService::new(community.id, http.clone(), store.clone()),
            messages: MessageService::new(
                community.id,
                signer.clone(),
                store.clone(),
                supervisor.clone(),
            ),
            media,
            read_state: ReadStateService::new(community.id, signer, store, supervisor),
        })
    }
}

#[derive(Debug)]
enum Background {
    Changed,
    Failed(String),
    Staged {
        community: Uuid,
        pending: crate::media::PendingAttachment,
    },
    Uploaded {
        community: Uuid,
        sha256: String,
        attachment: Box<crate::media::Attachment>,
    },
    UploadFailed {
        community: Uuid,
        sha256: String,
        message: String,
    },
    Saved,
    InboxLoaded {
        community: Uuid,
        items: Vec<InboxItem>,
    },
    SearchLoaded {
        community: Uuid,
        generation: u64,
        output: crate::service::search::SearchOutput,
    },
    DmOpened {
        community: Uuid,
        result: crate::service::dms::DmOpenResult,
    },
    DmHidden {
        community: Uuid,
        channel: Uuid,
        confirmed: bool,
    },
}

pub struct App {
    config: Config,
    paths: Paths,
    store: StoreHandle,
    runtime: Option<Runtime>,
    media: MediaRuntime,
    selected_community: usize,
    channels: Vec<Channel>,
    selected_channel: usize,
    showing_open_channel: bool,
    messages: Vec<Message>,
    profiles: HashMap<String, Profile>,
    reactions: HashMap<String, Vec<Reaction>>,
    profile_requested: HashSet<String>,
    thread_messages: Vec<Message>,
    thread_root: Option<String>,
    timeline: TimelineState,
    thread_timeline: TimelineState,
    mode: Mode,
    pane: Pane,
    composer: Composer,
    mention_picker: Option<MentionPicker>,
    finder: String,
    reaction_index: usize,
    command: String,
    attachment_input: String,
    inbox_items: Vec<InboxItem>,
    inbox_state: InboxState,
    inbox_loading: bool,
    inbox_task: Option<tokio::task::JoinHandle<()>>,
    search_state: SearchState,
    search_dirty_since: Option<Instant>,
    search_task: Option<tokio::task::JoinHandle<()>>,
    dm_picker: DmPickerState,
    dm_dirty_since: Option<Instant>,
    dm_search_task: Option<tokio::task::JoinHandle<()>>,
    preview_index: usize,
    preview_revealed: bool,
    uploading_media: HashSet<String>,
    sidebar: bool,
    theme: Theme,
    theme_picker: Option<ThemePicker>,
    theme_before_preview: Option<Theme>,
    connection: ConnectionState,
    status_error: Option<String>,
    should_quit: bool,
    awaiting_g: bool,
    cache_dirty: bool,
    manual_unread: HashSet<Uuid>,
    computed_unread: HashSet<Uuid>,
    subscribed_channels: HashSet<Uuid>,
    last_marked: HashMap<String, u32>,
    read_dirty_since: Option<Instant>,
    last_cache_refresh: Instant,
    last_directory_refresh: Instant,
    directory_task: Option<tokio::task::JoinHandle<()>>,
    last_inbox_refresh: Instant,
    background_tx: mpsc::Sender<Background>,
    background_rx: mpsc::Receiver<Background>,
    render_generation: u64,
    last_hit_map: Option<HitMap>,
}

impl App {
    pub async fn new(config: Config, paths: Paths, store: StoreHandle) -> Result<Self> {
        let selected_community = config
            .default_community
            .and_then(|id| config.communities.iter().position(|entry| entry.id == id))
            .unwrap_or_default();
        let (theme, theme_notice) = load_theme_safe(&config, selected_community, &paths);
        let (runtime, connection, status_error) = if config.communities.is_empty() {
            (None, ConnectionState::Offline, None)
        } else {
            match Runtime::build(&config, selected_community, &paths, store.clone(), None) {
                Ok(runtime) => (Some(runtime), ConnectionState::Connecting, None),
                Err(error) => {
                    let Some(connection) = identity_recovery_connection(&error) else {
                        return Err(error);
                    };
                    (None, connection, Some(error.to_string()))
                }
            }
        };
        let (background_tx, background_rx) = mpsc::channel(128);
        let mut media = MediaRuntime::new(config.media.clone(), &paths, store.clone());
        if let Some(active) = &runtime {
            media.bind(active.community_id, active.media.clone());
        } else if let Some(community) = config.communities.get(selected_community) {
            media.select_cached(community.id);
        }
        let media_notice = media
            .repair_cache_metadata()
            .await
            .err()
            .map(|error| format!("media cache repair: {}", public_media_error(&error)));
        let notices = [status_error, theme_notice, media_notice]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut app = Self {
            config,
            paths,
            store,
            runtime,
            media,
            selected_community,
            channels: Vec::new(),
            selected_channel: 0,
            showing_open_channel: false,
            messages: Vec::new(),
            profiles: HashMap::new(),
            reactions: HashMap::new(),
            profile_requested: HashSet::new(),
            thread_messages: Vec::new(),
            thread_root: None,
            timeline: TimelineState {
                at_live_bottom: true,
                ..TimelineState::default()
            },
            thread_timeline: TimelineState {
                at_live_bottom: true,
                ..TimelineState::default()
            },
            mode: Mode::Normal,
            pane: Pane::Channels,
            composer: Composer::default(),
            mention_picker: None,
            finder: String::new(),
            reaction_index: 0,
            command: String::new(),
            attachment_input: String::new(),
            inbox_items: Vec::new(),
            inbox_state: InboxState::default(),
            inbox_loading: false,
            inbox_task: None,
            search_state: SearchState::default(),
            search_dirty_since: None,
            search_task: None,
            dm_picker: DmPickerState::default(),
            dm_dirty_since: None,
            dm_search_task: None,
            preview_index: 0,
            preview_revealed: false,
            uploading_media: HashSet::new(),
            sidebar: true,
            theme,
            theme_picker: None,
            theme_before_preview: None,
            connection,
            status_error: (!notices.is_empty()).then(|| notices.join("; ")),
            should_quit: false,
            awaiting_g: false,
            cache_dirty: true,
            manual_unread: HashSet::new(),
            computed_unread: HashSet::new(),
            subscribed_channels: HashSet::new(),
            last_marked: HashMap::new(),
            read_dirty_since: None,
            last_cache_refresh: Instant::now(),
            last_directory_refresh: Instant::now(),
            directory_task: None,
            last_inbox_refresh: Instant::now(),
            background_tx,
            background_rx,
            render_generation: 0,
            last_hit_map: None,
        };
        app.hydrate_cache().await?;
        Ok(app)
    }

    pub async fn run(mut self) -> Result<()> {
        let shutdown = shutdown_signal();
        tokio::pin!(shutdown);
        tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(()),
            _ = tokio::task::yield_now() => {},
        }
        let (mut guard, mut terminal) = TerminalGuard::enter(self.config.ui.mouse.enabled())?;
        self.media.initialize_terminal();
        terminal
            .draw(|frame| self.render(frame))
            .map_err(|error| Error::io("terminal", error))?;
        self.start_sync().await?;
        let mut input = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        while !self.should_quit {
            terminal
                .draw(|frame| self.render(frame))
                .map_err(|error| Error::io("terminal", error))?;
            tokio::select! {
                maybe=input.next()=>if let Some(Ok(event))=maybe { match event {
                    TerminalEvent::Key(key) if key.kind==KeyEventKind::Press => self.handle_key(key,&mut guard,&mut terminal).await?,
                    TerminalEvent::Mouse(mouse) => self.handle_mouse(mouse,&mut guard,&mut terminal).await?,
                    _ => {}
                } },
                network=next_network(&mut self.runtime)=>if let Some(event)=network{self.handle_network(event).await?;},
                background=self.background_rx.recv()=>if let Some(event)=background{match event{
                    Background::Changed=>{
                        self.cache_dirty=true;
                        if self.mode==Mode::Inbox {
                            self.spawn_inbox_load(false);
                        }
                    },
                    Background::Failed(message)=>{
                        self.status_error=Some(message);
                        self.dm_picker.submitting=false;
                        self.inbox_loading=false;
                    },
                    Background::Staged { community, pending }=>{
                        if self.active_community_id()==Some(community)
                            && !self.composer.attachments.iter().any(|item| match item {
                                crate::media::DraftAttachment::Pending(value)=>value.sha256==pending.sha256,
                                crate::media::DraftAttachment::Uploaded(value)=>value.sha256==pending.sha256,
                            })
                        {
                            self.composer.attachments.push(
                                crate::media::DraftAttachment::Pending(pending.clone())
                            );
                            self.persist_draft().await?;
                            self.start_pending_upload(pending);
                        }
                    }
                    Background::Uploaded { community, sha256, attachment }=>{
                        self.uploading_media.remove(&format!("{community}:{sha256}"));
                        if self.active_community_id()==Some(community)
                            && let Some((index, item))=self.composer.attachments.iter_mut().enumerate().find(|(_, item)| matches!(item, crate::media::DraftAttachment::Pending(value) if value.sha256==sha256))
                        {
                            let mut attachment=*attachment;
                            attachment.index=index;
                            *item=crate::media::DraftAttachment::Uploaded(attachment);
                            self.status_error=None;
                            self.persist_draft().await?;
                        }
                    }
                    Background::UploadFailed { community, sha256, message }=>{
                        self.uploading_media.remove(&format!("{community}:{sha256}"));
                        if self.active_community_id()==Some(community) {
                            self.status_error=Some(message);
                        }
                    }
                    Background::Saved=>self.status_error=Some("attachment saved".into()),
                    Background::InboxLoaded { community, items }=>{
                        if self.active_community_id()==Some(community) {
                            self.inbox_items=items;
                            self.inbox_state.reconcile(&self.inbox_items);
                            self.cache_dirty=true;
                            self.inbox_loading=false;
                        }
                    }
                    Background::SearchLoaded { community, generation, output }=>{
                        if self.active_community_id()==Some(community)
                            && self.search_state.generation==generation
                        {
                            self.search_state.results=output.results;
                            self.search_state.local_only=output.local_only;
                            self.search_state.notice=output.notice;
                            self.search_state.loading=false;
                            self.search_state.reconcile();
                        }
                    }
                    Background::DmOpened { community, result }=>{
                        if self.active_community_id()==Some(community) {
                            self.cache_dirty=true;
                            self.hydrate_cache().await?;
                            if let Some(index)=self.channels.iter().position(|channel|channel.id==result.channel_id) {
                                self.selected_channel=index;
                                self.showing_open_channel=false;
                                self.load_selected_channel().await?;
                                self.pane=Pane::Timeline;
                            }
                            self.mode=Mode::Normal;
                            self.dm_picker= DmPickerState::default();
                            self.dm_dirty_since=None;
                            self.status_error=Some(if result.visibility_confirmed {
                                "Private workspace DM opened (relay-readable; not end-to-end encrypted)".into()
                            } else {
                                "DM opened; waiting for the visibility snapshot".into()
                            });
                        }
                    }
                    Background::DmHidden { community, channel, confirmed }=>{
                        if self.active_community_id()==Some(community) {
                            self.cache_dirty=true;
                            self.hydrate_cache().await?;
                            self.status_error=Some(if confirmed {
                                "DM hidden; reopen it by selecting the same participant set".into()
                            } else {
                                "DM hide accepted; waiting for the visibility snapshot".into()
                            });
                            if self.current_channel().is_some_and(|value|value.id==channel) {
                                self.pane=Pane::Channels;
                            }
                        }
                    }
                }},
                _=tick.tick()=>self.on_tick().await?,
                _=&mut shutdown=>self.should_quit=true,
            }
        }
        for task in [
            self.inbox_task.take(),
            self.search_task.take(),
            self.dm_search_task.take(),
            self.directory_task.take(),
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
        self.shutdown().await;
        guard.restore();
        Ok(())
    }

    async fn start_sync(&mut self) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            if !matches!(
                self.connection,
                ConnectionState::Locked
                    | ConnectionState::IdentityMissing
                    | ConnectionState::IdentityCorrupt
            ) {
                self.connection = ConnectionState::Offline;
            }
            return Ok(());
        };
        let pubkey = runtime.signer.public_key().to_hex();
        let channels = runtime.channels.clone();
        let inbox = runtime.inbox.clone();
        let profiles = runtime.profiles.clone();
        let http = runtime.http.clone();
        let supervisor = runtime.supervisor.clone();
        let community = runtime.community_id;
        supervisor
            .subscribe(
                "membership",
                subscriptions::membership(&pubkey, nostr::Timestamp::now().as_secs()),
            )
            .await?;
        supervisor
            .subscribe(
                "global-stream",
                subscriptions::global_stream(nostr::Timestamp::now().as_secs()),
            )
            .await?;
        supervisor
            .subscribe(
                "personal",
                subscriptions::personal(&pubkey, nostr::Timestamp::now().as_secs()),
            )
            .await?;
        supervisor
            .subscribe(
                "read-state",
                subscriptions::read_state(
                    &pubkey,
                    nostr::Timestamp::now()
                        .as_secs()
                        .saturating_sub(read_state::HORIZON_SECONDS),
                ),
            )
            .await?;
        let initial_channels = self
            .channels
            .iter()
            .filter(|channel| channel.is_member)
            .take(900)
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        for channel in initial_channels {
            self.subscribe_channel(channel).await?;
        }
        let store = self.store.clone();
        let authors = self
            .messages
            .iter()
            .map(|message| message.pubkey.clone())
            .collect::<HashSet<_>>();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let info = http.nip11().await?;
                let relay_pubkey = crate::protocol::http::relay_signing_pubkey(&info)
                    .ok_or_else(|| {
                        Error::Protocol("NIP-11 document has no relay signing key".into())
                    })?
                    .to_owned();
                store
                    .call(move |store| store.pin_relay_pubkey(community, &relay_pubkey))
                    .await?;
                channels.refresh(&pubkey).await?;
                let _ = inbox.refresh(&pubkey).await;
                let _ = profiles.hydrate(authors).await;
                let _ = outbox::flush(community, &http, &supervisor, &store).await;
                Ok::<_, Error>(())
            }
            .await;
            let _ = tx
                .send(match result {
                    Ok(()) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
        if let Some(channel) = self.current_channel().map(|channel| channel.id) {
            self.spawn_backfill(channel);
        }
        Ok(())
    }

    async fn subscribe_channel(&mut self, channel: Uuid) -> Result<()> {
        if self.subscribed_channels.contains(&channel) || self.subscribed_channels.len() >= 900 {
            return Ok(());
        }
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let community = runtime.community_id;
        let channel_string = channel.to_string();
        let cursor = self
            .store
            .call(move |store| store.sync_cursor(community, "history", &channel_string))
            .await?;
        runtime
            .supervisor
            .subscribe(
                format!("ch-{}", channel.simple()),
                subscriptions::channel(channel, cursor.high_created_at),
            )
            .await?;
        self.subscribed_channels.insert(channel);
        Ok(())
    }

    async fn reconcile_subscriptions(&mut self) -> Result<()> {
        let channels = self
            .channels
            .iter()
            .filter(|channel| channel.is_member)
            .take(900)
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        for channel in channels {
            self.subscribe_channel(channel).await?;
        }
        Ok(())
    }

    fn spawn_backfill(&self, channel: Uuid) {
        if let Some(runtime) = &self.runtime {
            let service = runtime.channels.clone();
            let tx = self.background_tx.clone();
            tokio::spawn(async move {
                let result = service.backfill(channel).await;
                let _ = tx
                    .send(match result {
                        Ok(_) => Background::Changed,
                        Err(error) => Background::Failed(error.to_string()),
                    })
                    .await;
            });
        }
    }

    fn spawn_directory_refresh(&mut self) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        if let Some(task) = self.directory_task.take() {
            task.abort();
        }
        let service = runtime.channels.clone();
        let pubkey = runtime.signer.public_key().to_hex();
        let tx = self.background_tx.clone();
        self.directory_task = Some(tokio::spawn(async move {
            let result = service.refresh(&pubkey).await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        }));
    }

    fn spawn_inbox_load(&mut self, refresh_remote: bool) {
        let Some(community) = self.active_community_id() else {
            return;
        };
        let Some(identity_pubkey) = self.self_pubkey().map(str::to_owned) else {
            return;
        };
        self.inbox_loading = true;
        if let Some(task) = self.inbox_task.take() {
            task.abort();
        }
        let runtime = self.runtime.as_ref().map(|runtime| runtime.inbox.clone());
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        self.inbox_task = Some(tokio::spawn(async move {
            let result = async {
                if refresh_remote && let Some(service) = &runtime {
                    let _ = service.refresh(&identity_pubkey).await;
                }
                if let Some(service) = runtime {
                    service.items(&identity_pubkey).await
                } else {
                    store
                        .call(move |store| store.inbox_items(community, &identity_pubkey))
                        .await
                }
            }
            .await;
            let event = match result {
                Ok(items) => Background::InboxLoaded { community, items },
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        }));
    }

    fn spawn_search(&mut self) {
        let Some(community) = self.active_community_id() else {
            return;
        };
        let Some(identity_pubkey) = self.self_pubkey().map(str::to_owned) else {
            return;
        };
        let generation = self.search_state.generation;
        let input = self.search_state.query.clone();
        let channels = self.channels.clone();
        let profiles = self.profiles.clone();
        let service = self.runtime.as_ref().map(|runtime| runtime.search.clone());
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.search_task = Some(tokio::spawn(async move {
            let local = SearchService::execute_local(
                community,
                &store,
                &input,
                &identity_pubkey,
                &channels,
                &profiles,
            )
            .await;
            let mut local = match local {
                Ok(output) => output,
                Err(error) => {
                    let _ = tx
                        .send(Background::SearchLoaded {
                            community,
                            generation,
                            output: crate::service::search::SearchOutput {
                                local_only: service.is_none(),
                                notice: Some(error.to_string()),
                                ..crate::service::search::SearchOutput::default()
                            },
                        })
                        .await;
                    return;
                }
            };
            local.local_only = service.is_none();
            if service.is_some() && local.notice.is_none() {
                local.notice = Some("searching the relay…".into());
            }
            let fallback = local.clone();
            let _ = tx
                .send(Background::SearchLoaded {
                    community,
                    generation,
                    output: local,
                })
                .await;
            if let Some(service) = service {
                let output = service
                    .execute(&input, &identity_pubkey, &channels, &profiles)
                    .await
                    .unwrap_or_else(|error| {
                        let mut output = fallback;
                        output.local_only = true;
                        output.notice = Some(format!(
                            "remote search unavailable; showing local results ({error})"
                        ));
                        output
                    });
                let _ = tx
                    .send(Background::SearchLoaded {
                        community,
                        generation,
                        output,
                    })
                    .await;
            }
        }));
    }

    fn spawn_dm_profile_search(&mut self) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        if self.mode != Mode::DmPicker {
            return;
        }
        let service = runtime.search.clone();
        let input = self.dm_picker.query.clone();
        let tx = self.background_tx.clone();
        if let Some(task) = self.dm_search_task.take() {
            task.abort();
        }
        self.dm_search_task = Some(tokio::spawn(async move {
            let event = match service.hydrate_profiles(&input).await {
                Ok(_) => Background::Changed,
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        }));
    }

    async fn hydrate_cache(&mut self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            self.channels.clear();
            self.messages.clear();
            self.reactions.clear();
            return Ok(());
        };
        self.channels = self
            .store
            .call(move |store| store.channels(community))
            .await?;
        self.selected_channel = if self.showing_open_channel {
            self.selected_channel
                .min(self.channels.len().saturating_sub(1))
        } else if self
            .channels
            .get(self.selected_channel)
            .is_some_and(|channel| channel.is_member)
        {
            self.selected_channel
        } else {
            self.channels
                .iter()
                .position(|channel| channel.is_member)
                .unwrap_or(self.channels.len())
        };
        if let Some(channel) = self.current_channel().map(|channel| channel.id) {
            let timeline_anchor = self.timeline.selected_event.clone();
            self.messages = self
                .store
                .call(move |store| {
                    let latest = store.messages(community, channel, 500)?;
                    if let Some(anchor) = timeline_anchor
                        && !latest.iter().any(|message| message.event_id == anchor)
                    {
                        return store.messages_around(community, channel, &anchor, 500);
                    }
                    Ok(latest)
                })
                .await?;
            self.timeline.reconcile(&self.messages);
            if let Some(root) = self.thread_root.clone() {
                let thread_anchor = self.thread_timeline.selected_event.clone();
                self.thread_messages = self
                    .store
                    .call(move |store| {
                        let latest = store.thread(community, &root, 500)?;
                        if let Some(anchor) = thread_anchor
                            && !latest.iter().any(|message| message.event_id == anchor)
                        {
                            return store.thread_around(community, channel, &root, &anchor, 500);
                        }
                        Ok(latest)
                    })
                    .await?;
                self.thread_timeline.reconcile(&self.thread_messages);
            }
        } else {
            self.messages.clear();
            self.thread_messages.clear();
        }
        let reaction_targets = self
            .messages
            .iter()
            .chain(self.thread_messages.iter())
            .map(|message| message.event_id.clone())
            .collect::<HashSet<_>>();
        self.reactions = self
            .store
            .call(move |store| {
                reaction_targets
                    .into_iter()
                    .map(|target| {
                        store
                            .reactions(community, &target)
                            .map(|rows| (target, rows))
                    })
                    .collect()
            })
            .await?;
        self.profiles = self
            .store
            .call(move |store| store.profiles(community))
            .await?;
        let dm_participants = self
            .store
            .call(move |store| store.dm_participants_map(community))
            .await?;
        let self_pubkey = self.self_pubkey().unwrap_or_default().to_owned();
        for channel in self
            .channels
            .iter_mut()
            .filter(|channel| channel.kind.is_dm())
        {
            if let Some(participants) = dm_participants.get(&channel.id) {
                channel.name = dm_label(participants, &self_pubkey, &self.profiles);
            }
        }
        if self.mode == Mode::DmPicker {
            let pubkey = self.self_pubkey().unwrap_or_default().to_owned();
            self.dm_picker.reconcile(&self.profiles, &pubkey);
        }
        let unknown_authors = self
            .messages
            .iter()
            .chain(self.thread_messages.iter())
            .map(|message| message.pubkey.clone())
            .filter(|pubkey| {
                !self.profiles.contains_key(pubkey) && !self.profile_requested.contains(pubkey)
            })
            .collect::<HashSet<_>>();
        self.profile_requested
            .extend(unknown_authors.iter().cloned());
        if !unknown_authors.is_empty()
            && let Some(runtime) = &self.runtime
        {
            let profiles = runtime.profiles.clone();
            let tx = self.background_tx.clone();
            tokio::spawn(async move {
                let result = profiles.hydrate(unknown_authors).await;
                let _ = tx
                    .send(match result {
                        Ok(_) => Background::Changed,
                        Err(error) => Background::Failed(error.to_string()),
                    })
                    .await;
            });
        }
        self.manual_unread = self
            .store
            .call(move |store| store.ui_state(community, "manual_unread"))
            .await?
            .unwrap_or_default();
        self.computed_unread = if let Some(runtime) = &self.runtime {
            let pubkey = runtime.signer.public_key().to_hex();
            self.store
                .call(move |store| store.unread_channels(community, &pubkey))
                .await?
        } else {
            HashSet::new()
        };
        self.cache_dirty = false;
        self.last_cache_refresh = Instant::now();
        Ok(())
    }

    async fn handle_network(&mut self, event: SupervisorEvent) -> Result<()> {
        match event {
            SupervisorEvent::Connecting => self.connection = ConnectionState::Connecting,
            SupervisorEvent::Backoff(_) => self.connection = ConnectionState::Offline,
            SupervisorEvent::Terminal(message) => {
                self.connection = if message.contains("clock-skew") {
                    ConnectionState::ClockSkew
                } else {
                    ConnectionState::AccessDenied
                };
                self.status_error = Some(message);
            }
            SupervisorEvent::Session(SessionEvent::Authenticated) => {
                self.connection = ConnectionState::Online;
                self.status_error = None;
                if let Some(runtime) = &self.runtime {
                    let community = runtime.community_id;
                    let http = runtime.http.clone();
                    let supervisor = runtime.supervisor.clone();
                    let store = self.store.clone();
                    let directory = runtime.channels.clone();
                    let inbox = runtime.inbox.clone();
                    let pubkey = runtime.signer.public_key().to_hex();
                    let tx = self.background_tx.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let info = http.nip11().await?;
                            let relay_pubkey = crate::protocol::http::relay_signing_pubkey(&info)
                                .ok_or_else(|| {
                                    Error::Protocol(
                                        "NIP-11 document has no relay signing key".into(),
                                    )
                                })?
                                .to_owned();
                            store
                                .call(move |store| store.pin_relay_pubkey(community, &relay_pubkey))
                                .await?;
                            directory.refresh(&pubkey).await?;
                            let _ = inbox.refresh(&pubkey).await;
                            outbox::flush(community, &http, &supervisor, &store).await
                        }
                        .await;
                        let _ = tx
                            .send(match result {
                                Ok(_) => Background::Changed,
                                Err(error) => Background::Failed(error.to_string()),
                            })
                            .await;
                    });
                    let channels = self
                        .channels
                        .iter()
                        .filter(|channel| channel.is_member)
                        .map(|channel| channel.id)
                        .collect::<Vec<_>>();
                    let service = runtime.channels.clone();
                    let tx = self.background_tx.clone();
                    tokio::spawn(async move {
                        let mut failure = None;
                        for channel in channels {
                            if let Err(error) = service.backfill(channel).await {
                                failure.get_or_insert_with(|| error.to_string());
                            }
                        }
                        let _ = tx
                            .send(failure.map_or(Background::Changed, Background::Failed))
                            .await;
                    });
                }
            }
            SupervisorEvent::Session(SessionEvent::Event { event, .. }) => {
                let Some(runtime) = &self.runtime else {
                    return Ok(());
                };
                let community = runtime.community_id;
                if event.kind.as_u16() == 30_078 {
                    let events = vec![event];
                    let signer = runtime.signer.clone();
                    let store = self.store.clone();
                    let tx = self.background_tx.clone();
                    tokio::spawn(async move {
                        let result =
                            read_state::merge_events(community, &events, &signer, &store).await;
                        let _ = tx
                            .send(match result {
                                Ok(_) => Background::Changed,
                                Err(error) => Background::Failed(error.to_string()),
                            })
                            .await;
                    });
                } else {
                    let kind = event.kind.as_u16();
                    let membership_refresh = matches!(kind, 44_100 | 44_101);
                    match self
                        .store
                        .call(move |store| store.apply_event(community, &event).map(|_| ()))
                        .await
                    {
                        Ok(()) => {
                            self.cache_dirty = true;
                            if matches!(kind, 9 | 40_002 | 30_622 | 46_010..=46_012) {
                                self.spawn_inbox_load(false);
                            }
                        }
                        Err(error) => self.status_error = Some(error.to_string()),
                    }
                    if membership_refresh {
                        self.spawn_directory_refresh();
                    }
                }
            }
            SupervisorEvent::Session(SessionEvent::Eose(_)) => {
                if self.connection != ConnectionState::Backfilling {
                    self.connection = ConnectionState::Online;
                }
            }
            SupervisorEvent::Session(SessionEvent::Notice(message)) => {
                self.status_error = Some(message)
            }
            SupervisorEvent::Session(SessionEvent::Closed {
                subscription,
                message,
            }) => {
                if subscription.starts_with("ch-") {
                    self.connection = ConnectionState::AccessDenied;
                    if let Some(channel) = self
                        .subscribed_channels
                        .iter()
                        .find(|channel| format!("ch-{}", channel.simple()) == subscription)
                        .copied()
                    {
                        self.subscribed_channels.remove(&channel);
                    }
                    if let Some(runtime) = &self.runtime {
                        let _ = runtime.supervisor.close(subscription.clone()).await;
                        self.spawn_directory_refresh();
                    }
                }
                self.status_error = Some(message);
            }
            SupervisorEvent::Session(SessionEvent::Disconnected(message)) => {
                self.connection = ConnectionState::Offline;
                self.status_error = Some(message);
            }
            SupervisorEvent::Session(SessionEvent::Count { .. }) => {}
        }
        Ok(())
    }

    async fn on_tick(&mut self) -> Result<()> {
        self.media.poll();
        if self
            .search_dirty_since
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(300))
        {
            self.search_dirty_since = None;
            self.spawn_search();
        }
        if self
            .dm_dirty_since
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(300))
        {
            self.dm_dirty_since = None;
            self.spawn_dm_profile_search();
        }
        if self.last_inbox_refresh.elapsed() >= Duration::from_secs(30) {
            self.last_inbox_refresh = Instant::now();
            if self.runtime.is_some() {
                self.spawn_inbox_load(true);
            }
        }
        if self.cache_dirty || self.last_cache_refresh.elapsed() > Duration::from_secs(1) {
            self.hydrate_cache().await?;
            self.reconcile_subscriptions().await?;
        }
        if ((self.pane == Pane::Timeline && self.timeline.at_live_bottom)
            || (self.pane == Pane::Thread && self.thread_timeline.at_live_bottom))
            && self.mode == Mode::Normal
        {
            self.mark_current_read().await?;
        }
        if self.last_directory_refresh.elapsed() >= Duration::from_secs(300) {
            self.last_directory_refresh = Instant::now();
            self.spawn_directory_refresh();
        }
        if self
            .read_dirty_since
            .is_some_and(|since| since.elapsed() >= Duration::from_secs(5))
        {
            self.read_dirty_since = None;
            if let Some(runtime) = &self.runtime {
                let service = runtime.read_state.clone();
                let tx = self.background_tx.clone();
                let max_seen = self.messages.last().map_or(0, |message| message.created_at);
                tokio::spawn(async move {
                    let result = service.publish(max_seen).await;
                    let _ = tx
                        .send(match result {
                            Ok(_) => Background::Changed,
                            Err(error) => Background::Failed(error.to_string()),
                        })
                        .await;
                });
            }
        }
        Ok(())
    }

    async fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        let Some(target) = self
            .last_hit_map
            .as_ref()
            .and_then(|map| map.hit(mouse.column, mouse.row))
            .cloned()
        else {
            return Ok(());
        };
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.mode == Mode::Normal => {
                let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                    -3
                } else {
                    3
                };
                match target {
                    HitTarget::ChannelPane | HitTarget::Channel(_) => {
                        self.pane = Pane::Channels;
                        self.move_selection(delta);
                    }
                    HitTarget::Timeline | HitTarget::TimelineMessage(_) => {
                        self.pane = Pane::Timeline;
                        self.move_selection(delta);
                    }
                    HitTarget::Thread | HitTarget::ThreadMessage(_) => {
                        self.pane = Pane::Thread;
                        self.move_selection(delta);
                    }
                    _ => {}
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.activate_mouse_target(target, mouse, guard, terminal)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn activate_mouse_target(
        &mut self,
        target: HitTarget,
        mouse: MouseEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        match target {
            HitTarget::Community(index)
                if self.mode == Mode::Normal && index < self.config.communities.len() =>
            {
                self.switch_community(index, guard, terminal).await?;
            }
            HitTarget::ChannelPane if self.mode == Mode::Normal => self.pane = Pane::Channels,
            HitTarget::Channel(index)
                if self.mode == Mode::Normal
                    && self
                        .channels
                        .get(index)
                        .is_some_and(|channel| channel.is_member) =>
            {
                self.selected_channel = index;
                self.showing_open_channel = false;
                self.load_selected_channel().await?;
                self.pane = Pane::Timeline;
            }
            HitTarget::Timeline if self.mode == Mode::Normal => self.pane = Pane::Timeline,
            HitTarget::TimelineMessage(event_id) if self.mode == Mode::Normal => {
                self.pane = Pane::Timeline;
                self.timeline.selected_event = Some(event_id);
                self.timeline.at_live_bottom = self.messages.last().is_some_and(|message| {
                    self.timeline.selected_event.as_deref() == Some(&message.event_id)
                });
            }
            HitTarget::Thread if self.mode == Mode::Normal => self.pane = Pane::Thread,
            HitTarget::ThreadMessage(event_id) if self.mode == Mode::Normal => {
                self.pane = Pane::Thread;
                self.thread_timeline.selected_event = Some(event_id);
                self.thread_timeline.at_live_bottom =
                    self.thread_messages.last().is_some_and(|message| {
                        self.thread_timeline.selected_event.as_deref() == Some(&message.event_id)
                    });
            }
            HitTarget::Composer if self.mode == Mode::Insert => {
                if let Some(area) = self
                    .last_hit_map
                    .as_ref()
                    .and_then(|map| map.area_of(&HitTarget::Composer))
                {
                    self.composer.set_cursor_from_display(
                        usize::from(mouse.row.saturating_sub(area.y)),
                        usize::from(mouse.column.saturating_sub(area.x)),
                        usize::from(area.width),
                    );
                    self.refresh_mention_picker().await?;
                }
            }
            HitTarget::MentionCandidate(index) if self.mode == Mode::Insert => {
                if let Some(picker) = &mut self.mention_picker
                    && index < picker.candidates.len()
                {
                    picker.selected = index;
                    self.accept_mention();
                    self.persist_draft().await?;
                }
            }
            HitTarget::FinderChannel(channel_id) if self.mode == Mode::Finder => {
                if let Some(index) = self
                    .channels
                    .iter()
                    .position(|channel| channel.id.to_string() == channel_id)
                {
                    self.selected_channel = index;
                    self.showing_open_channel = !self.channels[index].is_member;
                    self.load_selected_channel().await?;
                    self.pane = Pane::Timeline;
                }
                self.mode = Mode::Normal;
            }
            HitTarget::Theme(id) if self.mode == Mode::Theme => {
                if let Some(picker) = &mut self.theme_picker
                    && picker.select_id(&id)
                {
                    self.preview_selected_theme();
                }
            }
            HitTarget::Reaction(index)
                if self.mode == Mode::Reaction
                    && index < crate::ui::reaction_picker::REACTIONS.len() =>
            {
                self.reaction_index = index;
            }
            HitTarget::InboxItem(id)
                if self.mode == Mode::Inbox
                    && self
                        .inbox_items
                        .iter()
                        .any(|item| item.conversation_id == id) =>
            {
                self.inbox_state.selected_id = Some(id);
            }
            HitTarget::SearchResult(id)
                if self.mode == Mode::Search
                    && self
                        .search_state
                        .results
                        .iter()
                        .any(|result| result.stable_id == id) =>
            {
                self.search_state.selected_id = Some(id);
            }
            HitTarget::DmCandidate(pubkey)
                if self.mode == Mode::DmPicker && !self.dm_picker.submitting =>
            {
                self.dm_picker.selected_pubkey = Some(pubkey);
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        match self.mode {
            Mode::Normal => {
                if let KeyCode::Char(digit @ '1'..='9') = key.code
                    && key.modifiers.is_empty()
                {
                    let index = usize::from(digit as u8 - b'1');
                    if index < self.config.communities.len() {
                        self.switch_community(index, guard, terminal).await?;
                    }
                    return Ok(());
                }
                let action = map_normal(key, self.awaiting_g);
                self.awaiting_g = matches!(key.code, KeyCode::Char('g'));
                self.normal_action(action).await?;
            }
            Mode::Insert => self.insert_action(map_insert(key)).await?,
            Mode::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.mode = Mode::Normal
                }
            }
            Mode::Finder => self.text_overlay_key(key, Mode::Finder).await?,
            Mode::Command => self.text_overlay_key(key, Mode::Command).await?,
            Mode::Theme => self.theme_picker_key(key),
            Mode::Reaction => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.reaction_index =
                        (self.reaction_index + 1) % crate::ui::reaction_picker::REACTIONS.len()
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.reaction_index = self
                        .reaction_index
                        .checked_sub(1)
                        .unwrap_or(crate::ui::reaction_picker::REACTIONS.len() - 1)
                }
                KeyCode::Enter => self.send_reaction(),
                _ => {}
            },
            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('y' | 'Y') => {
                    self.delete_selected();
                    self.mode = Mode::Normal
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::MediaPreview => self.media_preview_key(key),
            Mode::Attachment => self.attachment_path_key(key, false).await?,
            Mode::SaveAttachment => self.attachment_path_key(key, true).await?,
            Mode::Inbox => self.inbox_key(key).await?,
            Mode::Search => self.search_key(key).await?,
            Mode::DmPicker => self.dm_picker_key(key),
        }
        Ok(())
    }

    async fn normal_action(&mut self, action: KeyAction) -> Result<()> {
        match action {
            KeyAction::Quit => self.should_quit = true,
            KeyAction::Help => self.mode = Mode::Help,
            KeyAction::ToggleSidebar => self.sidebar = !self.sidebar,
            KeyAction::Finder => {
                self.finder.clear();
                self.mode = Mode::Finder
            }
            KeyAction::Search => self.open_search(),
            KeyAction::Inbox => self.open_inbox(),
            KeyAction::NewDm => self.open_dm_picker(None),
            KeyAction::HideDm => self.hide_current_dm(),
            KeyAction::AddDmMember => {
                let channel = self
                    .current_channel()
                    .filter(|channel| channel.kind.is_dm())
                    .map(|channel| channel.id);
                if channel.is_some() {
                    self.open_dm_picker(channel);
                } else {
                    self.status_error =
                        Some("select a workspace DM before adding a participant".into());
                }
            }
            KeyAction::Theme => self.open_theme_picker(),
            KeyAction::Preview => self.open_media_preview(),
            KeyAction::Command => {
                self.command.clear();
                self.mode = Mode::Command
            }
            KeyAction::NextPane => {
                self.pane = match self.pane {
                    Pane::Channels => Pane::Timeline,
                    Pane::Timeline => {
                        if self.thread_root.is_some() {
                            Pane::Thread
                        } else {
                            Pane::Channels
                        }
                    }
                    Pane::Thread => Pane::Channels,
                }
            }
            KeyAction::PreviousPane => {
                self.pane = match self.pane {
                    Pane::Channels => {
                        if self.thread_root.is_some() {
                            Pane::Thread
                        } else {
                            Pane::Timeline
                        }
                    }
                    Pane::Timeline => Pane::Channels,
                    Pane::Thread => Pane::Timeline,
                }
            }
            KeyAction::Up => self.move_selection(-1),
            KeyAction::Down => self.move_selection(1),
            KeyAction::First => self.move_to_edge(false),
            KeyAction::Last => self.move_to_edge(true),
            KeyAction::PageUp => self.move_selection(-10),
            KeyAction::PageDown => self.move_selection(10),
            KeyAction::Open => self.open_selected().await?,
            KeyAction::Compose => self.enter_composer().await?,
            KeyAction::Thread => self.toggle_thread().await?,
            KeyAction::React if self.runtime.is_some() && self.selected_message().is_some() => {
                self.mode = Mode::Reaction
            }
            KeyAction::Delete
                if self.selected_message().is_some_and(|message| {
                    self.runtime.as_ref().is_some_and(|runtime| {
                        message.pubkey == runtime.signer.public_key().to_hex()
                    })
                }) =>
            {
                self.mode = Mode::ConfirmDelete
            }
            KeyAction::MarkUnread if self.runtime.is_some() => self.mark_unread().await?,
            KeyAction::React | KeyAction::Delete | KeyAction::MarkUnread
                if self.runtime.is_none() =>
            {
                self.status_error = Some(
                    "cached read-only mode: restore or unlock the identity, then restart bzz"
                        .into(),
                )
            }
            KeyAction::Escape => {
                self.thread_root = None;
                self.pane = Pane::Timeline
            }
            _ => {}
        }
        Ok(())
    }

    fn move_selection(&mut self, delta: isize) {
        match self.pane {
            Pane::Channels => {
                let joined = self
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, channel)| channel.is_member)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if !joined.is_empty() {
                    let current = joined
                        .iter()
                        .position(|index| *index == self.selected_channel)
                        .unwrap_or_default();
                    let next = current.saturating_add_signed(delta).min(joined.len() - 1);
                    self.selected_channel = joined[next];
                    self.showing_open_channel = false;
                }
            }
            Pane::Timeline => self.timeline.move_by(&self.messages, delta),
            Pane::Thread => self.thread_timeline.move_by(&self.thread_messages, delta),
        }
    }
    fn move_to_edge(&mut self, last: bool) {
        match self.pane {
            Pane::Channels => {
                let joined = self
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, channel)| channel.is_member)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if let Some(index) = if last { joined.last() } else { joined.first() } {
                    self.selected_channel = *index;
                    self.showing_open_channel = false;
                }
            }
            Pane::Timeline => {
                self.timeline.selected_event = (if last {
                    self.messages.last()
                } else {
                    self.messages.first()
                })
                .map(|message| message.event_id.clone());
                self.timeline.at_live_bottom = last
            }
            Pane::Thread => {
                self.thread_timeline.selected_event = (if last {
                    self.thread_messages.last()
                } else {
                    self.thread_messages.first()
                })
                .map(|message| message.event_id.clone());
                self.thread_timeline.at_live_bottom = last
            }
        }
    }

    async fn open_selected(&mut self) -> Result<()> {
        match self.pane {
            Pane::Channels => {
                self.load_selected_channel().await?;
                self.pane = Pane::Timeline
            }
            Pane::Timeline | Pane::Thread => self.toggle_thread().await?,
        }
        Ok(())
    }
    async fn load_selected_channel(&mut self) -> Result<()> {
        self.thread_root = None;
        self.timeline = TimelineState {
            at_live_bottom: true,
            ..TimelineState::default()
        };
        self.cache_dirty = true;
        self.hydrate_cache().await?;
        if let Some(channel) = self.current_channel().map(|channel| channel.id) {
            self.subscribe_channel(channel).await?;
            self.spawn_backfill(channel);
        }
        Ok(())
    }
    async fn toggle_thread(&mut self) -> Result<()> {
        if self.thread_root.is_some() {
            self.thread_root = None;
            self.pane = Pane::Timeline;
            return Ok(());
        }
        if let Some(message) = self.selected_message() {
            let root = message
                .root_event_id
                .clone()
                .unwrap_or_else(|| message.event_id.clone());
            self.thread_root = Some(root);
            self.pane = Pane::Thread;
            self.cache_dirty = true;
            self.hydrate_cache().await?;
        }
        Ok(())
    }
    async fn enter_composer(&mut self) -> Result<()> {
        if self.runtime.is_none() {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return Ok(());
        }
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return Ok(());
        };
        let root = self.thread_root.clone();
        let (body, attachments, mentions) = self
            .store
            .call(move |store| store.draft_with_media_mentions(community, channel, root.as_deref()))
            .await?;
        self.composer.set_draft(body, attachments, mentions);
        self.refresh_mention_picker().await?;
        self.mode = Mode::Insert;
        let pending = self
            .composer
            .attachments
            .iter()
            .filter_map(|attachment| match attachment {
                crate::media::DraftAttachment::Pending(value) => Some(value.clone()),
                crate::media::DraftAttachment::Uploaded(_) => None,
            })
            .collect::<Vec<_>>();
        for attachment in pending {
            self.start_pending_upload(attachment);
        }
        Ok(())
    }

    async fn insert_action(&mut self, action: KeyAction) -> Result<()> {
        if self.mention_picker.is_some() {
            match action {
                KeyAction::Up => {
                    if let Some(picker) = &mut self.mention_picker {
                        picker.move_by(-1);
                    }
                    return Ok(());
                }
                KeyAction::Down => {
                    if let Some(picker) = &mut self.mention_picker {
                        picker.move_by(1);
                    }
                    return Ok(());
                }
                KeyAction::Complete | KeyAction::Submit => {
                    self.accept_mention();
                    self.persist_draft().await?;
                    return Ok(());
                }
                KeyAction::Escape => {
                    self.mention_picker = None;
                    return Ok(());
                }
                _ => {}
            }
        }
        match action {
            KeyAction::Escape => {
                self.mention_picker = None;
                self.mode = Mode::Normal;
            }
            KeyAction::Character(character) => self.composer.insert(character),
            KeyAction::Backspace => self.composer.backspace(),
            KeyAction::ForwardDelete => self.composer.delete(),
            KeyAction::Left => self.composer.move_left(),
            KeyAction::Right => self.composer.move_right(),
            KeyAction::Newline => self.composer.newline(),
            KeyAction::Attach => {
                self.attachment_input.clear();
                self.mode = Mode::Attachment;
            }
            KeyAction::RemoveAttachment => {
                if let Some(crate::media::DraftAttachment::Pending(pending)) =
                    self.composer.attachments.pop()
                    && let Some(community) = self.active_community_id()
                {
                    self.uploading_media
                        .remove(&format!("{community}:{}", pending.sha256));
                    let path = self.media.staging_dir(community).join(pending.cache_name);
                    tokio::spawn(async move {
                        let _ = tokio::fs::remove_file(path).await;
                    });
                }
            }
            KeyAction::RetryAttachments => {
                let pending = self
                    .composer
                    .attachments
                    .iter()
                    .filter_map(|attachment| match attachment {
                        crate::media::DraftAttachment::Pending(value) => Some(value.clone()),
                        crate::media::DraftAttachment::Uploaded(_) => None,
                    })
                    .collect::<Vec<_>>();
                for attachment in pending {
                    self.start_pending_upload(attachment);
                }
            }
            KeyAction::Submit => {
                if let Some(message) = self.composer.take_message() {
                    self.queue_message(message);
                    self.mode = Mode::Normal
                }
            }
            _ => {}
        }
        self.refresh_mention_picker().await?;
        self.persist_draft().await
    }

    async fn refresh_mention_picker(&mut self) -> Result<()> {
        let Some(range) = self.composer.active_mention() else {
            self.mention_picker = None;
            return Ok(());
        };
        let Some(community) = self.active_community_id() else {
            self.mention_picker = None;
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            self.mention_picker = None;
            return Ok(());
        };
        let Some(self_pubkey) = self.self_pubkey().map(str::to_owned) else {
            self.mention_picker = None;
            return Ok(());
        };
        let query = self.composer.body[range.start + 1..range.end].to_owned();
        let lookup = query.clone();
        let candidates = self
            .store
            .call(move |store| store.mention_candidates(community, channel, &self_pubkey, &lookup))
            .await?;
        self.mention_picker = Some(MentionPicker::new(range, query, candidates));
        Ok(())
    }

    fn accept_mention(&mut self) {
        let selected = self.mention_picker.as_ref().and_then(|picker| {
            picker
                .selected()
                .cloned()
                .map(|candidate| (picker.range.clone(), candidate))
        });
        self.mention_picker = None;
        if let Some((range, candidate)) = selected
            && !self
                .composer
                .accept_mention(range, &candidate.label, &candidate.pubkey)
        {
            self.status_error = Some("could not add that mention".into());
        }
    }

    fn open_media_preview(&mut self) {
        if self
            .selected_message()
            .is_some_and(|message| !message.attachments.is_empty())
        {
            self.preview_index = 0;
            self.preview_revealed = false;
            self.mode = Mode::MediaPreview;
        } else {
            self.status_error = Some("the selected message has no attachments".into());
        }
    }

    fn preview_attachment(&self) -> Option<&crate::media::Attachment> {
        self.selected_message()
            .and_then(|message| message.attachments.get(self.preview_index))
    }

    fn media_preview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('[') | KeyCode::Left => {
                self.preview_index = self.preview_index.saturating_sub(1);
                self.preview_revealed = false;
            }
            KeyCode::Char(']') | KeyCode::Right => {
                let max = self
                    .selected_message()
                    .map_or(0, |message| message.attachments.len().saturating_sub(1));
                self.preview_index = (self.preview_index + 1).min(max);
                self.preview_revealed = false;
            }
            KeyCode::Enter => self.preview_revealed = true,
            KeyCode::Char('r') => {
                if let Some(attachment) = self.preview_attachment().cloned() {
                    self.media.retry(&attachment, 72);
                }
            }
            KeyCode::Char('s') => {
                self.attachment_input.clear();
                self.mode = Mode::SaveAttachment;
            }
            _ => {}
        }
    }

    async fn attachment_path_key(&mut self, key: KeyEvent, save: bool) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = if save {
                    Mode::MediaPreview
                } else {
                    Mode::Insert
                };
                self.attachment_input.clear();
            }
            KeyCode::Backspace => {
                self.attachment_input.pop();
            }
            KeyCode::Enter => {
                let path = std::path::PathBuf::from(self.attachment_input.trim());
                if path.as_os_str().is_empty() {
                    self.status_error = Some("enter an attachment path".into());
                    return Ok(());
                }
                self.attachment_input.clear();
                if save {
                    self.save_preview_attachment(path);
                    self.mode = Mode::MediaPreview;
                } else {
                    self.start_attachment_upload(path);
                    self.mode = Mode::Insert;
                }
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !character.is_control()
                    && self.attachment_input.len() < 4_096 =>
            {
                self.attachment_input.push(character);
            }
            _ => {}
        }
        Ok(())
    }

    fn start_attachment_upload(&mut self, source: std::path::PathBuf) {
        if self.composer.attachments.len() >= 8 {
            self.status_error = Some("a message can contain at most 8 attachments".into());
            return;
        }
        let Some(community) = self.active_community_id() else {
            return;
        };
        let staging = self.media.staging_dir(community);
        let tx = self.background_tx.clone();
        self.status_error = Some("processing attachment…".into());
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::media::decode::stage_file(&source, &staging)
            })
            .await
            .map_err(|_| Error::Protocol("attachment worker stopped".into()))
            .and_then(std::convert::identity);
            let _ = tx
                .send(match result {
                    Ok(staged) => Background::Staged {
                        community,
                        pending: staged.pending(),
                    },
                    Err(error) => Background::Failed(public_media_error(&error)),
                })
                .await;
        });
    }

    fn start_pending_upload(&mut self, pending: crate::media::PendingAttachment) {
        let Some(runtime) = &self.runtime else {
            self.status_error =
                Some("attachment staged; upload waits for an unlocked identity".into());
            return;
        };
        let community = runtime.community_id;
        let job = format!("{community}:{}", pending.sha256);
        if !self.uploading_media.insert(job) {
            return;
        }
        if pending.cache_name.contains(['/', '\\'])
            || !pending.cache_name.starts_with(&pending.sha256)
        {
            self.uploading_media
                .remove(&format!("{community}:{}", pending.sha256));
            self.status_error = Some("staged attachment metadata is invalid".into());
            return;
        }
        let path = self.media.staging_dir(community).join(&pending.cache_name);
        let client = runtime.media.clone();
        let tx = self.background_tx.clone();
        self.status_error = Some("uploading attachment…".into());
        tokio::spawn(async move {
            let result = client
                .upload(&path, &pending.mime, Some(pending.filename.clone()))
                .await;
            if result.is_ok() {
                let _ = tokio::fs::remove_file(&path).await;
            }
            let _ = tx
                .send(match result {
                    Ok(attachment) => Background::Uploaded {
                        community,
                        sha256: pending.sha256,
                        attachment: Box::new(attachment),
                    },
                    Err(error) => Background::UploadFailed {
                        community,
                        sha256: pending.sha256,
                        message: public_media_error(&error),
                    },
                })
                .await;
        });
    }

    fn save_preview_attachment(&mut self, destination: std::path::PathBuf) {
        let Some(attachment) = self.preview_attachment().cloned() else {
            return;
        };
        let Some(community) = self.active_community_id() else {
            return;
        };
        let cached = self.media.cache_path(community, &attachment);
        let client = self.runtime.as_ref().map(|runtime| runtime.media.clone());
        let tx = self.background_tx.clone();
        self.status_error = Some("saving attachment…".into());
        tokio::spawn(async move {
            let result = async {
                let source = if cached.exists() {
                    crate::media::client::verify_file(&cached, &attachment.sha256, attachment.size)
                        .await?;
                    cached
                } else {
                    client
                        .ok_or_else(|| {
                            Error::Locked("uncached media requires an unlocked identity".into())
                        })?
                        .fetch(&attachment, &cached)
                        .await?
                };
                let mut input = tokio::fs::File::open(&source)
                    .await
                    .map_err(|error| Error::io(&source, error))?;
                let mut output = tokio::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&destination)
                    .await
                    .map_err(|error| Error::io(&destination, error))?;
                crate::paths::set_private_permissions(&destination)?;
                tokio::io::copy(&mut input, &mut output)
                    .await
                    .map_err(|error| Error::io(&destination, error))?;
                output
                    .sync_all()
                    .await
                    .map_err(|error| Error::io(&destination, error))?;
                Ok::<_, Error>(())
            }
            .await;
            let _ = tx
                .send(match result {
                    Ok(()) => Background::Saved,
                    Err(error) => Background::Failed(public_media_error(&error)),
                })
                .await;
        });
    }

    async fn persist_draft(&self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return Ok(());
        };
        let root = self.thread_root.clone();
        let body = self.composer.body.clone();
        let attachments = self.composer.attachments.clone();
        let mentions = self.composer.mentions().to_vec();
        self.store
            .call(move |store| {
                store.save_draft_with_media_mentions(
                    community,
                    channel,
                    root.as_deref(),
                    &body,
                    &attachments,
                    &mentions,
                )
            })
            .await
    }

    fn queue_message(&mut self, message: crate::ui::composer::PreparedMessage) {
        let Some(runtime) = &self.runtime else { return };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return;
        };
        let service = runtime.messages.clone();
        let root = self.thread_root.clone();
        let parent = self
            .selected_message()
            .map(|message| message.event_id.clone())
            .or_else(|| root.clone());
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = if let (Some(root), Some(parent)) = (root, parent) {
                service
                    .reply_with_media_mentions(
                        channel,
                        &root,
                        &parent,
                        &message.body,
                        &message.attachments,
                        &message.mentions,
                    )
                    .await
            } else {
                service
                    .send_with_media_mentions(
                        channel,
                        &message.body,
                        &message.attachments,
                        &message.mentions,
                    )
                    .await
            };
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
        self.cache_dirty = true;
    }
    fn send_reaction(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let Some(message) = self.selected_message() else {
            return;
        };
        let service = runtime.messages.clone();
        let target = message.event_id.clone();
        let emoji = crate::ui::reaction_picker::REACTIONS[self.reaction_index].to_owned();
        let self_pubkey = runtime.signer.public_key().to_hex();
        let own_reaction = self.reactions.get(&target).and_then(|reactions| {
            reactions
                .iter()
                .find(|reaction| {
                    !reaction.deleted && reaction.pubkey == self_pubkey && reaction.emoji == emoji
                })
                .map(|reaction| reaction.event_id.clone())
        });
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = if let Some(reaction) = own_reaction {
                service.remove_reaction(&reaction).await
            } else {
                service.react(&target, &emoji).await
            };
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
        self.mode = Mode::Normal;
    }
    fn delete_selected(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return;
        };
        let Some(message) = self.selected_message() else {
            return;
        };
        let service = runtime.messages.clone();
        let target = message.event_id.clone();
        let author = message.pubkey.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = service.delete(channel, &target, &author).await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
    }

    async fn mark_unread(&mut self) -> Result<()> {
        if let Some(channel) = self.current_channel().map(|channel| channel.id) {
            self.manual_unread.insert(channel);
            self.persist_manual_unread().await?;
        }
        Ok(())
    }
    async fn mark_current_read(&mut self) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let service = runtime.read_state.clone();
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return Ok(());
        };
        let mut marks = Vec::new();
        if self.pane == Pane::Thread
            && let Some(root) = &self.thread_root
            && let Some(last) = self.thread_messages.last()
        {
            marks.push((
                format!("thread:{root}"),
                u32::try_from(last.created_at).unwrap_or(u32::MAX),
            ));
            marks.extend(self.thread_messages.iter().skip(1).map(|message| {
                (
                    format!("msg:{}", message.event_id),
                    u32::try_from(message.created_at).unwrap_or(u32::MAX),
                )
            }));
        } else if let Some(last) = self.messages.last() {
            marks.push((
                channel.to_string(),
                u32::try_from(last.created_at).unwrap_or(u32::MAX),
            ));
        }
        let mut advanced = false;
        for (context, at) in marks {
            if self
                .last_marked
                .get(&context)
                .is_some_and(|previous| *previous >= at)
            {
                continue;
            }
            service.mark(&context, at).await?;
            self.last_marked.insert(context, at);
            advanced = true;
        }
        if clear_visible_unread(&mut self.computed_unread, &mut self.manual_unread, channel) {
            self.persist_manual_unread().await?;
        }
        if advanced {
            self.read_dirty_since.get_or_insert_with(Instant::now);
        }
        Ok(())
    }
    async fn persist_manual_unread(&self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let values = self.manual_unread.clone();
        self.store
            .call(move |store| store.save_ui_state(community, "manual_unread", &values))
            .await
    }

    fn open_inbox(&mut self) {
        self.mode = Mode::Inbox;
        self.inbox_state.narrow_detail = false;
        self.spawn_inbox_load(self.runtime.is_some());
    }

    fn open_search(&mut self) {
        self.mode = Mode::Search;
        self.search_state = SearchState::default();
        self.search_state.changed();
        self.search_dirty_since = None;
        self.spawn_search();
    }

    fn open_dm_picker(&mut self, add_to: Option<Uuid>) {
        if self.runtime.is_none() {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return;
        }
        self.dm_picker = DmPickerState {
            add_to,
            ..DmPickerState::default()
        };
        self.dm_dirty_since = None;
        if let Some(pubkey) = self.self_pubkey().map(str::to_owned) {
            self.dm_picker.reconcile(&self.profiles, &pubkey);
        }
        self.mode = Mode::DmPicker;
    }

    fn hide_current_dm(&mut self) {
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return;
        };
        let Some(channel) = self
            .current_channel()
            .filter(|channel| channel.kind.is_dm())
            .map(|channel| channel.id)
        else {
            self.status_error = Some("select a workspace DM before hiding it".into());
            return;
        };
        let service = runtime.dms.clone();
        let community = runtime.community_id;
        let tx = self.background_tx.clone();
        self.status_error = Some("hiding DM…".into());
        tokio::spawn(async move {
            let event = match service.hide(channel).await {
                Ok(confirmed) => Background::DmHidden {
                    community,
                    channel,
                    confirmed,
                },
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        });
    }

    async fn search_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                if let Some(task) = self.search_task.take() {
                    task.abort();
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.search_state.query.pop();
                self.search_state.changed();
                self.search_dirty_since = Some(Instant::now());
            }
            KeyCode::Char('j') | KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_state.move_by(1)
            }
            KeyCode::Char('k') | KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_state.move_by(-1)
            }
            KeyCode::Down => self.search_state.move_by(1),
            KeyCode::Up => self.search_state.move_by(-1),
            KeyCode::Enter => self.open_search_result().await?,
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !character.is_control()
                    && self.search_state.query.len() + character.len_utf8() <= 4_096 =>
            {
                self.search_state.query.push(character);
                self.search_state.changed();
                self.search_dirty_since = Some(Instant::now());
            }
            _ => {}
        }
        Ok(())
    }

    async fn open_search_result(&mut self) -> Result<()> {
        let Some(result) = self.search_state.selected().cloned() else {
            return Ok(());
        };
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        if result.kind == SearchResultKind::Person {
            let Some(pubkey) = result.pubkey else {
                return Ok(());
            };
            self.start_dm_open(vec![pubkey]);
            return Ok(());
        }
        let Some(channel_id) = result.channel_id else {
            return Ok(());
        };
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        if result.kind == SearchResultKind::Message {
            let identity = self.self_pubkey().unwrap_or_default().to_owned();
            let event_id = result.event_id.clone().unwrap_or_default();
            let current = self
                .store
                .call(move |store| store.search_result_for_event(community, &identity, &event_id))
                .await?;
            if current.is_none() {
                self.status_error =
                    Some("the search result is deleted, hidden, or no longer accessible".into());
                return Ok(());
            }
        }
        self.open_channel_context(
            channel_id,
            result.event_id.as_deref(),
            result.thread_root.as_deref(),
        )
        .await?;
        self.mode = Mode::Normal;
        Ok(())
    }

    fn dm_picker_key(&mut self, key: KeyEvent) {
        let self_pubkey = self.self_pubkey().unwrap_or_default().to_owned();
        match key.code {
            KeyCode::Esc => {
                if let Some(task) = self.dm_search_task.take() {
                    task.abort();
                }
                self.dm_picker = DmPickerState::default();
                self.dm_dirty_since = None;
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace if !self.dm_picker.submitting => {
                self.dm_picker.query.pop();
                self.dm_picker.reconcile(&self.profiles, &self_pubkey);
                self.dm_dirty_since = Some(Instant::now());
            }
            KeyCode::Char('j')
                if key.modifiers.contains(KeyModifiers::CONTROL) && !self.dm_picker.submitting =>
            {
                self.dm_picker.move_by(&self.profiles, &self_pubkey, 1);
            }
            KeyCode::Down if !self.dm_picker.submitting => {
                self.dm_picker.move_by(&self.profiles, &self_pubkey, 1);
            }
            KeyCode::Char('k')
                if key.modifiers.contains(KeyModifiers::CONTROL) && !self.dm_picker.submitting =>
            {
                self.dm_picker.move_by(&self.profiles, &self_pubkey, -1);
            }
            KeyCode::Up if !self.dm_picker.submitting => {
                self.dm_picker.move_by(&self.profiles, &self_pubkey, -1);
            }
            KeyCode::Char(' ') if !self.dm_picker.submitting => {
                if let Err(message) = self.dm_picker.toggle_selected() {
                    self.status_error = Some(message.into());
                }
            }
            KeyCode::Enter if !self.dm_picker.submitting => {
                if self.dm_picker.recipients.is_empty() {
                    self.status_error = Some("select at least one recipient with Space".into());
                    return;
                }
                self.dm_picker.submitting = true;
                if let Some(channel) = self.dm_picker.add_to {
                    let Some(pubkey) = self.dm_picker.recipients.iter().next().cloned() else {
                        return;
                    };
                    self.start_dm_add(channel, pubkey);
                } else {
                    self.start_dm_open(self.dm_picker.recipients.iter().cloned().collect());
                }
            }
            KeyCode::Char(character)
                if !self.dm_picker.submitting
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !character.is_control()
                    && self.dm_picker.query.len() + character.len_utf8() <= 4_096 =>
            {
                self.dm_picker.query.push(character);
                self.dm_picker.reconcile(&self.profiles, &self_pubkey);
                self.dm_dirty_since = Some(Instant::now());
            }
            _ => {}
        }
    }

    fn start_dm_open(&mut self, recipients: Vec<String>) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        let service = runtime.dms.clone();
        let community = runtime.community_id;
        let tx = self.background_tx.clone();
        self.status_error = Some("opening private workspace DM…".into());
        tokio::spawn(async move {
            let event = match service.open(recipients).await {
                Ok(result) => Background::DmOpened { community, result },
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        });
    }

    fn start_dm_add(&mut self, channel: Uuid, pubkey: String) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        let service = runtime.dms.clone();
        let community = runtime.community_id;
        let tx = self.background_tx.clone();
        self.status_error = Some("opening a new participant-set DM…".into());
        tokio::spawn(async move {
            let event = match service.add_member(channel, pubkey).await {
                Ok(result) => Background::DmOpened { community, result },
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        });
    }

    async fn inbox_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc if self.inbox_state.narrow_detail => {
                self.inbox_state.narrow_detail = false;
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => self.inbox_state.move_by(&self.inbox_items, 1),
            KeyCode::Char('k') | KeyCode::Up => self.inbox_state.move_by(&self.inbox_items, -1),
            KeyCode::Char('f') => {
                self.inbox_state.filter = self.inbox_state.filter.next();
                self.inbox_state.reconcile(&self.inbox_items);
            }
            KeyCode::Enter => self.inbox_state.narrow_detail = true,
            KeyCode::Char('o') => self.open_inbox_item(false).await?,
            KeyCode::Char('i') => self.open_inbox_item(true).await?,
            KeyCode::Char('m') => self.mark_selected_inbox_read().await?,
            KeyCode::Char('U') => self.toggle_selected_inbox_unread().await?,
            KeyCode::Char('a') => self.mark_all_loaded_inbox_read().await?,
            _ => {}
        }
        Ok(())
    }

    async fn open_inbox_item(&mut self, reply: bool) -> Result<()> {
        let Some(item) = self.inbox_state.selected(&self.inbox_items).cloned() else {
            return Ok(());
        };
        let Some(channel) = item.channel_id else {
            self.status_error = Some("this read-only Inbox card has no channel context".into());
            return Ok(());
        };
        self.open_channel_context(
            channel,
            item.event_id.as_deref(),
            item.thread_root.as_deref(),
        )
        .await?;
        self.mode = Mode::Normal;
        if reply {
            self.enter_composer().await?;
        }
        Ok(())
    }

    async fn open_channel_context(
        &mut self,
        channel: Uuid,
        event_id: Option<&str>,
        thread_root: Option<&str>,
    ) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let visible = self
            .store
            .call(move |store| {
                Ok(store
                    .channels(community)?
                    .into_iter()
                    .any(|value| value.id == channel))
            })
            .await?;
        if !visible {
            self.status_error =
                Some("the source channel is unavailable or no longer accessible".into());
            return Ok(());
        }
        if !self.channels.iter().any(|value| value.id == channel) {
            self.cache_dirty = true;
            self.hydrate_cache().await?;
        }
        let Some(index) = self.channels.iter().position(|value| value.id == channel) else {
            self.status_error =
                Some("the source channel is unavailable or no longer accessible".into());
            return Ok(());
        };
        self.selected_channel = index;
        self.showing_open_channel = !self.channels[index].is_member;
        self.load_selected_channel().await?;
        if let Some(root) = thread_root {
            self.thread_root = Some(root.to_owned());
            self.pane = Pane::Thread;
            self.thread_timeline.selected_event = event_id.map(str::to_owned);
        } else {
            self.pane = Pane::Timeline;
            self.timeline.selected_event = event_id.map(str::to_owned);
        }
        self.cache_dirty = true;
        self.hydrate_cache().await?;
        Ok(())
    }

    async fn mark_selected_inbox_read(&mut self) -> Result<()> {
        let Some(item) = self.inbox_state.selected(&self.inbox_items).cloned() else {
            return Ok(());
        };
        self.mark_inbox_item_read(&item).await?;
        self.spawn_inbox_load(false);
        Ok(())
    }

    async fn mark_inbox_item_read(&mut self, item: &InboxItem) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(identity) = self.self_pubkey().map(str::to_owned) else {
            return Ok(());
        };
        let conversation = item.conversation_id.clone();
        let created_at = item.created_at;
        self.store
            .call(move |store| {
                store.set_inbox_override(
                    community,
                    &identity,
                    &conversation,
                    false,
                    Some(created_at),
                )
            })
            .await?;
        if let Some(runtime) = &self.runtime {
            let at = u32::try_from(item.created_at).unwrap_or(u32::MAX);
            if item.categories.contains(&InboxCategory::Dm)
                && let Some(channel) = item.channel_id
            {
                runtime.read_state.mark(&channel.to_string(), at).await?;
            }
            if let Some(root) = &item.thread_root {
                runtime
                    .read_state
                    .mark(&format!("thread:{root}"), at)
                    .await?;
            }
            if let Some(event_id) = &item.event_id {
                runtime
                    .read_state
                    .mark(&format!("msg:{event_id}"), at)
                    .await?;
            }
            self.read_dirty_since.get_or_insert_with(Instant::now);
        }
        Ok(())
    }

    async fn toggle_selected_inbox_unread(&mut self) -> Result<()> {
        let Some(item) = self.inbox_state.selected(&self.inbox_items).cloned() else {
            return Ok(());
        };
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(identity) = self.self_pubkey().map(str::to_owned) else {
            return Ok(());
        };
        let conversation = item.conversation_id.clone();
        let forced = !item.forced_unread;
        self.store
            .call(move |store| {
                store.set_inbox_override(community, &identity, &conversation, forced, None)
            })
            .await?;
        self.spawn_inbox_load(false);
        Ok(())
    }

    async fn mark_all_loaded_inbox_read(&mut self) -> Result<()> {
        let items = self
            .inbox_state
            .visible(&self.inbox_items)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        for item in &items {
            self.mark_inbox_item_read(item).await?;
        }
        self.spawn_inbox_load(false);
        Ok(())
    }

    async fn text_overlay_key(&mut self, key: KeyEvent, mode: Mode) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                if mode == Mode::Finder {
                    self.finder.pop();
                } else {
                    self.command.pop();
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if mode == Mode::Finder {
                    self.finder.push(character)
                } else {
                    self.command.push(character)
                }
            }
            KeyCode::Enter => {
                if mode == Mode::Finder {
                    if let Some(found) =
                        crate::ui::finder::rank(&self.finder, &self.channels).first()
                        && let Some(index) = self
                            .channels
                            .iter()
                            .position(|channel| channel.id == found.id)
                    {
                        self.selected_channel = index;
                        self.showing_open_channel = !found.is_member;
                        self.load_selected_channel().await?;
                        self.pane = Pane::Timeline;
                    }
                    self.mode = Mode::Normal
                } else {
                    self.execute_command().await?;
                    self.mode = Mode::Normal
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn open_theme_picker(&mut self) {
        let scope = if self.config.communities.is_empty() {
            ThemeScope::Global
        } else {
            ThemeScope::Community
        };
        self.theme_before_preview = Some(self.theme.clone());
        self.theme_picker = Some(ThemePicker::open(self.theme.id(), scope));
        self.mode = Mode::Theme;
    }

    fn theme_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if let Some(theme) = self.theme_before_preview.take() {
                    self.theme = theme;
                }
                self.theme_picker = None;
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Enter => {
                self.confirm_theme_selection();
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(picker) = &mut self.theme_picker {
                    picker.toggle_scope();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(picker) = &mut self.theme_picker {
                    picker.move_by(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(picker) = &mut self.theme_picker {
                    picker.move_by(-1);
                }
            }
            KeyCode::Backspace => {
                if let Some(picker) = &mut self.theme_picker {
                    picker.backspace();
                }
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(picker) = &mut self.theme_picker {
                    picker.push(character);
                }
            }
            _ => return,
        }
        self.preview_selected_theme();
    }

    fn preview_selected_theme(&mut self) {
        let Some(entry) = self.theme_picker.as_ref().and_then(ThemePicker::selected) else {
            return;
        };
        match theme::load(&self.paths, entry.id) {
            Ok(loaded) => {
                self.theme = loaded.theme;
                if let Some(warning) = loaded.warnings.first() {
                    self.status_error = Some(format!("theme warning: {warning}"));
                }
            }
            Err(error) => {
                self.theme = Theme::builtin(entry.id).unwrap_or_default();
                self.status_error = Some(format!("theme warning: {error}"));
            }
        }
    }

    fn confirm_theme_selection(&mut self) {
        let Some((entry, scope)) = self
            .theme_picker
            .as_ref()
            .and_then(|picker| picker.selected().map(|entry| (entry, picker.scope())))
        else {
            return;
        };
        let previous = self.config.clone();
        match scope {
            ThemeScope::Community if !self.config.communities.is_empty() => {
                self.config.communities[self.selected_community].theme = Some(entry.id.into());
            }
            ThemeScope::Global | ThemeScope::Community => self.config.ui.theme = entry.id.into(),
        }
        if let Err(error) = self.config.save(&self.paths) {
            self.config = previous;
            if let Some(theme) = self.theme_before_preview.take() {
                self.theme = theme;
            }
            self.status_error = Some(format!("could not save theme: {error}"));
        } else {
            self.reload_theme();
            self.status_error = Some(format!("theme: {} ({scope:?})", entry.name));
            self.theme_before_preview = None;
        }
        self.theme_picker = None;
        self.mode = Mode::Normal;
    }

    fn reload_theme(&mut self) {
        let selected = self
            .config
            .resolved_theme(self.selected_community)
            .to_owned();
        match theme::load(&self.paths, &selected) {
            Ok(loaded) => {
                self.theme = loaded.theme;
                self.status_error = loaded
                    .warnings
                    .first()
                    .map(|warning| format!("theme warning: {warning}"))
                    .or_else(|| Some(format!("theme reloaded: {}", self.theme.name())));
            }
            Err(error) => {
                self.theme = Theme::builtin(&selected).unwrap_or_default();
                self.status_error = Some(format!("theme warning: {error}"));
            }
        }
    }

    async fn execute_command(&mut self) -> Result<()> {
        match crate::ui::command::parse(&self.command) {
            crate::ui::command::Command::Lock => {
                if let Some(runtime) = self.runtime.take() {
                    runtime.supervisor.shutdown().await;
                    runtime.signer.lock().await;
                }
                self.connection = ConnectionState::Locked;
                self.status_error =
                    Some("identity locked for this process; restart bzz to unlock it again".into())
            }
            crate::ui::command::Command::Reconnect => {
                if let Some(runtime) = &self.runtime {
                    runtime.supervisor.reconnect().await
                }
            }
            crate::ui::command::Command::Resync => {
                if let (Some(community), Some(channel)) = (
                    self.active_community_id(),
                    self.current_channel().map(|channel| channel.id),
                ) {
                    let scope_id = channel.to_string();
                    self.store
                        .call(move |store| store.reset_sync_cursor(community, "history", &scope_id))
                        .await?;
                    self.spawn_backfill(channel)
                }
            }
            crate::ui::command::Command::ThemeReload => self.reload_theme(),
            crate::ui::command::Command::MediaReload => {
                self.media.initialize_terminal();
                self.status_error = Some(format!(
                    "media renderer reloaded: {}",
                    self.media.protocol_name()
                ));
            }
            crate::ui::command::Command::Inbox => self.open_inbox(),
            crate::ui::command::Command::Search => self.open_search(),
            crate::ui::command::Command::Dm => self.open_dm_picker(None),
            crate::ui::command::Command::DmHide => self.hide_current_dm(),
            crate::ui::command::Command::DmAdd => {
                let channel = self
                    .current_channel()
                    .filter(|channel| channel.kind.is_dm())
                    .map(|channel| channel.id);
                self.open_dm_picker(channel);
            }
            crate::ui::command::Command::PurgeCache => {
                if let Some(community) = self.active_community_id() {
                    self.store
                        .call(move |store| store.purge_community(community))
                        .await?;
                    self.channels.clear();
                    self.messages.clear()
                }
            }
            crate::ui::command::Command::Unknown(value) => {
                self.status_error = Some(format!("unknown command: {value}"))
            }
            _ => self.status_error = Some("use the CLI to add/remove communities".into()),
        }
        Ok(())
    }

    async fn switch_community(
        &mut self,
        index: usize,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        if index == self.selected_community {
            return Ok(());
        }
        let target = &self.config.communities[index];
        let target_id = target.id;
        let reuse = self
            .runtime
            .as_ref()
            .filter(|runtime| runtime.identity_id == target.identity_id)
            .map(|runtime| runtime.signer.clone());
        let needs_prompt = reuse.is_none()
            && self
                .config
                .identities
                .iter()
                .find(|identity| identity.id == target.identity_id)
                .is_some_and(|identity| matches!(identity.backend, KeyBackend::EncryptedFile));
        if needs_prompt {
            guard.restore();
        }
        let built = Runtime::build(&self.config, index, &self.paths, self.store.clone(), reuse);
        if needs_prompt {
            let (new_guard, new_terminal) = TerminalGuard::enter(self.config.ui.mouse.enabled())?;
            *guard = new_guard;
            *terminal = new_terminal;
        }
        match built {
            Ok(runtime) => {
                if let Some(old) = self.runtime.take() {
                    old.supervisor.shutdown().await;
                    if old.identity_id != runtime.identity_id {
                        old.signer.lock().await;
                    }
                }
                self.media.bind(runtime.community_id, runtime.media.clone());
                self.runtime = Some(runtime);
                self.selected_community = index;
                self.config.default_community = Some(target_id);
                self.config.save(&self.paths)?;
                self.reload_theme();
                self.selected_channel = 0;
                self.showing_open_channel = false;
                self.thread_root = None;
                self.last_marked.clear();
                self.profile_requested.clear();
                self.subscribed_channels.clear();
                self.inbox_items.clear();
                self.inbox_state = InboxState::default();
                self.search_state = SearchState::default();
                self.dm_picker = DmPickerState::default();
                self.dm_dirty_since = None;
                self.connection = ConnectionState::Connecting;
                self.last_directory_refresh = Instant::now();
                self.last_inbox_refresh = Instant::now();
                self.hydrate_cache().await?;
                self.start_sync().await?;
            }
            Err(error) => {
                let recovery = identity_recovery_connection(&error);
                if let Some(connection) = recovery {
                    if let Some(old) = self.runtime.take() {
                        old.supervisor.shutdown().await;
                        old.signer.lock().await;
                    }
                    self.media.select_cached(target_id);
                    self.selected_community = index;
                    self.config.default_community = Some(target_id);
                    self.config.save(&self.paths)?;
                    self.reload_theme();
                    self.selected_channel = 0;
                    self.showing_open_channel = false;
                    self.thread_root = None;
                    self.last_marked.clear();
                    self.profile_requested.clear();
                    self.subscribed_channels.clear();
                    self.inbox_items.clear();
                    self.inbox_state = InboxState::default();
                    self.search_state = SearchState::default();
                    self.dm_picker = DmPickerState::default();
                    self.dm_dirty_since = None;
                    self.connection = connection;
                    self.status_error = Some(error.to_string());
                    self.hydrate_cache().await?;
                } else {
                    self.status_error = Some(error.to_string());
                }
            }
        }
        Ok(())
    }

    fn active_community_id(&self) -> Option<Uuid> {
        self.config
            .communities
            .get(self.selected_community)
            .map(|entry| entry.id)
    }
    fn current_channel(&self) -> Option<&Channel> {
        self.channels.get(self.selected_channel)
    }
    fn self_pubkey(&self) -> Option<&str> {
        let identity_id = self
            .config
            .communities
            .get(self.selected_community)?
            .identity_id;
        self.config
            .identities
            .iter()
            .find(|identity| identity.id == identity_id)
            .map(|identity| identity.pubkey.as_str())
    }
    fn selected_message(&self) -> Option<&Message> {
        match self.pane {
            Pane::Thread => self
                .thread_timeline
                .selected_index(&self.thread_messages)
                .and_then(|index| self.thread_messages.get(index)),
            _ => self
                .timeline
                .selected_index(&self.messages)
                .and_then(|index| self.messages.get(index)),
        }
    }

    async fn shutdown(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                outbox::flush(
                    runtime.community_id,
                    &runtime.http,
                    &runtime.supervisor,
                    &self.store,
                ),
            )
            .await;
            if self.read_dirty_since.is_some() {
                let max_seen = self.messages.last().map_or(0, |message| message.created_at);
                let _ = tokio::time::timeout(
                    Duration::from_secs(3),
                    runtime.read_state.publish(max_seen),
                )
                .await;
            }
            runtime.supervisor.shutdown().await;
            runtime.signer.lock().await;
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.render_generation = self.render_generation.wrapping_add(1);
        let mut hit_map = HitMap::new(self.render_generation);
        let area = frame.area();
        if area.width < 50 || area.height < 12 {
            frame.render_widget(
                Paragraph::new("bzz needs at least 50×12\nResize the terminal or press Q to quit")
                    .style(self.theme.style(HighlightGroup::Normal))
                    .alignment(Alignment::Center)
                    .block(
                        Block::bordered()
                            .border_type(self.theme.border_type(BorderSurface::Pane))
                            .border_style(self.theme.style(HighlightGroup::PaneBorder))
                            .title_style(self.theme.style(HighlightGroup::PaneTitle))
                            .title(" terminal too small "),
                    ),
                area,
            );
            self.last_hit_map = Some(hit_map);
            return;
        }
        if self.config.communities.is_empty() {
            self.render_empty(frame, area);
            self.render_overlay(frame, area, &mut hit_map);
            self.last_hit_map = Some(hit_map);
            return;
        }
        let panes = layout::panes(
            area,
            self.sidebar,
            self.thread_root.is_some(),
            self.config.ui.sidebar_width,
            self.config.ui.thread_width,
        );
        if let Some(rail) = panes.community {
            self.render_communities(frame, rail);
            for (index, _) in self.config.communities.iter().enumerate() {
                if let Some(area) = list_row(rail, index, 1) {
                    hit_map.push(area, HitTarget::Community(index));
                }
            }
        }
        if let Some(sidebar) = panes.sidebar {
            hit_map.push(sidebar, HitTarget::ChannelPane);
            for (row, (index, _)) in self
                .channels
                .iter()
                .enumerate()
                .filter(|(_, channel)| channel.is_member)
                .enumerate()
            {
                if let Some(area) = list_row(sidebar, row, 1) {
                    hit_map.push(area, HitTarget::Channel(index));
                }
            }
            crate::ui::sidebar::render(
                frame,
                sidebar,
                &self.channels,
                self.selected_channel,
                &self.unread_channels(),
                &self.theme,
                self.pane == Pane::Channels,
            );
        }
        let title = self
            .current_channel()
            .map_or_else(|| "timeline".to_owned(), |channel| channel.name.clone());
        let self_pubkey = self.self_pubkey().map(str::to_owned);
        hit_map.push(panes.timeline, HitTarget::Timeline);
        let mut timeline_hits = Vec::new();
        if self.mode == Mode::Normal {
            timeline::render_with_media_and_hits(
                frame,
                panes.timeline,
                &self.messages,
                &self.profiles,
                &self.reactions,
                &self.timeline,
                &title,
                &self.theme,
                self.pane == Pane::Timeline,
                self_pubkey.as_deref(),
                &mut self.media,
                &mut timeline_hits,
            );
        } else {
            timeline::render(
                frame,
                panes.timeline,
                &self.messages,
                &self.profiles,
                &self.reactions,
                &self.timeline,
                &title,
                &self.theme,
                self.pane == Pane::Timeline,
                self_pubkey.as_deref(),
            );
        }
        for hit in timeline_hits {
            hit_map.push(hit.area, HitTarget::TimelineMessage(hit.event_id));
        }
        if let Some(thread) = panes.thread {
            hit_map.push(thread, HitTarget::Thread);
            let mut thread_hits = Vec::new();
            if self.mode == Mode::Normal {
                timeline::render_with_media_and_hits(
                    frame,
                    thread,
                    &self.thread_messages,
                    &self.profiles,
                    &self.reactions,
                    &self.thread_timeline,
                    "thread",
                    &self.theme,
                    self.pane == Pane::Thread,
                    self_pubkey.as_deref(),
                    &mut self.media,
                    &mut thread_hits,
                );
            } else {
                timeline::render(
                    frame,
                    thread,
                    &self.thread_messages,
                    &self.profiles,
                    &self.reactions,
                    &self.thread_timeline,
                    "thread",
                    &self.theme,
                    self.pane == Pane::Thread,
                    self_pubkey.as_deref(),
                );
            }
            for hit in thread_hits {
                hit_map.push(hit.area, HitTarget::ThreadMessage(hit.event_id));
            }
        }
        self.render_status(frame, panes.status);
        self.render_overlay(frame, area, &mut hit_map);
        self.last_hit_map = Some(hit_map);
    }
    fn render_empty(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::styled(
                    "Welcome to bzz",
                    self.theme.style(HighlightGroup::CommunitySelected),
                ),
                Line::default(),
                Line::from("No communities are configured."),
                Line::from("Run bzz identity new, then bzz community add."),
                Line::default(),
                Line::from("? help · Q quit"),
            ]))
            .style(self.theme.style(HighlightGroup::Normal))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .border_type(self.theme.border_type(BorderSurface::Pane))
                    .border_style(self.theme.style(HighlightGroup::PaneBorder))
                    .title_style(self.theme.style(HighlightGroup::PaneTitle))
                    .title(" bzz "),
            ),
            area,
        );
    }
    fn render_communities(&self, frame: &mut Frame<'_>, area: Rect) {
        let items = self
            .config
            .communities
            .iter()
            .enumerate()
            .map(|(index, community)| {
                ListItem::new(
                    sanitize::single_line(&community.label)
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_string(),
                )
                .style(self.theme.style(if index == self.selected_community {
                    HighlightGroup::CommunitySelected
                } else {
                    HighlightGroup::CommunityRail
                }))
            });
        frame.render_widget(
            List::new(items)
                .style(self.theme.style(HighlightGroup::CommunityRail))
                .block(
                    Block::bordered()
                        .border_type(self.theme.border_type(BorderSurface::Pane))
                        .border_style(self.theme.style(HighlightGroup::PaneBorder))
                        .title_style(self.theme.style(HighlightGroup::PaneTitle))
                        .title(" b "),
                ),
            area,
        );
    }
    fn render_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let mode = match self.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Finder => "FINDER",
            Mode::Reaction => "REACTION",
            Mode::ConfirmDelete => "CONFIRM",
            Mode::Command => "COMMAND",
            Mode::Theme => "THEME",
            Mode::Help => "HELP",
            Mode::MediaPreview => "MEDIA",
            Mode::Attachment => "ATTACH",
            Mode::SaveAttachment => "SAVE",
            Mode::Inbox => "INBOX",
            Mode::Search => "SEARCH",
            Mode::DmPicker => "DM",
        };
        let mode_group = match self.mode {
            Mode::Insert => HighlightGroup::StatusModeInsert,
            Mode::Command => HighlightGroup::StatusModeCommand,
            _ => HighlightGroup::StatusMode,
        };
        let connection = crate::ui::status::connection_label(self.connection);
        let error = self
            .status_error
            .as_deref()
            .map(sanitize::single_line)
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {mode} "), self.theme.style(mode_group)),
                Span::styled(
                    format!(
                        " · {connection} · img {} · {error} · ? help · Q quit",
                        self.media.protocol_name()
                    ),
                    self.theme.style(HighlightGroup::StatusBar),
                ),
            ]))
            .style(self.theme.style(HighlightGroup::StatusBar)),
            area,
        );
    }
    fn render_overlay(&mut self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        match self.mode {
            Mode::Help => {
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(crate::ui::help::HELP)
                        .style(self.theme.style(HighlightGroup::Normal))
                        .block(
                            Block::bordered()
                                .border_type(self.theme.border_type(BorderSurface::Modal))
                                .border_style(self.theme.style(HighlightGroup::ModalBorder))
                                .title_style(self.theme.style(HighlightGroup::ModalTitle))
                                .title(" help "),
                        )
                        .wrap(Wrap { trim: false }),
                    area,
                )
            }
            Mode::Finder => self.render_finder(frame, area, hit_map),
            Mode::Command => {
                self.render_prompt(frame, area, " command ", &format!(":{}", self.command))
            }
            Mode::Theme => self.render_theme_picker(frame, area, hit_map),
            Mode::Reaction => {
                let popup = centered(area, 70, 5);
                let inner = inner_rect(popup);
                let count = u16::try_from(crate::ui::reaction_picker::REACTIONS.len()).unwrap_or(1);
                let width = (inner.width / count).max(1);
                for index in 0..usize::from(count) {
                    let x = inner.x.saturating_add(width.saturating_mul(index as u16));
                    let final_width = if index + 1 == usize::from(count) {
                        inner.right().saturating_sub(x)
                    } else {
                        width
                    };
                    hit_map.push(
                        Rect::new(x, inner.y, final_width, inner.height),
                        HitTarget::Reaction(index),
                    );
                }
                self.render_prompt(
                    frame,
                    area,
                    " reaction · Enter toggle ",
                    crate::ui::reaction_picker::REACTIONS
                        .iter()
                        .enumerate()
                        .map(|(index, value)| {
                            if index == self.reaction_index {
                                format!("[{value}]")
                            } else {
                                (*value).into()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("  ")
                        .as_str(),
                )
            }
            Mode::ConfirmDelete => self.render_prompt(
                frame,
                area,
                " delete message? ",
                "Press y to delete or n/Esc to cancel",
            ),
            Mode::Insert => {
                let popup = bottom_popup(area, 5);
                let composer_inner = inner_rect(popup);
                hit_map.push(composer_inner, HitTarget::Composer);
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Paragraph::new(format!(
                        "{}{}",
                        sanitize::text(&self.composer.body),
                        if self.composer.attachments.is_empty() {
                            String::new()
                        } else {
                            let ready = self
                                .composer
                                .attachments
                                .iter()
                                .filter(|attachment| attachment.uploaded().is_some())
                                .count();
                            format!(
                                "\n{ready}/{} attachment(s) ready",
                                self.composer.attachments.len()
                            )
                        }
                    ))
                    .style(self.theme.style(HighlightGroup::Composer))
                    .block(
                        Block::bordered()
                            .border_type(self.theme.border_type(BorderSurface::Composer))
                            .border_style(self.theme.style(HighlightGroup::ActiveComposerBorder))
                            .title_style(self.theme.style(HighlightGroup::ComposerTitle))
                            .title(" message · Ctrl-a attach · Ctrl-r retry · Ctrl-x remove "),
                    )
                    .wrap(Wrap { trim: false }),
                    popup,
                );
                if let Some(picker) = &self.mention_picker {
                    let height = u16::try_from(picker.candidates.len().min(5) + 2).unwrap_or(7);
                    let mention_area = Rect::new(
                        popup.x,
                        popup.y.saturating_sub(height),
                        popup.width,
                        height.min(popup.y.saturating_sub(area.y)),
                    );
                    if !mention_area.is_empty() {
                        for index in 0..picker.candidates.len().min(5) {
                            if let Some(row) = list_row(mention_area, index, 1) {
                                hit_map.push(row, HitTarget::MentionCandidate(index));
                            }
                        }
                        crate::ui::mention_picker::render(frame, mention_area, picker, &self.theme);
                    }
                }
            }
            Mode::MediaPreview => self.render_media_preview(frame, area),
            Mode::Attachment => self.render_prompt(
                frame,
                area,
                " attach file · Enter upload · Esc cancel ",
                &self.attachment_input,
            ),
            Mode::SaveAttachment => self.render_prompt(
                frame,
                area,
                " save attachment · no overwrite · Esc cancel ",
                &self.attachment_input,
            ),
            Mode::Inbox => {
                frame.render_widget(Clear, area);
                self.map_inbox_hits(area, hit_map);
                crate::ui::inbox::render(
                    frame,
                    area,
                    &self.inbox_items,
                    &self.profiles,
                    &self.inbox_state,
                    &self.theme,
                    self.inbox_loading,
                );
            }
            Mode::Search => {
                let popup = centered(area, 86, area.height.saturating_sub(4).min(28));
                let offset = 1 + usize::from(self.search_state.notice.is_some());
                for (index, result) in self.search_state.results.iter().enumerate() {
                    if let Some(row) =
                        list_row(popup, offset.saturating_add(index.saturating_mul(2)), 2)
                    {
                        hit_map.push(row, HitTarget::SearchResult(result.stable_id.clone()));
                    }
                }
                crate::ui::search::render(frame, popup, &self.search_state, &self.theme);
            }
            Mode::DmPicker => {
                let popup = centered(area, 86, area.height.saturating_sub(4).min(28));
                let self_pubkey = self.self_pubkey().unwrap_or_default();
                for (index, profile) in self
                    .dm_picker
                    .candidates(&self.profiles, self_pubkey)
                    .into_iter()
                    .take(100)
                    .enumerate()
                {
                    if let Some(row) = list_row(popup, index.saturating_add(2), 1) {
                        hit_map.push(row, HitTarget::DmCandidate(profile.pubkey.clone()));
                    }
                }
                crate::ui::dm_picker::render(
                    frame,
                    popup,
                    &self.dm_picker,
                    &self.profiles,
                    self_pubkey,
                    &self.theme,
                );
            }
            Mode::Normal => {}
        }
    }
    fn render_media_preview(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let popup = centered(area, 86, 80.min(area.height.saturating_sub(2)));
        frame.render_widget(Clear, popup);
        let Some(attachment) = self.preview_attachment().cloned() else {
            self.mode = Mode::Normal;
            return;
        };
        let block = Block::bordered()
            .border_type(self.theme.border_type(BorderSurface::Modal))
            .border_style(self.theme.style(HighlightGroup::ModalBorder))
            .title_style(self.theme.style(HighlightGroup::ModalTitle))
            .title(format!(
                " attachment {}/{} · [/] navigate · s save · Esc close ",
                self.preview_index + 1,
                self.selected_message()
                    .map_or(0, |message| message.attachments.len())
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let details = format!(
            "{}\n{} · {}\n{}",
            sanitize::single_line(attachment.label()),
            attachment.mime,
            crate::media::model::human_size(attachment.size),
            if attachment.spoiler && !self.preview_revealed {
                "spoiler hidden · Enter to reveal"
            } else {
                "r retry · s save"
            }
        );
        frame.render_widget(
            Paragraph::new(details)
                .style(self.theme.style(HighlightGroup::Normal))
                .wrap(Wrap { trim: false }),
            Rect::new(inner.x, inner.y, inner.width, inner.height.min(4)),
        );
        let width = inner.width.saturating_sub(2).max(2);
        let protocol = if attachment.kind == crate::media::MediaKind::Image
            && (!attachment.spoiler || self.preview_revealed)
        {
            self.media.request_inline(&attachment, width, true);
            self.media.state(&attachment, width)
        } else if attachment.kind == crate::media::MediaKind::Video
            && attachment.poster.is_some()
            && (!attachment.spoiler || self.preview_revealed)
        {
            self.media.request_poster(&attachment, width);
            self.media.poster_state(&attachment, width)
        } else {
            None
        };
        if let Some(crate::media::runtime::MediaState::Ready(protocol)) = protocol {
            let image_area = Rect::new(
                inner.x,
                inner.y.saturating_add(4),
                inner.width,
                inner.height.saturating_sub(4),
            );
            frame.render_widget(
                SlicedImage::new(protocol.as_ref(), SignedPosition::from((1, 0))),
                image_area,
            );
        }
    }

    fn render_finder(&self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        let popup = centered(area, 70, 12);
        frame.render_widget(Clear, popup);
        let ranked = crate::ui::finder::rank(&self.finder, &self.channels);
        for (index, channel) in ranked.iter().take(8).enumerate() {
            if let Some(row) = list_row(popup, index.saturating_add(1), 1) {
                hit_map.push(row, HitTarget::FinderChannel(channel.id.to_string()));
            }
        }
        let mut items = vec![ListItem::new(format!(
            "> {}",
            sanitize::single_line(&self.finder)
        ))];
        items.extend(
            ranked
                .into_iter()
                .take(8)
                .enumerate()
                .map(|(index, channel)| {
                    let membership = if channel.is_member { "#" } else { "+" };
                    ListItem::new(format!(
                        "{} {membership}{}",
                        if index == 0 { "›" } else { " " },
                        sanitize::single_line(&channel.name)
                    ))
                }),
        );
        frame.render_widget(
            List::new(items)
                .style(self.theme.style(HighlightGroup::Normal))
                .block(
                    Block::bordered()
                        .border_type(self.theme.border_type(BorderSurface::Picker))
                        .border_style(self.theme.style(HighlightGroup::ModalBorder))
                        .title_style(self.theme.style(HighlightGroup::ModalTitle))
                        .title(" channel finder · Enter open "),
                ),
            popup,
        );
    }
    fn render_theme_picker(&self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        let popup = centered(area, 70, 18);
        frame.render_widget(Clear, popup);
        let Some(picker) = &self.theme_picker else {
            return;
        };
        for (index, (entry, _)) in picker.visible(12).into_iter().enumerate() {
            if let Some(row) = list_row(popup, index.saturating_add(1), 1) {
                hit_map.push(row, HitTarget::Theme(entry.id.into()));
            }
        }
        let scope = match picker.scope() {
            ThemeScope::Global => "global",
            ThemeScope::Community => "community",
        };
        let mut items = vec![ListItem::new(Line::from(vec![
            Span::styled("> ", self.theme.style(HighlightGroup::SelectionMarker)),
            Span::styled(
                sanitize::single_line(picker.query()),
                self.theme.style(HighlightGroup::Normal),
            ),
        ]))];
        items.extend(picker.visible(12).into_iter().map(|(entry, selected)| {
            ListItem::new(format!(
                "{} {}  [{}]",
                if selected { "›" } else { " " },
                entry.name,
                entry.id
            ))
            .style(self.theme.style(if selected {
                HighlightGroup::SelectedRow
            } else {
                HighlightGroup::Normal
            }))
        }));
        frame.render_widget(
            List::new(items)
                .style(self.theme.style(HighlightGroup::Normal))
                .block(
                    Block::bordered()
                        .border_type(self.theme.border_type(BorderSurface::Picker))
                        .border_style(self.theme.style(HighlightGroup::ModalBorder))
                        .title_style(self.theme.style(HighlightGroup::ModalTitle))
                        .title(format!(
                            " themes · {scope} · Tab scope · Enter save · Esc cancel "
                        )),
                ),
            popup,
        );
    }
    fn map_inbox_hits(&self, area: Rect, hit_map: &mut HitMap) {
        let inner = inner_rect(area);
        if inner.width < 88 && self.inbox_state.narrow_detail {
            return;
        }
        let list = if inner.width < 88 {
            inner
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
                .split(inner)[0]
        };
        for (index, item) in self
            .inbox_state
            .visible(&self.inbox_items)
            .into_iter()
            .enumerate()
        {
            let y = list
                .y
                .saturating_add(u16::try_from(index.saturating_mul(2)).unwrap_or(u16::MAX));
            if y >= list.bottom() {
                break;
            }
            hit_map.push(
                Rect::new(
                    list.x,
                    y,
                    list.width,
                    list.bottom().saturating_sub(y).min(2),
                ),
                HitTarget::InboxItem(item.conversation_id.clone()),
            );
        }
    }

    fn render_prompt(&self, frame: &mut Frame<'_>, area: Rect, title: &str, value: &str) {
        let popup = centered(area, 70, 5);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(sanitize::text(value))
                .style(self.theme.style(HighlightGroup::Normal))
                .block(
                    Block::bordered()
                        .border_type(self.theme.border_type(BorderSurface::Modal))
                        .border_style(self.theme.style(HighlightGroup::ModalBorder))
                        .title_style(self.theme.style(HighlightGroup::ModalTitle))
                        .title(title),
                ),
            popup,
        );
    }
    fn unread_channels(&self) -> HashSet<Uuid> {
        self.manual_unread
            .union(&self.computed_unread)
            .copied()
            .collect()
    }
}

#[cfg(unix)]
fn shutdown_signal() -> impl std::future::Future<Output = ()> {
    use tokio::signal::unix::{SignalKind, signal};
    let terminate = signal(SignalKind::terminate());
    let hangup = signal(SignalKind::hangup());
    async move {
        let (Ok(mut terminate), Ok(mut hangup)) = (terminate, hangup) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
            _ = hangup.recv() => {},
        }
    }
}

#[cfg(not(unix))]
fn shutdown_signal() -> impl std::future::Future<Output = ()> {
    async {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn load_theme_safe(
    config: &Config,
    community_index: usize,
    paths: &Paths,
) -> (Theme, Option<String>) {
    let selected = config.resolved_theme(community_index);
    match theme::load(paths, selected) {
        Ok(loaded) => {
            let notice = (!loaded.warnings.is_empty())
                .then(|| format!("theme warning: {}", loaded.warnings.join("; ")));
            (loaded.theme, notice)
        }
        Err(error) => (
            Theme::builtin(selected).unwrap_or_default(),
            Some(format!("theme warning: {error}")),
        ),
    }
}

fn identity_recovery_connection(error: &Error) -> Option<ConnectionState> {
    match error {
        Error::Locked(_) => Some(ConnectionState::Locked),
        Error::IdentityMissing(_) => Some(ConnectionState::IdentityMissing),
        Error::IdentityCorrupt(_) => Some(ConnectionState::IdentityCorrupt),
        _ => None,
    }
}

fn clear_visible_unread(
    computed: &mut HashSet<Uuid>,
    manual: &mut HashSet<Uuid>,
    channel: Uuid,
) -> bool {
    computed.remove(&channel);
    manual.remove(&channel)
}

async fn next_network(runtime: &mut Option<Runtime>) -> Option<SupervisorEvent> {
    match runtime {
        Some(runtime) => loop {
            match runtime.events.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        },
        None => std::future::pending().await,
    }
}
fn dm_label(
    participants: &[String],
    self_pubkey: &str,
    profiles: &HashMap<String, Profile>,
) -> String {
    let labels = participants
        .iter()
        .filter(|pubkey| pubkey.as_str() != self_pubkey)
        .map(|pubkey| {
            profiles
                .get(pubkey)
                .map_or_else(|| crate::domain::abbreviated_pubkey(pubkey), Profile::label)
        })
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [] => "Private workspace DM".into(),
        [label] => label.clone(),
        many => {
            let visible = many.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            if many.len() > 3 {
                format!("{visible} +{}", many.len() - 3)
            } else {
                visible
            }
        }
    }
}

fn public_media_error(error: &Error) -> String {
    match error {
        Error::Config(message) | Error::Protocol(message) | Error::Access(message) => {
            sanitize::single_line(message)
        }
        Error::Io { .. } => "attachment I/O failed".into(),
        Error::Network(_) | Error::Timeout(_) => "attachment network operation failed".into(),
        Error::Locked(_)
        | Error::IdentityMissing(_)
        | Error::IdentityCorrupt(_)
        | Error::Auth(_) => "attachment authorization is unavailable".into(),
        Error::Database(_) | Error::Serialization(_) | Error::Unsupported(_) => {
            "attachment operation failed".into()
        }
    }
}

fn centered(area: Rect, percent: u16, height: u16) -> Rect {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ])
        .split(area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent) / 2),
            Constraint::Percentage(percent),
            Constraint::Percentage((100 - percent) / 2),
        ])
        .split(rows[1]);
    cols[1]
}
fn bottom_popup(area: Rect, height: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(2),
        y: area.bottom().saturating_sub(height + 1),
        width: area.width.saturating_sub(4),
        height: height.min(area.height),
    }
}

fn inner_rect(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn list_row(area: Rect, row: usize, height: u16) -> Option<Rect> {
    let inner = inner_rect(area);
    let y = inner
        .y
        .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
    if y >= inner.bottom() {
        return None;
    }
    Some(Rect::new(
        inner.x,
        y,
        inner.width,
        inner.bottom().saturating_sub(y).min(height),
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Mode, clear_visible_unread, identity_recovery_connection};
    use crate::{
        config::Config,
        domain::{Channel, ChannelKind, ConnectionState, MentionCandidate, Message, Visibility},
        error::Error,
        paths::Paths,
        store::{Store, writer::StoreHandle},
        ui::{hit_map::HitTarget, mention_picker::MentionPicker},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use tempfile::TempDir;
    use uuid::Uuid;

    #[test]
    fn identity_failures_enter_distinct_cache_only_states() {
        assert_eq!(
            identity_recovery_connection(&Error::Locked("keyring".into())),
            Some(ConnectionState::Locked)
        );
        assert_eq!(
            identity_recovery_connection(&Error::IdentityMissing("missing".into())),
            Some(ConnectionState::IdentityMissing)
        );
        assert_eq!(
            identity_recovery_connection(&Error::IdentityCorrupt("corrupt".into())),
            Some(ConnectionState::IdentityCorrupt)
        );
        assert_eq!(
            identity_recovery_connection(&Error::Config("bad".into())),
            None
        );
    }

    #[test]
    fn clearing_manual_unread_does_not_require_a_remote_marker_advance() {
        let channel = Uuid::new_v4();
        let mut computed = HashSet::from([channel]);
        let mut manual = HashSet::from([channel]);
        assert!(clear_visible_unread(&mut computed, &mut manual, channel));
        assert!(!computed.contains(&channel));
        assert!(!manual.contains(&channel));
        assert!(!clear_visible_unread(&mut computed, &mut manual, channel));
    }

    #[tokio::test]
    async fn invalid_theme_toml_falls_back_without_blocking_startup() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        std::fs::write(paths.theme_file(), "[highlight.Normal\n").unwrap();
        let config = Config::default();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let app = super::App::new(config, paths, handle).await.unwrap();
        assert_eq!(app.theme.id(), "bzz");
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|error| error.contains("theme warning"))
        );
    }

    #[tokio::test]
    async fn theme_preview_is_reversible_and_selection_persists_by_scope() {
        let temporary = TempDir::new().unwrap();
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
        let mut app = super::App::new(config, paths.clone(), handle)
            .await
            .unwrap();

        app.open_theme_picker();
        for character in "nord".chars() {
            app.theme_picker_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(app.theme.id(), "nord");
        app.theme_picker_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.theme.id(), "bzz");
        assert_eq!(app.config.ui.theme, "bzz");

        app.open_theme_picker();
        for character in "nord".chars() {
            app.theme_picker_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.theme_picker_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.theme.id(), "nord");
        assert_eq!(app.config.ui.theme, "nord");

        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "theme-test".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:theme-test".into(),
        };
        app.config.identities.push(identity.clone());
        app.config
            .add_community(
                "theme-test".into(),
                "wss://theme.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        app.open_theme_picker();
        for character in "dracula".chars() {
            app.theme_picker_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.theme_picker_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.theme.id(), "dracula");
        assert_eq!(app.config.communities[0].theme.as_deref(), Some("dracula"));
        assert_eq!(Config::load(&paths).unwrap(), app.config);
    }

    #[tokio::test]
    async fn rendered_core_and_mention_rows_have_semantic_hit_targets() {
        let temporary = TempDir::new().unwrap();
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
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "mouse-test".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:mouse-test".into(),
        };
        app.config.identities.push(identity.clone());
        app.config
            .add_community(
                "mouse-test".into(),
                "wss://mouse.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        let channel_id = Uuid::new_v4();
        app.channels = vec![Channel {
            id: channel_id,
            name: "general".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 1,
            last_event_at: None,
        }];
        app.messages = vec![Message {
            event_id: "event-1".into(),
            channel_id,
            pubkey: identity.pubkey,
            created_at: 1,
            content: "hello".into(),
            attachments: vec![],
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            pending: false,
            rejected: None,
        }];
        app.timeline.reconcile(&app.messages);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let panes = crate::ui::layout::panes(
            terminal.size().unwrap().into(),
            app.sidebar,
            false,
            app.config.ui.sidebar_width,
            app.config.ui.thread_width,
        );
        let map = app.last_hit_map.as_ref().unwrap();
        let sidebar = panes.sidebar.unwrap();
        assert_eq!(
            map.hit(sidebar.x.saturating_add(1), sidebar.y.saturating_add(1)),
            Some(&HitTarget::Channel(0))
        );
        assert_eq!(
            map.hit(
                panes.timeline.x.saturating_add(1),
                panes.timeline.y.saturating_add(1)
            ),
            Some(&HitTarget::TimelineMessage("event-1".into()))
        );

        app.mode = Mode::Insert;
        app.mention_picker = Some(MentionPicker::new(
            0..1,
            String::new(),
            vec![MentionCandidate {
                pubkey: "b".repeat(64),
                label: "member".into(),
            }],
        ));
        terminal.draw(|frame| app.render(frame)).unwrap();
        let popup = super::bottom_popup(terminal.size().unwrap().into(), 5);
        let mention = Rect::new(popup.x, popup.y.saturating_sub(3), popup.width, 3);
        assert_eq!(
            app.last_hit_map
                .as_ref()
                .unwrap()
                .hit(mention.x.saturating_add(1), mention.y.saturating_add(1)),
            Some(&HitTarget::MentionCandidate(0))
        );
    }

    #[tokio::test]
    async fn empty_state_renders_help_overlay() {
        let temporary = TempDir::new().unwrap();
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
        app.mode = Mode::Help;

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("bzz keys"));
        assert!(text.contains("Inbox"));
    }
}
