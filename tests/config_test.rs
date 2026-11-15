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

    // A granted folder needs a path that is absolute where the test runs.
    let granted = if cfg!(windows) {
        r"C:\Users\ann\site"
    } else {
        "/home/ann/site"
    };
    let local = format!(
        "{v013}\n[hosted_agents.local]\nharness = \"pi-durable\"\nwake = \"allowlist\"\nwake_allowlist = [\"{key}\"]\n\
         instructions = \"Be brief.\"\nnetwork = [\"github.com\"]\n\n\
         [hosted_agents.local.model]\nprovider = \"openai-codex\"\nmodel_id = \"gpt-6-luna\"\nthinking_level = \"low\"\n\n\
         [[hosted_agents.local.folders]]\nname = \"site\"\npath = '{granted}'\n"
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
    assert_eq!(settings.folders[0].path, std::path::Path::new(granted));
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

/// v0.15 M7: a folder grant says whether real commands may run there, and
/// whether the owner is asked; an agent says whether Pi packages are loaded.
/// A file written before either existed means no commands, and packages on.
#[test]
fn a_folder_grant_carries_exec_and_an_agent_its_packages_switch() {
    use bzz::config::ExecGrant;
    let identity = uuid::Uuid::new_v4();
    let community = uuid::Uuid::new_v4();
    let key = "b".repeat(64);
    let granted = if cfg!(windows) {
        r"C:\Users\ann\site"
    } else {
        "/home/ann/site"
    };
    let agent = format!(
        "[[identities]]\nid = \"{identity}\"\nlabel = \"owner\"\npubkey = \"{key}\"\nbackend = \"encrypted-file\"\nkey_ref = \"k\"\n\n\
         [[hosted_agents]]\nid = \"{community}\"\nlabel = \"Scout\"\npubkey = \"{key}\"\ncommunity = \"{community}\"\n\
         owner_identity = \"{identity}\"\nauth_tag = \"[]\"\ncredential_hash = \"{}\"\n\n\
         [hosted_agents.local]\nharness = \"pi-durable\"\n",
        "c".repeat(64)
    );
    let folder = |more: &str| {
        format!("\n[[hosted_agents.local.folders]]\nname = \"site\"\npath = '{granted}'\n{more}")
    };
    let read = |text: &str| {
        let config = toml::from_str::<Config>(text).unwrap();
        config.validate().unwrap();
        config.hosted_agents[0].local.clone().unwrap()
    };
    // As v0.15.0 before M7 wrote it: no commands, and packages on.
    let before = read(&format!("{agent}{}", folder("write = true\n")));
    assert_eq!(before.folders[0].exec, ExecGrant::No);
    assert!(before.folders[0].write && before.packages);
    let again = toml::to_string_pretty(
        &toml::from_str::<Config>(&format!("{agent}{}", folder(""))).unwrap(),
    )
    .unwrap();
    assert!(
        !again.contains("exec") && !again.contains("packages"),
        "what is the default is not written: {again}"
    );

    for (word, exec) in [
        ("ask", ExecGrant::Ask),
        ("without-asking", ExecGrant::WithoutAsking),
        ("no", ExecGrant::No),
    ] {
        let local = read(&format!(
            "{agent}{}",
            folder(&format!("exec = \"{word}\"\n"))
        ));
        assert_eq!(local.folders[0].exec, exec, "{word}");
        assert!(!local.folders[0].write, "exec is apart from write");
        assert_eq!(exec.granted(), word != "no");
    }
    let off = read(&format!(
        "{agent}packages = false\n{}",
        folder("exec = \"ask\"\n")
    ));
    assert!(!off.packages);
    let mut config = toml::from_str::<Config>(&format!(
        "{agent}packages = false\n{}",
        folder("exec = \"ask\"\n")
    ))
    .unwrap();
    let text = toml::to_string_pretty(&config).unwrap();
    assert!(
        text.contains("exec = \"ask\"") && text.contains("packages = false"),
        "{text}"
    );
    assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    // Anything else is refused, as every unknown setting is: never read as
    // a yes.
    for wrong in [
        "exec = true\n",
        "exec = \"yes\"\n",
        "exec = \"always\"\n",
        "run = true\n",
    ] {
        assert!(
            toml::from_str::<Config>(&format!("{agent}{}", folder(wrong))).is_err(),
            "{wrong}"
        );
    }
    assert!(toml::from_str::<Config>(&format!("{agent}packages = \"off\"\n")).is_err());
    // A folder's name is one the host takes: it starts with a letter or a digit.
    config.hosted_agents[0].local.as_mut().unwrap().folders[0].name = "-site".into();
    assert!(config.validate().is_err());
}

/// One writer for the settings: a copy that saves writes only what it
/// changed since it last read or wrote the file, over what the file holds
/// now. So a command's edit survives a later save by a bzz that was open
/// all along (a TUI), and the reverse.
#[test]
fn a_save_does_not_write_over_what_another_bzz_saved_meanwhile() {
    use bzz::config::{
        Harness, HostedAgentConfig, IdentityConfig, KeyBackend, LocalAgentConfig, LocalAgentState,
    };
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    let key = "b".repeat(64);
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "owner".into(),
        pubkey: key.clone(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "k".into(),
    };
    let agent = |label: &str| HostedAgentConfig {
        id: uuid::Uuid::new_v4(),
        label: label.into(),
        pubkey: key.clone(),
        community: uuid::Uuid::new_v4(),
        owner_identity: identity.id,
        auth_tag: "[]".into(),
        credential_hash: "c".repeat(64),
        allowed: Vec::new(),
        paused: false,
        local: Some(LocalAgentConfig::new(Harness::PiDurable)),
    };
    let (scout, ranger) = (agent("Scout"), agent("Ranger"));
    let mut first = Config::default();
    first.identities.push(identity.clone());
    first.hosted_agents = vec![scout.clone(), ranger.clone()];
    first.save(&paths).unwrap();
    let local = |config: &Config, id| {
        config
            .hosted_agents
            .iter()
            .find(|agent| agent.id == id)
            .and_then(|agent| agent.local.clone())
    };
    fn find(config: &mut Config, id: uuid::Uuid) -> &mut HostedAgentConfig {
        config
            .hosted_agents
            .iter_mut()
            .find(|agent| agent.id == id)
            .unwrap()
    }

    // A TUI that is open, and a command that starts later: each has its
    // own copy of the settings.
    let mut tui = Config::load(&paths).unwrap();
    let mut command = Config::load(&paths).unwrap();
    // The command edits an agent (`bzz agent edit Scout --allow-host …`),
    // and creates another (`bzz agent new`).
    find(&mut command, scout.id).local.as_mut().unwrap().network = vec!["github.com".into()];
    let created = agent("Courier");
    command.hosted_agents.push(created.clone());
    command.save(&paths).unwrap();
    // Later the TUI saves a change of its own: a pane's width, and a pause
    // of the very agent the command edited.
    tui.ui.sidebar_width = 40;
    find(&mut tui, scout.id).local.as_mut().unwrap().state = LocalAgentState::Paused;
    tui.save(&paths).unwrap();
    let saved = Config::load(&paths).unwrap();
    assert_eq!(saved.ui.sidebar_width, 40, "the TUI's change is there");
    let scouts = local(&saved, scout.id).unwrap();
    assert_eq!(
        (scouts.network.clone(), scouts.state),
        (vec!["github.com".to_owned()], LocalAgentState::Paused),
        "the command's edit of an agent survived the TUI's save, with the TUI's own change of it"
    );
    assert!(
        local(&saved, created.id).is_some(),
        "the agent the command created survived the TUI's save"
    );
    assert_eq!(saved.hosted_agents.len(), 3);

    // The reverse: the TUI saves, then the command, which read the file
    // before that, saves a change of its own.
    let mut command = Config::load(&paths).unwrap();
    tui.ui.thread_width = 60;
    find(&mut tui, ranger.id).label = "Ranger II".into();
    tui.save(&paths).unwrap();
    find(&mut command, created.id)
        .local
        .as_mut()
        .unwrap()
        .instructions = "Be brief.".into();
    command.save(&paths).unwrap();
    let saved = Config::load(&paths).unwrap();
    assert_eq!(
        (saved.ui.thread_width, saved.ui.sidebar_width),
        (60, 40),
        "the TUI's changes survived the command's save"
    );
    assert_eq!(
        saved
            .hosted_agents
            .iter()
            .find(|agent| agent.id == ranger.id)
            .unwrap()
            .label,
        "Ranger II"
    );
    assert_eq!(local(&saved, created.id).unwrap().instructions, "Be brief.");

    // An agent a command deleted stays deleted, whatever the TUI changed in
    // its own copy of it: its key is gone.
    let mut command = Config::load(&paths).unwrap();
    command.hosted_agents.retain(|agent| agent.id != ranger.id);
    command.save(&paths).unwrap();
    find(&mut tui, ranger.id).local.as_mut().unwrap().state = LocalAgentState::Paused;
    tui.save(&paths).unwrap();
    let saved = Config::load(&paths).unwrap();
    assert!(
        local(&saved, ranger.id).is_none(),
        "a deleted agent does not come back"
    );
    assert_eq!(saved.hosted_agents.len(), 2);
    // And one the TUI deletes goes, whatever a command changed in it.
    let mut command = Config::load(&paths).unwrap();
    find(&mut command, created.id).label = "Courier II".into();
    command.save(&paths).unwrap();
    tui.hosted_agents
        .retain(|agent| agent.id != created.id && agent.id != ranger.id);
    // The TUI never heard of Courier, so for it nothing was removed.
    tui.save(&paths).unwrap();
    assert_eq!(Config::load(&paths).unwrap().hosted_agents.len(), 2);

    // Both changed one setting: the later save wins, and only that setting.
    let mut command = Config::load(&paths).unwrap();
    command.ui.sidebar_width = 30;
    command.ui.message_width = 90;
    command.save(&paths).unwrap();
    tui.ui.sidebar_width = 50;
    tui.save(&paths).unwrap();
    let saved = Config::load(&paths).unwrap();
    assert_eq!((saved.ui.sidebar_width, saved.ui.message_width), (50, 90));

    // Two changes that do not fit together are not saved: the command
    // removed the identity a community the TUI adds would belong to.
    let mut command = Config::load(&paths).unwrap();
    command.hosted_agents.clear();
    command.identities.clear();
    command.save(&paths).unwrap();
    tui.add_community("m".into(), "wss://relay.example".into(), identity.id, false)
        .unwrap();
    let error = tui.save(&paths).unwrap_err().to_string();
    assert!(
        error.contains("changed by another bzz meanwhile") && error.contains("nothing was saved"),
        "{error}"
    );
    assert!(Config::load(&paths).unwrap().communities.is_empty());

    // A copy that never read the file writes itself whole, as before.
    let fresh = Config::default();
    fresh.save(&paths).unwrap();
    assert_eq!(Config::load(&paths).unwrap(), Config::default());
    // The lock is a file beside the settings, and no temporary file stays.
    let mut names = std::fs::read_dir(&paths.config_dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["config.toml", "config.toml.lock"]);
}

/// A save waits for another bzz that is saving, and two that save at the
/// same moment both leave their change in the file.
#[test]
fn two_saves_at_once_both_land() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    Config::default().save(&paths).unwrap();
    let mut widths = Vec::new();
    for round in 0..20_u16 {
        let (mut one, mut two) = (Config::load(&paths).unwrap(), Config::load(&paths).unwrap());
        one.ui.sidebar_width = 20 + round;
        two.ui.thread_width = 31 + round;
        let (paths_one, paths_two) = (paths.clone(), paths.clone());
        let first = std::thread::spawn(move || one.save(&paths_one).unwrap());
        let second = std::thread::spawn(move || two.save(&paths_two).unwrap());
        first.join().unwrap();
        second.join().unwrap();
        let saved = Config::load(&paths).unwrap();
        widths.push((saved.ui.sidebar_width, saved.ui.thread_width));
        assert_eq!(
            (saved.ui.sidebar_width, saved.ui.thread_width),
            (20 + round, 31 + round),
            "round {round}: neither save wrote over the other"
        );
    }
}
