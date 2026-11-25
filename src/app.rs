use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
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
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

mod manage;
mod palette;
mod pane;
#[cfg(feature = "apps")]
mod share;
// v0.17 C3 to C7: the help, the `:` line, the finder, the leader panel
// and two keys (task `find`).
#[cfg(test)]
mod find_tests;

use crate::clock::WallClock as _;
use crate::{
    agents::typing::{AgentTypingState, TypingScope},
    auth::{IdentityManager, read_passphrase, signer::SignerHandle},
    config::{ClipboardImportMode, ClipboardMode, Config, KeyBackend, validate_relay_url},
    diagnostics::{DiagnosticEvent, DiagnosticHandle, ErrorClass},
    domain::{
        Channel, ConnectionState, InboxCategory, InboxItem, Message, Profile, Reaction,
        SearchResultKind, SystemEventKind, ThreadSummary,
    },
    error::{Error, Result},
    media::{
        client::MediaClient,
        clipboard::{
            ClipboardContents, ClipboardReader, NativeClipboard, encode_clipboard_png,
            sanitize_pasted_text,
        },
        file_picker::{FilePicker, FilePickerOutcome, NativeFilePicker},
        runtime::MediaRuntime,
    },
    paths::Paths,
    protocol::{http::HttpClient, system, thread_summary::LiveThreadSummary},
    realtime::{
        admission::SubscriptionPriority,
        session::SessionEvent,
        subscriptions,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    render::sanitize,
    service::{
        channels::ChannelService, dms::DmService, inbox::InboxService, messages::MessageService,
        profiles::ProfileService, read_state::ReadStateService, search::SearchService,
    },
    store::{models::DraftSubmission, writer::StoreHandle},
    sync::{outbox, read_state},
    ui::{
        action::{
            InboxEffect, InboxWorkspaceState, ViewportScroll, WorkspaceEffect, WorkspaceState,
            reduce_inbox, reduce_workspace,
        },
        actions::{ActionContext, ActionMenu, derive as derive_actions},
        agents::AgentDirectoryState,
        composer::Composer,
        dm_picker::DmPickerState,
        hit_map::{HitMap, HitTarget},
        inbox::InboxState,
        input::{InputContext, InputDispatch, InputOwner, InputRouter},
        keymap::{KeyAction, KeyMap, KeyScope, UiAction, map_insert},
        layout,
        mention_picker::MentionPicker,
        redraw_gate::RedrawGate,
        search::SearchState,
        state::{
            AttachmentPrompt, ComposerTarget, ConfirmationKind, FocusSurface, Overlay,
            PresentationState, Route, ViewportState,
        },
        terminal::{TerminalGuard, Tui, copy_osc52},
        theme::{self, BorderSurface, HighlightGroup, Theme, ThemeScope},
        theme_picker::ThemePicker,
        timeline::{self, TimelineState},
    },
};

struct Runtime {
    community_id: Uuid,
    identity_id: Uuid,
    signer: SignerHandle,
    supervisor: SupervisorHandle,
    events: broadcast::Receiver<SupervisorEvent>,
    http: HttpClient,
    channels: ChannelService,
    artifacts: crate::service::artifacts::ArtifactService,
    dms: DmService,
    inbox: InboxService,
    profiles: ProfileService,
    search: SearchService,
    messages: MessageService,
    media: MediaClient,
    read_state: ReadStateService,
    /// NIP-FI evidence shared by the supervisor, HTTP and media clients.
    evidence: crate::auth::federated::EvidenceSlot,
    /// The community's NIP-FA adapter origin, when configured.
    identity_adapter: Option<url::Url>,
    data_dir: std::path::PathBuf,
    /// The records of the community's apps (v0.16).
    #[cfg(feature = "apps")]
    records: crate::service::records::RecordsService,
}

impl Runtime {
    fn build(
        config: &Config,
        index: usize,
        paths: &Paths,
        store: StoreHandle,
        signer: Option<SignerHandle>,
        diagnostics: DiagnosticHandle,
    ) -> Result<Self> {
        Self::build_at(config, index, paths, store, signer, diagnostics, None)
    }

    /// As [`Self::build`], with the relay reached under `relay_path`
    /// (`bzz demo`'s token) instead of the origin's root.
    fn build_at(
        config: &Config,
        index: usize,
        paths: &Paths,
        store: StoreHandle,
        signer: Option<SignerHandle>,
        diagnostics: DiagnosticHandle,
        relay_path: Option<&str>,
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
            IdentityManager::new(paths).signer(identity, passphrase.as_ref())?
        };
        let mut endpoint =
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
        if let Some(path) = relay_path {
            endpoint.websocket.set_path(path);
            endpoint.http_base.set_path(path);
        }
        let evidence = crate::auth::federated::EvidenceSlot::default();
        if identity.backend == KeyBackend::BuzzDesktop {
            // Buzz Desktop owns the enterprise login; it supplies evidence.
            evidence.set(crate::auth::provider::ProviderEvidence::new(
                std::sync::Arc::new(crate::auth::desktop_client(identity)?),
                crate::clock::system(),
            ));
        }
        let identity_adapter = community
            .identity_adapter
            .as_deref()
            .map(url::Url::parse)
            .transpose()
            .map_err(|_| Error::Config("identity_adapter is not a URL".into()))?;
        let supervisor = SupervisorHandle::spawn_with_evidence(
            endpoint.websocket,
            signer.clone(),
            diagnostics.clone(),
            evidence.clone(),
        );
        let events = supervisor.subscribe_events();
        let http = HttpClient::new(endpoint.http_base.clone(), signer.clone())?
            .with_evidence(evidence.clone());
        let media = MediaClient::new(
            endpoint.http_base,
            endpoint.authority,
            signer.clone(),
            config.media.download_concurrency,
        )?
        .with_evidence(evidence.clone());
        Ok(Self {
            community_id: community.id,
            identity_id: identity.id,
            signer: signer.clone(),
            supervisor: supervisor.clone(),
            events,
            http: http.clone(),
            channels: ChannelService::new(community.id, http.clone(), store.clone()),
            artifacts: crate::service::artifacts::ArtifactService::new(
                community.id,
                http.clone(),
                store.clone(),
                signer.clone(),
                supervisor.clone(),
            ),
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
            #[cfg(feature = "apps")]
            records: crate::service::records::RecordsService::new(
                community.id,
                http.clone(),
                store.clone(),
                crate::service::records::Writer::new(signer.clone(), supervisor.clone()),
            ),
            read_state: ReadStateService::new(community.id, signer, store, supervisor),
            evidence,
            identity_adapter,
            data_dir: paths.data_dir.clone(),
        })
    }
}

/// One `:section` change, applied to the relay's current head.
type SectionEdit = Box<
    dyn FnOnce(
            &mut crate::protocol::sidebar_doc::Edit<'_>,
            &crate::protocol::sidebar_doc::Tree,
        ) -> Result<()>
        + Send,
>;

/// Recency of explicit mention choices (mention rules v3, section 4, rule
/// 8.1): in memory, for 100 channels and 100 keys each, as desktop keeps it.
#[derive(Debug, Default)]
struct MentionHistory {
    clock: u64,
    channels: std::collections::VecDeque<(Uuid, HashMap<String, u64>)>,
}

impl MentionHistory {
    const CHANNELS: usize = 100;
    const KEYS: usize = 100;

    fn get(&self, channel: Uuid) -> HashMap<String, u64> {
        self.channels
            .iter()
            .find(|(id, _)| *id == channel)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default()
    }

    fn record(&mut self, channel: Uuid, pubkey: &str) {
        self.clock += 1;
        let mut keys = self
            .channels
            .iter()
            .position(|(id, _)| *id == channel)
            .and_then(|index| self.channels.remove(index))
            .map_or_else(HashMap::new, |(_, keys)| keys);
        keys.insert(pubkey.to_owned(), self.clock);
        if keys.len() > Self::KEYS
            && let Some(oldest) = keys
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(key, _)| key.clone())
        {
            keys.remove(&oldest);
        }
        self.channels.push_front((channel, keys));
        self.channels.truncate(Self::CHANNELS);
    }
}

/// An own message open in the composer for editing. Its text never enters
/// the draft store; attachments are kept as the relay last saw them.
#[derive(Clone, Debug, Eq, PartialEq)]
struct EditingMessage {
    channel_id: Uuid,
    event_id: String,
    kept_attachments: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StagingAttachment {
    id: String,
    filename: String,
}

#[derive(Debug)]
enum AttachmentBackground {
    FilesPicked {
        target: ComposerTarget,
        outcome: FilePickerOutcome,
    },
    ClipboardImported {
        target: ComposerTarget,
        contents: Box<ClipboardContents>,
    },
    Staged {
        target: ComposerTarget,
        community: Uuid,
        pending: crate::media::PendingAttachment,
    },
    StageFailed {
        target: ComposerTarget,
        attachment_id: String,
        message: String,
    },
    Uploaded {
        target: ComposerTarget,
        community: Uuid,
        attachment_id: String,
        attachment: Box<crate::media::Attachment>,
    },
    UploadFailed {
        target: ComposerTarget,
        community: Uuid,
        pending: crate::media::PendingAttachment,
        message: String,
    },
}

#[derive(Debug)]
enum Background {
    Changed,
    /// An app's session has something for the loop (v0.16). One at a time:
    /// the session's waker sends no other until this one is taken.
    #[cfg_attr(not(feature = "apps"), allow(dead_code))]
    AppWake,
    /// What came of the offers of apps in the channel shown (v0.16 M6).
    #[cfg(feature = "apps")]
    AppOffers(crate::service::offers::OfferNews),
    /// A federated sign-in finished; evidence is installed, reconnect now.
    FederatedReady,
    /// A fixed status notice (federated identity, reports, send to channel).
    Notice(String),
    /// Selected-channel history landed; relay-signed 39005 summaries from
    /// channel windows refine the live thread badges.
    HistoryLoaded {
        community: Uuid,
        summaries: Vec<nostr::Event>,
    },
    /// The selected channel's replayed huddle state.
    HuddleState {
        community: Uuid,
        channel: Uuid,
        active: Option<crate::protocol::huddle::ActiveHuddle>,
    },
    /// A huddle this viewer started is ready to join.
    HuddleStarted {
        parent: Uuid,
        backing: Uuid,
    },
    #[cfg(feature = "huddles-audio")]
    HuddleJoined(std::result::Result<Box<HuddleCall>, String>),
    /// NIP-11 artifact capabilities for `community`.
    ArtifactCaps {
        community: Uuid,
        caps: crate::protocol::artifact::Capabilities,
    },
    /// An artifact edit or assigned-task refresh finished.
    ArtifactDone(std::result::Result<String, String>),
    /// A verified thread window page landed for `root`.
    ThreadWindow {
        community: Uuid,
        root: String,
        next: Option<crate::protocol::window::Cursor>,
        supported: bool,
    },
    /// NIP-CL public membership lookup answer for a profile card.
    PublicMemberships {
        community: Uuid,
        subject: String,
        generation: u64,
        result: std::result::Result<crate::protocol::membership_public::Lookup, String>,
    },
    /// Remote NIP-CE prefix search results for the reaction picker.
    EmojiSearch {
        query: String,
        found: Vec<crate::protocol::emoji::Emoji>,
    },
    /// Authoritative relay presence for the people asked about.
    PresenceSnapshot {
        community: Uuid,
        asked: Vec<String>,
        observations: Vec<crate::protocol::presence::Observation>,
    },
    DraftAcknowledged,
    Failed(String),
    Saved,
    InboxLoaded {
        community: Uuid,
        items: Vec<InboxItem>,
    },
    InboxDetailLoaded {
        community: Uuid,
        conversation_id: String,
        messages: Vec<Message>,
    },
    SearchLoaded {
        community: Uuid,
        generation: u64,
        output: crate::service::search::SearchOutput,
    },
    /// An older page of message results for the current search.
    SearchOlder {
        community: Uuid,
        generation: u64,
        output: Result<crate::service::search::SearchOutput>,
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
    AgentsLoaded {
        community: Uuid,
        agents: Vec<crate::store::agents::RemoteAgentView>,
        notice: Option<String>,
    },
    /// How each local agent's last turn ended, for the agents panel.
    LocalAgentTurns {
        community: Uuid,
        turns: Vec<(Uuid, Option<crate::store::agent_runtime::RecentTurn>)>,
    },
    /// The checks before a creation finished (`:agent new`): the lines for
    /// the owner, or why nothing can be created.
    AgentPreflight {
        new: Box<crate::agents::manage::NewAgent>,
        result: std::result::Result<Vec<String>, String>,
    },
}

pub struct App {
    config: Config,
    diagnostics: DiagnosticHandle,
    paths: Paths,
    store: StoreHandle,
    runtime: Option<Runtime>,
    media: MediaRuntime,
    selected_community: usize,
    channels: Vec<Channel>,
    selected_channel: usize,
    showing_open_channel: bool,
    messages: Vec<Message>,
    local_thread_summaries: HashMap<String, ThreadSummary>,
    live_thread_summaries: HashMap<String, LiveThreadSummary>,
    thread_summaries: HashMap<String, ThreadSummary>,
    profiles: HashMap<String, Profile>,
    agents: HashMap<String, crate::store::agents::RemoteAgentView>,
    agent_typing: AgentTypingState,
    typing_spinner_frame: usize,
    typing_spinner_advanced_at: Instant,
    typing_subscription_channel: Option<Uuid>,
    closed_typing_channel: Option<Uuid>,
    reactions: HashMap<String, Vec<Reaction>>,
    show_system_channels: bool,
    /// The user's synced sidebar sections and sort modes.
    sidebar_layout: crate::domain::SidebarLayout,
    /// `(reason, note)` awaiting the report confirmation.
    /// `(reason, note, author only)` awaiting the report confirmation.
    pending_report: Option<(String, Option<String>, bool)>,
    presence: crate::protocol::presence::PresenceBook,
    /// NIP-AR: capabilities per community and the selected channel's sync.
    artifact_caps: Option<(Uuid, crate::protocol::artifact::Capabilities)>,
    broker: Option<BrokerRuntime>,
    /// Local agents (v0.15): bzz runs their processes while the owner's
    /// runtime is unlocked, beside the broker.
    local_agents: Option<crate::agents::runtime::service::CommunityAgents>,
    local_agents_attempted: Option<Uuid>,
    /// The owner paused or resumed a local agent in the panel.
    local_agents_dirty: bool,
    /// Huddles: the selected channel's replayed state and your call.
    huddle_view: Option<(Uuid, Option<crate::protocol::huddle::ActiveHuddle>)>,
    huddle_loading: bool,
    huddle_dirty: bool,
    huddle_subscribed: Option<Uuid>,
    huddle_backing_subscribed: Option<Uuid>,
    huddle_reactions: std::collections::VecDeque<(String, String)>,
    huddle_guidelines: Option<String>,
    huddle_devices: Option<(Vec<String>, Vec<String>)>,
    huddle_joining: bool,
    huddle_ptt_until: Option<Instant>,
    #[cfg(feature = "huddles-audio")]
    huddle_call: Option<HuddleCall>,
    /// The visualizer (plan v0.17.0 B): levels read each frame, drawn and
    /// forgotten; cleared when the call stops.
    huddle_viz: HuddleViz,
    broker_attempted: Option<Uuid>,
    artifact_caps_requested: Option<Uuid>,
    artifact_sync: Option<ArtifactSync>,
    artifact_panel: crate::ui::artifacts::ArtifactPanelState,
    /// `(artifact, destination)` awaiting the move confirmation.
    pending_artifact_move: Option<(Uuid, Uuid)>,
    /// NIP-CW thread window state for the open thread: `(root, next page)`.
    thread_window: Option<(String, Option<crate::protocol::window::Cursor>)>,
    thread_window_loading: bool,
    /// Open profile card and the generation counter for its lookups.
    profile_card: Option<crate::ui::profile::ProfileCard>,
    /// The `:broker` panel's selected agent.
    broker_selected: usize,
    /// Presence of the other person in each one-to-one DM, for the sidebar.
    dm_presence: HashMap<Uuid, crate::protocol::presence::Presence>,
    profile_generation: u64,
    /// Verified public lists keyed by (community, subject); display data
    /// only, in memory, never authority.
    public_lookup_cache:
        HashMap<(Uuid, String), Vec<crate::protocol::membership_public::PublicMembership>>,
    /// Per community: no lookups before this unix second; `true` when the
    /// relay rate-limited (429), `false` when it does not offer the lookup.
    public_lookup_backoff: HashMap<Uuid, (u64, bool)>,
    /// Shortcode -> image URL from 30030 sets and 30180 entries.
    emoji_palette: HashMap<String, String>,
    /// Shortcodes already looked up remotely (30180 `#d`), bounded.
    emoji_lookups: HashSet<String>,
    last_presence_snapshot: Instant,
    last_presence_publish: Option<(Instant, crate::protocol::presence::Presence)>,
    last_input_at: Instant,
    profile_requested: HashSet<String>,
    thread_messages: Vec<Message>,
    thread_root: Option<String>,
    timeline: TimelineState,
    thread_timeline: TimelineState,
    composer: Composer,
    /// Revision for the editable draft currently hydrated into `composer`.
    /// Never retain it once the composer is closed or submitted.
    composer_draft_revision: Option<String>,
    /// The composer is editing this own message instead of drafting.
    editing: Option<EditingMessage>,
    mention_picker: Option<MentionPicker>,
    /// The composer channel's complete mention choice set, reloaded when a
    /// new `@` query starts.
    mention_choices: Option<(Uuid, Uuid, crate::domain::MentionChoices)>,
    /// Explicit mention selections per channel, most recent highest.
    mention_history: MentionHistory,
    finder: crate::ui::finder::FinderState,
    /// Where the help is scrolled to (v0.17 C3).
    help_scroll: crate::ui::help::HelpScroll,
    /// What the last `:` command answered when it was mistyped or missing
    /// its arguments: shown, wrapped, under the prompt it keeps open
    /// (v0.17 C4).
    command_answer: Option<String>,
    reaction_index: usize,
    /// `Some(query)` while the reaction picker searches custom emoji.
    reaction_query: Option<String>,
    reaction_candidates: Vec<crate::protocol::emoji::Emoji>,
    reaction_candidate: usize,
    command: String,
    attachment_input: String,
    inbox_items: Vec<InboxItem>,
    inbox_messages: Vec<Message>,
    inbox_state: InboxState,
    inbox_loading: bool,
    inbox_detail_loading: bool,
    inbox_task: Option<tokio::task::JoinHandle<()>>,
    inbox_detail_task: Option<tokio::task::JoinHandle<()>>,
    pending_inbox_read: Vec<InboxItem>,
    search_state: SearchState,
    search_dirty_since: Option<Instant>,
    search_task: Option<tokio::task::JoinHandle<()>>,
    dm_picker: DmPickerState,
    dm_dirty_since: Option<Instant>,
    dm_search_task: Option<tokio::task::JoinHandle<()>>,
    agent_directory: AgentDirectoryState,
    agent_directory_task: Option<tokio::task::JoinHandle<()>>,
    /// `:agents`: the panel of local agents, and what it last read.
    local_agent_panel: crate::ui::local_agents::PanelState,
    local_agent_turns: HashMap<Uuid, crate::store::agent_runtime::RecentTurn>,
    local_agent_refreshed: Option<Instant>,
    /// `:agent new`: the form, while it is open.
    agent_form: Option<crate::ui::local_agents::NewAgentForm>,
    /// The real commands that wait for the owner's answer (v0.15 M7), as
    /// the agent runtime last listed them; read again every tick.
    approvals: Vec<crate::agents::runtime::approval::Asked>,
    /// The dialog that shows one of them.
    approval: crate::ui::approval::ApprovalState,
    /// What was open before that dialog, shown again when it closes.
    approval_return: Option<Overlay>,
    /// The seconds left that the line at the top last showed.
    approval_clock: Option<u64>,
    /// How bzz's own environment is read to find Node for an agent's host.
    agent_environment: crate::agents::runtime::agent::Environment,
    /// Tests read which agent the panel asked to add to which channel.
    #[cfg(test)]
    agent_additions: Vec<(Uuid, Uuid)>,
    /// Tests publish an agent's records to a recorded relay.
    #[cfg(test)]
    agent_publisher: Option<std::sync::Arc<dyn crate::agents::manage::Publisher>>,
    preview_index: usize,
    preview_revealed: bool,
    clipboard: Arc<dyn ClipboardReader>,
    file_picker: Arc<dyn FilePicker>,
    staging_attachments: Vec<StagingAttachment>,
    staging_media: HashSet<String>,
    uploading_media: HashSet<String>,
    community_rail: bool,
    sidebar: bool,
    community_viewport: ViewportState,
    channel_viewport: ViewportState,
    theme: Theme,
    theme_picker: Option<ThemePicker>,
    theme_before_preview: Option<Theme>,
    action_menu: Option<ActionMenu>,
    /// The palette while it is open (v0.17 C1).
    palette: Option<crate::ui::palette::Palette>,
    /// Actions run from the palette in this session, newest first.
    palette_recent: Vec<&'static str>,
    keymap: KeyMap,
    input_router: InputRouter,
    /// When the pending key sequence started, on Tokio's clock so a test
    /// can move it: its panel appears 300 ms later (v0.17 C6).
    leader_started_at: Option<tokio::time::Instant>,
    presentation: PresentationState,
    community_cursor: usize,
    connection: ConnectionState,
    status_error: Option<String>,
    rate_limit_until: Option<Instant>,
    rate_limit_display_seconds: Option<u64>,
    should_quit: bool,
    cache_dirty: bool,
    manual_unread: HashSet<Uuid>,
    computed_unread: HashSet<Uuid>,
    subscribed_channels: HashSet<Uuid>,
    closed_channel_subscriptions: HashSet<String>,
    handled_relay_events: HashMap<Uuid, HashSet<String>>,
    handled_relay_event_order: VecDeque<(Uuid, String)>,
    last_marked: HashMap<String, u32>,
    read_dirty_since: Option<Instant>,
    last_cache_refresh: Instant,
    directory_task: Option<tokio::task::JoinHandle<()>>,
    last_inbox_refresh: Instant,
    background_tx: mpsc::Sender<Background>,
    background_rx: mpsc::Receiver<Background>,
    attachment_tx: mpsc::Sender<AttachmentBackground>,
    attachment_rx: mpsc::Receiver<AttachmentBackground>,
    render_generation: u64,
    last_hit_map: Option<HitMap>,
    last_primary_click: Option<(HitTarget, Instant)>,
    /// The apps open in the community's channels, and their pane (v0.16).
    apps: pane::AppPanes,
    /// The last layout drew the open thread. A reply goes into a thread
    /// only while it is drawn.
    thread_drawn: bool,
    /// `bzz demo` (v0.17): a generated world on a loopback relay. The
    /// status line says so, and what needs a real community says how to
    /// join one (`crate::demo`).
    demo: bool,
    /// The first screen, while no community exists (v0.17 A4).
    first_screen: crate::ui::first_screen::FirstScreen,
    /// `d` on the first screen: leave, and open the demo in this process.
    open_demo: bool,
    /// When the transient notice (`status_error`) expires (C2).
    notice_timer: crate::ui::status::NoticeTimer,
    /// The channel in view and the read time it had when it was opened:
    /// the `── new ──` line goes before its first later message (L6).
    new_since: Option<(Uuid, u64)>,
    /// Unread messages per channel, for the count in the sidebar (L6).
    unread_counts: HashMap<Uuid, usize>,
}

/// How [`App::run_to_exit`] ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exit {
    Quit,
    /// The person chose the demo on the first screen.
    OpenDemo,
}

const LEADER_TIMEOUT: Duration = Duration::from_millis(750);
const TERMINAL_ERROR_YIELD: Duration = Duration::from_millis(50);
const RELAY_LAG_YIELD: Duration = Duration::from_millis(5);
const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const AGENT_SPINNER_INTERVAL: Duration = Duration::from_millis(200);
const RELAY_EVENT_DEDUP_CAPACITY: usize = 16_384;
const AGENT_TYPING_SUBSCRIPTION: &str = "agent-typing-selected";
const ARTIFACT_SYNC_SUBSCRIPTION: &str = "artifact-sync-selected";
/// The panel's note when the channel holds more live artifacts, of all
/// types together, than the relay returns in one sync.
const ARTIFACT_SYNC_OVERFLOW_NOTE: &str = "too many artifacts here to sync · list may be stale";
const HUDDLE_LIFECYCLE_SUBSCRIPTION: &str = "huddle-lifecycle-selected";
const HUDDLE_BACKING_SUBSCRIPTION: &str = "huddle-backing-selected";

/// The local agent broker, served only while the owner's runtime is unlocked.
struct BrokerRuntime {
    community: Uuid,
    host: crate::broker::host::Host,
    server: crate::broker::http::BrokerServer,
    signers: Vec<SignerHandle>,
    supervisors: Vec<SupervisorHandle>,
}

impl BrokerRuntime {
    async fn stop(self) {
        for supervisor in &self.supervisors {
            supervisor.shutdown().await;
        }
        for signer in &self.signers {
            signer.lock().await;
        }
    }
}

/// What the huddle visualizer keeps between frames.
struct HuddleViz {
    meter: crate::huddle::meter::Meter,
    pacer: crate::huddle::meter::Pacer,
    epoch: Instant,
    look: crate::ui::huddle_lanes::Look,
    /// The other people of the call, named, in roster order.
    people: Vec<crate::ui::huddle_lanes::Person>,
    muted: bool,
    on: bool,
    /// A generated huddle (the demo's, and tests'): levels with no sound
    /// device, relay or voice behind them.
    generated: Option<GeneratedHuddle>,
    /// The zoomed view is on (plan v0.17.0 B5); it shows while the panel is
    /// open and a call is on.
    zoomed: bool,
    /// The style picker's selected row, while it is open. The style it
    /// selects is drawn live; Esc puts the saved one back.
    picker: Option<usize>,
    /// The call's spectrum, analysed only while bars or the orb are seen.
    spectrum: crate::huddle::spectrum::Spectrum,
    /// Steps the orb's rate down when writing to the terminal is slow.
    throttle: crate::huddle::meter::Throttle,
    /// The microphone check of the join confirmation (plan B7).
    mic_check: Option<MicCheckRun>,
    mic_pacer: crate::huddle::meter::Pacer,
    /// After a check, the join waits for its devices to be let go.
    mic_released: Option<tokio::sync::oneshot::Receiver<()>>,
    /// With motion off: who spoke when the spectrum was last read.
    still: Option<(Vec<u8>, bool)>,
}

/// A microphone check in progress: the check, and the device task that
/// feeds it (none in tests, which feed it themselves).
struct MicCheckRun {
    check: crate::huddle::mic_check::MicCheck,
    /// Dropping it stops the device task.
    _stop: Option<tokio::sync::oneshot::Sender<()>>,
    failed: Option<tokio::sync::oneshot::Receiver<String>>,
    /// Fires once the device task let the devices go.
    released: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// Generated levels and the names they speak as.
struct GeneratedHuddle {
    source: crate::huddle::synth::Synthetic,
    /// Relay-style index and public key of each generated person.
    people: Vec<(u8, String)>,
    muted: bool,
}

impl Default for HuddleViz {
    fn default() -> Self {
        Self {
            meter: crate::huddle::meter::Meter::default(),
            pacer: crate::huddle::meter::Pacer::default(),
            epoch: Instant::now(),
            look: crate::ui::huddle_lanes::Look::from_env(),
            people: Vec::new(),
            muted: true,
            on: false,
            generated: None,
            zoomed: false,
            picker: None,
            spectrum: crate::huddle::spectrum::Spectrum::default(),
            throttle: crate::huddle::meter::Throttle::default(),
            mic_check: None,
            mic_pacer: crate::huddle::meter::Pacer::default(),
            mic_released: None,
            still: None,
        }
    }
}

impl std::fmt::Debug for HuddleViz {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HuddleViz { .. }")
    }
}

/// What feeds the visualizer: a source of levels and the people it names.
struct HuddleFeed {
    levels: std::sync::Arc<crate::huddle::levels::Levels>,
    /// Relay index and public key of everyone else in the call.
    people: Vec<(u8, String)>,
    muted: bool,
}

/// A joined huddle call: audio engine plus the devices it owns. Dropping it
/// leaves the room and releases both devices.
#[cfg(feature = "huddles-audio")]
struct HuddleCall {
    call: crate::huddle::audio::AudioCall,
    _devices: crate::huddle::audio::Devices,
    backing: Uuid,
    parent: Uuid,
}

#[cfg(feature = "huddles-audio")]
impl std::fmt::Debug for HuddleCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never device names or audio state beyond the room identity.
        formatter
            .debug_struct("HuddleCall")
            .field("backing", &self.backing)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "huddles-audio")]
impl HuddleCall {
    async fn stop(self) {
        self.call.stop().await;
    }
}

/// The selected channel's NIP-AR sync. Heads before EOSE are buffered and
/// replace the stored set only at EOSE; a drop before EOSE discards them.
#[derive(Debug)]
struct ArtifactSync {
    community: Uuid,
    channel: Uuid,
    buffer: Vec<crate::protocol::artifact::Artifact>,
    complete: bool,
    failures: u32,
    retry_at: Option<Instant>,
    /// `restricted`: this reader cannot read the channel's artifacts.
    blocked: bool,
    /// `error: sync set exceeds max_events`: the channel holds more live
    /// artifacts than one sync returns. Asking again gives the same answer
    /// and downloads the relay's whole bound each time, so bzz does not.
    overflowed: bool,
}

impl ArtifactSync {
    const fn new(community: Uuid, channel: Uuid) -> Self {
        Self {
            community,
            channel,
            buffer: Vec::new(),
            complete: false,
            failures: 0,
            retry_at: None,
            blocked: false,
            overflowed: false,
        }
    }

    /// The relay cannot give this channel's set in one sync. The last
    /// complete set stays; nothing is scheduled.
    fn overflow(&mut self) {
        self.buffer.clear();
        self.complete = false;
        self.retry_at = None;
        self.overflowed = true;
    }

    /// Exponential backoff with jitter before the next sync attempt.
    fn schedule_retry(&mut self) {
        self.failures = self.failures.saturating_add(1);
        let base = 2_u64.saturating_pow(self.failures.min(6)).min(60);
        let jitter = u64::from(rand::random::<u16>()) % (base * 500 + 1);
        self.buffer.clear();
        self.complete = false;
        self.retry_at = Some(Instant::now() + Duration::from_millis(base * 1000 + jitter));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypingSubscriptionTransition {
    Unchanged,
    Leave,
    Subscribe(Uuid),
}

fn typing_subscription_transition(
    current: Option<Uuid>,
    desired: Option<Uuid>,
) -> TypingSubscriptionTransition {
    match (current, desired) {
        (current, desired) if current == desired => TypingSubscriptionTransition::Unchanged,
        (Some(_), None) => TypingSubscriptionTransition::Leave,
        (_, Some(channel)) => TypingSubscriptionTransition::Subscribe(channel),
        (None, None) => TypingSubscriptionTransition::Unchanged,
    }
}

fn publish_presence(runtime: &Runtime, presence: crate::protocol::presence::Presence) {
    let signer = runtime.signer.clone();
    let supervisor = runtime.supervisor.clone();
    tokio::spawn(async move {
        if let Ok(builder) = buzz_sdk::build_presence_update(presence.as_str())
            && let Ok(event) = signer.sign(builder).await
        {
            let _ = supervisor.publish_maintenance(event).await;
        }
    });
}

/// Kind 20003: the owner states its hosted agents' presence (`p` per agent).
pub fn agent_presence_builder(
    presence: crate::protocol::presence::Presence,
    agents: &[String],
) -> Result<nostr::EventBuilder> {
    let tags = agents
        .iter()
        .take(256)
        .map(|agent| nostr::Tag::parse(["p", agent.as_str()]))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| Error::Protocol(error.to_string()))?;
    Ok(nostr::EventBuilder::new(
        nostr::Kind::Custom(crate::protocol::presence::KIND_AGENT_PRESENCE),
        presence.as_str(),
    )
    .tags(tags))
}

fn publish_agent_presence(
    runtime: &Runtime,
    presence: crate::protocol::presence::Presence,
    agents: Vec<String>,
) {
    let signer = runtime.signer.clone();
    let supervisor = runtime.supervisor.clone();
    tokio::spawn(async move {
        if let Ok(builder) = agent_presence_builder(presence, &agents)
            && let Ok(event) = signer.sign(builder).await
        {
            let _ = supervisor.publish_maintenance(event).await;
        }
    });
}

/// Publish `offline` and wait briefly for the relay, so lock and quit clear
/// presence immediately instead of after the 180 s TTL.
async fn publish_offline(runtime: &Runtime) {
    let attempt = async {
        let builder = buzz_sdk::build_presence_update("offline").ok()?;
        let event = runtime.signer.sign(builder).await.ok()?;
        runtime.supervisor.publish_maintenance(event).await.ok()
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), attempt).await;
}

/// Best-effort: open `url` in the user's browser. The caller always shows the
/// URL too, so a headless or remote terminal can copy it.
fn open_in_browser(url: &url::Url) {
    let (program, arguments): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url.as_str()])
    } else if cfg!(windows) {
        (
            "rundll32",
            vec!["url.dll,FileProtocolHandler", url.as_str()],
        )
    } else {
        ("xdg-open", vec![url.as_str()])
    };
    let _ = std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// M10.2: "send to channel" needs a local thread root in the same channel.
fn send_to_channel_root(
    root: Option<Message>,
    channel: Uuid,
) -> std::result::Result<Message, &'static str> {
    let root = root.ok_or("the thread root is not available locally")?;
    if root.channel_id != channel {
        return Err("the thread root belongs to another channel");
    }
    if root.root_event_id.is_some() || root.system.is_some() || root.deleted {
        return Err("only a thread root's thread can be sent to the channel");
    }
    Ok(root)
}

fn triggers_directory_refresh(kind: u16, content: &str) -> bool {
    matches!(kind, 39_002 | 44_100 | 44_101)
        || (kind == 40_099
            && matches!(
                system::parse(content).kind,
                SystemEventKind::MemberJoined
                    | SystemEventKind::MemberLeft
                    | SystemEventKind::MemberRemoved
                    | SystemEventKind::AdminKick
                    | SystemEventKind::VisibilityChanged
                    | SystemEventKind::JoinRoleChanged
                    | SystemEventKind::UnlistedChanged
                    | SystemEventKind::ChannelDeleted
            ))
}

impl App {
    pub async fn new(config: Config, paths: Paths, store: StoreHandle) -> Result<Self> {
        Self::new_with_diagnostics(config, paths, store, DiagnosticHandle::disabled()).await
    }

    pub async fn new_with_diagnostics(
        config: Config,
        paths: Paths,
        store: StoreHandle,
        diagnostics: DiagnosticHandle,
    ) -> Result<Self> {
        Self::new_with(config, paths, store, diagnostics, None).await
    }

    /// `bzz demo` (`crate::demo`): the generated viewer's key is handed in
    /// memory, nothing is unlocked or asked for, the relay is reached under
    /// `relay_path`, and `#team` (`opening`) is open on the first frame.
    pub async fn new_demo(
        config: Config,
        paths: Paths,
        store: StoreHandle,
        signer: SignerHandle,
        relay_path: &str,
        opening: Uuid,
    ) -> Result<Self> {
        let mut app = Self::new_with(
            config,
            paths,
            store,
            DiagnosticHandle::disabled(),
            Some((signer, relay_path)),
        )
        .await?;
        app.demo = true;
        let (voices, people) = crate::demo::huddle();
        app.set_generated_huddle(voices, people, true);
        // One community: its rail only takes room from the conversation.
        app.community_rail = false;
        if let Some(index) = app
            .channels
            .iter()
            .position(|channel| channel.id == opening)
        {
            app.select_channel_index(index);
            app.hydrate_cache().await?;
        }
        Ok(app)
    }

    async fn new_with(
        config: Config,
        paths: Paths,
        store: StoreHandle,
        diagnostics: DiagnosticHandle,
        demo: Option<(SignerHandle, &str)>,
    ) -> Result<Self> {
        diagnostics.emit(DiagnosticEvent::ClientStarted {
            version: env!("CARGO_PKG_VERSION").into(),
            os: crate::diagnostics::event::normalized_os().into(),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .into(),
        });
        let selected_community = config
            .default_community
            .and_then(|id| config.communities.iter().position(|entry| entry.id == id))
            .unwrap_or_default();
        let (theme, theme_notice) = load_theme_safe(&config, selected_community, &paths);
        let keymap = KeyMap::load(&paths)?;
        let (runtime, connection, status_error) = if config.communities.is_empty() {
            (None, ConnectionState::Offline, None)
        } else {
            let (signer, relay_path) = demo.unzip();
            match Runtime::build_at(
                &config,
                selected_community,
                &paths,
                store.clone(),
                signer,
                diagnostics.clone(),
                relay_path,
            ) {
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
        // Attachment completions have a dedicated bounded lane so relay/cache
        // activity cannot leave a local file permanently shown as processing.
        let (attachment_tx, attachment_rx) = mpsc::channel(32);
        let mut media = MediaRuntime::new(
            config.media.clone(),
            config.ui.profile_avatars,
            &paths,
            store.clone(),
        );
        if let Some(active) = &runtime {
            media.bind(
                active.community_id,
                active.identity_id,
                active.media.clone(),
            );
        } else if let Some(community) = config.communities.get(selected_community) {
            media.select_cached(community.id, community.identity_id);
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
            diagnostics,
            paths,
            store,
            runtime,
            media,
            selected_community,
            channels: Vec::new(),
            selected_channel: 0,
            showing_open_channel: false,
            messages: Vec::new(),
            local_thread_summaries: HashMap::new(),
            live_thread_summaries: HashMap::new(),
            thread_summaries: HashMap::new(),
            profiles: HashMap::new(),
            agents: HashMap::new(),
            agent_typing: AgentTypingState::default(),
            typing_spinner_frame: 0,
            typing_spinner_advanced_at: Instant::now(),
            typing_subscription_channel: None,
            closed_typing_channel: None,
            reactions: HashMap::new(),
            show_system_channels: false,
            sidebar_layout: crate::domain::SidebarLayout::default(),
            pending_report: None,
            presence: crate::protocol::presence::PresenceBook::default(),
            emoji_palette: HashMap::new(),
            thread_window: None,
            thread_window_loading: false,
            profile_card: None,
            broker_selected: 0,
            dm_presence: HashMap::new(),
            profile_generation: 0,
            public_lookup_cache: HashMap::new(),
            public_lookup_backoff: HashMap::new(),
            artifact_caps: None,
            broker: None,
            huddle_view: None,
            huddle_loading: false,
            huddle_dirty: false,
            huddle_subscribed: None,
            huddle_backing_subscribed: None,
            huddle_reactions: std::collections::VecDeque::new(),
            huddle_guidelines: None,
            huddle_devices: None,
            huddle_joining: false,
            huddle_ptt_until: None,
            #[cfg(feature = "huddles-audio")]
            huddle_call: None,
            huddle_viz: HuddleViz::default(),
            broker_attempted: None,
            local_agents: None,
            local_agents_attempted: None,
            local_agents_dirty: false,
            artifact_caps_requested: None,
            artifact_sync: None,
            artifact_panel: crate::ui::artifacts::ArtifactPanelState::default(),
            pending_artifact_move: None,
            emoji_lookups: HashSet::new(),
            last_presence_snapshot: Instant::now() - Duration::from_secs(3600),
            last_presence_publish: None,
            last_input_at: Instant::now(),
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
            composer: Composer::default(),
            composer_draft_revision: None,
            editing: None,
            mention_picker: None,
            mention_choices: None,
            mention_history: MentionHistory::default(),
            finder: crate::ui::finder::FinderState::default(),
            help_scroll: crate::ui::help::HelpScroll::default(),
            command_answer: None,
            reaction_index: 0,
            reaction_query: None,
            reaction_candidates: Vec::new(),
            reaction_candidate: 0,
            command: String::new(),
            attachment_input: String::new(),
            inbox_items: Vec::new(),
            inbox_messages: Vec::new(),
            inbox_state: InboxState::default(),
            inbox_loading: false,
            inbox_detail_loading: false,
            inbox_task: None,
            inbox_detail_task: None,
            pending_inbox_read: Vec::new(),
            search_state: SearchState::default(),
            search_dirty_since: None,
            search_task: None,
            dm_picker: DmPickerState::default(),
            dm_dirty_since: None,
            dm_search_task: None,
            agent_directory: AgentDirectoryState::default(),
            agent_directory_task: None,
            local_agent_panel: crate::ui::local_agents::PanelState::default(),
            local_agent_turns: HashMap::new(),
            local_agent_refreshed: None,
            approvals: Vec::new(),
            approval: crate::ui::approval::ApprovalState::default(),
            approval_return: None,
            approval_clock: None,
            agent_form: None,
            agent_environment: std::sync::Arc::new(|name| std::env::var(name).ok()),
            #[cfg(test)]
            agent_additions: Vec::new(),
            #[cfg(test)]
            agent_publisher: None,
            preview_index: 0,
            preview_revealed: false,
            clipboard: Arc::new(NativeClipboard),
            file_picker: Arc::new(NativeFilePicker),
            staging_attachments: Vec::new(),
            staging_media: HashSet::new(),
            uploading_media: HashSet::new(),
            community_rail: true,
            sidebar: true,
            community_viewport: ViewportState::default(),
            channel_viewport: ViewportState::default(),
            theme,
            theme_picker: None,
            theme_before_preview: None,
            action_menu: None,
            palette: None,
            palette_recent: Vec::new(),
            keymap,
            input_router: InputRouter::default(),
            leader_started_at: None,
            presentation: PresentationState::default(),
            community_cursor: selected_community,
            connection,
            status_error: (!notices.is_empty()).then(|| notices.join("; ")),
            rate_limit_until: None,
            rate_limit_display_seconds: None,
            should_quit: false,
            cache_dirty: true,
            manual_unread: HashSet::new(),
            computed_unread: HashSet::new(),
            subscribed_channels: HashSet::new(),
            closed_channel_subscriptions: HashSet::new(),
            handled_relay_events: HashMap::new(),
            handled_relay_event_order: VecDeque::new(),
            last_marked: HashMap::new(),
            read_dirty_since: None,
            last_cache_refresh: Instant::now(),
            directory_task: None,
            last_inbox_refresh: Instant::now(),
            background_tx,
            background_rx,
            attachment_tx,
            attachment_rx,
            render_generation: 0,
            last_hit_map: None,
            last_primary_click: None,
            apps: pane::AppPanes::default(),
            thread_drawn: false,
            demo: false,
            first_screen: crate::ui::first_screen::FirstScreen::default(),
            open_demo: false,
            notice_timer: crate::ui::status::NoticeTimer::default(),
            new_since: None,
            unread_counts: HashMap::new(),
        };
        app.sync_workspace_viewports();
        app.hydrate_cache().await?;
        Ok(app)
    }

    pub async fn run(self) -> Result<()> {
        self.run_to_exit().await.map(|_| ())
    }

    /// Runs bzz until the person leaves, and says how they left.
    pub async fn run_to_exit(mut self) -> Result<Exit> {
        let shutdown = shutdown_signal();
        tokio::pin!(shutdown);
        tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(Exit::Quit),
            _ = tokio::task::yield_now() => {},
        }
        let (mut guard, mut terminal) = TerminalGuard::enter(self.config.ui.mouse.enabled())?;
        self.media.initialize_terminal();
        // For development and the film only: a generated huddle (Ada, Lin
        // and Sam, and you muted) feeds the visualizer, with no call, sound
        // device or relay; `:huddle zoom` shows it. Not a user feature.
        if std::env::var_os("BZZ_DEV_GENERATED_HUDDLE").is_some()
            && self.huddle_viz.generated.is_none()
        {
            self.set_generated_huddle(
                crate::huddle::synth::Synthetic::demo(),
                vec![(1, "Ada".into()), (2, "Lin".into()), (3, "Sam".into())],
                true,
            );
        }
        self.start_sync().await?;
        let mut input = EventStream::new();
        // Where keys enter: Windows' Alt codes as presses, and the events
        // read ahead of an Enter to see whether a paste goes on behind it.
        let mut door = crate::ui::keymap::KeyDoor::default();
        let mut read_ahead = std::collections::VecDeque::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut frame_tick = tokio::time::interval(FRAME_INTERVAL);
        frame_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut redraw = RedrawGate::default();
        while !self.should_quit {
            // A notice lives about five seconds; the loop sleeps until then
            // and draws one frame when it goes (rule 20).
            self.notice_timer
                .observe(self.status_error.as_deref(), tokio::time::Instant::now());
            let notice_deadline = self.notice_timer.deadline();
            tokio::select! {
                biased;
                _=&mut shutdown=>self.should_quit=true,
                attachment=self.attachment_rx.recv()=>if let Some(event)=attachment{
                    self.handle_attachment_background(event).await?;
                    redraw.request();
                },
                maybe=next_read_ahead_event(&mut read_ahead, &mut input)=>{
                    if let Some(event)=maybe {
                        match event {
                            TerminalEvent::Key(key) => {
                                let key = door.admit(key);
                                if key.kind==KeyEventKind::Press {
                                    self.last_input_at=Instant::now();
                                    let key = if cfg!(windows) && self.composer_takes_enter(key) {
                                        let more = presses_wait(&mut read_ahead, &mut input);
                                        crate::ui::keymap::paste_line_break(key, more, true)
                                    } else {
                                        key
                                    };
                                    self.handle_key(key,&mut guard,&mut terminal).await?
                                }
                            },
                            TerminalEvent::Paste(text) => self.handle_terminal_paste(text).await?,
                            TerminalEvent::Mouse(mouse) => self.handle_mouse(mouse,&mut guard,&mut terminal).await?,
                            TerminalEvent::Resize(_, _) => {},
                            _ => {}
                        }
                        redraw.request();
                    }
                },
                _=frame_tick.tick()=>{
                    redraw.request_if(self.tick_huddle_viz());
                    if redraw.take() {
                        // What the apps produced is taken before the frame
                        // that shows them.
                        self.poll_apps();
                        let drawing = Instant::now();
                        terminal
                            .draw(|frame| self.render(frame))
                            .map_err(|error| Error::io("terminal", error))?;
                        self.huddle_frame_drawn(drawing.elapsed().as_secs_f64());
                    }
                },
                background=self.background_rx.recv()=>if let Some(event)=background{match event{
                    Background::AppWake=>{
                        self.app_woken();
                        redraw.request_if(self.poll_apps());
                        continue;
                    },
                    #[cfg(feature = "apps")]
                    Background::AppOffers(news)=>self.offer_news(news),
                    Background::Changed | Background::DraftAcknowledged=>{
                        self.cache_dirty=true;
                        if matches!(event, Background::DraftAcknowledged) {
                            self.status_error = None;
                        }
                        if self.presentation.route == Route::Inbox {
                            self.spawn_inbox_load(false);
                        }
                    },
                    Background::Failed(message)=>{
                        self.status_error=Some(message);
                        self.dm_picker.submitting=false;
                        self.inbox_loading=false;
                        self.inbox_detail_loading=false;
                        self.agent_directory.loading=false;
                    },
                    Background::Saved=>self.status_error=Some("attachment saved".into()),
                    Background::FederatedReady=>{
                        self.status_error=Some("federated sign-in complete; reconnecting".into());
                        if let Some(runtime)=&self.runtime {
                            runtime.supervisor.reconnect().await;
                        }
                        // Re-run the startup sync, now carrying evidence.
                        if let Err(error)=self.start_sync().await {
                            self.status_error=Some(error.to_string());
                        }
                    },
                    Background::Notice(message)=>self.status_error=Some(message),
                    Background::HistoryLoaded { community, summaries }=>{
                        self.cache_dirty=true;
                        if self.active_community_id()==Some(community) {
                            self.hydrate_cache().await?;
                            for event in summaries {
                                let parsed=self.store.call(move |store| store.live_thread_summary(community,&event)).await?;
                                if let Some(summary)=parsed {
                                    self.apply_live_thread_summary(summary);
                                }
                            }
                        }
                    },
                    Background::HuddleState { community, channel, active }=>{
                        self.huddle_loading=false;
                        if self.active_community_id()==Some(community)
                            && self.current_channel().map(|channel| channel.id)==Some(channel)
                        {
                            self.huddle_view=Some((channel, active));
                            self.cache_dirty=true;
                            #[cfg(feature = "huddles-audio")]
                            {
                                let ended = self.huddle_call.as_ref().is_some_and(|call| {
                                    call.parent == channel
                                        && self
                                            .huddle_view
                                            .as_ref()
                                            .and_then(|(_, active)| active.as_ref())
                                            .is_none_or(|active| active.backing != call.backing)
                                });
                                if ended && let Some(call) = self.huddle_call.take() {
                                    call.stop().await;
                                    self.status_error=Some("the huddle ended; audio devices released".into());
                                }
                            }
                        }
                    },
                    Background::HuddleStarted { parent, backing }=>{
                        self.huddle_dirty=true;
                        self.spawn_huddle_join(parent, backing);
                    },
                    #[cfg(feature = "huddles-audio")]
                    Background::HuddleJoined(result)=>{
                        self.huddle_joining=false;
                        match result {
                            Ok(call) => {
                                if let Some(previous) = self.huddle_call.replace(*call) {
                                    previous.stop().await;
                                }
                                self.status_error=Some("joined the huddle · muted · m to unmute, space to talk".into());
                            }
                            Err(error) => self.status_error=Some(error),
                        }
                        self.cache_dirty=true;
                    },
                    Background::ArtifactCaps { community, caps }=>{
                        if self.active_community_id()==Some(community) {
                            self.artifact_caps=Some((community, caps));
                            self.reconcile_artifact_sync().await?;
                        }
                    },
                    Background::ArtifactDone(result)=>{
                        self.artifact_panel.loading=false;
                        match result {
                            Ok(message) if message.is_empty() => {},
                            Ok(message) => self.status_error=Some(message),
                            Err(error) => self.status_error=Some(error),
                        }
                        self.refresh_artifact_panel().await?;
                    },
                    Background::ThreadWindow { community, root, next, supported }=>{
                        self.thread_window_loading=false;
                        if self.active_community_id()==Some(community)
                            && self.thread_root.as_deref()==Some(root.as_str())
                        {
                            self.thread_window=supported.then_some((root, next));
                            self.cache_dirty=true;
                        }
                    },
                    Background::PublicMemberships { community, subject, generation, result }=>{
                        self.apply_public_lookup(community, &subject, generation, result);
                    },
                    Background::EmojiSearch { query, found }=>{
                        if self.reaction_query.as_deref()==Some(query.as_str()) {
                            for emoji in found {
                                if !self.reaction_candidates.iter().any(|known| known.shortcode==emoji.shortcode)
                                    && self.reaction_candidates.len()<8
                                {
                                    self.reaction_candidates.push(emoji);
                                }
                            }
                        }
                    },
                    Background::PresenceSnapshot { community, asked, observations }=>{
                        if self.active_community_id()==Some(community) {
                            self.presence.apply_snapshot(&asked, &observations, crate::clock::SystemClock.now_unix());
                            self.cache_dirty=true;
                        }
                    },
                    Background::InboxLoaded { community, items }=>{
                        if self.active_community_id()==Some(community) {
                            self.inbox_items=items;
                            self.inbox_state.reconcile(&self.inbox_items);
                            if self.presentation.route == Route::Inbox {
                                self.spawn_inbox_detail_load();
                            }
                            self.cache_dirty=true;
                            self.inbox_loading=false;
                        }
                    }
                    Background::InboxDetailLoaded { community, conversation_id, messages }=>{
                        if self.active_community_id()==Some(community)
                            && self.presentation.route == Route::Inbox
                            && self.inbox_state.selected_id() == Some(conversation_id.as_str())
                        {
                            self.inbox_messages=messages;
                            let anchor=self.inbox_state.selected(&self.inbox_items)
                                .and_then(|item| item.first_unread_event_id.as_deref());
                            self.inbox_state.reconcile_detail(&self.inbox_messages, anchor);
                            self.cache_dirty=true;
                            self.inbox_detail_loading=false;
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
                    Background::SearchOlder { community, generation, output }=>{
                        if self.active_community_id()==Some(community)
                            && self.search_state.generation==generation
                        {
                            self.search_state.older_loading=false;
                            self.search_state.notice=Some(match output {
                                Ok(output)=>match self.search_state.append_older(output.results) {
                                    0=>"no older results".into(),
                                    added=>format!("{added} older result(s) added below"),
                                },
                                Err(error)=>format!("older results unavailable ({error})"),
                            });
                            self.search_state.reconcile();
                        }
                    }
                    Background::DmOpened { community, result }=>{
                        if self.active_community_id()==Some(community) {
                            self.cache_dirty=true;
                            self.hydrate_cache().await?;
                            if let Some(index)=self.channels.iter().position(|channel|channel.id==result.channel_id) {
                                self.select_channel_index(index);
                                self.showing_open_channel=false;
                                self.load_selected_channel().await?;
                                self.presentation.set_workspace_focus(FocusSurface::Timeline);
                            }
                            self.presentation.close_overlay();
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
                                self.presentation.set_workspace_focus(FocusSurface::Channels);
                            }
                        }
                    }
                    Background::AgentsLoaded { community, agents, notice }=>{
                        if self.active_community_id()==Some(community) {
                            self.agents = agents.iter().cloned()
                                .map(|agent| (agent.pubkey.clone(), agent))
                                .collect();
                            self.agent_directory.agents=agents;
                            self.agent_directory.loading=false;
                            self.agent_directory.reconcile();
                            self.status_error=notice;
                        }
                    }
                    Background::LocalAgentTurns { community, turns }=>{
                        if self.active_community_id()==Some(community) {
                            self.local_agent_turns = turns
                                .into_iter()
                                .filter_map(|(agent, turn)| turn.map(|turn| (agent, turn)))
                                .collect();
                        }
                    }
                    Background::AgentPreflight { new, result }=>self.agent_preflight_done(*new, result),
                    }
                    redraw.request();
                },
                () = async {
                    match notice_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if self.notice_timer.expire(self.status_error.as_deref(), tokio::time::Instant::now()) {
                        self.status_error = None;
                        redraw.request();
                    }
                },
                _=tick.tick()=>{
                    let changed=self.on_tick().await?;
                    let apps=self.tick_apps().await?;
                    redraw.request_if(changed || apps);
                },
                network=next_network(&mut self.runtime)=>{
                    if let Some(event)=network {
                        let visible_changed = self.handle_network(event).await?;
                        redraw.request_if(visible_changed);
                    }
                },
            }
        }
        for task in [
            self.inbox_task.take(),
            self.inbox_detail_task.take(),
            self.search_task.take(),
            self.dm_search_task.take(),
            self.agent_directory_task.take(),
            self.directory_task.take(),
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
        self.close_all_apps();
        self.shutdown().await;
        guard.restore();
        self.diagnostics.emit(DiagnosticEvent::ClientStopped {
            reason: "user".into(),
        });
        self.diagnostics.shutdown().await;
        Ok(if self.open_demo {
            Exit::OpenDemo
        } else {
            Exit::Quit
        })
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
        let signer = runtime.signer.clone();
        let community = runtime.community_id;
        let evidence = runtime.evidence.clone();
        let adapter = runtime.identity_adapter.clone();
        let data_dir = runtime.data_dir.clone();
        let identity_id = runtime.identity_id;
        let selected = self.current_channel().map(|channel| channel.id);
        if let Some(channel) = selected {
            self.subscribe_channel_with_priority(channel, SubscriptionPriority::Foreground)
                .await?;
            self.reconcile_agent_typing_subscription().await?;
        }
        supervisor
            .subscribe_with_priority(
                "membership",
                subscriptions::membership(&pubkey, nostr::Timestamp::now().as_secs()),
                SubscriptionPriority::Baseline,
            )
            .await?;
        supervisor
            .subscribe_with_priority(
                "global-stream",
                subscriptions::global_stream(nostr::Timestamp::now().as_secs()),
                SubscriptionPriority::Baseline,
            )
            .await?;
        supervisor
            .subscribe_with_priority(
                "presence",
                subscriptions::presence(nostr::Timestamp::now().as_secs()),
                SubscriptionPriority::Background,
            )
            .await?;
        supervisor
            .subscribe_with_priority(
                "personal",
                subscriptions::personal(&pubkey, nostr::Timestamp::now().as_secs()),
                SubscriptionPriority::Baseline,
            )
            .await?;
        supervisor
            .subscribe_with_priority(
                "read-state",
                subscriptions::read_state(
                    &pubkey,
                    nostr::Timestamp::now()
                        .as_secs()
                        .saturating_sub(read_state::HORIZON_SECONDS),
                ),
                SubscriptionPriority::Baseline,
            )
            .await?;
        let initial_channels = self
            .channels
            .iter()
            .filter(|channel| channel.is_member && Some(channel.id) != selected)
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
        let diagnostics = self.diagnostics.clone();
        tokio::spawn(async move {
            let result = async {
                let info = http.nip11().await?;
                let relay_pubkey = crate::protocol::http::relay_signing_pubkey(&info)
                    .ok_or_else(|| {
                        Error::Protocol("NIP-11 document has no relay signing key".into())
                    })?
                    .to_owned();
                store
                    .call({
                        let relay_pubkey = relay_pubkey.clone();
                        move |store| store.pin_relay_pubkey(community, &relay_pubkey)
                    })
                    .await?;
                let had_evidence = evidence.is_set();
                match crate::auth::federated::install_from_store(
                    &info,
                    adapter.as_ref(),
                    &data_dir,
                    identity_id,
                    &signer,
                    &evidence,
                )
                .await
                {
                    crate::auth::federated::Startup::NotRequired => {}
                    crate::auth::federated::Startup::Ready => {
                        if !had_evidence {
                            supervisor.reconnect().await;
                        }
                    }
                    blocked => {
                        // Fail once and quietly: no HTTP sync, no reconnect loop.
                        let notice = if blocked == crate::auth::federated::Startup::NeedsSignIn {
                            "this community requires federated sign-in: run :signin"
                        } else {
                            crate::auth::federated::RelayDenial::EvidenceRequired.notice()
                        };
                        let _ = tx.send(Background::Notice(notice.into())).await;
                        return Ok(());
                    }
                }
                channels.refresh(&pubkey).await?;
                // NIP-RS atomic snapshot: a complete read-state load when the
                // relay can prove it; otherwise the subscription above is it.
                let _ = read_state::load_snapshot(community, &http, &signer, &store).await;
                let _ = inbox.refresh(&pubkey).await;
                let _ = crate::sync::directory::hydrate_emoji_sets(community, &http, &store).await;
                if info.pointer("/federated_identity/public_profile").is_some() {
                    let _ = crate::sync::directory::hydrate_verified_profiles(
                        community,
                        authors.iter().cloned(),
                        &relay_pubkey,
                        &http,
                        &store,
                    )
                    .await;
                }
                let _ = profiles.hydrate(authors).await;
                let _ = outbox::flush_with_diagnostics(
                    community,
                    &http,
                    &supervisor,
                    &store,
                    &diagnostics,
                )
                .await;
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
        self.reconcile_agent_typing_subscription().await
    }

    async fn subscribe_channel(&mut self, channel: Uuid) -> Result<()> {
        self.subscribe_channel_with_priority(channel, SubscriptionPriority::Background)
            .await
    }

    async fn subscribe_channel_with_priority(
        &mut self,
        channel: Uuid,
        priority: SubscriptionPriority,
    ) -> Result<()> {
        let subscription = format!("ch-{}", channel.simple());
        if self.subscribed_channels.contains(&channel)
            || self.closed_channel_subscriptions.contains(&subscription)
            || self.subscribed_channels.len() >= 900
        {
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
            .subscribe_with_priority(
                subscription,
                subscriptions::channel(channel, cursor.high_created_at),
                priority,
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
        self.reconcile_artifact_sync().await?;
        self.reconcile_broker().await;
        self.reconcile_local_agents().await;
        self.reconcile_huddle().await?;
        self.reconcile_agent_typing_subscription().await
    }

    async fn reconcile_agent_typing_subscription(&mut self) -> Result<()> {
        let desired = self
            .runtime
            .as_ref()
            .and_then(|_| self.current_channel().map(|channel| channel.id));
        let transition = typing_subscription_transition(self.typing_subscription_channel, desired);
        if transition == TypingSubscriptionTransition::Unchanged {
            return Ok(());
        }

        self.typing_subscription_channel = None;
        self.agent_typing.clear();
        if self.closed_typing_channel != desired {
            self.closed_typing_channel = None;
            if self.status_error.as_deref() == Some("agent typing unavailable for this channel") {
                self.status_error = None;
            }
        }

        if transition == TypingSubscriptionTransition::Leave {
            if let Some(runtime) = &self.runtime {
                runtime.supervisor.close(AGENT_TYPING_SUBSCRIPTION).await?;
            }
            return Ok(());
        }
        let TypingSubscriptionTransition::Subscribe(channel) = transition else {
            return Ok(());
        };
        if self.closed_typing_channel == Some(channel) {
            return Ok(());
        }
        if let Some(runtime) = &self.runtime {
            // NIP-01 REQ replacement avoids a stale CLOSED acknowledgement for
            // the previous channel being attributed to this static ID.
            runtime
                .supervisor
                .subscribe_with_priority(
                    AGENT_TYPING_SUBSCRIPTION,
                    subscriptions::agent_typing(channel, nostr::Timestamp::now().as_secs()),
                    SubscriptionPriority::Foreground,
                )
                .await?;
            self.typing_subscription_channel = Some(channel);
        }
        Ok(())
    }

    fn spawn_backfill(&self, channel: Uuid) {
        if let Some(runtime) = &self.runtime {
            let service = runtime.channels.clone();
            let community = runtime.community_id;
            let tx = self.background_tx.clone();
            tokio::spawn(async move {
                let result = service.selected_history(channel).await;
                let _ = tx
                    .send(match result {
                        Ok(outcome) => Background::HistoryLoaded {
                            community,
                            summaries: outcome.summaries,
                        },
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
        let diagnostics = self.diagnostics.clone();
        self.directory_task = Some(tokio::spawn(async move {
            let started = Instant::now();
            let result = service.refresh(&pubkey).await;
            let _ = tx
                .send(match result {
                    Ok(report) => {
                        diagnostics.emit(DiagnosticEvent::AgentDirectoryRefreshed {
                            candidates: u32::try_from(report.agent_candidates).unwrap_or(u32::MAX),
                            verified: u32::try_from(report.verified_agents).unwrap_or(u32::MAX),
                            projection_changes: u32::try_from(report.agent_projection_changes)
                                .unwrap_or(u32::MAX),
                            duration_ms: u64::try_from(started.elapsed().as_millis())
                                .unwrap_or(u64::MAX),
                        });
                        Background::Changed
                    }
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

    fn spawn_inbox_detail_load(&mut self) {
        let Some(community) = self.active_community_id() else {
            return;
        };
        let Some(identity_pubkey) = self.self_pubkey().map(str::to_owned) else {
            return;
        };
        let Some(conversation_id) = self.inbox_state.selected_id().map(str::to_owned) else {
            self.inbox_messages.clear();
            self.inbox_detail_loading = false;
            return;
        };
        self.inbox_detail_loading = true;
        if let Some(task) = self.inbox_detail_task.take() {
            task.abort();
        }
        let service = self.runtime.as_ref().map(|runtime| runtime.inbox.clone());
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        self.inbox_detail_task = Some(tokio::spawn(async move {
            let result = if let Some(service) = service {
                service
                    .conversation_context(&identity_pubkey, &conversation_id)
                    .await
            } else {
                let context_id = conversation_id.clone();
                store
                    .call(move |store| {
                        store.inbox_conversation_context(community, &identity_pubkey, &context_id)
                    })
                    .await
            };
            let event = match result {
                Ok(messages) => Background::InboxDetailLoaded {
                    community,
                    conversation_id,
                    messages,
                },
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

    /// Ctrl-o in search: the next page of messages older than those shown.
    fn spawn_older_search(&mut self) {
        if self.search_state.loading || self.search_state.older_loading {
            return;
        }
        let Some(before) = self.search_state.oldest_message() else {
            self.search_state.notice = Some("no message results to page from".into());
            return;
        };
        let (Some(community), Some(identity_pubkey), Some(service)) = (
            self.active_community_id(),
            self.self_pubkey().map(str::to_owned),
            self.runtime.as_ref().map(|runtime| runtime.search.clone()),
        ) else {
            self.search_state.notice = Some("older results need a connected community".into());
            return;
        };
        let generation = self.search_state.generation;
        let input = self.search_state.query.clone();
        let channels = self.channels.clone();
        let profiles = self.profiles.clone();
        let tx = self.background_tx.clone();
        self.search_state.older_loading = true;
        tokio::spawn(async move {
            let output = service
                .older(&input, before, &identity_pubkey, &channels, &profiles)
                .await;
            let _ = tx
                .send(Background::SearchOlder {
                    community,
                    generation,
                    output,
                })
                .await;
        });
    }

    fn spawn_dm_profile_search(&mut self) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        if self.presentation.overlay != Some(Overlay::DmPicker) {
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
            self.local_thread_summaries.clear();
            self.live_thread_summaries.clear();
            self.thread_summaries.clear();
            self.thread_messages.clear();
            self.agents.clear();
            self.agent_typing.clear();
            self.reactions.clear();
            self.cache_dirty = false;
            self.last_cache_refresh = Instant::now();
            return Ok(());
        };
        let selected_channel_id = self.current_channel().map(|channel| channel.id);
        let selected_was_member = self
            .current_channel()
            .is_some_and(|channel| channel.is_member);
        self.channels = self
            .store
            .call(move |store| store.channels(community))
            .await?;
        self.sidebar_layout = match self.self_pubkey().map(str::to_owned) {
            Some(pubkey) => {
                self.store
                    .call(move |store| store.sidebar_layout(community, &pubkey))
                    .await?
            }
            None => crate::domain::SidebarLayout::default(),
        };
        // Membership removed by the relay (kick, delete, moderation): keep the
        // persisted draft, close the composer and move to a joined channel.
        let removed = selected_was_member
            && selected_channel_id.is_some_and(|id| {
                !self
                    .channels
                    .iter()
                    .any(|channel| channel.id == id && channel.is_member)
            });
        if removed {
            if self
                .presentation
                .composer_target
                .as_ref()
                .is_some_and(|target| Some(target.channel_id) == selected_channel_id)
            {
                self.persist_draft().await?;
                self.presentation.composer_target = None;
            }
            self.thread_root = None;
            self.showing_open_channel = false;
            self.status_error =
                Some("you are no longer a member of that channel; draft kept".into());
        }
        self.selected_channel = selected_channel_id
            .filter(|_| !removed)
            .and_then(|id| self.channels.iter().position(|channel| channel.id == id))
            .or_else(|| {
                self.channels
                    .iter()
                    .position(|channel| self.showing_open_channel || channel.is_member)
            })
            .unwrap_or(self.channels.len());
        self.sync_workspace_viewports();
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
            let root_event_ids = self
                .messages
                .iter()
                .filter(|message| message.system.is_none())
                .map(|message| message.event_id.clone())
                .collect::<Vec<_>>();
            let visible_roots = root_event_ids.iter().cloned().collect::<HashSet<_>>();
            self.local_thread_summaries = self
                .store
                .call(move |store| store.thread_summaries(community, channel, &root_event_ids))
                .await?;
            self.live_thread_summaries.retain(|root, summary| {
                summary.channel_id == channel && visible_roots.contains(root)
            });
            self.rebuild_thread_summaries();
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
            // Contextual author labels (identity names v1): this channel's
            // members plus the authors shown here.
            if let Some(viewer) = self.self_pubkey().map(str::to_owned) {
                let authors = self
                    .messages
                    .iter()
                    .chain(self.thread_messages.iter())
                    .map(|message| message.pubkey.clone())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let labels = self
                    .store
                    .call(move |store| {
                        store.channel_identity_labels(community, channel, &viewer, &authors)
                    })
                    .await?;
                self.thread_timeline.labels.clone_from(&labels);
                self.timeline.labels = labels;
            }
        } else {
            self.messages.clear();
            self.local_thread_summaries.clear();
            self.live_thread_summaries.clear();
            self.thread_summaries.clear();
            self.thread_messages.clear();
        }
        self.resolve_custom_emoji(community).await?;
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
        let now = crate::clock::SystemClock.now_unix();
        let huddle_roster = self
            .active_huddle()
            .map(|active| active.participants.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();
        for profile in self.profiles.values_mut() {
            profile.presence = Some(self.presence.get(&profile.pubkey, now));
            profile.in_huddle = huddle_roster.contains(&profile.pubkey);
        }
        let self_pubkey = self.self_pubkey().unwrap_or_default().to_owned();
        self.agents = if self_pubkey.is_empty() {
            HashMap::new()
        } else {
            let identity = self_pubkey.clone();
            self.store
                .call(move |store| store.list_remote_agents(community, &identity))
                .await?
                .into_iter()
                .map(|mut agent| {
                    // Owner-signed 20003 presence the pinned relay accepted.
                    match self.presence.get(&agent.pubkey, now) {
                        crate::protocol::presence::Presence::Online => {
                            agent.presence = crate::agents::Presence::Online;
                        }
                        crate::protocol::presence::Presence::Away => {
                            agent.presence = crate::agents::Presence::Away;
                        }
                        _ => {}
                    }
                    (agent.pubkey.clone(), agent)
                })
                .collect()
        };
        let authorized_typing_agents = self
            .current_channel()
            .map(|channel| {
                self.agents
                    .values()
                    .filter(|agent| {
                        !agent.stale
                            && agent.channel_ids.contains(&channel.id)
                            && (!channel.kind.is_dm()
                                || self.self_pubkey().is_some_and(|pubkey| {
                                    agent.owner_pubkey.eq_ignore_ascii_case(pubkey)
                                }))
                    })
                    .map(|agent| agent.pubkey.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        if self.cache_dirty {
            self.agent_typing.retain_agents(&authorized_typing_agents);
        }
        let dm_participants = self
            .store
            .call(move |store| store.dm_participants_map(community))
            .await?;
        self.dm_presence.clear();
        for channel in self
            .channels
            .iter_mut()
            .filter(|channel| channel.kind.is_dm())
        {
            if let Some(participants) = dm_participants.get(&channel.id) {
                channel.name = dm_label(participants, &self_pubkey, &self.profiles);
                let others = participants
                    .iter()
                    .filter(|pubkey| !pubkey.eq_ignore_ascii_case(&self_pubkey))
                    .collect::<Vec<_>>();
                if let [other] = others.as_slice() {
                    self.dm_presence
                        .insert(channel.id, self.presence.get(other, now));
                }
            }
        }
        if self.presentation.overlay == Some(Overlay::DmPicker) {
            let pubkey = self.self_pubkey().unwrap_or_default().to_owned();
            self.dm_picker.reconcile(&self.profiles, &pubkey);
        }
        let mut profile_subjects = self
            .messages
            .iter()
            .chain(self.thread_messages.iter())
            .filter(|message| message.system.is_none())
            .map(|message| message.pubkey.clone())
            .chain(
                self.agents
                    .values()
                    .flat_map(|agent| [agent.pubkey.clone(), agent.owner_pubkey.clone()]),
            )
            .collect::<HashSet<_>>();
        for system in self
            .messages
            .iter()
            .chain(self.thread_messages.iter())
            .filter_map(|message| message.system.as_ref())
        {
            profile_subjects.extend(system.actor.iter().cloned());
            profile_subjects.extend(system.target.iter().cloned());
            profile_subjects.extend(system.participants.iter().cloned());
        }
        let unknown_authors = profile_subjects
            .into_iter()
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
        self.unread_counts = if let Some(runtime) = &self.runtime {
            let pubkey = runtime.signer.public_key().to_hex();
            self.store
                .call(move |store| store.unread_channel_counts(community, &pubkey))
                .await?
        } else {
            HashMap::new()
        };
        self.computed_unread = self.unread_counts.keys().copied().collect();
        self.cache_dirty = false;
        self.last_cache_refresh = Instant::now();
        Ok(())
    }

    fn visible_typing_scope(&self) -> TypingScope {
        self.thread_root
            .as_ref()
            .map_or(TypingScope::Channel, |root| {
                TypingScope::Thread(root.clone())
            })
    }

    fn agent_typing_authorized(&self, pubkey: &str, channel_id: Uuid) -> bool {
        let Some(channel) = self
            .current_channel()
            .filter(|channel| channel.id == channel_id)
        else {
            return false;
        };
        self.agents.get(pubkey).is_some_and(|agent| {
            !agent.stale
                && agent.channel_ids.contains(&channel_id)
                && (!channel.kind.is_dm()
                    || self
                        .self_pubkey()
                        .is_some_and(|active| agent.owner_pubkey.eq_ignore_ascii_case(active)))
        })
    }

    fn apply_agent_typing_event(&mut self, event: &nostr::Event, now: u64) -> bool {
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return false;
        };
        let Some(signal) = crate::agents::typing::parse(event, channel, now) else {
            return false;
        };
        if signal.scope != self.visible_typing_scope()
            || !self.agent_typing_authorized(&signal.agent_pubkey, channel)
        {
            return false;
        }
        let was_empty = self.agent_typing.is_empty();
        let changed = self.agent_typing.apply(signal, now);
        if changed && was_empty {
            self.typing_spinner_frame = 0;
            self.typing_spinner_advanced_at = Instant::now();
        }
        changed
    }

    fn visible_agent_typing_line(&self, width: u16) -> Option<String> {
        let scope = self.visible_typing_scope();
        let now = nostr::Timestamp::now().as_secs();
        let labels = self
            .agent_typing
            .active_pubkeys(&scope, now)
            .into_iter()
            .filter_map(|pubkey| {
                let agent = self.agents.get(pubkey)?;
                let label = self
                    .profiles
                    .get(pubkey)
                    .map(Profile::label)
                    .unwrap_or_else(|| agent.name.clone());
                Some(sanitize::single_line(&label))
            })
            .collect::<Vec<_>>();
        crate::ui::typing::format_typing_indicator(&labels, self.typing_spinner_frame, width)
    }

    fn rebuild_thread_summaries(&mut self) {
        self.thread_summaries = self.local_thread_summaries.clone();
        for (root, live) in &self.live_thread_summaries {
            let combined = self.thread_summaries.entry(root.clone()).or_default();
            combined.descendant_count =
                combined.descendant_count.max(live.summary.descendant_count);
            combined.last_reply_at = match (combined.last_reply_at, live.summary.last_reply_at) {
                (Some(local), Some(remote)) => Some(local.max(remote)),
                (local, remote) => local.or(remote),
            };
        }
        self.thread_summaries
            .retain(|_, summary| summary.descendant_count > 0);
    }

    fn apply_live_thread_summary(&mut self, summary: LiveThreadSummary) -> bool {
        if self.current_channel().map(|channel| channel.id) != Some(summary.channel_id)
            || !self.messages.iter().any(|message| {
                message.event_id == summary.root_event_id
                    && message.root_event_id.is_none()
                    && message.system.is_none()
            })
        {
            return false;
        }
        if self
            .live_thread_summaries
            .get(&summary.root_event_id)
            .is_some_and(|current| {
                (current.source_created_at, current.source_event_id.as_str())
                    >= (summary.source_created_at, summary.source_event_id.as_str())
            })
        {
            return false;
        }
        self.live_thread_summaries
            .insert(summary.root_event_id.clone(), summary);
        self.rebuild_thread_summaries();
        true
    }

    async fn handle_network(&mut self, event: SupervisorEvent) -> Result<bool> {
        let previous_connection = self.connection;
        let previous_status = self.status_error.clone();
        let previous_rate_limit = self.rate_limit_display_seconds;
        let mut transient_changed = false;
        match event {
            SupervisorEvent::Connecting => {
                self.connection = ConnectionState::Connecting;
                self.rate_limit_until = None;
                self.rate_limit_display_seconds = None;
                transient_changed |= self.agent_typing.clear();
            }
            SupervisorEvent::Backoff(_) => {
                self.connection = ConnectionState::Offline;
                self.rate_limit_until = None;
                self.rate_limit_display_seconds = None;
                transient_changed |= self.agent_typing.clear();
            }
            SupervisorEvent::RateLimited { retry_after } => {
                let retry_after = retry_after.max(Duration::from_secs(1));
                self.rate_limit_until = Some(Instant::now() + retry_after);
                self.rate_limit_display_seconds = Some(retry_after.as_secs().max(1));
            }
            SupervisorEvent::RateLimitCleared => {
                self.rate_limit_until = None;
                self.rate_limit_display_seconds = None;
            }
            SupervisorEvent::Terminal(message) => {
                self.connection = if message.contains("clock-skew") {
                    ConnectionState::ClockSkew
                } else {
                    ConnectionState::AccessDenied
                };
                self.status_error = Some(message);
                transient_changed |= self.agent_typing.clear();
            }
            SupervisorEvent::Session(SessionEvent::Authenticated) => {
                self.connection = ConnectionState::Online;
                self.status_error = None;
                self.closed_typing_channel = None;
                self.typing_subscription_channel = None;
                transient_changed |= self.agent_typing.clear();
                if let Some(runtime) = &self.runtime {
                    let community = runtime.community_id;
                    let http = runtime.http.clone();
                    let supervisor = runtime.supervisor.clone();
                    let store = self.store.clone();
                    let directory = runtime.channels.clone();
                    let inbox = runtime.inbox.clone();
                    let pubkey = runtime.signer.public_key().to_hex();
                    let tx = self.background_tx.clone();
                    let diagnostics = self.diagnostics.clone();
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
                            outbox::flush_with_diagnostics(
                                community,
                                &http,
                                &supervisor,
                                &store,
                                &diagnostics,
                            )
                            .await
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
            SupervisorEvent::Session(SessionEvent::Event {
                subscription,
                event,
            }) => {
                let Some(runtime) = &self.runtime else {
                    return Ok(false);
                };
                let community = runtime.community_id;
                // Sync may resend a known head (resync, reconnect); it must
                // reach the buffer, so artifacts bypass the relay dedupe.
                // What arrives on an app's records subscription is the
                // records service's, whatever its kind.
                if subscription == ARTIFACT_SYNC_SUBSCRIPTION
                    || crate::protocol::artifact::is_apps_subscription(&subscription)
                    || matches!(event.kind.as_u16(), 45_010 | 45_011)
                {
                    return self.apply_artifact_sync_event(&subscription, &event).await;
                }
                if matches!(event.kind.as_u16(), 48_100..=48_106 | 24_810) {
                    return Ok(self.apply_huddle_event(&event));
                }
                let event_id = event.id.to_hex();
                if self.relay_event_was_handled(community, &event_id) {
                    return Ok(false);
                }
                if matches!(
                    event.kind.as_u16(),
                    crate::protocol::presence::KIND_PRESENCE
                        | crate::protocol::presence::KIND_AGENT_PRESENCE
                ) {
                    self.remember_relay_event(community, event_id);
                    if crate::protocol::events::verify(&event).is_err() {
                        return Ok(false);
                    }
                    let relay = self
                        .store
                        .call(move |store| store.pinned_relay_pubkey(community))
                        .await?;
                    let observation =
                        crate::protocol::presence::observe(&event, relay.as_deref(), |agent| {
                            self.agents
                                .get(agent)
                                .filter(|view| !view.stale)
                                .map(|view| view.owner_pubkey.clone())
                        });
                    if let Some(observation) = observation {
                        self.presence
                            .apply(&observation, crate::clock::SystemClock.now_unix());
                        self.cache_dirty = true;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                if event.kind.as_u16() == crate::agents::typing::KIND_TYPING_INDICATOR {
                    let changed = subscription == AGENT_TYPING_SUBSCRIPTION
                        && self.apply_agent_typing_event(&event, nostr::Timestamp::now().as_secs());
                    self.remember_relay_event(community, event_id);
                    return Ok(changed);
                } else if event.kind.as_u16() == 39_005 {
                    let parsed = self
                        .store
                        .call(move |store| store.live_thread_summary(community, &event))
                        .await?;
                    self.remember_relay_event(community, event_id);
                    if let Some(summary) = parsed {
                        return Ok(self.apply_live_thread_summary(summary));
                    }
                } else if crate::protocol::sidebar::Lane::of(&event).is_some() {
                    // Synced sidebar sections or sort: keep the newest
                    // readable head; an unreadable one changes nothing.
                    let signer = runtime.signer.clone();
                    if let Ok((lane, tree)) =
                        crate::protocol::sidebar::decode_event(&event, &signer).await
                    {
                        let pubkey = event.pubkey.to_hex();
                        let id = event.id.to_hex();
                        let at = event.created_at.as_secs();
                        let changed = self
                            .store
                            .call(move |store| {
                                store.record_sidebar_document(
                                    community, &pubkey, lane, &id, at, &tree,
                                )
                            })
                            .await?;
                        self.cache_dirty |= changed;
                    }
                    self.remember_relay_event(community, event_id);
                } else if event.kind.as_u16() == 30_078 {
                    // Serialize read-state reduction so an echo flood cannot
                    // enqueue concurrent decrypt/write work ahead of input.
                    let signer = runtime.signer.clone();
                    let store = self.store.clone();
                    match read_state::merge_events(community, &[event], &signer, &store).await {
                        Ok(_) => {
                            self.remember_relay_event(community, event_id);
                            self.cache_dirty = true;
                        }
                        Err(error) => self.status_error = Some(error.to_string()),
                    }
                } else {
                    let kind = event.kind.as_u16();
                    // Buzz announces agent roster edits as pinned-relay
                    // semantic rows before clients necessarily observe the
                    // replacement kind 39002. Treat these rows only as a
                    // bounded refresh trigger; the signed membership snapshot
                    // remains the sole source of role authority.
                    let membership_refresh = triggers_directory_refresh(kind, &event.content);
                    let thread_mutation = if matches!(kind, 9 | 40_002) {
                        crate::protocol::events::thread_coordinates(&event).0
                    } else {
                        None
                    };
                    let clears_live_thread_summaries = matches!(kind, 5 | 9005);
                    let typing_message =
                        self.current_channel()
                            .map(|channel| channel.id)
                            .and_then(|channel| {
                                let author = event.pubkey.to_hex();
                                if self.agent_typing_authorized(&author, channel) {
                                    crate::agents::typing::message_scope(&event, channel)
                                        .map(|scope| (author, scope, event.created_at.as_secs()))
                                } else {
                                    None
                                }
                            });
                    match self
                        .store
                        .call(move |store| store.apply_event(community, &event))
                        .await
                    {
                        Ok(changed) => {
                            self.remember_relay_event(community, event_id);
                            if let Some((author, scope, created_at)) = typing_message {
                                transient_changed |= self.agent_typing.observe_message(
                                    &author,
                                    scope,
                                    created_at,
                                    nostr::Timestamp::now().as_secs(),
                                );
                            }
                            if changed {
                                if let Some(root) = thread_mutation {
                                    self.live_thread_summaries.remove(&root);
                                    self.rebuild_thread_summaries();
                                } else if clears_live_thread_summaries {
                                    self.live_thread_summaries.clear();
                                    self.rebuild_thread_summaries();
                                }
                                self.cache_dirty = true;
                                if matches!(kind, 9 | 40_002 | 40_003 | 30_622 | 46_010..=46_012) {
                                    self.spawn_inbox_load(false);
                                }
                                if membership_refresh {
                                    self.spawn_directory_refresh();
                                }
                            }
                        }
                        Err(error) => self.status_error = Some(error.to_string()),
                    }
                }
            }
            SupervisorEvent::Session(SessionEvent::Eose(subscription)) => {
                if subscription == ARTIFACT_SYNC_SUBSCRIPTION {
                    self.commit_artifact_sync().await?;
                    return Ok(true);
                }
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
                if subscription == ARTIFACT_SYNC_SUBSCRIPTION {
                    if let Some(sync) = &mut self.artifact_sync {
                        use crate::realtime::admission::{ClosedReason, closed_reason};
                        // The last complete set stays; it is possibly stale.
                        match closed_reason(&message) {
                            ClosedReason::Restricted => {
                                sync.blocked = true;
                                sync.buffer.clear();
                            }
                            ClosedReason::SyncOverflow => sync.overflow(),
                            ClosedReason::Resync | ClosedReason::Other => sync.schedule_retry(),
                        }
                    }
                    self.refresh_artifact_panel().await?;
                    return Ok(true);
                }
                // The state of an app's records is its pane's to show, and
                // that of the offers of apps the offers service's: not the
                // status line's.
                if crate::protocol::artifact::is_apps_subscription(&subscription) {
                    return Ok(false);
                }
                if subscription == AGENT_TYPING_SUBSCRIPTION {
                    self.closed_typing_channel = self.typing_subscription_channel.take();
                    transient_changed |= self.agent_typing.clear();
                    self.diagnostics
                        .emit(DiagnosticEvent::AgentTypingSubscriptionClosed {
                            error_class: ErrorClass::from_subscription_closed(&message),
                        });
                    self.status_error = Some("agent typing unavailable for this channel".into());
                } else {
                    if subscription.starts_with("ch-")
                        && self
                            .closed_channel_subscriptions
                            .insert(subscription.clone())
                    {
                        self.connection = ConnectionState::AccessDenied;
                        if let Some(channel) = self
                            .subscribed_channels
                            .iter()
                            .find(|channel| format!("ch-{}", channel.simple()) == subscription)
                            .copied()
                        {
                            self.subscribed_channels.remove(&channel);
                        }
                        if self.runtime.is_some() {
                            self.spawn_directory_refresh();
                        }
                    }
                    self.status_error = Some(message);
                }
            }
            SupervisorEvent::Session(SessionEvent::Disconnected(message)) => {
                self.connection = ConnectionState::Offline;
                self.status_error = Some(message);
                // No resume: the supervisor re-sends the sync on reconnect and
                // the new set replaces the old one only at its EOSE.
                if let Some(sync) = &mut self.artifact_sync {
                    sync.buffer.clear();
                    sync.complete = false;
                }
                self.rate_limit_until = None;
                self.rate_limit_display_seconds = None;
                transient_changed |= self.agent_typing.clear();
            }
            SupervisorEvent::Session(SessionEvent::Count { .. }) => {}
        }
        Ok(transient_changed
            || self.connection != previous_connection
            || self.status_error != previous_status
            || self.rate_limit_display_seconds != previous_rate_limit)
    }

    fn relay_event_was_handled(&self, community: Uuid, event_id: &str) -> bool {
        self.handled_relay_events
            .get(&community)
            .is_some_and(|events| events.contains(event_id))
    }

    fn remember_relay_event(&mut self, community: Uuid, event_id: String) {
        if !self
            .handled_relay_events
            .entry(community)
            .or_default()
            .insert(event_id.clone())
        {
            return;
        }
        self.handled_relay_event_order
            .push_back((community, event_id));
        while self.handled_relay_event_order.len() > RELAY_EVENT_DEDUP_CAPACITY {
            if let Some((community, expired)) = self.handled_relay_event_order.pop_front() {
                let remove_community =
                    self.handled_relay_events
                        .get_mut(&community)
                        .is_some_and(|events| {
                            events.remove(&expired);
                            events.is_empty()
                        });
                if remove_community {
                    self.handled_relay_events.remove(&community);
                }
            }
        }
    }

    /// Performs bounded timer work and reports whether it changed visible
    /// presentation. The run loop can therefore remain idle without emitting a
    /// full terminal frame every 100 ms.
    async fn on_tick(&mut self) -> Result<bool> {
        let mut changed = self.media.poll();
        if let Some(deadline) = self.rate_limit_until {
            let now = Instant::now();
            if deadline <= now {
                self.rate_limit_until = None;
                self.rate_limit_display_seconds = None;
                changed = true;
            } else {
                let remaining = deadline.duration_since(now);
                let seconds = u64::try_from(remaining.as_millis().div_ceil(1_000))
                    .unwrap_or(u64::MAX)
                    .max(1);
                if self.rate_limit_display_seconds != Some(seconds) {
                    self.rate_limit_display_seconds = Some(seconds);
                    changed = true;
                }
            }
        }
        changed |= self.agent_typing.expire(nostr::Timestamp::now().as_secs());
        let typing_visible = self.visible_agent_typing_line(5).is_some();
        if typing_visible && self.typing_spinner_advanced_at.elapsed() >= AGENT_SPINNER_INTERVAL {
            self.typing_spinner_frame =
                (self.typing_spinner_frame + 1) % crate::ui::typing::SPINNER_FRAME_COUNT;
            self.typing_spinner_advanced_at = Instant::now();
            changed = true;
        } else if !typing_visible && self.typing_spinner_frame != 0 {
            self.typing_spinner_frame = 0;
            self.typing_spinner_advanced_at = Instant::now();
        }
        if let Some(started) = self.leader_started_at {
            let elapsed = started.elapsed();
            if !self.input_router.sequence_active() {
                self.leader_started_at = None;
            } else if self.presentation.overlay.is_none()
                && crate::ui::help::leader_panel_due(elapsed)
            {
                // Who knows the next key never sees the panel; once shown it
                // waits for that key, or Esc (v0.17 C6).
                self.presentation.open_overlay(Overlay::WhichKey);
                changed = true;
            } else if self.presentation.overlay.is_some()
                && self.presentation.overlay != Some(Overlay::WhichKey)
                && elapsed >= LEADER_TIMEOUT
            {
                // Something else took the screen: the panel cannot show, and
                // the sequence ends as in v0.16.
                self.input_router.cancel_sequence();
                self.leader_started_at = None;
                changed = true;
            }
        }
        if self
            .search_dirty_since
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(300))
        {
            self.search_dirty_since = None;
            self.spawn_search();
            changed = true;
        }
        changed |= self.refresh_approvals();
        // The agents panel shows what runs now: it is read again every
        // second while it is open.
        if self.presentation.overlay == Some(Overlay::LocalAgents)
            && self
                .local_agent_refreshed
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            self.refresh_local_agent_panel();
            changed = true;
        }
        if self
            .dm_dirty_since
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(300))
        {
            self.dm_dirty_since = None;
            self.spawn_dm_profile_search();
            changed = true;
        }
        if self.last_presence_snapshot.elapsed()
            >= Duration::from_secs(crate::protocol::presence::HEARTBEAT_SECONDS)
        {
            self.last_presence_snapshot = Instant::now();
            self.spawn_presence_snapshot();
        }
        self.maybe_publish_presence().await;
        if self.last_inbox_refresh.elapsed() >= Duration::from_secs(30) {
            self.last_inbox_refresh = Instant::now();
            if self.runtime.is_some() {
                self.spawn_inbox_load(true);
                changed = true;
            }
        }
        let cache_was_dirty = self.cache_dirty;
        if cache_was_dirty || self.last_cache_refresh.elapsed() > Duration::from_secs(1) {
            self.hydrate_cache().await?;
            changed |= cache_was_dirty;
            self.reconcile_subscriptions().await?;
        }
        if should_mark_visible_read(&self.presentation, &self.timeline, &self.thread_timeline) {
            let unread_before = (self.computed_unread.len(), self.manual_unread.len());
            self.mark_current_read().await?;
            changed |= unread_before != (self.computed_unread.len(), self.manual_unread.len());
        }
        self.reconcile_agent_typing_subscription().await?;
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
        Ok(changed)
    }

    async fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        // Presentation overlays own pointer input. An action-menu click only
        // changes its menu selection; it never leaks to or activates the
        // workspace beneath it.
        if self.presentation.overlay.is_some() {
            if self.presentation.overlay == Some(Overlay::Actions) {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    && let Some(HitTarget::ActionMenu(action)) = self
                        .last_hit_map
                        .as_ref()
                        .and_then(|map| map.hit(mouse.column, mouse.row))
                        .cloned()
                {
                    let double_click = self.record_primary_click(&HitTarget::ActionMenu(action));
                    if let Some(menu) = &mut self.action_menu {
                        menu.select_action(action);
                    }
                    if double_click {
                        let entry = self.action_menu.as_ref().and_then(ActionMenu::selected);
                        self.presentation.overlay = None;
                        self.action_menu = None;
                        if let Some(entry) = entry {
                            if entry.enabled {
                                self.dispatch_route_action(entry.action, guard, terminal)
                                    .await?;
                            } else if let Some(reason) = entry.reason {
                                self.status_error = Some(format!("{}: {reason}", entry.label));
                            }
                        }
                    }
                }
                return Ok(());
            }
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && let Some(target) = self
                    .last_hit_map
                    .as_ref()
                    .and_then(|map| map.hit(mouse.column, mouse.row))
                    .cloned()
            {
                if self.select_remote_agent_mouse_target(&target) {
                    return Ok(());
                }
                // Other overlay hit targets are guarded again inside the
                // activation adapter. Background pane targets remain inert.
                self.activate_mouse_target(target, mouse, guard, terminal)
                    .await?;
            }
            return Ok(());
        }
        let Some(target) = self
            .last_hit_map
            .as_ref()
            .and_then(|map| map.hit(mouse.column, mouse.row))
            .cloned()
        else {
            return Ok(());
        };
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if self.presentation.composer_target.is_none() =>
            {
                let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                    -3
                } else {
                    3
                };
                self.wheel_over(target, delta);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.activate_mouse_target(target, mouse, guard, terminal)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The wheel scrolls the pane under it. With an app focused, the app
    /// keeps the keyboard; an app has no mouse, so the wheel over its pane
    /// does nothing.
    fn wheel_over(&mut self, target: HitTarget, delta: isize) {
        let app_focused = self.app_has_focus();
        match target {
            HitTarget::App => {}
            HitTarget::Community(_) => {
                self.presentation
                    .set_workspace_focus(FocusSurface::Communities);
                self.scroll_focused_viewport(ViewportScroll::Lines(delta));
            }
            HitTarget::ChannelPane | HitTarget::Channel(_) => {
                self.presentation
                    .set_workspace_focus(FocusSurface::Channels);
                self.scroll_focused_viewport(ViewportScroll::Lines(delta));
            }
            HitTarget::Timeline | HitTarget::TimelineMessage(_) => {
                self.presentation
                    .set_workspace_focus(FocusSurface::Timeline);
                self.scroll_focused_viewport(ViewportScroll::Lines(delta));
            }
            HitTarget::Thread | HitTarget::ThreadMessage(_) => {
                self.presentation.set_workspace_focus(FocusSurface::Context);
                self.scroll_focused_viewport(ViewportScroll::Lines(delta));
            }
            HitTarget::InboxList | HitTarget::InboxItem(_)
                if self.presentation.route == Route::Inbox =>
            {
                self.presentation.set_inbox_focus(false);
                self.inbox_state.scroll_list(delta, &self.inbox_items);
            }
            HitTarget::InboxDetail if self.presentation.route == Route::Inbox => {
                self.presentation.set_inbox_focus(true);
                self.inbox_state.scroll_detail(delta, &self.inbox_messages);
            }
            _ => {}
        }
        if app_focused {
            self.presentation.set_workspace_focus(FocusSurface::App);
        }
    }

    async fn activate_mouse_target(
        &mut self,
        target: HitTarget,
        mouse: MouseEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        let double_click = self.record_primary_click(&target);
        match target {
            HitTarget::Community(index)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none()
                    && index < self.config.communities.len() =>
            {
                self.select_community_index(index);
                self.presentation
                    .set_workspace_focus(FocusSurface::Communities);
                if double_click {
                    self.switch_community(index, guard, terminal).await?;
                }
            }
            HitTarget::ChannelPane
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.presentation
                    .set_workspace_focus(FocusSurface::Channels)
            }
            HitTarget::Channel(index)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none()
                    && self
                        .channels
                        .get(index)
                        .is_some_and(|channel| channel.is_member) =>
            {
                self.activate_channel_click(index, double_click).await?;
            }
            HitTarget::Timeline
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.presentation
                    .set_workspace_focus(FocusSurface::Timeline)
            }
            HitTarget::TimelineMessage(event_id)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.presentation
                    .set_workspace_focus(FocusSurface::Timeline);
                self.timeline.selected_event = Some(event_id);
                self.timeline.at_live_bottom = self.messages.last().is_some_and(|message| {
                    self.timeline.selected_event.as_deref() == Some(&message.event_id)
                });
                self.timeline.keep_selection_visible = true;
                if double_click {
                    self.toggle_thread().await?;
                }
            }
            HitTarget::Thread
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.presentation.set_workspace_focus(FocusSurface::Context)
            }
            // A click on the app's pane gives it the focus, and the app is
            // told nothing.
            HitTarget::App
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.focus_app();
            }
            HitTarget::ThreadMessage(event_id)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none() =>
            {
                self.presentation.set_workspace_focus(FocusSurface::Context);
                self.thread_timeline.selected_event = Some(event_id);
                self.thread_timeline.at_live_bottom =
                    self.thread_messages.last().is_some_and(|message| {
                        self.thread_timeline.selected_event.as_deref() == Some(&message.event_id)
                    });
                self.thread_timeline.keep_selection_visible = true;
            }
            HitTarget::InboxList
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none()
                    && self.presentation.route == Route::Inbox =>
            {
                self.presentation.set_inbox_focus(false);
            }
            HitTarget::InboxItem(id)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none()
                    && self.presentation.route == Route::Inbox
                    && self
                        .inbox_items
                        .iter()
                        .any(|item| item.conversation_id == id) =>
            {
                self.inbox_state.select(id);
                self.presentation.set_inbox_focus(false);
                self.spawn_inbox_detail_load();
                if double_click {
                    self.inbox_state.narrow_detail = self.inbox_state.narrow_layout;
                    self.presentation.set_inbox_focus(true);
                }
            }
            HitTarget::InboxDetail
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_none()
                    && self.presentation.route == Route::Inbox =>
            {
                self.presentation.set_inbox_focus(true);
            }
            HitTarget::Composer if self.presentation.overlay.is_none() => {
                if self.presentation.composer_target.is_none() {
                    self.enter_composer().await?;
                } else if let Some(area) = self
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
            HitTarget::MentionCandidate(index)
                if self.presentation.overlay.is_none()
                    && self.presentation.composer_target.is_some() =>
            {
                if let Some(picker) = &mut self.mention_picker
                    && index < picker.candidates.len()
                {
                    picker.selected = index;
                    self.accept_mention();
                    self.persist_draft().await?;
                }
            }
            HitTarget::FinderChannel(channel_id)
                if self.presentation.overlay == Some(Overlay::Finder) =>
            {
                if let Some(index) = self
                    .channels
                    .iter()
                    .position(|channel| channel.id.to_string() == channel_id)
                {
                    self.select_channel_index(index);
                    self.showing_open_channel = !self.channels[index].is_member;
                    self.load_selected_channel().await?;
                    self.presentation
                        .set_workspace_focus(FocusSurface::Timeline);
                }
                self.presentation.close_overlay();
            }
            HitTarget::Theme(id) if self.presentation.overlay == Some(Overlay::Theme) => {
                if let Some(picker) = &mut self.theme_picker
                    && picker.select_id(&id)
                {
                    self.preview_selected_theme();
                }
            }
            HitTarget::Reaction(index)
                if self.presentation.overlay == Some(Overlay::Reaction)
                    && index < crate::ui::reaction_picker::REACTIONS.len() =>
            {
                self.reaction_index = index;
            }
            HitTarget::SearchResult(id)
                if self.presentation.overlay == Some(Overlay::Search)
                    && self
                        .search_state
                        .results
                        .iter()
                        .any(|result| result.stable_id == id) =>
            {
                self.search_state.selected_id = Some(id);
            }
            HitTarget::DmCandidate(pubkey)
                if self.presentation.overlay == Some(Overlay::DmPicker)
                    && !self.dm_picker.submitting =>
            {
                self.dm_picker.selected_pubkey = Some(pubkey);
            }
            _ => {}
        }
        Ok(())
    }

    fn select_remote_agent_mouse_target(&mut self, target: &HitTarget) -> bool {
        let HitTarget::RemoteAgent(pubkey) = target else {
            return false;
        };
        if self.presentation.overlay != Some(Overlay::Agents)
            || !self
                .agent_directory
                .agents
                .iter()
                .any(|agent| agent.pubkey == *pubkey)
        {
            return false;
        }
        self.agent_directory.selected_pubkey = Some(pubkey.clone());
        true
    }

    /// A channel row is an activation target, not only a movable selection.
    /// Keep the sidebar focused after one click, while always replacing the
    /// timeline with the selected channel. A second click may move focus into
    /// the timeline, but is never required to open the channel.
    async fn activate_channel_click(&mut self, index: usize, double_click: bool) -> Result<()> {
        self.select_channel_index(index);
        self.showing_open_channel = false;
        self.presentation
            .set_workspace_focus(FocusSurface::Channels);
        self.load_selected_channel().await?;
        if double_click {
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
        }
        Ok(())
    }

    /// A bracketed paste: always one piece of text, never keys
    /// (`ui::app_pane::paste_target`).
    async fn handle_terminal_paste(&mut self, text: String) -> Result<()> {
        match self.paste_target() {
            // The form of `:agent new` takes a paste into the answer in focus.
            crate::ui::app_pane::PasteTarget::AgentForm => {
                if let (Ok(text), Some(form)) = (sanitize_pasted_text(&text), &mut self.agent_form)
                {
                    form.insert(&text);
                }
                return Ok(());
            }
            // A paste never answers a question: it only adds to the reason
            // of a refusal that is being written.
            crate::ui::app_pane::PasteTarget::ApprovalReason => {
                if let Ok(text) = sanitize_pasted_text(&text) {
                    if self.presentation.overlay == Some(Overlay::AppApproval) {
                        #[cfg(feature = "apps")]
                        self.apps.review.type_reason(&text);
                    } else {
                        self.approval.type_reason(&text);
                    }
                }
                return Ok(());
            }
            crate::ui::app_pane::PasteTarget::App => {
                self.paste_to_app(&text);
                return Ok(());
            }
            crate::ui::app_pane::PasteTarget::Dropped => return Ok(()),
            crate::ui::app_pane::PasteTarget::Composer => {}
        }
        match sanitize_pasted_text(&text) {
            Ok(text) if text.is_empty() => {}
            Ok(text) => {
                self.composer.insert_text(&text);
                self.note_dropped_mention();
                self.refresh_mention_picker().await?;
                self.persist_draft().await?;
            }
            Err(rejection) => self.status_error = Some(rejection.status().into()),
        }
        Ok(())
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        if self.global_key(key) {
            return Ok(());
        }
        // The first screen's own keys, while no community exists (v0.17).
        if self.config.communities.is_empty() && self.presentation.overlay.is_none() {
            match self.first_screen.key(key) {
                crate::ui::first_screen::Outcome::OpenDemo => {
                    self.open_demo = true;
                    self.should_quit = true;
                    return Ok(());
                }
                crate::ui::first_screen::Outcome::Handled => return Ok(()),
                crate::ui::first_screen::Outcome::Ignored => {}
            }
        }
        // Ctrl+Y when what waits is a version of an app (v0.16 M4).
        if self.app_review_key(key).await? {
            return Ok(());
        }
        // While the app has the focus every key is its own but Ctrl+C,
        // which leaves it. An overlay and the writing dock come first
        // (`ui::app_pane::key_owner`).
        if self.app_key(key) {
            return Ok(());
        }
        match self.presentation.overlay {
            Some(Overlay::Help | Overlay::WhichKey | Overlay::Actions) => {
                if self.presentation.route == Route::Inbox {
                    self.handle_inbox_route_key(key, guard, terminal).await?
                } else {
                    self.handle_workspace_key(key, guard, terminal).await?
                }
            }
            Some(Overlay::Confirmation) => match self.presentation.confirmation {
                Some(ConfirmationKind::Quit) => match key.code {
                    KeyCode::Char('y' | 'Y') => self.should_quit = true,
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.presentation.close_overlay();
                    }
                    _ => {}
                },
                Some(ConfirmationKind::Delete) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        self.delete_selected();
                        self.presentation.close_overlay();
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N') => self.presentation.close_overlay(),
                    _ => {}
                },
                Some(
                    ref kind @ (ConfirmationKind::HuddleStart
                    | ConfirmationKind::HuddleJoin
                    | ConfirmationKind::HuddleEnd),
                ) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        let kind = kind.clone();
                        self.presentation.return_to_overlay(Overlay::Huddle);
                        match kind {
                            ConfirmationKind::HuddleStart => self.spawn_huddle_start(),
                            ConfirmationKind::HuddleJoin => {
                                self.stop_mic_check();
                                if let Some(active) = self.active_huddle().cloned()
                                    && let Some(parent) =
                                        self.current_channel().map(|channel| channel.id)
                                {
                                    self.spawn_huddle_join(parent, active.backing);
                                }
                            }
                            _ => self.end_huddle().await,
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.stop_mic_check();
                        self.presentation.return_to_overlay(Overlay::Huddle);
                    }
                    _ => {}
                },
                Some(ConfirmationKind::ArtifactDelete) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        if let Some(id) = self.artifact_panel.selected {
                            self.spawn_artifact_change(
                                id,
                                crate::protocol::artifact::Change::Delete,
                            );
                        }
                        self.presentation.return_to_overlay(Overlay::Artifacts);
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.presentation.return_to_overlay(Overlay::Artifacts);
                    }
                    _ => {}
                },
                Some(ConfirmationKind::ArtifactMove) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        if let Some((id, destination)) = self.pending_artifact_move.take() {
                            self.spawn_artifact_change(
                                id,
                                crate::protocol::artifact::Change::Move(destination),
                            );
                        }
                        self.presentation.return_to_overlay(Overlay::Artifacts);
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.pending_artifact_move = None;
                        self.presentation.return_to_overlay(Overlay::Artifacts);
                    }
                    _ => {}
                },
                Some(ConfirmationKind::Report) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        self.send_pending_report();
                        self.presentation.close_overlay();
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.pending_report = None;
                        self.presentation.close_overlay();
                    }
                    _ => {}
                },
                Some(ConfirmationKind::InboxRead) => match key.code {
                    KeyCode::Char('y' | 'Y') => self.mark_pending_inbox_read().await?,
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.pending_inbox_read.clear();
                        self.presentation.close_overlay();
                    }
                    _ => {}
                },
                Some(ConfirmationKind::ClearDraft) => match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        self.clear_composer_draft().await?;
                        self.presentation.close_overlay();
                    }
                    KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => {
                        self.presentation.close_overlay();
                    }
                    _ => {}
                },
                None => self.presentation.close_overlay(),
            },
            Some(Overlay::Finder) => self.text_overlay_key(key, true).await?,
            Some(Overlay::Command) => self.text_overlay_key(key, false).await?,
            Some(Overlay::Theme) => self.theme_picker_key(key),
            Some(Overlay::Reaction) if self.reaction_query.is_some() => match key.code {
                KeyCode::Esc => {
                    self.reaction_query = None;
                    self.reaction_candidates.clear();
                }
                KeyCode::Backspace => {
                    if let Some(query) = self.reaction_query.as_mut() {
                        query.pop();
                    }
                    self.refresh_reaction_candidates();
                }
                KeyCode::Char(character)
                    if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') =>
                {
                    if let Some(query) = self.reaction_query.as_mut()
                        && query.len() < 64
                    {
                        query.push(character.to_ascii_lowercase());
                    }
                    self.refresh_reaction_candidates();
                }
                KeyCode::Down | KeyCode::Tab => {
                    if !self.reaction_candidates.is_empty() {
                        self.reaction_candidate =
                            (self.reaction_candidate + 1) % self.reaction_candidates.len();
                    }
                }
                KeyCode::Up | KeyCode::BackTab => {
                    if !self.reaction_candidates.is_empty() {
                        self.reaction_candidate = self
                            .reaction_candidate
                            .checked_sub(1)
                            .unwrap_or(self.reaction_candidates.len() - 1);
                    }
                }
                KeyCode::Enter => self.send_custom_reaction(),
                _ => {}
            },
            Some(Overlay::Reaction) => match key.code {
                KeyCode::Esc => self.presentation.close_overlay(),
                KeyCode::Char(':') => {
                    self.reaction_query = Some(String::new());
                    self.refresh_reaction_candidates();
                }
                KeyCode::Char(digit @ '1'..='8') => {
                    self.reaction_index = usize::from(digit as u8 - b'1');
                    self.send_reaction();
                }
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
            Some(Overlay::MediaPreview) => self.media_preview_key(key),
            Some(Overlay::Attachment) => {
                let save = self.presentation.attachment_prompt == Some(AttachmentPrompt::Save);
                self.attachment_path_key(key, save).await?
            }
            Some(Overlay::Search) => self.search_key(key).await?,
            Some(Overlay::DmPicker) => self.dm_picker_key(key),
            Some(Overlay::Agents) => self.agent_directory_key(key).await?,
            Some(Overlay::LocalAgents) => self.local_agents_key(key).await?,
            Some(Overlay::AgentNew) => self.agent_form_key(key).await?,
            Some(Overlay::Approval) => self.approval_key(key),
            Some(Overlay::AppApproval) => self.app_review_answer_key(key).await?,
            Some(Overlay::Artifacts) => self.artifact_panel_key(key).await?,
            Some(Overlay::Huddle) => self.huddle_key(key).await?,
            Some(Overlay::Profile) => self.profile_key(key),
            Some(Overlay::Apps) => self.apps_panel_key(key).await?,
            Some(Overlay::Broker) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.presentation.close_overlay(),
                // The owner's one-key pause for every hosted agent.
                KeyCode::Char('p') => {
                    if let Some(broker) = &self.broker {
                        broker.host.set_paused_all(!broker.host.paused_all());
                    }
                }
                KeyCode::Char('j') | KeyCode::Down => self.broker_select(1),
                KeyCode::Char('k') | KeyCode::Up => self.broker_select(-1),
                KeyCode::Char(digit @ '1'..='9') => {
                    self.broker_toggle_action(usize::from(digit as u8 - b'1'))
                }
                KeyCode::Char(' ') => self.broker_toggle_paused(),
                // Opt in or out of owner-signed agent presence (kind 20003).
                KeyCode::Char('o') => {
                    self.config.ui.broker_agent_presence = !self.config.ui.broker_agent_presence;
                    if let Err(error) = self.config.save(&self.paths) {
                        self.status_error = Some(error.to_string());
                    }
                }
                _ => {}
            },
            Some(Overlay::Palette) => self.palette_key(key, guard, terminal).await?,
            None if self.presentation.composer_target.is_some() => {
                self.handle_composer_key(key).await?
            }
            None if self.presentation.route == Route::Inbox => {
                self.handle_inbox_route_key(key, guard, terminal).await?
            }
            None => self.handle_workspace_key(key, guard, terminal).await?,
        }
        Ok(())
    }

    async fn handle_composer_key(&mut self, key: KeyEvent) -> Result<()> {
        let context = InputContext {
            overlay_open: false,
            composer_completion_open: self.mention_picker.is_some(),
            composer_open: true,
            filter_open: false,
            route_scope: KeyScope::Workspace,
        };
        match self.input_router.dispatch(&self.keymap, context, key) {
            InputDispatch::Action(action) => self.composer_router_action(action).await?,
            InputDispatch::Owned(InputOwner::Composer)
            | InputDispatch::Owned(InputOwner::ComposerCompletion) => {
                self.insert_action(map_insert(key)).await?;
            }
            InputDispatch::Pending { .. } | InputDispatch::Owned(_) | InputDispatch::Noop => {}
        }
        Ok(())
    }

    async fn composer_router_action(&mut self, action: UiAction) -> Result<()> {
        match action {
            UiAction::BackOrQuit => self.insert_action(KeyAction::Escape).await?,
            UiAction::Submit => self.insert_action(KeyAction::Submit).await?,
            UiAction::InsertNewline => self.insert_action(KeyAction::Newline).await?,
            UiAction::Complete => self.insert_action(KeyAction::Complete).await?,
            UiAction::DeletePreviousWord => self.composer.delete_previous_word(),
            UiAction::DeleteToStart => self.composer.delete_to_line_start(),
            UiAction::DeleteToEnd => self.composer.delete_to_line_end(),
            UiAction::ClearComposer => {
                if !self.composer.body.is_empty()
                    || !self.composer.attachments.is_empty()
                    || !self.staging_attachments.is_empty()
                {
                    self.presentation
                        .open_confirmation(ConfirmationKind::ClearDraft);
                }
                return Ok(());
            }
            UiAction::PasteClipboard => return self.import_clipboard().await,
            UiAction::AttachFile => {
                self.open_file_picker();
                return Ok(());
            }
            UiAction::AttachPath => {
                self.attachment_input.clear();
                self.presentation
                    .open_attachment_prompt(AttachmentPrompt::Upload);
                return Ok(());
            }
            UiAction::RemoveLastAttachment => return self.remove_last_attachment().await,
            UiAction::RetryAttachments => return self.retry_failed_attachments().await,
            UiAction::MoveWordLeft => self.composer.move_word_left(),
            UiAction::MoveWordRight => self.composer.move_word_right(),
            UiAction::MoveLineStart => self.composer.move_to_line_start(),
            UiAction::MoveLineEnd => self.composer.move_to_line_end(),
            _ => return Ok(()),
        }
        self.refresh_mention_picker().await?;
        self.persist_draft().await
    }

    async fn handle_workspace_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        let context = InputContext {
            overlay_open: self.presentation.overlay.is_some(),
            composer_completion_open: false,
            composer_open: false,
            filter_open: false,
            route_scope: KeyScope::Workspace,
        };
        match self.input_router.dispatch(&self.keymap, context, key) {
            InputDispatch::Action(action) => {
                self.leader_started_at = None;
                if self.presentation.overlay == Some(Overlay::WhichKey) {
                    self.presentation.overlay = None;
                }
                if self.presentation.overlay == Some(Overlay::Actions)
                    && action == UiAction::BackOrQuit
                {
                    self.action_menu = None;
                }
                self.dispatch_workspace_action(action, guard, terminal)
                    .await?;
            }
            InputDispatch::Pending { .. } => {
                self.leader_started_at
                    .get_or_insert_with(tokio::time::Instant::now);
            }
            InputDispatch::Owned(InputOwner::Overlay) => {
                self.handle_presentation_overlay_key(key, guard, terminal)
                    .await?
            }
            InputDispatch::Owned(_) | InputDispatch::Noop => {
                if !self.input_router.sequence_active() {
                    self.leader_started_at = None;
                }
                if !self.input_router.sequence_active()
                    && self.presentation.overlay == Some(Overlay::WhichKey)
                {
                    self.presentation.overlay = None;
                }
            }
        }
        Ok(())
    }

    async fn handle_inbox_route_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        let context = InputContext {
            overlay_open: self.presentation.overlay.is_some(),
            composer_completion_open: false,
            composer_open: false,
            filter_open: false,
            route_scope: KeyScope::Inbox,
        };
        match self.input_router.dispatch(&self.keymap, context, key) {
            InputDispatch::Action(action) => {
                self.leader_started_at = None;
                if self.presentation.overlay == Some(Overlay::WhichKey) {
                    self.presentation.overlay = None;
                }
                self.dispatch_inbox_action(action, guard, terminal).await?;
            }
            InputDispatch::Pending { .. } => {
                self.leader_started_at
                    .get_or_insert_with(tokio::time::Instant::now);
            }
            InputDispatch::Owned(InputOwner::Overlay) => {
                self.handle_presentation_overlay_key(key, guard, terminal)
                    .await?
            }
            InputDispatch::Owned(_) | InputDispatch::Noop => {
                if !self.input_router.sequence_active() {
                    self.leader_started_at = None;
                }
                if !self.input_router.sequence_active()
                    && self.presentation.overlay == Some(Overlay::WhichKey)
                {
                    self.presentation.overlay = None;
                }
            }
        }
        Ok(())
    }

    /// The keys of the help: it scrolls (v0.17 C3).
    fn help_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if !control => self.help_scroll.by(1),
            KeyCode::Char('k') | KeyCode::Up if !control => self.help_scroll.by(-1),
            KeyCode::Char('n') if control => self.help_scroll.by(1),
            KeyCode::Char('p') if control => self.help_scroll.by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.help_scroll.page_by(1),
            KeyCode::Char('d') if control => self.help_scroll.page_by(1),
            KeyCode::PageUp => self.help_scroll.page_by(-1),
            KeyCode::Char('u') if control => self.help_scroll.page_by(-1),
            KeyCode::Char('g') | KeyCode::Home => self.help_scroll.top(),
            KeyCode::Char('G') | KeyCode::End => self.help_scroll.bottom(),
            _ => {}
        }
    }

    async fn handle_presentation_overlay_key(
        &mut self,
        key: KeyEvent,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        match self.presentation.overlay {
            Some(Overlay::Help) if matches!(key.code, KeyCode::Esc | KeyCode::Char('?' | 'q')) => {
                self.presentation.overlay = None;
            }
            // v0.17 C3: the help scrolls.
            Some(Overlay::Help) => self.help_key(key),
            Some(Overlay::Actions) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.presentation.overlay = None;
                    self.action_menu = None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if let Some(menu) = &mut self.action_menu {
                        menu.move_by(1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if let Some(menu) = &mut self.action_menu {
                        menu.move_by(-1);
                    }
                }
                KeyCode::Enter => {
                    let entry = self.action_menu.as_ref().and_then(ActionMenu::selected);
                    self.presentation.overlay = None;
                    self.action_menu = None;
                    if let Some(entry) = entry {
                        if entry.enabled {
                            self.dispatch_route_action(entry.action, guard, terminal)
                                .await?;
                        } else if let Some(reason) = entry.reason {
                            self.status_error = Some(format!("{}: {reason}", entry.label));
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    async fn dispatch_route_action(
        &mut self,
        action: UiAction,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        if self.presentation.route == Route::Inbox {
            self.dispatch_inbox_action(action, guard, terminal).await
        } else {
            self.dispatch_workspace_action(action, guard, terminal)
                .await
        }
    }

    async fn dispatch_inbox_action(
        &mut self,
        action: UiAction,
        _guard: &mut TerminalGuard,
        _terminal: &mut Tui,
    ) -> Result<()> {
        let mut state = InboxWorkspaceState {
            presentation: self.presentation.clone(),
            narrow_layout: self.inbox_state.narrow_layout,
            narrow_detail: self.inbox_state.narrow_detail,
            detail_available: self.inbox_state.selected(&self.inbox_items).is_some(),
        };
        let effect = reduce_inbox(&mut state, action);
        if action == UiAction::OpenHelp {
            self.help_scroll = crate::ui::help::HelpScroll::default();
        }
        self.presentation = state.presentation;
        self.inbox_state.narrow_detail = state.narrow_detail;
        match effect {
            InboxEffect::None => {}
            InboxEffect::MoveSelection(delta) => {
                self.inbox_state.move_by(&self.inbox_items, delta);
                self.spawn_inbox_detail_load();
            }
            InboxEffect::MoveSelectionToEdge { last } => {
                self.inbox_state.move_to_edge(&self.inbox_items, last);
                self.spawn_inbox_detail_load();
            }
            InboxEffect::ScrollList(scroll) => self.scroll_inbox_list(scroll),
            InboxEffect::ScrollDetail(scroll) => self.scroll_inbox_detail(scroll),
            InboxEffect::CycleFilter => {
                self.inbox_state.filter = self.inbox_state.filter.next();
                self.inbox_state.reconcile(&self.inbox_items);
                self.spawn_inbox_detail_load();
            }
            InboxEffect::LoadDetail => self.spawn_inbox_detail_load(),
            InboxEffect::OpenComposer => self.reply_to_inbox().await?,
            InboxEffect::OpenCanonicalContext => self.open_inbox_item().await?,
            InboxEffect::MarkRead => self.mark_selected_inbox_read().await?,
            InboxEffect::MarkUnread => self.toggle_selected_inbox_unread().await?,
            InboxEffect::ConfirmMarkVisibleRead => self.confirm_visible_inbox_read().await?,
            InboxEffect::OpenContextActions => {
                let actions = derive_actions(self.action_context());
                if actions.is_empty() {
                    self.status_error = Some("no actions are available here".into());
                } else {
                    self.action_menu = Some(ActionMenu::new(actions));
                    self.presentation.open_overlay(Overlay::Actions);
                }
            }
            InboxEffect::Refresh => {
                self.cache_dirty = true;
                self.spawn_inbox_load(self.runtime.is_some());
            }
            InboxEffect::RequestQuitConfirmation => {
                self.presentation.open_confirmation(ConfirmationKind::Quit);
            }
            InboxEffect::Unavailable(action) => {
                self.status_error = Some(format!("{} is not available in Inbox", action.label()));
            }
            InboxEffect::OpenPalette => self.open_palette(),
        }
        Ok(())
    }

    fn scroll_inbox_list(&mut self, scroll: ViewportScroll) {
        let amount = match scroll {
            ViewportScroll::Lines(lines) => lines,
            ViewportScroll::HalfPage(direction) => {
                isize::try_from((self.inbox_state.list_viewport.viewport_height / 2).max(1))
                    .unwrap_or(1)
                    .saturating_mul(direction)
            }
        };
        self.inbox_state.scroll_list(amount, &self.inbox_items);
    }

    fn scroll_inbox_detail(&mut self, scroll: ViewportScroll) {
        let amount = match scroll {
            ViewportScroll::Lines(lines) => lines,
            ViewportScroll::HalfPage(direction) => {
                isize::try_from((self.inbox_state.detail_viewport.viewport_height / 2).max(1))
                    .unwrap_or(1)
                    .saturating_mul(direction)
            }
        };
        self.inbox_state.scroll_detail(amount, &self.inbox_messages);
    }

    async fn dispatch_workspace_action(
        &mut self,
        action: UiAction,
        guard: &mut TerminalGuard,
        terminal: &mut Tui,
    ) -> Result<()> {
        let mut state = WorkspaceState::new(
            self.presentation.clone(),
            self.community_cursor,
            self.config.communities.len(),
            self.community_rail,
            self.sidebar,
            self.thread_root.is_some(),
        )
        .with_app(self.shown_app().is_some() && self.apps.placement.is_drawn());
        if matches!(action, UiAction::FocusContext | UiAction::ToggleContext) {
            // The thread was asked for last: it takes the right side.
            self.apps.front = false;
        }
        let effect = reduce_workspace(&mut state, action);
        if action == UiAction::OpenHelp {
            self.help_scroll = crate::ui::help::HelpScroll::default();
        }
        self.presentation = state.presentation;
        self.community_cursor = state.community_cursor;
        self.select_community_index(self.community_cursor);
        self.community_rail = state.communities_visible;
        self.sidebar = state.channels_visible;

        match effect {
            WorkspaceEffect::None => {}
            WorkspaceEffect::RequestQuitConfirmation => {
                self.presentation.open_confirmation(ConfirmationKind::Quit);
            }
            WorkspaceEffect::CloseContext => self.thread_root = None,
            WorkspaceEffect::EnsureContext => {
                self.toggle_thread().await?;
                if self.thread_root.is_some() {
                    self.presentation.set_workspace_focus(FocusSurface::Context);
                }
            }
            WorkspaceEffect::MoveSelection(delta) => self.move_selection(delta),
            WorkspaceEffect::MoveSelectionToEdge { last } => self.move_to_edge(last),
            WorkspaceEffect::ScrollViewport(scroll) => self.scroll_focused_viewport(scroll),
            WorkspaceEffect::ResizeFocusedSidePane(delta) => {
                self.resize_focused_side_pane(delta)?
            }
            WorkspaceEffect::ActivateFocused => self.open_selected().await?,
            WorkspaceEffect::ActivateCommunity(index) => {
                self.switch_community(index, guard, terminal).await?;
            }
            WorkspaceEffect::OpenComposer => self.enter_composer().await?,
            WorkspaceEffect::OpenSearch => self.open_search(),
            WorkspaceEffect::OpenInbox => self.open_inbox(),
            WorkspaceEffect::CycleChannelSort => self.cycle_channel_sort()?,
            WorkspaceEffect::OpenFinder => {
                self.finder.clear();
                self.presentation.open_overlay(Overlay::Finder);
            }
            WorkspaceEffect::OpenContextActions => {
                let actions = derive_actions(self.action_context());
                if actions.is_empty() {
                    self.status_error = Some("no actions are available here".into());
                } else {
                    self.action_menu = Some(ActionMenu::new(actions));
                    self.presentation.open_overlay(Overlay::Actions);
                }
            }
            WorkspaceEffect::Refresh => {
                self.cache_dirty = true;
                if self.runtime.is_some() {
                    self.spawn_inbox_load(true);
                }
            }
            WorkspaceEffect::OpenOptions => self.open_theme_picker(),
            WorkspaceEffect::OpenCommand => {
                self.command.clear();
                self.presentation.open_overlay(Overlay::Command);
            }
            WorkspaceEffect::OpenDmPicker => self.open_dm_picker(None),
            WorkspaceEffect::ToggleThread => self.toggle_thread().await?,
            WorkspaceEffect::ToggleCopySelection => self.toggle_copy_selection(),
            WorkspaceEffect::CopyMessages => self.copy_selected_messages(),
            WorkspaceEffect::OpenMediaPreview => self.open_media_preview(),
            WorkspaceEffect::OpenReaction => {
                if self.runtime.is_none() {
                    self.status_error = Some(
                        "cached read-only mode: restore or unlock the identity, then restart bzz"
                            .into(),
                    );
                } else if let Some(reason) =
                    self.current_channel().and_then(Channel::read_only_reason)
                {
                    self.status_error = Some(reason.notice().into());
                } else if self.selected_message().is_some() {
                    self.presentation.open_overlay(Overlay::Reaction);
                }
            }
            WorkspaceEffect::OpenEdit => self.open_edit().await?,
            WorkspaceEffect::ConfirmDelete => {
                if self.runtime.is_none() {
                    self.status_error = Some(
                        "cached read-only mode: restore or unlock the identity, then restart bzz"
                            .into(),
                    );
                } else if let Some(refusal) = self.context_refusal(UiAction::Delete) {
                    // `d` on another person's message says why not, as the
                    // actions menu does (v0.17 D5).
                    self.status_error = Some(refusal);
                } else if self.selected_message().is_some_and(|message| {
                    self.runtime.as_ref().is_some_and(|runtime| {
                        message.pubkey == runtime.signer.public_key().to_hex()
                    })
                }) && !self.refused_by_signer("delete", &[5])
                {
                    self.presentation
                        .open_confirmation(ConfirmationKind::Delete);
                }
            }
            WorkspaceEffect::MarkUnread => {
                if self.runtime.is_none() {
                    self.status_error = Some(
                        "cached read-only mode: restore or unlock the identity, then restart bzz"
                            .into(),
                    );
                } else if let Some(refusal) = self.context_refusal(UiAction::MarkUnread) {
                    self.status_error = Some(refusal);
                } else {
                    self.mark_unread().await?;
                    let marked = self.current_channel().map(|channel| {
                        format!(
                            "marked {}{} unread",
                            crate::ui::finder::marker(channel),
                            sanitize::single_line(&channel.name)
                        )
                    });
                    if marked.is_some() {
                        self.status_error = marked;
                    }
                }
            }
            WorkspaceEffect::FocusApp => self.focus_app(),
            WorkspaceEffect::ZoomApp => self.zoom_app(),
            WorkspaceEffect::OpenProfile => {
                match self
                    .selected_message()
                    .map(|message| message.pubkey.clone())
                {
                    Some(pubkey) => self.open_profile(pubkey).await?,
                    None => self.status_error = Some("select a message first".into()),
                }
            }
            WorkspaceEffect::Unavailable(action) => {
                self.status_error = Some(format!(
                    "{} is not available in this workspace",
                    action.label()
                ));
            }
            WorkspaceEffect::OpenPalette => self.open_palette(),
        }
        Ok(())
    }

    /// Why the actions menu would not offer `action` here, in its words: the
    /// keys `d` and `u` and the menu answer alike (v0.17 D5).
    fn context_refusal(&self, action: UiAction) -> Option<String> {
        let actions = derive_actions(self.action_context());
        match actions.iter().find(|entry| entry.action == action) {
            Some(entry) if entry.enabled => None,
            Some(entry) => Some(format!(
                "{}: {}",
                entry.label,
                entry.reason.unwrap_or("not available here")
            )),
            None => Some(format!(
                "{}: select a message in the conversation first (3 focuses it)",
                action.label()
            )),
        }
    }

    fn scroll_focused_viewport(&mut self, scroll: ViewportScroll) {
        let apply = |timeline: &mut TimelineState| match scroll {
            ViewportScroll::Lines(lines) => timeline.scroll_by(lines),
            ViewportScroll::HalfPage(direction) => timeline.scroll_half_page(direction),
        };
        match self.presentation.focus {
            FocusSurface::Timeline => apply(&mut self.timeline),
            FocusSurface::Context => apply(&mut self.thread_timeline),
            FocusSurface::Communities => {
                let ids = self.community_ids();
                match scroll {
                    ViewportScroll::Lines(lines) => {
                        self.community_viewport.scroll_by(lines, ids.len())
                    }
                    ViewportScroll::HalfPage(direction) => {
                        let amount =
                            isize::try_from((self.community_viewport.viewport_height / 2).max(1))
                                .unwrap_or(1);
                        self.community_viewport
                            .scroll_by(amount.saturating_mul(direction), ids.len());
                    }
                }
            }
            FocusSurface::Channels => {
                let ids = self.sidebar_row_ids();
                match scroll {
                    ViewportScroll::Lines(lines) => {
                        self.channel_viewport.scroll_by(lines, ids.len())
                    }
                    ViewportScroll::HalfPage(direction) => {
                        let amount =
                            isize::try_from((self.channel_viewport.viewport_height / 2).max(1))
                                .unwrap_or(1);
                        self.channel_viewport
                            .scroll_by(amount.saturating_mul(direction), ids.len());
                    }
                }
            }
            FocusSurface::InboxList | FocusSurface::InboxDetail | FocusSurface::App => {}
        }
    }

    fn toggle_copy_selection(&mut self) {
        let count = match self.presentation.focus {
            FocusSurface::Timeline => self.timeline.toggle_copy_selection(&self.messages),
            FocusSurface::Context => self
                .thread_timeline
                .toggle_copy_selection(&self.thread_messages),
            _ => None,
        };
        self.status_error = Some(match count {
            Some(0) => "message selection cancelled".into(),
            Some(1) => "message selection started · move then y to copy".into(),
            Some(count) => format!("{count} messages selected · y to copy"),
            None => "focus a selected message before starting a copy range".into(),
        });
    }

    fn copy_selected_messages(&mut self) {
        if self.config.ui.clipboard == ClipboardMode::Disabled {
            self.status_error = Some("clipboard is disabled in [ui].clipboard".into());
            return;
        }
        let (messages, indexes, selected_range) = match self.presentation.focus {
            FocusSurface::Timeline => (
                &self.messages,
                self.timeline.copy_indexes(&self.messages),
                self.timeline.copy_anchor.is_some(),
            ),
            FocusSurface::Context => (
                &self.thread_messages,
                self.thread_timeline.copy_indexes(&self.thread_messages),
                self.thread_timeline.copy_anchor.is_some(),
            ),
            _ => {
                self.status_error = Some("focus a selected message before copying".into());
                return;
            }
        };
        let mut selected = indexes
            .into_iter()
            .filter_map(|index| messages.get(index))
            .collect::<Vec<_>>();
        selected.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        let text = selected
            .iter()
            .map(|message| sanitize::text(&message.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.is_empty() {
            self.status_error = Some("selected message has no copyable text".into());
            return;
        }
        match copy_osc52(&text) {
            Ok(bytes) => {
                if selected_range {
                    match self.presentation.focus {
                        FocusSurface::Timeline => self.timeline.copy_anchor = None,
                        FocusSurface::Context => self.thread_timeline.copy_anchor = None,
                        _ => {}
                    }
                }
                self.status_error = Some(format!(
                    "copied {} message(s), {} bytes to clipboard",
                    selected.len(),
                    bytes
                ));
            }
            Err(error) => self.status_error = Some(format!("copy unavailable: {error}")),
        }
    }

    fn cycle_channel_sort(&mut self) -> Result<()> {
        self.config.ui.channel_sort = self.config.ui.channel_sort.next();
        self.config.save(&self.paths)?;
        self.sync_workspace_viewports();
        self.status_error = Some(format!(
            "channel order: {}",
            self.config.ui.channel_sort.label()
        ));
        Ok(())
    }

    fn resize_focused_side_pane(&mut self, direction: isize) -> Result<()> {
        let (width, minimum, maximum, label) = match self.presentation.focus {
            FocusSurface::Channels => (
                &mut self.config.ui.sidebar_width,
                18_u16,
                60_u16,
                "channel pane",
            ),
            FocusSurface::Context => (
                &mut self.config.ui.thread_width,
                30_u16,
                80_u16,
                "context pane",
            ),
            _ => {
                self.status_error =
                    Some("focus the channel or context pane before resizing".into());
                return Ok(());
            }
        };
        let step = 2_i16.saturating_mul(i16::try_from(direction.signum()).unwrap_or_default());
        let next = width.saturating_add_signed(step).clamp(minimum, maximum);
        if next == *width {
            self.status_error = Some(format!("{label} is already at its safe width limit"));
            return Ok(());
        }
        *width = next;
        self.config.save(&self.paths)?;
        self.status_error = Some(format!("{label} width saved as {next}"));
        Ok(())
    }

    fn move_selection(&mut self, delta: isize) {
        match self.presentation.focus {
            FocusSurface::Communities => {
                self.community_cursor = self
                    .community_cursor
                    .saturating_add_signed(delta)
                    .min(self.config.communities.len().saturating_sub(1));
                self.select_community_index(self.community_cursor);
            }
            FocusSurface::Channels => {
                let joined = self.ordered_channel_indexes();
                if !joined.is_empty() {
                    let current = joined
                        .iter()
                        .position(|index| *index == self.selected_channel)
                        .unwrap_or_default();
                    let next = current.saturating_add_signed(delta).min(joined.len() - 1);
                    self.select_channel_index(joined[next]);
                    self.showing_open_channel = false;
                }
            }
            FocusSurface::Timeline => self.timeline.move_by(&self.messages, delta),
            FocusSurface::Context => {
                let at_oldest = self
                    .thread_timeline
                    .selected_index(&self.thread_messages)
                    .is_some_and(|index| index <= 1);
                self.thread_timeline.move_by(&self.thread_messages, delta);
                // Moving above the oldest loaded reply pages older history.
                if delta < 0
                    && at_oldest
                    && let Some((root, Some(next))) = self.thread_window.clone()
                {
                    self.spawn_thread_window(root, Some(next));
                }
            }
            FocusSurface::InboxList | FocusSurface::InboxDetail | FocusSurface::App => {}
        }
    }

    fn active_huddle(&self) -> Option<&crate::protocol::huddle::ActiveHuddle> {
        let channel = self.current_channel().map(|channel| channel.id)?;
        self.huddle_view
            .as_ref()
            .filter(|(id, _)| *id == channel)
            .and_then(|(_, active)| active.as_ref())
    }

    fn huddle_call_state(&self) -> Option<bool> {
        #[cfg(feature = "huddles-audio")]
        {
            self.huddle_call.as_ref().map(|call| call.call.is_muted())
        }
        #[cfg(not(feature = "huddles-audio"))]
        {
            None
        }
    }

    /// SEAM (agent `demo`, plan A3): the demo's generated huddle feeds the
    /// visualizer here, with `crate::huddle::synth::Synthetic` behind the same
    /// levels: `set_generated_huddle`. A joined call comes first.
    fn huddle_feed(&self) -> Option<HuddleFeed> {
        let generated = self
            .huddle_viz
            .generated
            .as_ref()
            .map(|generated| HuddleFeed {
                levels: generated.source.levels(),
                people: generated.people.clone(),
                muted: generated.muted,
            })
            // The demo's recording plays only where its huddle is running
            // (`#infra`): nowhere else does the panel or the strip show it.
            .filter(|_| !self.demo || self.active_huddle().is_some());
        #[cfg(feature = "huddles-audio")]
        {
            self.huddle_call
                .as_ref()
                .map(|call| HuddleFeed {
                    levels: call.call.levels.clone(),
                    people: call.call.others(),
                    muted: call.call.is_muted(),
                })
                .or(generated)
        }
        #[cfg(not(feature = "huddles-audio"))]
        {
            generated
        }
    }

    /// Feeds the visualizer from generated voices (`people`: index and
    /// public key of each generated person). For `bzz demo` and tests.
    #[cfg_attr(not(test), allow(dead_code))]
    fn set_generated_huddle(
        &mut self,
        source: crate::huddle::synth::Synthetic,
        people: Vec<(u8, String)>,
        muted: bool,
    ) {
        self.huddle_viz.generated = Some(GeneratedHuddle {
            source,
            people,
            muted,
        });
    }

    /// Whether the status line shows the strip now: a call is on, the panel
    /// is closed, and no app or agent line takes the activity slot.
    fn huddle_strip_shown(&self) -> bool {
        self.huddle_viz.on
            && self.presentation.overlay != Some(Overlay::Huddle)
            && self.app_status_line().is_none()
            && self.visible_agent_typing_line(u16::MAX).is_none()
    }

    /// Reads the call's levels into the meter, once a frame tick, and says
    /// whether the visualizer needs a frame (plan rule 20: none while hidden,
    /// none after 2 s of silence, none with motion off unless the speaker
    /// changed).
    fn tick_huddle_viz(&mut self) -> bool {
        let now = self.huddle_viz.epoch.elapsed().as_secs_f64();
        self.tick_huddle_viz_at(now)
    }

    fn tick_huddle_viz_at(&mut self, now: f64) -> bool {
        let mic_check = self.tick_mic_check(now);
        mic_check | self.tick_call_viz(now)
    }

    fn tick_call_viz(&mut self, now: f64) -> bool {
        if let Some(generated) = &mut self.huddle_viz.generated {
            generated.source.advance_to(now);
        }
        let Some(feed) = self.huddle_feed() else {
            if self.huddle_viz.on {
                self.huddle_viz.meter.clear();
                self.huddle_viz.people.clear();
                self.huddle_viz.on = false;
                self.huddle_viz.zoomed = false;
                self.huddle_viz.picker = None;
                self.huddle_viz.spectrum.clear();
                self.huddle_viz.throttle.reset();
            }
            return false;
        };
        let indices = feed
            .people
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>();
        self.huddle_viz
            .meter
            .update(feed.levels.as_ref(), &indices, now);
        let people = feed
            .people
            .iter()
            .map(|(index, pubkey)| crate::ui::huddle_lanes::Person {
                index: *index,
                name: self.huddle_person_name(pubkey),
            })
            .collect::<Vec<_>>();
        let people_changed = people.len() != self.huddle_viz.people.len()
            || people
                .iter()
                .zip(&self.huddle_viz.people)
                .any(|(new, old)| new.index != old.index || new.name != old.name);
        self.huddle_viz.people = people;
        let muted_changed = self.huddle_viz.muted != feed.muted;
        self.huddle_viz.muted = feed.muted;
        let started = !self.huddle_viz.on;
        self.huddle_viz.on = true;
        let visible =
            self.presentation.overlay == Some(Overlay::Huddle) || self.huddle_strip_shown();
        let motion = self.config.ui.motion();
        // The spectrum: the audio side hands samples over only while bars or
        // the orb are on screen, and only then are they analysed.
        let style = self.huddle_style();
        let zoomed = self.huddle_zoomed();
        let spectral = zoomed
            && matches!(
                style,
                crate::config::HuddleStyle::Bars | crate::config::HuddleStyle::Orb
            );
        let mix = feed.levels.mix();
        if mix.is_wanted() != spectral {
            mix.want(spectral);
            self.huddle_viz.spectrum.clear();
        }
        // With motion off the picture holds: the spectrum moves only when
        // the speaker changes, with the frame that shows it.
        if spectral {
            let still =
                (motion == crate::config::Motion::Off).then(|| self.huddle_viz.meter.signature());
            if still.is_none() || still != self.huddle_viz.still {
                self.huddle_viz.spectrum.update(mix, now);
                self.huddle_viz.still = still;
            }
        }
        let interval = self.huddle_interval(motion);
        let wants =
            self.huddle_viz
                .pacer
                .wants_frame_every(&self.huddle_viz.meter, visible, interval, now);
        let falling = spectral
            && motion != crate::config::Motion::Off
            && !self.huddle_viz.spectrum.is_still()
            && self.huddle_viz.meter.is_silent();
        wants || falling || (visible && (started || muted_changed || people_changed))
    }

    /// The zoomed view's style: the picker's selection while it is open,
    /// else the saved one.
    fn huddle_style(&self) -> crate::config::HuddleStyle {
        self.huddle_viz
            .picker
            .and_then(|row| crate::config::HuddleStyle::ALL.get(row).copied())
            .unwrap_or(self.config.ui.huddle_style)
    }

    /// Whether the zoomed view is on screen.
    fn huddle_zoomed(&self) -> bool {
        self.huddle_viz.zoomed
            && self.huddle_viz.on
            && self.presentation.overlay == Some(Overlay::Huddle)
    }

    /// The shortest time between two frames of the visualizer: the orb, the
    /// costliest to write, at 20 a second and slower on a slow terminal.
    fn huddle_interval(&self, motion: crate::config::Motion) -> Option<f64> {
        let base = crate::huddle::meter::Pacer::interval(motion)?;
        if self.huddle_zoomed() && self.huddle_style() == crate::config::HuddleStyle::Orb {
            Some(self.huddle_viz.throttle.interval(base.max(1.0 / 20.0)))
        } else {
            Some(base)
        }
    }

    /// How long the last frame took to draw and write; the orb's rate
    /// follows it.
    fn huddle_frame_drawn(&mut self, seconds: f64) {
        if self.huddle_zoomed() && self.huddle_style() == crate::config::HuddleStyle::Orb {
            if let Some(base) = crate::huddle::meter::Pacer::interval(self.config.ui.motion()) {
                self.huddle_viz
                    .throttle
                    .record(seconds, base.max(1.0 / 20.0));
            }
        } else {
            self.huddle_viz.throttle.reset();
        }
    }

    /// The microphone check of the join confirmation: reads its level and
    /// says whether it needs a frame.
    fn tick_mic_check(&mut self, now: f64) -> bool {
        let shown = self.presentation.overlay == Some(Overlay::Confirmation)
            && self.presentation.confirmation == Some(ConfirmationKind::HuddleJoin);
        let motion = self.config.ui.motion();
        if !shown && self.huddle_viz.mic_check.is_some() {
            // The confirmation went away another way: let the devices go.
            self.stop_mic_check();
        }
        let Some(run) = &mut self.huddle_viz.mic_check else {
            return false;
        };
        if let Some(failed) = &mut run.failed
            && let Ok(message) = failed.try_recv()
        {
            run.check.fail(message);
            run.failed = None;
        }
        let changed = run.check.update(now);
        let wants = self
            .huddle_viz
            .mic_pacer
            .wants_frame(run.check.meter(), shown, motion, now);
        shown && (changed || wants)
    }

    /// Opens your microphone for the check, without the call: nothing is
    /// encoded or sent. In tests nothing is opened; they feed the check.
    fn start_mic_check(&mut self) {
        let check =
            crate::huddle::mic_check::MicCheck::new(self.config.ui.huddle_input_device.clone());
        #[cfg_attr(not(all(feature = "huddles-audio", not(test))), allow(unused_mut))]
        let mut run = MicCheckRun {
            check,
            _stop: None,
            failed: None,
            released: None,
        };
        #[cfg(all(feature = "huddles-audio", not(test)))]
        {
            let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
            let (failed_tx, failed_rx) = tokio::sync::oneshot::channel::<String>();
            let (released_tx, released_rx) = tokio::sync::oneshot::channel::<()>();
            let levels = run.check.levels();
            let input = self.config.ui.huddle_input_device.clone();
            let output = self.config.ui.huddle_output_device.clone();
            tokio::spawn(async move {
                let opened = tokio::task::spawn_blocking(move || {
                    crate::huddle::audio::open_devices(input.as_deref(), output.as_deref())
                })
                .await;
                match opened {
                    Ok(Ok((devices, mut capture, _playback))) => {
                        loop {
                            tokio::select! {
                                _ = &mut stop_rx => break,
                                frame = capture.recv() => match frame {
                                    Some(frame) => levels.publish_mic(&frame),
                                    None => break,
                                },
                            }
                        }
                        drop(devices);
                    }
                    Ok(Err(error)) => {
                        let _ = failed_tx.send(error.to_string());
                    }
                    Err(_) => {
                        let _ = failed_tx.send(crate::huddle::audio::MICROPHONE_DENIED.into());
                    }
                }
                let _ = released_tx.send(());
            });
            run._stop = Some(stop_tx);
            run.failed = Some(failed_rx);
            run.released = Some(released_rx);
        }
        self.huddle_viz.mic_pacer = crate::huddle::meter::Pacer::default();
        self.huddle_viz.mic_check = Some(run);
    }

    /// Ends the check and lets its devices go; a join that follows waits
    /// for them.
    fn stop_mic_check(&mut self) {
        if let Some(run) = self.huddle_viz.mic_check.take() {
            self.huddle_viz.mic_released = run.released;
        }
    }

    fn huddle_person_name(&self, pubkey: &str) -> String {
        self.profiles
            .get(pubkey)
            .and_then(|profile| {
                profile
                    .display_name
                    .clone()
                    .or_else(|| profile.name.clone())
            })
            .map(|name| crate::protocol::artifact::display(&name, 32))
            .unwrap_or_else(|| pubkey.chars().take(8).collect())
    }

    fn huddle_lanes(&self) -> Option<crate::ui::huddle_lanes::Lanes<'_>> {
        self.huddle_viz.on.then(|| crate::ui::huddle_lanes::Lanes {
            meter: &self.huddle_viz.meter,
            people: &self.huddle_viz.people,
            muted: self.huddle_viz.muted,
            motion: self.config.ui.motion(),
            look: self.huddle_viz.look,
            recording: self.demo,
        })
    }

    async fn stop_huddle_call(&mut self) {
        self.huddle_ptt_until = None;
        #[cfg(feature = "huddles-audio")]
        if let Some(call) = self.huddle_call.take() {
            call.stop().await;
        }
    }

    /// One live lifecycle subscription for the selected channel, one for the
    /// active huddle's backing channel; push-to-talk expiry; call liveness.
    async fn reconcile_huddle(&mut self) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let supervisor = runtime.supervisor.clone();
        let now = nostr::Timestamp::now().as_secs();
        let desired = self.current_channel().map(|channel| channel.id);
        if desired != self.huddle_subscribed {
            self.huddle_subscribed = desired;
            self.huddle_view = desired.map(|channel| (channel, None));
            self.huddle_reactions.clear();
            self.huddle_guidelines = None;
            match desired {
                Some(channel) => {
                    supervisor
                        .subscribe_with_priority(
                            HUDDLE_LIFECYCLE_SUBSCRIPTION,
                            vec![serde_json::json!({
                                "kinds": crate::protocol::huddle::LIFECYCLE_KINDS,
                                "#h": [channel.to_string()],
                                "since": now,
                            })],
                            SubscriptionPriority::Foreground,
                        )
                        .await?;
                    self.huddle_dirty = true;
                }
                None => supervisor.close(HUDDLE_LIFECYCLE_SUBSCRIPTION).await?,
            }
        }
        if self.huddle_dirty && !self.huddle_loading {
            self.huddle_dirty = false;
            self.spawn_huddle_query();
        }
        let backing = self.active_huddle().map(|active| active.backing);
        if backing != self.huddle_backing_subscribed {
            self.huddle_backing_subscribed = backing;
            self.huddle_reactions.clear();
            self.huddle_guidelines = None;
            match backing {
                Some(backing) => {
                    supervisor
                        .subscribe_with_priority(
                            HUDDLE_BACKING_SUBSCRIPTION,
                            vec![serde_json::json!({
                                "kinds": [crate::protocol::huddle::KIND_REACTION, crate::protocol::huddle::KIND_GUIDELINES],
                                "#h": [backing.to_string()],
                                "since": now.saturating_sub(crate::protocol::huddle::JOINABLE_WINDOW_SECONDS),
                                "limit": 50,
                            })],
                            SubscriptionPriority::Foreground,
                        )
                        .await?;
                }
                None => supervisor.close(HUDDLE_BACKING_SUBSCRIPTION).await?,
            }
        }
        #[cfg(feature = "huddles-audio")]
        {
            if self
                .huddle_ptt_until
                .is_some_and(|until| Instant::now() >= until)
            {
                self.huddle_ptt_until = None;
                if let Some(call) = &self.huddle_call {
                    call.call.set_muted(true);
                }
                self.cache_dirty = true;
            }
            // The relay closed the room or the socket dropped.
            if self
                .huddle_call
                .as_ref()
                .is_some_and(|call| !call.call.is_running())
            {
                self.stop_huddle_call().await;
                self.status_error = Some("left the huddle; audio devices released".into());
            }
        }
        Ok(())
    }

    fn spawn_huddle_query(&mut self) {
        let (Some(runtime), Some(channel)) = (
            &self.runtime,
            self.current_channel().map(|channel| channel.id),
        ) else {
            return;
        };
        self.huddle_loading = true;
        let http = runtime.http.clone();
        let store = self.store.clone();
        let community = runtime.community_id;
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let relay = store
                .call(move |store| store.pinned_relay_pubkey(community))
                .await
                .ok()
                .flatten();
            let events = http
                .query_raw(&[crate::protocol::huddle::lifecycle_filter(channel)])
                .await
                .unwrap_or_default();
            let active = relay.and_then(|relay| {
                crate::protocol::huddle::active(&events, &relay, nostr::Timestamp::now().as_secs())
            });
            let _ = tx
                .send(Background::HuddleState {
                    community,
                    channel,
                    active,
                })
                .await;
        });
    }

    /// Lifecycle events re-replay the channel; reactions and the creator's
    /// guidelines come from the backing channel. Nothing reaches the store.
    fn apply_huddle_event(&mut self, event: &nostr::Event) -> bool {
        let kind = event.kind.as_u16();
        if crate::protocol::huddle::LIFECYCLE_KINDS.contains(&kind) {
            let channel = self.current_channel().map(|channel| channel.id.to_string());
            if crate::protocol::events::first_tag(event, "h") == channel {
                self.huddle_dirty = true;
            }
            return false;
        }
        let Some(active) = self.active_huddle().cloned() else {
            return false;
        };
        if kind == crate::protocol::huddle::KIND_REACTION {
            if let Some((sender, emoji)) =
                crate::protocol::huddle::parse_reaction(event, active.backing)
            {
                self.huddle_reactions.push_back((sender, emoji));
                while self.huddle_reactions.len() > 20 {
                    self.huddle_reactions.pop_front();
                }
                return true;
            }
        } else if kind == crate::protocol::huddle::KIND_GUIDELINES
            && event.pubkey.to_hex() == active.creator
            && crate::protocol::events::first_tag(event, "h").as_deref()
                == Some(active.backing.to_string().as_str())
            && crate::protocol::events::verify(event).is_ok()
        {
            self.huddle_guidelines = Some(crate::protocol::artifact::display(&event.content, 2000));
            return true;
        }
        false
    }

    fn huddle_panel_view(&self) -> crate::ui::huddle::HuddleView {
        let name = |pubkey: &str| {
            self.profiles
                .get(pubkey)
                .and_then(|profile| {
                    profile
                        .display_name
                        .clone()
                        .or_else(|| profile.name.clone())
                })
                .unwrap_or_else(|| pubkey.chars().take(8).collect())
        };
        let active = self.active_huddle();
        crate::ui::huddle::HuddleView {
            channel: self
                .current_channel()
                .map(|channel| channel.name.clone())
                .unwrap_or_default(),
            roster: active.map(|active| {
                active
                    .participants
                    .iter()
                    .map(|pubkey| name(pubkey))
                    .collect()
            }),
            started_minutes_ago: active.map_or(0, |active| {
                nostr::Timestamp::now()
                    .as_secs()
                    .saturating_sub(active.started_at)
                    / 60
            }),
            guidelines: self.huddle_guidelines.clone(),
            reactions: self
                .huddle_reactions
                .iter()
                .map(|(sender, emoji)| format!("{emoji} {}", name(sender)))
                .collect(),
            call: self.huddle_call_state(),
            joining: self.huddle_joining,
            audio_available: crate::huddle::AUDIO,
            devices: self.huddle_devices.clone(),
            you_created: active
                .is_some_and(|active| Some(active.creator.as_str()) == self.self_pubkey()),
            recording: self.demo && active.is_some(),
        }
    }

    async fn huddle_key(&mut self, key: KeyEvent) -> Result<()> {
        // The style picker: moving previews, Enter keeps, Esc puts back.
        if let Some(row) = self.huddle_viz.picker {
            let count = crate::config::HuddleStyle::ALL.len();
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.huddle_viz.picker = Some((row + count - 1) % count);
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    self.huddle_viz.picker = Some((row + 1) % count);
                }
                KeyCode::Enter => {
                    let style = self.huddle_style();
                    self.huddle_viz.picker = None;
                    self.save_huddle_style(style);
                }
                KeyCode::Esc | KeyCode::Char('q' | 'v') => self.huddle_viz.picker = None,
                _ => {}
            }
            return Ok(());
        }
        if self.huddle_zoomed() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q' | 'z') => {
                    self.huddle_viz.zoomed = false;
                    return Ok(());
                }
                KeyCode::Char('v') => {
                    self.open_huddle_picker();
                    return Ok(());
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('z') | KeyCode::Char('v') if self.huddle_viz.on => {
                self.huddle_viz.zoomed = true;
                if key.code == KeyCode::Char('v') {
                    self.open_huddle_picker();
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => self.presentation.close_overlay(),
            KeyCode::Char('s') => return self.huddle_command("start").await,
            KeyCode::Char('j') => return self.huddle_command("join").await,
            KeyCode::Char('l') => return self.huddle_command("leave").await,
            KeyCode::Char('e') => return self.huddle_command("end").await,
            KeyCode::Char('m') => return self.huddle_command("mute").await,
            // Push-to-talk: each press (and key repeat) keeps you live 600 ms.
            #[cfg(feature = "huddles-audio")]
            KeyCode::Char(' ') => {
                if let Some(call) = &self.huddle_call {
                    call.call.set_muted(false);
                    self.huddle_ptt_until = Some(Instant::now() + Duration::from_millis(600));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Opens the zoomed view's style picker on the style in use.
    fn open_huddle_picker(&mut self) {
        let current = self.huddle_style();
        self.huddle_viz.picker = crate::config::HuddleStyle::ALL
            .iter()
            .position(|style| *style == current);
    }

    fn save_huddle_style(&mut self, style: crate::config::HuddleStyle) {
        self.config.ui.huddle_style = style;
        self.status_error = Some(match self.config.save(&self.paths) {
            Ok(()) => format!("huddle style: {}", style.name()),
            Err(error) => error.to_string(),
        });
    }

    /// `:huddle zoom` and `:huddle style [name]` (plan v0.17.0 B5). They
    /// need no channel: a generated huddle has none.
    fn huddle_view_command(&mut self, verb: &str, rest: &str) {
        match (verb, rest) {
            ("zoom", _) if !self.huddle_viz.on => {
                self.status_error =
                    Some("the zoomed view shows a call: join the huddle first".into());
            }
            ("zoom", _) => {
                let zoomed = self.huddle_zoomed();
                self.huddle_viz.picker = None;
                self.huddle_viz.zoomed = !zoomed;
                self.presentation.open_overlay(Overlay::Huddle);
            }
            (_, "") if self.huddle_viz.on => {
                self.huddle_viz.zoomed = true;
                self.presentation.open_overlay(Overlay::Huddle);
                self.open_huddle_picker();
            }
            (_, "") => {
                self.status_error = Some(format!(
                    "huddle style: {} (lanes, bars or orb: :huddle style <name>)",
                    self.config.ui.huddle_style.name()
                ));
            }
            (_, name) => match crate::config::HuddleStyle::from_name(name) {
                Some(style) => {
                    self.huddle_viz.picker = None;
                    self.save_huddle_style(style);
                    if self.huddle_viz.on {
                        self.huddle_viz.zoomed = true;
                        self.presentation.open_overlay(Overlay::Huddle);
                    }
                }
                None => {
                    self.status_error = Some(format!(
                        "unknown huddle style {:?}: lanes, bars or orb",
                        crate::protocol::artifact::display(name, 24)
                    ));
                }
            },
        }
    }

    async fn huddle_command(&mut self, arguments: &str) -> Result<()> {
        // The panel's keys come here too: the demo answers before any
        // confirmation that would lead to a device.
        if self.demo
            && let Some(answer) =
                crate::demo::refuses(&crate::ui::command::Command::Huddle(arguments.into()))
        {
            self.status_error = Some(answer.into());
            return Ok(());
        }
        let (verb, rest) = arguments.split_once(' ').unwrap_or((arguments, ""));
        let rest = rest.trim();
        if matches!(verb, "zoom" | "style") {
            self.huddle_view_command(verb, rest);
            return Ok(());
        }
        if self.runtime.is_none() || self.current_channel().is_none() {
            self.status_error = Some("select a channel in a connected community".into());
            return Ok(());
        }
        let active = self.active_huddle().cloned();
        let in_call = self.huddle_call_state().is_some();
        match verb {
            "" => {
                self.huddle_dirty = true;
                self.presentation.open_overlay(Overlay::Huddle);
            }
            "start" if active.is_some() => {
                self.status_error = Some("a huddle is already running here; join it".into());
            }
            "start"
                if self.refused_by_signer(
                    "start a huddle",
                    &[
                        9007,
                        crate::protocol::huddle::KIND_STARTED,
                        crate::protocol::huddle::KIND_ENDED,
                    ],
                ) => {}
            "start" => {
                self.presentation.open_overlay(Overlay::Huddle);
                self.presentation
                    .open_confirmation(ConfirmationKind::HuddleStart);
            }
            "join" if !crate::huddle::AUDIO => {
                self.status_error = Some(crate::huddle::VIEW_ONLY.into())
            }
            "join" if active.is_none() => {
                self.status_error = Some("no huddle is running here".into())
            }
            "join" if in_call || self.huddle_joining => {}
            "join" => {
                self.presentation.open_overlay(Overlay::Huddle);
                self.presentation
                    .open_confirmation(ConfirmationKind::HuddleJoin);
                self.start_mic_check();
            }
            "leave" => self.leave_huddle().await,
            "end"
                if self.refused_by_signer(
                    "end the huddle",
                    &[crate::protocol::huddle::KIND_ENDED],
                ) => {}
            "end"
                if active
                    .as_ref()
                    .is_some_and(|active| Some(active.creator.as_str()) == self.self_pubkey()) =>
            {
                self.presentation.open_overlay(Overlay::Huddle);
                self.presentation
                    .open_confirmation(ConfirmationKind::HuddleEnd);
            }
            "end" => self.status_error = Some("only the huddle's creator can end it".into()),
            "react"
                if self.refused_by_signer(
                    "huddle reactions",
                    &[crate::protocol::huddle::KIND_REACTION],
                ) => {}
            "mute" => {
                #[cfg(feature = "huddles-audio")]
                if let Some(call) = &self.huddle_call {
                    call.call.set_muted(!call.call.is_muted());
                    self.huddle_ptt_until = None;
                }
                if !in_call {
                    self.status_error = Some("you are not in a huddle call".into());
                }
            }
            "react" if !rest.is_empty() => {
                if let (Some(active), Some(runtime)) = (active, &self.runtime) {
                    let name = self
                        .self_pubkey()
                        .and_then(|pubkey| self.profiles.get(pubkey))
                        .and_then(|profile| {
                            profile
                                .display_name
                                .clone()
                                .or_else(|| profile.name.clone())
                        })
                        .unwrap_or_default();
                    match crate::protocol::huddle::reaction(active.backing, rest, &name) {
                        Ok(builder) => {
                            let signer = runtime.signer.clone();
                            let supervisor = runtime.supervisor.clone();
                            tokio::spawn(async move {
                                if let Ok(event) = signer.sign(builder).await {
                                    let _ = supervisor.publish(event).await;
                                }
                            });
                        }
                        Err(error) => self.status_error = Some(error.to_string()),
                    }
                }
            }
            "devices" => {
                #[cfg(feature = "huddles-audio")]
                {
                    self.huddle_devices = Some(crate::huddle::audio::device_names());
                }
                self.presentation.open_overlay(Overlay::Huddle);
            }
            "input" | "output" => {
                let value = (!rest.is_empty()).then(|| rest.to_owned());
                if verb == "input" {
                    self.config.ui.huddle_input_device = value;
                } else {
                    self.config.ui.huddle_output_device = value;
                }
                if let Err(error) = self.config.save(&self.paths) {
                    self.status_error = Some(error.to_string());
                } else {
                    self.status_error = Some(format!(
                        "huddle {verb} device saved; it applies on the next join"
                    ));
                }
            }
            _ => {
                self.status_error = Some(
                    "usage: :huddle [start|join|leave|end|mute|react <emoji>|devices|input <name>|output <name>|zoom|style [lanes|bars|orb]]".into(),
                );
            }
        }
        Ok(())
    }

    /// Start (after confirmation): backing channel, then 48100; then join.
    fn spawn_huddle_start(&mut self) {
        if self.demo {
            self.status_error = Some(crate::demo::HUDDLE_IS_A_RECORDING.into());
            return;
        }
        let (Some(runtime), Some(parent)) = (
            &self.runtime,
            self.current_channel().map(|channel| channel.id),
        ) else {
            return;
        };
        let signer = runtime.signer.clone();
        let supervisor = runtime.supervisor.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let backing = Uuid::new_v4();
            let steps = async {
                for builder in [
                    crate::protocol::huddle::backing_channel(backing)?,
                    crate::protocol::huddle::started(parent, backing)?,
                ] {
                    let ack = supervisor.publish(signer.sign(builder).await?).await?;
                    if !ack.accepted {
                        return Err(Error::Protocol(format!(
                            "the relay refused the huddle: {}",
                            crate::protocol::artifact::display(&ack.message, 120)
                        )));
                    }
                }
                Ok::<_, Error>(())
            };
            let message = match steps.await {
                Ok(()) => Background::HuddleStarted { parent, backing },
                Err(error) => {
                    // Roll back the backing channel, as desktop does.
                    if let Ok(builder) = crate::protocol::huddle::archive(backing)
                        && let Ok(event) = signer.sign(builder).await
                    {
                        let _ = supervisor.publish(event).await;
                    }
                    Background::Failed(error.to_string())
                }
            };
            let _ = tx.send(message).await;
        });
    }

    #[cfg_attr(not(feature = "huddles-audio"), allow(unused_variables))]
    fn spawn_huddle_join(&mut self, parent: Uuid, backing: Uuid) {
        if self.demo {
            self.status_error = Some(crate::demo::HUDDLE_IS_A_RECORDING.into());
            return;
        }
        #[cfg(not(feature = "huddles-audio"))]
        {
            self.status_error = Some(crate::huddle::VIEW_ONLY.into());
        }
        #[cfg(feature = "huddles-audio")]
        {
            let Some(runtime) = &self.runtime else { return };
            let Some(entry) = self
                .config
                .communities
                .iter()
                .find(|entry| entry.id == runtime.community_id)
            else {
                return;
            };
            let Ok(endpoint) = validate_relay_url(&entry.relay_url, entry.allow_insecure_localhost)
            else {
                return;
            };
            self.huddle_joining = true;
            let signer = runtime.signer.clone();
            let input = self.config.ui.huddle_input_device.clone();
            let output = self.config.ui.huddle_output_device.clone();
            let tx = self.background_tx.clone();
            let released = self.huddle_viz.mic_released.take();
            tokio::spawn(async move {
                // The microphone check lets its devices go first.
                if let Some(released) = released {
                    let _ = tokio::time::timeout(Duration::from_secs(2), released).await;
                }
                let joined = async {
                    let (devices, capture, playback) = tokio::task::spawn_blocking(move || {
                        crate::huddle::audio::open_devices(input.as_deref(), output.as_deref())
                    })
                    .await
                    .map_err(|_| {
                        Error::Unsupported(crate::huddle::audio::MICROPHONE_DENIED.into())
                    })??;
                    let joined =
                        crate::huddle::audio::join(&endpoint.websocket, backing, parent, &signer)
                            .await?;
                    let call = crate::huddle::audio::start(joined, capture, playback)?;
                    Ok::<_, Error>(Box::new(HuddleCall {
                        call,
                        _devices: devices,
                        backing,
                        parent,
                    }))
                };
                let _ = tx
                    .send(Background::HuddleJoined(
                        joined.await.map_err(|error| error.to_string()),
                    ))
                    .await;
            });
        }
    }

    /// Leave: if other humans remain, leave the backing channel; the last
    /// human out who created it ends it, as desktop does.
    async fn leave_huddle(&mut self) {
        let active = self.active_huddle().cloned();
        self.stop_huddle_call().await;
        let (Some(active), Some(runtime)) = (active, &self.runtime) else {
            return;
        };
        let me = runtime.signer.public_key().to_hex();
        let others = active
            .participants
            .iter()
            .filter(|pubkey| **pubkey != me)
            .count();
        if others == 0 && active.creator == me {
            self.end_huddle().await;
            return;
        }
        if let Some(reason) = runtime.signer.refusal(9022) {
            self.status_error = Some(format!(
                "left the call; audio devices released. Leaving the huddle channel: {reason}"
            ));
            self.huddle_dirty = true;
            return;
        }
        if let Ok(builder) = crate::protocol::huddle::leave(active.backing) {
            let signer = runtime.signer.clone();
            let supervisor = runtime.supervisor.clone();
            tokio::spawn(async move {
                if let Ok(event) = signer.sign(builder).await {
                    let _ = supervisor.publish(event).await;
                }
            });
        }
        self.status_error = Some("left the huddle; audio devices released".into());
        self.huddle_dirty = true;
    }

    async fn end_huddle(&mut self) {
        let active = self.active_huddle().cloned();
        let parent = self.current_channel().map(|channel| channel.id);
        self.stop_huddle_call().await;
        let (Some(active), Some(parent), Some(runtime)) = (active, parent, &self.runtime) else {
            return;
        };
        if let Some(reason) = runtime.signer.refusal(crate::protocol::huddle::KIND_ENDED) {
            self.status_error = Some(format!(
                "left the call; audio devices released. Ending the huddle: {reason}"
            ));
            self.huddle_dirty = true;
            return;
        }
        let signer = runtime.signer.clone();
        let supervisor = runtime.supervisor.clone();
        tokio::spawn(async move {
            for builder in [
                crate::protocol::huddle::ended(parent, active.backing),
                crate::protocol::huddle::archive(active.backing),
            ]
            .into_iter()
            .flatten()
            {
                if let Ok(event) = signer.sign(builder).await {
                    let _ = supervisor.publish(event).await;
                }
            }
        });
        self.status_error = Some("huddle ended".into());
        self.huddle_dirty = true;
    }

    /// Serves the broker for this community's hosted agents while the owner
    /// runtime is unlocked; stops it when the runtime goes away or changes.
    async fn reconcile_broker(&mut self) {
        let Some(runtime) = &self.runtime else {
            if let Some(broker) = self.broker.take() {
                broker.stop().await;
            }
            self.broker_attempted = None;
            return;
        };
        let community = runtime.community_id;
        if self
            .broker
            .as_ref()
            .is_some_and(|broker| broker.community == community)
            || self.broker_attempted == Some(community)
        {
            return;
        }
        if let Some(broker) = self.broker.take() {
            broker.stop().await;
        }
        self.broker_attempted = Some(community);
        let agents = self
            .config
            .hosted_agents
            .iter()
            .filter(|agent| {
                // A local agent's process is bzz's own: it gets its tools
                // over ACP, never a long-lived broker credential.
                agent.local.is_none()
                    && agent.community == community
                    && agent.owner_identity == runtime.identity_id
            })
            .cloned()
            .collect::<Vec<_>>();
        let Some(entry) = self
            .config
            .communities
            .iter()
            .find(|entry| entry.id == community)
        else {
            return;
        };
        if agents.is_empty() {
            return;
        }
        let Ok(endpoint) = validate_relay_url(&entry.relay_url, entry.allow_insecure_localhost)
        else {
            return;
        };
        let keys_dir = self.paths.keys_dir();
        let mut runtimes = HashMap::new();
        let mut hosted = Vec::new();
        let mut signers = Vec::new();
        let mut supervisors = Vec::new();
        for agent in &agents {
            let built = async {
                let signer =
                    crate::broker::custody::unlock(&keys_dir, &runtime.signer, agent).await?;
                let auth_tag = crate::broker::relay::auth_tag(&agent.auth_tag)?;
                // NIP-FI communities: the agent proves itself with its own
                // owner-authorized grant, never with the owner's session.
                let evidence = crate::auth::federated::EvidenceSlot::default();
                if runtime.evidence.is_set()
                    && let Some(adapter) = &runtime.identity_adapter
                {
                    match crate::auth::federated::agent_evidence(
                        adapter,
                        &runtime.data_dir,
                        runtime.identity_id,
                        &runtime.signer,
                        &signer,
                        &agent.auth_tag,
                    )
                    .await?
                    {
                        Some(source) => evidence.set(source),
                        None => {
                            return Err(Error::Auth(
                                "sign in (:signin) to enroll agents with the identity adapter"
                                    .into(),
                            ));
                        }
                    }
                }
                let http = HttpClient::new(endpoint.http_base.clone(), signer.clone())?
                    .with_evidence(evidence.clone());
                let supervisor = SupervisorHandle::spawn_with_evidence(
                    endpoint.websocket.clone(),
                    signer.clone(),
                    self.diagnostics.clone(),
                    evidence,
                );
                Ok::<_, Error>(crate::broker::relay::AgentRuntime {
                    signer,
                    http,
                    supervisor,
                    owner: runtime.signer.public_key(),
                    auth_tag,
                })
            }
            .await;
            match built {
                Ok(agent_runtime) => {
                    signers.push(agent_runtime.signer.clone());
                    supervisors.push(agent_runtime.supervisor.clone());
                    runtimes.insert(agent.id, agent_runtime);
                    hosted.push(crate::broker::custody::hosted(agent));
                }
                Err(error) => {
                    self.status_error = Some(format!("agent {} unavailable: {error}", agent.label));
                }
            }
        }
        let executor =
            crate::broker::relay::RelayExecutor::new(community, self.store.clone(), runtimes);
        let host = crate::broker::host::Host::new(std::sync::Arc::new(executor), hosted);
        let port_file = self.paths.data_dir.join("broker.port");
        match crate::broker::http::serve(host.clone(), Some(&port_file)).await {
            Ok(server) => {
                self.broker = Some(BrokerRuntime {
                    community,
                    host,
                    server,
                    signers,
                    supervisors,
                });
            }
            Err(error) => self.status_error = Some(format!("agent broker unavailable: {error}")),
        }
    }

    /// Runs this community's local agents while the owner runtime is
    /// unlocked; stops them when the runtime goes away or changes. Leftovers
    /// of an earlier bzz are stopped, and open requests settled, before any
    /// agent starts (ADR v0.15 local agents).
    async fn reconcile_local_agents(&mut self) {
        let Some(runtime) = &self.runtime else {
            if let Some(agents) = self.local_agents.take() {
                agents.stop().await;
            }
            self.local_agents_attempted = None;
            return;
        };
        let community = runtime.community_id;
        let dirty = std::mem::take(&mut self.local_agents_dirty);
        if dirty && self.local_agents.is_none() {
            // The first local agent of this community was just created.
            self.local_agents_attempted = None;
        }
        if dirty && let Some(agents) = &self.local_agents {
            // Pause and resume apply to a running agent at once. One that
            // was never started needs the runtime started again.
            let mut restart = false;
            for agent in self.config.hosted_agents.iter().filter(|agent| {
                agent.local.is_some()
                    && agent.community == community
                    && agent.owner_identity == runtime.identity_id
            }) {
                if agents.host.runs(agent.id) {
                    agents
                        .host
                        .set_paused(agent.id, !agent.runs_locally())
                        .await;
                } else {
                    restart |= agent.runs_locally();
                }
            }
            if restart {
                if let Some(agents) = self.local_agents.take() {
                    agents.stop().await;
                }
                self.local_agents_attempted = None;
            }
        }
        if self
            .local_agents
            .as_ref()
            .is_some_and(|agents| agents.community == community)
            || self.local_agents_attempted == Some(community)
        {
            return;
        }
        if let Some(agents) = self.local_agents.take() {
            agents.stop().await;
        }
        self.local_agents_attempted = Some(community);
        let Some(entry) = self
            .config
            .communities
            .iter()
            .find(|entry| entry.id == community)
        else {
            return;
        };
        let federated = runtime
            .evidence
            .is_set()
            .then(|| runtime.identity_adapter.clone())
            .flatten();
        let served =
            crate::agents::runtime::service::serve(crate::agents::runtime::service::ServeOptions {
                paths: &self.paths,
                store: self.store.clone(),
                diagnostics: self.diagnostics.clone(),
                community: entry,
                agents: &self.config.hosted_agents,
                owner: &runtime.signer,
                owner_identity: runtime.identity_id,
                federated,
                limits: crate::agents::runtime::Limits::default(),
                // The owner is here to be asked before a real command runs.
                owner_present: true,
                environment: Some(self.agent_environment.clone()),
            })
            .await;
        match served {
            Ok(Some(agents)) => {
                if let Some(problem) = agents.problems.first() {
                    self.status_error = Some(problem.clone());
                }
                self.local_agents = Some(agents);
            }
            Ok(None) => {}
            Err(error) => self.status_error = Some(format!("local agents unavailable: {error}")),
        }
    }

    /// Keeps one NIP-AR sync open for the selected channel when the relay
    /// advertises `artifacts.sync`, and retries after `resync`/`error`.
    async fn reconcile_artifact_sync(&mut self) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let community = runtime.community_id;
        let supervisor = runtime.supervisor.clone();
        let caps = match self.artifact_caps {
            Some((owner, caps)) if owner == community => caps,
            _ => {
                if self.artifact_caps_requested != Some(community) {
                    self.artifact_caps_requested = Some(community);
                    let service = runtime.artifacts.clone();
                    let tx = self.background_tx.clone();
                    tokio::spawn(async move {
                        // An unreadable NIP-11 means "no artifact sync".
                        let caps = service.capabilities().await.unwrap_or_default();
                        let _ = tx.send(Background::ArtifactCaps { community, caps }).await;
                    });
                }
                return Ok(());
            }
        };
        let desired = caps
            .sync
            .then(|| self.current_channel().map(|channel| channel.id))
            .flatten();
        let current = self
            .artifact_sync
            .as_ref()
            .filter(|sync| sync.community == community)
            .map(|sync| sync.channel);
        if desired != current {
            self.artifact_sync = None;
            match desired {
                Some(channel) => {
                    supervisor
                        .subscribe_with_priority(
                            ARTIFACT_SYNC_SUBSCRIPTION,
                            vec![crate::protocol::artifact::sync_filter(channel)],
                            SubscriptionPriority::Foreground,
                        )
                        .await?;
                    self.artifact_sync = Some(ArtifactSync::new(community, channel));
                }
                None if current.is_some() => supervisor.close(ARTIFACT_SYNC_SUBSCRIPTION).await?,
                None => {}
            }
            return Ok(());
        }
        if let Some(sync) = &mut self.artifact_sync
            && !sync.blocked
            && !sync.overflowed
            && sync.retry_at.is_some_and(|at| Instant::now() >= at)
        {
            sync.retry_at = None;
            supervisor
                .subscribe_with_priority(
                    ARTIFACT_SYNC_SUBSCRIPTION,
                    vec![crate::protocol::artifact::sync_filter(sync.channel)],
                    SubscriptionPriority::Foreground,
                )
                .await?;
        }
        Ok(())
    }

    async fn apply_artifact_sync_event(
        &mut self,
        subscription: &str,
        event: &nostr::Event,
    ) -> Result<bool> {
        let max_events = self.artifact_caps.map_or(0, |(_, caps)| caps.max_events);
        let Some(sync) = self
            .artifact_sync
            .as_mut()
            .filter(|_| subscription == ARTIFACT_SYNC_SUBSCRIPTION)
        else {
            return Ok(false);
        };
        let community = sync.community;
        match event.kind.as_u16() {
            crate::protocol::artifact::KIND_ARTIFACT => {
                let Ok(head) = crate::protocol::artifact::parse(event) else {
                    return Ok(false);
                };
                if !sync.complete {
                    if sync.buffer.len() >= max_events.max(1) {
                        // Over the advertised bound: never commit a partial set.
                        sync.schedule_retry();
                    } else {
                        sync.buffer.push(head);
                    }
                    return Ok(false);
                }
                let changed = self
                    .store
                    .call(move |store| store.apply_artifact_head(community, &head))
                    .await?;
                if changed {
                    self.refresh_artifact_panel().await?;
                }
                Ok(changed)
            }
            crate::protocol::artifact::KIND_ARTIFACT_REMOVAL if sync.complete => {
                let relay = self
                    .store
                    .call(move |store| store.pinned_relay_pubkey(community))
                    .await?;
                let Ok(removal) = crate::protocol::artifact::parse_removal(event, relay.as_deref())
                else {
                    return Ok(false);
                };
                let changed = self
                    .store
                    .call(move |store| store.remove_artifact(community, removal.id, removal.source))
                    .await?;
                if changed {
                    self.refresh_artifact_panel().await?;
                }
                Ok(changed)
            }
            _ => Ok(false),
        }
    }

    /// EOSE: the buffered heads are the complete set at one cut.
    async fn commit_artifact_sync(&mut self) -> Result<()> {
        let Some(sync) = &mut self.artifact_sync else {
            return Ok(());
        };
        if sync.complete || sync.retry_at.is_some() {
            return Ok(());
        }
        let (community, channel) = (sync.community, sync.channel);
        let heads = std::mem::take(&mut sync.buffer);
        sync.complete = true;
        sync.failures = 0;
        self.store
            .call(move |store| store.replace_channel_artifacts(community, channel, &heads))
            .await?;
        self.refresh_artifact_panel().await
    }

    fn artifact_sync_note(&self, channel: Uuid) -> Option<String> {
        let caps = self.artifact_caps.map(|(_, caps)| caps)?;
        if !caps.sync {
            return Some("this relay has no artifact sync".into());
        }
        let sync = self
            .artifact_sync
            .as_ref()
            .filter(|sync| sync.channel == channel)?;
        Some(
            if sync.blocked {
                "artifacts unavailable here"
            } else if sync.overflowed {
                ARTIFACT_SYNC_OVERFLOW_NOTE
            } else if sync.complete {
                "synced"
            } else if sync.retry_at.is_some() {
                "possibly stale · resyncing"
            } else {
                "syncing"
            }
            .into(),
        )
    }

    async fn refresh_artifact_panel(&mut self) -> Result<()> {
        if self.presentation.overlay != Some(Overlay::Artifacts)
            && !matches!(
                self.presentation.confirmation,
                Some(ConfirmationKind::ArtifactDelete | ConfirmationKind::ArtifactMove)
            )
        {
            return Ok(());
        }
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let scope = self.artifact_panel.scope.clone();
        let pubkey = self.self_pubkey().unwrap_or_default().to_owned();
        let items = self
            .store
            .call(move |store| match scope {
                crate::ui::artifacts::ArtifactScope::Channel(channel) => {
                    store.channel_artifacts(community, channel, None)
                }
                crate::ui::artifacts::ArtifactScope::Thread(channel, root) => {
                    store.channel_artifacts(community, channel, Some(&root))
                }
                crate::ui::artifacts::ArtifactScope::Assigned => {
                    store.assigned_artifacts(community, &pubkey)
                }
            })
            .await?;
        self.artifact_panel.sync_note = match &self.artifact_panel.scope {
            crate::ui::artifacts::ArtifactScope::Channel(channel)
            | crate::ui::artifacts::ArtifactScope::Thread(channel, _) => {
                self.artifact_sync_note(*channel)
            }
            crate::ui::artifacts::ArtifactScope::Assigned => None,
        };
        self.artifact_panel.set_items(items);
        self.cache_dirty = true;
        Ok(())
    }

    async fn open_artifacts(&mut self, scope: crate::ui::artifacts::ArtifactScope) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            self.status_error = Some("connect to a community to see artifacts".into());
            return Ok(());
        };
        if scope == crate::ui::artifacts::ArtifactScope::Assigned {
            let service = runtime.artifacts.clone();
            let pubkey = runtime.signer.public_key().to_hex();
            let tx = self.background_tx.clone();
            self.artifact_panel.loading = true;
            tokio::spawn(async move {
                let result = service
                    .refresh_assigned(&pubkey)
                    .await
                    .map(|_| String::new())
                    .map_err(|error| error.to_string());
                let _ = tx.send(Background::ArtifactDone(result)).await;
            });
        }
        self.artifact_panel.scope = scope;
        self.presentation.open_overlay(Overlay::Artifacts);
        self.refresh_artifact_panel().await
    }

    async fn artifact_panel_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.presentation.close_overlay(),
            KeyCode::Up | KeyCode::Char('k') => self.artifact_panel.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.artifact_panel.move_by(1),
            KeyCode::Enter => {
                // Open the artifact's home channel (its "home link").
                if let Some(home) = self.artifact_panel.selected().map(|item| item.head.home)
                    && let Some(index) = self.channels.iter().position(|channel| channel.id == home)
                {
                    self.presentation.close_overlay();
                    self.select_channel_index(index);
                    self.cache_dirty = true;
                }
            }
            KeyCode::Char('d') => {
                if self
                    .artifact_panel
                    .selected()
                    .is_some_and(|item| !item.head.is_deleted())
                {
                    self.presentation
                        .open_confirmation(ConfirmationKind::ArtifactDelete);
                }
            }
            KeyCode::Char('u') => {
                if let Some(item) = self
                    .artifact_panel
                    .selected()
                    .filter(|item| item.head.is_deleted())
                {
                    let id = item.head.id;
                    self.spawn_artifact_change(id, crate::protocol::artifact::Change::Restore);
                }
            }
            KeyCode::Char('r') => {
                self.command = "artifact rename ".into();
                self.presentation.open_overlay(Overlay::Command);
            }
            KeyCode::Char('m') => {
                self.command = "artifact move #".into();
                self.presentation.open_overlay(Overlay::Command);
            }
            _ => {}
        }
        Ok(())
    }

    /// `:artifact rename <title> | delete | restore | move <#channel>` on the
    /// panel's selected artifact.
    async fn artifact_command(&mut self, arguments: &str) -> Result<()> {
        use crate::protocol::artifact::Change;
        let Some(item) = self.artifact_panel.selected().cloned() else {
            self.status_error = Some("open :artifacts or :tasks and select an artifact".into());
            return Ok(());
        };
        let (verb, rest) = arguments.split_once(' ').unwrap_or((arguments, ""));
        let rest = rest.trim();
        match verb {
            "rename" if !rest.is_empty() && !item.head.is_deleted() => {
                self.spawn_artifact_change(item.head.id, Change::Rename(rest.to_owned()));
            }
            "delete" if !item.head.is_deleted() => {
                self.presentation.open_overlay(Overlay::Artifacts);
                self.presentation
                    .open_confirmation(ConfirmationKind::ArtifactDelete);
                return Ok(());
            }
            "restore" if item.head.is_deleted() => {
                self.spawn_artifact_change(item.head.id, Change::Restore);
            }
            "move" if !rest.is_empty() && !item.head.is_deleted() => {
                let name = rest.trim_start_matches('#');
                let Some(destination) = self
                    .channels
                    .iter()
                    .find(|channel| channel.is_member && channel.name.eq_ignore_ascii_case(name))
                    .map(|channel| channel.id)
                else {
                    self.status_error = Some(format!("no joined channel named #{name}"));
                    return Ok(());
                };
                if destination == item.head.home {
                    self.status_error = Some("the artifact already lives there".into());
                    return Ok(());
                }
                self.pending_artifact_move = Some((item.head.id, destination));
                self.presentation.open_overlay(Overlay::Artifacts);
                self.presentation
                    .open_confirmation(ConfirmationKind::ArtifactMove);
                return Ok(());
            }
            _ => {
                self.status_error = Some(
                    "usage: :artifact rename <title> | delete | restore | move <#channel> (deleted artifacts can only be restored)".into(),
                );
                return Ok(());
            }
        }
        self.presentation.open_overlay(Overlay::Artifacts);
        self.refresh_artifact_panel().await
    }

    fn spawn_artifact_change(&mut self, id: Uuid, change: crate::protocol::artifact::Change) {
        let Some(runtime) = &self.runtime else { return };
        let service = runtime.artifacts.clone();
        let tx = self.background_tx.clone();
        self.artifact_panel.loading = true;
        tokio::spawn(async move {
            let result = service
                .apply(id, change)
                .await
                .map(|applied| match applied {
                    crate::service::artifacts::Applied::Published(_) => "artifact updated".into(),
                    crate::service::artifacts::Applied::AlreadyCurrent => {
                        "artifact was already up to date".into()
                    }
                })
                .map_err(|error| format!("artifact not changed: {error}"));
            let _ = tx.send(Background::ArtifactDone(result)).await;
        });
    }

    /// NIP-AR requires showing the destination audience and what is shared.
    fn artifact_move_preview(&self) -> String {
        let Some((id, destination)) = self.pending_artifact_move else {
            return String::new();
        };
        let item = self
            .artifact_panel
            .items
            .iter()
            .find(|item| item.head.id == id);
        let title = item
            .and_then(|item| item.head.title.as_deref())
            .map_or_else(String::new, |title| {
                crate::protocol::artifact::display(title, 60)
            });
        let tags = item.map_or(0, |item| item.head.client_tags().len());
        let channel = self
            .channels
            .iter()
            .find(|channel| channel.id == destination);
        let audience = channel.map_or_else(
            || "unknown audience".to_owned(),
            |channel| {
                format!(
                    "#{} · {} · {} members",
                    crate::protocol::artifact::display(&channel.name, 40),
                    format!("{:?}", channel.visibility).to_lowercase(),
                    channel.member_count
                )
            },
        );
        format!(
            "\"{title}\" moves to {audience}. Everyone there can read its title, {tags} tags and content · y move · n/Esc cancel"
        )
    }

    /// Hosted agents of the active community, in panel order.
    fn broker_agent_indexes(&self) -> Vec<usize> {
        let community = self.active_community_id();
        self.config
            .hosted_agents
            .iter()
            .enumerate()
            .filter(|(_, agent)| Some(agent.community) == community)
            .map(|(index, _)| index)
            .take(8)
            .collect()
    }

    fn broker_select(&mut self, delta: isize) {
        let count = self.broker_agent_indexes().len();
        if count > 0 {
            self.broker_selected = self
                .broker_selected
                .saturating_add_signed(delta)
                .min(count - 1);
        }
    }

    /// Owner policy edit from the panel: allow or deny one hosted action for
    /// the selected agent. Saved, then applied to the running host at once.
    fn broker_toggle_action(&mut self, index: usize) {
        let Some(&agent) = self.broker_agent_indexes().get(self.broker_selected) else {
            return;
        };
        let Some(action) = crate::broker::host::hosted_actions().into_iter().nth(index) else {
            return;
        };
        let mut allowed = self.config.hosted_agents[agent].allowed.clone();
        if let Some(position) = allowed.iter().position(|value| *value == action) {
            allowed.remove(position);
        } else {
            allowed.push(action);
        }
        if let Err(error) =
            crate::broker::custody::set_allowed(&mut self.config.hosted_agents[agent], &allowed)
        {
            self.status_error = Some(error.to_string());
            return;
        }
        self.save_broker_policy();
    }

    fn broker_toggle_paused(&mut self) {
        let Some(&agent) = self.broker_agent_indexes().get(self.broker_selected) else {
            return;
        };
        let paused = &mut self.config.hosted_agents[agent].paused;
        *paused = !*paused;
        // A local agent's process follows on the next tick.
        self.local_agents_dirty |= self.config.hosted_agents[agent].local.is_some();
        self.save_broker_policy();
    }

    fn save_broker_policy(&mut self) {
        if let Err(error) = self.config.save(&self.paths) {
            self.status_error = Some(error.to_string());
            return;
        }
        if let Some(broker) = &self.broker {
            for agent in &self.config.hosted_agents {
                broker.host.update_policy(
                    agent.id,
                    agent.allowed.iter().cloned().collect(),
                    agent.paused,
                );
            }
        }
    }

    fn profile_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Closing cancels the lookup: a late answer is discarded.
                self.profile_card = None;
                self.presentation.close_overlay();
            }
            KeyCode::Char('r') => self.spawn_public_lookup(),
            _ => {}
        }
    }

    /// Open the profile card for `pubkey` and start its public lookup.
    async fn open_profile(&mut self, pubkey: String) -> Result<()> {
        let community = self.active_community_id();
        let profile = match self.profiles.get(&pubkey) {
            Some(profile) => Some(profile.clone()),
            None => match community {
                Some(community) => {
                    let subject = pubkey.clone();
                    self.store
                        .call(move |store| store.profile(community, &subject))
                        .await?
                }
                None => None,
            },
        };
        let mut card = crate::ui::profile::ProfileCard::new(pubkey.clone(), profile);
        card.stale = community
            .and_then(|community| self.public_lookup_cache.get(&(community, pubkey)).cloned());
        self.profile_card = Some(card);
        self.presentation.open_overlay(Overlay::Profile);
        self.spawn_public_lookup();
        Ok(())
    }

    fn spawn_public_lookup(&mut self) {
        use crate::ui::profile::PublicChannels;
        let now = crate::clock::WallClock::now_unix(&crate::clock::SystemClock);
        let Some(card) = self.profile_card.as_mut() else {
            return;
        };
        let Some(runtime) = &self.runtime else {
            card.public = PublicChannels::Locked;
            return;
        };
        // Respect the relay's retry delay and back off capability probes.
        if let Some(&(until, rate_limited)) = self.public_lookup_backoff.get(&runtime.community_id)
            && now < until
        {
            card.public = if rate_limited {
                PublicChannels::RateLimited(until - now)
            } else {
                PublicChannels::Unsupported
            };
            return;
        }
        self.profile_generation = self.profile_generation.wrapping_add(1);
        let generation = self.profile_generation;
        card.outstanding = Some(generation);
        card.public = PublicChannels::Loading;
        let service = runtime.profiles.clone();
        let community = runtime.community_id;
        let subject = card.pubkey.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = service
                .public_memberships(&subject)
                .await
                .map_err(|error| error.to_string());
            let _ = tx
                .send(Background::PublicMemberships {
                    community,
                    subject,
                    generation,
                    result,
                })
                .await;
        });
    }

    fn apply_public_lookup(
        &mut self,
        community: Uuid,
        subject: &str,
        generation: u64,
        result: std::result::Result<crate::protocol::membership_public::Lookup, String>,
    ) {
        use crate::{protocol::membership_public::Lookup, ui::profile::PublicChannels};
        let now = crate::clock::WallClock::now_unix(&crate::clock::SystemClock);
        // Backoff state follows the relay, whether or not the card is open.
        match &result {
            Ok(Lookup::Unsupported) => {
                self.public_lookup_backoff
                    .insert(community, (now.saturating_add(600), false));
            }
            Ok(Lookup::RateLimited(seconds)) => {
                let jitter = u64::from(rand::random::<u16>()) % (seconds / 4 + 2);
                self.public_lookup_backoff.insert(
                    community,
                    (now.saturating_add(*seconds).saturating_add(jitter), true),
                );
            }
            _ => {}
        }
        if let Ok(Lookup::Verified(verified)) = &result {
            // A verified list for this community and subject replaces the
            // cached display list; it is never membership authority.
            self.public_lookup_cache.insert(
                (community, subject.to_owned()),
                verified.memberships.clone(),
            );
        }
        let active = self.active_community_id();
        let Some(card) = self.profile_card.as_mut() else {
            return;
        };
        // Canceled, superseded or foreign answers never replace current data.
        if active != Some(community)
            || card.pubkey != subject
            || card.outstanding != Some(generation)
        {
            return;
        }
        card.outstanding = None;
        card.public = match result {
            Ok(Lookup::Verified(verified)) => {
                card.stale = None;
                PublicChannels::Verified(verified.memberships)
            }
            Ok(Lookup::Unsupported) => PublicChannels::Unsupported,
            Ok(Lookup::RateLimited(seconds)) => PublicChannels::RateLimited(seconds),
            Err(_) => PublicChannels::Failed,
        };
    }

    fn spawn_thread_window(
        &mut self,
        root: String,
        cursor: Option<crate::protocol::window::Cursor>,
    ) {
        if self.thread_window_loading {
            return;
        }
        let (Some(runtime), Some(channel)) = (
            &self.runtime,
            self.current_channel().map(|channel| channel.id),
        ) else {
            return;
        };
        let Some(host) = runtime.channels.relay_host() else {
            return;
        };
        self.thread_window_loading = true;
        let service = runtime.channels.clone();
        let community = runtime.community_id;
        let reader = runtime.signer.public_key().to_hex();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let request = crate::protocol::window::ThreadRequest {
                host: &host,
                reader: &reader,
                channel,
                root: &root,
                limit: crate::sync::window::THREAD_PAGE,
                depth: 100,
                include_aux: true,
                cursor: cursor.as_ref(),
            };
            let result = service.thread_page(&request).await;
            let message = match result {
                Ok(page) => Background::ThreadWindow {
                    community,
                    root: root.clone(),
                    supported: page.is_some(),
                    next: page.and_then(|page| page.next),
                },
                // A failed page (timeout, 503, changed access) keeps the local
                // archive and never downgrades to another protocol mode.
                Err(_) => Background::ThreadWindow {
                    community,
                    root: root.clone(),
                    supported: false,
                    next: None,
                },
            };
            let _ = tx.send(message).await;
        });
    }

    fn move_to_edge(&mut self, last: bool) {
        match self.presentation.focus {
            FocusSurface::Communities => {
                self.community_cursor = if last {
                    self.config.communities.len().saturating_sub(1)
                } else {
                    0
                };
                self.select_community_index(self.community_cursor);
            }
            FocusSurface::Channels => {
                let joined = self.ordered_channel_indexes();
                if let Some(index) = if last { joined.last() } else { joined.first() } {
                    self.select_channel_index(*index);
                    self.showing_open_channel = false;
                }
            }
            FocusSurface::Timeline => {
                self.timeline.selected_event = (if last {
                    self.messages.last()
                } else {
                    self.messages.first()
                })
                .map(|message| message.event_id.clone());
                self.timeline.at_live_bottom = last;
                self.timeline.keep_selection_visible = true;
            }
            FocusSurface::Context => {
                self.thread_timeline.selected_event = (if last {
                    self.thread_messages.last()
                } else {
                    self.thread_messages.first()
                })
                .map(|message| message.event_id.clone());
                self.thread_timeline.at_live_bottom = last;
                self.thread_timeline.keep_selection_visible = true;
            }
            FocusSurface::InboxList | FocusSurface::InboxDetail | FocusSurface::App => {}
        }
    }

    async fn open_selected(&mut self) -> Result<()> {
        match self.presentation.focus {
            FocusSurface::Communities => {}
            FocusSurface::Channels => {
                self.load_selected_channel().await?;
                self.presentation
                    .set_workspace_focus(FocusSurface::Timeline);
            }
            FocusSurface::Timeline | FocusSurface::Context => self.toggle_thread().await?,
            FocusSurface::InboxList | FocusSurface::InboxDetail | FocusSurface::App => {}
        }
        Ok(())
    }
    async fn load_selected_channel(&mut self) -> Result<()> {
        self.thread_root = None;
        self.agent_typing.clear();
        self.timeline = TimelineState {
            at_live_bottom: true,
            ..TimelineState::default()
        };
        self.cache_dirty = true;
        self.hydrate_cache().await?;
        if let Some(channel) = self.current_channel().map(|channel| channel.id) {
            // A reopened channel must get a fresh durable read-state attempt;
            // `last_marked` is only an in-session write coalescer.
            self.last_marked.remove(&channel.to_string());
            self.subscribe_channel(channel).await?;
            self.spawn_backfill(channel);
            // What was unread when the channel opened keeps its `── new ──`
            // line while the channel stays open, though it is read now.
            self.new_since = self.channel_read_at(channel).await?.map(|at| (channel, at));
            // Explicitly opening a channel starts at its live edge. Persist the
            // local marker now rather than waiting for a later redraw/tick.
            self.mark_current_read().await?;
        }
        self.reconcile_agent_typing_subscription().await
    }
    async fn toggle_thread(&mut self) -> Result<()> {
        self.agent_typing.clear();
        if self.thread_root.is_some() {
            self.thread_root = None;
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
            return Ok(());
        }
        if let Some(message) = self.selected_message() {
            // A "sent from thread" copy opens the thread it came from.
            let root = message
                .sent_from_thread
                .as_ref()
                .map(|reference| reference.root_event_id.clone())
                .or_else(|| message.root_event_id.clone())
                .unwrap_or_else(|| message.event_id.clone());
            self.thread_root = Some(root.clone());
            self.thread_window = None;
            self.spawn_thread_window(root, None);
            // The thread was asked for last: it takes the right side.
            self.apps.front = false;
            self.presentation.set_workspace_focus(FocusSurface::Context);
            self.cache_dirty = true;
            self.hydrate_cache().await?;
        }
        Ok(())
    }
    async fn enter_composer(&mut self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return Ok(());
        };
        let root = self.reply_root();
        let parent = self
            .selected_message()
            .map(|message| message.event_id.clone())
            .or_else(|| root.clone());
        self.open_composer_target(community, channel, root, parent)
            .await
    }

    async fn open_composer_target(
        &mut self,
        community: Uuid,
        channel: Uuid,
        root: Option<String>,
        parent: Option<String>,
    ) -> Result<()> {
        self.editing = None;
        if let Some(reason) = self
            .channels
            .iter()
            .find(|value| value.id == channel)
            .and_then(Channel::read_only_reason)
        {
            self.status_error = Some(reason.notice().into());
            return Ok(());
        }
        if self.runtime.is_none() {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return Ok(());
        }
        if self.active_community_id() != Some(community) {
            self.status_error = Some("the composer target belongs to a different community".into());
            return Ok(());
        }
        let draft_root = root.clone();
        let draft = self
            .store
            .call(move |store| store.draft_record(community, channel, draft_root.as_deref()))
            .await?;
        let (body, attachments, mentions, revision) = draft
            .map(|draft| {
                (
                    draft.body,
                    draft.attachments,
                    draft.mentions,
                    Some(draft.revision),
                )
            })
            .unwrap_or_default();
        self.composer.set_draft(body, attachments, mentions);
        self.composer_draft_revision = revision;
        let target = ComposerTarget {
            community_id: community,
            channel_id: channel,
            thread_root_id: root,
            parent_event_id: parent,
        };
        self.presentation.composer_target = Some(target.clone());
        if self.repair_pending_attachment_ids() {
            self.persist_draft().await?;
        }
        self.refresh_mention_picker().await?;
        let pending = self
            .composer
            .attachments
            .iter()
            .filter_map(|attachment| match attachment {
                crate::media::DraftAttachment::Pending(value) => Some(value.clone()),
                crate::media::DraftAttachment::Failed(_)
                | crate::media::DraftAttachment::Uploaded(_) => None,
            })
            .collect::<Vec<_>>();
        for attachment in pending {
            self.start_pending_upload(target.clone(), attachment);
        }
        Ok(())
    }

    fn repair_pending_attachment_ids(&mut self) -> bool {
        let mut changed = false;
        for attachment in &mut self.composer.attachments {
            if let Some(pending) = attachment.pending_mut()
                && Uuid::parse_str(&pending.id).is_err()
            {
                pending.id = Uuid::new_v4().to_string();
                changed = true;
            }
        }
        changed
    }

    /// Says so when an edit turned a mention into plain text: a message
    /// never loses a mention the picker put in without a word.
    fn note_dropped_mention(&mut self) {
        if let Some(name) = self.composer.take_dropped_mention() {
            self.status_error = Some(format!(
                "{} is no longer a mention: its name was edited, so nobody is notified. Type @ and choose the name again to mention",
                crate::render::sanitize::single_line(&name)
            ));
        }
    }

    async fn insert_action(&mut self, action: KeyAction) -> Result<()> {
        let done = self.composer_action(action).await;
        self.note_dropped_mention();
        done
    }

    async fn composer_action(&mut self, action: KeyAction) -> Result<()> {
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
                self.staging_media.clear();
                self.staging_attachments.clear();
                self.presentation.composer_target = None;
                self.composer_draft_revision = None;
                if self.editing.take().is_some() {
                    self.composer
                        .set_draft(String::new(), Vec::new(), Vec::new());
                }
            }
            KeyAction::Character(character) => {
                if !self.complete_exact_mention(character) {
                    self.composer.insert(character);
                }
            }
            KeyAction::Backspace => self.composer.backspace(),
            KeyAction::ForwardDelete => self.composer.delete(),
            KeyAction::Left => self.composer.move_left(),
            KeyAction::Right => self.composer.move_right(),
            KeyAction::Newline => self.composer.newline(),
            KeyAction::Submit if self.active_edit().is_some() => self.save_edit().await?,
            KeyAction::Submit => {
                if !self.staging_attachments.is_empty() {
                    self.status_error = Some("attachment processing is not ready to send".into());
                } else if !self.composer.sendable() && !self.composer.attachments.is_empty() {
                    self.status_error = Some("wait for every attachment to become ready".into());
                } else if let Some(reason) = self
                    .presentation
                    .composer_target
                    .as_ref()
                    .and_then(|target| {
                        self.channels
                            .iter()
                            .find(|value| value.id == target.channel_id)
                    })
                    .and_then(Channel::read_only_reason)
                {
                    // The role or channel settings changed while composing:
                    // keep the draft, refuse locally, let the user decide.
                    self.status_error = Some(reason.notice().into());
                } else if self.composer.sendable() {
                    let Some(prepared_target) = self.presentation.composer_target.clone() else {
                        return Ok(());
                    };
                    let prepared_mentions = self
                        .composer
                        .mentions()
                        .iter()
                        .map(|mention| mention.pubkey.clone())
                        .collect::<Vec<_>>();
                    let agent_mentions_to_validate = self
                        .store
                        .call({
                            let mentions = prepared_mentions.clone();
                            let target = prepared_target.clone();
                            move |store| {
                                store.agent_mentions_need_validation(
                                    target.community_id,
                                    target.channel_id,
                                    &mentions,
                                )
                            }
                        })
                        .await?;
                    if agent_mentions_to_validate != 0 {
                        let Some(runtime) = &self.runtime else {
                            self.status_error = Some(
                                "remote agent validation requires an unlocked online community"
                                    .into(),
                            );
                            return Ok(());
                        };
                        if runtime.community_id != prepared_target.community_id {
                            self.status_error =
                                Some("the agent mention belongs to another community".into());
                            return Ok(());
                        }
                        let identity_pubkey = runtime.signer.public_key().to_hex();
                        if let Err(error) = runtime.channels.refresh(&identity_pubkey).await {
                            self.diagnostics
                                .emit(DiagnosticEvent::AgentMentionValidated {
                                    count: u32::try_from(agent_mentions_to_validate)
                                        .unwrap_or(u32::MAX),
                                    outcome: "refresh_failed".into(),
                                });
                            self.status_error = Some(format!(
                                "remote agent revalidation failed; draft preserved ({error})"
                            ));
                            return Ok(());
                        }
                        let validation = self
                            .store
                            .call({
                                let mentions = prepared_mentions.clone();
                                let target = prepared_target.clone();
                                move |store| {
                                    store.validate_agent_mentions(
                                        target.community_id,
                                        target.channel_id,
                                        &identity_pubkey,
                                        &mentions,
                                    )
                                }
                            })
                            .await;
                        if let Err(error) = validation {
                            self.diagnostics
                                .emit(DiagnosticEvent::AgentMentionValidated {
                                    count: u32::try_from(agent_mentions_to_validate)
                                        .unwrap_or(u32::MAX),
                                    outcome: "ineligible".into(),
                                });
                            self.status_error = Some(format!(
                                "remote agent mention not sent; draft preserved ({error})"
                            ));
                            return Ok(());
                        }
                        self.diagnostics
                            .emit(DiagnosticEvent::AgentMentionValidated {
                                count: u32::try_from(agent_mentions_to_validate)
                                    .unwrap_or(u32::MAX),
                                outcome: "eligible".into(),
                            });
                        self.cache_dirty = true;
                    }
                    // Mention rules v3, section 8: every recipient is a member
                    // when the message is written, at most 32 of them. bzz
                    // only offers members, so this catches stale drafts.
                    if !prepared_mentions.is_empty() {
                        let mentions = prepared_mentions.clone();
                        let target = prepared_target.clone();
                        let sender = self.self_pubkey().map(str::to_owned).unwrap_or_default();
                        let checked = self
                            .store
                            .call(move |store| {
                                let members = store
                                    .mention_choices(
                                        target.community_id,
                                        target.channel_id,
                                        &sender,
                                    )?
                                    .choices
                                    .into_iter()
                                    .map(|choice| choice.pubkey)
                                    .collect::<Vec<_>>();
                                Ok(crate::protocol::mention_rules::write_tags(
                                    &sender,
                                    &members,
                                    &mentions,
                                    &[],
                                ))
                            })
                            .await?;
                        if let Err(error) = checked {
                            use crate::protocol::mention_rules::TagError;
                            self.status_error = Some(
                                match error {
                                    TagError::NotMember => {
                                        "a mentioned person is no longer in this channel; remove the mention (draft kept)"
                                    }
                                    TagError::TooMany => {
                                        "a message can mention at most 32 people (draft kept)"
                                    }
                                    TagError::Invalid => "a mention is invalid; remove it (draft kept)",
                                }
                                .into(),
                            );
                            return Ok(());
                        }
                    }
                    // Persist the final edit before making its acknowledgement
                    // boundary durable. A send without a revision is refused:
                    // it could not be recovered after a failed acknowledgement.
                    self.persist_draft().await?;
                    let Some(target) = self.presentation.composer_target.clone() else {
                        return Ok(());
                    };
                    let Some(revision) = self.composer_draft_revision.clone() else {
                        self.status_error = Some("could not prepare the draft for sending".into());
                        return Ok(());
                    };
                    let submission = DraftSubmission {
                        community_id: target.community_id,
                        channel_id: target.channel_id,
                        thread_root_id: target.thread_root_id.clone(),
                        revision,
                    };
                    let sending = submission.clone();
                    let marked = self
                        .store
                        .call(move |store| store.mark_draft_sending(&sending))
                        .await?;
                    if !marked {
                        self.status_error =
                            Some("the draft changed before it could be sent".into());
                        return Ok(());
                    }
                    if let Some(message) = self.composer.take_message() {
                        self.status_error =
                            Some("sending; waiting for relay acknowledgement".into());
                        self.queue_message(message, submission);
                        self.presentation.composer_target = None;
                        self.composer_draft_revision = None;
                    }
                }
            }
            _ => {}
        }
        self.refresh_mention_picker().await?;
        self.persist_draft().await
    }

    fn discard_pending_attachment(
        &mut self,
        community: Uuid,
        pending: crate::media::PendingAttachment,
    ) {
        self.uploading_media.remove(&pending.id);
        if pending.cache_name.contains(['/', '\\']) {
            return;
        }
        let path = self.media.staging_dir(community).join(pending.cache_name);
        tokio::spawn(async move {
            let _ = tokio::fs::remove_file(path).await;
        });
    }

    fn remove_staged_attachment(&self, community: Uuid, pending: crate::media::PendingAttachment) {
        if pending.cache_name.contains(['/', '\\']) {
            return;
        }
        let path = self.media.staging_dir(community).join(pending.cache_name);
        tokio::spawn(async move {
            let _ = tokio::fs::remove_file(path).await;
        });
    }

    async fn remove_last_attachment(&mut self) -> Result<()> {
        if let Some(staging) = self.staging_attachments.pop() {
            self.staging_media.remove(&staging.id);
            self.status_error = Some("attachment removed".into());
            return Ok(());
        }
        let Some(attachment) = self.composer.attachments.pop() else {
            self.composer.delete();
            self.refresh_mention_picker().await?;
            return self.persist_draft().await;
        };
        if let Some(pending) = attachment.pending().cloned()
            && let Some(target) = self.presentation.composer_target.as_ref()
        {
            self.discard_pending_attachment(target.community_id, pending);
        }
        self.status_error = Some("attachment removed".into());
        self.persist_draft().await
    }

    async fn retry_failed_attachments(&mut self) -> Result<()> {
        let Some(target) = self.presentation.composer_target.clone() else {
            return Ok(());
        };
        let mut retry = Vec::new();
        for attachment in &mut self.composer.attachments {
            if let crate::media::DraftAttachment::Failed(pending) = attachment {
                let pending = pending.clone();
                *attachment = crate::media::DraftAttachment::Pending(pending.clone());
                retry.push(pending);
            }
        }
        if retry.is_empty() {
            return Ok(());
        }
        self.persist_draft().await?;
        for pending in retry {
            self.start_pending_upload(target.clone(), pending);
        }
        Ok(())
    }

    async fn clear_composer_draft(&mut self) -> Result<()> {
        self.staging_media.clear();
        self.staging_attachments.clear();
        let community = self
            .presentation
            .composer_target
            .as_ref()
            .map(|target| target.community_id);
        for attachment in self.composer.clear() {
            if let (Some(community), Some(pending)) = (community, attachment.pending().cloned()) {
                self.discard_pending_attachment(community, pending);
            }
        }
        self.mention_picker = None;
        self.persist_draft().await
    }

    async fn refresh_mention_picker(&mut self) -> Result<()> {
        let Some(range) = self.composer.active_mention() else {
            self.mention_picker = None;
            return Ok(());
        };
        let Some(target) = self.presentation.composer_target.clone() else {
            self.mention_picker = None;
            return Ok(());
        };
        if self.active_community_id() != Some(target.community_id) {
            self.mention_picker = None;
            return Ok(());
        }
        let community = target.community_id;
        let channel = target.channel_id;
        let Some(self_pubkey) = self.self_pubkey().map(str::to_owned) else {
            self.mention_picker = None;
            return Ok(());
        };
        let query = self.composer.body[range.start + 1..range.end].to_owned();
        let cached =
            self.mention_choices
                .as_ref()
                .is_some_and(|(cached_community, cached_channel, _)| {
                    (*cached_community, *cached_channel) == (community, channel)
                });
        if !cached || query.is_empty() {
            let choices = self
                .store
                .call(move |store| store.mention_choices(community, channel, &self_pubkey))
                .await?;
            self.mention_choices = Some((community, channel, choices));
        }
        let Some((_, _, choices)) = &self.mention_choices else {
            return Ok(());
        };
        // A multi-word query stays open only while it starts a known name.
        if !crate::protocol::mention_rules::admitted(&query, &choices.names()) {
            self.mention_picker = None;
            return Ok(());
        }
        let now = crate::clock::SystemClock.now_unix();
        let presence = |pubkey: &str| match self.presence.get(pubkey, now) {
            crate::protocol::presence::Presence::Online => {
                crate::protocol::mention_rules::Presence::Online
            }
            crate::protocol::presence::Presence::Away => {
                crate::protocol::mention_rules::Presence::Away
            }
            crate::protocol::presence::Presence::Offline => {
                crate::protocol::mention_rules::Presence::Unknown
            }
        };
        let mut candidates = choices.ranked(&query, &self.mention_history.get(channel), presence);
        for candidate in &mut candidates {
            candidate.presence = Some(self.presence.get(&candidate.pubkey, now));
        }
        self.mention_picker = Some(MentionPicker::new(range, query, candidates));
        Ok(())
    }

    /// Space or closing punctuation right after a query selects the one
    /// exact name, if there is one (mention rules v3, section 5). The typed
    /// character then follows the mention. Returns whether it selected.
    fn complete_exact_mention(&mut self, typed: char) -> bool {
        if typed != ' ' && !crate::protocol::mention_rules::is_closing_punctuation(typed) {
            return false;
        }
        let Some(picker) = &self.mention_picker else {
            return false;
        };
        let Some((_, channel, choices)) = &self.mention_choices else {
            return false;
        };
        let Some(candidate) = choices.exact(&picker.query).cloned() else {
            return false;
        };
        let (range, channel) = (picker.range.clone(), *channel);
        self.mention_picker = None;
        if !self
            .composer
            .accept_mention(range, &candidate.label, &candidate.pubkey)
        {
            self.status_error = Some("could not add that mention".into());
            return false;
        }
        self.mention_history.record(channel, &candidate.pubkey);
        self.composer.insert(typed);
        true
    }

    fn accept_mention(&mut self) {
        let selected = self.mention_picker.as_ref().and_then(|picker| {
            picker
                .selected()
                .cloned()
                .map(|candidate| (picker.range.clone(), candidate))
        });
        self.mention_picker = None;
        if let Some((range, candidate)) = selected {
            // The cursor is left after a space, as Buzz leaves it: what is
            // typed next must not run into the name and undo the mention.
            if self
                .composer
                .accept_mention_from_picker(range, &candidate.label, &candidate.pubkey)
            {
                if let Some((_, channel, _)) = &self.mention_choices {
                    let channel = *channel;
                    self.mention_history.record(channel, &candidate.pubkey);
                }
            } else {
                self.status_error = Some("could not add that mention".into());
            }
        }
    }

    fn open_media_preview(&mut self) {
        if self
            .selected_message()
            .is_some_and(|message| !message.attachments.is_empty())
        {
            self.preview_index = 0;
            self.preview_revealed = false;
            self.presentation.open_overlay(Overlay::MediaPreview);
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
            KeyCode::Esc | KeyCode::Char('q') => self.presentation.close_overlay(),
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
                self.presentation
                    .open_attachment_prompt(AttachmentPrompt::Save);
            }
            _ => {}
        }
    }

    async fn attachment_path_key(&mut self, key: KeyEvent, save: bool) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                if save {
                    self.presentation.open_overlay(Overlay::MediaPreview);
                } else {
                    self.presentation.close_overlay();
                }
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
                    self.presentation.open_overlay(Overlay::MediaPreview);
                } else {
                    self.start_attachment_upload(path);
                    self.presentation.close_overlay();
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

    async fn handle_attachment_background(&mut self, event: AttachmentBackground) -> Result<()> {
        match event {
            AttachmentBackground::FilesPicked { target, outcome } => {
                if self.presentation.composer_target.as_ref() == Some(&target) {
                    match outcome {
                        FilePickerOutcome::Files(paths) => self.start_selected_files(paths),
                        FilePickerOutcome::Cancelled => self.status_error = None,
                        FilePickerOutcome::Unavailable => {
                            self.status_error = Some(
                                "native file picker is unavailable; use Alt-o to enter a path"
                                    .into(),
                            );
                        }
                        FilePickerOutcome::Rejected(rejection) => {
                            self.status_error = Some(rejection.status().into());
                        }
                    }
                }
            }
            AttachmentBackground::ClipboardImported { target, contents } => {
                if self.presentation.composer_target.as_ref() == Some(&target) {
                    self.handle_clipboard_contents(target, *contents).await?;
                }
            }
            AttachmentBackground::Staged {
                target,
                community,
                pending,
            } => {
                self.staging_attachments
                    .retain(|item| item.id != pending.id);
                let accepted = self.staging_media.remove(&pending.id)
                    && self.active_community_id() == Some(community)
                    && self.presentation.composer_target.as_ref() == Some(&target)
                    && self.composer.attachments.len() < 8;
                if accepted {
                    self.composer
                        .attachments
                        .push(crate::media::DraftAttachment::Pending(pending.clone()));
                    self.persist_draft().await?;
                    self.start_pending_upload(target, pending);
                } else {
                    self.remove_staged_attachment(community, pending);
                }
            }
            AttachmentBackground::StageFailed {
                target,
                attachment_id,
                message,
            } => {
                self.staging_media.remove(&attachment_id);
                self.staging_attachments
                    .retain(|item| item.id != attachment_id);
                if self.presentation.composer_target.as_ref() == Some(&target) {
                    self.status_error = Some(message);
                }
            }
            AttachmentBackground::Uploaded {
                target,
                community,
                attachment_id,
                attachment,
            } => {
                self.uploading_media.remove(&attachment_id);
                let active = self.active_community_id() == Some(community)
                    && self.presentation.composer_target.as_ref() == Some(&target);
                let replacement = crate::media::DraftAttachment::Uploaded(*attachment);
                if active
                    && let Some((index, item)) = self
                        .composer
                        .attachments
                        .iter_mut()
                        .enumerate()
                        .find(|(_, item)| matches!(item, crate::media::DraftAttachment::Pending(value) if value.id == attachment_id))
                {
                    let mut replacement = replacement;
                    if let crate::media::DraftAttachment::Uploaded(value) = &mut replacement {
                        value.index = index;
                    }
                    *item = replacement;
                    self.status_error = None;
                    self.persist_draft().await?;
                } else {
                    let root = target.thread_root_id.clone();
                    self.store
                        .call(move |store| {
                            store.replace_draft_attachment(
                                community,
                                target.channel_id,
                                root.as_deref(),
                                &attachment_id,
                                replacement,
                            )
                        })
                        .await?;
                }
            }
            AttachmentBackground::UploadFailed {
                target,
                community,
                pending,
                message,
            } => {
                self.uploading_media.remove(&pending.id);
                let active = self.active_community_id() == Some(community)
                    && self.presentation.composer_target.as_ref() == Some(&target);
                let replacement = crate::media::DraftAttachment::Failed(pending.clone());
                if active
                    && let Some(item) = self.composer.attachments.iter_mut().find(
                        |item| matches!(item, crate::media::DraftAttachment::Pending(value) if value.id == pending.id),
                    )
                {
                    *item = replacement;
                    self.status_error = Some(message);
                    self.persist_draft().await?;
                } else {
                    let root = target.thread_root_id.clone();
                    self.store
                        .call(move |store| {
                            store.replace_draft_attachment(
                                community,
                                target.channel_id,
                                root.as_deref(),
                                &pending.id,
                                replacement,
                            )
                        })
                        .await?;
                }
            }
        }
        Ok(())
    }

    fn start_selected_files(&mut self, paths: Vec<std::path::PathBuf>) {
        if paths.len() > self.attachment_capacity() {
            self.status_error = Some("selected files exceed the attachment limit".into());
            return;
        }
        for path in paths {
            self.start_attachment_upload(path);
        }
    }

    fn attachment_capacity(&self) -> usize {
        8_usize.saturating_sub(
            self.composer
                .attachments
                .len()
                .saturating_add(self.staging_attachments.len()),
        )
    }

    fn reserve_attachment_staging(
        &mut self,
        filename: String,
    ) -> Option<(ComposerTarget, Uuid, String)> {
        let target = self.presentation.composer_target.clone()?;
        if self.active_community_id() != Some(target.community_id) {
            return None;
        }
        if self.attachment_capacity() == 0 {
            self.status_error = Some("a message can contain at most 8 attachments".into());
            return None;
        }
        let attachment_id = Uuid::new_v4().to_string();
        self.staging_media.insert(attachment_id.clone());
        self.staging_attachments.push(StagingAttachment {
            id: attachment_id.clone(),
            filename: crate::media::decode::sanitize_filename(&filename),
        });
        Some((target.clone(), target.community_id, attachment_id))
    }

    fn start_attachment_upload(&mut self, source: std::path::PathBuf) {
        let filename = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("file");
        let Some((target, community, attachment_id)) =
            self.reserve_attachment_staging(filename.to_owned())
        else {
            return;
        };
        let staging = self.media.staging_dir(community);
        let tx = self.attachment_tx.clone();
        self.status_error = Some("processing attachment…".into());
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::media::decode::stage_file(&source, &staging)
            })
            .await
            .map_err(|_| Error::Protocol("attachment worker stopped".into()))
            .and_then(std::convert::identity);
            let event = match result {
                Ok(staged) => {
                    let mut pending = staged.pending();
                    pending.id = attachment_id;
                    AttachmentBackground::Staged {
                        target,
                        community,
                        pending,
                    }
                }
                Err(error) => AttachmentBackground::StageFailed {
                    target,
                    attachment_id,
                    message: public_media_error(&error),
                },
            };
            let _ = tx.send(event).await;
        });
    }

    fn start_clipboard_image_stage(&mut self, image: crate::media::clipboard::ClipboardImage) {
        let Some((target, community, attachment_id)) =
            self.reserve_attachment_staging("pasted image".into())
        else {
            return;
        };
        let staging = self.media.staging_dir(community);
        let tx = self.attachment_tx.clone();
        self.status_error = Some("processing pasted image…".into());
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let (filename, bytes) = encode_clipboard_png(image)
                    .map_err(|rejection| Error::Protocol(rejection.status().into()))?;
                crate::media::decode::stage_bytes(&staging, &filename, bytes)
            })
            .await
            .map_err(|_| Error::Protocol("attachment worker stopped".into()))
            .and_then(std::convert::identity);
            let event = match result {
                Ok(staged) => {
                    let mut pending = staged.pending();
                    pending.id = attachment_id;
                    AttachmentBackground::Staged {
                        target,
                        community,
                        pending,
                    }
                }
                Err(error) => AttachmentBackground::StageFailed {
                    target,
                    attachment_id,
                    message: public_media_error(&error),
                },
            };
            let _ = tx.send(event).await;
        });
    }

    fn open_file_picker(&mut self) {
        let Some(target) = self.presentation.composer_target.clone() else {
            return;
        };
        if self.attachment_capacity() == 0 {
            self.status_error = Some("a message can contain at most 8 attachments".into());
            return;
        }
        let picker = self.file_picker.clone();
        let tx = self.attachment_tx.clone();
        self.status_error = Some("opening native file picker…".into());
        tokio::spawn(async move {
            let outcome = picker.pick_files().await;
            let _ = tx
                .send(AttachmentBackground::FilesPicked { target, outcome })
                .await;
        });
    }

    async fn import_clipboard(&mut self) -> Result<()> {
        if self.config.media.clipboard_import != ClipboardImportMode::Explicit {
            self.status_error = Some("clipboard import is disabled".into());
            return Ok(());
        }
        let Some(target) = self.presentation.composer_target.clone() else {
            return Ok(());
        };
        let reader = self.clipboard.clone();
        let tx = self.attachment_tx.clone();
        self.status_error = Some("reading clipboard…".into());
        tokio::spawn(async move {
            let contents = tokio::task::spawn_blocking(move || reader.read_once())
                .await
                .unwrap_or(ClipboardContents::Unavailable);
            let _ = tx
                .send(AttachmentBackground::ClipboardImported {
                    target,
                    contents: Box::new(contents),
                })
                .await;
        });
        Ok(())
    }

    async fn handle_clipboard_contents(
        &mut self,
        _target: ComposerTarget,
        contents: ClipboardContents,
    ) -> Result<()> {
        match contents {
            ClipboardContents::Files(paths) => self.start_selected_files(paths),
            ClipboardContents::Image(image) => self.start_clipboard_image_stage(image),
            ClipboardContents::Text(text) => {
                self.composer.insert_text(&text);
                self.status_error = None;
                self.note_dropped_mention();
                self.refresh_mention_picker().await?;
                self.persist_draft().await?;
            }
            ClipboardContents::Empty => {
                self.status_error = Some("clipboard has no pasteable content".into());
            }
            ClipboardContents::Unavailable => {
                self.status_error =
                    Some("native clipboard is unavailable; use Ctrl-o to attach a file".into());
            }
            ClipboardContents::Rejected(rejection) => {
                self.status_error = Some(rejection.status().into());
            }
        }
        Ok(())
    }

    fn start_pending_upload(
        &mut self,
        target: ComposerTarget,
        pending: crate::media::PendingAttachment,
    ) {
        let Some(runtime) = &self.runtime else {
            self.status_error =
                Some("attachment staged; upload waits for an unlocked identity".into());
            return;
        };
        let community = runtime.community_id;
        if community != target.community_id || self.active_community_id() != Some(community) {
            return;
        }
        if Uuid::parse_str(&pending.id).is_err()
            || pending.cache_name.contains(['/', '\\'])
            || !pending.cache_name.starts_with(&pending.sha256)
        {
            self.status_error = Some("staged attachment metadata is invalid".into());
            return;
        }
        if !self.uploading_media.insert(pending.id.clone()) {
            return;
        }
        let path = self.media.staging_dir(community).join(&pending.cache_name);
        let client = runtime.media.clone();
        let tx = self.attachment_tx.clone();
        self.status_error = Some("uploading attachment…".into());
        tokio::spawn(async move {
            let result = client
                .upload(&path, &pending.mime, Some(pending.filename.clone()))
                .await;
            if result.is_ok() {
                let _ = tokio::fs::remove_file(&path).await;
            }
            let event = match result {
                Ok(attachment) => AttachmentBackground::Uploaded {
                    target,
                    community,
                    attachment_id: pending.id,
                    attachment: Box::new(attachment),
                },
                Err(error) => AttachmentBackground::UploadFailed {
                    target,
                    community,
                    pending,
                    message: public_media_error(&error),
                },
            };
            let _ = tx.send(event).await;
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

    async fn persist_draft(&mut self) -> Result<()> {
        if self.active_edit().is_some() {
            return Ok(());
        }
        let Some(target) = self.presentation.composer_target.clone() else {
            return Ok(());
        };
        if self.active_community_id() != Some(target.community_id) {
            return Ok(());
        }
        let community = target.community_id;
        let channel = target.channel_id;
        let root = target.thread_root_id;
        let body = self.composer.body.clone();
        let attachments = self.composer.attachments.clone();
        let mentions = self.composer.mentions().to_vec();
        let revision = self
            .store
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
            .await?;
        self.composer_draft_revision = Some(revision);
        Ok(())
    }

    fn queue_message(
        &mut self,
        message: crate::ui::composer::PreparedMessage,
        submission: DraftSubmission,
    ) {
        let Some(runtime) = &self.runtime else { return };
        let Some(target) = self.presentation.composer_target.clone() else {
            return;
        };
        if runtime.community_id != target.community_id {
            self.status_error = Some("the composer target belongs to a different community".into());
            return;
        }
        let service = runtime.messages.clone();
        let channel = target.channel_id;
        let root = target.thread_root_id;
        let parent = target.parent_event_id.or_else(|| root.clone());
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = if let (Some(root), Some(parent)) = (root, parent) {
                service
                    .reply_draft_with_media_mentions(
                        channel,
                        (&root, &parent),
                        &message.body,
                        &message.attachments,
                        &message.mentions,
                        submission,
                    )
                    .await
            } else {
                service
                    .send_draft_with_media_mentions(
                        channel,
                        &message.body,
                        &message.attachments,
                        &message.mentions,
                        submission,
                    )
                    .await
            };
            let _ = tx
                .send(match result {
                    Ok(_) => Background::DraftAcknowledged,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
        self.cache_dirty = true;
    }
    fn send_reaction(&mut self) {
        if let Some(reason) = self.current_channel().and_then(Channel::read_only_reason) {
            self.status_error = Some(reason.notice().into());
            self.presentation.close_overlay();
            return;
        }
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
        if own_reaction.is_some() && self.refused_by_signer("remove a reaction", &[5]) {
            return;
        }
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
        self.presentation.close_overlay();
    }
    fn refresh_reaction_candidates(&mut self) {
        let Some(query) = self.reaction_query.clone() else {
            return;
        };
        self.reaction_candidates = crate::protocol::emoji::matches(&self.emoji_palette, &query, 8);
        self.reaction_candidate = 0;
        // NIP-CE prefix search over the relay index (at most 50 hits, as the
        // spec asks); the relay skips prefixes shorter than three characters.
        if query.chars().count() < 3 {
            return;
        }
        let Some(runtime) = &self.runtime else { return };
        let (http, community, store, tx) = (
            runtime.http.clone(),
            runtime.community_id,
            self.store.clone(),
            self.background_tx.clone(),
        );
        tokio::spawn(async move {
            let Ok(events) = http
                .query(&[crate::protocol::types::QueryFilter {
                    kinds: vec![u32::from(crate::protocol::emoji::KIND_EMOJI_ENTRY)],
                    search: Some(query.clone()),
                    search_mode: Some(crate::protocol::types::SearchMode::Prefix),
                    limit: Some(50),
                    ..crate::protocol::types::QueryFilter::default()
                }])
                .await
            else {
                return;
            };
            let found = events
                .iter()
                .filter(|event| crate::protocol::events::verify(event).is_ok())
                .flat_map(crate::protocol::emoji::contributions)
                .map(|contribution| contribution.emoji)
                .collect::<Vec<_>>();
            let _ = store
                .call(move |store| {
                    for event in events {
                        let _ = store.apply_event(community, &event);
                    }
                    Ok(())
                })
                .await;
            let _ = tx.send(Background::EmojiSearch { query, found }).await;
        });
    }

    fn send_custom_reaction(&mut self) {
        if let Some(reason) = self.current_channel().and_then(Channel::read_only_reason) {
            self.status_error = Some(reason.notice().into());
            self.presentation.close_overlay();
            return;
        }
        let Some(emoji) = self
            .reaction_candidates
            .get(self.reaction_candidate)
            .cloned()
        else {
            return;
        };
        let (Some(runtime), Some(message)) = (&self.runtime, self.selected_message()) else {
            return;
        };
        let service = runtime.messages.clone();
        let target = message.event_id.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = service
                .react_custom(&target, &emoji.shortcode, &emoji.url)
                .await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Changed,
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
        self.reaction_query = None;
        self.reaction_candidates.clear();
        self.presentation.close_overlay();
    }

    /// `:presence [on|off]`. Off is the default; turning it off after a
    /// publish sends `offline` once, so others do not see a stale dot.
    async fn presence_command(&mut self, argument: &str) -> Result<()> {
        let enable = match argument {
            "" => None,
            "on" => Some(true),
            "off" => Some(false),
            _ => {
                self.status_error = Some("usage: :presence [on|off]".into());
                return Ok(());
            }
        };
        if let Some(enable) = enable
            && enable != self.config.ui.publish_presence
        {
            self.config.ui.publish_presence = enable;
            self.config.save(&self.paths)?;
            if enable {
                self.maybe_publish_presence().await;
            } else if self.last_presence_publish.take().is_some()
                && let Some(runtime) = &self.runtime
            {
                publish_offline(runtime).await;
            }
        }
        self.status_error = Some(
            if self.config.ui.publish_presence {
                "presence: on · online while active, away after inactivity, offline on exit"
            } else {
                "presence: off · nothing is published"
            }
            .into(),
        );
        Ok(())
    }

    /// `:status`: your status as the community holds it, and how to change
    /// it (v0.17 D6).
    async fn show_status(&mut self) -> Result<()> {
        let (Some(community), Some(pubkey)) = (
            self.active_community_id(),
            self.self_pubkey().map(str::to_owned),
        ) else {
            self.status_error = Some("no identity here, so no status".into());
            return Ok(());
        };
        let status = self
            .store
            .call(move |store| store.user_status(community, &pubkey))
            .await?;
        let now = crate::clock::SystemClock.now_unix();
        self.status_error = Some(status_notice(status.as_ref(), now));
        Ok(())
    }

    fn set_status(&mut self, arguments: &str) {
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return;
        };
        let now = crate::clock::SystemClock.now_unix();
        let (text, expires) = if arguments == "clear" {
            (String::new(), None)
        } else if let Some(rest) = arguments.strip_prefix("--expires ") {
            let (expiry, text) = rest.split_once(' ').unwrap_or((rest, ""));
            match crate::protocol::presence::parse_expiry(expiry, now) {
                Some(at) => (text.trim().to_owned(), Some(at)),
                None => {
                    self.status_error =
                        Some("usage: :status [--expires <unix|N[smhdw]>] <text> | clear".into());
                    return;
                }
            }
        } else {
            (arguments.to_owned(), None)
        };
        let builder = match buzz_sdk::build_user_status_expiring(&text, None, expires) {
            Ok(builder) => builder,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return;
            }
        };
        let service = runtime.messages.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = service.publish_builder(builder).await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Notice(if text.is_empty() {
                        "status cleared".into()
                    } else {
                        "status updated".into()
                    }),
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
    }

    fn send_pending_report(&mut self) {
        let Some((reason, note, author_only)) = self.pending_report.take() else {
            return;
        };
        let Some(runtime) = &self.runtime else { return };
        let Some(message) = self.selected_message() else {
            return;
        };
        let service = runtime.messages.clone();
        let (target, author) = (message.event_id.clone(), message.pubkey.clone());
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = service
                .report(
                    (!author_only).then_some(target.as_str()),
                    &author,
                    &reason,
                    note.as_deref(),
                )
                .await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Notice("report sent to the moderators".into()),
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
    }

    /// `:tochannel`: send the open thread's draft to the channel as a new
    /// top-level message that points back at the thread.
    fn send_draft_to_channel(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let (Some(root), Some(channel)) = (
            self.thread_root.clone(),
            self.current_channel().map(|channel| channel.id),
        ) else {
            self.status_error = Some("open a thread to send its draft to the channel".into());
            return;
        };
        if let Some(reason) = self.current_channel().and_then(Channel::read_only_reason) {
            self.status_error = Some(reason.notice().into());
            return;
        }
        let root_message = self
            .thread_messages
            .iter()
            .find(|message| message.event_id == root)
            .cloned();
        let root_message = match send_to_channel_root(root_message, channel) {
            Ok(message) => message,
            Err(reason) => {
                self.status_error = Some(reason.into());
                return;
            }
        };
        let content = self.composer.body.trim().to_owned();
        if content.is_empty() {
            self.status_error = Some("write the message in the thread composer first".into());
            return;
        }
        let mentions = self
            .composer
            .mentions()
            .iter()
            .map(|mention| mention.pubkey.clone())
            .collect::<Vec<_>>();
        let service = runtime.messages.clone();
        let tx = self.background_tx.clone();
        // The thread draft stays until the user clears it; the channel copy
        // goes through the same acknowledgement-aware outbox as any send.
        tokio::spawn(async move {
            let result = service
                .send_to_channel(channel, &root, &root_message.content, &content, &mentions)
                .await;
            let _ = tx
                .send(match result {
                    Ok(_) => Background::Notice("sent to the channel".into()),
                    Err(error) => Background::Failed(error.to_string()),
                })
                .await;
        });
    }

    /// The edit in progress, while the composer is still open on its channel.
    fn active_edit(&self) -> Option<&EditingMessage> {
        self.editing.as_ref().filter(|editing| {
            self.presentation
                .composer_target
                .as_ref()
                .is_some_and(|target| target.channel_id == editing.channel_id)
        })
    }

    /// Read the relay's current head of a sidebar document, apply `apply`,
    /// and publish the result once; then confirm it is the head. A head bzz
    /// cannot read is never overwritten.
    async fn sidebar_write(
        &mut self,
        lane: crate::protocol::sidebar::Lane,
        apply: impl FnOnce(
            &mut crate::protocol::sidebar_doc::Edit<'_>,
            &crate::protocol::sidebar_doc::Tree,
        ) -> Result<()>,
    ) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return Ok(());
        };
        let community = runtime.community_id;
        let (http, signer, messages) = (
            runtime.http.clone(),
            runtime.signer.clone(),
            runtime.messages.clone(),
        );
        let result = crate::service::sidebar::write(&http, &signer, &messages, lane, apply).await;
        match result {
            Ok(Some((event, tree))) => {
                let (pubkey, id, at) = (
                    event.pubkey.to_hex(),
                    event.id.to_hex(),
                    event.created_at.as_secs(),
                );
                self.store
                    .call(move |store| {
                        store.record_sidebar_document(community, &pubkey, lane, &id, at, &tree)
                    })
                    .await?;
                self.cache_dirty = true;
                self.status_error = Some("sidebar updated".into());
            }
            Ok(None) => self.status_error = Some("sidebar unchanged".into()),
            Err(error) => self.status_error = Some(format!("sidebar not changed: {error}")),
        }
        Ok(())
    }

    /// `:section new <name> | rename <name> | delete | move <name> | remove |
    /// up | down`, for the selected channel's section.
    async fn section_command(&mut self, arguments: &str) -> Result<()> {
        use crate::protocol::{sidebar::Lane, sidebar_doc};
        use serde_json::json;
        let (verb, rest) = arguments.split_once(' ').unwrap_or((arguments, ""));
        let rest = rest.trim().to_owned();
        let channel = self
            .current_channel()
            .map(|channel| channel.id.hyphenated().to_string());
        let current = self.current_channel().and_then(|channel| {
            crate::ui::sidebar::channel_section(channel, &self.sidebar_layout).map(|s| s.id.clone())
        });
        let usage = "usage: :section new <name> | rename <name> | delete | move <name> | remove | up | down";
        let need =
            |what: Option<String>, why: &str| what.ok_or_else(|| Error::Config(why.to_owned()));
        let channel_needed = || need(channel.clone(), "select a channel first");
        let section_needed = || need(current.clone(), "the selected channel is in no section");
        let resolve = |view: &sidebar_doc::Projection, key: &str| -> Result<String> {
            let mut named = view
                .sections
                .iter()
                .filter(|section| section.name == key.trim());
            match (named.next(), named.next()) {
                (Some(section), None) => Ok(section.id.clone()),
                (Some(_), Some(_)) => Err(Error::Config(format!(
                    "more than one section is named \"{key}\""
                ))),
                (None, _) => Err(Error::Config(format!("no section is named \"{key}\""))),
            }
        };
        let plan: Result<SectionEdit> = match verb {
            "new" | "create" if !rest.is_empty() => {
                let id = Uuid::new_v4().to_string();
                let channel = channel.clone();
                Ok(Box::new(move |edit, _| {
                    edit.create_section(&id, &rest, None);
                    if let Some(channel) = channel {
                        edit.set(&["a", &channel], json!(id));
                    }
                    Ok(())
                }))
            }
            "rename" if !rest.is_empty() => section_needed().map(|id| {
                Box::new(
                    move |edit: &mut sidebar_doc::Edit<'_>, _: &sidebar_doc::Tree| {
                        edit.set(&["s", &id, "name"], json!(rest));
                        Ok(())
                    },
                ) as Box<_>
            }),
            "delete" => section_needed().map(|id| {
                Box::new(
                    move |edit: &mut sidebar_doc::Edit<'_>, _: &sidebar_doc::Tree| {
                        edit.set(&["s", &id, "live"], json!(false));
                        Ok(())
                    },
                ) as Box<_>
            }),
            "move" if !rest.is_empty() => channel_needed().map(|channel| {
                Box::new(
                    move |edit: &mut sidebar_doc::Edit<'_>, head: &sidebar_doc::Tree| {
                        let id = resolve(&sidebar_doc::project(head), &rest)?;
                        edit.set(&["a", &channel], json!(id));
                        Ok(())
                    },
                ) as Box<_>
            }),
            "remove" => channel_needed().map(|channel| {
                Box::new(
                    move |edit: &mut sidebar_doc::Edit<'_>, _: &sidebar_doc::Tree| {
                        edit.clear(&["a", &channel]);
                        Ok(())
                    },
                ) as Box<_>
            }),
            "up" | "down" => section_needed().map(|id| {
                let up = verb == "up";
                Box::new(
                    move |edit: &mut sidebar_doc::Edit<'_>, head: &sidebar_doc::Tree| {
                        let mut ids = sidebar_doc::project(head)
                            .sections
                            .into_iter()
                            .map(|section| section.id)
                            .collect::<Vec<_>>();
                        if let Some(at) = ids.iter().position(|live| *live == id) {
                            let to = if up {
                                at.saturating_sub(1)
                            } else {
                                (at + 1).min(ids.len() - 1)
                            };
                            ids.swap(at, to);
                        }
                        edit.reorder(&ids);
                        Ok(())
                    },
                ) as Box<_>
            }),
            _ => Err(Error::Config(usage.into())),
        };
        match plan {
            Ok(apply) => self.sidebar_write(Lane::Sections, apply).await,
            Err(error) => {
                self.status_error = Some(match error {
                    Error::Config(message) => message,
                    other => other.to_string(),
                });
                Ok(())
            }
        }
    }

    /// `:sort alpha | recent | default` for the selected channel's group.
    async fn sort_command(&mut self, arguments: &str) -> Result<()> {
        use serde_json::json;
        let mode = match arguments.trim() {
            "alpha" => Some("alpha"),
            "recent" => Some("recent"),
            "default" | "reset" => None,
            _ => {
                self.status_error = Some("usage: :sort alpha | recent | default".into());
                return Ok(());
            }
        };
        let Some(key) = self.current_channel().and_then(|channel| {
            crate::ui::sidebar::channel_sort_key(channel, &self.sidebar_layout)
        }) else {
            self.status_error = Some("this group has no synced sort mode".into());
            return Ok(());
        };
        self.sidebar_write(crate::protocol::sidebar::Lane::Sort, move |edit, _| {
            match mode {
                Some(mode) => edit.set(&["g", &key], json!(mode)),
                None => edit.clear(&["g", &key]),
            }
            Ok(())
        })
        .await
    }

    /// `:solo <name>`: create a Solo chat as Buzz 1.0 does (kind 9007,
    /// private stream labeled `buzz.solo`; the relay records the creator).
    async fn solo_command(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            self.status_error = Some("usage: :solo <name>".into());
            return Ok(());
        }
        if self.refused_by_signer("create a Solo chat", &[9007]) {
            return Ok(());
        }
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return Ok(());
        };
        let builder = match crate::protocol::sidebar::solo_creation(Uuid::new_v4(), name) {
            Ok(builder) => builder,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return Ok(());
            }
        };
        match runtime.messages.publish_builder(builder).await {
            Ok(_) => {
                self.status_error = Some(format!("Solo chat \"{name}\" created"));
                self.spawn_directory_refresh();
            }
            Err(error) => self.status_error = Some(format!("Solo chat not created: {error}")),
        }
        Ok(())
    }

    /// Refuse an action locally when the signer (Buzz Desktop) will not sign
    /// one of its kinds, so it never fails after the person commits to it.
    fn refused_by_signer(&mut self, action: &str, kinds: &[u16]) -> bool {
        let reason = self
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.signer.refusal_any(kinds));
        if let Some(reason) = reason {
            self.status_error = Some(format!("{action}: {reason}"));
        }
        reason.is_some()
    }

    /// Open the selected own message in the composer for editing.
    async fn open_edit(&mut self) -> Result<()> {
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return Ok(());
        };
        let own = runtime.signer.public_key().to_hex();
        let community = runtime.community_id;
        let Some(message) = self.selected_message().cloned() else {
            return Ok(());
        };
        let reason = self
            .channels
            .iter()
            .find(|channel| channel.id == message.channel_id)
            .and_then(Channel::read_only_reason);
        if message.pubkey != own {
            self.status_error = Some("only your own message can be edited".into());
        } else if message.deleted || message.system.is_some() {
            self.status_error = Some("this message cannot be edited".into());
        } else if message.delivery != crate::domain::DeliveryState::Delivered {
            self.status_error = Some("wait until the message is delivered, then edit it".into());
        } else if let Some(reason) = reason {
            self.status_error = Some(reason.notice().into());
        } else {
            self.mention_picker = None;
            self.staging_media.clear();
            self.staging_attachments.clear();
            self.composer_draft_revision = None;
            self.composer
                .set_draft(message.content.clone(), Vec::new(), Vec::new());
            self.editing = Some(EditingMessage {
                channel_id: message.channel_id,
                event_id: message.event_id.clone(),
                kept_attachments: message.attachments.len(),
            });
            self.presentation.composer_target = Some(ComposerTarget {
                community_id: community,
                channel_id: message.channel_id,
                thread_root_id: message.root_event_id.clone(),
                parent_event_id: None,
            });
        }
        Ok(())
    }

    /// Publish the composer's text as an edit of the message being edited.
    /// On failure the text stays in the composer.
    async fn save_edit(&mut self) -> Result<()> {
        let Some(editing) = self.active_edit().cloned() else {
            return Ok(());
        };
        if !self.composer.attachments.is_empty() || !self.staging_attachments.is_empty() {
            self.status_error =
                Some("an edit keeps the message's attachments; remove the new ones to save".into());
            return Ok(());
        }
        if self.composer.body.trim().is_empty() && editing.kept_attachments == 0 {
            self.status_error = Some("an edit needs text; delete the message instead".into());
            return Ok(());
        }
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let author = runtime.signer.public_key().to_hex();
        let body = self.composer.body.clone();
        match runtime
            .messages
            .edit(
                &runtime.http,
                editing.channel_id,
                &editing.event_id,
                &author,
                &body,
            )
            .await
        {
            Ok(_) => {
                self.editing = None;
                self.composer
                    .set_draft(String::new(), Vec::new(), Vec::new());
                self.presentation.composer_target = None;
                self.cache_dirty = true;
            }
            Err(error) => {
                self.status_error = Some(format!("edit not saved; your text is kept ({error})"));
            }
        }
        Ok(())
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
                    Err(error) => Background::Failed(format!("delete: {error}")),
                })
                .await;
        });
    }

    /// Fill each visible message's emoji from the palette (its own tags win),
    /// and look up still-unknown shortcodes once, batched by NIP-CE `#d`.
    async fn resolve_custom_emoji(&mut self, community: Uuid) -> Result<()> {
        self.emoji_palette = self
            .store
            .call(move |store| store.emoji_palette(community))
            .await?;
        let mut unknown = Vec::new();
        for message in self
            .messages
            .iter_mut()
            .chain(self.thread_messages.iter_mut())
        {
            if !message.content.contains(':') {
                continue;
            }
            for shortcode in crate::protocol::emoji::scan_shortcodes(&message.content) {
                if message
                    .emoji
                    .iter()
                    .any(|emoji| emoji.shortcode == shortcode)
                {
                    continue;
                }
                match self.emoji_palette.get(&shortcode) {
                    Some(url) => message.emoji.push(crate::protocol::emoji::Emoji {
                        shortcode,
                        url: url.clone(),
                    }),
                    None => unknown.push(shortcode),
                }
            }
        }
        unknown.retain(|shortcode| {
            self.emoji_lookups.len() < 4096 && self.emoji_lookups.insert(shortcode.clone())
        });
        if unknown.is_empty() {
            return Ok(());
        }
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let http = runtime.http.clone();
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let mut changed = false;
            for chunk in unknown.chunks(crate::protocol::emoji::MAX_LOOKUP_NAMES) {
                let Ok(events) = http
                    .query(&[crate::protocol::types::QueryFilter {
                        kinds: vec![u32::from(crate::protocol::emoji::KIND_EMOJI_ENTRY)],
                        limit: Some(u32::try_from(chunk.len() * 4).unwrap_or(200)),
                        ..crate::protocol::types::QueryFilter::default()
                    }
                    .tag("d", chunk.to_vec())])
                    .await
                else {
                    return;
                };
                let applied = store
                    .call(move |store| {
                        let mut any = false;
                        for event in events {
                            any |= store.apply_event(community, &event).unwrap_or(false);
                        }
                        Ok(any)
                    })
                    .await
                    .unwrap_or(false);
                changed |= applied;
            }
            if changed {
                let _ = tx.send(Background::Changed).await;
            }
        });
        Ok(())
    }

    /// People whose presence is visible now: DM participants and recent
    /// authors, bounded.
    fn presence_subjects(&self) -> Vec<String> {
        let mut subjects = std::collections::BTreeSet::new();
        for message in self.messages.iter().rev().take(200) {
            subjects.insert(message.pubkey.clone());
        }
        for agent in self.agents.values() {
            subjects.insert(agent.pubkey.clone());
        }
        subjects.into_iter().take(100).collect()
    }

    fn spawn_presence_snapshot(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let asked = self.presence_subjects();
        if asked.is_empty() {
            return;
        }
        let http = runtime.http.clone();
        let community = runtime.community_id;
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let relay = store
                .call(move |store| store.pinned_relay_pubkey(community))
                .await
                .ok()
                .flatten();
            let Ok(events) = http
                .query(&[crate::protocol::types::QueryFilter {
                    kinds: vec![u32::from(crate::protocol::presence::KIND_PRESENCE)],
                    authors: asked.clone(),
                    limit: Some(u32::try_from(asked.len()).unwrap_or(100)),
                    ..crate::protocol::types::QueryFilter::default()
                }])
                .await
            else {
                return;
            };
            let observations = events
                .iter()
                .filter(|event| crate::protocol::events::verify(event).is_ok())
                .filter(|event| relay.as_deref() == Some(event.pubkey.to_hex().as_str()))
                .filter_map(|event| {
                    crate::protocol::presence::observe(event, relay.as_deref(), |_| None)
                })
                .collect();
            let _ = tx
                .send(Background::PresenceSnapshot {
                    community,
                    asked,
                    observations,
                })
                .await;
        });
    }

    /// Opt-in own presence: online while active, away after inactivity,
    /// heartbeat within the relay TTL, maintenance priority.
    async fn maybe_publish_presence(&mut self) {
        if !self.config.ui.publish_presence {
            return;
        }
        let Some(runtime) = &self.runtime else { return };
        let wanted = if self.last_input_at.elapsed()
            >= Duration::from_secs(crate::protocol::presence::AWAY_AFTER_SECONDS)
        {
            crate::protocol::presence::Presence::Away
        } else {
            crate::protocol::presence::Presence::Online
        };
        let due = self.last_presence_publish.is_none_or(|(at, last)| {
            last != wanted
                || at.elapsed() >= Duration::from_secs(crate::protocol::presence::HEARTBEAT_SECONDS)
        });
        if !due {
            return;
        }
        self.last_presence_publish = Some((Instant::now(), wanted));
        publish_presence(runtime, wanted);
        let agents = self.hosted_agent_pubkeys();
        if self.config.ui.broker_agent_presence && !agents.is_empty() {
            publish_agent_presence(runtime, wanted, agents);
        }
    }

    /// Hosted agents the broker is serving right now.
    fn hosted_agent_pubkeys(&self) -> Vec<String> {
        let Some(broker) = &self.broker else {
            return Vec::new();
        };
        self.config
            .hosted_agents
            .iter()
            .filter(|agent| agent.community == broker.community && !agent.paused)
            .map(|agent| agent.pubkey.clone())
            .collect()
    }

    async fn begin_federated_sign_in(&mut self) {
        self.presentation.close_overlay();
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "cached read-only mode: restore or unlock the identity, then restart bzz".into(),
            );
            return;
        };
        let Some(adapter) = runtime.identity_adapter.clone() else {
            self.status_error =
                Some("set identity_adapter for this community to use federated sign-in".into());
            return;
        };
        let client = match crate::auth::federated::AdapterClient::new(adapter) {
            Ok(client) => client,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return;
            }
        };
        let pending = match client.begin_login().await {
            Ok(pending) => pending,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return;
            }
        };
        open_in_browser(&pending.url);
        // The login URL carries only the S256 challenge, never a secret.
        self.status_error = Some(format!(
            "federated sign-in: finish in your browser · {}",
            pending.url
        ));
        let tx = self.background_tx.clone();
        let signer = runtime.signer.clone();
        let evidence = runtime.evidence.clone();
        let store = crate::auth::federated::SessionStore::new(&runtime.data_dir);
        let identity = runtime.identity_id;
        tokio::spawn(async move {
            let outcome = async {
                let session = client
                    .complete_login(pending, std::time::Duration::from_secs(300))
                    .await?;
                store
                    .save(identity, &client.origin(), &session, &signer)
                    .await?;
                evidence.set(crate::auth::federated::AdapterEvidence::new(
                    client,
                    session,
                    signer,
                    crate::clock::system(),
                ));
                Ok::<_, Error>(())
            }
            .await;
            let _ = tx
                .send(match outcome {
                    Ok(()) => Background::FederatedReady,
                    Err(error) => Background::Notice(error.to_string()),
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
    /// The time up to which `channel` was read, for the `── new ──` line; a
    /// channel never read counts from its latest message here, so only what
    /// arrives from now on is new.
    async fn channel_read_at(&self, channel: Uuid) -> Result<Option<u64>> {
        let (Some(community), Some(identity)) = (
            self.active_community_id(),
            self.self_pubkey().map(str::to_owned),
        ) else {
            return Ok(None);
        };
        let contexts = self
            .store
            .call(move |store| store.read_contexts(community, &identity, false))
            .await?;
        let read_at = contexts
            .get(&channel.to_string())
            .copied()
            .map_or(0, u64::from);
        Ok(Some(if read_at == 0 {
            self.messages
                .iter()
                .map(|message| message.created_at)
                .max()
                .unwrap_or(0)
        } else {
            read_at
        }))
    }

    /// The first message after the read time the channel had when it was
    /// opened, written by someone else: the `── new ──` line goes before it.
    fn new_marker(&self) -> Option<String> {
        let (channel, since) = self.new_since?;
        if self.current_channel().map(|current| current.id) != Some(channel) {
            return None;
        }
        let me = self.self_pubkey();
        self.messages
            .iter()
            .find(|message| {
                message.created_at > since
                    && message.system.is_none()
                    && me.is_none_or(|me| !message.pubkey.eq_ignore_ascii_case(me))
            })
            .map(|message| message.event_id.clone())
    }

    /// One line over the conversation when bzz is not online: what it shows
    /// and what to do next (L5).
    fn connection_banner(&self) -> Option<String> {
        let saved = "showing what is saved here";
        match self.connection {
            ConnectionState::Offline if self.runtime.is_some() => Some(format!(
                "offline · {saved} · bzz retries on its own · :reconnect tries now"
            )),
            ConnectionState::Offline => Some(format!("offline · {saved}")),
            ConnectionState::Locked
            | ConnectionState::IdentityMissing
            | ConnectionState::IdentityCorrupt => Some(format!(
                "{} · {saved} · restore or unlock the identity, then restart bzz",
                crate::ui::status::connection_label(self.connection)
            )),
            ConnectionState::AccessDenied => Some(
                "access denied · ask the community's admin for access, then :reconnect".into(),
            ),
            ConnectionState::ClockSkew => Some(
                "clock skew · the relay refuses this machine's time · set the clock, then :reconnect"
                    .into(),
            ),
            ConnectionState::Connecting
            | ConnectionState::Authenticating
            | ConnectionState::Online
            | ConnectionState::Backfilling => None,
        }
    }

    /// What to do in an empty conversation (L5).
    fn empty_hint(&self) -> Option<String> {
        let channel = self.current_channel()?;
        Some(if let Some(reason) = channel.read_only_reason() {
            reason.notice().to_owned()
        } else if self.runtime.is_none() {
            "Read-only: restore or unlock the identity to write.".to_owned()
        } else {
            "i writes the first one".to_owned()
        })
    }

    async fn mark_current_read(&mut self) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let Some(identity) = self.self_pubkey().map(str::to_owned) else {
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            return Ok(());
        };
        let mut marks = Vec::new();
        let mut visible_channel = false;
        if self.presentation.focus == FocusSurface::Context
            && let Some(root) = &self.thread_root
            && let Some(last) = self.thread_messages.last()
            && last.channel_id == channel
        {
            visible_channel = true;
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
        } else if let Some((context, displayed_at)) =
            timeline_read_mark(&self.timeline, channel, &self.messages)
        {
            // A channel view renders top-level messages only, but its unread
            // badge includes replies. Opening the live edge therefore
            // acknowledges the latest local channel activity, not only the
            // latest rendered root message.
            let latest = self
                .store
                .call(move |store| store.latest_channel_activity_at(community, channel))
                .await?
                .unwrap_or(u64::from(displayed_at));
            visible_channel = true;
            marks.push((context, u32::try_from(latest).unwrap_or(u32::MAX)));
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
            // Recording the local read state must not depend on a live relay
            // session. Publishing is separately debounced below when a runtime
            // is available.
            let identity = identity.clone();
            let context_for_store = context.clone();
            self.store
                .call(move |store| {
                    store.advance_read(community, &identity, &context_for_store, at, true)
                })
                .await?;
            self.last_marked.insert(context, at);
            advanced = true;
        }
        if visible_channel {
            self.unread_counts.remove(&channel);
        }
        if visible_channel
            && clear_visible_unread(&mut self.computed_unread, &mut self.manual_unread, channel)
        {
            self.persist_manual_unread().await?;
        }
        if advanced && self.runtime.is_some() {
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
        self.presentation.enter_inbox();
        self.inbox_state.narrow_detail = false;
        self.inbox_messages.clear();
        self.spawn_inbox_load(self.runtime.is_some());
    }

    fn open_search(&mut self) {
        self.presentation.open_overlay(Overlay::Search);
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
        if add_to.is_some()
            && self.refused_by_signer(
                "add someone to a DM",
                &[buzz_core::kind::KIND_DM_ADD_MEMBER as u16],
            )
        {
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
        self.presentation.open_overlay(Overlay::DmPicker);
    }

    fn clear_agent_directory_state(&mut self) {
        if let Some(task) = self.agent_directory_task.take() {
            task.abort();
        }
        self.agent_directory = AgentDirectoryState::default();
        // The panel and the form belong to the community that was open.
        self.local_agent_panel = crate::ui::local_agents::PanelState::default();
        self.local_agent_turns.clear();
        self.agent_form = None;
        if matches!(
            self.presentation.overlay,
            Some(Overlay::Agents | Overlay::LocalAgents | Overlay::AgentNew)
        ) {
            self.presentation.close_overlay();
        }
    }

    fn open_agents(&mut self, refresh_remote: bool) {
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("configure a community before opening agents".into());
            return;
        };
        let Some(identity_pubkey) = self.self_pubkey().map(str::to_owned) else {
            self.status_error = Some("an active identity is required to inspect agents".into());
            return;
        };
        self.presentation.open_overlay(Overlay::Agents);
        self.agent_directory.loading = true;
        if let Some(task) = self.agent_directory_task.take() {
            task.abort();
        }
        let active_is_dm = self
            .current_channel()
            .is_some_and(|channel| channel.kind.is_dm());
        let refresh_unavailable = refresh_remote && self.runtime.is_none();
        let service = refresh_remote
            .then(|| {
                self.runtime
                    .as_ref()
                    .map(|runtime| runtime.channels.clone())
            })
            .flatten();
        let store = self.store.clone();
        let tx = self.background_tx.clone();
        let diagnostics = self.diagnostics.clone();
        self.agent_directory_task = Some(tokio::spawn(async move {
            let refresh_started = Instant::now();
            let refresh = if let Some(service) = service {
                service.refresh(&identity_pubkey).await.map(|report| {
                    diagnostics.emit(DiagnosticEvent::AgentDirectoryRefreshed {
                        candidates: u32::try_from(report.agent_candidates).unwrap_or(u32::MAX),
                        verified: u32::try_from(report.verified_agents).unwrap_or(u32::MAX),
                        projection_changes: u32::try_from(report.agent_projection_changes)
                            .unwrap_or(u32::MAX),
                        duration_ms: u64::try_from(refresh_started.elapsed().as_millis())
                            .unwrap_or(u64::MAX),
                    });
                })
            } else {
                Ok(())
            };
            let notice = if refresh_unavailable {
                Some(
                    "cached read-only mode: remote agent refresh requires an unlocked identity"
                        .into(),
                )
            } else {
                refresh.err().map(|error| {
                    format!("remote refresh unavailable; showing verified cache ({error})")
                })
            };
            let listing_identity = identity_pubkey.clone();
            let result = store
                .call(move |store| store.list_remote_agents(community, &listing_identity))
                .await;
            let event = match result {
                Ok(mut agents) => {
                    for agent in &mut agents {
                        if !agent.stale {
                            agent.eligibility = crate::agents::policy::evaluate(
                                agent.respond_to,
                                &agent.respond_to_allowlist,
                                &agent.owner_pubkey,
                                &identity_pubkey,
                                active_is_dm,
                            );
                        }
                    }
                    Background::AgentsLoaded {
                        community,
                        agents,
                        notice,
                    }
                }
                Err(error) => Background::Failed(error.to_string()),
            };
            let _ = tx.send(event).await;
        }));
    }

    // ---- `:agents` and `:agent new` (v0.15 M4) ----------------------------

    /// The local agents of the open community that its identity owns, in
    /// the order of the settings.
    fn local_agent_configs(&self) -> Vec<&crate::config::HostedAgentConfig> {
        let Some(community) = self.config.communities.get(self.selected_community) else {
            return Vec::new();
        };
        self.config
            .hosted_agents
            .iter()
            .filter(|agent| {
                agent.local.is_some()
                    && agent.community == community.id
                    && agent.owner_identity == community.identity_id
            })
            .collect()
    }

    /// One row per local agent: what its settings say, what its process is
    /// doing, and how its last turn ended.
    fn local_agent_rows(&self) -> Vec<crate::ui::local_agents::AgentRow> {
        let running = self
            .local_agents
            .as_ref()
            .map(|agents| agents.host.status())
            .unwrap_or_default();
        self.local_agent_configs()
            .into_iter()
            .filter_map(|agent| {
                let local = agent.local.as_ref()?;
                let status = running.iter().find(|status| status.id == agent.id);
                Some(crate::ui::local_agents::AgentRow {
                    id: agent.id,
                    name: agent.label.clone(),
                    harness: local.harness,
                    state: match status {
                        Some(status) => status.state_text(),
                        None => format!("{} · not running", crate::agents::manage::state_of(agent)),
                    },
                    doing: status.and_then(crate::agents::runtime::agent::AgentStatus::doing),
                    last: self.local_agent_turns.get(&agent.id).map(|turn| {
                        format!(
                            "{} · {}",
                            crate::agents::manage::outcome(turn),
                            crate::agents::manage::when(turn.updated_at)
                        )
                    }),
                    settings: crate::agents::manage::settings_lines(local),
                    record: crate::agents::capability::record_for(local).lines(),
                })
            })
            .collect()
    }

    fn open_local_agents(&mut self) {
        if self.active_community_id().is_none() {
            self.status_error = Some("configure a community before opening agents".into());
            return;
        }
        let rows = self.local_agent_configs().len();
        self.local_agent_panel.move_by(0, rows);
        self.presentation.open_overlay(Overlay::LocalAgents);
        self.refresh_local_agent_panel();
    }

    /// Reads again what the panel shows that is not in memory: how each
    /// agent's last turn ended, and the end of the selected agent's log.
    fn refresh_local_agent_panel(&mut self) {
        self.local_agent_refreshed = Some(Instant::now());
        let Some(community) = self.active_community_id() else {
            return;
        };
        let agents = self
            .local_agent_configs()
            .into_iter()
            .map(|agent| agent.id)
            .collect::<Vec<_>>();
        if self.local_agent_panel.show_log
            && let Some(agent) = agents.get(self.local_agent_panel.selected)
        {
            let log = crate::agents::runtime::process::agent_dir(&self.paths.data_dir, *agent)
                .join("harness.log");
            self.local_agent_panel.log =
                crate::ui::local_agents::log_tail(&read_tail(&log, 32 * 1024), 200, 400);
        }
        let (store, tx) = (self.store.clone(), self.background_tx.clone());
        tokio::spawn(async move {
            let turns = store
                .call(move |store| {
                    agents
                        .into_iter()
                        .map(|agent| {
                            Ok((
                                agent,
                                store.agent_recent_turns(agent, 1)?.into_iter().next(),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .await;
            if let Ok(turns) = turns {
                let _ = tx
                    .send(Background::LocalAgentTurns { community, turns })
                    .await;
            }
        });
    }

    async fn local_agents_key(&mut self, key: KeyEvent) -> Result<()> {
        let agents = self
            .local_agent_configs()
            .into_iter()
            .map(|agent| (agent.id, agent.label.clone(), agent.runs_locally()))
            .collect::<Vec<_>>();
        let selected = agents.get(self.local_agent_panel.selected).cloned();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.presentation.close_overlay(),
            KeyCode::Char('j') | KeyCode::Down => {
                self.local_agent_panel.move_by(1, agents.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.local_agent_panel.move_by(-1, agents.len());
            }
            // The other half of `:agents`: the verified agents others run.
            KeyCode::Tab => self.open_agents(false),
            KeyCode::Char('n') => self.open_agent_form(""),
            KeyCode::Char('l') => {
                self.local_agent_panel.show_log = !self.local_agent_panel.show_log;
                self.refresh_local_agent_panel();
            }
            // Pause, or resume: its process follows on the next tick.
            KeyCode::Char(' ') => {
                if let Some((id, name, runs)) = selected {
                    match crate::agents::manage::set_paused(&mut self.config, id, runs)
                        .and_then(|()| self.config.save(&self.paths))
                    {
                        Ok(()) => {
                            self.local_agents_dirty = true;
                            self.status_error = Some(format!(
                                "{name} {}",
                                if runs { "paused" } else { "resumed" }
                            ));
                        }
                        Err(error) => self.status_error = Some(error.to_string()),
                    }
                }
            }
            // Add it to the channel that is open, as its owner.
            KeyCode::Char('i') => {
                if let Some((id, name, _)) = selected {
                    self.add_selected_agent_to_open_channel(id, &name);
                }
            }
            // Stop what it is answering now; the agent itself stays.
            KeyCode::Char('x') => {
                if let Some((id, name, _)) = selected {
                    let stopped = match &self.local_agents {
                        Some(agents) => agents.host.stop_turns(id).await,
                        None => 0,
                    };
                    self.status_error = Some(match stopped {
                        0 => format!("{name} is not answering anything now"),
                        1 => format!("{name}: its running turn was stopped"),
                        turns => format!("{name}: its {turns} running turns were stopped"),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ---- a real command waits for the owner (v0.15 M7) --------------------

    /// Reads again which commands wait for the owner. `true`: what the
    /// screen shows of them changed.
    fn refresh_approvals(&mut self) -> bool {
        let waiting = self
            .local_agents
            .as_ref()
            .map(|agents| agents.host.approvals())
            .unwrap_or_default();
        let ids = |list: &[crate::agents::runtime::approval::Asked]| {
            list.iter().map(|asked| asked.id).collect::<Vec<_>>()
        };
        let mut changed = ids(&waiting) != ids(&self.approvals);
        self.approvals = waiting;
        // A notice that nothing waits is no longer true.
        if !self.approvals.is_empty() && self.status_error.as_deref() == Some(NOTHING_WAITS) {
            self.status_error = None;
        }
        // The time left is on screen: it is drawn again each second.
        let clock = self.approvals.first().map(seconds_left);
        changed |= clock != self.approval_clock;
        self.approval_clock = clock;
        changed
    }

    /// The conversation a command was asked for in, as the owner knows it.
    fn conversation_label(&self, asked: &crate::agents::runtime::approval::Asked) -> String {
        let thread = if asked.place.contains('/') {
            " (a thread)"
        } else {
            ""
        };
        match self
            .channels
            .iter()
            .find(|channel| channel.id == asked.channel)
        {
            Some(channel) if channel.kind.is_dm() => "a direct message".to_owned(),
            Some(channel) => format!("#{}{thread}", channel.name),
            None => format!("a channel that is not open here{thread}"),
        }
    }

    /// Who wrote the message the agent is answering.
    fn requester_label(&self, asked: &crate::agents::runtime::approval::Asked) -> String {
        if self.self_pubkey() == Some(asked.requester.as_str()) {
            return "you".to_owned();
        }
        self.profiles.get(&asked.requester).map_or_else(
            || crate::domain::abbreviated_pubkey(&asked.requester),
            crate::domain::Profile::label,
        )
    }

    /// An Enter that the composer takes to send: nothing above it (an
    /// overlay, the mention list) takes it first.
    fn composer_takes_enter(&self, key: KeyEvent) -> bool {
        key.code == KeyCode::Enter
            && key.modifiers.is_empty()
            && self.presentation.overlay.is_none()
            && self.presentation.composer_target.is_some()
            && self.mention_picker.is_none()
    }

    /// The key that works on every screen: Ctrl-Y opens the dialog of the
    /// command that waits. `true`: the key was taken.
    fn global_key(&mut self, key: KeyEvent) -> bool {
        let asks = key.code == KeyCode::Char('y')
            && key.modifiers == KeyModifiers::CONTROL
            && !self.approvals.is_empty()
            && self.presentation.overlay != Some(Overlay::Approval);
        if asks {
            self.open_approval();
        }
        asks
    }

    /// Opens the dialog on the command that has waited longest. It is
    /// opened by the owner only, never by an agent's asking.
    fn open_approval(&mut self) {
        self.refresh_approvals();
        let Some(first) = self.approvals.first() else {
            self.status_error = Some(NOTHING_WAITS.into());
            return;
        };
        self.approval.show(first.id);
        // What was open is shown again afterwards: a form half filled in
        // is not lost to a question. A confirmation is not: what it asked
        // about is gone once another dialog has been open over it, and
        // going back to it would leave a prompt with nothing in it. The
        // owner asks again.
        self.approval_return = self.presentation.overlay.filter(|overlay| {
            !matches!(
                overlay,
                Overlay::Approval | Overlay::Command | Overlay::Confirmation
            )
        });
        self.presentation.open_overlay(Overlay::Approval);
    }

    fn close_approval(&mut self) {
        self.approval = crate::ui::approval::ApprovalState::default();
        match self.approval_return.take() {
            Some(overlay) => self.presentation.return_to_overlay(overlay),
            None => self.presentation.close_overlay(),
        }
    }

    /// Gives the owner's answer to the agent runtime, which passes it on
    /// when the question still waits, and says what happened.
    fn answer_approval(&mut self, id: u64, answer: crate::agents::runtime::approval::Answer) {
        let allowed = answer == crate::agents::runtime::approval::Answer::Allow;
        let asked = self.approvals.iter().find(|asked| asked.id == id).cloned();
        let taken = self
            .local_agents
            .as_ref()
            .is_some_and(|agents| agents.host.answer_approval(id, answer));
        self.status_error = Some(match (taken, asked) {
            (true, Some(asked)) if allowed => format!(
                "allowed: {} runs its command in /mnt/{}",
                asked.agent_name, asked.folder
            ),
            (true, Some(asked)) => format!(
                "refused: {} was told no, and nothing was started",
                asked.agent_name
            ),
            _ => {
                "that command no longer waits (it timed out, or its turn ended); it was not started"
                    .to_owned()
            }
        });
        self.approvals.retain(|asked| asked.id != id);
        self.close_approval();
    }

    fn approval_key(&mut self, key: KeyEvent) {
        use crate::agents::runtime::approval::Answer;
        let Some(id) = self.approval.selected else {
            self.close_approval();
            return;
        };
        let waits = self.approvals.iter().any(|asked| asked.id == id);
        let control = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        // Writing why: every key is text, but for the three that end it.
        if self.approval.reason.is_some() {
            match key.code {
                KeyCode::Esc => self.approval.reason = None,
                KeyCode::Enter => {
                    let reason = self.approval.reason.take().unwrap_or_default();
                    self.answer_approval(id, Answer::Refuse(Some(reason)));
                }
                KeyCode::Backspace => {
                    if let Some(reason) = &mut self.approval.reason {
                        reason.pop();
                    }
                }
                KeyCode::Char(character) if !control => {
                    self.approval
                        .type_reason(character.encode_utf8(&mut [0; 4]));
                }
                _ => {}
            }
            return;
        }
        let page = isize::try_from(self.approval.page.max(1)).unwrap_or(1);
        match key.code {
            // The question keeps waiting: the line at the top stays.
            KeyCode::Esc | KeyCode::Char('q') => self.close_approval(),
            KeyCode::Char('j') | KeyCode::Down => self.approval.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.approval.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.approval.scroll_by(page),
            KeyCode::PageUp => self.approval.scroll_by(-page),
            KeyCode::Char('G') | KeyCode::End => self.approval.scroll_by(isize::MAX),
            KeyCode::Char('g') | KeyCode::Home => self.approval.scroll_by(isize::MIN),
            KeyCode::Tab => {
                let next = self
                    .approvals
                    .iter()
                    .position(|asked| asked.id == id)
                    .map_or(0, |index| (index + 1) % self.approvals.len().max(1));
                if let Some(next) = self.approvals.get(next) {
                    self.approval.show(next.id);
                }
            }
            KeyCode::Char('a' | 'y') if !control => {
                if !waits {
                    self.answer_approval(id, Answer::Allow);
                } else if !self.approval.seen_end {
                    // What is approved is what was shown.
                    self.approval.notice = Some(
                        "scroll to the end of the command first (j, PgDn): a yes is for all of it"
                            .into(),
                    );
                } else {
                    self.answer_approval(id, Answer::Allow);
                }
            }
            KeyCode::Char('r' | 'n') if !control => {
                if waits {
                    self.approval.reason = Some(String::new());
                    self.approval.notice = None;
                } else {
                    self.answer_approval(id, Answer::Refuse(None));
                }
            }
            _ => {}
        }
    }

    /// The agents panel's key: adds the selected agent to the channel that
    /// is open, as its owner, with the `bot` role as Buzz adds an agent.
    fn add_selected_agent_to_open_channel(&mut self, id: Uuid, name: &str) {
        use crate::agents::manage;
        let Some(agent) = self
            .config
            .hosted_agents
            .iter()
            .find(|agent| agent.id == id)
            .cloned()
        else {
            return;
        };
        let Some((channel, channel_name)) = self
            .current_channel()
            .map(|channel| (channel.id, channel.name.clone()))
        else {
            self.status_error = Some("open a channel first: the agent is added to it".into());
            return;
        };
        let Some(runtime) = &self.runtime else {
            self.status_error = Some(
                "adding an agent needs your identity unlocked: restart bzz and unlock it".into(),
            );
            return;
        };
        let Some(community) = self
            .config
            .communities
            .iter()
            .find(|entry| entry.id == runtime.community_id)
            .cloned()
        else {
            return;
        };
        let federated = runtime
            .evidence
            .is_set()
            .then(|| runtime.identity_adapter.clone())
            .flatten();
        let relay = match manage::OwnerRelay::from_runtime(
            &self.paths,
            &community,
            runtime.identity_id,
            runtime.signer.clone(),
            runtime.http.clone(),
            runtime.supervisor.clone(),
            federated,
            self.diagnostics.clone(),
        ) {
            Ok(relay) => relay,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return;
            }
        };
        self.status_error = Some(format!("adding {name} to #{channel_name}…"));
        #[cfg(test)]
        self.agent_additions.push((id, channel));
        let (store, tx, name) = (
            self.store.clone(),
            self.background_tx.clone(),
            name.to_owned(),
        );
        tokio::spawn(async move {
            let added = manage::add_to_channel(&relay, &store, &agent, channel).await;
            relay.close().await;
            let notice = match added {
                Ok(added) if added.already => {
                    format!("{name} was already a member of #{}", added.channel_name)
                }
                Ok(added) => format!(
                    "{name} added to #{} with the {} role; mention it there to wake it",
                    added.channel_name, added.role
                ),
                Err(error) => format!("{name} was not added to #{channel_name}: {error}"),
            };
            let _ = tx.send(Background::Notice(notice)).await;
        });
    }

    /// The agents of the open community whose key this bzz holds itself.
    fn own_agents(&self) -> HashMap<String, crate::agents::Own> {
        self.active_community_id()
            .map(|community| crate::agents::own_agents(&self.config.hosted_agents, community))
            .unwrap_or_default()
    }

    fn open_agent_form(&mut self, name: &str) {
        if self.active_community_id().is_none() {
            self.status_error = Some("configure a community before creating an agent".into());
            return;
        }
        if self.runtime.is_none() {
            self.status_error = Some(
                "creating an agent needs your identity unlocked: restart bzz and unlock it".into(),
            );
            return;
        }
        // An identity whose key Buzz Desktop holds cannot have agents: said
        // before the form, the checks and the install of the host.
        let owner = self.runtime.as_ref().and_then(|runtime| {
            self.config
                .identities
                .iter()
                .find(|identity| identity.id == runtime.identity_id)
        });
        if let Some(Err(error)) = owner.map(crate::agents::manage::may_own_agents) {
            self.status_error = Some(error.to_string());
            return;
        }
        self.agent_form = Some(crate::ui::local_agents::NewAgentForm::new(name));
        self.presentation.open_overlay(Overlay::AgentNew);
    }

    async fn agent_form_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::ui::local_agents::FormStage;
        let Some(form) = &mut self.agent_form else {
            self.presentation.close_overlay();
            return Ok(());
        };
        let control = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match (&form.stage, key.code) {
            (_, KeyCode::Esc) => {
                // A check that is still running finds no form and is dropped.
                self.agent_form = None;
                self.presentation.close_overlay();
            }
            (FormStage::Editing, KeyCode::Char('s')) if control => self.check_agent_form(),
            (FormStage::Editing, KeyCode::Enter) if form.focus + 1 == form.fields.len() => {
                self.check_agent_form();
            }
            (FormStage::Editing, KeyCode::Enter | KeyCode::Tab | KeyCode::Down) => {
                form.move_by(1);
            }
            (FormStage::Editing, KeyCode::BackTab | KeyCode::Up) => form.move_by(-1),
            (FormStage::Editing, KeyCode::Backspace) => form.backspace(),
            (FormStage::Editing, KeyCode::Char(character)) if !control => {
                form.insert(character.encode_utf8(&mut [0; 4]));
            }
            (FormStage::Confirm { .. }, KeyCode::Char('e')) => form.stage = FormStage::Editing,
            (FormStage::Confirm { .. }, KeyCode::Enter) => self.create_agent_from_form().await,
            (FormStage::Done(_), KeyCode::Enter | KeyCode::Char('q')) => {
                self.agent_form = None;
                self.presentation.close_overlay();
                self.open_local_agents();
            }
            _ => {}
        }
        Ok(())
    }

    /// Reads the answers and, when they can be read, starts the checks that
    /// can fail before anything exists: Node, the install of the host (made
    /// now when this is its first use) and the model.
    fn check_agent_form(&mut self) {
        use crate::ui::local_agents::FormStage;
        let Some(form) = &mut self.agent_form else {
            return;
        };
        let new = match form.answers().read() {
            Ok(new) => new,
            Err(error) => {
                form.error = Some(error.to_string());
                return;
            }
        };
        if crate::agents::manage::name_taken(&self.config, &new.name, None) {
            form.error = Some(format!("an agent named {} exists", new.name));
            return;
        }
        form.error = None;
        form.stage = FormStage::Checking;
        let (data_dir, environment, tx) = (
            self.paths.data_dir.clone(),
            self.agent_environment.clone(),
            self.background_tx.clone(),
        );
        tokio::spawn(async move {
            let pi_dir = crate::agents::runtime::install::pi_dir(|name| environment(name));
            let result = crate::agents::manage::preflight(
                &data_dir,
                &new,
                pi_dir.as_deref(),
                &environment,
                &|_| {},
            )
            .await
            .map(|preflight| preflight.notes)
            .map_err(|error| error.to_string());
            let _ = tx
                .send(Background::AgentPreflight {
                    new: Box::new(new),
                    result,
                })
                .await;
        });
    }

    fn agent_preflight_done(
        &mut self,
        new: crate::agents::manage::NewAgent,
        result: std::result::Result<Vec<String>, String>,
    ) {
        use crate::ui::local_agents::FormStage;
        // Only the form that asked: one cancelled or edited since is left alone.
        let Some(form) = self
            .agent_form
            .as_mut()
            .filter(|form| form.stage == FormStage::Checking)
        else {
            return;
        };
        match result {
            Ok(notes) => {
                form.stage = FormStage::Confirm {
                    new: Box::new(new),
                    notes,
                }
            }
            Err(error) => {
                form.stage = FormStage::Editing;
                form.error = Some(error);
            }
        }
    }

    /// The owner confirmed: creates the agent, all or nothing, with the
    /// connection and the signer this bzz already holds.
    async fn create_agent_from_form(&mut self) {
        use crate::agents::manage;
        use crate::ui::local_agents::FormStage;
        let Some(FormStage::Confirm { new, .. }) =
            self.agent_form.as_ref().map(|form| form.stage.clone())
        else {
            return;
        };
        let fail = |app: &mut Self, error: String| {
            if let Some(form) = &mut app.agent_form {
                form.stage = FormStage::Editing;
                form.error = Some(error);
            }
        };
        let Some(runtime) = &self.runtime else {
            fail(
                self,
                "your identity is locked: restart bzz and unlock it".into(),
            );
            return;
        };
        let Some(community) = self
            .config
            .communities
            .iter()
            .find(|entry| entry.id == runtime.community_id)
            .cloned()
        else {
            fail(self, "the community is no longer configured".into());
            return;
        };
        let federated = runtime
            .evidence
            .is_set()
            .then(|| runtime.identity_adapter.clone())
            .flatten();
        let relay = match manage::OwnerRelay::from_runtime(
            &self.paths,
            &community,
            runtime.identity_id,
            runtime.signer.clone(),
            runtime.http.clone(),
            runtime.supervisor.clone(),
            federated.clone(),
            self.diagnostics.clone(),
        ) {
            Ok(relay) => relay,
            Err(error) => {
                fail(self, error.to_string());
                return;
            }
        };
        let context = manage::OwnerContext {
            paths: &self.paths,
            owner: &runtime.signer,
            owner_identity: runtime.identity_id,
            community: runtime.community_id,
            federated: federated.as_ref(),
        };
        #[cfg(test)]
        let publisher: &dyn manage::Publisher = match &self.agent_publisher {
            Some(publisher) => publisher.as_ref(),
            None => &relay,
        };
        #[cfg(not(test))]
        let publisher: &dyn manage::Publisher = &relay;
        let created = manage::create(&context, &mut self.config, &new, publisher).await;
        relay.close().await;
        match created {
            Ok(created) => {
                // Its process is started by the next tick.
                self.local_agents_dirty = true;
                let mut lines = vec![format!(
                    "agent {} ({}) created in {}",
                    created.agent.label, created.agent.pubkey, community.label
                )];
                lines.extend(
                    created
                        .agent
                        .local
                        .as_ref()
                        .map_or_else(
                            || crate::agents::capability::record(new.harness),
                            crate::agents::capability::record_for,
                        )
                        .lines(),
                );
                lines.push(String::new());
                lines.push(format!(
                    "Next: add it to a channel with `bzz agent add {} <channel>`, then mention it there. It answers while bzz is open.",
                    created.agent.label
                ));
                if let Some(form) = &mut self.agent_form {
                    form.stage = FormStage::Done(lines);
                    form.error = None;
                }
            }
            Err(failure) => fail(self, failure.to_string()),
        }
    }

    async fn agent_directory_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if let Some(task) = self.agent_directory_task.take() {
                    task.abort();
                }
                self.presentation.close_overlay();
            }
            KeyCode::Up | KeyCode::Char('k') => self.agent_directory.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.agent_directory.move_by(1),
            KeyCode::Char('r') => self.open_agents(true),
            // The other half of `:agents`: the ones this bzz runs.
            KeyCode::Tab => {
                if let Some(task) = self.agent_directory_task.take() {
                    task.abort();
                }
                self.open_local_agents();
            }
            KeyCode::Char('m') | KeyCode::Enter => self.mention_selected_agent().await?,
            _ => {}
        }
        Ok(())
    }

    async fn mention_selected_agent(&mut self) -> Result<()> {
        let Some(agent) = self.agent_directory.selected().cloned() else {
            return Ok(());
        };
        let Some(channel) = self.current_channel().map(|channel| channel.id) else {
            self.status_error = Some("select a channel before mentioning an agent".into());
            return Ok(());
        };
        if !agent.channel_ids.contains(&channel) {
            self.status_error = Some(
                if self
                    .current_channel()
                    .is_some_and(|channel| channel.kind.is_dm())
                {
                    "the verified remote agent is not a current participant in this DM".into()
                } else {
                    "the verified remote agent is not a member of this channel".into()
                },
            );
            return Ok(());
        }
        if self
            .current_channel()
            .is_some_and(|channel| channel.kind.is_dm())
            && self
                .self_pubkey()
                .is_none_or(|pubkey| !agent.owner_pubkey.eq_ignore_ascii_case(pubkey))
        {
            self.status_error =
                Some("only the verified owner may invoke a remote agent in a DM".into());
            return Ok(());
        }
        if agent.eligibility != crate::agents::Eligibility::Eligible {
            self.status_error =
                Some("the selected remote agent is not currently eligible for invocation".into());
            return Ok(());
        }
        self.presentation.close_overlay();
        self.enter_composer().await?;
        if self.presentation.composer_target.is_none() {
            return Ok(());
        }
        if self
            .composer
            .body
            .chars()
            .next_back()
            .is_some_and(|character| !character.is_whitespace())
        {
            self.composer.insert(' ');
        }
        let start = self.composer.cursor;
        if self
            .composer
            .accept_mention(start..start, &agent.name, &agent.pubkey)
        {
            self.composer.insert(' ');
            self.persist_draft().await?;
            self.status_error =
                Some("remote agent mention inserted; review and send explicitly".into());
        }
        Ok(())
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
                self.presentation.close_overlay();
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
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.spawn_older_search();
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
        self.presentation.close_overlay();
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
                self.presentation.close_overlay();
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

    async fn open_inbox_item(&mut self) -> Result<()> {
        let Some(item) = self.inbox_state.selected(&self.inbox_items).cloned() else {
            return Ok(());
        };
        let Some(channel) = item.channel_id else {
            self.status_error = Some("this read-only Inbox card has no channel context".into());
            return Ok(());
        };
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let member = self
            .store
            .call(move |store| {
                Ok(store
                    .channels(community)?
                    .into_iter()
                    .any(|value| value.id == channel && value.is_member))
            })
            .await?;
        if !member {
            self.status_error =
                Some("the Inbox source is unavailable or no longer accessible".into());
            return Ok(());
        }
        if item.event_id.is_some() && !item.categories.contains(&InboxCategory::NeedsAction) {
            let Some(identity) = self.self_pubkey().map(str::to_owned) else {
                return Ok(());
            };
            let conversation_id = item.conversation_id.clone();
            let context = self
                .store
                .call(move |store| {
                    store.inbox_conversation_context(community, &identity, &conversation_id)
                })
                .await?;
            if context.is_empty() {
                self.status_error =
                    Some("the Inbox context is deleted, hidden, or no longer accessible".into());
                return Ok(());
            }
        }
        if !self
            .open_channel_context(
                channel,
                (!item.categories.contains(&InboxCategory::NeedsAction))
                    .then_some(item.event_id.as_deref())
                    .flatten(),
                item.thread_root.as_deref(),
            )
            .await?
        {
            return Ok(());
        }
        self.presentation
            .open_inbox_context(item.thread_root.is_some());
        Ok(())
    }

    async fn reply_to_inbox(&mut self) -> Result<()> {
        let Some(item) = self.inbox_state.selected(&self.inbox_items).cloned() else {
            return Ok(());
        };
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        if item.categories.contains(&InboxCategory::NeedsAction) {
            self.status_error =
                Some("needs-action cards are informational and cannot be replied to".into());
            return Ok(());
        }
        let Some(channel) = item.channel_id else {
            self.status_error = Some("this Inbox work has no reply target".into());
            return Ok(());
        };
        let visible = self
            .store
            .call(move |store| {
                Ok(store
                    .channels(community)?
                    .into_iter()
                    .any(|value| value.id == channel && value.is_member))
            })
            .await?;
        if !visible {
            self.status_error =
                Some("the Inbox reply target is unavailable or no longer accessible".into());
            return Ok(());
        }
        self.open_composer_target(community, channel, item.thread_root, item.event_id)
            .await
    }

    async fn open_channel_context(
        &mut self,
        channel: Uuid,
        event_id: Option<&str>,
        thread_root: Option<&str>,
    ) -> Result<bool> {
        let Some(community) = self.active_community_id() else {
            return Ok(false);
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
            return Ok(false);
        }
        if !self.channels.iter().any(|value| value.id == channel) {
            self.cache_dirty = true;
            self.hydrate_cache().await?;
        }
        let Some(index) = self.channels.iter().position(|value| value.id == channel) else {
            self.status_error =
                Some("the source channel is unavailable or no longer accessible".into());
            return Ok(false);
        };
        self.select_channel_index(index);
        self.showing_open_channel = !self.channels[index].is_member;
        self.load_selected_channel().await?;
        self.presentation.route = Route::Workspace;
        if let Some(root) = thread_root {
            self.thread_root = Some(root.to_owned());
            self.thread_timeline.selected_event = event_id.map(str::to_owned);
            self.thread_timeline.at_live_bottom = event_id.is_none();
            self.thread_timeline.keep_selection_visible = true;
            self.presentation.set_workspace_focus(FocusSurface::Context);
        } else {
            self.timeline.selected_event = event_id.map(str::to_owned);
            self.timeline.at_live_bottom = event_id.is_none();
            self.timeline.keep_selection_visible = true;
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
        }
        self.cache_dirty = true;
        self.hydrate_cache().await?;
        Ok(true)
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

    async fn confirm_visible_inbox_read(&mut self) -> Result<()> {
        let items = self
            .inbox_state
            .visible(&self.inbox_items)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if items.is_empty() {
            return Ok(());
        }
        if items.len() == 1 {
            self.mark_inbox_item_read(&items[0]).await?;
            self.spawn_inbox_load(false);
            return Ok(());
        }
        self.pending_inbox_read = items;
        self.presentation
            .open_confirmation(ConfirmationKind::InboxRead);
        Ok(())
    }

    async fn mark_pending_inbox_read(&mut self) -> Result<()> {
        let items = std::mem::take(&mut self.pending_inbox_read);
        for item in &items {
            self.mark_inbox_item_read(item).await?;
        }
        self.presentation.close_overlay();
        self.spawn_inbox_load(false);
        Ok(())
    }

    async fn text_overlay_key(&mut self, key: KeyEvent, finder: bool) -> Result<()> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // v0.17 C5: the finder's selection moves; Enter opens that row.
        if finder {
            let delta = match key.code {
                KeyCode::Down | KeyCode::Tab => Some(1),
                KeyCode::Up | KeyCode::BackTab => Some(-1),
                KeyCode::Char('n') if control => Some(1),
                KeyCode::Char('p') if control => Some(-1),
                _ => None,
            };
            if let Some(delta) = delta {
                let len = crate::ui::finder::rows(&self.finder.query, &self.channels).len();
                self.finder.move_by(delta, len);
                return Ok(());
            }
        }
        match key.code {
            KeyCode::Esc => {
                self.command_answer = None;
                self.presentation.close_overlay();
            }
            KeyCode::Backspace => {
                if finder {
                    self.finder.pop();
                } else {
                    self.command_answer = None;
                    self.command.pop();
                }
            }
            KeyCode::Char(character) if !control => {
                if finder {
                    self.finder.push(character)
                } else {
                    self.command_answer = None;
                    self.command.push(character)
                }
            }
            KeyCode::Enter => {
                if finder {
                    let selected = crate::ui::finder::rows(&self.finder.query, &self.channels)
                        .get(self.finder.selected)
                        .map(|row| match row {
                            crate::ui::finder::FinderRow::Channel(channel) => {
                                Some((channel.id, channel.is_member))
                            }
                            crate::ui::finder::FinderRow::NewDm => None,
                        });
                    match selected {
                        Some(Some((id, is_member))) => {
                            if let Some(index) =
                                self.channels.iter().position(|channel| channel.id == id)
                            {
                                self.select_channel_index(index);
                                self.showing_open_channel = !is_member;
                                self.load_selected_channel().await?;
                                self.presentation
                                    .set_workspace_focus(FocusSurface::Timeline);
                            }
                            self.presentation.close_overlay();
                        }
                        // The last row: a DM with someone, the query as
                        // the search for the person.
                        Some(None) => {
                            let query = std::mem::take(&mut self.finder.query);
                            self.presentation.close_overlay();
                            self.open_dm_picker(None);
                            if self.presentation.overlay == Some(Overlay::DmPicker)
                                && !query.is_empty()
                            {
                                self.dm_picker.query = query;
                                if let Some(pubkey) = self.self_pubkey().map(str::to_owned) {
                                    self.dm_picker.reconcile(&self.profiles, &pubkey);
                                }
                                self.dm_dirty_since = Some(Instant::now());
                            }
                        }
                        None => self.presentation.close_overlay(),
                    }
                } else {
                    // Close only the command prompt before executing. Commands
                    // such as :search, :dm, and :agents may deliberately open
                    // their own overlay, which must survive this dispatch.
                    self.presentation.close_overlay();
                    self.command_answer = None;
                    let before = self.status_error.clone();
                    self.execute_command().await?;
                    self.keep_prompt_for_answer(before);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A `:` command that was mistyped, or lacks what it needs, keeps its
    /// prompt open with the text as typed and the answer under it, wrapped,
    /// instead of one status line cut at the window's edge (v0.17 C4).
    fn keep_prompt_for_answer(&mut self, before: Option<String>) {
        if self.presentation.overlay.is_some() || self.status_error == before {
            return;
        }
        let is_answer = self.status_error.as_deref().is_some_and(|answer| {
            answer.starts_with("usage:")
                || answer.starts_with("unknown command")
                || answer == crate::ui::command::COMMUNITY_ANSWER
                || answer.starts_with("no theme named")
        });
        if is_answer {
            self.command_answer = self.status_error.take();
            self.presentation.open_overlay(Overlay::Command);
        }
    }
    fn open_theme_picker(&mut self) {
        let scope = if self.config.communities.is_empty() {
            ThemeScope::Global
        } else {
            ThemeScope::Community
        };
        self.theme_before_preview = Some(self.theme.clone());
        self.theme_picker = Some(ThemePicker::open(self.theme.id(), scope));
        self.presentation.open_overlay(Overlay::Theme);
    }

    fn theme_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if let Some(theme) = self.theme_before_preview.take() {
                    self.theme = theme;
                }
                self.theme_picker = None;
                self.presentation.close_overlay();
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
        self.presentation.close_overlay();
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
        let command = crate::ui::command::parse(&self.command);
        if self.demo
            && let Some(answer) = crate::demo::refuses(&command)
        {
            self.status_error = Some(answer.into());
            return Ok(());
        }
        match command {
            crate::ui::command::Command::Lock => {
                // Nothing is served while bzz is locked.
                if let Some(broker) = self.broker.take() {
                    broker.stop().await;
                }
                // No agent runs while bzz is locked: its key is sealed again.
                if let Some(agents) = self.local_agents.take() {
                    agents.stop().await;
                }
                // No audio runs while bzz is locked.
                self.stop_huddle_call().await;
                // Nor any app: its records need the relay.
                self.close_all_apps();
                if let Some(runtime) = self.runtime.take() {
                    if self.config.ui.publish_presence
                        && self.last_presence_publish.take().is_some()
                    {
                        publish_offline(&runtime).await;
                    }
                    runtime.supervisor.shutdown().await;
                    runtime.signer.lock().await;
                }
                self.clear_agent_directory_state();
                self.agent_typing.clear();
                self.typing_subscription_channel = None;
                self.closed_typing_channel = None;
                self.presentation.close_overlay();
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
            crate::ui::command::Command::SignIn => self.begin_federated_sign_in().await,
            crate::ui::command::Command::ToChannel => self.send_draft_to_channel(),
            // v0.17 D6: a bare `:status` shows the status; v0.16 cleared it.
            crate::ui::command::Command::Status(arguments) if arguments.is_empty() => {
                return self.show_status().await;
            }
            crate::ui::command::Command::Status(arguments) => self.set_status(&arguments),
            crate::ui::command::Command::Presence(argument) => {
                return self.presence_command(&argument).await;
            }
            crate::ui::command::Command::Artifacts => {
                let scope = match (
                    self.current_channel().map(|channel| channel.id),
                    self.thread_root.clone(),
                ) {
                    (Some(channel), Some(root)) => {
                        crate::ui::artifacts::ArtifactScope::Thread(channel, root)
                    }
                    (Some(channel), None) => crate::ui::artifacts::ArtifactScope::Channel(channel),
                    (None, _) => {
                        self.status_error = Some("select a channel to see its artifacts".into());
                        return Ok(());
                    }
                };
                return self.open_artifacts(scope).await;
            }
            crate::ui::command::Command::Huddle(arguments) => {
                return self.huddle_command(&arguments).await;
            }
            crate::ui::command::Command::Profile(argument) => {
                let pubkey = if argument.is_empty() {
                    self.selected_message()
                        .map(|message| message.pubkey.clone())
                } else {
                    nostr::PublicKey::parse(&argument)
                        .ok()
                        .map(|key| key.to_hex())
                };
                match pubkey {
                    Some(pubkey) => return self.open_profile(pubkey).await,
                    None => {
                        self.status_error = Some(
                            "usage: :profile (selected message author) or :profile <npub|hex>"
                                .into(),
                        );
                    }
                }
            }
            crate::ui::command::Command::Broker => {
                self.presentation.open_overlay(Overlay::Broker);
            }
            crate::ui::command::Command::Tasks => {
                return self
                    .open_artifacts(crate::ui::artifacts::ArtifactScope::Assigned)
                    .await;
            }
            crate::ui::command::Command::Artifact(arguments) => {
                return self.artifact_command(&arguments).await;
            }
            crate::ui::command::Command::Section(arguments) => {
                return self.section_command(&arguments).await;
            }
            crate::ui::command::Command::Sort(arguments) => {
                return self.sort_command(&arguments).await;
            }
            crate::ui::command::Command::Solo(name) => return self.solo_command(&name).await,
            crate::ui::command::Command::Report(arguments) => {
                // `:report author <reason> [note]` reports the selected
                // message's author as a user instead of the message.
                let (author_only, arguments) = match arguments.strip_prefix("author") {
                    Some(rest) if rest.is_empty() || rest.starts_with(' ') => (true, rest.trim()),
                    _ => (false, arguments.as_str()),
                };
                let mut parts = arguments.splitn(2, ' ');
                let reason = parts.next().unwrap_or_default().to_owned();
                let note = parts.next().map(str::trim).filter(|note| !note.is_empty());
                if !buzz_sdk::REPORT_TYPES.contains(&reason.as_str()) {
                    self.status_error = Some(format!(
                        "usage: :report [author] <{}> [note]",
                        buzz_sdk::REPORT_TYPES.join("|")
                    ));
                } else if self.runtime.is_none() || self.selected_message().is_none() {
                    self.status_error = Some("select a message to report".into());
                } else if self.refused_by_signer("report", &[buzz_core::kind::KIND_REPORT as u16]) {
                } else {
                    self.pending_report = Some((reason, note.map(str::to_owned), author_only));
                    self.presentation
                        .open_confirmation(ConfirmationKind::Report);
                    return Ok(());
                }
            }
            crate::ui::command::Command::System => {
                self.show_system_channels = !self.show_system_channels;
                self.presentation.close_overlay();
            }
            crate::ui::command::Command::Inbox => self.open_inbox(),
            crate::ui::command::Command::Search => self.open_search(),
            crate::ui::command::Command::Dm => self.open_dm_picker(None),
            crate::ui::command::Command::Agents(which) => match which.as_str() {
                "" if !self.local_agent_configs().is_empty() => self.open_local_agents(),
                "" | "directory" | "remote" => self.open_agents(false),
                "local" => self.open_local_agents(),
                _ => {
                    self.status_error = Some(
                        "usage: :agents (yours, when this community has any), :agents local, :agents directory"
                            .into(),
                    );
                }
            },
            crate::ui::command::Command::Agent(arguments) => {
                match arguments
                    .strip_prefix("new")
                    .filter(|rest| rest.is_empty() || rest.starts_with(' '))
                {
                    Some(name) => self.open_agent_form(name),
                    None => {
                        self.status_error = Some(
                            "usage: :agent new [name]; :agents shows the ones you have".into(),
                        );
                    }
                }
            }
            crate::ui::command::Command::Approvals => {
                self.presentation.close_overlay();
                self.refresh_approvals();
                // An agent's command first: it is refused when its time is
                // up. Then the versions of apps.
                if self.approvals.is_empty() && !self.apps.waiting.is_empty() {
                    return self.open_app_review(None).await;
                }
                self.open_approval();
            }
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
            crate::ui::command::Command::Apps => return self.open_apps_panel().await,
            crate::ui::command::Command::App(arguments) => {
                return self.app_command(&arguments).await;
            }
            crate::ui::command::Command::Unknown(value) => {
                self.status_error = Some(crate::ui::command::unknown_answer(&value));
            }
            crate::ui::command::Command::AddCommunity
            | crate::ui::command::Command::RemoveCommunity => {
                self.status_error = Some(crate::ui::command::COMMUNITY_ANSWER.into());
            }
            crate::ui::command::Command::Quit => {
                self.presentation.open_confirmation(ConfirmationKind::Quit);
            }
            crate::ui::command::Command::Help => {
                self.help_scroll = crate::ui::help::HelpScroll::default();
                self.presentation.open_overlay(Overlay::Help);
            }
            crate::ui::command::Command::Theme(name) => self.theme_command(&name),
        }
        Ok(())
    }

    /// `:theme` opens the picker; `:theme <name>` sets that theme as the
    /// picker's Enter would (v0.17 C4).
    fn theme_command(&mut self, name: &str) {
        self.open_theme_picker();
        if name.is_empty() {
            return;
        }
        let wanted = name.to_lowercase().replace([' ', '_'], "-");
        let found = crate::ui::theme::ThemeRegistry::canonical_id(&wanted).or_else(|| {
            crate::ui::theme::ThemeRegistry::entries()
                .find(|entry| entry.name.to_lowercase().replace([' ', '_'], "-") == wanted)
                .map(|entry| entry.id)
        });
        match found {
            Some(id)
                if self
                    .theme_picker
                    .as_mut()
                    .is_some_and(|picker| picker.select_id(id)) =>
            {
                self.confirm_theme_selection();
            }
            _ => {
                // Put back what the picker previewed, and say what exists.
                if let Some(theme) = self.theme_before_preview.take() {
                    self.theme = theme;
                }
                self.theme_picker = None;
                self.presentation.close_overlay();
                let names = crate::ui::theme::ThemeRegistry::entries()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.status_error = Some(format!(
                    "no theme named {} · :theme opens the picker · themes: {names}",
                    sanitize::single_line(name)
                ));
            }
        }
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
        let built = Runtime::build(
            &self.config,
            index,
            &self.paths,
            self.store.clone(),
            reuse,
            self.diagnostics.clone(),
        );
        if needs_prompt {
            let (new_guard, new_terminal) = TerminalGuard::enter(self.config.ui.mouse.enabled())?;
            *guard = new_guard;
            *terminal = new_terminal;
        }
        match built {
            Ok(runtime) => {
                self.close_all_apps();
                if let Some(old) = self.runtime.take() {
                    old.supervisor.shutdown().await;
                    if old.identity_id != runtime.identity_id {
                        old.signer.lock().await;
                    }
                }
                self.media.bind(
                    runtime.community_id,
                    runtime.identity_id,
                    runtime.media.clone(),
                );
                self.runtime = Some(runtime);
                self.selected_community = index;
                self.select_community_index(index);
                self.config.default_community = Some(target_id);
                self.config.save(&self.paths)?;
                self.reload_theme();
                self.selected_channel = 0;
                self.channel_viewport = ViewportState::default();
                self.showing_open_channel = false;
                self.thread_root = None;
                self.last_marked.clear();
                self.profile_requested.clear();
                self.subscribed_channels.clear();
                self.closed_channel_subscriptions.clear();
                self.agent_typing.clear();
                self.typing_subscription_channel = None;
                self.closed_typing_channel = None;
                self.inbox_items.clear();
                self.inbox_messages.clear();
                self.inbox_state = InboxState::default();
                self.search_state = SearchState::default();
                self.dm_picker = DmPickerState::default();
                self.dm_dirty_since = None;
                self.clear_agent_directory_state();
                self.presentation.close_overlay();
                self.connection = ConnectionState::Connecting;
                self.last_inbox_refresh = Instant::now();
                self.hydrate_cache().await?;
                self.start_sync().await?;
            }
            Err(error) => {
                let recovery = identity_recovery_connection(&error);
                if let Some(connection) = recovery {
                    let target_identity = target.identity_id;
                    self.close_all_apps();
                    if let Some(old) = self.runtime.take() {
                        old.supervisor.shutdown().await;
                        old.signer.lock().await;
                    }
                    self.media.select_cached(target_id, target_identity);
                    self.selected_community = index;
                    self.select_community_index(index);
                    self.config.default_community = Some(target_id);
                    self.config.save(&self.paths)?;
                    self.reload_theme();
                    self.selected_channel = 0;
                    self.channel_viewport = ViewportState::default();
                    self.showing_open_channel = false;
                    self.thread_root = None;
                    self.last_marked.clear();
                    self.profile_requested.clear();
                    self.subscribed_channels.clear();
                    self.closed_channel_subscriptions.clear();
                    self.agent_typing.clear();
                    self.typing_subscription_channel = None;
                    self.closed_typing_channel = None;
                    self.inbox_items.clear();
                    self.inbox_messages.clear();
                    self.inbox_state = InboxState::default();
                    self.search_state = SearchState::default();
                    self.dm_picker = DmPickerState::default();
                    self.dm_dirty_since = None;
                    self.clear_agent_directory_state();
                    self.presentation.close_overlay();
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

    fn community_ids(&self) -> Vec<String> {
        self.config
            .communities
            .iter()
            .map(|community| community.id.to_string())
            .collect()
    }

    fn ordered_channel_indexes(&self) -> Vec<usize> {
        crate::ui::sidebar::ordered_indexes(
            &self.channels,
            &self.unread_channels(),
            self.config.ui.channel_sort,
            self.show_system_channels,
            &self.sidebar_layout,
        )
    }

    fn sidebar_rows(&self) -> Vec<crate::ui::sidebar::SidebarRow> {
        crate::ui::sidebar::rows(
            &self.channels,
            &self.unread_channels(),
            self.config.ui.channel_sort,
            self.show_system_channels,
            &self.sidebar_layout,
        )
    }

    /// Viewport ids of every sidebar row, headings included.
    fn sidebar_row_ids(&self) -> Vec<String> {
        crate::ui::sidebar::row_ids(&self.channels, &self.sidebar_rows())
    }

    fn visible_channel_ids(&self) -> Vec<String> {
        self.ordered_channel_indexes()
            .into_iter()
            .map(|index| self.channels[index].id.to_string())
            .collect()
    }

    fn sync_workspace_viewports(&mut self) {
        let community_ids = self.community_ids();
        if self.community_viewport.selected_id.is_none() {
            self.community_viewport.select(
                self.config
                    .communities
                    .get(self.selected_community)
                    .map(|community| community.id.to_string()),
            );
        }
        self.community_viewport.reconcile(community_ids);

        if self.showing_open_channel {
            self.channel_viewport.selected_id = None;
            return;
        }
        // Selection is a channel; scrolling counts heading rows too.
        let channel_ids = self.visible_channel_ids();
        if !self
            .channel_viewport
            .selected_id
            .as_ref()
            .is_some_and(|selected| channel_ids.contains(selected))
        {
            self.channel_viewport.selected_id = channel_ids.first().cloned();
            self.channel_viewport.keep_selection_visible = true;
        }
        let row_ids = self.sidebar_row_ids();
        self.channel_viewport.reconcile_scroll(&row_ids);
        if let Some(selected) = self.channel_viewport.selected_id.as_deref()
            && let Some(index) = self
                .channels
                .iter()
                .position(|channel| channel.id.to_string() == selected)
        {
            self.selected_channel = index;
        }
    }

    fn select_channel_index(&mut self, index: usize) {
        let Some(channel) = self.channels.get(index) else {
            return;
        };
        self.selected_channel = index;
        self.channel_viewport.select(Some(channel.id.to_string()));
    }

    fn select_community_index(&mut self, index: usize) {
        let Some(community) = self.config.communities.get(index) else {
            return;
        };
        self.community_cursor = index;
        self.community_viewport
            .select(Some(community.id.to_string()));
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
        if self.presentation.focus == FocusSurface::Context {
            self.thread_timeline
                .selected_index(&self.thread_messages)
                .and_then(|index| self.thread_messages.get(index))
        } else {
            self.timeline
                .selected_index(&self.messages)
                .and_then(|index| self.messages.get(index))
        }
    }

    fn action_context(&self) -> ActionContext {
        let selected = self.selected_message();
        let inbox_selected = self.inbox_state.selected(&self.inbox_items);
        let self_pubkey = self.self_pubkey();
        ActionContext {
            route: self.presentation.route,
            focus: self.presentation.focus,
            has_inbox_selection: inbox_selected.is_some(),
            inbox_has_context: inbox_selected.is_some_and(|item| item.channel_id.is_some()),
            inbox_can_reply: inbox_selected.is_some_and(|item| {
                item.channel_id.is_some() && !item.categories.contains(&InboxCategory::NeedsAction)
            }),
            inbox_visible_count: self.inbox_state.visible(&self.inbox_items).len(),
            has_channel: self.current_channel().is_some(),
            has_selected_event: selected.is_some(),
            selected_event_is_own: selected
                .is_some_and(|message| self_pubkey.is_some_and(|pubkey| message.pubkey == pubkey)),
            selected_event_has_media: selected
                .is_some_and(|message| !message.attachments.is_empty()),
            context_open: self.thread_root.is_some(),
            can_publish: self.runtime.is_some(),
            delete_refused: self
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.signer.refusal(5)),
        }
    }

    async fn shutdown(&mut self) {
        // Each harness is stopped: end of input, then TERM, then KILL.
        if let Some(agents) = self.local_agents.take() {
            agents.stop().await;
        }
        self.stop_huddle_call().await;
        self.agent_typing.clear();
        self.typing_subscription_channel = None;
        self.closed_typing_channel = None;
        if let Some(runtime) = self.runtime.take() {
            if self.config.ui.publish_presence && self.last_presence_publish.is_some() {
                publish_offline(&runtime).await;
            }
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                outbox::flush_with_diagnostics(
                    runtime.community_id,
                    &runtime.http,
                    &runtime.supervisor,
                    &self.store,
                    &self.diagnostics,
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

    fn record_primary_click(&mut self, target: &HitTarget) -> bool {
        let now = Instant::now();
        let repeated = self
            .last_primary_click
            .as_ref()
            .is_some_and(|(previous, when)| {
                previous == target && now.duration_since(*when) <= Duration::from_millis(500)
            });
        self.last_primary_click = Some((target.clone(), now));
        repeated
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        let color_mode = crate::ui::frame::ColorMode::detected();
        self.theme.set_color_mode(color_mode);
        self.render_frame(frame);
        // The last pass: no colour under NO_COLOR, fewer without true colour.
        crate::ui::frame::finish(frame.buffer_mut(), color_mode);
    }

    fn render_frame(&mut self, frame: &mut Frame<'_>) {
        // A command that waits for the owner is said on every screen,
        // whatever is open, until he has seen it. It takes its own row
        // above the panes, so no pane loses its title to it (L5).
        let area = frame.area();
        let banner = self
            .waiting_banner()
            .filter(|_| area.width >= 50 && area.height >= 13);
        let Some(text) = banner else {
            self.render_app(frame, area);
            return;
        };
        let below = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height - 1,
        );
        self.render_app(frame, below);
        crate::ui::approval::render_banner(frame, area, &text, &self.theme);
    }

    /// What waits for the owner's answer, as the banner says it.
    fn waiting_banner(&self) -> Option<String> {
        if let Some(first) = self.approvals.first()
            && self.presentation.overlay != Some(Overlay::Approval)
        {
            return Some(crate::ui::approval::banner_text(
                first,
                &self.conversation_label(first),
                self.approvals.len(),
                seconds_left(first),
            ));
        }
        if let Some(first) = self.apps.waiting.first()
            && !matches!(
                self.presentation.overlay,
                Some(Overlay::Approval | Overlay::AppApproval)
            )
        {
            // A version of an app waits: said in the same place, the same
            // way, after an agent's command.
            #[cfg(feature = "apps")]
            return Some(crate::ui::app_approval::banner_text(
                first,
                self.apps.waiting.len(),
            ));
            #[cfg(not(feature = "apps"))]
            let _ = first;
        }
        None
    }

    fn render_app(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.render_generation = self.render_generation.wrapping_add(1);
        let mut hit_map = HitMap::new(self.render_generation);
        if area.width < 50 || area.height < 12 {
            // While an app has the focus `q` is the app's.
            let way_out = if self.app_has_focus() {
                "Resize the terminal; Ctrl-c leaves the app"
            } else {
                "Resize the terminal or press q to quit"
            };
            frame.render_widget(
                Paragraph::new(format!("bzz needs at least 50×12\n{way_out}"))
                    .style(self.theme.style(HighlightGroup::Normal))
                    .alignment(Alignment::Center)
                    .block(crate::ui::frame::block(
                        &self.theme,
                        BorderSurface::Pane,
                        "terminal too small",
                        true,
                    )),
                area,
            );
            self.last_hit_map = Some(hit_map);
            return;
        }
        if self.huddle_zoomed() {
            // The huddle's zoomed view and the status line; nothing under
            // them is laid out, so its frames stay cheap.
            let status = Rect {
                y: area.bottom() - 1,
                height: 1,
                ..area
            };
            self.render_status(frame, status);
            self.render_overlay(frame, area, &mut hit_map);
            self.last_hit_map = Some(hit_map);
            return;
        }
        if self.config.communities.is_empty() {
            self.render_empty(frame, area);
            self.render_overlay(frame, area, &mut hit_map);
            self.last_hit_map = Some(hit_map);
            return;
        }
        if self.presentation.route == Route::Inbox {
            self.render_inbox_workspace(frame, area, &mut hit_map);
            self.render_overlay(frame, area, &mut hit_map);
            self.last_hit_map = Some(hit_map);
            return;
        }
        // The focus is on a pane that is drawn: an app that is no longer
        // open here (another channel was selected) gives it back first.
        if self.presentation.focus == FocusSurface::App && self.shown_app().is_none() {
            self.leave_app();
        }
        let panes = layout::panes_with_app(
            area,
            self.community_rail,
            self.sidebar,
            self.thread_root.is_some(),
            self.config.ui.sidebar_width,
            self.config.ui.thread_width,
            self.composer_dock_height(),
            self.app_request(),
        );
        self.apps.placement = panes.app_placement;
        self.thread_drawn = panes.thread.is_some();
        if (self.presentation.focus == FocusSurface::Communities && panes.community.is_none())
            || (self.presentation.focus == FocusSurface::Channels && panes.sidebar.is_none())
            || (self.presentation.focus == FocusSurface::Context && panes.thread.is_none())
            || (self.presentation.focus == FocusSurface::App && panes.app.is_none())
        {
            self.presentation
                .set_workspace_focus(FocusSurface::Timeline);
        }
        if panes.app_placement == layout::AppPlacement::Zoomed {
            // Zoom: the app and the status line, and the overlays over them.
            if let Some(app) = panes.app {
                self.render_app_pane(frame, app, true, &mut hit_map);
            }
            self.render_status(frame, panes.status);
            self.render_overlay(frame, area, &mut hit_map);
            self.last_hit_map = Some(hit_map);
            return;
        }
        if let Some(rail) = panes.community {
            let ids = self.community_ids();
            self.community_viewport
                .set_viewport_height(usize::from(inner_rect(rail).height), &ids);
            self.render_communities(
                frame,
                rail,
                self.presentation.focus == FocusSurface::Communities,
            );
            for (row, index) in (self.community_viewport.scroll..self.config.communities.len())
                .take(usize::from(inner_rect(rail).height))
                .enumerate()
            {
                if let Some(area) = list_row(rail, row, 1) {
                    hit_map.push(area, HitTarget::Community(index));
                }
            }
        }
        if let Some(sidebar) = panes.sidebar {
            let rows = self.sidebar_rows();
            let ids = crate::ui::sidebar::row_ids(&self.channels, &rows);
            self.channel_viewport
                .set_viewport_height(usize::from(inner_rect(sidebar).height), &ids);
            hit_map.push(sidebar, HitTarget::ChannelPane);
            for (row, entry) in rows
                .into_iter()
                .skip(self.channel_viewport.scroll)
                .take(usize::from(inner_rect(sidebar).height))
                .enumerate()
            {
                if let crate::ui::sidebar::SidebarRow::Channel(index) = entry
                    && let Some(area) = list_row(sidebar, row, 1)
                {
                    hit_map.push(area, HitTarget::Channel(index));
                }
            }
            crate::ui::sidebar::render(
                frame,
                sidebar,
                &self.channels,
                &self.agents,
                &self.channel_viewport,
                &self.unread_channels(),
                &self.unread_counts,
                self.config.ui.channel_sort,
                self.show_system_channels,
                &self.sidebar_layout,
                &self.dm_presence,
                self.active_huddle()
                    .and(self.current_channel().map(|channel| channel.id)),
                &self.theme,
                self.presentation.focus == FocusSurface::Channels,
            );
        }
        let title = self.current_channel().map_or_else(
            || "timeline".to_owned(),
            |channel| {
                let agent = (channel.kind.is_dm() && channel.member_count == 2)
                    .then(|| {
                        self.agents
                            .values()
                            .filter(|agent| !agent.stale && agent.channel_ids.contains(&channel.id))
                            .min_by(|left, right| left.pubkey.cmp(&right.pubkey))
                    })
                    .flatten();
                agent.map_or_else(
                    || channel.name.clone(),
                    |agent| {
                        let owner = if self
                            .self_pubkey()
                            .is_some_and(|pubkey| agent.owner_pubkey.eq_ignore_ascii_case(pubkey))
                        {
                            "managed by you".to_owned()
                        } else {
                            let label = self
                                .profiles
                                .get(&agent.owner_pubkey)
                                .map(Profile::label)
                                .unwrap_or_else(|| {
                                    crate::domain::abbreviated_pubkey(&agent.owner_pubkey)
                                });
                            format!("owned by {}", sanitize::single_line(&label))
                        };
                        format!("◆ {} · {owner}", sanitize::single_line(&channel.name))
                    },
                )
            },
        );
        let self_pubkey = self.self_pubkey().map(str::to_owned);
        self.timeline.banner = self.connection_banner();
        self.timeline.empty_hint = self.empty_hint();
        self.timeline.new_marker = self.new_marker();
        hit_map.push(panes.timeline, HitTarget::Timeline);
        let mut timeline_hits = Vec::new();
        if self.presentation.overlay.is_none() && self.presentation.composer_target.is_none() {
            timeline::render_with_media_and_hits_limited(
                frame,
                panes.timeline,
                &self.messages,
                &self.thread_summaries,
                &self.profiles,
                &self.agents,
                &self.reactions,
                &mut self.timeline,
                &title,
                &self.theme,
                self.presentation.focus == FocusSurface::Timeline,
                self_pubkey.as_deref(),
                &mut self.media,
                &mut timeline_hits,
                self.config.ui.message_width,
            );
        } else {
            timeline::render_limited(
                frame,
                panes.timeline,
                &self.messages,
                &self.thread_summaries,
                &self.profiles,
                &self.agents,
                &self.reactions,
                &mut self.timeline,
                &title,
                &self.theme,
                self.presentation.focus == FocusSurface::Timeline,
                self_pubkey.as_deref(),
                self.config.ui.message_width,
            );
        }
        for hit in timeline_hits {
            hit_map.push(hit.area, HitTarget::TimelineMessage(hit.event_id));
        }
        if let Some(thread) = panes.thread {
            hit_map.push(thread, HitTarget::Thread);
            let reply_count = self
                .thread_root
                .as_ref()
                .and_then(|root| self.thread_summaries.get(root))
                .map_or_else(
                    || self.thread_messages.len().saturating_sub(1),
                    |summary| summary.descendant_count as usize,
                );
            let reply_label = match reply_count {
                0 => "no replies".to_owned(),
                1 => "1 reply".to_owned(),
                count => format!("{count} replies"),
            };
            let thread_title = format!("context · {reply_label}");
            self.thread_timeline.hints = vec![("q", "close")];
            let no_thread_summaries = HashMap::new();
            let mut thread_hits = Vec::new();
            if self.presentation.overlay.is_none() && self.presentation.composer_target.is_none() {
                timeline::render_with_media_and_hits_limited(
                    frame,
                    thread,
                    &self.thread_messages,
                    &no_thread_summaries,
                    &self.profiles,
                    &self.agents,
                    &self.reactions,
                    &mut self.thread_timeline,
                    &thread_title,
                    &self.theme,
                    self.presentation.focus == FocusSurface::Context,
                    self_pubkey.as_deref(),
                    &mut self.media,
                    &mut thread_hits,
                    self.config.ui.message_width,
                );
            } else {
                timeline::render_limited(
                    frame,
                    thread,
                    &self.thread_messages,
                    &no_thread_summaries,
                    &self.profiles,
                    &self.agents,
                    &self.reactions,
                    &mut self.thread_timeline,
                    &thread_title,
                    &self.theme,
                    self.presentation.focus == FocusSurface::Context,
                    self_pubkey.as_deref(),
                    self.config.ui.message_width,
                );
            }
            for hit in thread_hits {
                hit_map.push(hit.area, HitTarget::ThreadMessage(hit.event_id));
            }
        }
        if let Some(app) = panes.app {
            self.render_app_pane(frame, app, false, &mut hit_map);
        }
        if let Some(composer) = panes.composer {
            self.render_composer_dock(frame, composer, &mut hit_map);
        }
        self.render_status(frame, panes.status);
        self.render_overlay(frame, area, &mut hit_map);
        self.last_hit_map = Some(hit_map);
    }
    fn render_inbox_workspace(&mut self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        let inbox_self_pubkey = self.self_pubkey().map(str::to_owned);
        let route_layout = crate::ui::inbox::render(
            frame,
            vertical[0],
            &mut self.inbox_state,
            crate::ui::inbox::InboxView {
                items: &self.inbox_items,
                messages: &self.inbox_messages,
                profiles: &self.profiles,
                agents: &self.agents,
                self_pubkey: inbox_self_pubkey.as_deref(),
                focus: self.presentation.focus,
                theme: &self.theme,
                loading: self.inbox_loading || self.inbox_detail_loading,
            },
        );
        if let Some(list) = route_layout.list {
            hit_map.push(list, HitTarget::InboxList);
            for (row, item) in self
                .inbox_state
                .visible(&self.inbox_items)
                .into_iter()
                .skip(self.inbox_state.list_viewport.scroll)
                .take(usize::from(list.height.saturating_div(2).max(1)))
                .enumerate()
            {
                let y = list
                    .y
                    .saturating_add(u16::try_from(row.saturating_mul(2)).unwrap_or(u16::MAX));
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
        if let Some(detail) = route_layout.detail {
            hit_map.push(detail, HitTarget::InboxDetail);
        }
        self.render_status(frame, vertical[1]);
    }

    fn render_empty(&self, frame: &mut Frame<'_>, area: Rect) {
        // Without a workspace there is no status bar: command feedback
        // (`:huddle`, `:presence`, ...) is shown here instead.
        crate::ui::first_screen::render(
            frame,
            area,
            &self.theme,
            &self.first_screen,
            self.status_error.as_deref(),
        );
    }
    fn render_communities(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let items = self.config.communities.iter().map(|community| {
            let selected = self
                .community_viewport
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &community.id.to_string());
            let marker = if selected { "●" } else { "·" };
            ListItem::new(Line::from(format!(
                "{marker} {}",
                sanitize::single_line(&community.label)
            )))
            .style(self.theme.style(if selected {
                HighlightGroup::CommunitySelected
            } else {
                HighlightGroup::CommunityRail
            }))
        });
        let selected = self
            .community_viewport
            .selected_id
            .as_ref()
            .and_then(|selected| {
                self.config
                    .communities
                    .iter()
                    .position(|community| community.id.to_string() == *selected)
            });
        let mut state = ratatui::widgets::ListState::default()
            .with_selected(selected)
            .with_offset(self.community_viewport.scroll);
        frame.render_stateful_widget(
            List::new(items)
                .style(self.theme.style(HighlightGroup::CommunityRail))
                .block(crate::ui::frame::block(
                    &self.theme,
                    BorderSurface::Pane,
                    "communities",
                    focused,
                ))
                .highlight_style(self.theme.style(HighlightGroup::Selection))
                .highlight_symbol("› "),
            area,
            &mut state,
        );
    }
    /// Computes a bounded dock height before shell measurement. The text model
    /// uses identical Unicode wrapping for mouse placement, so the dock never
    /// overlays the timeline simply because a draft spans several rows.
    fn composer_dock_height(&self) -> u16 {
        if self.presentation.composer_target.is_none() {
            return 3;
        }
        let attachment_count = self
            .composer
            .attachments
            .len()
            .saturating_add(self.staging_attachments.len());
        let attachment_rows = u16::try_from(attachment_count.min(3))
            .unwrap_or(u16::MAX)
            .saturating_add(u16::from(attachment_count > 3));
        let body_rows = u16::try_from(self.composer.display_rows(80)).unwrap_or(u16::MAX);
        body_rows
            .saturating_add(attachment_rows)
            .saturating_add(2)
            .clamp(4, 11)
    }

    fn render_composer_dock(&mut self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        let active = self.presentation.composer_target.is_some();
        let channel = self
            .current_channel()
            .map(|channel| sanitize::single_line(&channel.name))
            .unwrap_or_else(|| "conversation".into());
        // An open composer says where it sends; a closed one says where it
        // would: into the thread only while the thread is drawn.
        let into_thread = match &self.presentation.composer_target {
            Some(target) => target.thread_root_id.is_some(),
            None => self.thread_root.is_some() && self.thread_drawn,
        };
        let target = if into_thread {
            format!("reply in #{channel}")
        } else {
            format!("message #{channel}")
        };
        let (title, content, _border, content_group) = if active {
            let mut attachment_lines = self
                .composer
                .attachments
                .iter()
                .map(|attachment| match attachment {
                    crate::media::DraftAttachment::Pending(pending) => {
                        let state = if self.uploading_media.contains(&pending.id) {
                            "uploading"
                        } else {
                            "queued"
                        };
                        format!(
                            "[{state}] {} · {}",
                            sanitize::single_line(&pending.filename),
                            crate::media::model::human_size(pending.size)
                        )
                    }
                    crate::media::DraftAttachment::Failed(pending) => format!(
                        "[failed] {} · Ctrl-r retry",
                        sanitize::single_line(&pending.filename)
                    ),
                    crate::media::DraftAttachment::Uploaded(attachment) => format!(
                        "[ready] {} · {}",
                        sanitize::single_line(attachment.label()),
                        crate::media::model::human_size(attachment.size)
                    ),
                })
                .chain(self.staging_attachments.iter().map(|attachment| {
                    format!(
                        "[processing] {}",
                        sanitize::single_line(&attachment.filename)
                    )
                }))
                .collect::<Vec<_>>();
            let overflow = attachment_lines.len().saturating_sub(3);
            attachment_lines.truncate(3);
            if overflow > 0 {
                attachment_lines.push(format!("+{overflow} more attachment(s)"));
            }
            let attachments = if attachment_lines.is_empty() {
                String::new()
            } else {
                format!("\n{}", attachment_lines.join("\n"))
            };
            let content = format!("{}{}", sanitize::text(&self.composer.body), attachments);
            let title = match self.active_edit() {
                Some(editing) if editing.kept_attachments > 0 => format!(
                    " edit message · keeps {} attachment(s) ",
                    editing.kept_attachments
                ),
                Some(_) => " edit message ".into(),
                None => format!(" {target} "),
            };
            (
                title,
                content,
                HighlightGroup::ActiveComposerBorder,
                HighlightGroup::Composer,
            )
        } else if self.current_channel().is_none() {
            (
                " message ".into(),
                "Select a joined channel to write.".into(),
                HighlightGroup::ComposerBorder,
                HighlightGroup::ComposerHint,
            )
        } else if let Some(reason) = self.current_channel().and_then(Channel::read_only_reason) {
            (
                format!(" {target} "),
                reason.notice().into(),
                HighlightGroup::ComposerBorder,
                HighlightGroup::ComposerDisabled,
            )
        } else if self.runtime.is_none() {
            (
                format!(" {target} "),
                "Read-only: restore or unlock the identity to write.".into(),
                HighlightGroup::ComposerBorder,
                HighlightGroup::ComposerDisabled,
            )
        } else {
            (
                format!(" {target} "),
                "Press i or click here to write.".into(),
                HighlightGroup::ComposerBorder,
                HighlightGroup::ComposerHint,
            )
        };
        // What to press lives at the foot of the frame, not in its title.
        let keys: &[(&str, &str)] = match (active, self.active_edit().is_some()) {
            (false, _) => &[],
            (true, true) => &[("Enter", "save"), ("Esc", "cancel")],
            (true, false) => &[
                ("Enter", "send"),
                ("Ctrl-v", "paste"),
                ("Ctrl-o", "choose"),
                ("Alt-o", "path"),
                ("Del", "remove"),
                ("Ctrl-c", "clear"),
                ("Esc", "close"),
            ],
        };
        let mut block =
            crate::ui::frame::block(&self.theme, BorderSurface::Composer, &title, active);
        if !keys.is_empty() {
            block = block.title_bottom(crate::ui::frame::hints(&self.theme, keys));
        }
        let inner = block.inner(area);
        if !inner.is_empty() {
            hit_map.push(inner, HitTarget::Composer);
        }
        frame.render_widget(
            Paragraph::new(content)
                .style(self.theme.style(content_group))
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        if active && !inner.is_empty() {
            let (row, column) = self
                .composer
                .cursor_display_position(usize::from(inner.width));
            frame.set_cursor_position((
                inner
                    .x
                    .saturating_add(u16::try_from(column).unwrap_or(u16::MAX))
                    .min(inner.right().saturating_sub(1)),
                inner
                    .y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX))
                    .min(inner.bottom().saturating_sub(1)),
            ));
        }
        if active && let Some(picker) = &self.mention_picker {
            let height = u16::try_from(picker.candidates.len().min(5) + 2).unwrap_or(7);
            let mention_area = Rect::new(
                area.x,
                area.y.saturating_sub(height),
                area.width,
                height.min(area.y),
            );
            if !mention_area.is_empty() {
                for index in 0..picker.candidates.len().min(5) {
                    if let Some(row) = list_row(mention_area, index, 1) {
                        hit_map.push(row, HitTarget::MentionCandidate(index));
                    }
                }
                crate::ui::mention_picker::render(
                    frame,
                    mention_area,
                    picker,
                    &self.own_agents(),
                    &self.theme,
                );
            }
        }
    }

    fn render_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let mode = match self.presentation.overlay {
            Some(Overlay::Finder) => "FINDER",
            Some(Overlay::Command) => "COMMAND",
            Some(Overlay::Theme) => "THEME",
            Some(Overlay::Reaction) => "REACTION",
            Some(Overlay::Confirmation) => match self.presentation.confirmation {
                Some(ConfirmationKind::Quit) => "CONFIRM QUIT",
                _ => "CONFIRM",
            },
            Some(Overlay::MediaPreview) => "MEDIA",
            Some(Overlay::Attachment) => match self.presentation.attachment_prompt {
                Some(AttachmentPrompt::Save) => "SAVE",
                _ => "ATTACH",
            },
            Some(Overlay::Search) => "SEARCH",
            Some(Overlay::DmPicker) => "DM",
            Some(Overlay::Agents | Overlay::LocalAgents) => "AGENTS",
            Some(Overlay::AgentNew) => "NEW AGENT",
            Some(Overlay::Approval | Overlay::AppApproval) => "APPROVE",
            Some(Overlay::Artifacts) => "ARTIFACTS",
            Some(Overlay::Broker) => "BROKER",
            Some(Overlay::Profile) => "PROFILE",
            Some(Overlay::Palette) => "PALETTE",
            Some(Overlay::Huddle) => "HUDDLE",
            Some(Overlay::Apps) => "APPS",
            Some(Overlay::Help | Overlay::WhichKey | Overlay::Actions) | None
                if self.presentation.composer_target.is_some() =>
            {
                "INSERT"
            }
            Some(Overlay::Help | Overlay::WhichKey | Overlay::Actions) | None
                if self.presentation.route == Route::Inbox =>
            {
                "INBOX"
            }
            None if self.app_has_focus() => "APP",
            Some(Overlay::Help | Overlay::WhichKey | Overlay::Actions) | None => "NORMAL",
        };
        let mode_group = match self.presentation.overlay {
            Some(Overlay::Command) => HighlightGroup::StatusModeCommand,
            _ if self.presentation.composer_target.is_some() => HighlightGroup::StatusModeInsert,
            _ => HighlightGroup::StatusMode,
        };
        let rate_limit_notice = self
            .rate_limit_display_seconds
            .filter(|_| self.connection == ConnectionState::Online)
            .map(|seconds| format!("relay busy · retrying in {seconds}s"));
        let notice = rate_limit_notice.unwrap_or_else(|| {
            self.status_error
                .as_deref()
                .map(sanitize::single_line)
                .unwrap_or_default()
        });
        let activity = self
            .app_status_line()
            .or_else(|| self.visible_agent_typing_line(area.width));
        let app_focused = self.app_has_focus() && self.presentation.overlay.is_none();
        let channel = self.current_channel().map(|channel| {
            let name = sanitize::single_line(&channel.name);
            if channel.kind.is_dm() {
                name
            } else {
                format!("#{name}")
            }
        });
        let mut view = crate::ui::status::StatusBarView::new(mode, self.connection);
        view.mode_group = mode_group;
        view.channel = channel.as_deref();
        view.activity = activity.as_deref();
        view.notice = &notice;
        view.app_focused = app_focused;
        if let Some((lines, priority)) = self.status_huddle_slot() {
            view.huddle_slot_shortening(lines, priority);
        }
        if let Some((line, priority)) = self.status_mark_slot() {
            view.mark_slot(line, priority);
            // The demo's relay is this process: "online" beside "offline"
            // would contradict the mark.
            view.mark_is_connection = self.demo;
        }
        crate::ui::status::render(frame, area, &self.theme, view);
    }

    /// The huddle strip, left of centre in the status line, longest first:
    /// the status line takes a shorter one before it drops it. While a call
    /// is on it is the strip of `huddle_lanes`; with only a huddle in the
    /// channel, what v0.16.0 said.
    fn status_huddle_slot(&self) -> Option<(Vec<Line<'static>>, u8)> {
        if let Some(lanes) = self
            .huddle_strip_shown()
            .then(|| self.huddle_lanes())
            .flatten()
        {
            return Some((
                crate::ui::huddle_lanes::strips(&lanes, &self.theme).to_vec(),
                crate::ui::status::priority::HUDDLE,
            ));
        }
        let active = self.active_huddle()?;
        let call = match self.huddle_call_state() {
            Some(true) => " · you: muted",
            Some(false) => " · you: LIVE",
            None => "",
        };
        Some((
            vec![Line::from(Span::styled(
                format!(
                    "🎧 huddle · {} in it{call} · :huddle",
                    active.participants.len()
                ),
                self.theme.style(HighlightGroup::StatusAgent),
            ))],
            crate::ui::status::priority::HUDDLE,
        ))
    }

    /// SEAM (agent `demo`, A1): a mark at the right of the status line, such
    /// as `DEMO · generated, offline` with `priority::ALWAYS`. None outside
    /// the demo.
    fn status_mark_slot(&self) -> Option<(Line<'static>, u8)> {
        self.demo.then(|| {
            (
                Line::from(Span::styled(
                    crate::demo::STATUS_MARK,
                    self.theme.style(HighlightGroup::StatusOnline),
                )),
                crate::ui::status::priority::ALWAYS,
            )
        })
    }
    fn render_overlay(&mut self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        if self.presentation.overlay.is_some() {
            // Behind an overlay the screen steps back (L4); the overlay
            // clears and draws its own cells.
            crate::ui::frame::dim(frame.buffer_mut(), area, &self.theme);
        }
        match self.presentation.overlay {
            Some(Overlay::Help) => {
                let actions = derive_actions(self.action_context());
                frame.render_widget(Clear, area);
                // Wrapped here to the frame's inside, so the scroll knows
                // exactly how many rows there are (v0.17 C3).
                let lines = crate::ui::help::help_lines(
                    &self.keymap,
                    if self.presentation.route == Route::Inbox {
                        KeyScope::Inbox
                    } else {
                        KeyScope::Workspace
                    },
                    &actions,
                    usize::from(area.width.saturating_sub(2)),
                );
                self.help_scroll
                    .set_view(lines.len(), usize::from(area.height.saturating_sub(2)));
                let shown = lines
                    .iter()
                    .skip(self.help_scroll.offset)
                    .take(self.help_scroll.page)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                frame.render_widget(
                    Paragraph::new(shown)
                        .style(self.theme.style(HighlightGroup::Normal))
                        .block(crate::ui::frame::modal(
                            &self.theme,
                            BorderSurface::Modal,
                            self.help_scroll.title().trim(),
                            &[("Esc", "close")],
                        )),
                    area,
                );
            }
            Some(Overlay::WhichKey) => {
                // Sized to show every key (v0.17 C6).
                let (popup, lines) = crate::ui::help::leader_panel(
                    &self.keymap,
                    if self.presentation.route == Route::Inbox {
                        KeyScope::Inbox
                    } else {
                        KeyScope::Workspace
                    },
                    self.input_router.sequence(),
                    area,
                );
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Paragraph::new(lines.join("\n"))
                    .style(self.theme.style(HighlightGroup::Normal))
                    .block(crate::ui::frame::modal(
                        &self.theme,
                        BorderSurface::Modal,
                        "leader",
                        &[],
                    ))
                    .wrap(Wrap { trim: false }),
                    popup,
                );
            }
            Some(Overlay::Actions) => {
                self.render_action_menu(frame, area, hit_map);
            }
            Some(Overlay::Confirmation) => match self.presentation.confirmation {
                Some(ConfirmationKind::Quit) => self.render_prompt(
                    frame,
                    area,
                    " quit bzz? ",
                    "Press y to quit or n/Esc to stay in bzz",
                ),
                Some(ConfirmationKind::Delete) => self.render_prompt(
                    frame,
                    area,
                    " delete message? ",
                    "Press y to delete or n/Esc to cancel",
                ),
                Some(ConfirmationKind::Report) => self.render_prompt(
                    frame,
                    area,
                    if self
                        .pending_report
                        .as_ref()
                        .is_some_and(|(_, _, author_only)| *author_only)
                    {
                        " report this author to moderators? "
                    } else {
                        " report message to moderators? "
                    },
                    &format!(
                        "Reason: {} · press y to report or n/Esc to cancel",
                        self.pending_report
                            .as_ref()
                            .map_or("", |(reason, _, _)| reason.as_str())
                    ),
                ),
                Some(ConfirmationKind::InboxRead) => self.render_prompt(
                    frame,
                    area,
                    " mark visible Inbox work read? ",
                    &format!(
                        "Press y to mark {} conversation(s) read or n/Esc to cancel",
                        self.pending_inbox_read.len()
                    ),
                ),
                Some(ConfirmationKind::ArtifactDelete) => self.render_prompt(
                    frame,
                    area,
                    " delete artifact? ",
                    &format!(
                        "\"{}\" leaves every current view and can be restored · y delete · n/Esc cancel",
                        self.artifact_panel
                            .selected()
                            .and_then(|item| item.head.title.clone())
                            .unwrap_or_default()
                    ),
                ),
                Some(ConfirmationKind::HuddleStart) => self.render_prompt(
                    frame,
                    area,
                    " start a huddle? ",
                    "Everyone in this channel can see it and join · you join muted · y start · n/Esc cancel",
                ),
                Some(ConfirmationKind::HuddleJoin) => match &self.huddle_viz.mic_check {
                    // Your lane, live before anything is sent (plan B7).
                    Some(run) => crate::ui::huddle_mic::render(
                        frame.buffer_mut(),
                        centered(area, 74, crate::ui::huddle_mic::HEIGHT),
                        &run.check,
                        self.huddle_viz.look,
                        self.config.ui.motion(),
                        &self.theme,
                    ),
                    None => self.render_prompt(
                        frame,
                        area,
                        " join the huddle? ",
                        "bzz opens your microphone and speaker; you start muted · y join · n/Esc cancel",
                    ),
                },
                Some(ConfirmationKind::HuddleEnd) => self.render_prompt(
                    frame,
                    area,
                    " end the huddle for everyone? ",
                    "Everyone is disconnected · y end · n/Esc cancel",
                ),
                Some(ConfirmationKind::ArtifactMove) => {
                    let detail = self.artifact_move_preview();
                    self.render_prompt(frame, area, " move artifact? ", &detail)
                }
                Some(ConfirmationKind::ClearDraft) => self.render_prompt(
                    frame,
                    area,
                    " clear draft? ",
                    "Press y to remove text and attachments or n/Esc to keep the draft",
                ),
                None => {}
            },
            Some(Overlay::Finder) => self.render_finder(frame, area, hit_map),
            Some(Overlay::Command) => match &self.command_answer {
                None => {
                    self.render_prompt(frame, area, " command ", &format!(":{}", self.command))
                }
                // The answer to the last Enter, wrapped under the text as
                // typed, never cut (v0.17 C4).
                Some(answer) => {
                    let inner = usize::from((area.width * 70 / 100).saturating_sub(3)).max(10);
                    let mut lines = vec![sanitize::single_line(&format!(":{}", self.command))];
                    lines.push(String::new());
                    lines.extend(crate::ui::help::wrap_text(
                        &sanitize::single_line(answer),
                        inner,
                    ));
                    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
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
                                    .title(" command · edit and Enter, or Esc "),
                            ),
                        popup,
                    );
                }
            },
            Some(Overlay::Theme) => self.render_theme_picker(frame, area, hit_map),
            Some(Overlay::Reaction) => {
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
                if let Some(query) = &self.reaction_query {
                    let list = if self.reaction_candidates.is_empty() {
                        "no matching custom emoji".to_owned()
                    } else {
                        self.reaction_candidates
                            .iter()
                            .enumerate()
                            .map(|(index, emoji)| {
                                if index == self.reaction_candidate {
                                    format!("[:{}:]", emoji.shortcode)
                                } else {
                                    format!(":{}:", emoji.shortcode)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("  ")
                    };
                    self.render_prompt_with_keys(
                        frame,
                        area,
                        "custom emoji",
                        &[("Tab", "next"), ("Enter", "react"), ("Esc", "back")],
                        &sanitize::single_line(&format!(":{query}  {list}")),
                    );
                    return;
                }
                self.render_prompt_with_keys(
                    frame,
                    area,
                    "reaction",
                    &[("Enter", "toggle"), (":", "custom"), ("Esc", "close")],
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
            Some(Overlay::MediaPreview) => self.render_media_preview(frame, area),
            Some(Overlay::Attachment) => self.render_prompt_with_keys(
                frame,
                area,
                if self.presentation.attachment_prompt == Some(AttachmentPrompt::Save) {
                    "save attachment · no overwrite"
                } else {
                    "attach by local path"
                },
                if self.presentation.attachment_prompt == Some(AttachmentPrompt::Save) {
                    &[("Enter", "save"), ("Esc", "cancel")]
                } else {
                    &[("Enter", "upload"), ("Esc", "cancel")]
                },
                &self.attachment_input,
            ),
            Some(Overlay::Search) => {
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
            Some(Overlay::Profile) => {
                if let Some(card) = &self.profile_card {
                    let popup = centered(area, 80, area.height.saturating_sub(2).min(28));
                    let channels = &self.channels;
                    let name = |id: Uuid| {
                        channels
                            .iter()
                            .find(|channel| channel.id == id)
                            .map(|channel| channel.name.clone())
                    };
                    crate::ui::profile::render(
                        frame,
                        popup,
                        card,
                        crate::clock::WallClock::now_unix(&crate::clock::SystemClock),
                        &name,
                        &self.theme,
                    );
                }
            }
            Some(Overlay::Apps) => {
                let lines = self.apps_panel_lines();
                let height = u16::try_from(lines.len().saturating_add(2))
                    .unwrap_or(u16::MAX)
                    .min(area.height.saturating_sub(2))
                    .max(4);
                let popup = centered(area, 90, height);
                frame.render_widget(Clear, popup);
                let text = lines
                    .into_iter()
                    .map(|(line, selected)| {
                        if selected {
                            Line::styled(format!("› {line}"), self.theme.style(HighlightGroup::Selection))
                        } else {
                            Line::from(format!("  {line}"))
                        }
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(text)
                        .style(self.theme.style(HighlightGroup::Normal))
                        .wrap(Wrap { trim: false })
                        .block(crate::ui::frame::modal(
                            &self.theme,
                            BorderSurface::Modal,
                            "apps",
                            &[("Enter", "open here"), ("Esc", "close")],
                        )),
                    popup,
                );
            }
            Some(Overlay::Huddle) if self.huddle_zoomed() => {
                // The zoomed view: the whole window but the status line.
                let zoom_area = Rect {
                    height: area.height.saturating_sub(1),
                    ..area
                };
                if let Some(lanes) = self.huddle_lanes() {
                    let channel = self
                        .current_channel()
                        .map(|channel| channel.name.clone())
                        .unwrap_or_default();
                    let zoom = crate::ui::huddle_zoom::Zoom {
                        lanes,
                        spectrum: &self.huddle_viz.spectrum,
                        style: self.huddle_style(),
                        channel: &channel,
                        picker: self.huddle_viz.picker,
                    };
                    crate::ui::huddle_zoom::render(
                        frame.buffer_mut(),
                        zoom_area,
                        &zoom,
                        &self.theme,
                    );
                }
            }
            Some(Overlay::Huddle) => {
                let popup = centered(area, 90, area.height.saturating_sub(2).min(30));
                let view = self.huddle_panel_view();
                let lanes = self.huddle_lanes();
                crate::ui::huddle::render_with_lanes(
                    frame,
                    popup,
                    &view,
                    &self.theme,
                    lanes.as_ref(),
                );
            }
            Some(Overlay::Broker) => {
                let popup = centered(area, 100, area.height.saturating_sub(2).min(30));
                let community = self.active_community_id();
                let agents = self
                    .config
                    .hosted_agents
                    .iter()
                    .filter(|agent| Some(agent.community) == community)
                    .cloned()
                    .collect::<Vec<_>>();
                let activity = self
                    .broker
                    .as_ref()
                    .map(|broker| broker.host.activity())
                    .unwrap_or_default();
                let local = self
                    .local_agents
                    .as_ref()
                    .map(|agents| {
                        agents
                            .host
                            .status()
                            .iter()
                            .map(crate::agents::runtime::agent::AgentStatus::line)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::ui::broker::render(
                    frame,
                    popup,
                    &agents,
                    &local,
                    &activity,
                    crate::ui::broker::BrokerStatus {
                        serving: self.broker.as_ref().map(|broker| broker.server.address.port()),
                        paused_all: self.broker.as_ref().is_some_and(|broker| broker.host.paused_all()),
                        selected: self.broker_selected,
                        agent_presence: self.config.ui.broker_agent_presence
                            && self.config.ui.publish_presence,
                    },
                    &self.theme,
                );
            }
            Some(Overlay::Artifacts) => {
                let popup = centered(area, 104, area.height.saturating_sub(2).min(30));
                let names = self
                    .channels
                    .iter()
                    .map(|channel| (channel.id, channel.name.clone()))
                    .collect::<HashMap<_, _>>();
                crate::ui::artifacts::render(
                    frame,
                    popup,
                    &self.artifact_panel,
                    &names,
                    &self.profiles,
                    &self.theme,
                );
            }
            Some(Overlay::LocalAgents) => {
                let popup = centered(area, 98, area.height.saturating_sub(2).min(44));
                let rows = self.local_agent_rows();
                crate::ui::local_agents::render(
                    frame,
                    popup,
                    &rows,
                    &self.local_agent_panel,
                    self.local_agents.is_some(),
                    &self.theme,
                );
            }
            Some(Overlay::AppApproval) => {
                let popup = centered(area, 100, area.height.saturating_sub(2).min(48));
                #[cfg(feature = "apps")]
                {
                    let others = self.apps.waiting.len().saturating_sub(1);
                    crate::ui::app_approval::render(
                        frame,
                        popup,
                        &mut self.apps.review,
                        others,
                        &self.theme,
                    );
                }
                #[cfg(not(feature = "apps"))]
                let _ = popup;
            }
            Some(Overlay::Approval) => {
                let popup = centered(area, 96, area.height.saturating_sub(2).min(40));
                let asked = self
                    .approval
                    .selected
                    .and_then(|id| self.approvals.iter().find(|asked| asked.id == id))
                    .cloned();
                let labels = asked
                    .as_ref()
                    .map(|asked| (self.conversation_label(asked), self.requester_label(asked)));
                let view = asked.as_ref().zip(labels.as_ref()).map(|(asked, labels)| {
                    crate::ui::approval::ApprovalView {
                        asked,
                        conversation: &labels.0,
                        requester: &labels.1,
                        others: self.approvals.len().saturating_sub(1),
                        seconds_left: seconds_left(asked),
                    }
                });
                crate::ui::approval::render(frame, popup, view, &mut self.approval, &self.theme);
            }
            Some(Overlay::AgentNew) => {
                if let Some(form) = &self.agent_form {
                    let popup = centered(area, 92, area.height.saturating_sub(2).min(40));
                    let community = self
                        .config
                        .communities
                        .get(self.selected_community)
                        .map_or("", |entry| entry.label.as_str());
                    let owner = self
                        .config
                        .communities
                        .get(self.selected_community)
                        .and_then(|entry| {
                            self.config
                                .identities
                                .iter()
                                .find(|identity| identity.id == entry.identity_id)
                        })
                        .map_or("you", |identity| identity.label.as_str());
                    crate::ui::local_agents::render_form(
                        frame, popup, form, community, owner, &self.theme,
                    );
                }
            }
            Some(Overlay::Agents) => {
                let popup = centered(area, 96, area.height.saturating_sub(2).min(34));
                crate::ui::agents::render(
                    frame,
                    popup,
                    &self.agent_directory,
                    &self.own_agents(),
                    &self.profiles,
                    self.self_pubkey(),
                    &self.theme,
                    hit_map,
                );
            }
            Some(Overlay::DmPicker) => {
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
            Some(Overlay::Palette) => {
                self.refresh_palette();
                if let Some(palette) = &self.palette {
                    crate::ui::palette::render(frame, area, palette, &self.theme);
                }
            }
            None => {}
        }
    }
    fn render_action_menu(&self, frame: &mut Frame<'_>, area: Rect, hit_map: &mut HitMap) {
        let Some(menu) = &self.action_menu else {
            return;
        };
        let height = u16::try_from(menu.entries().len().saturating_add(4)).unwrap_or(u16::MAX);
        let popup = centered(area, 86, height.min(area.height.saturating_sub(2)).max(4));
        let entries = menu.entries();
        for (index, entry) in entries.iter().enumerate() {
            if let Some(row) = list_row(popup, index, 1) {
                hit_map.push(row, HitTarget::ActionMenu(entry.action));
            }
        }
        let items = entries.iter().map(|entry| {
            let text = match entry.reason {
                Some(reason) => format!("{} — {reason}", entry.label),
                None => entry.label.to_owned(),
            };
            ListItem::new(text).style(self.theme.style(if entry.enabled {
                HighlightGroup::Normal
            } else {
                HighlightGroup::SidebarText
            }))
        });
        let selected = menu
            .selected()
            .and_then(|selected| entries.iter().position(|entry| *entry == selected));
        let mut state = ratatui::widgets::ListState::default().with_selected(selected);
        frame.render_widget(Clear, popup);
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("› ")
                .highlight_style(self.theme.style(HighlightGroup::SelectedRow))
                .block(crate::ui::frame::modal(
                    &self.theme,
                    BorderSurface::Modal,
                    "actions",
                    &[("Enter", "run"), ("Esc", "close")],
                )),
            popup,
            &mut state,
        );
    }

    fn render_media_preview(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let popup = centered(area, 86, 80.min(area.height.saturating_sub(2)));
        frame.render_widget(Clear, popup);
        let Some(attachment) = self.preview_attachment().cloned() else {
            self.presentation.close_overlay();
            return;
        };
        let block = crate::ui::frame::modal(
            &self.theme,
            BorderSurface::Modal,
            &format!(
                "attachment {}/{}",
                self.preview_index + 1,
                self.selected_message()
                    .map_or(0, |message| message.attachments.len())
            ),
            &[("[ ]", "navigate"), ("s", "save"), ("Esc", "close")],
        );
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
        const SHOWN: usize = 8;
        let popup = centered(area, 70, 12);
        frame.render_widget(Clear, popup);
        let rows = crate::ui::finder::rows(&self.finder.query, &self.channels);
        let start = self.finder.window_start(SHOWN);
        for (index, row) in rows.iter().skip(start).take(SHOWN).enumerate() {
            if let crate::ui::finder::FinderRow::Channel(channel) = row
                && let Some(row) = list_row(popup, index.saturating_add(1), 1)
            {
                hit_map.push(row, HitTarget::FinderChannel(channel.id.to_string()));
            }
        }
        let mut items = vec![ListItem::new(format!(
            "> {}",
            sanitize::single_line(&self.finder.query)
        ))];
        items.extend(
            rows.into_iter()
                .enumerate()
                .skip(start)
                .take(SHOWN)
                .map(|(index, row)| {
                    let mark = if index == self.finder.selected {
                        "›"
                    } else {
                        " "
                    };
                    let text = match row {
                        crate::ui::finder::FinderRow::Channel(channel) => format!(
                            "{mark} {}{}",
                            crate::ui::finder::marker(channel),
                            sanitize::single_line(&channel.name)
                        ),
                        crate::ui::finder::FinderRow::NewDm if self.finder.query.is_empty() => {
                            format!("{mark} → start a DM with someone")
                        }
                        crate::ui::finder::FinderRow::NewDm => format!(
                            "{mark} → start a DM: find people named \"{}\"",
                            sanitize::single_line(&self.finder.query)
                        ),
                    };
                    ListItem::new(text)
                }),
        );
        frame.render_widget(
            List::new(items)
                .style(self.theme.style(HighlightGroup::Normal))
                .block(crate::ui::frame::modal(
                    &self.theme,
                    BorderSurface::Picker,
                    "channel finder \u{b7} channels and DMs",
                    &[
                        ("\u{2191}\u{2193} Tab", "move"),
                        ("Enter", "open"),
                        ("Esc", "close"),
                    ],
                )),
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
                    crate::ui::frame::modal(
                        &self.theme,
                        BorderSurface::Picker,
                        "themes",
                        &[("Tab", "scope"), ("Enter", "save"), ("Esc", "cancel")],
                    )
                    .title(crate::ui::frame::info(&self.theme, scope)),
                ),
            popup,
        );
    }
    fn render_prompt(&self, frame: &mut Frame<'_>, area: Rect, title: &str, value: &str) {
        self.render_prompt_with_keys(frame, area, title, &[], value);
    }
    fn render_prompt_with_keys(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        title: &str,
        keys: &[(&str, &str)],
        value: &str,
    ) {
        let popup = centered(area, 70, 5);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(sanitize::text(value))
                .style(self.theme.style(HighlightGroup::Normal))
                .block(crate::ui::frame::modal(
                    &self.theme,
                    BorderSurface::Modal,
                    title,
                    keys,
                )),
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

/// Returns whether the visible workspace content is safe to acknowledge as
/// read. The channel list may retain focus while the selected channel's
/// timeline remains visibly at its live edge, so focus alone is not evidence
/// that the content was not read.
fn should_mark_visible_read(
    presentation: &PresentationState,
    timeline: &TimelineState,
    thread_timeline: &TimelineState,
) -> bool {
    presentation.route == Route::Workspace
        && presentation.overlay.is_none()
        && presentation.composer_target.is_none()
        && if presentation.focus == FocusSurface::Context {
            thread_timeline.visible_at_live_edge()
        } else {
            timeline.visible_at_live_edge()
        }
}

/// Produces a channel read marker only when the bottom of the displayed
/// timeline belongs to the currently selected channel. This prevents a rapid
/// sidebar selection change from acknowledging a channel whose messages have
/// not been loaded yet.
fn timeline_read_mark(
    timeline: &TimelineState,
    channel: Uuid,
    messages: &[Message],
) -> Option<(String, u32)> {
    let last = messages.last()?;
    (timeline.visible_at_live_edge() && last.channel_id == channel).then(|| {
        (
            channel.to_string(),
            u32::try_from(last.created_at).unwrap_or(u32::MAX),
        )
    })
}

fn clear_visible_unread(
    computed: &mut HashSet<Uuid>,
    manual: &mut HashSet<Uuid>,
    channel: Uuid,
) -> bool {
    computed.remove(&channel);
    manual.remove(&channel)
}

/// A terminal backend can fail repeatedly after its descriptor becomes
/// unavailable. Throttle transient errors and park a permanently ended stream
/// so neither case can turn the UI into a redraw loop.
async fn next_terminal_event<S>(input: &mut S) -> Option<TerminalEvent>
where
    S: futures_util::Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
{
    match input.next().await {
        // Where keys enter bzz: one form for every platform's terminal.
        Some(Ok(TerminalEvent::Key(key))) => Some(TerminalEvent::Key(
            crate::ui::keymap::normalize_key_event(key),
        )),
        Some(Ok(event)) => Some(event),
        Some(Err(_)) => {
            tokio::time::sleep(TERMINAL_ERROR_YIELD).await;
            None
        }
        None => std::future::pending().await,
    }
}

/// The next terminal event: first what was read ahead, then the stream.
async fn next_read_ahead_event<S>(
    read_ahead: &mut std::collections::VecDeque<TerminalEvent>,
    input: &mut S,
) -> Option<TerminalEvent>
where
    S: futures_util::Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
{
    match read_ahead.pop_front() {
        Some(event) => Some(event),
        None => next_terminal_event(input).await,
    }
}

/// Whether a key press already waits behind the one being handled: the
/// events ready now are read ahead (at most a few, never waiting for one)
/// and kept for the loop. A release is not a press, but a release of a
/// character outside ASCII may be an Alt code, which is (`KeyDoor`).
fn presses_wait<S>(
    read_ahead: &mut std::collections::VecDeque<TerminalEvent>,
    input: &mut S,
) -> bool
where
    S: futures_util::Stream<Item = std::io::Result<TerminalEvent>> + Unpin,
{
    const READ_AHEAD: usize = 8;
    let is_press = |event: &TerminalEvent| match event {
        TerminalEvent::Key(key) => {
            key.kind != KeyEventKind::Release
                || matches!(key.code, KeyCode::Char(character) if !character.is_ascii())
        }
        TerminalEvent::Paste(_) => true,
        _ => false,
    };
    if read_ahead.iter().any(is_press) {
        return true;
    }
    while read_ahead.len() < READ_AHEAD {
        let Some(Some(event)) = futures_util::FutureExt::now_or_never(next_terminal_event(input))
        else {
            return false;
        };
        let press = is_press(&event);
        read_ahead.push_back(event);
        if press {
            return true;
        }
    }
    false
}

/// Receives at most one relay event per turn. A flood can overrun the bounded
/// broadcast receiver; yield control on that condition so local input and
/// attachment staging cannot be starved while the relay is catching up.
#[cfg(test)]
async fn next_supervisor_event(
    events: &mut broadcast::Receiver<SupervisorEvent>,
) -> Option<SupervisorEvent> {
    match events.recv().await {
        Ok(event) => Some(event),
        Err(broadcast::error::RecvError::Lagged(_)) => {
            tokio::time::sleep(RELAY_LAG_YIELD).await;
            None
        }
        // A live Runtime owns a sender, so a closed receiver is exceptional.
        // Never turn it into a ready future: that would spin the UI loop.
        Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
    }
}

async fn next_network(runtime: &mut Option<Runtime>) -> Option<SupervisorEvent> {
    match runtime {
        Some(runtime) => match runtime.events.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                runtime
                    .supervisor
                    .diagnostics()
                    .emit(DiagnosticEvent::ReceiverLagged {
                        skipped_event_count: skipped,
                    });
                tokio::time::sleep(RELAY_LAG_YIELD).await;
                None
            }
            Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}
/// What a bare `:status` says (v0.17 D6).
fn status_notice(status: Option<&crate::protocol::presence::UserStatus>, now: u64) -> String {
    match status.filter(|status| status.visible(now)) {
        Some(status) => {
            let text = [status.emoji.as_str(), status.text.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .map(sanitize::single_line)
                .collect::<Vec<_>>()
                .join(" ");
            let until = status
                .expires_at
                .and_then(|at| u64::try_from(at).ok())
                .map(|at| {
                    let left = at.saturating_sub(now);
                    let (amount, unit) = match left {
                        0..=59 => (left, "s"),
                        60..=3_599 => (left / 60, "m"),
                        3_600..=86_399 => (left / 3_600, "h"),
                        _ => (left / 86_400, "d"),
                    };
                    format!(" · ends in {amount}{unit}")
                })
                .unwrap_or_default();
            format!("your status: {text}{until} · :status clear clears it")
        }
        None => "you have no status · :status <text> sets one".into(),
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
        Error::Config(message)
        | Error::Protocol(message)
        | Error::Access(message)
        | Error::SignerRefused(message) => sanitize::single_line(message),
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

/// The last `bytes` of a file as text; empty when it cannot be read.
fn read_tail(path: &std::path::Path, bytes: u64) -> String {
    use std::io::{Read as _, Seek as _};
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    if file
        .seek(std::io::SeekFrom::Start(length.saturating_sub(bytes)))
        .is_err()
    {
        return String::new();
    }
    let mut tail = Vec::new();
    let _ = file.take(bytes).read_to_end(&mut tail);
    String::from_utf8_lossy(&tail).into_owned()
}

/// What `:approvals` says when no agent asks for anything.
const NOTHING_WAITS: &str = "no command waits for your answer";

/// The whole seconds before a command that waits counts as refused.
fn seconds_left(asked: &crate::agents::runtime::approval::Asked) -> u64 {
    asked
        .expires
        .saturating_duration_since(Instant::now())
        .as_secs()
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
    use std::{
        collections::HashSet,
        sync::Arc,
        time::{Duration, Instant},
    };

    use super::{
        TypingSubscriptionTransition, clear_visible_unread, identity_recovery_connection,
        next_supervisor_event, next_terminal_event, should_mark_visible_read, timeline_read_mark,
        triggers_directory_refresh, typing_subscription_transition,
    };
    use crate::{
        config::Config,
        domain::{
            Channel, ChannelKind, ConnectionState, InboxCategory, InboxItem, MentionCandidate,
            Message, ThreadSummary, Visibility,
        },
        error::Error,
        paths::Paths,
        store::{Store, writer::StoreHandle},
        ui::{
            actions::{ActionMenu, ContextAction},
            hit_map::HitTarget,
            keymap::UiAction,
            mention_picker::MentionPicker,
            state::{ComposerTarget, FocusSurface, Overlay, PresentationState, Route},
            timeline::TimelineState,
        },
    };
    use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use tempfile::TempDir;
    use uuid::Uuid;

    #[derive(Clone)]
    struct TestClipboard(crate::media::clipboard::ClipboardContents);

    impl crate::media::clipboard::ClipboardReader for TestClipboard {
        fn read_once(&self) -> crate::media::clipboard::ClipboardContents {
            self.0.clone()
        }
    }

    #[derive(Clone)]
    struct TestFilePicker(crate::media::file_picker::FilePickerOutcome);

    #[async_trait::async_trait]
    impl crate::media::file_picker::FilePicker for TestFilePicker {
        async fn pick_files(&self) -> crate::media::file_picker::FilePickerOutcome {
            self.0.clone()
        }
    }

    async fn attachment_test_app(temporary: &TempDir) -> super::App {
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
            label: "attachment-test".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:attachment-test".into(),
        };
        app.config.identities.push(identity.clone());
        let community = app
            .config
            .add_community(
                "attachment-test".into(),
                "wss://attachment.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        let synced = app.config.clone();
        app.store
            .call(move |store| store.sync_config(&synced))
            .await
            .unwrap();
        app.presentation.composer_target = Some(ComposerTarget {
            community_id: community,
            channel_id: Uuid::new_v4(),
            thread_root_id: None,
            parent_event_id: None,
        });
        app
    }

    /// v0.14 M3: Space and closing punctuation select only an exact, unique
    /// name; a longer known name keeps them ordinary; a multi-word query
    /// stays open only while it starts a known name.
    #[tokio::test]
    async fn space_selects_only_an_exact_unique_mention() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let target = app.presentation.composer_target.clone().unwrap();
        let (community, channel) = (target.community_id, target.channel_id);
        let relay = Keys::generate();
        let people = ["Honey", "Wes", "Wes Smith"].map(|name| (Keys::generate(), name));
        let mut events = vec![
            EventBuilder::new(Kind::Custom(39_000), "")
                .tags([
                    Tag::parse(["d", &channel.to_string()]).unwrap(),
                    Tag::parse(["name", "general"]).unwrap(),
                    Tag::parse(["t", "stream"]).unwrap(),
                    Tag::parse(["public"]).unwrap(),
                ])
                .sign_with_keys(&relay)
                .unwrap(),
            EventBuilder::new(Kind::Custom(39_002), "")
                .tags(
                    std::iter::once(Tag::parse(["d", &channel.to_string()]).unwrap()).chain(
                        people.iter().map(|(keys, _)| {
                            Tag::parse(["p", &keys.public_key().to_hex()]).unwrap()
                        }),
                    ),
                )
                .sign_with_keys(&relay)
                .unwrap(),
        ];
        for (keys, name) in &people {
            events.push(
                EventBuilder::new(
                    Kind::Metadata,
                    serde_json::json!({ "name": name }).to_string(),
                )
                .sign_with_keys(keys)
                .unwrap(),
            );
        }
        let relay_pubkey = relay.public_key().to_hex();
        app.store
            .call(move |store| {
                store.pin_relay_pubkey(community, &relay_pubkey)?;
                for event in &events {
                    store.apply_event(community, event)?;
                }
                Ok(())
            })
            .await
            .unwrap();
        let key = |index: usize| people[index].0.public_key().to_hex();
        let typed = |text: &str| {
            text.chars()
                .map(super::KeyAction::Character)
                .collect::<Vec<_>>()
        };

        for action in typed("hi @Hon") {
            app.insert_action(action).await.unwrap();
        }
        assert!(app.mention_picker.is_some());
        for action in typed("ey ok") {
            app.insert_action(action).await.unwrap();
        }
        assert_eq!(app.composer.body, "hi @Honey ok");
        assert_eq!(app.composer.mentions().len(), 1);
        assert_eq!(app.composer.mentions()[0].pubkey, key(0));

        // `Wes Smith` continues `Wes`: Space and `,` stay ordinary text.
        app.composer.clear();
        for action in typed("@Wes ") {
            app.insert_action(action).await.unwrap();
        }
        assert!(app.composer.mentions().is_empty());
        // `Wes ` is a complete name and a space: the chooser closes, and the
        // next word may still complete `Wes Smith` (section 5).
        assert!(app.mention_picker.is_none());
        for action in typed("Smith,") {
            app.insert_action(action).await.unwrap();
        }
        assert_eq!(app.composer.body, "@Wes Smith,");
        assert_eq!(app.composer.mentions()[0].pubkey, key(2));

        // A multi-word query that starts no name closes the chooser.
        app.composer.clear();
        for action in typed("@Honey bee") {
            app.insert_action(action).await.unwrap();
        }
        assert!(app.mention_picker.is_none());

        // A stale draft that mentions someone who left is not sent.
        app.composer.clear();
        for action in typed("@") {
            app.insert_action(action).await.unwrap();
        }
        assert!(app.composer.accept_mention(0..1, "Ghost", &"9".repeat(64)));
        app.mention_picker = None;
        app.insert_action(super::KeyAction::Submit).await.unwrap();
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|error| error.contains("no longer in this channel")),
            "{:?}",
            app.status_error
        );
        assert_eq!(app.composer.body, "@Ghost");
    }

    /// From the run against Buzz 1.0: `@Sc`, Enter, and text typed at once
    /// gave "@Scouthello from bzz", sent with no mention tag and no warning.
    /// Accepting a name in the picker, with Enter or with Tab, leaves the
    /// cursor after a space; and when an edit does turn a mention into
    /// plain text, bzz says so.
    #[tokio::test]
    async fn the_picker_leaves_the_cursor_after_a_space_and_a_lost_mention_is_told() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let target = app.presentation.composer_target.clone().unwrap();
        let (community, channel) = (target.community_id, target.channel_id);
        let relay = Keys::generate();
        let scout = Keys::generate();
        let events = vec![
            EventBuilder::new(Kind::Custom(39_000), "")
                .tags([
                    Tag::parse(["d", &channel.to_string()]).unwrap(),
                    Tag::parse(["name", "general"]).unwrap(),
                    Tag::parse(["t", "stream"]).unwrap(),
                    Tag::parse(["public"]).unwrap(),
                ])
                .sign_with_keys(&relay)
                .unwrap(),
            EventBuilder::new(Kind::Custom(39_002), "")
                .tags([
                    Tag::parse(["d", &channel.to_string()]).unwrap(),
                    Tag::parse(["p", &scout.public_key().to_hex()]).unwrap(),
                ])
                .sign_with_keys(&relay)
                .unwrap(),
            EventBuilder::new(
                Kind::Metadata,
                serde_json::json!({ "name": "Scout" }).to_string(),
            )
            .sign_with_keys(&scout)
            .unwrap(),
        ];
        let relay_pubkey = relay.public_key().to_hex();
        app.store
            .call(move |store| {
                store.pin_relay_pubkey(community, &relay_pubkey)?;
                for event in &events {
                    store.apply_event(community, event)?;
                }
                Ok(())
            })
            .await
            .unwrap();
        let scout = scout.public_key().to_hex();
        let typed = |text: &str| {
            text.chars()
                .map(super::KeyAction::Character)
                .collect::<Vec<_>>()
        };

        // Enter accepts the name, and the next word is typed at once.
        for accept in [super::KeyAction::Submit, super::KeyAction::Complete] {
            app.composer.clear();
            app.status_error = None;
            for action in typed("@Sc") {
                app.insert_action(action).await.unwrap();
            }
            assert!(app.mention_picker.is_some(), "{accept:?}");
            app.insert_action(accept).await.unwrap();
            assert!(app.mention_picker.is_none());
            assert_eq!(app.composer.body, "@Scout ", "{accept:?}");
            assert_eq!(app.composer.cursor, app.composer.body.len());
            for action in typed("hello from bzz") {
                app.insert_action(action).await.unwrap();
            }
            assert_eq!(app.composer.body, "@Scout hello from bzz", "{accept:?}");
            assert_eq!(
                app.composer
                    .mentions()
                    .iter()
                    .map(|mention| mention.pubkey.as_str())
                    .collect::<Vec<_>>(),
                [scout.as_str()],
                "{accept:?}: the message still mentions Scout"
            );
            assert_eq!(app.status_error, None, "{accept:?}");
        }

        // The space taken away again, and a letter typed onto the name: it
        // is no longer a mention, and bzz says so where the user looks.
        app.composer.clear();
        for action in typed("@Sc") {
            app.insert_action(action).await.unwrap();
        }
        app.insert_action(super::KeyAction::Submit).await.unwrap();
        app.insert_action(super::KeyAction::Backspace)
            .await
            .unwrap();
        assert_eq!(app.composer.mentions().len(), 1);
        assert_eq!(app.status_error, None, "the name itself is untouched");
        for action in typed("h") {
            app.insert_action(action).await.unwrap();
        }
        assert_eq!(app.composer.body, "@Scouth");
        assert!(app.composer.mentions().is_empty());
        let told = app.status_error.clone().unwrap_or_default();
        assert!(
            told.starts_with("@Scout is no longer a mention")
                && told.contains("nobody is notified"),
            "{told}"
        );
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("@Scout is no longer a mention"), "{screen}");

        // Cutting into the name says the same.
        app.composer.clear();
        app.status_error = None;
        for action in typed("@Sc") {
            app.insert_action(action).await.unwrap();
        }
        app.insert_action(super::KeyAction::Submit).await.unwrap();
        for _ in 0..2 {
            app.insert_action(super::KeyAction::Backspace)
                .await
                .unwrap();
        }
        assert_eq!(app.composer.body, "@Scou");
        assert!(app.composer.mentions().is_empty());
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|told| told.starts_with("@Scout is no longer a mention")),
            "{:?}",
            app.status_error
        );
    }

    #[test]
    fn typing_subscription_switch_replaces_req_without_close() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        assert_eq!(
            typing_subscription_transition(None, Some(first)),
            TypingSubscriptionTransition::Subscribe(first)
        );
        assert_eq!(
            typing_subscription_transition(Some(first), Some(second)),
            TypingSubscriptionTransition::Subscribe(second)
        );
        assert_eq!(
            typing_subscription_transition(Some(first), None),
            TypingSubscriptionTransition::Leave
        );
        assert_eq!(
            typing_subscription_transition(Some(first), Some(first)),
            TypingSubscriptionTransition::Unchanged
        );
    }

    #[test]
    fn semantic_roster_rows_trigger_authoritative_directory_refresh() {
        const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const AGENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        for event_type in ["member_joined", "member_left", "member_removed"] {
            let content = serde_json::json!({
                "type": event_type,
                "actor": OWNER,
                "target": AGENT,
            })
            .to_string();
            assert!(triggers_directory_refresh(40_099, &content));
        }
        assert!(triggers_directory_refresh(39_002, ""));
        assert!(triggers_directory_refresh(44_100, ""));
        assert!(triggers_directory_refresh(44_101, ""));
        assert!(!triggers_directory_refresh(
            40_099,
            r#"{"type":"channel_created"}"#
        ));
        assert!(!triggers_directory_refresh(40_099, "malformed"));
        assert!(!triggers_directory_refresh(9, "member_joined"));
    }

    /// M9.4: the owner sets per-agent, per-action policy and pauses one agent
    /// from the `:broker` panel; it is saved and shown at once.
    #[tokio::test]
    async fn broker_panel_edits_and_saves_agent_policy() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let community = app.active_community_id().unwrap();
        for label in ["first", "second"] {
            app.config
                .hosted_agents
                .push(crate::config::HostedAgentConfig {
                    id: Uuid::new_v4(),
                    label: label.into(),
                    pubkey: "b".repeat(64),
                    community,
                    owner_identity: Uuid::new_v4(),
                    auth_tag: "[]".into(),
                    credential_hash: "c".repeat(64),
                    allowed: vec!["channel.read".into()],
                    paused: false,
                    local: None,
                });
        }
        app.command = "broker".into();
        app.execute_command().await.unwrap();
        // Hosted actions in order: channel.read, message.post, message.reply,
        // profile.set, reaction.add, storage.address.
        app.broker_select(1);
        app.broker_toggle_action(1);
        app.broker_toggle_action(0);
        app.broker_toggle_paused();
        let saved = crate::config::Config::load(&app.paths).unwrap();
        assert_eq!(saved.hosted_agents[0].allowed, vec!["channel.read"]);
        assert!(!saved.hosted_agents[0].paused);
        assert_eq!(saved.hosted_agents[1].allowed, vec!["message.post"]);
        assert!(saved.hosted_agents[1].paused);
        let screen = screen_text(&mut app);
        assert!(screen.contains("› second · paused"), "{screen}");
        assert!(screen.contains("1[ ] channel.read"), "{screen}");
        assert!(screen.contains("2[x] message.post"), "{screen}");
        // A digit past the hosted actions changes nothing.
        app.broker_toggle_action(8);
        assert_eq!(
            crate::config::Config::load(&app.paths)
                .unwrap()
                .hosted_agents[1]
                .allowed,
            vec!["message.post"]
        );
    }

    /// M10 fixtures: every NIP-56 reason is accepted (then needs a selected
    /// message), anything else is a usage error; "send to channel" refuses a
    /// missing, foreign, reply, system or deleted root.
    #[tokio::test]
    async fn report_reasons_and_send_to_channel_roots_are_checked() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        for reason in buzz_sdk::REPORT_TYPES {
            for command in [
                format!("report {reason}"),
                format!("report author {reason} note"),
            ] {
                app.status_error = None;
                app.command = command.clone();
                app.execute_command().await.unwrap();
                assert_eq!(
                    app.status_error.as_deref(),
                    Some("select a message to report"),
                    "{command}"
                );
            }
        }
        for command in ["report", "report bogus", "report author", "report SPAM"] {
            app.status_error = None;
            app.command = command.into();
            app.execute_command().await.unwrap();
            assert!(
                app.status_error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("usage: :report [author]")),
                "{command}: {:?}",
                app.status_error
            );
        }

        let channel = Uuid::new_v4();
        let root = Message {
            event_id: "a".repeat(64),
            channel_id: channel,
            pubkey: "b".repeat(64),
            created_at: 1,
            content: "root".into(),
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
        };
        assert!(super::send_to_channel_root(Some(root.clone()), channel).is_ok());
        assert!(super::send_to_channel_root(None, channel).is_err());
        assert!(super::send_to_channel_root(Some(root.clone()), Uuid::new_v4()).is_err());
        let reply = Message {
            root_event_id: Some("c".repeat(64)),
            ..root.clone()
        };
        assert!(super::send_to_channel_root(Some(reply), channel).is_err());
        let deleted = Message {
            deleted: true,
            ..root.clone()
        };
        assert!(super::send_to_channel_root(Some(deleted), channel).is_err());
    }

    /// v0.15 M1: with the owner's identity unlocked the TUI runs the
    /// community's local agents beside the broker, which does not serve
    /// them; the panel shows each one's state; pausing one there stops it;
    /// locking stops them all.
    #[tokio::test]
    async fn local_agents_run_while_the_owner_is_unlocked_and_stop_on_lock() {
        use crate::agents::runtime::agent::Phase;
        use crate::config::{Harness, LocalAgentConfig};
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let owner_keys = nostr::Keys::generate();
        let owner = crate::auth::signer::SignerHandle::spawn(owner_keys.clone());
        let mut config = Config::default();
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "owner".into(),
            pubkey: owner_keys.public_key().to_hex(),
            backend: crate::config::KeyBackend::EncryptedFile,
            key_ref: "owner".into(),
        };
        config.identities.push(identity.clone());
        // A loopback port nothing listens on: no relay is needed to start.
        let community = config
            .add_community("m1".into(), "ws://127.0.0.1:9".into(), identity.id, true)
            .unwrap();
        // One local agent, and one the user runs himself.
        let (mut scout, _) = crate::broker::custody::create(
            &paths.keys_dir(),
            &owner,
            identity.id,
            community,
            "Scout",
        )
        .await
        .unwrap();
        scout.local = Some(LocalAgentConfig::new(Harness::PiDurable));
        config.hosted_agents.push(scout.clone());
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        store.pin_relay_pubkey(community, &"d".repeat(64)).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        // Started without a community, so that nothing asks for a passphrase.
        let mut app = super::App::new(Config::default(), paths.clone(), handle.clone())
            .await
            .unwrap();
        app.config = config;
        // Not unlocked: nothing runs.
        app.reconcile_local_agents().await;
        assert!(app.local_agents.is_none());

        app.runtime = Some(
            super::Runtime::build(
                &app.config,
                0,
                &paths,
                handle,
                Some(owner.clone()),
                crate::diagnostics::DiagnosticHandle::disabled(),
            )
            .unwrap(),
        );
        app.reconcile_broker().await;
        assert!(
            app.broker.is_none(),
            "the broker does not serve a local agent"
        );
        app.reconcile_local_agents().await;
        assert_eq!(app.status_error, None);
        let phase = |app: &super::App| {
            app.local_agents
                .as_ref()
                .and_then(|agents| agents.host.status().first().map(|status| status.phase))
        };
        let wait_for = async |app: &super::App, expected: Phase| {
            for _ in 0..500 {
                if phase(app) == Some(expected) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the agent never became {expected:?}: {:?}", phase(app));
        };
        // Its key is unsealed and its requests settled; no process yet.
        wait_for(&app, Phase::Idle).await;
        assert!(
            !crate::agents::runtime::process::run_dir(&paths.data_dir)
                .join(format!("{}.json", scout.id))
                .exists()
        );
        app.command = "broker".into();
        app.execute_command().await.unwrap();
        let screen = |app: &mut super::App| {
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };
        let shown = screen(&mut app);
        assert!(
            shown.contains("Scout · active · local: bzz runs it"),
            "{shown}"
        );
        assert!(shown.contains("Scout · pi-durable · idle"), "{shown}");
        assert!(shown.contains("┌ agent broker ─"), "{shown}");
        assert!(shown.contains(" not serving ┐"), "{shown}");

        // Space in the panel pauses the selected agent, and resumes it.
        app.broker_toggle_paused();
        assert!(app.config.hosted_agents[0].paused);
        app.reconcile_local_agents().await;
        wait_for(&app, Phase::Paused).await;
        let shown = screen(&mut app);
        assert!(shown.contains("Scout · pi-durable · paused"), "{shown}");
        app.broker_toggle_paused();
        app.reconcile_local_agents().await;
        wait_for(&app, Phase::Idle).await;

        // Locked: no agent runs, and the next unlock starts them again.
        app.presentation.close_overlay();
        app.command = "lock".into();
        app.execute_command().await.unwrap();
        assert!(app.local_agents.is_none() && app.runtime.is_none());
        app.reconcile_local_agents().await;
        assert!(app.local_agents.is_none());
        assert_eq!(app.local_agents_attempted, None);
        owner.lock().await;
    }

    /// v0.15 M4: `:agent new` asks what the command asks, checks before it
    /// makes anything, creates the agent all or nothing with the owner's
    /// confirmation, and `:agents` then shows it with its state, its last
    /// outcome and its capability record, and pauses, resumes, stops and
    /// shows the log of the one selected.
    #[cfg(unix)]
    #[tokio::test]
    async fn agent_new_in_the_tui_creates_an_agent_and_the_panel_manages_it() {
        use crate::agents::runtime::{agent::Phase, relay::Published};
        use crate::config::{Harness, LocalAgentState, WakeRule};
        use crate::ui::local_agents::FormStage;
        use std::os::unix::fs::PermissionsExt as _;

        /// The relay, recorded: who published which kind, and whether the
        /// next publish is refused.
        #[derive(Default)]
        struct Recorded {
            calls: std::sync::Mutex<Vec<(&'static str, u16)>>,
            refuse: std::sync::atomic::AtomicBool,
        }
        impl Recorded {
            fn publish(&self, who: &'static str, event: &nostr::Event) -> Published {
                self.calls.lock().unwrap().push((who, event.kind.as_u16()));
                if self.refuse.load(std::sync::atomic::Ordering::Acquire) {
                    Published::Refused("blocked: not today".into())
                } else {
                    Published::Accepted
                }
            }
        }
        #[async_trait::async_trait]
        impl crate::agents::manage::Publisher for Recorded {
            async fn as_owner(&self, event: nostr::Event) -> Published {
                self.publish("owner", &event)
            }
            async fn as_agent(
                &self,
                _: &crate::config::HostedAgentConfig,
                _: &crate::auth::signer::SignerHandle,
                event: nostr::Event,
            ) -> Published {
                self.publish("agent", &event)
            }
        }

        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        // Stand-ins for the owner's Node and npm: the checks and the install
        // of first use run for real, with nothing downloaded.
        let bin = temporary.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, script) in [
            ("node", "echo v22.19.0"),
            (
                "npm",
                "if [ \"$1\" = --version ]; then echo 11.0.0; exit 0; fi\nmkdir -p node_modules/pkg",
            ),
        ] {
            let program = bin.join(name);
            std::fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = format!("{}:/usr/bin:/bin", bin.display());

        let owner_keys = nostr::Keys::generate();
        let owner = crate::auth::signer::SignerHandle::spawn(owner_keys.clone());
        let mut config = Config::default();
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "ann".into(),
            pubkey: owner_keys.public_key().to_hex(),
            backend: crate::config::KeyBackend::EncryptedFile,
            key_ref: "owner".into(),
        };
        config.identities.push(identity.clone());
        // A loopback port nothing listens on: no relay is needed.
        let community = config
            .add_community("rikuy".into(), "ws://127.0.0.1:9".into(), identity.id, true)
            .unwrap();
        config.save(&paths).unwrap();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        store.pin_relay_pubkey(community, &"d".repeat(64)).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = super::App::new(Config::default(), paths.clone(), handle.clone())
            .await
            .unwrap();
        app.config = config;
        let recorded = Arc::new(Recorded::default());
        app.agent_publisher = Some(recorded.clone());
        app.agent_environment = Arc::new(move |name| (name == "PATH").then(|| path.clone()));
        let screen = |app: &mut super::App| {
            let mut terminal = Terminal::new(TestBackend::new(150, 46)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let check = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        // The answer of the checks, as the event loop hands it over.
        let checked = async |app: &mut super::App| loop {
            match tokio::time::timeout(Duration::from_secs(30), app.background_rx.recv())
                .await
                .expect("the checks answer")
            {
                Some(super::Background::AgentPreflight { new, result }) => {
                    app.agent_preflight_done(*new, result);
                    break;
                }
                Some(_) => {}
                None => panic!("the background channel closed"),
            }
        };

        // Locked: nothing can be signed, so the form does not open.
        app.command = "agent new Scout".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, None);
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|notice| notice.contains("unlocked")),
            "{:?}",
            app.status_error
        );
        app.runtime = Some(
            super::Runtime::build(
                &app.config,
                0,
                &paths,
                handle,
                Some(owner.clone()),
                crate::diagnostics::DiagnosticHandle::disabled(),
            )
            .unwrap(),
        );
        // As every tick does: with no local agent, nothing is started.
        app.reconcile_local_agents().await;
        assert!(app.local_agents.is_none());
        // No local agent yet: `:agents` is the directory, as before v0.15.
        app.command = "agents".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Agents));
        app.presentation.close_overlay();

        // ---- :agent new ----------------------------------------------------
        app.command = "agent new Scout".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::AgentNew));
        let shown = screen(&mut app);
        for (label, _) in crate::agents::manage::QUESTIONS {
            assert!(shown.contains(&format!("{label}:")), "`{label}`: {shown}");
        }
        assert!(shown.contains("Community: rikuy"), "{shown}");
        assert!(shown.contains("Name: Scout"), "{shown}");
        // Instructions, then who may wake it, with a word it does not know.
        app.agent_form_key(key(KeyCode::Tab)).await.unwrap();
        for character in "Be brief.".chars() {
            app.agent_form_key(key(KeyCode::Char(character)))
                .await
                .unwrap();
        }
        app.agent_form_key(key(KeyCode::Enter)).await.unwrap();
        for character in "everyone".chars() {
            app.agent_form_key(key(KeyCode::Char(character)))
                .await
                .unwrap();
        }
        app.agent_form_key(check).await.unwrap();
        let form = app.agent_form.as_ref().unwrap();
        assert_eq!(form.stage, FormStage::Editing);
        assert!(
            form.error
                .as_deref()
                .is_some_and(|error| error.contains("choose owner-only, allowlist or anyone")),
            "refused where it was written: {:?}",
            form.error
        );
        for _ in 0.."everyone".len() {
            app.agent_form_key(key(KeyCode::Backspace)).await.unwrap();
        }
        for character in "anyone".chars() {
            app.agent_form_key(key(KeyCode::Char(character)))
                .await
                .unwrap();
        }
        // The checks: Node, and the install of the host on first use.
        assert!(!crate::agents::runtime::install::is_installed(
            &paths.data_dir
        ));
        app.agent_form_key(check).await.unwrap();
        assert_eq!(app.agent_form.as_ref().unwrap().stage, FormStage::Checking);
        assert!(screen(&mut app).contains("Checking Node.js"));
        checked(&mut app).await;
        assert!(crate::agents::runtime::install::is_installed(
            &paths.data_dir
        ));
        let shown = screen(&mut app);
        for expected in [
            "Node.js 22.19.0 at",
            "model: Pi's own default",
            "Enter signs an owner authorization (NIP-OA) letting agent \"Scout\" act for ann",
            "in rikuy",
        ] {
            assert!(shown.contains(expected), "`{expected}`: {shown}");
        }
        // Until the owner confirms, nothing exists anywhere.
        assert!(app.config.hosted_agents.is_empty());
        assert!(recorded.calls.lock().unwrap().is_empty());
        assert!(
            std::fs::read_dir(paths.keys_dir())
                .map(|entries| entries.count())
                .unwrap_or(0)
                == 0
        );

        app.agent_form_key(key(KeyCode::Enter)).await.unwrap();
        let shown = screen(&mut app);
        assert!(
            matches!(app.agent_form.as_ref().unwrap().stage, FormStage::Done(_)),
            "{:?}",
            app.agent_form.as_ref().unwrap().error
        );
        for expected in [
            "agent Scout (",
            "created in rikuy",
            "capability record: pi-durable",
            "bzz agent add Scout <channel>",
        ] {
            assert!(shown.contains(expected), "`{expected}`: {shown}");
        }
        assert_eq!(
            *recorded.calls.lock().unwrap(),
            [("agent", 0), ("owner", 30_177)]
        );
        let saved = Config::load(&paths).unwrap();
        assert_eq!(saved.hosted_agents, app.config.hosted_agents);
        let scout = saved.hosted_agents[0].clone();
        let local = scout.local.clone().unwrap();
        assert_eq!(
            (scout.label.as_str(), local.harness, local.wake),
            ("Scout", Harness::PiDurable, WakeRule::Anyone)
        );
        assert_eq!(local.instructions, "Be brief.");
        assert!(crate::broker::custody::has_key(&paths.keys_dir(), &scout));

        // ---- :agents -------------------------------------------------------
        app.agent_form_key(key(KeyCode::Enter)).await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::LocalAgents));
        assert!(app.agent_form.is_none());
        // Its process is bzz's to start: the next tick takes the new agent in.
        assert!(app.local_agents.is_none());
        app.reconcile_local_agents().await;
        let phase = |app: &super::App| {
            app.local_agents
                .as_ref()
                .and_then(|agents| agents.host.status().first().map(|status| status.phase))
        };
        let wait_for = async |app: &super::App, expected: Phase| {
            for _ in 0..500 {
                if phase(app) == Some(expected) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the agent never became {expected:?}: {:?}", phase(app));
        };
        wait_for(&app, Phase::Idle).await;
        let shown = screen(&mut app);
        for expected in [
            "┌ local agents ─",
            " j/k select · space pause/resume · x stop its turn · i add to this channel · l log",
            "› Scout · pi-durable · idle",
            "state:        idle",
            "doing now:    nothing",
            "last outcome: no turn yet",
            "wakes for:   anyone in its channels",
            "capability record: pi-durable",
            "one final reply per mention: bzz",
        ] {
            assert!(shown.contains(expected), "`{expected}`: {shown}");
        }
        // How its last turn ended comes from bzz's own record of it.
        let request = crate::store::agent_runtime::NewRequest {
            agent_id: scout.id,
            request_id: "a".repeat(64),
            community_id: community,
            channel_id: Uuid::new_v4(),
            place: "place".into(),
            author: owner_keys.public_key().to_hex(),
            created_at: 1,
            event_json: "{}".into(),
        };
        // What the panel says of the newest request, read again.
        macro_rules! last_outcome {
            () => {{
                app.local_agent_turns.clear();
                app.refresh_local_agent_panel();
                loop {
                    if let Some(super::Background::LocalAgentTurns { turns, .. }) =
                        app.background_rx.recv().await
                    {
                        app.local_agent_turns = turns
                            .into_iter()
                            .filter_map(|(agent, turn)| turn.map(|turn| (agent, turn)))
                            .collect();
                        if !app.local_agent_turns.is_empty() {
                            break;
                        }
                    }
                }
                screen(&mut app)
            }};
        }
        let failed = request.clone();
        app.store
            .call(move |store| {
                store.agent_inbox_offer(&failed)?;
                store.agent_reply_claim(
                    failed.agent_id,
                    &failed.request_id,
                    &"b".repeat(64),
                    "{}",
                    crate::store::agent_runtime::ReplyOrigin::Failure,
                    Some("timeout"),
                )?;
                store.agent_reply_published(failed.agent_id, &failed.request_id)
            })
            .await
            .unwrap();
        let shown = last_outcome!();
        assert!(
            shown.contains("last outcome: failed: timeout · "),
            "{shown}"
        );
        // A reply that is signed and stored is not "answered" before the
        // relay has it: until then the state has a name of its own.
        let answered = crate::store::agent_runtime::NewRequest {
            request_id: "c".repeat(64),
            created_at: 2,
            ..request
        };
        let (agent_id, request_id) = (answered.agent_id, answered.request_id.clone());
        app.store
            .call(move |store| {
                store.agent_inbox_offer(&answered)?;
                // Newer than the first, whatever the clock's second.
                store.agent_reply_claim(
                    answered.agent_id,
                    &answered.request_id,
                    &"d".repeat(64),
                    "{}",
                    crate::store::agent_runtime::ReplyOrigin::Send,
                    None,
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let shown = last_outcome!();
        assert!(
            shown.contains(
                "last outcome: sending its reply: signed, and not yet accepted by the relay · "
            ) && !shown.contains("last outcome: answered"),
            "{shown}"
        );
        app.store
            .call(move |store| store.agent_reply_published(agent_id, &request_id))
            .await
            .unwrap();
        let shown = last_outcome!();
        assert!(shown.contains("last outcome: answered · "), "{shown}");

        // Space pauses the selected agent, and resumes it.
        app.local_agents_key(key(KeyCode::Char(' '))).await.unwrap();
        assert_eq!(app.status_error.as_deref(), Some("Scout paused"));
        assert_eq!(
            Config::load(&paths).unwrap().hosted_agents[0]
                .local
                .as_ref()
                .unwrap()
                .state,
            LocalAgentState::Paused
        );
        app.reconcile_local_agents().await;
        wait_for(&app, Phase::Paused).await;
        assert!(screen(&mut app).contains("› Scout · pi-durable · paused"));
        app.local_agents_key(key(KeyCode::Char(' '))).await.unwrap();
        assert_eq!(app.status_error.as_deref(), Some("Scout resumed"));
        app.reconcile_local_agents().await;
        wait_for(&app, Phase::Idle).await;
        // x stops the running turn; with none, it says so.
        app.local_agents_key(key(KeyCode::Char('x'))).await.unwrap();
        assert_eq!(
            app.status_error.as_deref(),
            Some("Scout is not answering anything now")
        );
        // l shows its log in place of its record, and l again its record.
        let log = crate::agents::runtime::process::agent_dir(&paths.data_dir, scout.id)
            .join("harness.log");
        std::fs::write(
            &log,
            "host started\nturn \u{1b}[31mfailed\u{1b}[0m: timeout\n",
        )
        .unwrap();
        app.local_agents_key(key(KeyCode::Char('l'))).await.unwrap();
        let shown = screen(&mut app);
        assert!(
            shown.contains("log (what its harness wrote") && shown.contains("host started"),
            "{shown}"
        );
        assert!(!shown.contains("capability record"), "{shown}");
        app.local_agents_key(key(KeyCode::Char('l'))).await.unwrap();
        assert!(screen(&mut app).contains("capability record: pi-durable"));
        // Tab is the directory of agents others run, and Tab there is back.
        app.local_agents_key(key(KeyCode::Tab)).await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Agents));
        app.agent_directory_key(key(KeyCode::Tab)).await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::LocalAgents));
        app.presentation.close_overlay();
        // With a local agent, `:agents` is the panel.
        app.command = "agents".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::LocalAgents));
        app.command = "agents directory".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Agents));
        app.presentation.close_overlay();

        // ---- a creation that fails leaves nothing --------------------------
        // n in the panel opens the form; a name that is taken is refused.
        app.command = "agents".into();
        app.execute_command().await.unwrap();
        app.local_agents_key(key(KeyCode::Char('n'))).await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::AgentNew));
        for character in "scout".chars() {
            app.agent_form_key(key(KeyCode::Char(character)))
                .await
                .unwrap();
        }
        app.agent_form_key(check).await.unwrap();
        assert!(
            app.agent_form
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .is_some_and(|error| error.contains("exists"))
        );
        for _ in 0.."scout".len() {
            app.agent_form_key(key(KeyCode::Backspace)).await.unwrap();
        }
        for character in "Ranger".chars() {
            app.agent_form_key(key(KeyCode::Char(character)))
                .await
                .unwrap();
        }
        app.agent_form_key(check).await.unwrap();
        checked(&mut app).await;
        recorded
            .refuse
            .store(true, std::sync::atomic::Ordering::Release);
        app.agent_form_key(key(KeyCode::Enter)).await.unwrap();
        let form = app.agent_form.as_ref().unwrap();
        assert_eq!(form.stage, FormStage::Editing, "back to the answers");
        let error = form.error.clone().unwrap_or_default();
        assert!(
            error.contains("the agent was not created")
                && error.contains("refused the agent's profile")
                && error.contains("Nothing was left"),
            "{error}"
        );
        assert!(screen(&mut app).contains("the agent was not created"));
        assert_eq!(app.config.hosted_agents.len(), 1, "only Scout");
        assert_eq!(Config::load(&paths).unwrap().hosted_agents.len(), 1);
        assert_eq!(
            std::fs::read_dir(paths.keys_dir())
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("agent-"))
                .count(),
            1,
            "no key of the agent that was not created"
        );
        // Esc leaves the form; nothing more was published than the refusal.
        app.agent_form_key(key(KeyCode::Esc)).await.unwrap();
        assert!(app.agent_form.is_none() && app.presentation.overlay.is_none());
        if let Some(agents) = app.local_agents.take() {
            agents.stop().await;
        }
        owner.lock().await;
    }

    /// A profile with an unlocked owner, one channel open, and local agents
    /// whose harness is a shell script that speaks just enough of the
    /// protocol: it answers `initialize`, `session/new` and the prompt, and
    /// puts `commands` to bzz one after the other, as bzz's own host does
    /// before it starts one. Each answer is written to `<out>/answer-<n>`;
    /// a yes also writes `<out>/ran-<n>`, which stands for the command.
    #[cfg(unix)]
    struct HandsWorld {
        temporary: TempDir,
        paths: Paths,
        owner_keys: Keys,
        owner: crate::auth::signer::SignerHandle,
        channel: Uuid,
        agents: Vec<crate::config::HostedAgentConfig>,
        app: super::App,
    }

    #[cfg(unix)]
    impl HandsWorld {
        /// `agents`: each a name and the commands its harness asks for, as
        /// JSON strings without their quotes.
        async fn new(agents: &[(&str, &[&str])]) -> Self {
            use crate::config::{ExecGrant, FolderGrant, Harness, LocalAgentConfig};
            let temporary = TempDir::new().unwrap();
            let paths = Paths {
                config_dir: temporary.path().join("config"),
                data_dir: temporary.path().join("data"),
                cache_dir: temporary.path().join("cache"),
            };
            paths.ensure().unwrap();
            let owner_keys = Keys::generate();
            let owner = crate::auth::signer::SignerHandle::spawn(owner_keys.clone());
            let mut config = Config::default();
            let identity = crate::config::IdentityConfig {
                id: Uuid::new_v4(),
                label: "ann".into(),
                pubkey: owner_keys.public_key().to_hex(),
                backend: crate::config::KeyBackend::EncryptedFile,
                key_ref: "owner".into(),
            };
            config.identities.push(identity.clone());
            // A loopback port nothing listens on: no relay is needed.
            let community = config
                .add_community("rikuy".into(), "ws://127.0.0.1:9".into(), identity.id, true)
                .unwrap();
            let work = temporary.path().join("work");
            std::fs::create_dir_all(&work).unwrap();
            let mut made = Vec::new();
            for (name, commands) in agents {
                let (mut agent, _) = crate::broker::custody::create(
                    &paths.keys_dir(),
                    &owner,
                    identity.id,
                    community,
                    name,
                )
                .await
                .unwrap();
                let out = temporary.path().join(format!("out-{name}"));
                std::fs::create_dir_all(&out).unwrap();
                let mut script = String::from(
                    "read line; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":1,\"agentCapabilities\":{},\"_meta\":{\"bzz\":{\"host\":\"sh\",\"tools\":\"pipe\",\"run\":[\"work\"],\"packages\":[]}}}}'\n\
                     read line; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"sessionId\":\"s1\"}}'\n\
                     read line\n",
                );
                for (index, command) in commands.iter().enumerate() {
                    let number = index + 1;
                    script.push_str(&format!(
                        "printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":\"h{number}\",\"method\":\"_bzz/approve\",\"params\":{{\"sessionId\":\"s1\",\"kind\":\"exec\",\"mount\":\"work\",\"command\":\"{command}\"}}}}'\n\
                         read answer; printf '%s\\n' \"$answer\" > '{out}/answer-{number}'\n\
                         case \"$answer\" in *'\"allow\":true'*) echo ran > '{out}/ran-{number}';; esac\n",
                        out = out.display()
                    ));
                }
                script.push_str("cat > /dev/null\n");
                let program = temporary.path().join(format!("harness-{name}.sh"));
                std::fs::write(&program, script).unwrap();
                let mut local = LocalAgentConfig::new(Harness::PiDurable);
                local.command = vec!["/bin/sh".into(), program.display().to_string()];
                local.folders = vec![FolderGrant {
                    name: "work".into(),
                    path: work.clone(),
                    write: false,
                    exec: ExecGrant::Ask,
                }];
                agent.local = Some(local);
                config.hosted_agents.push(agent.clone());
                made.push(agent);
            }
            config.save(&paths).unwrap();
            let mut store = Store::open(paths.database_file()).unwrap();
            store.sync_config(&config).unwrap();
            store.pin_relay_pubkey(community, &"d".repeat(64)).unwrap();
            let handle = StoreHandle::spawn(store).unwrap();
            let mut app = super::App::new(Config::default(), paths.clone(), handle.clone())
                .await
                .unwrap();
            app.config = config;
            app.runtime = Some(
                super::Runtime::build(
                    &app.config,
                    0,
                    &paths,
                    handle,
                    Some(owner.clone()),
                    crate::diagnostics::DiagnosticHandle::disabled(),
                )
                .unwrap(),
            );
            let channel = Uuid::new_v4();
            app.channels = vec![Channel {
                id: channel,
                name: "general".into(),
                about: String::new(),
                kind: ChannelKind::Stream,
                visibility: Visibility::Public,
                is_member: true,
                is_hidden: false,
                member_count: 3,
                last_event_at: None,
                facts: Default::default(),
            }];
            app.selected_channel = 0;
            Self {
                temporary,
                paths,
                owner_keys,
                owner,
                channel,
                agents: made,
                app,
            }
        }

        fn out(&self, name: &str, file: &str) -> std::path::PathBuf {
            self.temporary.path().join(format!("out-{name}")).join(file)
        }

        /// Starts the agents, as a tick does, and waits until each can be
        /// handed a request.
        async fn start_agents(&mut self) {
            self.app.reconcile_local_agents().await;
            assert_eq!(self.app.status_error, None);
            let wanted = self.agents.len();
            Self::until("the agents to be ready", || {
                self.app.local_agents.as_ref().is_some_and(|agents| {
                    agents.host.status().len() == wanted
                        && self.agents.iter().all(|agent| agents.host.runs(agent.id))
                })
            })
            .await;
        }

        /// The owner mentions `agent` at the top of the channel.
        async fn mention(&self, agent: usize) {
            use crate::agents::runtime::{
                admission::{ChannelFacts, PlaceKind},
                host::Offer,
            };
            let agent = &self.agents[agent];
            let event = buzz_sdk::build_message(
                self.channel,
                "please build",
                None,
                &[agent.pubkey.as_str()],
                false,
                &[],
                &[],
            )
            .unwrap()
            .sign_with_keys(&self.owner_keys)
            .unwrap();
            let facts = ChannelFacts {
                id: self.channel,
                name: "general".into(),
                read_only: Vec::new(),
                kind: PlaceKind::Channel,
                members: vec![self.owner_keys.public_key().to_hex(), agent.pubkey.clone()],
            };
            let host = &self.app.local_agents.as_ref().unwrap().host;
            assert_eq!(host.offer(agent.id, &event, &facts).await, Offer::Queued);
        }

        async fn until(what: &str, mut check: impl FnMut() -> bool) {
            for _ in 0..1500 {
                if check() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("timed out waiting for {what}");
        }

        /// Waits until `count` commands wait for the owner, and reads them
        /// as a tick does.
        async fn waiting(&mut self, count: usize) {
            let host = self.app.local_agents.as_ref().unwrap().host.clone();
            Self::until(&format!("{count} command(s) to wait"), || {
                host.approvals().len() == count
            })
            .await;
            self.app.refresh_approvals();
            assert_eq!(self.app.approvals.len(), count);
        }

        fn screen(&mut self) -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(150, 40)).unwrap();
            terminal.draw(|frame| self.app.render(frame)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..buffer.area.height)
                .map(|row| {
                    (0..buffer.area.width)
                        .map(|column| buffer[(column, row)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect()
        }

        async fn stop(mut self) {
            if let Some(agents) = self.app.local_agents.take() {
                agents.stop().await;
            }
            self.owner.lock().await;
        }
    }

    /// v0.15 M7, in the TUI: a real command an agent wants to run is said
    /// on every screen, whatever is open; the owner opens it when he
    /// chooses and sees which agent asks, in which conversation, for which
    /// folder, and the whole command as it will run; his yes starts it and
    /// his no, with his reason, does not. Two agents each have one open.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_waits_for_the_owner_on_every_screen_and_runs_only_on_his_yes() {
        // Scout's command has two lines, a tab and a terminal escape.
        // Ranger's first is longer than the screen; its second is asked
        // after the first was answered.
        let long = (1..=60)
            .map(|line| format!("echo step {line}"))
            .collect::<Vec<_>>()
            .join("\\n");
        let mut world = HandsWorld::new(&[
            (
                "Scout",
                &["cd app &&\\n  npm test -- --watch=false\\t# \\u001b[2K"],
            ),
            ("Ranger", &[long.as_str(), "make deploy"]),
        ])
        .await;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let ctrl_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        world.start_agents().await;
        // Nothing waits: the key is not taken, and no line says otherwise.
        assert!(!world.app.global_key(ctrl_y));
        assert!(!world.screen()[0].contains("asks to run a command"));
        world.app.command = "approvals".into();
        world.app.execute_command().await.unwrap();
        assert_eq!(
            world.app.status_error.as_deref(),
            Some("no command waits for your answer")
        );

        world.mention(0).await;
        world.waiting(1).await;
        world.mention(1).await;
        world.waiting(2).await;

        // ---- seen from anywhere -------------------------------------------
        let banner = "Scout asks to run a command in /mnt/work (#general (a thread)) · and 1 more · Ctrl-Y to see and answer it";
        let top = world.screen()[0].clone();
        assert!(top.contains(banner), "the workspace: {top}");
        assert!(top.contains("refused in "), "{top}");
        // The banner has its own row: the panes' titles are whole under it
        // (L5).
        let titles = world.screen()[1].clone();
        assert!(titles.contains("┌ channels ─"), "{titles}");
        assert!(titles.contains("┌ general ─"), "{titles}");
        // Not only in the agents panel: with the directory of other
        // people's agents open, with the help, a search, the Inbox, and
        // while a message is being written.
        for command in ["agents directory", "search", "broker", "agents"] {
            world.app.command = command.into();
            world.app.execute_command().await.unwrap();
            assert!(
                world.app.presentation.overlay.is_some(),
                "`:{command}` opened something"
            );
            let top = world.screen()[0].clone();
            assert!(top.contains(banner), "`:{command}`: {top}");
            if command != "agents" {
                world.app.presentation.close_overlay();
            }
        }
        // In the agents panel the agent's own line says what it waits for.
        let shown = world.screen().join("\n");
        assert!(
            shown.contains("waiting for your answer: run in /mnt/work: cd app &&"),
            "{shown}"
        );
        // Under the help, which fills the screen; and in the Inbox.
        world.app.presentation.open_overlay(Overlay::Help);
        assert!(world.screen()[0].contains(banner), "the help");
        assert!(
            world.screen()[1].contains("effective keymap"),
            "the help starts under the banner"
        );
        world.app.presentation.close_overlay();
        world.app.open_inbox();
        assert!(world.screen()[0].contains(banner), "the Inbox");
        world.app.presentation.route = Route::Workspace;
        world.app.presentation.focus = FocusSurface::Timeline;
        // The dialog never opens by itself, and a plain `y` or `a` typed
        // anywhere answers nothing.
        assert_eq!(world.app.presentation.overlay, None);
        assert!(!world.app.global_key(key(KeyCode::Char('y'))));
        assert!(!world.app.global_key(key(KeyCode::Char('a'))));
        assert_eq!(world.app.approvals.len(), 2);

        // Opened over a confirmation (Esc had asked whether to quit), the
        // dialog does not go back to it afterwards: a confirmation that
        // lost its question would be a prompt with nothing in it, which
        // takes the next key.
        world
            .app
            .presentation
            .open_confirmation(crate::ui::state::ConfirmationKind::Quit);
        assert!(world.app.global_key(ctrl_y));
        assert_eq!(world.app.presentation.overlay, Some(Overlay::Approval));
        world.app.approval_key(key(KeyCode::Esc));
        assert!(
            world.app.presentation.overlay.is_none()
                && world.app.presentation.confirmation.is_none(),
            "deciding later leaves no confirmation behind"
        );
        assert_eq!(world.app.approvals.len(), 2, "and answers nothing");

        // ---- the command, whole --------------------------------------------
        // Opened from inside another panel, which is there again afterwards.
        world.app.command = "agents".into();
        world.app.execute_command().await.unwrap();
        assert!(world.app.global_key(ctrl_y));
        assert_eq!(world.app.presentation.overlay, Some(Overlay::Approval));
        let shown = world.screen().join("\n");
        for expected in [
            "┌ a real command waits for your answer ─",
            " a allow · r refuse ",
            "agent:        Scout",
            "conversation: #general (a thread) · it is answering you",
            &format!(
                "folder:       /mnt/work = {}",
                world.temporary.path().join("work").display()
            ),
            "It runs as your user, starting in that folder, and can reach whatever you can.",
            "then it counts as refused · 1 more wait (Tab)",
            "the command, exactly as /bin/sh will get it (2 line(s), 44 bytes):",
            "It holds characters that would not show; each is written out",
            " 1 │ cd app &&",
            " 2 │   npm test -- --watch=false\\t# \\x1b[2K",
            "that is the whole command",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(!shown.contains('\u{1b}'), "the escape itself is not drawn");
        assert!(
            !world.screen()[0].contains("asks to run a command"),
            "the dialog is the notice while it is open"
        );
        // Nothing has started while it was only looked at.
        assert!(!world.out("Scout", "answer-1").exists());

        // ---- yes ------------------------------------------------------------
        world.app.approval_key(key(KeyCode::Char('a')));
        assert_eq!(
            world.app.status_error.as_deref(),
            Some("allowed: Scout runs its command in /mnt/work")
        );
        assert_eq!(
            world.app.presentation.overlay,
            Some(Overlay::LocalAgents),
            "back where the owner was"
        );
        let ran = world.out("Scout", "ran-1");
        HandsWorld::until("Scout's command to start", || ran.exists()).await;
        let answer = std::fs::read_to_string(world.out("Scout", "answer-1")).unwrap();
        assert!(answer.contains("\"id\":\"h1\""), "{answer}");
        assert!(answer.contains("\"result\":{\"allow\":true}"), "{answer}");
        world.app.presentation.close_overlay();
        world.waiting(1).await;
        let top = world.screen()[0].clone();
        assert!(
            top.contains(
                "Ranger asks to run a command in /mnt/work (#general (a thread)) · Ctrl-Y"
            ),
            "{top}"
        );

        // ---- a command longer than the screen ------------------------------
        world.app.command = "approvals".into();
        world.app.execute_command().await.unwrap();
        assert_eq!(world.app.presentation.overlay, Some(Overlay::Approval));
        let shown = world.screen().join("\n");
        assert!(shown.contains("agent:        Ranger"), "{shown}");
        assert!(shown.contains(" 1 │ echo step 1"), "{shown}");
        assert!(!shown.contains("echo step 60"), "{shown}");
        assert!(
            shown.contains("of 60 · scroll to the end before you allow"),
            "{shown}"
        );
        // A yes for a command whose end was never on screen is not taken.
        world.app.approval_key(key(KeyCode::Char('a')));
        assert_eq!(world.app.presentation.overlay, Some(Overlay::Approval));
        assert!(
            world
                .screen()
                .join("\n")
                .contains("scroll to the end of the command first"),
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !world.out("Ranger", "answer-1").exists(),
            "nothing was answered"
        );
        world.app.approval_key(key(KeyCode::PageDown));
        world.screen();
        world.app.approval_key(key(KeyCode::Char('G')));
        let shown = world.screen().join("\n");
        assert!(shown.contains("60 │ echo step 60"), "{shown}");
        assert!(shown.contains("of 60: the end"), "{shown}");

        // ---- no, with a reason ----------------------------------------------
        world.app.approval_key(key(KeyCode::Char('r')));
        for character in "not on a Friday".chars() {
            world.app.approval_key(key(KeyCode::Char(character)));
        }
        // While the reason is written, `a` is a letter of it.
        assert!(
            world
                .screen()
                .join("\n")
                .contains("refuse, and tell the agent why (optional): not on a Friday▏"),
        );
        assert!(!world.out("Ranger", "answer-1").exists());
        world.app.approval_key(key(KeyCode::Enter));
        assert_eq!(
            world.app.status_error.as_deref(),
            Some("refused: Ranger was told no, and nothing was started")
        );
        assert_eq!(world.app.presentation.overlay, None);
        let refused = world.out("Ranger", "answer-1");
        HandsWorld::until("Ranger to be told no", || refused.exists()).await;
        let answer = std::fs::read_to_string(&refused).unwrap();
        assert!(answer.contains("\"allow\":false"), "{answer}");
        assert!(
            answer.contains("Your owner refused this command, and says: not on a Friday"),
            "{answer}"
        );
        assert!(
            !world.out("Ranger", "ran-1").exists(),
            "a refused command does not start"
        );

        // ---- a question that goes away while it is looked at ----------------
        world.waiting(1).await;
        assert!(world.app.global_key(ctrl_y));
        assert!(world.screen().join("\n").contains(" 1 │ make deploy"));
        let ranger = world.agents[1].id;
        let host = world.app.local_agents.as_ref().unwrap().host.clone();
        assert_eq!(host.stop_turns(ranger).await, 1);
        world.app.refresh_approvals();
        assert!(world.app.approvals.is_empty());
        assert!(
            world
                .screen()
                .join("\n")
                .contains("That command no longer waits")
        );
        // A yes now starts nothing, and says so.
        world.app.approval_key(key(KeyCode::Char('a')));
        assert!(
            world
                .app
                .status_error
                .as_deref()
                .is_some_and(
                    |notice| notice.contains("no longer waits") && notice.contains("not started")
                ),
            "{:?}",
            world.app.status_error
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!world.out("Ranger", "ran-2").exists());
        assert!(!world.screen()[0].contains("asks to run a command"));

        // Each agent's activity has its questions and their answers.
        let lines = |agent: Uuid| {
            crate::agents::activity::recent(&world.paths.data_dir, agent, 10)
                .iter()
                .map(|entry| entry.answer.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            lines(world.agents[0].id),
            ["asked of you", "allowed by you"]
        );
        assert_eq!(
            lines(ranger),
            [
                "asked of you",
                "refused by you: not on a Friday",
                "asked of you",
                "void: its owner stopped the turn",
            ]
        );
        world.stop().await;
    }

    /// v0.15 M7: locking, which stops the agents, leaves no question
    /// behind; and the panel's key adds the selected agent to the channel
    /// that is open.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_question_is_void_when_bzz_locks_and_the_panel_adds_an_agent_to_the_open_channel() {
        let mut world = HandsWorld::new(&[("Scout", &["make deploy"])]).await;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        world.start_agents().await;
        world.mention(0).await;
        world.waiting(1).await;
        let scout = world.agents[0].id;

        // The panel's key: the agent selected, into the channel open.
        world.app.command = "agents".into();
        world.app.execute_command().await.unwrap();
        assert!(world.screen().join("\n").contains("i add to this channel"));
        world
            .app
            .local_agents_key(key(KeyCode::Char('i')))
            .await
            .unwrap();
        assert_eq!(world.app.agent_additions, [(scout, world.channel)]);
        assert_eq!(
            world.app.status_error.as_deref(),
            Some("adding Scout to #general…")
        );
        // No relay answers here, and the owner is told that it failed.
        let notice = loop {
            match tokio::time::timeout(Duration::from_secs(30), world.app.background_rx.recv())
                .await
                .expect("the addition answers")
            {
                Some(super::Background::Notice(notice)) => break notice,
                Some(_) => {}
                None => panic!("the background channel closed"),
            }
        };
        assert!(
            notice.starts_with("Scout was not added to #general: "),
            "{notice}"
        );
        // With no channel open there is nothing to add it to.
        let channels = std::mem::take(&mut world.app.channels);
        world
            .app
            .local_agents_key(key(KeyCode::Char('i')))
            .await
            .unwrap();
        assert_eq!(
            world.app.status_error.as_deref(),
            Some("open a channel first: the agent is added to it")
        );
        assert_eq!(world.app.agent_additions.len(), 1);
        world.app.channels = channels;

        // Locked: the agents stop, and the question with them.
        let host = world.app.local_agents.as_ref().unwrap().host.clone();
        let asked = host.approvals()[0].id;
        world.app.presentation.close_overlay();
        world.app.command = "lock".into();
        world.app.execute_command().await.unwrap();
        assert!(world.app.local_agents.is_none());
        assert!(host.approvals().is_empty());
        assert!(!host.answer_approval(asked, crate::agents::runtime::approval::Answer::Allow));
        world.app.refresh_approvals();
        assert!(!world.screen()[0].contains("asks to run a command"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!world.out("Scout", "ran-1").exists());
        let kept = crate::agents::activity::recent(&world.paths.data_dir, scout, 10);
        assert!(
            kept.last()
                .is_some_and(|entry| entry.answer.starts_with("void: ")),
            "{kept:?}"
        );
        world.stop().await;
    }

    /// One writer for the settings: a command that edits the settings
    /// while a TUI is open is not written over by the TUI's next save, and
    /// the TUI's change is not written over by a command that read the
    /// settings before it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_commands_edit_survives_a_save_by_the_running_tui_and_the_reverse() {
        use crate::agents::manage;
        use crate::config::LocalAgentState;
        let mut world = HandsWorld::new(&[("Scout", &[]), ("Ranger", &[])]).await;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let (scout, ranger) = (world.agents[0].id, world.agents[1].id);
        let local = |config: &Config, id: Uuid| {
            config
                .hosted_agents
                .iter()
                .find(|agent| agent.id == id)
                .and_then(|agent| agent.local.clone())
                .unwrap()
        };
        // `bzz agent edit Scout --allow-host github.com --packages off`,
        // from another terminal, as the command does it: read, change, save.
        let mut command = Config::load(&world.paths).unwrap();
        manage::apply_edit(
            &mut command,
            scout,
            &manage::Edit {
                network: Some(vec!["github.com".into()]),
                packages: Some(false),
                ..manage::Edit::default()
            },
        )
        .unwrap();
        command.save(&world.paths).unwrap();
        // The TUI, open all along, knows nothing of it; its owner pauses
        // Ranger, and then Scout itself, in the agents panel.
        assert!(local(&world.app.config, scout).network.is_empty());
        world.app.command = "agents".into();
        world.app.execute_command().await.unwrap();
        world
            .app
            .local_agents_key(key(KeyCode::Char('j')))
            .await
            .unwrap();
        world
            .app
            .local_agents_key(key(KeyCode::Char(' ')))
            .await
            .unwrap();
        assert_eq!(world.app.status_error.as_deref(), Some("Ranger paused"));
        world
            .app
            .local_agents_key(key(KeyCode::Char('k')))
            .await
            .unwrap();
        world
            .app
            .local_agents_key(key(KeyCode::Char(' ')))
            .await
            .unwrap();
        assert_eq!(world.app.status_error.as_deref(), Some("Scout paused"));
        let saved = Config::load(&world.paths).unwrap();
        let scouts = local(&saved, scout);
        assert_eq!(
            (scouts.network.clone(), scouts.packages, scouts.state),
            (
                vec!["github.com".to_owned()],
                false,
                LocalAgentState::Paused
            ),
            "the command's edit survived the TUI's saves, with the TUI's own change of that agent"
        );
        assert_eq!(local(&saved, ranger).state, LocalAgentState::Paused);

        // The reverse: a command reads the settings, the TUI saves a change
        // (the owner resumes Ranger), and then the command saves its own.
        let mut command = Config::load(&world.paths).unwrap();
        world
            .app
            .local_agents_key(key(KeyCode::Char('j')))
            .await
            .unwrap();
        world
            .app
            .local_agents_key(key(KeyCode::Char(' ')))
            .await
            .unwrap();
        assert_eq!(world.app.status_error.as_deref(), Some("Ranger resumed"));
        manage::apply_edit(
            &mut command,
            scout,
            &manage::Edit {
                instructions: Some("Be brief.".into()),
                ..manage::Edit::default()
            },
        )
        .unwrap();
        command.save(&world.paths).unwrap();
        let saved = Config::load(&world.paths).unwrap();
        assert_eq!(
            local(&saved, ranger).state,
            LocalAgentState::Active,
            "the TUI's change survived the command's save"
        );
        let scouts = local(&saved, scout);
        assert_eq!(
            (
                scouts.instructions.as_str(),
                scouts.network.len(),
                scouts.state
            ),
            ("Be brief.", 1, LocalAgentState::Paused)
        );
        world.stop().await;
    }

    /// v0.15 M1: the `:broker` panel has one status line per local agent,
    /// and a local agent is told apart from one the user runs himself.
    #[test]
    fn broker_panel_shows_a_status_line_per_local_agent() {
        use crate::agents::runtime::agent::{AgentStatus, Phase};
        use crate::config::{Harness, HostedAgentConfig, LocalAgentConfig};
        let agent = |label: &str, local| HostedAgentConfig {
            id: Uuid::new_v4(),
            label: label.into(),
            pubkey: "b".repeat(64),
            community: Uuid::new_v4(),
            owner_identity: Uuid::new_v4(),
            auth_tag: "[]".into(),
            credential_hash: "c".repeat(64),
            allowed: Vec::new(),
            paused: false,
            local,
        };
        let mut moved = LocalAgentConfig::new(Harness::PiDurable);
        moved.state = crate::config::LocalAgentState::MovedAway;
        let agents = [
            agent("helper", None),
            agent("Scout", Some(LocalAgentConfig::new(Harness::PiDurable))),
            agent("Rover", Some(moved)),
        ];
        let status = |phase, turns, activity: Option<&str>, problem: Option<&str>| AgentStatus {
            id: agents[1].id,
            name: "Scout".into(),
            harness: Harness::PiDurable,
            phase,
            turns,
            pid: Some(4242),
            activity: activity.map(str::to_owned),
            problem: problem.map(str::to_owned),
        };
        let lines = [
            status(Phase::Running, 1, Some("mcp__buzz__send"), None).line(),
            status(
                Phase::Unavailable,
                0,
                None,
                Some("its harness stopped unexpectedly"),
            )
            .line(),
        ];
        assert_eq!(
            lines[0],
            "Scout · pi-durable · answering 1 (mcp__buzz__send)"
        );
        assert_eq!(
            lines[1],
            "Scout · pi-durable · unavailable · its harness stopped unexpectedly"
        );
        assert_eq!(
            status(Phase::Idle, 0, None, None).line(),
            "Scout · pi-durable · idle"
        );
        let mut terminal = Terminal::new(TestBackend::new(110, 20)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::broker::render(
                    frame,
                    frame.area(),
                    &agents,
                    &lines,
                    &[],
                    crate::ui::broker::BrokerStatus::default(),
                    &crate::ui::theme::Theme::default(),
                )
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(screen.contains("helper · active  "), "{screen}");
        assert!(
            screen.contains("Scout · active · local: bzz runs it"),
            "{screen}"
        );
        // An agent that was moved away is not called active, nor run by bzz.
        assert!(
            screen.contains("Rover · moved away · local: it runs elsewhere"),
            "{screen}"
        );
        assert!(screen.contains("local agents"), "{screen}");
        for line in &lines {
            assert!(screen.contains(line.as_str()), "{screen}");
        }
        assert!(screen.contains("recent activity (no content)"), "{screen}");
    }

    /// L4: behind an overlay every cell steps back (the DIM attribute, so
    /// it holds without colour); the overlay's own cells do not.
    #[tokio::test]
    async fn behind_an_overlay_every_cell_is_dim() {
        use ratatui::style::Modifier;

        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|cell| !cell.modifier.contains(Modifier::DIM)),
            "nothing is dim without an overlay"
        );
        app.presentation.open_overlay(Overlay::Finder);
        terminal.draw(|frame| app.render(frame)).unwrap();
        let popup = super::centered(Rect::new(0, 0, 100, 30), 70, 12);
        let buffer = terminal.backend().buffer();
        for y in 0..30 {
            for x in 0..100 {
                let inside = popup.contains(ratatui::layout::Position { x, y });
                assert_eq!(
                    buffer[(x, y)].modifier.contains(Modifier::DIM),
                    !inside,
                    "({x}, {y}) inside the finder: {inside}"
                );
            }
        }
        // Its keys are in the footer, the key in the hint colour.
        let footer = (popup.x..popup.right())
            .map(|x| buffer[(x, popup.bottom() - 1)].symbol())
            .collect::<String>();
        assert!(footer.contains("Enter open · Esc close"), "{footer}");
    }

    /// The deterministic twin of the `v013-panels-empty-profile` Herdr scenario.
    #[tokio::test]
    async fn v013_commands_on_an_empty_profile_explain_or_open_their_panels() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.command = "broker".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Broker));
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::broker::render(
                    frame,
                    frame.area(),
                    &[],
                    &[],
                    &[],
                    crate::ui::broker::BrokerStatus::default(),
                    &app.theme,
                )
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(screen.contains("┌ agent broker ─"), "{screen}");
        assert!(screen.contains(" not serving ┐"), "{screen}");
        assert!(!screen.contains("local agents"), "{screen}");
        assert!(
            screen.contains("o owner-signed agent presence: off"),
            "{screen}"
        );
        app.presentation.close_overlay();
        for (command, expected) in [
            ("huddle", "select a channel in a connected community"),
            ("tasks", "connect to a community to see artifacts"),
            ("artifacts", "select a channel to see its artifacts"),
            ("presence", "presence: off · nothing is published"),
            ("presence maybe", "usage: :presence [on|off]"),
            (
                "presence on",
                "presence: on · online while active, away after inactivity, offline on exit",
            ),
            ("presence off", "presence: off · nothing is published"),
        ] {
            app.status_error = None;
            app.command = command.into();
            app.execute_command().await.unwrap();
            assert_eq!(app.status_error.as_deref(), Some(expected), "{command}");
        }
        // The setting is persisted, and off again after the last command.
        let saved = crate::config::Config::load(&app.paths).unwrap();
        assert!(!saved.ui.publish_presence);
        app.command = "presence on".into();
        app.execute_command().await.unwrap();
        assert!(
            crate::config::Config::load(&app.paths)
                .unwrap()
                .ui
                .publish_presence
        );
    }

    /// A profile with no community has no workspace and no status bar; the
    /// welcome screen must still show command feedback. The release Herdr
    /// run of `v013-panels-empty-profile` caught it missing.
    #[tokio::test]
    async fn empty_profile_welcome_screen_shows_command_feedback() {
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
        assert!(app.config.communities.is_empty());
        for (command, expected) in [
            ("huddle", "select a channel in a connected community"),
            ("tasks", "connect to a community to see artifacts"),
            ("presence", "presence: off · nothing is published"),
        ] {
            app.status_error = None;
            app.command = command.into();
            app.execute_command().await.unwrap();
            let screen = screen_text(&mut app);
            assert!(screen.contains("Welcome to bzz"), "{screen}");
            assert!(screen.contains(expected), "{command}: {screen}");
        }
    }

    #[tokio::test]
    async fn huddle_panel_shows_roster_reactions_and_only_creator_guidelines() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let channel = Uuid::new_v4();
        let backing = Uuid::new_v4();
        app.channels = vec![Channel {
            id: channel,
            name: "talk".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        let creator = Keys::generate();
        let guest = Keys::generate();
        app.huddle_view = Some((
            channel,
            Some(crate::protocol::huddle::ActiveHuddle {
                backing,
                creator: creator.public_key().to_hex(),
                started_at: nostr::Timestamp::now().as_secs(),
                participants: vec![creator.public_key().to_hex(), guest.public_key().to_hex()],
            }),
        ));
        let reaction = crate::protocol::huddle::reaction(backing, "🎉", "guest")
            .unwrap()
            .sign_with_keys(&guest)
            .unwrap();
        assert!(app.apply_huddle_event(&reaction));
        let guidelines = |keys: &Keys, text: &str| {
            EventBuilder::new(Kind::Custom(crate::protocol::huddle::KIND_GUIDELINES), text)
                .tags([Tag::parse(["h", backing.to_string().as_str()]).unwrap()])
                .sign_with_keys(keys)
                .unwrap()
        };
        assert!(!app.apply_huddle_event(&guidelines(&guest, "forged")));
        assert!(
            app.huddle_guidelines.is_none(),
            "only the creator sets guidelines"
        );
        assert!(app.apply_huddle_event(&guidelines(&creator, "Keep it short")));
        // A live lifecycle event for this channel re-replays its state.
        let ended = crate::protocol::huddle::ended(channel, backing)
            .unwrap()
            .sign_with_keys(&creator)
            .unwrap();
        app.apply_huddle_event(&ended);
        assert!(app.huddle_dirty);
        // Without a connected runtime, commands explain instead of acting.
        app.huddle_command("join").await.unwrap();
        assert!(app.status_error.is_some());

        let view = app.huddle_panel_view();
        assert_eq!(view.roster.as_ref().map(Vec::len), Some(2));
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::huddle::render(frame, frame.area(), &view, &app.theme))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(screen.contains("2 in it"), "{screen}");
        assert!(screen.contains("Keep it short"), "{screen}");
        // Names come from profiles, never the sender's own `sender_name`.
        assert!(screen.contains("🎉"), "{screen}");
        assert!(!screen.contains("guest"), "{screen}");
        assert!(!screen.contains("forged"), "{screen}");

        // M8.1: the channel row and the names of people in the huddle carry
        // the active-huddle marker.
        app.presentation.close_overlay();
        app.presentation.close_overlay();
        let mut profile = crate::domain::Profile::verified_only(
            &guest.public_key().to_hex(),
            crate::protocol::verified_profile::VerifiedName {
                display_name: "Guest Person".into(),
                username: "guest".into(),
                full_name: None,
                issuer_label: "IdP".into(),
                verified_at: 1,
            },
            "e".repeat(64),
            1,
        );
        profile.in_huddle = app
            .active_huddle()
            .is_some_and(|active| active.participants.contains(&profile.pubkey));
        assert!(profile.in_huddle);
        app.profiles.insert(profile.pubkey.clone(), profile);
        app.messages = vec![Message {
            event_id: "f".repeat(64),
            channel_id: channel,
            pubkey: guest.public_key().to_hex(),
            created_at: 1,
            content: "in the call".into(),
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
        let screen = screen_text(&mut app);
        assert!(screen.contains("#talk 🎧"), "{screen}");
        // A wide glyph takes two cells in the test backend.
        assert!(
            screen.contains("🎧  Guest Person") || screen.contains("🎧 Guest Person"),
            "{screen}"
        );
    }

    /// Rule 20: with no huddle call the visualizer asks for no frame, and
    /// the redraw gate admits only the screen's first frame.
    #[tokio::test]
    async fn the_huddle_visualizer_asks_for_no_frame_without_a_call() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let mut gate = crate::ui::redraw_gate::RedrawGate::default();
        assert!(gate.take());
        for _ in 0..300 {
            gate.request_if(app.tick_huddle_viz());
            assert!(!gate.take());
        }
        assert!(app.huddle_lanes().is_none());
        assert!(!app.huddle_strip_shown());
    }

    /// The whole screen with a generated huddle: the strip in the status
    /// line while the panel is closed, the lanes in `:huddle`. Captures go to
    /// `BZZ_VIZ_CAPTURES` when set.
    #[tokio::test]
    async fn a_generated_huddle_shows_its_strip_and_its_lanes() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.channels = vec![Channel {
            id: Uuid::new_v4(),
            name: "team".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 4,
            last_event_at: None,
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        let mut people = Vec::new();
        for (index, (name, key)) in [("Ada", 'a'), ("Lin", 'b'), ("Sam", 'c')]
            .into_iter()
            .enumerate()
        {
            let pubkey = key.to_string().repeat(63) + "1";
            app.profiles.insert(
                pubkey.clone(),
                crate::domain::Profile {
                    pubkey: pubkey.clone(),
                    display_name: Some(name.into()),
                    name: None,
                    picture: None,
                    nip05: None,
                    about: None,
                    event_id: "e".repeat(64),
                    created_at: 1,
                    verified: None,
                    status: None,
                    presence: None,
                    in_huddle: true,
                },
            );
            people.push((index as u8 + 1, pubkey));
        }
        app.set_generated_huddle(
            crate::huddle::synth::Synthetic::new(&crate::huddle::synth::DEMO_SCRIPT, None),
            people,
            true,
        );
        app.huddle_viz.look = crate::ui::huddle_lanes::Look::default();
        let mut frames = 0;
        let mut t = 0.0;
        while t <= 5.5 {
            frames += u32::from(app.tick_huddle_viz_at(t));
            t += 0.033;
        }
        assert!(frames > 100, "the strip moves while people speak: {frames}");
        let draw = |app: &mut super::App, width: u16, height: u16| {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..height)
                .map(|y| {
                    (0..width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut captures = Vec::new();
        for (width, height) in [(80, 24), (160, 45)] {
            let screen = draw(&mut app, width, height);
            let status = screen.lines().last().unwrap();
            assert!(status.contains("🎧"), "{status}");
            assert!(status.contains("Ada and Lin"), "{status}");
            captures.push((format!("strip-{width}x{height}"), screen));
        }
        app.presentation.open_overlay(Overlay::Huddle);
        assert!(!app.huddle_strip_shown());
        for (width, height) in [(80, 24), (160, 45)] {
            let screen = draw(&mut app, width, height);
            assert!(screen.contains("Ada and Lin at once"), "{screen}");
            assert!(screen.contains("You · your microphone"), "{screen}");
            assert!(!screen.lines().last().unwrap().contains("🎧"));
            captures.push((format!("panel-{width}x{height}"), screen));
        }
        // Silence after the passage: the frames stop.
        let mut late = 0;
        let mut t = 12.0;
        while t <= 20.0 {
            late += u32::from(app.tick_huddle_viz_at(t));
            t += 0.033;
        }
        assert!(late <= 1, "no frames after 2 s of silence: {late}");
        if let Some(directory) = std::env::var_os("BZZ_VIZ_CAPTURES") {
            for (name, screen) in captures {
                std::fs::write(
                    std::path::Path::new(&directory).join(format!("{name}.txt")),
                    screen,
                )
                .unwrap();
            }
        }
    }

    /// `bzz demo`'s huddle: the generated voices play only in the channel
    /// whose huddle runs, and the panel says it is a recording, with no key
    /// to join or unmute and nothing that contradicts the lanes.
    #[tokio::test]
    async fn the_demo_huddle_is_a_recording_only_where_it_runs() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.demo = true;
        let channel = |name: &str| Channel {
            id: Uuid::new_v4(),
            name: name.into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 4,
            last_event_at: None,
            facts: Default::default(),
        };
        app.channels = vec![channel("team"), channel("infra")];
        let infra = app.channels[1].id;
        let (voices, people) = crate::demo::huddle();
        app.huddle_view = Some((
            infra,
            Some(crate::protocol::huddle::ActiveHuddle {
                backing: Uuid::new_v4(),
                creator: people[2].1.clone(),
                started_at: nostr::Timestamp::now().as_secs(),
                participants: people.iter().map(|(_, pubkey)| pubkey.clone()).collect(),
            }),
        ));
        app.set_generated_huddle(voices, people, true);
        app.huddle_viz.look = crate::ui::huddle_lanes::Look::default();
        let draw = |app: &mut super::App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..24)
                .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        // #team: no huddle runs there, so nothing of it shows.
        app.selected_channel = 0;
        for step in 0..60 {
            app.tick_huddle_viz_at(f64::from(step) * 0.05);
        }
        assert!(app.huddle_lanes().is_none());
        app.presentation.open_overlay(Overlay::Huddle);
        let team = draw(&mut app);
        assert!(!team.contains("You · your microphone"), "{team}");
        app.presentation.close_overlay();
        // #infra: the lanes move, and the panel agrees with them.
        app.selected_channel = 1;
        for step in 0..60 {
            app.tick_huddle_viz_at(3.0 + f64::from(step) * 0.05);
        }
        assert!(app.huddle_lanes().is_some());
        app.presentation.open_overlay(Overlay::Huddle);
        let panel = draw(&mut app);
        for said in [
            "a generated recording",
            "4 in the call",
            "You · your microphone",
            "nothing is sent",
        ] {
            assert!(panel.contains(said), "{said}: {panel}");
        }
        for contradiction in [
            "No huddle is running here",
            "you are not in the call",
            "j join",
            "m unmutes",
            "s start",
        ] {
            assert!(!panel.contains(contradiction), "{contradiction}: {panel}");
        }
        assert!(app.huddle_panel_view().recording);
    }

    #[tokio::test]
    async fn live_thread_summaries_are_visible_root_scoped_and_deterministic() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let channel = Uuid::new_v4();
        let root = "b".repeat(64);
        app.channels = vec![Channel {
            id: channel,
            name: "threads".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 1,
            last_event_at: None,
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        app.messages = vec![Message {
            event_id: root.clone(),
            channel_id: channel,
            pubkey: "c".repeat(64),
            created_at: 1,
            content: "root".into(),
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
        app.local_thread_summaries.insert(
            root.clone(),
            ThreadSummary {
                descendant_count: 1,
                last_reply_at: Some(10),
            },
        );
        app.rebuild_thread_summaries();
        let live = crate::protocol::thread_summary::LiveThreadSummary {
            root_event_id: root.clone(),
            channel_id: channel,
            summary: ThreadSummary {
                descendant_count: 3,
                last_reply_at: Some(30),
            },
            source_created_at: 40,
            source_event_id: "d".repeat(64),
        };
        assert!(app.apply_live_thread_summary(live.clone()));
        assert_eq!(app.thread_summaries[&root].descendant_count, 3);
        let mut older = live.clone();
        older.source_created_at = 39;
        older.summary.descendant_count = 9;
        assert!(!app.apply_live_thread_summary(older));
        assert_eq!(app.thread_summaries[&root].descendant_count, 3);
        let mut wrong_channel = live;
        wrong_channel.channel_id = Uuid::new_v4();
        wrong_channel.source_created_at = 41;
        assert!(!app.apply_live_thread_summary(wrong_channel));
    }

    #[tokio::test]
    async fn verified_agent_typing_is_fresh_authorized_and_exactly_scoped() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let community = app.active_community_id().unwrap();
        let channel = Uuid::new_v4();
        app.channels = vec![Channel {
            id: channel,
            name: "agents".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        app.agents.insert(
            pubkey.clone(),
            crate::store::agents::RemoteAgentView {
                schema_version: 1,
                community_id: community,
                pubkey: pubkey.clone(),
                owner_pubkey: "a".repeat(64),
                name: "Fizz".into(),
                capabilities: vec!["messages".into()],
                presence: crate::agents::Presence::Unknown,
                respond_to: Some(crate::agents::RespondTo::OwnerOnly),
                respond_to_allowlist: Vec::new(),
                eligibility: crate::agents::Eligibility::Eligible,
                stale: false,
                channel_ids: vec![channel],
                last_verified_at: nostr::Timestamp::now().as_secs(),
            },
        );
        let now = nostr::Timestamp::now().as_secs();
        let channel_typing = EventBuilder::new(Kind::Custom(20_002), "")
            .tag(Tag::parse(["h", &channel.to_string()]).unwrap())
            .custom_created_at(nostr::Timestamp::from_secs(now))
            .sign_with_keys(&keys)
            .unwrap();
        assert!(app.apply_agent_typing_event(&channel_typing, now));
        assert_eq!(
            app.visible_agent_typing_line(80).as_deref(),
            Some("◆ Fizz ⠋")
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("◆ Fizz ⠋"));
        let status = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .skip(29 * 100)
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(status.contains("◆ Fizz ⠋"));
        assert!(status.contains(&format!(
            " {} ",
            crate::ui::status::connection_label(app.connection)
        )));
        // No protocol word: the graphics protocol is :doctor's business.
        assert!(!status.contains(&format!(" {} ", app.media.protocol_name().to_uppercase())));

        let mut narrow = Terminal::new(TestBackend::new(50, 12)).unwrap();
        narrow.draw(|frame| app.render(frame)).unwrap();
        let narrow_status = narrow
            .backend()
            .buffer()
            .content()
            .iter()
            .skip(11 * 50)
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            narrow_status.contains("◆ Fizz ⠋"),
            "narrow status: {narrow_status:?}"
        );
        assert!(!narrow_status.contains("? keys"));

        app.typing_spinner_advanced_at = Instant::now() - super::AGENT_SPINNER_INTERVAL;
        assert!(app.on_tick().await.unwrap());
        assert_eq!(
            app.visible_agent_typing_line(80).as_deref(),
            Some("◆ Fizz ⠙")
        );

        let panes = crate::ui::layout::panes_with_composer(
            Rect::new(0, 0, 50, 12),
            app.community_rail,
            app.sidebar,
            false,
            app.config.ui.sidebar_width,
            app.config.ui.thread_width,
            app.composer_dock_height(),
        );
        let dock = panes.composer.unwrap();
        assert_eq!(
            app.last_hit_map
                .as_ref()
                .and_then(|map| map.hit(dock.x.saturating_add(2), dock.y.saturating_add(1))),
            Some(&HitTarget::Composer)
        );
        assert!(!app.apply_agent_typing_event(&channel_typing, now));

        app.agent_typing.clear();
        let root = "b".repeat(64);
        app.thread_root = Some(root.clone());
        let thread_typing = EventBuilder::new(Kind::Custom(20_002), "")
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["e", &root, "", "reply"]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from_secs(now))
            .sign_with_keys(&keys)
            .unwrap();
        assert!(app.apply_agent_typing_event(&thread_typing, now));
        assert_eq!(
            app.visible_agent_typing_line(80).as_deref(),
            Some("◆ Fizz ⠋")
        );

        let wrong_root = "c".repeat(64);
        let wrong_thread = EventBuilder::new(Kind::Custom(20_002), "")
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["e", &wrong_root, "", "reply"]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from_secs(now))
            .sign_with_keys(&keys)
            .unwrap();
        assert!(!app.apply_agent_typing_event(&wrong_thread, now));

        app.thread_root = None;
        app.agent_typing.clear();
        let stranger = EventBuilder::new(Kind::Custom(20_002), "")
            .tag(Tag::parse(["h", &channel.to_string()]).unwrap())
            .custom_created_at(nostr::Timestamp::from_secs(now))
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(!app.apply_agent_typing_event(&stranger, now));
        assert!(app.agent_typing.is_empty());

        app.channels[0].kind = ChannelKind::Dm;
        app.agents.get_mut(&pubkey).unwrap().owner_pubkey = "c".repeat(64);
        assert!(!app.apply_agent_typing_event(&channel_typing, now));
        app.agents.get_mut(&pubkey).unwrap().owner_pubkey = "a".repeat(64);
        assert!(app.apply_agent_typing_event(&channel_typing, now));

        app.agent_typing.clear();
        app.typing_spinner_frame = 3;
        app.typing_spinner_advanced_at = Instant::now() - super::AGENT_SPINNER_INTERVAL;
        assert!(!app.on_tick().await.unwrap());
        assert_eq!(app.typing_spinner_frame, 0);
    }

    #[tokio::test]
    async fn closed_typing_subscription_is_quarantined_and_classified_locally() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        let mut app = attachment_test_app(&temporary).await;
        let diagnostics = crate::diagnostics::DiagnosticHandle::start(
            &paths,
            crate::config::LocalJournalMode::On,
            None,
        )
        .unwrap();
        app.diagnostics = diagnostics.clone();
        let channel = Uuid::new_v4();
        app.typing_subscription_channel = Some(channel);
        let relay_text = "restricted: SENTINEL nsec1secret /private/path";
        assert!(
            app.handle_network(crate::realtime::supervisor::SupervisorEvent::Session(
                crate::realtime::session::SessionEvent::Closed {
                    subscription: super::AGENT_TYPING_SUBSCRIPTION.into(),
                    message: relay_text.into(),
                },
            ))
            .await
            .unwrap()
        );
        assert_eq!(app.typing_subscription_channel, None);
        assert_eq!(app.closed_typing_channel, Some(channel));
        assert_eq!(
            app.status_error.as_deref(),
            Some("agent typing unavailable for this channel")
        );
        diagnostics.shutdown().await;
        let records = crate::diagnostics::report::load_records(&paths);
        let closure = records
            .iter()
            .find(|record| {
                matches!(
                    record.event,
                    crate::diagnostics::DiagnosticEvent::AgentTypingSubscriptionClosed { .. }
                )
            })
            .unwrap();
        assert_eq!(
            closure.event,
            crate::diagnostics::DiagnosticEvent::AgentTypingSubscriptionClosed {
                error_class: crate::diagnostics::ErrorClass::AccessDenied
            }
        );
        let encoded = serde_json::to_string(&records).unwrap();
        assert!(!encoded.contains(relay_text));
        assert!(!encoded.contains("SENTINEL"));
        assert!(!encoded.contains("nsec1secret"));
        assert!(!encoded.contains("private/path"));
        assert!(!encoded.contains(&channel.to_string()));
    }

    #[tokio::test]
    async fn rate_limit_notice_is_transient_and_does_not_change_online_state() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.connection = ConnectionState::Online;
        app.status_error = Some("older generated notice".into());
        assert!(
            app.handle_network(crate::realtime::supervisor::SupervisorEvent::RateLimited {
                retry_after: Duration::from_secs(2),
            })
            .await
            .unwrap()
        );
        assert_eq!(app.connection, ConnectionState::Online);
        assert_eq!(app.rate_limit_display_seconds, Some(2));

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| app.render_status(frame, frame.area()))
            .unwrap();
        let row = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(row.contains("relay busy"));
        assert!(row.contains("online"));
        assert!(!row.contains("older generated notice"));

        app.rate_limit_until = Some(Instant::now() - Duration::from_millis(1));
        assert!(app.on_tick().await.unwrap());
        assert_eq!(app.rate_limit_display_seconds, None);
        assert_eq!(app.status_error.as_deref(), Some("older generated notice"));
    }

    #[tokio::test]
    async fn community_or_lock_reset_clears_stale_agent_directory_handles() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.agent_directory.selected_pubkey = Some("b".repeat(64));
        app.presentation.open_overlay(Overlay::Agents);
        app.clear_agent_directory_state();
        assert_eq!(
            app.agent_directory,
            crate::ui::agents::AgentDirectoryState::default()
        );
        assert_eq!(app.presentation.overlay, None);
    }

    #[tokio::test]
    async fn command_execution_preserves_the_overlay_opened_by_the_command() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.command = "agents".into();
        app.presentation.open_overlay(Overlay::Command);
        app.text_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false)
            .await
            .unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Agents));
        if let Some(task) = app.agent_directory_task.take() {
            task.abort();
        }
    }

    #[tokio::test]
    async fn verified_remote_agent_overlay_renders_wide_and_narrow_with_hit_targets() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let community = app.active_community_id().unwrap();
        let pubkey = "b".repeat(64);
        app.agent_directory = crate::ui::agents::AgentDirectoryState {
            agents: vec![crate::store::agents::RemoteAgentView {
                schema_version: 1,
                community_id: community,
                pubkey: pubkey.clone(),
                owner_pubkey: "c".repeat(64),
                name: "Verified Worker".into(),
                capabilities: vec!["messages".into()],
                presence: crate::agents::Presence::Online,
                respond_to: Some(crate::agents::RespondTo::Anyone),
                respond_to_allowlist: Vec::new(),
                eligibility: crate::agents::Eligibility::Eligible,
                stale: false,
                channel_ids: vec![Uuid::new_v4()],
                last_verified_at: 1,
            }],
            selected_pubkey: Some(pubkey.clone()),
            loading: false,
        };
        app.presentation.open_overlay(Overlay::Agents);
        app.agent_directory.selected_pubkey = None;
        assert!(app.select_remote_agent_mouse_target(&HitTarget::RemoteAgent(pubkey.clone())));
        assert_eq!(
            app.agent_directory.selected_pubkey.as_deref(),
            Some(pubkey.as_str())
        );
        assert!(!app.select_remote_agent_mouse_target(&HitTarget::Timeline));

        for (width, height) in [(120, 40), (72, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.render(frame)).unwrap();
            let map = app.last_hit_map.as_ref().unwrap();
            assert!(
                map.area_of(&HitTarget::RemoteAgent(pubkey.clone()))
                    .is_some(),
                "agent row must remain clickable at {width}x{height}"
            );
            let screen = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(screen.contains("Verified Worker"));
            assert!(screen.contains("verified agents"));
        }
    }

    #[tokio::test]
    async fn relay_event_deduplication_is_bounded_and_community_scoped() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let community = app.active_community_id().unwrap();
        let other = Uuid::new_v4();
        let event_id = "a".repeat(64);

        assert!(!app.relay_event_was_handled(community, &event_id));
        app.remember_relay_event(community, event_id.clone());
        assert!(app.relay_event_was_handled(community, &event_id));
        assert!(!app.relay_event_was_handled(other, &event_id));

        for index in 0..super::RELAY_EVENT_DEDUP_CAPACITY {
            app.remember_relay_event(community, format!("{index:064x}"));
        }
        assert_eq!(
            app.handled_relay_event_order.len(),
            super::RELAY_EVENT_DEDUP_CAPACITY
        );
        assert!(!app.relay_event_was_handled(community, &event_id));
    }

    #[tokio::test]
    async fn a_closed_channel_subscription_is_not_retried_in_a_hot_loop() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let channel = Uuid::new_v4();
        let subscription = format!("ch-{}", channel.simple());
        app.subscribed_channels.insert(channel);
        let closed = crate::realtime::supervisor::SupervisorEvent::Session(
            crate::realtime::session::SessionEvent::Closed {
                subscription: subscription.clone(),
                message: "generated close reason".into(),
            },
        );

        assert!(app.handle_network(closed.clone()).await.unwrap());
        assert!(!app.subscribed_channels.contains(&channel));
        assert!(app.closed_channel_subscriptions.contains(&subscription));
        app.subscribe_channel(channel).await.unwrap();
        assert!(!app.subscribed_channels.contains(&channel));
        assert!(!app.handle_network(closed).await.unwrap());
    }

    #[tokio::test]
    async fn a_terminal_input_error_yields_to_local_work() {
        let mut input =
            futures_util::stream::iter([Err(std::io::Error::other("terminal unavailable"))]);

        assert!(next_terminal_event(&mut input).await.is_none());
    }

    #[tokio::test]
    async fn keys_enter_in_one_form_for_every_platform() {
        // `?` as a Windows console reports it, with Shift; the key map has it
        // without (docs/e2e-windows.md).
        let mut input = futures_util::stream::iter([Ok(TerminalEvent::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::SHIFT,
        )))]);
        let Some(TerminalEvent::Key(key)) = next_terminal_event(&mut input).await else {
            panic!("a key");
        };
        assert_eq!(key, KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    }

    #[tokio::test]
    async fn a_paste_on_windows_is_seen_behind_its_line_break() {
        use super::{next_read_ahead_event, presses_wait};
        use crossterm::event::KeyEventKind;
        // A paste in a Windows console, as crossterm gives it: a press and a
        // release for each key; `¿` as an Alt code, a release alone.
        let key = |code, kind| {
            Ok(TerminalEvent::Key(KeyEvent::new_with_kind(
                code,
                KeyModifiers::NONE,
                kind,
            )))
        };
        let mut input = futures_util::stream::iter([
            key(KeyCode::Enter, KeyEventKind::Press),
            key(KeyCode::Enter, KeyEventKind::Release),
            key(KeyCode::Char('p'), KeyEventKind::Press),
            key(KeyCode::Char('p'), KeyEventKind::Release),
        ]);
        let mut read_ahead = std::collections::VecDeque::new();
        let first = next_read_ahead_event(&mut read_ahead, &mut input).await;
        assert!(matches!(first, Some(TerminalEvent::Key(key)) if key.code == KeyCode::Enter));
        assert!(presses_wait(&mut read_ahead, &mut input));
        // What was read ahead comes next, in order, and nothing is lost.
        let mut rest = Vec::new();
        for _ in 0..3 {
            if let Some(TerminalEvent::Key(key)) =
                next_read_ahead_event(&mut read_ahead, &mut input).await
            {
                rest.push((key.code, key.kind));
            }
        }
        assert_eq!(
            rest,
            [
                (KeyCode::Enter, KeyEventKind::Release),
                (KeyCode::Char('p'), KeyEventKind::Press),
                (KeyCode::Char('p'), KeyEventKind::Release),
            ]
        );

        // A typed Enter: only its own release waits behind it.
        let mut input = futures_util::stream::iter([
            key(KeyCode::Enter, KeyEventKind::Press),
            key(KeyCode::Enter, KeyEventKind::Release),
        ]);
        let mut read_ahead = std::collections::VecDeque::new();
        next_read_ahead_event(&mut read_ahead, &mut input).await;
        assert!(!presses_wait(&mut read_ahead, &mut input));

        // An Alt code behind the Enter is a press.
        let mut input = futures_util::stream::iter([
            key(KeyCode::Enter, KeyEventKind::Press),
            key(KeyCode::Char('¿'), KeyEventKind::Release),
        ]);
        let mut read_ahead = std::collections::VecDeque::new();
        next_read_ahead_event(&mut read_ahead, &mut input).await;
        assert!(presses_wait(&mut read_ahead, &mut input));
    }

    #[tokio::test]
    async fn an_ended_terminal_stream_does_not_spin_the_ui_loop() {
        let mut input = futures_util::stream::empty::<std::io::Result<TerminalEvent>>();

        assert!(
            tokio::time::timeout(Duration::from_millis(10), next_terminal_event(&mut input))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_lagged_relay_receiver_yields_to_local_work() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(1);
        sender
            .send(crate::realtime::supervisor::SupervisorEvent::Connecting)
            .unwrap();
        sender
            .send(crate::realtime::supervisor::SupervisorEvent::Connecting)
            .unwrap();

        assert!(next_supervisor_event(&mut receiver).await.is_none());
    }

    #[tokio::test]
    async fn a_closed_relay_receiver_does_not_spin_the_ui_loop() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(1);
        drop(sender);

        assert!(
            tokio::time::timeout(
                Duration::from_millis(10),
                next_supervisor_event(&mut receiver)
            )
            .await
            .is_err()
        );
    }

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
    async fn cached_workspace_read_is_persisted_without_a_runtime() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "offline-read".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:offline-read".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community(
                "offline-read".into(),
                "wss://offline-read.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        config.default_community = Some(community);
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = super::App::new(config, paths, handle).await.unwrap();
        let channel = Uuid::new_v4();
        app.channels = vec![Channel {
            id: channel,
            name: "offline".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 1,
            last_event_at: Some(42),
            facts: Default::default(),
        }];
        app.messages = vec![Message {
            event_id: "event-1".into(),
            channel_id: channel,
            pubkey: "b".repeat(64),
            created_at: 42,
            content: "cached".into(),
            attachments: vec![],
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
        app.timeline.at_live_bottom = true;
        app.computed_unread.insert(channel);

        app.mark_current_read().await.unwrap();

        let pubkey = identity.pubkey;
        let contexts = app
            .store
            .call(move |store| store.read_contexts(community, &pubkey, false))
            .await
            .unwrap();
        assert_eq!(contexts.get(&channel.to_string()), Some(&42));
        assert!(!app.computed_unread.contains(&channel));
    }

    #[tokio::test]
    async fn a_single_channel_click_opens_its_timeline() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let own = Keys::generate();
        let relay = Keys::generate();
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "mouse-channel".into(),
            pubkey: own.public_key().to_hex(),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:mouse-channel".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community(
                "mouse-channel".into(),
                "wss://mouse-channel.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        config.default_community = Some(community);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        store
            .pin_relay_pubkey(community, &relay.public_key().to_hex())
            .unwrap();
        for (channel, name, content) in [(first, "alpha", "first"), (second, "beta", "second")] {
            let metadata = EventBuilder::new(Kind::Custom(39_000), "")
                .tags([
                    Tag::parse(["d", &channel.to_string()]).unwrap(),
                    Tag::parse(["name", name]).unwrap(),
                    Tag::parse(["t", "stream"]).unwrap(),
                    Tag::parse(["closed"]).unwrap(),
                    Tag::parse(["public"]).unwrap(),
                ])
                .sign_with_keys(&relay)
                .unwrap();
            store.apply_event(community, &metadata).unwrap();
            let membership = EventBuilder::new(Kind::Custom(39_002), "")
                .tags([
                    Tag::parse(["d", &channel.to_string()]).unwrap(),
                    Tag::parse(["p", &own.public_key().to_hex()]).unwrap(),
                ])
                .sign_with_keys(&relay)
                .unwrap();
            store.apply_event(community, &membership).unwrap();
            let message = EventBuilder::new(Kind::Custom(9), content)
                .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
                .sign_with_keys(&own)
                .unwrap();
            store.apply_event(community, &message).unwrap();
        }
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = super::App::new(config, paths, handle).await.unwrap();
        let first_index = app
            .channels
            .iter()
            .position(|channel| channel.id == first)
            .unwrap();
        app.select_channel_index(first_index);
        app.load_selected_channel().await.unwrap();
        assert!(
            app.messages
                .iter()
                .all(|message| message.channel_id == first)
        );

        let second_index = app
            .channels
            .iter()
            .position(|channel| channel.id == second)
            .unwrap();
        app.activate_channel_click(second_index, false)
            .await
            .unwrap();

        assert_eq!(
            app.current_channel().map(|channel| channel.id),
            Some(second)
        );
        assert!(
            !app.messages.is_empty()
                && app
                    .messages
                    .iter()
                    .all(|message| message.channel_id == second)
        );
        assert_eq!(app.presentation.focus, FocusSurface::Channels);
    }

    #[test]
    fn visible_timeline_is_acknowledged_even_when_sidebar_has_focus() {
        let mut presentation = PresentationState {
            route: Route::Workspace,
            focus: FocusSurface::Channels,
            ..PresentationState::default()
        };
        let timeline = TimelineState {
            at_live_bottom: true,
            ..TimelineState::default()
        };
        let mut thread = TimelineState::default();

        assert!(should_mark_visible_read(&presentation, &timeline, &thread));

        presentation.focus = FocusSurface::Context;
        assert!(!should_mark_visible_read(&presentation, &timeline, &thread));
        thread.at_live_bottom = true;
        assert!(should_mark_visible_read(&presentation, &timeline, &thread));

        presentation.overlay = Some(Overlay::Help);
        assert!(!should_mark_visible_read(&presentation, &timeline, &thread));
        presentation.overlay = None;
        presentation.route = Route::Inbox;
        assert!(!should_mark_visible_read(&presentation, &timeline, &thread));
    }

    #[test]
    fn timeline_read_marker_requires_the_selected_channel_at_its_live_edge() {
        let channel = Uuid::new_v4();
        let message = Message {
            event_id: "event-1".into(),
            channel_id: channel,
            pubkey: "a".repeat(64),
            created_at: 42,
            content: "hello".into(),
            attachments: vec![],
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            delivery: crate::domain::DeliveryState::Delivered,
            system: None,
            sent_from_thread: None,
            broadcast: false,
            emoji: Vec::new(),
            edited: false,
        };
        let mut timeline = TimelineState {
            at_live_bottom: true,
            ..TimelineState::default()
        };

        assert_eq!(
            timeline_read_mark(&timeline, channel, std::slice::from_ref(&message)),
            Some((channel.to_string(), 42))
        );
        assert_eq!(
            timeline_read_mark(&timeline, Uuid::new_v4(), std::slice::from_ref(&message)),
            None
        );
        timeline.at_live_bottom = false;
        assert_eq!(
            timeline_read_mark(&timeline, channel, std::slice::from_ref(&message)),
            None
        );
    }

    #[tokio::test]
    async fn clipboard_image_staging_completes_when_the_general_background_lane_is_full() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        for _ in 0..128 {
            app.background_tx
                .try_send(super::Background::Changed)
                .unwrap();
        }
        app.clipboard = Arc::new(TestClipboard(
            crate::media::clipboard::ClipboardContents::Image(
                crate::media::clipboard::ClipboardImage {
                    width: 1,
                    height: 1,
                    rgba: vec![12, 34, 56, 255],
                },
            ),
        ));

        app.import_clipboard().await.unwrap();
        for _ in 0..2 {
            let event = tokio::time::timeout(Duration::from_secs(2), app.attachment_rx.recv())
                .await
                .unwrap()
                .unwrap();
            app.handle_attachment_background(event).await.unwrap();
        }

        assert!(app.staging_attachments.is_empty());
        assert!(matches!(
            app.composer.attachments.as_slice(),
            [crate::media::DraftAttachment::Pending(pending)] if pending.mime == "image/png"
        ));
    }

    #[tokio::test]
    async fn native_file_picker_stages_selected_file_with_a_full_general_lane() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let source = temporary.path().join("selected.txt");
        std::fs::write(&source, b"bounded attachment").unwrap();
        app.file_picker = Arc::new(TestFilePicker(
            crate::media::file_picker::FilePickerOutcome::Files(vec![source]),
        ));
        for _ in 0..128 {
            app.background_tx
                .try_send(super::Background::Changed)
                .unwrap();
        }

        app.open_file_picker();
        for _ in 0..2 {
            let event = tokio::time::timeout(Duration::from_secs(2), app.attachment_rx.recv())
                .await
                .unwrap()
                .unwrap();
            app.handle_attachment_background(event).await.unwrap();
        }

        assert!(app.staging_attachments.is_empty());
        assert!(matches!(
            app.composer.attachments.as_slice(),
            [crate::media::DraftAttachment::Pending(pending)] if pending.filename == "selected.txt"
        ));
    }

    #[tokio::test]
    async fn file_picker_cancel_unavailable_and_stale_results_are_inert() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        let target = app.presentation.composer_target.clone().unwrap();
        app.status_error = Some("opening native file picker…".into());
        app.handle_attachment_background(super::AttachmentBackground::FilesPicked {
            target: target.clone(),
            outcome: crate::media::file_picker::FilePickerOutcome::Cancelled,
        })
        .await
        .unwrap();
        assert!(app.status_error.is_none());

        app.handle_attachment_background(super::AttachmentBackground::FilesPicked {
            target: target.clone(),
            outcome: crate::media::file_picker::FilePickerOutcome::Unavailable,
        })
        .await
        .unwrap();
        assert!(
            app.status_error
                .as_deref()
                .is_some_and(|status| status.contains("Alt-o"))
        );

        let source = temporary.path().join("stale.txt");
        std::fs::write(&source, b"stale target").unwrap();
        app.presentation.composer_target = None;
        app.handle_attachment_background(super::AttachmentBackground::FilesPicked {
            target,
            outcome: crate::media::file_picker::FilePickerOutcome::Files(vec![source]),
        })
        .await
        .unwrap();
        assert!(app.composer.attachments.is_empty());
        assert!(app.staging_attachments.is_empty());
    }

    fn artifact_event(keys: &Keys, home: Uuid, title: &str) -> nostr::Event {
        let id = Uuid::new_v4().to_string();
        let home = home.to_string();
        EventBuilder::new(Kind::Custom(45_010), "{\"version\":1}")
            .tags(
                [
                    ["ar", "1"],
                    ["d", id.as_str()],
                    ["h", home.as_str()],
                    ["type", "buzz.task"],
                    ["title", title],
                    ["op", "create"],
                ]
                .map(|parts| Tag::parse(parts).unwrap()),
            )
            .sign_with_keys(keys)
            .unwrap()
    }

    #[tokio::test]
    async fn artifact_sync_commits_only_at_eose_and_never_touches_chat() {
        use crate::config::{IdentityConfig, KeyBackend};
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let keys = Keys::generate();
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "a".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "a".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community("a".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        // No runtime: a configured community would prompt for the identity.
        let mut app = super::App::new(Config::default(), paths, handle)
            .await
            .unwrap();
        app.config = config;
        let channel = Uuid::new_v4();
        let caps = crate::protocol::artifact::Capabilities {
            sync: true,
            head: true,
            max_events: 2,
        };
        app.artifact_caps = Some((community, caps));
        app.artifact_sync = Some(super::ArtifactSync::new(community, channel));
        let sync = super::ARTIFACT_SYNC_SUBSCRIPTION;
        let stored = |app: &super::App| {
            let store = app.store.clone();
            async move {
                store
                    .call(move |store| store.channel_artifacts(community, channel, None))
                    .await
                    .unwrap()
            }
        };

        // Heads before EOSE are only buffered.
        let first = artifact_event(&keys, channel, "First");
        assert!(!app.apply_artifact_sync_event(sync, &first).await.unwrap());
        assert!(stored(&app).await.is_empty());
        // Another subscription's artifact is ignored.
        assert!(
            !app.apply_artifact_sync_event("other", &first)
                .await
                .unwrap()
        );
        app.commit_artifact_sync().await.unwrap();
        assert_eq!(stored(&app).await.len(), 1, "committed at EOSE");

        // After EOSE a live head applies immediately.
        let live = artifact_event(&keys, channel, "Live");
        assert!(app.apply_artifact_sync_event(sync, &live).await.unwrap());
        assert_eq!(stored(&app).await.len(), 2);

        // A set larger than max_events is never committed partially.
        app.artifact_sync = Some(super::ArtifactSync::new(community, channel));
        for title in ["A", "B", "C"] {
            let event = artifact_event(&keys, channel, title);
            app.apply_artifact_sync_event(sync, &event).await.unwrap();
        }
        assert!(app.artifact_sync.as_ref().unwrap().retry_at.is_some());
        app.commit_artifact_sync().await.unwrap();
        assert_eq!(stored(&app).await.len(), 2, "the last complete set stays");
        assert!(!app.artifact_sync.as_ref().unwrap().complete);

        // Redaction of a current head: the relay ends the sync with
        // `CLOSED "resync: ..."`. The held set stays (marked possibly stale)
        // and a fresh sync is scheduled; its complete cut is authoritative.
        app.artifact_sync = Some(super::ArtifactSync::new(community, channel));
        app.artifact_sync.as_mut().unwrap().complete = true;
        app.handle_network(crate::realtime::supervisor::SupervisorEvent::Session(
            crate::realtime::session::SessionEvent::Closed {
                subscription: sync.into(),
                message: "resync: artifact retired".into(),
            },
        ))
        .await
        .unwrap();
        let retry = app.artifact_sync.as_ref().unwrap();
        assert!(retry.retry_at.is_some() && !retry.complete && !retry.blocked);
        assert_eq!(
            stored(&app).await.len(),
            2,
            "nothing dropped before the new cut"
        );

        // More live artifacts in the channel than one sync returns: the
        // relay ends every attempt with the same error. The held set stays,
        // nothing is scheduled, and the panel says so in plain words.
        app.artifact_sync = Some(super::ArtifactSync::new(community, channel));
        app.artifact_sync.as_mut().unwrap().complete = true;
        app.artifact_panel.scope = crate::ui::artifacts::ArtifactScope::Channel(channel);
        app.presentation.open_overlay(Overlay::Artifacts);
        app.handle_network(crate::realtime::supervisor::SupervisorEvent::Session(
            crate::realtime::session::SessionEvent::Closed {
                subscription: sync.into(),
                message: crate::realtime::admission::CLOSED_SYNC_OVERFLOW.into(),
            },
        ))
        .await
        .unwrap();
        let over = app.artifact_sync.as_ref().unwrap();
        assert!(over.overflowed && !over.complete && !over.blocked);
        assert!(over.retry_at.is_none(), "not asked again on a timer");
        assert_eq!(stored(&app).await.len(), 2, "the last complete set stays");
        assert_eq!(
            app.artifact_panel.sync_note.as_deref(),
            Some("too many artifacts here to sync · list may be stale")
        );
        assert_eq!(app.artifact_panel.items.len(), 2);
        // A dropped connection does not turn it back into "syncing".
        app.handle_network(crate::realtime::supervisor::SupervisorEvent::Session(
            crate::realtime::session::SessionEvent::Disconnected("generated".into()),
        ))
        .await
        .unwrap();
        app.refresh_artifact_panel().await.unwrap();
        assert_eq!(
            app.artifact_panel.sync_note.as_deref(),
            Some("too many artifacts here to sync · list may be stale")
        );
        app.presentation.close_overlay();

        // The closing of an app's records subscription is its pane's to
        // show: nothing reaches the status line.
        app.status_error = None;
        let changed = app
            .handle_network(crate::realtime::supervisor::SupervisorEvent::Session(
                crate::realtime::session::SessionEvent::Closed {
                    subscription: format!(
                        "{}0123456789abcdef01234567",
                        crate::protocol::artifact::APP_RECORDS_SUBSCRIPTION_PREFIX
                    ),
                    message: crate::realtime::admission::CLOSED_RESYNC.into(),
                },
            ))
            .await
            .unwrap();
        assert!(!changed && app.status_error.is_none());

        // The panel shows the set; artifacts never become messages or unread.
        // A new cut holding only `live` replaces the set: `first` left it.
        app.artifact_sync = Some(super::ArtifactSync::new(community, channel));
        app.apply_artifact_sync_event(sync, &live).await.unwrap();
        app.commit_artifact_sync().await.unwrap();
        let held = stored(&app).await;
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].head.title.as_deref(), Some("Live"));
        app.artifact_panel.scope = crate::ui::artifacts::ArtifactScope::Channel(channel);
        app.presentation.open_overlay(Overlay::Artifacts);
        app.refresh_artifact_panel().await.unwrap();
        assert_eq!(app.artifact_panel.sync_note.as_deref(), Some("synced"));
        assert!(app.messages.is_empty());
        assert!(app.computed_unread.is_empty() && app.manual_unread.is_empty());
        let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::artifacts::render(
                    frame,
                    frame.area(),
                    &app.artifact_panel,
                    &std::collections::HashMap::new(),
                    &app.profiles,
                    &app.theme,
                )
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(screen.contains("synced"), "{screen}");
        assert!(screen.contains("Live"), "{screen}");
        assert!(screen.contains("buzz.task"), "{screen}");
    }

    #[tokio::test]
    async fn idle_tick_does_not_request_a_redraw_without_visible_work() {
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

        assert!(!app.cache_dirty);
        assert!(!app.on_tick().await.unwrap());
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
    async fn inbox_workspace_emits_list_detail_and_row_hit_targets() {
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
            label: "inbox-test".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:inbox-test".into(),
        };
        app.config.identities.push(identity.clone());
        app.config
            .add_community(
                "inbox-test".into(),
                "wss://inbox.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        let channel = Uuid::new_v4();
        let item = InboxItem {
            conversation_id: format!("event:{}", "b".repeat(64)),
            categories: vec![InboxCategory::Mention],
            event_id: Some("b".repeat(64)),
            channel_id: Some(channel),
            thread_root: None,
            sender_pubkey: Some(identity.pubkey),
            created_at: 1,
            preview: "Inbox row".into(),
            unread_count: 1,
            first_unread_event_id: Some("b".repeat(64)),
            first_unread_at: Some(1),
            draft_count: 0,
            latest_draft_at: None,
            forced_unread: false,
        };
        app.inbox_items = vec![item.clone()];
        app.inbox_state.reconcile(&app.inbox_items);
        app.presentation.enter_inbox();

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let map = app.last_hit_map.as_ref().unwrap();
        assert!(map.area_of(&HitTarget::InboxList).is_some());
        assert!(map.area_of(&HitTarget::InboxDetail).is_some());
        assert!(
            map.area_of(&HitTarget::InboxItem(item.conversation_id))
                .is_some()
        );
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
            facts: Default::default(),
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
            delivery: crate::domain::DeliveryState::Delivered,
            system: None,
            sent_from_thread: None,
            broadcast: false,
            emoji: Vec::new(),
            edited: false,
        }];
        app.timeline.reconcile(&app.messages);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let panes = crate::ui::layout::panes_with_composer(
            terminal.size().unwrap().into(),
            app.community_rail,
            app.sidebar,
            false,
            app.config.ui.sidebar_width,
            app.config.ui.thread_width,
            app.composer_dock_height(),
        );
        let map = app.last_hit_map.as_ref().unwrap();
        assert!(map.area_of(&HitTarget::Composer).is_some());
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("Read-only: restore or unlock the identity to write."));
        let sidebar = panes.sidebar.unwrap();
        assert_eq!(
            map.hit(sidebar.x.saturating_add(1), sidebar.y.saturating_add(1)),
            Some(&HitTarget::Channel(0))
        );
        // Offline, the first row of the conversation says so and what to
        // do; the first message starts under it.
        assert!(screen.contains("offline · showing what is saved here"));
        assert_eq!(
            map.hit(
                panes.timeline.x.saturating_add(1),
                panes.timeline.y.saturating_add(2)
            ),
            Some(&HitTarget::TimelineMessage("event-1".into()))
        );

        app.presentation.composer_target = Some(ComposerTarget {
            community_id: app.config.communities[0].id,
            channel_id,
            thread_root_id: None,
            parent_event_id: None,
        });
        app.mention_picker = Some(MentionPicker::new(
            0..1,
            String::new(),
            vec![MentionCandidate {
                pubkey: "b".repeat(64),
                label: "member".into(),
                is_agent: false,
                agent_eligibility: None,
                verified: false,
                presence: None,
            }],
        ));
        terminal.draw(|frame| app.render(frame)).unwrap();
        let panes = crate::ui::layout::panes_with_composer(
            terminal.size().unwrap().into(),
            app.community_rail,
            app.sidebar,
            false,
            app.config.ui.sidebar_width,
            app.config.ui.thread_width,
            app.composer_dock_height(),
        );
        let dock = panes.composer.unwrap();
        let mention = Rect::new(dock.x, dock.y.saturating_sub(3), dock.width, 3);
        assert_eq!(
            app.last_hit_map
                .as_ref()
                .unwrap()
                .hit(mention.x.saturating_add(1), mention.y.saturating_add(1)),
            Some(&HitTarget::MentionCandidate(0))
        );

        app.mention_picker = None;
        app.community_rail = false;
        app.sidebar = false;
        app.composer
            .attachments
            .push(crate::media::DraftAttachment::Pending(
                crate::media::PendingAttachment {
                    id: Uuid::new_v4().to_string(),
                    cache_name: "a".repeat(64) + ".txt",
                    mime: "text/plain".into(),
                    filename: "generated.txt".into(),
                    sha256: "a".repeat(64),
                    size: 12,
                },
            ));
        app.staging_attachments.push(super::StagingAttachment {
            id: Uuid::new_v4().to_string(),
            filename: "pasted-image.png".into(),
        });
        terminal.draw(|frame| app.render(frame)).unwrap();
        let queue = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(queue.contains("Ctrl-v paste"));
        assert!(queue.contains("[queued] generated.txt"));
        assert!(queue.contains("[processing] pasted-image.png"));
    }

    #[tokio::test]
    async fn empty_state_renders_leader_which_key_overlay() {
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
        let _ = app.input_router.dispatch(
            &app.keymap,
            crate::ui::input::InputContext::workspace(),
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        app.presentation
            .open_overlay(crate::ui::state::Overlay::WhichKey);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("leader"));
        assert!(text.contains("open Inbox"));
    }

    #[tokio::test]
    async fn empty_state_renders_contextual_actions_overlay() {
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
        app.action_menu = Some(ActionMenu::new(vec![
            ContextAction {
                action: UiAction::Compose,
                label: "reply",
                enabled: true,
                reason: None,
            },
            ContextAction {
                action: UiAction::Delete,
                label: "delete own message",
                enabled: false,
                reason: Some("only your own message can be deleted"),
            },
        ]));
        app.presentation.open_overlay(Overlay::Actions);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("actions"));
        assert!(text.contains("reply"));
        assert!(text.contains("only your own message can be deleted"));
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
        app.presentation
            .open_overlay(crate::ui::state::Overlay::Help);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("bzz effective keymap"));
        assert!(text.contains("active scoped overrides"));
    }

    async fn read_only_fixture(
        temporary: &TempDir,
        kind: ChannelKind,
        facts: crate::domain::ChannelFacts,
    ) -> super::App {
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
            label: "read-only".into(),
            pubkey: "a".repeat(64),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:read-only".into(),
        };
        app.config.identities.push(identity.clone());
        app.config
            .add_community(
                "read-only".into(),
                "wss://read-only.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        app.channels = vec![Channel {
            id: Uuid::new_v4(),
            name: "announcements".into(),
            about: String::new(),
            kind,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 1,
            last_event_at: None,
            facts,
        }];
        app
    }

    fn screen_text(app: &mut super::App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[tokio::test]
    async fn guest_channel_shows_fixed_read_only_notice_and_refuses_composer() {
        let temporary = TempDir::new().unwrap();
        let mut app = read_only_fixture(
            &temporary,
            ChannelKind::Stream,
            crate::domain::ChannelFacts {
                join_role: crate::domain::JoinRole::Guest,
                viewer_role: Some(crate::domain::MemberRole::Guest),
                ..Default::default()
            },
        )
        .await;
        let screen = screen_text(&mut app);
        assert!(
            screen.contains("read-only · announce channel (guest)"),
            "{screen}"
        );
        let community = app.active_community_id().unwrap();
        let channel = app.channels[0].id;
        app.open_composer_target(community, channel, None, None)
            .await
            .unwrap();
        assert!(app.presentation.composer_target.is_none());
        assert_eq!(
            app.status_error.as_deref(),
            Some("read-only · announce channel (guest)")
        );
    }

    #[tokio::test]
    async fn unknown_channel_type_is_read_only_and_member_role_writes() {
        let temporary = TempDir::new().unwrap();
        let mut app = read_only_fixture(
            &temporary,
            ChannelKind::Other("future".into()),
            crate::domain::ChannelFacts::default(),
        )
        .await;
        assert!(screen_text(&mut app).contains("read-only · channel type not supported"));
        app.channels[0].kind = ChannelKind::Stream;
        app.channels[0].facts.viewer_role = Some(crate::domain::MemberRole::Member);
        let screen = screen_text(&mut app);
        assert!(!screen.contains("read-only ·"), "{screen}");
    }

    #[tokio::test]
    async fn system_channels_stay_collapsed_until_the_system_command() {
        let temporary = TempDir::new().unwrap();
        let mut app = read_only_fixture(
            &temporary,
            ChannelKind::Stream,
            crate::domain::ChannelFacts::default(),
        )
        .await;
        let mut system = app.channels[0].clone();
        system.id = Uuid::new_v4();
        system.name = "agent-config".into();
        system.kind = ChannelKind::System;
        app.channels.push(system);
        let collapsed = screen_text(&mut app);
        assert!(collapsed.contains("· ⚙1 "), "{collapsed}");
        assert!(!collapsed.contains("agent-config"));
        assert_eq!(app.ordered_channel_indexes(), vec![0]);
        app.command = ":system".into();
        app.execute_command().await.unwrap();
        let expanded = screen_text(&mut app);
        assert!(expanded.contains("· ⚙ open"), "{expanded}");
        assert!(expanded.contains("⚙agent-config"), "{expanded}");
        assert_eq!(app.ordered_channel_indexes(), vec![0, 1]);
    }

    #[tokio::test]
    async fn kicked_selected_channel_keeps_its_draft_and_moves_selection() {
        let temporary = TempDir::new().unwrap();
        let mut app = read_only_fixture(
            &temporary,
            ChannelKind::Stream,
            crate::domain::ChannelFacts::default(),
        )
        .await;
        let community = app.active_community_id().unwrap();
        let relay = nostr::Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let config = app.config.clone();
        app.store
            .call(move |store| {
                store.sync_config(&config)?;
                store.pin_relay_pubkey(community, &relay_hex)
            })
            .await
            .unwrap();
        let (kept, kicked) = (Uuid::new_v4(), Uuid::new_v4());
        let me = "a".repeat(64);
        let sign = |kind: u16, tags: Vec<Vec<String>>, at: u64| {
            nostr::EventBuilder::new(nostr::Kind::Custom(kind), "")
                .tags(tags.into_iter().map(|tag| nostr::Tag::parse(tag).unwrap()))
                .custom_created_at(nostr::Timestamp::from(at))
                .sign_with_keys(&relay)
                .unwrap()
        };
        for (channel, name) in [(kept, "alpha"), (kicked, "zulu")] {
            let id = channel.to_string();
            let events = vec![
                sign(
                    39_000,
                    vec![
                        vec!["d".into(), id.clone()],
                        vec!["name".into(), name.into()],
                    ],
                    1,
                ),
                sign(
                    39_002,
                    vec![vec!["d".into(), id.clone()], vec!["p".into(), me.clone()]],
                    1,
                ),
            ];
            app.store
                .call(move |store| {
                    for event in events {
                        store.apply_event(community, &event)?;
                    }
                    Ok(())
                })
                .await
                .unwrap();
        }
        app.hydrate_cache().await.unwrap();
        app.selected_channel = app.channels.iter().position(|c| c.id == kicked).unwrap();
        app.presentation.composer_target = Some(crate::ui::state::ComposerTarget {
            community_id: community,
            channel_id: kicked,
            thread_root_id: None,
            parent_event_id: None,
        });
        app.composer
            .set_draft("unsent words".into(), vec![], vec![]);

        let kick = sign(39_002, vec![vec!["d".into(), kicked.to_string()]], 2);
        app.store
            .call(move |store| store.apply_event(community, &kick))
            .await
            .unwrap();
        app.hydrate_cache().await.unwrap();

        assert!(app.presentation.composer_target.is_none());
        assert_eq!(app.current_channel().map(|c| c.id), Some(kept));
        assert_eq!(
            app.status_error.as_deref(),
            Some("you are no longer a member of that channel; draft kept")
        );
        let draft = app
            .store
            .call(move |store| store.draft_record(community, kicked, None))
            .await
            .unwrap()
            .expect("draft persisted");
        assert_eq!(draft.body, "unsent words");
    }

    /// v0.13 M2.2/M3: the profile card shows the verified identity and the
    /// NIP-CL public channels, applies only its own outstanding answer once,
    /// keeps a failed refresh explicitly stale and records relay backoff.
    #[tokio::test]
    async fn profile_card_applies_only_its_outstanding_public_lookup() {
        use crate::{
            domain::MemberRole,
            protocol::membership_public::{Lookup, PublicMembership, VerifiedPublic},
            ui::profile::PublicChannels,
        };
        let temporary = TempDir::new().unwrap();
        let mut app = read_only_fixture(
            &temporary,
            ChannelKind::Stream,
            crate::domain::ChannelFacts::default(),
        )
        .await;
        let community = app.active_community_id().unwrap();
        let known = app.channels[0].id;
        let subject = "c".repeat(64);
        app.profiles.insert(
            subject.clone(),
            crate::domain::Profile::verified_only(
                &subject,
                crate::protocol::verified_profile::VerifiedName {
                    display_name: "Ada Lovelace".into(),
                    username: "ada@corp.example".into(),
                    full_name: None,
                    issuer_label: "Corp IdP".into(),
                    verified_at: 1,
                },
                "e".repeat(64),
                1,
            ),
        );
        app.command = format!("profile {subject}");
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Profile));
        // No runtime: the lookup cannot be signed.
        assert_eq!(
            app.profile_card.as_ref().unwrap().public,
            PublicChannels::Locked
        );
        let screen = screen_text(&mut app);
        assert!(screen.contains("Ada Lovelace ✓"), "{screen}");
        assert!(
            screen.contains("✓ verified by Corp IdP · ada@corp.example"),
            "{screen}"
        );
        assert!(screen.contains("unlock the identity"), "{screen}");

        let verified = |channels: Vec<PublicMembership>| {
            Ok(Lookup::Verified(VerifiedPublic {
                snapshot_id: "0".repeat(64),
                memberships: channels,
            }))
        };
        let unknown = Uuid::new_v4();
        let list = vec![
            PublicMembership {
                channel_id: known,
                role: MemberRole::Guest,
                channel_type: "stream".into(),
                archived: true,
            },
            PublicMembership {
                channel_id: unknown,
                role: MemberRole::Owner,
                channel_type: "forum".into(),
                archived: false,
            },
        ];
        app.profile_card.as_mut().unwrap().outstanding = Some(7);
        app.profile_card.as_mut().unwrap().public = PublicChannels::Loading;
        // Superseded generation, another subject, another community: ignored.
        app.apply_public_lookup(community, &subject, 6, verified(list.clone()));
        app.apply_public_lookup(community, &"d".repeat(64), 7, verified(list.clone()));
        app.apply_public_lookup(Uuid::new_v4(), &subject, 7, verified(list.clone()));
        assert_eq!(
            app.profile_card.as_ref().unwrap().public,
            PublicChannels::Loading
        );
        app.apply_public_lookup(community, &subject, 7, verified(list.clone()));
        assert_eq!(
            app.profile_card.as_ref().unwrap().public,
            PublicChannels::Verified(list.clone())
        );
        // Consumed once: a second answer for the same request changes nothing.
        app.apply_public_lookup(community, &subject, 7, Err("late".into()));
        assert_eq!(
            app.profile_card.as_ref().unwrap().public,
            PublicChannels::Verified(list.clone())
        );
        let screen = screen_text(&mut app);
        assert!(
            screen.contains("#announcements · stream · guest · archived"),
            "{screen}"
        );
        assert!(
            screen.contains(&format!(
                "(not cached) {} · forum · owner",
                &unknown.to_string()[..8]
            )),
            "{screen}"
        );
        // A failed refresh is never an empty list: the old one stays, stale.
        app.profile_card.as_mut().unwrap().stale = app
            .public_lookup_cache
            .get(&(community, subject.clone()))
            .cloned();
        app.profile_card.as_mut().unwrap().outstanding = Some(8);
        app.apply_public_lookup(community, &subject, 8, Err("bad signature".into()));
        assert_eq!(
            app.profile_card.as_ref().unwrap().public,
            PublicChannels::Failed
        );
        let screen = screen_text(&mut app);
        assert!(screen.contains("could not verify"), "{screen}");
        assert!(screen.contains("stale — last verified list"), "{screen}");
        // 429 records a retry delay for the community.
        app.profile_card.as_mut().unwrap().outstanding = Some(9);
        app.apply_public_lookup(community, &subject, 9, Ok(Lookup::RateLimited(30)));
        let (until, rate_limited) = app.public_lookup_backoff[&community];
        let now = crate::clock::WallClock::now_unix(&crate::clock::SystemClock);
        assert!(rate_limited && until >= now + 29, "{until} vs {now}");
        // Esc closes and cancels.
        app.profile_key(KeyEvent::from(KeyCode::Esc));
        assert!(app.profile_card.is_none());
        assert_eq!(app.presentation.overlay, None);
    }

    /// An app with a generated huddle in `#team`: Ada (1), Lin (2) and Sam
    /// (3), you muted, the passage played once (`looped: None`) or looping.
    async fn zoom_test_app(
        temporary: &TempDir,
        script: &[crate::huddle::synth::Line],
    ) -> super::App {
        let mut app = attachment_test_app(temporary).await;
        app.channels = vec![Channel {
            id: Uuid::new_v4(),
            name: "team".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 4,
            last_event_at: None,
            facts: Default::default(),
        }];
        app.selected_channel = 0;
        app.set_generated_huddle(
            crate::huddle::synth::Synthetic::new(script, None),
            vec![(1, "Ada".into()), (2, "Lin".into()), (3, "Sam".into())],
            true,
        );
        app.huddle_viz.look = crate::ui::huddle_lanes::Look::default();
        app
    }

    fn zoom_screen(app: &mut super::App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Plan B5: `z` in the panel and `:huddle zoom` zoom the visualizer to
    /// the window and back; `v` opens the picker, which draws the selected
    /// style live; Esc puts the saved one back, Enter keeps and saves it.
    #[tokio::test]
    async fn the_zoomed_view_and_its_picker_preview_keep_and_restore() {
        use crate::config::HuddleStyle;
        let temporary = TempDir::new().unwrap();
        let mut app = zoom_test_app(&temporary, &crate::huddle::synth::DEMO_SCRIPT).await;
        let mut t = 0.0;
        while t <= 5.5 {
            app.tick_huddle_viz_at(t);
            t += 1.0 / 30.0;
        }
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        // Not zoomed until asked; then the whole window but the status line.
        app.presentation.open_overlay(Overlay::Huddle);
        assert!(!app.huddle_zoomed());
        app.huddle_key(key(KeyCode::Char('z'))).await.unwrap();
        assert!(app.huddle_zoomed());
        let screen = zoom_screen(&mut app, 120, 30);
        assert!(
            screen.contains("huddle in #team · 4 in the call · Ada and Lin at once"),
            "{screen}"
        );
        assert!(screen.contains("You · your microphone"), "{screen}");
        assert!(
            screen.lines().last().unwrap().contains("HUDDLE"),
            "{screen}"
        );
        app.huddle_key(key(KeyCode::Char('z'))).await.unwrap();
        assert!(!app.huddle_zoomed());
        assert_eq!(app.presentation.overlay, Some(Overlay::Huddle));
        app.huddle_command("zoom").await.unwrap();
        assert!(app.huddle_zoomed());
        // The picker: Down previews bars, Down again the orb; Esc restores.
        app.huddle_key(key(KeyCode::Char('v'))).await.unwrap();
        assert_eq!(app.huddle_viz.picker, Some(0));
        app.huddle_key(key(KeyCode::Down)).await.unwrap();
        assert_eq!(app.huddle_style(), HuddleStyle::Bars);
        app.tick_huddle_viz_at(t);
        let screen = zoom_screen(&mut app, 120, 30);
        assert!(screen.contains("bars · the call's spectrum"), "{screen}");
        assert!(screen.contains("▶ bars"), "{screen}");
        assert!(screen.contains("Enter keep"), "{screen}");
        app.huddle_key(key(KeyCode::Down)).await.unwrap();
        assert_eq!(app.huddle_style(), HuddleStyle::Orb);
        app.huddle_key(key(KeyCode::Esc)).await.unwrap();
        assert_eq!(app.huddle_viz.picker, None);
        assert_eq!(app.huddle_style(), HuddleStyle::Lanes);
        assert_eq!(app.config.ui.huddle_style, HuddleStyle::Lanes);
        assert!(app.huddle_zoomed(), "Esc closes the picker, not the view");
        let screen = zoom_screen(&mut app, 120, 30);
        assert!(screen.contains("lanes · one lane per person"), "{screen}");
        // Enter keeps the selection and saves it.
        app.huddle_key(key(KeyCode::Char('v'))).await.unwrap();
        app.huddle_key(key(KeyCode::Up)).await.unwrap();
        app.huddle_key(key(KeyCode::Enter)).await.unwrap();
        assert_eq!(app.config.ui.huddle_style, HuddleStyle::Orb);
        let saved = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(saved.contains("huddle_style = \"orb\""), "{saved}");
        assert_eq!(app.status_error.as_deref(), Some("huddle style: orb"));
        // `:huddle style <name>` and a wrong name.
        app.huddle_command("style bars").await.unwrap();
        assert_eq!(app.config.ui.huddle_style, HuddleStyle::Bars);
        app.huddle_command("style disco").await.unwrap();
        assert_eq!(app.config.ui.huddle_style, HuddleStyle::Bars);
        assert!(
            app.status_error
                .as_deref()
                .unwrap()
                .contains("lanes, bars or orb")
        );
        // Esc leaves the zoom, then the panel; the keys of v0.16 still work.
        app.huddle_key(key(KeyCode::Esc)).await.unwrap();
        assert!(!app.huddle_zoomed());
        assert_eq!(app.presentation.overlay, Some(Overlay::Huddle));
        app.huddle_key(key(KeyCode::Esc)).await.unwrap();
        assert_eq!(app.presentation.overlay, None);
    }

    /// With no call the zoom says what it needs and changes nothing.
    #[tokio::test]
    async fn zoom_without_a_call_says_so() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.huddle_command("zoom").await.unwrap();
        assert!(!app.huddle_zoomed());
        assert!(
            app.status_error
                .as_deref()
                .unwrap()
                .contains("join the huddle first")
        );
        app.huddle_command("style").await.unwrap();
        assert!(
            app.status_error
                .as_deref()
                .unwrap()
                .starts_with("huddle style: lanes")
        );
    }

    /// Rule 20 for the zoomed view: the spectrum is asked of the audio side
    /// only while bars or the orb are seen; frames stop in silence; with
    /// motion off, only a change of speaker draws.
    #[tokio::test]
    async fn the_zoomed_view_counts_its_frames() {
        use crate::huddle::synth::DEMO_SCRIPT;
        let count = |app: &mut super::App, from: f64, to: f64| {
            let mut frames = 0;
            let mut t = from;
            while t < to {
                frames += u32::from(app.tick_huddle_viz_at(t));
                t += 0.033;
            }
            frames
        };
        // Bars, zoomed: about 30 a second while people speak (0.35 to 9.6 s,
        // and the bars' fall), none once the call is quiet.
        let mut app = zoom_test_app(&TempDir::new().unwrap(), &DEMO_SCRIPT).await;
        app.config.ui.huddle_style = crate::config::HuddleStyle::Bars;
        app.tick_huddle_viz_at(0.0);
        app.huddle_command("zoom").await.unwrap();
        let speaking = count(&mut app, 0.0, 13.0);
        let mix = app.huddle_feed().unwrap().levels;
        assert!(mix.mix().is_wanted(), "the bars ask for the mix");
        // Two seconds after the meters fall quiet, nothing more.
        let quiet = count(&mut app, 13.0, 20.0);
        assert!((250..=370).contains(&speaking), "{speaking}");
        assert_eq!(quiet, 0, "no frame in silence");
        // The zoom hidden: the mix is no longer asked for, nor analysed.
        app.huddle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await
            .unwrap();
        app.tick_huddle_viz_at(20.1);
        assert!(!mix.mix().is_wanted());
        // A call that never speaks: the first frame only.
        let mut app = zoom_test_app(&TempDir::new().unwrap(), &[]).await;
        app.config.ui.huddle_style = crate::config::HuddleStyle::Orb;
        assert!(
            app.tick_huddle_viz_at(0.0),
            "the first frame shows the call"
        );
        app.huddle_command("zoom").await.unwrap();
        assert_eq!(count(&mut app, 0.0, 10.0), 0);
        // Motion off: a still picture, a frame when the speaker changes.
        for style in crate::config::HuddleStyle::ALL {
            let mut app = zoom_test_app(&TempDir::new().unwrap(), &DEMO_SCRIPT).await;
            app.config.ui.motion = crate::config::Motion::Off;
            app.config.ui.huddle_style = style;
            app.tick_huddle_viz_at(0.0);
            app.huddle_command("zoom").await.unwrap();
            let off = count(&mut app, 0.0, 12.0);
            // Start, Ada, quiet, Lin, Lin and Ada, Ada, quiet, your mic,
            // quiet, Sam, quiet: about eleven.
            assert!((5..=14).contains(&off), "{style:?}: {off}");
        }
        // The orb drops its rate when frames are slow to write.
        let mut app = zoom_test_app(&TempDir::new().unwrap(), &DEMO_SCRIPT).await;
        app.config.ui.huddle_style = crate::config::HuddleStyle::Orb;
        app.tick_huddle_viz_at(0.0);
        app.huddle_command("zoom").await.unwrap();
        assert_eq!(app.huddle_interval(crate::config::Motion::Full), Some(0.05));
        for _ in 0..3 {
            app.huddle_frame_drawn(0.04);
        }
        assert_eq!(app.huddle_interval(crate::config::Motion::Full), Some(0.1));
        let full = count(&mut app, 1.0, 2.0);
        assert!((8..=11).contains(&full), "ten frames a second: {full}");
    }

    /// Plan B7: the join confirmation shows your lane before anything is
    /// sent, from a fake device; three seconds of nothing names the device.
    #[tokio::test]
    async fn the_join_confirmation_checks_the_microphone() {
        let temporary = TempDir::new().unwrap();
        let mut app = attachment_test_app(&temporary).await;
        app.huddle_viz.look = crate::ui::huddle_lanes::Look::default();
        app.presentation.open_overlay(Overlay::Huddle);
        app.presentation
            .open_confirmation(crate::ui::state::ConfirmationKind::HuddleJoin);
        app.start_mic_check();
        let levels = app.huddle_viz.mic_check.as_ref().unwrap().check.levels();
        let mut frames = 0;
        let mut t = 0.0;
        while t < 1.0 {
            levels.publish_mic(&crate::huddle::synth::tone(220.0, -22.0, 0, 960));
            frames += u32::from(app.tick_huddle_viz_at(t));
            t += 0.033;
        }
        assert!(frames > 20, "your lane moves: {frames}");
        let screen = zoom_screen(&mut app, 80, 24);
        assert!(screen.contains("join the huddle?"), "{screen}");
        assert!(
            screen.contains("nothing is sent until you join"),
            "{screen}"
        );
        assert!(screen.contains("your microphone works"), "{screen}");
        assert!(screen.contains("y join"), "{screen}");
        assert_eq!(app.huddle_call_state(), None, "nothing joined");
        app.stop_mic_check();
        // A device that gives nothing.
        app.config.ui.huddle_input_device = Some("USB Mic".into());
        app.start_mic_check();
        let mut t = 10.0;
        while t < 13.2 {
            app.tick_huddle_viz_at(t);
            t += 0.033;
        }
        let screen = zoom_screen(&mut app, 80, 24);
        assert!(screen.contains("no sound from “USB Mic”"), "{screen}");
        if let Some(directory) = std::env::var_os("BZZ_VIZ_CAPTURES") {
            std::fs::write(
                std::path::Path::new(&directory).join("mic-check-80x24.txt"),
                &screen,
            )
            .unwrap();
        }
        app.stop_mic_check();
        assert!(app.huddle_viz.mic_check.is_none());
        // A confirmation closed any other way ends the check too.
        app.start_mic_check();
        app.presentation.close_overlay();
        assert!(!app.tick_huddle_viz_at(14.0));
        assert!(app.huddle_viz.mic_check.is_none());
    }

    /// Captures of the zoomed view in the real `App::render`, with the
    /// generated huddle, to `BZZ_VIZ_CAPTURES` when set.
    #[tokio::test]
    async fn zoomed_view_captures() {
        let temporary = TempDir::new().unwrap();
        let mut app = zoom_test_app(&temporary, &crate::huddle::synth::DEMO_SCRIPT).await;
        app.tick_huddle_viz_at(0.0);
        app.huddle_command("zoom").await.unwrap();
        for style in crate::config::HuddleStyle::ALL {
            app.config.ui.huddle_style = style;
            app.huddle_viz.spectrum.clear();
            let mut t = 0.0;
            while t <= 5.5 {
                app.tick_huddle_viz_at(t);
                t += 1.0 / 30.0;
            }
            for (width, height) in [(80, 24), (160, 45)] {
                let screen = zoom_screen(&mut app, width, height);
                assert!(screen.contains("Ada and Lin at once"), "{screen}");
                if let Some(directory) = std::env::var_os("BZZ_VIZ_CAPTURES") {
                    std::fs::write(
                        std::path::Path::new(&directory)
                            .join(format!("zoom-{}-{width}x{height}.txt", style.name())),
                        &screen,
                    )
                    .unwrap();
                }
            }
            app.huddle_viz.generated.as_mut().unwrap().source =
                crate::huddle::synth::Synthetic::new(&crate::huddle::synth::DEMO_SCRIPT, None);
            app.huddle_viz.meter.clear();
        }
    }

    /// Counts what a frame writes to the terminal.
    #[derive(Clone, Default)]
    struct CountingWriter(Arc<std::sync::atomic::AtomicUsize>);

    impl std::io::Write for CountingWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0
                .fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The cost of each zoomed style in bzz: the real `App`, the generated
    /// huddle looping, the frame tick every 33 ms, a frame drawn through the
    /// crossterm backend whenever the visualizer asks for one; 12 s after
    /// a 1 s warm-up. Prints a table; `--ignored` to run (B6, B8).
    #[tokio::test]
    #[ignore = "a measurement; run with --ignored --nocapture"]
    async fn measure_the_zoomed_styles() {
        use crate::config::HuddleStyle;
        println!("| style | size | frames/s | KiB/s | ms a frame | CPU % of a core |");
        println!("|---|---|---|---|---|---|");
        for (size, zoomed) in [
            ((120_u16, 30_u16), true),
            ((50, 12), true),
            ((120, 30), false),
        ] {
            for style in HuddleStyle::ALL {
                if !zoomed && style != HuddleStyle::Lanes {
                    continue;
                }
                let temporary = TempDir::new().unwrap();
                let mut app = zoom_test_app(&temporary, &[]).await;
                app.set_generated_huddle(
                    crate::huddle::synth::Synthetic::demo(),
                    vec![(1, "Ada".into()), (2, "Lin".into()), (3, "Sam".into())],
                    true,
                );
                app.config.ui.huddle_style = style;
                app.tick_huddle_viz_at(0.0);
                app.presentation.open_overlay(Overlay::Huddle);
                app.huddle_viz.zoomed = zoomed;
                let counter = CountingWriter::default();
                let mut terminal = Terminal::with_options(
                    ratatui::backend::CrosstermBackend::new(counter.clone()),
                    ratatui::TerminalOptions {
                        viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, size.0, size.1)),
                    },
                )
                .unwrap();
                let (mut frames, mut busy, mut drawing) = (0_u32, 0.0_f64, 0.0_f64);
                let mut bytes_at_start = 0;
                let mut t = 0.0;
                while t < 13.0 {
                    let started = Instant::now();
                    let wants = app.tick_huddle_viz_at(t);
                    if wants || t == 0.0 {
                        let drawn = Instant::now();
                        terminal.draw(|frame| app.render(frame)).unwrap();
                        let seconds = drawn.elapsed().as_secs_f64();
                        app.huddle_frame_drawn(seconds);
                        if t >= 1.0 {
                            frames += 1;
                            drawing += seconds;
                        }
                    }
                    if t >= 1.0 {
                        busy += started.elapsed().as_secs_f64();
                    } else {
                        bytes_at_start = counter.0.load(std::sync::atomic::Ordering::Relaxed);
                    }
                    t += 0.033;
                }
                let bytes = counter.0.load(std::sync::atomic::Ordering::Relaxed) - bytes_at_start;
                println!(
                    "| {}{} | {}×{} | {:.1} | {:.1} | {:.2} | {:.2} |",
                    style.name(),
                    if zoomed { "" } else { " (panel, not zoomed)" },
                    size.0,
                    size.1,
                    f64::from(frames) / 12.0,
                    bytes as f64 / 12.0 / 1024.0,
                    drawing * 1000.0 / f64::from(frames.max(1)),
                    busy / 12.0 * 100.0,
                );
            }
        }
    }
}
