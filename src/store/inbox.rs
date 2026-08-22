use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension as _, Transaction, params};
use uuid::Uuid;

use crate::{
    domain::{InboxCategory, InboxCursor, InboxItem, InboxPage, Message},
    error::{Error, Result},
    render::sanitize,
    store::{Store, events::u64_to_i64},
};

/// No single active conversation can crowd every other conversation out of a
/// local rebuild. The bound is also the maximum persisted event window.
const EVENT_WINDOW_CAP: usize = 64;
const INBOX_CAP: usize = 500;
const EVENT_SCAN_CAP: usize = INBOX_CAP * EVENT_WINDOW_CAP;
const DRAFT_SCAN_CAP: usize = 500;
const PARTICIPATED_SCAN_CAP: usize = 10_000;

#[derive(Debug)]
struct Candidate {
    event_id: String,
    kind: u32,
    pubkey: String,
    created_at: u64,
    channel_id: Option<Uuid>,
    channel_type: String,
    content: String,
    tags_json: String,
    http_base: String,
    root: Option<String>,
    mentioned: bool,
}

#[derive(Clone, Debug)]
struct ProjectionEvent {
    event_id: String,
    created_at: u64,
}

#[derive(Debug)]
struct DerivedProjection {
    items: Vec<InboxItem>,
    windows: BTreeMap<String, Vec<ProjectionEvent>>,
}

impl Store {
    /// Return the first bounded page of the rebuildable local projection.
    /// Existing callers retain this convenience API while M4 can use
    /// [`Self::inbox_page`] for cursor pagination.
    pub fn inbox_items(&self, community_id: Uuid, identity_pubkey: &str) -> Result<Vec<InboxItem>> {
        Ok(self
            .inbox_page(community_id, identity_pubkey, None, INBOX_CAP)?
            .items)
    }

