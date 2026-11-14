//! The three coding harnesses: Grok, Codex and Claude Code (plan v0.15.0 M6,
//! D14 to D19; ADR v0.15 local agents, decisions 3 and 8).
//!
//! Each is the owner's own installed program with the sign-in he made there.
//! This file knows, for each one:
//!
//! - **where it is and which version it is**, and the versions bzz knows to
//!   work. Outside that range nothing is started (D16).
//! - **how it is started.** Grok speaks ACP itself. Codex and Claude Code go
//!   through their ACP adapters from npm, which bzz installs on first use
//!   from a lock file inside this binary, without install scripts and
//!   without their optional packages (their own copies of the two programs,
//!   which are not used: the adapter is pointed at the owner's).
//! - **its guard, which bzz sets and never leaves to the owner's settings.**
//!   A sandbox profile for Grok; a mode for Codex and for Claude Code. In
//!   the mode the owner's own settings name, all three write anywhere
//!   without asking (spikes README, "M6 groundwork").
//! - **how it takes** a system prompt, extra folders and a model.
//!
//! Nothing here starts a session, and nothing here reads a sign-in.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    agents::runtime::{
        install::{self, InstallLimits, Node},
        process,
    },
    config::{ExecGrant, Harness, LocalAgentConfig},
};

pub type Version = (u32, u32, u32);

fn version_text(version: Version) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

/// The versions of a program bzz starts: from the one that was tested up
/// to, and not including, `below`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    /// The version the tests ran with (`docs/validation-v0.15.0.md`, M6).
    pub from: Version,
    pub below: Version,
}

impl Range {
    pub const fn contains(&self, version: Version) -> bool {
        // Tuples do not compare in a `const fn`.
        let at_least = version.0 > self.from.0
            || (version.0 == self.from.0
                && (version.1 > self.from.1
                    || (version.1 == self.from.1 && version.2 >= self.from.2)));
        let below = version.0 < self.below.0
            || (version.0 == self.below.0
                && (version.1 < self.below.1
                    || (version.1 == self.below.1 && version.2 < self.below.2)));
        at_least && below
    }

    /// The range as an owner is told it.
    pub fn text(&self) -> String {
        format!(
            "{} (tested), and later ones below {}",
            version_text(self.from),
            version_text(self.below)
        )
    }
}

/// An ACP adapter from npm, pinned by a lock file inside this binary.
#[derive(Debug, Eq, PartialEq)]
pub struct AdapterSpec {
    /// The name of its folder in bzz's data.
    pub name: &'static str,
    pub package: &'static str,
    /// The one version bzz installs and runs.
    pub version: &'static str,
    /// `package.json` and `package-lock.json`, as committed in
    /// `agent-adapters/<name>/`.
    pub files: &'static [(&'static str, &'static str)],
    /// The variable that points the adapter at the owner's program.
    pub program_variable: &'static str,
}

/// One coding program.
#[derive(Debug, Eq, PartialEq)]
pub struct ProgramSpec {
    pub harness: Harness,
    /// Its name on `PATH`.
    pub command: &'static str,
    /// The variable of bzz's environment that names it instead.
    pub variable: &'static str,
    pub title: &'static str,
    pub range: Range,
    /// How the owner signs in to it.
    pub sign_in: &'static str,
    /// Where its own history of sessions is.
    pub history: &'static str,
    pub adapter: Option<&'static AdapterSpec>,
    /// Variables that say where its sign-in is when the owner moved it.
    home_variables: &'static [&'static str],
}

macro_rules! adapter_files {
    ($name:literal) => {
        &[
            (
                "package.json",
                include_str!(concat!("../../../agent-adapters/", $name, "/package.json")),
            ),
            (
                "package-lock.json",
                include_str!(concat!(
                    "../../../agent-adapters/",
                    $name,
                    "/package-lock.json"
                )),
            ),
        ]
    };
}

pub const CLAUDE_ADAPTER: AdapterSpec = AdapterSpec {
    name: "claude-code",
    package: "@agentclientprotocol/claude-agent-acp",
    version: "0.101.0",
    files: adapter_files!("claude-code"),
    program_variable: "CLAUDE_CODE_EXECUTABLE",
};

pub const CODEX_ADAPTER: AdapterSpec = AdapterSpec {
    name: "codex",
    package: "@agentclientprotocol/codex-acp",
    version: "2.5.0",
    files: adapter_files!("codex"),
    program_variable: "CODEX_PATH",
};

pub const GROK: ProgramSpec = ProgramSpec {
    harness: Harness::Grok,
    command: "grok",
    variable: "BZZ_GROK",
    title: "Grok Build",
    range: Range {
        from: (1, 0, 77),
        below: (1, 1, 0),
    },
    sign_in: "grok login",
    history: "`grok sessions`",
    adapter: None,
    home_variables: &["GROK_HOME"],
};

/// Codex's minor number changes with every release, and its adapter is
/// built against one of them: the range is one minor line.
pub const CODEX: ProgramSpec = ProgramSpec {
    harness: Harness::Codex,
    command: "codex",
    variable: "BZZ_CODEX",
    title: "Codex",
    range: Range {
        from: (0, 166, 0),
        below: (0, 167, 0),
    },
    sign_in: "codex login",
    history: "`codex resume`",
    adapter: Some(&CODEX_ADAPTER),
    home_variables: &["CODEX_HOME"],
};

pub const CLAUDE: ProgramSpec = ProgramSpec {
    harness: Harness::ClaudeCode,
    command: "claude",
    variable: "BZZ_CLAUDE",
    title: "Claude Code",
    range: Range {
        from: (2, 1, 322),
        below: (2, 2, 0),
    },
    sign_in: "claude auth login",
    history: "`claude --resume`",
    adapter: Some(&CLAUDE_ADAPTER),
    home_variables: &["CLAUDE_CONFIG_DIR"],
};

/// The program of a coding harness. `None` for bzz's own host.
pub const fn spec(harness: Harness) -> Option<&'static ProgramSpec> {
    match harness {
        Harness::PiDurable => None,
        Harness::Grok => Some(&GROK),
        Harness::Codex => Some(&CODEX),
        Harness::ClaudeCode => Some(&CLAUDE),
    }
}

/// The name of the sandbox profile bzz writes for Grok.
pub const GROK_PROFILE: &str = "bzz-agent";
/// The mode bzz sets on every Codex session: writes in its folders without
/// asking, and asks before it leaves them.
pub const CODEX_MODE: &str = "workspace-write";
/// The mode bzz sets on every Claude Code session: it asks before every
/// edit and every command.
pub const CLAUDE_MODE: &str = "default";

/// The mode bzz sets on every session of the harness before its first
/// prompt, new or loaded. `None`: the harness has no modes over ACP (Grok,
/// whose sandbox profile is its whole guard).
pub const fn required_mode(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Codex => Some(CODEX_MODE),
        Harness::ClaudeCode => Some(CLAUDE_MODE),
        Harness::Grok | Harness::PiDurable => None,
    }
}

