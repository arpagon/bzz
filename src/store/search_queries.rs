use rusqlite::params;
use uuid::Uuid;

use crate::{
    domain::{SearchResult, SearchResultKind},
    error::Result,
    render::sanitize,
    store::{Store, models::MessageSearchQuery},
};

impl Store {
    pub fn search_messages(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        query: &MessageSearchQuery,
    ) -> Result<Vec<SearchResult>> {
        if query.fts_query.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self.connection.prepare(
            "SELECT sd.event_id,sd.channel_id,sd.pubkey,sd.created_at,sd.content,e.root_event_id,bm25(search_fts)
             FROM search_fts
             JOIN search_documents sd ON sd.rowid=search_fts.rowid
             JOIN events e ON e.community_id=sd.community_id AND e.event_id=sd.event_id
             JOIN channels c ON c.community_id=sd.community_id AND c.channel_id=sd.channel_id
             LEFT JOIN outbox o ON o.community_id=sd.community_id AND o.event_id=sd.event_id
             WHERE search_fts MATCH ?2 AND sd.community_id=?1
               AND c.is_member=1 AND e.deleted_by_event_id IS NULL
               AND COALESCE(o.state,'')<>'rejected'
               AND (?3 IS NULL OR sd.pubkey=?3)
               AND (?4 IS NULL OR sd.channel_id=?4)
               AND (?5 IS NULL OR sd.created_at>=?5)
               AND (?6 IS NULL OR sd.created_at<?6)
               AND NOT EXISTS(
                 SELECT 1 FROM dm_visibility v
                 WHERE c.channel_type='dm' AND v.community_id=sd.community_id
                   AND v.identity_pubkey=?7 AND v.channel_id=sd.channel_id
               )
             ORDER BY bm25(search_fts),sd.created_at DESC,sd.event_id ASC
             LIMIT ?8",
        )?;
        let author = query.author.as_deref();
        let channel = query.channel_id.map(|value| value.to_string());
        let since = query.since.and_then(|value| i64::try_from(value).ok());
        let until = query.until.and_then(|value| i64::try_from(value).ok());
        let limit = i64::try_from(query.limit.min(500)).unwrap_or(500);
        let values = statement
            .query_map(
                params![
                    community_id.to_string(),
                    query.fts_query,
                    author,
                    channel,
                    since,
                    until,
                    identity_pubkey,
                    limit,
                ],
                |row| {
                    let event_id: String = row.get(0)?;
                    let channel_id =
                        Uuid::parse_str(&row.get::<_, String>(1)?).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                1,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?;
                    let content: String = row.get(4)?;
                    Ok(SearchResult {
                        stable_id: format!("message:{event_id}"),
                        kind: SearchResultKind::Message,
                        label: sanitize::single_line(&content),
                        detail: crate::domain::abbreviated_pubkey(&row.get::<_, String>(2)?),
                        channel_id: Some(channel_id),
                        event_id: Some(event_id),
                        thread_root: row.get(5)?,
                        pubkey: Some(row.get(2)?),
                        created_at: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                        remote_rank: None,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn search_result_for_event(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        event_id: &str,
    ) -> Result<Option<SearchResult>> {
        let mut values = self.connection.prepare(
            "SELECT e.event_id,e.channel_id,e.pubkey,e.created_at,sd.content,e.root_event_id
             FROM events e JOIN search_documents sd ON sd.community_id=e.community_id AND sd.event_id=e.event_id
             JOIN channels c ON c.community_id=e.community_id AND c.channel_id=e.channel_id
             LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
             WHERE e.community_id=?1 AND e.event_id=?2 AND e.kind IN (9,40002)
               AND c.is_member=1 AND e.deleted_by_event_id IS NULL AND COALESCE(o.state,'')<>'rejected'
               AND NOT EXISTS(
                 SELECT 1 FROM dm_visibility v WHERE c.channel_type='dm'
                   AND v.community_id=e.community_id AND v.identity_pubkey=?3 AND v.channel_id=e.channel_id
               )",
        )?;
        let mut rows =
            values.query(params![community_id.to_string(), event_id, identity_pubkey])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let event_id: String = row.get(0)?;
        let channel_id = Uuid::parse_str(&row.get::<_, String>(1)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let content: String = row.get(4)?;
        Ok(Some(SearchResult {
            stable_id: format!("message:{event_id}"),
            kind: SearchResultKind::Message,
            label: sanitize::single_line(&content),
            detail: crate::domain::abbreviated_pubkey(&row.get::<_, String>(2)?),
            channel_id: Some(channel_id),
            event_id: Some(event_id),
            thread_root: row.get(5)?,
            pubkey: Some(row.get(2)?),
            created_at: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            remote_rank: None,
        }))
    }
}
