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
        let is_bot_member: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memberships WHERE community_id=?1 AND pubkey=?2 AND role='bot')",
            params![community_id.to_string(), agent_pubkey],
            |row| row.get(0),
        )?;
        if !is_bot_member {
            return Ok(self.connection.execute(
                "UPDATE remote_agents SET verification_state='removed',failure_reason='not-a-bot-member',updated_at=unixepoch() WHERE community_id=?1 AND agent_pubkey=?2 AND verification_state!='removed'",
                params![community_id.to_string(), agent_pubkey],
            )? > 0);
        }

        let profile = self.latest_agent_event(community_id, 0, &agent_pubkey)?;
        let declaration = self.latest_agent_event(community_id, 10_100, &agent_pubkey)?;
        let (Some(profile), Some(declaration)) = (profile, declaration) else {
            return self.upsert_agent_failure(
                community_id,
                &agent_pubkey,
                "incomplete",
                "required-public-record-missing",
            );
        };

        // Verify ownership before using the owner as a query coordinate.
        let without_policy = match verify_public_agent(&profile, &declaration, None) {
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
        match verify_public_agent(&profile, &declaration, policy.as_ref()) {
            Ok(agent) => self.upsert_verified_agent(community_id, &agent),
            Err(failure) => {
                self.upsert_agent_failure(community_id, &agent_pubkey, "invalid", failure.as_str())
            }
        }
    }

    pub fn remote_agent_candidate_pubkeys(&self, community_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT pubkey FROM memberships
             WHERE community_id=?1 AND role='bot' ORDER BY pubkey LIMIT 5000",
        )?;
        statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Reconcile every currently known bot candidate plus projections whose
    /// membership was removed. The bounded membership source prevents a
    /// hostile global profile stream from creating unbounded candidates.
    pub fn reconcile_remote_agents(&mut self, community_id: Uuid) -> Result<usize> {
        let mut statement = self.connection.prepare(
            "SELECT pubkey FROM memberships WHERE community_id=?1 AND role='bot'
             UNION SELECT agent_pubkey FROM remote_agents WHERE community_id=?1
             ORDER BY 1 LIMIT 5000",
        )?;
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
        PublicKey::from_hex(active_pubkey)
            .map_err(|_| Error::Config("active identity has an invalid pubkey".into()))?;
        let mut statement = self.connection.prepare(
            "SELECT agent_pubkey,owner_pubkey,name,capabilities_json,presence,respond_to,
                    respond_to_allowlist_json,last_verified_at
             FROM remote_agents a
             WHERE community_id=?1 AND verification_state='verified'
               AND EXISTS(SELECT 1 FROM memberships m WHERE m.community_id=a.community_id
                          AND m.pubkey=a.agent_pubkey AND m.role='bot')
             ORDER BY name COLLATE NOCASE,agent_pubkey LIMIT 5000",
        )?;
        let rows = statement.query_map([community_id.to_string()], |row| {
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
        for mention in mentions {
            let mention = PublicKey::from_hex(mention)
                .map_err(|_| Error::Protocol("message mention has an invalid pubkey".into()))?
                .to_hex();
            let (is_agent_candidate, is_bot): (bool, bool) = self.connection.query_row(
                "SELECT
                    EXISTS(SELECT 1 FROM remote_agents WHERE community_id=?1 AND agent_pubkey=?3)
                      OR EXISTS(SELECT 1 FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3 AND role='bot'),
                    EXISTS(SELECT 1 FROM memberships WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3 AND role='bot')",
                params![community_id.to_string(),channel_id.to_string(),mention],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if !is_agent_candidate {
                continue;
            }
            if !is_bot {
                return Err(Error::Access(
                    "remote agent mention is no longer a bot member of this channel".into(),
                ));
            }
            let agent = self
                .remote_agent(community_id, &mention, active_pubkey, Some(channel_id))?
                .ok_or_else(|| {
                    Error::Access(
                        "remote agent mention is not currently verified for this channel".into(),
                    )
                })?;
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
        let mut agents = self.list_remote_agents(community_id, active_pubkey)?;
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
        let mut statement = self.connection.prepare(
            "SELECT channel_id FROM memberships WHERE community_id=?1 AND pubkey=?2 AND role='bot'
             ORDER BY channel_id LIMIT 5000",
        )?;
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
        let events: usize = store
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
