use nostr::{Event, JsonUtil as _};
use rusqlite::{OptionalExtension as _, params};
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
    pub fn apply_event(&mut self, community_id: Uuid, event: &Event) -> Result<bool> {
        verify(event)?;
        if matches!(event.kind.as_u16(), 39_000..=39_003 | 44_100 | 44_101) {
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
            self.mark_outbox_observed(community_id, &event.id.to_hex())?;
            return Ok(false);
        }
        let channel = channel_id(event);
        let (root, parent) = thread_coordinates(event);
        let tags = serde_json::to_string(&event.tags)
            .map_err(|error| Error::Serialization(error.to_string()))?;
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
        if matches!(event.kind.as_u16(), 9 | 40_002 | 40_099)
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
                "INSERT INTO channels(community_id,channel_id,name,about,visibility,is_hidden,metadata_event_id,metadata_created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(community_id,channel_id) DO UPDATE SET name=excluded.name,about=excluded.about,visibility=excluded.visibility,is_hidden=excluded.is_hidden,metadata_event_id=excluded.metadata_event_id,metadata_created_at=excluded.metadata_created_at
                 WHERE (excluded.metadata_created_at,excluded.metadata_event_id) > (COALESCE(channels.metadata_created_at,0),COALESCE(channels.metadata_event_id,''))",
                params![community_id.to_string(),channel.id.to_string(),channel.name,channel.about,visibility,channel.is_hidden,event.id.to_hex(),u64_to_i64(event.created_at.as_secs())?],
            )?;
        }
        if event.kind.as_u16() == 39_002
            && let Some(channel) = crate::protocol::events::first_tag(event, "d")
            && Uuid::parse_str(&channel).is_ok()
        {
            transaction.execute(
                "INSERT INTO channels(community_id,channel_id,name,visibility) VALUES(?1,?2,?2,'private') ON CONFLICT DO NOTHING",
                params![community_id.to_string(),channel],
            )?;
            transaction.execute(
                "DELETE FROM memberships WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel],
            )?;
            for pubkey in crate::protocol::events::tag_values(event, "p") {
                transaction.execute(
                    "INSERT OR IGNORE INTO memberships(community_id,channel_id,pubkey,source_event_id) VALUES(?1,?2,?3,?4)",
                    params![community_id.to_string(),channel,pubkey,event.id.to_hex()],
                )?;
            }
            transaction.execute(
                "UPDATE channels SET member_count=(SELECT count(*) FROM memberships WHERE community_id=?1 AND channel_id=?2),
                   is_member=EXISTS(SELECT 1 FROM memberships m JOIN communities c ON c.id=m.community_id JOIN identities i ON i.id=c.identity_id WHERE m.community_id=?1 AND m.channel_id=?2 AND m.pubkey=i.pubkey)
                 WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(),channel],
            )?;
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
        }
        transaction.execute(
            "UPDATE outbox SET state='delivered',updated_at=unixepoch(),last_error_code=NULL WHERE community_id=?1 AND event_id=?2",
            params![community_id.to_string(),event.id.to_hex()],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    fn mark_outbox_observed(&self, community_id: Uuid, event_id: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE outbox SET state='delivered',updated_at=unixepoch(),last_error_code=NULL WHERE community_id=?1 AND event_id=?2",
            params![community_id.to_string(),event_id],
        )?;
        Ok(())
    }
}

pub(crate) fn u64_to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Protocol("timestamp is out of range".into()))
}
