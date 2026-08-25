use std::collections::BTreeSet;

use nostr::{Event, JsonUtil as _};
use rusqlite::{OptionalExtension as _, Transaction, params};
use uuid::Uuid;

use crate::{
    domain::Visibility,
    error::{Error, Result},
    protocol::events::{
        as_channel, as_profile, as_reaction, channel_id, event_references, thread_coordinates,
        verify,
    },
    store::Store,
};

impl Store {
    pub fn live_thread_summary(
        &self,
        community_id: Uuid,
        event: &Event,
    ) -> Result<Option<crate::protocol::thread_summary::LiveThreadSummary>> {
        let relay_pubkey = self.connection.query_row(
            "SELECT relay_pubkey FROM communities WHERE id=?1",
            [community_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )?;
        Ok(relay_pubkey
            .as_deref()
            .and_then(|relay| crate::protocol::thread_summary::parse(event, relay)))
    }

    pub fn apply_event(&mut self, community_id: Uuid, event: &Event) -> Result<bool> {
        verify(event)?;
        if event.kind.as_u16() == 39_005 {
            return Err(Error::Protocol(
                "transient thread summaries cannot enter the durable event store".into(),
            ));
        }
        if matches!(
            event.kind.as_u16(),
            30_622 | 39_000..=39_003 | 40_099 | 44_100 | 44_101
        ) {
            let relay_pubkey = self.connection.query_row(
                "SELECT relay_pubkey FROM communities WHERE id=?1",
                [community_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )?;
            if relay_pubkey.as_deref() != Some(event.pubkey.to_hex().as_str()) {
                return Err(Error::Protocol(
                    "relay projection was not signed by the pinned relay key".into(),
                ));
            }
        }
        let raw = event.as_json();
        let existing = self
            .connection
            .query_row(
                "SELECT raw_json FROM events WHERE community_id=?1 AND event_id=?2",
                params![community_id.to_string(), event.id.to_hex()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != raw {
                return Err(Error::Protocol(
                    "conflicting bytes for an existing event ID".into(),
                ));
            }
            let projection_changed = if event.kind.as_u16() == 39_002 {
                self.repair_current_membership_projection(community_id, event)?
            } else {
                false
            };
            let outbox_changed = self.mark_outbox_observed(community_id, &event.id.to_hex())?;
            if projection_changed {
                self.reconcile_remote_agents(community_id)?;
            }
            return Ok(projection_changed || outbox_changed);
        }
        let channel = channel_id(event);
        let (root, parent) = thread_coordinates(event);
        let tags = serde_json::to_string(&event.tags)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let http_base = self.connection.query_row(
            "SELECT http_base_url FROM communities WHERE id=?1",
            [community_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO events(community_id,event_id,kind,pubkey,created_at,channel_id,content,tags_json,raw_json,root_event_id,parent_event_id,received_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,unixepoch())",
            params![
                community_id.to_string(), event.id.to_hex(), i64::from(event.kind.as_u16()),
                event.pubkey.to_hex(), u64_to_i64(event.created_at.as_secs())?,
                channel.map(|id| id.to_string()), event.content, tags, raw, root, parent,
            ],
        )?;
        crate::store::search::project_event(&transaction, community_id, event, &http_base)?;
        if event.kind.as_u16() == 30_622 {
            apply_dm_visibility_snapshot(&transaction, community_id, event)?;
        }
        if matches!(event.kind.as_u16(), 9 | 40_002)
            && let Some(channel) = channel
        {
            transaction.execute(
                "UPDATE channels SET last_event_at=max(COALESCE(last_event_at,0),?3) WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel.to_string(), u64_to_i64(event.created_at.as_secs())?],
            )?;
        }
        if let Some(profile) = as_profile(event) {
            transaction.execute(
                "INSERT INTO profiles(community_id,pubkey,display_name,name,picture,nip05,about,event_id,created_at,raw_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(community_id,pubkey) DO UPDATE SET display_name=excluded.display_name,name=excluded.name,picture=excluded.picture,nip05=excluded.nip05,about=excluded.about,event_id=excluded.event_id,created_at=excluded.created_at,raw_json=excluded.raw_json
                 WHERE (excluded.created_at,excluded.event_id) > (profiles.created_at,profiles.event_id)",
                params![community_id.to_string(),profile.pubkey,profile.display_name,profile.name,profile.picture,profile.nip05,profile.about,profile.event_id,u64_to_i64(profile.created_at)?,event.as_json()],
            )?;
        }
        if let Some(channel) = as_channel(event) {
            let visibility = match channel.visibility {
                Visibility::Public => "public",
                Visibility::Private => "private",
            };
            transaction.execute(
                "INSERT INTO channels(community_id,channel_id,name,about,channel_type,visibility,is_hidden,metadata_event_id,metadata_created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,0,?7,?8)
                 ON CONFLICT(community_id,channel_id) DO UPDATE SET name=excluded.name,about=excluded.about,channel_type=excluded.channel_type,visibility=excluded.visibility,metadata_event_id=excluded.metadata_event_id,metadata_created_at=excluded.metadata_created_at
                 WHERE (excluded.metadata_created_at,excluded.metadata_event_id) > (COALESCE(channels.metadata_created_at,0),COALESCE(channels.metadata_event_id,''))",
                params![community_id.to_string(),channel.id.to_string(),channel.name,channel.about,channel.kind.as_str(),visibility,event.id.to_hex(),u64_to_i64(event.created_at.as_secs())?],
            )?;
            if channel.kind.is_dm() {
                let participant_count: usize = transaction.query_row(
                    "SELECT count(*) FROM memberships WHERE community_id=?1 AND channel_id=?2",
                    params![community_id.to_string(), channel.id.to_string()],
                    |row| row.get(0),
                )?;
                if participant_count != 0 && !(2..=9).contains(&participant_count) {
                    return Err(Error::Protocol(
                        "DM metadata has an invalid participant set".into(),
                    ));
                }
            }
        }
        if let Some(snapshot) = membership_snapshot(event)? {
            validate_dm_membership(&transaction, community_id, &snapshot)?;
            transaction.execute(
                "INSERT INTO channels(community_id,channel_id,name,visibility) VALUES(?1,?2,?2,'private') ON CONFLICT DO NOTHING",
                params![community_id.to_string(),snapshot.channel],
            )?;
            let source_event_id = event.id.to_hex();
            let source_created_at = event.created_at.as_secs();
            let existing = transaction
                .query_row(
                    "SELECT source_created_at,source_event_id FROM channel_membership_heads WHERE community_id=?1 AND channel_id=?2",
                    params![community_id.to_string(), snapshot.channel],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let is_newer = existing.is_none_or(|(created_at, event_id)| {
                let created_at = u64::try_from(created_at).unwrap_or(0);
                source_created_at > created_at
                    || (source_created_at == created_at && source_event_id < event_id)
            });
            if is_newer {
                transaction.execute(
                    "INSERT INTO channel_membership_heads(community_id,channel_id,source_event_id,source_created_at) VALUES(?1,?2,?3,?4)
                     ON CONFLICT(community_id,channel_id) DO UPDATE SET source_event_id=excluded.source_event_id,source_created_at=excluded.source_created_at",
                    params![community_id.to_string(),snapshot.channel,source_event_id,u64_to_i64(source_created_at)?],
                )?;
                replace_membership_projection(
                    &transaction,
                    community_id,
                    &snapshot,
                    &source_event_id,
                )?;
            }
        }
        if let Some(reaction) = as_reaction(event) {
            transaction.execute(
                "INSERT INTO reactions(community_id,reaction_event_id,target_event_id,pubkey,emoji,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![community_id.to_string(),reaction.event_id,reaction.target_event_id,reaction.pubkey,reaction.emoji,u64_to_i64(reaction.created_at)?],
            )?;
        }
        if matches!(event.kind.as_u16(), 5 | 9005) {
            for target in event_references(event) {
                transaction.execute(
                    "INSERT OR IGNORE INTO deletion_targets(community_id,deletion_event_id,target_event_id,deletion_kind,deletion_pubkey) VALUES(?1,?2,?3,?4,?5)",
                    params![community_id.to_string(),event.id.to_hex(),target,i64::from(event.kind.as_u16()),event.pubkey.to_hex()],
                )?;
                transaction.execute(
                    "UPDATE events SET deleted_by_event_id=COALESCE(deleted_by_event_id,?3) WHERE community_id=?1 AND event_id=?2 AND (?4=9005 OR pubkey=?5)",
                    params![community_id.to_string(),target,event.id.to_hex(),i64::from(event.kind.as_u16()),event.pubkey.to_hex()],
                )?;
                transaction.execute(
                    "UPDATE reactions SET deleted_by_event_id=COALESCE(deleted_by_event_id,?3) WHERE community_id=?1 AND reaction_event_id=?2 AND (?4=9005 OR pubkey=?5)",
                    params![community_id.to_string(),target,event.id.to_hex(),i64::from(event.kind.as_u16()),event.pubkey.to_hex()],
                )?;
                transaction.execute(
                    "DELETE FROM search_documents WHERE community_id=?1 AND event_id=?2
                     AND EXISTS(SELECT 1 FROM events WHERE community_id=?1 AND event_id=?2 AND deleted_by_event_id IS NOT NULL)",
                    params![community_id.to_string(),target],
                )?;
            }
        } else {
            transaction.execute(
                "UPDATE events SET deleted_by_event_id=(SELECT deletion_event_id FROM deletion_targets d WHERE d.community_id=?1 AND d.target_event_id=?2 AND (d.deletion_kind=9005 OR d.deletion_pubkey=?3) ORDER BY deletion_event_id LIMIT 1) WHERE community_id=?1 AND event_id=?2",
                params![community_id.to_string(),event.id.to_hex(),event.pubkey.to_hex()],
            )?;
            transaction.execute(
                "UPDATE reactions SET deleted_by_event_id=(SELECT deletion_event_id FROM deletion_targets d WHERE d.community_id=?1 AND d.target_event_id=?2 AND (d.deletion_kind=9005 OR d.deletion_pubkey=?3) ORDER BY deletion_event_id LIMIT 1) WHERE community_id=?1 AND reaction_event_id=?2",
                params![community_id.to_string(),event.id.to_hex(),event.pubkey.to_hex()],
            )?;
            transaction.execute(
                "DELETE FROM search_documents WHERE community_id=?1 AND event_id=?2
                 AND EXISTS(SELECT 1 FROM events WHERE community_id=?1 AND event_id=?2 AND deleted_by_event_id IS NOT NULL)",
                params![community_id.to_string(),event.id.to_hex()],
            )?;
        }
        transaction.execute(
            "UPDATE outbox SET state='delivered',updated_at=unixepoch(),last_error_code=NULL WHERE community_id=?1 AND event_id=?2",
            params![community_id.to_string(),event.id.to_hex()],
        )?;
        transaction.execute(
            "DELETE FROM drafts WHERE community_id=?1 AND outbox_event_id=?2",
            params![community_id.to_string(), event.id.to_hex()],
        )?;
        if event.kind.as_u16() != 40_099 {
            crate::store::inbox::mark_projection_dirty(&transaction, community_id)?;
        }
        transaction.commit()?;
        if event.kind.as_u16() == 39_002 {
            // Membership is the bounded candidate authority. Public agent
            // records are reconciled in one coalesced pass after directory
            // hydration instead of writing once per profile/policy event.
            self.reconcile_remote_agents(community_id)?;
        }
        Ok(true)
    }

    /// An event ID is immutable, but an older bzz binary may have projected
    /// its role fields differently. Re-derive only the current relay-signed
    /// membership head and write only when the durable rows disagree.
    fn repair_current_membership_projection(
        &mut self,
        community_id: Uuid,
        event: &Event,
    ) -> Result<bool> {
        let Some(snapshot) = membership_snapshot(event)? else {
            return Ok(false);
        };
        let current: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM channel_membership_heads
             WHERE community_id=?1 AND channel_id=?2 AND source_event_id=?3)",
            params![
                community_id.to_string(),
                snapshot.channel,
                event.id.to_hex()
            ],
            |row| row.get(0),
        )?;
        if !current {
            return Ok(false);
        }
        validate_dm_membership(&self.connection, community_id, &snapshot)?;
        let mut expected = snapshot
            .members
            .iter()
            .map(|(pubkey, role)| (pubkey.clone(), (*role).to_owned(), event.id.to_hex()))
            .collect::<Vec<_>>();
        expected.sort();
        let mut statement = self.connection.prepare(
            "SELECT pubkey,role,source_event_id FROM memberships
             WHERE community_id=?1 AND channel_id=?2 ORDER BY pubkey",
        )?;
        let projected = statement
            .query_map(params![community_id.to_string(), snapshot.channel], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        if projected == expected {
            return Ok(false);
        }
        let transaction = self.connection.transaction()?;
        replace_membership_projection(&transaction, community_id, &snapshot, &event.id.to_hex())?;
        transaction.commit()?;
        Ok(true)
    }

    fn mark_outbox_observed(&mut self, community_id: Uuid, event_id: &str) -> Result<bool> {
        let transaction = self.connection.transaction()?;
        let previous = transaction
            .query_row(
                "SELECT state,kind,attempts FROM outbox WHERE community_id=?1 AND event_id=?2",
                params![community_id.to_string(), event_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        u16::try_from(row.get::<_, i64>(1)?).unwrap_or(u16::MAX),
                        u32::try_from(row.get::<_, i64>(2)?).unwrap_or(u32::MAX),
                    ))
                },
            )
            .optional()?;
        // An observed echo is authoritative delivery, but not another publish
        // attempt. A relay may echo the same event through overlapping
        // subscriptions, so already-delivered echoes must remain true no-ops:
        // do not rewrite timestamps or dirty the Inbox projection.
        let outbox_changed = transaction.execute(
            "UPDATE outbox SET state='delivered',updated_at=unixepoch(),last_error_code=NULL
             WHERE community_id=?1 AND event_id=?2
               AND (state<>'delivered' OR last_error_code IS NOT NULL)",
            params![community_id.to_string(), event_id],
        )?;
        let drafts_deleted = transaction.execute(
            "DELETE FROM drafts WHERE community_id=?1 AND outbox_event_id=?2",
            params![community_id.to_string(), event_id],
        )?;
        let changed = outbox_changed != 0 || drafts_deleted != 0;
        if changed {
            crate::store::inbox::mark_projection_dirty(&transaction, community_id)?;
        }
        transaction.commit()?;
        if outbox_changed != 0
            && let Some((old_state, kind, attempts)) = previous
        {
            self.diagnostics
                .emit(crate::diagnostics::DiagnosticEvent::OutboxStateChanged {
                    event_id: event_id.into(),
                    kind,
                    old_state,
                    new_state: "delivered".into(),
                    attempts,
                });
        }
        Ok(changed)
    }
}

struct MembershipSnapshot {
    channel: String,
    members: Vec<(String, &'static str)>,
}

fn membership_snapshot(event: &Event) -> Result<Option<MembershipSnapshot>> {
    if event.kind.as_u16() != 39_002 {
        return Ok(None);
    }
    let d_tags = crate::protocol::events::tag_values(event, "d");
    if d_tags.len() != 1 || Uuid::parse_str(&d_tags[0]).is_err() {
        return Err(Error::Protocol(
            "membership snapshot has an invalid channel ID".into(),
        ));
    }
    let mut members = Vec::new();
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        if values.first().map(String::as_str) != Some("p") {
            continue;
        }
        let Some(pubkey) = values.get(1) else {
            return Err(Error::Protocol(
                "membership snapshot has an invalid participant tag".into(),
            ));
        };
        let role = if values.len() == 4 && values[3] == "bot" {
            "bot"
        } else {
            "member"
        };
        members.push((pubkey.clone(), role));
    }
    if members.len() > 10_000
        || members.iter().any(|(pubkey, _)| {
            pubkey.len() != 64
                || pubkey != &pubkey.to_ascii_lowercase()
                || !pubkey.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || members
            .iter()
            .map(|(pubkey, _)| pubkey)
            .collect::<BTreeSet<_>>()
            .len()
            != members.len()
    {
        return Err(Error::Protocol(
            "membership snapshot has an invalid or oversized participant set".into(),
        ));
    }
    Ok(Some(MembershipSnapshot {
        channel: d_tags[0].clone(),
        members,
    }))
}

fn validate_dm_membership(
    connection: &rusqlite::Connection,
    community_id: Uuid,
    snapshot: &MembershipSnapshot,
) -> Result<()> {
    let channel_type: Option<String> = connection
        .query_row(
            "SELECT channel_type FROM channels WHERE community_id=?1 AND channel_id=?2",
            params![community_id.to_string(), snapshot.channel],
            |row| row.get(0),
        )
        .optional()?;
    if channel_type.as_deref() == Some("dm") && !(2..=9).contains(&snapshot.members.len()) {
        return Err(Error::Protocol(
            "DM membership snapshot must contain 2-9 participants".into(),
        ));
    }
    Ok(())
}

fn replace_membership_projection(
    transaction: &Transaction<'_>,
    community_id: Uuid,
    snapshot: &MembershipSnapshot,
    source_event_id: &str,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM memberships WHERE community_id=?1 AND channel_id=?2",
        params![community_id.to_string(), snapshot.channel],
    )?;
    for (pubkey, role) in &snapshot.members {
        transaction.execute(
            "INSERT INTO memberships(community_id,channel_id,pubkey,role,source_event_id)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                community_id.to_string(),
                snapshot.channel,
                pubkey,
                role,
                source_event_id
            ],
        )?;
    }
    transaction.execute(
        "UPDATE channels SET
           member_count=(SELECT count(*) FROM memberships WHERE community_id=?1 AND channel_id=?2),
           is_member=EXISTS(SELECT 1 FROM memberships m
             JOIN communities c ON c.id=m.community_id
             JOIN identities i ON i.id=c.identity_id
             WHERE m.community_id=?1 AND m.channel_id=?2 AND m.pubkey=i.pubkey)
         WHERE community_id=?1 AND channel_id=?2",
        params![community_id.to_string(), snapshot.channel],
    )?;
    Ok(())
}

fn apply_dm_visibility_snapshot(
    transaction: &Transaction<'_>,
    community_id: Uuid,
    event: &Event,
) -> Result<()> {
    let self_pubkey: String = transaction.query_row(
        "SELECT i.pubkey FROM communities c JOIN identities i ON i.id=c.identity_id WHERE c.id=?1",
        [community_id.to_string()],
        |row| row.get(0),
    )?;
    let d_tags = crate::protocol::events::tag_values(event, "d");
    let p_tags = crate::protocol::events::tag_values(event, "p");
    if d_tags.as_slice() != [self_pubkey.clone()] || p_tags.as_slice() != [self_pubkey.clone()] {
        return Err(Error::Protocol(
            "DM visibility snapshot does not belong to the active identity".into(),
        ));
    }
    let source_event_id = event.id.to_hex();
    let source_created_at = event.created_at.as_secs();
    let existing = transaction
        .query_row(
            "SELECT source_created_at,source_event_id FROM dm_visibility_heads WHERE community_id=?1 AND identity_pubkey=?2",
            params![community_id.to_string(),self_pubkey],
            |row| Ok((row.get::<_, i64>(0)?,row.get::<_,String>(1)?)),
        )
        .optional()?;
    if let Some((created_at, event_id)) = existing {
        let created_at = u64::try_from(created_at).unwrap_or(0);
        if source_created_at < created_at
            || (source_created_at == created_at && source_event_id >= event_id)
        {
            return Ok(());
        }
    }
    let h_tags = crate::protocol::events::tag_values(event, "h");
    if h_tags.len() > 10_000 {
        return Err(Error::Protocol(
            "DM visibility snapshot contains too many channels".into(),
        ));
    }
    let hidden = h_tags
        .into_iter()
        .map(|value| {
            Uuid::parse_str(&value).map_err(|_| {
                Error::Protocol("DM visibility snapshot has an invalid channel ID".into())
            })
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if hidden.len() > 10_000 {
        return Err(Error::Protocol(
            "DM visibility snapshot contains too many channels".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO dm_visibility_heads(community_id,identity_pubkey,source_event_id,source_created_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(community_id,identity_pubkey) DO UPDATE SET source_event_id=excluded.source_event_id,source_created_at=excluded.source_created_at",
        params![community_id.to_string(),self_pubkey,source_event_id,u64_to_i64(source_created_at)?],
    )?;
    transaction.execute(
        "DELETE FROM dm_visibility WHERE community_id=?1 AND identity_pubkey=?2",
        params![community_id.to_string(), self_pubkey],
    )?;
    for channel_id in hidden {
        transaction.execute(
            "INSERT INTO dm_visibility(community_id,identity_pubkey,channel_id,source_event_id,source_created_at) VALUES(?1,?2,?3,?4,?5)",
            params![community_id.to_string(),self_pubkey,channel_id.to_string(),source_event_id,u64_to_i64(source_created_at)?],
        )?;
    }
    Ok(())
}

pub(crate) fn u64_to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Protocol("timestamp is out of range".into()))
}
