//! `bzz agent …`: create, run, manage and move local agents from the command
//! line (plan v0.15.0, M2, M4 and M5). The work is in `bzz::agents::manage`; this file
//! asks the questions, reads the owner's passphrase and prints.

use std::{
    io::IsTerminal as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use bzz::{
    Result,
    agents::{
        capability, manage, moving, remote,
        runtime::{
            agent::Environment,
            install::{self, ModelCheck},
        },
    },
    auth::IdentityManager,
    config::{Config, Harness, HostedAgentConfig, WakeRule},
    error::Error,
    paths::Paths,
    store::{Store, writer::StoreHandle},
};
use clap::{Args, Subcommand};
use uuid::Uuid;

use crate::{confirm, find_identity, identity_passphrase, selected_community};

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Create a local agent: bzz holds its key and starts its process.
    New(Box<NewArgs>),
    /// List local agents and their state.
    List,
    /// Show one agent: state, model, folders, hosts, channels, last turns
    /// and its capability record.
    Show { name: String },
    /// Add an agent to a channel, as its owner. In a Solo chat it gets the
    /// bot role; elsewhere it is a plain member, as Buzz adds one.
    Add {
        name: String,
        /// The channel's name or id.
        channel: String,
    },
    /// Change an agent's settings and publish again what Buzz shows of it.
    Edit(Box<EditArgs>),
    /// Stop an agent's process; it answers nothing until it is resumed.
    Pause { name: String },
    /// Let a paused agent run again.
    Resume { name: String },
    /// Delete an agent: its key, its folder with memory and disk, its rows
    /// in bzz's database, and your record of it on the relay.
    Delete {
        name: String,
        #[arg(long)]
        yes: bool,
        /// Delete what is on this machine even when the relay cannot be
        /// reached to remove your record of the agent.
        #[arg(long)]
        force: bool,
    },
    /// Report Node, npm, the agent host's install and each agent's
    /// readiness, without installing or starting anything.
    Doctor {
        /// Also ask the installed host whether each agent's model can be
        /// used with Pi's own sign-in.
        #[arg(long)]
        models: bool,
    },
    /// Run this community's local agents without the TUI, until interrupted.
    Run {
        #[arg(long)]
        community: Option<Uuid>,
    },
    /// Move an agent to another bzz: stop it, and seal its key, its memory,
    /// its disk and its settings in one file with a passphrase. It is then
    /// marked moved away here, and this bzz no longer starts it.
    Move {
        name: String,
        /// The file to write. It must not exist.
        #[arg(long, value_name = "FILE")]
        to: PathBuf,
    },
    /// Receive an agent: from a file made by `bzz agent move`, or from a
    /// launch kit folder that came back. An agent this bzz had moved away
    /// is taken back.
    Receive {
        /// The sealed file, or the launch kit's folder.
        file: PathBuf,
    },
    /// Move an agent to a machine without bzz: write a folder from which
    /// Buzz's buzz-acp runs it. The folder holds the agent's key in the
    /// clear. The agent is then marked moved away here.
    LaunchKit {
        name: String,
        /// The folder to write. It must not exist.
        #[arg(long, value_name = "FOLDER")]
        to: PathBuf,
        /// Write the key in the clear without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Stop a copy of an agent that runs elsewhere: send Buzz's own stop
    /// command (`!shutdown`) as its owner, and say whether it then left.
    Shutdown {
        name: String,
        /// The channel to send it in (name or id). Default: a direct
        /// message or Solo chat you share with it, else one of its channels.
        #[arg(long)]
        channel: Option<String>,
        /// How many seconds to wait for it to leave.
        #[arg(long, default_value_t = 30)]
        wait: u64,
    },
}

/// The questions of `agent new`, each also a flag. Without `--yes`, on a
/// terminal, the ones not given are asked.
#[derive(Args, Debug, Default)]
pub struct NewArgs {
    /// The agent's name, as Buzz will show it.
    pub name: Option<String>,
    /// The community it belongs to. Default: the default one, or the only one.
    #[arg(long)]
    pub community: Option<Uuid>,
    /// pi-durable (the others are not available in this version yet).
    #[arg(long)]
    pub harness: Option<String>,
    /// provider/model as Pi names them. Default: Pi's own default model.
    #[arg(long)]
    pub model: Option<String>,
    /// The model's thinking level, as Pi names it; needs --model.
    #[arg(long)]
    pub thinking: Option<String>,
    /// What the agent is told about itself and its work.
    #[arg(long, conflicts_with = "instructions_file")]
    pub instructions: Option<String>,
    /// Read the instructions from a file.
    #[arg(long)]
    pub instructions_file: Option<PathBuf>,
    /// Who may wake it: owner-only (default), allowlist or anyone.
    #[arg(long)]
    pub wake: Option<String>,
    /// A person who may wake it (npub or hex); repeat for more. Implies
    /// `--wake allowlist`.
    #[arg(long = "allow", value_name = "PUBKEY")]
    pub allow: Vec<String>,
    /// A folder of this machine it may read: name=/path, or name=/path:rw
    /// to let it write. Repeat for more.
    #[arg(long = "folder", value_name = "NAME=PATH[:rw]")]
    pub folders: Vec<String>,
    /// A host it may reach (host or host:port). Repeat for more.
    #[arg(long = "allow-host", value_name = "HOST")]
    pub hosts: Vec<String>,
    /// An HTTPS address of its picture.
    #[arg(long)]
    pub picture: Option<String>,
    /// Ask nothing: use the defaults for what was not given, and sign the
    /// owner authorization without asking.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug, Default)]
pub struct EditArgs {
    /// The agent to change.
    pub agent: String,
    /// A new name.
    #[arg(long)]
    pub name: Option<String>,
    /// provider/model as Pi names them; checked before it is saved.
    #[arg(long, conflicts_with = "default_model")]
    pub model: Option<String>,
    /// The model's thinking level, as Pi names it; needs --model.
    #[arg(long)]
    pub thinking: Option<String>,
    /// Use the harness's own default model again.
    #[arg(long)]
    pub default_model: bool,
    /// What the agent is told about itself and its work.
    #[arg(long, conflicts_with = "instructions_file")]
    pub instructions: Option<String>,
    /// Read the instructions from a file.
    #[arg(long)]
    pub instructions_file: Option<PathBuf>,
    /// Who may wake it: owner-only, allowlist or anyone.
    #[arg(long)]
    pub wake: Option<String>,
    /// The people who may wake it, replacing the list. Implies
    /// `--wake allowlist`.
    #[arg(long = "allow", value_name = "PUBKEY")]
    pub allow: Vec<String>,
    /// The folders it may reach, replacing the list.
    #[arg(
        long = "folder",
        value_name = "NAME=PATH[:rw]",
        conflicts_with = "no_folders"
    )]
    pub folders: Vec<String>,
    /// Take every granted folder away.
    #[arg(long)]
    pub no_folders: bool,
    /// The hosts it may reach, replacing the list.
    #[arg(long = "allow-host", value_name = "HOST", conflicts_with = "no_hosts")]
    pub hosts: Vec<String>,
    /// Let it reach no host at all.
    #[arg(long)]
    pub no_hosts: bool,
    /// An HTTPS address of its picture.
    #[arg(long, conflicts_with = "no_picture")]
    pub picture: Option<String>,
    /// Take its picture away.
    #[arg(long)]
    pub no_picture: bool,
    /// Publish the agent's profile and your record of it again, whatever
    /// changed.
    #[arg(long)]
    pub republish: bool,
    /// Load your Pi packages for it again (the default).
    #[arg(long, conflicts_with = "no_packages")]
    pub packages: bool,
    /// Load no Pi package for it: none of their code runs in its host. The
    /// choice moves with the agent.
    #[arg(long)]
    pub no_packages: bool,
}

