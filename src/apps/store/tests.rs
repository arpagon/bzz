//! The proofs of M4's store: what runs, what is shown, what is kept.
//!
//! Scripts are loaded apart with [`Limits::patient`]: these tests are about
//! what the store does with a version, not about how fast the machine is.

use std::fs;
use std::path::PathBuf;

use nostr::{EventBuilder, Keys, Kind, Tag};
use tempfile::TempDir;
use uuid::Uuid;

use super::manifest::{BRIEF_BYTES, BRIEF_FILE, MANIFEST_FILE, SCRIPT_BYTES, SCRIPT_FILE};
use super::review::TypesChange;
use super::*;
use crate::config::{Config, IdentityConfig, KeyBackend};
use crate::protocol::artifact::parse;

struct Fixture {
    _temporary: TempDir,
    store: Store,
    root: PathBuf,
    community: Uuid,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let identity = IdentityConfig {
            id: Uuid::new_v4(),
            label: "owner".into(),
            pubkey: Keys::generate().public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "owner".into(),
        };
        let mut config = Config::default();
        config.identities.push(identity.clone());
        let community = config
            .add_community("c".into(), "ws://localhost:3030".into(), identity.id, true)
            .unwrap();
        let mut store = Store::open(temporary.path().join("bzz.db")).unwrap();
        store.sync_config(&config).unwrap();
        Self {
            root: temporary.path().join("apps"),
            _temporary: temporary,
            store,
            community,
        }
    }

    fn apps(&mut self) -> Apps<'_> {
        Apps::at(&mut self.store, self.root.clone())
    }

    /// Proposes a version, loaded apart with time to spare.
    fn propose(&mut self, submission: Submission) -> Recorded {
        let examined = examine_with(submission, &Limits::patient()).unwrap();
        let community = self.community;
        self.apps().record(community, examined).unwrap()
    }

    /// Proposes a version as the owner and approves it.
    fn install(&mut self, script: &str) -> u32 {
        let recorded = self.propose(Submission::new(
            "board",
            script,
            "A board.\n",
            Proposer::owner(),
        ));
        let Recorded::Waiting { version, hash, .. } = recorded else {
            panic!("{recorded:?}");
        };
        let community = self.community;
        self.apps()
            .approve(community, "board", version, &hash)
            .unwrap();
        version
    }

    fn script_path(&mut self, version: u32) -> PathBuf {
        let community = self.community;
        self.apps()
            .version_dir(community, "board", version)
            .join(SCRIPT_FILE)
    }
}

/// A script that declares `types` and draws `says`.
fn script(types: &[&str], says: &str) -> String {
    let types = types
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "local app = {{}}\napp.api = 1\napp.types = {{ {types} }}\nfunction app.view(state, ctx)\n  return ui.text {{ \"{says}\" }}\nend\nreturn app\n"
    )
}

fn agent() -> Proposer {
    Proposer::agent("d".repeat(64), "Scout")
}

fn waiting(recorded: Recorded) -> (u32, String) {
    match recorded {
        Recorded::Waiting { version, hash, .. } => (version, hash),
        other => panic!("{other:?}"),
    }
}

/// Rule 16 at the one function that hands out a script to run: whatever
/// was proposed, refused, failed or left waiting, [`Apps::runnable`] gives
/// only bytes the owner approved, and it gives exactly those.
#[test]
fn a_version_that_was_not_approved_is_never_handed_to_a_runtime() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let one = script(&["card"], "one");

    // Not installed at all.
    assert!(matches!(
        fixture.apps().runnable(community, "board"),
        Err(AppError::NotInstalled(_))
    ));

    // Proposed by the owner himself: it waits, and nothing runs.
    let (first, hash) = waiting(fixture.propose(Submission::new(
        "board",
        one.clone(),
        "A board.\n",
        Proposer::owner(),
    )));
    let refused = fixture.apps().runnable(community, "board").unwrap_err();
    assert!(
        matches!(refused, AppError::NothingApproved { waiting: 1, .. }),
        "{refused}"
    );
    assert_eq!(
        refused.to_string(),
        "board has no approved version (one waits for your answer): nothing of it runs"
    );

    // Approved: exactly those bytes are handed out.
    fixture
        .apps()
        .approve(community, "board", first, &hash)
        .unwrap();
    let runnable = fixture.apps().runnable(community, "board").unwrap();
    assert_eq!(runnable.script(), one.as_bytes());
    assert_eq!(
        (runnable.version(), runnable.hash(), runnable.types()),
        (first, hash.as_str(), &["card".to_owned()][..])
    );

    // An agent proposes another version: it waits, and the approved one is
    // still what runs.
    let two = script(&["card"], "two");
    let (second, second_hash) = waiting(fixture.propose(Submission::new(
        "board",
        two.clone(),
        "A board, changed.\n",
        agent(),
    )));
    assert_eq!(
        fixture
            .apps()
            .runnable(community, "board")
            .unwrap()
            .script(),
        one.as_bytes(),
        "a waiting version does not run"
    );

    // Refused: still the approved one. And a refused version cannot be
    // approved afterwards, nor gone back to.
    fixture
        .apps()
        .refuse(community, "board", second, Some("not now"))
        .unwrap();
    assert_eq!(
        fixture
            .apps()
            .runnable(community, "board")
            .unwrap()
            .script(),
        one.as_bytes()
    );
    assert!(matches!(
        fixture
            .apps()
            .approve(community, "board", second, &second_hash),
        Err(AppError::NotWaiting {
            state: VersionState::Refused,
            ..
        })
    ));
    assert!(matches!(
        fixture.apps().rollback(community, "board", Some(second)),
        Err(AppError::NotUsable { .. })
    ));

    // A version that does not load: still the approved one.
    let broken = fixture.propose(Submission::new(
        "board",
        "local app = {}\nreturn app(\n",
        "A board.\n",
        agent(),
    ));
    assert!(matches!(broken, Recorded::Failed { .. }), "{broken:?}");
    let runnable = fixture.apps().runnable(community, "board").unwrap();
    assert_eq!(
        (runnable.version(), runnable.script()),
        (first, one.as_bytes())
    );

    // Every version that is not approved, by every way in: none is current.
    for version in fixture.apps().versions(community, "board").unwrap() {
        if version.state != VersionState::Approved {
            assert!(
                fixture
                    .apps()
                    .rollback(community, "board", Some(version.version))
                    .is_err(),
                "{version:?}"
            );
        }
    }
    assert_eq!(
        fixture.apps().get(community, "board").unwrap().app.current,
        Some(first)
    );

    // Disabled: nothing is handed out, although a version is approved.
    fixture
        .apps()
        .set_enabled(community, "board", false)
        .unwrap();
    assert!(matches!(
        fixture.apps().runnable(community, "board"),
        Err(AppError::Disabled(_))
    ));
    fixture
        .apps()
        .set_enabled(community, "board", true)
        .unwrap();
    assert!(fixture.apps().runnable(community, "board").is_ok());
}

