//! `bzz app …` from end to end, as the owner uses it at his shell: the
//! binary itself, a temporary data directory, and no terminal (so nothing
//! is ever asked: an approval names its hash). Plan v0.16.0, M4.
//!
//! Beside each command the test opens the same data directory with the
//! library and asks the one function that hands out a script to run
//! (`Apps::runnable`) what it would give a runtime.
#![cfg(feature = "apps")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bzz::{
    apps::store::{AppError, Apps},
    config::{Config, IdentityConfig, KeyBackend},
    paths::Paths,
    store::Store,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use tempfile::TempDir;
use uuid::Uuid;

struct Profile {
    temporary: TempDir,
    paths: Paths,
    community: Uuid,
}

impl Profile {
    /// A profile with one community. Its identity is a public key only:
    /// no command of `bzz app` needs a key, and there is none to find.
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("c"),
            data_dir: temporary.path().join("d"),
            cache_dir: temporary.path().join("k"),
        };
        paths.ensure().unwrap();
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "owner".into(),
            pubkey: Keys::generate().public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "no-such-key".into(),
        };
        let mut config = Config {
            identities: vec![identity.clone()],
            ..Config::default()
        };
        let community = config
            .add_community(
                "testers".into(),
                "ws://localhost:3030".into(),
                identity.id,
                true,
            )
            .unwrap();
        config.save(&paths).unwrap();
        Self {
            temporary,
            paths,
            community,
        }
    }

    fn folder(&self) -> PathBuf {
        self.temporary.path().join("work").join("tasks")
    }

    /// Runs `bzz` with no terminal. Answers whether it succeeded, and
    /// everything it printed.
    fn bzz(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_CONFIG_DIR", &self.paths.config_dir)
            .env("BZZ_DATA_DIR", &self.paths.data_dir)
            .env("BZZ_CACHE_DIR", &self.paths.cache_dir)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    fn ok(&self, args: &[&str]) -> String {
        let (ok, said) = self.bzz(args);
        assert!(ok, "bzz {args:?}:\n{said}");
        said
    }

    fn refused(&self, args: &[&str]) -> String {
        let (ok, said) = self.bzz(args);
        assert!(!ok, "bzz {args:?} should have failed:\n{said}");
        said
    }

    /// What `Apps::runnable` would hand to a runtime right now.
    fn runnable(&self) -> Result<(u32, Vec<u8>), AppError> {
        let mut store = Store::open(self.paths.database_file()).unwrap();
        let apps = Apps::new(&mut store, &self.paths);
        apps.runnable(self.community, "tasks")
            .map(|runnable| (runnable.version(), runnable.script().to_vec()))
    }

    fn script(&self) -> PathBuf {
        self.folder().join("app.lua")
    }
}

/// The hash a command printed after `--hash `.
fn hash_in(said: &str) -> String {
    said.split("--hash ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("a hash to approve in:\n{said}"))
        .to_owned()
}

fn has(said: &str, parts: &[&str]) {
    for part in parts {
        assert!(said.contains(part), "`{part}` in:\n{said}");
    }
}

fn card(keys: &Keys, home: Uuid, artifact_type: &str) -> bzz::protocol::artifact::Artifact {
    let tags = [
        ["ar", "1"],
        ["d", &Uuid::new_v4().to_string()],
        ["h", &home.to_string()],
        ["type", artifact_type],
        ["title", "A card"],
        ["op", "create"],
    ]
    .map(|parts| Tag::parse(parts).unwrap());
    bzz::protocol::artifact::parse(
        &EventBuilder::new(Kind::Custom(45_010), "{\"title\":\"A card\"}")
            .tags(tags)
            .sign_with_keys(keys)
            .unwrap(),
    )
    .unwrap()
}

fn replace_in(path: &Path, from: &str, to: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "`{from}` in {}", path.display());
    let changed = text.replacen(from, to, 1);
    std::fs::write(path, &changed).unwrap();
    changed.into_bytes()
}

