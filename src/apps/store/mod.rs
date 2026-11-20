//! Where apps are kept, and the rule that nothing runs unapproved (plan
//! v0.16.0, M4; rule 16).
//!
//! ```text
//!   <data>/apps/<community>/<name>/versions/<n>/app.lua     the script
//!                                               app.toml    the manifest
//!                                               brief.md    the brief
//! ```
//!
//! **Files hold bytes; the database holds decisions.** A version's three
//! files are written once, when it is proposed, and never changed. Which
//! version runs, which hash was approved, who proposed a version and what
//! became of it are rows (`crate::store::apps`, migration 21). A file on
//! disk says nothing about whether it may run: the folder can be read, and
//! copied, with any tool, and a file changed there is found out, because
//! every read of a version through this module compares the bytes with the
//! hash recorded when the version was proposed.
//!
//! **One function hands out a script to run:** [`Apps::runnable`]. It gives
//! the bytes of the app's current version, and only when that version was
//! approved, the app is enabled, and the bytes on disk hash to the approved
//! hash. A [`Runnable`] cannot be made any other way. The owner's own folder
//! under `bzz app dev` is the one thing that runs without it (plan D5).
//!
//! The life of a version:
//!
//! 1. [`examine`] checks what was proposed and **loads the script apart**,
//!    with the headless driver. It runs the script, so it can take as long
//!    as a stuck script is given: call it off bzz's own thread.
//! 2. [`Apps::record`] writes the result down. A version that does not load
//!    is recorded as failed, with the error; its bytes are not kept and it
//!    is never offered. One that loads waits.
//! 3. [`Apps::review`] is what the owner is shown ([`review::Review`]).
//! 4. [`Apps::approve`] takes the version's number and the hash of the
//!    script the owner was shown; [`Apps::refuse`] takes a reason.
//! 5. The approved version becomes the one [`Apps::runnable`] gives. If it
//!    fails as it is put to run, [`Apps::failed_to_run`] records that and
//!    the app goes back to the version that ran before.
//! 6. [`Apps::rollback`] makes an earlier approved version the current one.
//!
//! How many versions are kept: [`KEPT_APPROVED`], [`KEPT_LINES`].

pub mod diff;
pub mod folder;
pub mod manifest;
pub mod review;
pub mod templates;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::agents::runtime::approval::clean_reason;
use crate::apps::runtime::headless::{Headless, HeadlessOptions};
use crate::apps::runtime::{API_VERSION, Limits, Phase};
use crate::error::Error;
use crate::paths::{Paths, set_private_permissions};
use crate::protocol::artifact::APP_RECORD_TYPE_PREFIX;
use crate::store::Store;
use crate::store::apps::NewAppVersion;

pub use crate::store::apps::{AppVersion, InstalledApp, Proposer, ProposerKind, VersionState};

use manifest::{
    BRIEF_FILE, MANIFEST_FILE, Manifest, SCRIPT_BYTES, SCRIPT_FILE, check_brief, check_name,
    check_title, sha256, title_from_name,
};
use review::{Review, Running, TypesChange, hidden_in};

/// Approved versions whose files are kept: the newest ten, besides the one
/// that runs and the one that ran before it. An app of the largest size is
/// then under 2 MiB on disk. An older approved version keeps its line and
/// can no longer be gone back to.
pub const KEPT_APPROVED: usize = 10;

/// Versions without files (refused, failed, superseded, or approved long
/// ago) of which the line is kept: who, when, the hashes, what became of
/// it. Older lines are dropped.
pub const KEPT_LINES: usize = 50;

/// Versions of one app that may wait for an answer at once. A proposer has
/// at most one waiting (a later one takes its place), so this bounds how
/// many proposers can queue up before the owner answers.
pub const WAITING_AT_ONCE: usize = 8;