/// The hash an approval names must be the hash of the script that waits
/// under that number; another hash, or another number, approves nothing.
#[test]
fn approval_is_of_exact_bytes() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let (version, hash) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "one"),
        "A board.\n",
        agent(),
    )));
    assert_eq!(hash, manifest::sha256(script(&["card"], "one").as_bytes()));
    let other = manifest::sha256(script(&["card"], "two").as_bytes());
    let error = fixture
        .apps()
        .approve(community, "board", version, &other)
        .unwrap_err();
    assert!(matches!(error, AppError::AnotherHash { .. }), "{error}");
    assert!(error.to_string().contains("Nothing was approved"));
    assert!(matches!(
        fixture.apps().approve(community, "board", 7, &hash),
        Err(AppError::NoSuchVersion { version: 7, .. })
    ));
    assert!(fixture.apps().runnable(community, "board").is_err());

    // The files of a waiting version are changed on disk before the owner
    // answers: what he was shown is no longer what is there. It is not
    // approved, and it can never be.
    let path = fixture.script_path(version);
    fs::write(&path, script(&["card"], "swapped")).unwrap();
    let error = fixture
        .apps()
        .approve(community, "board", version, &hash)
        .unwrap_err();
    assert!(
        matches!(
            error,
            AppError::Changed {
                approved: false,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("is not what was proposed"));
    let row = fixture
        .apps()
        .versions(community, "board")
        .unwrap()
        .remove(0);
    assert_eq!(row.state, VersionState::Failed);
    assert!(fixture.apps().runnable(community, "board").is_err());
    assert!(!path.exists(), "the changed files are not kept");
}

/// Bytes changed on disk after approval do not run, and bzz says why.
#[test]
fn bytes_changed_on_disk_after_approval_do_not_run_and_bzz_says_why() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let approved = script(&["card"], "approved");
    let version = fixture.install(&approved);
    let path = fixture.script_path(version);
    assert_eq!(fs::read(&path).unwrap(), approved.as_bytes());

    // Someone edits the installed script behind bzz's back.
    let edited = approved.replace("approved", "edited behind bzz");
    fs::write(&path, &edited).unwrap();
    let error = fixture.apps().runnable(community, "board").unwrap_err();
    let AppError::Changed {
        file,
        approved: was_approved,
        recorded,
        found,
        ..
    } = &error
    else {
        panic!("{error}");
    };
    assert_eq!((*file, *was_approved), (SCRIPT_FILE, true));
    assert_eq!(recorded, &manifest::sha256(approved.as_bytes()));
    assert_eq!(found, &manifest::sha256(edited.as_bytes()));
    let said = error.to_string();
    for part in [
        "board: app.lua of version 1 is not what was approved",
        "the recorded hash is",
        "the file on disk has",
        "It was changed outside bzz, and it is not used",
    ] {
        assert!(said.contains(part), "`{part}` in: {said}");
    }

    // One byte more is enough; so is a file that is gone.
    fs::write(&path, format!("{approved} ")).unwrap();
    assert!(matches!(
        fixture.apps().runnable(community, "board"),
        Err(AppError::Changed { .. })
    ));
    fs::remove_file(&path).unwrap();
    let missing = fixture.apps().runnable(community, "board").unwrap_err();
    assert!(matches!(missing, AppError::Missing { .. }), "{missing}");

    // The approved bytes put back are the approved bytes.
    fs::write(&path, &approved).unwrap();
    assert_eq!(
        fixture
            .apps()
            .runnable(community, "board")
            .unwrap()
            .script(),
        approved.as_bytes()
    );

    // An edited brief or manifest does not stop the script, which is what
    // runs; it is found out where the brief or the manifest is shown.
    let dir = path.parent().unwrap().to_owned();
    fs::write(dir.join(BRIEF_FILE), "It only shows cats.\n").unwrap();
    assert!(fixture.apps().runnable(community, "board").is_ok());
    let error = fixture
        .apps()
        .bundle(community, "board", version)
        .unwrap_err();
    assert!(
        matches!(
            error,
            AppError::Changed {
                file: BRIEF_FILE,
                ..
            }
        ),
        "{error}"
    );
    fs::write(dir.join(BRIEF_FILE), "A board.\n").unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        "name = \"board\"\ntitle = \"Board\"\napi = 1\ntypes = [\"card\", \"secret\"]\n",
    )
    .unwrap();
    assert_eq!(
        fixture.apps().runnable(community, "board").unwrap().types(),
        ["card"],
        "the approved types are the database's, not the file's"
    );
    assert!(matches!(
        fixture.apps().bundle(community, "board", version),
        Err(AppError::Changed {
            file: MANIFEST_FILE,
            ..
        })
    ));
}

