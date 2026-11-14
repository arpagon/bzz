use bzz::{
    config::{
        ChannelSort, ClipboardImportMode, ClipboardMode, Config, MouseMode, ProfileAvatars,
        validate_relay_url,
    },
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
        assert_eq!(config.ui.profile_avatars, ProfileAvatars::Trusted);
    }
    assert!(
        toml::from_str::<Config>(
            "[ui]\nsidebar_width=28\nthread_width=44\ntheme='bzz'\nmouse='sometimes'\n"
        )
        .is_err()
    );

    let config = toml::from_str::<Config>(
        "[ui]\nchannel_sort='alphabetical'\nclipboard='disabled'\nprofile_avatars='off'\n",
    )
    .unwrap();
    assert_eq!(config.ui.channel_sort, ChannelSort::Alphabetical);
    assert_eq!(config.ui.clipboard, ClipboardMode::Disabled);
    assert_eq!(config.ui.profile_avatars, ProfileAvatars::Off);
    assert!(toml::from_str::<Config>("[ui]\nprofile_avatars='sometimes'\n").is_err());
}

#[test]
fn clipboard_import_is_explicit_by_default_and_strict_when_configured() {
    assert_eq!(
        Config::default().media.clipboard_import,
        ClipboardImportMode::Explicit
    );
    let config = toml::from_str::<Config>("[media]\nclipboard_import='off'\n").unwrap();
    assert_eq!(config.media.clipboard_import, ClipboardImportMode::Off);
    assert!(toml::from_str::<Config>("[media]\nclipboard_import='always'\n").is_err());
}

#[test]
fn removed_local_agents_are_dropped_from_existing_configurations() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        "[[local_agents]]\nid='retired'\nlabel='writer'\nbackend='codex'\n",
    )
    .unwrap();

    assert_eq!(Config::load(&paths).unwrap(), Config::default());
    assert!(
        !std::fs::read_to_string(paths.config_file())
            .unwrap()
            .contains("local_agents")
    );
}

#[test]
fn diagnostics_are_local_on_by_default_and_strictly_configured() {
    assert_eq!(
        Config::default().diagnostics.local_journal,
        bzz::config::LocalJournalMode::On
    );
    let config = toml::from_str::<Config>("[diagnostics]\nlocal_journal='off'\n").unwrap();
    assert_eq!(
        config.diagnostics.local_journal,
        bzz::config::LocalJournalMode::Off
    );
    assert!(toml::from_str::<Config>("[diagnostics]\nlocal_journal='remote'\n").is_err());
}

