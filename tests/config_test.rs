use bzz::{
    config::{ChannelSort, ClipboardMode, Config, MouseMode, validate_relay_url},
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
fn media_limits_and_unknown_fields_fail_closed() {
    let mut config = Config::default();
    config.media.max_inline_rows = 1;
    assert!(config.validate().is_err());
    config.media.max_inline_rows = 12;
    config.media.download_concurrency = 17;
    assert!(config.validate().is_err());

    let mut config = Config::default();
    config.ui.message_width = 47;
    assert!(config.validate().is_err());
    config.ui.message_width = 201;
    assert!(config.validate().is_err());

    let parsed = toml::from_str::<Config>(
        "[media]\nenabled=true\nprotocol='auto'\nautoload='visible'\nunknown=true\n",
    );
    assert!(parsed.is_err());
}

#[test]
fn mouse_policy_parses_strictly() {
    for (value, expected) in [
        ("auto", MouseMode::Auto),
        ("on", MouseMode::On),
        ("off", MouseMode::Off),
    ] {
        let config = toml::from_str::<Config>(&format!(
            "[ui]\nsidebar_width=28\nthread_width=44\ntheme='bzz'\nmouse='{value}'\n"
        ))
        .unwrap();
        assert_eq!(config.ui.mouse, expected);
        assert_eq!(config.ui.message_width, 110);
        assert_eq!(config.ui.channel_sort, ChannelSort::Smart);
        assert_eq!(config.ui.clipboard, ClipboardMode::Osc52);
    }
    assert!(
        toml::from_str::<Config>(
            "[ui]\nsidebar_width=28\nthread_width=44\ntheme='bzz'\nmouse='sometimes'\n"
        )
        .is_err()
    );

    let config =
        toml::from_str::<Config>("[ui]\nchannel_sort='alphabetical'\nclipboard='disabled'\n")
            .unwrap();
    assert_eq!(config.ui.channel_sort, ChannelSort::Alphabetical);
    assert_eq!(config.ui.clipboard, ClipboardMode::Disabled);
}

#[test]
fn local_agents_are_unique_and_require_canonical_workdirs() {
    let temporary = TempDir::new().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let canonical = workspace.canonicalize().unwrap();
    let mut config = Config::default();
    let id = config
        .add_local_agent("writer".into(), Some(workspace))
        .unwrap();
    assert_eq!(config.local_agents[0].id, id);
    assert_eq!(
        config.local_agents[0].workdir.as_deref(),
        Some(canonical.as_path())
    );
    assert!(config.add_local_agent("WRITER".into(), None).is_err());
    config.local_agents[0].workdir = Some(temporary.path().join("missing"));
    assert!(config.validate().is_err());
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