/// A proposal that fails to load is recorded as failed with its error, is
/// not among what waits for the owner, cannot be approved, and leaves the
/// version that runs as it was.
#[test]
fn a_proposal_that_fails_to_load_is_not_offered_and_keeps_the_old_version() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let good = script(&["card"], "good");
    let first = fixture.install(&good);

    let broken = "local app = {}\napp.api = 1\napp.types = { \"card\" }\nfunction app.view(s, ctx)\n  return ui.text { \"x\" \nend\nreturn app\n";
    let recorded = fixture.propose(Submission::new("board", broken, "A board.\n", agent()));
    let Recorded::Failed { version, error } = recorded else {
        panic!("{recorded:?}");
    };
    assert!(
        error.starts_with("board.lua:6:"),
        "the error has the file and the line: {error}"
    );

    // Not offered.
    assert!(fixture.apps().waiting(community).unwrap().is_empty());
    assert!(
        fixture
            .apps()
            .get(community, "board")
            .unwrap()
            .waiting
            .is_empty()
    );
    // Recorded, with who and why, and without its bytes.
    let row = fixture
        .apps()
        .versions(community, "board")
        .unwrap()
        .remove(0);
    assert_eq!((row.version, row.state), (version, VersionState::Failed));
    assert_eq!(row.detail.as_deref(), Some(error.as_str()));
    assert_eq!(row.proposer, agent());
    assert!(!row.kept);
    assert!(
        !fixture
            .apps()
            .version_dir(community, "board", version)
            .exists()
    );
    // It cannot be approved, shown for approval, or gone back to.
    assert!(matches!(
        fixture
            .apps()
            .approve(community, "board", version, &row.script_sha256),
        Err(AppError::NotWaiting {
            state: VersionState::Failed,
            ..
        })
    ));
    assert!(fixture.apps().review(community, "board", version).is_err());
    assert!(
        fixture
            .apps()
            .rollback(community, "board", Some(version))
            .is_err()
    );
    // The old version runs.
    let runnable = fixture.apps().runnable(community, "board").unwrap();
    assert_eq!(
        (runnable.version(), runnable.script()),
        (first, good.as_bytes())
    );

    // The other ways a version fails before it is ever offered.
    let failures: Vec<(Submission, &str)> = vec![
        (
            Submission::new("board", script(&["card"], "x"), "", agent()),
            "the brief is empty",
        ),
        (
            Submission::new(
                "board",
                script(&["card"], "x"),
                vec![b'a'; BRIEF_BYTES + 1],
                agent(),
            ),
            "a brief may be 16384",
        ),
        (
            Submission::new(
                "board",
                format!("{}--{}", script(&["card"], "x"), "x".repeat(SCRIPT_BYTES)),
                "A board.\n",
                agent(),
            ),
            "an app's script may be 131072",
        ),
        (
            Submission::new(
                "board",
                script(&["card"], "x").replace("app.api = 1", "app.api = 2"),
                "A board.\n",
                agent(),
            ),
            "this bzz knows only api 1",
        ),
        (
            Submission::new(
                "board",
                script(&["card"], "x")
                    .replace("return ui.text", "error('at start') return ui.text"),
                "A board.\n",
                agent(),
            ),
            "at start",
        ),
        (
            Submission::new("board", "", "A board.\n", agent()),
            "the script is empty",
        ),
    ];
    for (submission, expected) in failures {
        let recorded = fixture.propose(submission);
        let Recorded::Failed { error, .. } = &recorded else {
            panic!("`{expected}`: {recorded:?}");
        };
        assert!(error.contains(expected), "`{expected}` in: {error}");
    }
    assert!(fixture.apps().waiting(community).unwrap().is_empty());
    assert_eq!(
        fixture
            .apps()
            .runnable(community, "board")
            .unwrap()
            .version(),
        first
    );
    // A name no app may have is refused before anything is recorded.
    assert!(matches!(
        examine(Submission::new("../etc", "x", "y", agent())),
        Err(AppError::Name(_))
    ));
}

/// A change of the declared record types is marked as such in what the
/// owner is shown, and a version that changes none says so.
#[test]
fn a_change_of_the_declared_types_is_marked_in_what_the_owner_is_shown() {
    let mut fixture = Fixture::new();
    let community = fixture.community;

    // The first version: every type is new.
    let (first, hash) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card", "tag"], "one"),
        "A board.\n",
        Proposer::owner(),
    )));
    let review = fixture.apps().review(community, "board", first).unwrap();
    assert_eq!(review.types_change, TypesChange::First);
    assert!(
        review
            .render(|at| at.to_string())
            .contains("No version of this app runs yet: all of them are new.")
    );
    fixture
        .apps()
        .approve(community, "board", first, &hash)
        .unwrap();

    // The same types: said to be the same.
    let (same, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["tag", "card"], "two"),
        "A board.\n",
        agent(),
    )));
    let review = fixture.apps().review(community, "board", same).unwrap();
    assert_eq!(review.types_change, TypesChange::Unchanged);
    let shown = review.render(|at| at.to_string());
    assert!(shown.contains("Record types: tag, card\n  The same as the version that runs."));
    assert!(!shown.contains("CHANGED from the version that runs:"));

    // Other types: marked, with which are new and which are gone.
    let (changed, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card", "vote"], "three"),
        "A board.\n",
        Proposer::member("e".repeat(64), "Ana"),
    )));
    let review = fixture.apps().review(community, "board", changed).unwrap();
    assert_eq!(
        review.types_change,
        TypesChange::Changed {
            added: vec!["vote".into()],
            removed: vec!["tag".into()],
        }
    );
    let shown = review.render(|at| at.to_string());
    for line in [
        "Record types: card, vote",
        "  CHANGED from the version that runs:",
        "    + vote   new: the app could not read or write these records before",
        "    - tag   no longer declared: the app stops reading and writing them",
    ] {
        assert!(shown.contains(line), "`{line}` in:\n{shown}");
    }
}