/// Why a coding harness cannot be started. Every text says what to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    ProgramMissing {
        spec: &'static ProgramSpec,
    },
    /// It did not answer `--version` with a version.
    ProgramUnusable {
        spec: &'static ProgramSpec,
        path: PathBuf,
    },
    VersionOutOfRange {
        /// The program or the adapter.
        what: String,
        found: String,
        known: String,
        /// How to choose another one.
        remedy: String,
    },
    /// Node or npm, for an adapter.
    Node {
        spec: &'static ProgramSpec,
        why: String,
    },
    AdapterInstallFailed {
        adapter: &'static AdapterSpec,
        log: PathBuf,
        why: String,
    },
    /// Grok's sandbox on Linux hides bzz's keys with bubblewrap, and
    /// refuses to start without it.
    BubblewrapMissing,
    Io {
        what: &'static str,
        path: PathBuf,
        why: String,
    },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProgramMissing { spec } => write!(
                formatter,
                "an agent on the {} harness runs your own `{}` ({}), and none was found on PATH. Install it and sign in with `{}`, or set {} to its path, then run `bzz agent doctor`",
                spec.harness.as_str(),
                spec.command,
                spec.title,
                spec.sign_in,
                spec.variable
            ),
            Self::ProgramUnusable { spec, path } => write!(
                formatter,
                "{} did not answer `--version` as {} does. Set {} to the path of `{}`, or unset it to use the one on PATH",
                path.display(),
                spec.title,
                spec.variable,
                spec.command
            ),
            Self::VersionOutOfRange {
                what,
                found,
                known,
                remedy,
            } => write!(
                formatter,
                "{what} is version {found}, which is outside the versions bzz knows to work: {known}. bzz does not start it. {remedy}"
            ),
            Self::Node { spec, why } => write!(
                formatter,
                "the {} harness needs Node.js {}.{} or newer for its adapter: {why}",
                spec.harness.as_str(),
                install::MIN_NODE.0,
                install::MIN_NODE.1
            ),
            Self::AdapterInstallFailed { adapter, log, why } => write!(
                formatter,
                "installing the adapter {} {} failed ({why}). npm's output is in {}. The install needs the npm registry once: check the network and try again. Nothing was left half installed",
                adapter.package,
                adapter.version,
                log.display()
            ),
            Self::BubblewrapMissing => write!(
                formatter,
                "an agent on the grok harness needs bubblewrap (`bwrap`) on Linux, and none was found on PATH: Grok's sandbox uses it to keep bzz's keys from being read, and does not start without it. Install it (the package is `bubblewrap`), then run `bzz agent doctor`"
            ),
            Self::Io { what, path, why } => write!(
                formatter,
                "{what} could not be written in {}: {why}. Check that the folder can be written and has room",
                path.display()
            ),
        }
    }
}

/// The first `a.b.c` in what a program prints for `--version`:
/// `grok 1.0.77 (50ca579fa996) [stable]`, `2.1.322 (Claude Code)`,
/// `codex-cli 0.166.0`.
pub fn parse_program_version(text: &str) -> Option<Version> {
    text.split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .find_map(|word| {
            let mut parts = word.split('.');
            Some((
                parts.next()?.parse().ok()?,
                parts.next()?.parse().ok()?,
                parts.next()?.parse().ok()?,
            ))
        })
}

/// The owner's program, found and asked for its version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    /// As found: what bzz starts.
    pub path: PathBuf,
    /// With links followed: what an adapter is pointed at.
    pub real: PathBuf,
    pub version: Version,
}

impl Program {
    pub fn version_text(&self) -> String {
        version_text(self.version)
    }
}

/// Finds the program of `spec`, the one its variable names or the first on
/// `PATH`, and reads its version. It is not checked against the range here.
pub async fn find_program(
    spec: &'static ProgramSpec,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Program, Problem> {
    let named = lookup(spec.variable).filter(|value| !value.trim().is_empty());
    let path = match &named {
        Some(named) => PathBuf::from(named),
        None => install::on_path(spec.command, &lookup)
            .into_iter()
            .next()
            .ok_or(Problem::ProgramMissing { spec })?,
    };
    let env = install::tool_environment(&lookup, path.parent());
    let output = install::output_of(&path, &["--version"], &env, install::PROBE_TIMEOUT).await;
    let version = output
        .filter(|output| output.status.success())
        .and_then(|output| {
            parse_program_version(&String::from_utf8_lossy(&output.stdout))
                .or_else(|| parse_program_version(&String::from_utf8_lossy(&output.stderr)))
        });
    let Some(version) = version else {
        return Err(Problem::ProgramUnusable { spec, path });
    };
    Ok(Program {
        real: std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()),
        path,
        version,
    })
}

/// Refuses a program whose version is outside what bzz knows to work.
pub fn check_range(spec: &'static ProgramSpec, program: &Program) -> Result<(), Problem> {
    if spec.range.contains(program.version) {
        return Ok(());
    }
    Err(Problem::VersionOutOfRange {
        what: format!("{} at {}", spec.title, program.path.display()),
        found: program.version_text(),
        known: spec.range.text(),
        remedy: format!(
            "Install a version in that range, or set {} to the path of one, then run `bzz agent doctor`.",
            spec.variable
        ),
    })
}

// ---- the adapters -----------------------------------------------------------

impl AdapterSpec {
    /// SHA-256 over the two embedded files, as hex.
    pub fn digest(&self) -> String {
        let mut hash = Sha256::new();
        for (path, content) in self.files {
            hash.update(path.as_bytes());
            hash.update([0]);
            hash.update((content.len() as u64).to_be_bytes());
            hash.update(content.as_bytes());
        }
        hex::encode(hash.finalize())
    }

    /// `<data>/agent-adapters/<name>-<sha16>/`: a change to the lock file
    /// is another install.
    pub fn dir(&self, data_dir: &Path) -> PathBuf {
        adapters_root(data_dir).join(format!("{}-{}", self.name, &self.digest()[..16]))
    }

    fn package_dir(&self, install: &Path) -> PathBuf {
        self.package
            .split('/')
            .fold(install.join("node_modules"), |folder, part| {
                folder.join(part)
            })
    }

    /// The file Node runs.
    pub fn entry(&self, install: &Path) -> PathBuf {
        self.package_dir(install).join("dist").join("index.js")
    }

    /// The version of the package as it is installed in `install`.
    pub fn installed_version(&self, install: &Path) -> Option<String> {
        let manifest = std::fs::read(self.package_dir(install).join("package.json")).ok()?;
        serde_json::from_slice::<Value>(&manifest)
            .ok()?
            .get("version")?
            .as_str()
            .map(str::to_owned)
    }

    /// Whether its folder is a whole install of exactly the pinned version.
    pub fn is_installed(&self, data_dir: &Path) -> bool {
        let folder = self.dir(data_dir);
        marker_of(&folder).is_some_and(|marker| marker.digest == self.digest())
            && self.entry(&folder).is_file()
            && self.installed_version(&folder).as_deref() == Some(self.version)
    }
}

/// `<data>/agent-adapters/`.
pub fn adapters_root(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-adapters")
}

#[derive(Debug, Deserialize, Serialize)]
struct Marker {
    digest: String,
    node: String,
    installed_at: u64,
}

fn marker_of(folder: &Path) -> Option<Marker> {
    serde_json::from_slice(&std::fs::read(folder.join(install::MARKER)).ok()?).ok()
}

/// The adapter's folder, installed now if it is not: the two files from
/// this binary, then `npm ci --ignore-scripts --omit=optional --omit=dev`,
/// and only then the name under which the folder counts. The optional
/// packages are the adapters' own copies of `claude` and `codex` (760 MB);
/// both adapters run without them when pointed at the owner's program.
pub async fn ensure_adapter(
    data_dir: &Path,
    spec: &'static ProgramSpec,
    adapter: &'static AdapterSpec,
    node: &Node,
    lookup: impl Fn(&str) -> Option<String>,
    limits: InstallLimits,
) -> Result<PathBuf, Problem> {
    let folder = adapter.dir(data_dir);
    if adapter.is_installed(data_dir) {
        return Ok(folder);
    }
    let io = |path: &Path, why: String| Problem::Io {
        what: "the adapter",
        path: path.to_path_buf(),
        why,
    };
    let npm = install::find_npm(node, &lookup).map_err(|problem| Problem::Node {
        spec,
        why: problem.to_string(),
    })?;
    let root = adapters_root(data_dir);
    process::create_private_dir(&root).map_err(|error| io(&root, error.to_string()))?;
    install::remove_stale_attempts(&root, limits.stale_attempt);
    // A folder with the final name that is not a whole install was not
    // made by a rename: it is not used and not kept.
    if folder.exists() {
        std::fs::remove_dir_all(&folder).map_err(|error| io(&folder, error.to_string()))?;
    }
    let attempt = root.join(format!(
        "{}{}-{}-{:016x}",
        install::TEMPORARY_PREFIX,
        adapter.name,
        std::process::id(),
        rand::random::<u64>()
    ));
    process::create_private_dir(&attempt).map_err(|error| io(&attempt, error.to_string()))?;
    let result = install_adapter(
        &attempt, &folder, &root, &npm, adapter, node, &lookup, limits,
    )
    .await;
    if result.is_err() || attempt.exists() {
        let _ = std::fs::remove_dir_all(&attempt);
    }
    result.map(|()| folder)
}