/// The longest error bzz keeps of a version that failed.
const ERROR_BYTES: usize = 2000;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The name cannot be an app's.
    #[error("{0}")]
    Name(String),
    #[error("no app named {0} is installed in this community; `bzz app list` shows them")]
    NotInstalled(String),
    #[error("{name} has no version {version}; `bzz app show {name}` lists its versions")]
    NoSuchVersion { name: String, version: u32 },
    #[error("version {version} of {name} does not wait for an answer: it is {}", .state.as_str())]
    NotWaiting {
        name: String,
        version: u32,
        state: VersionState,
    },
    #[error(
        "the hash does not name version {version} of {name}: its script is {recorded}. Nothing was approved"
    )]
    AnotherHash {
        name: String,
        version: u32,
        recorded: String,
    },
    /// The bytes on disk are not the ones that were proposed and approved.
    #[error(
        "{name}: {file} of version {version} is not what was {}: the recorded hash is {recorded}, and the file on disk has {found}. It was changed outside bzz, and it is not used",
        if *.approved { "approved" } else { "proposed" }
    )]
    Changed {
        name: String,
        version: u32,
        file: &'static str,
        approved: bool,
        recorded: String,
        found: String,
    },
    #[error("{name}: {file} of version {version} is missing from {}", .dir.display())]
    Missing {
        name: String,
        version: u32,
        file: &'static str,
        dir: PathBuf,
    },
    #[error("{0} is disabled; `bzz app enable {0}` lets it run again")]
    Disabled(String),
    #[error("{name} has no approved version{}: nothing of it runs", waiting_words(*.waiting))]
    NothingApproved { name: String, waiting: usize },
    #[error("version {version} of {name} cannot be used: {why}")]
    NotUsable {
        name: String,
        version: u32,
        why: String,
    },
    #[error("{0} has no earlier approved version that bzz still keeps")]
    NothingEarlier(String),
    #[error(
        "{WAITING_AT_ONCE} versions of {0} already wait for an answer; answer one before another is proposed"
    )]
    TooManyWaiting(String),
    #[error(transparent)]
    Store(#[from] Error),
}

fn waiting_words(waiting: usize) -> String {
    match waiting {
        0 => String::new(),
        1 => " (one waits for your answer)".into(),
        n => format!(" ({n} wait for your answer)"),
    }
}

impl From<AppError> for Error {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Store(error) => error,
            other => Error::Config(other.to_string()),
        }
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A version as it is proposed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Submission {
    /// The app's name.
    pub name: String,
    /// Its title. None: the title the app has, or its name with a capital.
    pub title: Option<String>,
    pub script: Vec<u8>,
    /// The brief, as bytes: it is checked to be text.
    pub brief: Vec<u8>,
    /// A manifest that came with the script (a folder's `app.toml`). It is
    /// checked against what the script declares.
    pub declared: Option<Manifest>,
    pub from: Proposer,
    /// What the script is called in an error: `app.lua`, `board.lua`.
    pub script_name: String,
}

impl Submission {
    /// A script and its brief, as an agent or the channel gives them.
    pub fn new(
        name: impl Into<String>,
        script: impl Into<Vec<u8>>,
        brief: impl Into<Vec<u8>>,
        from: Proposer,
    ) -> Self {
        let name = name.into();
        Self {
            script_name: format!("{name}.lua"),
            name,
            title: None,
            script: script.into(),
            brief: brief.into(),
            declared: None,
            from,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A proposed version after it was checked and loaded apart. Only
/// [`examine`] makes one, so nothing is recorded as loading that was not
/// loaded.
#[derive(Clone, Debug)]
pub struct Examined {
    submission: Submission,
    /// The record types the script declared, or why the version fails.
    verdict: Result<Vec<String>, String>,
}

impl Examined {
    pub fn name(&self) -> &str {
        &self.submission.name
    }

    /// The record types the script declared when it was loaded.
    pub fn types(&self) -> Option<&[String]> {
        self.verdict.as_deref().ok()
    }

    /// Why the version fails, when it does: with file and line when the
    /// script is at fault.
    pub fn error(&self) -> Option<&str> {
        self.verdict.as_ref().err().map(String::as_str)
    }
}

/// What a script declared when it was loaded apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Loaded {
    pub types: Vec<String>,
    /// The first screen it drew, at the size of the pane.
    pub screen: String,
}

/// Loads a script apart: on a thread of its own, on an empty record store
/// in memory, at the size of the pane, and lets it draw once. Answers what
/// it declared, or the error with its file and line.
///
/// A script that loads and then fails in `app.init` or in its first
/// `app.view` fails here too: it would show nothing but its error.
pub fn load_apart(script: &[u8], script_name: &str, limits: &Limits) -> Result<Loaded, String> {
    if script.is_empty() {
        return Err(format!("{script_name}: the script is empty"));
    }
    if script.len() > SCRIPT_BYTES {
        return Err(format!(
            "{script_name}: the script is {} bytes; an app's script may be {SCRIPT_BYTES}",
            script.len()
        ));
    }
    let mut app = Headless::start(
        script.to_vec(),
        HeadlessOptions::new(script_name).limits(limits.clone()),
    )
    .map_err(|error| format!("{script_name}: no thread could be started to load it: {error}"))?;
    let tried = app.tried();
    if let Some(error) = tried.error {
        return Err(error);
    }
    if !tried.responding {
        return Err(format!(
            "{script_name}: the script did not come to rest after it was loaded"
        ));
    }
    match app.phase() {
        Phase::Running | Phase::Closed => Ok(Loaded {
            types: tried.types,
            screen: tried.screen,
        }),
        phase => Err(format!(
            "{script_name}: the script did not start ({phase:?})"
        )),
    }
}

/// Checks a proposed version and loads its script apart, with the limits
/// an app runs under. See [`examine_with`].
pub fn examine(submission: Submission) -> AppResult<Examined> {
    examine_with(submission, &Limits::default())
}

/// Checks a proposed version and loads its script apart.
///
/// The only error is a name no app may have: there is then nothing to
/// record the version under. Everything else that is wrong with it (a
/// script or a brief too large, a brief that is not text, a script that
/// does not load, a manifest that says something else than the script) is
/// the version's failure, which [`Apps::record`] writes down.
///
/// This runs the script. It returns when the script has drawn, or when the
/// time a stuck script is given has passed (a few seconds).
pub fn examine_with(submission: Submission, limits: &Limits) -> AppResult<Examined> {
    check_name(&submission.name).map_err(AppError::Name)?;
    let verdict = verdict(&submission, limits);
    Ok(Examined {
        submission,
        verdict,
    })
}

fn verdict(submission: &Submission, limits: &Limits) -> Result<Vec<String>, String> {
    if let Some(title) = &submission.title {
        check_title(title)?;
    }
    if let Some(declared) = &submission.declared
        && declared.name != submission.name
    {
        return Err(format!(
            "{MANIFEST_FILE} names the app {}, and it was proposed as {}",
            declared.name, submission.name
        ));
    }
    check_brief(&submission.brief)?;
    let loaded = load_apart(&submission.script, &submission.script_name, limits)?;
    if let Some(declared) = &submission.declared {
        declared.agrees_with(&loaded.types)?;
    }
    Ok(loaded.types)
}

/// What recording a proposed version did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Recorded {
    /// It loads, and waits for the owner's answer. Nothing of it runs.
    Waiting {
        version: u32,
        /// The hash an approval must name.
        hash: String,
        /// Waiting versions of the same proposer that it took the place of.
        superseded: Vec<u32>,
    },
    /// It does not load. It is recorded as failed, is never offered for
    /// approval, and whatever ran before still runs.
    Failed { version: u32, error: String },
    /// Its script, brief and manifest are those of the version that runs:
    /// there is nothing to approve, and nothing was recorded.
    Running { version: u32 },
    // The same bytes proposed again by a proposer whose version still
    // waits are `Waiting` with that version's number: nothing is recorded
    // twice.
}

/// The files of one version, read from disk and found to be the bytes that
/// were recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bundle {
    pub script: Vec<u8>,
    pub manifest: Manifest,
    pub manifest_text: String,
    pub brief: String,
}

