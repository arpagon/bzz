//! Apps offered in a channel (migration 22, plan v0.16.0 M6).
//!
//! Two sides. **What this bzz shared** (`app_shares`): one artifact of type
//! `bzz.app` per app and channel, which version it carries, its last
//! revision, and whether the brief of that version was posted. **What this
//! bzz took in** (`app_offers`): every revision of an offer, by its event
//! id, with its signer and what became of it.
//!
//! An offer's row follows the version it was recorded as: the functions of
//! `store::apps` that answer, supersede, fail or forget a version move the
//! offer's row with it, in the same transaction. Nothing here reads or
//! writes a file or runs anything; like `store::apps` it is not behind the
//! `apps` feature.

use rusqlite::{OptionalExtension as _, Row, params};
use uuid::Uuid;

use crate::{
    error::Result,
    store::{Store, events::u64_to_i64},
};

/// Lines of `app_offers` kept for a community, besides the waiting ones.
/// Older lines are dropped: what they guard against (taking a revision in
/// twice, a refused offer coming back) is then as old as 500 offers.
pub const KEPT_OFFER_LINES: usize = 500;

/// What became of a revision of an offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfferState {
    /// Recorded as a version that waits for the owner's answer.
    Waiting,
    /// Approved here, or its bytes are those that run.
    Installed,
    /// The owner refused it.
    Declined,
    /// It did not load. It was never offered.
    Failed,
    /// bzz did not take it; `detail` says why.
    Refused,
    /// Deleted in the channel before an answer.
    Withdrawn,
    /// A later revision of the same artifact took its place.
    Replaced,
}

impl OfferState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Installed => "installed",
            Self::Declined => "declined",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::Withdrawn => "withdrawn",
            Self::Replaced => "replaced",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        Ok(match value {
            "waiting" => Self::Waiting,
            "installed" => Self::Installed,
            "declined" => Self::Declined,
            "failed" => Self::Failed,
            "refused" => Self::Refused,
            "withdrawn" => Self::Withdrawn,
            "replaced" => Self::Replaced,
            _ => return Err(rusqlite::Error::InvalidQuery),
        })
    }
}

/// One revision of an offer that reached this bzz.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfferRow {
    pub community: Uuid,
    /// The event id of the revision.
    pub revision: String,
    pub channel: Uuid,
    pub artifact: Uuid,
    /// Who signed the revision (public key, hex).
    pub signer: String,
    pub name: Option<String>,
    pub script_sha256: Option<String>,
    pub brief_sha256: Option<String>,
    pub version: Option<u32>,
    pub state: OfferState,
    pub detail: Option<String>,
    pub received_at: u64,
}

/// One app this bzz offered in a channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShareRow {
    pub community: Uuid,
    pub channel: Uuid,
    pub name: String,
    pub artifact: Uuid,
    pub version: u32,
    pub script_sha256: String,
    pub revision: String,
    pub brief_posted_version: Option<u32>,
    pub shared_at: u64,
    pub withdrawn_at: Option<u64>,
}

impl ShareRow {
    pub const fn is_shared(&self) -> bool {
        self.withdrawn_at.is_none()
    }
}

fn bad() -> rusqlite::Error {
    rusqlite::Error::InvalidQuery
}

fn uuid_of(text: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(text).map_err(|_| bad())
}

fn unsigned(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| bad())
}

fn number(value: i64) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|_| bad())
}

const OFFER_COLUMNS: &str = "community_id,revision,channel_id,artifact_id,signer,name,script_sha256,brief_sha256,version,state,detail,received_at";

fn offer_of(row: &Row<'_>) -> rusqlite::Result<OfferRow> {
    Ok(OfferRow {
        community: uuid_of(&row.get::<_, String>(0)?)?,
        revision: row.get(1)?,
        channel: uuid_of(&row.get::<_, String>(2)?)?,
        artifact: uuid_of(&row.get::<_, String>(3)?)?,
        signer: row.get(4)?,
        name: row.get(5)?,
        script_sha256: row.get(6)?,
        brief_sha256: row.get(7)?,
        version: row.get::<_, Option<i64>>(8)?.map(number).transpose()?,
        state: OfferState::parse(&row.get::<_, String>(9)?)?,
        detail: row.get(10)?,
        received_at: unsigned(row.get(11)?)?,
    })
}

