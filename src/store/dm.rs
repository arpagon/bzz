use std::collections::BTreeSet;

use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    store::Store,
};

impl Store {
    pub fn dm_participants_map(
        &self,
        community_id: Uuid,
    ) -> Result<std::collections::HashMap<Uuid, Vec<String>>> {
        let mut statement = self.connection.prepare(
            "SELECT m.channel_id,m.pubkey FROM memberships m JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1 AND c.channel_type='dm' ORDER BY m.channel_id,m.pubkey LIMIT 10000",
        )?;
        let rows = statement
            .query_map([community_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut values = std::collections::HashMap::<Uuid, Vec<String>>::new();
        for (channel, pubkey) in rows {
            if let Ok(channel) = Uuid::parse_str(&channel) {
                values.entry(channel).or_default().push(pubkey);
            }
        }
        Ok(values)
    }

    pub fn dm_participants(&self, community_id: Uuid, channel_id: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT m.pubkey FROM memberships m JOIN channels c ON c.community_id=m.community_id AND c.channel_id=m.channel_id
             WHERE m.community_id=?1 AND m.channel_id=?2 AND c.channel_type='dm'
             ORDER BY m.pubkey LIMIT 10",
        )?;
        Ok(statement
            .query_map(
                params![community_id.to_string(), channel_id.to_string()],
                |row| row.get(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn find_dm_by_participants(
        &self,
        community_id: Uuid,
        participants: &BTreeSet<String>,
    ) -> Result<Option<Uuid>> {
        if !(2..=9).contains(&participants.len())
            || participants.iter().any(|value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(Error::Config(
                "a workspace DM requires 2-9 valid unique participants".into(),
            ));
        }
        let mut statement = self.connection.prepare(
            "SELECT channel_id FROM channels WHERE community_id=?1 AND channel_type='dm' AND is_member=1 ORDER BY channel_id LIMIT 1000",
        )?;
        let channel_ids = statement
            .query_map([community_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for channel in channel_ids {
            let channel_id = Uuid::parse_str(&channel)
                .map_err(|_| Error::Database(rusqlite::Error::InvalidQuery))?;
            let members = self
                .dm_participants(community_id, channel_id)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            if &members == participants {
                return Ok(Some(channel_id));
            }
        }
        Ok(None)
    }

    pub fn hidden_dms(&self, community_id: Uuid, identity_pubkey: &str) -> Result<BTreeSet<Uuid>> {
        let mut statement = self.connection.prepare(
            "SELECT channel_id FROM dm_visibility WHERE community_id=?1 AND identity_pubkey=?2 ORDER BY channel_id LIMIT 10000",
        )?;
        let values = statement
            .query_map(params![community_id.to_string(), identity_pubkey], |row| {
                let value: String = row.get(0)?;
                Ok(Uuid::parse_str(&value).ok())
            })?
            .filter_map(std::result::Result::transpose)
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        Ok(values)
    }

    pub fn dm_visibility_head(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<Option<(u64, String)>> {
        self.connection
            .query_row(
                "SELECT source_created_at,source_event_id FROM dm_visibility_heads WHERE community_id=?1 AND identity_pubkey=?2",
                params![community_id.to_string(), identity_pubkey],
                |row| {
                    Ok((
                        u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                        row.get(1)?,
                    ))
                },
            )
            .optional()
            .map_err(Into::into)
    }
}