/// An approved script, ready to be given to a runtime. Only
/// [`Apps::runnable`] makes one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Runnable {
    community: Uuid,
    name: String,
    title: String,
    version: u32,
    hash: String,
    types: Vec<String>,
    script: Vec<u8>,
}

impl Runnable {
    pub fn community(&self) -> Uuid {
        self.community
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The approved hash, which these bytes were found to have.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The record types the owner approved, in the script's order. Build
    /// the fence of rule 17 from these.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// The exact bytes that were approved.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    pub fn into_script(self) -> Vec<u8> {
        self.script
    }

    /// The script's name in error messages.
    pub fn script_name(&self) -> String {
        format!("{}.lua", self.name)
    }

    /// Holds a running script to what was approved: the record types a
    /// runtime reports when it has loaded these bytes
    /// (`Output::Loaded { types }`) must be the approved ones. A script can
    /// compute `app.types`, so the same bytes could declare more at another
    /// time than they did when they were loaded apart. Whoever runs the
    /// script calls this, and stops the app when it is an error.
    pub fn check_loaded(&self, loaded: &[String]) -> Result<(), String> {
        let mut approved = self.types.clone();
        let mut found = loaded.to_vec();
        approved.sort();
        found.sort();
        if approved == found {
            return Ok(());
        }
        Err(format!(
            "{} declares the record types {} now; you approved it with {}. It was stopped: a change of record types needs your approval",
            self.name,
            loaded.join(", "),
            self.types.join(", ")
        ))
    }
}

/// What became of a proposed version, as its proposer is told it (an
/// agent's tool in M5, an offer in the channel in M6). A proposer calls
/// [`Apps::record`] (after [`examine`]) and then asks [`Apps::outcome`]
/// until it is no longer [`Outcome::Waiting`]; the owner answers in the
/// TUI's approval or at the shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The owner has not answered.
    Waiting,
    /// Approved, and the version that runs (or ran: a later one may have
    /// replaced it since).
    Applied,
    /// The owner refused it, with his reason if he gave one.
    Refused { reason: Option<String> },
    /// It did not load, or it failed as it was put to run: the error, with
    /// file and line when the script is at fault.
    Failed { error: String },
    /// Its proposer replaced it with a later version before an answer.
    Superseded,
}

