//! Governing edits for stored messages. Rules: [`crate::protocol::edit`].

use std::collections::HashMap;

use nostr::Event;
use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};
use uuid::Uuid;

use crate::{
    error::Result,
    protocol::edit::{self, Candidate},
};

/// SQLite's default host-parameter limit is far above this; it bounds one IN list.
const CHUNK: usize = 400;

/// A stored original message, as edit selection needs it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Original<'a> {
    pub event_id: &'a str,
    pub pubkey: &'a str,
    pub kind: u16,
    pub channel_id: &'a str,
}

/// The governing edit's body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditBody {
    pub event_id: String,
    pub content: String,
    pub tags_json: String,
}

struct Stored {
    candidate: Candidate,
    channel_id: String,
    content: String,
    tags_json: String,
}

/// Governing edit per original event ID, for those that have one.
pub(crate) fn governing(
    connection: &Connection,
    community_id: Uuid,
    originals: &[Original<'_>],
) -> Result<HashMap<String, EditBody>> {
    let governed = originals
        .iter()
        .filter(|original| edit::GOVERNED_KINDS.contains(&original.kind))
        .collect::<Vec<_>>();
    let mut stored = HashMap::<String, Vec<Stored>>::new();
    for chunk in governed.chunks(CHUNK) {
        let placeholders = (0..chunk.len())
            .map(|index| format!("?{}", index + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT t.target_event_id,x.event_id,x.pubkey,x.created_at,x.deleted_by_event_id IS NOT NULL,
                    t.channel_id,x.content,x.tags_json
             FROM edit_targets t JOIN events x ON x.community_id=t.community_id AND x.event_id=t.edit_event_id
             WHERE t.community_id=?1 AND x.kind=40003 AND t.target_event_id IN ({placeholders})"
        );
        let mut values = Vec::<Value>::with_capacity(chunk.len() + 1);
        values.push(community_id.to_string().into());
        values.extend(
            chunk
                .iter()
                .map(|original| original.event_id.to_owned().into()),
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                Stored {
                    candidate: Candidate {
                        event_id: row.get(1)?,
                        pubkey: row.get(2)?,
                        created_at: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                        deleted: row.get(4)?,
                    },
                    channel_id: row.get(5)?,
                    content: row.get(6)?,
                    tags_json: row.get(7)?,
                },
            ))
        })?;
        for row in rows {
            let (target, value) = row?;
            stored.entry(target).or_default().push(value);
        }
    }
    if stored.is_empty() {
        return Ok(HashMap::new());
    }
    let owners = agent_owners(
        connection,
        community_id,
        governed
            .iter()
            .filter(|original| stored.contains_key(original.event_id))
            .map(|original| original.pubkey),
    )?;
    let mut governing = HashMap::new();
    for original in governed {
        let Some(edits) = stored.get(original.event_id) else {
            continue;
        };
        let in_channel = edits
            .iter()
            .filter(|edit| edit.channel_id == original.channel_id)
            .collect::<Vec<_>>();
        let chosen = edit::latest(
            in_channel.iter().map(|edit| &edit.candidate),
            original.pubkey,
            original.kind,
            owners.get(original.pubkey).map(String::as_str),
        );
        if let Some(chosen) = chosen
            && let Some(body) = in_channel
                .iter()
                .find(|edit| edit.candidate.event_id == chosen.event_id)
        {
            governing.insert(
                original.event_id.to_owned(),
                EditBody {
                    event_id: chosen.event_id.clone(),
                    content: body.content.clone(),
                    tags_json: body.tags_json.clone(),
                },
            );
        }
    }
    Ok(governing)
}

/// Verified owners of the given agent keys.
fn agent_owners<'a>(
    connection: &Connection,
    community_id: Uuid,
    authors: impl Iterator<Item = &'a str>,
) -> Result<HashMap<String, String>> {
    let mut authors = authors.map(str::to_owned).collect::<Vec<_>>();
    authors.sort();
    authors.dedup();
    let mut owners = HashMap::new();
    for chunk in authors.chunks(CHUNK) {
        let placeholders = (0..chunk.len())
            .map(|index| format!("?{}", index + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT agent_pubkey,owner_pubkey FROM remote_agents
             WHERE community_id=?1 AND verification_state='verified' AND owner_pubkey IS NOT NULL
               AND agent_pubkey IN ({placeholders})"
        );
        let mut values = Vec::<Value>::with_capacity(chunk.len() + 1);
        values.push(community_id.to_string().into());
        values.extend(chunk.iter().cloned().map(Into::into));
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (agent, owner) = row?;
            owners.insert(agent, owner);
        }
    }
    Ok(owners)
}

/// Record a newly stored edit. Returns its target when it has a valid one.
pub(crate) fn index(
    transaction: &Transaction<'_>,
    community_id: Uuid,
    event: &Event,
) -> Result<Option<String>> {
    let (Some(target), Some(channel)) = (
        edit::target(event),
        crate::protocol::events::channel_id(event),
    ) else {
        return Ok(None);
    };
    transaction.execute(
        "INSERT OR IGNORE INTO edit_targets(community_id,edit_event_id,target_event_id,channel_id) VALUES(?1,?2,?3,?4)",
        params![community_id.to_string(), event.id.to_hex(), target, channel.to_string()],
    )?;
    Ok(Some(target))
}

