//! The apps installed on this machine (migration 21, ADR v0.16 apps).
//!
//! These rows are what bzz decided about an app: which version runs, which
//! hash was approved, who proposed each version and what became of it. The
//! bytes are files, kept by `apps::store`; no file says by itself whether it
//! may run. Nothing here reads or writes a file, and nothing here is behind
//! the `apps` feature: a bzz built without the interpreter still opens,
//! migrates and keeps these tables.
//!
//! A version's row is written once and its hashes never change. Only its
//! state moves, and only forward:
//!
//! ```text
//!   waiting ──▶ approved ──▶ failed      (it failed as it was put to run)
//!      ├──────▶ refused
//!      ├──────▶ superseded               (its proposer replaced it)
//!      └──────▶ failed                   (its files changed before the answer)
//!   failed                               (it never loaded: recorded so)
//! ```

use rusqlite::{OptionalExtension as _, Row, TransactionBehavior, params};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    protocol::artifact::APP_RECORD_TYPE_PREFIX,
    store::{Store, events::u64_to_i64},
};

/// What became of a proposed version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionState {
    /// It loads, and waits for the owner's answer. It does not run.
    Waiting,
    /// The owner approved its exact bytes.
    Approved,
    /// The owner refused it.
    Refused,
    /// It did not load, or it failed as it was put to run.
    Failed,
    /// Its proposer replaced it with a later one before it was answered.
    Superseded,
}

impl VersionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Approved => "approved",
            Self::Refused => "refused",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "waiting" => Self::Waiting,
            "approved" => Self::Approved,
            "refused" => Self::Refused,
            "failed" => Self::Failed,
            "superseded" => Self::Superseded,
            _ => return Err(Error::Database(rusqlite::Error::InvalidQuery)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposerKind {
    /// The owner himself, at his own shell or in his own bzz.
    Owner,
    /// One of the owner's local agents.
    Agent,
    /// A person of the channel, who offered the app there.
    Member,
}

impl ProposerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Agent => "agent",
            Self::Member => "member",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "owner" => Self::Owner,
            "agent" => Self::Agent,
            "member" => Self::Member,
            _ => return Err(Error::Database(rusqlite::Error::InvalidQuery)),
        })
    }
}

/// Who proposed a version. Public facts only: no key is ever part of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposer {
    pub kind: ProposerKind,
    /// The public key (hex) of the agent or the person. None for the owner.
    pub id: Option<String>,
    /// Its name when it proposed, for showing.
    pub name: Option<String>,
    /// The channel it was proposed or offered in.
    pub channel: Option<Uuid>,
}

impl Proposer {
    pub const fn owner() -> Self {
        Self {
            kind: ProposerKind::Owner,
            id: None,
            name: None,
            channel: None,
        }
    }

    /// One of the owner's agents, by its public key.
    pub fn agent(pubkey: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: ProposerKind::Agent,
            id: Some(pubkey.into()),
            name: Some(name.into()),
            channel: None,
        }
    }

    /// A person of the channel, by public key.
    pub fn member(pubkey: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: ProposerKind::Member,
            id: Some(pubkey.into()),
            name: Some(name.into()),
            channel: None,
        }
    }

    pub fn in_channel(mut self, channel: Uuid) -> Self {
        self.channel = Some(channel);
        self
    }

    /// The same proposer, whatever it is called now and wherever it asks.
    pub fn is(&self, other: &Self) -> bool {
        self.kind == other.kind && self.id == other.id
    }
}

/// One installed app of a community.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledApp {
    pub community: Uuid,
    pub name: String,
    pub enabled: bool,
    /// The approved version that runs; none until the first approval.
    pub current: Option<u32>,
    /// The version that ran before the current one was put to run.
    pub previous: Option<u32>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// One proposed version of an app, and what became of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppVersion {
    pub community: Uuid,
    pub name: String,
    pub version: u32,
    pub state: VersionState,
    /// Refused: the owner's reason. Failed: the error. Superseded: by what.
    pub detail: Option<String>,
    /// None for a version that never loaded.
    pub title: Option<String>,
    pub api: Option<i64>,
    /// The record types it declares; empty for a version that never loaded.
    pub types: Vec<String>,
    pub script_sha256: String,
    pub manifest_sha256: Option<String>,
    pub brief_sha256: String,
    pub script_bytes: u64,
    pub brief_bytes: u64,
    pub proposer: Proposer,
    pub proposed_at: u64,
    pub decided_at: Option<u64>,
    /// Its files are still on disk.
    pub kept: bool,
}

