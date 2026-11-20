//! What the owner is shown of a version before he answers (rule 16): who it
//! comes from, its brief, the record types it declares **and any change in
//! them, marked as such**, its sizes, and its difference from the version
//! that runs.
//!
//! A [`Review`] is data, so that the overlay of the TUI can lay it out as it
//! likes; [`Review::render`] is the same thing as plain text, which the
//! command line prints. Everything in it that a proposer wrote (the brief,
//! the script's lines, a name) goes through [`visible`] before it is shown.

use uuid::Uuid;

use crate::store::apps::{Proposer, ProposerKind};

use super::diff::Difference;
use super::manifest::{bytes, hidden_characters, visible};

/// How the declared record types of a version differ from those of the
/// version that runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypesChange {
    /// No version runs yet: every type is new to this machine.
    First,
    /// The same types as the version that runs.
    Unchanged,
    /// Other types than the version that runs. Never empty on both sides.
    Changed {
        /// Declared now, and not by the version that runs.
        added: Vec<String>,
        /// Declared by the version that runs, and no longer.
        removed: Vec<String>,
    },
}

impl TypesChange {
    pub fn between(running: Option<&[String]>, proposed: &[String]) -> Self {
        let Some(running) = running else {
            return Self::First;
        };
        let added: Vec<String> = proposed
            .iter()
            .filter(|name| !running.contains(name))
            .cloned()
            .collect();
        let removed: Vec<String> = running
            .iter()
            .filter(|name| !proposed.contains(name))
            .cloned()
            .collect();
        if added.is_empty() && removed.is_empty() {
            Self::Unchanged
        } else {
            Self::Changed { added, removed }
        }
    }

    pub const fn is_changed(&self) -> bool {
        matches!(self, Self::Changed { .. })
    }
}

/// The version that runs, as far as a review compares with it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Running {
    pub version: u32,
    pub hash: String,
    pub title: String,
    pub script_bytes: u64,
    pub brief_bytes: u64,
    pub types: Vec<String>,
}

/// The offer in a channel a version was taken in from (M6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfferFacts {
    /// The artifact of the offer.
    pub artifact: Uuid,
    /// The event id of the exact revision, which `from` signed.
    pub revision: String,
}

/// One version, as the owner is shown it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    /// Who the version that runs came from, when one runs.
    pub installed_from: Option<Proposer>,
    /// The revision of an offer in a channel it came from, if it did.
    pub offer: Option<OfferFacts>,
    pub community: Uuid,
    pub name: String,
    pub title: String,
    pub version: u32,
    /// The SHA-256 of the script: what an approval names.
    pub hash: String,
    pub from: Proposer,
    /// Unix seconds.
    pub proposed_at: u64,
    pub script_bytes: u64,
    pub brief_bytes: u64,
    /// The brief, as it was given.
    pub brief: String,
    /// The brief is not the one of the version that runs.
    pub brief_changed: bool,
    /// The record types the version declares.
    pub types: Vec<String>,
    pub types_change: TypesChange,
    /// The version that runs, if one does.
    pub running: Option<Running>,
    /// Why there is nothing to compare with although a version runs: its
    /// files are not the ones that were approved.
    pub running_unreadable: Option<String>,
    /// The script against the script that runs; against nothing when none
    /// runs, so that every line is new.
    pub difference: Difference,
    /// Characters of the script and the brief that are not text, or that
    /// change the direction text is shown in. They are written out as
    /// `<U+XXXX>` wherever the script or the brief is shown.
    pub hidden_characters: usize,
}

/// Who a version comes from, in words.
pub fn from_whom(proposer: &Proposer) -> String {
    let name = proposer.name.as_deref().map(visible).unwrap_or_default();
    let key = proposer.id.as_deref().map(visible).unwrap_or_default();
    match proposer.kind {
        ProposerKind::Owner => "you".to_owned(),
        ProposerKind::Agent => match &proposer.asked_by {
            Some(asker) => format!(
                "your agent {name} ({key}), asked by {} ({})",
                visible(&asker.name),
                visible(&asker.id)
            ),
            None => format!("your agent {name} ({key})"),
        },
        ProposerKind::Member => format!("{name}, a person of the channel ({key})"),
    }
}

