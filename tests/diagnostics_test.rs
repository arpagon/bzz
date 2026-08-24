use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    diagnostics::report,
    paths::Paths,
    store::{Store, models::OutboxState},
};
use nostr::{EventBuilder, JsonUtil as _, Keys, Kind, Tag};
use tempfile::TempDir;
use uuid::Uuid;

fn fixture() -> (TempDir, Paths, Uuid, Uuid, nostr::Event) {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let keys = Keys::generate();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "diagnostic-owner".into(),
        pubkey: keys.public_key().to_hex(),
        backend: KeyBackend::Keychain,
        key_ref: "identity:diagnostic-owner".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "private community label".into(),
            "wss://diagnostics.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let channel = Uuid::new_v4();
    let event = EventBuilder::new(
        Kind::Custom(9),
        "SENTINEL-MESSAGE-CONTENT must never enter diagnostics",
    )
    .tags([Tag::parse(["h", &channel.to_string()]).unwrap()])
    .sign_with_keys(&keys)
    .unwrap();
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    store.insert_outbox(community, &event).unwrap();
    store
        .set_outbox_state(
            community,
            &event.id.to_hex(),
            OutboxState::Unknown,
            Some("SENTINEL-RAW-ERROR /private/source/path nsec1secret"),
        )
        .unwrap();
    drop(store);
    (temporary, paths, community, channel, event)
}

#[test]
fn metadata_only_outbox_projection_never_returns_event_json_or_raw_errors() {
    let (_temporary, paths, community, _channel, event) = fixture();
    let database_before = std::fs::read(paths.database_file()).unwrap();
    let rows = report::load_outbox(&paths, Some(community)).unwrap();
    let database_after = std::fs::read(paths.database_file()).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_id, event.id.to_hex());
    assert_eq!(rows[0].state, OutboxState::Unknown);
    let json = serde_json::to_string(&report::outbox_view(&rows)).unwrap();
    assert!(!json.contains("SENTINEL"));
    assert!(!json.contains("private/source"));
    assert!(!json.contains("event_json"));
    assert_eq!(database_before, database_after);
}

#[test]
fn support_report_excludes_message_identity_scope_and_raw_error_data() {
    let (_temporary, paths, _community, channel, event) = fixture();
    let output = paths.data_dir.join("support-report.json");
    report::create_report(&paths, &output).unwrap();
    let text = std::fs::read_to_string(output).unwrap();
    let channel_id = channel.to_string();
    let event_json = event.as_json();

    for prohibited in [
        "SENTINEL-MESSAGE-CONTENT",
        "SENTINEL-RAW-ERROR",
        "private community label",
        channel_id.as_str(),
        "nsec1secret",
        "private/source/path",
        event_json.as_str(),
    ] {
        assert!(!text.contains(prohibited));
    }
    assert!(text.contains(&event.id.to_hex()));
    assert!(text.contains("unknown"));
}
