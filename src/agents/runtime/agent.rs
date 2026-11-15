//! One local agent: its harness process, its sessions and its turns.
//!
//! The rules this file keeps (ADR v0.15 local agents):
//!
//! - **One final reply per request.** A final `send` is signed, stored under
//!   the request in the ledger, and only then published. The request comes
//!   from the calling session's live turn, never from the call's arguments.
//!   The stored reply is never replaced: it is published again unchanged
//!   until the relay answers, and the relay's refusal ends the request at
//!   once, with nothing more signed for it.
//! - **Only where the agent may write.** Before anything is published as
//!   the agent (a reply, a stored reply again, a message of a turn, a
//!   reaction, a typing indicator) bzz asks the relay's own records whether
//!   the agent is still a member of that channel and may write there. If
//!   not, nothing is published, then or later, and a mention is not handed
//!   to the harness at all.
//! - **A turn always ends in one of four ways:** a final `send`; the
//!   harness's last plain text, when it never called `send`; something said
//!   without a final reply; or a failure notice named by kind.
//! - **After a crash** a request is handed over again only when that cannot
//!   answer twice: the harness can tell a request it has seen (bzz's own
//!   host), or nothing of the turn was published. Otherwise the turn fails
//!   with a notice.
//! - **A harness is not trusted and not waited for.** Every wait on it has a
//!   limit, and a harness that keeps dying is left alone for a while.
//! - **A real command needs a yes.** bzz's own host asks before it starts one
//!   (`_bzz/approve`). bzz answers yes only for a folder the settings grant
//!   for commands, in a turn that is live, and, unless the folder is allowed
//!   without asking, only after the owner said so in the TUI. Anything else
//!   is a no with its reason: nobody to ask, no answer in time, a turn that
//!   was stopped.
//! - **A git credential is given to the host that asks, and to nothing
//!   else** (`_bzz/credential`): for a host on the agent's own allow-list,
//!   from what the owner stored for that agent. It is in no log and no
//!   record.
//! - **A coding harness is guarded by bzz, not by its owner's settings.**
//!   Its sandbox or its mode is set before a session is prompted, and each
//!   thing it asks leave for is answered by rule ([`guard`]).

mod guard;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use nostr::{Event, JsonUtil as _};
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    agents::{
        activity,
        git_credentials::GitCredentials,
        runtime::{
            FailureKind, Limits,
            acp::{self, CallError, Connection, Incoming, RpcError},
            admission::{self, ChannelFacts, PlaceKind, Refusal, ReplyTarget, Rule, Unwritable},
            approval::{self, Approvals, Asked, NotAsked, Outcome},
            coding,
            credentials::Credentials,
            hold,
            install::{self, StartReport},
            permission as permission_rule,
            process::{self, Harness, Launch},
            prompt::{self, PromptContext},
            record::Recorder,
            relay::{self, AgentRelay, Published},
            tools::{self, ToolCall, ToolReply},
        },
    },
    config::{ExecGrant, Harness as HarnessKind, LocalAgentConfig, WakeRule},
    error::{Error, Result},
    store::{
        agent_runtime::{InboxRow, NewRequest, ReplyOrigin, RequestState},
        writer::StoreHandle,
    },
};

/// The most plain text of a turn that is kept: the reply it may become.
const MAX_PLAIN_TEXT_CHARS: usize = 16_000;
/// Tool calls of one harness that bzz works on at once.
const CONCURRENT_TOOL_CALLS: usize = 8;
/// A stored reply this old and still unacknowledged is published again.
const REPUBLISH_AFTER_SECONDS: u64 = 30;
/// How often a running bzz looks whether another bzz is moving, deleting or
/// pausing its agent.
const HOLD_POLL: Duration = Duration::from_millis(500);
/// How long bzz waits for the relay to take a presence update.
const PRESENCE_LIMIT: Duration = Duration::from_secs(3);

/// What the settings file says of an agent that a running bzz serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsSay {
    Active,
    Paused,
    /// It is no longer in the settings: it was deleted.
    Gone,
}
/// Git credentials of one harness that bzz looks up at once. A git command
/// asks once, so more than a few at a time is a harness gone wrong.
const CONCURRENT_CREDENTIALS: usize = 4;

/// What the runtime needs to know of one agent. Never its key.
#[derive(Clone, Debug)]
pub struct AgentSpec {
    pub id: Uuid,
    pub name: String,
    pub pubkey: String,
    /// The owner's public key (hex).
    pub owner: String,
    pub community: Uuid,
    pub local: LocalAgentConfig,
}

impl AgentSpec {
    /// Whether the harness can tell a request it has already seen. bzz's own
    /// host uses the request id as its idempotency key; the coding harnesses
    /// start a new turn every time.
    pub fn recognises_requests(&self) -> bool {
        self.local.harness == HarnessKind::PiDurable
    }
}

/// Reads one variable of bzz's own environment.
pub type Environment = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// bzz's own host, ready to be started: the Node that runs it and the
/// folder it is installed in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PiHost {
    pub node: PathBuf,
    pub directory: PathBuf,
}

/// What every agent of a host shares.
pub struct Shared {
    pub data_dir: PathBuf,
    pub store: StoreHandle,
    pub credentials: Arc<Credentials>,
    /// Where `bzz agent-tools` reaches bzz, and the bzz it is run from.
    pub tools_address: SocketAddr,
    pub tools_program: PathBuf,
    /// Pi's own configuration folder, passed on to bzz's host (D11).
    pub pi_dir: Option<PathBuf>,
    pub limits: Limits,
    pub environment: Environment,
    /// Found and installed once, at the first start that needs it.
    pub pi_host: tokio::sync::Mutex<Option<PiHost>>,
    /// The real commands that wait for the owner's answer.
    pub approvals: Arc<Approvals>,
    /// What the owner stored for private git remotes. `None`: nothing can
    /// be given.
    pub git_credentials: Option<Arc<dyn GitCredentials>>,
    /// The settings file. An agent runs on the settings bzz started with,
    /// but what the owner has taken away since (a folder for commands, an
    /// allowed host) is refused at once: the file is read again before a
    /// command is allowed or a credential given. `None`: there is no file
    /// to read, and the settings bzz started with are all there is.
    pub settings_file: Option<PathBuf>,
}

impl Shared {
    /// bzz's own host, installed now when this is its first use. One caller
    /// installs; the others wait for it.
    pub async fn pi_host(&self) -> std::result::Result<PiHost, install::Problem> {
        let mut known = self.pi_host.lock().await;
        if let Some(host) = known.as_ref()
            && install::is_installed(&self.data_dir)
        {
            return Ok(host.clone());
        }
        let lookup = |name: &str| (self.environment)(name);
        let node = install::find_node(lookup).await?;
        let directory = install::ensure_installed(&self.data_dir, &node, lookup).await?;
        let host = PiHost {
            node: node.program,
            directory,
        };
        *known = Some(host.clone());
        Ok(host)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// No process; one starts when a request arrives.
    Idle,
    Starting,
    Running,
    /// It died too often; no start is tried until the window has passed.
    Unavailable,
    Paused,
    Stopped,
}

/// One agent's state for a status line. No content, no key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentStatus {
    pub id: Uuid,
    pub name: String,
    pub harness: HarnessKind,
    pub phase: Phase,
    /// Turns running now.
    pub turns: usize,
    pub pid: Option<u32>,
    /// The title of the last tool call a harness reported.
    pub activity: Option<String>,
    pub problem: Option<String>,
}

impl AgentStatus {
    /// Its state in a few words: idle, starting, ready, answering N,
    /// unavailable, paused, stopped.
    pub fn state_text(&self) -> String {
        match self.phase {
            Phase::Idle => "idle".to_owned(),
            Phase::Starting => "starting".to_owned(),
            Phase::Running if self.turns == 0 => "ready".to_owned(),
            Phase::Running => format!("answering {}", self.turns),
            Phase::Unavailable => "unavailable".to_owned(),
            Phase::Paused => "paused".to_owned(),
            Phase::Stopped => "stopped".to_owned(),
        }
    }

    /// What it is doing now: the last tool call of a running turn, or what
    /// is wrong while no turn runs.
    pub fn doing(&self) -> Option<String> {
        if self.turns > 0 {
            self.activity.clone()
        } else {
            self.problem.clone()
        }
    }

    pub fn line(&self) -> String {
        let mut state = self.state_text();
        if self.turns > 0
            && let Some(activity) = &self.activity
        {
            state.push_str(&format!(
                " ({})",
                crate::protocol::artifact::display(activity, 40)
            ));
        }
        let mut line = format!(
            "{} · {} · {state}",
            crate::protocol::artifact::display(&self.name, 32),
            crate::agents::manage::harness_label(self.harness)
        );
        if let Some(problem) = &self.problem
            && self.turns == 0
        {
            line.push_str(&format!(
                " · {}",
                crate::protocol::artifact::display(problem, 80)
            ));
        }
        line
    }
}

/// Stops a task when its owner goes away.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A request being answered, in memory. The ledger is the record; this only
/// says which request a session's tool calls belong to.
struct LiveTurn {
    run: u64,
    row: InboxRow,
    request: Arc<Event>,
    /// A message of this turn other than its final reply was posted.
    sent_any: bool,
    final_sent: bool,
    /// The owner stopped this turn from the agents panel.
    stopped: bool,
    /// bzz refused something the harness asked leave for in this turn.
    refused: bool,
    /// The tool calls a coding harness reported in this turn, by their id:
    /// what a request names when it names only its call.
    calls: HashMap<String, Value>,
    effects: u32,
    /// Tool calls of this turn still at work.
    in_flight: usize,
    /// The plain text of the message being written, and of the one before.
    text: String,
    previous_text: String,
    activity: Option<String>,
    typing: Option<AbortOnDrop>,
}

impl LiveTurn {
    fn last_text(&self) -> String {
        let text = if self.text.trim().is_empty() {
            &self.previous_text
        } else {
            &self.text
        };
        text.trim().chars().take(MAX_PLAIN_TEXT_CHARS).collect()
    }

    fn push_text(&mut self, chunk: &str) {
        let room = MAX_PLAIN_TEXT_CHARS.saturating_sub(self.text.chars().count());
        self.text.extend(chunk.chars().take(room));
    }

    /// Keeps what a harness reported of one of its tool calls, for the
    /// request that may ask leave for it by its id alone.
    fn remember_call(&mut self, update: &Value) {
        let Some(id) = update
            .get("toolCallId")
            .and_then(Value::as_str)
            .filter(|id| id.len() <= 256)
        else {
            return;
        };
        if self.calls.len() >= permission_rule::MAX_CALLS && !self.calls.contains_key(id) {
            return;
        }
        permission_rule::remember(self.calls.entry(id.to_owned()).or_default(), update);
    }

    /// A tool call starts: what follows is a new message.
    fn end_message(&mut self) {
        if !self.text.trim().is_empty() {
            self.previous_text = std::mem::take(&mut self.text);
        }
    }
}

/// One start of the harness.
struct Run {
    id: u64,
    connection: Connection,
    /// bzz's own host: it asks for tools over the pipe and needs no server.
    pipe_tools: bool,
    load_session: bool,
    /// How a coding harness takes a system prompt and extra folders.
    offer: coding::Offer,
    /// The transcript of this process, when one is being recorded.
    recorder: Option<Arc<Recorder>>,
    pid: Option<u32>,
    harness: tokio::sync::Mutex<Option<Harness>>,
    /// Place -> session id, for this process.
    sessions: Mutex<HashMap<String, String>>,
    /// Stopped on purpose: its end is not a crash.
    deliberate: AtomicBool,
    dead: AtomicBool,
    tool_calls: Arc<tokio::sync::Semaphore>,
    credentials: Arc<tokio::sync::Semaphore>,
}