#[allow(clippy::too_many_arguments)]
async fn install_adapter(
    attempt: &Path,
    folder: &Path,
    root: &Path,
    npm: &Path,
    adapter: &'static AdapterSpec,
    node: &Node,
    lookup: &impl Fn(&str) -> Option<String>,
    limits: InstallLimits,
) -> Result<(), Problem> {
    for (path, content) in adapter.files {
        let file = attempt.join(path);
        std::fs::write(&file, content).map_err(|error| Problem::Io {
            what: "the adapter",
            path: file.clone(),
            why: error.to_string(),
        })?;
    }
    let log = root.join(format!("install-{}.log", adapter.name));
    let failed = |why: String| Problem::AdapterInstallFailed {
        adapter,
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
            install::tool_environment(lookup, node.program.parent())
                .iter()
                .map(|(name, value)| (name, value)),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(limits.timeout, install::run_to_end(&mut command)).await
    {
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
    if !adapter.entry(attempt).is_file() {
        return Err(failed(format!(
            "`npm ci` did not install {}",
            adapter.package
        )));
    }
    // The lock file names one version; anything else is not what was tested.
    let installed = adapter.installed_version(attempt);
    if installed.as_deref() != Some(adapter.version) {
        return Err(Problem::VersionOutOfRange {
            what: format!("the adapter {}", adapter.package),
            found: installed.unwrap_or_else(|| "unknown".into()),
            known: adapter.version.to_owned(),
            remedy: "It was not kept. This is a fault in bzz's lock file: report it.".into(),
        });
    }
    let marker = serde_json::to_vec(&Marker {
        digest: adapter.digest(),
        node: node.version_text(),
        installed_at: install::unix_now(),
    })
    .unwrap_or_default();
    let marker_file = attempt.join(install::MARKER);
    std::fs::write(&marker_file, marker).map_err(|error| Problem::Io {
        what: "the adapter",
        path: marker_file.clone(),
        why: error.to_string(),
    })?;
    // The rename is what makes the folder count. When another bzz finished
    // first, its folder is there and this rename fails: that one is used.
    match std::fs::rename(attempt, folder) {
        Ok(()) => Ok(()),
        Err(_) if marker_of(folder).is_some_and(|marker| marker.digest == adapter.digest()) => {
            Ok(())
        }
        Err(error) => Err(Problem::Io {
            what: "the adapter",
            path: folder.to_path_buf(),
            why: error.to_string(),
        }),
    }
}

/// Removes installs of adapters that another bzz's lock file named, when
/// no agent of this data folder is running: a running adapter reads its
/// files from one of them. Returns what was removed.
pub fn remove_unused_adapters(data_dir: &Path) -> Vec<PathBuf> {
    let any_running = std::fs::read_dir(process::run_dir(data_dir))
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
    let root = adapters_root(data_dir);
    install::remove_stale_attempts(&root, InstallLimits::default().stale_attempt);
    let current = [&CLAUDE_ADAPTER, &CODEX_ADAPTER]
        .map(|adapter| adapter.dir(data_dir))
        .to_vec();
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let an_install = path.is_dir()
            && name.rsplit_once('-').is_some_and(|(_, id)| {
                id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        if an_install && !current.contains(&path) && std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

// ---- starting one -----------------------------------------------------------

/// A coding harness that can be started now.
#[derive(Clone, Debug)]
pub struct Ready {
    pub spec: &'static ProgramSpec,
    pub program: Program,
    /// The Node that runs the adapter, and the adapter's install.
    pub adapter: Option<(Node, PathBuf)>,
}

/// Finds the program of `spec`, checks its version, and for Codex and
/// Claude Code finds Node and installs the adapter when this is its first
/// use. Starts nothing and opens no session.
pub async fn prepare(
    data_dir: &Path,
    spec: &'static ProgramSpec,
    lookup: impl Fn(&str) -> Option<String>,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Ready, Problem> {
    let program = find_program(spec, &lookup).await?;
    check_range(spec, &program)?;
    if needs_bubblewrap(spec.harness) && install::on_path("bwrap", &lookup).is_empty() {
        return Err(Problem::BubblewrapMissing);
    }
    let adapter = match spec.adapter {
        None => None,
        Some(adapter) => {
            let node = install::find_node(&lookup)
                .await
                .map_err(|problem| Problem::Node {
                    spec,
                    why: node_problem(&problem),
                })?;
            if !adapter.is_installed(data_dir) {
                progress(&format!(
                    "installing the adapter {} {} (first use; this needs the npm registry)…",
                    adapter.package, adapter.version
                ));
            }
            let folder = ensure_adapter(
                data_dir,
                spec,
                adapter,
                &node,
                &lookup,
                InstallLimits::default(),
            )
            .await?;
            Some((node, folder))
        }
    };
    Ok(Ready {
        spec,
        program,
        adapter,
    })
}

/// Whether the harness's sandbox needs bubblewrap here: Grok's, on Linux,
/// for the folders bzz tells it to hide.
pub const fn needs_bubblewrap(harness: Harness) -> bool {
    cfg!(target_os = "linux") && matches!(harness, Harness::Grok)
}

/// What is wrong with Node, without the words that are about bzz's own
/// host.
fn node_problem(problem: &install::Problem) -> String {
    match problem {
        install::Problem::NodeMissing => {
            "no `node` was found on PATH. Install Node.js, or set BZZ_NODE to the path of one"
                .into()
        }
        install::Problem::NodeTooOld { program, version } => format!(
            "Node.js {version} at {} is older. Install a newer one, or set BZZ_NODE to the path of one",
            program.display()
        ),
        install::Problem::NodeUnusable { program } => format!(
            "{} did not answer `--version` as Node.js does. Set BZZ_NODE to the path of Node.js, or unset it",
            program.display()
        ),
        other => other.to_string(),
    }
}

/// What to run, and what its environment holds beyond the four variables
/// every harness gets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Start {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Ready {
    /// The command that starts the harness. Its guard and what it needs to
    /// find its sign-in are added by [`environment`].
    pub fn start(&self) -> Start {
        match (&self.adapter, self.spec.adapter) {
            (Some((node, folder)), Some(adapter)) => Start {
                program: node.program.display().to_string(),
                args: vec![adapter.entry(folder).display().to_string()],
                // The owner's own program, never the adapter's copy.
                env: vec![(
                    adapter.program_variable.to_owned(),
                    self.program.real.display().to_string(),
                )],
            },
            _ => Start {
                program: self.program.path.display().to_string(),
                // Its own agent, never the shared leader process: a sandbox
                // is refused under a leader.
                args: vec!["agent".into(), "--no-leader".into(), "stdio".into()],
                env: Vec::new(),
            },
        }
    }
}

/// Variables handed on from bzz's environment so that a program reaches its
/// provider through the owner's proxy and trusts what he trusts. No key and
/// nothing of bzz's.
const NETWORK_VARIABLES: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

/// What a coding harness's environment holds beyond `PATH`, `HOME`, `LANG`
/// and `TMPDIR`: its guard, and where its sign-in is when the owner moved
/// it. Before it returns, Grok's sandbox profile is written in `workspace`,
/// in place of whatever was there: the agent can write in its own folder,
/// and what it wrote there must never be the profile of its next start.
///
/// No API key is handed on: a harness uses the sign-in the owner made with
/// the program itself (D17).
pub fn environment(
    harness: Harness,
    local: &LocalAgentConfig,
    workspace: &Path,
    data_dir: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Vec<(String, String)>, Problem> {
    let Some(spec) = spec(harness) else {
        return Ok(Vec::new());
    };
    let mut env = spec
        .home_variables
        .iter()
        .chain(NETWORK_VARIABLES)
        .filter_map(|name| lookup(name).map(|value| ((*name).to_owned(), value)))
        .collect::<Vec<_>>();
    match harness {
        Harness::Grok if has_own_sandbox(harness, cfg!(windows)) => {
            write_grok_profile(local, workspace, &hidden_from_grok(data_dir))?;
            env.push(("GROK_SANDBOX".into(), GROK_PROFILE.into()));
        }
        // The adapter starts every session in this mode; bzz sets it again
        // on each session, and checks that it took.
        Harness::Codex => env.push(("INITIAL_AGENT_MODE".into(), CODEX_MODE.into())),
        _ => {}
    }
    Ok(env)
}

/// Whether the program has a sandbox of its own that bzz switches on, on
/// this kind of system. Claude Code has one for shell commands only, which
/// bzz does not use: nothing was proven about it.
pub const fn has_own_sandbox(harness: Harness, windows: bool) -> bool {
    match harness {
        Harness::Grok => !windows,
        Harness::Codex | Harness::PiDurable => true,
        Harness::ClaudeCode => false,
    }
}

/// What Grok's sandbox is told to hide, for reading as for writing: the
/// folder of bzz's keys, where every agent's sealed key is, the owner's
/// own when it is kept in a file, and the git credentials. Only what is
/// there: a path that is not cannot be hidden, and stops Grok at its start.
pub fn hidden_from_grok(data_dir: &Path) -> Vec<PathBuf> {
    let keys = data_dir.join("keys");
    std::fs::canonicalize(&keys)
        .ok()
        .filter(|keys| keys.is_dir())
        .into_iter()
        .collect()
}

/// Grok's sandbox profile for one agent: Grok's `workspace` profile (it
/// reads everything the owner can, and writes in its working folder, its
/// own `~/.grok` and the temporary folders), with the folders the owner
/// granted for writing, and without `hidden`, which it may neither read
/// nor write. A profile of this kind cannot be applied by halves: Grok
/// refuses to start when a custom profile cannot be enforced, where it
/// only warns for a built-in one.
pub fn grok_profile(local: &LocalAgentConfig, hidden: &[PathBuf]) -> String {
    let mut profile = toml::Table::new();
    profile.insert("extends".into(), "workspace".into());
    profile.insert(
        "deny".into(),
        toml::Value::Array(
            hidden
                .iter()
                .map(|path| toml::Value::String(path.display().to_string()))
                .collect(),
        ),
    );
    profile.insert(
        "read_write".into(),
        toml::Value::Array(
            local
                .folders
                .iter()
                .filter(|folder| folder.write)
                .map(|folder| toml::Value::String(folder.path.display().to_string()))
                .collect(),
        ),
    );
    let mut profiles = toml::Table::new();
    profiles.insert(GROK_PROFILE.into(), toml::Value::Table(profile));
    let mut file = toml::Table::new();
    file.insert("profiles".into(), toml::Value::Table(profiles));
    format!(
        "# Written by bzz at every start of this agent. Changes are lost.\n{}",
        toml::to_string(&file).unwrap_or_default()
    )
}

fn write_grok_profile(
    local: &LocalAgentConfig,
    workspace: &Path,
    hidden: &[PathBuf],
) -> Result<(), Problem> {
    let folder = workspace.join(".grok");
    let file = folder.join("sandbox.toml");
    let io = |path: &Path, why: String| Problem::Io {
        what: "Grok's sandbox profile",
        path: path.to_path_buf(),
        why,
    };
    process::create_private_dir(&folder).map_err(|error| io(&folder, error.to_string()))?;
    // By rename over whatever is there, also a link the agent left.
    let temporary = folder.join(format!(".sandbox-{:016x}.tmp", rand::random::<u64>()));
    std::fs::write(&temporary, grok_profile(local, hidden))
        .map_err(|error| io(&temporary, error.to_string()))?;
    std::fs::rename(&temporary, &file).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        io(&file, error.to_string())
    })
}

// ---- its sessions -----------------------------------------------------------

/// What a harness said at `initialize` about how it takes a session's
/// settings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Offer {
    /// The standard `systemPrompt` of `session/new`.
    pub system_prompt: bool,
    /// The standard `additionalDirectories` of `session/new`.
    pub additional_directories: bool,
}

impl Offer {
    pub fn from_initialize(response: &Value) -> Self {
        let offers = |name: &str| {
            response
                .pointer("/agentCapabilities/sessionCapabilities")
                .and_then(|capabilities| capabilities.get(name))
                .is_some_and(|capability| !capability.is_null())
        };
        Self {
            system_prompt: offers("systemPrompt"),
            additional_directories: offers("additionalDirectories"),
        }
    }
}

/// The folders a harness is told of beside its working folder.
///
/// - Claude Code: every granted folder. bzz's answers decide what it may
///   write.
/// - Codex: the folders granted for writing only. Whatever it is told of
///   becomes writable in its sandbox, and it reads everything anyway.
/// - Grok: none over the protocol. Its sandbox profile names them.
pub fn extra_directories(harness: Harness, local: &LocalAgentConfig) -> Vec<PathBuf> {
    local
        .folders
        .iter()
        .filter(|folder| match harness {
            Harness::ClaudeCode => true,
            Harness::Codex => folder.write,
            Harness::Grok | Harness::PiDurable => false,
        })
        .map(|folder| folder.path.clone())
        .collect()
}

/// Whether a coding harness of this agent may ever run a real command
/// outside its own sandbox, and whether the owner is asked.
fn commands_line(harness: Harness, local: &LocalAgentConfig) -> String {
    let granted = local
        .folders
        .iter()
        .filter(|folder| folder.exec.granted())
        .collect::<Vec<_>>();
    let asked = granted.iter().any(|folder| folder.exec == ExecGrant::Ask);
    let outside = match (granted.is_empty(), asked) {
        (true, _) => "your owner has granted no folder for commands, so it is refused",
        (false, true) => {
            "it waits for your owner's approval, and is refused when your owner is not there"
        }
        (false, false) => "your owner allowed it without being asked",
    };
    match harness {
        Harness::ClaudeCode => format!("A shell command: {outside}."),
        Harness::Codex => format!(
            "A shell command runs inside your sandbox: it writes only in the folders above and reaches no network. To run one outside it: {outside}."
        ),
        _ => {
            "A shell command runs inside your sandbox: it writes only in the folders above.".into()
        }
    }
}

/// What a coding harness is told once per session, in the way it takes a
/// system prompt: the rules of the chat, where it works, and the agent's
/// instructions.
pub fn system_prompt(
    name: &str,
    harness: Harness,
    local: &LocalAgentConfig,
    workspace: &Path,
) -> String {
    let mut text = format!(
        "You are {name}, an agent in a Buzz team chat, started by bzz for your owner. People see only what you post with the `send` tool of the `buzz` server: set `final` to true on your answer. `read` shows earlier messages and `react` adds a reaction. What you write outside those tools is shown to nobody.\n\nYour working folder is {}.",
        workspace.display()
    );
    let listed = |write: bool| {
        local
            .folders
            .iter()
            .filter(|folder| folder.write == write)
            .map(|folder| format!("{} ({})", folder.path.display(), folder.name))
            .collect::<Vec<_>>()
    };
    let (read_only, writable) = (listed(false), listed(true));
    if !writable.is_empty() {
        text.push_str(&format!(
            " Your owner also lets you read and write in: {}.",
            writable.join(", ")
        ));
    }
    if !read_only.is_empty() {
        text.push_str(&format!(
            " Your owner also lets you read, and not change: {}.",
            read_only.join(", ")
        ));
    }
    text.push_str(&format!(
        " Writing anywhere else is refused. {} When something is refused, say so in your answer and do not try it another way.",
        commands_line(harness, local)
    ));
    // A model that cannot post looks for another way to, with a real shell
    // (seen with Grok: it went through bzz's own files for a key).
    text.push_str(
        " If the `send` tool is missing or fails, stop: write one line that says so and end your turn. Never look for another way to post, and never read or search for keys, credentials or bzz's own files.",
    );
    if !local.instructions.trim().is_empty() {
        text.push_str(&format!(
            "\n\nYour instructions:\n{}",
            local.instructions.trim()
        ));
    }
    text
}

/// The parameters of `session/new` for a coding harness; `session/load`
/// takes the same with the session's id. The system prompt, the extra
/// folders and what switches the owner's own settings off go in the way
/// each program takes them.
pub fn session_params(
    harness: Harness,
    offer: Offer,
    workspace: &Path,
    servers: &Value,
    place: &str,
    prompt: &str,
    local: &LocalAgentConfig,
) -> Value {
    let mut params = json!({
        "cwd": workspace,
        "mcpServers": servers,
        "_meta": {"bzz/place": place},
    });
    let extra = extra_directories(harness, local);
    if offer.additional_directories && !extra.is_empty() {
        params["additionalDirectories"] = json!(extra);
    }
    if offer.system_prompt {
        params["systemPrompt"] =
            json!({"mode": "append", "content": [{"type": "text", "text": prompt}]});
    }
    match harness {
        // Grok takes extra rules for its system prompt here.
        Harness::Grok => params["_meta"]["rules"] = json!(prompt),
        // Claude Code would otherwise read the owner's own settings and
        // those of its working folder, which the agent can write: allow
        // rules that answer for bzz, hooks, plugins and MCP servers. The
        // sign-in is not a setting and is still found.
        Harness::ClaudeCode => {
            params["_meta"]["claudeCode"] =
                json!({"options": {"settingSources": [], "strictMcpConfig": true}});
        }
        Harness::Codex | Harness::PiDurable => {}
    }
    params
}

/// Whether the prompt of [`system_prompt`] reached the harness as one, so
/// that the agent's instructions need not travel in every message.
pub const fn takes_system_prompt(harness: Harness, offer: Offer) -> bool {
    offer.system_prompt || matches!(harness, Harness::Grok)
}

/// What became of a setting bzz wants on a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pick {
    /// `session/set_config_option` with this id and value.
    Set { config_id: String, value: String },
    /// The harness has the option and not this value.
    NotOffered { offered: Vec<String> },
    /// The harness has no such option.
    NoOption,
}

/// Finds, among the `configOptions` a harness answered with, the option of
/// `category` (`model`, `thought_level`) and whether it offers `wanted`.
pub fn pick_option(config_options: Option<&Value>, category: &str, wanted: &str) -> Pick {
    let option = config_options
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|option| option.get("category").and_then(Value::as_str) == Some(category));
    let Some(option) = option else {
        return Pick::NoOption;
    };
    let Some(config_id) = option.get("id").and_then(Value::as_str) else {
        return Pick::NoOption;
    };
    // Values come flat, or in groups of them.
    let mut offered = Vec::new();
    for entry in option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match entry.get("options").and_then(Value::as_array) {
            Some(group) => offered.extend(group.iter().filter_map(|entry| entry.get("value"))),
            None => offered.extend(entry.get("value")),
        }
    }
    let offered = offered
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if offered.iter().any(|value| value == wanted) {
        Pick::Set {
            config_id: config_id.to_owned(),
            value: wanted.to_owned(),
        }
    } else {
        Pick::NotOffered { offered }
    }
}

