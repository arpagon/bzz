//! What each harness can and cannot do, and who enforces each rule (plan
//! v0.15.0, design point 3 and rule 12). Printed when an agent is created and
//! by `bzz agent show`, so that an owner never has to guess what an agent on
//! a given harness can reach.
//!
//! The record for Pi Durable states what bzz's own host does and its tests
//! show (`docs/validation-v0.15.0.md`, M2 and M3). The records for Grok,
//! Codex and Claude Code state what was seen with the owner's installed
//! programs in the M6 groundwork (2026-11-14, Linux); bzz does not start
//! those harnesses yet.

use crate::config::Harness;

/// What v0.15.0 promises for a harness (D16).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Promise {
    /// Everything in the plan.
    Full,
    Supported,
    /// Behind a tested version range; may fail in ways bzz then reports.
    Experimental,
}

impl Promise {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "everything in v0.15.0",
            Self::Supported => "supported",
            Self::Experimental => "experimental",
        }
    }
}

/// Who keeps a rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Enforcer {
    /// bzz, where it delivers a mention or serves a tool. The harness cannot
    /// get around it.
    Bzz,
    /// The harness's own sandbox. bzz switches it on and does not check it.
    Harness,
    /// Only bzz's answers to the harness's permission requests: the harness
    /// has no boundary of its own.
    BzzOnly,
    /// Nobody: the limit does not exist for this harness.
    Nobody,
}

impl Enforcer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bzz => "bzz",
            Self::Harness => "the harness",
            Self::BzzOnly => "bzz only",
            Self::Nobody => "nobody",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rule {
    pub rule: &'static str,
    pub by: Enforcer,
    pub how: &'static str,
}

