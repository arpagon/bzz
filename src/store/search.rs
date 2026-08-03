use nostr::{Event, JsonUtil as _};
use rusqlite::{Transaction, params};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::events::tag_values,
    render::sanitize,
    store::{Store, events::u64_to_i64},
};

const PROJECTION_VERSION: &str = "1";
const MAX_SEARCHABLE_BYTES: usize = 64 * 1024;

pub(crate) fn project_event(
    transaction: &Transaction<'_>,
    community_id: Uuid,
    event: &Event,
    http_base: &str,
) -> Result<()> {
    let kind = event.kind.as_u16();
    if matches!(kind, 9 | 40_002) {
        for pubkey in tag_values(event, "p") {
            let pubkey = pubkey.to_ascii_lowercase();
            if valid_hex_32(&pubkey) {
                transaction.execute(
                    "INSERT OR IGNORE INTO event_mentions(community_id,event_id,mentioned_pubkey,created_at) VALUES(?1,?2,?3,?4)",
                    params![
                        community_id.to_string(),
                        event.id.to_hex(),
                        pubkey,
                        u64_to_i64(event.created_at.as_secs())?,
                    ],
                )?;
            }
        }
        let content = searchable_content(event, http_base);
        if !content.trim().is_empty() {
            transaction.execute(
                "INSERT INTO search_documents(community_id,event_id,channel_id,pubkey,kind,created_at,content)
                 SELECT ?1,?2,?3,?4,?5,?6,?7
                 WHERE NOT EXISTS(SELECT 1 FROM outbox WHERE community_id=?1 AND event_id=?2 AND state='rejected')
                 ON CONFLICT(community_id,event_id) DO UPDATE SET channel_id=excluded.channel_id,pubkey=excluded.pubkey,kind=excluded.kind,created_at=excluded.created_at,content=excluded.content",
                params![
                    community_id.to_string(),
                    event.id.to_hex(),
                    crate::protocol::events::channel_id(event).map(|value| value.to_string()).unwrap_or_default(),
                    event.pubkey.to_hex(),
                    i64::from(kind),
                    u64_to_i64(event.created_at.as_secs())?,
                    content,
                ],
            )?;
        }
    }
    Ok(())
}

fn searchable_content(event: &Event, http_base: &str) -> String {
    let tags_json = serde_json::to_string(&event.tags).unwrap_or_else(|_| "[]".into());
    let stripped = url::Url::parse(http_base)
        .ok()
        .map(|base| {
            let attachments = crate::media::imeta::parse_tags(&tags_json, &event.content, &base);
            crate::media::imeta::strip_attachment_lines(&event.content, &attachments)
        })
        .unwrap_or_else(|| event.content.clone());
    truncate_utf8(sanitize::text(&stripped), MAX_SEARCHABLE_BYTES)
}

fn truncate_utf8(mut value: String, maximum: usize) -> String {
    if value.len() <= maximum {
        return value;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.truncate(end);
    value
}

fn valid_hex_32(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Store {
    pub fn ensure_search_projections(&mut self) -> Result<()> {
        let projected = self
            .connection
            .query_row(
                "SELECT value FROM search_projection_meta WHERE key='version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .as_deref()
            == Some(PROJECTION_VERSION);
        if projected {
            return self.search_integrity();
        }
        self.rebuild_search_projections()
    }

    pub fn rebuild_search_projections(&mut self) -> Result<()> {
        let mut statement = self.connection.prepare(
            "SELECT e.community_id,e.raw_json,c.http_base_url
             FROM events e JOIN communities c ON c.id=e.community_id
             LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
             WHERE e.kind IN (9,40002) AND e.deleted_by_event_id IS NULL
               AND COALESCE(o.state,'')<>'rejected'
             ORDER BY e.community_id,e.created_at,e.event_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM event_mentions", [])?;
        transaction.execute("DELETE FROM search_documents", [])?;
        for (community, raw, http_base) in rows {
            let community_id = Uuid::parse_str(&community).map_err(|error| {
                Error::Protocol(format!("invalid stored community UUID: {error}"))
            })?;
            let event = Event::from_json(raw)
                .map_err(|error| Error::Protocol(format!("invalid stored event: {error}")))?;
            project_event(&transaction, community_id, &event, &http_base)?;
        }
        transaction.execute(
            "INSERT INTO search_projection_meta(key,value) VALUES('version',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [PROJECTION_VERSION],
        )?;
        transaction.commit()?;
        self.search_integrity()
    }

    pub fn search_integrity(&self) -> Result<()> {
        self.connection.execute(
            "INSERT INTO search_fts(search_fts) VALUES('integrity-check')",
            [],
        )?;
        Ok(())
    }
}