fn environment() -> Environment {
    Arc::new(|name| std::env::var(name).ok())
}

fn pi_dir() -> Option<PathBuf> {
    install::pi_dir(|name| std::env::var(name).ok())
}

fn open_store(paths: &Paths, config: &Config) -> Result<StoreHandle> {
    let mut store = Store::open(paths.database_file())?;
    store.sync_config(config)?;
    StoreHandle::spawn(store)
}

/// One answer from the terminal; an empty one is `default`.
fn ask(question: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        eprint!("{question}: ");
    } else {
        eprint!("{question} [{default}]: ");
    }
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| Error::io("stdin", error))?;
    let answer = answer.trim();
    Ok(if answer.is_empty() { default } else { answer }.to_owned())
}

fn instructions_from(text: Option<String>, file: Option<&PathBuf>) -> Result<Option<String>> {
    match (text, file) {
        (Some(text), _) => Ok(Some(text)),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|error| Error::io(path, error)),
        (None, None) => Ok(None),
    }
}

/// The agent's name: the argument, or the first question.
fn name_answer(args: &NewArgs, interactive: bool) -> Result<String> {
    match (&args.name, interactive) {
        (Some(name), _) => Ok(name.clone()),
        (None, true) => ask("Name", ""),
        (None, false) => Err(Error::Config(
            "give the agent's name: bzz agent new <name>".into(),
        )),
    }
}

/// The community the agent belongs to: the flag, the only one, or the
/// second question.
fn community_answer(
    config: &Config,
    args: &NewArgs,
    interactive: bool,
) -> Result<bzz::config::CommunityConfig> {
    if args.community.is_some() || !interactive || config.communities.len() < 2 {
        return selected_community(config, args.community).cloned();
    }
    eprintln!("Community:");
    for (index, community) in config.communities.iter().enumerate() {
        eprintln!("  {}. {}", index + 1, community.label);
    }
    let default = config
        .default_community
        .and_then(|id| config.communities.iter().position(|entry| entry.id == id))
        .map_or_else(String::new, |index| (index + 1).to_string());
    let answer = ask("Community (number or name)", &default)?;
    answer
        .parse::<usize>()
        .ok()
        .and_then(|number| config.communities.get(number.checked_sub(1)?))
        .or_else(|| {
            config.communities.iter().find(|entry| {
                entry.label.eq_ignore_ascii_case(&answer) || entry.id.to_string() == answer
            })
        })
        .cloned()
        .ok_or_else(|| Error::Config(format!("no community is called {answer:?}")))
}

