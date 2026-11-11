//! NIP-AR current heads. The sync subscription replaces a channel's set only
//! at EOSE; live heads and move removals then update single rows. Rows are
//! never messages, so no unread, mention or reply counter reads this table.

use nostr::{Event, JsonUtil as _};
use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    error::Result,
    protocol::artifact::{self, Artifact},
};

use super::Store;

/// A stored head plus the last live revision (the restore snapshot).
#[derive(Clone, Debug)]
pub struct StoredArtifact {
    pub head: Artifact,
    pub live: Option<Artifact>,
}

fn decode(json: &str) -> Option<Artifact> {
    Event::from_json(json)
        .ok()
        .and_then(|event| artifact::parse(&event).ok())
}

impl Store {
    /// Upserts one head. A deletion keeps the previous live revision so the
    /// artifact can be restored; any other head becomes the live revision.
    pub fn apply_artifact_head(&mut self, community: Uuid, head: &Artifact) -> Result<bool> {
        let json = head.event.as_json();
        let live = (!head.is_deleted()).then(|| json.clone());
        let changed = self.connection.execute(
            "INSERT INTO artifacts(community_id,artifact_id,home,artifact_type,title,op,root,head,author,created_at,event_json,live_json)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(community_id,artifact_id) DO UPDATE SET
               home=excluded.home,artifact_type=excluded.artifact_type,title=excluded.title,op=excluded.op,
               root=excluded.root,head=excluded.head,author=excluded.author,created_at=excluded.created_at,
               event_json=excluded.event_json,live_json=COALESCE(excluded.live_json,artifacts.live_json)
             WHERE artifacts.head<>excluded.head",
            params![
                community.to_string(),
                head.id.to_string(),
                head.home.to_string(),
                head.artifact_type,
                head.title,
                format!("{:?}", head.op).to_lowercase(),
                head.root,
                head.head,
                head.author,
                head.created_at as i64,
                json,
                live,
            ],
        )?;
        Ok(changed > 0)
    }