const SHARE_COLUMNS: &str = "community_id,channel_id,name,artifact_id,version,script_sha256,revision,brief_posted_version,shared_at,withdrawn_at";

fn share_of(row: &Row<'_>) -> rusqlite::Result<ShareRow> {
    Ok(ShareRow {
        community: uuid_of(&row.get::<_, String>(0)?)?,
        channel: uuid_of(&row.get::<_, String>(1)?)?,
        name: row.get(2)?,
        artifact: uuid_of(&row.get::<_, String>(3)?)?,
        version: number(row.get(4)?)?,
        script_sha256: row.get(5)?,
        revision: row.get(6)?,
        brief_posted_version: row.get::<_, Option<i64>>(7)?.map(number).transpose()?,
        shared_at: unsigned(row.get(8)?)?,
        withdrawn_at: row.get::<_, Option<i64>>(9)?.map(unsigned).transpose()?,
    })
}

/// Moves the offer rows of one version of an app with the version: called
/// by `store::apps` inside its own transactions. `only_from` limits it to
/// rows in that state.
pub(crate) fn follow_version(
    connection: &rusqlite::Connection,
    community: &str,
    name: &str,
    version: u32,
    to: OfferState,
    only_from: OfferState,
) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE app_offers SET state=?4 WHERE community_id=?1 AND name=?2 AND version=?3 AND state=?5",
        params![community, name, version, to.as_str(), only_from.as_str()],
    )
}

