//! Every local agent of one community, for as long as its owner's identity
//! is unlocked: in the TUI beside the broker, or as `bzz agent run`.
//!
//! Starting a host stops what a dead bzz left running, before anything else;
//! adding an agent settles what its last process left open, before it may
//! start again. An agent that is paused, moved away, or run by another live
//! bzz is listed and never started.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use nostr::Event;
use serde_json::Value;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    agents::git_credentials::GitCredentials,
    agents::runtime::{
        Limits,
        admission::{ChannelFacts, Refusal},
        agent::{Agent, AgentSpec, AgentStatus, Environment, Phase, Shared},
        approval::{Answer, Approvals, Asked},
        credentials::{Credentials, Refused},
        endpoint::{self, Denied, ToolsServer},
        hold, install,
        process::{self, Leftover},
        relay::{AgentRelay, RelayPort},
        tools::ToolReply,
    },
    config::{HostedAgentConfig, LocalAgentState},
    error::{Error, Result},
    store::writer::StoreHandle,
};

pub struct HostOptions {
    pub data_dir: PathBuf,
    pub store: StoreHandle,
    pub limits: Limits,
    /// The bzz a harness starts as `bzz agent-tools`: this program when
    /// absent.
    pub tools_program: Option<PathBuf>,
    /// Pi's own configuration folder, told to bzz's host; never read here.
    pub pi_dir: Option<PathBuf>,
    /// How bzz's own environment is read, to find Node and to build a
    /// harness's environment. Tests give their own.
    pub environment: Environment,
    /// What the owner stored for private git remotes, opened with his key.
    /// `None`: an agent that asks for a credential is told bzz has none.
    pub git_credentials: Option<Arc<dyn GitCredentials>>,
    /// Whether someone is there to answer when an agent asks to run a real
    /// command: the TUI is, `bzz agent run` is not.
    pub owner_present: bool,
    /// The settings file, read again before a command is allowed or a
    /// credential given, so that what the owner took away since this bzz
    /// started is refused at once.
    pub settings_file: Option<PathBuf>,
}

impl HostOptions {
    pub fn new(data_dir: PathBuf, store: StoreHandle) -> Self {
        Self {
            data_dir,
            store,
            limits: Limits::default(),
            tools_program: None,
            pi_dir: install::pi_dir(|name| std::env::var(name).ok()),
            environment: Arc::new(|name| std::env::var(name).ok()),
            git_credentials: None,
            owner_present: false,
            settings_file: None,
        }
    }
}

/// What became of an event offered to an agent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Offer {
    /// Admitted: the agent will answer it.
    Queued,
    /// Admitted before.
    Duplicate,
    Refused(Refusal),
    /// This host does not run that agent.
    NotRunning,
}

type Agents = Arc<Mutex<HashMap<Uuid, Arc<Agent>>>>;

struct Inner {
    shared: Arc<Shared>,
    agents: Agents,
    /// Agents of the configuration this host does not run, and why.
    idle: Mutex<Vec<AgentStatus>>,
    leftovers: Vec<Leftover>,
    intakes: Mutex<Vec<(Arc<Notify>, tokio::task::JoinHandle<()>)>>,
    /// Set by `shutdown`: nothing is added or listened to after it.
    closed: std::sync::atomic::AtomicBool,
    _server: ToolsServer,
}

#[derive(Clone)]
pub struct AgentHost(Arc<Inner>);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn agent_of(agents: &Agents, id: Uuid) -> Option<Arc<Agent>> {
    lock(agents).get(&id).cloned()
}

