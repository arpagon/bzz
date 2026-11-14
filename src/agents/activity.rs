//! What an agent asked of its owner and what it was answered: each real
//! command put to bzz for approval, and each git credential asked for
//! (plan v0.15.0, M7 and M3). `bzz agent show` prints the end of it.
//!
//! It is a file of the machine, `<data>/agents/<id>/activity.jsonl`, one
//! JSON object per line, for the owner only. It holds the first line of a
//! command and the name of a host. It never holds a credential, a key or
//! what a command printed.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{agents::runtime::process, render::sanitize};

/// The most of a command's first line that is kept.
pub const MAX_COMMAND_CHARS: usize = 200;
/// Past this size the file is cut to its newest [`KEPT_LINES`] lines.
const MAX_BYTES: u64 = 512 * 1024;
const KEPT_LINES: usize = 1000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "what", rename_all = "kebab-case")]
pub enum What {
    /// A real command the agent wanted to start in a granted folder.
    Command {
        /// The folder's name: `/mnt/<folder>` to the agent.
        folder: String,
        /// The command's first line, with what would not show written out.
        command: String,
        /// How many lines the whole command has.
        lines: usize,
    },
    /// A git credential the agent's host asked for.
    Credential { host: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Entry {
    /// Unix seconds.
    pub at: u64,
    #[serde(flatten)]
    pub what: What,
    /// What happened: `asked of you`, `allowed by you`, `refused by you:
    /// …`, `allowed without asking`, `refused: …`, `given`.
    pub answer: String,
}

impl Entry {
    fn now(what: What, answer: impl Into<String>) -> Self {
        Self {
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            what,
            answer: sanitize::visible(&answer.into()),
        }
    }

    /// An entry for a command, of which the first line is kept.
    pub fn command(folder: &str, command: &str, answer: impl Into<String>) -> Self {
        let first = command.split('\n').next().unwrap_or_default();
        let mut shown = sanitize::visible(first)
            .chars()
            .take(MAX_COMMAND_CHARS)
            .collect::<String>();
        if sanitize::visible(first).chars().count() > MAX_COMMAND_CHARS {
            shown.push('…');
        }
        Self::now(
            What::Command {
                folder: sanitize::visible(folder),
                command: shown,
                lines: command.split('\n').count(),
            },
            answer,
        )
    }

    /// An entry for a credential asked for. Only the host is kept.
    pub fn credential(host: &str, answer: impl Into<String>) -> Self {
        Self::now(
            What::Credential {
                host: sanitize::visible(host).chars().take(300).collect(),
            },
            answer,
        )
    }

    /// The entry as `bzz agent show` prints it: when, the folder, the
    /// command's first line, the answer.
    pub fn line(&self) -> String {
        let when = crate::agents::manage::when(self.at);
        match &self.what {
            What::Command {
                folder,
                command,
                lines,
            } => format!(
                "{when} · command in /mnt/{folder} · {command}{} · {}",
                if *lines > 1 {
                    format!(" (and {} more line(s))", lines - 1)
                } else {
                    String::new()
                },
                self.answer
            ),
            What::Credential { host } => {
                format!("{when} · git credential for {host} · {}", self.answer)
            }
        }
    }
}

pub fn path(data_dir: &Path, agent: Uuid) -> PathBuf {
    process::agent_dir(data_dir, agent).join("activity.jsonl")
}

/// Adds an entry. Best effort: a turn never fails because its record
/// could not be written.
pub fn record(data_dir: &Path, agent: Uuid, entry: &Entry) {
    let path = path(data_dir, agent);
    if let Some(parent) = path.parent()
        && process::create_private_dir(parent).is_err()
    {
        return;
    }
    let Ok(mut line) = serde_json::to_string(entry) else {
        return;
    };
    line.push('\n');
    if std::fs::metadata(&path).is_ok_and(|file| file.len() > MAX_BYTES) {
        trim(&path);
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(&path) {
        // One write per entry, at the end of the file.
        let _ = file.write_all(line.as_bytes());
    }
}

fn trim(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines = text.lines().collect::<Vec<_>>();
    let kept = lines[lines.len().saturating_sub(KEPT_LINES)..].join("\n") + "\n";
    let temporary = path.with_extension("jsonl.tmp");
    if std::fs::write(&temporary, kept).is_ok()
        && crate::paths::set_private_permissions(&temporary).is_ok()
    {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// The newest `limit` entries, oldest first. A line that is no entry is
/// passed over.
pub fn recent(data_dir: &Path, agent: Uuid, limit: usize) -> Vec<Entry> {
    let text = std::fs::read_to_string(path(data_dir, agent)).unwrap_or_default();
    let mut entries = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
        .take(limit)
        .collect::<Vec<_>>();
    entries.reverse();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_and_its_answer_are_kept_as_one_line_each() {
        let temporary = tempfile::TempDir::new().unwrap();
        let agent = Uuid::new_v4();
        assert!(recent(temporary.path(), agent, 10).is_empty());
        record(
            temporary.path(),
            agent,
            &Entry::command("site", "npm test", "asked of you"),
        );
        // Several lines, an escape in the first, and far too long.
        let long = format!("echo \u{1b}[2K{}\nrm -rf ~\necho done", "x".repeat(400));
        record(
            temporary.path(),
            agent,
            &Entry::command("site", &long, "refused by you: not\nthat"),
        );
        record(
            temporary.path(),
            agent,
            &Entry::credential("github.com", "given"),
        );
        let entries = recent(temporary.path(), agent, 10);
        assert_eq!(entries.len(), 3);
        let lines = entries.iter().map(Entry::line).collect::<Vec<_>>();
        assert!(
            lines[0].ends_with(" · command in /mnt/site · npm test · asked of you"),
            "{lines:?}"
        );
        assert!(lines[0].starts_with("20"), "when: {}", lines[0]);
        let What::Command {
            command,
            lines: count,
            ..
        } = &entries[1].what
        else {
            panic!("a command")
        };
        assert!(command.starts_with("echo \\x1b[2Kxxx") && command.ends_with('…'));
        assert_eq!(command.chars().count(), MAX_COMMAND_CHARS + 1);
        assert_eq!(*count, 3);
        assert!(
            lines[1].contains("(and 2 more line(s)) · refused by you: not\\nthat"),
            "{}",
            lines[1]
        );
        assert!(!lines[1].contains("rm -rf"), "only the first line is kept");
        assert!(
            lines[2].ends_with(" · git credential for github.com · given"),
            "{}",
            lines[2]
        );
        // The file is one JSON object per line, for the owner only.
        let text = std::fs::read_to_string(path(temporary.path(), agent)).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(!text.contains('\u{1b}'));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(path(temporary.path(), agent))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // The newest are the ones returned, oldest first; a damaged line
        // is passed over.
        std::fs::OpenOptions::new()
            .append(true)
            .open(path(temporary.path(), agent))
            .unwrap()
            .write_all(b"{not json\n")
            .unwrap();
        assert_eq!(recent(temporary.path(), agent, 2), entries[1..]);
    }

    #[test]
    fn the_file_does_not_grow_without_bound() {
        let temporary = tempfile::TempDir::new().unwrap();
        let agent = Uuid::new_v4();
        let entry = Entry::command(
            "site",
            &"x".repeat(MAX_COMMAND_CHARS),
            "allowed without asking",
        );
        for _ in 0..4000 {
            record(temporary.path(), agent, &entry);
        }
        let size = std::fs::metadata(path(temporary.path(), agent))
            .unwrap()
            .len();
        assert!(size < MAX_BYTES * 2, "{size}");
        assert_eq!(recent(temporary.path(), agent, 5).len(), 5);
    }
}
