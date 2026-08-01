use bzz::{
    config::{Config, validate_relay_url},
    paths::Paths,
};
use tempfile::TempDir;

#[test]
fn secure_and_explicit_loopback_urls_are_valid() {
    let endpoint = validate_relay_url("wss://Buzz.Example", false).unwrap();
    assert_eq!(endpoint.websocket.as_str(), "wss://buzz.example/");
    assert_eq!(endpoint.http_base.as_str(), "https://buzz.example/");
    assert!(validate_relay_url("ws://localhost:3030", false).is_err());
    assert!(validate_relay_url("ws://localhost:3030", true).is_ok());
}

#[test]
fn hostile_url_shapes_are_rejected() {
    for input in [
        "https://buzz.example",
        "wss://user@buzz.example",
        "wss://buzz.example/path",
        "wss://buzz.example/?secret=x",
        "wss://buzz.example/#fragment",
    ] {
        assert!(
            validate_relay_url(input, false).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn unknown_configuration_fields_are_rejected() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("config.toml");
    let secret = "nsec1must-never-appear";
    std::fs::write(
        &path,
        format!("[ui]\nsidebar_width=28\nthread_width=44\nsurprise='{secret}'\n"),
    )
    .unwrap();
    let error = bzz::config::load_from(&path).unwrap_err().to_string();
    assert!(!error.contains(secret));
}

#[test]
fn removing_default_community_selects_the_remaining_one() {
    let identity = bzz::config::IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "me".into(),
        pubkey: "a".repeat(64),
        backend: bzz::config::KeyBackend::EncryptedFile,
        key_ref: "identity:test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let first = config
        .add_community(
            "first".into(),
            "wss://first.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let second = config
        .add_community(
            "second".into(),
            "wss://second.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    assert!(config.remove_community(first));
    assert_eq!(config.default_community, Some(second));
    assert!(!config.remove_community(first));
}

#[test]
fn community_theme_overrides_global_theme_and_can_inherit_again() {
    let identity = bzz::config::IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "me".into(),
        pubkey: "a".repeat(64),
        backend: bzz::config::KeyBackend::EncryptedFile,
        key_ref: "identity:test".into(),
    };
    let mut config = Config::default();
    config.ui.theme = "nord".into();
    config.identities.push(identity.clone());
    config
        .add_community(
            "first".into(),
            "wss://first.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    assert_eq!(config.resolved_theme(0), "nord");
    config.communities[0].theme = Some("dracula".into());
    assert_eq!(config.resolved_theme(0), "dracula");
    config.communities[0].theme = None;
    assert_eq!(config.resolved_theme(0), "nord");
}

#[test]
fn empty_config_round_trips_with_private_paths() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    let config = Config::default();
    config.save(&paths).unwrap();
    config.save(&paths).unwrap();
    assert_eq!(Config::load(&paths).unwrap(), config);
    assert!(!paths.config_file().with_extension("toml.tmp").exists());
}