/// What the owner is shown: who, the brief, sizes, and the difference from
/// the version that runs. Nothing a proposer wrote can draw on his screen.
#[test]
fn the_owner_is_shown_who_the_brief_the_sizes_and_the_difference() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let one = script(&["card"], "one");
    let first = fixture.install(&one);
    let channel = Uuid::new_v4();
    let two = one.replace("\"one\"", "\"two\"") + "-- \u{202E}hidden\x1b[2J\n";
    let (second, hash) = waiting(
        fixture.propose(
            Submission::new(
                "board",
                two.clone(),
                "A board with a second line.\n",
                agent().in_channel(channel),
            )
            .titled("Task board"),
        ),
    );
    let review = fixture.apps().review(community, "board", second).unwrap();
    assert_eq!(review.hash, hash);
    assert_eq!(review.from, agent().in_channel(channel));
    assert_eq!(review.brief, "A board with a second line.\n");
    assert!(review.brief_changed);
    assert_eq!(review.script_bytes, two.len() as u64);
    assert_eq!(review.running.as_ref().unwrap().version, first);
    assert_eq!(
        review.running.as_ref().unwrap().script_bytes,
        one.len() as u64
    );
    assert_eq!(
        (review.difference.added(), review.difference.removed()),
        (2, 1)
    );
    assert_eq!(review.hidden_characters, 2);
    let shown = review.render(|at| format!("at {at}"));
    for part in [
        "board (Task board), version 2",
        &format!(
            "From:     your agent Scout ({}), in channel {channel}",
            "d".repeat(64)
        ),
        &format!("Hash:     {hash}"),
        &format!(
            "Script:   {} bytes (version 1, which runs, is {} bytes)",
            two.len(),
            one.len()
        ),
        "Brief:    28 bytes, CHANGED from the version that runs",
        "Title:    CHANGED from Board",
        "WARNING: 2 characters of the script or the brief are not text",
        "  | A board with a second line.",
        "Difference from version 1, which runs: 2 lines added, 1 removed:",
        "-  return ui.text { \"one\" }",
        "+  return ui.text { \"two\" }",
        "+-- <U+202E>hidden<U+001B>[2J",
    ] {
        assert!(shown.contains(part), "`{part}` in:\n{shown}");
    }
    assert!(
        !shown.contains('\x1b') && !shown.contains('\u{202E}'),
        "nothing that is not text reaches the screen"
    );
}

/// Rollback makes an earlier approved version the current one, and what is
/// then handed out is that version's exact bytes.
#[test]
fn rollback_restores_the_earlier_versions_exact_bytes() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let scripts = [
        script(&["card"], "one"),
        script(&["card"], "two\\r\\n"),
        script(&["card", "vote"], "three"),
    ];
    let versions: Vec<u32> = scripts
        .iter()
        .map(|script| fixture.install(script))
        .collect();
    assert_eq!(versions, [1, 2, 3]);
    let running = |fixture: &mut Fixture| {
        let runnable = fixture.apps().runnable(community, "board").unwrap();
        (runnable.version(), runnable.script().to_vec())
    };
    assert_eq!(running(&mut fixture), (3, scripts[2].clone().into_bytes()));

    // Back one: the bytes of version 2, to the byte.
    assert_eq!(
        fixture.apps().rollback(community, "board", None).unwrap(),
        (Some(3), 2)
    );
    assert_eq!(running(&mut fixture), (2, scripts[1].clone().into_bytes()));
    assert_eq!(
        fixture.apps().runnable(community, "board").unwrap().types(),
        ["card"],
        "and its approved record types"
    );
    // Back again, then forward to a named one.
    assert_eq!(
        fixture.apps().rollback(community, "board", None).unwrap(),
        (Some(2), 1)
    );
    assert_eq!(running(&mut fixture), (1, scripts[0].clone().into_bytes()));
    assert!(matches!(
        fixture.apps().rollback(community, "board", None),
        Err(AppError::NothingEarlier(_))
    ));
    assert_eq!(
        fixture
            .apps()
            .rollback(community, "board", Some(3))
            .unwrap(),
        (Some(1), 3)
    );
    assert_eq!(running(&mut fixture), (3, scripts[2].clone().into_bytes()));

    // An earlier version whose bytes were changed on disk is not gone back
    // to: the app stays on what runs.
    let path = fixture.script_path(2);
    fs::write(&path, "return {}").unwrap();
    assert!(matches!(
        fixture.apps().rollback(community, "board", Some(2)),
        Err(AppError::Changed { version: 2, .. })
    ));
    assert_eq!(running(&mut fixture).0, 3);
}