impl Outcome {
    pub fn of(version: &AppVersion) -> Self {
        match version.state {
            VersionState::Waiting => Self::Waiting,
            VersionState::Approved => Self::Applied,
            VersionState::Refused => Self::Refused {
                reason: version.detail.clone(),
            },
            VersionState::Failed => Self::Failed {
                error: version.detail.clone().unwrap_or_default(),
            },
            VersionState::Superseded => Self::Superseded,
        }
    }
}

/// What approving a version did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Approved {
    pub version: u32,
    /// The version that ran until now.
    pub replaced: Option<u32>,
}

/// What uninstalling an app left behind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removed {
    pub name: String,
    /// Live records of the app's types that this bzz has stored. They are
    /// the channel's, and were left alone.
    pub records: usize,
    /// In how many channels those records are.
    pub record_channels: usize,
    /// The prefix of the record types on the relay: `bzz.app.<name>.`.
    pub type_prefix: String,
    /// The channels the app was open in.
    pub was_open_in: Vec<Uuid>,
}

/// An app with what a list shows of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listed {
    pub app: InstalledApp,
    /// The version that runs.
    pub running: Option<AppVersion>,
    /// The versions that wait for an answer, oldest first.
    pub waiting: Vec<AppVersion>,
    /// The newest version of all.
    pub newest: Option<AppVersion>,
    pub channels: Vec<Uuid>,
}

impl Listed {
    /// The app's title: of the version that runs, else of the newest that
    /// has one.
    pub fn title(&self) -> String {
        self.running
            .iter()
            .chain(&self.waiting)
            .chain(&self.newest)
            .find_map(|version| version.title.clone())
            .unwrap_or_else(|| title_from_name(&self.app.name))
    }
}

/// The installed apps of this machine: the database and the folder.
pub struct Apps<'a> {
    store: &'a mut Store,
    root: PathBuf,
}

fn create_private_dir(path: &Path) -> Result<(), Error> {
    fs::create_dir_all(path).map_err(|error| Error::io(path, error))?;
    set_private_permissions(path)
}

fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

impl<'a> Apps<'a> {
    pub fn new(store: &'a mut Store, paths: &Paths) -> Self {
        Self::at(store, paths.apps_dir())
    }

    /// With the folder given: `<data>/apps`.
    pub fn at(store: &'a mut Store, root: impl Into<PathBuf>) -> Self {
        Self {
            store,
            root: root.into(),
        }
    }

    /// The folder of one app: `<data>/apps/<community>/<name>`.
    pub fn app_dir(&self, community: Uuid, name: &str) -> PathBuf {
        self.root.join(community.to_string()).join(name)
    }

    /// The folder of one version's three files.
    pub fn version_dir(&self, community: Uuid, name: &str, version: u32) -> PathBuf {
        self.app_dir(community, name)
            .join("versions")
            .join(format!("{version:04}"))
    }

    fn installed(&self, community: Uuid, name: &str) -> AppResult<InstalledApp> {
        check_name(name).map_err(AppError::Name)?;
        self.store
            .app(community, name)?
            .ok_or_else(|| AppError::NotInstalled(name.to_owned()))
    }

    fn version_row(&self, community: Uuid, name: &str, version: u32) -> AppResult<AppVersion> {
        self.store
            .app_version(community, name, version)?
            .ok_or_else(|| AppError::NoSuchVersion {
                name: name.to_owned(),
                version,
            })
    }

    /// The apps of a community, with what a list shows of each.
    pub fn list(&self, community: Uuid) -> AppResult<Vec<Listed>> {
        let mut listed = Vec::new();
        for app in self.store.apps(community)? {
            listed.push(self.listed(app)?);
        }
        Ok(listed)
    }

    /// One app, with what a list shows of it.
    pub fn get(&self, community: Uuid, name: &str) -> AppResult<Listed> {
        let app = self.installed(community, name)?;
        self.listed(app)
    }

    fn listed(&self, app: InstalledApp) -> AppResult<Listed> {
        let versions = self.store.app_versions(app.community, &app.name)?;
        let running = app
            .current
            .and_then(|current| versions.iter().find(|version| version.version == current))
            .cloned();
        let mut waiting: Vec<AppVersion> = versions
            .iter()
            .filter(|version| version.state == VersionState::Waiting)
            .cloned()
            .collect();
        waiting.reverse();
        let channels = self.store.app_channels(app.community, &app.name)?;
        Ok(Listed {
            running,
            waiting,
            newest: versions.first().cloned(),
            channels,
            app,
        })
    }

    /// Every version of an app that bzz has a line of, newest first.
    pub fn versions(&self, community: Uuid, name: &str) -> AppResult<Vec<AppVersion>> {
        self.installed(community, name)?;
        Ok(self.store.app_versions(community, name)?)
    }

    /// What became of a version, for whoever proposed it.
    pub fn outcome(&self, community: Uuid, name: &str, version: u32) -> AppResult<Outcome> {
        self.installed(community, name)?;
        Ok(Outcome::of(&self.version_row(community, name, version)?))
    }

