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
    assert_eq!(version, 23);
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
    assert_eq!(version, 23);
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
            identity_adapter: None,
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
        .execute_batch(
            "DELETE FROM schema_migrations WHERE version IN (8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23);
             DROP TABLE app_channels;
             DROP TABLE app_versions;
             DROP TABLE apps;
             DROP INDEX memberships_by_member;
             DROP TABLE agent_replies;
             DROP TABLE agent_inbox;
             DROP TABLE agent_sessions;
             DROP TABLE agent_cursors;
             DROP TABLE sidebar_documents;
             DROP TABLE edit_targets;
             DROP TABLE viewer_memberships;
             DROP TABLE verified_profiles;
             DROP TABLE user_statuses;
             DROP TABLE artifacts;
             DROP TABLE emoji_contributions;
             ALTER TABLE reactions DROP COLUMN emoji_url;
             ALTER TABLE channels DROP COLUMN facts_json;",
        )
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

#[test]
fn schema_nine_projects_guest_roles_and_repairs_channel_facts_on_reingest() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let owner = Keys::generate();
    let relay = Keys::generate();
    let identity_id = Uuid::new_v4();
    let community_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity_id,
            label: "owner".into(),
            pubkey: owner.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "migration-guest".into(),
        }],
        communities: vec![bzz::config::CommunityConfig {
            id: community_id,
            label: "migration".into(),
            relay_url: "wss://migration-guest.example".into(),
            identity_id,
            allow_insecure_localhost: false,
            theme: None,
            identity_adapter: None,
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
            Tag::parse(["name", "announcements"]).unwrap(),
            Tag::parse(["public"]).unwrap(),
            Tag::parse(["t", "stream"]).unwrap(),
            Tag::parse(["t", "releases"]).unwrap(),
            Tag::parse(["join_role", "guest"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community_id, &metadata).unwrap();
    let membership = EventBuilder::new(Kind::Custom(39_002), "")
        .tags([
            Tag::parse(["d", &channel_id.to_string()]).unwrap(),
            Tag::parse(["p", &owner.public_key().to_hex(), "", "guest"]).unwrap(),
        ])
        .sign_with_keys(&relay)
        .unwrap();
    store.apply_event(community_id, &membership).unwrap();
    drop(store);

    // Simulate a v0.12 (schema 8) database: no facts column and the guest
    // collapsed into the old `member` projection.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "UPDATE memberships SET role='member' WHERE channel_id='{channel_id}';
             ALTER TABLE channels DROP COLUMN facts_json;
             DROP TABLE app_channels;
             DROP TABLE app_versions;
             DROP TABLE apps;
             DROP TABLE agent_replies;
             DROP TABLE agent_inbox;
             DROP TABLE agent_sessions;
             DROP TABLE agent_cursors;
             DROP TABLE sidebar_documents;
             DROP TABLE edit_targets;
             DROP TABLE viewer_memberships;
             DROP TABLE verified_profiles;
             DROP TABLE user_statuses;
             DROP TABLE artifacts;
             DROP TABLE emoji_contributions;
             ALTER TABLE reactions DROP COLUMN emoji_url;
             DROP INDEX memberships_by_member;
             DELETE FROM schema_migrations WHERE version IN (9,10,11,12,13,14,15,16,17,18,19,20,21,22,23);"
        ))
        .unwrap();
    connection.pragma_update(None, "user_version", 8).unwrap();
    drop(connection);

    let mut store = Store::open(&path).unwrap();
    let channel = store
        .channels(community_id)
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == channel_id)
        .unwrap();
    assert_eq!(
        channel.facts.viewer_role,
        Some(bzz::domain::MemberRole::Guest),
        "migration 0009 re-derives the guest role from the trusted head"
    );
    assert!(channel.facts.labels.is_empty(), "facts start unknown");
    assert_eq!(
        channel.read_only_reason(),
        Some(bzz::domain::ReadOnlyReason::Guest)
    );

    // The next directory refresh re-ingests the same 39000 and repairs facts.
    assert!(store.apply_event(community_id, &metadata).unwrap());
    assert!(!store.apply_event(community_id, &metadata).unwrap());
    let channel = store
        .channels(community_id)
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == channel_id)
        .unwrap();
    assert_eq!(channel.facts.labels, vec!["releases"]);
    assert_eq!(channel.facts.join_role, bzz::domain::JoinRole::Guest);
}