/// A version that was approved and then fails as it is put to run leaves
/// the previous one as the one that runs.
#[test]
fn an_approved_version_that_fails_as_it_is_put_to_run_leaves_the_previous_one() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let one = script(&["card"], "one");
    let first = fixture.install(&one);
    let second = fixture.install(&script(&["card"], "two"));
    assert_eq!(
        fixture
            .apps()
            .runnable(community, "board")
            .unwrap()
            .version(),
        second
    );
    // The runtime reports that the reload failed.
    let now = fixture
        .apps()
        .failed_to_run(
            community,
            "board",
            second,
            "board.lua:4: attempt to call a nil value",
        )
        .unwrap();
    assert_eq!(now, Some(first));
    let runnable = fixture.apps().runnable(community, "board").unwrap();
    assert_eq!(
        (runnable.version(), runnable.script()),
        (first, one.as_bytes())
    );
    let row = fixture
        .apps()
        .versions(community, "board")
        .unwrap()
        .remove(0);
    assert_eq!(row.state, VersionState::Failed);
    assert_eq!(
        row.detail.as_deref(),
        Some("board.lua:4: attempt to call a nil value")
    );

    // The first version ever fails the same way: nothing runs.
    let mut fresh = Fixture::new();
    let community = fresh.community;
    let only = fresh.install(&script(&["card"], "only"));
    assert_eq!(
        fresh
            .apps()
            .failed_to_run(community, "board", only, "boom")
            .unwrap(),
        None
    );
    assert!(matches!(
        fresh.apps().runnable(community, "board"),
        Err(AppError::NothingApproved { .. })
    ));
}

/// The record types a running script reports are held to the approved ones.
#[test]
fn a_running_script_is_held_to_the_types_that_were_approved() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    fixture.install(&script(&["card", "vote"], "one"));
    let runnable = fixture.apps().runnable(community, "board").unwrap();
    assert!(
        runnable
            .check_loaded(&["vote".into(), "card".into()])
            .is_ok()
    );
    let error = runnable
        .check_loaded(&["card".into(), "vote".into(), "secret".into()])
        .unwrap_err();
    assert!(
        error.contains("declares the record types card, vote, secret now")
            && error.contains("you approved it with card, vote"),
        "{error}"
    );
    assert!(runnable.check_loaded(&["card".into()]).is_err());
}

fn record(keys: &Keys, home: Uuid, artifact_type: &str) -> crate::protocol::artifact::Artifact {
    let tags = [
        ["ar", "1"],
        ["d", &Uuid::new_v4().to_string()],
        ["h", &home.to_string()],
        ["type", artifact_type],
        ["title", "A card"],
        ["op", "create"],
    ]
    .map(|parts| Tag::parse(parts).unwrap());
    parse(
        &EventBuilder::new(Kind::Custom(45_010), "{\"title\":\"A card\"}")
            .tags(tags)
            .sign_with_keys(keys)
            .unwrap(),
    )
    .unwrap()
}

/// Uninstalling removes the app's rows and its folder, and leaves the
/// channel's records alone, and says how many it left.
#[test]
fn remove_leaves_the_channels_records_alone_and_says_so() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    fixture.install(&script(&["card"], "one"));
    let keys = Keys::generate();
    let (general, random) = (Uuid::new_v4(), Uuid::new_v4());
    let card = "bzz.app.board.card".to_owned();
    for (channel, kind) in [
        (general, card.as_str()),
        (general, card.as_str()),
        (random, card.as_str()),
        // Another app's, and one whose name only starts the same.
        (general, "bzz.app.poll.vote"),
        (general, "bzz.app.board-2.card"),
        (general, "buzz.task"),
    ] {
        fixture
            .store
            .apply_artifact_head(community, &record(&keys, channel, kind))
            .unwrap();
    }
    fixture.apps().open_in(community, "board", general).unwrap();
    let before: i64 = fixture
        .store
        .connection
        .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
        .unwrap();
    let dir = fixture.apps().app_dir(community, "board");
    assert!(dir.join("versions/0001/app.lua").is_file());

    let removed = fixture.apps().remove(community, "board").unwrap();
    assert_eq!(
        removed,
        Removed {
            name: "board".into(),
            records: 3,
            record_channels: 2,
            type_prefix: "bzz.app.board.".into(),
            was_open_in: vec![general],
        }
    );
    // Gone: the app, its versions, where it was open, its folder.
    assert!(matches!(
        fixture.apps().runnable(community, "board"),
        Err(AppError::NotInstalled(_))
    ));
    assert!(fixture.apps().list(community).unwrap().is_empty());
    assert!(!dir.exists());
    assert!(
        fixture
            .store
            .apps_open_in(community, general)
            .unwrap()
            .is_empty()
    );
    // Left alone: every record, the app's own included.
    let after: i64 = fixture
        .store
        .connection
        .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        fixture
            .store
            .typed_artifacts(community, general, std::slice::from_ref(&card))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fixture
            .store
            .typed_artifacts(community, random, &[card])
            .unwrap()
            .len(),
        1
    );
    // An app of the same name installed again finds them.
    fixture.install(&script(&["card"], "again"));
    assert_eq!(
        fixture.store.app_record_count(community, "board").unwrap(),
        (3, 2)
    );
}