/// The rest of the answers of `agent new`: from the flags, and from the
/// terminal for what no flag gave. The questions and how an answer is read
/// are `manage`'s, shared with the form of `:agent new`.
fn answers(args: &NewArgs, interactive: bool, name: &str) -> Result<manage::NewAgent> {
    let question = |index: usize| {
        let (label, hint) = manage::QUESTIONS[index];
        format!("{label} ({hint})")
    };
    let asked = |given: &Option<String>, index: usize, default: &str| match (given, interactive) {
        (Some(value), _) => Ok(value.clone()),
        (None, true) => ask(&question(index), default),
        (None, false) => Ok(default.to_owned()),
    };
    let listed = |given: &[String], index: usize| -> Result<Vec<String>> {
        if !given.is_empty() || !interactive {
            return Ok(given.to_vec());
        }
        Ok(manage::split_list(&ask(&question(index), "")?)
            .into_iter()
            .map(str::to_owned)
            .collect())
    };
    let harness = asked(&args.harness, 1, "pi-durable")?;
    let model = asked(&args.model, 2, "")?;
    let instructions =
        match instructions_from(args.instructions.clone(), args.instructions_file.as_ref())? {
            Some(text) => text,
            None => asked(&None, 3, "")?,
        };
    // People named by a flag make the rule an allowlist without asking.
    let wake = match (&args.wake, args.allow.is_empty()) {
        (Some(wake), _) => wake.clone(),
        (None, false) => "allowlist".into(),
        (None, true) => asked(&None, 4, "owner-only")?,
    };
    let allow = if manage::parse_wake(&wake)? == WakeRule::Allowlist {
        listed(&args.allow, 5)?
    } else {
        args.allow.clone()
    };
    // A folder's path may hold a space: folders are split at commas only.
    let folders = if !args.folders.is_empty() || !interactive {
        args.folders.clone()
    } else {
        manage::split_commas(&ask(&question(6), "")?)
    };
    let hosts = listed(&args.hosts, 7)?;
    let picture = asked(&args.picture, 8, "")?;
    manage::Answers {
        name: name.to_owned(),
        harness,
        model,
        thinking: args.thinking.clone(),
        instructions,
        wake,
        allow,
        folders,
        hosts,
        picture,
    }
    .read()
}

async fn new_agent(paths: &Paths, config: &mut Config, args: NewArgs) -> Result<()> {
    let interactive = !args.yes && std::io::stdin().is_terminal();
    let name = name_answer(&args, interactive)?;
    let community = community_answer(config, &args, interactive)?;
    let identity = find_identity(config, community.identity_id)?.clone();
    let new = answers(&args, interactive, &name)?;
    if manage::name_taken(config, &new.name, None) {
        return Err(Error::Config(format!("an agent named {} exists", new.name)));
    }
    // What can fail before anything exists.
    let preflight = manage::preflight(
        &paths.data_dir,
        &new,
        pi_dir().as_deref(),
        &environment(),
        &|line| eprintln!("{line}"),
    )
    .await?;
    for note in &preflight.notes {
        println!("{note}");
    }
    if !args.yes
        && !confirm(&format!(
            "Sign an owner authorization (NIP-OA) letting agent \"{}\" act for {}, and publish its profile and your record of it in {}?",
            new.name, identity.label, community.label
        ))?
    {
        return Err(Error::Config("agent creation cancelled".into()));
    }
    let passphrase = identity_passphrase(&identity, "Owner identity passphrase: ", false)?;
    let owner = IdentityManager::new(paths).signer(&identity, passphrase.as_ref())?;
    let relay = match manage::OwnerRelay::connect(paths, &community, &identity, owner.clone()).await
    {
        Ok(relay) => relay,
        Err(error) => {
            owner.lock().await;
            return Err(error);
        }
    };
    let federated = community
        .identity_adapter
        .as_deref()
        .and_then(|adapter| url::Url::parse(adapter).ok());
    let created = manage::create(
        &manage::OwnerContext {
            paths,
            owner: &owner,
            owner_identity: identity.id,
            community: community.id,
            federated: federated.as_ref(),
        },
        config,
        &new,
        &relay,
    )
    .await;
    relay.close().await;
    owner.lock().await;
    let created = created.map_err(|failure| Error::Config(failure.to_string()))?;
    println!(
        "agent {} ({}) created in {}",
        created.agent.label, created.agent.pubkey, community.label
    );
    for line in capability::record(new.harness).lines() {
        println!("{line}");
    }
    println!(
        "next: add it to a channel with `bzz agent add {} <channel>`. It answers while bzz is open, or under `bzz agent run`.",
        shell_word(&created.agent.label)
    );
    Ok(())
}