#[test]
fn the_command_line_from_a_new_folder_to_an_uninstalled_app() {
    let profile = Profile::new();
    let folder = profile.folder();
    let folder_arg = folder.to_str().unwrap();
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();

    // ---- new: a folder to start from ---------------------------------
    let listed = profile.ok(&["app", "new", "--templates"]);
    has(&listed, &["board\tBoard\t"]);
    let said = profile.ok(&["app", "new", "tasks", "--to", folder_arg]);
    has(
        &said,
        &[
            "from the template board",
            "name tasks, title Tasks, record types card",
            "bzz app add",
        ],
    );
    for file in ["app.lua", "app.toml", "brief.md"] {
        assert!(folder.join(file).is_file(), "{file}");
    }
    let again = profile.refused(&["app", "new", "tasks", "--to", folder_arg]);
    has(&again, &["exists already; nothing was written"]);
    has(
        &profile.refused(&["app", "new", "tasks", "--template", "chess"]),
        &["no template called chess", "the templates are: board"],
    );
    has(
        &profile.refused(&["app", "new", "Tasks", "--to", "/nonexistent/x"]),
        &["is not a name an app may have"],
    );
    let original = std::fs::read(profile.script()).unwrap();

    // ---- check: errors with file and line ----------------------------
    let said = profile.ok(&["app", "check", folder_arg]);
    has(
        &said,
        &[
            "ok: tasks (Tasks) loads. api 1, record types: card",
            "app.toml says what the script says",
        ],
    );
    replace_in(
        &profile.script(),
        "app.types = { \"card\" }",
        "app.types = { \"card\" ",
    );
    let said = profile.refused(&["app", "check", folder_arg]);
    has(
        &said,
        &["app.lua:11:", "'}' expected (to close '{' at line 9)"],
    );
    let said = profile.refused(&["app", "check", profile.script().to_str().unwrap()]);
    has(&said, &["app.lua:11:"]);
    std::fs::write(profile.script(), &original).unwrap();
    // The manifest says something the script does not.
    let manifest = folder.join("app.toml");
    replace_in(&manifest, "[\"card\"]", "[\"card\", \"vote\"]");
    has(
        &profile.refused(&["app", "check", folder_arg]),
        &["app.toml says the record types are card, vote; the script declares card"],
    );
    replace_in(&manifest, "[\"card\", \"vote\"]", "[\"card\"]");

    // ---- try: the screen it draws, with keys and a seeded store -------
    let said = profile.ok(&["app", "try", folder_arg, "n", "type:Buy milk", "enter"]);
    has(&said, &["To do (1)", "> Buy milk", "Doing (0)", "Saved."]);
    assert_eq!(said.lines().count(), 16, "the pane's sixteen rows:\n{said}");
    let seed = profile.temporary.path().join("seed.json");
    std::fs::write(
        &seed,
        r#"[{"type":"card","content":{"title":"Seeded","column":"doing","order":1},"by":"bo"}]"#,
    )
    .unwrap();
    let said = profile.ok(&[
        "app",
        "try",
        folder_arg,
        "--seed",
        seed.to_str().unwrap(),
        "--size",
        "60x16",
    ]);
    has(&said, &["Doing (1)", "Seeded", "Loaded 1 cards."]);
    assert_eq!(said.lines().count(), 16);
    has(
        &profile.refused(&["app", "try", folder_arg, "nosuchkey"]),
        &["nosuchkey: not a key"],
    );

    // ---- add: it waits; nothing runs before the approval --------------
    has(
        &profile.ok(&["app", "list"]),
        &["no apps are installed in testers"],
    );
    let said = profile.ok(&["app", "add", folder_arg]);
    has(
        &said,
        &[
            "tasks (Tasks), version 1",
            "From:     you",
            "Record types: card",
            "No version of this app runs yet: all of them are new.",
            "  | A task board that a channel shares",
            "No version runs yet: the whole script is new",
            "+local app = {}",
            "tasks: version 1 waits for your approval. Nothing of it runs until then.",
            "bzz app approve tasks --version 1 --hash ",
        ],
    );
    let hash = hash_in(&said);
    assert_eq!(hash.len(), 64);
    let error = profile.runnable().unwrap_err();
    assert!(
        matches!(error, AppError::NothingApproved { waiting: 1, .. }),
        "a script added from a folder does not run before it is approved: {error}"
    );
    has(
        &profile.ok(&["app", "list"]),
        &["tasks\tTasks\tnothing approved, nothing runs; 1 version waits for your answer\tcard"],
    );

    // ---- the approval -------------------------------------------------
    let waiting = profile.ok(&["app", "pending"]);
    has(&waiting, &["tasks\t1\tyou\t", &hash]);
    let shown = profile.ok(&["app", "pending", "tasks"]);
    has(
        &shown,
        &["Hash:     ", &hash, "approve: bzz app approve tasks"],
    );
    // No terminal to ask at, and no hash: nothing is approved.
    let said = profile.refused(&["app", "approve", "tasks"]);
    has(
        &said,
        &["nothing was approved: there is no terminal to ask you at"],
    );
    assert!(profile.runnable().is_err());
    // Another hash approves nothing.
    let said = profile.refused(&["app", "approve", "tasks", "--hash", &"0".repeat(64)]);
    has(
        &said,
        &[
            "the hash does not name version 1 of tasks",
            "Nothing was approved",
        ],
    );
    assert!(profile.runnable().is_err());
    // The hash he was shown.
    let said = profile.ok(&["app", "approve", "tasks", "--hash", &hash]);
    has(
        &said,
        &["tasks: version 1 is approved and is the version that runs now"],
    );
    assert_eq!(profile.runnable().unwrap(), (1, original.clone()));
    has(
        &profile.refused(&["app", "approve", "tasks", "--hash", &hash]),
        &["no version of tasks waits for an answer"],
    );

    // ---- list and show ------------------------------------------------
    has(
        &profile.ok(&["app", "list"]),
        &[&format!(
            "tasks\tTasks\tversion 1 runs ({})\tcard\topen in no channel",
            &hash[..12]
        )],
    );
    let shown = profile.ok(&["app", "show", "tasks"]);
    has(
        &shown,
        &[
            "tasks (Tasks)",
            "state:    version 1 runs",
            &format!("hash:     {hash}"),
            "types:    card   (on the relay: bzz.app.tasks.card)",
            "from:     you, proposed ",
            "open in:  no channel",
            "  | A task board that a channel shares",
            "  1\tapproved; it runs\tyou\t",
        ],
    );
    assert!(!shown.contains("it does not run"), "{shown}");
    let script = profile.ok(&["app", "show", "tasks", "--script"]);
    assert_eq!(script.as_bytes(), original.as_slice());
    // The same folder again: nothing to approve.
    has(
        &profile.ok(&["app", "add", folder_arg]),
        &["the folder is version 1, which runs: there is nothing to approve"],
    );

    // ---- an update: shown with its difference, runs after approval ----
    let second = replace_in(&profile.script(), "\"Loading...\"", "\"Fetching...\"");
    let said = profile.ok(&["app", "add", folder_arg]);
    has(
        &said,
        &[
            "tasks (Tasks), version 2",
            "  The same as the version that runs.",
            "Brief:    1.6 KiB, the same as the version that runs",
            "Difference from version 1, which runs: 1 line added, 1 removed:",
            "-  s.status = \"Loading...\"",
            "+  s.status = \"Fetching...\"",
            "version 2 waits for your approval",
        ],
    );
    assert_eq!(profile.runnable().unwrap(), (1, original.clone()));
    profile.ok(&["app", "approve", "tasks", "--hash", &hash_in(&said)]);
    assert_eq!(profile.runnable().unwrap(), (2, second.clone()));

    // ---- a failing update keeps the old version ------------------------
    replace_in(&profile.script(), "return app\n", "return app(\n");
    let said = profile.refused(&["app", "add", folder_arg]);
    has(
        &said,
        &[
            "tasks: it does not load, and nothing was changed (recorded as version 3, failed)",
            "app.lua:",
        ],
    );
    assert_eq!(profile.runnable().unwrap(), (2, second.clone()));
    has(
        &profile.ok(&["app", "pending"]),
        &["nothing waits for your answer"],
    );
    std::fs::write(profile.script(), &second).unwrap();

    // ---- a change of record types is marked; refused with a reason ----
    replace_in(
        &profile.script(),
        "app.types = { \"card\" }",
        "app.types = { \"card\", \"vote\" }",
    );
    replace_in(&manifest, "[\"card\"]", "[\"card\", \"vote\"]");
    let said = profile.ok(&["app", "add", folder_arg]);
    has(
        &said,
        &[
            "Record types: card, vote",
            "  CHANGED from the version that runs:",
            "    + vote   new: the app could not read or write these records before",
        ],
    );
    let said = profile.ok(&[
        "app",
        "refuse",
        "tasks",
        "--reason",
        "no votes on this board",
    ]);
    has(
        &said,
        &[
            "tasks: version 4 is refused: no votes on this board",
            "version 2 still runs",
        ],
    );
    assert_eq!(profile.runnable().unwrap(), (2, second.clone()));
    let shown = profile.ok(&["app", "show", "tasks"]);
    has(
        &shown,
        &[
            "  4\trefused: no votes on this board\tyou\t",
            "  3\tfailed: app.lua:",
            "  2\tapproved; it runs\tyou\t",
            "  1\tapproved\tyou\t",
        ],
    );

    // ---- rollback: the earlier version's exact bytes -------------------
    let said = profile.ok(&["app", "rollback", "tasks"]);
    has(
        &said,
        &[
            "tasks: version 1 is the version that runs again, in place of version 2",
            &format!("hash:  {hash}"),
        ],
    );
    assert_eq!(profile.runnable().unwrap(), (1, original.clone()));
    has(
        &profile.refused(&["app", "rollback", "tasks"]),
        &["tasks has no earlier approved version"],
    );
    has(
        &profile.refused(&["app", "rollback", "tasks", "--to", "4"]),
        &["version 4 of tasks cannot be used: it is refused, not approved"],
    );

    // ---- bytes changed on disk behind bzz's back do not run ------------
    let installed = profile
        .paths
        .data_dir
        .join("apps")
        .join(profile.community.to_string())
        .join("tasks/versions/0001/app.lua");
    assert_eq!(std::fs::read(&installed).unwrap(), original);
    std::fs::write(&installed, [original.as_slice(), b"-- edited\n"].concat()).unwrap();
    assert!(matches!(profile.runnable(), Err(AppError::Changed { .. })));
    let shown = profile.ok(&["app", "show", "tasks"]);
    has(
        &shown,
        &[
            "it does not run: tasks: app.lua of version 1 is not what was approved",
            "It was changed outside bzz, and it is not used",
        ],
    );
    std::fs::write(&installed, &original).unwrap();
    assert!(profile.runnable().is_ok());

    // ---- disable and enable --------------------------------------------
    has(
        &profile.ok(&["app", "disable", "tasks"]),
        &["tasks is disabled: it stays installed and runs nowhere"],
    );
    assert!(matches!(profile.runnable(), Err(AppError::Disabled(_))));
    has(
        &profile.ok(&["app", "list"]),
        &["tasks\tTasks\tdisabled (version 1 is approved, "],
    );
    has(
        &profile.ok(&["app", "enable", "tasks"]),
        &["tasks is enabled: version 1 runs where it is open"],
    );
    assert!(profile.runnable().is_ok());

    // ---- dev needs a terminal, and says what it cannot do yet ----------
    has(
        &profile.refused(&["app", "dev", folder_arg]),
        &["`bzz app dev` draws the app in a terminal, and this is not one"],
    );
    has(
        &profile.refused(&["app", "dev", folder_arg, "--channel", "general"]),
        &["needs the app pane of bzz's window, which this version does not have yet"],
    );

    // ---- remove: the channel's records are left alone -------------------
    let keys = Keys::generate();
    let channel = Uuid::new_v4();
    {
        let mut store = Store::open(profile.paths.database_file()).unwrap();
        for kind in [
            "bzz.app.tasks.card",
            "bzz.app.tasks.card",
            "bzz.app.other.card",
        ] {
            store
                .apply_artifact_head(profile.community, &card(&keys, channel, kind))
                .unwrap();
        }
        Apps::new(&mut store, &profile.paths)
            .open_in(profile.community, "tasks", channel)
            .unwrap();
    }
    has(
        &profile.ok(&["app", "show", "tasks"]),
        &[&format!("open in:  {channel}")],
    );
    has(
        &profile.refused(&["app", "remove", "tasks"]),
        &["nothing was removed"],
    );
    assert!(profile.runnable().is_ok(), "not removed without --yes");
    let said = profile.ok(&["app", "remove", "tasks", "--yes"]);
    has(
        &said,
        &[
            "tasks is uninstalled: every version bzz kept of it is gone from this machine",
            "The channel's records were left alone.",
            "2 records of types bzz.app.tasks.* are stored by this bzz, in 1 channel",
            "An app named tasks that is installed again finds them.",
        ],
    );
    assert!(matches!(profile.runnable(), Err(AppError::NotInstalled(_))));
    has(&profile.ok(&["app", "list"]), &["no apps are installed"]);
    assert!(!installed.exists());
    assert!(
        !installed.ancestors().nth(3).unwrap().exists(),
        "the app's folder is gone"
    );
    let store = Store::open(profile.paths.database_file()).unwrap();
    assert_eq!(
        store
            .typed_artifacts(
                profile.community,
                channel,
                &["bzz.app.tasks.card".to_owned()]
            )
            .unwrap()
            .len(),
        2,
        "the records are where they were"
    );
    has(
        &profile.refused(&["app", "show", "tasks"]),
        &["no app named tasks is installed"],
    );
    // The owner's own folder was never touched by any of it.
    assert!(profile.script().is_file());
}

