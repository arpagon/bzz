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
    agents::runtime::{
        Limits,
        admission::{ChannelFacts, Refusal},
        agent::{Agent, AgentSpec, AgentStatus, Phase, Shared},
        credentials::{Credentials, Refused},
        endpoint::{self, Denied, ToolsServer},
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
}

impl HostOptions {
    pub fn new(data_dir: PathBuf, store: StoreHandle) -> Self {
        Self {
            data_dir,
            store,
            limits: Limits::default(),
            tools_program: None,
            pi_dir: directories::BaseDirs::new()
                .map(|dirs| dirs.home_dir().join(".pi").join("agent")),
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
        Ok(Self(Arc::new(Inner {
            shared: Arc::new(Shared {
                data_dir: options.data_dir,
                store: options.store,
                credentials,
                tools_address: server.address,
                tools_program,
                pi_dir: options.pi_dir,
                limits: options.limits,
            }),
            agents,
            idle: Mutex::new(Vec::new()),
            leftovers,
            intakes: Mutex::new(Vec::new()),
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
        match (local.state, agent.paused, owned_elsewhere) {
            (LocalAgentState::MovedAway, ..) => {
                self.note_idle(agent, Phase::Stopped, "moved away: it runs elsewhere");
            }
            (LocalAgentState::Paused, ..) | (_, true, _) => {
                self.note_idle(agent, Phase::Paused, "paused by its owner");
            }
            (_, _, Some(pid)) => self.note_idle(
                agent,
                Phase::Stopped,
                &format!("another bzz runs it (process {pid})"),
            ),
            (LocalAgentState::Active, false, None) => return true,
        }
        false
    }

    /// Settles what the agent's last process left open, then lets it run.
    pub async fn add(&self, spec: AgentSpec, relay: Arc<dyn AgentRelay>) {
        let agent = Agent::new(spec, relay, self.0.shared.clone());
        agent.recover().await;
        lock(&self.0.agents).insert(agent.spec.id, agent.clone());
        agent.start();
    }

    /// Brings the agent its requests from its own relay connection.
    pub fn listen(&self, port: Arc<RelayPort>, agent: Uuid, owner: String) {
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
