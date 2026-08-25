use std::collections::BTreeSet;

use nostr::{Event, PublicKey};
use serde::Deserialize;

use super::{Presence, RespondTo, VerificationFailure, VerifiedPublicAgent};

const MAX_PROFILE_BYTES: usize = 64 * 1024;
const MAX_DECLARATION_BYTES: usize = 64 * 1024;
const MAX_POLICY_BYTES: usize = 64 * 1024;
const MAX_NAME_BYTES: usize = 256;
const MAX_CAPABILITIES: usize = 32;
const MAX_CAPABILITY_BYTES: usize = 128;
const MAX_ALLOWLIST: usize = 256;

#[derive(Debug, Default, Deserialize)]
struct PublicMetadata {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AgentDeclaration {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ManagedPolicy {
    name: String,
    #[serde(default)]
    respond_to: Option<RespondTo>,
    #[serde(default)]
    respond_to_allowlist: Vec<String>,
}

/// Verify the signed public records that identify a remote managed agent.
/// Membership and relay scoping are validated by the store/reconciliation
/// layer; this function is pure and community-independent.
pub fn verify_public_agent(
    profile: &Event,
    declaration: &Event,
    policy: Option<&Event>,
) -> Result<VerifiedPublicAgent, VerificationFailure> {
    if profile.kind.as_u16() != 0 || declaration.kind.as_u16() != 10_100 {
        return Err(VerificationFailure::WrongKind);
    }
    verify_event(profile)?;
    verify_event(declaration)?;
    if profile.pubkey != declaration.pubkey {
        return Err(VerificationFailure::InvalidDeclaration);
    }
    let agent_pubkey = profile.pubkey.to_hex();
    let owner_pubkey = verified_owner_pubkey(profile)?;
    let profile_metadata = parse_profile(profile)?;
    let declaration_metadata = parse_declaration(declaration)?;
    let capabilities = normalize_capabilities(declaration_metadata.capabilities)?;

    let (policy_name, respond_to, respond_to_allowlist, policy_event_id) =
        if let Some(policy) = policy {
            if policy.kind.as_u16() != 30_177 {
                return Err(VerificationFailure::WrongKind);
            }
            verify_event(policy)?;
            if policy.pubkey.to_hex() != owner_pubkey {
                return Err(VerificationFailure::WrongPolicyOwner);
            }
            let d_tags = tag_values(policy, "d");
            if d_tags.as_slice() != [agent_pubkey.as_str()] {
                return Err(VerificationFailure::WrongPolicyCoordinate);
            }
            let parsed = parse_policy(policy)?;
            (
                Some(parsed.name),
                parsed.respond_to,
                parsed.respond_to_allowlist,
                Some(policy.id.to_hex()),
            )
        } else {
            (None, None, Vec::new(), None)
        };

    let name = policy_name
        .or(declaration_metadata.display_name)
        .or(declaration_metadata.name)
        .or(profile_metadata.display_name)
        .or(profile_metadata.name)
        .and_then(normalize_name)
        .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&agent_pubkey));
    // Kind 10100 describes the agent but is not a freshness-bounded runtime
    // signal. v0.11 does not subscribe to ephemeral kind 20001 presence, so it
    // must not turn a cached declaration's `status` field into a readiness
    // claim.
    let presence = Presence::Unknown;

    Ok(VerifiedPublicAgent {
        pubkey: agent_pubkey,
        owner_pubkey,
        name,
        capabilities,
        presence,
        respond_to,
        respond_to_allowlist,
        profile_event_id: profile.id.to_hex(),
        declaration_event_id: declaration.id.to_hex(),
        policy_event_id,
        verified_at: nostr::Timestamp::now().as_secs(),
    })
}

fn verify_event(event: &Event) -> Result<(), VerificationFailure> {
    buzz_core::verify_event(event).map_err(|_| VerificationFailure::InvalidSignature)
}

pub fn verified_owner_pubkey(profile: &Event) -> Result<String, VerificationFailure> {
    let mut owners = BTreeSet::new();
    for tag in profile.tags.iter() {
        let values = tag.as_slice();
        if values.first().map(String::as_str) != Some("auth") || values.len() != 4 {
            continue;
        }
        let Ok(encoded) = serde_json::to_string(values) else {
            continue;
        };
        if let Ok(owner) = buzz_sdk::nip_oa::verify_auth_tag(&encoded, &profile.pubkey) {
            owners.insert(owner.to_hex());
        }
    }
    match owners.len() {
        0 => Err(VerificationFailure::MissingOwnership),
        1 => Ok(owners.into_iter().next().expect("one owner exists")),
        _ => Err(VerificationFailure::ConflictingOwner),
    }
}

