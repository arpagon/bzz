//! `bzz demo`: a generated team in this terminal, offline, with no account,
//! no key on disk, and nothing of the person's real configuration read or
//! written (plan v0.17.0, D2 and rule 22).
//!
//! The real client runs unchanged against [`relay::Relay`], a small relay
//! held in memory on a loopback port the system picks, guarded by a random
//! token in its path. The same [`world`] is written first to a fresh store
//! held in memory, so the first screen is full at once. The rest (the
//! configuration, the media cache) lives in a private folder of the
//! temporary directory, `bzz-demo-<pid>-<random>`, removed when the demo
//! ends: on quitting, on SIGINT, SIGTERM and SIGHUP, and after a panic. A
//! folder left by a demo that was killed outright is removed by the next
//! one.

mod relay;
pub mod world;

use std::{
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    Result,
    app::App,
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    error::Error,
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::command::Command,
};

/// The status line, always, while the demo runs.
pub const STATUS_MARK: &str = "DEMO · generated, offline";

/// The name of the demo's community in the rail.
pub const COMMUNITY_LABEL: &str = "DEMO";

/// What the demo answers where a real community is needed.
pub const NEEDS_A_COMMUNITY: &str = "Demo only: quit and run bzz to join a real community.";

/// Joining or starting a huddle: nothing is opened, no device is touched.
pub const HUDDLE_IS_A_RECORDING: &str = "Demo: the huddle is a recording to look at.";

/// `:apps` and `:app`, until the demo has its Board (plan A2).
pub const NO_APPS_YET: &str = "Demo: the Board comes in a later version.";

const FOLDER_PREFIX: &str = "bzz-demo-";

/// Set once this process has made a demo folder.
static STARTED: AtomicBool = AtomicBool::new(false);

/// The answer the demo gives instead of running `command`, if any.
pub fn refuses(command: &Command) -> Option<&'static str> {
    match command {
        Command::AddCommunity
        | Command::RemoveCommunity
        | Command::SignIn
        | Command::Broker
        | Command::Agent(_) => Some(NEEDS_A_COMMUNITY),
        Command::Agents(which) if which == "local" => Some(NEEDS_A_COMMUNITY),
        Command::Apps | Command::App(_) => Some(NO_APPS_YET),
        Command::Huddle(arguments) => {
            let verb = arguments.split_whitespace().next().unwrap_or_default();
            matches!(
                verb,
                "start" | "join" | "devices" | "input" | "output" | "mute"
            )
            .then_some(HUDDLE_IS_A_RECORDING)
        }
        _ => None,
    }
}

/// The huddle of `#infra` (plan A3, with agent `viz`'s visualizer): Ada,
/// Lin and Sam take turns in generated voices, looping, and You listen with
/// the microphone muted. Your lane stays still: no microphone is opened, so
/// the script's lines for you are left out rather than shown as yours.
pub fn huddle() -> (crate::huddle::synth::Synthetic, Vec<(u8, String)>) {
    use crate::huddle::synth::{DEMO_LOOP_SECONDS, DEMO_SCRIPT, Speaker, Synthetic};
    let others = DEMO_SCRIPT
        .iter()
        .copied()
        .filter(|line| line.speaker != Speaker::Mic)
        .collect::<Vec<_>>();
    let cast = world::Cast::default();
    let people = [(1, &cast.ada), (2, &cast.lin), (3, &cast.sam)]
        .into_iter()
        .map(|(index, keys)| (index, keys.public_key().to_hex()))
        .collect();
    (Synthetic::new(&others, Some(DEMO_LOOP_SECONDS)), people)
}

