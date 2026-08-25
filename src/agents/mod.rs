//! Verified, relay-only managed-agent interoperability.
//!
//! This module deliberately has no process, ACP, key-custody, memory, or
//! observer surface. A remote agent is only a set of signed public relay
//! records scoped later by community membership.

pub mod policy;
pub mod protocol;

use serde::{Deserialize, Serialize};

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
