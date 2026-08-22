use std::{fs, path::Path};

use rusqlite::{Connection, backup::Backup, params};
use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};

const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../../migrations/0001_init.sql")),
    (2, include_str!("../../migrations/0002_media.sql")),
    (3, include_str!("../../migrations/0003_inbox_dm_search.sql")),
    (4, include_str!("../../migrations/0004_mentions.sql")),
    (
        5,
        include_str!("../../migrations/0005_inbox_conversations.sql"),
    ),
    (
        6,
        include_str!("../../migrations/0006_draft_submission_state.sql"),
    ),
];

pub fn configure(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
    )?;
    Ok(())
}

pub fn migrate(connection: &mut Connection, database_path: &Path) -> Result<()> {
    let current: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > MIGRATIONS.last().map_or(0, |(version, _)| *version) {
        return Err(Error::Database(rusqlite::Error::InvalidQuery));
    }
    if current > 0 {
        verify_applied_checksums(connection)?;
    }
    if current < MIGRATIONS.last().map_or(0, |(version, _)| *version)
        && database_path.exists()
        && fs::metadata(database_path).is_ok_and(|metadata| metadata.len() > 0)
    {
        backup(connection, database_path)?;
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version > current) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version,sha256,applied_at) VALUES(?1,?2,unixepoch())",
            params![version, checksum(sql)],
        )?;
        transaction.pragma_update(None, "user_version", version)?;
        transaction.commit()?;
    }
    verify_applied_checksums(connection)
}

fn verify_applied_checksums(connection: &Connection) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::Config(
            "database has a schema version but no migration ledger".into(),
        ));
    }
    for (version, sql) in MIGRATIONS {
        let stored = connection.query_row(
            "SELECT sha256 FROM schema_migrations WHERE version=?1",
            [version],
            |row| row.get::<_, String>(0),
        );
        match stored {
            Ok(value) if value == checksum(sql) => {}
            Ok(_) => {
                return Err(Error::Config(format!(
                    "migration {version} checksum changed"
                )));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn backup(connection: &Connection, database_path: &Path) -> Result<()> {
    let backup_path = database_path.with_extension(format!("pre-migration-{}.bak", unix_now()));
    let mut destination = Connection::open(&backup_path)?;
    let backup = Backup::new(connection, &mut destination)?;
    backup.run_to_completion(16, std::time::Duration::from_millis(10), None)?;
    drop(backup);
    crate::paths::set_private_permissions(&backup_path)?;
    prune_backups(database_path)?;
    Ok(())
}

fn prune_backups(database_path: &Path) -> Result<()> {
    let Some(parent) = database_path.parent() else {
        return Ok(());
    };
    let Some(stem) = database_path.file_stem().and_then(|value| value.to_str()) else {
        return Ok(());
    };
    let mut backups = fs::read_dir(parent)
        .map_err(|error| Error::io(parent, error))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{stem}.pre-migration-"))
        })
        .collect::<Vec<_>>();
    backups.sort_by_key(|entry| entry.file_name());
    let remove = backups.len().saturating_sub(2);
    for entry in backups.into_iter().take(remove) {
        fs::remove_file(entry.path()).map_err(|error| Error::io(entry.path(), error))?;
    }
    Ok(())
}

fn checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.as_bytes()))
}
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_backup_is_a_readable_database_copy() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("bzz.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('safe');")
            .unwrap();
        backup(&connection, &path).unwrap();
        let backup_path = fs::read_dir(temporary.path())
            .unwrap()
            .map(std::result::Result::unwrap)
            .map(|entry| entry.path())
            .find(|candidate| candidate.extension().is_some_and(|value| value == "bak"))
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&backup_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let backup = Connection::open(backup_path).unwrap();
        let marker: String = backup
            .query_row("SELECT value FROM marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marker, "safe");
    }
}
