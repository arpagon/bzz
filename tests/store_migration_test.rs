use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::Store,
};
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn fresh_database_has_expected_pragmas_and_schema() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.path(), path);
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(
        foreign_keys, 1,
        "foreign-key enforcement must remain enabled"
    );
    let checksum: String = connection
        .query_row(
            "SELECT sha256 FROM schema_migrations WHERE version=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(checksum.len(), 64);
    connection
        .execute(
            "UPDATE schema_migrations SET sha256='tampered' WHERE version=1",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(Store::open(&path).is_err());
}

#[test]
fn config_sync_requires_and_preserves_identity_scope() {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "me".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "identity:test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    config
        .add_community("one".into(), "wss://one.example".into(), identity.id, false)
        .unwrap();
    config
        .add_community("two".into(), "wss://two.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    assert!(store.channels(config.communities[0].id).unwrap().is_empty());
    assert!(store.channels(config.communities[1].id).unwrap().is_empty());
}