fn shell_word(text: &str) -> String {
    if text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        text.to_owned()
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

fn list(paths: &Paths, config: &Config) {
    let local = config
        .hosted_agents
        .iter()
        .filter(|agent| agent.local.is_some())
        .collect::<Vec<_>>();
    if local.is_empty() {
        println!("no local agents; create one with `bzz agent new <name>`");
    }
    for agent in local {
        let Some(settings) = &agent.local else {
            continue;
        };
        println!(
            "{}\t{}{}\t{}\t{}\t{}",
            agent.label,
            manage::state_of(agent),
            manage::running_pid(&paths.data_dir, agent.id)
                .map(|pid| format!(" (running, process {pid})"))
                .unwrap_or_default(),
            settings.harness.as_str(),
            settings.model.as_ref().map_or_else(
                || "default model".to_owned(),
                |model| format!("{}/{}", model.provider, model.model_id)
            ),
            agent.pubkey,
        );
    }
}

/// The owner's signer and connection for the agent's community.
async fn owner_relay(
    paths: &Paths,
    config: &Config,
    agent: &HostedAgentConfig,
) -> Result<(bzz::auth::signer::SignerHandle, manage::OwnerRelay)> {
    let community = selected_community(config, Some(agent.community))?.clone();
    let identity = find_identity(config, agent.owner_identity)?.clone();
    let passphrase = identity_passphrase(&identity, "Owner identity passphrase: ", false)?;
    let owner = IdentityManager::new(paths).signer(&identity, passphrase.as_ref())?;
    match manage::OwnerRelay::connect(paths, &community, &identity, owner.clone()).await {
        Ok(relay) => Ok((owner, relay)),
        Err(error) => {
            owner.lock().await;
            Err(error)
        }
    }
}

fn context<'a>(
    paths: &'a Paths,
    owner: &'a bzz::auth::signer::SignerHandle,
    agent: &HostedAgentConfig,
    federated: Option<&'a url::Url>,
) -> manage::OwnerContext<'a> {
    manage::OwnerContext {
        paths,
        owner,
        owner_identity: agent.owner_identity,
        community: agent.community,
        federated,
    }
}

fn adapter_of(config: &Config, community: Uuid) -> Option<url::Url> {
    config
        .communities
        .iter()
        .find(|entry| entry.id == community)
        .and_then(|entry| entry.identity_adapter.as_deref())
        .and_then(|adapter| url::Url::parse(adapter).ok())
}

async fn edit(paths: &Paths, config: &mut Config, args: EditArgs) -> Result<()> {
    let id = manage::find(config, &args.agent)?.id;
    let wake = match (&args.wake, args.allow.is_empty()) {
        (Some(wake), _) => Some(manage::parse_wake(wake)?),
        (None, false) => Some(WakeRule::Allowlist),
        (None, true) => None,
    };
    let list = |values: &[String], parse: fn(&str) -> Result<String>| {
        let mut parsed = values
            .iter()
            .map(|value| parse(value))
            .collect::<Result<Vec<_>>>()?;
        parsed.sort();
        parsed.dedup();
        Ok::<_, Error>(parsed)
    };
    let change = manage::Edit {
        name: args.name.as_ref().map(|name| name.trim().to_owned()),
        model: match (&args.model, args.default_model) {
            (Some(model), _) => Some(manage::parse_model(model, args.thinking.as_deref())?),
            (None, true) => Some(None),
            (None, false) => None,
        },
        instructions: instructions_from(
            args.instructions.clone(),
            args.instructions_file.as_ref(),
        )?,
        wake,
        wake_allowlist: (!args.allow.is_empty())
            .then(|| list(&args.allow, manage::parse_pubkey))
            .transpose()?,
        folders: match (args.folders.is_empty(), args.no_folders) {
            (false, _) => Some(
                args.folders
                    .iter()
                    .map(|folder| manage::parse_folder(folder))
                    .collect::<Result<Vec<_>>>()?,
            ),
            (true, true) => Some(Vec::new()),
            (true, false) => None,
        },
        network: match (args.hosts.is_empty(), args.no_hosts) {
            (false, _) => Some(list(&args.hosts, manage::parse_host)?),
            (true, true) => Some(Vec::new()),
            (true, false) => None,
        },
        picture: match (&args.picture, args.no_picture) {
            (Some(picture), _) => Some(manage::parse_picture(picture)?),
            (None, true) => Some(None),
            (None, false) => None,
        },
        packages: match (args.packages, args.no_packages) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        },
    };
    if change == manage::Edit::default() && !args.republish {
        return Err(Error::Config(
            "nothing to change: see `bzz agent edit --help`".into(),
        ));
    }
    let before = config.clone();
    let mut republish = manage::apply_edit(config, id, &change)?;
    if args.republish {
        republish = manage::Republish::BOTH;
    }
    let agent = config
        .hosted_agents
        .iter()
        .find(|agent| agent.id == id)
        .cloned()
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    // A model that was named is checked before it is saved.
    if let (Some(Some(_)), Some(settings)) = (&change.model, &agent.local)
        && settings.harness == Harness::PiDurable
    {
        let mut probe = manage::NewAgent::named(&agent.label);
        probe.model = settings.model.clone();
        if let Err(error) = manage::preflight(
            &paths.data_dir,
            &probe,
            pi_dir().as_deref(),
            &environment(),
            &|line| eprintln!("{line}"),
        )
        .await
        {
            *config = before;
            return Err(error);
        }
    }
    config.save(paths)?;
    println!("{} saved", agent.label);
    if republish.any() {
        let (owner, relay) = owner_relay(paths, config, &agent).await?;
        let federated = adapter_of(config, agent.community);
        let published = manage::republish(
            &context(paths, &owner, &agent, federated.as_ref()),
            &agent,
            republish,
            &relay,
        )
        .await;
        relay.close().await;
        owner.lock().await;
        if let Err(error) = published {
            return Err(Error::Config(format!(
                "the settings are saved here, but the relay still shows the agent as before: {error}. Run `bzz agent edit {} --republish` to publish them",
                shell_word(&agent.label)
            )));
        }
        println!(
            "published again: {}",
            match (republish.profile, republish.record) {
                (true, true) => "its profile and your record of it",
                (true, false) => "its profile",
                _ => "your record of it",
            }
        );
    }
    if manage::state_of(&agent) == "moved away" {
        println!(
            "{} was moved away: this changes nothing where it runs. When it is taken back, its settings are the ones that arrive with it; the folders granted here stay as you set them",
            agent.label
        );
    } else {
        println!("a bzz that is running uses the new settings at its next start");
    }
    Ok(())
}

