//! One local agent: its harness process, its sessions and its turns.
//!
//! The rules this file keeps (ADR v0.15 local agents):
//!
//! - **One final reply per request.** A final `send` is signed, stored under
//!   the request in the ledger, and only then published. The request comes
//!   from the calling session's live turn, never from the call's arguments.
//! - **A turn always ends in one of four ways:** a final `send`; the
//!   harness's last plain text, when it never called `send`; something said
//!   without a final reply; or a failure notice named by kind.
//! - **After a crash** a request is handed over again only when that cannot
//!   answer twice: the harness can tell a request it has seen (bzz's own
//!   host), or nothing of the turn was published. Otherwise the turn fails
//!   with a notice.
//! - **A harness is not trusted and not waited for.** Every wait on it has a
//!   limit, and a harness that keeps dying is left alone for a while.

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
    agents::runtime::{
        FailureKind, Limits,
        acp::{CallError, Connection, Incoming, RpcError},
        admission::{self, ChannelFacts, PlaceKind, Refusal, ReplyTarget, Rule},
        credentials::Credentials,
        process::{self, Harness, Launch},
        prompt::{self, PromptContext},
        relay::{self, AgentRelay, Published},
        tools::{self, ToolCall, ToolReply},
    },
    config::{Harness as HarnessKind, LocalAgentConfig},
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
    pub fn line(&self) -> String {
        let state = match self.phase {
            Phase::Idle => "idle".to_owned(),
            Phase::Starting => "starting".to_owned(),
            Phase::Running if self.turns == 0 => "ready".to_owned(),
            Phase::Running => format!(
                "answering {}{}",
                self.turns,
                self.activity
                    .as_deref()
                    .map(|activity| format!(
                        " ({})",
                        crate::protocol::artifact::display(activity, 40)
                    ))
                    .unwrap_or_default()
            ),
            Phase::Unavailable => "unavailable".to_owned(),
            Phase::Paused => "paused".to_owned(),
            Phase::Stopped => "stopped".to_owned(),
        };
        let mut line = format!(
            "{} · {} · {state}",
            crate::protocol::artifact::display(&self.name, 32),
            self.harness.as_str()
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
    pid: Option<u32>,
    harness: tokio::sync::Mutex<Option<Harness>>,
    /// Place -> session id, for this process.
    sessions: Mutex<HashMap<String, String>>,
    /// Stopped on purpose: its end is not a crash.
    deliberate: AtomicBool,
    dead: AtomicBool,
    tool_calls: Arc<tokio::sync::Semaphore>,
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
}

/// What became of a reply offered to the ledger.
enum Reply {
    Sent(String),
    /// The request already had its final reply.
    Already(String),
    /// Stored; the relay has not acknowledged it yet.
    Pending(String),
    Refused(String),
    /// It could not be signed or stored.
    Failed,
}

fn is_chat_tool(tool: &Value) -> bool {
    // A tool that runs, edits, deletes or moves something is never ours.
    if !matches!(
        tool.get("kind").and_then(Value::as_str),
        None | Some("other")
    ) {
        return false;
    }
    let title = tool
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let words = title
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty() && !matches!(*word, "mcp" | "tool"))
        .collect::<Vec<_>>();
    matches!(words.as_slice(), ["buzz", "send" | "read" | "react"])
}

