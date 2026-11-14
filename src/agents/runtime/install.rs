//! bzz's own Pi Durable host: its files, which are inside this binary, the
//! Node that runs it, and its install on first use (ADR v0.15 local agents,
//! D1 and rule 13).
//!
//! - The host's sources and its lock file are compiled into bzz
//!   ([`HOST_FILES`]). Nothing is downloaded but the packages the lock file
//!   names, with install scripts off.
//! - An install lives in `<data>/agent-host/<id>/`, where `<id>` names the
//!   exact files of this bzz. It is built in a folder of its own and renamed
//!   into place only when complete, so a folder with that name is always a
//!   whole install, whoever made it and whatever was killed on the way.
//! - Nothing here starts an agent. `npm` is the owner's own program and runs
//!   with a small environment that holds nothing of bzz's.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{agents::runtime::process, config::AgentModelConfig};

/// The oldest Node the host runs on (`agent-host/package.json`, `engines`).
pub const MIN_NODE: (u32, u32) = (22, 19);

macro_rules! host_files {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../../agent-host/", $path)))),*]
    };
}

/// Every file the host reads when it runs, by its path in the host's folder.
/// `package.json` sits beside `main.mjs`: the host reads its own version
/// from it.
pub const HOST_FILES: &[(&str, &str)] = host_files![
    "main.mjs",
    "package.json",
    "package-lock.json",
    "src/adapters.mjs",
    "src/disk.mjs",
    "src/env.mjs",
    "src/git.mjs",
    "src/granted.mjs",
    "src/host.mjs",
    "src/model.mjs",
    "src/network.mjs",
    "src/packages.mjs",
    "src/remote.mjs",
    "src/rpc.mjs",
    "src/run-child.mjs",
    "src/run.mjs",
    "src/vfs.mjs",
];

/// What `agent-host/` holds that a running host never reads: a file, or a
/// folder when it ends in `/`. A test fails for a file that is neither here
/// nor in [`HOST_FILES`].
pub const NOT_NEEDED_AT_RUN_TIME: &[&str] = &[
    "README.md",
    ".gitattributes",
    ".gitignore",
    "test/",
    "node_modules/",
    "work/",
];

const MARKER: &str = ".bzz-installed";
const TEMPORARY_PREFIX: &str = ".installing-";
/// A folder of an install that was never finished is removed after this.
const STALE_INSTALL: Duration = Duration::from_secs(3600);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// SHA-256 over every embedded file with its path, as hex.
pub fn host_digest() -> String {
    let mut files = HOST_FILES.to_vec();
    files.sort_by_key(|(path, _)| *path);
    let mut hash = Sha256::new();
    for (path, content) in files {
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update((content.len() as u64).to_be_bytes());
        hash.update(content.as_bytes());
    }
    hex::encode(hash.finalize())
}

/// The name of this bzz's install: the first 16 characters of
/// [`host_digest`].
pub fn host_id() -> String {
    host_digest()[..16].to_owned()
}

/// `<data>/agent-host/`.
pub fn host_root(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-host")
}

/// `<data>/agent-host/<id>/`: where this bzz's host is, once installed.
pub fn host_dir(data_dir: &Path) -> PathBuf {
    host_root(data_dir).join(host_id())
}

/// Pi's own folder, as the `pi` command finds it: `PI_CODING_AGENT_DIR`, or
/// `~/.pi/agent`. bzz tells its host where it is and never reads it.
pub fn pi_dir(lookup: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    match lookup("PI_CODING_AGENT_DIR").filter(|value| !value.trim().is_empty()) {
        Some(value) => Some(match (value.strip_prefix('~'), &home) {
            (Some(""), Some(home)) => home.clone(),
            (Some(rest), Some(home)) if rest.starts_with(['/', '\\']) => home.join(&rest[1..]),
            _ => PathBuf::from(value),
        }),
        None => home.map(|home| home.join(".pi").join("agent")),
    }
}

/// A Node that can run the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub program: PathBuf,
    pub version: (u32, u32, u32),
}

impl Node {
    pub fn version_text(&self) -> String {
        format!("{}.{}.{}", self.version.0, self.version.1, self.version.2)
    }
}