async fn delete(
    paths: &Paths,
    config: &mut Config,
    name: &str,
    yes: bool,
    force: bool,
) -> Result<()> {
    let agent = manage::find(config, name)?.clone();
    if let Some(pid) = manage::running_pid(&paths.data_dir, agent.id) {
        return Err(Error::Config(format!(
            "{} is running (process {pid}): close bzz or pause the agent first",
            agent.label
        )));
    }
    let moved_away = manage::state_of(&agent) == "moved away";
    if moved_away {
        eprintln!(
            "{} was moved away: a copy of it may be running elsewhere. Deleting it here also removes your record of it from the relay and its place in its channels, so that copy stops being listed and heard. The copy keeps its key",
            agent.label
        );
    }
    if !yes
        && !confirm(&format!(
            "Delete agent {} with its key, its memory and its disk? This cannot be undone.",
            agent.label
        ))?
    {
        return Err(Error::Config("agent deletion cancelled".into()));
    }
    let store = open_store(paths, config)?;
    let mut removal = manage::Removal::default();
    match owner_relay(paths, config, &agent).await {
        Ok((owner, relay)) => {
            // NIP-FI communities: the agent's grants with the adapter go
            // first, as `broker delete` does.
            if let Some(adapter) = adapter_of(config, agent.community)
                && let Err(error) = bzz::auth::federated::revoke_agent(
                    &adapter,
                    &paths.data_dir,
                    agent.owner_identity,
                    &owner,
                    &agent.pubkey,
                )
                .await
            {
                relay.close().await;
                owner.lock().await;
                return Err(Error::Auth(format!(
                    "agent {} kept: revoking it with the identity adapter failed: {error}",
                    agent.label
                )));
            }
            removal = manage::remove_from_relay(&relay, &agent).await;
            relay.close().await;
            owner.lock().await;
        }
        Err(error) if force => removal
            .left
            .push(format!("everything on the relay ({error})")),
        Err(error) => {
            return Err(Error::Config(format!(
                "agent {} kept: the relay could not be reached to remove your record of it ({error}). Try again, or pass --force to delete what is on this machine anyway",
                agent.label
            )));
        }
    }
    if !force && removal.left.iter().any(|left| left == manage::RECORD_LEFT) {
        return Err(Error::Config(format!(
            "agent {} kept: the relay did not remove your record of it, so Buzz would still list an agent nobody runs. Try again, or pass --force",
            agent.label
        )));
    }
    let local = manage::remove_local(paths, config, &store, agent.id).await?;
    removal.removed.extend(local.removed);
    removal.left.extend(local.left);
    println!("agent {} deleted", agent.label);
    for removed in &removal.removed {
        println!("  removed: {removed}");
    }
    for left in &removal.left {
        println!("  could not remove: {left}");
    }
    println!("  its messages and its profile stay on the relay, as with any member who leaves");
    Ok(())
}