impl Store {
    /// The revision of an offer with this event id, if it reached this bzz.
    pub fn app_offer(&self, community: Uuid, revision: &str) -> Result<Option<OfferRow>> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {OFFER_COLUMNS} FROM app_offers WHERE community_id=?1 AND revision=?2"
                ),
                params![community.to_string(), revision],
                offer_of,
            )
            .optional()?)
    }

    /// Writes down a revision that reached this bzz, and what became of it.
    /// A revision already written down is left as it is: false.
    pub fn app_offer_insert(&mut self, row: &OfferRow) -> Result<bool> {
        let inserted = self.connection.execute(
            &format!(
                "INSERT OR IGNORE INTO app_offers({OFFER_COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
            ),
            params![
                row.community.to_string(),
                row.revision,
                row.channel.to_string(),
                row.artifact.to_string(),
                row.signer,
                row.name,
                row.script_sha256,
                row.brief_sha256,
                row.version,
                row.state.as_str(),
                row.detail,
                u64_to_i64(row.received_at)?,
            ],
        )?;
        self.app_offers_prune(row.community)?;
        Ok(inserted > 0)
    }

    /// Every revision of one artifact that reached this bzz, newest first.
    pub fn app_offers_of_artifact(&self, community: Uuid, artifact: Uuid) -> Result<Vec<OfferRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {OFFER_COLUMNS} FROM app_offers WHERE community_id=?1 AND artifact_id=?2 ORDER BY received_at DESC, rowid DESC"
        ))?;
        let rows = statement
            .query_map(
                params![community.to_string(), artifact.to_string()],
                offer_of,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The offers that wait for the owner's answer, in one channel or in
    /// the whole community, the oldest first.
    pub fn app_offers_waiting(
        &self,
        community: Uuid,
        channel: Option<Uuid>,
    ) -> Result<Vec<OfferRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {OFFER_COLUMNS} FROM app_offers WHERE community_id=?1 AND state='waiting' AND (?2 IS NULL OR channel_id=?2) ORDER BY received_at, rowid"
        ))?;
        let rows = statement
            .query_map(
                params![
                    community.to_string(),
                    channel.map(|channel| channel.to_string())
                ],
                offer_of,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The offer a version of an app was recorded from, if it was one.
    pub fn app_offer_of_version(
        &self,
        community: Uuid,
        name: &str,
        version: u32,
    ) -> Result<Option<OfferRow>> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {OFFER_COLUMNS} FROM app_offers WHERE community_id=?1 AND name=?2 AND version=?3 ORDER BY received_at DESC LIMIT 1"
                ),
                params![community.to_string(), name, version],
                offer_of,
            )
            .optional()?)
    }

    /// Moves one revision to another state, with why. False when it was
    /// not written down.
    pub fn app_offer_set_state(
        &mut self,
        community: Uuid,
        revision: &str,
        state: OfferState,
        detail: Option<&str>,
    ) -> Result<bool> {
        let changed = self.connection.execute(
            "UPDATE app_offers SET state=?3,detail=COALESCE(?4,detail) WHERE community_id=?1 AND revision=?2",
            params![community.to_string(), revision, state.as_str(), detail],
        )?;
        Ok(changed > 0)
    }

    /// Whether the owner refused these exact bytes of this app, offered in
    /// a channel or proposed any other way.
    pub fn app_bytes_declined(
        &self,
        community: Uuid,
        name: &str,
        script_sha256: &str,
        brief_sha256: &str,
    ) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_offers WHERE community_id=?1 AND name=?2 AND script_sha256=?3 AND brief_sha256=?4 AND state='declined')
                 OR EXISTS(SELECT 1 FROM app_versions WHERE community_id=?1 AND name=?2 AND script_sha256=?3 AND brief_sha256=?4 AND state='refused')",
            params![community.to_string(), name, script_sha256, brief_sha256],
            |row| row.get(0),
        )?)
    }

    fn app_offers_prune(&mut self, community: Uuid) -> Result<()> {
        self.connection.execute(
            "DELETE FROM app_offers WHERE community_id=?1 AND state<>'waiting'
               AND rowid NOT IN (SELECT rowid FROM app_offers WHERE community_id=?1 AND state<>'waiting' ORDER BY received_at DESC, rowid DESC LIMIT ?2)",
            params![
                community.to_string(),
                i64::try_from(KEPT_OFFER_LINES).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// What this bzz shared of `name` in `channel`, withdrawn or not.
    pub fn app_share(
        &self,
        community: Uuid,
        channel: Uuid,
        name: &str,
    ) -> Result<Option<ShareRow>> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {SHARE_COLUMNS} FROM app_shares WHERE community_id=?1 AND channel_id=?2 AND name=?3"
                ),
                params![community.to_string(), channel.to_string(), name],
                share_of,
            )
            .optional()?)
    }

    /// Everything this bzz shared in a community, withdrawn or not, by app
    /// and channel.
    pub fn app_shares(&self, community: Uuid) -> Result<Vec<ShareRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {SHARE_COLUMNS} FROM app_shares WHERE community_id=?1 ORDER BY name, shared_at, channel_id"
        ))?;
        let rows = statement
            .query_map([community.to_string()], share_of)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Writes down a revision of an offer this bzz published: a new share,
    /// or a later version of one. The posted brief is kept.
    pub fn app_share_published(&mut self, row: &ShareRow) -> Result<()> {
        self.connection.execute(
            &format!(
                "INSERT INTO app_shares({SHARE_COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)
                 ON CONFLICT (community_id,channel_id,name) DO UPDATE SET
                   artifact_id=excluded.artifact_id,version=excluded.version,
                   script_sha256=excluded.script_sha256,revision=excluded.revision,
                   shared_at=excluded.shared_at,withdrawn_at=NULL"
            ),
            params![
                row.community.to_string(),
                row.channel.to_string(),
                row.name,
                row.artifact.to_string(),
                row.version,
                row.script_sha256,
                row.revision,
                row.brief_posted_version,
                u64_to_i64(row.shared_at)?,
            ],
        )?;
        Ok(())
    }

    /// The brief of `version` was posted where `name` is shared.
    pub fn app_share_brief_posted(
        &mut self,
        community: Uuid,
        channel: Uuid,
        name: &str,
        version: u32,
    ) -> Result<bool> {
        let changed = self.connection.execute(
            "UPDATE app_shares SET brief_posted_version=?4 WHERE community_id=?1 AND channel_id=?2 AND name=?3",
            params![community.to_string(), channel.to_string(), name, version],
        )?;
        Ok(changed > 0)
    }

    /// The offer was withdrawn: a delete revision was published.
    pub fn app_share_withdrawn(
        &mut self,
        community: Uuid,
        channel: Uuid,
        name: &str,
        revision: &str,
        now: u64,
    ) -> Result<bool> {
        let changed = self.connection.execute(
            "UPDATE app_shares SET withdrawn_at=?5,revision=?4 WHERE community_id=?1 AND channel_id=?2 AND name=?3",
            params![
                community.to_string(),
                channel.to_string(),
                name,
                revision,
                u64_to_i64(now)?
            ],
        )?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IdentityConfig, KeyBackend};

    fn store() -> (Store, Uuid) {
        let mut store = Store::open_memory().unwrap();
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "me".into(),
            pubkey: "a".repeat(64),
            backend: KeyBackend::EncryptedFile,
            key_ref: "test".into(),
        };
        let mut config = Config {
            identities: vec![identity.clone()],
            ..Config::default()
        };
        let community = config
            .add_community("c".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        store.sync_config(&config).unwrap();
        (store, community)
    }

    fn offer(community: Uuid, revision: char, state: OfferState) -> OfferRow {
        OfferRow {
            community,
            revision: revision.to_string().repeat(64),
            channel: Uuid::from_u128(7),
            artifact: Uuid::from_u128(9),
            signer: "e".repeat(64),
            name: Some("board".into()),
            script_sha256: Some("1".repeat(64)),
            brief_sha256: Some("2".repeat(64)),
            version: Some(1),
            state,
            detail: None,
            received_at: 10,
        }
    }

    #[test]
    fn a_revision_is_written_down_once_and_read_back() {
        let (mut store, community) = store();
        let row = offer(community, 'a', OfferState::Waiting);
        assert!(store.app_offer_insert(&row).unwrap());
        assert!(!store.app_offer_insert(&row).unwrap(), "once");
        assert_eq!(
            store.app_offer(community, &row.revision).unwrap(),
            Some(row.clone())
        );
        assert_eq!(
            store
                .app_offers_waiting(community, Some(row.channel))
                .unwrap(),
            std::slice::from_ref(&row)
        );
        assert!(
            store
                .app_offers_waiting(community, Some(Uuid::from_u128(8)))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.app_offer_of_version(community, "board", 1).unwrap(),
            Some(row.clone())
        );
        assert!(
            store
                .app_offer_set_state(community, &row.revision, OfferState::Declined, Some("no"))
                .unwrap()
        );
        assert!(
            store
                .app_bytes_declined(community, "board", &"1".repeat(64), &"2".repeat(64))
                .unwrap()
        );
        assert!(
            !store
                .app_bytes_declined(community, "board", &"1".repeat(64), &"3".repeat(64))
                .unwrap(),
            "another brief is other bytes"
        );
    }

    #[test]
    fn a_share_keeps_its_posted_brief_across_a_withdrawal() {
        let (mut store, community) = store();
        let channel = Uuid::from_u128(7);
        let mut share = ShareRow {
            community,
            channel,
            name: "board".into(),
            artifact: Uuid::from_u128(1),
            version: 2,
            script_sha256: "1".repeat(64),
            revision: "a".repeat(64),
            brief_posted_version: None,
            shared_at: 5,
            withdrawn_at: None,
        };
        store.app_share_published(&share).unwrap();
        assert!(
            store
                .app_share_brief_posted(community, channel, "board", 2)
                .unwrap()
        );
        assert!(
            store
                .app_share_withdrawn(community, channel, "board", &"b".repeat(64), 6)
                .unwrap()
        );
        let withdrawn = store
            .app_share(community, channel, "board")
            .unwrap()
            .unwrap();
        assert!(!withdrawn.is_shared());
        assert_eq!(withdrawn.brief_posted_version, Some(2));
        share.artifact = Uuid::from_u128(2);
        share.revision = "c".repeat(64);
        share.shared_at = 7;
        store.app_share_published(&share).unwrap();
        let again = store
            .app_share(community, channel, "board")
            .unwrap()
            .unwrap();
        assert!(again.is_shared());
        assert_eq!(
            (again.artifact, again.brief_posted_version),
            (Uuid::from_u128(2), Some(2))
        );
        assert_eq!(store.app_shares(community).unwrap(), [again]);
    }
}