/// One harness's capability record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityRecord {
    pub harness: Harness,
    pub promise: Promise,
    /// Whether this bzz can create and run an agent on it.
    pub available: bool,
    /// What a restart of bzz or of the harness keeps.
    pub survives_restart: &'static str,
    pub own_disk: (bool, &'static str),
    pub git_without_a_process: (bool, &'static str),
    pub real_folders: &'static str,
    pub move_with_memory: (bool, &'static str),
    /// What runs on the machine for this agent, and as whom.
    pub runs: &'static str,
    /// Its own sandbox on this platform, or `None`.
    pub sandbox: Option<&'static str>,
    pub rules: Vec<Rule>,
    /// D18: shown where the harness has no sandbox of its own.
    pub warning: Option<&'static str>,
}

/// D19, word for word what bzz says of a coding harness.
pub const CODING_HARNESS_RUNS: &str = "a real shell as your user, in a folder bzz makes plus the folders you grant, with the harness's own sandbox on where it has one. bzz never gives it the key; bzz cannot promise the key is out of reach";

/// D18: the short warning for a harness with no sandbox of its own, where
/// bzz's answers to its permission requests are the only boundary.
pub const NO_SANDBOX_WARNING: &str = "Warning: this harness has no sandbox of its own here. It runs as your user, and only bzz's answers to its permission requests keep it in its folders.";

/// D18, where the harness has no sandbox and never asks either.
pub const NO_BOUNDARY_WARNING: &str = "Warning: this harness has no sandbox of its own here and never asks before it acts. It runs as your user, and nothing keeps it in its folders.";

const ONE_REPLY: Rule = Rule {
    rule: "one final reply per mention",
    by: Enforcer::Bzz,
    how: "the reply is signed and stored under the mention before it is published",
};
const WHO_MAY_WAKE: Rule = Rule {
    rule: "who may wake it",
    by: Enforcer::Bzz,
    how: "checked before anything reaches the harness",
};
const ONLY_THROUGH_TOOLS: Rule = Rule {
    rule: "it acts on the chat only through bzz's tools",
    by: Enforcer::Bzz,
    how: "it is given no key and no relay connection",
};

/// The record of `harness` on this platform.
pub fn record(harness: Harness) -> CapabilityRecord {
    record_on(harness, cfg!(windows))
}

/// The record of `harness`; `windows` says whether the platform is Windows,
/// where Grok has no sandbox.
pub fn record_on(harness: Harness, windows: bool) -> CapabilityRecord {
    let coding =
        |promise, sandbox: Option<&'static str>, folders: Rule, commands: Rule| CapabilityRecord {
            harness,
            promise,
            available: false,
            survives_restart: "its own saved session for each conversation, loaded again by its id",
            own_disk: (
                false,
                "it works in a folder bzz makes for it on this machine",
            ),
            git_without_a_process: (false, "it runs the real git, as your user"),
            real_folders: "its working folder, and the folders you grant",
            move_with_memory: (
                false,
                "its memory belongs to its own program and stays on this machine",
            ),
            runs: CODING_HARNESS_RUNS,
            sandbox,
            rules: vec![
                ONE_REPLY,
                WHO_MAY_WAKE,
                ONLY_THROUGH_TOOLS,
                folders,
                commands,
            ],
            warning: match (sandbox, folders.by) {
                (Some(_), _) => None,
                (None, Enforcer::Nobody) => Some(NO_BOUNDARY_WARNING),
                (None, _) => Some(NO_SANDBOX_WARNING),
            },
        };
    match harness {
        Harness::PiDurable => CapabilityRecord {
            harness,
            promise: Promise::Full,
            available: true,
            survives_restart: "its conversations, a turn that was cut off, and its disk",
            own_disk: (true, "one SQLite file, reached only through its host"),
            git_without_a_process: (
                true,
                "the everyday commands, over HTTPS; it names what it lacks",
            ),
            real_folders: "only the folders you grant, under /mnt/<name>, read-only unless you allow writes",
            move_with_memory: (
                true,
                "`bzz agent move` seals its key, its memory and its disk in one file for another bzz, and `bzz agent launch-kit` writes a folder for a machine with Buzz's buzz-acp; granted folders and git credentials stay on this machine",
            ),
            runs: "no program of this machine: its shell and its git run inside its own host process",
            sandbox: Some("its own disk, shell and git, with a list of hosts it may reach"),
            rules: vec![
                ONE_REPLY,
                WHO_MAY_WAKE,
                ONLY_THROUGH_TOOLS,
                Rule {
                    rule: "it stays in its disk and its granted folders",
                    by: Enforcer::Harness,
                    how: "bzz's own host resolves every path and starts no program",
                },
                Rule {
                    rule: "it reaches only the hosts you allow",
                    by: Enforcer::Harness,
                    how: "bzz's own host checks every request and every redirect",
                },
                Rule {
                    rule: "real commands on this machine",
                    by: Enforcer::Harness,
                    how: "there are none: nothing of the machine is started for it",
                },
            ],
            warning: None,
        },
        Harness::Grok => {
            let sandbox = (!windows).then_some("Grok's own sandbox, profile `workspace`");
            coding(
                Promise::Supported,
                sandbox,
                Rule {
                    rule: "it writes only in its folders",
                    by: if windows {
                        Enforcer::Nobody
                    } else {
                        Enforcer::Harness
                    },
                    how: if windows {
                        "Grok has no sandbox on Windows"
                    } else {
                        "Grok's sandbox refuses a write outside; Grok never asks, so there is nothing for bzz to answer"
                    },
                },
                Rule {
                    rule: "real commands on this machine",
                    by: if windows {
                        Enforcer::Nobody
                    } else {
                        Enforcer::Harness
                    },
                    how: "they run as your user, inside the sandbox where there is one",
                },
            )
        }
        Harness::Codex => coding(
            Promise::Experimental,
            Some("Codex's own sandbox, mode `workspace-write`"),
            Rule {
                rule: "it writes only in its folders",
                by: Enforcer::Harness,
                how: "Codex's sandbox holds it, and bzz refuses its requests to leave",
            },
            Rule {
                rule: "real commands on this machine",
                by: Enforcer::Harness,
                how: "they run as your user, inside Codex's sandbox",
            },
        ),
        Harness::ClaudeCode => coding(
            Promise::Experimental,
            None,
            Rule {
                rule: "it writes only in its folders",
                by: Enforcer::BzzOnly,
                how: "bzz answers each permission request by the path it names; Claude Code has no boundary of its own",
            },
            Rule {
                rule: "real commands on this machine",
                by: Enforcer::BzzOnly,
                how: "bzz refuses a command unless you granted it; allowed, it runs as your user",
            },
        ),
    }
}

impl CapabilityRecord {
    /// The record as `bzz agent show` and creation print it.
    pub fn lines(&self) -> Vec<String> {
        let yes_no =
            |(has, how): (bool, &str)| format!("{}: {how}", if has { "yes" } else { "no" });
        let mut lines = vec![
            format!(
                "capability record: {} ({}{})",
                self.harness.as_str(),
                self.promise.as_str(),
                if self.available {
                    ""
                } else {
                    "; not available in this version yet"
                }
            ),
            format!("  survives a restart:  {}", self.survives_restart),
            format!("  its own disk:        {}", yes_no(self.own_disk)),
            format!(
                "  git with no process: {}",
                yes_no(self.git_without_a_process)
            ),
            format!("  real folders:        {}", self.real_folders),
            format!("  a move with memory:  {}", yes_no(self.move_with_memory)),
            format!("  what runs:           {}", self.runs),
            format!(
                "  its own sandbox:     {}",
                self.sandbox.unwrap_or("none that bzz can switch on here")
            ),
            "  rules, and who enforces each:".to_owned(),
        ];
        lines.extend(
            self.rules
                .iter()
                .map(|rule| format!("    {}: {} ({})", rule.rule, rule.by.as_str(), rule.how)),
        );
        if let Some(warning) = self.warning {
            lines.push(format!("  {warning}"));
        }
        lines
    }

    /// One line for a list or a panel.
    pub fn summary(&self) -> String {
        format!(
            "{} · own disk {} · sandbox {} · move with memory {}",
            self.promise.as_str(),
            if self.own_disk.0 { "yes" } else { "no" },
            if self.sandbox.is_some() {
                "yes"
            } else {
                "none"
            },
            if self.move_with_memory.0 { "yes" } else { "no" },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Harness; 4] = [
        Harness::PiDurable,
        Harness::Grok,
        Harness::Codex,
        Harness::ClaudeCode,
    ];

    #[test]
    fn every_harness_states_each_capability_and_who_enforces_each_rule() {
        for harness in ALL {
            for windows in [false, true] {
                let record = record_on(harness, windows);
                let text = record.lines().join("\n");
                for stated in [
                    "survives a restart",
                    "its own disk",
                    "git with no process",
                    "real folders",
                    "a move with memory",
                    "what runs",
                    "its own sandbox",
                    "one final reply per mention: bzz",
                    "who may wake it: bzz",
                    "only through bzz's tools: bzz",
                    "real commands on this machine",
                ] {
                    assert!(text.contains(stated), "{harness:?}: {stated}\n{text}");
                }
                // Rule 12: a capability a harness lacks is stated with what
                // takes its place, never left out.
                for (has, how) in [
                    record.own_disk,
                    record.git_without_a_process,
                    record.move_with_memory,
                ] {
                    assert!(!how.is_empty(), "{harness:?}: {has}");
                }
                // D18: the warning is there exactly where no sandbox is.
                assert_eq!(record.warning.is_some(), record.sandbox.is_none());
                assert_eq!(text.contains("Warning:"), record.sandbox.is_none());
            }
        }
    }

    #[test]
    fn the_records_say_what_the_tests_and_the_groundwork_showed() {
        let pi = record(Harness::PiDurable);
        assert!(pi.available && pi.own_disk.0 && pi.git_without_a_process.0);
        assert!(pi.move_with_memory.0 && pi.warning.is_none());
        // The move line names what M5 built, and what does not move.
        for said in [
            "`bzz agent move`",
            "one file",
            "`bzz agent launch-kit`",
            "buzz-acp",
            "granted folders and git credentials stay",
        ] {
            assert!(pi.move_with_memory.1.contains(said), "{said}");
        }
        // And bzz moves exactly the harnesses whose record says so.
        for harness in ALL {
            assert_eq!(
                crate::agents::moving::movable(harness).is_ok(),
                record(harness).move_with_memory.0,
                "{harness:?}"
            );
        }
        let refused = crate::agents::moving::movable(Harness::Codex)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("cannot be moved")
                && refused.contains(record(Harness::Codex).move_with_memory.1),
            "{refused}"
        );
        assert_eq!(pi.promise, Promise::Full);
        assert!(pi.runs.contains("no program of this machine"));
        // The three coding harnesses: not offered yet, no disk of their own,
        // no move with memory, and D19's wording.
        for harness in [Harness::Grok, Harness::Codex, Harness::ClaudeCode] {
            let record = record_on(harness, false);
            assert!(!record.available);
            assert!(!record.own_disk.0 && !record.git_without_a_process.0);
            assert!(!record.move_with_memory.0);
            assert_eq!(record.runs, CODING_HARNESS_RUNS);
            assert!(
                record
                    .lines()
                    .join("\n")
                    .contains("not available in this version yet")
            );
        }
        assert!(CODING_HARNESS_RUNS.contains("a real shell as your user"));
        assert!(CODING_HARNESS_RUNS.contains("bzz cannot promise the key is out of reach"));
        assert_eq!(record(Harness::Grok).promise, Promise::Supported);
        assert_eq!(record(Harness::Codex).promise, Promise::Experimental);
        assert_eq!(record(Harness::ClaudeCode).promise, Promise::Experimental);
        // Who holds the folder boundary, as the groundwork found it.
        let folders = |harness, windows| {
            record_on(harness, windows)
                .rules
                .iter()
                .find(|rule| rule.rule == "it writes only in its folders")
                .map(|rule| rule.by)
        };
        assert_eq!(folders(Harness::Grok, false), Some(Enforcer::Harness));
        assert_eq!(folders(Harness::Codex, false), Some(Enforcer::Harness));
        assert_eq!(folders(Harness::ClaudeCode, false), Some(Enforcer::BzzOnly));
        // D18: Claude Code everywhere, and Grok on Windows, have no sandbox
        // of their own.
        assert!(record_on(Harness::ClaudeCode, false).warning.is_some());
        assert!(record_on(Harness::ClaudeCode, true).warning.is_some());
        assert!(record_on(Harness::Grok, false).warning.is_none());
        assert_eq!(
            record_on(Harness::ClaudeCode, false).warning,
            Some(NO_SANDBOX_WARNING)
        );
        // Grok never asks, so on Windows nothing holds it: the warning says
        // that, and does not promise bzz's answers.
        assert_eq!(
            record_on(Harness::Grok, true).warning,
            Some(NO_BOUNDARY_WARNING)
        );
        assert_eq!(folders(Harness::Grok, true), Some(Enforcer::Nobody));
        assert!(record_on(Harness::Codex, true).warning.is_none());
    }
}