async fn doctor(paths: &Paths, config: &Config, models: bool) -> Result<()> {
    let lookup = |name: &str| std::env::var(name).ok();
    let report = install::report(&paths.data_dir, lookup).await;
    let mut problems = 0;
    match &report.node {
        Ok(node) => println!(
            "Node.js:  {} at {} (needs {}.{} or newer: ok)",
            node.version_text(),
            node.program.display(),
            install::MIN_NODE.0,
            install::MIN_NODE.1
        ),
        Err(problem) => {
            problems += 1;
            println!("Node.js:  not ready: {problem}");
        }
    }
    match &report.npm {
        Some(Ok((npm, version))) => println!(
            "npm:      {} at {}",
            version.as_deref().unwrap_or("unknown version"),
            npm.display()
        ),
        Some(Err(problem)) => {
            // Only the install needs npm.
            if !report.installed {
                problems += 1;
            }
            println!("npm:      not ready: {problem}");
        }
        None => println!("npm:      not looked for without Node.js"),
    }
    println!(
        "host:     {} {}",
        if report.installed {
            "installed in"
        } else {
            "not installed yet; `bzz agent new` or an agent's first start installs it in"
        },
        report.host_dir.display()
    );
    match &report.pi_dir {
        Some(folder) => println!(
            "Pi:       {} (models and sign-ins are Pi's own; bzz stores no provider key)",
            folder.display()
        ),
        None => println!("Pi:       its folder could not be found"),
    }
    let local = config
        .hosted_agents
        .iter()
        .filter(|agent| agent.local.is_some())
        .collect::<Vec<_>>();
    if local.is_empty() {
        println!("agents:   none; create one with `bzz agent new <name>`");
    }
    for agent in local {
        let Some(settings) = &agent.local else {
            continue;
        };
        let mut missing = Vec::new();
        if !bzz::broker::custody::has_key(&paths.keys_dir(), agent) {
            missing.push("its sealed key is missing".to_owned());
        }
        if !capability::record(settings.harness).available {
            missing.push("its harness is not available in this version yet".to_owned());
        } else if settings.command.is_empty() && !report.ready() {
            missing.push("Node.js or npm is not ready (above)".to_owned());
        }
        if models
            && settings.harness == Harness::PiDurable
            && let (Ok(node), true, Some(pi)) = (&report.node, report.installed, &report.pi_dir)
        {
            match install::check_model(node, &report.host_dir, pi, settings.model.as_ref(), lookup)
                .await
            {
                ModelCheck::Available { .. } => {}
                ModelCheck::Unavailable { kind, message } => {
                    missing.push(format!("its model cannot be used ({kind}): {message}"));
                }
                ModelCheck::Unknown(why) => {
                    missing.push(format!("its model could not be checked: {why}"));
                }
            }
        }
        problems += missing.len();
        println!(
            "agent:    {} · {} · {} · {}",
            agent.label,
            settings.harness.as_str(),
            manage::state_of(agent),
            if missing.is_empty() {
                "ready".to_owned()
            } else {
                format!("not ready: {}", missing.join("; "))
            }
        );
    }
    if problems == 0 {
        Ok(())
    } else {
        Err(Error::Config(format!("{problems} thing(s) are not ready")))
    }
}