fn parse_profile(profile: &Event) -> Result<PublicMetadata, VerificationFailure> {
    if profile.content.len() > MAX_PROFILE_BYTES {
        return Err(VerificationFailure::InvalidProfile);
    }
    serde_json::from_str(&profile.content).map_err(|_| VerificationFailure::InvalidProfile)
}

fn parse_declaration(declaration: &Event) -> Result<AgentDeclaration, VerificationFailure> {
    if declaration.content.len() > MAX_DECLARATION_BYTES {
        return Err(VerificationFailure::InvalidDeclaration);
    }
    serde_json::from_str(&declaration.content).map_err(|_| VerificationFailure::InvalidDeclaration)
}

fn parse_policy(policy: &Event) -> Result<ManagedPolicy, VerificationFailure> {
    if policy.content.len() > MAX_POLICY_BYTES {
        return Err(VerificationFailure::InvalidPolicy);
    }
    let mut parsed: ManagedPolicy =
        serde_json::from_str(&policy.content).map_err(|_| VerificationFailure::InvalidPolicy)?;
    if normalize_name(parsed.name.clone()).is_none() {
        return Err(VerificationFailure::InvalidPolicy);
    }
    if parsed.respond_to != Some(RespondTo::Allowlist) && !parsed.respond_to_allowlist.is_empty() {
        return Err(VerificationFailure::InvalidPolicy);
    }
    if parsed.respond_to == Some(RespondTo::Allowlist) && parsed.respond_to_allowlist.is_empty() {
        return Err(VerificationFailure::InvalidPolicy);
    }
    if parsed.respond_to_allowlist.len() > MAX_ALLOWLIST {
        return Err(VerificationFailure::InvalidPolicy);
    }
    let mut normalized = BTreeSet::new();
    for value in parsed.respond_to_allowlist {
        let pubkey =
            PublicKey::from_hex(value.trim()).map_err(|_| VerificationFailure::InvalidPolicy)?;
        normalized.insert(pubkey.to_hex());
    }
    parsed.respond_to_allowlist = normalized.into_iter().collect();
    Ok(parsed)
}

fn normalize_name(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_NAME_BYTES {
        return None;
    }
    Some(crate::render::sanitize::single_line(trimmed))
}

fn normalize_capabilities(values: Vec<String>) -> Result<Vec<String>, VerificationFailure> {
    if values.len() > MAX_CAPABILITIES {
        return Err(VerificationFailure::InvalidDeclaration);
    }
    let mut capabilities = BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_CAPABILITY_BYTES {
            return Err(VerificationFailure::InvalidDeclaration);
        }
        capabilities.insert(crate::render::sanitize::single_line(trimmed));
    }
    Ok(capabilities.into_iter().collect())
}

