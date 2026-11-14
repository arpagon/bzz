//! A harness's process: started in its own process group with an environment
//! built from nothing, recorded so that a later bzz can find and stop it, and
//! stopped as a whole group (ADR v0.15 local agents, decision 9).
//!
//! A process is identified by its pid, its start time and the machine's boot
//! id together, never by its name or arguments: a pid alone can belong to
//! another program after the first one exits.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use uuid::Uuid;

use crate::error::{Error, Result};

/// How long a harness has to leave after each step of a stop
/// (`docs/agent-host-protocol.md`, section 2).
pub const STOP_GRACE: Duration = Duration::from_secs(2);
/// The most a harness's standard error log may hold per start.
pub const LOG_LIMIT_BYTES: u64 = 8 * 1024 * 1024;

/// What to start and with what. `env` is the whole environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Where the harness's standard error goes.
    pub log: PathBuf,
}

/// Variables copied from bzz's own environment when present, because a
/// program cannot start without them. Everything else is left out, so no
/// credential that happens to be in bzz's environment reaches a harness.
#[cfg(not(windows))]
const INHERITED: &[&str] = &["PATH", "LANG", "TMPDIR"];
#[cfg(windows)]
const INHERITED: &[&str] = &[
    "PATH",
    "LANG",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "ProgramData",
];

/// The environment of a harness: `PATH`, `HOME`, `LANG`, `TMPDIR` and
/// nothing else. `lookup` reads bzz's own environment.
pub fn base_environment(
    home: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut env = INHERITED
        .iter()
        .filter_map(|name| lookup(name).map(|value| ((*name).to_owned(), value)))
        .collect::<Vec<_>>();
    let mut default = |name: &str, value: String| {
        if !env.iter().any(|(existing, _)| existing == name) {
            env.push((name.to_owned(), value));
        }
    };
    #[cfg(not(windows))]
    default("PATH", "/usr/local/bin:/usr/bin:/bin".into());
    default("LANG", "C.UTF-8".into());
    default("TMPDIR", std::env::temp_dir().display().to_string());
    env.push(("HOME".into(), home.display().to_string()));
    env
}

/// What tells one process from every other, now and after a restart.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Opaque: clock ticks since boot on Linux, a date elsewhere.
    pub start_time: String,
    /// Absent where the start time is absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_id: Option<String>,
}

#[cfg(target_os = "linux")]
fn boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn boot_id() -> Option<String> {
    None
}

/// The start time field of `/proc/<pid>/stat`, or `None` for a zombie. The
/// command name may hold spaces and parentheses, so fields are counted from
/// the last `)`.
#[cfg(any(target_os = "linux", test))]
fn linux_start_time(stat: &str) -> Option<String> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_ascii_whitespace();
    // Field 3 is the state; field 22, the start time, is 19 further on.
    if fields.next()? == "Z" {
        return None;
    }
    fields.nth(18).map(str::to_owned)
}

/// The identity of the live process `pid`, or `None` when there is none.
#[cfg(target_os = "linux")]
pub fn identity_of(pid: u32) -> Option<ProcessIdentity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    Some(ProcessIdentity {
        pid,
        start_time: linux_start_time(&stat)?,
        boot_id: boot_id(),
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn identity_of(pid: u32) -> Option<ProcessIdentity> {
    // `lstart` is an absolute date, the same on every read of one process.
    let output = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let start_time = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !start_time.is_empty()).then_some(ProcessIdentity {
        pid,
        start_time,
        boot_id: boot_id(),
    })
}

#[cfg(windows)]
pub fn identity_of(pid: u32) -> Option<ProcessIdentity> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-Process -Id {pid}).StartTime.ToFileTimeUtc()"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let start_time = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !start_time.is_empty()).then_some(ProcessIdentity {
        pid,
        start_time,
        boot_id: None,
    })
}

/// Whether the process that `identity` names is still running.
pub fn is_alive(identity: &ProcessIdentity) -> bool {
    identity_of(identity.pid).is_some_and(|now| &now == identity)
}

/// `<data>/agents/run/<agent id>.json`: which process runs an agent and
/// which bzz started it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OwnershipRecord {
    pub agent: Uuid,
    pub process: ProcessIdentity,
    /// The bzz that started it. While that bzz lives, the agent is its.
    pub owner: ProcessIdentity,
}