/// The target of a stored edit, for re-projection after the edit is deleted.
pub(crate) fn target_of(
    connection: &Connection,
    community_id: Uuid,
    edit_event_id: &str,
) -> Result<Option<String>> {
    use rusqlite::OptionalExtension as _;
    Ok(connection
        .query_row(
            "SELECT target_event_id FROM edit_targets WHERE community_id=?1 AND edit_event_id=?2",
            params![community_id.to_string(), edit_event_id],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use crate::{
        config::{CommunityConfig, Config, IdentityConfig, KeyBackend},
        store::Store,
    };

    fn setup() -> (Store, uuid::Uuid) {
        let identity = uuid::Uuid::new_v4();
        let community = uuid::Uuid::new_v4();
        let config = Config {
            identities: vec![IdentityConfig {
                id: identity,
                label: "edits".into(),
                pubkey: "a".repeat(64),
                backend: KeyBackend::Keychain,
                key_ref: "edits".into(),
            }],
            communities: vec![CommunityConfig {
                id: community,
                label: "edits".into(),
                relay_url: "wss://buzz.example/".into(),
                identity_id: identity,
                allow_insecure_localhost: false,
                theme: None,
                identity_adapter: None,
            }],
            default_community: Some(community),
            ..Config::default()
        };
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        (store, community)
    }

    fn event(
        keys: &Keys,
        kind: u16,
        content: &str,
        tags: Vec<Vec<String>>,
        at: u64,
    ) -> nostr::Event {
        EventBuilder::new(Kind::Custom(kind), content)
            .tags(tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    }

    fn indexed(store: &Store, id: &str) -> Option<String> {
        store
            .connection
            .query_row(
                "SELECT content FROM search_documents WHERE event_id=?1",
                [id],
                |row| row.get(0),
            )
            .ok()
    }

    #[test]
    fn search_indexes_the_governing_edit_and_falls_back_when_it_is_deleted() {
        let (mut store, community) = setup();
        let alice = Keys::generate();
        let channel = uuid::Uuid::new_v4().to_string();
        let h = vec!["h".to_owned(), channel.clone()];
        let original = event(&alice, 9, "teh original", vec![h.clone()], 10);
        let id = original.id.to_hex();
        store.apply_event(community, &original).unwrap();
        let fix = event(
            &alice,
            40_003,
            "the corrected",
            vec![h.clone(), vec!["e".into(), id.clone()]],
            20,
        );
        store.apply_event(community, &fix).unwrap();
        assert_eq!(indexed(&store, &id).as_deref(), Some("the corrected"));
        let deletion = event(
            &alice,
            5,
            "",
            vec![vec!["e".into(), fix.id.to_hex()], h],
            30,
        );
        store.apply_event(community, &deletion).unwrap();
        assert_eq!(indexed(&store, &id).as_deref(), Some("teh original"));
    }

    #[test]
    fn a_verified_agent_owner_may_edit_the_agents_message() {
        let (mut store, community) = setup();
        let agent = Keys::generate();
        let owner = Keys::generate();
        let channel = uuid::Uuid::new_v4();
        store
            .connection
            .execute(
                "INSERT INTO remote_agents(community_id,agent_pubkey,owner_pubkey,name,verification_state,updated_at)
                 VALUES(?1,?2,?3,'helper','verified',unixepoch())",
                rusqlite::params![
                    community.to_string(),
                    agent.public_key().to_hex(),
                    owner.public_key().to_hex()
                ],
            )
            .unwrap();
        let h = vec!["h".to_owned(), channel.to_string()];
        let original = event(&agent, 9, "draft", vec![h.clone()], 10);
        store.apply_event(community, &original).unwrap();
        let fix = event(
            &owner,
            40_003,
            "approved",
            vec![h, vec!["e".into(), original.id.to_hex()]],
            20,
        );
        store.apply_event(community, &fix).unwrap();
        let messages = store.messages(community, channel, 10).unwrap();
        assert_eq!(messages[0].content, "approved");
    }

    #[test]
    fn migration_indexes_edits_stored_by_older_versions() {
        let (mut store, community) = setup();
        let alice = Keys::generate();
        let channel = uuid::Uuid::new_v4();
        let h = vec!["h".to_owned(), channel.to_string()];
        let original = event(&alice, 9, "old", vec![h.clone()], 10);
        let id = original.id.to_hex();
        let fix = event(
            &alice,
            40_003,
            "new",
            vec![
                h,
                vec!["e".into(), "zz".into()],
                vec!["e".into(), id.to_ascii_uppercase()],
            ],
            20,
        );
        store.apply_event(community, &original).unwrap();
        store.apply_event(community, &fix).unwrap();
        // As if v0.13 had stored the edit: no index row, then the migration.
        store
            .connection
            .execute("DELETE FROM edit_targets", [])
            .unwrap();
        assert_eq!(
            store.messages(community, channel, 10).unwrap()[0].content,
            "old"
        );
        let sql = include_str!("../../migrations/0016_message_edits.sql");
        let backfill = &sql[sql.find("INSERT OR IGNORE").unwrap()..];
        store.connection.execute_batch(backfill).unwrap();
        let target: String = store
            .connection
            .query_row("SELECT target_event_id FROM edit_targets", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(target, id, "first valid e tag, lowercased");
        assert_eq!(
            store.messages(community, channel, 10).unwrap()[0].content,
            "new"
        );
    }
}