struct State {
    /// Session id -> the turn running in it.
    turns: HashMap<String, LiveTurn>,
    busy_places: HashSet<String>,
    crashes: VecDeque<Instant>,
    phase: Phase,
    problem: Option<String>,
    pid: Option<u32>,
}

pub struct Agent {
    pub spec: AgentSpec,
    relay: Arc<dyn AgentRelay>,
    shared: Arc<Shared>,
    state: Mutex<State>,
    run: tokio::sync::Mutex<Option<Arc<Run>>>,
    next_run: AtomicU64,
    /// Counts turns started and ended: an idle checkpoint is sent only if
    /// nothing happened since it was planned.
    activity: AtomicU64,
    wake: Notify,
    turn_changed: Notify,
    stopping: AtomicBool,
    paused: AtomicBool,
    /// A `bzz agent move` or `bzz agent delete` in another process holds the
    /// agent ([`hold`]): no process, no listening, until it lets go.
    held: AtomicBool,
    /// The settings file says the agent is paused: `bzz agent pause` in
    /// another process. No process, no listening, until it says otherwise.
    suspended: AtomicBool,
    /// This bzz let the agent go for good (moved away, or deleted): its
    /// connection is closed and its key forgotten.
    retired: AtomicBool,
    /// Wakes the task that says the agent is present.
    presence_changed: Notify,
    /// When this agent's rows were last pruned.
    pruned: Mutex<Option<Instant>>,
    /// Whether the agent might write in a channel by the relay's records
    /// as they came with the last request from it, and when that was. Used
    /// only to decide whether a request is handed to the harness while the
    /// relay cannot be asked again; never to publish.
    admitted: Mutex<HashMap<Uuid, (Instant, bool)>>,
}

/// The kind a request fails with when the relay refuses its reply.
pub const REPLY_REFUSED: &str = "reply-refused";
/// The kind a request fails with when its stored reply was never accepted
/// in the time a message can be a request at all.
pub const REPLY_UNDELIVERED: &str = "reply-undelivered";
/// How long bzz waits for the relay's records of a channel before it
/// publishes there.
const STANDING_LIMIT: Duration = Duration::from_secs(10);
/// For how long the relay's records that came with a request may decide
/// that it is handed to the harness, while the relay cannot be asked.
const ADMITTED_FRESH: Duration = Duration::from_secs(60);

/// What became of a reply offered to the ledger.
enum Reply {
    /// The relay accepted it.
    Sent(String),
    /// The request already had its final reply. `refused`: one that will
    /// never be published.
    Already { id: String, refused: bool },
    /// Stored; the relay has not accepted it yet.
    Pending(String),
    /// The relay refused it. It stays the request's one reply, the request
    /// is over, and nothing more is signed for it.
    Refused(String),
    /// Nothing was signed: the agent may not write in that place.
    Withheld(Unwritable),
    /// It could not be signed or stored.
    Failed,
}

/// Whether the agent may write in a channel, by the relay's own records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Standing {
    Writer,
    Not(Unwritable),
    /// The relay could not be asked just now. Nothing is published on a
    /// guess.
    Unknown,
}

/// Answers `session/request_permission` for a harness that is not a coding
/// harness: bzz's own chat tools are allowed, once; anything else is
/// refused. The answer names an option by its kind, never by a fixed id,
/// and never one that the harness would remember ("always"). A coding
/// harness is answered by [`Agent::permission_request`].
pub fn permission(params: &Value) -> Value {
    permission_rule::answer(
        params,
        permission_rule::is_chat_tool(params.get("toolCall").unwrap_or(&Value::Null)),
    )
}

impl Agent {
    pub fn new(spec: AgentSpec, relay: Arc<dyn AgentRelay>, shared: Arc<Shared>) -> Arc<Self> {
        Arc::new(Self {
            spec,
            relay,
            shared,
            state: Mutex::new(State {
                turns: HashMap::new(),
                busy_places: HashSet::new(),
                crashes: VecDeque::new(),
                phase: Phase::Idle,
                problem: None,
                pid: None,
            }),
            run: tokio::sync::Mutex::new(None),
            next_run: AtomicU64::new(1),
            activity: AtomicU64::new(0),
            wake: Notify::new(),
            turn_changed: Notify::new(),
            stopping: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            held: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            presence_changed: Notify::new(),
            pruned: Mutex::new(None),
            admitted: Mutex::new(HashMap::new()),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn limits(&self) -> &Limits {
        &self.shared.limits
    }

    fn halted(&self) -> bool {
        self.stopping.load(Ordering::Acquire) || self.resting()
    }

    /// Stopped for now by its owner or by another bzz: paused here, paused
    /// in the settings, or held for a move or a deletion.
    fn resting(&self) -> bool {
        self.paused.load(Ordering::Acquire)
            || self.held.load(Ordering::Acquire)
            || self.suspended.load(Ordering::Acquire)
    }

    /// Whether this bzz let the agent go for good.
    pub fn retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }

    async fn store<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut crate::store::Store, Uuid) -> Result<T> + Send + 'static,
    {
        let agent = self.spec.id;
        self.shared
            .store
            .call(move |store| operation(store, agent))
            .await
    }

    pub fn status(&self) -> AgentStatus {
        let state = self.state();
        AgentStatus {
            id: self.spec.id,
            name: self.spec.name.clone(),
            harness: self.spec.local.harness,
            phase: state.phase,
            turns: state.turns.len(),
            pid: state.pid,
            activity: state.turns.values().find_map(|turn| turn.activity.clone()),
            problem: state.problem.clone(),
        }
    }

    // ---- admission -------------------------------------------------------

    /// Whether the agent takes requests: not while paused or stopping.
    pub fn listening(&self) -> bool {
        !self.halted()
    }

    /// Admits `event` as a request when its author may wake the agent.
    pub async fn offer(
        &self,
        event: &Event,
        channel: &ChannelFacts,
    ) -> std::result::Result<bool, Refusal> {
        let admitted = admission::admit(
            event,
            Rule {
                agent: &self.spec.pubkey,
                owner: &self.spec.owner,
                wake: self.spec.local.wake,
                allowlist: &self.spec.local.wake_allowlist,
            },
            channel,
        )?;
        self.admitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                channel.id,
                (Instant::now(), channel.may_write(&self.spec.pubkey).is_ok()),
            );
        // Another agent's message wakes this one only so often in one
        // conversation: two agents that may wake each other cannot answer
        // each other for ever. Its owner and other people are not counted.
        if admission::written_by_an_agent(event)
            && !admitted.author.eq_ignore_ascii_case(&self.spec.owner)
        {
            let (place, author) = (admitted.place.clone(), admitted.author.clone());
            let since = unix_now().saturating_sub(self.limits().agent_exchange_window.as_secs());
            let woken = self
                .store(move |store, agent| {
                    store.agent_inbox_recent_from(agent, &place, &author, since)
                })
                .await
                // In doubt it is not admitted: a person can always ask again.
                .unwrap_or(u32::MAX);
            if woken >= self.limits().agent_exchanges {
                self.state().problem = Some(
                    "it stopped answering another agent in one conversation for now: they may be answering each other in a loop".into(),
                );
                return Err(Refusal::AgentExchangeLimit);
            }
        }
        let request = NewRequest {
            agent_id: self.spec.id,
            request_id: admitted.request_id,
            community_id: self.spec.community,
            channel_id: admitted.channel,
            place: admitted.place,
            author: admitted.author,
            created_at: admitted.created_at,
            event_json: event.as_json(),
        };
        let fresh = self
            .shared
            .store
            .call(move |store| store.agent_inbox_offer(&request))
            .await
            .unwrap_or(false);
        self.wake.notify_one();
        Ok(fresh)
    }

    // ---- lifecycle -------------------------------------------------------

    /// Settles what an earlier bzz or an earlier process left open. Runs
    /// before any harness is started.
    pub async fn recover(&self) {
        // Held for a move or a deletion by another bzz: nothing is
        // published and nothing decided until that is over.
        if hold::read(&self.shared.data_dir, self.spec.id).is_some() {
            return;
        }
        self.republish(0).await;
        self.prune().await;
        let Ok(open) = self
            .store(|store, agent| store.agent_inbox_in_state(agent, RequestState::Delivered))
            .await
        else {
            return;
        };
        for row in open {
            if self.state().busy_places.contains(&row.place) {
                continue;
            }
            self.after_crash(&row).await;
        }
    }

    /// Removes this agent's finished requests and idle sessions by the rule
    /// of [`crate::store::Store::agent_prune`], at most once per interval.
    async fn prune(&self) {
        {
            let mut pruned = self
                .pruned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pruned.is_some_and(|at| at.elapsed() < self.limits().prune_interval) {
                return;
            }
            *pruned = Some(Instant::now());
        }
        let now = unix_now();
        let _ = self
            .store(move |store, agent| store.agent_prune(agent, now))
            .await;
    }

    /// Starts handing queued requests to the harness.
    pub fn start(self: &Arc<Self>) {
        // From here this bzz holds the agent's key and connection, and says
        // so where a `bzz agent delete` in another process looks.
        let _ = hold::serve(&self.shared.data_dir, self.spec.id);
        tokio::spawn(self.clone().schedule());
        tokio::spawn(self.clone().watch());
        tokio::spawn(self.clone().say_present());
        self.wake.notify_one();
    }

    /// What the settings file says of this agent now, when it says anything
    /// a running bzz acts on.
    fn settings_say(&self) -> Option<SettingsSay> {
        let path = self.shared.settings_file.as_ref()?;
        // Unreadable just now: nothing is concluded from that.
        let config = crate::config::load_from(path).ok()?;
        Some(
            match config
                .hosted_agents
                .iter()
                .find(|agent| agent.id == self.spec.id)
            {
                // No longer an agent of this bzz at all: it was deleted.
                None => SettingsSay::Gone,
                // There, and not as an agent bzz runs: nothing a `bzz
                // agent` command writes. Nothing is concluded from it.
                Some(agent) if agent.local.is_none() => return None,
                Some(agent) if agent.runs_locally() => SettingsSay::Active,
                // Moved away is the business of `move.json`.
                Some(agent)
                    if agent.local.as_ref().is_some_and(|local| {
                        local.state == crate::config::LocalAgentState::MovedAway
                    }) =>
                {
                    return None;
                }
                Some(_) => SettingsSay::Paused,
            },
        )
    }

    /// Lets the agent go for good: its process is stopped, it says that it
    /// is gone, its connection is closed and its key forgotten. Then, and
    /// only then, this bzz no longer counts as one that serves it.
    async fn retire(&self, why: &'static str) {
        self.stop().await;
        self.state().problem = Some(why.into());
        self.relay.close().await;
        self.retired.store(true, Ordering::Release);
        hold::let_go(&self.shared.data_dir, self.spec.id);
    }