    /// Read a projection page in `(latest_activity_at DESC, conversation_id
    /// ASC)` order. The cursor is entirely local and never changes relay
    /// queries or authoritative event storage.
    pub fn inbox_page(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        cursor: Option<&InboxCursor>,
        limit: usize,
    ) -> Result<InboxPage> {
        if !self.inbox_identity_matches(community_id, identity_pubkey)? {
            return Ok(InboxPage {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        self.ensure_inbox_projection(community_id, identity_pubkey)?;
        self.read_inbox_projection_page(community_id, identity_pubkey, cursor, limit)
    }

    /// Bounded event IDs retained for one visible conversation. Detail still
    /// loads bodies from `events`; this table intentionally contains no body.
    pub fn inbox_conversation_event_ids(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        conversation_id: &str,
    ) -> Result<Vec<String>> {
        if !self.inbox_identity_matches(community_id, identity_pubkey)?
            || !valid_conversation_id(conversation_id)
        {
            return Ok(Vec::new());
        }
        let mut statement = self.connection.prepare(
            "SELECT event_id FROM inbox_conversation_events
             WHERE community_id=?1 AND identity_pubkey=?2 AND conversation_id=?3
             ORDER BY created_at DESC,event_id ASC LIMIT ?4",
        )?;
        statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    conversation_id,
                    i64::try_from(EVENT_WINDOW_CAP).unwrap_or(64),
                ],
                |row| row.get(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Load a bounded, still-authorized context around the projection's stable
    /// unread anchor (or latest event). Event bodies remain authoritative
    /// `events` rows; this does not turn the Inbox projection into a store.
    pub fn inbox_conversation_context(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        conversation_id: &str,
    ) -> Result<Vec<Message>> {
        if !self.inbox_identity_matches(community_id, identity_pubkey)?
            || !valid_conversation_id(conversation_id)
        {
            return Ok(Vec::new());
        }
        let row = self
            .connection
            .query_row(
                "SELECT p.channel_id,p.thread_root_id,
                    COALESCE(p.first_unread_event_id,p.latest_event_id)
             FROM inbox_conversations p
             JOIN channels c ON c.community_id=p.community_id AND c.channel_id=p.channel_id
             WHERE p.community_id=?1 AND p.identity_pubkey=?2 AND p.conversation_id=?3
               AND c.is_member=1
               AND NOT EXISTS(
                 SELECT 1 FROM dm_visibility v WHERE c.channel_type='dm'
                   AND v.community_id=p.community_id AND v.identity_pubkey=?2
                   AND v.channel_id=p.channel_id
               )",
                params![community_id.to_string(), identity_pubkey, conversation_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((Some(channel), root, Some(anchor))) = row else {
            return Ok(Vec::new());
        };
        let Ok(channel_id) = Uuid::parse_str(&channel) else {
            return Ok(Vec::new());
        };
        if !valid_event_id(&anchor) || root.as_ref().is_some_and(|value| !valid_event_id(value)) {
            return Ok(Vec::new());
        }
        match root {
            Some(root) => {
                self.thread_around(community_id, channel_id, &root, &anchor, EVENT_WINDOW_CAP)
            }
            None => self.messages_around(community_id, channel_id, &anchor, EVENT_WINDOW_CAP),
        }
    }

    pub(crate) fn mark_inbox_projection_dirty(&self, community_id: Uuid) -> Result<()> {
        self.connection.execute(
            "INSERT INTO inbox_projection_meta(community_id,identity_pubkey,dirty)
             SELECT c.id,i.pubkey,1 FROM communities c JOIN identities i ON i.id=c.identity_id
             WHERE c.id=?1
             ON CONFLICT(community_id,identity_pubkey) DO UPDATE SET dirty=1",
            [community_id.to_string()],
        )?;
        Ok(())
    }

    pub(crate) fn mark_inbox_projection_dirty_for_identity(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO inbox_projection_meta(community_id,identity_pubkey,dirty)
             SELECT c.id,i.pubkey,1 FROM communities c JOIN identities i ON i.id=c.identity_id
             WHERE c.id=?1 AND i.pubkey=?2
             ON CONFLICT(community_id,identity_pubkey) DO UPDATE SET dirty=1",
            params![community_id.to_string(), identity_pubkey],
        )?;
        Ok(())
    }

    fn inbox_identity_matches(&self, community_id: Uuid, identity_pubkey: &str) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS(
               SELECT 1 FROM communities c JOIN identities i ON i.id=c.identity_id
               WHERE c.id=?1 AND i.pubkey=?2
             )",
                params![community_id.to_string(), identity_pubkey],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn ensure_inbox_projection(&self, community_id: Uuid, identity_pubkey: &str) -> Result<()> {
        let dirty = self.connection.query_row(
            "SELECT dirty FROM inbox_projection_meta WHERE community_id=?1 AND identity_pubkey=?2",
            params![community_id.to_string(), identity_pubkey],
            |row| row.get::<_, bool>(0),
        ).optional()?;
        if dirty.unwrap_or(true) {
            self.rebuild_inbox_projection(community_id, identity_pubkey)?;
        }
        Ok(())
    }

    fn rebuild_inbox_projection(&self, community_id: Uuid, identity_pubkey: &str) -> Result<()> {
        let overrides = self.inbox_overrides(community_id, identity_pubkey)?;
        let derived = self.derive_inbox_projection(community_id, identity_pubkey, &overrides)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM inbox_conversations WHERE community_id=?1 AND identity_pubkey=?2",
            params![community_id.to_string(), identity_pubkey],
        )?;
        for item in &derived.items {
            let categories = serde_json::to_string(&item.categories)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let local_done_at = overrides
                .get(&item.conversation_id)
                .and_then(|(_, done)| *done)
                .map(u64_to_i64)
                .transpose()?;
            transaction.execute(
                "INSERT INTO inbox_conversations(
                   community_id,identity_pubkey,conversation_id,latest_event_id,latest_activity_at,
                   first_unread_event_id,first_unread_at,unread_count,categories_json,channel_id,
                   thread_root_id,sender_pubkey,preview,draft_count,latest_draft_at,forced_unread,local_done_at
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    item.conversation_id,
                    item.event_id,
                    u64_to_i64(item.created_at)?,
                    item.first_unread_event_id,
                    item.first_unread_at.map(u64_to_i64).transpose()?,
                    i64::from(item.unread_count),
                    categories,
                    item.channel_id.map(|id| id.to_string()),
                    item.thread_root,
                    item.sender_pubkey,
                    bounded_preview(&item.preview),
                    i64::from(item.draft_count),
                    item.latest_draft_at.map(u64_to_i64).transpose()?,
                    item.forced_unread,
                    local_done_at,
                ],
            )?;
            if let Some(events) = derived.windows.get(&item.conversation_id) {
                for event in events.iter().take(EVENT_WINDOW_CAP) {
                    transaction.execute(
                        "INSERT INTO inbox_conversation_events(
                           community_id,identity_pubkey,conversation_id,event_id,created_at
                         ) VALUES(?1,?2,?3,?4,?5)",
                        params![
                            community_id.to_string(),
                            identity_pubkey,
                            item.conversation_id,
                            event.event_id,
                            u64_to_i64(event.created_at)?,
                        ],
                    )?;
                }
            }
        }
        transaction.execute(
            "INSERT INTO inbox_projection_meta(community_id,identity_pubkey,dirty,rebuilt_at)
             VALUES(?1,?2,0,unixepoch())
             ON CONFLICT(community_id,identity_pubkey) DO UPDATE SET dirty=0,rebuilt_at=excluded.rebuilt_at",
            params![community_id.to_string(), identity_pubkey],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn read_inbox_projection_page(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        cursor: Option<&InboxCursor>,
        limit: usize,
    ) -> Result<InboxPage> {
        let limit = limit.clamp(1, INBOX_CAP);
        let cursor_at = cursor
            .map(|value| u64_to_i64(value.latest_activity_at))
            .transpose()?;
        let cursor_id = cursor.map(|value| value.conversation_id.as_str());
        let mut statement = self.connection.prepare(
            "SELECT conversation_id,latest_event_id,latest_activity_at,first_unread_event_id,
                    first_unread_at,unread_count,categories_json,channel_id,thread_root_id,
                    sender_pubkey,preview,draft_count,latest_draft_at,forced_unread
             FROM inbox_conversations
             WHERE community_id=?1 AND identity_pubkey=?2
               AND (?3 IS NULL OR latest_activity_at<?3
                    OR (latest_activity_at=?3 AND conversation_id>?4))
             ORDER BY latest_activity_at DESC,conversation_id ASC LIMIT ?5",
        )?;
        let rows = statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    cursor_at,
                    cursor_id,
                    i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX),
                ],
                projection_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut items = rows.into_iter().flatten().collect::<Vec<_>>();
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = has_more
            .then(|| {
                items.last().map(|item| InboxCursor {
                    latest_activity_at: item.created_at,
                    conversation_id: item.conversation_id.clone(),
                })
            })
            .flatten();
        Ok(InboxPage { items, next_cursor })
    }

    fn derive_inbox_projection(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        overrides: &BTreeMap<String, (bool, Option<u64>)>,
    ) -> Result<DerivedProjection> {
        let read = self.read_contexts(community_id, identity_pubkey, false)?;
        let participated = self.participated_roots(community_id, identity_pubkey)?;
        let mut statement = self.connection.prepare(
            "WITH candidate_rows AS (
               SELECT e.event_id,e.kind,e.pubkey,e.created_at,e.channel_id,
                      COALESCE(c.channel_type,'') AS channel_type,
                      e.content,e.tags_json,e.root_event_id,
                      EXISTS(SELECT 1 FROM event_mentions m WHERE m.community_id=e.community_id
                        AND m.event_id=e.event_id AND m.mentioned_pubkey=?2) AS mentioned,
                      co.http_base_url,
                      CASE
                        WHEN c.channel_type='dm' THEN 'dm:' || COALESCE(e.channel_id,'')
                        WHEN e.root_event_id IS NOT NULL
                          AND instr(e.tags_json,'\"broadcast\",\"1\"')=0
                          THEN 'thread:' || e.root_event_id
                        ELSE 'event:' || e.event_id
                      END AS window_key
               FROM events e
               JOIN communities co ON co.id=e.community_id
               LEFT JOIN channels c ON c.community_id=e.community_id AND c.channel_id=e.channel_id
               LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
               WHERE e.community_id=?1 AND e.kind IN (9,40002,46010,46011,46012)
                 AND e.deleted_by_event_id IS NULL
                 AND (o.event_id IS NULL OR o.state='delivered')
                 AND (e.channel_id IS NULL OR c.is_member=1)
                 AND NOT EXISTS(
                   SELECT 1 FROM dm_visibility v WHERE c.channel_type='dm'
                     AND v.community_id=e.community_id AND v.identity_pubkey=?2
                     AND v.channel_id=e.channel_id
                 )
                 AND (
                   c.channel_type='dm'
                   OR EXISTS(SELECT 1 FROM event_mentions m WHERE m.community_id=e.community_id
                     AND m.event_id=e.event_id AND m.mentioned_pubkey=?2)
                   OR (e.kind IN (46010,46011,46012)
                     AND instr(e.tags_json,'\"p\",\"' || ?2 || '\"')>0)
                   OR (e.kind IN (9,40002) AND e.root_event_id IS NOT NULL AND EXISTS(
                     SELECT 1 FROM events mine
                     WHERE mine.community_id=e.community_id AND mine.pubkey=?2
                       AND (mine.event_id=e.root_event_id OR mine.root_event_id=e.root_event_id)
                   ))
                 )
             ), windowed AS (
               SELECT *,ROW_NUMBER() OVER(
                 PARTITION BY window_key ORDER BY created_at DESC,event_id ASC
               ) AS window_rank
               FROM candidate_rows
             )
             SELECT event_id,kind,pubkey,created_at,channel_id,channel_type,content,tags_json,
                    root_event_id,mentioned,http_base_url
             FROM windowed WHERE window_rank<=?3
             ORDER BY created_at DESC,event_id ASC LIMIT ?4",
        )?;
        let candidates = statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    i64::try_from(EVENT_WINDOW_CAP).unwrap_or(64),
                    i64::try_from(EVENT_SCAN_CAP.saturating_add(1)).unwrap_or(i64::MAX),
                ],
                |row| {
                    let channel_id = row
                        .get::<_, Option<String>>(4)?
                        .and_then(|value| Uuid::parse_str(&value).ok());
                    Ok(Candidate {
                        event_id: row.get(0)?,
                        kind: u32::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                        pubkey: row.get(2)?,
                        created_at: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                        channel_id,
                        channel_type: row.get(5)?,
                        content: row.get(6)?,
                        tags_json: row.get(7)?,
                        root: row.get(8)?,
                        mentioned: row.get(9)?,
                        http_base: row.get(10)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        if candidates.len() > EVENT_SCAN_CAP {
            return Err(Error::Config(format!(
                "inbox projection exceeded the {EVENT_SCAN_CAP}-event safety cap"
            )));
        }

        let mut grouped = BTreeMap::<String, InboxItem>::new();
        let mut windows = BTreeMap::<String, Vec<ProjectionEvent>>::new();
        for candidate in candidates {
            let is_action = matches!(candidate.kind, 46_010..=46_012);
            if is_action && !json_tag_contains(&candidate.tags_json, "p", identity_pubkey) {
                continue;
            }
            let dm = candidate.channel_type == "dm";
            let broadcast = json_tag_contains(&candidate.tags_json, "broadcast", "1");
            let thread_root = (!broadcast).then(|| candidate.root.clone()).flatten();
            let relevant_thread = thread_root
                .as_ref()
                .is_some_and(|root| candidate.mentioned || participated.contains(root));
            if !dm && !candidate.mentioned && !relevant_thread && !is_action {
                continue;
            }
            let conversation_id = if dm {
                let Some(channel_id) = candidate.channel_id else {
                    continue;
                };
                format!("dm:{channel_id}")
            } else if let Some(root) = &thread_root {
                format!("thread:{root}")
            } else {
                format!("event:{}", candidate.event_id)
            };
            if !valid_conversation_id(&conversation_id) {
                continue;
            }
            let mut categories = Vec::new();
            if candidate.mentioned {
                categories.push(InboxCategory::Mention);
            }
            if relevant_thread {
                categories.push(InboxCategory::Thread);
            }
            if dm {
                categories.push(InboxCategory::Dm);
            }
            if is_action {
                categories.push(InboxCategory::NeedsAction);
            }
            let unread = candidate.pubkey != identity_pubkey
                && candidate.created_at
                    > effective_read_at(
                        &read,
                        candidate.channel_id,
                        thread_root.as_deref(),
                        &candidate.event_id,
                    );
            let override_value = overrides.get(&conversation_id);
            let locally_read = override_value
                .and_then(|(_, done)| *done)
                .is_some_and(|done| done >= candidate.created_at);
            let forced_unread = override_value.is_some_and(|(forced, _)| *forced);
            let entry = grouped
                .entry(conversation_id.clone())
                .or_insert_with(|| InboxItem {
                    conversation_id: conversation_id.clone(),
                    categories: Vec::new(),
                    event_id: Some(candidate.event_id.clone()),
                    channel_id: candidate.channel_id,
                    thread_root: thread_root.clone(),
                    sender_pubkey: Some(candidate.pubkey.clone()),
                    created_at: candidate.created_at,
                    preview: bounded_event_preview(
                        &candidate.content,
                        &candidate.tags_json,
                        &candidate.http_base,
                    ),
                    unread_count: 0,
                    first_unread_event_id: None,
                    first_unread_at: None,
                    draft_count: 0,
                    latest_draft_at: None,
                    forced_unread,
                });
            for category in categories {
                if !entry.categories.contains(&category) {
                    entry.categories.push(category);
                }
            }
            if unread && !locally_read {
                entry.unread_count = entry.unread_count.saturating_add(1);
                let replace_anchor = entry.first_unread_at.is_none_or(|current| {
                    candidate.created_at < current
                        || (candidate.created_at == current
                            && candidate.event_id
                                < entry.first_unread_event_id.clone().unwrap_or_default())
                });
                if replace_anchor {
                    entry.first_unread_at = Some(candidate.created_at);
                    entry.first_unread_event_id = Some(candidate.event_id.clone());
                }
            }
            entry.forced_unread |= forced_unread;
            if candidate.created_at > entry.created_at
                || (candidate.created_at == entry.created_at
                    && candidate.event_id < entry.event_id.clone().unwrap_or_default())
            {
                entry.event_id = Some(candidate.event_id.clone());
                entry.channel_id = candidate.channel_id;
                entry.thread_root = thread_root;
                entry.sender_pubkey = Some(candidate.pubkey.clone());
                entry.created_at = candidate.created_at;
                entry.preview = bounded_event_preview(
                    &candidate.content,
                    &candidate.tags_json,
                    &candidate.http_base,
                );
            }
            let window = windows.entry(conversation_id).or_default();
            if window.len() < EVENT_WINDOW_CAP
                && !window
                    .iter()
                    .any(|event| event.event_id == candidate.event_id)
            {
                window.push(ProjectionEvent {
                    event_id: candidate.event_id,
                    created_at: candidate.created_at,
                });
            }
        }
        self.add_drafts(community_id, identity_pubkey, overrides, &mut grouped)?;
        for item in grouped.values_mut() {
            let local_done_at = overrides
                .get(&item.conversation_id)
                .and_then(|(_, done)| *done);
            let (count, first_event_id, first_at) =
                self.unread_summary(community_id, identity_pubkey, &read, item, local_done_at)?;
            item.unread_count = count;
            item.first_unread_event_id = first_event_id;
            item.first_unread_at = first_at;
        }
        let mut items = grouped.into_values().collect::<Vec<_>>();
        for item in &mut items {
            item.categories.sort_by_key(category_order);
        }
        items.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        items.truncate(INBOX_CAP);
        let retained = items
            .iter()
            .map(|item| item.conversation_id.as_str())
            .collect::<BTreeSet<_>>();
        windows.retain(|conversation_id, events| {
            events.sort_by(|left, right| {
                right
                    .created_at
                    .cmp(&left.created_at)
                    .then_with(|| left.event_id.cmp(&right.event_id))
            });
            events.truncate(EVENT_WINDOW_CAP);
            retained.contains(conversation_id.as_str())
        });
        Ok(DerivedProjection { items, windows })
    }

    fn unread_summary(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        read: &BTreeMap<String, u32>,
        item: &InboxItem,
        local_done_at: Option<u64>,
    ) -> Result<(u32, Option<String>, Option<u64>)> {
        let (scope, scope_value) = if let Some(channel) = item.conversation_id.strip_prefix("dm:") {
            ("e.channel_id=?3", channel.to_owned())
        } else if let Some(root) = item.conversation_id.strip_prefix("thread:") {
            (
                "(e.event_id=?3 OR (e.root_event_id=?3 AND instr(e.tags_json,'\"broadcast\",\"1\"')=0))",
                root.to_owned(),
            )
        } else if let Some(event_id) = item.conversation_id.strip_prefix("event:") {
            ("e.event_id=?3", event_id.to_owned())
        } else {
            return Ok((0, None, None));
        };
        let channel_read = item
            .channel_id
            .and_then(|channel| read.get(&channel.to_string()).copied())
            .unwrap_or(0);
        let thread_read = item
            .thread_root
            .as_ref()
            .and_then(|root| read.get(&format!("thread:{root}")).copied())
            .unwrap_or(0);
        let local_done_at = local_done_at.unwrap_or(0);
        let where_clause = format!(
            "e.community_id=?1 AND e.pubkey<>?2 AND {scope}
             AND e.kind IN (9,40002,46010,46011,46012) AND e.deleted_by_event_id IS NULL
             AND (o.event_id IS NULL OR o.state='delivered')
             AND e.created_at>MAX(?4,?5,
               COALESCE((SELECT read_at FROM read_contexts r
                 WHERE r.community_id=e.community_id AND r.identity_pubkey=?2
                   AND r.context_id='msg:' || e.event_id),0),?6)"
        );
        let count: i64 = self.connection.query_row(
            &format!("SELECT COUNT(*) FROM events e LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id WHERE {where_clause}"),
            params![
                community_id.to_string(),
                identity_pubkey,
                scope_value,
                i64::from(channel_read),
                i64::from(thread_read),
                u64_to_i64(local_done_at)?,
            ],
            |row| row.get(0),
        )?;
        if count <= 0 {
            return Ok((0, None, None));
        }
        let first = self.connection.query_row(
            &format!("SELECT e.event_id,e.created_at FROM events e LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id WHERE {where_clause} ORDER BY e.created_at,e.event_id LIMIT 1"),
            params![
                community_id.to_string(),
                identity_pubkey,
                scope_value,
                i64::from(channel_read),
                i64::from(thread_read),
                u64_to_i64(local_done_at)?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        Ok((
            u32::try_from(count).unwrap_or(u32::MAX),
            Some(first.0),
            u64::try_from(first.1).ok(),
        ))
    }

    fn participated_roots(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<BTreeSet<String>> {
        let mut statement = self.connection.prepare(
            "SELECT conversation_root FROM (
               SELECT CASE WHEN root_event_id IS NULL THEN event_id ELSE root_event_id END
                        AS conversation_root,
                      MAX(created_at) AS latest_activity_at
               FROM events WHERE community_id=?1 AND pubkey=?2 AND kind IN (9,40002)
               GROUP BY CASE WHEN root_event_id IS NULL THEN event_id ELSE root_event_id END
               ORDER BY latest_activity_at DESC,conversation_root ASC LIMIT ?3
             )",
        )?;
        let values = statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    i64::try_from(PARTICIPATED_SCAN_CAP.saturating_add(1)).unwrap_or(i64::MAX),
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if values.len() > PARTICIPATED_SCAN_CAP {
            return Err(Error::Config(format!(
                "inbox projection exceeded the {PARTICIPATED_SCAN_CAP}-thread participation safety cap"
            )));
        }
        Ok(values.into_iter().collect())
    }

    fn inbox_overrides(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<BTreeMap<String, (bool, Option<u64>)>> {
        let mut statement = self.connection.prepare(
            "SELECT conversation_id,forced_unread,local_done_at FROM inbox_overrides
             WHERE community_id=?1 AND identity_pubkey=?2 ORDER BY conversation_id LIMIT 1000",
        )?;
        statement
            .query_map(params![community_id.to_string(), identity_pubkey], |row| {
                Ok((
                    row.get(0)?,
                    (
                        row.get(1)?,
                        row.get::<_, Option<i64>>(2)?
                            .and_then(|value| u64::try_from(value).ok()),
                    ),
                ))
            })?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()
            .map_err(Into::into)
    }

    fn add_drafts(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        overrides: &BTreeMap<String, (bool, Option<u64>)>,
        grouped: &mut BTreeMap<String, InboxItem>,
    ) -> Result<()> {
        let mut statement = self.connection.prepare(
            "SELECT d.channel_id,d.thread_root_id,d.body,d.attachments_json,d.updated_at,COALESCE(c.channel_type,'')
             FROM drafts d JOIN channels c ON c.community_id=d.community_id AND c.channel_id=d.channel_id
             WHERE d.community_id=?1 AND d.state='editing'
               AND (length(d.body)>0 OR d.attachments_json<>'[]')
               AND c.is_member=1 AND NOT EXISTS(
                 SELECT 1 FROM dm_visibility v WHERE c.channel_type='dm'
                   AND v.community_id=d.community_id AND v.identity_pubkey=?2 AND v.channel_id=d.channel_id
               )
             ORDER BY d.updated_at DESC,d.channel_id,d.thread_root_id LIMIT ?3",
        )?;
        let drafts = statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    i64::try_from(DRAFT_SCAN_CAP.saturating_add(1)).unwrap_or(i64::MAX),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        row.get::<_, String>(5)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if drafts.len() > DRAFT_SCAN_CAP {
            return Err(Error::Config(format!(
                "inbox projection exceeded the {DRAFT_SCAN_CAP}-draft safety cap"
            )));
        }
        for (channel, root, body, updated_at, channel_type) in drafts {
            let Ok(channel_id) = Uuid::parse_str(&channel) else {
                continue;
            };
            let conversation_id = if channel_type == "dm" {
                format!("dm:{channel}")
            } else if valid_event_id(&root) {
                format!("thread:{root}")
            } else if root.is_empty() {
                format!("draft:{channel}")
            } else {
                continue;
            };
            let forced_unread = overrides
                .get(&conversation_id)
                .is_some_and(|(forced, _)| *forced);
            let entry = grouped
                .entry(conversation_id.clone())
                .or_insert_with(|| InboxItem {
                    conversation_id,
                    categories: Vec::new(),
                    event_id: None,
                    channel_id: Some(channel_id),
                    thread_root: (!root.is_empty()).then_some(root.clone()),
                    sender_pubkey: None,
                    created_at: updated_at,
                    preview: bounded_preview(&body),
                    unread_count: 0,
                    first_unread_event_id: None,
                    first_unread_at: None,
                    draft_count: 0,
                    latest_draft_at: None,
                    forced_unread,
                });
            if !entry.categories.contains(&InboxCategory::Draft) {
                entry.categories.push(InboxCategory::Draft);
            }
            entry.draft_count = entry.draft_count.saturating_add(1);
            entry.latest_draft_at = Some(entry.latest_draft_at.unwrap_or(0).max(updated_at));
            if updated_at > entry.created_at {
                entry.created_at = updated_at;
                entry.preview = bounded_preview(&body);
            }
        }
        Ok(())
    }

    pub fn set_inbox_override(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
        conversation_id: &str,
        forced_unread: bool,
        local_done_at: Option<u64>,
    ) -> Result<()> {
        if !self.inbox_identity_matches(community_id, identity_pubkey)?
            || !valid_conversation_id(conversation_id)
        {
            return Ok(());
        }
        self.connection.execute(
            "INSERT INTO inbox_overrides(community_id,identity_pubkey,conversation_id,forced_unread,local_done_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,unixepoch())
             ON CONFLICT(community_id,identity_pubkey,conversation_id) DO UPDATE SET
               forced_unread=excluded.forced_unread,
               local_done_at=COALESCE(excluded.local_done_at,inbox_overrides.local_done_at),
               updated_at=unixepoch()",
            params![
                community_id.to_string(),
                identity_pubkey,
                conversation_id,
                forced_unread,
                local_done_at.map(u64_to_i64).transpose()?,
            ],
        )?;
        self.mark_inbox_projection_dirty_for_identity(community_id, identity_pubkey)
    }
}

pub(crate) fn mark_projection_dirty(
    transaction: &Transaction<'_>,
    community_id: Uuid,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO inbox_projection_meta(community_id,identity_pubkey,dirty)
         SELECT c.id,i.pubkey,1 FROM communities c JOIN identities i ON i.id=c.identity_id
         WHERE c.id=?1
         ON CONFLICT(community_id,identity_pubkey) DO UPDATE SET dirty=1",
        [community_id.to_string()],
    )?;
    Ok(())
}

fn projection_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<InboxItem>> {
    let conversation_id: String = row.get(0)?;
    let event_id: Option<String> = row.get(1)?;
    let created_at = u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0);
    let first_unread_event_id: Option<String> = row.get(3)?;
    let first_unread_at = row
        .get::<_, Option<i64>>(4)?
        .and_then(|value| u64::try_from(value).ok());
    let categories = row
        .get::<_, String>(6)
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<InboxCategory>>(&value).ok());
    let Some(categories) = categories else {
        return Ok(None);
    };
    let channel_id = row
        .get::<_, Option<String>>(7)?
        .and_then(|value| Uuid::parse_str(&value).ok());
    let thread_root: Option<String> = row.get(8)?;
    let sender_pubkey: Option<String> = row.get(9)?;
    let preview: String = row.get(10)?;
    let latest_draft_at = row
        .get::<_, Option<i64>>(12)?
        .and_then(|value| u64::try_from(value).ok());
    if !valid_conversation_id(&conversation_id)
        || event_id
            .as_ref()
            .is_some_and(|value| !valid_event_id(value))
        || first_unread_event_id
            .as_ref()
            .is_some_and(|value| !valid_event_id(value))
        || thread_root
            .as_ref()
            .is_some_and(|value| !valid_event_id(value))
        || sender_pubkey
            .as_ref()
            .is_some_and(|value| !valid_pubkey(value))
    {
        return Ok(None);
    }
    Ok(Some(InboxItem {
        conversation_id,
        categories,
        event_id,
        channel_id,
        thread_root,
        sender_pubkey,
        created_at,
        preview: bounded_preview(&preview),
        unread_count: row
            .get::<_, i64>(5)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        first_unread_event_id,
        first_unread_at,
        draft_count: row
            .get::<_, i64>(11)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        latest_draft_at,
        forced_unread: row.get::<_, i64>(13).unwrap_or(0) == 1,
    }))
}

fn effective_read_at(
    read: &BTreeMap<String, u32>,
    channel_id: Option<Uuid>,
    root: Option<&str>,
    event_id: &str,
) -> u64 {
    let channel = channel_id
        .and_then(|id| read.get(&id.to_string()).copied())
        .unwrap_or(0);
    let thread = root
        .and_then(|id| read.get(&format!("thread:{id}")).copied())
        .unwrap_or(0);
    let message = read.get(&format!("msg:{event_id}")).copied().unwrap_or(0);
    u64::from(channel.max(thread).max(message))
}

fn bounded_event_preview(content: &str, tags_json: &str, http_base: &str) -> String {
    let (visible, has_attachments) = url::Url::parse(http_base).map_or_else(
        |_| (content.to_owned(), false),
        |base| {
            let attachments = crate::media::imeta::parse_tags(tags_json, content, &base);
            (
                crate::media::imeta::strip_attachment_lines(content, &attachments),
                !attachments.is_empty(),
            )
        },
    );
    if visible.trim().is_empty() && has_attachments {
        "[attachment]".into()
    } else {
        bounded_preview(&visible)
    }
}

fn bounded_preview(value: &str) -> String {
    sanitize::single_line(value).chars().take(280).collect()
}

fn json_tag_contains(tags_json: &str, name: &str, value: &str) -> bool {
    serde_json::from_str::<Vec<Vec<String>>>(tags_json).is_ok_and(|tags| {
        tags.iter().any(|tag| {
            tag.first().map(String::as_str) == Some(name)
                && tag.get(1).map(String::as_str) == Some(value)
        })
    })
}

fn valid_event_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_pubkey(value: &str) -> bool {
    valid_event_id(value)
}

fn valid_conversation_id(value: &str) -> bool {
    if value.len() > 256 {
        return false;
    }
    if let Some(channel) = value
        .strip_prefix("dm:")
        .or_else(|| value.strip_prefix("draft:"))
    {
        return Uuid::parse_str(channel).is_ok();
    }
    value
        .strip_prefix("thread:")
        .or_else(|| value.strip_prefix("event:"))
        .is_some_and(valid_event_id)
}

const fn category_order(category: &InboxCategory) -> u8 {
    match category {
        InboxCategory::Mention => 0,
        InboxCategory::Thread => 1,
        InboxCategory::Dm => 2,
        InboxCategory::NeedsAction => 3,
        InboxCategory::Draft => 4,
    }
}
