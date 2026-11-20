//! `bzz app …`: start, try, install, approve and manage apps from the
//! command line (plan v0.16.0, M4). The work is in `bzz::apps::store` and
//! `bzz::apps::dev`; this file reads the arguments, asks and prints.
//!
//! Whoever runs these commands at the owner's shell is the owner: `add`
//! proposes as him, and `approve`, `refuse` and `rollback` are his answers.
//! Everything a script or a proposer wrote goes through `visible` before it
//! is printed, so that it cannot draw on the terminal.

use std::{
    collections::HashMap,
    io::IsTerminal as _,
    path::{Path, PathBuf},
};

use bzz::{
    Result,
    agents::manage::when,
    apps::{
        dev::{self, DevOptions, Trial},
        runtime::{Limits, headless::Step},
        store::{
            AppError, AppVersion, Apps, Listed, Proposer, Recorded, VersionState, examine,
            folder::Folder,
            load_apart,
            manifest::{
                BRIEF_BYTES, BRIEF_FILE, MANIFEST_FILE, SCRIPT_BYTES, SCRIPT_FILE, bytes, visible,
            },
            review::from_whom,
            templates,
        },
    },
    config::Config,
    error::Error,
    paths::Paths,
    protocol::artifact::APP_RECORD_TYPE_PREFIX,
    store::Store,
};
use clap::Subcommand;
use uuid::Uuid;

use crate::{confirm, selected_community};