#[test]
fn schema_eleven_accepts_buzz_desktop_identities_with_their_communities() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let identity_id = Uuid::new_v4();
    let community_id = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity_id,
            label: "desktop".into(),
            pubkey: "d".repeat(64),
            backend: KeyBackend::BuzzDesktop,
            key_ref: bzz::auth::desktop_reference("dev.test.buzz"),
        }],
        communities: vec![bzz::config::CommunityConfig {
            id: community_id,
            label: "desktop".into(),
            relay_url: "wss://desktop.example".into(),
            identity_id,
            allow_insecure_localhost: false,
            theme: None,
            identity_adapter: None,
        }],
        ..Config::default()
    };
    let mut store = Store::open(&path).unwrap();
    store.sync_config(&config).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let (backend, community): (String, String) = connection
        .query_row(
            "SELECT i.key_backend,c.id FROM identities i JOIN communities c ON c.identity_id=i.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(backend, "buzz-desktop");
    assert_eq!(community, community_id.to_string());
    let violations: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

/// Migration 20 (after the run against Buzz 1.0): a stored reply can be
/// marked as one that will never be published. A database of schema 19,
/// with a reply the relay had not acknowledged, keeps that reply as it was:
/// neither accepted nor refused, and so still published again.
#[test]
fn schema_nineteen_keeps_the_stored_replies_of_schema_eighteen() {
    use bzz::store::agent_runtime::{NewRequest, ReplyOrigin, ReplyStanding};
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let agent = Uuid::new_v4();
    let mut store = Store::open(&path).unwrap();
    for (request, published) in [("m1", false), ("m2", true)] {
        store
            .agent_inbox_offer(&NewRequest {
                agent_id: agent,
                request_id: request.into(),
                community_id: Uuid::nil(),
                channel_id: Uuid::nil(),
                place: Uuid::nil().to_string(),
                author: "a".repeat(64),
                created_at: 10,
                event_json: "{}".into(),
            })
            .unwrap();
        store
            .agent_reply_claim(agent, request, "r", "{}", ReplyOrigin::Send, None)
            .unwrap();
        if published {
            store.agent_reply_published(agent, request).unwrap();
        }
    }
    drop(store);
    // The database as schema 19 left it.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP INDEX agent_replies_unpublished;
             ALTER TABLE agent_replies DROP COLUMN refused_at;
             ALTER TABLE agent_replies DROP COLUMN refusal;
             CREATE INDEX agent_replies_unpublished ON agent_replies (agent_id) WHERE published_at IS NULL;
             DROP TABLE app_channels;
             DROP TABLE app_versions;
             DROP TABLE apps;
             DELETE FROM schema_migrations WHERE version IN (20,21,22,23);",
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 19).unwrap();
    drop(connection);

    let mut store = Store::open(&path).unwrap();
    let standing = |store: &Store, request: &str| {
        store
            .agent_reply_get(agent, request)
            .unwrap()
            .unwrap()
            .standing()
    };
    assert_eq!(standing(&store, "m1"), ReplyStanding::Pending);
    assert_eq!(standing(&store, "m2"), ReplyStanding::Accepted);
    assert_eq!(store.agent_replies_unpublished(agent, 0).unwrap().len(), 1);
    assert!(
        store
            .agent_reply_refused(agent, "m1", "r", "reply-refused")
            .unwrap()
    );
    assert_eq!(standing(&store, "m1"), ReplyStanding::Refused);
    assert!(
        store
            .agent_replies_unpublished(agent, 0)
            .unwrap()
            .is_empty()
    );
    drop(store);
    let version: u32 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 23);
}

