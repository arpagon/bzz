#![forbid(unsafe_code)]

use std::io::Write as _;

use bzz::{
    Result,
    app::App,
    auth::{IdentityManager, read_passphrase},
    config::{Config, KeyBackend},
    error::Error,
    paths::Paths,
    store::{Store, writer::StoreHandle},
};
use clap::{CommandFactory as _, Parser, Subcommand, ValueEnum};
use nostr::ToBech32 as _;
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

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
        #[arg(long)]
        show_backup: bool,
    },
    /// Import an nsec or 64-character secret read from the controlling terminal.
    Import {
        #[arg(long)]
        label: String,
        #[arg(long, value_enum, default_value = "keychain")]
        backend: BackendArg,
    },
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
        Some(Command::Check) => {
            config.validate()?;
            let mut store = Store::open(paths.database_file())?;
            store.sync_config(&config)?;
            println!("configuration and database are valid");
            Ok(())
        }
        Some(Command::Paths) => {
            println!("config: {}", paths.config_file().display());
            println!("data:   {}", paths.database_file().display());
            println!("cache:  {}", paths.cache_dir.display());
            Ok(())
        }
        Some(Command::Completions { .. }) => Ok(()),
        None => {
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
        IdentityCommand::New {
            label,
            backend,
            show_backup,
        } => {
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
            if show_backup {
                confirm_backup()?;
                let keys = manager.unlock(&identity, passphrase.as_ref())?;
                let mut backup = Zeroizing::new(
                    keys.secret_key()
                        .to_bech32()
                        .map_err(|_| Error::Auth("could not encode backup".into()))?,
                );
                println!("nsec backup (shown once): {}", backup.as_str());
                std::io::stdout()
                    .flush()
                    .map_err(|error| Error::io("stdout", error))?;
                backup.zeroize();
            }
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
            if purge && paths.database_file().exists() {
                let store = Store::open(paths.database_file())?;
                store.purge_community(id)?;
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
            } else if let Some(id) = community {
                let store = Store::open(paths.database_file())?;
                store.purge_community(id)?;
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

fn confirm_backup() -> Result<()> {
    let confirmation = rpassword::prompt_password("Type SHOW to reveal the nsec backup: ")
        .map_err(|error| Error::io("controlling terminal", error))?;
    if confirmation != "SHOW" {
        return Err(Error::Auth("backup display cancelled".into()));
    }
    Ok(())
}