/// A folder's manifest is checked against its script: the script is the
/// authority. The manifest bzz keeps is its own, written from the script.
#[test]
fn a_folders_manifest_must_say_what_its_script_says() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let temporary = TempDir::new().unwrap();
    let dir = temporary.path().join("board");
    let template = templates::find("board").unwrap();
    template.write("board", None, &dir).unwrap();

    // As written by `bzz app new`: it waits.
    let folder = folder::Folder::read(&dir).unwrap();
    let (version, hash) = waiting(fixture.propose(folder.submission(Proposer::owner())));
    // The same folder proposed again while it waits is the same version.
    let folder = folder::Folder::read(&dir).unwrap();
    assert_eq!(
        fixture.propose(folder.submission(Proposer::owner())),
        Recorded::Waiting {
            version,
            hash: hash.clone(),
            superseded: Vec::new()
        }
    );
    assert_eq!(
        fixture.apps().versions(community, "board").unwrap().len(),
        1
    );
    fixture
        .apps()
        .approve(community, "board", version, &hash)
        .unwrap();
    let bundle = fixture.apps().bundle(community, "board", version).unwrap();
    assert_eq!(bundle.script, template.script.as_bytes());
    assert_eq!(bundle.brief, template.brief);
    assert_eq!(
        bundle.manifest_text, "name = \"board\"\ntitle = \"Board\"\napi = 1\ntypes = [\"card\"]\n",
        "the kept manifest is the one bzz writes, without the folder's comments"
    );

    // The manifest names a type the script does not declare.
    let manifest = fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        manifest.replace("[\"card\"]", "[\"card\", \"vote\"]"),
    )
    .unwrap();
    let folder = folder::Folder::read(&dir).unwrap();
    let recorded = fixture.propose(folder.submission(Proposer::owner()));
    let Recorded::Failed { error, .. } = recorded else {
        panic!("{recorded:?}");
    };
    assert!(
        error.contains("app.toml says the record types are card, vote; the script declares card"),
        "{error}"
    );
    assert!(fixture.apps().waiting(community).unwrap().is_empty());

    // The same folder again, unchanged from what runs: nothing to approve.
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    let folder = folder::Folder::read(&dir).unwrap();
    assert_eq!(
        fixture.propose(folder.submission(Proposer::owner())),
        Recorded::Running { version }
    );
}

/// Every template loads, declares the types its entry says, and has a
/// brief that may be one. A template added to the table is covered here.
/// The three templates of plan D9 at the pane's 40 by 16: a person is
/// stored by id and shown by the name bzz knows (`host.name`), and the
/// name of someone bzz learns of later replaces the short id on screen.
#[test]
fn the_templates_store_ids_and_show_names_at_forty_by_sixteen() {
    use crate::apps::runtime::clock::ManualClock;
    use crate::apps::runtime::{Limits, Who};
    use crate::apps::session::headless::{Headless, HeadlessOptions, START_OF_TIME};
    use crate::apps::session::memory::{MemoryStore, content};
    use serde_json::json;

    let ana = Who::new("a1".repeat(32), "ana");
    let bo = Who::new("b0".repeat(32), "bo");
    let cy = Who::new("c7".repeat(32), "cy");
    let start = |name: &str, seed: &dyn Fn(&mut MemoryStore)| {
        let template = templates::find(name).expect(name);
        let clock = ManualClock::new(START_OF_TIME);
        let mut store = MemoryStore::new(ana.clone(), clock.clone());
        seed(&mut store);
        Headless::start_on(
            template.script,
            HeadlessOptions::new(format!("{name}.lua"))
                .size(40, 16)
                .limits(Limits::patient()),
            store,
            clock,
        )
        .expect("starts")
    };
    let fits = |screen: &str| {
        use unicode_width::UnicodeWidthStr;
        assert_eq!(screen.lines().count(), 16, "{screen}");
        assert!(screen.lines().all(|row| row.width() <= 40), "{screen}");
    };

    // The Board: a card holds no person; it fits.
    let mut board = start("board", &|_| {});
    board.key("n").type_text("Buy milk").key("enter");
    let screen = board.screen();
    fits(&screen);
    assert!(screen.contains("Buy milk"), "{screen}");

    // The Poll: a vote stores the voter's id; who voted is shown by name.
    let mut poll = start("poll", &|store| {
        store.seed(
            "poll",
            content(json!({ "question": "Lunch?", "options": ["Tacos", "Sushi"] })),
            &bo,
        );
        store.seed(
            "vote",
            content(json!({ "voter": bo.id, "option": "Tacos" })),
            &bo,
        );
        // cy voted; bzz has no name for cy yet.
        store.seed(
            "vote",
            content(json!({ "voter": cy.id, "option": "Tacos" })),
            &bo,
        );
    });
    poll.key("enter");
    let screen = poll.screen();
    fits(&screen);
    let short = crate::apps::session::short_name(&cy.id);
    assert!(
        screen.contains(&format!("By: bo, {short}, you")),
        "{screen}"
    );
    let mine = poll.store().find_by("voter", &ana.id).expect("ana's vote");
    assert_eq!(mine.content.get("option"), Some(&json!("Tacos")));
    assert!(poll.session().unnamed().contains(&cy.id));
    poll.session()
        .set_names([(cy.id.clone(), "cy".to_string())]);
    poll.settle();
    assert!(
        poll.screen().contains("By: bo, cy, you"),
        "{}",
        poll.screen()
    );

    // The Approvals: author and decider are ids, shown by name.
    let mut approvals = start("approvals", &|store| {
        store.seed(
            "request",
            content(json!({
                "title": "New laptop", "author": bo.id, "state": "pending",
                "decided_by": null, "reason": null, "order": 1,
            })),
            &bo,
        );
    });
    let screen = approvals.screen();
    fits(&screen);
    assert!(screen.contains("New laptop  bo"), "{screen}");
    approvals.key("a");
    approvals.key("enter");
    let screen = approvals.screen();
    fits(&screen);
    assert!(screen.contains("Author: bo"), "{screen}");
    assert!(screen.contains("Decided by: ana"), "{screen}");
    let decided = approvals.store().find_by("title", "New laptop").unwrap();
    assert_eq!(decided.content.get("author"), Some(&json!(bo.id)));
    assert_eq!(decided.content.get("decided_by"), Some(&json!(ana.id)));
    // Nobody decides their own request: the id is compared.
    approvals.key("n").type_text("Gym pass").key("enter");
    approvals.key("a");
    assert!(
        approvals
            .screen()
            .contains("You cannot decide your own request."),
        "{}",
        approvals.screen()
    );
}

