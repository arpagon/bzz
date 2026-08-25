pub mod agents;
pub mod dm;
pub mod events;
pub mod inbox;
pub mod migrate;
pub mod models;
pub mod queries;
pub mod search;
pub mod search_queries;
pub mod writer;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::{
    config::{Config, KeyBackend, validate_relay_url},
    error::Result,
    paths::set_private_permissions,
};

pub struct Store {
    pub(crate) connection: Connection,
    path: PathBuf,
    diagnostics: crate::diagnostics::DiagnosticHandle,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| crate::Error::io(parent, error))?;
        }
        let mut connection = Connection::open(&path)?;
        migrate::configure(&connection)?;
        let migrated = migrate::migrate(&mut connection, &path)?;
        set_private_permissions(&path)?;
        let mut store = Self {
            connection,
            path,
            diagnostics: crate::diagnostics::DiagnosticHandle::disabled(),
        };
        store.ensure_search_projections()?;
        store.reconcile_draft_submissions()?;
        if migrated {
            store.reconcile_all_remote_agents()?;
        }
        Ok(store)
    }

    /// Opens the existing database without migrations, projection repair, or
    /// write capability. Operator diagnostics use this path so inspection
    /// cannot mutate conversation or outbox state.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "query_only", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(Self {
            connection,
            path,
            diagnostics: crate::diagnostics::DiagnosticHandle::disabled(),
        })
    }

    pub fn open_memory() -> Result<Self> {
        let mut connection = Connection::open_in_memory()?;
        migrate::configure(&connection)?;
        let migrated = migrate::migrate(&mut connection, Path::new(":memory:"))?;
        let mut store = Self {
            connection,
            path: PathBuf::from(":memory:"),
            diagnostics: crate::diagnostics::DiagnosticHandle::disabled(),
        };
        store.ensure_search_projections()?;
        store.reconcile_draft_submissions()?;
        if migrated {
            store.reconcile_all_remote_agents()?;
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_diagnostics(&mut self, diagnostics: crate::diagnostics::DiagnosticHandle) {
        self.diagnostics = diagnostics;
    }

    pub fn sync_config(&mut self, config: &Config) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for identity in &config.identities {
            let backend = match identity.backend {
                KeyBackend::Keychain => "keychain",
                KeyBackend::EncryptedFile => "encrypted-file",
            };
            transaction.execute(
                "INSERT INTO identities(id,pubkey,label,key_backend,key_ref,created_at) VALUES(?1,?2,?3,?4,?5,unixepoch()) ON CONFLICT(id) DO UPDATE SET pubkey=excluded.pubkey,label=excluded.label,key_backend=excluded.key_backend,key_ref=excluded.key_ref",
                params![identity.id.to_string(),identity.pubkey,identity.label,backend,identity.key_ref],
            )?;
        }
        for community in &config.communities {
            let endpoint =
                validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
            transaction.execute(
                "INSERT INTO communities(id,identity_id,relay_url,authority,http_base_url,label,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,unixepoch(),unixepoch())
                 ON CONFLICT(id) DO UPDATE SET identity_id=excluded.identity_id,relay_url=excluded.relay_url,authority=excluded.authority,http_base_url=excluded.http_base_url,label=excluded.label,updated_at=unixepoch()",
                params![community.id.to_string(),community.identity_id.to_string(),endpoint.websocket.as_str(),endpoint.authority,endpoint.http_base.as_str(),community.label],
            )?;
        }
        transaction.commit()?;
        for community in &config.communities {
            self.mark_inbox_projection_dirty(community.id)?;
        }
        Ok(())
    }
}