/// Whether a session's answer offers the mode `wanted`. `None`: it lists no
/// modes at all.
pub fn offers_mode(opened: &Value, wanted: &str) -> Option<bool> {
    let modes = opened.pointer("/modes/availableModes")?.as_array()?;
    Some(
        modes
            .iter()
            .any(|mode| mode.get("id").and_then(Value::as_str) == Some(wanted)),
    )
}

// ---- what `agent doctor` and `agent new` say ---------------------------------

/// One coding harness as `bzz agent doctor` reports it. Nothing is
/// installed and nothing is started to find it out.
#[derive(Clone, Debug)]
pub struct Report {
    pub spec: &'static ProgramSpec,
    pub program: Result<Program, Problem>,
    /// For Codex and Claude Code: where the adapter is, and whether it is
    /// installed.
    pub adapter: Option<(PathBuf, bool)>,
}

pub async fn report(
    data_dir: &Path,
    spec: &'static ProgramSpec,
    lookup: impl Fn(&str) -> Option<String>,
) -> Report {
    let program = match find_program(spec, &lookup).await {
        Ok(program) => check_range(spec, &program).map(|()| program),
        Err(problem) => Err(problem),
    };
    Report {
        spec,
        program,
        adapter: spec
            .adapter
            .map(|adapter| (adapter.dir(data_dir), adapter.is_installed(data_dir))),
    }
}

