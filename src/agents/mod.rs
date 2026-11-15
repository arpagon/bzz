//! Agents as bzz sees them.
//!
//! [`policy`], [`protocol`] and [`typing`] are verified, relay-only
//! interoperability with agents that something else runs: a remote agent is
//! only a set of signed public relay records, scoped later by community
//! membership. [`runtime`] (v0.15) runs local agents: bzz holds the key and
//! starts the process; [`manage`] creates and manages them, and
//! [`capability`] says what each harness can do. [`moving`] moves one to
//! another bzz ([`bundle`] is the sealed file) or to a machine with Buzz's
//! `buzz-acp` ([`launch_kit`]); [`remote`] reads a moved agent's presence
//! and stops it.

pub mod activity;
pub mod bundle;
pub mod capability;
pub mod git_credentials;
pub mod launch_kit;
pub mod manage;
pub mod moving;
pub mod policy;
pub mod protocol;
pub mod remote;
pub mod runtime;
pub mod typing;

use serde::{Deserialize, Serialize};

/// How this bzz stands to an agent of the directory whose key it holds
/// itself. The directory is built from public relay records, and an
/// owner's own agents are in it like any other: they are his all the same,
/// and bzz says so instead of calling them remote.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Own {
    /// A local agent: this bzz holds its key and starts its process.
    RunsHere,
    /// A local agent that is paused here.
    Paused,
    /// A local agent that was moved away: it runs elsewhere, and this bzz
    /// still holds its sealed key.
    MovedAway,
    /// A v0.13 hosted agent: bzz holds its key, its owner runs its process.
    Hosted,
}

impl Own {
    /// A few words for a list.
    pub const fn label(self) -> &'static str {
        match self {
            Self::RunsHere => "yours · run by this bzz",
            Self::Paused => "yours · paused here",
            Self::MovedAway => "yours · moved away",
            Self::Hosted => "yours · you run its process",
        }
    }

    /// What bzz can do with it, for the directory's detail pane.
    pub const fn says(self) -> [&'static str; 2] {
        match self {
            Self::RunsHere => [
                "This is your own agent: this bzz holds its key and runs it.",
                "Tab shows it among your agents: its state, its log, pause and resume.",
            ],
            Self::Paused => [
                "This is your own agent: this bzz holds its key, and it is paused here.",
                "Tab shows it among your agents, where Space resumes it.",
            ],
            Self::MovedAway => [
                "This is your own agent, moved away: it runs elsewhere, and this bzz does not start it.",
                "`bzz agent show` says where it went; `bzz agent receive` takes it back.",
            ],
            Self::Hosted => [
                "This is your own agent: bzz holds its key for a process you run yourself.",
                "`:broker` shows it.",
            ],
        }
    }
}

/// How this bzz stands to the agent `pubkey` of `community`, when it holds
/// the agent's key. `None`: the agent is someone else's, or runs somewhere
/// this bzz knows nothing of.
pub fn own(
    agents: &[crate::config::HostedAgentConfig],
    community: uuid::Uuid,
    pubkey: &str,
) -> Option<Own> {
    use crate::config::LocalAgentState;
    let agent = agents
        .iter()
        .find(|agent| agent.community == community && agent.pubkey.eq_ignore_ascii_case(pubkey))?;
    Some(match agent.local.as_ref().map(|local| local.state) {
        None => Own::Hosted,
        Some(LocalAgentState::MovedAway) => Own::MovedAway,
        Some(LocalAgentState::Paused) => Own::Paused,
        Some(LocalAgentState::Active) if agent.paused => Own::Paused,
        Some(LocalAgentState::Active) => Own::RunsHere,
    })
}

/// Every agent of `community` this bzz holds the key of, by its key.
pub fn own_agents(
    agents: &[crate::config::HostedAgentConfig],
    community: uuid::Uuid,
) -> std::collections::HashMap<String, Own> {
    agents
        .iter()
        .filter(|agent| agent.community == community)
        .filter_map(|agent| {
            own(agents, community, &agent.pubkey)
                .map(|standing| (agent.pubkey.to_ascii_lowercase(), standing))
        })
        .collect()
}

/// Public inbound audience declared by the verified agent owner.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RespondTo {
    OwnerOnly,
    Allowlist,
    Anyone,
}

impl RespondTo {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OwnerOnly => "owner-only",
            Self::Allowlist => "allowlist",
            Self::Anyone => "anyone",
        }
    }
}

/// Whether the active human identity appears able to invoke the remote agent.
/// This is a local projection of public policy, not proof that a runtime is
/// online or will answer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Eligibility {
    Eligible,
    Ineligible,
    PolicyUnknown,
}

