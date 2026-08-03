use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;
use uuid::Uuid;

use crate::{
    domain::{InboxCategory, InboxItem},
    error::Result,
    render::sanitize,
    store::{Store, events::u64_to_i64},
};

const EVENT_SCAN_CAP: usize = 2_000;
const DRAFT_SCAN_CAP: usize = 500;
const INBOX_CAP: usize = 500;

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

impl Store {
    pub fn inbox_items(&self, community_id: Uuid, identity_pubkey: &str) -> Result<Vec<InboxItem>> {
        let read = self.read_contexts(community_id, identity_pubkey, false)?;
        let participated = self.participated_roots(community_id, identity_pubkey)?;
        let overrides = self.inbox_overrides(community_id, identity_pubkey)?;
        let mut statement = self.connection.prepare(
            "SELECT e.event_id,e.kind,e.pubkey,e.created_at,e.channel_id,COALESCE(c.channel_type,''),
                    e.content,e.tags_json,e.root_event_id,
                    EXISTS(SELECT 1 FROM event_mentions m WHERE m.community_id=e.community_id AND m.event_id=e.event_id AND m.mentioned_pubkey=?2),
                    co.http_base_url
             FROM events e
             JOIN communities co ON co.id=e.community_id
             LEFT JOIN channels c ON c.community_id=e.community_id AND c.channel_id=e.channel_id
             LEFT JOIN outbox o ON o.community_id=e.community_id AND o.event_id=e.event_id
             WHERE e.community_id=?1 AND e.kind IN (9,40002,46010,46011,46012)
               AND e.deleted_by_event_id IS NULL
               AND (o.event_id IS NULL OR o.state='delivered')
               AND (e.channel_id IS NULL OR c.is_member=1)
               AND NOT EXISTS(
                 SELECT 1 FROM dm_visibility v
                 WHERE c.channel_type='dm' AND v.community_id=e.community_id
                   AND v.identity_pubkey=?2 AND v.channel_id=e.channel_id
               )
             ORDER BY e.created_at DESC,e.event_id ASC LIMIT ?3",
        )?;
        let candidates = statement
            .query_map(
                params![
                    community_id.to_string(),
                    identity_pubkey,
                    EVENT_SCAN_CAP as i64
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

        let mut grouped = BTreeMap::<String, InboxItem>::new();
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
                format!(
                    "dm:{}",
                    candidate
                        .channel_id
                        .map_or_else(String::new, |value| value.to_string())
                )
            } else if let Some(root) = &thread_root {
                format!("thread:{root}")
            } else {
                format!("event:{}", candidate.event_id)
            };
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
                    conversation_id,
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
                    draft_count: 0,
                    forced_unread,
                });
            for category in categories {
                if !entry.categories.contains(&category) {
                    entry.categories.push(category);
                }
            }
            if unread && !locally_read {
                entry.unread_count = entry.unread_count.saturating_add(1);
            }
            entry.forced_unread |= forced_unread;
            if candidate.created_at > entry.created_at
                || (candidate.created_at == entry.created_at
                    && candidate.event_id < entry.event_id.clone().unwrap_or_default())
            {
                entry.event_id = Some(candidate.event_id);
                entry.channel_id = candidate.channel_id;
                entry.thread_root = thread_root;
                entry.sender_pubkey = Some(candidate.pubkey);
                entry.created_at = candidate.created_at;
                entry.preview = bounded_event_preview(
                    &candidate.content,
                    &candidate.tags_json,
                    &candidate.http_base,
                );
            }
        }
        self.add_drafts(community_id, identity_pubkey, &overrides, &mut grouped)?;
        let mut values = grouped.into_values().collect::<Vec<_>>();
        for item in &mut values {
            item.categories.sort_by_key(category_order);
        }
        values.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        values.truncate(INBOX_CAP);
        Ok(values)
    }

    fn participated_roots(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<BTreeSet<String>> {
        let mut statement = self.connection.prepare(
            "SELECT CASE WHEN root_event_id IS NULL THEN event_id ELSE root_event_id END
             FROM (
               SELECT event_id,root_event_id FROM events
               WHERE community_id=?1 AND pubkey=?2 AND kind IN (9,40002)
               ORDER BY created_at DESC,event_id LIMIT 10000
             )",
        )?;
        Ok(statement
            .query_map(params![community_id.to_string(), identity_pubkey], |row| {
                row.get(0)
            })?
            .collect::<std::result::Result<BTreeSet<_>, _>>()?)
    }

    fn inbox_overrides(
        &self,
        community_id: Uuid,
        identity_pubkey: &str,
    ) -> Result<BTreeMap<String, (bool, Option<u64>)>> {
        let mut statement = self.connection.prepare(
            "SELECT conversation_id,forced_unread,local_done_at FROM inbox_overrides WHERE community_id=?1 AND identity_pubkey=?2 LIMIT 1000",
        )?;
        Ok(statement
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
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?)
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
             WHERE d.community_id=?1 AND (length(d.body)>0 OR d.attachments_json<>'[]')
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
                    DRAFT_SCAN_CAP as i64
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
        for (channel, root, body, updated_at, channel_type) in drafts {
            let Ok(channel_id) = Uuid::parse_str(&channel) else {
                continue;
            };
            let conversation_id = if channel_type == "dm" {
                format!("dm:{channel}")
            } else if !root.is_empty() {
                format!("thread:{root}")
            } else {
                format!("draft:{channel}")
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
                    draft_count: 0,
                    forced_unread,
                });
            if !entry.categories.contains(&InboxCategory::Draft) {
                entry.categories.push(InboxCategory::Draft);
            }
            entry.draft_count = entry.draft_count.saturating_add(1);
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
        if conversation_id.len() > 256 {
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
        Ok(())
    }
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

const fn category_order(category: &InboxCategory) -> u8 {
    match category {
        InboxCategory::Mention => 0,
        InboxCategory::Thread => 1,
        InboxCategory::Dm => 2,
        InboxCategory::NeedsAction => 3,
        InboxCategory::Draft => 4,
    }
}