/// Who a version comes from, in a few words: for a line that says a
/// version waits.
pub fn from_whom_short(proposer: &Proposer) -> String {
    let name = proposer.name.as_deref().map(visible).unwrap_or_default();
    match proposer.kind {
        ProposerKind::Owner => "you".to_owned(),
        ProposerKind::Agent => match &proposer.asked_by {
            Some(asker) => format!("your agent {name}, asked by {}", visible(&asker.name)),
            None => format!("your agent {name}"),
        },
        ProposerKind::Member => format!("{name}, a person of the channel"),
    }
}

impl Review {
    /// The version comes from a person of the channel, and the version
    /// installed here from another one (or from you, or your agent).
    pub fn another_source(&self) -> Option<&Proposer> {
        let installed = self.installed_from.as_ref()?;
        (self.from.kind == ProposerKind::Member && !installed.is(&self.from)).then_some(installed)
    }

    /// May this version be approved on what is shown alone: it differs in
    /// nothing from the one that runs but in its brief or title.
    pub fn script_unchanged(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.hash == self.hash)
    }

    /// The lines that say what record types the version declares, with any
    /// change in them marked as such.
    pub fn types_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Record types: {}", self.types.join(", "))];
        match &self.types_change {
            TypesChange::First => {
                lines.push("  No version of this app runs yet: all of them are new.".into());
            }
            TypesChange::Unchanged => {
                lines.push("  The same as the version that runs.".into());
            }
            TypesChange::Changed { added, removed } => {
                lines.push("  CHANGED from the version that runs:".into());
                for name in added {
                    lines.push(format!(
                        "    + {name}   new: the app could not read or write these records before"
                    ));
                }
                for name in removed {
                    lines.push(format!(
                        "    - {name}   no longer declared: the app stops reading and writing them"
                    ));
                }
            }
        }
        lines
    }

    /// The review as plain text. `when` writes a time.
    pub fn render(&self, when: impl Fn(u64) -> String) -> String {
        let mut out = Vec::new();
        out.push(format!(
            "{} ({}), version {}",
            self.name,
            visible(&self.title),
            self.version
        ));
        let place = match self.from.channel {
            Some(channel) => format!(", in channel {channel}"),
            None => String::new(),
        };
        // Who asked an agent for it has a line of its own, so that it is
        // not cut off at the edge of the overlay (plan D7).
        let mut from = self.from.clone();
        let asker = from.asked_by.take();
        out.push(format!("From:     {}{place}", from_whom(&from)));
        if let Some(asker) = asker {
            out.push(format!(
                "Asked by: {} ({}), whose message asked the agent for it",
                visible(&asker.name),
                visible(&asker.id)
            ));
        }
        if let Some(offer) = &self.offer {
            out.push(format!(
                "Offer:    shared in the channel; this exact revision ({}…) is signed by {}",
                &offer.revision[..16.min(offer.revision.len())],
                self.from
                    .name
                    .as_deref()
                    .map_or_else(|| "them".to_owned(), visible)
            ));
        }
        if let Some(installed) = self.another_source() {
            out.push(format!(
                "Source:   CHANGED: the version installed here came from {}",
                from_whom(installed)
            ));
        }
        out.push(format!("Proposed: {}", when(self.proposed_at)));
        out.push(format!("Hash:     {}", self.hash));
        match &self.running {
            Some(running) => {
                out.push(format!(
                    "Script:   {} (version {}, which runs, is {})",
                    bytes(self.script_bytes),
                    running.version,
                    bytes(running.script_bytes)
                ));
                out.push(format!(
                    "Brief:    {}, {}",
                    bytes(self.brief_bytes),
                    if self.brief_changed {
                        "CHANGED from the version that runs"
                    } else {
                        "the same as the version that runs"
                    }
                ));
                if running.title != self.title {
                    out.push(format!(
                        "Title:    CHANGED from {}",
                        visible(&running.title)
                    ));
                }
            }
            None => {
                out.push(format!("Script:   {}", bytes(self.script_bytes)));
                out.push(format!("Brief:    {}", bytes(self.brief_bytes)));
            }
        }
        out.extend(self.types_lines());
        if self.hidden_characters > 0 {
            out.push(format!(
                "WARNING: {} character{} of the script or the brief {} not text, or change{} the direction text is shown in. Each is written out below as <U+XXXX>.",
                self.hidden_characters,
                if self.hidden_characters == 1 { "" } else { "s" },
                if self.hidden_characters == 1 { "is" } else { "are" },
                if self.hidden_characters == 1 { "s" } else { "" },
            ));
        }
        out.push(String::new());
        out.push("The brief:".into());
        for line in visible(&self.brief).lines() {
            out.push(format!("  | {line}"));
        }
        out.push(String::new());
        match (&self.running, &self.running_unreadable) {
            (_, Some(why)) => {
                out.push(format!(
                    "Nothing to compare with: {why}. Every line of the script is shown as new:"
                ));
            }
            (None, None) => {
                out.push(format!(
                    "No version runs yet: the whole script is new ({} lines):",
                    self.difference.added()
                ));
            }
            (Some(running), None) if self.script_unchanged() => {
                out.push(format!(
                    "The script is the one that runs (version {}): only what is said above differs.",
                    running.version
                ));
            }
            (Some(running), None) if self.difference.is_empty() => {
                out.push(format!(
                    "No line differs from version {}, which runs: only line endings do.",
                    running.version
                ));
            }
            (Some(running), None) => {
                out.push(format!(
                    "Difference from version {}, which runs: {} line{} added, {} removed:",
                    running.version,
                    self.difference.added(),
                    if self.difference.added() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    self.difference.removed()
                ));
            }
        }
        let difference = self.difference.unified(visible);
        if !difference.is_empty() {
            out.push(difference.trim_end_matches('\n').to_owned());
        }
        let mut text = out.join("\n");
        text.push('\n');
        text
    }
}