#[test]
fn telemetry_is_default_off_and_requires_complete_endpoint_binding() {
    let config = Config::default();
    assert!(!config.telemetry.enabled);
    assert!(config.telemetry.endpoint.is_none());

    let mut incomplete = Config::default();
    incomplete.telemetry.enabled = true;
    assert!(incomplete.validate().is_err());

    let mut configured = Config::default();
    bzz::telemetry::config::configure(&mut configured, "https://otel.example/v1/logs").unwrap();
    assert!(!configured.telemetry.enabled);
    assert!(configured.validate().is_ok());
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

/// v0.15 M1: a hosted agent made by v0.13 loads unchanged and stays an agent
/// whose process the user runs; a local agent is the same record with more
/// settings.
#[test]
fn hosted_agents_of_v013_load_unchanged_and_local_agents_add_settings() {
    use bzz::config::{Harness, LocalAgentState, WakeRule};
    let identity = uuid::Uuid::new_v4();
    let community = uuid::Uuid::new_v4();
    let key = "b".repeat(64);
    // Exactly what v0.13 and v0.14 wrote.
    let v013 = format!(
        "[[identities]]\nid = \"{identity}\"\nlabel = \"owner\"\npubkey = \"{key}\"\nbackend = \"encrypted-file\"\nkey_ref = \"k\"\n\n\
         [[hosted_agents]]\nid = \"{community}\"\nlabel = \"helper\"\npubkey = \"{key}\"\ncommunity = \"{community}\"\n\
         owner_identity = \"{identity}\"\nauth_tag = \"[]\"\ncredential_hash = \"{}\"\nallowed = [\"channel.read\"]\npaused = false\n",
        "c".repeat(64)
    );
    let old = toml::from_str::<Config>(&v013).unwrap();
    old.validate().unwrap();
    assert_eq!(old.hosted_agents[0].local, None);
    assert!(!old.hosted_agents[0].runs_locally());
    assert!(
        !toml::to_string_pretty(&old)
            .unwrap()
            .contains("hosted_agents.local"),
        "saving it again adds nothing"
    );

    let local = format!(
        "{v013}\n[hosted_agents.local]\nharness = \"pi-durable\"\nwake = \"allowlist\"\nwake_allowlist = [\"{key}\"]\n\
         instructions = \"Be brief.\"\nnetwork = [\"github.com\"]\n\n\
         [hosted_agents.local.model]\nprovider = \"openai-codex\"\nmodel_id = \"gpt-6-luna\"\nthinking_level = \"low\"\n\n\
         [[hosted_agents.local.folders]]\nname = \"site\"\npath = \"/home/ann/site\"\n"
    );
    let config = toml::from_str::<Config>(&local).unwrap();
    config.validate().unwrap();
    let agent = &config.hosted_agents[0];
    let settings = agent.local.as_ref().unwrap();
    assert_eq!(
        (settings.harness, settings.wake, settings.state),
        (
            Harness::PiDurable,
            WakeRule::Allowlist,
            LocalAgentState::Active
        )
    );
    assert!(
        !settings.folders[0].write,
        "a granted folder is read-only unless said"
    );
    assert!(agent.runs_locally());
    let again = toml::from_str::<Config>(&toml::to_string_pretty(&config).unwrap()).unwrap();
    assert_eq!(again, config);

    // The defaults: owner only, active. Paused and moved-away agents never run.
    let minimal = format!("{v013}\n[hosted_agents.local]\nharness = \"claude-code\"\n");
    let mut config = toml::from_str::<Config>(&minimal).unwrap();
    assert_eq!(
        config.hosted_agents[0].local.as_ref().unwrap().wake,
        WakeRule::OwnerOnly
    );
    for state in [LocalAgentState::Paused, LocalAgentState::MovedAway] {
        config.hosted_agents[0].local.as_mut().unwrap().state = state;
        assert!(!config.hosted_agents[0].runs_locally(), "{state:?}");
    }
    config.hosted_agents[0].local.as_mut().unwrap().state = LocalAgentState::Active;
    config.hosted_agents[0].paused = true;
    assert!(!config.hosted_agents[0].runs_locally());
    let moved = toml::to_string_pretty(&{
        let mut config = config.clone();
        config.hosted_agents[0].local.as_mut().unwrap().state = LocalAgentState::MovedAway;
        config
    })
    .unwrap();
    assert!(moved.contains("state = \"moved-away\""), "{moved}");

    // Strict as the rest of the file: unknown harnesses and settings fail.
    for wrong in [
        "harness = \"gemini\"\n",
        "harness = \"grok\"\nsecret_key = \"x\"\n",
        "wake = \"anyone\"\n",
    ] {
        let text = format!("{v013}\n[hosted_agents.local]\n{wrong}");
        assert!(toml::from_str::<Config>(&text).is_err(), "{wrong}");
    }
    for wrong in [
        "harness = \"grok\"\nwake_allowlist = [\"npub1x\"]\n",
        "harness = \"grok\"\n[[hosted_agents.local.folders]]\nname = \"../etc\"\npath = \"/etc\"\n",
        "harness = \"grok\"\n[[hosted_agents.local.folders]]\nname = \"site\"\npath = \"relative\"\n",
    ] {
        let text = format!("{v013}\n[hosted_agents.local]\n{wrong}");
        assert!(
            toml::from_str::<Config>(&text).unwrap().validate().is_err(),
            "{wrong}"
        );
    }
}
