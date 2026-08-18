#![forbid(unsafe_code)]

use std::{io::Write as _, path::PathBuf};

use bzz::{
    Result,
    agent::{CodexExecutable, Doctor},
    app::App,
    auth::{IdentityManager, backup, read_passphrase},
    config::{Config, KeyBackend},
    error::Error,
    paths::Paths,
    store::{Store, writer::StoreHandle},
};
use clap::{CommandFactory as _, Parser, Subcommand, ValueEnum};
use secrecy::ExposeSecret as _;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
#[command(
    name = "bzz",
    version,
    about = "A human-first terminal client for Buzz"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage signing identities without exposing secrets in arguments.
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// Manage host-derived Buzz communities.
    Community {
        #[command(subcommand)]
        command: CommunityCommand,
    },
    /// Manage cached conversation data.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Inspect and manage terminal media support.
    Media {
        #[command(subcommand)]
        command: MediaCommand,
    },
    /// Inspect and select color themes.
    Theme {
        #[command(subcommand)]
        command: ThemeCommand,
    },
    /// Manage local, draft-only assistants.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Validate configuration and database migrations.
    Check,
    /// Print non-secret filesystem paths.
    Paths,
    /// Generate a shell completion script on standard output.
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}
impl From<CompletionShell> for clap_complete::Shell {
    fn from(value: CompletionShell) -> Self {
        match value {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Elvish => Self::Elvish,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::PowerShell => Self::PowerShell,
            CompletionShell::Zsh => Self::Zsh,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackendArg {
    Keychain,
    EncryptedFile,
}
impl From<BackendArg> for KeyBackend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Keychain => Self::Keychain,
            BackendArg::EncryptedFile => Self::EncryptedFile,
        }
    }
}