impl Eligibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Ineligible => "ineligible",
            Self::PolicyUnknown => "policy-unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Online,
    Away,
    Offline,
    Unknown,
}

impl Presence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Away => "away",
            Self::Offline => "offline",
            Self::Unknown => "unknown",
        }
    }
}

/// A cryptographically verified public agent projection. Community membership
/// is intentionally not part of this pure protocol type.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifiedPublicAgent {
    pub pubkey: String,
    pub owner_pubkey: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub presence: Presence,
    pub respond_to: Option<RespondTo>,
    pub respond_to_allowlist: Vec<String>,
    pub profile_event_id: String,
    pub declaration_event_id: Option<String>,
    pub policy_event_id: Option<String>,
    pub verified_at: u64,
}

impl VerifiedPublicAgent {
    pub fn eligibility(&self, active_pubkey: &str, is_dm: bool) -> Eligibility {
        policy::evaluate(
            self.respond_to,
            &self.respond_to_allowlist,
            &self.owner_pubkey,
            active_pubkey,
            is_dm,
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationFailure {
    WrongKind,
    InvalidSignature,
    InvalidProfile,
    MissingOwnership,
    ConflictingOwner,
    InvalidDeclaration,
    InvalidPolicy,
    WrongPolicyOwner,
    WrongPolicyCoordinate,
}

impl VerificationFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongKind => "wrong-kind",
            Self::InvalidSignature => "invalid-signature",
            Self::InvalidProfile => "invalid-profile",
            Self::MissingOwnership => "missing-ownership",
            Self::ConflictingOwner => "conflicting-owner",
            Self::InvalidDeclaration => "invalid-declaration",
            Self::InvalidPolicy => "invalid-policy",
            Self::WrongPolicyOwner => "wrong-policy-owner",
            Self::WrongPolicyCoordinate => "wrong-policy-coordinate",
        }
    }
}

#[cfg(test)]
mod own_tests {
    use super::*;
    use crate::config::{Harness, HostedAgentConfig, LocalAgentConfig, LocalAgentState};
    use uuid::Uuid;

    /// From the run against Buzz 1.0: `bzz agents list` and the directory
    /// called the owner's own local agents "remote" agents that bzz "cannot
    /// start". An agent this bzz holds the key of is its owner's own.
    #[test]
    fn an_agent_this_bzz_holds_the_key_of_is_its_owners_own() {
        let community = Uuid::new_v4();
        let agent =
            |pubkey: &str, state: Option<LocalAgentState>, paused: bool| HostedAgentConfig {
                id: Uuid::new_v4(),
                label: pubkey[..1].to_owned(),
                pubkey: pubkey.to_owned(),
                community,
                owner_identity: Uuid::nil(),
                auth_tag: "[]".into(),
                credential_hash: "c".repeat(64),
                allowed: Vec::new(),
                paused,
                local: state.map(|state| LocalAgentConfig {
                    state,
                    ..LocalAgentConfig::new(Harness::PiDurable)
                }),
            };
        let agents = [
            agent(&"a".repeat(64), Some(LocalAgentState::Active), false),
            agent(&"b".repeat(64), Some(LocalAgentState::Paused), false),
            agent(&"c".repeat(64), Some(LocalAgentState::Active), true),
            agent(&"d".repeat(64), Some(LocalAgentState::MovedAway), false),
            agent(&"e".repeat(64), None, false),
        ];
        let standing = |pubkey: &str| own(&agents, community, pubkey);
        assert_eq!(standing(&"a".repeat(64)), Some(Own::RunsHere));
        assert_eq!(standing(&"A".repeat(64)), Some(Own::RunsHere));
        assert_eq!(standing(&"b".repeat(64)), Some(Own::Paused));
        assert_eq!(standing(&"c".repeat(64)), Some(Own::Paused));
        assert_eq!(standing(&"d".repeat(64)), Some(Own::MovedAway));
        assert_eq!(standing(&"e".repeat(64)), Some(Own::Hosted));
        // Someone else's agent, and the same key in another community.
        assert_eq!(standing(&"f".repeat(64)), None);
        assert_eq!(own(&agents, Uuid::new_v4(), &"a".repeat(64)), None);
        assert_eq!(own_agents(&agents, community).len(), 5);
        assert!(own_agents(&agents, Uuid::new_v4()).is_empty());
        for standing in [Own::RunsHere, Own::Paused, Own::MovedAway, Own::Hosted] {
            assert!(standing.label().starts_with("yours"));
            assert!(standing.says()[0].contains("your own agent"));
        }
    }
}