    /// Follows what another bzz process does to the agent, twice a second:
    ///
    /// - the agent's `move.json` ([`hold`]), which a `bzz agent move` or a
    ///   `bzz agent delete` writes. While one is at work the agent has no
    ///   process and takes no request; when it fails the agent runs again,
    ///   and when it succeeds this bzz never runs the agent again;
    /// - the settings file, when it changed: `bzz agent pause` and `bzz
    ///   agent resume` hold at once, and an agent that is no longer in the
    ///   settings is let go.
    async fn watch(self: Arc<Self>) {
        let mut tick = tokio::time::interval(HOLD_POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut settings_seen = None;
        loop {
            tick.tick().await;
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            let held = hold::read(&self.shared.data_dir, self.spec.id);
            if let Some(hold) = &held {
                if !hold.passes() {
                    // It is another home's agent now, or nobody's: this bzz
                    // keeps neither a connection as the agent nor its key
                    // in memory.
                    self.retire(hold.words()).await;
                    break;
                }
                if !self.held.swap(true, Ordering::AcqRel) {
                    self.halt(Phase::Stopped).await;
                    self.state().problem = Some(hold.words().into());
                }
            }
            // The settings, read again only when the file changed.
            let stamp = self.shared.settings_file.as_ref().and_then(|path| {
                let file = std::fs::metadata(path).ok()?;
                Some((file.modified().ok()?, file.len()))
            });
            let mut resumed = false;
            if stamp.is_some() && stamp != settings_seen {
                settings_seen = stamp;
                match self.settings_say() {
                    Some(SettingsSay::Gone) => {
                        self.retire(hold::DELETED).await;
                        break;
                    }
                    Some(SettingsSay::Paused) => {
                        if !self.suspended.swap(true, Ordering::AcqRel) {
                            self.halt(Phase::Paused).await;
                            self.state().problem =
                                Some("paused by its owner, with `bzz agent pause`".into());
                        }
                    }
                    Some(SettingsSay::Active) => {
                        resumed |= self.suspended.swap(false, Ordering::AcqRel);
                    }
                    None => {}
                }
            }
            if held.is_none() {
                resumed |= self.held.swap(false, Ordering::AcqRel);
            }
            if resumed {
                self.rest_or_run().await;
            }
        }
    }

    /// After something that stopped the agent for a while is over: it runs
    /// again unless something else still stops it.
    async fn rest_or_run(&self) {
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        let resting = self.resting();
        {
            let mut state = self.state();
            let paused =
                self.paused.load(Ordering::Acquire) || self.suspended.load(Ordering::Acquire);
            if !resting {
                state.problem = None;
                state.phase = Phase::Idle;
            } else if paused && !self.held.load(Ordering::Acquire) {
                state.phase = Phase::Paused;
            }
        }
        if !resting {
            // As after a pause: what was open is settled, and what is
            // queued is handed over.
            self.recover().await;
            self.wake.notify_one();
            self.presence_changed.notify_one();
        }
    }

    /// Says on the relay, with the agent's own key, that the agent is
    /// present (kind 20001, `online`), now and at every interval for as
    /// long as this bzz runs it. Buzz shows an agent as online from this;
    /// without it a bzz agent was "Offline" while it answered. Nothing is
    /// said while the agent is paused, held or stopped: [`Agent::halt`] has
    /// said `offline` then.
    async fn say_present(self: Arc<Self>) {
        loop {
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            if !self.resting() {
                self.say(crate::protocol::presence::Presence::Online).await;
            }
            tokio::select! {
                () = tokio::time::sleep(self.limits().presence_interval) => {}
                () = self.presence_changed.notified() => {}
            }
        }
    }

    /// One presence update as the agent, waited for only briefly: it is
    /// ephemeral, and the next one says the same.
    async fn say(&self, presence: crate::protocol::presence::Presence) {
        let said = async {
            let builder = buzz_sdk::build_presence_update(presence.as_str()).ok()?;
            let event = self.relay.sign(builder).await.ok()?;
            Some(self.relay.publish(event).await)
        };
        let _ = tokio::time::timeout(PRESENCE_LIMIT, said).await;
    }

    async fn schedule(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = self.wake.notified() => {}
                _ = tick.tick() => {}
            }
            if self.stopping.load(Ordering::Acquire) {
                break;
            }
            if self.resting() {
                continue;
            }
            self.republish(REPUBLISH_AFTER_SECONDS).await;
            self.prune().await;
            let Ok(queued) = self
                .store(|store, agent| store.agent_inbox_in_state(agent, RequestState::Queued))
                .await
            else {
                continue;
            };
            for row in queued {
                let unavailable = {
                    let mut state = self.state();
                    // One turn per place: a request for a busy conversation
                    // waits here and becomes the next prompt of the same
                    // session when the first ends.
                    if state.busy_places.contains(&row.place)
                        || state.busy_places.len() >= self.limits().concurrent_turns
                    {
                        continue;
                    }
                    let unavailable = self.breaker_open(&mut state);
                    if !unavailable {
                        state.busy_places.insert(row.place.clone());
                    }
                    unavailable
                };
                if unavailable {
                    self.fail(&row, FailureKind::Unavailable).await;
                } else {
                    tokio::spawn(self.clone().run_turn(row));
                }
            }
        }
    }

    /// Whether the harness died too often lately to be started again.
    fn breaker_open(&self, state: &mut State) -> bool {
        let window = self.limits().crash_window;
        while state
            .crashes
            .front()
            .is_some_and(|at| at.elapsed() > window)
        {
            state.crashes.pop_front();
        }
        let open = state.crashes.len() >= self.limits().max_crashes;
        if open && state.turns.is_empty() {
            state.phase = Phase::Unavailable;
        }
        open
    }

    /// What to start for this agent. bzz's own host is `node <host
    /// folder>/main.mjs <agent folder>`, installed first when this is its
    /// first use; `command` in the settings replaces the program, for a
    /// harness run from a checkout.
    async fn launch(&self) -> std::result::Result<Launch, String> {
        let directory = process::agent_dir(&self.shared.data_dir, self.spec.id);
        process::create_private_dir(&directory).map_err(|error| error.to_string())?;
        let local = &self.spec.local;
        let own_host = local.harness == HarnessKind::PiDurable;
        // bzz's own host is told where Pi's folder is at every start: its
        // own home is the agent's folder, where Pi has nothing.
        if own_host && self.shared.pi_dir.is_none() {
            return Err(
                "Pi's folder could not be found, so its host would have no sign-in: set PI_CODING_AGENT_DIR"
                    .into(),
            );
        }
        // What a coding harness's own launch adds to its environment: where
        // its adapter finds the owner's program.
        let mut launch_env = Vec::new();
        let (program, mut args) = match local.command.split_first() {
            Some((program, arguments)) => (program.clone(), arguments.to_vec()),
            None if own_host => {
                if !install::is_installed(&self.shared.data_dir) {
                    self.state().problem =
                        Some("installing the Pi Durable host (first use)".into());
                }
                let host = self
                    .shared
                    .pi_host()
                    .await
                    .map_err(|problem| problem.to_string())?;
                (
                    host.node.display().to_string(),
                    vec![host.directory.join("main.mjs").display().to_string()],
                )
            }
            // The owner's own program: found, held to the versions bzz
            // knows to work, and for Codex and Claude Code started through
            // its adapter, which is installed first when this is its first
            // use.
            None => {
                let Some(spec) = coding::spec(local.harness) else {
                    return Err("this harness has no program of its own".into());
                };
                if spec
                    .adapter
                    .is_some_and(|adapter| !adapter.is_installed(&self.shared.data_dir))
                {
                    self.state().problem = Some(format!(
                        "installing the adapter of {} (first use)",
                        spec.title
                    ));
                }
                let ready = coding::prepare(
                    &self.shared.data_dir,
                    spec,
                    |name: &str| (self.shared.environment)(name),
                    &|_| {},
                )
                .await
                .map_err(|problem| problem.to_string())?;
                let start = ready.start();
                launch_env = start.env;
                (start.program, start.args)
            }
        };
        // bzz's own host works in the agent's folder and nowhere else; a
        // coding harness gets a working folder inside it and the owner's
        // home, where its own sign-in is.
        let (cwd, home) = if own_host {
            args.push(directory.display().to_string());
            (directory.clone(), directory.clone())
        } else {
            let home = directories::BaseDirs::new()
                .map_or_else(|| directory.clone(), |dirs| dirs.home_dir().to_path_buf());
            (directory.join("workspace"), home)
        };
        let mut env = process::base_environment(&home, |name| (self.shared.environment)(name));
        if !own_host {
            // Its guard is in its environment whatever starts it, and for
            // Grok in a file of its working folder, written now.
            process::create_private_dir(&cwd).map_err(|error| error.to_string())?;
            env.extend(launch_env);
            env.extend(
                coding::environment(
                    local.harness,
                    local,
                    &cwd,
                    &self.shared.data_dir,
                    |name: &str| (self.shared.environment)(name),
                )
                .map_err(|problem| problem.to_string())?,
            );
        }
        Ok(Launch {
            program,
            args,
            env,
            cwd,
            log: directory.join("harness.log"),
        })
    }

    fn initialize_params(&self) -> Value {
        let local = &self.spec.local;
        let mut bzz = json!({
            "agent": {"id": self.spec.id, "pubkey": self.spec.pubkey, "name": self.spec.name},
            "instructions": local.instructions,
            "mounts": local.folders.iter().map(|folder| json!({
                "name": folder.name,
                "path": folder.path,
                "write": folder.write,
                // Whether the host offers its `run` tool there. Whether the
                // owner is asked about a command is bzz's alone to know.
                "exec": folder.exec.granted(),
            })).collect::<Vec<_>>(),
            "network": local.network,
            "packages": local.packages,
        });
        if let Some(model) = &local.model {
            bzz["model"] = json!({"provider": model.provider, "modelId": model.model_id});
            if let Some(level) = &model.thinking_level {
                bzz["model"]["thinkingLevel"] = json!(level);
            }
        }
        if let Some(pi_dir) = &self.shared.pi_dir {
            bzz["piDir"] = json!(pi_dir);
        }
        // Only `false` means something to the host: no Pi package's code runs.
        if !local.packages {
            bzz["packages"] = json!(false);
        }
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {"name": "bzz", "version": env!("CARGO_PKG_VERSION")},
            "_meta": {"bzz": bzz},
        })
    }

    async fn start_run(self: &Arc<Self>) -> std::result::Result<Arc<Run>, String> {
        let launch = self.launch().await?;
        // Read at the last moment before the process exists: a move that
        // began while the host was being found or installed is seen here.
        if let Some(hold) = hold::read(&self.shared.data_dir, self.spec.id) {
            return Err(hold.words().to_owned());
        }
        let run_dir = process::run_dir(&self.shared.data_dir);
        let (harness, stdin, stdout) =
            Harness::spawn(&launch, &run_dir, self.spec.id).map_err(|error| error.to_string())?;
        let recorder = self.recorder(&launch);
        let tap = recorder.clone().map(|recorder| {
            Arc::new(move |outgoing: bool, line: &str| recorder.line(outgoing, line)) as acp::Tap
        });
        let (connection, incoming) = Connection::start_tapped(stdout, stdin, tap);
        let answer = tokio::time::timeout(
            self.limits().start,
            connection.request("initialize", self.initialize_params()),
        )
        .await;
        let problem = match &answer {
            Ok(Ok(response)) if response.get("protocolVersion") == Some(&json!(1)) => None,
            Ok(Ok(_)) => Some("its harness speaks another version of the protocol".to_owned()),
            // bzz's own host says what it refused, such as a granted
            // folder it cannot take. Its words stay on this machine.
            Ok(Err(CallError::Rpc(error)))
                if self.spec.local.harness == HarnessKind::PiDurable
                    && error.code == acp::INVALID_PARAMS =>
            {
                Some(format!(
                    "its harness refused its settings: {}",
                    crate::protocol::artifact::display(&error.message, 200)
                ))
            }
            Ok(Err(CallError::Rpc(_))) => Some("its harness refused to start".to_owned()),
            Ok(Err(_)) => Some("its harness exited while starting".to_owned()),
            Err(_) => Some("its harness did not answer in time".to_owned()),
        };
        if let Some(problem) = problem {
            let closing = connection.clone();
            harness
                .stop(move || closing.close_input(), self.limits().stop_grace)
                .await;
            return Err(problem);
        }
        let response = answer
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        let pipe_tools =
            response.pointer("/_meta/bzz/tools").and_then(Value::as_str) == Some("pipe");
        if pipe_tools {
            // What the host says it loaded and where it offers `run`, kept
            // for `agent show` and `agent doctor`.
            StartReport::from_initialize(&response, self.spec.local.packages)
                .write(&self.shared.data_dir, self.spec.id);
        }
        let run = Arc::new(Run {
            id: self.next_run.fetch_add(1, Ordering::Relaxed),
            pipe_tools,
            load_session: response.pointer("/agentCapabilities/loadSession") == Some(&json!(true)),
            offer: coding::Offer::from_initialize(&response),
            recorder,
            pid: harness.pid(),
            connection,
            harness: tokio::sync::Mutex::new(Some(harness)),
            sessions: Mutex::new(HashMap::new()),
            deliberate: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            tool_calls: Arc::new(tokio::sync::Semaphore::new(CONCURRENT_TOOL_CALLS)),
            credentials: Arc::new(tokio::sync::Semaphore::new(CONCURRENT_CREDENTIALS)),
        });
        tokio::spawn(self.clone().listen(run.clone(), incoming));
        Ok(run)
    }

    /// The running harness, started when there is none.
    async fn ensure_run(self: &Arc<Self>) -> Option<Arc<Run>> {
        let mut slot = self.run.lock().await;
        if let Some(run) = slot
            .as_ref()
            .filter(|run| !run.dead.load(Ordering::Acquire))
        {
            return Some(run.clone());
        }
        if self.halted() || self.breaker_open(&mut self.state()) {
            return None;
        }
        self.state().phase = Phase::Starting;
        match self.start_run().await {
            Ok(run) => {
                let mut state = self.state();
                state.phase = Phase::Running;
                state.problem = None;
                state.pid = run.pid;
                *slot = Some(run.clone());
                Some(run)
            }
            Err(problem) => {
                let mut state = self.state();
                // Held for a move or a deletion: nothing was started, so
                // nothing crashed.
                if ![
                    hold::MOVING,
                    hold::DELETING,
                    hold::DELETED,
                    hold::MOVED_AWAY,
                ]
                .contains(&problem.as_str())
                {
                    state.crashes.push_back(Instant::now());
                }
                state.problem = Some(problem);
                state.phase = Phase::Idle;
                self.breaker_open(&mut state);
                None
            }
        }
    }

    /// Stops a harness: end of input, TERM, KILL. `listen` does the rest
    /// when the connection closes.
    async fn stop_run(&self, run: &Run, deliberate: bool) {
        if deliberate {
            run.deliberate.store(true, Ordering::Release);
        }
        if let Some(harness) = run.harness.lock().await.take() {
            let connection = run.connection.clone();
            harness
                .stop(move || connection.close_input(), self.limits().stop_grace)
                .await;
        }
    }

    async fn listen(
        self: Arc<Self>,
        run: Arc<Run>,
        mut incoming: tokio::sync::mpsc::Receiver<Incoming>,
    ) {
        while let Some(message) = incoming.recv().await {
            match message {
                Incoming::Notification { method, params } => {
                    // Notifications bzz does not know (`_x.ai/…`) are ignored.
                    if method == "session/update" {
                        self.on_update(&run, &params);
                    }
                }
                Incoming::Request { id, method, params } => match method.as_str() {
                    "_bzz/tool" => {
                        // At most a few at once: more wait in the pipe.
                        let Ok(permit) = run.tool_calls.clone().acquire_owned().await else {
                            break;
                        };
                        let (agent, run) = (self.clone(), run.clone());
                        tokio::spawn(async move {
                            let reply = match (
                                params.get("sessionId").and_then(Value::as_str),
                                params.get("name").and_then(Value::as_str),
                            ) {
                                (Some(session), Some(name)) => {
                                    agent
                                        .tool_call(
                                            run.id,
                                            session,
                                            name,
                                            params.get("arguments").unwrap_or(&Value::Null),
                                        )
                                        .await
                                }
                                _ => ToolReply::refused(
                                    "`_bzz/tool` needs `sessionId` and `name`. Nothing was done.",
                                ),
                            };
                            let _ = run.connection.respond(id, Ok(reply.to_json()));
                            drop(permit);
                        });
                    }
                    // A real command, put to bzz before it starts. Each waits
                    // in a task of its own, for as long as the owner takes:
                    // the host's other requests are answered meanwhile, in
                    // whatever order they end.
                    "_bzz/approve" if run.pipe_tools => {
                        let (agent, run) = (self.clone(), run.clone());
                        tokio::spawn(async move {
                            let answer = agent.approve(run.id, &params).await;
                            let _ = run.connection.respond(id, Ok(answer));
                        });
                    }
                    "_bzz/credential" if run.pipe_tools => {
                        let Ok(permit) = run.credentials.clone().try_acquire_owned() else {
                            let _ = run.connection.respond(
                                id,
                                Err(RpcError::new(
                                    acp::REFUSED,
                                    "bzz is already looking up credentials for this agent. Try the git command again.",
                                )),
                            );
                            continue;
                        };
                        let (agent, run) = (self.clone(), run.clone());
                        tokio::spawn(async move {
                            let answer = agent.credential(run.id, &params).await;
                            let _ = run.connection.respond(id, answer);
                            drop(permit);
                        });
                    }
                    // A coding harness asks leave for an edit or a command.
                    // A command may wait for the owner, so each request is
                    // answered in a task of its own.
                    "session/request_permission" if self.is_coding(&run) => {
                        let (agent, run) = (self.clone(), run.clone());
                        tokio::spawn(async move {
                            let answer = agent.permission_request(&run, &params).await;
                            let _ = run.connection.respond(id, Ok(answer));
                        });
                    }
                    "session/request_permission" => {
                        let _ = run.connection.respond(id, Ok(permission(&params)));
                    }
                    other => {
                        let _ = run
                            .connection
                            .respond(id, Err(RpcError::method_not_found(other)));
                    }
                },
                Incoming::Closed => break,
            }
        }
        // The harness is gone, or as good as: its output is closed.
        run.dead.store(true, Ordering::Release);
        // Nobody waits for the owner's answer any more.
        self.shared
            .approvals
            .void_agent(self.spec.id, "the agent's process stopped");
        // Its credentials stop resolving before anything else happens. Only
        // this run's: a process started in its place has its own.
        self.shared.credentials.revoke_run(self.spec.id, run.id);
        let deliberate = run.deliberate.load(Ordering::Acquire) || self.halted();
        if let Some(harness) = run.harness.lock().await.take() {
            let connection = run.connection.clone();
            harness
                .stop(move || connection.close_input(), self.limits().stop_grace)
                .await;
        }
        {
            let mut state = self.state();
            if !deliberate {
                state.crashes.push_back(Instant::now());
                state.problem = Some("its harness stopped unexpectedly".into());
            }
            state.pid = None;
            if !matches!(state.phase, Phase::Paused | Phase::Stopped) {
                state.phase = Phase::Idle;
            }
        }
        let mut slot = self.run.lock().await;
        if slot.as_ref().is_some_and(|current| current.id == run.id) {
            *slot = None;
        }
        drop(slot);
        self.wake.notify_one();
    }

    fn on_update(&self, run: &Run, params: &Value) {
        let Some(session) = params.get("sessionId").and_then(Value::as_str) else {
            return;
        };
        let update = params.get("update").unwrap_or(&Value::Null);
        let mut state = self.state();
        let Some(turn) = state
            .turns
            .get_mut(session)
            .filter(|turn| turn.run == run.id)
        else {
            return;
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                // A notice of the program itself is not the model's text.
                if coding::is_model_text(self.spec.local.harness, update)
                    && let Some(text) = update.pointer("/content/text").and_then(Value::as_str)
                {
                    turn.push_text(text);
                }
            }
            Some("tool_call") => {
                turn.end_message();
                turn.activity = update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|title| title.chars().take(80).collect());
                turn.remember_call(update);
            }
            Some("tool_call_update") => {
                if let Some(title) = update.get("title").and_then(Value::as_str) {
                    turn.activity = Some(title.chars().take(80).collect());
                }
                turn.remember_call(update);
            }
            // Kinds bzz does not know are ignored.
            _ => {}
        }
    }

    /// Stops the agent for good: bzz is closing or locking.
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.presence_changed.notify_one();
        self.halt(Phase::Stopped).await;
    }

    async fn halt(&self, phase: Phase) {
        {
            let mut state = self.state();
            state.phase = phase;
            for turn in state.turns.values_mut() {
                turn.typing = None;
            }
        }
        // It is gone from here on, and says so while it still can: a
        // mention would not be answered.
        self.say(crate::protocol::presence::Presence::Offline).await;
        // A command that waits for the owner's answer is not started by an
        // agent that is paused, stopped, or whose bzz is closing.
        self.shared.approvals.void_agent(
            self.spec.id,
            if phase == Phase::Paused {
                "the agent was paused"
            } else {
                "the agent was stopped, or bzz is closing"
            },
        );
        self.wake.notify_one();
        let run = self.run.lock().await.clone();
        if let Some(run) = run {
            self.stop_run(&run, true).await;
        }
    }

    /// Stops every turn that is running now, as the owner asks from the
    /// agents panel: each is cancelled, and ends with a notice that says so.
    /// A harness that does not honour the cancel is stopped. Returns how
    /// many turns were running.
    pub async fn stop_turns(self: &Arc<Self>) -> usize {
        let sessions = {
            let mut state = self.state();
            state
                .turns
                .iter_mut()
                .map(|(session, turn)| {
                    turn.stopped = true;
                    (session.clone(), turn.run)
                })
                .collect::<Vec<_>>()
        };
        if sessions.is_empty() {
            return 0;
        }
        for (session, _) in &sessions {
            self.shared
                .approvals
                .void_session(self.spec.id, session, "its owner stopped the turn");
        }
        let Some(run) = self.run.lock().await.clone() else {
            return sessions.len();
        };
        for (session, _) in sessions.iter().filter(|(_, id)| *id == run.id) {
            let _ = run
                .connection
                .notify("session/cancel", json!({"sessionId": session}));
        }
        let (agent, stopped) = (self.clone(), sessions.clone());
        tokio::spawn(async move {
            tokio::time::sleep(agent.limits().cancel_grace).await;
            let still_running = {
                let state = agent.state();
                stopped.iter().any(|(session, id)| {
                    state
                        .turns
                        .get(session)
                        .is_some_and(|turn| turn.run == *id && turn.stopped)
                })
            };
            if still_running {
                agent.stop_run(&run, true).await;
            }
        });
        sessions.len()
    }

    /// Pauses or resumes the agent. A paused agent has no process.
    pub async fn set_paused(self: &Arc<Self>, paused: bool) {
        if self.paused.swap(paused, Ordering::AcqRel) == paused {
            return;
        }
        // An agent that was stopped for good is not paused or resumed.
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        if paused {
            self.halt(Phase::Paused).await;
        } else {
            self.rest_or_run().await;
        }
    }

    // ---- sessions and turns ----------------------------------------------

    /// The session of a place in this process, opened when there is none.
    async fn ensure_session(
        &self,
        run: &Run,
        place: &str,
    ) -> std::result::Result<String, CallError> {
        let known = run
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(place)
            .cloned();
        if let Some(session) = known {
            return Ok(session);
        }
        let limit = self.limits().session;
        let call = |method: &'static str, params: Value| async move {
            tokio::time::timeout(limit, run.connection.request(method, params))
                .await
                .unwrap_or(Err(CallError::Stalled))
        };
        let session = if run.pipe_tools {
            // bzz's own host keeps one conversation per place and finds it
            // again whatever the session id.
            let opened = call(
                "session/new",
                json!({"cwd": "/home/agent", "mcpServers": [], "_meta": {"bzz/place": place}}),
            )
            .await?;
            session_id(&opened)?
        } else {
            let credential = self.shared.credentials.mint(self.spec.id, run.id);
            let workspace =
                process::agent_dir(&self.shared.data_dir, self.spec.id).join("workspace");
            let servers = json!([{
                "name": "buzz",
                "command": self.shared.tools_program,
                "args": ["agent-tools"],
                "env": [{
                    "name": "BZZ_AGENT_TOOLS",
                    "value": format!("{}#{credential}", self.shared.tools_address),
                }],
            }]);
            let place_owned = place.to_owned();
            let stored = self
                .store(move |store, agent| store.agent_session(agent, &place_owned))
                .await
                .ok()
                .flatten();
            // A coding harness is told its system prompt, its folders and
            // what switches its owner's own settings off in the way that
            // program takes them, on a new session and on a loaded one.
            let params = if self.is_coding(run) {
                coding::session_params(
                    self.spec.local.harness,
                    run.offer,
                    &workspace,
                    &servers,
                    place,
                    &coding::system_prompt(
                        &self.spec.name,
                        self.spec.local.harness,
                        &self.spec.local,
                        &workspace,
                    ),
                    &self.spec.local,
                )
            } else {
                json!({"cwd": workspace, "mcpServers": servers, "_meta": {"bzz/place": place}})
            };
            let mut opened = None;
            if run.load_session
                && let Some(stored) = stored
            {
                let mut load = params.clone();
                load["sessionId"] = json!(stored);
                if let Ok(answer) = call("session/load", load).await {
                    opened = Some((stored, answer));
                }
            }
            let opened = match opened {
                Some(opened) => Ok(opened),
                None => call("session/new", params)
                    .await
                    .and_then(|answer| session_id(&answer).map(|session| (session, answer))),
            };
            // Its guard, before the session can be prompted: the mode bzz
            // sets on every session, and the model its owner chose.
            let opened = match opened {
                Ok((session, answer)) => self
                    .guard_session(run, &session, &answer)
                    .await
                    .map(|()| session),
                Err(error) => Err(error),
            };
            match opened {
                Ok(session) => {
                    self.shared.credentials.bind(&credential, &session);
                    let (place_owned, session_owned) = (place.to_owned(), session.clone());
                    let _ = self
                        .store(move |store, agent| {
                            store.set_agent_session(agent, &place_owned, &session_owned)
                        })
                        .await;
                    session
                }
                Err(error) => {
                    self.shared.credentials.forget(&credential);
                    return Err(error);
                }
            }
        };
        run.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(place.to_owned(), session.clone());
        Ok(session)
    }

    async fn run_turn(self: Arc<Self>, row: InboxRow) {
        let place = row.place.clone();
        self.activity.fetch_add(1, Ordering::AcqRel);
        self.turn(row).await;
        self.state().busy_places.remove(&place);
        self.wake.notify_one();
        // When nothing else happens for a while, bzz's own host is asked to
        // checkpoint its files.
        let planned = self.activity.fetch_add(1, Ordering::AcqRel) + 1;
        let idle = self.limits().checkpoint_idle;
        tokio::spawn(async move {
            tokio::time::sleep(idle).await;
            if self.activity.load(Ordering::Acquire) == planned {
                self.checkpoint().await;
            }
        });
    }

    /// Asks bzz's own host to finish its writes and checkpoint both of its
    /// files (`_bzz/checkpoint`), when it runs and no turn does. `true`: it
    /// answered. Sent when the agent has been idle, and before a move.
    pub async fn checkpoint(&self) -> bool {
        let run = self.run.lock().await.clone();
        let Some(run) = run.filter(|run| run.pipe_tools && !run.dead.load(Ordering::Acquire))
        else {
            return false;
        };
        if self.halted() || !self.state().busy_places.is_empty() {
            return false;
        }
        matches!(
            tokio::time::timeout(
                self.limits().session,
                run.connection.request("_bzz/checkpoint", json!({})),
            )
            .await,
            Ok(Ok(_))
        )
    }

    async fn turn(self: &Arc<Self>, row: InboxRow) {
        let Ok(request) = Event::from_json(&row.event_json) else {
            let request_id = row.request_id.clone();
            let _ = self
                .store(move |store, agent| {
                    store.agent_inbox_settle(
                        agent,
                        &request_id,
                        RequestState::Failed,
                        Some("unreadable"),
                    )
                })
                .await;
            return;
        };
        let request = Arc::new(request);
        // Before any harness sees it: an agent that was taken out of the
        // channel, or made read-only there, could not answer. The request
        // ends, with the reason, and nothing is started for it.
        match self.standing(row.channel_id, true).await {
            Standing::Writer => {}
            Standing::Not(why) => {
                self.settle_failed(&row, why.as_str()).await;
                return;
            }
            // The relay cannot be asked just now. The records that came
            // with the request say whether it may be handed over, while
            // they are fresh; what the turn then wants to publish is asked
            // about again, and waits for the relay. Otherwise the request
            // waits.
            Standing::Unknown => {
                let fresh = self
                    .admitted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&row.channel_id)
                    .is_some_and(|(at, may_write)| *may_write && at.elapsed() < ADMITTED_FRESH);
                if !fresh {
                    tokio::time::sleep(self.limits().restart_backoff).await;
                    return;
                }
            }
        }
        let Some(run) = self.ensure_run().await else {
            // The request stays queued; a start is tried again shortly.
            tokio::time::sleep(self.limits().restart_backoff).await;
            return;
        };
        let session = match self.ensure_session(&run, &row.place).await {
            Ok(session) => session,
            // The harness answered, with an error: say why by kind.
            Err(CallError::Rpc(error)) => {
                self.fail(&row, FailureKind::classify(&error)).await;
                return;
            }
            Err(CallError::Stalled) => {
                self.stop_run(&run, false).await;
                return;
            }
            Err(CallError::Closed) => {
                tokio::time::sleep(self.limits().restart_backoff).await;
                return;
            }
        };
        // From here the request counts as handed to a harness.
        let request_id = row.request_id.clone();
        if !self
            .store(move |store, agent| store.agent_inbox_delivered(agent, &request_id))
            .await
            .unwrap_or(false)
        {
            return;
        }
        let target = admission::reply_target(&request, &row.place, row.channel_id);
        self.state().turns.insert(
            session.clone(),
            LiveTurn {
                run: run.id,
                row: row.clone(),
                request: request.clone(),
                sent_any: false,
                final_sent: false,
                stopped: false,
                refused: false,
                calls: HashMap::new(),
                effects: 0,
                in_flight: 0,
                text: String::new(),
                previous_text: String::new(),
                activity: None,
                typing: Some(self.typing(&target)),
            },
        );
        let text = self
            .prompt_text(&row, &request, self.prompt_instructions(&run))
            .await;
        let prompt = run.connection.request(
            "session/prompt",
            json!({
                "sessionId": session,
                "prompt": [{"type": "text", "text": text}],
                "_meta": {"bzz/request": row.request_id},
            }),
        );
        tokio::pin!(prompt);
        let (result, timed_out) = tokio::select! {
            result = &mut prompt => (result, false),
            () = tokio::time::sleep(self.limits().turn) => {
                // The time limit: ask the harness to stop the turn, and stop
                // the harness itself when it does not. A command that still
                // waits for the owner's answer is not started.
                self.shared.approvals.void_session(
                    self.spec.id,
                    &session,
                    "the turn's time limit passed",
                );
                let _ = run.connection.notify("session/cancel", json!({"sessionId": session}));
                match tokio::time::timeout(self.limits().cancel_grace, &mut prompt).await {
                    Ok(result) => (result, true),
                    Err(_) => {
                        self.stop_run(&run, false).await;
                        (Err(CallError::Closed), true)
                    }
                }
            }
        };
        if matches!(result, Err(CallError::Stalled)) {
            self.stop_run(&run, false).await;
        }
        self.finish(&session, &row, result, timed_out).await;
    }

    /// Ends a turn. What is published, if anything, goes through the ledger.
    async fn finish(
        &self,
        session: &str,
        row: &InboxRow,
        result: std::result::Result<Value, CallError>,
        timed_out: bool,
    ) {
        // The turn is over, however it ended: a question of it that is
        // still open has nobody to answer to.
        self.shared
            .approvals
            .void_session(self.spec.id, session, "the turn ended");
        // Tool calls of this turn that are still publishing finish first.
        let waited =
            tokio::time::timeout(self.limits().publish * 2 + Duration::from_secs(5), async {
                loop {
                    let changed = self.turn_changed.notified();
                    if self
                        .state()
                        .turns
                        .get(session)
                        .is_none_or(|turn| turn.in_flight == 0)
                    {
                        break;
                    }
                    changed.await;
                }
            })
            .await;
        let Some(turn) = self.state().turns.remove(session) else {
            return;
        };
        // bzz is closing or the agent was paused: the request stays as it
        // is, and recovery decides at the next start.
        if self.halted() {
            return;
        }
        // A request that has its reply, accepted or refused, or that ended
        // while its turn ran, is over: nothing more is signed for it.
        let request_id = row.request_id.clone();
        let over = self
            .store(move |store, agent| {
                Ok(store.agent_reply_get(agent, &request_id)?.is_some()
                    || store
                        .agent_inbox_get(agent, &request_id)?
                        .is_none_or(|current| current.state != RequestState::Delivered))
            })
            .await
            .unwrap_or(true);
        if over {
            return;
        }
        match result {
            // The owner stopped it: that is how it ends, whatever the harness
            // answered or however it went away.
            _ if turn.stopped => self.fail(row, FailureKind::Stopped).await,
            _ if timed_out || waited.is_err() => self.fail(row, FailureKind::Timeout).await,
            Ok(response) => {
                // Plain text is never published when a `send` happened.
                if turn.sent_any {
                    let request_id = row.request_id.clone();
                    let _ = self
                        .store(move |store, agent| {
                            store.agent_inbox_settle(agent, &request_id, RequestState::Ended, None)
                        })
                        .await;
                    return;
                }
                let text = turn.last_text();
                // A harness that ends its turn when it is refused (Codex)
                // leaves nothing to publish: the notice says why.
                let nothing = if turn.refused {
                    FailureKind::NotAllowed
                } else {
                    FailureKind::NoReply
                };
                match response.get("stopReason").and_then(Value::as_str) {
                    Some("cancelled") => self.fail(row, nothing).await,
                    _ if !text.is_empty() => {
                        if let Reply::Withheld(why) = self
                            .reply(row, &turn.request, &text, &[], ReplyOrigin::Text, None)
                            .await
                        {
                            self.settle_failed(row, why.as_str()).await;
                        }
                    }
                    Some("refusal") => self.fail(row, FailureKind::Refused).await,
                    _ => self.fail(row, nothing).await,
                }
            }
            Err(CallError::Rpc(error)) => self.fail(row, FailureKind::classify(&error)).await,
            Err(CallError::Closed | CallError::Stalled) => self.after_crash(row).await,
        }
    }

    /// Decides what becomes of a request whose harness died while it was
    /// handed over. The same rule serves a crash of bzz, at the next start.
    async fn after_crash(&self, row: &InboxRow) {
        let request_id = row.request_id.clone();
        let Ok(Some(current)) = self
            .store(move |store, agent| store.agent_inbox_get(agent, &request_id))
            .await
        else {
            return;
        };
        if current.state != RequestState::Delivered {
            return;
        }
        // Handing it over again cannot answer twice when the harness knows
        // the request, or when nothing of that turn was published.
        let safe = self.spec.recognises_requests() || current.effects == 0;
        if safe && current.attempts < self.limits().max_attempts {
            let request_id = row.request_id.clone();
            let _ = self
                .store(move |store, agent| store.agent_inbox_requeue(agent, &request_id))
                .await;
            self.wake.notify_one();
        } else if safe {
            self.fail(&current, FailureKind::Crashed).await;
        } else {
            // In doubt: a notice, rather than a second answer.
            self.fail(&current, FailureKind::Interrupted).await;
        }
    }

    /// `instructions`: for a harness that is not bzz's own host, which is
    /// told in every message how to answer; the agent's own instructions
    /// are in it unless the harness took them as its system prompt.
    async fn prompt_text(
        &self,
        row: &InboxRow,
        request: &Event,
        instructions: Option<&str>,
    ) -> String {
        let limit = Duration::from_secs(10);
        let relay = self.relay.as_ref();
        // The conversation as it stands now, the request aside: a message
        // that waited behind another turn sees that turn's answer too.
        let mut recent = tokio::time::timeout(
            limit,
            relay::conversation(relay, &row.place, None, prompt::RECENT_MESSAGES + 1),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        recent.retain(|event| event.id != request.id);
        let (kind, channel_name) =
            match tokio::time::timeout(limit, relay.channel(row.channel_id)).await {
                Ok(Ok(facts)) => (facts.kind, facts.name),
                _ if row.place.contains('/') => (PlaceKind::Channel, row.channel_id.to_string()),
                _ => (PlaceKind::Solo, row.channel_id.to_string()),
            };
        let mut authors = recent
            .iter()
            .map(|event| event.pubkey.to_hex())
            .collect::<Vec<_>>();
        authors.push(request.pubkey.to_hex());
        authors.sort();
        authors.dedup();
        let names = tokio::time::timeout(limit, relay::names(relay, &authors))
            .await
            .unwrap_or_default();
        prompt::build(&PromptContext {
            agent_name: &self.spec.name,
            agent_pubkey: &self.spec.pubkey,
            owner_pubkey: &self.spec.owner,
            channel_name: &channel_name,
            kind,
            request,
            recent: &recent,
            names: &names,
            resumed: row.attempts > 0,
            instructions,
        })
    }

    /// The agent's typing indicator, until the returned guard is dropped.
    fn typing(&self, target: &ReplyTarget) -> AbortOnDrop {
        let relay = self.relay.clone();
        let target = target.clone();
        let interval = self.limits().typing_interval;
        let agent = self.spec.pubkey.clone();
        AbortOnDrop(tokio::spawn(async move {
            loop {
                // Only where the agent may write, by the relay's records as
                // bzz last read them: a membership notice makes bzz read
                // them again.
                let allowed = tokio::time::timeout(STANDING_LIMIT, relay.channel(target.channel))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some_and(|facts| facts.may_write(&agent).is_ok());
                if !allowed {
                    tokio::time::sleep(interval).await;
                    continue;
                }
                let thread = target
                    .thread
                    .as_ref()
                    .map(|(root, parent)| (root.as_str(), parent.as_str()));
                if let Some(builder) = crate::agents::typing::build(target.channel, thread)
                    && let Ok(event) = relay.sign(builder).await
                {
                    // Ephemeral: nobody waits for the relay's word on it.
                    let relay = relay.clone();
                    tokio::spawn(async move { relay.publish(event).await });
                }
                tokio::time::sleep(interval).await;
            }
        }))
    }

    // ---- the ledger ------------------------------------------------------

    /// Offers `text` as the request's one final reply: sign, store, then
    /// publish. A request that already has its reply gets that one back and
    /// nothing is published.
    async fn reply(
        &self,
        row: &InboxRow,
        request: &Event,
        text: &str,
        mentions: &[String],
        origin: ReplyOrigin,
        failure: Option<FailureKind>,
    ) -> Reply {
        // Asked before anything is signed: an agent that was taken out of
        // the channel, or made read-only there, signs no reply at all.
        let standing = self.standing(row.channel_id, true).await;
        if let Standing::Not(why) = standing {
            return Reply::Withheld(why);
        }
        let Ok(event) = self.message(row, request, text, mentions).await else {
            return Reply::Failed;
        };
        let (request_id, event_id, event_json) =
            (row.request_id.clone(), event.id.to_hex(), event.as_json());
        let stored_id = event_id.clone();
        let Ok(claim) = self
            .store(move |store, agent| {
                store.agent_reply_claim(
                    agent,
                    &request_id,
                    &stored_id,
                    &event_json,
                    origin,
                    failure.map(FailureKind::as_str),
                )
            })
            .await
        else {
            return Reply::Failed;
        };
        if !claim.fresh {
            return Reply::Already {
                refused: claim.reply.refused.is_some(),
                id: claim.reply.event_id,
            };
        }
        if standing == Standing::Unknown {
            // Signed and stored, and not sent on a guess: it is published,
            // unchanged, once the relay's records can be read.
            return Reply::Pending(event_id);
        }
        self.publish_stored(&row.request_id, event).await
    }

    /// Whether the agent may write in `channel` by the relay's own records:
    /// asked now when `fresh`, else as bzz last read them.
    async fn standing(&self, channel: Uuid, fresh: bool) -> Standing {
        let facts = async {
            if fresh {
                self.relay.channel_now(channel).await
            } else {
                self.relay.channel(channel).await
            }
        };
        match tokio::time::timeout(STANDING_LIMIT, facts).await {
            Ok(Ok(facts)) => match facts.may_write(&self.spec.pubkey) {
                Ok(()) => Standing::Writer,
                Err(why) => Standing::Not(why),
            },
            // The relay shows the agent no such channel: it is not in it.
            Ok(Err(Error::Access(_))) => Standing::Not(Unwritable::NotAMember),
            Ok(Err(_)) | Err(_) => Standing::Unknown,
        }
    }

    /// Ends an open request as failed with `kind`, and posts nothing.
    async fn settle_failed(&self, row: &InboxRow, kind: &'static str) {
        let request_id = row.request_id.clone();
        let _ = self
            .store(move |store, agent| {
                store.agent_inbox_settle(agent, &request_id, RequestState::Failed, Some(kind))
            })
            .await;
    }

    /// Publishes a reply the ledger holds and records what the relay said.
    /// The caller has asked whether the agent may write there.
    async fn publish_stored(&self, request_id: &str, event: Event) -> Reply {
        let event_id = event.id.to_hex();
        match self.relay.publish(event).await {
            Published::Accepted => {
                let request_id = request_id.to_owned();
                let _ = self
                    .store(move |store, agent| store.agent_reply_published(agent, &request_id))
                    .await;
                Reply::Sent(event_id)
            }
            Published::Refused(message) => {
                // The relay's verdict on this event, and so on this request:
                // the reply stays in the ledger as refused, the request is
                // over, and nothing more is signed for it.
                self.refuse_stored(request_id, &event_id, REPLY_REFUSED)
                    .await;
                Reply::Refused(message)
            }
            // It stays stored and is published again, unchanged, later.
            Published::Later(_) | Published::Unknown => Reply::Pending(event_id),
        }
    }

    /// Marks a stored reply as one that will never be published, and its
    /// request as failed with `kind`.
    async fn refuse_stored(&self, request_id: &str, event_id: &str, kind: &'static str) {
        let (request_id, event_id) = (request_id.to_owned(), event_id.to_owned());
        let _ = self
            .store(move |store, agent| {
                store.agent_reply_refused(agent, &request_id, &event_id, kind)
            })
            .await;
    }

    /// Publishes again, unchanged, every stored reply the relay has neither
    /// accepted nor refused. A relay that already has one keeps a single
    /// copy. Before each, the relay's records are asked whether the agent
    /// may still write there: a reply for a channel it was taken out of, or
    /// made read-only in, is never published.
    async fn republish(&self, older_than_seconds: u64) {
        // A reply nobody took in the time a message can be a request at
        // all is not tried for ever.
        let too_old = relay::MAX_REQUEST_AGE.as_secs();
        let _ = self
            .store(move |store, agent| {
                store.agent_replies_expire(agent, too_old, REPLY_UNDELIVERED)
            })
            .await;
        let Ok(pending) = self
            .store(move |store, agent| store.agent_replies_unpublished(agent, older_than_seconds))
            .await
        else {
            return;
        };
        let mut standings = HashMap::<Uuid, Standing>::new();
        for reply in pending {
            if self.halted() {
                return;
            }
            let Ok(event) = Event::from_json(&reply.event_json) else {
                continue;
            };
            let Some(channel) = crate::protocol::events::channel_id(&event) else {
                continue;
            };
            let standing = match standings.get(&channel) {
                Some(standing) => *standing,
                None => {
                    let standing = self.standing(channel, true).await;
                    standings.insert(channel, standing);
                    standing
                }
            };
            match standing {
                Standing::Writer => {
                    self.publish_stored(&reply.request_id, event).await;
                }
                Standing::Not(why) => {
                    self.refuse_stored(&reply.request_id, &reply.event_id, why.as_str())
                        .await;
                }
                // Not now: it waits for the next round.
                Standing::Unknown => {}
            }
        }
    }

    /// Ends a request as failed and says so in its thread, once.
    async fn fail(&self, row: &InboxRow, kind: FailureKind) {
        let settle = async |failure: &'static str| {
            let request_id = row.request_id.clone();
            let _ = self
                .store(move |store, agent| {
                    store.agent_inbox_settle(
                        agent,
                        &request_id,
                        RequestState::Failed,
                        Some(failure),
                    )
                })
                .await;
        };
        let Ok(request) = Event::from_json(&row.event_json) else {
            settle(kind.as_str()).await;
            return;
        };
        match self
            .reply(
                row,
                &request,
                kind.notice(),
                &[],
                ReplyOrigin::Failure,
                Some(kind),
            )
            .await
        {
            // No notice can be posted: the request still ends, with the
            // kind its turn failed with.
            Reply::Withheld(_) | Reply::Failed => settle(kind.as_str()).await,
            // A notice the relay refused stays in the ledger as refused, and
            // the request keeps its kind: bzz stops here.
            Reply::Refused(_) => {}
            Reply::Sent(_) | Reply::Already { .. } | Reply::Pending(_) => {}
        }
    }

    async fn message(
        &self,
        row: &InboxRow,
        request: &Event,
        text: &str,
        mentions: &[String],
    ) -> Result<Event> {
        let target = admission::reply_target(request, &row.place, row.channel_id);
        let thread = match &target.thread {
            Some((root, parent)) => Some(buzz_sdk::ThreadRef {
                root_event_id: nostr::EventId::from_hex(root)
                    .map_err(|_| Error::Protocol("the thread root is not an event id".into()))?,
                parent_event_id: nostr::EventId::from_hex(parent)
                    .map_err(|_| Error::Protocol("the request is not an event id".into()))?,
            }),
            None => None,
        };
        let mentions = mentions.iter().map(String::as_str).collect::<Vec<_>>();
        let builder = buzz_sdk::build_message(
            target.channel,
            text,
            thread.as_ref(),
            &mentions,
            false,
            &[],
            &[],
        )
        .map_err(|error| Error::Protocol(error.to_string()))?;
        self.relay.sign(builder).await
    }

    // ---- tools -----------------------------------------------------------

    /// Serves one tool call of a session of the process `run`. The request
    /// it acts for is the one of the session's live turn.
    pub async fn tool_call(
        &self,
        run: u64,
        session: &str,
        name: &str,
        arguments: &Value,
    ) -> ToolReply {
        // As it arrived, also a call bzz refuses: a transcript holds what a
        // model really called.
        self.record_tool(run, session, name, arguments).await;
        let call = match tools::parse(name, arguments) {
            Ok(call) => call,
            Err(text) => return ToolReply::refused(text),
        };
        let publishes = !matches!(call, ToolCall::Read { .. });
        let is_final = matches!(call, ToolCall::Send { is_final: true, .. });
        let (row, request) = {
            let mut state = self.state();
            let Some(turn) = state.turns.get_mut(session).filter(|turn| turn.run == run) else {
                return ToolReply::refused(
                    "No message is being answered in this session right now, so nothing was done.",
                );
            };
            if publishes && !is_final {
                if turn.final_sent {
                    return ToolReply::refused(
                        "This turn already has its final reply. Nothing more can be posted for this message.",
                    );
                }
                if turn.effects >= self.limits().effects_per_turn {
                    return ToolReply::refused(
                        "This turn has posted as much as one turn may. Send your final reply.",
                    );
                }
                turn.effects += 1;
            }
            turn.in_flight += 1;
            (turn.row.clone(), turn.request.clone())
        };
        let reply = match call {
            ToolCall::Send {
                text,
                is_final: true,
                mentions,
            } => match self
                .reply(&row, &request, &text, &mentions, ReplyOrigin::Send, None)
                .await
            {
                Reply::Sent(id) => ToolReply::final_reply(format!("sent as {id}")),
                Reply::Already { id, refused: false } => ToolReply::final_reply(format!(
                    "already sent as {id}: this message has its final reply, and nothing new was posted"
                )),
                Reply::Already { refused: true, .. } => ToolReply::ended(
                    "The reply to this message was refused, and that is final: nothing was posted, and nothing more can be posted for this message. Do not try again; end your turn.",
                ),
                Reply::Pending(id) => ToolReply::final_reply(format!(
                    "sent as {id}; the relay has not confirmed it yet and bzz will keep trying"
                )),
                // The relay's no is final for this message: the turn ends
                // here, and a harness that tries again is told the same.
                Reply::Refused(why) => ToolReply::ended(format!(
                    "The relay refused the reply: {why}. That is final: nothing was posted, and nothing more can be posted for this message. Do not try again; end your turn."
                )),
                Reply::Withheld(why) => {
                    self.settle_failed(&row, why.as_str()).await;
                    ToolReply::ended(format!(
                        "Nothing was posted: {}. Nothing more can be posted for this message. Do not try again; end your turn.",
                        unwritable_words(why)
                    ))
                }
                Reply::Failed => {
                    ToolReply::refused("bzz could not sign or store the reply. Nothing was sent.")
                }
            },
            ToolCall::Send { text, mentions, .. } => {
                self.progress(&row, &request, &text, &mentions).await
            }
            ToolCall::Read { limit, before } => self.read(&row, &request, limit, before).await,
            ToolCall::React { emoji, event } => self.react(&row, &request, &emoji, event).await,
        };
        {
            let mut state = self.state();
            if let Some(turn) = state.turns.get_mut(session) {
                turn.in_flight = turn.in_flight.saturating_sub(1);
                if publishes && !is_final && !reply.is_error {
                    turn.sent_any = true;
                }
                if reply.is_final {
                    turn.final_sent = true;
                    turn.typing = None;
                }
            }
        }
        self.turn_changed.notify_waiters();
        reply
    }

    // ---- a real command, and a git credential ---------------------------

    /// The agent's settings as the settings file has them at this moment.
    /// `Ok(None)`: bzz has no settings file to read. `Err`: the file cannot
    /// be read, or no longer has this agent as one bzz runs; in that doubt
    /// nothing is allowed and nothing is given.
    fn settings_now(&self) -> std::result::Result<Option<LocalAgentConfig>, &'static str> {
        let Some(path) = &self.shared.settings_file else {
            return Ok(None);
        };
        let config = crate::config::load_from(path)
            .map_err(|_| "bzz could not read its settings just now")?;
        config
            .hosted_agents
            .into_iter()
            .find(|agent| agent.id == self.spec.id)
            .filter(|agent| agent.runs_locally())
            .and_then(|agent| agent.local)
            .map(Some)
            .ok_or("this agent is no longer one that bzz runs, by its settings")
    }

    fn record(&self, entry: &activity::Entry) {
        activity::record(&self.shared.data_dir, self.spec.id, entry);
    }

    /// Answers `_bzz/approve`: whether the real command the host is about
    /// to start may start. `{"allow": true}` is the only yes; everything
    /// else is a no whose reason the model reads.
    ///
    /// What bzz says yes to is the string the host sent, and bzz sends
    /// nothing of it back: the host runs what it asked about.
    pub async fn approve(self: &Arc<Self>, run: u64, params: &Value) -> Value {
        let refuse = |reason: String| json!({"allow": false, "reason": reason});
        let text = |name: &str| params.get(name).and_then(Value::as_str);
        let (Some(session), Some("exec"), Some(mount), Some(command)) = (
            text("sessionId"),
            text("kind"),
            text("mount"),
            text("command"),
        ) else {
            return refuse(
                "`_bzz/approve` needs `sessionId`, `kind` \"exec\", `mount` and `command`. Nothing was started.".into(),
            );
        };
        // What can never run. The host does not ask about these; a harness
        // that does is told no.
        if command.is_empty()
            || command.contains('\0')
            || command.len() > approval::MAX_COMMAND_BYTES
        {
            return refuse(format!(
                "A command is not empty, holds no NUL character and is at most {} bytes. Nothing was started.",
                approval::MAX_COMMAND_BYTES
            ));
        }
        // The folder, by bzz's own settings and not by what the host was
        // told at its start.
        let folder = self
            .spec
            .local
            .folders
            .iter()
            .find(|folder| folder.name == mount)
            .filter(|folder| folder.exec.granted());
        let Some(folder) = folder else {
            let shown = crate::render::sanitize::visible(mount)
                .chars()
                .take(64)
                .collect::<String>();
            self.record(&activity::Entry::command(
                &shown,
                command,
                "refused: the folder is not granted for commands",
            ));
            return refuse(format!(
                "No real command may run in /mnt/{shown}: your owner has not granted that folder for commands. Nothing was started."
            ));
        };
        // And by the settings as they are now: what the owner has taken
        // away since this bzz started is refused at once. What he has added
        // since counts only after a restart, when the host is told of it.
        // "Without asking" holds only while the people who may wake the
        // agent are ones its owner named. An agent that anyone may wake
        // would run commands for anyone, as its owner: there he is asked.
        let mut asks = folder.exec == ExecGrant::Ask || self.spec.local.wake == WakeRule::Anyone;
        match self.settings_now() {
            Ok(None) => {}
            Ok(Some(now)) => {
                asks |= now.wake == WakeRule::Anyone;
                let still = now.folders.iter().find(|granted| {
                    granted.name == folder.name
                        && granted.path == folder.path
                        && granted.exec.granted()
                });
                let Some(still) = still else {
                    self.record(&activity::Entry::command(
                        &folder.name,
                        command,
                        "refused: the folder is no longer granted for commands",
                    ));
                    return refuse(format!(
                        "No real command may run in /mnt/{}: your owner has taken that grant away. Nothing was started.",
                        folder.name
                    ));
                };
                // Asked about there now, though not when this bzz started.
                asks |= still.exec == ExecGrant::Ask;
            }
            Err(why) => {
                self.record(&activity::Entry::command(
                    &folder.name,
                    command,
                    format!("refused: {why}"),
                ));
                return refuse(format!("Nothing was started: {why}."));
            }
        }
        let refused = |agent: &Self, why: &str, reason: String| {
            agent.record(&activity::Entry::command(&folder.name, command, why));
            refuse(reason)
        };
        // The turn the command belongs to: live, in this process, and not
        // stopped.
        let live = |agent: &Self| {
            let state = agent.state();
            state
                .turns
                .get(session)
                .filter(|turn| turn.run == run && !turn.stopped)
                .map(|turn| {
                    (
                        turn.row.channel_id,
                        turn.row.place.clone(),
                        turn.row.author.clone(),
                    )
                })
        };
        let Some((channel, place, requester)) = live(self).filter(|_| !self.halted()) else {
            return refused(
                self,
                "refused: no turn is running for it",
                "No message is being answered in this session right now, so no command is started."
                    .into(),
            );
        };
        if !asks {
            self.record(&activity::Entry::command(
                &folder.name,
                command,
                "allowed without asking",
            ));
            return json!({"allow": true});
        }
        let limit = self.limits().approval;
        let asked = Asked {
            id: 0,
            agent: self.spec.id,
            agent_name: self.spec.name.clone(),
            channel,
            place,
            requester,
            folder: folder.name.clone(),
            path: folder.path.clone(),
            command: command.to_owned(),
            coding: false,
            asked_at: unix_now(),
            expires: Instant::now() + limit,
        };
        let (id, outcome) = match self.shared.approvals.ask(asked, session) {
            Ok(opened) => opened,
            Err(NotAsked::NobodyToAsk) => {
                return refused(
                    self,
                    "refused: nobody was at bzz to ask",
                    format!(
                        "Nothing was started: a command in /mnt/{} needs your owner's approval, and your owner is not at bzz to give it (bzz is running without its window). Tell the people in this conversation that your owner must open bzz to approve the command, and do not try it again until someone asks.",
                        folder.name
                    ),
                );
            }
            Err(NotAsked::OneAtATime) => {
                return refuse(
                    "Another command of this conversation is waiting for your owner's answer. One at a time: nothing was started.".into(),
                );
            }
        };
        self.record(&activity::Entry::command(
            &folder.name,
            command,
            "asked of you",
        ));
        let first_line = command.split('\n').next().unwrap_or_default();
        let waiting = format!(
            "waiting for your answer: run in /mnt/{}: {}",
            folder.name,
            crate::render::sanitize::visible(first_line)
        )
        .chars()
        .take(120)
        .collect::<String>();
        if let Some(turn) = self.state().turns.get_mut(session) {
            turn.activity = Some(waiting);
        }
        // The turn may have ended between the look and the question.
        if live(self).is_none() || self.halted() {
            self.shared.approvals.close(id);
            return refused(
                self,
                "void: the turn ended",
                "The turn ended before your owner could be asked. Nothing was started.".into(),
            );
        }
        tokio::pin!(outcome);
        let ended = tokio::select! {
            biased;
            ended = &mut outcome => ended.ok(),
            () = tokio::time::sleep(limit) => {
                if self.shared.approvals.close(id) {
                    None
                } else {
                    // Answered at that very moment: the answer counts.
                    (&mut outcome).await.ok()
                }
            }
        };
        if let Some(turn) = self.state().turns.get_mut(session) {
            turn.activity = Some(format!(
                "run in /mnt/{}: {}",
                folder.name,
                crate::render::sanitize::visible(first_line)
                    .chars()
                    .take(80)
                    .collect::<String>()
            ));
        }
        match ended {
            // A yes for a turn that has gone meanwhile starts nothing.
            Some(Outcome::Allowed) if live(self).is_some() && !self.halted() => {
                self.record(&activity::Entry::command(
                    &folder.name,
                    command,
                    "allowed by you",
                ));
                json!({"allow": true})
            }
            Some(Outcome::Allowed) => refused(
                self,
                "void: the turn ended before your answer",
                "The turn ended before your owner's answer. Nothing was started.".into(),
            ),
            Some(Outcome::Refused(reason)) => refused(
                self,
                &match &reason {
                    Some(reason) => format!("refused by you: {reason}"),
                    None => "refused by you".to_owned(),
                },
                format!(
                    "Your owner refused this command{}. Nothing was started. Do not try it again in another form; say in the conversation that it was refused.",
                    reason
                        .map(|reason| format!(", and says: {reason}"))
                        .unwrap_or_default()
                ),
            ),
            Some(Outcome::Void(why)) => refused(
                self,
                &format!("void: {why}"),
                format!("The question to your owner was withdrawn: {why}. Nothing was started."),
            ),
            None => refused(
                self,
                &format!("refused: no answer in {} s", limit.as_secs()),
                format!(
                    "Your owner did not answer within {} seconds, which counts as a refusal. Nothing was started. Say so in the conversation, and try again only when someone asks you to.",
                    limit.as_secs()
                ),
            ),
        }
    }

    /// Answers `_bzz/credential`: the user name and token the owner stored
    /// for this agent and `host`, when the host is on this agent's list of
    /// allowed hosts and the asking session has a turn running. Otherwise
    /// an error whose message tells the model what its owner can do.
    ///
    /// The agent is the one this process was started for: no parameter
    /// names it. The answer is written to the pipe and nowhere else.
    pub async fn credential(
        &self,
        run: u64,
        params: &Value,
    ) -> std::result::Result<Value, RpcError> {
        let no = |message: String| Err(RpcError::new(acp::REFUSED, message));
        let (Some(session), Some(host)) = (
            params.get("sessionId").and_then(Value::as_str),
            params.get("host").and_then(Value::as_str),
        ) else {
            return no("`_bzz/credential` needs `sessionId` and `host`.".into());
        };
        let name = crate::protocol::artifact::display(&self.spec.name, 64);
        let live = self
            .state()
            .turns
            .get(session)
            .is_some_and(|turn| turn.run == run && !turn.stopped);
        if !live || self.halted() {
            return no(
                "No message is being answered in this session right now, so bzz gives no credential."
                    .into(),
            );
        }
        // As the allow-list names a host: lower case, with its port when
        // that is not the scheme's own. Compared exactly.
        let host = host.to_ascii_lowercase();
        let shown = crate::render::sanitize::visible(&host)
            .chars()
            .take(260)
            .collect::<String>();
        // By the settings bzz started with, and by the settings as they are
        // now: a host the owner has taken off the list since is refused.
        let listed = self.spec.local.network.contains(&host)
            && match self.settings_now() {
                Ok(None) => true,
                Ok(Some(now)) => now.network.contains(&host),
                Err(_) => false,
            };
        if !listed {
            self.record(&activity::Entry::credential(
                &shown,
                "refused: the host is not on its list of allowed hosts",
            ));
            return no(format!(
                "bzz gives no credential for {shown}: it is not on this agent's list of allowed hosts. Your owner can allow it with `bzz agent edit {name} --allow-host {shown}` and store a credential with `bzz agent credential set {name} {shown}`."
            ));
        }
        let Some(store) = &self.shared.git_credentials else {
            self.record(&activity::Entry::credential(
                &shown,
                "refused: none can be opened",
            ));
            return no(format!(
                "bzz cannot open stored credentials now, so it has none for {shown}."
            ));
        };
        match store.get(self.spec.id, &host).await {
            Ok(Some(credential)) => {
                self.record(&activity::Entry::credential(&shown, "given"));
                Ok(json!({
                    "username": credential.username.as_str(),
                    "password": credential.password.as_str(),
                }))
            }
            Ok(None) => {
                self.record(&activity::Entry::credential(&shown, "none is stored"));
                no(format!(
                    "No credential is stored for {shown}. Your owner can store one with `bzz agent credential set {name} {shown}`."
                ))
            }
            Err(_) => {
                self.record(&activity::Entry::credential(
                    &shown,
                    "refused: the stored one could not be opened",
                ));
                no(format!(
                    "The credential stored for {shown} could not be opened. Your owner can store it again with `bzz agent credential set {name} {shown}`."
                ))
            }
        }
    }

    /// Counts something about to be published for a request besides its
    /// final reply. Before the publish, so a crash can only overcount.
    async fn count_effect(&self, row: &InboxRow) -> bool {
        let request_id = row.request_id.clone();
        self.store(move |store, agent| store.agent_inbox_effect(agent, &request_id))
            .await
            .is_ok()
    }

    /// A `send` that is not final: a message of the turn, not its answer.
    async fn progress(
        &self,
        row: &InboxRow,
        request: &Event,
        text: &str,
        mentions: &[String],
    ) -> ToolReply {
        if let Some(refusal) = self.may_post(row, "Nothing was sent.").await {
            return refusal;
        }
        let Ok(event) = self.message(row, request, text, mentions).await else {
            return ToolReply::refused("bzz could not sign the message. Nothing was sent.");
        };
        if !self.count_effect(row).await {
            return ToolReply::refused("bzz could not record the message. Nothing was sent.");
        }
        let id = event.id.to_hex();
        match self.relay.publish(event).await {
            Published::Accepted => ToolReply::ok(format!("sent as {id}")),
            Published::Unknown => {
                ToolReply::ok(format!("sent as {id}; the relay has not confirmed it"))
            }
            Published::Refused(why) | Published::Later(why) => ToolReply::refused(format!(
                "The relay refused the message: {why}. Nothing was sent."
            )),
        }
    }

    /// For a message of a turn or a reaction: `None` when the agent may
    /// write in the request's channel by the relay's records as they are
    /// now, else what the harness is told.
    async fn may_post(&self, row: &InboxRow, nothing: &str) -> Option<ToolReply> {
        match self.standing(row.channel_id, true).await {
            Standing::Writer => None,
            Standing::Not(why) => Some(ToolReply::refused(format!(
                "{nothing} {}. Do not try again; end your turn.",
                capitalised(unwritable_words(why))
            ))),
            Standing::Unknown => Some(ToolReply::refused(format!(
                "{nothing} bzz could not ask the relay just now whether you may still write in this conversation. Try again shortly."
            ))),
        }
    }

    async fn event_of_place(&self, row: &InboxRow, id: &str) -> Option<Event> {
        self.relay
            .query(vec![json!({"ids": [id], "limit": 1})])
            .await
            .ok()?
            .into_iter()
            .find(|event| {
                event.id.to_hex() == id
                    && crate::protocol::events::channel_id(event) == Some(row.channel_id)
                    && crate::protocol::events::verify(event).is_ok()
            })
    }

    async fn read(
        &self,
        row: &InboxRow,
        request: &Event,
        limit: usize,
        before: Option<String>,
    ) -> ToolReply {
        let before = match before {
            Some(id) => match self.event_of_place(row, &id).await {
                Some(event) => Some(event),
                None => {
                    return ToolReply::refused(format!(
                        "There is no message {id} in this conversation that you can read."
                    ));
                }
            },
            None => None,
        };
        let relay = self.relay.as_ref();
        let Ok(recent) = relay::conversation(relay, &row.place, before.as_ref(), limit).await
        else {
            return ToolReply::refused("The conversation could not be read just now. Try again.");
        };
        let mut authors = recent
            .iter()
            .map(|event| event.pubkey.to_hex())
            .collect::<Vec<_>>();
        authors.sort();
        authors.dedup();
        let names = relay::names(relay, &authors).await;
        ToolReply::ok(prompt::transcript(&PromptContext {
            agent_name: &self.spec.name,
            agent_pubkey: &self.spec.pubkey,
            owner_pubkey: &self.spec.owner,
            channel_name: "",
            kind: PlaceKind::Channel,
            request,
            recent: &recent,
            names: &names,
            resumed: false,
            instructions: None,
        }))
    }

    async fn react(
        &self,
        row: &InboxRow,
        request: &Event,
        emoji: &str,
        event: Option<String>,
    ) -> ToolReply {
        const NOTHING: &str = "No reaction was added.";
        let target = match event {
            None => request.id,
            Some(id) => match self.event_of_place(row, &id).await {
                Some(event) => event.id,
                None => {
                    return ToolReply::refused(format!(
                        "There is no message {id} in this conversation. {NOTHING}"
                    ));
                }
            },
        };
        if let Some(refusal) = self.may_post(row, NOTHING).await {
            return refusal;
        }
        let Ok(builder) = buzz_sdk::build_reaction(target, emoji) else {
            return ToolReply::refused(format!("`emoji` of `react` is not a reaction. {NOTHING}"));
        };
        let Ok(reaction) = self.relay.sign(builder).await else {
            return ToolReply::refused(format!("bzz could not sign the reaction. {NOTHING}"));
        };
        if !self.count_effect(row).await {
            return ToolReply::refused(format!("bzz could not record the reaction. {NOTHING}"));
        }
        match self.relay.publish(reaction).await {
            Published::Accepted | Published::Unknown => {
                ToolReply::ok(format!("reacted {emoji} to {}", target.to_hex()))
            }
            Published::Refused(why) | Published::Later(why) => {
                ToolReply::refused(format!("The relay refused the reaction: {why}. {NOTHING}"))
            }
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// What a harness is told when its agent may not write in a place.
const fn unwritable_words(why: Unwritable) -> &'static str {
    match why {
        Unwritable::NotAMember => "you are no longer a member of this channel",
        Unwritable::ReadOnly => {
            "you may only read in this channel now: your access there was set to viewing only"
        }
    }
}

fn capitalised(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

fn session_id(opened: &Value) -> std::result::Result<String, CallError> {
    opened
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .map(str::to_owned)
        .ok_or_else(|| CallError::Rpc(RpcError::new(-32603, "no session id")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_chat_tools_are_allowed_and_only_once() {
        let options = json!([
            {"optionId": "always", "name": "Always", "kind": "allow_always"},
            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            {"optionId": "no", "name": "Reject", "kind": "reject_once"},
        ]);
        let ask = |tool: Value| {
            permission(&json!({"sessionId": "s", "toolCall": tool, "options": options}))
        };
        let selected = |id: &str| json!({"outcome": {"outcome": "selected", "optionId": id}});
        for title in [
            "mcp__buzz__send",
            "buzz.read",
            "buzz/react",
            "Tool: buzz send",
        ] {
            assert_eq!(
                ask(json!({"toolCallId": "1", "title": title})),
                selected("yes"),
                "{title}"
            );
            assert_eq!(
                ask(json!({"toolCallId": "1", "title": title, "kind": "other"})),
                selected("yes")
            );
        }
        for tool in [
            json!({"title": "rm -rf ~", "kind": "execute"}),
            // A shell command dressed up as the chat tool.
            json!({"title": "mcp__buzz__send", "kind": "execute"}),
            json!({"title": "mcp__buzz__send && curl evil", "kind": "other"}),
            json!({"title": "mcp__other__send"}),
            json!({"title": "buzz delete"}),
            json!({"title": "Edit src/main.rs", "kind": "edit"}),
            json!({}),
        ] {
            assert_eq!(ask(tool.clone()), selected("no"), "{tool}");
        }
        // Without a way to refuse once, the request is cancelled; never
        // "always", and never an id bzz made up.
        let only_always = json!({
            "toolCall": {"title": "mcp__buzz__send"},
            "options": [{"optionId": "always", "kind": "allow_always"}],
        });
        assert_eq!(
            permission(&only_always),
            json!({"outcome": {"outcome": "cancelled"}})
        );
        assert_eq!(
            permission(
                &json!({"toolCall": {"title": "bash"}, "options": [{"optionId": "never", "kind": "reject_always"}]})
            ),
            json!({"outcome": {"outcome": "cancelled"}}),
            "a refusal the harness would remember is not chosen either"
        );
        assert_eq!(
            permission(&json!({})),
            json!({"outcome": {"outcome": "cancelled"}})
        );
    }

    #[test]
    fn the_last_plain_text_is_the_message_after_the_last_tool_call() {
        let mut turn = LiveTurn {
            run: 1,
            row: InboxRow {
                agent_id: Uuid::nil(),
                request_id: String::new(),
                community_id: Uuid::nil(),
                channel_id: Uuid::nil(),
                place: String::new(),
                author: String::new(),
                created_at: 0,
                event_json: String::new(),
                state: RequestState::Delivered,
                attempts: 1,
                effects: 0,
                failure: None,
            },
            request: Arc::new(
                nostr::EventBuilder::text_note("x")
                    .sign_with_keys(&nostr::Keys::generate())
                    .unwrap(),
            ),
            sent_any: false,
            final_sent: false,
            stopped: false,
            refused: false,
            calls: HashMap::new(),
            effects: 0,
            in_flight: 0,
            text: String::new(),
            previous_text: String::new(),
            activity: None,
            typing: None,
        };
        assert_eq!(turn.last_text(), "");
        turn.push_text("Let me ");
        turn.push_text("look.");
        turn.end_message();
        turn.push_text(" 391. ");
        assert_eq!(turn.last_text(), "391.");
        // A tool call with no text after it leaves the text before it.
        turn.end_message();
        turn.end_message();
        assert_eq!(turn.last_text(), "391.");
        // A flood of text is kept only up to the limit, however it arrives.
        for _ in 0..3 {
            turn.push_text(&"y".repeat(MAX_PLAIN_TEXT_CHARS));
        }
        assert_eq!(
            turn.text.chars().count(),
            MAX_PLAIN_TEXT_CHARS,
            "held in memory"
        );
        assert_eq!(turn.last_text().chars().count(), MAX_PLAIN_TEXT_CHARS);
    }
}