#[test]
fn every_template_loads_and_declares_what_its_entry_says() {
    assert!(templates::find(templates::DEFAULT).is_some());
    for template in templates::TEMPLATES {
        let loaded = load_apart(template.script.as_bytes(), SCRIPT_FILE, &Limits::patient())
            .unwrap_or_else(|error| panic!("{}: {error}", template.name));
        assert_eq!(loaded.types, template.types, "{}", template.name);
        assert!(
            manifest::check_brief(template.brief.as_bytes()).is_ok(),
            "{}",
            template.name
        );
        for kind in template.types {
            assert!(
                template.brief.contains(&format!("`{kind}`")),
                "the brief of {} says what `{kind}` holds",
                template.name
            );
        }
        let manifest = template.manifest("my-app", None).unwrap();
        assert_eq!(
            (manifest.name.as_str(), manifest.title.as_str()),
            ("my-app", "My-app")
        );
        assert_eq!(
            template.manifest(template.name, None).unwrap().title,
            template.title
        );
        assert!(template.manifest("Not A Name", None).is_err());
    }
}

/// A proposer's later version takes the place of its earlier waiting one;
/// another proposer's waits beside it. The files follow the database.
#[test]
fn a_later_proposal_replaces_the_same_proposers_and_files_follow_the_database() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let (first, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "one"),
        "A board.\n",
        agent(),
    )));
    let (other, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "ana's"),
        "A board.\n",
        Proposer::member("e".repeat(64), "Ana"),
    )));
    let recorded = fixture.propose(Submission::new(
        "board",
        script(&["card"], "two"),
        "A board.\n",
        agent(),
    ));
    let Recorded::Waiting {
        version: second,
        superseded,
        ..
    } = recorded
    else {
        panic!("{recorded:?}");
    };
    assert_eq!(superseded, [first]);
    let waiting_now: Vec<u32> = fixture
        .apps()
        .waiting(community)
        .unwrap()
        .iter()
        .map(|version| version.version)
        .collect();
    assert_eq!(waiting_now, [other, second]);
    let exists = |fixture: &mut Fixture, version| {
        fixture
            .apps()
            .version_dir(community, "board", version)
            .exists()
    };
    assert!(
        !exists(&mut fixture, first),
        "the replaced version's files are gone"
    );
    assert!(exists(&mut fixture, other) && exists(&mut fixture, second));

    // Refused: its files go too, and its reason is kept as one line.
    let reason = fixture
        .apps()
        .refuse(community, "board", other, Some("  not\nthis\x1b[2J one  "))
        .unwrap();
    assert_eq!(reason.as_deref(), Some("not this [2J one"));
    assert!(!exists(&mut fixture, other));
    let row = fixture
        .apps()
        .versions(community, "board")
        .unwrap()
        .into_iter()
        .find(|version| version.version == other)
        .unwrap();
    assert_eq!(
        (row.state, row.detail.as_deref()),
        (VersionState::Refused, Some("not this [2J one"))
    );

    // What a crash left half written, and what is not bzz's, in the folder.
    let versions = fixture.apps().app_dir(community, "board").join("versions");
    fs::create_dir(versions.join("0099")).unwrap();
    fs::write(versions.join("0099").join(SCRIPT_FILE), "left by a crash").unwrap();
    fs::write(versions.join("notes.txt"), "the owner's own").unwrap();
    let (third, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "three"),
        "A board.\n",
        Proposer::owner(),
    )));
    assert!(!versions.join("0099").exists());
    assert!(versions.join("notes.txt").exists());
    assert!(exists(&mut fixture, third));
}

/// Only so many approved versions keep their files; the one that runs
/// always does.
#[test]
fn old_approved_versions_lose_their_files_and_keep_their_line() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let total = KEPT_APPROVED as u32 + 3;
    for number in 1..=total {
        fixture.install(&script(&["card"], &format!("version {number}")));
    }
    let versions = fixture.apps().versions(community, "board").unwrap();
    assert_eq!(versions.len(), total as usize, "every line is kept");
    let kept: Vec<u32> = versions
        .iter()
        .filter(|version| version.kept)
        .map(|version| version.version)
        .collect();
    assert_eq!(kept.len(), KEPT_APPROVED);
    assert_eq!(kept.first(), Some(&total));
    assert_eq!(kept.last(), Some(&4));
    for version in &versions {
        assert_eq!(
            fixture
                .apps()
                .version_dir(community, "board", version.version)
                .exists(),
            version.kept,
            "version {}",
            version.version
        );
    }
    let error = fixture
        .apps()
        .rollback(community, "board", Some(1))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "version 1 of board cannot be used: it was approved, and bzz no longer keeps its files"
    );
}

/// The folder and the files are private, as bzz's other data.
#[cfg(unix)]
#[test]
fn an_apps_folder_and_files_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let version = fixture.install(&script(&["card"], "one"));
    let dir = fixture.apps().version_dir(community, "board", version);
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let mut folder = dir.clone();
    while folder != fixture.root.parent().unwrap() {
        assert_eq!(mode(&folder), 0o700, "{}", folder.display());
        folder = folder.parent().unwrap().to_owned();
    }
    for file in [SCRIPT_FILE, MANIFEST_FILE, BRIEF_FILE] {
        assert_eq!(mode(&dir.join(file)), 0o600, "{file}");
    }
    assert_eq!(
        dir,
        fixture
            .root
            .join(community.to_string())
            .join("board/versions/0001")
    );
}