    /// The complete set for `channel` at EOSE: heads in the set replace the
    /// stored rows; live rows of that home missing from the set left it
    /// (retired, moved away, access changed). Known deletions stay so they can
    /// still be restored.
    pub fn replace_channel_artifacts(
        &mut self,
        community: Uuid,
        channel: Uuid,
        heads: &[Artifact],
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "CREATE TEMP TABLE IF NOT EXISTS artifact_set(artifact_id TEXT PRIMARY KEY)",
            [],
        )?;
        transaction.execute("DELETE FROM artifact_set", [])?;
        for head in heads.iter().filter(|head| head.home == channel) {
            transaction.execute(
                "INSERT OR IGNORE INTO artifact_set(artifact_id) VALUES(?1)",
                [head.id.to_string()],
            )?;
        }
        transaction.execute(
            "DELETE FROM artifacts WHERE community_id=?1 AND home=?2 AND op<>'delete'
             AND artifact_id NOT IN (SELECT artifact_id FROM artifact_set)",
            params![community.to_string(), channel.to_string()],
        )?;
        transaction.commit()?;
        for head in heads.iter().filter(|head| head.home == channel) {
            self.apply_artifact_head(community, head)?;
        }
        Ok(())
    }

    /// A move removal: the artifact left `source` (only that home's row).
    pub fn remove_artifact(&mut self, community: Uuid, id: Uuid, source: Uuid) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM artifacts WHERE community_id=?1 AND artifact_id=?2 AND home=?3",
            params![community.to_string(), id.to_string(), source.to_string()],
        )? > 0)
    }

    pub fn artifact(&self, community: Uuid, id: Uuid) -> Result<Option<StoredArtifact>> {
        Ok(self
            .connection
            .query_row(
                "SELECT event_json,live_json FROM artifacts WHERE community_id=?1 AND artifact_id=?2",
                params![community.to_string(), id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?
            .and_then(|(head, live)| {
                Some(StoredArtifact {
                    head: decode(&head)?,
                    live: live.as_deref().and_then(decode),
                })
            }))
    }

    /// Artifacts homed in `channel`, optionally only those anchored to the
    /// thread `root`; live first, then known deletions, newest first.
    pub fn channel_artifacts(
        &self,
        community: Uuid,
        channel: Uuid,
        root: Option<&str>,
    ) -> Result<Vec<StoredArtifact>> {
        self.artifacts_where(
            "home=?2 AND (?3 IS NULL OR root=?3)",
            params![community.to_string(), channel.to_string(), root],
        )
    }

    /// Live `buzz.task` artifacts whose `assignee` tag names `pubkey`.
    pub fn assigned_artifacts(&self, community: Uuid, pubkey: &str) -> Result<Vec<StoredArtifact>> {
        Ok(self
            .artifacts_where(
                "artifact_type='buzz.task' AND op<>'delete'",
                params![community.to_string()],
            )?
            .into_iter()
            .filter(|stored| {
                stored
                    .head
                    .client_tags()
                    .iter()
                    .any(|(name, value)| name == "assignee" && value.eq_ignore_ascii_case(pubkey))
            })
            .collect())
    }

    fn artifacts_where(
        &self,
        predicate: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<StoredArtifact>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT event_json,live_json FROM artifacts WHERE community_id=?1 AND {predicate}
             ORDER BY op='delete', created_at DESC, artifact_id LIMIT 500"
        ))?;
        let rows = statement
            .query_map(params, |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(head, live)| {
                Some(StoredArtifact {
                    head: decode(&head)?,
                    live: live.as_deref().and_then(decode),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Config, IdentityConfig, KeyBackend},
        protocol::artifact::{Change, build, parse},
    };
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn store() -> (Store, Uuid) {
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "a".into(),
            pubkey: Keys::generate().public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "a".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community("a".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        let mut store = Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        (store, community)
    }

    fn task(keys: &Keys, home: Uuid, assignee: &str) -> Artifact {
        let tags = [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &home.to_string()],
            ["type", "buzz.task"],
            ["title", "Task"],
            ["op", "create"],
            ["assignee", assignee],
        ]
        .map(|parts| Tag::parse(parts).unwrap());
        parse(
            &EventBuilder::new(Kind::Custom(45_010), "{}")
                .tags(tags)
                .sign_with_keys(keys)
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn eose_replaces_the_set_and_deletions_keep_the_restore_snapshot() {
        let (mut store, community) = store();
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let me = keys.public_key().to_hex();
        let (a, b) = (task(&keys, channel, &me), task(&keys, channel, "other"));
        store
            .replace_channel_artifacts(community, channel, &[a.clone(), b.clone()])
            .unwrap();
        assert_eq!(
            store
                .channel_artifacts(community, channel, None)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(store.assigned_artifacts(community, &me).unwrap().len(), 1);

        let deleted = parse(
            &build(&Change::Delete, &a, None)
                .unwrap()
                .sign_with_keys(&keys)
                .unwrap(),
        )
        .unwrap();
        assert!(store.apply_artifact_head(community, &deleted).unwrap());
        assert!(
            !store.apply_artifact_head(community, &deleted).unwrap(),
            "same head is a no-op"
        );
        let stored = store.artifact(community, a.id).unwrap().unwrap();
        assert!(stored.head.is_deleted());
        assert_eq!(stored.live.unwrap().head, a.head, "restore snapshot kept");
        assert!(store.assigned_artifacts(community, &me).unwrap().is_empty());

        // A new complete set without `b`: `b` left; the known deletion stays.
        store
            .replace_channel_artifacts(community, channel, &[])
            .unwrap();
        let left = store.channel_artifacts(community, channel, None).unwrap();
        assert_eq!(left.len(), 1);
        assert!(left[0].head.is_deleted());

        // A move removal only removes the row homed in the source.
        let c = task(&keys, channel, &me);
        store.apply_artifact_head(community, &c).unwrap();
        assert!(
            !store
                .remove_artifact(community, c.id, Uuid::new_v4())
                .unwrap()
        );
        assert!(store.remove_artifact(community, c.id, channel).unwrap());
    }
}
