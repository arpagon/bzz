use std::collections::BTreeMap;

use nostr::{Event, JsonUtil as _};
use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    domain::{Channel, Message, Profile, Reaction, Visibility},
    error::{Error, Result},
    store::{
        Store,
        events::u64_to_i64,
        models::{OutboxItem, OutboxState, ReadSlotRecord, SyncCursor},
    },
};

impl Store {
    pub fn reconcile_self_memberships(
        &mut self,
        community_id: Uuid,
        joined: &std::collections::BTreeSet<Uuid>,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE channels SET is_member=0 WHERE community_id=?1",
            [community_id.to_string()],
        )?;
        for channel in joined {
            transaction.execute(
                "UPDATE channels SET is_member=1 WHERE community_id=?1 AND channel_id=?2",
                params![community_id.to_string(), channel.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn channels(&self, community_id: Uuid) -> Result<Vec<Channel>> {
        let mut statement = self.connection.prepare(
            "SELECT channel_id,name,about,visibility,is_member,is_hidden,member_count,last_event_at FROM channels WHERE community_id=?1 AND is_hidden=0 ORDER BY is_member DESC,name COLLATE NOCASE",
        )?;
        let values = statement
            .query_map([community_id.to_string()], |row| {
                Ok(Channel {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                    name: row.get(1)?,
                    about: row.get(2)?,
                    visibility: if row.get::<_, String>(3)? == "private" {
                        Visibility::Private
                    } else {
                        Visibility::Public
                    },
                    is_member: row.get(4)?,
                    is_hidden: row.get(5)?,
                    member_count: row.get(6)?,
                    last_event_at: row
                        .get::<_, Option<i64>>(7)?
                        .and_then(|value| u64::try_from(value).ok()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn channel_content_ids(&self, community_id: Uuid, channel_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT event_id FROM events WHERE community_id=?1 AND channel_id=?2 AND kind IN (9,40002,40099) ORDER BY created_at,event_id",
        )?;
        Ok(statement
            .query_map(
                params![community_id.to_string(), channel_id.to_string()],
                |row| row.get(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn messages(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        limit: usize,
    ) -> Result<Vec<Message>> {
        self.message_query(
            "SELECT e.event_id,e.channel_id,e.pubkey,e.created_at,e.content,e.root_event_id,e.parent_event_id,e.deleted_by_event_id,o.state,o.last_error_code,e.tags_json,c.http_base_url
             FROM events e JOIN communities c ON c.id=e.community_id LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
             WHERE e.community_id=?1 AND e.channel_id=?2 AND e.kind IN (9,40002,40099) AND e.root_event_id IS NULL
             ORDER BY e.created_at DESC,e.event_id DESC LIMIT ?3",
            params![community_id.to_string(),channel_id.to_string(),i64::try_from(limit).unwrap_or(i64::MAX)],
        ).map(|mut values| { values.reverse(); values })
    }

    pub fn thread(&self, community_id: Uuid, root: &str, limit: usize) -> Result<Vec<Message>> {
        self.message_query(
            "SELECT e.event_id,e.channel_id,e.pubkey,e.created_at,e.content,e.root_event_id,e.parent_event_id,e.deleted_by_event_id,o.state,o.last_error_code,e.tags_json,c.http_base_url
             FROM events e JOIN communities c ON c.id=e.community_id LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
             WHERE e.community_id=?1 AND e.kind IN (9,40002,40099) AND (e.event_id=?2 OR e.root_event_id=?2)
             ORDER BY e.created_at,e.event_id LIMIT ?3",
            params![community_id.to_string(),root,i64::try_from(limit).unwrap_or(i64::MAX)],
        )
    }

    fn message_query<P: rusqlite::Params>(&self, sql: &str, parameters: P) -> Result<Vec<Message>> {
        let mut statement = self.connection.prepare(sql)?;
        let values = statement
            .query_map(parameters, |row| {
                let state: Option<String> = row.get(8)?;
                let content: String = row.get(4)?;
                let tags_json: String = row.get(10)?;
                let http_base: String = row.get(11)?;
                let attachments = url::Url::parse(&http_base)
                    .ok()
                    .map(|base| crate::media::imeta::parse_tags(&tags_json, &content, &base))
                    .unwrap_or_default();
                let visible_content =
                    crate::media::imeta::strip_attachment_lines(&content, &attachments);
                Ok(Message {
                    event_id: row.get(0)?,
                    channel_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                    pubkey: row.get(2)?,
                    created_at: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    content: visible_content,
                    attachments,
                    root_event_id: row.get(5)?,
                    parent_event_id: row.get(6)?,
                    deleted: row.get::<_, Option<String>>(7)?.is_some(),
                    pending: matches!(state.as_deref(), Some("pending" | "unknown")),
                    rejected: if state.as_deref() == Some("rejected") {
                        row.get(9)?
                    } else {
                        None
                    },
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn reactions(&self, community_id: Uuid, target: &str) -> Result<Vec<Reaction>> {
        let mut statement = self.connection.prepare(
            "SELECT reaction_event_id,target_event_id,pubkey,emoji,created_at,deleted_by_event_id FROM reactions WHERE community_id=?1 AND target_event_id=?2 ORDER BY created_at,reaction_event_id",
        )?;
        Ok(statement
            .query_map(params![community_id.to_string(), target], |row| {
                Ok(Reaction {
                    event_id: row.get(0)?,
                    target_event_id: row.get(1)?,
                    pubkey: row.get(2)?,
                    emoji: row.get(3)?,
                    created_at: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    deleted: row.get::<_, Option<String>>(5)?.is_some(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn profile(&self, community_id: Uuid, pubkey: &str) -> Result<Option<Profile>> {
        self.connection.query_row(
            "SELECT pubkey,display_name,name,picture,nip05,about,event_id,created_at FROM profiles WHERE community_id=?1 AND pubkey=?2",
            params![community_id.to_string(),pubkey],
            |row| Ok(Profile { pubkey:row.get(0)?,display_name:row.get(1)?,name:row.get(2)?,picture:row.get(3)?,nip05:row.get(4)?,about:row.get(5)?,event_id:row.get(6)?,created_at:u64::try_from(row.get::<_,i64>(7)?).unwrap_or(0) }),
        ).optional().map_err(Into::into)
    }

    pub fn insert_outbox(&mut self, community_id: Uuid, event: &Event) -> Result<()> {
        // Conversation rows may appear optimistically. Destructive/auxiliary
        // events remain only in the durable outbox until relay acceptance or
        // an observed echo makes them authoritative.
        if matches!(event.kind.as_u16(), 9 | 40_002) {
            self.apply_event(community_id, event)?;
        }
        self.connection.execute(
            "INSERT INTO outbox(community_id,event_id,event_json,kind,channel_id,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'pending',unixepoch(),unixepoch())
             ON CONFLICT(community_id,event_id) DO NOTHING",
            params![community_id.to_string(),event.id.to_hex(),event.as_json(),i64::from(event.kind.as_u16()),crate::protocol::events::channel_id(event).map(|id|id.to_string())],
        )?;
        Ok(())
    }

    pub fn set_outbox_state(
        &self,
        community_id: Uuid,
        event_id: &str,
        state: OutboxState,
        error: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE outbox SET state=?3,last_error_code=?4,attempts=attempts+1,updated_at=unixepoch() WHERE community_id=?1 AND event_id=?2",
            params![community_id.to_string(),event_id,state.as_str(),error],
        )?;
        Ok(())
    }

    pub fn pending_outbox(&self, community_id: Uuid) -> Result<Vec<OutboxItem>> {
        let mut statement = self.connection.prepare("SELECT event_json,state,attempts,last_error_code FROM outbox WHERE community_id=?1 AND state IN ('pending','unknown') ORDER BY created_at")?;
        let rows = statement.query_map([community_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (json, state, attempts, last_error) = row?;
            result.push(OutboxItem {
                community_id,
                event: Event::from_json(json)
                    .map_err(|error| Error::Protocol(error.to_string()))?,
                state: OutboxState::parse(&state)
                    .ok_or_else(|| Error::Database(rusqlite::Error::InvalidQuery))?,
                attempts,
                last_error,
            });
        }
        Ok(result)
    }

    pub fn advance_read(
        &self,
        community_id: Uuid,
        pubkey: &str,
        context: &str,
        read_at: u32,
        publishable: bool,
    ) -> Result<u32> {
        self.connection.execute(
            "INSERT INTO read_contexts(community_id,identity_pubkey,context_id,read_at,publishable) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(community_id,identity_pubkey,context_id) DO UPDATE SET read_at=max(read_contexts.read_at,excluded.read_at),publishable=max(read_contexts.publishable,excluded.publishable)",
            params![community_id.to_string(),pubkey,context,i64::from(read_at),publishable],
        )?;
        self.connection.query_row("SELECT read_at FROM read_contexts WHERE community_id=?1 AND identity_pubkey=?2 AND context_id=?3",params![community_id.to_string(),pubkey,context],|row|row.get(0)).map_err(Into::into)
    }

    pub fn merge_read_contexts(
        &mut self,
        community_id: Uuid,
        pubkey: &str,
        contexts: &BTreeMap<String, u32>,
        source_created_at: u64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for (context, read_at) in contexts {
            if context.len() > 256 {
                continue;
            }
            transaction.execute(
                "INSERT INTO read_contexts(community_id,identity_pubkey,context_id,read_at,source_created_at,publishable) VALUES(?1,?2,?3,?4,?5,1)
                 ON CONFLICT(community_id,identity_pubkey,context_id) DO UPDATE SET read_at=max(read_contexts.read_at,excluded.read_at),source_created_at=max(read_contexts.source_created_at,excluded.source_created_at),publishable=1",
                params![community_id.to_string(),pubkey,context,i64::from(*read_at),u64_to_i64(source_created_at)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn read_contexts(
        &self,
        community_id: Uuid,
        pubkey: &str,
        publishable_only: bool,
    ) -> Result<BTreeMap<String, u32>> {
        let mut statement=self.connection.prepare("SELECT context_id,read_at FROM read_contexts WHERE community_id=?1 AND identity_pubkey=?2 AND (?3=0 OR publishable=1)")?;
        Ok(statement
            .query_map(
                params![community_id.to_string(), pubkey, publishable_only],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?)
    }

    pub fn sync_cursor(
        &self,
        community_id: Uuid,
        scope: &str,
        scope_id: &str,
    ) -> Result<SyncCursor> {
        self.connection.query_row(
            "SELECT high_created_at,high_event_id,complete_through FROM sync_cursors WHERE community_id=?1 AND scope=?2 AND scope_id=?3",
            params![community_id.to_string(),scope,scope_id],
            |row|Ok(SyncCursor{high_created_at:u64::try_from(row.get::<_,i64>(0)?).unwrap_or(0),high_event_id:row.get(1)?,complete_through:u64::try_from(row.get::<_,i64>(2)?).unwrap_or(0)}),
        ).optional().map(|value|value.unwrap_or(SyncCursor{high_created_at:0,high_event_id:String::new(),complete_through:0})).map_err(Into::into)
    }

    pub fn reset_sync_cursor(&self, community_id: Uuid, scope: &str, scope_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM sync_cursors WHERE community_id=?1 AND scope=?2 AND scope_id=?3",
            params![community_id.to_string(), scope, scope_id],
        )?;
        Ok(())
    }

    pub fn save_sync_cursor(
        &self,
        community_id: Uuid,
        scope: &str,
        scope_id: &str,
        cursor: &SyncCursor,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sync_cursors(community_id,scope,scope_id,high_created_at,high_event_id,complete_through,updated_at) VALUES(?1,?2,?3,?4,?5,?6,unixepoch())
             ON CONFLICT(community_id,scope,scope_id) DO UPDATE SET high_created_at=max(sync_cursors.high_created_at,excluded.high_created_at),high_event_id=CASE WHEN excluded.high_created_at>=sync_cursors.high_created_at THEN excluded.high_event_id ELSE sync_cursors.high_event_id END,complete_through=max(sync_cursors.complete_through,excluded.complete_through),updated_at=unixepoch()",
            params![community_id.to_string(),scope,scope_id,u64_to_i64(cursor.high_created_at)?,cursor.high_event_id,u64_to_i64(cursor.complete_through)?],
        )?;
        Ok(())
    }

    pub fn save_draft(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        root: Option<&str>,
        body: &str,
    ) -> Result<()> {
        self.save_draft_with_media(community_id, channel_id, root, body, &[])
    }

    pub fn save_draft_with_media(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        root: Option<&str>,
        body: &str,
        attachments: &[crate::media::DraftAttachment],
    ) -> Result<()> {
        let attachments = serde_json::to_string(attachments)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        self.connection.execute("INSERT INTO drafts(community_id,channel_id,thread_root_id,body,attachments_json,updated_at) VALUES(?1,?2,?3,?4,?5,unixepoch()) ON CONFLICT DO UPDATE SET body=excluded.body,attachments_json=excluded.attachments_json,updated_at=excluded.updated_at",params![community_id.to_string(),channel_id.to_string(),root.unwrap_or_default(),body,attachments])?;
        Ok(())
    }

    pub fn draft(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        root: Option<&str>,
    ) -> Result<String> {
        self.draft_with_media(community_id, channel_id, root)
            .map(|(body, _)| body)
    }

    pub fn draft_with_media(
        &self,
        community_id: Uuid,
        channel_id: Uuid,
        root: Option<&str>,
    ) -> Result<(String, Vec<crate::media::DraftAttachment>)> {
        let value = self.connection.query_row("SELECT body,attachments_json FROM drafts WHERE community_id=?1 AND channel_id=?2 AND thread_root_id=?3",params![community_id.to_string(),channel_id.to_string(),root.unwrap_or_default()],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).optional()?;
        let Some((body, attachments)) = value else {
            return Ok((String::new(), Vec::new()));
        };
        let attachments = serde_json::from_str(&attachments)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok((body, attachments))
    }

    pub fn record_media_cache(
        &self,
        community_id: Uuid,
        sha256: &str,
        mime: &str,
        byte_size: u64,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<()> {
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Protocol("invalid media cache hash".into()));
        }
        self.connection.execute(
            "INSERT INTO media_cache(community_id,sha256,variant,mime,byte_size,width,height,validated_at,last_accessed_at) VALUES(?1,?2,'original',?3,?4,?5,?6,unixepoch(),unixepoch()) ON CONFLICT DO UPDATE SET mime=excluded.mime,byte_size=excluded.byte_size,width=excluded.width,height=excluded.height,validated_at=unixepoch(),last_accessed_at=unixepoch()",
            params![community_id.to_string(),sha256,mime,u64_to_i64(byte_size)?,width.map(i64::from),height.map(i64::from)],
        )?;
        Ok(())
    }

    pub fn unread_channels(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<std::collections::HashSet<Uuid>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT e.channel_id
             FROM events e
             LEFT JOIN read_contexts r ON r.community_id=e.community_id
               AND r.identity_pubkey=?2 AND r.context_id=e.channel_id
             WHERE e.community_id=?1 AND e.channel_id IS NOT NULL
               AND e.kind IN (9,40002,40099) AND e.pubkey<>?2
               AND e.deleted_by_event_id IS NULL AND e.created_at>COALESCE(r.read_at,0)",
        )?;
        let values = statement
            .query_map(params![community_id.to_string(), identity_pubkey], |row| {
                let value: String = row.get(0)?;
                Ok(Uuid::parse_str(&value).ok())
            })?
            .filter_map(std::result::Result::transpose)
            .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
        Ok(values)
    }

    pub fn profiles(
        &self,
        community_id: Uuid,
    ) -> Result<std::collections::HashMap<String, Profile>> {
        let mut statement = self.connection.prepare(
            "SELECT pubkey,display_name,name,picture,nip05,about,event_id,created_at FROM profiles WHERE community_id=?1",
        )?;
        Ok(statement
            .query_map([community_id.to_string()], |row| {
                let profile = Profile {
                    pubkey: row.get(0)?,
                    display_name: row.get(1)?,
                    name: row.get(2)?,
                    picture: row.get(3)?,
                    nip05: row.get(4)?,
                    about: row.get(5)?,
                    event_id: row.get(6)?,
                    created_at: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
                };
                Ok((profile.pubkey.clone(), profile))
            })?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()?)
    }

    pub fn save_ui_state<T: serde::Serialize>(
        &self,
        community_id: Uuid,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let json = serde_json::to_string(value)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        self.connection.execute(
            "INSERT INTO ui_state(community_id,key,value_json) VALUES(?1,?2,?3) ON CONFLICT DO UPDATE SET value_json=excluded.value_json",
            params![community_id.to_string(), key, json],
        )?;
        Ok(())
    }

    pub fn ui_state<T: serde::de::DeserializeOwned>(
        &self,
        community_id: Uuid,
        key: &str,
    ) -> Result<Option<T>> {
        let json = self
            .connection
            .query_row(
                "SELECT value_json FROM ui_state WHERE community_id=?1 AND key=?2",
                params![community_id.to_string(), key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        json.map(|value| {
            serde_json::from_str(&value).map_err(|error| Error::Serialization(error.to_string()))
        })
        .transpose()
    }

    pub fn pin_relay_pubkey(&self, community_id: Uuid, pubkey: &str) -> Result<()> {
        if pubkey.len() != 64 || !pubkey.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Protocol("NIP-11 relay pubkey is invalid".into()));
        }
        let existing: Option<String> = self.connection.query_row(
            "SELECT relay_pubkey FROM communities WHERE id=?1",
            [community_id.to_string()],
            |row| row.get(0),
        )?;
        if let Some(existing) = existing {
            if existing != pubkey {
                return Err(Error::Access(format!(
                    "relay key changed from {existing} to {pubkey}; remove and re-add the community to trust it"
                )));
            }
            return Ok(());
        }
        self.connection.execute(
            "UPDATE communities SET relay_pubkey=?2,updated_at=unixepoch() WHERE id=?1",
            params![community_id.to_string(), pubkey],
        )?;
        Ok(())
    }

    pub fn ensure_local_read_slots(
        &self,
        community_id: Uuid,
        pubkey: &str,
        generated_client_id: &str,
        generated_slot_id: &str,
    ) -> Result<(String, Vec<String>, u64)> {
        let count: u32 = self.connection.query_row(
            "SELECT count(*) FROM read_slots WHERE community_id=?1 AND identity_pubkey=?2 AND is_local=1",
            params![community_id.to_string(), pubkey],
            |row| row.get(0),
        )?;
        if count == 0 {
            self.connection.execute(
                "INSERT INTO read_slots(community_id,identity_pubkey,slot_id,client_id,is_local) VALUES(?1,?2,?3,?4,1)",
                params![community_id.to_string(), pubkey, generated_slot_id, generated_client_id],
            )?;
        }
        let client_id: String = self.connection.query_row(
            "SELECT client_id FROM read_slots WHERE community_id=?1 AND identity_pubkey=?2 AND is_local=1 ORDER BY slot_id LIMIT 1",
            params![community_id.to_string(), pubkey],
            |row| row.get(0),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT slot_id,event_created_at FROM read_slots WHERE community_id=?1 AND identity_pubkey=?2 AND is_local=1 ORDER BY slot_id",
        )?;
        let rows = statement
            .query_map(params![community_id.to_string(), pubkey], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let max_seen: i64 = self.connection.query_row(
            "SELECT COALESCE(max(event_created_at),0) FROM read_slots WHERE community_id=?1 AND identity_pubkey=?2",
            params![community_id.to_string(), pubkey],
            |row| row.get(0),
        )?;
        Ok((
            client_id,
            rows.into_iter().map(|(slot, _)| slot).collect(),
            u64::try_from(max_seen).unwrap_or(0),
        ))
    }

    pub fn record_read_slot(&self, record: &ReadSlotRecord) -> Result<()> {
        self.connection.execute(
            "UPDATE read_slots SET is_local=0 WHERE community_id=?1 AND identity_pubkey=?2 AND slot_id=?3 AND is_local=1 AND client_id<>?4",
            params![record.community_id.to_string(), record.pubkey, record.slot_id, record.client_id],
        )?;
        self.connection.execute(
            "INSERT INTO read_slots(community_id,identity_pubkey,slot_id,client_id,event_id,event_created_at,is_local) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(community_id,identity_pubkey,slot_id) DO UPDATE SET client_id=excluded.client_id,event_id=excluded.event_id,event_created_at=max(read_slots.event_created_at,excluded.event_created_at),is_local=CASE WHEN read_slots.client_id=excluded.client_id THEN max(read_slots.is_local,excluded.is_local) ELSE excluded.is_local END",
            params![record.community_id.to_string(),record.pubkey,record.slot_id,record.client_id,record.event_id,u64_to_i64(record.event_created_at)?,record.local],
        )?;
        Ok(())
    }

    pub fn purge_community(&self, community_id: Uuid) -> Result<()> {
        self.connection.execute(
            "DELETE FROM communities WHERE id=?1",
            [community_id.to_string()],
        )?;
        Ok(())
    }
}