pub fn run_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("run")
}

/// An agent's own folder (`docs/agent-host-protocol.md`, section 1).
pub fn agent_dir(data_dir: &Path, agent: Uuid) -> PathBuf {
    data_dir.join("agents").join(agent.to_string())
}

fn record_path(run_dir: &Path, agent: Uuid) -> PathBuf {
    run_dir.join(format!("{agent}.json"))
}

pub fn create_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|error| Error::io(path, error))?;
    crate::paths::set_private_permissions(path)
}

fn write_record(run_dir: &Path, record: &OwnershipRecord) -> Result<()> {
    create_private_dir(run_dir)?;
    let path = record_path(run_dir, record.agent);
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
    std::fs::write(&temporary, bytes).map_err(|error| Error::io(&temporary, error))?;
    crate::paths::set_private_permissions(&temporary)?;
    // A rename, so that a reader never sees half a record.
    std::fs::rename(&temporary, &path).map_err(|error| Error::io(&path, error))
}

pub fn read_record(run_dir: &Path, agent: Uuid) -> Option<OwnershipRecord> {
    serde_json::from_slice(&std::fs::read(record_path(run_dir, agent)).ok()?).ok()
}

fn remove_record(run_dir: &Path, agent: Uuid) {
    let _ = std::fs::remove_file(record_path(run_dir, agent));
}

/// What the startup sweep found for one ownership record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Leftover {
    /// A harness whose bzz is gone was still running and has been stopped.
    Stopped { agent: Uuid, pid: u32 },
    /// Another running bzz owns this agent's process; it is left alone and
    /// this bzz must not start the agent.
    OwnedElsewhere { agent: Uuid, owner_pid: u32 },
    /// The record named no running process.
    Stale { agent: Uuid },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GroupSignal {
    Terminate,
    Kill,
}

/// Signals every process of the group that `pid` leads.
#[cfg(unix)]
fn signal_group(pid: u32, signal: GroupSignal) {
    use rustix::process::{Pid, Signal, getpgrp, kill_process_group};
    let Some(group) = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 1)
        .and_then(Pid::from_raw)
    else {
        return;
    };
    // Never bzz's own group, whatever a damaged record says.
    if group == getpgrp() {
        return;
    }
    let _ = kill_process_group(
        group,
        match signal {
            GroupSignal::Terminate => Signal::TERM,
            GroupSignal::Kill => Signal::KILL,
        },
    );
}

/// Windows has no process groups to signal: `taskkill /T` walks the tree.
#[cfg(windows)]
fn signal_group(pid: u32, signal: GroupSignal) {
    let mut command = std::process::Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if signal == GroupSignal::Kill {
        command.arg("/F");
    }
    let _ = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

async fn gone_within(identity: &ProcessIdentity, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if !is_alive(identity) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Stops the group of a process this bzz does not hold as a child: TERM, a
/// grace period, then KILL. Each signal is sent only while the pid still
/// names the same process.
async fn stop_foreign(identity: &ProcessIdentity, grace: Duration) {
    if !is_alive(identity) {
        return;
    }
    signal_group(identity.pid, GroupSignal::Terminate);
    if gone_within(identity, grace).await {
        return;
    }
    if is_alive(identity) {
        signal_group(identity.pid, GroupSignal::Kill);
        gone_within(identity, grace).await;
    }
}

/// Stops every harness left by a bzz that is gone, and reports what each
/// ownership record was. Runs at startup, before any recovery: a leftover
/// could still be answering.
pub async fn sweep(run_dir: &Path) -> Vec<Leftover> {
    sweep_with_grace(run_dir, STOP_GRACE).await
}

pub async fn sweep_with_grace(run_dir: &Path, grace: Duration) -> Vec<Leftover> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(run_dir) else {
        return found;
    };
    let own_pid = std::process::id();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            // A half-written record of a bzz that died.
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let record = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OwnershipRecord>(&bytes).ok());
        let Some(record) = record else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        // A record of this very process is from an earlier run inside it.
        if record.owner.pid != own_pid && is_alive(&record.owner) {
            found.push(Leftover::OwnedElsewhere {
                agent: record.agent,
                owner_pid: record.owner.pid,
            });
            continue;
        }
        if is_alive(&record.process) {
            stop_foreign(&record.process, grace).await;
            found.push(Leftover::Stopped {
                agent: record.agent,
                pid: record.process.pid,
            });
        } else {
            found.push(Leftover::Stale {
                agent: record.agent,
            });
        }
        let _ = std::fs::remove_file(&path);
    }
    found
}