fn tag_values<'a>(event: &'a Event, name: &str) -> Vec<&'a str> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some(name))
                .then(|| values.get(1).map(String::as_str))
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use super::*;

    fn records(
        respond_to: Option<&str>,
        allowlist: &[String],
    ) -> (Event, Event, Option<Event>, Keys, Keys) {
        let owner = Keys::generate();
        let agent = Keys::generate();
        let auth = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap();
        let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
        let profile = EventBuilder::new(Kind::Metadata, "{}")
            .tags([Tag::parse(auth).unwrap()])
            .sign_with_keys(&agent)
            .unwrap();
        let declaration = EventBuilder::new(
            Kind::Custom(10_100),
            serde_json::json!({
                "display_name": "Relay Agent",
                "capabilities": ["messages", "search"],
                "status": "online"
            })
            .to_string(),
        )
        .sign_with_keys(&agent)
        .unwrap();
        let policy = respond_to.map(|mode| {
            EventBuilder::new(
                Kind::Custom(30_177),
                serde_json::json!({
                    "name": "Policy Agent",
                    "parallelism": 1,
                    "respond_to": mode,
                    "respond_to_allowlist": allowlist,
                })
                .to_string(),
            )
            .tags([Tag::parse(["d", &agent.public_key().to_hex()]).unwrap()])
            .sign_with_keys(&owner)
            .unwrap()
        });
        (profile, declaration, policy, owner, agent)
    }

    #[test]
    fn verifies_owner_policy_and_declaration() {
        let viewer = Keys::generate().public_key().to_hex();
        let (profile, declaration, policy, owner, agent) =
            records(Some("allowlist"), std::slice::from_ref(&viewer));
        let verified = verify_public_agent(&profile, &declaration, policy.as_ref()).unwrap();
        assert_eq!(verified.pubkey, agent.public_key().to_hex());
        assert_eq!(verified.owner_pubkey, owner.public_key().to_hex());
        assert_eq!(verified.name, "Policy Agent");
        assert_eq!(verified.presence, Presence::Unknown);
        assert_eq!(verified.respond_to, Some(RespondTo::Allowlist));
        assert_eq!(
            verified.eligibility(&viewer, false),
            super::super::Eligibility::Eligible
        );
    }

    #[test]
    fn accepts_verified_agent_without_public_policy_as_unknown() {
        let (profile, declaration, _, _, _) = records(None, &[]);
        let verified = verify_public_agent(&profile, &declaration, None).unwrap();
        assert_eq!(verified.respond_to, None);
        assert_eq!(
            verified.eligibility(&"b".repeat(64), false),
            super::super::Eligibility::PolicyUnknown
        );
    }

    #[test]
    fn rejects_policy_signed_by_someone_other_than_owner() {
        let (profile, declaration, policy, _, agent) = records(Some("anyone"), &[]);
        let attacker = Keys::generate();
        let forged = EventBuilder::new(Kind::Custom(30_177), policy.unwrap().content)
            .tags([Tag::parse(["d", &agent.public_key().to_hex()]).unwrap()])
            .sign_with_keys(&attacker)
            .unwrap();
        assert_eq!(
            verify_public_agent(&profile, &declaration, Some(&forged)),
            Err(VerificationFailure::WrongPolicyOwner)
        );
    }

    #[test]
    fn rejects_policy_for_another_coordinate() {
        let (profile, declaration, policy, owner, _) = records(Some("anyone"), &[]);
        let forged = EventBuilder::new(Kind::Custom(30_177), policy.unwrap().content)
            .tags([Tag::parse(["d", &Keys::generate().public_key().to_hex()]).unwrap()])
            .sign_with_keys(&owner)
            .unwrap();
        assert_eq!(
            verify_public_agent(&profile, &declaration, Some(&forged)),
            Err(VerificationFailure::WrongPolicyCoordinate)
        );
    }

    #[test]
    fn two_valid_owner_attestations_fail_as_a_conflict() {
        let (profile, declaration, _, owner, agent) = records(None, &[]);
        let second_owner = Keys::generate();
        let second =
            buzz_sdk::nip_oa::compute_auth_tag(&second_owner, &agent.public_key(), "").unwrap();
        let second: Vec<String> = serde_json::from_str(&second).unwrap();
        let first = profile
            .tags
            .iter()
            .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
            .unwrap()
            .clone();
        let conflicting = EventBuilder::new(Kind::Metadata, "{}")
            .tags([first, Tag::parse(second).unwrap()])
            .sign_with_keys(&agent)
            .unwrap();
        assert_ne!(owner.public_key(), second_owner.public_key());
        assert_eq!(
            verify_public_agent(&conflicting, &declaration, None),
            Err(VerificationFailure::ConflictingOwner)
        );
    }

    #[test]
    fn oversized_or_semantically_inconsistent_public_fields_fail_closed() {
        let viewer = Keys::generate().public_key().to_hex();
        let (profile, _, policy, _, agent) = records(Some("anyone"), &[viewer]);
        assert_eq!(
            verify_public_agent(
                &profile,
                &EventBuilder::new(
                    Kind::Custom(10_100),
                    serde_json::json!({"capabilities": vec!["x"; MAX_CAPABILITIES + 1]})
                        .to_string(),
                )
                .sign_with_keys(&agent)
                .unwrap(),
                policy.as_ref(),
            ),
            Err(VerificationFailure::InvalidDeclaration)
        );
        // An allowlist attached to `anyone` is ambiguous and cannot be used.
        let (profile, declaration, policy, _, _) = records(Some("anyone"), &["b".repeat(64)]);
        assert_eq!(
            verify_public_agent(&profile, &declaration, policy.as_ref()),
            Err(VerificationFailure::InvalidPolicy)
        );
    }

    #[test]
    fn dm_hardening_is_applied_to_verified_policy() {
        let viewer = Keys::generate().public_key().to_hex();
        let (profile, declaration, policy, _, _) = records(Some("anyone"), &[]);
        let verified = verify_public_agent(&profile, &declaration, policy.as_ref()).unwrap();
        assert_eq!(
            verified.eligibility(&viewer, true),
            super::super::Eligibility::Ineligible
        );
    }
}