/// Every row of every table, as text, for comparing a database with itself
/// after a migration. The search index is left out: opening a store
/// maintains it.
fn dump(path: &std::path::Path) -> std::collections::BTreeMap<String, Vec<String>> {
    let connection = rusqlite::Connection::open(path).unwrap();
    let tables = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'search_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .unwrap();
            let columns = statement.column_count();
            let mut rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|index| {
                            row.get::<_, rusqlite::types::Value>(index)
                                .map(|value| format!("{value:?}"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(|cells| cells.join("|"))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows.sort();
            (table, rows)
        })
        .collect()
}

/// Migration 21 (v0.16, apps): a database as v0.15.0 left it, at schema 20
/// with an identity, a community, an agent's request and stored reply, and
/// an artifact, opens at schema 21. A backup at schema 20 is made first;
/// every row of every table v0.15.0 had is as it was; the three tables of
/// apps exist and are empty; and an app can be recorded in them.
#[test]
fn schema_twenty_of_v015_gains_the_apps_tables_and_keeps_every_row() {
    use bzz::store::agent_runtime::{NewRequest, ReplyOrigin, ReplyStanding};
    use bzz::store::apps::{NewAppVersion, Proposer, VersionState};
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "owner".into(),
        pubkey: Keys::generate().public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "owner".into(),
    };
    let mut config = Config {
        identities: vec![identity.clone()],
        ..Config::default()
    };
    let community = config
        .add_community("c".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let agent = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let task = {
        let tags = [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &channel.to_string()],
            ["type", "buzz.task"],
            ["title", "Kept across the upgrade"],
            ["op", "create"],
        ]
        .map(|parts| Tag::parse(parts).unwrap());
        bzz::protocol::artifact::parse(
            &EventBuilder::new(Kind::Custom(45_010), "{}")
                .tags(tags)
                .sign_with_keys(&Keys::generate())
                .unwrap(),
        )
        .unwrap()
    };
    let mut store = Store::open(&path).unwrap();
    store.sync_config(&config).unwrap();
    store
        .agent_inbox_offer(&NewRequest {
            agent_id: agent,
            request_id: "m1".into(),
            community_id: community,
            channel_id: channel,
            place: channel.to_string(),
            author: "a".repeat(64),
            created_at: 10,
            event_json: "{}".into(),
        })
        .unwrap();
    store
        .agent_reply_claim(agent, "m1", "r", "{}", ReplyOrigin::Send, None)
        .unwrap();
    store.apply_artifact_head(community, &task).unwrap();
    drop(store);

    // The database as schema 20 (v0.15.0) left it.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE app_channels; DROP TABLE app_versions; DROP TABLE apps;
             DELETE FROM schema_migrations WHERE version IN (21,22,23);",
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 20).unwrap();
    drop(connection);
    for entry in std::fs::read_dir(temporary.path()).unwrap().flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .contains("pre-migration")
        {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    let before = dump(&path);
    assert!(!before.contains_key("apps"), "{:?}", before.keys());
    assert_eq!(before["agent_replies"].len(), 1);
    assert_eq!(before["artifacts"].len(), 1);
    assert_eq!(before["schema_migrations"].len(), 20);

    let mut store = Store::open(&path).unwrap();
    // The backup made before the migration is the database at schema 20.
    let backup = std::fs::read_dir(temporary.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|candidate| {
            candidate
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("pre-migration"))
        })
        .expect("a backup made before migration 21");
    let backed_up: u32 = rusqlite::Connection::open(&backup)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(backed_up, 20);

    // Old data, read through the store as v0.15 code reads it.
    assert_eq!(
        store
            .agent_reply_get(agent, "m1")
            .unwrap()
            .unwrap()
            .standing(),
        ReplyStanding::Pending
    );
    assert_eq!(
        store
            .artifact(community, task.id)
            .unwrap()
            .unwrap()
            .head
            .title,
        task.title
    );
    // Apps: nothing is installed, and the tables take a version.
    assert!(store.apps(community).unwrap().is_empty());
    let recorded = store
        .app_record_version(
            &NewAppVersion {
                community,
                name: "board".into(),
                state: VersionState::Waiting,
                detail: None,
                title: Some("Board".into()),
                api: Some(1),
                types: Some(vec!["card".into()]),
                script_sha256: "1".repeat(64),
                manifest_sha256: Some("2".repeat(64)),
                brief_sha256: "3".repeat(64),
                script_bytes: 1,
                brief_bytes: 1,
                proposer: Proposer::owner(),
            },
            1,
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(recorded.version, 1);
    assert!(store.app_remove(community, "board").unwrap());
    drop(store);

    // Every row of every table v0.15.0 had is as it was.
    let after = dump(&path);
    for (table, rows) in &before {
        if table == "schema_migrations" {
            assert_eq!(after[table].len(), 22);
            assert!(rows.iter().all(|row| after[table].contains(row)));
        } else {
            assert_eq!(&after[table], rows, "table {table}");
        }
    }
    let new: Vec<&String> = after
        .keys()
        .filter(|table| !before.contains_key(*table))
        .collect();
    assert_eq!(new, ["app_channels", "app_versions", "apps"]);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 23);
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let violations: u32 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