/// A harness this bzz started.
pub struct Harness {
    child: Child,
    identity: Option<ProcessIdentity>,
    agent: Uuid,
    run_dir: PathBuf,
}

/// Copies at most `limit` bytes of a harness's standard error to its log and
/// keeps reading after that, so a noisy harness never blocks on it.
async fn copy_log(mut stderr: tokio::process::ChildStderr, path: PathBuf, limit: u64) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).await.ok();
    let mut written = 0_u64;
    let mut buffer = vec![0_u8; 16 * 1024];
    while let Ok(read) = stderr.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let room = usize::try_from(limit.saturating_sub(written)).unwrap_or(usize::MAX);
        let take = read.min(room);
        if take == 0 {
            // Past the limit: read on, so the harness never blocks on it.
            continue;
        }
        let Some(log) = file.as_mut() else { continue };
        if log.write_all(&buffer[..take]).await.is_err() {
            file = None;
            continue;
        }
        written += take as u64;
        if written >= limit {
            let _ = log.write_all(b"\n[bzz: log limit reached]\n").await;
        }
    }
    if let Some(mut log) = file {
        let _ = log.flush().await;
    }
}

impl Harness {
    /// Starts `launch` for `agent` and writes its ownership record before
    /// anything is said to it.
    pub fn spawn(
        launch: &Launch,
        run_dir: &Path,
        agent: Uuid,
    ) -> Result<(Self, ChildStdin, ChildStdout)> {
        if let Some(parent) = launch.log.parent() {
            create_private_dir(parent)?;
        }
        create_private_dir(&launch.cwd)?;
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .env_clear()
            .envs(launch.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Its own group: one signal reaches the harness and everything it
        // starts, and a Ctrl-C in bzz's terminal does not reach it.
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
        let mut child = command.spawn().map_err(|error| {
            Error::Config(format!(
                "the agent's harness could not be started ({}): {error}",
                launch.program
            ))
        })?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(Error::Config("the harness has no standard streams".into()));
        };
        tokio::spawn(copy_log(stderr, launch.log.clone(), LOG_LIMIT_BYTES));
        let identity = child.id().and_then(identity_of);
        let mut harness = Self {
            child,
            identity,
            agent,
            run_dir: run_dir.to_path_buf(),
        };
        let owner = identity_of(std::process::id());
        if let (Some(process), Some(owner)) = (harness.identity.clone(), owner)
            && let Err(error) = write_record(
                run_dir,
                &OwnershipRecord {
                    agent,
                    process,
                    owner,
                },
            )
        {
            // Without a record a crash of bzz would leave it unfindable.
            harness.kill_now();
            return Err(error);
        }
        Ok((harness, stdin, stdout))
    }

    pub fn pid(&self) -> Option<u32> {
        self.identity.as_ref().map(|identity| identity.pid)
    }

    /// Waits for the harness itself to exit.
    pub async fn wait(&mut self) -> Option<std::process::ExitStatus> {
        self.child.wait().await.ok()
    }

    fn signal(&self, signal: GroupSignal) {
        // Before the child is reaped its pid cannot name anything else.
        if let Some(identity) = &self.identity {
            signal_group(identity.pid, signal);
        }
    }

    fn kill_now(&mut self) {
        self.signal(GroupSignal::Kill);
        let _ = self.child.start_kill();
        remove_record(&self.run_dir, self.agent);
    }

    /// Stops the harness and everything in its group. `close_input` closes
    /// its standard input, which a harness answers by exiting; after `grace`
    /// the group gets TERM, and after `grace` again KILL.
    pub async fn stop(mut self, close_input: impl FnOnce(), grace: Duration) {
        close_input();
        let exited = async |child: &mut Child, limit: Duration| {
            tokio::time::timeout(limit, child.wait()).await.is_ok()
        };
        if !exited(&mut self.child, grace).await {
            self.signal(GroupSignal::Terminate);
            if !exited(&mut self.child, grace).await {
                self.signal(GroupSignal::Kill);
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
            }
        }
        // What the harness started and left behind goes with it.
        self.signal(GroupSignal::Kill);
        self.identity = None;
        remove_record(&self.run_dir, self.agent);
    }