#[derive(Debug, Subcommand)]
enum IdentityCommand {
    /// Generate an identity and store it securely.
    New {
        #[arg(long)]
        label: String,
        #[arg(long, value_enum, default_value = "keychain")]
        backend: BackendArg,
    },
    /// Import an nsec or 64-character secret read from the controlling terminal.
    Import {
        #[arg(long)]
        label: String,
        #[arg(long, value_enum, default_value = "keychain")]
        backend: BackendArg,
    },
    /// Export a password-encrypted NIP-49 backup to a new owner-only file.
    Backup {
        id: Uuid,
        #[arg(long)]
        output: PathBuf,
    },
    /// Import a password-encrypted NIP-49 backup as a new identity.
    ImportBackup {
        #[arg(long)]
        label: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, value_enum, default_value = "keychain")]
        backend: BackendArg,
    },
    /// Restore a missing/corrupt credential from an nsec read interactively.
    Restore { id: Uuid },
    /// Restore a missing/corrupt credential from a NIP-49 backup.
    RestoreBackup {
        id: Uuid,
        #[arg(long)]
        input: PathBuf,
    },
    /// Verify that a credential is available and matches its configured pubkey.
    Verify { id: Uuid },
    /// List public identity metadata.
    List,
    /// Delete an identity only when no community uses it.
    Remove {
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CommunityCommand {
    Add {
        label: String,
        relay_url: String,
        identity_id: Uuid,
        #[arg(long)]
        allow_insecure_localhost: bool,
    },
    List,
    Remove {
        id: Uuid,
        #[arg(long)]
        purge: bool,
        #[arg(long)]
        yes: bool,
    },
    Default {
        id: Uuid,
    },
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    Purge {
        #[arg(long)]
        community: Option<Uuid>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MediaCommand {
    /// Print configured protocol, limits, and disk-cache use.
    Status,
    /// Evict oldest verified blobs until the configured quota is met.
    Prune,
    /// Remove cached and staged media without altering messages.
    Clear {
        #[arg(long)]
        community: Option<Uuid>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// Add a local Codex assistant without storing credentials.
    Add {
        #[arg(long)]
        label: String,
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    /// List configured local assistants.
    List,
    /// Remove a local assistant configuration.
    Remove {
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
    /// Check whether the locally installed Codex supports bzz's safe invocation.
    Doctor,
}

#[derive(Debug, Subcommand)]
enum ThemeCommand {
    /// List themes compiled into this bzz binary.
    List,
    /// Print an exportable semantic definition for a built-in theme.
    Show { name: String },
    /// Validate configured themes and theme.toml.
    Check,
    /// Select a global or per-community theme.
    Use {
        name: String,
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Reset the global or per-community theme selection.
    Reset {
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Export a built-in theme to a new owner-only file.
    Export {
        name: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Print the optional theme.toml path.
    Path,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("bzz: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Command::Completions { shell }) = &cli.command {
        let mut output = Vec::new();
        clap_complete::generate(
            clap_complete::Shell::from(*shell),
            &mut Cli::command(),
            "bzz",
            &mut output,
        );
        if let Err(error) = std::io::stdout().write_all(&output)
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(Error::io("stdout", error));
        }
        return Ok(());
    }
    let paths = Paths::discover()?;
    paths.ensure()?;
    let mut config = Config::load(&paths)?;
    match cli.command {
        Some(Command::Identity { command }) => identity_command(command, &paths, &mut config),
        Some(Command::Community { command }) => {
            community_command(command, &paths, &mut config).await
        }
        Some(Command::Cache { command }) => cache_command(command, &paths, &mut config),
        Some(Command::Media { command }) => media_command(command, &paths, &config),
        Some(Command::Theme { command }) => theme_command(command, &paths, &mut config),
        Some(Command::Agent { command }) => agent_command(command, &paths, &mut config).await,
        Some(Command::Check) => {
            config.validate()?;
            bzz::ui::keymap::KeyMap::load(&paths)?;
            let warnings = bzz::ui::theme::check(&paths, configured_theme_names(&config))?;
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            let mut store = Store::open(paths.database_file())?;
            store.sync_config(&config)?;
            println!("configuration, theme, media, and database are valid");
            Ok(())
        }
        Some(Command::Paths) => {
            println!("config: {}", paths.config_file().display());
            println!("keymap: {}", paths.keymap_file().display());
            println!("theme:  {}", paths.theme_file().display());
            println!("data:   {}", paths.database_file().display());
            println!("cache:  {}", paths.cache_dir.display());
            Ok(())
        }
        Some(Command::Completions { .. }) => Ok(()),
        None => {
            // Validate keymap.toml before entering raw mode. The M1 router
            // consumes this typed map during its vertical-slice cutover; this
            // early load already guarantees malformed input cannot leave a
            // user in a partially initialized terminal.
            bzz::ui::keymap::KeyMap::load(&paths)?;
            let mut store = Store::open(paths.database_file())?;
            store.sync_config(&config)?;
            let handle = StoreHandle::spawn(store)?;
            App::new(config, paths, handle).await?.run().await
        }
    }
}

fn identity_command(command: IdentityCommand, paths: &Paths, config: &mut Config) -> Result<()> {
    let manager = IdentityManager::new(paths);
    match command {
        IdentityCommand::New { label, backend } => {
            let backend = KeyBackend::from(backend);
            let passphrase = matches!(backend, KeyBackend::EncryptedFile)
                .then(|| read_passphrase("New identity passphrase: ", true))
                .transpose()?;
            let identity = manager.create(config, label, backend, passphrase.as_ref())?;
            if let Err(error) = config.save(paths) {
                config.identities.retain(|entry| entry.id != identity.id);
                let _ = manager.delete(&identity);
                return Err(error);
            }
            println!("identity: {}\npubkey:   {}", identity.id, identity.pubkey);
            Ok(())
        }
        IdentityCommand::Import { label, backend } => {
            let input = Zeroizing::new(
                rpassword::prompt_password("Nostr nsec or secret hex: ")
                    .map_err(|error| Error::io("controlling terminal", error))?,
            );
            let backend = KeyBackend::from(backend);
            let passphrase = matches!(backend, KeyBackend::EncryptedFile)
                .then(|| read_passphrase("New identity passphrase: ", true))
                .transpose()?;
            let identity = manager.import(config, label, backend, input, passphrase.as_ref())?;
            if let Err(error) = config.save(paths) {
                config.identities.retain(|entry| entry.id != identity.id);
                let _ = manager.delete(&identity);
                return Err(error);
            }
            println!("identity: {}\npubkey:   {}", identity.id, identity.pubkey);
            Ok(())
        }
        IdentityCommand::Backup { id, output } => {
            if output.exists() {
                return Err(Error::Config(format!(
                    "backup output {} already exists; choose a new path",
                    output.display()
                )));
            }
            let identity = find_identity(config, id)?.clone();
            let identity_passphrase =
                identity_passphrase(&identity, "Identity passphrase: ", false)?;
            let keys = manager.unlock(&identity, identity_passphrase.as_ref())?;
            let backup_passphrase = read_passphrase("New backup passphrase: ", true)?;
            let encoded = backup::create_backup(&keys, backup_passphrase.expose_secret())?;
            backup::write_backup_file(&output, &encoded)?;
            println!("backup:  {}\npubkey: {}", output.display(), identity.pubkey);
            Ok(())
        }
        IdentityCommand::ImportBackup {
            label,
            input,
            backend,
        } => {
            let encoded = backup::read_backup_file(&input)?;
            let backup_passphrase = read_passphrase("Backup passphrase: ", false)?;
            let keys = backup::decrypt_backup(&encoded, backup_passphrase.expose_secret())?;
            let backend = KeyBackend::from(backend);
            let storage_passphrase = matches!(backend, KeyBackend::EncryptedFile)
                .then(|| read_passphrase("New identity passphrase: ", true))
                .transpose()?;
            let identity =
                manager.import_keys(config, label, backend, keys, storage_passphrase.as_ref())?;
            save_new_identity(&manager, config, paths, &identity)?;
            println!("identity: {}\npubkey:   {}", identity.id, identity.pubkey);
            Ok(())
        }
        IdentityCommand::Restore { id } => {
            let identity = find_identity(config, id)?.clone();
            let input = Zeroizing::new(
                rpassword::prompt_password("Nostr nsec or secret hex: ")
                    .map_err(|error| Error::io("controlling terminal", error))?,
            );
            let storage_passphrase =
                identity_passphrase(&identity, "New identity passphrase: ", true)?;
            manager.restore(&identity, input, storage_passphrase.as_ref())?;
            println!(
                "restored identity: {}\npubkey:            {}",
                id, identity.pubkey
            );
            Ok(())
        }
        IdentityCommand::RestoreBackup { id, input } => {
            let identity = find_identity(config, id)?.clone();
            let encoded = backup::read_backup_file(&input)?;
            let backup_passphrase = read_passphrase("Backup passphrase: ", false)?;
            let keys = backup::decrypt_backup(&encoded, backup_passphrase.expose_secret())?;
            let storage_passphrase =
                identity_passphrase(&identity, "New identity passphrase: ", true)?;
            manager.restore_keys(&identity, keys, storage_passphrase.as_ref())?;
            println!(
                "restored identity: {}\npubkey:            {}",
                id, identity.pubkey
            );
            Ok(())
        }
        IdentityCommand::Verify { id } => {
            let identity = find_identity(config, id)?.clone();
            let passphrase = identity_passphrase(&identity, "Identity passphrase: ", false)?;
            let keys = manager.unlock(&identity, passphrase.as_ref())?;
            println!(
                "identity available: {}\npubkey:            {}",
                identity.id,
                keys.public_key().to_hex()
            );
            Ok(())
        }
        IdentityCommand::List => {
            for identity in &config.identities {
                println!(
                    "{}\t{}\t{}\t{:?}",
                    identity.id, identity.label, identity.pubkey, identity.backend
                );
            }
            Ok(())
        }
        IdentityCommand::Remove { id, yes } => {
            if !yes {
                return Err(Error::Config("identity removal requires --yes".into()));
            }
            if config
                .communities
                .iter()
                .any(|community| community.identity_id == id)
            {
                return Err(Error::Config(
                    "remove communities using this identity first".into(),
                ));
            }
            let index = config
                .identities
                .iter()
                .position(|identity| identity.id == id)
                .ok_or_else(|| Error::Config(format!("identity {id} does not exist")))?;
            let identity = config.identities.remove(index);
            if let Err(error) = config.save(paths) {
                config.identities.insert(index, identity);
                return Err(error);
            }
            if let Err(error) = manager.delete(&identity) {
                config.identities.insert(index, identity);
                let _ = config.save(paths);
                return Err(error);
            }
            Ok(())
        }
    }
}

async fn community_command(
    command: CommunityCommand,
    paths: &Paths,
    config: &mut Config,
) -> Result<()> {
    match command {
        CommunityCommand::Add {
            label,
            relay_url,
            identity_id,
            allow_insecure_localhost,
        } => {
            let endpoint = bzz::config::validate_relay_url(&relay_url, allow_insecure_localhost)?;
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|error| Error::Network(error.to_string()))?;
            let response = client
                .get(endpoint.http_base)
                .header("Accept", "application/nostr+json")
                .send()
                .await
                .map_err(|error| Error::Network(format!("NIP-11 probe failed: {error}")))?;
            if !response.status().is_success() {
                return Err(Error::Network(format!(
                    "NIP-11 probe returned {}",
                    response.status()
                )));
            }
            let info: serde_json::Value = response
                .json()
                .await
                .map_err(|error| Error::Protocol(format!("invalid NIP-11 document: {error}")))?;
            let relay_pubkey = bzz::protocol::http::relay_signing_pubkey(&info)
                .ok_or_else(|| Error::Protocol("NIP-11 document has no relay signing key".into()))?
                .to_owned();
            let supported = info
                .get("supported_nips")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for required in [29_u64, 42_u64, 98_u64] {
                if !supported
                    .iter()
                    .any(|value| value.as_u64() == Some(required))
                {
                    eprintln!(
                        "warning: relay does not advertise NIP-{required}; compatibility may be degraded"
                    );
                }
            }
            let id =
                config.add_community(label, relay_url, identity_id, allow_insecure_localhost)?;
            config.save(paths)?;
            let mut store = Store::open(paths.database_file())?;
            store.sync_config(config)?;
            store.pin_relay_pubkey(id, &relay_pubkey)?;
            println!("community: {id}");
            Ok(())
        }
        CommunityCommand::List => {
            for community in &config.communities {
                let default = if config.default_community == Some(community.id) {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{default} {}\t{}\t{}",
                    community.id, community.label, community.relay_url
                );
            }
            Ok(())
        }
        CommunityCommand::Remove { id, purge, yes } => {
            if !yes {
                return Err(Error::Config("community removal requires --yes".into()));
            }
            if !config.remove_community(id) {
                return Err(Error::Config(format!("community {id} does not exist")));
            }
            config.save(paths)?;
            if purge {
                if paths.database_file().exists() {
                    let store = Store::open(paths.database_file())?;
                    store.purge_community(id)?;
                }
                let media = paths.media_cache_dir().join(id.to_string());
                if media.exists() {
                    std::fs::remove_dir_all(&media).map_err(|error| Error::io(&media, error))?;
                }
            }
            Ok(())
        }
        CommunityCommand::Default { id } => {
            if !config
                .communities
                .iter()
                .any(|community| community.id == id)
            {
                return Err(Error::Config(format!("community {id} does not exist")));
            }
            config.default_community = Some(id);
            config.save(paths)
        }
    }
}

fn cache_command(command: CacheCommand, paths: &Paths, config: &mut Config) -> Result<()> {
    match command {
        CacheCommand::Purge {
            community,
            all,
            yes,
        } => {
            if !yes {
                return Err(Error::Config("cache purge requires --yes".into()));
            }
            if all {
                let path = paths.database_file();
                for candidate in [
                    path.clone(),
                    path.with_extension("db-wal"),
                    path.with_extension("db-shm"),
                ] {
                    if candidate.exists() {
                        std::fs::remove_file(&candidate)
                            .map_err(|error| Error::io(&candidate, error))?;
                    }
                }
                let media = paths.media_cache_dir();
                if media.exists() {
                    std::fs::remove_dir_all(&media).map_err(|error| Error::io(&media, error))?;
                    std::fs::create_dir_all(&media).map_err(|error| Error::io(&media, error))?;
                    bzz::paths::set_private_permissions(&media)?;
                }
            } else if let Some(id) = community {
                let store = Store::open(paths.database_file())?;
                store.purge_community(id)?;
                let media = paths.media_cache_dir().join(id.to_string());
                if media.exists() {
                    std::fs::remove_dir_all(&media).map_err(|error| Error::io(&media, error))?;
                }
                config.communities.retain(|entry| entry.id != id);
                if config.default_community == Some(id) {
                    config.default_community = config.communities.first().map(|entry| entry.id);
                }
                config.save(paths)?;
            } else {
                return Err(Error::Config("choose --all or --community <id>".into()));
            }
            Ok(())
        }
    }
}

fn media_command(command: MediaCommand, paths: &Paths, config: &Config) -> Result<()> {
    match command {
        MediaCommand::Status => {
            let used = directory_size(&paths.media_cache_dir())?;
            println!(
                "enabled:       {}\nprotocol:      {:?}\nautoload:      {:?}\ninline rows:   {}\ncache used:    {} bytes\ncache limit:   {} bytes\ndownload jobs: {}\ndecode jobs:   {}",
                config.media.enabled,
                config.media.protocol,
                config.media.autoload,
                config.media.max_inline_rows,
                used,
                config.media.disk_cache_bytes,
                config.media.download_concurrency,
                config.media.decode_concurrency,
            );
            Ok(())
        }
        MediaCommand::Prune => {
            let (removed, entries) =
                prune_media_cache(&paths.media_cache_dir(), config.media.disk_cache_bytes)?;
            if !entries.is_empty() && paths.database_file().exists() {
                let mut store = Store::open(paths.database_file())?;
                store.delete_media_cache_entries(&entries)?;
            }
            println!("removed {removed} cached media file(s)");
            Ok(())
        }
        MediaCommand::Clear {
            community,
            all,
            yes,
        } => {
            if !yes {
                return Err(Error::Config("media cache clear requires --yes".into()));
            }
            let path = if all {
                paths.media_cache_dir()
            } else if let Some(id) = community {
                paths.media_cache_dir().join(id.to_string())
            } else {
                return Err(Error::Config("choose --all or --community <id>".into()));
            };
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|error| Error::io(&path, error))?;
            }
            std::fs::create_dir_all(&path).map_err(|error| Error::io(&path, error))?;
            bzz::paths::set_private_permissions(&path)?;
            if paths.database_file().exists() {
                let store = Store::open(paths.database_file())?;
                store.clear_media_cache_entries(if all { None } else { community })?;
            }
            Ok(())
        }
    }
}

fn prune_media_cache(path: &std::path::Path, limit: u64) -> Result<(usize, Vec<(Uuid, String)>)> {
    if !path.exists() {
        return Ok((0, Vec::new()));
    }
    let mut directories = vec![path.to_path_buf()];
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut visited = 0_usize;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|error| Error::io(&directory, error))? {
            let entry = entry.map_err(|error| Error::io(&directory, error))?;
            visited += 1;
            if visited > 10_000 {
                return Err(Error::Config(
                    "media cache contains too many entries to prune safely".into(),
                ));
            }
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| Error::io(&path, error))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if entry.file_name() != "staging" {
                    directories.push(path);
                }
            } else if file_type.is_file() {
                let metadata = entry.metadata().map_err(|error| Error::io(&path, error))?;
                total = total.saturating_add(metadata.len());
                files.push((
                    metadata
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                    metadata.len(),
                    path,
                ));
            }
        }
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut removed = 0;
    let mut removed_entries = Vec::new();
    for (_, size, file) in files {
        if total <= limit {
            break;
        }
        std::fs::remove_file(&file).map_err(|error| Error::io(&file, error))?;
        total = total.saturating_sub(size);
        removed += 1;
        if let Some(entry) = media_cache_identity(path, &file) {
            removed_entries.push(entry);
        }
    }
    Ok((removed, removed_entries))
}

fn media_cache_identity(root: &std::path::Path, path: &std::path::Path) -> Option<(Uuid, String)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let community = components.next()?.as_os_str().to_str()?.parse().ok()?;
    let filename = components.next()?.as_os_str().to_str()?;
    if components.next().is_some() {
        return None;
    }
    let hash = filename.split_once('.')?.0;
    (hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then(|| (community, hash.to_owned()))
}

fn directory_size(path: &std::path::Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut directories = vec![path.to_path_buf()];
    let mut total = 0_u64;
    let mut visited = 0_usize;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|error| Error::io(&directory, error))? {
            let entry = entry.map_err(|error| Error::io(&directory, error))?;
            visited += 1;
            if visited > 10_000 {
                return Err(Error::Config(
                    "media cache contains too many entries to measure safely".into(),
                ));
            }
            let entry_path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| Error::io(&entry_path, error))?;
            if file_type.is_dir() {
                directories.push(entry_path);
            } else if file_type.is_file() {
                let metadata = entry
                    .metadata()
                    .map_err(|error| Error::io(&entry_path, error))?;
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn theme_command(command: ThemeCommand, paths: &Paths, config: &mut Config) -> Result<()> {
    use bzz::ui::theme::{DEFAULT_THEME_ID, ThemeRegistry};

    match command {
        ThemeCommand::List => {
            let mut output = String::new();
            for entry in ThemeRegistry::entries() {
                output.push_str(&format!("{}\t{}\tbuilt-in\n", entry.id, entry.name));
            }
            write_stdout(output.as_bytes())
        }
        ThemeCommand::Show { name } => {
            let output = ThemeRegistry::export(&name)?;
            write_stdout(output.as_bytes())
        }
        ThemeCommand::Check => {
            let warnings = bzz::ui::theme::check(paths, configured_theme_names(config))?;
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            println!("theme configuration is valid");
            Ok(())
        }
        ThemeCommand::Use { name, community } => {
            let canonical = ThemeRegistry::canonical_id(&name)
                .ok_or_else(|| Error::Config(format!("unknown theme: {name}")))?;
            if let Some(id) = community {
                let entry = config
                    .communities
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| Error::Config(format!("community {id} does not exist")))?;
                entry.theme = Some(canonical.to_owned());
            } else {
                config.ui.theme = canonical.to_owned();
            }
            config.save(paths)?;
            println!("theme: {canonical}");
            Ok(())
        }
        ThemeCommand::Reset { community } => {
            if let Some(id) = community {
                let entry = config
                    .communities
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| Error::Config(format!("community {id} does not exist")))?;
                entry.theme = None;
                println!("theme: inherited from global selection");
            } else {
                config.ui.theme = DEFAULT_THEME_ID.into();
                println!("theme: {DEFAULT_THEME_ID}");
            }
            config.save(paths)
        }
        ThemeCommand::Export { name, output } => {
            bzz::ui::theme::export_to(&name, &output)?;
            println!("theme exported: {}", output.display());
            Ok(())
        }
        ThemeCommand::Path => {
            println!("{}", bzz::ui::theme::theme_path(paths).display());
            Ok(())
        }
    }
}

async fn agent_command(command: AgentCommand, paths: &Paths, config: &mut Config) -> Result<()> {
    match command {
        AgentCommand::Add { label, workdir } => {
            let id = config.add_local_agent(label, workdir)?;
            config.save(paths)?;
            println!("local agent: {id}");
            Ok(())
        }
        AgentCommand::List => {
            if config.local_agents.is_empty() {
                println!("no local assistants are configured");
            } else {
                for agent in &config.local_agents {
                    println!(
                        "{}\t{}\tcodex\t{}",
                        agent.id,
                        bzz::render::sanitize::single_line(&agent.label),
                        agent.workdir.as_ref().map_or_else(
                            || "isolated scratch".into(),
                            |path| bzz::render::sanitize::single_line(&path.display().to_string())
                        )
                    );
                }
            }
            Ok(())
        }
        AgentCommand::Remove { id, yes } => {
            if !yes {
                return Err(Error::Config(
                    "use --yes to remove a local assistant".into(),
                ));
            }
            if !config.remove_local_agent(id) {
                return Err(Error::Config(format!("local agent {id} does not exist")));
            }
            config.save(paths)?;
            println!("removed local agent: {id}");
            Ok(())
        }
        AgentCommand::Doctor => {
            let doctor = match CodexExecutable::resolve() {
                Some(executable) => executable.doctor().await,
                None => Doctor::Unavailable,
            };
            match doctor {
                Doctor::Ready => println!("codex: ready for draft-only read-only execution"),
                Doctor::Unavailable => println!("codex: unavailable"),
                Doctor::Unsupported => println!("codex: missing a required safe exec flag"),
            }
            Ok(())
        }
    }
}

fn configured_theme_names(config: &Config) -> Vec<String> {
    std::iter::once(config.ui.theme.clone())
        .chain(
            config
                .communities
                .iter()
                .filter_map(|community| community.theme.clone()),
        )
        .collect()
}

fn write_stdout(value: &[u8]) -> Result<()> {
    if let Err(error) = std::io::stdout().write_all(value)
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(Error::io("stdout", error));
    }
    Ok(())
}

fn find_identity(config: &Config, id: Uuid) -> Result<&bzz::config::IdentityConfig> {
    config
        .identities
        .iter()
        .find(|identity| identity.id == id)
        .ok_or_else(|| Error::Config(format!("identity {id} does not exist")))
}

fn identity_passphrase(
    identity: &bzz::config::IdentityConfig,
    prompt: &str,
    confirm: bool,
) -> Result<Option<secrecy::SecretString>> {
    matches!(identity.backend, KeyBackend::EncryptedFile)
        .then(|| read_passphrase(prompt, confirm))
        .transpose()
}

fn save_new_identity(
    manager: &IdentityManager<'_>,
    config: &mut Config,
    paths: &Paths,
    identity: &bzz::config::IdentityConfig,
) -> Result<()> {
    if let Err(error) = config.save(paths) {
        config.identities.retain(|entry| entry.id != identity.id);
        let _ = manager.delete(identity);
        return Err(error);
    }
    Ok(())
}