/// A version to record. Its number is given when it is recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAppVersion {
    pub community: Uuid,
    pub name: String,
    /// `Waiting` for one that loads, `Failed` for one that does not.
    pub state: VersionState,
    pub detail: Option<String>,
    pub title: Option<String>,
    pub api: Option<i64>,
    pub types: Option<Vec<String>>,
    pub script_sha256: String,
    pub manifest_sha256: Option<String>,
    pub brief_sha256: String,
    pub script_bytes: u64,
    pub brief_bytes: u64,
    pub proposer: Proposer,
}

/// What recording a version did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedVersion {
    pub version: u32,
    /// Waiting versions of the same proposer that this one replaced.
    pub superseded: Vec<u32>,
}

const VERSION_COLUMNS: &str = "community_id,name,version,state,detail,title,api,types_json,script_sha256,manifest_sha256,brief_sha256,script_bytes,brief_bytes,proposer_kind,proposer_id,proposer_name,proposer_channel,proposed_at,decided_at,kept";

fn bad() -> rusqlite::Error {
    rusqlite::Error::InvalidQuery
}

fn uuid_of(text: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(text).map_err(|_| bad())
}

fn unsigned(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| bad())
}

fn version_number(value: i64) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|_| bad())
}

fn version_of(row: &Row<'_>) -> rusqlite::Result<AppVersion> {
    let types = match row.get::<_, Option<String>>(7)? {
        Some(json) => serde_json::from_str(&json).map_err(|_| bad())?,
        None => Vec::new(),
    };
    Ok(AppVersion {
        community: uuid_of(&row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        version: version_number(row.get(2)?)?,
        state: VersionState::parse(&row.get::<_, String>(3)?).map_err(|_| bad())?,
        detail: row.get(4)?,
        title: row.get(5)?,
        api: row.get(6)?,
        types,
        script_sha256: row.get(8)?,
        manifest_sha256: row.get(9)?,
        brief_sha256: row.get(10)?,
        script_bytes: unsigned(row.get(11)?)?,
        brief_bytes: unsigned(row.get(12)?)?,
        proposer: Proposer {
            kind: ProposerKind::parse(&row.get::<_, String>(13)?).map_err(|_| bad())?,
            id: row.get(14)?,
            name: row.get(15)?,
            channel: row
                .get::<_, Option<String>>(16)?
                .map(|text| uuid_of(&text))
                .transpose()?,
        },
        proposed_at: unsigned(row.get(17)?)?,
        decided_at: row.get::<_, Option<i64>>(18)?.map(unsigned).transpose()?,
        kept: row.get::<_, i64>(19)? != 0,
    })
}

fn app_of(row: &Row<'_>) -> rusqlite::Result<InstalledApp> {
    Ok(InstalledApp {
        community: uuid_of(&row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        current: row
            .get::<_, Option<i64>>(3)?
            .map(version_number)
            .transpose()?,
        previous: row
            .get::<_, Option<i64>>(4)?
            .map(version_number)
            .transpose()?,
        created_at: unsigned(row.get(5)?)?,
        updated_at: unsigned(row.get(6)?)?,
    })
}

const APP_COLUMNS: &str =
    "community_id,name,enabled,current_version,previous_version,created_at,updated_at";

impl Store {
    /// The apps of a community, by name.
    pub fn apps(&self, community: Uuid) -> Result<Vec<InstalledApp>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {APP_COLUMNS} FROM apps WHERE community_id=?1 ORDER BY name"
        ))?;
        let rows = statement
            .query_map([community.to_string()], app_of)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn app(&self, community: Uuid, name: &str) -> Result<Option<InstalledApp>> {
        Ok(self
            .connection
            .query_row(
                &format!("SELECT {APP_COLUMNS} FROM apps WHERE community_id=?1 AND name=?2"),
                params![community.to_string(), name],
                app_of,
            )
            .optional()?)
    }

    /// Every version of an app that bzz still has a line of, newest first.
    pub fn app_versions(&self, community: Uuid, name: &str) -> Result<Vec<AppVersion>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {VERSION_COLUMNS} FROM app_versions WHERE community_id=?1 AND name=?2 ORDER BY version DESC"
        ))?;
        let rows = statement
            .query_map(params![community.to_string(), name], version_of)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn app_version(
        &self,
        community: Uuid,
        name: &str,
        version: u32,
    ) -> Result<Option<AppVersion>> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {VERSION_COLUMNS} FROM app_versions WHERE community_id=?1 AND name=?2 AND version=?3"
                ),
                params![community.to_string(), name, version],
                version_of,
            )
            .optional()?)
    }

    /// The versions of a community's apps that wait for the owner's answer,
    /// the one that has waited longest first.
    pub fn app_versions_waiting(&self, community: Uuid) -> Result<Vec<AppVersion>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {VERSION_COLUMNS} FROM app_versions WHERE community_id=?1 AND state='waiting' ORDER BY proposed_at,name,version"
        ))?;
        let rows = statement
            .query_map([community.to_string()], version_of)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Records a proposed version under the next number of its app, and the
    /// app itself if this is its first. A waiting version takes the place of
    /// the waiting versions of the same proposer.
    ///
    /// `write_files` is given the number before anything is committed: when
    /// it fails, nothing was recorded.
    pub fn app_record_version(
        &mut self,
        new: &NewAppVersion,
        now: u64,
        write_files: impl FnOnce(u32) -> Result<()>,
    ) -> Result<RecordedVersion> {
        if !matches!(new.state, VersionState::Waiting | VersionState::Failed) {
            return Err(Error::Database(bad()));
        }
        let community = new.community.to_string();
        let now = u64_to_i64(now)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT OR IGNORE INTO apps(community_id,name,enabled,created_at,updated_at) VALUES(?1,?2,1,?3,?3)",
            params![community, new.name, now],
        )?;
        let version: u32 = transaction.query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM app_versions WHERE community_id=?1 AND name=?2",
            params![community, new.name],
            |row| row.get(0),
        )?;
        let waiting = new.state == VersionState::Waiting;
        let mut superseded = Vec::new();
        if waiting {
            let mut statement = transaction.prepare(
                "SELECT version FROM app_versions WHERE community_id=?1 AND name=?2 AND state='waiting' AND proposer_kind=?3 AND proposer_id IS ?4 ORDER BY version",
            )?;
            superseded = statement
                .query_map(
                    params![
                        community,
                        new.name,
                        new.proposer.kind.as_str(),
                        new.proposer.id
                    ],
                    |row| row.get::<_, u32>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(statement);
            for earlier in &superseded {
                transaction.execute(
                    "UPDATE app_versions SET state='superseded',detail=?4,decided_at=?5,kept=0 WHERE community_id=?1 AND name=?2 AND version=?3",
                    params![
                        community,
                        new.name,
                        earlier,
                        format!("replaced by version {version}"),
                        now
                    ],
                )?;
            }
        }
        let types = new
            .types
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| Error::Serialization(error.to_string()))?;
        transaction.execute(
            &format!(
                "INSERT INTO app_versions({VERSION_COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"
            ),
            params![
                community,
                new.name,
                version,
                new.state.as_str(),
                new.detail,
                new.title,
                new.api,
                types,
                new.script_sha256,
                new.manifest_sha256,
                new.brief_sha256,
                u64_to_i64(new.script_bytes)?,
                u64_to_i64(new.brief_bytes)?,
                new.proposer.kind.as_str(),
                new.proposer.id,
                new.proposer.name,
                new.proposer.channel.map(|channel| channel.to_string()),
                now,
                // A version that never loaded is decided as it is recorded.
                (!waiting).then_some(now),
                i64::from(waiting),
            ],
        )?;
        transaction.execute(
            "UPDATE apps SET updated_at=?3 WHERE community_id=?1 AND name=?2",
            params![community, new.name, now],
        )?;
        write_files(version)?;
        transaction.commit()?;
        Ok(RecordedVersion {
            version,
            superseded,
        })
    }

    /// Approves a waiting version, by its number and the hash of its script,
    /// and makes it the version that runs. False when no waiting version has
    /// that number and that hash: nothing changed then.
    pub fn app_approve(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        script_sha256: &str,
        now: u64,
    ) -> Result<bool> {
        let community = community.to_string();
        let now = u64_to_i64(now)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let approved = transaction.execute(
            "UPDATE app_versions SET state='approved',detail=NULL,decided_at=?5 WHERE community_id=?1 AND name=?2 AND version=?3 AND state='waiting' AND script_sha256=?4",
            params![community, name, version, script_sha256, now],
        )?;
        if approved == 0 {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE apps SET previous_version=current_version,current_version=?3,updated_at=?4 WHERE community_id=?1 AND name=?2",
            params![community, name, version, now],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Refuses a waiting version. False when it was not waiting.
    pub fn app_refuse(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        reason: Option<&str>,
        now: u64,
    ) -> Result<bool> {
        let refused = self.connection.execute(
            "UPDATE app_versions SET state='refused',detail=?4,decided_at=?5,kept=0 WHERE community_id=?1 AND name=?2 AND version=?3 AND state='waiting'",
            params![community.to_string(), name, version, reason, u64_to_i64(now)?],
        )?;
        Ok(refused > 0)
    }

    /// Marks a version as failed, with the error. If it was the one that
    /// runs, the app goes back to the version that ran before it, when that
    /// one is still approved and kept. Answers the version that runs now.
    pub fn app_fail_version(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        error: &str,
        now: u64,
    ) -> Result<Option<u32>> {
        let community = community.to_string();
        let now = u64_to_i64(now)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE app_versions SET state='failed',detail=?4,decided_at=?5,kept=0 WHERE community_id=?1 AND name=?2 AND version=?3 AND state IN ('waiting','approved')",
            params![community, name, version, error, now],
        )?;
        transaction.execute(
            "UPDATE apps SET
               current_version=(SELECT v.version FROM app_versions v WHERE v.community_id=apps.community_id AND v.name=apps.name
                                AND v.version=apps.previous_version AND v.state='approved' AND v.kept=1),
               previous_version=NULL,updated_at=?4
             WHERE community_id=?1 AND name=?2 AND current_version=?3",
            params![community, name, version, now],
        )?;
        let current = transaction
            .query_row(
                "SELECT current_version FROM apps WHERE community_id=?1 AND name=?2",
                params![community, name],
                |row| row.get::<_, Option<u32>>(0),
            )
            .optional()?
            .flatten();
        transaction.commit()?;
        Ok(current)
    }

    /// Makes an approved version whose files are kept the one that runs.
    /// False when the version is not such a one: nothing changed then.
    pub fn app_use_version(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        now: u64,
    ) -> Result<bool> {
        let used = self.connection.execute(
            "UPDATE apps SET previous_version=current_version,current_version=?3,updated_at=?4
             WHERE community_id=?1 AND name=?2
               AND EXISTS(SELECT 1 FROM app_versions v WHERE v.community_id=?1 AND v.name=?2 AND v.version=?3 AND v.state='approved' AND v.kept=1)",
            params![community.to_string(), name, version, u64_to_i64(now)?],
        )?;
        Ok(used > 0)
    }

    pub fn app_set_enabled(
        &mut self,
        community: Uuid,
        name: &str,
        enabled: bool,
        now: u64,
    ) -> Result<bool> {
        let changed = self.connection.execute(
            "UPDATE apps SET enabled=?3,updated_at=?4 WHERE community_id=?1 AND name=?2",
            params![
                community.to_string(),
                name,
                i64::from(enabled),
                u64_to_i64(now)?
            ],
        )?;
        Ok(changed > 0)
    }

    /// Remembers that an app is open in a channel. One instance for each
    /// channel: opening it again changes nothing.
    pub fn app_open_in(
        &mut self,
        community: Uuid,
        name: &str,
        channel: Uuid,
        now: u64,
    ) -> Result<bool> {
        let opened = self.connection.execute(
            "INSERT OR IGNORE INTO app_channels(community_id,name,channel_id,opened_at) VALUES(?1,?2,?3,?4)",
            params![
                community.to_string(),
                name,
                channel.to_string(),
                u64_to_i64(now)?
            ],
        )?;
        Ok(opened > 0)
    }

    pub fn app_close_in(&mut self, community: Uuid, name: &str, channel: Uuid) -> Result<bool> {
        let closed = self.connection.execute(
            "DELETE FROM app_channels WHERE community_id=?1 AND name=?2 AND channel_id=?3",
            params![community.to_string(), name, channel.to_string()],
        )?;
        Ok(closed > 0)
    }

    /// The channels an app is open in, the one opened first first.
    pub fn app_channels(&self, community: Uuid, name: &str) -> Result<Vec<Uuid>> {
        let mut statement = self.connection.prepare(
            "SELECT channel_id FROM app_channels WHERE community_id=?1 AND name=?2 ORDER BY opened_at,channel_id",
        )?;
        let rows = statement
            .query_map(params![community.to_string(), name], |row| {
                uuid_of(&row.get::<_, String>(0)?)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The apps open in a channel, by name.
    pub fn apps_open_in(&self, community: Uuid, channel: Uuid) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT name FROM app_channels WHERE community_id=?1 AND channel_id=?2 ORDER BY name",
        )?;
        let rows = statement
            .query_map(params![community.to_string(), channel.to_string()], |row| {
                row.get(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Lets go of what need not be kept of an app's versions. The files of
    /// the `keep_approved` newest approved versions stay, and those of the
    /// version that runs, the one before it and every waiting one; the
    /// others are marked as no longer kept. Of the versions without files
    /// only the `keep_lines` newest keep their line.
    pub fn app_prune_versions(
        &mut self,
        community: Uuid,
        name: &str,
        keep_approved: usize,
        keep_lines: usize,
    ) -> Result<()> {
        let community = community.to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE app_versions SET kept=0
             WHERE community_id=?1 AND name=?2 AND state='approved' AND kept=1
               AND version NOT IN (SELECT version FROM app_versions WHERE community_id=?1 AND name=?2 AND state='approved' AND kept=1 ORDER BY version DESC LIMIT ?3)
               AND version NOT IN (SELECT COALESCE(current_version,0) FROM apps WHERE community_id=?1 AND name=?2
                                   UNION SELECT COALESCE(previous_version,0) FROM apps WHERE community_id=?1 AND name=?2)",
            params![community, name, i64::try_from(keep_approved).unwrap_or(i64::MAX)],
        )?;
        transaction.execute(
            "DELETE FROM app_versions
             WHERE community_id=?1 AND name=?2 AND kept=0
               AND version NOT IN (SELECT version FROM app_versions WHERE community_id=?1 AND name=?2 AND kept=0 ORDER BY version DESC LIMIT ?3)",
            params![community, name, i64::try_from(keep_lines).unwrap_or(i64::MAX)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Forgets an app: its row, its versions and where it was open. The
    /// channel's records are rows of `artifacts` and are not touched.
    pub fn app_remove(&mut self, community: Uuid, name: &str) -> Result<bool> {
        let removed = self.connection.execute(
            "DELETE FROM apps WHERE community_id=?1 AND name=?2",
            params![community.to_string(), name],
        )?;
        Ok(removed > 0)
    }

    /// How many live records of an app's types this bzz has stored, and in
    /// how many channels: `(records, channels)`.
    pub fn app_record_count(&self, community: Uuid, name: &str) -> Result<(usize, usize)> {
        let prefix = format!("{APP_RECORD_TYPE_PREFIX}{name}.");
        let (records, channels): (i64, i64) = self.connection.query_row(
            "SELECT COUNT(*),COUNT(DISTINCT home) FROM artifacts
             WHERE community_id=?1 AND op<>'delete' AND substr(artifact_type,1,?3)=?2",
            params![
                community.to_string(),
                prefix,
                i64::try_from(prefix.len()).unwrap_or(i64::MAX)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((
            usize::try_from(records).unwrap_or(0),
            usize::try_from(channels).unwrap_or(0),
        ))
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

    fn version(community: Uuid, script: char, proposer: Proposer) -> NewAppVersion {
        NewAppVersion {
            community,
            name: "board".into(),
            state: VersionState::Waiting,
            detail: None,
            title: Some("Board".into()),
            api: Some(1),
            types: Some(vec!["card".into()]),
            script_sha256: script.to_string().repeat(64),
            manifest_sha256: Some("b".repeat(64)),
            brief_sha256: "c".repeat(64),
            script_bytes: 10,
            brief_bytes: 20,
            proposer,
        }
    }

    #[test]
    fn a_version_is_numbered_recorded_and_read_back_as_it_was() {
        let (mut store, community) = store();
        let channel = Uuid::new_v4();
        let agent = Proposer::agent("d".repeat(64), "Scout").in_channel(channel);
        let recorded = store
            .app_record_version(&version(community, '1', agent.clone()), 100, |_| Ok(()))
            .unwrap();
        assert_eq!(recorded.version, 1);
        let read = store.app_version(community, "board", 1).unwrap().unwrap();
        assert_eq!(read.state, VersionState::Waiting);
        assert_eq!(read.proposer, agent);
        assert_eq!(read.types, ["card"]);
        assert_eq!(read.proposed_at, 100);
        assert!(read.kept && read.decided_at.is_none());
        let app = store.app(community, "board").unwrap().unwrap();
        assert!(app.enabled && app.current.is_none());
        assert_eq!(store.app_versions_waiting(community).unwrap(), [read]);
    }

    #[test]
    fn files_that_cannot_be_written_leave_nothing_recorded() {
        let (mut store, community) = store();
        let failed =
            store.app_record_version(&version(community, '1', Proposer::owner()), 1, |_| {
                Err(Error::Config("the disk is full".into()))
            });
        assert!(failed.is_err());
        assert!(store.app(community, "board").unwrap().is_none());
        assert!(store.app_versions(community, "board").unwrap().is_empty());
    }

    #[test]
    fn a_proposer_replaces_only_its_own_waiting_version() {
        let (mut store, community) = store();
        let scout = Proposer::agent("d".repeat(64), "Scout");
        let ana = Proposer::member("e".repeat(64), "Ana");
        store
            .app_record_version(&version(community, '1', scout.clone()), 1, |_| Ok(()))
            .unwrap();
        store
            .app_record_version(&version(community, '2', ana), 2, |_| Ok(()))
            .unwrap();
        // The same agent under another name, from another channel.
        let again = Proposer::agent("d".repeat(64), "Scout II").in_channel(Uuid::new_v4());
        let third = store
            .app_record_version(&version(community, '3', again), 3, |_| Ok(()))
            .unwrap();
        assert_eq!((third.version, third.superseded), (3, vec![1]));
        let states = store
            .app_versions(community, "board")
            .unwrap()
            .into_iter()
            .map(|version| (version.version, version.state, version.kept))
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            [
                (3, VersionState::Waiting, true),
                (2, VersionState::Waiting, true),
                (1, VersionState::Superseded, false),
            ]
        );
        assert_eq!(
            store
                .app_version(community, "board", 1)
                .unwrap()
                .unwrap()
                .detail
                .as_deref(),
            Some("replaced by version 3")
        );
    }

    #[test]
    fn approval_is_of_a_number_and_a_hash_and_only_while_it_waits() {
        let (mut store, community) = store();
        store
            .app_record_version(&version(community, '1', Proposer::owner()), 1, |_| Ok(()))
            .unwrap();
        let hash = "1".repeat(64);
        assert!(
            !store
                .app_approve(community, "board", 1, &"9".repeat(64), 5)
                .unwrap(),
            "another hash approves nothing"
        );
        assert!(
            !store.app_approve(community, "board", 2, &hash, 5).unwrap(),
            "another number approves nothing"
        );
        assert!(
            store
                .app(community, "board")
                .unwrap()
                .unwrap()
                .current
                .is_none()
        );
        assert!(store.app_approve(community, "board", 1, &hash, 5).unwrap());
        let app = store.app(community, "board").unwrap().unwrap();
        assert_eq!((app.current, app.previous), (Some(1), None));
        assert!(
            !store.app_approve(community, "board", 1, &hash, 6).unwrap(),
            "an answered version is not answered again"
        );
        assert!(!store.app_refuse(community, "board", 1, None, 6).unwrap());
        assert_eq!(
            store
                .app_version(community, "board", 1)
                .unwrap()
                .unwrap()
                .state,
            VersionState::Approved
        );
    }

    #[test]
    fn a_version_that_fails_as_it_is_put_to_run_gives_way_to_the_one_before() {
        let (mut store, community) = store();
        for (script, at) in [('1', 1), ('2', 2), ('3', 3)] {
            let recorded = store
                .app_record_version(&version(community, script, Proposer::owner()), at, |_| {
                    Ok(())
                })
                .unwrap();
            store
                .app_approve(
                    community,
                    "board",
                    recorded.version,
                    &script.to_string().repeat(64),
                    at,
                )
                .unwrap();
        }
        // Back to 1, then 3 again: what ran before 3 is now 1, not 2.
        assert!(store.app_use_version(community, "board", 1, 4).unwrap());
        assert!(store.app_use_version(community, "board", 3, 5).unwrap());
        let now = store
            .app_fail_version(community, "board", 3, "board.lua:3: boom", 6)
            .unwrap();
        assert_eq!(now, Some(1));
        let failed = store.app_version(community, "board", 3).unwrap().unwrap();
        assert_eq!(failed.state, VersionState::Failed);
        assert_eq!(failed.detail.as_deref(), Some("board.lua:3: boom"));
        assert!(!failed.kept);
        assert!(
            !store.app_use_version(community, "board", 3, 7).unwrap(),
            "a failed version is not gone back to"
        );
        // Failing a version that does not run changes nothing of what runs.
        assert_eq!(
            store
                .app_fail_version(community, "board", 2, "late", 8)
                .unwrap(),
            Some(1)
        );
    }

    #[test]
    fn pruning_keeps_what_runs_what_waits_and_the_newest_approved() {
        let (mut store, community) = store();
        for number in 1..=6_u32 {
            let script = char::from_digit(number, 10).unwrap();
            store
                .app_record_version(
                    &version(community, script, Proposer::owner()),
                    u64::from(number),
                    |_| Ok(()),
                )
                .unwrap();
            if number < 6 {
                store
                    .app_approve(
                        community,
                        "board",
                        number,
                        &script.to_string().repeat(64),
                        u64::from(number),
                    )
                    .unwrap();
            }
        }
        // 1 runs, 5 ran before it, 6 waits.
        store.app_use_version(community, "board", 1, 10).unwrap();
        store.app_prune_versions(community, "board", 2, 1).unwrap();
        let left = store
            .app_versions(community, "board")
            .unwrap()
            .into_iter()
            .map(|version| (version.version, version.kept))
            .collect::<Vec<_>>();
        // 5 and 4 are the two newest approved; 1 runs; 6 waits. Of 3 and 2,
        // which lost their files, one line is kept: the newer.
        assert_eq!(
            left,
            [(6, true), (5, true), (4, true), (3, false), (1, true)]
        );
    }

    #[test]
    fn channels_are_remembered_one_instance_each_and_forgotten_with_the_app() {
        let (mut store, community) = store();
        store
            .app_record_version(&version(community, '1', Proposer::owner()), 1, |_| Ok(()))
            .unwrap();
        let (general, random) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(store.app_open_in(community, "board", general, 1).unwrap());
        assert!(!store.app_open_in(community, "board", general, 2).unwrap());
        assert!(store.app_open_in(community, "board", random, 3).unwrap());
        assert_eq!(
            store.app_channels(community, "board").unwrap(),
            [general, random]
        );
        assert_eq!(store.apps_open_in(community, general).unwrap(), ["board"]);
        assert!(store.app_close_in(community, "board", random).unwrap());
        assert!(store.app_set_enabled(community, "board", false, 4).unwrap());
        assert!(!store.app(community, "board").unwrap().unwrap().enabled);
        assert!(store.app_remove(community, "board").unwrap());
        assert!(store.app_channels(community, "board").unwrap().is_empty());
        assert!(store.app_versions(community, "board").unwrap().is_empty());
        assert!(!store.app_remove(community, "board").unwrap());
    }
}