/// What does not travel with an agent, said after a move.
fn stays_here(agent: &HostedAgentConfig) -> String {
    let folders = agent
        .local
        .as_ref()
        .map(|local| {
            local
                .folders
                .iter()
                .map(|folder| folder.name.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    format!(
        "  not moved: {}git credentials. They belong to this machine",
        if folders.is_empty() {
            "granted folders (it had none) and ".to_owned()
        } else {
            format!("its granted folder(s) ({}) and ", folders.join(", "))
        }
    )
}

/// The owner's signer for an agent, with the passphrase asked for when the
/// identity needs one.
fn unlock_owner(
    paths: &Paths,
    config: &Config,
    identity: Uuid,
) -> Result<bzz::auth::signer::SignerHandle> {
    let identity = find_identity(config, identity)?.clone();
    let passphrase = identity_passphrase(&identity, "Owner identity passphrase: ", false)?;
    IdentityManager::new(paths).signer(&identity, passphrase.as_ref())
}

async fn move_agent(paths: &Paths, config: &mut Config, name: &str, to: &Path) -> Result<()> {
    let agent = manage::find(config, name)?.clone();
    // What can be refused before any passphrase is asked for.
    moving::check_movable(config, agent.id)?;
    let owner = unlock_owner(paths, config, agent.owner_identity)?;
    let passphrase = match bzz::auth::read_passphrase_from(
        bzz::auth::MOVE_PASSPHRASE_FD,
        "Passphrase for the file (at least 12 characters; it is all that protects the agent's key in it): ",
        true,
    ) {
        Ok(passphrase) => passphrase,
        Err(error) => {
            owner.lock().await;
            return Err(error);
        }
    };
    eprintln!(
        "stopping {} and packing it; nothing changes here until the file is whole…",
        agent.label
    );
    let moved = match open_store(paths, config) {
        Ok(store) => {
            let mover = moving::Mover::new(paths, &store, &owner);
            moving::move_to_file(&mover, config, agent.id, &passphrase, to).await
        }
        Err(error) => Err(error),
    };
    owner.lock().await;
    let moved = moved?;
    println!(
        "agent {} moved into {} ({} bytes), sealed with your passphrase",
        moved.name,
        moved.file.display(),
        moved.bytes
    );
    println!(
        "  in it: its key, its memory, its disk, and its settings (name, model, instructions, who may wake it, the hosts it may reach, Pi packages)"
    );
    println!("{}", stays_here(&agent));
    println!(
        "{} is marked moved away here: this bzz no longer starts it. Its sealed key and its files stay here until you delete it",
        moved.name
    );
    println!(
        "next: copy the file to the other machine and run `bzz agent receive <file>` there, as the same owner in the same community. `bzz agent receive {}` here takes it back",
        shell_word(&moved.file.display().to_string())
    );
    Ok(())
}

async fn receive(paths: &Paths, config: &mut Config, source: &Path) -> Result<()> {
    let from_kit = source.is_dir();
    let arrival = if from_kit {
        moving::open_kit(source)?
    } else {
        let passphrase = bzz::auth::read_passphrase_from(
            bzz::auth::MOVE_PASSPHRASE_FD,
            "Passphrase of the file: ",
            false,
        )?;
        moving::open_file(source, &passphrase).await?
    };
    // Whose it is and where it goes, before the owner's identity is asked
    // for: an agent of another owner is refused here.
    let plan = moving::plan(config, &arrival.carried)?;
    let identity = find_identity(config, plan.identity)?.clone();
    let owner = unlock_owner(paths, config, plan.identity)?;
    let received = match open_store(paths, config) {
        Ok(store) => {
            moving::receive(
                paths,
                &store,
                &owner,
                config,
                arrival,
                &bzz::agents::runtime::settle::HostAccess::of_this_process(),
            )
            .await
        }
        Err(error) => Err(error),
    };
    owner.lock().await;
    let received = received?;
    let community = config
        .communities
        .iter()
        .find(|community| community.id == received.community)
        .map_or_else(
            || received.community.to_string(),
            |entry| entry.label.clone(),
        );
    println!(
        "agent {} ({}) {} in {community}",
        received.name,
        received.pubkey,
        if received.taken_back {
            "taken back: its files here were replaced by the ones that arrived"
        } else {
            "received"
        }
    );
    println!(
        "  its key is in bzz's custody, sealed to {}",
        identity.label
    );
    println!(
        "  {}",
        if received.taken_back {
            "the folders this machine had granted it are as they were"
        } else {
            "no folder of this machine is granted to it: grant one with `bzz agent edit --folder`"
        }
    );
    println!("  it is active: it answers while bzz is open, or under `bzz agent run`");
    println!(
        "  {}",
        if from_kit {
            "delete the launch kit's folder now, here and where it ran: it still holds the key in the clear"
        } else {
            "the file still opens with its passphrase: delete it when you no longer need it"
        }
    );
    println!("  run it in one place only: nothing stops a copy left elsewhere from answering too");
    Ok(())
}

async fn launch_kit(
    paths: &Paths,
    config: &mut Config,
    name: &str,
    to: &Path,
    yes: bool,
) -> Result<()> {
    let agent = manage::find(config, name)?.clone();
    moving::check_movable(config, agent.id)?;
    eprintln!(
        "A launch kit holds the key of {} in the clear, in {}: Buzz's buzz-acp reads an agent's key from its environment, so on that machine the key sits beside the agent. Whoever can read the folder can act as {}.",
        agent.label,
        to.join(bzz::agents::launch_kit::ENV_FILE).display(),
        agent.label
    );
    if !yes
        && !confirm(&format!(
            "Write the key of {} in the clear into that folder, and mark it moved away here?",
            agent.label
        ))?
    {
        return Err(Error::Config("no launch kit was written".into()));
    }
    let owner = unlock_owner(paths, config, agent.owner_identity)?;
    eprintln!(
        "stopping {} and writing its launch kit; nothing changes here until the kit is whole…",
        agent.label
    );
    let kit = match open_store(paths, config) {
        Ok(store) => {
            let mover = moving::Mover::new(paths, &store, &owner);
            moving::launch_kit(&mover, config, agent.id, to).await
        }
        Err(error) => Err(error),
    };
    owner.lock().await;
    let kit = kit?;
    println!(
        "launch kit for {} written in {}",
        kit.name,
        kit.folder.display()
    );
    println!(
        "  THE AGENT'S KEY IS IN THE CLEAR in {} (mode 600), and in no other file of the kit",
        kit.key_file.display()
    );
    println!(
        "  in it: its memory, its disk, its settings, bzz's agent host with its lock file, and README.md with the steps on the other machine"
    );
    println!("{}", stays_here(&agent));
    println!(
        "{} is marked moved away here: this bzz no longer starts it",
        kit.name
    );
    println!(
        "next: copy the folder to the other machine over a channel you trust, follow its README.md, and delete it here. `bzz agent shutdown {}` stops it there; `bzz agent receive <folder>` takes it back",
        shell_word(&kit.name)
    );
    Ok(())
}

async fn shutdown(
    paths: &Paths,
    config: &Config,
    name: &str,
    channel: Option<&str>,
    wait: u64,
) -> Result<()> {
    let agent = manage::find(config, name)?.clone();
    if manage::state_of(&agent) != "moved away" {
        eprintln!(
            "{} was not moved away from this bzz: the command stops a copy of it that Buzz's buzz-acp runs elsewhere, and nothing here (use `bzz agent pause` for that)",
            agent.label
        );
    }
    let store = open_store(paths, config)?;
    let (owner, relay) = owner_relay(paths, config, &agent).await?;
    let stopped = async {
        let channel = match channel {
            Some(wanted) => Some(relay.find_channel(&store, wanted).await?),
            None => None,
        };
        remote::shutdown(
            &relay,
            &store,
            &agent,
            channel,
            std::time::Duration::from_secs(wait),
        )
        .await
    }
    .await;
    relay.close().await;
    owner.lock().await;
    let stopped = stopped?;
    println!(
        "sent Buzz's stop command (`{}`) to {} as its owner, in #{}",
        remote::SHUTDOWN,
        agent.label,
        stopped.channel_name
    );
    match stopped.left {
        Some(true) => {
            println!("{} left: it is no longer present on the relay", agent.label);
            Ok(())
        }
        None => {
            println!(
                "{} was not present on the relay before the command, so its presence cannot say whether a copy heard it",
                agent.label
            );
            Ok(())
        }
        Some(false) => Err(Error::Timeout(format!(
            "{} is still present on the relay {wait} s after the command. A copy hears it only in a channel it is a member of, and only from its owner. It also stays present while another copy of it is connected, and for up to {} s after a copy crashed. Try `--channel`, or stop it on its machine",
            agent.label,
            bzz::protocol::presence::PRESENCE_TTL_SECONDS
        ))),
    }
}

/// `bzz agent show`: for an agent that was moved away, the relay is asked
/// whether it is present. That needs the owner's identity; when it cannot
/// be had, the rest is shown and the reason said.
async fn show(paths: &Paths, config: &Config, name: &str) -> Result<()> {
    let agent = manage::find(config, name)?.clone();
    let store = open_store(paths, config)?;
    let mut details = manage::details(paths, &store, &agent).await?;
    if manage::state_of(&agent) == "moved away" {
        details.presence = Some(match owner_relay(paths, config, &agent).await {
            Ok((owner, relay)) => {
                let presence = remote::presence_of(&relay, &store, &agent.pubkey).await;
                relay.close().await;
                owner.lock().await;
                match presence {
                    Ok(presence) => remote::presence_line(presence),
                    Err(error) => format!("unknown: the relay could not be asked ({error})"),
                }
            }
            Err(error) => format!("unknown: the relay could not be asked ({error})"),
        });
    }
    let community = config
        .communities
        .iter()
        .find(|community| community.id == agent.community)
        .map_or_else(|| agent.community.to_string(), |entry| entry.label.clone());
    for line in manage::show_lines(&agent, &community, &details) {
        println!("{line}");
    }
    Ok(())
}

pub async fn command(command: AgentCommand, paths: &Paths, config: &mut Config) -> Result<()> {
    // An agent whose folder says it was moved away is moved away, whatever
    // an older copy of the settings said when it was written back.
    let corrected = moving::reconcile(paths, config);
    if !corrected.is_empty() {
        config.save(paths)?;
        for name in corrected {
            eprintln!("{name} was moved away: the settings said otherwise and were corrected");
        }
    }
    match command {
        AgentCommand::Run { community } => crate::agent_run(paths, config, community).await,
        AgentCommand::New(args) => new_agent(paths, config, *args).await,
        AgentCommand::List => {
            list(paths, config);
            Ok(())
        }
        AgentCommand::Show { name } => show(paths, config, &name).await,
        AgentCommand::Move { name, to } => move_agent(paths, config, &name, &to).await,
        AgentCommand::Receive { file } => receive(paths, config, &file).await,
        AgentCommand::LaunchKit { name, to, yes } => {
            launch_kit(paths, config, &name, &to, yes).await
        }
        AgentCommand::Shutdown {
            name,
            channel,
            wait,
        } => shutdown(paths, config, &name, channel.as_deref(), wait).await,
        AgentCommand::Add { name, channel } => {
            let agent = manage::find(config, &name)?.clone();
            let store = open_store(paths, config)?;
            let (owner, relay) = owner_relay(paths, config, &agent).await?;
            let added = async {
                let channel = relay.find_channel(&store, &channel).await?;
                manage::add_to_channel(&relay, &store, &agent, channel).await
            }
            .await;
            relay.close().await;
            owner.lock().await;
            let added = added?;
            println!(
                "{} {} #{} {}",
                agent.label,
                if added.already {
                    "was already a member of"
                } else {
                    "added to"
                },
                added.channel_name,
                match added.role {
                    Some(role) => format!("with the {role} role (a Solo chat)"),
                    None => "as a member".to_owned(),
                }
            );
            println!(
                "mention it there to wake it; it answers while bzz is open, or under `bzz agent run`"
            );
            Ok(())
        }
        AgentCommand::Edit(args) => edit(paths, config, *args).await,
        AgentCommand::Pause { ref name } | AgentCommand::Resume { ref name } => {
            let paused = matches!(command, AgentCommand::Pause { .. });
            let agent = manage::find(config, name)?.clone();
            manage::set_paused(config, agent.id, paused)?;
            config.save(paths)?;
            println!(
                "{} {}; a bzz that is running applies it at its next start (in the TUI, use the agents panel)",
                agent.label,
                if paused { "paused" } else { "resumed" }
            );
            Ok(())
        }
        AgentCommand::Delete { name, yes, force } => delete(paths, config, &name, yes, force).await,
        AgentCommand::Doctor { models } => doctor(paths, config, models).await,
    }
}
