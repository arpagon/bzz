//! What keeps a bzz from starting an agent that is being moved, or that was
//! moved away (ADR v0.15 local agents, decision 2).
//!
//! It is one small file in the agent's folder, `move.json`, read at every
//! start of the agent's process and twice a second by the bzz that serves
//! the agent. The settings also say "moved away", but a bzz that is already
//! running holds its own copy of the settings: this file is what it sees.
//!
//! - `moving`: a `bzz agent move`, `launch-kit` or `receive` is at work, in
//!   the process named. It counts only while that process lives, so a
//!   command that was killed holds nothing.
//! - `moved-away`: the agent runs elsewhere. It stays until `bzz agent
//!   receive` takes the agent back.
//! - `deleting`: a `bzz agent delete` is at work, in the process named.
//!   A bzz that serves the agent stops its process and stops listening, as
//!   for a move; if the deletion does not happen, it runs the agent again.
//! - `deleted`: the deletion goes on. A bzz that serves the agent closes
//!   the agent's connection and forgets its key, for good, before the
//!   agent's files go.
//!
//! The other half is who serves an agent: each bzz that does writes
//! `<data>/agents/serving/<agent id>.<its pid>.json` while it holds the
//! agent's key and connection, and removes it when it has let both go. A
//! deletion waits for these to be gone before it removes the agent's files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    agents::runtime::process::{self, ProcessIdentity},
    error::{Error, Result},
};

/// What a status line says of an agent that was moved away.
pub const MOVED_AWAY: &str = "moved away: it runs elsewhere, and this bzz does not start it (`bzz agent receive` takes it back)";
/// What a status line says while a move is at work.
pub const MOVING: &str = "being moved: stopped until the move is over";
/// What a status line says while a deletion is at work.
pub const DELETING: &str = "being deleted: stopped until the deletion is over";
/// What a status line says of an agent that was deleted under a running bzz.
pub const DELETED: &str =
    "deleted: `bzz agent delete` removed it, and this bzz let go of its key and its connection";

/// Where a moved agent went.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Destination {
    /// A sealed file, for another bzz.
    File,
    /// A folder for a machine that runs Buzz's `buzz-acp`.
    LaunchKit,
}

impl Destination {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "in a sealed file, for another bzz",
            Self::LaunchKit => "as a launch kit, for a machine with Buzz's buzz-acp",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum Record {
    Moving { holder: ProcessIdentity },
    MovedAway { at: u64, to: Destination },
    Deleting { holder: ProcessIdentity },
    Deleted { at: u64 },
}

/// What the file says, when it says anything that counts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Hold {
    /// The live process `pid` is moving the agent.
    Moving { pid: u32 },
    /// `at` and `to` are absent when the file is there and cannot be read:
    /// in doubt the agent is not started.
    MovedAway {
        at: Option<u64>,
        to: Option<Destination>,
    },
    /// The live process `pid` is deleting the agent, and may still give up.
    Deleting { pid: u32 },
    /// The agent is deleted: its files are about to go.
    Deleted,
}

impl Hold {
    /// What a status line says of an agent held like this.
    pub const fn words(&self) -> &'static str {
        match self {
            Self::Moving { .. } => MOVING,
            Self::MovedAway { .. } => MOVED_AWAY,
            Self::Deleting { .. } => DELETING,
            Self::Deleted => DELETED,
        }
    }

    /// Whether the agent may run here again when the file goes away.
    pub const fn passes(&self) -> bool {
        matches!(self, Self::Moving { .. } | Self::Deleting { .. })
    }
}

pub fn path(data_dir: &Path, agent: Uuid) -> PathBuf {
    process::agent_dir(data_dir, agent).join("move.json")
}