#[test]
fn a_command_that_needs_a_community_says_so_and_the_others_need_none() {
    let temporary = TempDir::new().unwrap();
    let bzz = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_bzz"))
            .args(args)
            .env("BZZ_CONFIG_DIR", temporary.path().join("c"))
            .env("BZZ_DATA_DIR", temporary.path().join("d"))
            .env("BZZ_CACHE_DIR", temporary.path().join("k"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    // With no community configured at all: write, check and try an app.
    let folder = temporary.path().join("notes");
    let folder_arg = folder.to_str().unwrap();
    let (ok, said) = bzz(&[
        "app",
        "new",
        "notes",
        "--title",
        "Team notes",
        "--to",
        folder_arg,
    ]);
    assert!(ok && said.contains("title Team notes"), "{said}");
    let (ok, said) = bzz(&["app", "check", folder_arg]);
    assert!(
        ok && said.contains("ok: notes (Team notes) loads"),
        "{said}"
    );
    let (ok, said) = bzz(&["app", "try", folder_arg]);
    assert!(ok && said.contains("To do (0)"), "{said}");
    // To install one, a community is needed.
    let (ok, said) = bzz(&["app", "add", folder_arg]);
    assert!(!ok && said.contains("select a community"), "{said}");
    let (ok, said) = bzz(&["app", "list"]);
    assert!(!ok && said.contains("select a community"), "{said}");
    assert!(
        !temporary.path().join("d").join("apps").exists(),
        "nothing was kept"
    );
}