/// Migration 23 (v0.16 M5, agents write apps): a database at the schema
/// before it (22, M6's, on v0.16.0) with an app whose version an agent
/// proposed and the owner answered opens at schema 23. The version reads back as it was, with nobody named
/// as having asked for it, and counts as told: no agent is told of
/// something from before it could be told anything.
#[test]
fn the_schema_before_twenty_three_gains_who_asked_and_counts_every_version_as_told() {
    use bzz::store::apps::{NewAppVersion, Proposer, VersionState};
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "owner".into(),
        pubkey: Keys::generate().public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "owner".into(),
    };
    let mut config = Config {
        identities: vec![identity.clone()],
        ..Config::default()
    };
    let community = config
        .add_community("c".into(), "ws://localhost:3030".into(), identity.id, true)
        .unwrap();
    let channel = Uuid::new_v4();
    let agent = "d".repeat(64);
    let mut store = Store::open(&path).unwrap();
    store.sync_config(&config).unwrap();
    store
        .app_record_version(
            &NewAppVersion {
                community,
                name: "board".into(),
                state: VersionState::Waiting,
                detail: None,
                title: Some("Board".into()),
                api: Some(1),
                types: Some(vec!["card".into()]),
                script_sha256: "1".repeat(64),
                manifest_sha256: Some("2".repeat(64)),
                brief_sha256: "3".repeat(64),
                script_bytes: 1,
                brief_bytes: 1,
                proposer: Proposer::agent(agent.clone(), "Scout").in_channel(channel),
            },
            1,
            |_| Ok(()),
        )
        .unwrap();
    assert!(
        store
            .app_approve(community, "board", 1, &"1".repeat(64), 2)
            .unwrap()
    );
    drop(store);
    // The database as the schema before 23 left it.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP INDEX app_versions_untold;
             ALTER TABLE app_versions DROP COLUMN told_at;
             ALTER TABLE app_versions DROP COLUMN asked_by_name;
             ALTER TABLE app_versions DROP COLUMN asked_by_id;
             DELETE FROM schema_migrations WHERE version=23;",
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 22).unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let version = store.app_version(community, "board", 1).unwrap().unwrap();
    assert_eq!(version.state, VersionState::Approved);
    assert_eq!(
        version.proposer,
        Proposer::agent(agent.clone(), "Scout").in_channel(channel)
    );
    assert_eq!(version.proposer.asked_by, None);
    assert_eq!(version.told_at, Some(2), "told as of when it was answered");
    assert!(
        store
            .app_versions_untold(community, &agent, channel)
            .unwrap()
            .is_empty()
    );
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let schema: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(schema, 23);
}