/// What holds `agent` now. `None`: nothing does.
pub fn read(data_dir: &Path, agent: Uuid) -> Option<Hold> {
    let bytes = match std::fs::read(path(data_dir, agent)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        // There and unreadable: in doubt, not started.
        Err(_) => return Some(Hold::MovedAway { at: None, to: None }),
    };
    match serde_json::from_slice::<Record>(&bytes) {
        Ok(Record::Moving { holder }) => {
            process::is_alive(&holder).then_some(Hold::Moving { pid: holder.pid })
        }
        Ok(Record::MovedAway { at, to }) => Some(Hold::MovedAway {
            at: Some(at),
            to: Some(to),
        }),
        Ok(Record::Deleting { holder }) => {
            process::is_alive(&holder).then_some(Hold::Deleting { pid: holder.pid })
        }
        Ok(Record::Deleted { .. }) => Some(Hold::Deleted),
        Err(_) => Some(Hold::MovedAway { at: None, to: None }),
    }
}

/// Whether the file says the agent was moved away.
pub fn moved_away(data_dir: &Path, agent: Uuid) -> bool {
    matches!(read(data_dir, agent), Some(Hold::MovedAway { .. }))
}

fn write(data_dir: &Path, agent: Uuid, record: &Record) -> Result<()> {
    let path = path(data_dir, agent);
    if let Some(folder) = path.parent() {
        process::create_private_dir(folder)?;
    }
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes =
        serde_json::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
    std::fs::write(&temporary, bytes).map_err(|error| Error::io(&temporary, error))?;
    crate::paths::set_private_permissions(&temporary)?;
    // A rename, so that a reader never sees half a record.
    std::fs::rename(&temporary, &path).map_err(|error| Error::io(&path, error))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The agent, held by this process for a move. Dropped without
/// [`Held::moved_away`] or [`Held::free`], it puts back what the file said
/// before: nothing, or "moved away".
#[derive(Debug)]
pub struct Held {
    data_dir: PathBuf,
    agent: Uuid,
    /// What the file held before, to put back.
    before: Option<Vec<u8>>,
    settled: bool,
}

/// Holds `agent` for this process, for a move. Refused while another live
/// process holds it.
pub fn take(data_dir: &Path, agent: Uuid) -> Result<Held> {
    take_as(data_dir, agent, |holder| Record::Moving { holder })
}

/// Holds `agent` for this process, for a deletion that may still not
/// happen. Refused while another live process holds it.
pub fn take_for_deletion(data_dir: &Path, agent: Uuid) -> Result<Held> {
    take_as(data_dir, agent, |holder| Record::Deleting { holder })
}

fn take_as(
    data_dir: &Path,
    agent: Uuid,
    record: impl FnOnce(ProcessIdentity) -> Record,
) -> Result<Held> {
    match read(data_dir, agent) {
        Some(Hold::Moving { pid }) if pid != std::process::id() => {
            return Err(Error::Config(format!(
                "another bzz is moving this agent right now (process {pid}); wait for it to finish"
            )));
        }
        Some(Hold::Deleting { pid }) if pid != std::process::id() => {
            return Err(Error::Config(format!(
                "another bzz is deleting this agent right now (process {pid}); wait for it to finish"
            )));
        }
        _ => {}
    }
    let holder = process::identity_of(std::process::id())
        .ok_or_else(|| Error::Config("this process could not identify itself".into()))?;
    // Only "moved away" is worth putting back: a dead holder's record is not.
    let before = std::fs::read(path(data_dir, agent))
        .ok()
        .filter(|bytes| matches!(serde_json::from_slice(bytes), Ok(Record::MovedAway { .. })));
    write(data_dir, agent, &record(holder))?;
    Ok(Held {
        data_dir: data_dir.to_path_buf(),
        agent,
        before,
        settled: false,
    })
}

impl Held {
    /// The last step of a move: from now on no bzz starts the agent here.
    pub fn moved_away(mut self, to: Destination) -> Result<()> {
        write(
            &self.data_dir,
            self.agent,
            &Record::MovedAway { at: unix_now(), to },
        )?;
        self.settled = true;
        Ok(())
    }

    /// The deletion goes on: from now on every bzz that serves the agent
    /// lets go of its key and its connection for good. The file goes with
    /// the agent's folder.
    pub fn deleted(mut self) -> Result<()> {
        write(
            &self.data_dir,
            self.agent,
            &Record::Deleted { at: unix_now() },
        )?;
        self.settled = true;
        Ok(())
    }

    /// The last step of taking an agent in: nothing holds it any more.
    pub fn free(mut self) -> Result<()> {
        let path = path(&self.data_dir, self.agent);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io(path, error)),
        }
        self.settled = true;
        Ok(())
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let path = path(&self.data_dir, self.agent);
        match &self.before {
            Some(bytes) => {
                let _ = std::fs::write(&path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

// ---- who serves an agent ---------------------------------------------------

fn serving_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("serving")
}

fn serving_path(data_dir: &Path, agent: Uuid, pid: u32) -> PathBuf {
    serving_dir(data_dir).join(format!("{agent}.{pid}.json"))
}

/// Says that this bzz holds `agent`'s key and its connection from now on.
/// Written before either is used.
pub fn serve(data_dir: &Path, agent: Uuid) -> Result<()> {
    let folder = serving_dir(data_dir);
    process::create_private_dir(&folder)?;
    let me = process::identity_of(std::process::id())
        .ok_or_else(|| Error::Config("this process could not identify itself".into()))?;
    let path = serving_path(data_dir, agent, me.pid);
    let bytes = serde_json::to_vec(&me).map_err(|error| Error::Serialization(error.to_string()))?;
    std::fs::write(&path, bytes).map_err(|error| Error::io(&path, error))?;
    crate::paths::set_private_permissions(&path)
}

/// Says that this bzz has let go of `agent`: its connection is closed and
/// its key is forgotten.
pub fn let_go(data_dir: &Path, agent: Uuid) {
    let _ = std::fs::remove_file(serving_path(data_dir, agent, std::process::id()));
}

/// The live bzz processes, other than this one, that hold `agent`'s key and
/// connection. A record whose process is gone is removed: a bzz that was
/// killed holds nothing.
pub fn served_by(data_dir: &Path, agent: Uuid) -> Vec<u32> {
    let prefix = format!("{agent}.");
    let Ok(entries) = std::fs::read_dir(serving_dir(data_dir)) else {
        return Vec::new();
    };
    let mut serving = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let alive = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProcessIdentity>(&bytes).ok())
            .filter(process::is_alive);
        match alive {
            Some(identity) if identity.pid != std::process::id() => serving.push(identity.pid),
            Some(_) => {}
            None => {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    serving.sort_unstable();
    serving
}

/// Removes whatever the file says. For a move whose last step could not be
/// written down in the settings: it did not happen.
pub fn clear(data_dir: &Path, agent: Uuid) {
    let _ = std::fs::remove_file(path(data_dir, agent));
}

/// Writes "moved away" for an agent whose settings say so and whose folder
/// does not: a settings file edited by hand, or one from before the file
/// existed.
pub fn mark_moved_away(data_dir: &Path, agent: Uuid, to: Destination) -> Result<()> {
    write(data_dir, agent, &Record::MovedAway { at: unix_now(), to })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_counts_while_its_holder_lives_and_moved_away_stays() {
        let data = tempfile::TempDir::new().unwrap();
        let agent = Uuid::new_v4();
        assert_eq!(read(data.path(), agent), None);

        // Held by this process, and let go when the move does not happen.
        let held = take(data.path(), agent).unwrap();
        assert_eq!(
            read(data.path(), agent),
            Some(Hold::Moving {
                pid: std::process::id()
            })
        );
        drop(held);
        assert_eq!(
            read(data.path(), agent),
            None,
            "a failed move holds nothing"
        );

        // A holder that is gone holds nothing, and another may take it.
        let gone = Record::Moving {
            holder: ProcessIdentity {
                pid: 1,
                start_time: "never".into(),
                boot_id: None,
            },
        };
        write(data.path(), agent, &gone).unwrap();
        assert_eq!(read(data.path(), agent), None);
        // A holder that lives is respected, for as long as it lives.
        #[cfg(unix)]
        {
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let holder = process::identity_of(child.id()).unwrap();
            write(data.path(), agent, &Record::Moving { holder }).unwrap();
            assert_eq!(
                read(data.path(), agent),
                Some(Hold::Moving { pid: child.id() })
            );
            let refused = take(data.path(), agent).unwrap_err().to_string();
            assert!(refused.contains("another bzz is moving"), "{refused}");
            child.kill().unwrap();
            child.wait().unwrap();
            assert_eq!(read(data.path(), agent), None);
        }

        // The last step of a move stays, whoever lives or dies.
        take(data.path(), agent)
            .unwrap()
            .moved_away(Destination::File)
            .unwrap();
        assert!(moved_away(data.path(), agent));
        assert!(matches!(
            read(data.path(), agent),
            Some(Hold::MovedAway {
                at: Some(_),
                to: Some(Destination::File)
            })
        ));
        // Taking it back that fails halfway leaves it moved away.
        drop(take(data.path(), agent).unwrap());
        assert!(matches!(
            read(data.path(), agent),
            Some(Hold::MovedAway {
                to: Some(Destination::File),
                ..
            })
        ));
        // Taking it back for good.
        take(data.path(), agent).unwrap().free().unwrap();
        assert_eq!(read(data.path(), agent), None);

        // A deletion holds the agent as a move does, and gives it back when
        // it does not happen; once it goes on, the agent never runs here.
        let held = take_for_deletion(data.path(), agent).unwrap();
        assert_eq!(
            read(data.path(), agent),
            Some(Hold::Deleting {
                pid: std::process::id()
            })
        );
        assert!(read(data.path(), agent).unwrap().passes());
        drop(held);
        assert_eq!(read(data.path(), agent), None, "a deletion that failed");
        take_for_deletion(data.path(), agent)
            .unwrap()
            .deleted()
            .unwrap();
        assert_eq!(read(data.path(), agent), Some(Hold::Deleted));
        assert!(!Hold::Deleted.passes() && !moved_away(data.path(), agent));
        clear(data.path(), agent);

        // A file that cannot be read: in doubt the agent is not started.
        std::fs::write(path(data.path(), agent), b"{ half a rec").unwrap();
        assert_eq!(
            read(data.path(), agent),
            Some(Hold::MovedAway { at: None, to: None })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            mark_moved_away(data.path(), agent, Destination::LaunchKit).unwrap();
            let mode = std::fs::metadata(path(data.path(), agent))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_bzz_that_serves_an_agent_says_so_until_it_lets_go() {
        let data = tempfile::TempDir::new().unwrap();
        let agent = Uuid::new_v4();
        assert!(served_by(data.path(), agent).is_empty());
        // This bzz itself is never one a deletion waits for.
        serve(data.path(), agent).unwrap();
        assert!(served_by(data.path(), agent).is_empty());
        assert!(serving_path(data.path(), agent, std::process::id()).exists());
        let_go(data.path(), agent);
        assert!(!serving_path(data.path(), agent, std::process::id()).exists());
        #[cfg(unix)]
        {
            // Another bzz that lives is one; when it is gone, so is its word.
            let mut other = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let identity = process::identity_of(other.id()).unwrap();
            let path = serving_path(data.path(), agent, other.id());
            std::fs::write(&path, serde_json::to_vec(&identity).unwrap()).unwrap();
            assert_eq!(served_by(data.path(), agent), vec![other.id()]);
            assert!(
                served_by(data.path(), Uuid::new_v4()).is_empty(),
                "another agent"
            );
            other.kill().unwrap();
            other.wait().unwrap();
            assert!(served_by(data.path(), agent).is_empty());
            assert!(!path.exists(), "a dead bzz's record is removed");
        }
    }
}