/// Why the host cannot be run or installed. Every text says what to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    NodeMissing,
    NodeTooOld {
        program: PathBuf,
        version: String,
    },
    /// `BZZ_NODE` names something that is not a working Node.
    NodeUnusable {
        program: PathBuf,
    },
    NpmMissing {
        node: PathBuf,
    },
    InstallFailed {
        log: PathBuf,
        why: String,
    },
    Io {
        path: PathBuf,
        why: String,
    },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let wanted = format!("{}.{}", MIN_NODE.0, MIN_NODE.1);
        match self {
            Self::NodeMissing => write!(
                formatter,
                "an agent on the pi-durable harness needs Node.js {wanted} or newer, and no `node` was found on PATH. Install Node.js (https://nodejs.org or your package manager), or set BZZ_NODE to the path of one, then run `bzz agent doctor`"
            ),
            Self::NodeTooOld { program, version } => write!(
                formatter,
                "Node.js {version} at {} is older than {wanted}, which an agent on the pi-durable harness needs. Install a newer Node.js, or set BZZ_NODE to the path of one, then run `bzz agent doctor`",
                program.display()
            ),
            Self::NodeUnusable { program } => write!(
                formatter,
                "{} did not answer `--version` as Node.js does. Set BZZ_NODE to the path of Node.js {wanted} or newer, or unset it to use the one on PATH",
                program.display()
            ),
            Self::NpmMissing { node } => write!(
                formatter,
                "npm was not found beside {} or on PATH. It comes with Node.js: install it, then try again",
                node.display()
            ),
            Self::InstallFailed { log, why } => write!(
                formatter,
                "installing the Pi Durable host failed ({why}). npm's output is in {}. The install needs the npm registry once: check the network and try again. Nothing was left half installed",
                log.display()
            ),
            Self::Io { path, why } => write!(
                formatter,
                "the Pi Durable host could not be installed in {}: {why}. Check that the folder can be written and has room",
                path.display()
            ),
        }
    }
}

fn io_problem(path: &Path, error: &std::io::Error) -> Problem {
    Problem::Io {
        path: path.to_path_buf(),
        why: error.to_string(),
    }
}

/// `v22.19.0` or `22.19.0-nightly…` as numbers.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut numbers = text
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u32>().ok());
    Some((numbers.next()??, numbers.next()??, numbers.next()??))
}

fn new_enough(version: (u32, u32, u32)) -> bool {
    (version.0, version.1) >= MIN_NODE
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_owned()]
    }
}

