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

/// Holds `agent` for this process. Refused while another live process
/// holds it.
pub fn take(data_dir: &Path, agent: Uuid) -> Result<Held> {
    if let Some(Hold::Moving { pid }) = read(data_dir, agent)
        && pid != std::process::id()
    {
        return Err(Error::Config(format!(
            "another bzz is moving this agent right now (process {pid}); wait for it to finish"
        )));
    }
    let holder = process::identity_of(std::process::id())
        .ok_or_else(|| Error::Config("this process could not identify itself".into()))?;
    // Only "moved away" is worth putting back: a dead holder's record is not.
    let before = std::fs::read(path(data_dir, agent))
        .ok()
        .filter(|bytes| matches!(serde_json::from_slice(bytes), Ok(Record::MovedAway { .. })));
    write(data_dir, agent, &Record::Moving { holder })?;
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
}