impl Report {
    /// What keeps an agent on this harness from starting. `node`: what is
    /// wrong with Node, or with npm when an install is still to be made.
    pub fn problems(&self, node: Option<&str>) -> Vec<String> {
        let mut problems = Vec::new();
        if let Err(problem) = &self.program {
            problems.push(problem.to_string());
        }
        if let (Some(_), Some(why)) = (&self.adapter, node) {
            problems.push(format!(
                "its adapter needs Node.js and, for its install, npm: {why}"
            ));
        }
        problems
    }

    /// The lines of the report: the program, its version and range, the
    /// adapter, and which sign-in is in use.
    pub fn lines(&self) -> Vec<String> {
        let spec = self.spec;
        let mut lines = vec![match &self.program {
            Ok(program) => format!(
                "{} at {} (known to work: {}: ok)",
                program.version_text(),
                program.path.display(),
                spec.range.text()
            ),
            Err(Problem::ProgramMissing { .. }) => format!(
                "not found on PATH (set {} to its path); known to work: {}",
                spec.variable,
                spec.range.text()
            ),
            Err(problem) => format!("not ready: {problem}"),
        }];
        if let (Some(adapter), Some((folder, installed))) = (spec.adapter, &self.adapter) {
            lines.push(format!(
                "adapter {} {} · {} {}",
                adapter.package,
                adapter.version,
                if *installed {
                    "installed in"
                } else {
                    "not installed yet; `bzz agent new` or an agent's first start installs it in"
                },
                folder.display()
            ));
        }
        lines.push(sign_in_note(spec));
        lines
    }
}