/// Runs the demo until the person leaves it.
pub async fn run() -> Result<()> {
    remove_leftovers();
    let scratch = Scratch::create()?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| Error::io("demo relay", error))?;
    let port = listener
        .local_addr()
        .map_err(|error| Error::io("demo relay", error))?
        .port();
    let token = hex::encode(rand::random::<[u8; 16]>());
    let world = world::build(&format!("http://127.0.0.1:{port}"));
    let viewer = world.viewer.clone();
    let seed = world.events.clone();
    let team = world.team;
    let relay_pubkey = world.relay_keys.public_key().to_hex();
    relay::Relay::new(world, token.clone()).serve(listener);

    let paths = scratch.paths();
    paths.ensure()?;
    let mut config = Config::default();
    let identity = IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "demo viewer (generated)".into(),
        pubkey: viewer.public_key().to_hex(),
        // Never unlocked: the runtime is handed the key in memory.
        backend: KeyBackend::EncryptedFile,
        key_ref: "demo".into(),
    };
    config.identities.push(identity.clone());
    config.add_community(
        COMMUNITY_LABEL.into(),
        format!("ws://127.0.0.1:{port}/"),
        identity.id,
        true,
    )?;
    let community = config.communities[0].id;
    // In memory: nothing to write to disk, and no wait on it.
    let mut store = Store::open_memory()?;
    store.sync_config(&config)?;
    store.pin_relay_pubkey(community, &relay_pubkey)?;
    for event in &seed {
        // The relay confirms the same events once connected.
        let _ = store.apply_event(community, event);
    }
    store.reconcile_remote_agents(community)?;
    let store = StoreHandle::spawn(store)?;
    let outcome = App::new_demo(
        config,
        paths,
        store,
        SignerHandle::spawn(viewer),
        &format!("/{token}/"),
        team,
    )
    .await?
    .run()
    .await;
    drop(scratch);
    outcome
}

/// The demo's private folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "{FOLDER_PREFIX}{}-{}",
            std::process::id(),
            hex::encode(rand::random::<[u8; 6]>())
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(&root)
            .map_err(|error| Error::io(&root, error))?;
        STARTED.store(true, Ordering::SeqCst);
        Ok(Self(root))
    }

    fn paths(&self) -> Paths {
        Paths {
            config_dir: self.0.join("config"),
            data_dir: self.0.join("data"),
            cache_dir: self.0.join("cache"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Called by `main` after bzz's runtime has stopped: removes this
/// process's demo folder again, in case a task still wrote into it after
/// the demo closed, or the demo ended in a panic.
pub fn clean_up_after_exit() {
    if !STARTED.load(Ordering::SeqCst) {
        return;
    }
    let own = format!("{FOLDER_PREFIX}{}-", std::process::id());
    for folder in demo_folders(&std::env::temp_dir()) {
        if file_name(&folder).starts_with(&own) {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
}

/// Removes the folders of demos that were killed before they could clean
/// up: those whose process is gone (on Unix), or older than a day.
fn remove_leftovers() {
    for folder in demo_folders(&std::env::temp_dir()) {
        if left_behind(&folder) {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Our own `bzz-demo-*` folders in `directory`: real folders, never links.
fn demo_folders(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| file_name(path).starts_with(FOLDER_PREFIX))
        .filter(|path| {
            std::fs::symlink_metadata(path).is_ok_and(|metadata| {
                #[cfg(unix)]
                let mine = std::os::unix::fs::MetadataExt::uid(&metadata)
                    == rustix::process::getuid().as_raw();
                #[cfg(not(unix))]
                let mine = true;
                metadata.is_dir() && mine
            })
        })
        .collect()
}

fn left_behind(folder: &Path) -> bool {
    let name = file_name(folder);
    let pid = name
        .strip_prefix(FOLDER_PREFIX)
        .and_then(|rest| rest.split('-').next())
        .and_then(|pid| pid.parse::<i32>().ok());
    #[cfg(unix)]
    if let Some(pid) = pid {
        if pid == i32::try_from(std::process::id()).unwrap_or_default() {
            return false;
        }
        return rustix::process::Pid::from_raw(pid).is_none_or(|pid| {
            matches!(
                rustix::process::test_kill_process(pid),
                Err(rustix::io::Errno::SRCH)
            )
        });
    }
    let _ = pid;
    std::fs::symlink_metadata(folder)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age.as_secs() > 24 * 60 * 60)
}

#[cfg(test)]
mod tests;