/// Counts what [`visible`] would write out in a script and a brief.
pub fn hidden_in(script: &[u8], brief: &str) -> usize {
    hidden_characters(&String::from_utf8_lossy(script)) + hidden_characters(brief)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_change_of_types_is_found_whatever_the_order() {
        assert_eq!(
            TypesChange::between(None, &names(&["card"])),
            TypesChange::First
        );
        assert_eq!(
            TypesChange::between(Some(&names(&["a", "b"])), &names(&["b", "a"])),
            TypesChange::Unchanged
        );
        assert_eq!(
            TypesChange::between(Some(&names(&["card", "tag"])), &names(&["vote", "card"])),
            TypesChange::Changed {
                added: names(&["vote"]),
                removed: names(&["tag"]),
            }
        );
        assert!(
            TypesChange::between(Some(&names(&["card"])), &names(&["card", "vote"])).is_changed()
        );
        assert!(
            TypesChange::between(Some(&names(&["card", "vote"])), &names(&["card"])).is_changed()
        );
    }

    #[test]
    fn who_it_comes_from_is_said_in_words_and_cannot_draw_on_the_screen() {
        assert_eq!(from_whom(&Proposer::owner()), "you");
        let key = "d".repeat(64);
        assert_eq!(
            from_whom(&Proposer::agent(key.clone(), "Scout")),
            format!("your agent Scout ({key})")
        );
        assert_eq!(
            from_whom(&Proposer::member(key.clone(), "Ana\x1b[2J")),
            format!("Ana<U+001B>[2J, a person of the channel ({key})")
        );
        // An agent's version says who asked for it (plan D7).
        let asker = "e".repeat(64);
        let asked = Proposer::agent(key.clone(), "Scout").asked_by(asker.clone(), "Ana\u{202e}");
        assert_eq!(
            from_whom(&asked),
            format!("your agent Scout ({key}), asked by Ana<U+202E> ({asker})")
        );
        assert_eq!(
            from_whom_short(&asked),
            "your agent Scout, asked by Ana<U+202E>"
        );
    }
}