#[derive(Debug, Subcommand)]
pub enum AppCommand {
    /// List the apps installed in a community, with what runs and what
    /// waits for your answer.
    List {
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Show one app: what runs, its hash, its record types, its brief, the
    /// channels it is open in and every version with what became of it.
    Show {
        name: String,
        /// Print that version's script instead.
        #[arg(long)]
        script: bool,
        /// The version to print the script of. Default: the one that runs.
        #[arg(long)]
        version: Option<u32>,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Propose the app in a folder as a new version, from you. It is loaded
    /// apart first, and it runs only after you approve it.
    Add {
        /// A folder with app.lua, app.toml and brief.md.
        folder: PathBuf,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// The versions that wait for your answer. With a name: what you are
    /// shown of that one before you answer.
    Pending {
        name: Option<String>,
        /// Which waiting version, when more than one waits.
        #[arg(long)]
        version: Option<u32>,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Approve a waiting version: its exact bytes, by the hash you were
    /// shown. It becomes the version that runs.
    Approve {
        name: String,
        /// Which waiting version, when more than one waits.
        #[arg(long)]
        version: Option<u32>,
        /// The hash of the script you approve, as `bzz app pending <name>`
        /// shows it. Without it you are shown the version and asked.
        #[arg(long)]
        hash: Option<String>,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Refuse a waiting version, with a reason that is kept with it.
    Refuse {
        name: String,
        #[arg(long)]
        version: Option<u32>,
        /// One line: why. It is kept with the version, for whoever proposed
        /// it.
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Go back to an earlier approved version: the one before the version
    /// that runs, or the one you name.
    Rollback {
        name: String,
        #[arg(long)]
        to: Option<u32>,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Let a disabled app run again.
    Enable {
        name: String,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Keep an app installed and run it nowhere.
    Disable {
        name: String,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Uninstall an app: every version bzz keeps of it. The channel's
    /// records are left alone.
    Remove {
        name: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Load a script apart and report what is wrong with it, with file and
    /// line. Given a folder, also check its app.toml and brief.md.
    Check {
        /// A script, or a folder with app.lua, app.toml and brief.md.
        path: PathBuf,
    },
    /// Load a script apart on records kept in memory, give it keys, and
    /// print the screen it draws.
    Try {
        /// A script, or a folder with app.lua.
        path: PathBuf,
        /// What to do to it, in order: a key (j, enter, esc, ctrl+u,
        /// shift+tab, f1), type:TEXT, paste:TEXT, resize:40x16, wait:MS.
        /// Put `--` before a step that starts with a hyphen.
        steps: Vec<String>,
        /// The app's region, in columns and rows. The pane gives 40x16.
        #[arg(long, default_value = "40x16")]
        size: String,
        /// Records to start with: a JSON list of
        /// {"type": ..., "content": {...}, "by": "a name"}.
        #[arg(long, value_name = "FILE")]
        seed: Option<PathBuf>,
        /// Also print the app's log.
        #[arg(long)]
        log: bool,
    },
    /// Run your own folder's app in this terminal and reload it whenever
    /// you save the script. No approval: it is your own folder. Its records
    /// are kept in memory and are gone when you leave.
    Dev {
        /// A folder with app.lua, or the script itself.
        folder: PathBuf,
        /// Run it on a channel's records: that is done in bzz's window,
        /// with `:app dev <folder>` in the channel; this says how.
        #[arg(long)]
        channel: Option<String>,
        /// The app's region, in columns and rows. The pane gives 40x16.
        #[arg(long, default_value = "40x16")]
        size: String,
        /// Records to start with, as for `bzz app try`.
        #[arg(long, value_name = "FILE")]
        seed: Option<PathBuf>,
    },
    /// Write a folder to start an app from: a script, its manifest and its
    /// brief.
    New {
        /// The app's name: a lowercase letter, then a-z, 0-9, _ or -.
        #[arg(required_unless_present = "templates")]
        name: Option<String>,
        /// The app to start from. `bzz app new --templates` names them.
        #[arg(long)]
        template: Option<String>,
        /// List the templates and write nothing.
        #[arg(long)]
        templates: bool,
        /// The app's title. Default: its name, with a capital.
        #[arg(long)]
        title: Option<String>,
        /// The folder to write. It must not exist. Default: ./<name>.
        #[arg(long, value_name = "FOLDER")]
        to: Option<PathBuf>,
    },
}

fn failed(message: impl Into<String>) -> Error {
    Error::Config(message.into())
}

fn open_store(paths: &Paths, config: &Config) -> Result<Store> {
    let mut store = Store::open(paths.database_file())?;
    store.sync_config(config)?;
    Ok(store)
}

/// The script of a path that is a script or a folder with one, and the
/// name it has in error messages.
fn script_of(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(SCRIPT_FILE)
    } else {
        path.to_owned()
    }
}

fn script_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| SCRIPT_FILE.to_owned())
}

fn read_script(path: &Path) -> Result<Vec<u8>> {
    bzz::apps::store::folder::read_bounded(path, SCRIPT_BYTES).map_err(failed)
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// What became of a version, in a few words.
fn outcome(version: &AppVersion, runs: bool) -> String {
    let detail = version
        .detail
        .as_deref()
        .map(|detail| visible(detail.lines().next().unwrap_or_default()))
        .filter(|detail| !detail.is_empty());
    match (version.state, detail) {
        (VersionState::Waiting, _) => "waits for your answer".into(),
        (VersionState::Approved, _) if runs => "approved; it runs".into(),
        (VersionState::Approved, _) if version.kept => "approved".into(),
        (VersionState::Approved, _) => "approved; its files are no longer kept".into(),
        (VersionState::Refused, Some(reason)) => format!("refused: {reason}"),
        (VersionState::Refused, None) => "refused".into(),
        (VersionState::Failed, Some(error)) => format!("failed: {error}"),
        (VersionState::Failed, None) => "failed".into(),
        (VersionState::Superseded, Some(by)) => format!("superseded: {by}"),
        (VersionState::Superseded, None) => "superseded".into(),
    }
}

/// The state of an app, in a few words.
fn state_of(listed: &Listed) -> String {
    let mut words = match (&listed.running, listed.app.enabled) {
        (Some(running), true) => format!(
            "version {} runs ({})",
            running.version,
            short(&running.script_sha256)
        ),
        (Some(running), false) => format!(
            "disabled (version {} is approved, {})",
            running.version,
            short(&running.script_sha256)
        ),
        (None, _) => "nothing approved, nothing runs".to_owned(),
    };
    if !listed.waiting.is_empty() {
        words.push_str(&format!(
            "; {} for your answer",
            if listed.waiting.len() == 1 {
                "1 version waits".to_owned()
            } else {
                format!("{} versions wait", listed.waiting.len())
            }
        ));
    }
    words
}

fn list(apps: &Apps<'_>, community: &bzz::config::CommunityConfig) -> Result<()> {
    let listed = apps.list(community.id)?;
    if listed.is_empty() {
        println!(
            "no apps are installed in {}; write one with `bzz app new <name>` and add it with `bzz app add <folder>`",
            community.label
        );
    }
    for app in listed {
        let types = app
            .running
            .iter()
            .chain(&app.waiting)
            .map(|version| version.types.join(","))
            .next()
            .unwrap_or_default();
        println!(
            "{}\t{}\t{}\t{}\t{}",
            app.app.name,
            visible(&app.title()),
            state_of(&app),
            types,
            match app.channels.len() {
                0 => "open in no channel".to_owned(),
                n => format!("open in {}", plural(n, "channel", "channels")),
            },
        );
    }
    Ok(())
}

/// A waiting version to answer: the one named, or the only one.
fn waiting_version(listed: &Listed, version: Option<u32>) -> Result<u32> {
    let name = &listed.app.name;
    match (version, listed.waiting.as_slice()) {
        (Some(version), _) => Ok(version),
        (None, [only]) => Ok(only.version),
        (None, []) => Err(failed(format!(
            "no version of {name} waits for an answer; `bzz app show {name}` lists its versions"
        ))),
        (None, several) => Err(failed(format!(
            "{} of {name} wait for an answer: {}. Name one with --version",
            plural(several.len(), "version", "versions"),
            several
                .iter()
                .map(|version| format!(
                    "{} (from {})",
                    version.version,
                    from_whom(&version.proposer)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn show(
    apps: &Apps<'_>,
    store_channels: &HashMap<Uuid, String>,
    community: Uuid,
    name: &str,
) -> Result<()> {
    let listed = apps.get(community, name)?;
    let versions = apps.versions(community, name)?;
    println!("{} ({})", listed.app.name, visible(&listed.title()));
    println!("state:    {}", state_of(&listed));
    if let Some(running) = &listed.running {
        println!("hash:     {}", running.script_sha256);
        println!(
            "types:    {}   (on the relay: {})",
            running.types.join(", "),
            running
                .types
                .iter()
                .map(|kind| format!("{APP_RECORD_TYPE_PREFIX}{name}.{kind}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "sizes:    script {}, brief {}",
            bytes(running.script_bytes),
            bytes(running.brief_bytes)
        );
        println!(
            "from:     {}, proposed {}, approved {}",
            from_whom(&running.proposer),
            when(running.proposed_at),
            running.decided_at.map_or_else(|| "?".to_owned(), when)
        );
        println!(
            "files:    {}",
            apps.version_dir(community, name, running.version).display()
        );
    }
    if listed.channels.is_empty() {
        println!("open in:  no channel");
    }
    for channel in &listed.channels {
        match store_channels.get(channel) {
            Some(label) => println!("open in:  #{} ({channel})", visible(label)),
            None => println!("open in:  {channel}"),
        }
    }
    if let Some(running) = &listed.running {
        // The brief is read from disk and compared with what was approved.
        match apps.bundle(community, name, running.version) {
            Ok(bundle) => {
                println!("brief:");
                for line in visible(&bundle.brief).lines() {
                    println!("  | {line}");
                }
            }
            Err(error) => println!("brief:    not shown: {error}"),
        }
        if let Err(error) = apps.runnable(community, name) {
            println!("it does not run: {error}");
        }
    }
    println!("versions:");
    for version in &versions {
        println!(
            "  {}\t{}\t{}\t{}\t{}",
            version.version,
            outcome(version, listed.app.current == Some(version.version)),
            from_whom(&version.proposer),
            when(version.proposed_at),
            short(&version.script_sha256),
        );
    }
    Ok(())
}

fn print_review(apps: &Apps<'_>, community: Uuid, name: &str, version: u32) -> Result<String> {
    let review = apps.review(community, name, version)?;
    print!("{}", review.render(when));
    Ok(review.hash)
}

fn approve(
    apps: &mut Apps<'_>,
    community: Uuid,
    name: &str,
    version: u32,
    hash: Option<&str>,
) -> Result<()> {
    let hash = match hash {
        // The hash is the answer: the version is not shown again.
        Some(hash) => hash.trim().to_ascii_lowercase(),
        None => {
            let shown = print_review(apps, community, name, version)?;
            println!();
            if !std::io::stdin().is_terminal() {
                return Err(failed(format!(
                    "nothing was approved: there is no terminal to ask you at. To approve these exact bytes, name their hash: bzz app approve {name} --version {version} --hash {shown}"
                )));
            }
            if !confirm(&format!(
                "Approve these exact bytes as version {version} of {name}, and let it run?"
            ))? {
                return Err(failed(format!(
                    "nothing was approved; version {version} of {name} still waits. Refuse it with `bzz app refuse {name} --version {version}`"
                )));
            }
            shown
        }
    };
    let approved = apps.approve(community, name, version, &hash)?;
    match approved.replaced {
        Some(replaced) => println!(
            "{name}: version {version} is approved and is the version that runs now, in place of version {replaced}. `bzz app rollback {name}` goes back"
        ),
        None => println!("{name}: version {version} is approved and is the version that runs now"),
    }
    Ok(())
}

fn add(apps: &mut Apps<'_>, community: Uuid, folder: &Path) -> Result<()> {
    let folder = Folder::read(folder).map_err(failed)?;
    let name = folder.manifest.name.clone();
    let recorded = apps.propose(community, folder.submission(Proposer::owner()))?;
    match recorded {
        Recorded::Running { version } => {
            println!(
                "{name}: the folder is version {version}, which runs: there is nothing to approve"
            );
            Ok(())
        }
        Recorded::Failed { version, error } => Err(failed(format!(
            "{name}: it does not load, and nothing was changed (recorded as version {version}, failed):\n{}",
            visible(&error)
        ))),
        Recorded::Waiting {
            version,
            hash,
            superseded,
        } => {
            for earlier in superseded {
                println!("{name}: version {earlier}, which waited, is replaced by this one");
            }
            print_review(apps, community, name.as_str(), version)?;
            println!();
            if std::io::stdin().is_terminal() {
                if confirm(&format!(
                    "Approve these exact bytes as version {version} of {name}, and let it run?"
                ))? {
                    return approve(apps, community, &name, version, Some(&hash));
                }
                println!(
                    "{name}: version {version} waits for your answer. Nothing of it runs. `bzz app approve {name}` or `bzz app refuse {name}`"
                );
                return Ok(());
            }
            println!(
                "{name}: version {version} waits for your approval. Nothing of it runs until then.\napprove these exact bytes with: bzz app approve {name} --version {version} --hash {hash}"
            );
            Ok(())
        }
    }
}

fn pending(
    apps: &Apps<'_>,
    community: &bzz::config::CommunityConfig,
    name: Option<&str>,
    version: Option<u32>,
) -> Result<()> {
    if let Some(name) = name {
        let listed = apps.get(community.id, name)?;
        let version = waiting_version(&listed, version)?;
        let hash = print_review(apps, community.id, name, version)?;
        println!();
        println!("approve: bzz app approve {name} --version {version} --hash {hash}");
        println!("refuse:  bzz app refuse {name} --version {version} --reason \"...\"");
        return Ok(());
    }
    let waiting = apps.waiting(community.id)?;
    if waiting.is_empty() {
        println!("nothing waits for your answer in {}", community.label);
    }
    for version in waiting {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            version.name,
            version.version,
            from_whom(&version.proposer),
            when(version.proposed_at),
            bytes(version.script_bytes),
            version.script_sha256,
        );
    }
    Ok(())
}

fn check(path: &Path) -> Result<()> {
    if path.is_dir() {
        // Every file of the folder, as `bzz app add` would take it, with
        // nothing recorded.
        let folder = Folder::read(path).map_err(failed)?;
        let (script_bytes, brief_bytes) = (folder.script.len(), folder.brief.len());
        let manifest = folder.manifest.clone();
        let examined = examine(folder.submission(Proposer::owner())).map_err(Error::from)?;
        if let Some(error) = examined.error() {
            return Err(failed(format!("{}: {}", path.display(), visible(error))));
        }
        println!(
            "ok: {} ({}) loads. api {}, record types: {}",
            manifest.name,
            visible(&manifest.title),
            manifest.api,
            examined.types().unwrap_or_default().join(", ")
        );
        println!("    {SCRIPT_FILE}: {script_bytes} of {SCRIPT_BYTES} bytes");
        println!("    {BRIEF_FILE}: {brief_bytes} of {BRIEF_BYTES} bytes");
        println!("    {MANIFEST_FILE} says what the script says");
        return Ok(());
    }
    let script = read_script(path)?;
    let loaded = load_apart(&script, &script_name(path), &Limits::default())
        .map_err(|error| failed(visible(&error)))?;
    println!(
        "ok: {} loads. api 1, record types: {}",
        script_name(path),
        loaded.types.join(", ")
    );
    println!("    {} of {SCRIPT_BYTES} bytes", script.len());
    Ok(())
}

fn try_app(
    path: &Path,
    steps: &[String],
    size: &str,
    seed: Option<&Path>,
    log: bool,
) -> Result<()> {
    let script_path = script_of(path);
    let (width, height) = dev::parse_size(size).map_err(failed)?;
    let steps = steps
        .iter()
        .map(|written| Step::parse(written))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(failed)?;
    let seeds = seed
        .map(dev::read_seed)
        .transpose()
        .map_err(failed)?
        .unwrap_or_default();
    let trying = dev::try_script(Trial {
        script: read_script(&script_path)?,
        script_name: script_name(&script_path),
        width,
        height,
        steps,
        seeds,
    })
    .map_err(|error| Error::io("the script's thread", error))?;
    let tried = trying.tried;
    // What a script draws is cells of text; no control character is one.
    print!("{}", visible(&tried.screen));
    if log {
        for line in &tried.logs {
            eprintln!("log: {}", visible(line));
        }
    }
    if let Some(warning) = trying.seed_warning {
        eprintln!("note: {warning}");
    }
    if tried.closed {
        eprintln!("note: the app closed itself (host.quit)");
    }
    if !tried.responding {
        return Err(failed("the app did not come to rest: it does not respond"));
    }
    match tried.error {
        Some(error) => Err(failed(visible(&error))),
        None => Ok(()),
    }
}

fn dev(folder: &Path, channel: Option<&str>, size: &str, seed: Option<&Path>) -> Result<()> {
    if let Some(channel) = channel {
        return Err(failed(format!(
            "a folder runs on a channel's records in the pane of bzz's window, not from here: open bzz, select #{channel}, and type :app dev {}. Without --channel the folder's app runs in this terminal, on records kept in memory",
            visible(&folder.display().to_string())
        )));
    }
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(failed(
            "`bzz app dev` draws the app in a terminal, and this is not one. `bzz app try <folder> [keys]` prints the screen instead",
        ));
    }
    let script = script_of(folder);
    let (width, height) = dev::parse_size(size).map_err(failed)?;
    let seeds = seed
        .map(dev::read_seed)
        .transpose()
        .map_err(failed)?
        .unwrap_or_default();
    let logs = dev::run(DevOptions {
        script: script.clone(),
        width,
        height,
        seeds,
    })?;
    for line in logs.iter().rev().take(20).rev() {
        println!("log: {line}");
    }
    println!(
        "left {}. Its records were kept in memory and are gone. To install it: bzz app add {}",
        script.display(),
        script.parent().unwrap_or(folder).display()
    );
    Ok(())
}

fn new(
    name: Option<&str>,
    template: Option<&str>,
    list: bool,
    title: Option<&str>,
    to: Option<&Path>,
) -> Result<()> {
    if list {
        for template in templates::TEMPLATES {
            println!("{}\t{}\t{}", template.name, template.title, template.about);
        }
        return Ok(());
    }
    let name = name.ok_or_else(|| failed("give the app's name: bzz app new <name>"))?;
    let wanted = template.unwrap_or(templates::DEFAULT);
    let template = templates::find(wanted).ok_or_else(|| {
        failed(format!(
            "there is no template called {wanted}; the templates are: {}",
            templates::names()
        ))
    })?;
    let dir = to.map_or_else(|| PathBuf::from(name), Path::to_path_buf);
    let manifest = template.write(name, title, &dir).map_err(failed)?;
    println!(
        "wrote {} from the template {}:",
        dir.display(),
        template.name
    );
    println!(
        "  {SCRIPT_FILE}    the script, written against docs/app-api.md (api {})",
        manifest.api
    );
    println!(
        "  {MANIFEST_FILE}   name {}, title {}, record types {}",
        manifest.name,
        manifest.title,
        manifest.types.join(", ")
    );
    println!("  {BRIEF_FILE}   what it does and what its records hold: keep it true");
    println!("next:");
    println!(
        "  bzz app dev {0}     run it here and reload it as you save",
        dir.display()
    );
    println!(
        "  bzz app check {0}   load it apart and report what is wrong",
        dir.display()
    );
    println!(
        "  bzz app add {0}     propose it; it runs after you approve it",
        dir.display()
    );
    Ok(())
}

pub fn command(command: AppCommand, paths: &Paths, config: &Config) -> Result<()> {
    // The commands that need no community and no database.
    let command = match command {
        AppCommand::Check { path } => return check(&path),
        AppCommand::Try {
            path,
            steps,
            size,
            seed,
            log,
        } => return try_app(&path, &steps, &size, seed.as_deref(), log),
        AppCommand::Dev {
            folder,
            channel,
            size,
            seed,
        } => return dev(&folder, channel.as_deref(), &size, seed.as_deref()),
        AppCommand::New {
            name,
            template,
            templates,
            title,
            to,
        } => {
            return new(
                name.as_deref(),
                template.as_deref(),
                templates,
                title.as_deref(),
                to.as_deref(),
            );
        }
        other => other,
    };
    let requested = match &command {
        AppCommand::List { community }
        | AppCommand::Show { community, .. }
        | AppCommand::Add { community, .. }
        | AppCommand::Pending { community, .. }
        | AppCommand::Approve { community, .. }
        | AppCommand::Refuse { community, .. }
        | AppCommand::Rollback { community, .. }
        | AppCommand::Enable { community, .. }
        | AppCommand::Disable { community, .. }
        | AppCommand::Remove { community, .. } => *community,
        AppCommand::Check { .. }
        | AppCommand::Try { .. }
        | AppCommand::Dev { .. }
        | AppCommand::New { .. } => None,
    };
    let community = selected_community(config, requested)?.clone();
    let mut store = open_store(paths, config)?;
    let channels: HashMap<Uuid, String> = store
        .channels(community.id)?
        .into_iter()
        .map(|channel| (channel.id, channel.name))
        .collect();
    let mut apps = Apps::new(&mut store, paths);
    let id = community.id;
    match command {
        AppCommand::List { .. } => list(&apps, &community),
        AppCommand::Show {
            name,
            script,
            version,
            ..
        } => {
            if !script {
                return show(&apps, &channels, id, &name);
            }
            let listed = apps.get(id, &name)?;
            let version = version.or(listed.app.current).ok_or_else(|| {
                failed(format!(
                    "{name} has no version that runs; name one with --version (`bzz app show {name}` lists them)"
                ))
            })?;
            let bundle = apps.bundle(id, &name, version)?;
            print!("{}", visible(&String::from_utf8_lossy(&bundle.script)));
            Ok(())
        }
        AppCommand::Add { folder, .. } => add(&mut apps, id, &folder),
        AppCommand::Pending { name, version, .. } => {
            pending(&apps, &community, name.as_deref(), version)
        }
        AppCommand::Approve {
            name,
            version,
            hash,
            ..
        } => {
            let listed = apps.get(id, &name)?;
            let version = waiting_version(&listed, version)?;
            approve(&mut apps, id, &name, version, hash.as_deref())
        }
        AppCommand::Refuse {
            name,
            version,
            reason,
            ..
        } => {
            let listed = apps.get(id, &name)?;
            let version = waiting_version(&listed, version)?;
            let reason = apps.refuse(id, &name, version, reason.as_deref())?;
            match reason {
                Some(reason) => println!("{name}: version {version} is refused: {reason}"),
                None => println!("{name}: version {version} is refused"),
            }
            match listed.running {
                Some(running) => println!("version {} still runs", running.version),
                None => println!("nothing of {name} runs"),
            }
            Ok(())
        }
        AppCommand::Rollback { name, to, .. } => {
            let (from, to) = apps.rollback(id, &name, to)?;
            let runnable = apps.runnable(id, &name);
            match from {
                Some(from) if from == to => println!("{name}: version {to} is the one that runs"),
                Some(from) => println!(
                    "{name}: version {to} is the version that runs again, in place of version {from}"
                ),
                None => println!("{name}: version {to} is the version that runs"),
            }
            match runnable {
                Ok(runnable) => println!("hash:  {}", runnable.hash()),
                Err(AppError::Disabled(_)) => {
                    println!("{name} is disabled: `bzz app enable {name}` lets it run");
                }
                Err(error) => return Err(error.into()),
            }
            Ok(())
        }
        AppCommand::Enable { name, .. } => {
            apps.set_enabled(id, &name, true)?;
            match apps.runnable(id, &name) {
                Ok(runnable) => println!(
                    "{name} is enabled: version {} runs where it is open",
                    runnable.version()
                ),
                Err(error) => println!("{name} is enabled. {error}"),
            }
            Ok(())
        }
        AppCommand::Disable { name, .. } => {
            apps.set_enabled(id, &name, false)?;
            println!(
                "{name} is disabled: it stays installed and runs nowhere. `bzz app enable {name}` undoes this"
            );
            Ok(())
        }
        AppCommand::Remove { name, yes, .. } => {
            apps.get(id, &name)?;
            if !yes
                && !confirm(&format!(
                    "Uninstall {name} from {}: every version bzz keeps of it? The channel's records stay.",
                    community.label
                ))?
            {
                return Err(failed("nothing was removed"));
            }
            let removed = apps.remove(id, &name)?;
            println!(
                "{name} is uninstalled: every version bzz kept of it is gone from this machine"
            );
            println!(
                "The channel's records were left alone. They belong to the channel, on the relay, and whoever has an app named {name} still uses them: {} of types {}* {} stored by this bzz{}",
                plural(removed.records, "record", "records"),
                removed.type_prefix,
                if removed.records == 1 { "is" } else { "are" },
                match removed.record_channels {
                    0 => String::new(),
                    n => format!(", in {}", plural(n, "channel", "channels")),
                }
            );
            println!(
                "An app named {name} that is installed again finds them. To delete records, delete them in the app before you uninstall it."
            );
            Ok(())
        }
        AppCommand::Check { .. }
        | AppCommand::Try { .. }
        | AppCommand::Dev { .. }
        | AppCommand::New { .. } => Ok(()),
    }
}
