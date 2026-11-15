use nostr::{Event, JsonUtil as _, PublicKey};
use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    agents::{
        Eligibility, Presence, RespondTo, VerifiedPublicAgent, protocol::verify_public_agent,
    },
    error::{Error, Result},
    protocol::events::tag_values,
    store::Store,
};

const MAX_OWNER_POLICIES: usize = 512;
const AGENT_FRESHNESS_SECONDS: u64 = 10 * 60;

/// A membership in which a verified agent is listed, with `c` as its
/// channel: any channel that is not a DM, whatever the member's role, or a
/// small DM the viewer is in. Since v0.15 the `bot` role is not required:
/// Buzz 1.0 adds an agent to a channel as a plain member, and so does bzz
/// (ADR v0.15 local agents, decision 11). What makes a member an agent is
/// its owner's proof, checked by [`Store::reconcile_remote_agent`].
pub(crate) const AGENT_MEMBERSHIP: &str =
    "(c.channel_type!='dm' OR (c.is_member=1 AND c.member_count BETWEEN 2 AND 9))";

/// A member, with `m` as its membership, that has a stored profile carrying
/// an owner's authorization. Only a first look: the proof itself is
/// verified in Rust.
const PROFILE_CLAIMS_AN_OWNER: &str = "EXISTS(SELECT 1 FROM events p
       WHERE p.community_id=m.community_id AND p.kind=0 AND p.pubkey=m.pubkey
         AND p.deleted_by_event_id IS NULL AND p.tags_json LIKE '%\"auth\"%')";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteAgentView {
    pub schema_version: u32,
    pub community_id: Uuid,
    pub pubkey: String,
    pub owner_pubkey: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub presence: Presence,
    pub respond_to: Option<RespondTo>,
    pub respond_to_allowlist: Vec<String>,
    pub eligibility: Eligibility,
    pub stale: bool,
    pub channel_ids: Vec<Uuid>,
    pub last_verified_at: u64,
}

impl Store {
    /// Rebuild one community-scoped remote-agent projection from already
    /// verified event storage. Returns whether durable state changed.
    pub fn reconcile_remote_agent(
        &mut self,
        community_id: Uuid,
        agent_pubkey: &str,
    ) -> Result<bool> {
        let agent_pubkey = PublicKey::from_hex(agent_pubkey)
            .map_err(|_| Error::Protocol("agent candidate has an invalid pubkey".into()))?
            .to_hex();
        let (is_bot, is_channel_member, is_declared_dm_participant): (bool, bool, bool) =
            self.connection.query_row(
                "SELECT
               EXISTS(SELECT 1 FROM memberships
                 WHERE community_id=?1 AND pubkey=?2 AND role='bot'),
               EXISTS(SELECT 1 FROM memberships m
                 JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
                 WHERE m.community_id=?1 AND m.pubkey=?2 AND c.channel_type!='dm'),
               EXISTS(SELECT 1 FROM memberships m
                 JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
                 WHERE m.community_id=?1 AND m.pubkey=?2 AND c.channel_type='dm'
                   AND c.is_member=1 AND c.member_count BETWEEN 2 AND 9
                   AND EXISTS(SELECT 1 FROM events d WHERE d.community_id=m.community_id
                     AND d.kind=10100 AND d.pubkey=m.pubkey
                     AND d.deleted_by_event_id IS NULL))",
                params![community_id.to_string(), agent_pubkey],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let profile = self.latest_agent_event(community_id, 0, &agent_pubkey)?;
        // D13: a plain member of a channel is an agent when its profile
        // carries its owner's valid proof. A person's profile carries none,
        // so people never enter the directory, not even as failures. Asked
        // only of a member that is no bot: a bot is a candidate already, and
        // its proof is verified below in any case.
        let is_proven_member = !is_bot
            && is_channel_member
            && profile.as_ref().is_some_and(|profile| {
                crate::agents::protocol::verified_owner_pubkey(profile).is_ok()
            });
        if !is_bot && !is_proven_member && !is_declared_dm_participant {
            return Ok(self.connection.execute(
                "UPDATE remote_agents SET verification_state='removed',failure_reason='not-a-current-candidate',updated_at=unixepoch() WHERE community_id=?1 AND agent_pubkey=?2 AND verification_state!='removed'",
                params![community_id.to_string(), agent_pubkey],
            )? > 0);
        }

        let declaration = self.latest_agent_event(community_id, 10_100, &agent_pubkey)?;
        let Some(profile) = profile else {
            return self.upsert_agent_failure(
                community_id,
                &agent_pubkey,
                "incomplete",
                "required-public-record-missing",
            );
        };
        if !is_bot && !is_proven_member && declaration.is_none() {
            return self.upsert_agent_failure(
                community_id,
                &agent_pubkey,
                "incomplete",
                "required-public-record-missing",
            );
        }

        // Relay bot membership, or membership of a channel with the owner's
        // proof, establishes the agent class even when a Buzz-managed
        // identity has no kind 10100. NIP-OA still proves ownership; a
        // DM-only operational member must have the declaration.
        let without_policy = match verify_public_agent(&profile, declaration.as_ref(), None) {
            Ok(agent) => agent,
            Err(failure) => {
                return self.upsert_agent_failure(
                    community_id,
                    &agent_pubkey,
                    "invalid",
                    failure.as_str(),
                );
            }
        };
        let policy =
            self.latest_agent_policy(community_id, &without_policy.owner_pubkey, &agent_pubkey)?;
        match verify_public_agent(&profile, declaration.as_ref(), policy.as_ref()) {
            Ok(agent) => self.upsert_verified_agent(community_id, &agent),
            Err(failure) => {
                self.upsert_agent_failure(community_id, &agent_pubkey, "invalid", failure.as_str())
            }
        }
    }

    pub(crate) fn reconcile_all_remote_agents(&mut self) -> Result<usize> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM communities ORDER BY id LIMIT 1000")?;
        let communities = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let mut changed = 0;
        for community in communities {
            let community_id = Uuid::parse_str(&community)
                .map_err(|_| Error::Database(rusqlite::Error::InvalidQuery))?;
            changed += self.reconcile_remote_agents(community_id)?;
        }
        Ok(changed)
    }

