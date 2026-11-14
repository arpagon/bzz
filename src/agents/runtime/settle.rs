//! An agent's two files made whole for a move: bzz starts the agent's own
//! host for a moment, asks it to checkpoint (`_bzz/checkpoint`,
//! `docs/agent-host-protocol.md`, section 3) and stops it. Afterwards
//! `memory.sqlite` and `files.sqlite` are complete by themselves: no
//! write-ahead file is left beside either.
//!
//! The host is started with no granted folder, no allowed host, no model
//! and Pi packages off: it answers nothing here and runs no code of the
//! owner's. The caller holds the agent ([`super::hold`]), so no other bzz
//! starts it meanwhile.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    agents::runtime::{
        acp::Connection,
        agent::Environment,
        install,
        process::{self, Harness, Launch},
    },
    error::{Error, Result},
};

/// The two files a move carries, in the order they are packed.
pub const FILES: [&str; 2] = ["memory.sqlite", "files.sqlite"];

/// How long each step may take.
#[derive(Clone, Copy, Debug)]
pub struct SettleLimits {
    pub start: Duration,
    pub checkpoint: Duration,
    pub stop_grace: Duration,
}

impl Default for SettleLimits {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(60),
            checkpoint: Duration::from_secs(120),
            stop_grace: process::STOP_GRACE,
        }
    }
}

/// What to start instead of bzz's installed host, for a test: the program
/// and its first arguments. The agent's folder is added.
pub type HostCommand = Option<Vec<String>>;

/// How bzz reaches the agent's host to make files whole.
#[derive(Clone)]
pub struct HostAccess {
    /// How bzz's own environment is read, to find Node.
    pub environment: Environment,
    /// Pi's own folder, told to the host; never read here.
    pub pi_dir: Option<PathBuf>,
    pub command: HostCommand,
    pub limits: SettleLimits,
}

impl HostAccess {
    /// The installed host, found through this process's environment.
    pub fn of_this_process() -> Self {
        Self {
            environment: std::sync::Arc::new(|name| std::env::var(name).ok()),
            pi_dir: install::pi_dir(|name| std::env::var(name).ok()),
            command: None,
            limits: SettleLimits::default(),
        }
    }
}

/// Write-ahead files that are beside the agent's two files now.
pub fn write_ahead_files(folder: &Path) -> Vec<PathBuf> {
    FILES
        .iter()
        .flat_map(|name| [format!("{name}-wal"), format!("{name}-shm")])
        .map(|name| folder.join(name))
        .filter(|path| path.exists())
        .collect()
}

/// Makes the agent's two files whole, in its own folder. On success both
/// exist and nothing else of SQLite's is beside them.
pub async fn settle(data_dir: &Path, agent: Uuid, name: &str, host: &HostAccess) -> Result<()> {
    let folder = process::agent_dir(data_dir, agent);
    settle_folder(data_dir, &folder, agent, name, host).await
}

/// The same for `folder`, wherever it is: an agent's own folder, or the
/// files of one that arrives with their write-ahead files, as a host that
/// was not stopped cleanly leaves them.
pub async fn settle_folder(
    data_dir: &Path,
    folder: &Path,
    agent: Uuid,
    name: &str,
    host: &HostAccess,
) -> Result<()> {
    let HostAccess {
        environment,
        pi_dir,
        command,
        limits,
    } = host;
    let limits = *limits;
    let folder = folder.to_path_buf();
    process::create_private_dir(&folder)?;
    let lookup = |variable: &str| environment(variable);
    let (program, mut args) = match command.as_deref().and_then(<[String]>::split_first) {
        Some((program, arguments)) => (program.clone(), arguments.to_vec()),
        None => {
            let unavailable = |problem: install::Problem| {
                Error::Config(format!(
                    "the agent's files cannot be made ready for a move without its host: {problem}"
                ))
            };
            let node = install::find_node(lookup).await.map_err(unavailable)?;
            let installed = install::ensure_installed(data_dir, &node, lookup)
                .await
                .map_err(unavailable)?;
            (
                node.program.display().to_string(),
                vec![installed.join("main.mjs").display().to_string()],
            )
        }
    };
    args.push(folder.display().to_string());
    let launch = Launch {
        program,
        args,
        env: process::base_environment(&folder, lookup),
        cwd: folder.clone(),
        // Not the agent's own log: that one keeps what its last run wrote.
        log: folder.join("move.log"),
    };
    // Its ownership record is kept apart from those of running agents: the
    // hold is what keeps other bzz processes away, and a bzz that starts
    // meanwhile must not take the agent for one that another bzz runs.
    let run_dir = data_dir.join("agents").join("moving");
    let (harness, stdin, stdout) = Harness::spawn(&launch, &run_dir, agent)?;
    let (connection, _incoming) = Connection::start(stdout, stdin);
    let mut bzz = json!({
        "agent": {"id": agent, "name": name},
        "instructions": "",
        "mounts": [],
        "network": [],
        "packages": false,
    });
    if let Some(pi_dir) = pi_dir {
        bzz["piDir"] = json!(pi_dir);
    }
    let asked = async {
        let started = tokio::time::timeout(
            limits.start,
            connection.request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {},
                    "clientInfo": {"name": "bzz", "version": env!("CARGO_PKG_VERSION")},
                    "_meta": {"bzz": bzz},
                }),
            ),
        )
        .await
        .map_err(|_| "its host did not start in time".to_owned())?
        .map_err(|error| format!("its host did not start: {error}"))?;
        if started.pointer("/_meta/bzz/tools").and_then(Value::as_str) != Some("pipe") {
            return Err("what started is not bzz's own host".to_owned());
        }
        tokio::time::timeout(
            limits.checkpoint,
            connection.request("_bzz/checkpoint", json!({})),
        )
        .await
        .map_err(|_| "its host did not finish its writes in time".to_owned())?
        .map_err(|error| format!("its host could not finish its writes: {error}"))?;
        Ok::<_, String>(())
    }
    .await;
    let closing = connection.clone();
    harness
        .stop(move || closing.close_input(), limits.stop_grace)
        .await;
    let _ = std::fs::remove_dir(&run_dir);
    asked.map_err(|why| {
        Error::Config(format!(
            "the agent's files could not be made ready for a move: {why}. Its host's own words are in {}",
            launch.log.display()
        ))
    })?;
    for file in FILES {
        if !folder.join(file).is_file() {
            return Err(Error::Config(format!(
                "the agent's host left no {file} in {}",
                folder.display()
            )));
        }
    }
    let _ = std::fs::remove_file(folder.join("move.log"));
    if let Some(left) = write_ahead_files(&folder).first() {
        return Err(Error::Config(format!(
            "a write-ahead file is still beside the agent's files ({}): something else has them open",
            left.display()
        )));
    }
    Ok(())
}