/// D17: which sign-in a coding harness uses.
pub fn sign_in_note(spec: &ProgramSpec) -> String {
    format!(
        "sign-in: your own, made with `{}`. bzz has none of its own, hands on no API key, and does not read yours; a missing one shows at the agent's first message",
        spec.sign_in
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FolderGrant;

    const ALL: [&ProgramSpec; 3] = [&GROK, &CODEX, &CLAUDE];

    #[test]
    fn versions_are_read_as_each_program_prints_them() {
        assert_eq!(
            parse_program_version("grok 1.0.77 (50ca579fa996) [stable]\n"),
            Some((1, 0, 77))
        );
        assert_eq!(
            parse_program_version("2.1.322 (Claude Code)"),
            Some((2, 1, 322))
        );
        assert_eq!(
            parse_program_version("codex-cli 0.166.0"),
            Some((0, 166, 0))
        );
        assert_eq!(parse_program_version("v1.2.3-beta.4"), Some((1, 2, 3)));
        for unreadable in ["", "grok", "version 12", "1.2", "a.b.c"] {
            assert_eq!(parse_program_version(unreadable), None, "{unreadable}");
        }
    }

    #[test]
    fn a_version_outside_the_tested_range_is_refused_with_both_versions() {
        for spec in ALL {
            let range = spec.range;
            assert!(range.contains(range.from), "{}", spec.command);
            assert!(!range.contains(range.below));
            let before = match range.from {
                (major, minor, 0) => (major, minor - 1, 999),
                (major, minor, patch) => (major, minor, patch - 1),
            };
            assert!(!range.contains(before), "one before the tested one");
            let program = |version| Program {
                path: "/opt/bin/it".into(),
                real: "/opt/bin/it".into(),
                version,
            };
            assert_eq!(check_range(spec, &program(range.from)), Ok(()));
            let refused = check_range(spec, &program(range.below))
                .unwrap_err()
                .to_string();
            for stated in [
                version_text(range.below),
                version_text(range.from),
                "does not start it".to_owned(),
                spec.variable.to_owned(),
                spec.title.to_owned(),
            ] {
                assert!(refused.contains(&stated), "`{stated}` in: {refused}");
            }
        }
        // What ran on the machine the tests were made on.
        assert!(GROK.range.contains((1, 0, 77)) && GROK.range.contains((1, 0, 85)));
        assert!(!GROK.range.contains((1, 1, 0)) && !GROK.range.contains((1, 0, 45)));
        assert!(CLAUDE.range.contains((2, 1, 322)) && !CLAUDE.range.contains((2, 1, 282)));
        assert!(CODEX.range.contains((0, 166, 0)) && !CODEX.range.contains((0, 169, 0)));
    }

    #[test]
    fn the_lock_files_pin_the_adapters_the_code_names() {
        for adapter in [&CLAUDE_ADAPTER, &CODEX_ADAPTER] {
            let file = |name: &str| {
                let content = adapter
                    .files
                    .iter()
                    .find(|(path, _)| *path == name)
                    .unwrap()
                    .1;
                serde_json::from_str::<Value>(content).unwrap()
            };
            assert_eq!(
                file("package.json")["dependencies"][adapter.package],
                adapter.version,
                "an exact version, not a range"
            );
            let lock = file("package-lock.json");
            let locked = &lock["packages"][format!("node_modules/{}", adapter.package)];
            assert_eq!(locked["version"], adapter.version);
            // Rule 13: every package comes from the registry with a hash.
            for (path, package) in lock["packages"].as_object().unwrap() {
                if path.is_empty() {
                    continue;
                }
                assert!(
                    package["resolved"]
                        .as_str()
                        .is_some_and(|from| from.starts_with("https://registry.npmjs.org/")),
                    "{path}"
                );
                assert!(
                    package["integrity"]
                        .as_str()
                        .is_some_and(|hash| hash.starts_with("sha512-")),
                    "{path}"
                );
            }
            assert_eq!(adapter.digest().len(), 64);
        }
        assert_ne!(CLAUDE_ADAPTER.digest(), CODEX_ADAPTER.digest());
    }

    fn grants() -> LocalAgentConfig {
        let mut local = LocalAgentConfig::new(Harness::Grok);
        local.instructions = "Be brief.".into();
        local.folders = vec![
            FolderGrant::read_only("docs", "/srv/docs"),
            FolderGrant {
                name: "work".into(),
                path: "/srv/my \"work\"".into(),
                write: true,
                exec: ExecGrant::No,
            },
        ];
        local
    }

    #[test]
    fn groks_profile_extends_workspace_with_the_folders_granted_for_writing() {
        let hidden = [PathBuf::from("/data/keys")];
        let text = grok_profile(&grants(), &hidden);
        let read = toml::from_str::<toml::Table>(&text).unwrap();
        let profile = &read["profiles"][GROK_PROFILE];
        assert_eq!(profile["extends"].as_str(), Some("workspace"));
        // The folder for reading only is not in it; a path is written so
        // that TOML reads it back as it is.
        assert_eq!(
            profile["read_write"].as_array().unwrap(),
            &[toml::Value::String("/srv/my \"work\"".into())]
        );
        // bzz's keys are hidden from it, for reading as for writing.
        assert_eq!(
            profile["deny"].as_array().unwrap(),
            &[toml::Value::String("/data/keys".into())]
        );
        let none = grok_profile(&LocalAgentConfig::new(Harness::Grok), &[]);
        let read = toml::from_str::<toml::Table>(&none).unwrap();
        let profile = &read["profiles"][GROK_PROFILE];
        assert!(profile["read_write"].as_array().unwrap().is_empty());
        assert!(profile["deny"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_guard_is_in_the_environment_and_the_profile_replaces_what_was_there() {
        let temporary = tempfile::TempDir::new().unwrap();
        let workspace = temporary.path().join("workspace");
        let data = temporary.path().join("data");
        std::fs::create_dir_all(data.join("keys")).unwrap();
        let hidden = hidden_from_grok(&data);
        assert_eq!(hidden, [std::fs::canonicalize(data.join("keys")).unwrap()]);
        assert!(hidden_from_grok(&temporary.path().join("no-data")).is_empty());
        // What an agent left in its own folder for its next start.
        std::fs::create_dir_all(workspace.join(".grok")).unwrap();
        std::fs::write(
            workspace.join(".grok/sandbox.toml"),
            "[profiles.bzz-agent]\nextends = \"devbox\"\nread_write = [\"/\"]\n",
        )
        .unwrap();
        let lookup = |name: &str| match name {
            "GROK_HOME" => Some("/srv/grok".to_owned()),
            "HTTPS_PROXY" => Some("http://proxy:3128".to_owned()),
            // Never handed on.
            "XAI_API_KEY" | "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "CODEX_API_KEY"
            | "BZZ_PASSPHRASE_FD" | "NOSTR_SECRET_KEY" => Some("must not pass".to_owned()),
            _ => None,
        };
        let names = |harness| {
            let mut local = grants();
            local.harness = harness;
            let env = environment(harness, &local, &workspace, &data, lookup).unwrap();
            assert!(
                env.iter().all(|(_, value)| value != "must not pass"),
                "{env:?}"
            );
            env
        };
        let grok = names(Harness::Grok);
        if cfg!(windows) {
            assert!(!grok.iter().any(|(name, _)| name == "GROK_SANDBOX"));
        } else {
            assert!(grok.contains(&("GROK_SANDBOX".into(), GROK_PROFILE.into())));
            let written = std::fs::read_to_string(workspace.join(".grok/sandbox.toml")).unwrap();
            assert_eq!(written, grok_profile(&grants(), &hidden));
            assert!(!written.contains("devbox"));
            assert!(written.contains(&hidden[0].display().to_string()));
        }
        assert!(grok.contains(&("GROK_HOME".into(), "/srv/grok".into())));
        assert!(grok.contains(&("HTTPS_PROXY".into(), "http://proxy:3128".into())));
        let codex = names(Harness::Codex);
        assert!(codex.contains(&("INITIAL_AGENT_MODE".into(), CODEX_MODE.into())));
        assert!(!codex.iter().any(|(name, _)| name == "GROK_HOME"));
        assert!(
            !names(Harness::ClaudeCode)
                .iter()
                .any(|(name, _)| name.contains("MODE") || name.contains("SANDBOX"))
        );
        assert_eq!(required_mode(Harness::ClaudeCode), Some("default"));
        assert_eq!(required_mode(Harness::Codex), Some("workspace-write"));
        assert_eq!(required_mode(Harness::Grok), None);
    }

    #[test]
    fn a_session_is_told_its_prompt_and_folders_as_each_program_takes_them() {
        let local = grants();
        let workspace = Path::new("/data/agents/a/workspace");
        let servers = json!([{"name": "buzz"}]);
        let both = Offer {
            system_prompt: true,
            additional_directories: true,
        };
        let params = |harness, offer| {
            session_params(
                harness, offer, workspace, &servers, "place", "RULES", &local,
            )
        };
        // Claude Code: every granted folder, the prompt, and none of the
        // owner's settings.
        let claude = params(Harness::ClaudeCode, both);
        assert_eq!(claude["cwd"], "/data/agents/a/workspace");
        assert_eq!(claude["mcpServers"], servers);
        assert_eq!(claude["_meta"]["bzz/place"], "place");
        assert_eq!(
            claude["additionalDirectories"],
            json!(["/srv/docs", "/srv/my \"work\""])
        );
        assert_eq!(
            claude["systemPrompt"],
            json!({"mode": "append", "content": [{"type": "text", "text": "RULES"}]})
        );
        assert_eq!(
            claude["_meta"]["claudeCode"]["options"],
            json!({"settingSources": [], "strictMcpConfig": true})
        );
        // Codex: only what it may write, because what it is told of becomes
        // writable.
        let codex = params(Harness::Codex, both);
        assert_eq!(codex["additionalDirectories"], json!(["/srv/my \"work\""]));
        assert!(codex["_meta"].get("claudeCode").is_none());
        // Grok: its rules, and no folder: the profile names them.
        let grok = params(Harness::Grok, Offer::default());
        assert_eq!(grok["_meta"]["rules"], "RULES");
        assert!(grok.get("additionalDirectories").is_none());
        assert!(grok.get("systemPrompt").is_none());
        assert!(takes_system_prompt(Harness::Grok, Offer::default()));
        assert!(!takes_system_prompt(Harness::Codex, Offer::default()));
        // What a harness does not offer is not sent.
        let bare = params(Harness::ClaudeCode, Offer::default());
        assert!(bare.get("additionalDirectories").is_none() && bare.get("systemPrompt").is_none());

        assert_eq!(
            Offer::from_initialize(&json!({"agentCapabilities": {"sessionCapabilities": {
                "systemPrompt": {}, "additionalDirectories": {}, "list": {}}}})),
            both
        );
        assert_eq!(
            Offer::from_initialize(&json!({"agentCapabilities": {"loadSession": true}})),
            Offer::default()
        );
    }

    #[test]
    fn the_system_prompt_says_where_the_agent_works_and_what_is_refused() {
        let mut local = grants();
        let text = system_prompt("Scout", Harness::ClaudeCode, &local, Path::new("/w"));
        for stated in [
            "You are Scout",
            "`send` tool of the `buzz` server",
            "Your working folder is /w.",
            "read and write in: /srv/my \"work\" (work).",
            "read, and not change: /srv/docs (docs).",
            "Writing anywhere else is refused.",
            "If the `send` tool is missing or fails, stop",
            "never read or search for keys, credentials or bzz's own files",
            "A shell command: your owner has granted no folder for commands, so it is refused.",
            "Your instructions:\nBe brief.",
        ] {
            assert!(text.contains(stated), "`{stated}` in:\n{text}");
        }
        local.folders[1].exec = ExecGrant::Ask;
        assert!(
            system_prompt("Scout", Harness::ClaudeCode, &local, Path::new("/w"))
                .contains("A shell command: it waits for your owner's approval")
        );
        local.folders[1].exec = ExecGrant::WithoutAsking;
        let codex = system_prompt("Scout", Harness::Codex, &local, Path::new("/w"));
        assert!(codex.contains("inside your sandbox"), "{codex}");
        assert!(codex.contains("your owner allowed it without being asked"));
        local.instructions.clear();
        assert!(
            !system_prompt("Scout", Harness::Grok, &local, Path::new("/w"))
                .contains("Your instructions")
        );
    }

    #[test]
    fn a_model_is_set_only_when_the_harness_offers_it() {
        // As Grok and the two adapters answer `session/new`.
        let options = json!([
            {"id": "mode", "category": "mode", "options": [{"value": "default"}]},
            {"id": "model", "category": "model", "type": "select", "currentValue": "grok-4.7",
             "options": [{"value": "grok-4.7", "name": "Grok 4.7"}, {"value": "grok-4.5"}]},
            {"id": "reasoning_effort", "category": "thought_level",
             "options": [{"group": "g", "options": [{"value": "low"}, {"value": "high"}]}]},
        ]);
        assert_eq!(
            pick_option(Some(&options), "model", "grok-4.5"),
            Pick::Set {
                config_id: "model".into(),
                value: "grok-4.5".into()
            }
        );
        assert_eq!(
            pick_option(Some(&options), "model", "gpt-9"),
            Pick::NotOffered {
                offered: vec!["grok-4.7".into(), "grok-4.5".into()]
            }
        );
        assert_eq!(
            pick_option(Some(&options), "thought_level", "low"),
            Pick::Set {
                config_id: "reasoning_effort".into(),
                value: "low".into()
            }
        );
        assert_eq!(pick_option(Some(&options), "fast", "on"), Pick::NoOption);
        assert_eq!(pick_option(None, "model", "x"), Pick::NoOption);
        let opened = json!({"modes": {"currentModeId": "auto", "availableModes": [{"id": "default"}, {"id": "auto"}]}});
        assert_eq!(offers_mode(&opened, "default"), Some(true));
        assert_eq!(offers_mode(&opened, "workspace-write"), Some(false));
        assert_eq!(offers_mode(&json!({"sessionId": "s"}), "default"), None);
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

        fn lookup_with(
            path: &Path,
            extra: &'static [(&'static str, &'static str)],
        ) -> impl Fn(&str) -> Option<String> + Clone + use<> {
            let path = std::env::join_paths([path, Path::new("/usr/bin"), Path::new("/bin")])
                .unwrap()
                .to_string_lossy()
                .into_owned();
            move |name: &str| match name {
                "PATH" => Some(path.clone()),
                "BZZ_PASSPHRASE_FD" | "NOSTR_SECRET_KEY" | "BUZZ_RELAY_URL" => {
                    Some("must not pass".into())
                }
                other => extra
                    .iter()
                    .find(|(name, _)| *name == other)
                    .map(|(_, value)| (*value).to_owned()),
            }
        }

        #[tokio::test]
        async fn a_program_is_found_asked_for_its_version_and_held_to_its_range() {
            let temporary = tempfile::TempDir::new().unwrap();
            let bin = temporary.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let data = temporary.path().join("data");
            let quiet = |_: &str| {};
            // None on PATH.
            let missing = prepare(&data, &GROK, lookup_with(&bin, &[]), &quiet)
                .await
                .unwrap_err();
            assert_eq!(missing, Problem::ProgramMissing { spec: &GROK });
            let text = missing.to_string();
            for stated in ["`grok`", "grok login", "BZZ_GROK", "bzz agent doctor"] {
                assert!(text.contains(stated), "`{stated}` in: {text}");
            }
            // On Linux Grok's sandbox needs bubblewrap, and bzz says so
            // rather than start a Grok that would stop at once.
            let alone = temporary.path().join("alone");
            program(&alone, "grok", "echo 'grok 1.0.77 (50ca579fa996) [stable]'");
            let only = alone.to_string_lossy().into_owned();
            let without = prepare(
                &data,
                &GROK,
                move |name: &str| (name == "PATH").then(|| only.clone()),
                &quiet,
            )
            .await;
            if cfg!(target_os = "linux") {
                let problem = without.unwrap_err();
                assert_eq!(problem, Problem::BubblewrapMissing);
                assert!(problem.to_string().contains("`bubblewrap`"), "{problem}");
            } else {
                assert!(without.is_ok());
            }
            program(&bin, "bwrap", "exit 0");
            // The tested version, behind a link as an installer leaves it.
            let real = program(
                &temporary.path().join("downloads"),
                "grok-1.0.77",
                "echo 'grok 1.0.77 (50ca579fa996) [stable]'",
            );
            std::os::unix::fs::symlink(&real, bin.join("grok")).unwrap();
            let ready = prepare(&data, &GROK, lookup_with(&bin, &[]), &quiet)
                .await
                .unwrap();
            assert_eq!(ready.program.version, (1, 0, 77));
            assert_eq!(ready.program.path, bin.join("grok"));
            assert_eq!(ready.program.real, std::fs::canonicalize(&real).unwrap());
            assert!(ready.adapter.is_none());
            assert_eq!(
                ready.start(),
                Start {
                    program: bin.join("grok").display().to_string(),
                    args: vec!["agent".into(), "--no-leader".into(), "stdio".into()],
                    env: Vec::new(),
                }
            );
            // A newer one that was not tested: refused, with both versions.
            program(&bin.join("new"), "grok", "echo 'grok 1.1.0 (abc) [stable]'");
            let newer = lookup_with(&bin.join("new"), &[]);
            let refused = prepare(&data, &GROK, newer.clone(), &quiet)
                .await
                .unwrap_err();
            let text = refused.to_string();
            assert!(
                matches!(&refused, Problem::VersionOutOfRange { found, .. } if found == "1.1.0"),
                "{refused:?}"
            );
            assert!(text.contains("1.1.0") && text.contains("1.0.77"), "{text}");
            // The report says the same, and starts nothing.
            let reported = report(&data, &GROK, newer).await;
            assert!(reported.program.is_err());
            assert!(reported.lines()[0].contains("outside the versions bzz knows to work"));
            assert!(!reported.problems(None).is_empty());
            // Something that is not the program.
            let other = program(&temporary.path().join("other"), "grok", "echo hello");
            let named: &'static str = Box::leak(other.display().to_string().into_boxed_str());
            let extra: &'static [(&str, &str)] = Box::leak(Box::new([("BZZ_GROK", named)]));
            let unusable = prepare(&data, &GROK, lookup_with(&bin, extra), &quiet)
                .await
                .unwrap_err();
            assert!(
                matches!(unusable, Problem::ProgramUnusable { .. }),
                "{unusable:?}"
            );
        }

        /// A stand-in `npm ci` that installs what the lock file names, or
        /// something else. It counts its runs in the file `npm_config_cache`
        /// names: one of the few variables npm is handed.
        fn fake_npm(bin: &Path, adapter: &AdapterSpec, version: &str, body: &str) {
            let package = format!("node_modules/{}", adapter.package);
            program(
                bin,
                "npm",
                &format!(
                    "if [ \"$1\" = --version ]; then echo 11.0.0; exit 0; fi\necho \"$@\" > npm-arguments\nenv | grep -v npm_config_cache > npm-environment\necho run >> \"$npm_config_cache\"\n{body}\nmkdir -p {package}/dist && echo '// adapter' > {package}/dist/index.js && echo '{{\"version\": \"{version}\"}}' > {package}/package.json"
                ),
            );
        }

        #[tokio::test]
        async fn an_adapter_is_installed_once_whole_and_at_the_pinned_version() {
            let temporary = tempfile::TempDir::new().unwrap();
            let bin = temporary.path().join("bin");
            program(&bin, "node", "echo v22.19.0");
            program(&bin, "codex", "echo 'codex-cli 0.166.0'");
            let data = temporary.path().join("data");
            let counter: &'static str = Box::leak(
                temporary
                    .path()
                    .join("installs")
                    .display()
                    .to_string()
                    .into_boxed_str(),
            );
            let extra: &'static [(&str, &str)] =
                Box::leak(Box::new([("npm_config_cache", counter)]));
            let lookup = lookup_with(&bin, extra);
            let quiet = |_: &str| {};
            let adapter = &CODEX_ADAPTER;
            let installs = || {
                std::fs::read_to_string(counter)
                    .map(|text| text.lines().count())
                    .unwrap_or(0)
            };
            let npm = |version: &str, body: &str| fake_npm(&bin, adapter, version, body);

            // npm fails half way: nothing counts, nothing is left.
            npm(
                adapter.version,
                "mkdir -p node_modules/half; echo 'npm error network' >&2; exit 7",
            );
            let failed = prepare(&data, &CODEX, lookup.clone(), &quiet)
                .await
                .unwrap_err();
            assert!(
                matches!(&failed, Problem::AdapterInstallFailed { why, .. } if why.contains('7')),
                "{failed:?}"
            );
            assert!(failed.to_string().contains("install-codex.log"));
            assert!(!adapter.is_installed(&data) && !adapter.dir(&data).exists());
            assert_eq!(
                std::fs::read_dir(adapters_root(&data))
                    .unwrap()
                    .flatten()
                    .filter(|entry| entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(install::TEMPORARY_PREFIX))
                    .count(),
                0
            );

            // npm installs another version than the one pinned: not kept.
            npm("2.6.0", "");
            let other = prepare(&data, &CODEX, lookup.clone(), &quiet)
                .await
                .unwrap_err();
            assert!(
                matches!(&other, Problem::VersionOutOfRange { found, known, .. }
                    if found == "2.6.0" && known == "2.5.0"),
                "{other:?}"
            );
            assert!(!adapter.dir(&data).exists());

            // A folder with the install's name that no rename made.
            std::fs::create_dir_all(adapter.entry(&adapter.dir(&data)).parent().unwrap()).unwrap();
            std::fs::write(adapter.entry(&adapter.dir(&data)), "half").unwrap();
            assert!(!adapter.is_installed(&data));

            // Two at once, as two bzz would: one install, whole.
            npm(adapter.version, "sleep 0.3");
            let before = installs();
            let said = std::sync::Mutex::new(Vec::new());
            let progress = |line: &str| said.lock().unwrap().push(line.to_owned());
            let (first, second) = tokio::join!(
                prepare(&data, &CODEX, lookup.clone(), &progress),
                prepare(&data, &CODEX, lookup.clone(), &quiet)
            );
            let (first, second) = (first.unwrap(), second.unwrap());
            assert!(adapter.is_installed(&data));
            assert!(said.lock().unwrap()[0].contains("installing the adapter"));
            let folder = adapter.dir(&data);
            assert_eq!(first.adapter.as_ref().unwrap().1, folder);
            assert_eq!(second.adapter.as_ref().unwrap().1, folder);
            assert_eq!(
                std::fs::read_to_string(adapter.entry(&folder)).unwrap(),
                "// adapter\n",
                "the half-written file was replaced"
            );
            // The two files of this binary, byte for byte.
            for (path, content) in adapter.files {
                assert_eq!(
                    &std::fs::read_to_string(folder.join(path)).unwrap(),
                    content
                );
            }
            // Rule 13, and nothing of bzz's in npm's environment.
            assert_eq!(
                std::fs::read_to_string(folder.join("npm-arguments"))
                    .unwrap()
                    .trim(),
                "ci --ignore-scripts --omit=optional --omit=dev --no-audit --no-fund"
            );
            let environment = std::fs::read_to_string(folder.join("npm-environment")).unwrap();
            assert!(!environment.contains("must not pass"), "{environment}");
            assert!(!environment.contains("BZZ_"), "{environment}");
            // It is started with Node, pointed at the owner's program.
            let start = first.start();
            assert_eq!(start.program, bin.join("node").display().to_string());
            assert_eq!(start.args, [adapter.entry(&folder).display().to_string()]);
            assert_eq!(
                start.env,
                [(
                    "CODEX_PATH".to_owned(),
                    std::fs::canonicalize(bin.join("codex"))
                        .unwrap()
                        .display()
                        .to_string()
                )]
            );
            // Installed: npm does not run again.
            let after = installs();
            assert!(after > before);
            prepare(&data, &CODEX, lookup.clone(), &quiet)
                .await
                .unwrap();
            assert_eq!(installs(), after);
            // The report: installed, and no session was needed to say so.
            let reported = report(&data, &CODEX, lookup.clone()).await;
            assert!(reported.program.is_ok() && reported.adapter.as_ref().unwrap().1);
            assert!(reported.problems(None).is_empty());
            assert_eq!(
                reported.problems(Some("no `node` was found")).len(),
                1,
                "an adapter needs Node"
            );
            let lines = reported.lines().join("\n");
            for stated in [
                "0.166.0",
                "@agentclientprotocol/codex-acp 2.5.0",
                "installed in",
                "codex login",
            ] {
                assert!(lines.contains(stated), "`{stated}` in:\n{lines}");
            }

            // An install another bzz's lock file named goes; this one stays.
            let old = adapters_root(&data).join("codex-0123456789abcdef");
            std::fs::create_dir_all(&old).unwrap();
            assert_eq!(remove_unused_adapters(&data), std::slice::from_ref(&old));
            assert!(!old.exists() && adapter.is_installed(&data));

            // Without Node an adapter cannot run, and the text says for what.
            let empty = temporary.path().join("empty");
            program(&empty, "claude", "echo '2.1.322 (Claude Code)'");
            let lonely =
                move |name: &str| (name == "PATH").then(|| empty.to_string_lossy().into_owned());
            let no_node = prepare(&data, &CLAUDE, lonely, &quiet).await.unwrap_err();
            let text = no_node.to_string();
            assert!(matches!(no_node, Problem::Node { .. }), "{no_node:?}");
            assert!(
                text.contains("claude-code harness needs Node.js 22.19")
                    && !text.contains("pi-durable"),
                "{text}"
            );
        }
    }
}