/// Every program called `name` in the folders of `PATH`, in their order.
fn on_path(name: &str, lookup: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let Some(path) = lookup("PATH") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for folder in std::env::split_paths(&path) {
        for file in executable_names(name) {
            let candidate = folder.join(file);
            if candidate.is_file() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// The environment of a program of the owner's that bzz runs for the
/// install: what it needs to start, reach the registry and find its cache.
/// Nothing of bzz's own, and no variable that is not named here.
fn tool_environment(
    lookup: &impl Fn(&str) -> Option<String>,
    first_on_path: Option<&Path>,
) -> Vec<(String, String)> {
    const PASSED: &[&str] = &[
        "HOME",
        "LANG",
        "TMPDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "SystemRoot",
        "SystemDrive",
        "windir",
        "ComSpec",
        "PATHEXT",
        "ProgramFiles",
        "ProgramData",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        // Where npm keeps its cache and which registry it asks, when the
        // owner set them.
        "npm_config_cache",
        "npm_config_registry",
        "npm_config_userconfig",
        "npm_config_offline",
        "npm_config_prefer_offline",
        "NPM_CONFIG_CACHE",
        "NPM_CONFIG_REGISTRY",
        "NPM_CONFIG_USERCONFIG",
    ];
    let mut env = PASSED
        .iter()
        .filter_map(|name| lookup(name).map(|value| ((*name).to_owned(), value)))
        .collect::<Vec<_>>();
    let mut folders = first_on_path
        .map(|folder| vec![folder.to_path_buf()])
        .unwrap_or_default();
    if let Some(path) = lookup("PATH") {
        folders.extend(std::env::split_paths(&path));
    }
    if let Ok(path) = std::env::join_paths(folders) {
        env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    env
}

async fn output_of(
    program: &Path,
    args: &[&str],
    env: &[(String, String)],
    limit: Duration,
) -> Option<std::process::Output> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    tokio::time::timeout(limit, command.output())
        .await
        .ok()?
        .ok()
}

async fn version_of(
    program: &Path,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Option<(u32, u32, u32)> {
    let env = tool_environment(lookup, program.parent());
    let output = output_of(program, &["--version"], &env, PROBE_TIMEOUT).await?;
    output
        .status
        .success()
        .then(|| parse_version(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

/// Finds a Node that can run the host: the one `BZZ_NODE` names, or the
/// first on `PATH` that is new enough. `lookup` reads bzz's environment.
pub async fn find_node(lookup: impl Fn(&str) -> Option<String>) -> Result<Node, Problem> {
    if let Some(named) = lookup("BZZ_NODE").filter(|value| !value.trim().is_empty()) {
        let program = PathBuf::from(named);
        return match version_of(&program, &lookup).await {
            Some(version) if new_enough(version) => Ok(Node { program, version }),
            Some(version) => Err(Problem::NodeTooOld {
                program,
                version: format!("{}.{}.{}", version.0, version.1, version.2),
            }),
            None => Err(Problem::NodeUnusable { program }),
        };
    }
    let mut too_old = None;
    for program in on_path("node", &lookup) {
        match version_of(&program, &lookup).await {
            Some(version) if new_enough(version) => return Ok(Node { program, version }),
            Some(version) if too_old.is_none() => {
                too_old = Some(Problem::NodeTooOld {
                    program,
                    version: format!("{}.{}.{}", version.0, version.1, version.2),
                });
            }
            _ => {}
        }
    }
    Err(too_old.unwrap_or(Problem::NodeMissing))
}

/// The npm of `node`: the one beside it, or the first on `PATH`.
pub fn find_npm(node: &Node, lookup: impl Fn(&str) -> Option<String>) -> Result<PathBuf, Problem> {
    let beside = node.program.parent().into_iter().flat_map(|folder| {
        if cfg!(windows) {
            vec![folder.join("npm.cmd")]
        } else {
            vec![folder.join("npm")]
        }
    });
    beside
        .filter(|candidate| candidate.is_file())
        .chain(on_path("npm", &lookup))
        .next()
        .ok_or_else(|| Problem::NpmMissing {
            node: node.program.clone(),
        })
}

pub async fn npm_version(
    npm: &Path,
    node: &Node,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let env = tool_environment(&lookup, node.program.parent());
    let output = output_of(npm, &["--version"], &env, PROBE_TIMEOUT).await?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|version| !version.is_empty())
}

#[derive(Debug, Deserialize, Serialize)]
struct Marker {
    digest: String,
    node: String,
    installed_at: u64,
}

fn marker_of(folder: &Path) -> Option<Marker> {
    serde_json::from_slice(&std::fs::read(folder.join(MARKER)).ok()?).ok()
}

/// Whether `<data>/agent-host/<id>/` is a whole install of this bzz's host.
pub fn is_installed(data_dir: &Path) -> bool {
    let folder = host_dir(data_dir);
    marker_of(&folder).is_some_and(|marker| marker.digest == host_digest())
        && folder.join("main.mjs").is_file()
        && folder.join("node_modules").is_dir()
}

fn write_host_files(folder: &Path) -> Result<(), Problem> {
    for (path, content) in HOST_FILES {
        let file = folder.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_problem(parent, &error))?;
        }
        std::fs::write(&file, content).map_err(|error| io_problem(&file, &error))?;
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Removes folders of installs that were never finished and are old enough
/// that nothing can still be writing them.
fn remove_stale_attempts(root: &Path, older_than: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .file_name()
            .to_string_lossy()
            .starts_with(TEMPORARY_PREFIX)
            && entry
                .metadata()
                .and_then(|file| file.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= older_than);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// How an install is run. Tests shorten the limits.
#[derive(Clone, Copy, Debug)]
pub struct InstallLimits {
    pub timeout: Duration,
    pub stale_attempt: Duration,
}

impl Default for InstallLimits {
    fn default() -> Self {
        Self {
            timeout: INSTALL_TIMEOUT,
            stale_attempt: STALE_INSTALL,
        }
    }
}

/// The host's folder, installed now if it is not: the files from this
/// binary, then `npm ci --ignore-scripts --omit=optional --omit=dev`, and
/// only then the name under which the folder counts.
pub async fn ensure_installed(
    data_dir: &Path,
    node: &Node,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, Problem> {
    ensure_installed_with(data_dir, node, lookup, InstallLimits::default()).await
}

pub async fn ensure_installed_with(
    data_dir: &Path,
    node: &Node,
    lookup: impl Fn(&str) -> Option<String>,
    limits: InstallLimits,
) -> Result<PathBuf, Problem> {
    let folder = host_dir(data_dir);
    if is_installed(data_dir) {
        return Ok(folder);
    }
    let npm = find_npm(node, &lookup)?;
    let root = host_root(data_dir);
    process::create_private_dir(&root).map_err(|error| Problem::Io {
        path: root.clone(),
        why: error.to_string(),
    })?;
    remove_stale_attempts(&root, limits.stale_attempt);
    // A folder with the final name and no marker was not made by a rename
    // of a whole install: it is not used and not kept.
    if folder.exists() && !is_installed(data_dir) {
        std::fs::remove_dir_all(&folder).map_err(|error| io_problem(&folder, &error))?;
    }
    let attempt = root.join(format!(
        "{TEMPORARY_PREFIX}{}-{}-{:016x}",
        host_id(),
        std::process::id(),
        rand::random::<u64>()
    ));
    process::create_private_dir(&attempt).map_err(|error| Problem::Io {
        path: attempt.clone(),
        why: error.to_string(),
    })?;
    let result = install_into(&attempt, &folder, &root, &npm, node, &lookup, limits).await;
    if result.is_err() || attempt.exists() {
        let _ = std::fs::remove_dir_all(&attempt);
    }
    result.map(|()| folder)
}

async fn install_into(
    attempt: &Path,
    folder: &Path,
    root: &Path,
    npm: &Path,
    node: &Node,
    lookup: &impl Fn(&str) -> Option<String>,
    limits: InstallLimits,
) -> Result<(), Problem> {
    write_host_files(attempt)?;
    let log = root.join("install.log");
    let failed = |why: String| Problem::InstallFailed {
        log: log.clone(),
        why,
    };
    let mut command = tokio::process::Command::new(npm);
    command
        .args([
            "ci",
            "--ignore-scripts",
            "--omit=optional",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
        ])
        .current_dir(attempt)
        .env_clear()
        .envs(
            tool_environment(lookup, node.program.parent())
                .iter()
                .map(|(name, value)| (name, value)),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(limits.timeout, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(failed(format!("npm could not be started: {error}"))),
        Err(_) => {
            return Err(failed(format!(
                "npm did not finish in {} s",
                limits.timeout.as_secs()
            )));
        }
    };
    let mut text = output.stdout;
    text.extend_from_slice(&output.stderr);
    let _ = std::fs::write(&log, &text);
    let _ = crate::paths::set_private_permissions(&log);
    if !output.status.success() {
        return Err(failed(match output.status.code() {
            Some(code) => format!("`npm ci` ended with status {code}"),
            None => "`npm ci` was stopped".into(),
        }));
    }
    if !attempt.join("node_modules").is_dir() {
        return Err(failed("`npm ci` installed nothing".into()));
    }
    let marker = serde_json::to_vec(&Marker {
        digest: host_digest(),
        node: node.version_text(),
        installed_at: unix_now(),
    })
    .unwrap_or_default();
    std::fs::write(attempt.join(MARKER), marker)
        .map_err(|error| io_problem(&attempt.join(MARKER), &error))?;
    // The rename is what makes the folder count. When another bzz finished
    // first, its folder is there and this rename fails: that one is used.
    match std::fs::rename(attempt, folder) {
        Ok(()) => Ok(()),
        Err(_) if marker_of(folder).is_some_and(|marker| marker.digest == host_digest()) => Ok(()),
        Err(error) => Err(io_problem(folder, &error)),
    }
}

/// Removes installs of other versions of the host, when no agent of this
/// data folder is running: a running host reads its files from one of them.
/// Returns what was removed.
pub fn remove_unused_installs(data_dir: &Path) -> Vec<PathBuf> {
    let run_dir = process::run_dir(data_dir);
    let any_running = std::fs::read_dir(&run_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            serde_json::from_slice::<process::OwnershipRecord>(&std::fs::read(entry.path()).ok()?)
                .ok()
        })
        .any(|record| process::is_alive(&record.process));
    if any_running {
        return Vec::new();
    }
    let root = host_root(data_dir);
    remove_stale_attempts(&root, STALE_INSTALL);
    let current = host_id();
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let other_install = entry.path().is_dir()
            && name != current
            && name.len() == 16
            && name.bytes().all(|byte| byte.is_ascii_hexdigit());
        if other_install && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed.push(entry.path());
        }
    }
    removed
}

/// What the host says of a model, asked without starting an agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelCheck {
    Available {
        provider: String,
        model_id: String,
        thinking_level: Option<String>,
    },
    /// `kind` is the host's own (`provider_unavailable`, `model_unavailable`,
    /// `sign_in_needed`, `no_model`); `message` says what to do.
    Unavailable { kind: String, message: String },
    /// The question could not be asked.
    Unknown(String),
}

/// The question of [`check_model`], asked in the host's own Node.
const MODEL_PROBE: &str = r#"
import { pathToFileURL } from "node:url";
const [modelFile, piDir, wanted] = process.argv.slice(-3);
const say = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
try {
	const { openModels } = await import(pathToFileURL(modelFile).href);
	const { choose } = await openModels({ piDir });
	const chosen = await choose(JSON.parse(wanted));
	say({ ok: true, provider: chosen.model.provider, modelId: chosen.model.modelId, thinkingLevel: chosen.thinkingLevel ?? null });
} catch (error) {
	say({ ok: false, kind: typeof error?.kind === "string" ? error.kind : "error", message: String(error?.message ?? error) });
}
"#;

/// Asks the installed host's own code whether `model` can be used with Pi's
/// login in `pi_dir`: the same check a turn makes, before anything exists.
/// It reads Pi's login as the host does and starts no agent.
pub async fn check_model(
    node: &Node,
    host: &Path,
    pi_dir: &Path,
    model: Option<&AgentModelConfig>,
    lookup: impl Fn(&str) -> Option<String>,
) -> ModelCheck {
    let wanted = match model {
        Some(model) => serde_json::json!({
            "provider": model.provider,
            "modelId": model.model_id,
            "thinkingLevel": model.thinking_level,
        }),
        None => serde_json::json!({}),
    }
    .to_string();
    // As the host itself is started: an environment built from nothing.
    let env = process::base_environment(host, &lookup);
    let model_file = host.join("src").join("model.mjs");
    let mut command = tokio::process::Command::new(&node.program);
    command
        .args(["--input-type=module", "-e", MODEL_PROBE, "--"])
        .arg(&model_file)
        .arg(pi_dir)
        .arg(&wanted)
        .current_dir(host)
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let Ok(Ok(output)) = tokio::time::timeout(PROBE_TIMEOUT, command.output()).await else {
        return ModelCheck::Unknown("the host did not answer in time".into());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(answer) = text
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    else {
        return ModelCheck::Unknown("the host gave no answer".into());
    };
    let field = |name: &str| {
        answer
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    if answer.get("ok") == Some(&serde_json::Value::Bool(true)) {
        ModelCheck::Available {
            provider: field("provider").unwrap_or_default(),
            model_id: field("modelId").unwrap_or_default(),
            thinking_level: field("thinkingLevel"),
        }
    } else {
        ModelCheck::Unavailable {
            kind: field("kind").unwrap_or_else(|| "error".into()),
            message: crate::protocol::artifact::display(
                &field("message").unwrap_or_else(|| "the model cannot be used".into()),
                400,
            ),
        }
    }
}

/// What `bzz agent doctor` reports about the host. Nothing is installed and
/// no agent is started to find it out.
#[derive(Clone, Debug)]
pub struct HostReport {
    pub node: Result<Node, Problem>,
    pub npm: Option<Result<(PathBuf, Option<String>), Problem>>,
    pub host_dir: PathBuf,
    pub installed: bool,
    pub pi_dir: Option<PathBuf>,
}

impl HostReport {
    /// Whether an agent on the pi-durable harness can start now, or will
    /// after an install that has what it needs.
    pub fn ready(&self) -> bool {
        self.node.is_ok() && (self.installed || matches!(self.npm, Some(Ok(_))))
    }
}

pub async fn report(data_dir: &Path, lookup: impl Fn(&str) -> Option<String>) -> HostReport {
    let node = find_node(&lookup).await;
    let npm = match &node {
        Ok(node) => Some(match find_npm(node, &lookup) {
            Ok(npm) => {
                let version = npm_version(&npm, node, &lookup).await;
                Ok((npm, version))
            }
            Err(problem) => Err(problem),
        }),
        Err(_) => None,
    };
    HostReport {
        node,
        npm,
        host_dir: host_dir(data_dir),
        installed: is_installed(data_dir),
        pi_dir: pi_dir(&lookup),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_of_the_host_is_embedded_or_named_as_not_needed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("agent-host");
        let mut pending = vec![root.clone()];
        let mut seen = Vec::new();
        while let Some(folder) = pending.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(&root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let not_needed = NOT_NEEDED_AT_RUN_TIME.iter().any(|name| {
                    name.strip_suffix('/').map_or(relative == *name, |folder| {
                        relative == folder || relative.starts_with(name)
                    })
                });
                if not_needed {
                    continue;
                }
                if path.is_dir() {
                    pending.push(path);
                } else {
                    seen.push(relative);
                }
            }
        }
        seen.sort();
        let mut embedded = HOST_FILES
            .iter()
            .map(|(path, _)| (*path).to_owned())
            .collect::<Vec<_>>();
        embedded.sort();
        assert_eq!(
            seen, embedded,
            "a file of agent-host/ is neither embedded in bzz (HOST_FILES) nor listed as not needed at run time (NOT_NEEDED_AT_RUN_TIME)"
        );
        for (path, content) in HOST_FILES {
            assert!(!content.is_empty(), "{path} is empty");
        }
        // The install's name follows every embedded byte.
        assert_eq!(host_id().len(), 16);
        assert!(host_digest().starts_with(&host_id()));
    }

    #[test]
    fn versions_are_read_as_node_prints_them() {
        assert_eq!(parse_version("v22.19.0\n"), Some((22, 19, 0)));
        assert_eq!(parse_version("26.5.0"), Some((26, 5, 0)));
        assert_eq!(
            parse_version("v23.0.0-nightly20240101abc"),
            Some((23, 0, 0))
        );
        assert_eq!(parse_version("node"), None);
        assert!(new_enough((22, 19, 0)) && new_enough((24, 0, 0)));
        assert!(!new_enough((22, 18, 9)) && !new_enough((20, 30, 0)));
    }

    #[test]
    fn pis_folder_is_found_as_pi_finds_it() {
        let named = pi_dir(|name| (name == "PI_CODING_AGENT_DIR").then(|| "/srv/pi".to_owned()));
        assert_eq!(named, Some(PathBuf::from("/srv/pi")));
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            assert_eq!(pi_dir(|_| None), Some(home.join(".pi").join("agent")));
            assert_eq!(
                pi_dir(|name| (name == "PI_CODING_AGENT_DIR").then(|| "~/pi-b".to_owned())),
                Some(home.join("pi-b"))
            );
        }
    }

    #[cfg(unix)]
    mod with_programs {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt as _;

        fn program(folder: &Path, name: &str, script: &str) -> PathBuf {
            std::fs::create_dir_all(folder).unwrap();
            let path = folder.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn lookup_with(path: String) -> impl Fn(&str) -> Option<String> + Clone {
            move |name: &str| match name {
                "PATH" => Some(path.clone()),
                // What must never reach a program of the install.
                "BZZ_PASSPHRASE_FD" | "NOSTR_SECRET_KEY" | "BUZZ_RELAY_URL" => {
                    Some("must not pass".into())
                }
                _ => None,
            }
        }

        fn path_of(folders: &[&Path]) -> String {
            let mut all = folders
                .iter()
                .map(|folder| folder.to_path_buf())
                .collect::<Vec<_>>();
            all.push("/usr/bin".into());
            all.push("/bin".into());
            std::env::join_paths(all)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        }

        #[tokio::test]
        async fn node_is_the_first_on_path_that_is_new_enough() {
            let temporary = tempfile::TempDir::new().unwrap();
            let (old, new) = (temporary.path().join("old"), temporary.path().join("new"));
            program(&old, "node", "echo v20.11.1");
            let wanted = program(&new, "node", "echo v22.19.0");
            // None at all.
            let nothing = temporary.path().join("empty");
            std::fs::create_dir_all(&nothing).unwrap();
            let missing =
                find_node(|name| (name == "PATH").then(|| nothing.to_string_lossy().into_owned()))
                    .await
                    .unwrap_err();
            assert_eq!(missing, Problem::NodeMissing);
            assert!(
                missing.to_string().contains("22.19") && missing.to_string().contains("BZZ_NODE")
            );
            // Only one that is too old: the error names it and its version.
            let only_old = find_node(lookup_with(
                std::env::join_paths([&old])
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ))
            .await
            .unwrap_err();
            assert!(
                matches!(&only_old, Problem::NodeTooOld { version, .. } if version == "20.11.1"),
                "{only_old:?}"
            );
            assert!(
                only_old.to_string().contains("20.11.1"),
                "{}",
                only_old.to_string()
            );
            // An old one first: the next that is new enough is used.
            let found = find_node(lookup_with(path_of(&[&old, &new])))
                .await
                .unwrap();
            assert_eq!(
                found,
                Node {
                    program: wanted.clone(),
                    version: (22, 19, 0)
                }
            );
            // BZZ_NODE names the one to use, and is not second-guessed.
            let named = program(&temporary.path().join("named"), "node-24", "echo v24.1.0");
            let path = path_of(&[&old]);
            let by_name = find_node(|name: &str| match name {
                "BZZ_NODE" => Some(named.to_string_lossy().into_owned()),
                "PATH" => Some(path.clone()),
                _ => None,
            })
            .await
            .unwrap();
            assert_eq!(by_name.program, named);
            let broken = find_node(|name: &str| {
                (name == "BZZ_NODE").then(|| "/nonexistent/node".to_owned())
            })
            .await
            .unwrap_err();
            assert!(matches!(broken, Problem::NodeUnusable { .. }));
        }

        /// A stand-in `npm ci`: it records its arguments and environment and
        /// makes `node_modules`, slowly enough for two to overlap.
        fn fake_npm(folder: &Path, body: &str) -> PathBuf {
            program(
                folder,
                "npm",
                &format!(
                    "if [ \"$1\" = --version ]; then echo 11.0.0; exit 0; fi\necho \"$@\" > npm-arguments\nenv > npm-environment\n{body}"
                ),
            )
        }

        #[tokio::test]
        async fn the_host_is_installed_once_and_only_a_whole_install_counts() {
            let temporary = tempfile::TempDir::new().unwrap();
            let bin = temporary.path().join("bin");
            let node_program = program(&bin, "node", "echo v22.19.0");
            let data = temporary.path().join("data");
            let lookup = lookup_with(path_of(&[&bin]));
            let node = find_node(lookup.clone()).await.unwrap();
            assert_eq!(node.program, node_program);
            let counter = temporary.path().join("installs");
            fake_npm(
                &bin,
                &format!(
                    "echo run >> {}\nsleep 0.3\nmkdir -p node_modules/pkg && echo ok > node_modules/pkg/index.js",
                    counter.display()
                ),
            );
            assert!(!is_installed(&data));

            // Two installs at once, as two bzz processes would start them.
            let (first, second) = tokio::join!(
                ensure_installed(&data, &node, lookup.clone()),
                ensure_installed(&data, &node, lookup.clone())
            );
            let folder = host_dir(&data);
            assert_eq!(first.unwrap(), folder);
            assert_eq!(second.unwrap(), folder);
            assert!(is_installed(&data));
            // Everything the host reads is there, byte for byte.
            for (path, content) in HOST_FILES {
                assert_eq!(
                    &std::fs::read_to_string(folder.join(path)).unwrap(),
                    content,
                    "{path}"
                );
            }
            assert!(folder.join("node_modules/pkg/index.js").is_file());
            // npm ran as rule 13 says, with nothing of bzz's in its
            // environment.
            assert_eq!(
                std::fs::read_to_string(folder.join("npm-arguments"))
                    .unwrap()
                    .trim(),
                "ci --ignore-scripts --omit=optional --omit=dev --no-audit --no-fund"
            );
            let environment = std::fs::read_to_string(folder.join("npm-environment")).unwrap();
            assert!(!environment.contains("must not pass"), "{environment}");
            assert!(!environment.contains("BZZ_"), "{environment}");
            // Nothing of the loser is left beside the install.
            let beside = std::fs::read_dir(host_root(&data))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != "install.log")
                .collect::<Vec<_>>();
            assert_eq!(beside, [host_id()]);

            // Installed: nothing runs again.
            let before = std::fs::read_to_string(&counter).unwrap();
            ensure_installed(&data, &node, lookup.clone())
                .await
                .unwrap();
            assert_eq!(std::fs::read_to_string(&counter).unwrap(), before);
        }

        #[tokio::test]
        async fn a_failed_or_interrupted_install_is_never_used() {
            let temporary = tempfile::TempDir::new().unwrap();
            let bin = temporary.path().join("bin");
            program(&bin, "node", "echo v22.19.0");
            let data = temporary.path().join("data");
            let lookup = lookup_with(path_of(&[&bin]));
            let node = find_node(lookup.clone()).await.unwrap();

            // npm fails after it has written half of its packages.
            fake_npm(
                &bin,
                "mkdir -p node_modules/half && echo 'npm error network' >&2 && exit 7",
            );
            let failed = ensure_installed(&data, &node, lookup.clone())
                .await
                .unwrap_err();
            assert!(
                matches!(&failed, Problem::InstallFailed { why, .. } if why.contains('7')),
                "{failed:?}"
            );
            let text = failed.to_string();
            assert!(
                text.contains("install.log") && text.contains("try again"),
                "{text}"
            );
            assert!(!is_installed(&data));
            assert!(
                !host_dir(&data).exists(),
                "no folder with the install's name"
            );
            assert!(
                std::fs::read_to_string(host_root(&data).join("install.log"))
                    .unwrap()
                    .contains("npm error network")
            );
            let attempts = || {
                std::fs::read_dir(host_root(&data))
                    .unwrap()
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(TEMPORARY_PREFIX)
                    })
                    .count()
            };
            assert_eq!(attempts(), 0, "the failed attempt was removed");

            // npm never finishes: the install gives up and leaves nothing.
            fake_npm(&bin, "mkdir -p node_modules/half\nsleep 30");
            let limits = InstallLimits {
                timeout: Duration::from_millis(400),
                stale_attempt: Duration::ZERO,
            };
            let stuck = ensure_installed_with(&data, &node, lookup.clone(), limits)
                .await
                .unwrap_err();
            assert!(stuck.to_string().contains("did not finish"), "{stuck}");
            assert!(!is_installed(&data) && !host_dir(&data).exists());

            // What a bzz killed in the middle leaves: a folder of its own
            // with half the files, and a folder with the install's name that
            // no rename made.
            let left = host_root(&data).join(format!("{TEMPORARY_PREFIX}{}-1-dead", host_id()));
            std::fs::create_dir_all(left.join("node_modules")).unwrap();
            std::fs::create_dir_all(host_dir(&data).join("node_modules")).unwrap();
            std::fs::write(host_dir(&data).join("main.mjs"), "half").unwrap();
            assert!(
                !is_installed(&data),
                "a folder without its marker is no install"
            );
            fake_npm(&bin, "mkdir -p node_modules/pkg");
            let folder = ensure_installed_with(&data, &node, lookup.clone(), limits)
                .await
                .unwrap();
            assert!(is_installed(&data));
            assert_eq!(
                std::fs::read_to_string(folder.join("main.mjs")).unwrap(),
                HOST_FILES[0].1,
                "the half-written file was replaced"
            );
            assert!(!left.exists(), "the leftover attempt was removed");

            // Without npm the error says so.
            std::fs::remove_file(bin.join("npm")).unwrap();
            std::fs::remove_dir_all(host_root(&data)).unwrap();
            let lonely = lookup_with(
                std::env::join_paths([&bin])
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            let no_npm = ensure_installed(&data, &node, lonely).await.unwrap_err();
            assert!(matches!(no_npm, Problem::NpmMissing { .. }), "{no_npm:?}");
        }

        #[tokio::test]
        async fn installs_of_other_versions_go_once_no_agent_runs() {
            let temporary = tempfile::TempDir::new().unwrap();
            let data = temporary.path().join("data");
            let root = host_root(&data);
            let old = root.join("0123456789abcdef");
            std::fs::create_dir_all(old.join("node_modules")).unwrap();
            std::fs::create_dir_all(host_dir(&data)).unwrap();
            std::fs::write(root.join("install.log"), "kept").unwrap();
            // A harness of this data folder is running: nothing is removed.
            let run = process::run_dir(&data);
            std::fs::create_dir_all(&run).unwrap();
            let me = process::identity_of(std::process::id()).unwrap();
            let agent = uuid::Uuid::new_v4();
            std::fs::write(
                run.join(format!("{agent}.json")),
                serde_json::to_vec(&process::OwnershipRecord {
                    agent,
                    process: me.clone(),
                    owner: me,
                })
                .unwrap(),
            )
            .unwrap();
            assert!(remove_unused_installs(&data).is_empty());
            assert!(old.exists());
            // None is running: the other version goes, this one stays.
            std::fs::remove_dir_all(&run).unwrap();
            assert_eq!(remove_unused_installs(&data), std::slice::from_ref(&old));
            assert!(!old.exists() && host_dir(&data).exists());
            assert!(root.join("install.log").exists());
        }
    }
}
