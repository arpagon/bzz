use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::Store,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use sha2::{Digest as _, Sha256};
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
    assert_eq!(version, 8);
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
    let attachment_column: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('drafts') WHERE name='attachments_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attachment_column, 1);
    let remote_agents_table: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='remote_agents'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remote_agents_table, 1);
    let agent_membership_index: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='memberships_agent_role'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(agent_membership_index, 1);
    let mention_column: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('drafts') WHERE name='mentions_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(mention_column, 1);
    let draft_revision_column: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('drafts') WHERE name='revision'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let draft_state_column: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('drafts') WHERE name='state'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(draft_revision_column, 1);
    assert_eq!(draft_state_column, 1);
    let fts_table: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='search_fts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        fts_table, 1,
        "SQLite FTS5 must be available on every target"
    );
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
fn version_two_database_upgrades_with_backup_and_fts_rebuild() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    let migration_1 = include_str!("../migrations/0001_init.sql");
    let migration_2 = include_str!("../migrations/0002_media.sql");
    connection.execute_batch(migration_1).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version,sha256,applied_at) VALUES(1,?1,0)",
            [hex::encode(Sha256::digest(migration_1.as_bytes()))],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection.execute_batch(migration_2).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version,sha256,applied_at) VALUES(2,?1,0)",
            [hex::encode(Sha256::digest(migration_2.as_bytes()))],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let source_event = "1".repeat(64);
    connection
        .execute(
            "INSERT INTO identities(id,pubkey,label,key_backend,key_ref,created_at) VALUES(?1,?2,'me','encrypted-file','test',0)",
            rusqlite::params![identity.to_string(), "a".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO communities(id,identity_id,relay_url,authority,http_base_url,label,created_at,updated_at) VALUES(?1,?2,'wss://migration.example','migration.example','https://migration.example','migration',0,0)",
            rusqlite::params![community.to_string(), identity.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO channels(community_id,channel_id,name,visibility,is_member) VALUES(?1,?2,'DM','private',1)",
            rusqlite::params![community.to_string(), channel.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events(community_id,event_id,kind,pubkey,created_at,channel_id,content,tags_json,raw_json,received_at) VALUES(?1,?2,39002,?3,10,?4,'','[]','{}',10)",
            rusqlite::params![community.to_string(), source_event, "b".repeat(64), channel.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO memberships(community_id,channel_id,pubkey,source_event_id) VALUES(?1,?2,?3,?4)",
            rusqlite::params![community.to_string(), channel.to_string(), "a".repeat(64), source_event],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO drafts(community_id,channel_id,thread_root_id,body,attachments_json,updated_at) VALUES(?1,?2,'','legacy draft','[]',0)",
            rusqlite::params![community.to_string(), channel.to_string()],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    store.search_integrity().unwrap();
    drop(store);
    let upgraded = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = upgraded
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 8);
    let inbox_projection_table: u32 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='inbox_conversations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inbox_projection_table, 1);
    let membership_head: String = upgraded
        .query_row(
            "SELECT source_event_id FROM channel_membership_heads WHERE community_id=?1 AND channel_id=?2",
            rusqlite::params![community.to_string(), channel.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(membership_head, source_event);
    let (revision, state, outbox_event_id): (String, String, Option<String>) = upgraded
        .query_row(
            "SELECT revision,state,outbox_event_id FROM drafts WHERE community_id=?1 AND channel_id=?2",
            rusqlite::params![community.to_string(), channel.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(!revision.is_empty());
    assert_eq!(state, "editing");
    assert!(outbox_event_id.is_none());
    assert!(
        std::fs::read_dir(temporary.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".bak"))
    );
}

#[test]
fn schema_eight_repairs_historical_bot_roles_and_duplicate_projection_drift() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let owner = Keys::generate();
    let relay = Keys::generate();
    let agent = Keys::generate();
    let identity_id = Uuid::new_v4();
    let community_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity_id,
            label: "owner".into(),
            pubkey: owner.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "migration-agent".into(),
        }],
        communities: vec![bzz::config::CommunityConfig {
            id: community_id,
            label: "migration".into(),
            relay_url: "wss://migration-agent.example".into(),
            identity_id,
            allow_insecure_localhost: false,
            theme: None,
        }],
        ..Config::default()
    };
    let mut store = Store::open(&path).unwrap();
    store.sync_config(&config).unwrap();
    store
        .pin_relay_pubkey(community_id, &relay.public_key().to_hex())
        .unwrap();
    let metadata = EventBuilder::new(Kind::Custom(39_000), "")
        .tags([
            Tag::parse(["d", &channel_id.to_string()]).unwrap(),
            Tag::parse(["name", "agents"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community_id, &metadata).unwrap();
    let membership = EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel_id.to_string()]).unwrap(),
            Tag::parse(["p", &owner.public_key().to_hex()]).unwrap(),
            Tag::parse(["p", &agent.public_key().to_hex(), "", "bot"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    assert!(store.apply_event(community_id, &membership).unwrap());
    drop(store);

    // Simulate a v0.11.0 database whose already-cached source event was
    // projected before role-aware ingestion existed.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE memberships SET role='member' WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3",
            rusqlite::params![
                community_id.to_string(),
                channel_id.to_string(),
                agent.public_key().to_hex()
            ],
        )
        .unwrap();
    connection
        .execute("DELETE FROM schema_migrations WHERE version=8", [])
        .unwrap();
    connection.pragma_update(None, "user_version", 7).unwrap();
    drop(connection);

    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store.remote_agent_candidate_pubkeys(community_id).unwrap(),
        vec![agent.public_key().to_hex()]
    );
    let event_bytes_before: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT raw_json FROM events WHERE community_id=?1 AND event_id=?2",
            rusqlite::params![community_id.to_string(), membership.id.to_hex()],
            |row| row.get(0),
        )
        .unwrap();

    // A later local projection drift is repaired by the immutable duplicate
    // event, then repeated echoes become true no-ops.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE memberships SET role='member' WHERE community_id=?1 AND channel_id=?2 AND pubkey=?3",
            rusqlite::params![
                community_id.to_string(),
                channel_id.to_string(),
                agent.public_key().to_hex()
            ],
        )
        .unwrap();
    assert!(store.apply_event(community_id, &membership).unwrap());
    assert!(!store.apply_event(community_id, &membership).unwrap());
    assert_eq!(
        store.remote_agent_candidate_pubkeys(community_id).unwrap(),
        vec![agent.public_key().to_hex()]
    );
    let event_bytes_after: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT raw_json FROM events WHERE community_id=?1 AND event_id=?2",
            rusqlite::params![community_id.to_string(), membership.id.to_hex()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_bytes_before, event_bytes_after);
}

#[test]
fn malformed_persisted_mentions_degrade_to_plain_draft_text() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let community = Uuid::new_v4();
    let channel = Uuid::new_v4();
    drop(Store::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    connection
        .execute(
            "INSERT INTO drafts(community_id,channel_id,thread_root_id,body,attachments_json,mentions_json,updated_at) VALUES(?1,?2,'','@Someone','[]',?3,0)",
            rusqlite::params![
                community.to_string(),
                channel.to_string(),
                format!("[{{\"byte_start\":0,\"byte_end\":8,\"pubkey\":\"{}\"}}]", "A".repeat(64)),
            ],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let (body, attachments, mentions) = store
        .draft_with_media_mentions(community, channel, None)
        .unwrap();
    assert_eq!(body, "@Someone");
    assert!(attachments.is_empty());
    assert!(mentions.is_empty());
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