    /// The versions that wait for the owner's answer in a community, the
    /// one that has waited longest first.
    pub fn waiting(&self, community: Uuid) -> AppResult<Vec<AppVersion>> {
        Ok(self.store.app_versions_waiting(community)?)
    }

    /// Examines a proposed version and records it: for a caller that may
    /// wait while the script is loaded (the command line).
    pub fn propose(&mut self, community: Uuid, submission: Submission) -> AppResult<Recorded> {
        let examined = examine(submission)?;
        self.record(community, examined)
    }

    /// Records an examined version. One that loads waits for the owner's
    /// answer, in place of an earlier waiting one of the same proposer; one
    /// that does not is recorded as failed, with the error, and its bytes
    /// are not kept. Nothing that runs is changed either way.
    pub fn record(&mut self, community: Uuid, examined: Examined) -> AppResult<Recorded> {
        let Examined {
            submission,
            verdict,
        } = examined;
        let name = submission.name.clone();
        check_name(&name).map_err(AppError::Name)?;
        let app = self.store.app(community, &name)?;
        let running = match app.as_ref().and_then(|app| app.current) {
            Some(current) => self.store.app_version(community, &name, current)?,
            None => None,
        };
        let script_sha256 = sha256(&submission.script);
        let brief_sha256 = sha256(&submission.brief);
        let mut new = NewAppVersion {
            community,
            name: name.clone(),
            state: VersionState::Failed,
            detail: None,
            title: None,
            api: None,
            types: None,
            script_sha256: script_sha256.clone(),
            manifest_sha256: None,
            brief_sha256,
            script_bytes: submission.script.len() as u64,
            brief_bytes: submission.brief.len() as u64,
            proposer: submission.from.clone(),
        };
        let types = match verdict {
            Ok(types) => types,
            Err(error) => {
                let error = cut(&error, ERROR_BYTES);
                new.detail = Some(error.clone());
                let recorded = self.store.app_record_version(&new, now(), |_| Ok(()))?;
                self.tidy(community, &name)?;
                return Ok(Recorded::Failed {
                    version: recorded.version,
                    error,
                });
            }
        };
        let title = match &submission.title {
            // Checked when it was examined.
            Some(title) => title.trim().to_owned(),
            None => running
                .as_ref()
                .and_then(|running| running.title.clone())
                .unwrap_or_else(|| title_from_name(&name)),
        };
        let manifest = Manifest {
            name: name.clone(),
            title: title.clone(),
            api: API_VERSION,
            types: types.clone(),
        };
        let manifest_text = manifest.to_text();
        let manifest_sha256 = sha256(manifest_text.as_bytes());
        if let Some(running) = &running
            && running.state == VersionState::Approved
            && running.script_sha256 == script_sha256
            && running.brief_sha256 == new.brief_sha256
            && running.manifest_sha256.as_deref() == Some(manifest_sha256.as_str())
        {
            return Ok(Recorded::Running {
                version: running.version,
            });
        }
        let versions = self.store.app_versions(community, &name)?;
        // The same bytes from the same proposer, still waiting: it is that
        // version, not another one.
        if let Some(same) = versions.iter().find(|version| {
            version.state == VersionState::Waiting
                && version.proposer.is(&submission.from)
                && version.script_sha256 == script_sha256
                && version.brief_sha256 == new.brief_sha256
                && version.manifest_sha256.as_deref() == Some(manifest_sha256.as_str())
        }) {
            return Ok(Recorded::Waiting {
                version: same.version,
                hash: script_sha256,
                superseded: Vec::new(),
            });
        }
        let others_waiting = versions
            .iter()
            .filter(|version| {
                version.state == VersionState::Waiting && !version.proposer.is(&submission.from)
            })
            .count();
        if others_waiting >= WAITING_AT_ONCE {
            return Err(AppError::TooManyWaiting(name));
        }
        new.state = VersionState::Waiting;
        new.title = Some(title);
        new.api = Some(API_VERSION);
        new.types = Some(types);
        new.manifest_sha256 = Some(manifest_sha256);
        let versions_dir = self.app_dir(community, &name).join("versions");
        let (community_dir, app_dir) = (
            self.root.join(community.to_string()),
            self.app_dir(community, &name),
        );
        let root = self.root.clone();
        let recorded = self.store.app_record_version(&new, now(), |version| {
            for dir in [&root, &community_dir, &app_dir, &versions_dir] {
                create_private_dir(dir)?;
            }
            let dir = versions_dir.join(format!("{version:04}"));
            // A folder with this number and no row is what a crash left.
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|error| Error::io(&dir, error))?;
            }
            create_private_dir(&dir)?;
            for (file, bytes) in [
                (SCRIPT_FILE, submission.script.as_slice()),
                (MANIFEST_FILE, manifest_text.as_bytes()),
                (BRIEF_FILE, submission.brief.as_slice()),
            ] {
                let path = dir.join(file);
                fs::write(&path, bytes).map_err(|error| Error::io(&path, error))?;
                set_private_permissions(&path)?;
            }
            Ok(())
        })?;
        self.tidy(community, &name)?;
        Ok(Recorded::Waiting {
            version: recorded.version,
            hash: script_sha256,
            superseded: recorded.superseded,
        })
    }

    /// Reads one file of a version and compares it with the recorded hash.
    fn verified(&self, row: &AppVersion, file: &'static str, recorded: &str) -> AppResult<Vec<u8>> {
        let dir = self.version_dir(row.community, &row.name, row.version);
        let path = dir.join(file);
        let missing = || AppError::Missing {
            name: row.name.clone(),
            version: row.version,
            file,
            dir: dir.clone(),
        };
        if !row.kept {
            return Err(AppError::NotUsable {
                name: row.name.clone(),
                version: row.version,
                why: format!(
                    "it is {}, and bzz no longer keeps its files",
                    row.state.as_str()
                ),
            });
        }
        if !path.is_file() {
            return Err(missing());
        }
        // A file that grew past any size a version's file can have is not
        // read whole to find out that it changed.
        let read = folder::read_bounded(&path, SCRIPT_BYTES).map_err(|_| AppError::Changed {
            name: row.name.clone(),
            version: row.version,
            file,
            approved: row.state == VersionState::Approved,
            recorded: recorded.to_owned(),
            found: "more bytes than any version's file".into(),
        })?;
        let found = sha256(&read);
        if found != recorded {
            return Err(AppError::Changed {
                name: row.name.clone(),
                version: row.version,
                file,
                approved: row.state == VersionState::Approved,
                recorded: recorded.to_owned(),
                found,
            });
        }
        Ok(read)
    }

    fn bundle_of(&self, row: &AppVersion) -> AppResult<Bundle> {
        let script = self.verified(row, SCRIPT_FILE, &row.script_sha256)?;
        let manifest_hash = row
            .manifest_sha256
            .as_deref()
            .ok_or_else(|| AppError::NotUsable {
                name: row.name.clone(),
                version: row.version,
                why: "it never loaded, and has no manifest".into(),
            })?;
        let manifest_bytes = self.verified(row, MANIFEST_FILE, manifest_hash)?;
        let brief = self.verified(row, BRIEF_FILE, &row.brief_sha256)?;
        // Both were written by bzz and have the hashes bzz recorded.
        let unreadable = |what: &str| AppError::NotUsable {
            name: row.name.clone(),
            version: row.version,
            why: format!("its {what} cannot be read"),
        };
        let manifest_text =
            String::from_utf8(manifest_bytes).map_err(|_| unreadable(MANIFEST_FILE))?;
        let manifest = Manifest::parse(&manifest_text).map_err(|_| unreadable(MANIFEST_FILE))?;
        let brief = String::from_utf8(brief).map_err(|_| unreadable(BRIEF_FILE))?;
        Ok(Bundle {
            script,
            manifest,
            manifest_text,
            brief,
        })
    }

    /// The three files of a version, each found to be the bytes that were
    /// recorded. For showing an app, and for sharing it (M6).
    pub fn bundle(&self, community: Uuid, name: &str, version: u32) -> AppResult<Bundle> {
        self.installed(community, name)?;
        let row = self.version_row(community, name, version)?;
        self.bundle_of(&row)
    }

    /// What the owner is shown of a version: who it comes from, its brief,
    /// the record types it declares with any change in them, its sizes, and
    /// its difference from the version that runs.
    pub fn review(&self, community: Uuid, name: &str, version: u32) -> AppResult<Review> {
        let app = self.installed(community, name)?;
        let row = self.version_row(community, name, version)?;
        let bundle = self.bundle_of(&row)?;
        let mut running = None;
        let mut running_unreadable = None;
        let mut old_script = String::new();
        let mut old_brief = None;
        if let Some(current) = app.current.filter(|current| *current != version) {
            let current_row = self.version_row(community, name, current)?;
            match self.bundle_of(&current_row) {
                Ok(current_bundle) => {
                    old_script = String::from_utf8_lossy(&current_bundle.script).into_owned();
                    old_brief = Some(current_bundle.brief);
                    running = Some(Running {
                        version: current,
                        hash: current_row.script_sha256.clone(),
                        title: current_bundle.manifest.title,
                        script_bytes: current_row.script_bytes,
                        brief_bytes: current_row.brief_bytes,
                        types: current_row.types.clone(),
                    });
                }
                Err(error) => running_unreadable = Some(error.to_string()),
            }
        }
        // With a version that runs and cannot be read, the types are still
        // compared: they are in the database.
        let running_types = match (&running, app.current) {
            (Some(running), _) => Some(running.types.clone()),
            (None, Some(current)) if current != version => {
                Some(self.version_row(community, name, current)?.types)
            }
            _ => None,
        };
        let new_script = String::from_utf8_lossy(&bundle.script);
        Ok(Review {
            community,
            name: name.to_owned(),
            title: bundle.manifest.title.clone(),
            version,
            hash: row.script_sha256.clone(),
            from: row.proposer.clone(),
            proposed_at: row.proposed_at,
            script_bytes: row.script_bytes,
            brief_bytes: row.brief_bytes,
            brief_changed: old_brief.as_ref().is_some_and(|old| *old != bundle.brief),
            types_change: TypesChange::between(running_types.as_deref(), &row.types),
            types: row.types.clone(),
            running,
            running_unreadable,
            difference: diff::difference(&old_script, &new_script),
            hidden_characters: hidden_in(&bundle.script, &bundle.brief),
            brief: bundle.brief,
        })
    }

    /// Approves a waiting version: the one with this number, whose script
    /// has this hash, and whose files on disk are still the ones that were
    /// proposed. It becomes the version that runs.
    pub fn approve(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        hash: &str,
    ) -> AppResult<Approved> {
        let app = self.installed(community, name)?;
        let row = self.version_row(community, name, version)?;
        if row.state != VersionState::Waiting {
            return Err(AppError::NotWaiting {
                name: name.to_owned(),
                version,
                state: row.state,
            });
        }
        if hash != row.script_sha256 {
            return Err(AppError::AnotherHash {
                name: name.to_owned(),
                version,
                recorded: row.script_sha256,
            });
        }
        // What was shown is what is on disk, or it is not approved.
        if let Err(error) = self.bundle_of(&row) {
            if matches!(error, AppError::Changed { .. } | AppError::Missing { .. }) {
                self.store.app_fail_version(
                    community,
                    name,
                    version,
                    "its files were changed on disk after it was proposed",
                    now(),
                )?;
                self.tidy(community, name)?;
            }
            return Err(error);
        }
        if !self
            .store
            .app_approve(community, name, version, hash, now())?
        {
            let state = self.version_row(community, name, version)?.state;
            return Err(AppError::NotWaiting {
                name: name.to_owned(),
                version,
                state,
            });
        }
        self.tidy(community, name)?;
        Ok(Approved {
            version,
            replaced: app.current,
        })
    }

    /// Refuses a waiting version, with the owner's reason. The reason is
    /// kept as one bounded line; the version's files are not kept.
    pub fn refuse(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        reason: Option<&str>,
    ) -> AppResult<Option<String>> {
        self.installed(community, name)?;
        let row = self.version_row(community, name, version)?;
        let reason = reason.and_then(clean_reason);
        if row.state != VersionState::Waiting
            || !self
                .store
                .app_refuse(community, name, version, reason.as_deref(), now())?
        {
            let state = self.version_row(community, name, version)?.state;
            return Err(AppError::NotWaiting {
                name: name.to_owned(),
                version,
                state,
            });
        }
        self.tidy(community, name)?;
        Ok(reason)
    }

    /// Records that a version failed as it was put to run: the runtime
    /// could not load the approved bytes, or the app declared other record
    /// types than were approved. If it was the current version, the app
    /// goes back to the version that ran before it, which is the one still
    /// running. Answers the version that is current now.
    pub fn failed_to_run(
        &mut self,
        community: Uuid,
        name: &str,
        version: u32,
        error: &str,
    ) -> AppResult<Option<u32>> {
        self.installed(community, name)?;
        self.version_row(community, name, version)?;
        let current = self.store.app_fail_version(
            community,
            name,
            version,
            &cut(error, ERROR_BYTES),
            now(),
        )?;
        self.tidy(community, name)?;
        Ok(current)
    }

    /// Makes an earlier approved version the current one: `to`, or the
    /// newest approved version before the current one. Its bytes are the
    /// ones that were approved then, or it is not used. Answers `(from,
    /// to)`.
    pub fn rollback(
        &mut self,
        community: Uuid,
        name: &str,
        to: Option<u32>,
    ) -> AppResult<(Option<u32>, u32)> {
        let app = self.installed(community, name)?;
        let usable = |version: &AppVersion| version.state == VersionState::Approved && version.kept;
        let target = match to {
            Some(version) => self.version_row(community, name, version)?,
            None => {
                let before = app.current.unwrap_or(u32::MAX);
                self.store
                    .app_versions(community, name)?
                    .into_iter()
                    .find(|version| version.version < before && usable(version))
                    .ok_or_else(|| AppError::NothingEarlier(name.to_owned()))?
            }
        };
        if !usable(&target) {
            return Err(AppError::NotUsable {
                name: name.to_owned(),
                version: target.version,
                why: if target.state == VersionState::Approved {
                    "it was approved, and bzz no longer keeps its files".into()
                } else {
                    format!("it is {}, not approved", target.state.as_str())
                },
            });
        }
        self.bundle_of(&target)?;
        if app.current != Some(target.version)
            && !self
                .store
                .app_use_version(community, name, target.version, now())?
        {
            return Err(AppError::NotUsable {
                name: name.to_owned(),
                version: target.version,
                why: "it is no longer an approved version that bzz keeps".into(),
            });
        }
        Ok((app.current, target.version))
    }

    /// **The one function that hands out a script to run.** It gives the
    /// bytes of the app's current version when, and only when: the app is
    /// installed and enabled; a version was approved; and the script on
    /// disk hashes to the hash that was approved. Otherwise it says why
    /// not, and nothing runs.
    pub fn runnable(&self, community: Uuid, name: &str) -> AppResult<Runnable> {
        let app = self.installed(community, name)?;
        if !app.enabled {
            return Err(AppError::Disabled(name.to_owned()));
        }
        let nothing_approved = |store: &Store| -> AppResult<AppError> {
            let waiting = store
                .app_versions(community, name)?
                .iter()
                .filter(|version| version.state == VersionState::Waiting)
                .count();
            Ok(AppError::NothingApproved {
                name: name.to_owned(),
                waiting,
            })
        };
        let Some(current) = app.current else {
            return Err(nothing_approved(&*self.store)?);
        };
        let row = self.version_row(community, name, current)?;
        if row.state != VersionState::Approved {
            return Err(nothing_approved(&*self.store)?);
        }
        let script = self.verified(&row, SCRIPT_FILE, &row.script_sha256)?;
        Ok(Runnable {
            community,
            name: name.to_owned(),
            title: row.title.unwrap_or_else(|| title_from_name(name)),
            version: current,
            hash: row.script_sha256,
            types: row.types,
            script,
        })
    }

    /// Enables or disables an app. A disabled app stays installed, with
    /// its versions and its channels, and [`Apps::runnable`] refuses it.
    pub fn set_enabled(&mut self, community: Uuid, name: &str, enabled: bool) -> AppResult<bool> {
        let app = self.installed(community, name)?;
        self.store
            .app_set_enabled(community, name, enabled, now())?;
        Ok(app.enabled != enabled)
    }

    /// Remembers that the app is open in a channel. One instance for each
    /// channel (plan D4).
    pub fn open_in(&mut self, community: Uuid, name: &str, channel: Uuid) -> AppResult<bool> {
        self.installed(community, name)?;
        Ok(self.store.app_open_in(community, name, channel, now())?)
    }

    pub fn close_in(&mut self, community: Uuid, name: &str, channel: Uuid) -> AppResult<bool> {
        self.installed(community, name)?;
        Ok(self.store.app_close_in(community, name, channel)?)
    }

    /// The apps open in a channel, by name.
    pub fn open_in_channel(&self, community: Uuid, channel: Uuid) -> AppResult<Vec<String>> {
        Ok(self.store.apps_open_in(community, channel)?)
    }

    /// Uninstalls an app: its rows and its folder, with every version.
    /// **The channel's records are left alone**: they belong to the channel
    /// and to everyone who has the app, and the answer says how many this
    /// bzz knows of.
    pub fn remove(&mut self, community: Uuid, name: &str) -> AppResult<Removed> {
        self.installed(community, name)?;
        let (records, record_channels) = self.store.app_record_count(community, name)?;
        let was_open_in = self.store.app_channels(community, name)?;
        self.store.app_remove(community, name)?;
        let dir = self.app_dir(community, name);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|error| Error::io(&dir, error))?;
        }
        Ok(Removed {
            name: name.to_owned(),
            records,
            record_channels,
            type_prefix: format!("{APP_RECORD_TYPE_PREFIX}{name}."),
            was_open_in,
        })
    }

    /// Lets go of the versions that need not be kept, and removes from the
    /// app's folder every version's files that the database does not say
    /// are kept: those of a refused, failed or superseded version, of an
    /// approved one that is too old, and what a crash left half written.
    fn tidy(&mut self, community: Uuid, name: &str) -> AppResult<()> {
        self.store
            .app_prune_versions(community, name, KEPT_APPROVED, KEPT_LINES)?;
        let kept: Vec<u32> = self
            .store
            .app_versions(community, name)?
            .iter()
            .filter(|version| version.kept)
            .map(|version| version.version)
            .collect();
        let versions = self.app_dir(community, name).join("versions");
        let Ok(entries) = fs::read_dir(&versions) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let numbered = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok());
            // What is not a version's folder is not bzz's to remove.
            if let Some(version) = numbered
                && !kept.contains(&version)
            {
                let path = entry.path();
                fs::remove_dir_all(&path).map_err(|error| Error::io(&path, error))?;
            }
        }
        Ok(())
    }
}
