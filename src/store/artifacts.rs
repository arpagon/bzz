//! NIP-AR current heads. The sync subscription replaces a channel's set only
//! at EOSE; live heads and move removals then update single rows. Rows are
//! never messages, so no unread, mention or reply counter reads this table.
//!
//! An app's records (v0.16) are rows of the same table, of the types
//! `bzz.app.<app>.<record>`. They are synced by type, not by channel, so they
//! have their own replace and their own read; the channel listing leaves
//! them out.

use nostr::{Event, JsonUtil as _};
use rusqlite::{Connection, OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    error::Result,
    protocol::artifact::{self, APP_RECORD_TYPE_PREFIX, Artifact},
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

/// `?N,?N+1,...` for `count` values starting at parameter `first`.
fn placeholders(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn upsert_head(connection: &Connection, community: Uuid, head: &Artifact) -> Result<bool> {
    let json = head.event.as_json();
    let live = (!head.is_deleted()).then(|| json.clone());
    let changed = connection.execute(
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

impl Store {
    /// Upserts one head. A deletion keeps the previous live revision so the
    /// artifact can be restored; any other head becomes the live revision.
    pub fn apply_artifact_head(&mut self, community: Uuid, head: &Artifact) -> Result<bool> {
        upsert_head(&self.connection, community, head)
    }

    /// The complete set of `types` in `channel`, as one typed sync delivered
    /// it. Rows of those types homed there that are missing from the set are
    /// gone, deletions included: a record cannot be restored, so nothing of
    /// it is kept. Rows of every other type, and of every other channel, are
    /// not touched. One transaction: a reader sees the old set or the new.
    pub fn replace_artifact_types(
        &mut self,
        community: Uuid,
        channel: Uuid,
        types: &[String],
        heads: &[Artifact],
    ) -> Result<()> {
        if types.is_empty() {
            return Ok(());
        }
        let kept = |head: &&Artifact| {
            head.home == channel && !head.is_deleted() && types.contains(&head.artifact_type)
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "CREATE TEMP TABLE IF NOT EXISTS artifact_set(artifact_id TEXT PRIMARY KEY)",
            [],
        )?;
        transaction.execute("DELETE FROM artifact_set", [])?;
        for head in heads.iter().filter(kept) {
            transaction.execute(
                "INSERT OR IGNORE INTO artifact_set(artifact_id) VALUES(?1)",
                [head.id.to_string()],
            )?;
        }
        let mut values = vec![community.to_string(), channel.to_string()];
        values.extend(types.iter().cloned());
        transaction.execute(
            &format!(
                "DELETE FROM artifacts WHERE community_id=?1 AND home=?2
                 AND artifact_type IN ({})
                 AND artifact_id NOT IN (SELECT artifact_id FROM artifact_set)",
                placeholders(3, types.len())
            ),
            rusqlite::params_from_iter(values),
        )?;
        for head in heads.iter().filter(kept) {
            upsert_head(&transaction, community, head)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Every live head of `types` homed in `channel`, ordered by identity.
    /// No cut: an app shows all its records or says why it cannot.
    pub fn typed_artifacts(
        &self,
        community: Uuid,
        channel: Uuid,
        types: &[String],
    ) -> Result<Vec<Artifact>> {
        if types.is_empty() {
            return Ok(Vec::new());
        }
        let mut values = vec![community.to_string(), channel.to_string()];
        values.extend(types.iter().cloned());
        let mut statement = self.connection.prepare(&format!(
            "SELECT event_json FROM artifacts WHERE community_id=?1 AND home=?2
             AND op<>'delete' AND artifact_type IN ({}) ORDER BY artifact_id",
            placeholders(3, types.len())
        ))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(values), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.iter().filter_map(|json| decode(json)).collect())
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
    /// Without the records of apps: an app may hold a thousand of them, the
    /// listing shows five hundred rows, and they are its app's to show.
    pub fn channel_artifacts(
        &self,
        community: Uuid,
        channel: Uuid,
        root: Option<&str>,
    ) -> Result<Vec<StoredArtifact>> {
        self.artifacts_where(
            &format!(
                "home=?2 AND (?3 IS NULL OR root=?3) AND substr(artifact_type,1,{})<>'{}'",
                APP_RECORD_TYPE_PREFIX.len(),
                APP_RECORD_TYPE_PREFIX
            ),
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
        typed(keys, home, "buzz.task", assignee)
    }

    fn typed(keys: &Keys, home: Uuid, artifact_type: &str, assignee: &str) -> Artifact {
        let tags = [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &home.to_string()],
            ["type", artifact_type],
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

    fn deletion(keys: &Keys, head: &Artifact) -> Artifact {
        parse(
            &build(&Change::Delete, head, None)
                .unwrap()
                .sign_with_keys(keys)
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_typed_set_replaces_only_its_types_in_its_channel() {
        let (mut store, community) = store();
        let keys = Keys::generate();
        let (channel, elsewhere) = (Uuid::new_v4(), Uuid::new_v4());
        let card = "bzz.app.board.card".to_owned();
        let column = "bzz.app.board.column".to_owned();
        let ids = |store: &Store, channel, types: &[String]| {
            store
                .typed_artifacts(community, channel, types)
                .unwrap()
                .into_iter()
                .map(|head| head.id)
                .collect::<Vec<_>>()
        };
        let (a, b) = (
            typed(&keys, channel, &card, "x"),
            typed(&keys, channel, &card, "x"),
        );
        let lane = typed(&keys, channel, &column, "x");
        let vote = typed(&keys, channel, "bzz.app.poll.vote", "x");
        let task = task(&keys, channel, "x");
        let far = typed(&keys, elsewhere, &card, "x");
        for head in [&a, &b, &lane, &vote, &task, &far] {
            store.apply_artifact_head(community, head).unwrap();
        }

        // A new complete set of the cards: `a` revised is not in it, `b` is.
        store
            .replace_artifact_types(
                community,
                channel,
                std::slice::from_ref(&card),
                std::slice::from_ref(&b),
            )
            .unwrap();
        assert_eq!(ids(&store, channel, std::slice::from_ref(&card)), [b.id]);
        // Another type of the same app, another app, a task and another
        // channel are as they were.
        assert_eq!(
            ids(&store, channel, std::slice::from_ref(&column)),
            [lane.id]
        );
        assert_eq!(
            ids(&store, channel, &["bzz.app.poll.vote".to_owned()]),
            [vote.id]
        );
        assert_eq!(
            ids(&store, elsewhere, std::slice::from_ref(&card)),
            [far.id]
        );
        assert!(store.artifact(community, task.id).unwrap().is_some());

        // A head of another type or channel in the set is not stored, and a
        // deletion in the set is not a record.
        let stray = typed(&keys, channel, "bzz.app.poll.vote", "x");
        let gone = deletion(&keys, &b);
        store
            .replace_artifact_types(
                community,
                channel,
                std::slice::from_ref(&card),
                &[stray.clone(), far.clone(), gone],
            )
            .unwrap();
        assert!(store.artifact(community, stray.id).unwrap().is_none());
        assert!(ids(&store, channel, std::slice::from_ref(&card)).is_empty());
        assert!(
            store.artifact(community, b.id).unwrap().is_none(),
            "no deletion row is kept for a record"
        );
        assert_eq!(
            ids(&store, elsewhere, std::slice::from_ref(&card)),
            [far.id]
        );

        // No declared type: nothing is read and nothing is removed.
        store
            .replace_artifact_types(community, channel, &[], &[])
            .unwrap();
        assert!(ids(&store, channel, &[]).is_empty());
        assert!(store.artifact(community, lane.id).unwrap().is_some());
    }

    #[test]
    fn an_apps_records_are_read_whole_and_stay_out_of_the_channel_listing() {
        let (mut store, community) = store();
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let card = "bzz.app.board.card".to_owned();
        let heads = (0..620)
            .map(|_| typed(&keys, channel, &card, "x"))
            .collect::<Vec<_>>();
        store
            .replace_artifact_types(community, channel, std::slice::from_ref(&card), &heads)
            .unwrap();
        let task = task(&keys, channel, "x");
        let offer = typed(&keys, channel, "bzz.app", "x");
        let lookalike = typed(&keys, channel, "bzz.apple.note", "x");
        for head in [&task, &offer, &lookalike] {
            store.apply_artifact_head(community, head).unwrap();
        }

        // Past the listing's cut of 500, in the order of their identity.
        let read = store
            .typed_artifacts(community, channel, std::slice::from_ref(&card))
            .unwrap();
        assert_eq!(read.len(), 620);
        assert!(read.windows(2).all(|pair| pair[0].id < pair[1].id));

        // The listing shows what is not an app's record, and all of it.
        let mut listed = store
            .channel_artifacts(community, channel, None)
            .unwrap()
            .into_iter()
            .map(|stored| stored.head.artifact_type)
            .collect::<Vec<_>>();
        listed.sort();
        assert_eq!(listed, ["buzz.task", "bzz.app", "bzz.apple.note"]);

        // A deleted record is no record, also when the channel-wide sync
        // stored its deletion.
        let gone = deletion(&keys, &heads[0]);
        store.apply_artifact_head(community, &gone).unwrap();
        assert_eq!(
            store
                .typed_artifacts(community, channel, std::slice::from_ref(&card))
                .unwrap()
                .len(),
            619
        );
        assert_eq!(
            store
                .channel_artifacts(community, channel, None)
                .unwrap()
                .len(),
            3
        );
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