    /// After the harness exited by itself: clears what it left in its group
    /// and its record.
    pub async fn reap(mut self) {
        self.signal(GroupSignal::Kill);
        let _ = self.child.wait().await;
        self.identity = None;
        remove_record(&self.run_dir, self.agent);
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Reached without `stop` only when a task is torn down; the child
        // itself is killed by `kill_on_drop`, the group here.
        self.signal(GroupSignal::Kill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_is_built_from_nothing() {
        let env = base_environment(Path::new("/agents/a"), |name| match name {
            "PATH" => Some("/opt/bin:/usr/bin".into()),
            "BZZ_PASSPHRASE_FD" | "NOSTR_SECRET_KEY" | "BUZZ_RELAY_URL" | "HOME" => {
                Some("must not pass".into())
            }
            _ => None,
        });
        let get = |name: &str| {
            env.iter()
                .find(|(existing, _)| existing == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(get("PATH"), Some("/opt/bin:/usr/bin"));
        assert_eq!(
            get("HOME"),
            Some("/agents/a"),
            "bzz's own HOME is not passed on"
        );
        assert_eq!(get("LANG"), Some("C.UTF-8"));
        assert!(get("TMPDIR").is_some());
        let mut names = env
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        #[cfg(not(windows))]
        assert_eq!(names, ["HOME", "LANG", "PATH", "TMPDIR"]);
        assert!(!format!("{env:?}").contains("must not pass"));
    }

    #[test]
    fn the_start_time_is_read_past_a_hostile_command_name() {
        let stat = "4242 (a) R (b c) S 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        assert_eq!(linux_start_time(stat).as_deref(), Some("987654"));
        let zombie = "4242 (node) Z 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert_eq!(linux_start_time(zombie), None, "a zombie is not running");
        assert_eq!(linux_start_time("garbage"), None);
    }

    #[cfg(unix)]
    fn launch(directory: &Path, script: &str) -> Launch {
        Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: base_environment(directory, |name| std::env::var(name).ok()),
            cwd: directory.to_path_buf(),
            log: directory.join("harness.log"),
        }
    }

    #[cfg(unix)]
    async fn wait_for(path: &Path) -> String {
        for _ in 0..400 {
            if let Ok(text) = std::fs::read_to_string(path)
                && text.ends_with('\n')
            {
                return text.trim().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{} was not written", path.display());
    }

    #[cfg(unix)]
    fn running(pid: u32) -> bool {
        identity_of(pid).is_some()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_harness_leaves_on_end_of_input_and_its_record_goes() {
        let temporary = tempfile::TempDir::new().unwrap();
        let run = run_dir(temporary.path());
        let agent = Uuid::new_v4();
        let (harness, stdin, _stdout) = Harness::spawn(
            &launch(temporary.path(), "cat >/dev/null; echo bye >&2"),
            &run,
            agent,
        )
        .unwrap();
        let record = read_record(&run, agent).expect("an ownership record");
        assert_eq!(Some(record.process.pid), harness.pid());
        assert_eq!(record.owner.pid, std::process::id());
        assert!(is_alive(&record.process) && is_alive(&record.owner));
        #[cfg(target_os = "linux")]
        assert!(record.process.boot_id.is_some());
        let started = std::time::Instant::now();
        harness.stop(move || drop(stdin), STOP_GRACE).await;
        assert!(started.elapsed() < STOP_GRACE, "it left on end of input");
        assert!(!is_alive(&record.process));
        assert!(read_record(&run, agent).is_none());
        assert_eq!(wait_for(&temporary.path().join("harness.log")).await, "bye");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_deaf_harness_and_what_it_started_are_killed_as_a_group() {
        let temporary = tempfile::TempDir::new().unwrap();
        let run = run_dir(temporary.path());
        let pid_file = temporary.path().join("grandchild");
        // Ignores end of input and TERM, and starts a process of its own.
        let script = format!(
            "trap '' TERM; sleep 600 & echo $! > {}; while :; do sleep 1; done",
            pid_file.display()
        );
        let (harness, stdin, _stdout) =
            Harness::spawn(&launch(temporary.path(), &script), &run, Uuid::new_v4()).unwrap();
        let grandchild = wait_for(&pid_file).await.parse::<u32>().unwrap();
        let pid = harness.pid().unwrap();
        assert!(running(grandchild));
        let grace = Duration::from_millis(300);
        let started = std::time::Instant::now();
        harness.stop(move || drop(stdin), grace).await;
        assert!(started.elapsed() >= grace * 2, "TERM was tried before KILL");
        assert!(!running(pid), "the harness is gone");
        for _ in 0..200 {
            if !running(grandchild) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!running(grandchild), "what it started is gone too");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_sweep_stops_a_leftover_whose_bzz_is_gone_and_spares_a_live_one() {
        let temporary = tempfile::TempDir::new().unwrap();
        let run = run_dir(temporary.path());
        let (orphan, mine, stale) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let pid_file = temporary.path().join("grandchild");
        let script = format!(
            "trap '' TERM; sleep 600 & echo $! > {}; while :; do sleep 1; done",
            pid_file.display()
        );
        let (mut left, _stdin, _stdout) =
            Harness::spawn(&launch(temporary.path(), &script), &run, orphan).unwrap();
        let grandchild = wait_for(&pid_file).await.parse::<u32>().unwrap();
        let orphan_pid = left.pid().unwrap();
        // As if the bzz that started it had died: its owner names no process.
        let mut record = read_record(&run, orphan).unwrap();
        record.owner.start_time = "0".into();
        record.owner.pid = u32::MAX - 7;
        write_record(&run, &record).unwrap();
        // A second agent whose owner is another process that is alive.
        let mut owner = tokio::process::Command::new("sleep")
            .arg("600")
            .spawn()
            .unwrap();
        let (kept, kept_stdin, _kept_stdout) =
            Harness::spawn(&launch(temporary.path(), "cat >/dev/null"), &run, mine).unwrap();
        let mut record = read_record(&run, mine).unwrap();
        record.owner = identity_of(owner.id().unwrap()).unwrap();
        write_record(&run, &record).unwrap();
        // A record of a process that no longer exists, and a torn file.
        write_record(
            &run,
            &OwnershipRecord {
                agent: stale,
                process: ProcessIdentity {
                    pid: orphan_pid,
                    start_time: "1".into(),
                    boot_id: None,
                },
                owner: ProcessIdentity {
                    pid: u32::MAX - 7,
                    start_time: "0".into(),
                    boot_id: None,
                },
            },
        )
        .unwrap();
        std::fs::write(run.join("torn.json.tmp"), "{").unwrap();

        let mut found = sweep_with_grace(&run, Duration::from_millis(300)).await;
        found.sort_by_key(|leftover| format!("{leftover:?}"));
        assert!(
            found.contains(&Leftover::Stopped {
                agent: orphan,
                pid: orphan_pid
            }),
            "{found:?}"
        );
        assert!(
            found.contains(&Leftover::OwnedElsewhere {
                agent: mine,
                owner_pid: owner.id().unwrap()
            }),
            "{found:?}"
        );
        assert!(
            found.contains(&Leftover::Stale { agent: stale }),
            "{found:?}"
        );
        // Reaped here; had the sweep left it alone, this would never return.
        let reaped = tokio::time::timeout(Duration::from_secs(5), left.wait()).await;
        assert!(reaped.is_ok(), "the leftover was stopped");
        assert!(!running(orphan_pid));
        for _ in 0..200 {
            if !running(grandchild) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!running(grandchild), "with its group");
        assert!(
            running(kept.pid().unwrap()),
            "another bzz's agent is left alone"
        );
        assert!(read_record(&run, mine).is_some());
        assert!(read_record(&run, orphan).is_none() && read_record(&run, stale).is_none());
        assert!(!run.join("torn.json.tmp").exists());
        kept.stop(move || drop(kept_stdin), STOP_GRACE).await;
        let _ = owner.kill().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_program_that_does_not_exist_is_an_error_and_leaves_no_record() {
        let temporary = tempfile::TempDir::new().unwrap();
        let run = run_dir(temporary.path());
        let agent = Uuid::new_v4();
        let mut missing = launch(temporary.path(), "");
        missing.program = "/nonexistent/bzz-harness".into();
        let error = Harness::spawn(&missing, &run, agent).err().unwrap();
        assert!(
            error.to_string().contains("could not be started"),
            "{error}"
        );
        assert!(read_record(&run, agent).is_none());
    }
}