/// Only so many proposers can queue up for one app.
#[test]
fn only_so_many_versions_wait_at_once() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    for number in 0..WAITING_AT_ONCE {
        let member = Proposer::member(format!("{number:064}"), format!("m{number}"));
        fixture.propose(Submission::new(
            "board",
            script(&["card"], &format!("from {number}")),
            "A board.\n",
            member,
        ));
    }
    let one_more = examine_with(
        Submission::new(
            "board",
            script(&["card"], "one more"),
            "A board.\n",
            Proposer::member("f".repeat(64), "late"),
        ),
        &Limits::patient(),
    )
    .unwrap();
    assert!(matches!(
        fixture.apps().record(community, one_more),
        Err(AppError::TooManyWaiting(_))
    ));
    // One who already waits may still replace his own.
    let again = fixture.propose(Submission::new(
        "board",
        script(&["card"], "again"),
        "A board.\n",
        Proposer::member(format!("{:064}", 0), "m0"),
    ));
    assert!(matches!(again, Recorded::Waiting { .. }));
    assert_eq!(
        fixture.apps().waiting(community).unwrap().len(),
        WAITING_AT_ONCE
    );
}

/// v0.16 M5, plan D7: an agent cannot bury its owner in proposals. It has
/// at most [`AGENT_WAITING_AT_ONCE`] versions waiting of all apps together;
/// a later version of an app it has one waiting of takes that one's place.
#[test]
fn an_agent_has_at_most_three_versions_waiting_of_all_apps() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let names = ["board", "poll", "rota", "tally"];
    let mut recorded = Vec::new();
    for name in names {
        let examined = examine_with(
            Submission::new(name, script(&["card"], name), "Cards: `card`.\n", agent()),
            &Limits::patient(),
        )
        .unwrap();
        recorded.push(fixture.apps().record(community, examined));
    }
    for (name, result) in names.iter().zip(&recorded).take(AGENT_WAITING_AT_ONCE) {
        assert!(
            matches!(result, Ok(Recorded::Waiting { .. })),
            "{name}: {result:?}"
        );
    }
    let Err(AppError::TooManyFromAgent(held)) = &recorded[3] else {
        panic!("the fourth app: {:?}", recorded[3]);
    };
    assert_eq!(held, "board version 1, poll version 1, rota version 1");
    assert!(
        fixture.apps().get(community, "tally").is_err(),
        "nothing was recorded of the fourth"
    );
    // Its own waiting version of the board is replaced, not added to.
    let (again, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "again"),
        "Cards: `card`.\n",
        agent(),
    )));
    assert_eq!(again, 2);
    // Another agent, and the owner, are not held by this one's count.
    let other = Proposer::agent("e".repeat(64), "Ranger");
    assert!(matches!(
        fixture.propose(Submission::new("tally", script(&["card"], "t"), "x", other)),
        Recorded::Waiting { .. }
    ));
}

/// v0.16 M5: what became of an agent's versions is told to it once, in the
/// channel it proposed in: applied, refused with the owner's reason word
/// for word, failed with the error. A version it replaced itself, one that
/// waits, and the versions of other proposers are not told to it.
#[test]
fn an_agent_is_told_once_what_became_of_its_versions_in_that_channel() {
    let mut fixture = Fixture::new();
    let community = fixture.community;
    let (here, elsewhere) = (Uuid::new_v4(), Uuid::new_v4());
    let me = || agent().in_channel(here).asked_by("a".repeat(64), "Ana");
    fixture.install(&script(&["card"], "owner's"));
    let (applied, hash) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "applied"),
        "Cards: `card`.\n",
        me(),
    )));
    // Recorded with who asked, and shown so (plan D7).
    let review = fixture.apps().review(community, "board", applied).unwrap();
    assert_eq!(review.from, me());
    assert!(
        review.render(|_| String::new()).contains(&format!(
            "From:     your agent Scout ({}), asked by Ana ({}), in channel {here}",
            "d".repeat(64),
            "a".repeat(64)
        )),
        "{}",
        review.render(|_| String::new())
    );
    assert!(
        fixture
            .apps()
            .untold(community, &"d".repeat(64), here)
            .unwrap()
            .is_empty(),
        "nothing to tell while it waits"
    );
    fixture
        .apps()
        .approve(community, "board", applied, &hash)
        .unwrap();
    let (replaced, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "replaced"),
        "Cards: `card`.\n",
        me(),
    )));
    let (refused, _) = waiting(fixture.propose(Submission::new(
        "board",
        script(&["card"], "refused"),
        "Cards: `card`.\n",
        me(),
    )));
    assert_eq!(refused, replaced + 1);
    fixture
        .apps()
        .refuse(
            community,
            "board",
            refused,
            Some("no filter, only the priority"),
        )
        .unwrap();
    // The owner's own version and another channel's are not this one's.
    fixture.install(&script(&["card"], "owner's again"));
    let untold = fixture
        .apps()
        .untold(community, &"d".repeat(64), here)
        .unwrap();
    let told = untold
        .iter()
        .map(|(version, outcome)| (version.version, outcome.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        told,
        [
            (applied, Outcome::Applied),
            (
                refused,
                Outcome::Refused {
                    reason: Some("no filter, only the priority".into())
                }
            ),
        ]
    );
    assert!(
        fixture
            .apps()
            .untold(community, &"d".repeat(64), elsewhere)
            .unwrap()
            .is_empty()
    );
    for (version, _) in &untold {
        assert!(
            fixture
                .apps()
                .told(community, "board", version.version)
                .unwrap()
        );
        assert!(
            !fixture
                .apps()
                .told(community, "board", version.version)
                .unwrap()
        );
    }
    assert!(
        fixture
            .apps()
            .untold(community, &"d".repeat(64), here)
            .unwrap()
            .is_empty(),
        "told once"
    );
}