    /// Members whose class the relay or a declaration can establish: every
    /// `bot` member, and every participant of a small DM the viewer is in.
    pub fn remote_agent_candidate_pubkeys(&self, community_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT m.pubkey FROM memberships m
             JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1
               AND (m.role='bot' OR (c.channel_type='dm' AND c.is_member=1
                    AND c.member_count BETWEEN 2 AND 9))
             ORDER BY m.pubkey LIMIT 5000",
        )?;
        statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn remote_agent_bot_pubkeys(&self, community_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT pubkey FROM memberships
             WHERE community_id=?1 AND role='bot' ORDER BY pubkey LIMIT 5000",
        )?;
        statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// D13: plain members of channels whose profile may prove an owner, for
    /// a refresh to look at: those whose stored profile carries an owner's
    /// authorization, and those with no stored profile yet. A member whose
    /// stored profile carries none is a person and is not asked about again
    /// here. Bounded, like every candidate source.
    pub fn remote_agent_member_pubkeys(&self, community_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT DISTINCT m.pubkey FROM memberships m
             JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1 AND c.channel_type!='dm' AND m.role!='bot'
               AND ({PROFILE_CLAIMS_AN_OWNER}
                    OR NOT EXISTS(SELECT 1 FROM events p
                      WHERE p.community_id=m.community_id AND p.kind=0 AND p.pubkey=m.pubkey
                        AND p.deleted_by_event_id IS NULL))
             ORDER BY m.pubkey LIMIT 2000"
        ))?;
        statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Reconcile every currently known candidate plus projections whose
    /// membership was removed: `bot` members, declared participants of small
    /// DMs, and (D13) channel members whose profile claims an owner. The
    /// bounded membership source prevents a hostile global profile stream
    /// from creating unbounded candidates.
    pub fn reconcile_remote_agents(&mut self, community_id: Uuid) -> Result<usize> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT m.pubkey FROM memberships m
             JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1
               AND (m.role='bot'
                    OR (c.channel_type!='dm' AND {PROFILE_CLAIMS_AN_OWNER})
                    OR (c.channel_type='dm' AND c.is_member=1
                    AND c.member_count BETWEEN 2 AND 9 AND EXISTS(
                      SELECT 1 FROM events d WHERE d.community_id=m.community_id
                        AND d.kind=10100 AND d.pubkey=m.pubkey
                        AND d.deleted_by_event_id IS NULL)))
             UNION SELECT agent_pubkey FROM remote_agents WHERE community_id=?1
             ORDER BY 1 LIMIT 5000"
        ))?;
        let pubkeys = statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let mut changed = 0;
        for pubkey in pubkeys {
            changed += usize::from(self.reconcile_remote_agent(community_id, &pubkey)?);
        }
        Ok(changed)
    }

    pub fn list_remote_agents(
        &self,
        community_id: Uuid,
        active_pubkey: &str,
    ) -> Result<Vec<RemoteAgentView>> {
        self.verified_agents(community_id, active_pubkey, None)
    }

    /// The verified agents of a community, or only `only`. One agent is
    /// asked for when a mention of it is checked before a send: listing
    /// every agent for that made the check as slow as the whole directory.
    fn verified_agents(
        &self,
        community_id: Uuid,
        active_pubkey: &str,
        only: Option<&str>,
    ) -> Result<Vec<RemoteAgentView>> {
        PublicKey::from_hex(active_pubkey)
            .map_err(|_| Error::Config("active identity has an invalid pubkey".into()))?;
        let mut statement = self.connection.prepare(&format!(
            "SELECT agent_pubkey,owner_pubkey,name,capabilities_json,presence,respond_to,
                    respond_to_allowlist_json,last_verified_at
             FROM remote_agents a
             WHERE community_id=?1 AND verification_state='verified'
               AND (?2 IS NULL OR agent_pubkey=?2)
               AND EXISTS(SELECT 1 FROM memberships m
                 JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
                 WHERE m.community_id=a.community_id AND m.pubkey=a.agent_pubkey
                   AND {AGENT_MEMBERSHIP})
             ORDER BY name COLLATE NOCASE,agent_pubkey LIMIT 5000"
        ))?;
        let only = only.map(str::to_ascii_lowercase);
        let rows = statement.query_map(params![community_id.to_string(), only], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        let mut agents = Vec::new();
        for row in rows {
            let (
                pubkey,
                owner_pubkey,
                name,
                capabilities,
                presence,
                respond_to,
                allowlist,
                verified_at,
            ) = row?;
            let capabilities = serde_json::from_str(&capabilities)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let allowlist: Vec<String> = serde_json::from_str(&allowlist)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let respond_to = respond_to.as_deref().map(parse_respond_to).transpose()?;
            let presence = parse_presence(&presence)?;
            let channel_ids = self.remote_agent_channels(community_id, &pubkey)?;
            let last_verified_at = u64::try_from(verified_at).unwrap_or(0);
            let stale = nostr::Timestamp::now()
                .as_secs()
                .saturating_sub(last_verified_at)
                > AGENT_FRESHNESS_SECONDS;
            let eligibility = if stale {
                Eligibility::Ineligible
            } else {
                crate::agents::policy::evaluate(
                    respond_to,
                    &allowlist,
                    &owner_pubkey,
                    active_pubkey,
                    false,
                )
            };
            agents.push(RemoteAgentView {
                schema_version: 1,
                community_id,
                pubkey,
                owner_pubkey,
                name,
                capabilities,
                presence,
                respond_to,
                respond_to_allowlist: allowlist,
                eligibility,
                stale,
                channel_ids,
                last_verified_at,
            });
        }
        Ok(agents)
    }

    pub fn agent_mentions_need_validation(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        mentions: &[String],
    ) -> Result<usize> {
        if mentions.len() > crate::ui::composer::MENTION_CAP {
            return Err(Error::Protocol("message has too many mentions".into()));
        }
        let mut count = 0;
        for mention in mentions {
            let mention = PublicKey::from_hex(mention)
                .map_err(|_| Error::Protocol("message mention has an invalid pubkey".into()))?
                .to_hex();
            // A `bot` member is an agent by the relay's word; anyone else
            // only once the directory has a projection of it.
            let is_agent_candidate: bool = self.connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3 AND role='bot'
                    UNION ALL
                    SELECT 1 FROM remote_agents WHERE community_id=?1 AND agent_pubkey=?3
                 )",
                params![community_id.to_string(),channel_id.to_string(),mention],
                |row| row.get(0),
            )?;
            count += usize::from(is_agent_candidate);
        }
        Ok(count)
    }

    pub fn validate_agent_mentions(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        active_pubkey: &str,
        mentions: &[String],
    ) -> Result<()> {
        let channel_type = self
            .connection
            .query_row(
                "SELECT channel_type FROM channels WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let is_dm = channel_type.as_deref() == Some("dm");
        for mention in mentions {
            let mention = PublicKey::from_hex(mention)
                .map_err(|_| Error::Protocol("message mention has an invalid pubkey".into()))?
                .to_hex();
            let (is_agent_candidate, is_participant): (bool, bool) = self.connection.query_row(
                "SELECT
                    EXISTS(SELECT 1 FROM remote_agents WHERE community_id=?1 AND agent_pubkey=?3)
                      OR EXISTS(SELECT 1 FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3 AND role='bot'),
                    EXISTS(SELECT 1 FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3)",
                params![community_id.to_string(),channel_id.to_string(),mention],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if !is_agent_candidate {
                continue;
            }
            // A member of the channel, whatever its role (D13); whether it
            // is a verified agent there is checked below.
            if !is_participant {
                return Err(Error::Access(if is_dm {
                    "remote agent mention is no longer a current DM participant".into()
                } else {
                    "remote agent mention is no longer a member of this channel".into()
                }));
            }
            let agent = self
                .remote_agent(community_id, &mention, active_pubkey, Some(channel_id))?
                .ok_or_else(|| {
                    Error::Access(
                        "remote agent mention is not currently verified for this channel".into(),
                    )
                })?;
            if is_dm && !agent.owner_pubkey.eq_ignore_ascii_case(active_pubkey) {
                return Err(Error::Access(
                    "only the verified owner may invoke a remote agent in a DM".into(),
                ));
            }
            match agent.eligibility {
                Eligibility::Eligible => {}
                Eligibility::Ineligible => return Err(Error::Access(
                    "the verified remote agent's public policy does not allow this identity to invoke it".into(),
                )),
                Eligibility::PolicyUnknown => return Err(Error::Access(
                    "the verified remote agent has no usable public invocation policy".into(),
                )),
            }
        }
        Ok(())
    }

    pub fn remote_agent(
        &self,
        community_id: Uuid,
        agent_pubkey: &str,
        active_pubkey: &str,
        channel_id: Option<Uuid>,
    ) -> Result<Option<RemoteAgentView>> {
        let mut agents = self.verified_agents(community_id, active_pubkey, Some(agent_pubkey))?;
        let Some(mut agent) = agents
            .drain(..)
            .find(|agent| agent.pubkey.eq_ignore_ascii_case(agent_pubkey))
        else {
            return Ok(None);
        };
        if let Some(channel_id) = channel_id {
            if !agent.channel_ids.contains(&channel_id) {
                return Ok(None);
            }
            let is_dm = self
                .connection
                .query_row(
                    "SELECT channel_type FROM channels WHERE community_id=?1 AND channel_id=?2",
                    params![community_id.to_string(), channel_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .is_none_or(|kind| kind == "dm");
            if !agent.stale {
                agent.eligibility = crate::agents::policy::evaluate(
                    agent.respond_to,
                    &agent.respond_to_allowlist,
                    &agent.owner_pubkey,
                    active_pubkey,
                    is_dm,
                );
            }
        }
        Ok(Some(agent))
    }

    fn latest_agent_event(
        &self,
        community_id: Uuid,
        kind: u16,
        author: &str,
    ) -> Result<Option<Event>> {
        let raw = self
            .connection
            .query_row(
                "SELECT raw_json FROM events WHERE community_id=?1 AND kind=?2 AND pubkey=?3
               AND deleted_by_event_id IS NULL
             ORDER BY created_at DESC,event_id ASC LIMIT 1",
                params![community_id.to_string(), i64::from(kind), author],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| Event::from_json(raw).map_err(|error| Error::Protocol(error.to_string())))
            .transpose()
    }

    fn latest_agent_policy(
        &self,
        community_id: Uuid,
        owner_pubkey: &str,
        agent_pubkey: &str,
    ) -> Result<Option<Event>> {
        let mut statement = self.connection.prepare(
            "SELECT raw_json FROM events WHERE community_id=?1 AND kind=30177 AND pubkey=?2
               AND deleted_by_event_id IS NULL
             ORDER BY created_at DESC,event_id ASC LIMIT ?3",
        )?;
        let raws = statement
            .query_map(
                params![
                    community_id.to_string(),
                    owner_pubkey,
                    i64::try_from(MAX_OWNER_POLICIES).unwrap_or(512)
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for raw in raws {
            let event =
                Event::from_json(raw).map_err(|error| Error::Protocol(error.to_string()))?;
            if tag_values(&event, "d").as_slice() == [agent_pubkey] {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    fn remote_agent_channels(&self, community_id: Uuid, agent_pubkey: &str) -> Result<Vec<Uuid>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT m.channel_id FROM memberships m
             JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1 AND m.pubkey=?2 AND {AGENT_MEMBERSHIP}
             ORDER BY m.channel_id LIMIT 5000"
        ))?;
        statement
            .query_map(params![community_id.to_string(), agent_pubkey], |row| {
                row.get::<_, String>(0)
            })?
            .filter_map(|row| match row {
                Ok(value) => Uuid::parse_str(&value).ok().map(Ok),
                Err(error) => Some(Err(error.into())),
            })
            .collect()
    }

    fn upsert_verified_agent(
        &mut self,
        community_id: Uuid,
        agent: &VerifiedPublicAgent,
    ) -> Result<bool> {
        let capabilities = serde_json::to_string(&agent.capabilities)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let allowlist = serde_json::to_string(&agent.respond_to_allowlist)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let changed = self.connection.execute(
            "INSERT INTO remote_agents(
                community_id,agent_pubkey,owner_pubkey,name,capabilities_json,presence,respond_to,
                respond_to_allowlist_json,verification_state,failure_reason,profile_event_id,
                declaration_event_id,policy_event_id,last_verified_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'verified',NULL,?9,?10,?11,?12,unixepoch())
             ON CONFLICT(community_id,agent_pubkey) DO UPDATE SET
                owner_pubkey=excluded.owner_pubkey,name=excluded.name,
                capabilities_json=excluded.capabilities_json,presence=excluded.presence,
                respond_to=excluded.respond_to,
                respond_to_allowlist_json=excluded.respond_to_allowlist_json,
                verification_state='verified',failure_reason=NULL,
                profile_event_id=excluded.profile_event_id,
                declaration_event_id=excluded.declaration_event_id,
                policy_event_id=excluded.policy_event_id,last_verified_at=excluded.last_verified_at,
                updated_at=unixepoch()
             WHERE owner_pubkey IS NOT excluded.owner_pubkey OR name IS NOT excluded.name
                OR capabilities_json IS NOT excluded.capabilities_json
                OR presence IS NOT excluded.presence OR respond_to IS NOT excluded.respond_to
                OR respond_to_allowlist_json IS NOT excluded.respond_to_allowlist_json
                OR verification_state!='verified' OR failure_reason IS NOT NULL
                OR profile_event_id IS NOT excluded.profile_event_id
                OR declaration_event_id IS NOT excluded.declaration_event_id
                OR policy_event_id IS NOT excluded.policy_event_id",
            params![
                community_id.to_string(),
                agent.pubkey,
                agent.owner_pubkey,
                agent.name,
                capabilities,
                agent.presence.as_str(),
                agent.respond_to.map(RespondTo::as_str),
                allowlist,
                agent.profile_event_id,
                agent.declaration_event_id,
                agent.policy_event_id,
                i64::try_from(agent.verified_at).unwrap_or(i64::MAX),
            ],
        )? > 0;
        if !changed {
            // Renew freshness only after a coalesced successful refresh. This
            // does not request redraw and is never driven by duplicate echoes
            // or frame ticks.
            let verified_at = i64::try_from(agent.verified_at).unwrap_or(i64::MAX);
            self.connection.execute(
                "UPDATE remote_agents SET last_verified_at=?3
                 WHERE community_id=?1 AND agent_pubkey=?2
                   AND verification_state='verified' AND last_verified_at<?3",
                params![community_id.to_string(), agent.pubkey, verified_at],
            )?;
        }
        Ok(changed)
    }

    fn upsert_agent_failure(
        &mut self,
        community_id: Uuid,
        agent_pubkey: &str,
        state: &str,
        reason: &str,
    ) -> Result<bool> {
        let fallback = crate::domain::abbreviated_pubkey(agent_pubkey);
        Ok(self.connection.execute(
            "INSERT INTO remote_agents(
                community_id,agent_pubkey,name,verification_state,failure_reason,updated_at)
             VALUES(?1,?2,?3,?4,?5,unixepoch())
             ON CONFLICT(community_id,agent_pubkey) DO UPDATE SET
                owner_pubkey=NULL,name=excluded.name,capabilities_json='[]',presence='unknown',
                respond_to=NULL,respond_to_allowlist_json='[]',verification_state=excluded.verification_state,
                failure_reason=excluded.failure_reason,profile_event_id=NULL,declaration_event_id=NULL,
                policy_event_id=NULL,last_verified_at=NULL,updated_at=unixepoch()
             WHERE verification_state IS NOT excluded.verification_state
                OR failure_reason IS NOT excluded.failure_reason",
            params![community_id.to_string(), agent_pubkey, fallback, state, reason],
        )? > 0)
    }
}

pub(crate) fn parse_respond_to(value: &str) -> Result<RespondTo> {
    match value {
        "owner-only" => Ok(RespondTo::OwnerOnly),
        "allowlist" => Ok(RespondTo::Allowlist),
        "anyone" => Ok(RespondTo::Anyone),
        _ => Err(Error::Protocol(
            "stored remote-agent policy is invalid".into(),
        )),
    }
}

fn parse_presence(value: &str) -> Result<Presence> {
    match value {
        "online" => Ok(Presence::Online),
        "away" => Ok(Presence::Away),
        "offline" => Ok(Presence::Offline),
        "unknown" => Ok(Presence::Unknown),
        _ => Err(Error::Protocol(
            "stored remote-agent presence is invalid".into(),
        )),
    }
}

impl Store {
    /// The relay-projected role of `pubkey` in `channel`, if a member.
    pub fn member_role(
        &self,
        community: Uuid,
        channel: Uuid,
        pubkey: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT role FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3",
                params![community.to_string(), channel.to_string(), pubkey.to_ascii_lowercase()],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use super::*;
    use crate::config::{CommunityConfig, Config, IdentityConfig, KeyBackend};

    fn setup() -> (Store, Uuid, Keys, Keys, Keys, Uuid) {
        let mut store = Store::open_memory().unwrap();
        let community_id = Uuid::new_v4();
        let identity_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let viewer = Keys::generate();
        let owner = Keys::generate();
        let agent = Keys::generate();
        let config = Config {
            identities: vec![IdentityConfig {
                id: identity_id,
                label: "viewer".into(),
                pubkey: viewer.public_key().to_hex(),
                backend: KeyBackend::Keychain,
                key_ref: "test".into(),
            }],
            communities: vec![CommunityConfig {
                id: community_id,
                label: "test".into(),
                relay_url: "wss://relay.example".into(),
                identity_id,
                allow_insecure_localhost: false,
                theme: None,
                identity_adapter: None,
            }],
            ..Config::default()
        };
        store.sync_config(&config).unwrap();
        store
            .connection
            .execute(
                "UPDATE communities SET relay_pubkey=?2 WHERE id=?1",
                params![
                    community_id.to_string(),
                    Keys::generate().public_key().to_hex()
                ],
            )
            .unwrap();
        store.connection.execute(
            "INSERT INTO channels(community_id,channel_id,name,visibility) VALUES(?1,?2,'agents','public')",
            params![community_id.to_string(), channel_id.to_string()],
        ).unwrap();
        store.connection.execute(
            "INSERT INTO memberships(community_id,channel_id,pubkey,role,source_event_id) VALUES(?1,?2,?3,'bot',?4)",
            params![community_id.to_string(),channel_id.to_string(),agent.public_key().to_hex(),"a".repeat(64)],
        ).unwrap();
        (store, community_id, viewer, owner, agent, channel_id)
    }

    fn public_events(owner: &Keys, agent: &Keys, respond_to: &str) -> (Event, Event, Event) {
        let auth = buzz_sdk::nip_oa::compute_auth_tag(owner, &agent.public_key(), "").unwrap();
        let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
        let profile = EventBuilder::new(Kind::Metadata, r#"{"display_name":"Worker"}"#)
            .tags([Tag::parse(auth).unwrap()])
            .sign_with_keys(agent)
            .unwrap();
        let declaration = EventBuilder::new(
            Kind::Custom(10_100),
            r#"{"capabilities":["messages"],"status":"online"}"#,
        )
        .sign_with_keys(agent)
        .unwrap();
        let policy = EventBuilder::new(
            Kind::Custom(30_177),
            serde_json::json!({
                "name":"Worker", "parallelism":1, "respond_to":respond_to,
                "respond_to_allowlist": Vec::<String>::new()
            })
            .to_string(),
        )
        .tags([Tag::parse(["d", &agent.public_key().to_hex()]).unwrap()])
        .sign_with_keys(owner)
        .unwrap();
        (profile, declaration, policy)
    }

    #[test]
    fn verified_agent_projection_is_idempotent_and_community_scoped() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let (profile, declaration, policy) = public_events(&owner, &agent, "anyone");
        for event in [&profile, &declaration, &policy] {
            store.apply_event(community_id, event).unwrap();
        }
        assert!(
            store
                .reconcile_remote_agent(community_id, &agent.public_key().to_hex())
                .unwrap()
        );
        assert!(
            !store
                .reconcile_remote_agent(community_id, &agent.public_key().to_hex())
                .unwrap(),
            "an unchanged rebuild must be a durable no-op"
        );
        let agents = store
            .list_remote_agents(community_id, &viewer.public_key().to_hex())
            .unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].owner_pubkey, owner.public_key().to_hex());
        assert_eq!(agents[0].eligibility, Eligibility::Eligible);
        assert_eq!(agents[0].channel_ids, vec![channel_id]);
        let candidates = store
            .mention_candidates(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                "Worker",
            )
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].is_agent);
        assert_eq!(candidates[0].agent_eligibility, Some(Eligibility::Eligible));
        store
            .validate_agent_mentions(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE remote_agents SET last_verified_at=0 WHERE community_id=?1 AND agent_pubkey=?2",
                params![community_id.to_string(), agent.public_key().to_hex()],
            )
            .unwrap();
        let stale = store
            .remote_agent(
                community_id,
                &agent.public_key().to_hex(),
                &viewer.public_key().to_hex(),
                Some(channel_id),
            )
            .unwrap()
            .unwrap();
        assert!(stale.stale);
        assert_eq!(stale.eligibility, Eligibility::Ineligible);
    }

    /// A member's channels are asked for by the member, whatever its role
    /// (D13). SQLite answers that from the index of migration 19, not by
    /// reading every membership of the community for each agent: in a
    /// community of 5,000 members that was 33 ms to list fifty agents.
    #[test]
    fn a_members_channels_are_found_by_an_index_not_by_reading_every_membership() {
        let (store, ..) = setup();
        let plan = |sql: &str| {
            let mut statement = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            statement
                .query_map(["c", "p"], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
        };
        // The three places that ask: an agent's channels, whether a listed
        // agent is still a member anywhere, and whether a candidate is.
        for sql in [
            format!(
                "SELECT m.channel_id FROM memberships m
                 JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
                 WHERE m.community_id=?1 AND m.pubkey=?2 AND {AGENT_MEMBERSHIP}"
            ),
            format!(
                "SELECT EXISTS(SELECT 1 FROM memberships m
                 JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
                 WHERE m.community_id=?1 AND m.pubkey=?2 AND {AGENT_MEMBERSHIP})"
            ),
            "SELECT EXISTS(SELECT 1 FROM memberships m
             JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1 AND m.pubkey=?2 AND c.channel_type!='dm')"
                .to_owned(),
        ] {
            let plan = plan(&sql);
            assert!(
                plan.contains("memberships_by_member"),
                "the member's rows are found by the index:\n{plan}"
            );
            assert!(
                !plan.contains("SCAN m"),
                "no membership is read in turn:\n{plan}"
            );
        }
    }

    /// One agent asked for by its key is the entry the whole list has for
    /// it, whatever the case of the key, and not another agent's.
    #[test]
    fn one_agent_asked_for_is_its_entry_of_the_list() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let other = Keys::generate();
        store.connection.execute(
            "INSERT INTO memberships(community_id,channel_id,pubkey,role,source_event_id) VALUES(?1,?2,?3,'bot',?4)",
            params![community_id.to_string(),channel_id.to_string(),other.public_key().to_hex(),"b".repeat(64)],
        ).unwrap();
        for keys in [&agent, &other] {
            let (profile, declaration, policy) = public_events(&owner, keys, "anyone");
            for event in [&profile, &declaration, &policy] {
                store.apply_event(community_id, event).unwrap();
            }
        }
        store.reconcile_remote_agents(community_id).unwrap();
        let viewer = viewer.public_key().to_hex();
        let listed = store.list_remote_agents(community_id, &viewer).unwrap();
        assert_eq!(listed.len(), 2);
        for keys in [&agent, &other] {
            let hex = keys.public_key().to_hex();
            let expected = listed.iter().find(|entry| entry.pubkey == hex).unwrap();
            for asked in [hex.clone(), hex.to_ascii_uppercase()] {
                let one = store
                    .remote_agent(community_id, &asked, &viewer, Some(channel_id))
                    .unwrap()
                    .expect("the agent is found by its key");
                assert_eq!(&one, expected);
            }
        }
        // A key that is no agent's, and an agent asked for in a channel it
        // is not in, are nobody.
        let stranger = Keys::generate().public_key().to_hex();
        assert_eq!(
            store
                .remote_agent(community_id, &stranger, &viewer, None)
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .remote_agent(
                    community_id,
                    &agent.public_key().to_hex(),
                    &viewer,
                    Some(Uuid::new_v4())
                )
                .unwrap(),
            None
        );
    }

    #[test]
    fn relay_bot_with_nip_oa_is_owner_invocable_without_optional_public_records() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let (profile, _, _) = public_events(&owner, &agent, "anyone");
        store.apply_event(community_id, &profile).unwrap();
        store.reconcile_remote_agents(community_id).unwrap();

        let owner_view = store
            .list_remote_agents(community_id, &owner.public_key().to_hex())
            .unwrap();
        assert_eq!(owner_view.len(), 1);
        assert_eq!(owner_view[0].eligibility, Eligibility::Eligible);
        assert!(owner_view[0].capabilities.is_empty());
        store
            .validate_agent_mentions(
                community_id,
                channel_id,
                &owner.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            )
            .unwrap();

        let viewer_view = store
            .list_remote_agents(community_id, &viewer.public_key().to_hex())
            .unwrap();
        assert_eq!(viewer_view[0].eligibility, Eligibility::PolicyUnknown);
        assert!(matches!(
            store.validate_agent_mentions(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            ),
            Err(Error::Access(_))
        ));
    }

    #[test]
    fn dm_and_unknown_policy_mentions_fail_closed() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let (profile, declaration, _) = public_events(&owner, &agent, "anyone");
        for event in [&profile, &declaration] {
            store.apply_event(community_id, event).unwrap();
        }
        store.reconcile_remote_agents(community_id).unwrap();
        let unknown = store.validate_agent_mentions(
            community_id,
            channel_id,
            &viewer.public_key().to_hex(),
            &[agent.public_key().to_hex()],
        );
        assert!(matches!(unknown, Err(Error::Access(_))));

        let (_, _, policy) = public_events(&owner, &agent, "anyone");
        // Only the policy from this helper call is needed; the equivalent
        // profile/declaration records are intentionally ignored.
        store.apply_event(community_id, &policy).unwrap();
        store.reconcile_remote_agents(community_id).unwrap();
        store
            .connection
            .execute(
                "UPDATE channels SET channel_type='dm' WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel_id.to_string()],
            )
            .unwrap();
        let dm = store.validate_agent_mentions(
            community_id,
            channel_id,
            &viewer.public_key().to_hex(),
            &[agent.public_key().to_hex()],
        );
        assert!(matches!(dm, Err(Error::Access(_))));
    }

    #[test]
    fn owner_controlled_dm_member_verifies_without_relaxing_channel_bot_authority() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let (profile, declaration, policy) = public_events(&owner, &agent, "anyone");
        for event in [&profile, &declaration, &policy] {
            store.apply_event(community_id, event).unwrap();
        }
        store
            .connection
            .execute(
                "UPDATE channels SET channel_type='dm',is_member=1,member_count=2
                 WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel_id.to_string()],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE memberships SET role='member'
                 WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3",
                params![
                    community_id.to_string(),
                    channel_id.to_string(),
                    agent.public_key().to_hex()
                ],
            )
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO memberships(community_id,channel_id,pubkey,role,source_event_id)
                 VALUES(?1,?2,?3,'member',?4)",
                params![
                    community_id.to_string(),
                    channel_id.to_string(),
                    owner.public_key().to_hex(),
                    "d".repeat(64)
                ],
            )
            .unwrap();
        store.reconcile_remote_agents(community_id).unwrap();
        let candidates = store.remote_agent_candidate_pubkeys(community_id).unwrap();
        assert!(candidates.contains(&agent.public_key().to_hex()));
        store
            .validate_agent_mentions(
                community_id,
                channel_id,
                &owner.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            )
            .unwrap();
        assert!(matches!(
            store.validate_agent_mentions(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            ),
            Err(Error::Access(_))
        ));
        let mentions = store
            .mention_candidates(
                community_id,
                channel_id,
                &owner.public_key().to_hex(),
                "Worker",
            )
            .unwrap();
        assert_eq!(mentions.len(), 1);
        assert!(mentions[0].is_agent);
        assert_eq!(mentions[0].agent_eligibility, Some(Eligibility::Eligible));

        // Until v0.14 the same `member` role had no authority once the
        // destination was not a DM. Since v0.15 (D13) a member of a channel
        // whose profile carries its owner's proof is an agent there, as
        // Buzz 1.0 adds one; the DM stays the owner's alone all the same.
        store
            .connection
            .execute(
                "UPDATE channels SET channel_type='stream'
                 WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel_id.to_string()],
            )
            .unwrap();
        store.reconcile_remote_agents(community_id).unwrap();
        for person in [&owner, &viewer] {
            store
                .validate_agent_mentions(
                    community_id,
                    channel_id,
                    &person.public_key().to_hex(),
                    &[agent.public_key().to_hex()],
                )
                .unwrap();
        }
    }

    /// The membership of `setup`, as Buzz 1.0 and bzz add an agent to a
    /// channel: a plain member.
    fn as_plain_member(store: &Store, community_id: Uuid, agent: &Keys) {
        store
            .connection
            .execute(
                "UPDATE memberships SET role='member' WHERE community_id=?1 AND pubkey=?2",
                params![community_id.to_string(), agent.public_key().to_hex()],
            )
            .unwrap();
    }

    fn add_member(store: &Store, community_id: Uuid, channel_id: Uuid, member: &Keys) {
        store
            .connection
            .execute(
                "INSERT INTO memberships(community_id,channel_id,pubkey,role,source_event_id)
                 VALUES(?1,?2,?3,'member',?4)",
                params![
                    community_id.to_string(),
                    channel_id.to_string(),
                    member.public_key().to_hex(),
                    "e".repeat(64)
                ],
            )
            .unwrap();
    }

    fn directory_rows(store: &Store, community_id: Uuid) -> i64 {
        store
            .connection
            .query_row(
                "SELECT count(*) FROM remote_agents WHERE community_id=?1",
                [community_id.to_string()],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// D13: the records bzz and Buzz 1.0 publish for an agent (a profile
    /// marked as a bot with the owner's tag, and the owner's record), and a
    /// plain membership: the agent is listed, mentionable by its rule, and
    /// gone when it leaves.
    #[test]
    fn a_plain_member_with_its_owners_proof_is_an_agent() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        as_plain_member(&store, community_id, &agent);
        let auth = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap();
        let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
        let profile = EventBuilder::new(Kind::Metadata, r#"{"name":"Scout","bot":true}"#)
            .tags([Tag::parse(auth).unwrap()])
            .sign_with_keys(&agent)
            .unwrap();
        let record = EventBuilder::new(
            Kind::Custom(30_177),
            r#"{"parallelism":1,"respond_to":"owner-only","name":"Scout"}"#,
        )
        .tags([
            Tag::parse(["d", &agent.public_key().to_hex()]).unwrap(),
            Tag::parse(["runtime", "bzz/pi-durable"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
        let agent_hex = agent.public_key().to_hex();
        // Without its profile it is a member like any other.
        store.reconcile_remote_agents(community_id).unwrap();
        assert_eq!(directory_rows(&store, community_id), 0);
        assert_eq!(
            store.remote_agent_member_pubkeys(community_id).unwrap(),
            std::slice::from_ref(&agent_hex),
            "a member with no stored profile is looked at by a refresh"
        );
        for event in [&profile, &record] {
            store.apply_event(community_id, event).unwrap();
        }
        assert_eq!(store.reconcile_remote_agents(community_id).unwrap(), 1);
        let listed = store
            .list_remote_agents(community_id, &owner.public_key().to_hex())
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].name.as_str(), listed[0].owner_pubkey.clone()),
            ("Scout", owner.public_key().to_hex())
        );
        assert_eq!(listed[0].eligibility, Eligibility::Eligible);
        assert_eq!(listed[0].channel_ids, vec![channel_id]);
        assert_eq!(listed[0].respond_to, Some(RespondTo::OwnerOnly));
        // Mentioned as an agent, by its owner's rule.
        let mentions = store
            .mention_candidates(
                community_id,
                channel_id,
                &owner.public_key().to_hex(),
                "Scout",
            )
            .unwrap();
        assert_eq!(mentions.len(), 1);
        assert!(mentions[0].is_agent);
        assert_eq!(mentions[0].agent_eligibility, Some(Eligibility::Eligible));
        store
            .validate_agent_mentions(
                community_id,
                channel_id,
                &owner.public_key().to_hex(),
                std::slice::from_ref(&agent_hex),
            )
            .unwrap();
        assert!(
            matches!(
                store.validate_agent_mentions(
                    community_id,
                    channel_id,
                    &viewer.public_key().to_hex(),
                    std::slice::from_ref(&agent_hex),
                ),
                Err(Error::Access(_))
            ),
            "owner only: its owner's rule still decides who may wake it"
        );
        assert_eq!(
            store.remote_agent_member_pubkeys(community_id).unwrap(),
            std::slice::from_ref(&agent_hex),
            "its profile is refreshed with the directory"
        );
        // It leaves the channel: no longer listed.
        store
            .connection
            .execute(
                "DELETE FROM memberships WHERE community_id=?1 AND pubkey=?2",
                params![community_id.to_string(), agent_hex],
            )
            .unwrap();
        assert_eq!(store.reconcile_remote_agents(community_id).unwrap(), 1);
        assert!(
            store
                .list_remote_agents(community_id, &owner.public_key().to_hex())
                .unwrap()
                .is_empty()
        );
    }

    /// The proof requirements are those of v0.11: a member without a valid
    /// owner's proof on its profile is a person, and never enters the
    /// directory, not even as a failure that would get in the way of
    /// mentioning it.
    #[test]
    fn a_member_without_a_valid_owners_proof_is_not_an_agent() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        as_plain_member(&store, community_id, &agent);
        let person = Keys::generate();
        let claims_a_bot = Keys::generate();
        let borrowed_proof = Keys::generate();
        for member in [&person, &claims_a_bot, &borrowed_proof] {
            add_member(&store, community_id, channel_id, member);
        }
        // A person's profile; a profile that only says it is a bot; and one
        // that carries the owner's authorization of another key.
        let plain = EventBuilder::new(Kind::Metadata, r#"{"name":"Ann"}"#)
            .sign_with_keys(&person)
            .unwrap();
        let unproven = EventBuilder::new(Kind::Metadata, r#"{"name":"Bot","bot":true}"#)
            .sign_with_keys(&claims_a_bot)
            .unwrap();
        let auth = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap();
        let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
        let borrowed = EventBuilder::new(Kind::Metadata, r#"{"name":"Thief","bot":true}"#)
            .tags([Tag::parse(auth).unwrap()])
            .sign_with_keys(&borrowed_proof)
            .unwrap();
        for event in [&plain, &unproven, &borrowed] {
            store.apply_event(community_id, event).unwrap();
        }
        store.reconcile_remote_agents(community_id).unwrap();
        assert_eq!(
            directory_rows(&store, community_id),
            0,
            "none of them is in the directory, in any state"
        );
        for identity in [&viewer, &owner] {
            assert!(
                store
                    .list_remote_agents(community_id, &identity.public_key().to_hex())
                    .unwrap()
                    .is_empty()
            );
        }
        // Mentioning them is mentioning people: nothing to validate.
        let mentioned = [&person, &claims_a_bot, &borrowed_proof]
            .map(|member| member.public_key().to_hex())
            .to_vec();
        assert_eq!(
            store
                .agent_mentions_need_validation(community_id, channel_id, &mentioned)
                .unwrap(),
            0
        );
        store
            .validate_agent_mentions(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                &mentioned,
            )
            .unwrap();
        // A refresh looks again only at the one whose profile claims an
        // owner and at the one with no profile yet, never at the person.
        let mut looked_at = store.remote_agent_member_pubkeys(community_id).unwrap();
        looked_at.sort();
        let mut expected = vec![
            agent.public_key().to_hex(),
            borrowed_proof.public_key().to_hex(),
        ];
        expected.sort();
        assert_eq!(looked_at, expected);
    }

    #[test]
    fn removal_revokes_directory_visibility_without_deleting_history() {
        let (mut store, community_id, viewer, owner, agent, channel_id) = setup();
        let (profile, declaration, policy) = public_events(&owner, &agent, "anyone");
        for event in [&profile, &declaration, &policy] {
            store.apply_event(community_id, event).unwrap();
        }
        store.reconcile_remote_agents(community_id).unwrap();
        store
            .connection
            .execute(
                "DELETE FROM memberships WHERE community_id=?1 AND pubkey=?2",
                params![community_id.to_string(), agent.public_key().to_hex()],
            )
            .unwrap();
        assert_eq!(store.reconcile_remote_agents(community_id).unwrap(), 1);
        assert!(
            store
                .list_remote_agents(community_id, &viewer.public_key().to_hex())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .agent_mentions_need_validation(
                    community_id,
                    channel_id,
                    &[agent.public_key().to_hex()],
                )
                .unwrap(),
            1,
            "a revoked structured agent mention must still be revalidated"
        );
        assert!(matches!(
            store.validate_agent_mentions(
                community_id,
                channel_id,
                &viewer.public_key().to_hex(),
                &[agent.public_key().to_hex()],
            ),
            Err(Error::Access(_))
        ));
        let events: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM events WHERE community_id=?1",
                [community_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(events, 3);
    }

    #[test]
    fn wrong_owner_policy_never_becomes_verified() {
        let (mut store, community_id, viewer, owner, agent, _) = setup();
        let (profile, declaration, _) = public_events(&owner, &agent, "anyone");
        let attacker = Keys::generate();
        let forged = EventBuilder::new(
            Kind::Custom(30_177),
            serde_json::json!({
                "name":"Forged", "parallelism":1, "respond_to":"anyone"
            })
            .to_string(),
        )
        .tags([Tag::parse(["d", &agent.public_key().to_hex()]).unwrap()])
        .sign_with_keys(&attacker)
        .unwrap();
        for event in [&profile, &declaration, &forged] {
            store.apply_event(community_id, event).unwrap();
        }
        store.reconcile_remote_agents(community_id).unwrap();
        let agents = store
            .list_remote_agents(community_id, &viewer.public_key().to_hex())
            .unwrap();
        assert_eq!(
            agents.len(),
            1,
            "forged policy is ignored; valid ownership remains policy-unknown"
        );
        assert_eq!(agents[0].eligibility, Eligibility::PolicyUnknown);
    }
}