/// Answers `session/request_permission`: bzz's own chat tools are allowed,
/// once; anything else is refused (grants are M7). The answer names an
/// option by its kind, never by a fixed id.
pub fn permission(params: &Value) -> Value {
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let pick = |kind: &str| {
        options
            .iter()
            .find(|option| option.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|option| option.get("optionId").cloned())
    };
    let chosen = if is_chat_tool(params.get("toolCall").unwrap_or(&Value::Null)) {
        pick("allow_once")
    } else {
        pick("reject_once").or_else(|| pick("reject_always"))
    };
    match chosen {
        Some(option) => json!({"outcome": {"outcome": "selected", "optionId": option}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
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
        self.stopping.load(Ordering::Acquire) || self.paused.load(Ordering::Acquire)
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
        self.republish(0).await;
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

    /// Starts handing queued requests to the harness.
    pub fn start(self: &Arc<Self>) {
        tokio::spawn(self.clone().schedule());
        self.wake.notify_one();
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
            if self.paused.load(Ordering::Acquire) {
                continue;
            }
            self.republish(REPUBLISH_AFTER_SECONDS).await;
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

    fn launch(&self) -> Result<Launch> {
        let directory = process::agent_dir(&self.shared.data_dir, self.spec.id);
        process::create_private_dir(&directory)?;
        let local = &self.spec.local;
        let own_host = local.harness == HarnessKind::PiDurable;
        let Some((program, arguments)) = local.command.split_first() else {
            return Err(Error::Config(match local.harness {
                HarnessKind::PiDurable => "the Pi Durable host is not installed".into(),
                other => format!(
                    "the {} harness is not available in this bzz",
                    other.as_str()
                ),
            }));
        };
        let mut args = arguments.to_vec();
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
        Ok(Launch {
            program: program.clone(),
            args,
            env: process::base_environment(&home, |name| std::env::var(name).ok()),
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
            })).collect::<Vec<_>>(),
            "network": local.network,
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
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {"name": "bzz", "version": env!("CARGO_PKG_VERSION")},
            "_meta": {"bzz": bzz},
        })
    }

    async fn start_run(self: &Arc<Self>) -> std::result::Result<Arc<Run>, String> {
        let launch = self.launch().map_err(|error| error.to_string())?;
        let run_dir = process::run_dir(&self.shared.data_dir);
        let (harness, stdin, stdout) =
            Harness::spawn(&launch, &run_dir, self.spec.id).map_err(|error| error.to_string())?;
        let (connection, incoming) = Connection::start(stdout, stdin);
        let answer = tokio::time::timeout(
            self.limits().start,
            connection.request("initialize", self.initialize_params()),
        )
        .await;
        let problem = match &answer {
            Ok(Ok(response)) if response.get("protocolVersion") == Some(&json!(1)) => None,
            Ok(Ok(_)) => Some("its harness speaks another version of the protocol"),
            Ok(Err(CallError::Rpc(_))) => Some("its harness refused to start"),
            Ok(Err(_)) => Some("its harness exited while starting"),
            Err(_) => Some("its harness did not answer in time"),
        };
        if let Some(problem) = problem {
            let closing = connection.clone();
            harness
                .stop(move || closing.close_input(), self.limits().stop_grace)
                .await;
            return Err(problem.to_owned());
        }
        let response = answer
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        let run = Arc::new(Run {
            id: self.next_run.fetch_add(1, Ordering::Relaxed),
            pipe_tools: response.pointer("/_meta/bzz/tools").and_then(Value::as_str)
                == Some("pipe"),
            load_session: response.pointer("/agentCapabilities/loadSession") == Some(&json!(true)),
            pid: harness.pid(),
            connection,
            harness: tokio::sync::Mutex::new(Some(harness)),
            sessions: Mutex::new(HashMap::new()),
            deliberate: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            tool_calls: Arc::new(tokio::sync::Semaphore::new(CONCURRENT_TOOL_CALLS)),
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
                state.crashes.push_back(Instant::now());
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
        // Its credentials stop resolving before anything else happens.
        self.shared.credentials.revoke_agent(self.spec.id);
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
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    turn.push_text(text);
                }
            }
            Some("tool_call") => {
                turn.end_message();
                turn.activity = update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|title| title.chars().take(80).collect());
            }
            Some("tool_call_update") => {
                if let Some(title) = update.get("title").and_then(Value::as_str) {
                    turn.activity = Some(title.chars().take(80).collect());
                }
            }
            // Kinds bzz does not know are ignored.
            _ => {}
        }
    }

    /// Stops the agent for good: bzz is closing or locking.
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
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
        self.wake.notify_one();
        let run = self.run.lock().await.clone();
        if let Some(run) = run {
            self.stop_run(&run, true).await;
        }
    }

    /// Pauses or resumes the agent. A paused agent has no process.
    pub async fn set_paused(self: &Arc<Self>, paused: bool) {
        if self.paused.swap(paused, Ordering::AcqRel) == paused {
            return;
        }
        if paused {
            self.halt(Phase::Paused).await;
        } else {
            self.state().phase = Phase::Idle;
            self.recover().await;
            self.wake.notify_one();
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
            let mut session = None;
            if run.load_session
                && let Some(stored) = stored
                && call(
                    "session/load",
                    json!({"sessionId": stored, "cwd": workspace, "mcpServers": servers}),
                )
                .await
                .is_ok()
            {
                session = Some(stored);
            }
            let session = match session {
                Some(session) => Ok(session),
                None => call(
                    "session/new",
                    json!({"cwd": workspace, "mcpServers": servers, "_meta": {"bzz/place": place}}),
                )
                .await
                .and_then(|opened| session_id(&opened)),
            };
            match session {
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
                effects: 0,
                in_flight: 0,
                text: String::new(),
                previous_text: String::new(),
                activity: None,
                typing: Some(self.typing(&target)),
            },
        );
        let text = self.prompt_text(&row, &request, !run.pipe_tools).await;
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
                // the harness itself when it does not.
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
        let request_id = row.request_id.clone();
        let replied = self
            .store(move |store, agent| store.agent_reply_get(agent, &request_id))
            .await
            .ok()
            .flatten()
            .is_some();
        if replied {
            return;
        }
        match result {
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
                match response.get("stopReason").and_then(Value::as_str) {
                    Some("cancelled") => self.fail(row, FailureKind::NoReply).await,
                    _ if !text.is_empty() => {
                        self.reply(row, &turn.request, &text, &[], ReplyOrigin::Text, None)
                            .await;
                    }
                    Some("refusal") => self.fail(row, FailureKind::Refused).await,
                    _ => self.fail(row, FailureKind::NoReply).await,
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

    async fn prompt_text(&self, row: &InboxRow, request: &Event, foreign: bool) -> String {
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
            instructions: foreign.then_some(self.spec.local.instructions.as_str()),
        })
    }

    /// The agent's typing indicator, until the returned guard is dropped.
    fn typing(&self, target: &ReplyTarget) -> AbortOnDrop {
        let relay = self.relay.clone();
        let target = target.clone();
        let interval = self.limits().typing_interval;
        AbortOnDrop(tokio::spawn(async move {
            loop {
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
            return Reply::Already(claim.reply.event_id);
        }
        self.publish_stored(&row.request_id, event).await
    }

    /// Publishes a reply the ledger holds and records what the relay said.
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
                // The relay has none of it: the request is open again.
                let (request_id, discarded) = (request_id.to_owned(), event_id.clone());
                let _ = self
                    .store(move |store, agent| {
                        store.agent_reply_discard(agent, &request_id, &discarded)
                    })
                    .await;
                Reply::Refused(message)
            }
            // It stays stored and is published again, unchanged, later.
            Published::Unknown => Reply::Pending(event_id),
        }
    }

    /// Publishes again, unchanged, every stored reply the relay has not
    /// acknowledged. A relay that already has one keeps a single copy.
    async fn republish(&self, older_than_seconds: u64) {
        let Ok(pending) = self
            .store(move |store, agent| store.agent_replies_unpublished(agent, older_than_seconds))
            .await
        else {
            return;
        };
        for reply in pending {
            let Ok(event) = Event::from_json(&reply.event_json) else {
                continue;
            };
            if let Reply::Refused(_) = self.publish_stored(&reply.request_id, event).await {
                // It can never be delivered: the request ends as failed.
                let request_id = reply.request_id.clone();
                let _ = self
                    .store(move |store, agent| {
                        store.agent_inbox_settle(
                            agent,
                            &request_id,
                            RequestState::Failed,
                            Some("reply-refused"),
                        )
                    })
                    .await;
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
            Reply::Refused(_) | Reply::Failed => settle(kind.as_str()).await,
            Reply::Sent(_) | Reply::Already(_) | Reply::Pending(_) => {}
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
                Reply::Already(id) => ToolReply::final_reply(format!(
                    "already sent as {id}: this message has its final reply, and nothing new was posted"
                )),
                Reply::Pending(id) => ToolReply::final_reply(format!(
                    "sent as {id}; the relay has not confirmed it yet and bzz will keep trying"
                )),
                Reply::Refused(why) => ToolReply::refused(format!(
                    "The relay refused the message: {why}. Nothing was sent."
                )),
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
            Published::Refused(why) => ToolReply::refused(format!(
                "The relay refused the message: {why}. Nothing was sent."
            )),
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
            Published::Refused(why) => {
                ToolReply::refused(format!("The relay refused the reaction: {why}. {NOTHING}"))
            }
        }
    }
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
            selected("never")
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
        // A flood of text is kept only up to the limit.
        turn.push_text(&"y".repeat(MAX_PLAIN_TEXT_CHARS * 3));
        assert_eq!(turn.last_text().chars().count(), MAX_PLAIN_TEXT_CHARS);
    }
}