impl AgentHost {
    /// Stops the leftovers of a bzz that is gone, then opens the loopback
    /// endpoint of `bzz agent-tools`. No agent runs yet.
    pub async fn start(options: HostOptions) -> Result<Self> {
        let run_dir = process::run_dir(&options.data_dir);
        let leftovers = process::sweep_with_grace(&run_dir, options.limits.stop_grace).await;
        // Installs of the host that an older bzz made, once nothing runs
        // from them.
        install::remove_unused_installs(&options.data_dir);
        let credentials = Arc::new(Credentials::default());
        let agents = Agents::default();
        let denied = |refused| match refused {
            Refused::UnknownOrRevoked => Denied::Credential,
            Refused::SessionNotOpen => Denied::NotReady,
        };
        let authorize = {
            let credentials = credentials.clone();
            Arc::new(move |authorization: Option<&str>| {
                credentials
                    .resolve(authorization)
                    .map(|_| ())
                    .map_err(denied)
            })
        };
        let handle = {
            let (credentials, agents) = (credentials.clone(), agents.clone());
            Arc::new(
                move |authorization: Option<String>, name: String, arguments: Value| {
                    // Resolved again at the moment of the call: the process may
                    // have exited since the head was read.
                    let resolved = credentials
                        .resolve(authorization.as_deref())
                        .map_err(denied);
                    let agents = agents.clone();
                    Box::pin(async move {
                        let (agent, run, session) = resolved?;
                        let agent = agent_of(&agents, agent).ok_or(Denied::NotReady)?;
                        Ok(agent.tool_call(run, &session, &name, &arguments).await)
                    }) as endpoint::Handled
                },
            )
        };
        let server = endpoint::serve(authorize, handle).await?;
        let tools_program = match options.tools_program {
            Some(program) => program,
            None => std::env::current_exe().map_err(|error| Error::io("bzz executable", error))?,
        };
        let approvals = Arc::new(Approvals::default());
        approvals.set_owner_present(options.owner_present);
        Ok(Self(Arc::new(Inner {
            shared: Arc::new(Shared {
                approvals,
                git_credentials: options.git_credentials,
                settings_file: options.settings_file,
                data_dir: options.data_dir,
                store: options.store,
                credentials,
                tools_address: server.address,
                tools_program,
                pi_dir: options.pi_dir,
                environment: options.environment,
                pi_host: tokio::sync::Mutex::new(None),
                limits: options.limits,
            }),
            agents,
            idle: Mutex::new(Vec::new()),
            leftovers,
            intakes: Mutex::new(Vec::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
            _server: server,
        })))
    }

    /// What the startup sweep found.
    pub fn leftovers(&self) -> &[Leftover] {
        &self.0.leftovers
    }

    pub fn credentials(&self) -> &Credentials {
        &self.0.shared.credentials
    }

    pub fn tools_address(&self) -> std::net::SocketAddr {
        self.0.shared.tools_address
    }

    fn note_idle(&self, agent: &HostedAgentConfig, phase: Phase, problem: &str) {
        let Some(local) = &agent.local else { return };
        lock(&self.0.idle).push(AgentStatus {
            id: agent.id,
            name: agent.label.clone(),
            harness: local.harness,
            phase,
            turns: 0,
            pid: None,
            activity: None,
            problem: Some(problem.to_owned()),
        });
    }

    /// Whether this host may run `agent`; when not, it is listed with the
    /// reason and nothing of it starts.
    pub fn runnable(&self, agent: &HostedAgentConfig) -> bool {
        let Some(local) = &agent.local else {
            return false;
        };
        let owned_elsewhere = self.0.leftovers.iter().find_map(|leftover| match leftover {
            Leftover::OwnedElsewhere {
                agent: id,
                owner_pid,
            } if *id == agent.id => Some(*owner_pid),
            _ => None,
        });
        // The agent's own folder says so too, and says it to a bzz whose
        // copy of the settings is older than the move.
        if local.state == LocalAgentState::MovedAway
            || hold::moved_away(&self.0.shared.data_dir, agent.id)
        {
            self.note_idle(agent, Phase::Stopped, hold::MOVED_AWAY);
            return false;
        }
        match (local.state, agent.paused, owned_elsewhere) {
            (LocalAgentState::Paused, ..) | (_, true, _) => {
                self.note_idle(agent, Phase::Paused, "paused by its owner");
            }
            (_, _, Some(pid)) => self.note_idle(
                agent,
                Phase::Stopped,
                &format!("another bzz runs it (process {pid})"),
            ),
            (LocalAgentState::Active | LocalAgentState::MovedAway, false, None) => return true,
        }
        false
    }

    /// Settles what the agent's last process left open, then lets it run.
    pub async fn add(&self, spec: AgentSpec, relay: Arc<dyn AgentRelay>) {
        let agent = Agent::new(spec, relay, self.0.shared.clone());
        agent.recover().await;
        if self.closed() {
            return;
        }
        lock(&self.0.agents).insert(agent.spec.id, agent.clone());
        agent.start();
    }

    fn closed(&self) -> bool {
        self.0.closed.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Asks an agent's own host to checkpoint its files now (before a move).
    pub async fn checkpoint(&self, agent: Uuid) -> bool {
        match agent_of(&self.0.agents, agent) {
            Some(agent) => agent.checkpoint().await,
            None => false,
        }
    }

    /// Brings the agent its requests from its own relay connection.
    pub fn listen(&self, port: Arc<RelayPort>, agent: Uuid, owner: String) {
        if self.closed() {
            return;
        }
        let stop = Arc::new(Notify::new());
        let task = tokio::spawn(crate::agents::runtime::relay::intake(
            port,
            self.clone(),
            self.0.shared.store.clone(),
            agent,
            owner,
            stop.clone(),
        ));
        lock(&self.0.intakes).push((stop, task));
    }

    /// Offers an event to an agent. Admission decides.
    pub async fn offer(&self, agent: Uuid, event: &Event, channel: &ChannelFacts) -> Offer {
        // A paused agent does not listen: what is said meanwhile is not
        // answered later.
        let Some(agent) = agent_of(&self.0.agents, agent).filter(|agent| agent.listening()) else {
            return Offer::NotRunning;
        };
        match agent.offer(event, channel).await {
            Ok(true) => Offer::Queued,
            Ok(false) => Offer::Duplicate,
            Err(refusal) => Offer::Refused(refusal),
        }
    }

    /// The real commands that wait for the owner's answer, oldest first.
    pub fn approvals(&self) -> Vec<Asked> {
        self.0.shared.approvals.waiting()
    }

    /// The owner's answer to the question `id`. `false`: it no longer
    /// waits (answered, timed out, or void), and nothing happens.
    pub fn answer_approval(&self, id: u64, answer: Answer) -> bool {
        self.0.shared.approvals.answer(id, answer)
    }

    /// Says whether someone is at bzz to answer a question. When the owner
    /// leaves, every question that is open becomes void.
    pub fn set_owner_present(&self, present: bool) {
        self.0.shared.approvals.set_owner_present(present);
    }

    /// Whether this host has started serving `agent`.
    pub fn runs(&self, agent: Uuid) -> bool {
        lock(&self.0.agents).contains_key(&agent)
    }

    /// One line of state per agent, running or not.
    pub fn status(&self) -> Vec<AgentStatus> {
        let mut all = lock(&self.0.agents)
            .values()
            .map(|agent| agent.status())
            .collect::<Vec<_>>();
        all.extend(lock(&self.0.idle).iter().cloned());
        all.sort_by(|left, right| (&left.name, left.id).cmp(&(&right.name, right.id)));
        all
    }

    pub async fn set_paused(&self, agent: Uuid, paused: bool) {
        if let Some(agent) = agent_of(&self.0.agents, agent) {
            agent.set_paused(paused).await;
        }
    }

    /// Stops the turns an agent is running now (the owner's key in the
    /// agents panel). Returns how many there were.
    pub async fn stop_turns(&self, agent: Uuid) -> usize {
        match agent_of(&self.0.agents, agent) {
            Some(agent) => agent.stop_turns().await,
            None => 0,
        }
    }

    /// One tool call as `bzz agent-tools` makes it, without the socket.
    pub async fn tool_call(
        &self,
        authorization: Option<&str>,
        name: &str,
        arguments: &Value,
    ) -> std::result::Result<ToolReply, Refused> {
        let (agent, run, session) = self.0.shared.credentials.resolve(authorization)?;
        let agent = agent_of(&self.0.agents, agent).ok_or(Refused::UnknownOrRevoked)?;
        Ok(agent.tool_call(run, &session, name, arguments).await)
    }

    /// Stops every harness: end of input, TERM, KILL. Requests in flight
    /// stay in the inbox for the next start.
    pub async fn shutdown(&self) {
        self.0
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        // bzz is closing: nobody answers any more, and nothing is asked.
        self.0.shared.approvals.set_owner_present(false);
        for (stop, task) in lock(&self.0.intakes).drain(..) {
            stop.notify_one();
            // The intake holds a clone of this host; it must not outlive it.
            task.abort();
        }
        let agents = lock(&self.0.agents)
            .drain()
            .map(|(_, agent)| agent)
            .collect::<Vec<_>>();
        let mut stopping = tokio::task::JoinSet::new();
        for agent in agents {
            stopping.spawn(async move { agent.stop().await });
        }
        while stopping.join_next().await.is_some() {}
    }
}
