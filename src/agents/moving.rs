//! Moving an agent (plan v0.15.0, M5; ADR v0.15 local agents, decisions 2
//! and 12).
//!
//! - [`move_to_file`] (`bzz agent move`): the agent's key, its two files and
//!   the settings a new home needs, in one file sealed with a passphrase
//!   ([`bundle`]).
//! - [`receive_file`] and [`receive_kit`] (`bzz agent receive`): the agent
//!   made here from such a file, or taken back from a launch kit.
//! - [`launch_kit`] (`bzz agent launch-kit`): a folder for a machine without
//!   bzz, where Buzz's `buzz-acp` runs the agent
//!   ([`crate::agents::launch_kit`]).
//!
//! A move stops the agent, makes its files whole, packs, and only as its
//! last step marks the agent *moved away*: a failure before that leaves the
//! agent as it was. A moved-away agent keeps its entry, its sealed key and
//! its files here, and no bzz starts it ([`hold`]).
//!
//! What moves: name, public key, the owner's authorization, the community's
//! relay, harness, model, instructions, who may wake it, the hosts it may
//! reach, whether Pi packages are on, its picture. What never moves: granted
//! folders and git credentials, which belong to the machine.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use nostr::{Keys, PublicKey, ToBech32 as _};
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use zeroize::Zeroize as _;

use crate::{
    agents::{
        bundle::{self, Kdf},
        capability, launch_kit, manage,
        runtime::{
            hold::{self, Destination, Held, Hold},
            process,
            settle::{self, HostAccess},
        },
    },
    auth::signer::SignerHandle,
    broker::{
        custody,
        host::{credential_hash, new_credential},
    },
    config::{
        AgentModelConfig, CommunityConfig, Config, Harness, HostedAgentConfig, IdentityConfig,
        LocalAgentConfig, LocalAgentState, WakeRule, validate_relay_url,
    },
    error::{Error, Result},
    paths::Paths,
    store::{agent_runtime::RequestState, writer::StoreHandle},
};

/// The version of what a move carries.
pub const FORMAT: u32 = 1;
/// How long the bzz that runs an agent has to stop it for a move.
const RELEASE_LIMIT: Duration = Duration::from_secs(30);

/// Everything of an agent that goes to its new home, beside its two files.
/// `key` is the agent's secret key: it exists only inside a sealed file, and
/// in memory between custody and that file.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    pub format: u32,
    /// The bzz that made it, as `bzz <version>`.
    pub made_by: String,
    pub made_at: u64,
    pub id: Uuid,
    pub name: String,
    pub pubkey: String,
    /// The owner's NIP-OA authorization, as compact JSON.
    pub auth_tag: String,
    /// The relay of the agent's community.
    pub relay: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_insecure_localhost: bool,
    pub harness: Harness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModelConfig>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub wake: WakeRule,
    #[serde(default)]
    pub wake_allowlist: Vec<String>,
    /// The hosts it may reach.
    #[serde(default)]
    pub network: Vec<String>,
    /// Whether Pi packages are loaded for it.
    pub packages: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Secret key, hex. Empty in a launch kit's `bzz-agent.json`: there the
    /// key is in the environment file and nowhere else.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
}

impl Drop for Carried {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl std::fmt::Debug for Carried {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Carried")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("pubkey", &self.pubkey)
            .field("relay", &self.relay)
            .field("harness", &self.harness)
            .field("model", &self.model)
            .field("wake", &self.wake)
            .field("network", &self.network)
            .field("packages", &self.packages)
            .field(
                "key",
                &if self.key.is_empty() {
                    "none"
                } else {
                    "[secret]"
                },
            )
            .finish_non_exhaustive()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Whether an agent on `harness` can be moved, by its capability record.
pub fn movable(harness: Harness) -> Result<()> {
    let record = capability::record(harness);
    if record.move_with_memory.0 {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "an agent on the {} harness cannot be moved: {}",
            harness.as_str(),
            record.move_with_memory.1
        )))
    }
}

fn npub(hex: &str) -> String {
    PublicKey::from_hex(hex)
        .ok()
        .and_then(|key| key.to_bech32().ok())
        .unwrap_or_else(|| hex.to_owned())
}

impl Carried {
    /// What `agent` takes to a new home. Its granted folders and its launch
    /// override stay: they are this machine's.
    pub fn of(agent: &HostedAgentConfig, community: &CommunityConfig, key: &str) -> Result<Self> {
        let local = agent
            .local
            .as_ref()
            .ok_or_else(|| Error::Config("this agent is not run by bzz".into()))?;
        let relay = validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
        Ok(Self {
            format: FORMAT,
            made_by: format!("bzz {}", env!("CARGO_PKG_VERSION")),
            made_at: unix_now(),
            id: agent.id,
            name: agent.label.clone(),
            pubkey: agent.pubkey.clone(),
            auth_tag: agent.auth_tag.clone(),
            relay: relay.websocket.to_string(),
            allow_insecure_localhost: community.allow_insecure_localhost,
            harness: local.harness,
            model: local.model.clone(),
            instructions: local.instructions.clone(),
            wake: local.wake,
            wake_allowlist: local.wake_allowlist.clone(),
            network: local.network.clone(),
            packages: local.packages,
            picture: local.picture.clone(),
            key: key.to_owned(),
        })
    }

    /// The same without the key.
    pub fn without_key(&self) -> Self {
        let mut copy = self.clone();
        copy.key.zeroize();
        copy.key = String::new();
        copy
    }

    /// The owner the authorization names (hex). It fails unless the
    /// authorization was signed for this agent's key.
    pub fn owner(&self) -> Result<String> {
        custody::owner_pubkey(&self.auth_tag, &self.pubkey)
            .map(|owner| owner.to_hex())
            .map_err(|_| {
                Error::Config(
                    "the owner's authorization in it was not made for this agent's key: nothing was received"
                        .into(),
                )
            })
    }

    /// Checks everything that arrived before any of it is used: the format,
    /// the harness, the settings as `bzz agent new` would check them, and
    /// that the key is this agent's.
    pub fn check(&self) -> Result<()> {
        if self.format != FORMAT {
            return Err(Error::Unsupported(format!(
                "this agent was packed by {} in a form this bzz does not read (format {}; this bzz reads format {FORMAT}): update bzz and try again",
                self.made_by, self.format
            )));
        }
        movable(self.harness)?;
        manage::validate_settings(&self.name, self.wake, &self.wake_allowlist, &[])?;
        let hex_key = |value: &String| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if !hex_key(&self.pubkey) || !self.wake_allowlist.iter().all(hex_key) {
            return Err(Error::Config(
                "a public key in it is not a public key: nothing was received".into(),
            ));
        }
        for host in &self.network {
            if manage::parse_host(host)? != *host {
                return Err(Error::Config(format!(
                    "{host:?} in its allowed hosts is not written as bzz writes a host: nothing was received"
                )));
            }
        }
        if let Some(model) = &self.model {
            manage::parse_model(
                &format!("{}/{}", model.provider, model.model_id),
                model.thinking_level.as_deref(),
            )?;
        }
        if let Some(picture) = &self.picture {
            manage::parse_picture(picture)?;
        }
        validate_relay_url(&self.relay, self.allow_insecure_localhost)?;
        self.owner()?;
        if !self.key.is_empty() {
            let keys = Keys::parse(&self.key)
                .map_err(|_| Error::IdentityCorrupt("the agent's key in it is not a key".into()))?;
            if keys.public_key().to_hex() != self.pubkey {
                return Err(Error::IdentityCorrupt(
                    "the key in it is not the key of the agent it names: nothing was received"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    /// The agent's settings in its new home: active, with no folder of any
    /// machine and the default launch.
    pub fn settings(&self) -> LocalAgentConfig {
        LocalAgentConfig {
            wake: self.wake,
            wake_allowlist: self.wake_allowlist.clone(),
            model: self.model.clone(),
            instructions: self.instructions.clone(),
            network: self.network.clone(),
            picture: self.picture.clone(),
            packages: self.packages,
            ..LocalAgentConfig::new(self.harness)
        }
    }

    /// `agent.json`, which the Pi Durable host reads when Buzz's `buzz-acp`
    /// starts it (`docs/agent-host-protocol.md`, section 1). No folder of
    /// any machine is in it, and no key.
    pub fn agent_json(&self) -> Value {
        let mut file = json!({
            "name": self.name,
            "instructions": self.instructions,
            "network": self.network,
            "packages": self.packages,
        });
        if let Some(model) = &self.model {
            file["model"] = json!({"provider": model.provider, "modelId": model.model_id});
            if let Some(level) = &model.thinking_level {
                file["model"]["thinkingLevel"] = json!(level);
            }
        }
        file
    }
}

/// The community and the identity an arriving agent belongs to here. It is
/// refused unless this bzz has the agent's community, and the identity this
/// bzz uses there is the owner the authorization names.
pub fn home<'a>(
    config: &'a Config,
    carried: &Carried,
) -> Result<(&'a CommunityConfig, &'a IdentityConfig)> {
    let relay = validate_relay_url(&carried.relay, carried.allow_insecure_localhost)?.websocket;
    let community = config
        .communities
        .iter()
        .find(|community| {
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)
                .is_ok_and(|endpoint| endpoint.websocket == relay)
        })
        .ok_or_else(|| {
            Error::Config(format!(
                "this agent belongs to the community at {relay}, which this bzz does not have. Add that community with the identity that owns the agent (`bzz community add`), then receive it again. Nothing was received"
            ))
        })?;
    let identity = config
        .identities
        .iter()
        .find(|identity| identity.id == community.identity_id)
        .ok_or_else(|| Error::Config("the community's identity does not exist".into()))?;
    let owner = carried.owner()?;
    if !identity.pubkey.eq_ignore_ascii_case(&owner) {
        return Err(Error::Access(format!(
            "this agent is owned by {}, and this bzz's identity in {} is {} ({}). An agent is received only by its owner. Nothing was received",
            npub(&owner),
            community.label,
            identity.label,
            npub(&identity.pubkey),
        )));
    }
    Ok((community, identity))
}

/// What a move needs of the machine.
pub struct Mover<'a> {
    pub paths: &'a Paths,
    pub store: &'a StoreHandle,
    /// The signer of the agent's owner, unlocked.
    pub owner: &'a SignerHandle,
    /// The agent's host, which makes its files whole.
    pub host: HostAccess,
    pub kdf: Kdf,
    /// How long the bzz that runs the agent has to stop it.
    pub release_limit: Duration,
}

impl<'a> Mover<'a> {
    pub fn new(paths: &'a Paths, store: &'a StoreHandle, owner: &'a SignerHandle) -> Self {
        Self {
            paths,
            store,
            owner,
            host: HostAccess::of_this_process(),
            kdf: Kdf::default(),
            release_limit: RELEASE_LIMIT,
        }
    }
}

fn local_agent(config: &Config, id: Uuid) -> Result<(&HostedAgentConfig, &LocalAgentConfig)> {
    let agent = config
        .hosted_agents
        .iter()
        .find(|agent| agent.id == id)
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    let local = agent
        .local
        .as_ref()
        .ok_or_else(|| Error::Config("this agent is not run by bzz".into()))?;
    Ok((agent, local))
}

/// What can be told before any passphrase is asked for: whether this agent
/// can be moved at all, and where to.
pub fn check_movable(config: &Config, id: Uuid) -> Result<()> {
    let (agent, local) = local_agent(config, id)?;
    movable(local.harness)?;
    if local.state == LocalAgentState::MovedAway {
        return Err(Error::Config(format!(
            "{} was already moved away: it is not here to move. `bzz agent receive` takes it back first",
            agent.label
        )));
    }
    Ok(())
}

/// Requests the agent was woken by and has not finished answering.
async fn open_requests(store: &StoreHandle, agent: Uuid) -> Result<usize> {
    store
        .call(move |store| {
            Ok(store
                .agent_inbox_in_state(agent, RequestState::Delivered)?
                .len()
                + store
                    .agent_inbox_in_state(agent, RequestState::Queued)?
                    .len())
        })
        .await
}

fn still_answering(name: &str, open: usize) -> Error {
    Error::Config(format!(
        "{name} has {open} request(s) it has not finished answering. A move would cut them off: open bzz (or `bzz agent run`), let it answer, and move it then. Nothing was changed"
    ))
}

/// An agent stopped and with its files whole, held for the rest of a move.
/// Dropped without the move's last step, it lets the agent go: the bzz that
/// ran it runs it again.
pub struct Prepared {
    held: Held,
    pub carried: Carried,
    /// The agent's folder, with `memory.sqlite` and `files.sqlite`.
    pub folder: PathBuf,
}

/// The first steps of every move, in order: the checks, the key out of
/// custody, the agent stopped wherever it runs, its files made whole.
pub async fn prepare(mover: &Mover<'_>, config: &Config, id: Uuid) -> Result<Prepared> {
    check_movable(config, id)?;
    let (agent, local) = local_agent(config, id)?;
    // An agent whose host is run from a checkout is made whole by that host.
    let mut host = mover.host.clone();
    if host.command.is_none() && !local.command.is_empty() {
        host.command = Some(local.command.clone());
    }
    let community = config
        .communities
        .iter()
        .find(|community| community.id == agent.community)
        .ok_or_else(|| Error::Config("the agent's community does not exist".into()))?;
    let data_dir = &mover.paths.data_dir;
    let key = custody::take_out(&mover.paths.keys_dir(), mover.owner, agent).await?;
    let carried = Carried::of(agent, community, &key)?;
    // What cannot be received must not be sent.
    carried.check()?;
    if carried.owner()? != mover.owner.public_key().to_hex() {
        return Err(Error::Config(
            "the agent's authorization does not name the identity that holds its key".into(),
        ));
    }
    let open = open_requests(mover.store, id).await?;
    if open > 0 {
        return Err(still_answering(&agent.label, open));
    }
    // From here every bzz of this data folder keeps its hands off the agent.
    let held = hold::take(data_dir, id)?;
    // A bzz that was starting the agent this very moment has written its
    // record by now; one that runs it stops it within half a second.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let deadline = tokio::time::Instant::now() + mover.release_limit;
    while let Some(pid) = manage::running_pid(data_dir, id) {
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Timeout(format!(
                "the bzz that runs {} did not stop it (process {pid}). Close that bzz, or pause the agent there, and move it then. Nothing was changed",
                agent.label
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let open = open_requests(mover.store, id).await?;
    if open > 0 {
        return Err(still_answering(&agent.label, open));
    }
    settle::settle(data_dir, id, &agent.label, &host).await?;
    Ok(Prepared {
        held,
        carried,
        folder: process::agent_dir(data_dir, id),
    })
}

/// The last step of every move: the agent is moved away here. If it cannot
/// be written down, it did not happen.
fn finish(prepared: Prepared, paths: &Paths, config: &mut Config, to: Destination) -> Result<()> {
    let id = prepared.carried.id;
    let before = config.clone();
    let agent = config
        .hosted_agents
        .iter_mut()
        .find(|agent| agent.id == id)
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    if let Some(local) = agent.local.as_mut() {
        local.state = LocalAgentState::MovedAway;
    }
    agent.paused = false;
    // The agent's folder first: from this moment no bzz starts it, also one
    // that is running with an older copy of the settings.
    if let Err(error) = prepared.held.moved_away(to) {
        *config = before;
        return Err(error);
    }
    if let Err(error) = config.save(paths) {
        *config = before;
        hold::clear(&paths.data_dir, id);
        return Err(error);
    }
    Ok(())
}

/// What `bzz agent move` made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Moved {
    pub name: String,
    pub file: PathBuf,
    pub bytes: u64,
}

fn partial_beside(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    to.with_file_name(format!(".{name}.{:016x}.partial", rand::random::<u64>()))
}

/// `bzz agent move <name> --to <file>`: stops the agent, makes its files
/// whole, seals key, files and settings into the new file `to`, reads the
/// file back, and then marks the agent moved away. Before that last step a
/// failure leaves the agent as it was, and no file at `to`.
pub async fn move_to_file(
    mover: &Mover<'_>,
    config: &mut Config,
    id: Uuid,
    passphrase: &SecretString,
    to: &Path,
) -> Result<Moved> {
    check_movable(config, id)?;
    if passphrase.expose_secret().chars().count() < bundle::MIN_PASSPHRASE_CHARS {
        return Err(bundle::Refusal::WeakPassphrase.into());
    }
    if to.symlink_metadata().is_ok() {
        return Err(Error::Config(format!(
            "{} exists: bzz does not write over it. Name a file that is not there",
            to.display()
        )));
    }
    if !to
        .parent()
        .is_none_or(|folder| folder.as_os_str().is_empty() || folder.is_dir())
    {
        return Err(Error::Config(format!(
            "the folder of {} does not exist",
            to.display()
        )));
    }
    let prepared = prepare(mover, config, id).await?;
    let partial = partial_beside(to);
    let content = zeroize::Zeroizing::new(
        serde_json::to_vec(&prepared.carried)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    );
    let (memory, disk) = (
        prepared.folder.join(settle::FILES[0]),
        prepared.folder.join(settle::FILES[1]),
    );
    let sealed = async {
        let (passphrase, kdf) = (passphrase.clone(), mover.kdf);
        let (partial, content) = (partial.clone(), content.clone());
        // Deriving the key and sealing take a while: off the async threads.
        tokio::task::spawn_blocking(move || {
            let entries = bundle::seal(
                &partial,
                passphrase.expose_secret(),
                &content,
                &[(settle::FILES[0], &memory), (settle::FILES[1], &disk)],
                kdf,
            )?;
            // Read back as the receiving bzz will read it, before anything
            // depends on it.
            let opened = bundle::open(&partial, passphrase.expose_secret())?;
            if *opened.content != *content || opened.files != entries {
                return Err(bundle::Refusal::Changed(
                    "it did not read back as it was written",
                ));
            }
            Ok(())
        })
        .await
        .map_err(|error| Error::Config(format!("sealing the file failed: {error}")))?
        .map_err(Error::from)
    }
    .await;
    let placed = sealed.and_then(|()| {
        // Never over a file that appeared meanwhile.
        if to.symlink_metadata().is_ok() {
            return Err(Error::Config(format!("{} exists", to.display())));
        }
        std::fs::rename(&partial, to).map_err(|error| Error::io(to, error))
    });
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    let name = prepared.carried.name.clone();
    if let Err(error) = finish(prepared, mover.paths, config, Destination::File) {
        // The agent is still here: a sealed copy of it must not lie about.
        let _ = std::fs::remove_file(to);
        return Err(error);
    }
    Ok(Moved {
        name,
        bytes: std::fs::metadata(to).map_or(0, |file| file.len()),
        file: to.to_path_buf(),
    })
}

/// What `bzz agent launch-kit` made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kit {
    pub name: String,
    pub folder: PathBuf,
    /// The one file that holds the agent's key, in the clear.
    pub key_file: PathBuf,
}

/// `bzz agent launch-kit <name> --to <folder>`: as a move, but into a
/// folder that Buzz's `buzz-acp` can run the agent from, with the agent's
/// key in the clear in one file of it. The folder must not exist. The owner
/// was asked before this is called.
pub async fn launch_kit(
    mover: &Mover<'_>,
    config: &mut Config,
    id: Uuid,
    to: &Path,
) -> Result<Kit> {
    check_movable(config, id)?;
    if to.symlink_metadata().is_ok() {
        return Err(Error::Config(format!(
            "{} exists: bzz writes a launch kit only into a folder it makes. Name a folder that is not there",
            to.display()
        )));
    }
    let prepared = prepare(mover, config, id).await?;
    let written = launch_kit::write(to, &prepared.carried, &prepared.folder);
    let key_file = match written {
        Ok(key_file) => key_file,
        Err(error) => {
            let _ = std::fs::remove_dir_all(to);
            return Err(error);
        }
    };
    let name = prepared.carried.name.clone();
    if let Err(error) = finish(prepared, mover.paths, config, Destination::LaunchKit) {
        // The agent is still here: its key must not lie about in the clear.
        let _ = std::fs::remove_dir_all(to);
        return Err(error);
    }
    Ok(Kit {
        name,
        folder: to.to_path_buf(),
        key_file,
    })
}

/// How an agent arrived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Received {
    pub id: Uuid,
    pub name: String,
    pub pubkey: String,
    pub community: Uuid,
    /// This bzz had moved it away and has it back now. Otherwise it is new
    /// here.
    pub taken_back: bool,
}

/// What `receive` will do with what arrived, told before the owner's
/// identity is unlocked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub community: Uuid,
    pub identity: Uuid,
    /// The agent's id here.
    pub id: Uuid,
    pub taken_back: bool,
}

/// Decides where an arriving agent goes, and refuses what must not arrive:
/// an agent of another owner or community, one that is already here and
/// was not moved away, or one whose name another agent here has.
pub fn plan(config: &Config, carried: &Carried) -> Result<Plan> {
    carried.check()?;
    let (community, identity) = home(config, carried)?;
    let existing = config
        .hosted_agents
        .iter()
        .find(|agent| agent.pubkey.eq_ignore_ascii_case(&carried.pubkey));
    let taken_back = match existing {
        Some(agent)
            if agent
                .local
                .as_ref()
                .is_some_and(|local| local.state == LocalAgentState::MovedAway) =>
        {
            if agent.community != community.id || agent.owner_identity != identity.id {
                return Err(Error::Config(format!(
                    "{} is here in another community than the one it arrives for. Nothing was received",
                    agent.label
                )));
            }
            true
        }
        Some(agent) => {
            return Err(Error::Config(format!(
                "{} ({}) is already here and was not moved away: receiving it again would not make a second agent, and nothing was changed. To replace it with what arrives, move or delete the one here first",
                agent.label, agent.pubkey
            )));
        }
        None => false,
    };
    let id = match existing {
        Some(agent) => agent.id,
        None if manage::name_taken(config, &carried.name, None) => {
            return Err(Error::Config(format!(
                "another agent here is called {}. Give that one another name (`bzz agent edit {} --name …`) and receive this one again. Nothing was received",
                carried.name, carried.name
            )));
        }
        // Its id travels with it, unless something here has it.
        None if config
            .hosted_agents
            .iter()
            .all(|agent| agent.id != carried.id) =>
        {
            carried.id
        }
        None => Uuid::new_v4(),
    };
    Ok(Plan {
        community: community.id,
        identity: identity.id,
        id,
        taken_back,
    })
}

/// Where an arriving agent's two files come from.
enum Source {
    Sealed(Box<bundle::Opened>),
    /// A launch kit's `agent` folder.
    Folder(PathBuf),
}

/// A sealed file, opened: what it carries, before anything is received.
pub struct Arrival {
    pub carried: Carried,
    source: Source,
}

impl std::fmt::Debug for Arrival {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Arrival")
            .field("carried", &self.carried)
            .finish_non_exhaustive()
    }
}

/// Opens a file made by `bzz agent move`. It is checked whole first: cut
/// short, changed, or the wrong passphrase, and nothing of it is used.
pub async fn open_file(file: &Path, passphrase: &SecretString) -> Result<Arrival> {
    let (path, passphrase) = (file.to_path_buf(), passphrase.clone());
    let opened =
        tokio::task::spawn_blocking(move || bundle::open(&path, passphrase.expose_secret()))
            .await
            .map_err(|error| Error::Config(format!("opening the file failed: {error}")))??;
    let carried: Carried = serde_json::from_slice(&opened.content).map_err(|_| {
        Error::Config(
            "the file opened, and what it holds is not an agent as this bzz writes one. Was it made by a newer bzz? Nothing was received"
                .into(),
        )
    })?;
    if carried.key.is_empty() {
        return Err(Error::Config(
            "the file holds no key for the agent: nothing was received".into(),
        ));
    }
    let names = opened
        .files
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    if names != settle::FILES {
        return Err(Error::Config(format!(
            "the file holds {names:?} where an agent's two files were expected: nothing was received"
        )));
    }
    Ok(Arrival {
        carried,
        source: Source::Sealed(Box::new(opened)),
    })
}

/// Reads a launch kit that came back from the machine it ran on. Stop the
/// agent there first: nothing here can tell whether a copy of the folder
/// is still being run.
pub fn open_kit(folder: &Path) -> Result<Arrival> {
    let (mut carried, key) = launch_kit::read(folder)?;
    carried.key = key.to_string();
    let files = folder.join(launch_kit::AGENT_FOLDER);
    for file in settle::FILES {
        if !files.join(file).is_file() {
            return Err(Error::Config(format!(
                "the launch kit has no {}/{file}: nothing was received",
                launch_kit::AGENT_FOLDER
            )));
        }
    }
    Ok(Arrival {
        carried,
        source: Source::Folder(files),
    })
}

fn incoming(folder: &Path, name: &str, token: u64) -> PathBuf {
    folder.join(format!("{name}.{token:016x}.incoming"))
}

/// Copies `from` to the new private file `to`.
fn copy_private(from: &Path, to: &Path) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut source = std::fs::File::open(from).map_err(|error| Error::io(from, error))?;
    let mut target = options.open(to).map_err(|error| Error::io(to, error))?;
    std::io::copy(&mut source, &mut target).map_err(|error| Error::io(to, error))?;
    target.sync_all().map_err(|error| Error::io(to, error))
}

/// `bzz agent receive`: makes the agent here, with its key in bzz's custody,
/// its two files and its settings, active and with no folder granted. An
/// agent this bzz had moved away is taken back: its files are replaced by
/// what arrives, and it is active again. `owner` is the signer of the
/// identity [`plan`] named.
///
/// `host` is used only for a launch kit whose files came with write-ahead
/// files: `buzz-acp` does not give the host time to close them. The host
/// makes a copy of them whole here; the kit itself is not changed.
pub async fn receive(
    paths: &Paths,
    store: &StoreHandle,
    owner: &SignerHandle,
    config: &mut Config,
    arrival: Arrival,
    host: &HostAccess,
) -> Result<Received> {
    let Arrival { carried, source } = arrival;
    let plan = plan(config, &carried)?;
    if owner.public_key().to_hex() != carried.owner()? {
        return Err(Error::Access(
            "the identity that was unlocked is not the agent's owner: nothing was received".into(),
        ));
    }
    let data_dir = &paths.data_dir;
    let keys_dir = paths.keys_dir();
    let folder = process::agent_dir(data_dir, plan.id);
    let folder_was_there = folder.exists();
    // No bzz of this data folder starts it while its files are replaced.
    let held = hold::take(data_dir, plan.id)?;
    if let Some(pid) = manage::running_pid(data_dir, plan.id) {
        return Err(Error::Config(format!(
            "{} is running here (process {pid}), although it was moved away. Close the bzz that runs it and receive it again. Nothing was received",
            carried.name
        )));
    }
    let token = rand::random::<u64>();
    let staged = settle::FILES.map(|name| incoming(&folder, name, token));
    let before = config.clone();
    let mut wrote_key = false;
    let result = async {
        process::create_private_dir(&folder)?;
        // The files first, complete and checked, beside where they will be.
        match source {
            Source::Sealed(opened) => {
                let target = folder.clone();
                tokio::task::spawn_blocking(move || {
                    opened.unpack(|name| incoming(&target, name, token))
                })
                .await
                .map_err(|error| Error::Config(format!("unpacking the file failed: {error}")))??;
            }
            Source::Folder(from) => {
                let open = settle::write_ahead_files(&from);
                if open.is_empty() {
                    for (name, staged) in settle::FILES.iter().zip(&staged) {
                        copy_private(&from.join(name), staged)?;
                    }
                } else {
                    // Not closed where they ran: a copy of the files and of
                    // what is beside them, made whole by the agent's host.
                    let apart = folder.join(format!("{token:016x}.incoming"));
                    let made_whole = async {
                        process::create_private_dir(&apart)?;
                        for file in settle::FILES
                            .iter()
                            .map(|name| from.join(name))
                            .chain(open.iter().cloned())
                        {
                            if let Some(name) = file.file_name() {
                                copy_private(&file, &apart.join(name))?;
                            }
                        }
                        settle::settle_folder(data_dir, &apart, plan.id, &carried.name, host)
                            .await?;
                        for (name, staged) in settle::FILES.iter().zip(&staged) {
                            std::fs::rename(apart.join(name), staged)
                                .map_err(|error| Error::io(staged, error))?;
                        }
                        Ok::<_, Error>(())
                    }
                    .await;
                    let _ = std::fs::remove_dir_all(&apart);
                    made_whole?;
                }
            }
        }
        wrote_key =
            custody::take_in(&keys_dir, owner, plan.id, &carried.pubkey, &carried.key).await?;
        // Its entry in the settings.
        match config
            .hosted_agents
            .iter_mut()
            .find(|agent| agent.id == plan.id)
        {
            Some(agent) => {
                // What this machine granted it stays this machine's.
                let (folders, command) = agent
                    .local
                    .as_ref()
                    .map(|local| (local.folders.clone(), local.command.clone()))
                    .unwrap_or_default();
                agent.label = carried.name.clone();
                agent.auth_tag = carried.auth_tag.clone();
                agent.paused = false;
                agent.local = Some(LocalAgentConfig {
                    folders,
                    command,
                    ..carried.settings()
                });
            }
            None => config.hosted_agents.push(HostedAgentConfig {
                id: plan.id,
                label: carried.name.clone(),
                pubkey: carried.pubkey.clone(),
                community: plan.community,
                owner_identity: plan.identity,
                auth_tag: carried.auth_tag.clone(),
                // bzz itself runs it: no broker credential is ever given out.
                credential_hash: credential_hash(&new_credential()),
                allowed: Vec::new(),
                paused: false,
                local: Some(carried.settings()),
            }),
        }
        // The files into place, then the settings: from here it is here.
        for left in settle::write_ahead_files(&folder) {
            std::fs::remove_file(&left).map_err(|error| Error::io(&left, error))?;
        }
        for (name, staged) in settle::FILES.iter().zip(&staged) {
            let place = folder.join(name);
            std::fs::rename(staged, &place).map_err(|error| Error::io(&place, error))?;
        }
        config.save(paths)
    }
    .await;
    if let Err(error) = result {
        *config = before;
        for staged in &staged {
            let _ = std::fs::remove_file(staged);
        }
        if !plan.taken_back {
            // Nothing of an agent that did not arrive stays.
            if wrote_key {
                custody::delete_by_id(&keys_dir, plan.id);
            }
            drop(held);
            if !folder_was_there {
                let _ = std::fs::remove_dir_all(&folder);
            }
        }
        return Err(error);
    }
    if plan.taken_back {
        // What was said to it while it was away was its other home's to
        // answer: nothing from before this moment is a request here. The
        // intake reads from 30 s before its mark.
        let (id, mark) = (plan.id, unix_now() + 30);
        let _ = store
            .call(move |store| store.advance_agent_cursor(id, mark))
            .await;
    }
    held.free()?;
    Ok(Received {
        id: plan.id,
        name: carried.name.clone(),
        pubkey: carried.pubkey.clone(),
        community: plan.community,
        taken_back: plan.taken_back,
    })
}

/// Makes the settings agree with the agents' folders: an agent whose folder
/// says it was moved away is moved away in the settings too. A bzz that was
/// open during a move may have written its older copy of the settings back.
/// Returns the names it corrected; the caller saves.
pub fn reconcile(paths: &Paths, config: &mut Config) -> Vec<String> {
    let mut corrected = Vec::new();
    for agent in &mut config.hosted_agents {
        let Some(local) = agent.local.as_mut() else {
            continue;
        };
        let marked = matches!(
            hold::read(&paths.data_dir, agent.id),
            Some(Hold::MovedAway { .. })
        );
        if marked && local.state != LocalAgentState::MovedAway {
            local.state = LocalAgentState::MovedAway;
            agent.paused = false;
            corrected.push(agent.label.clone());
        } else if !marked && local.state == LocalAgentState::MovedAway {
            // Settings that say so by themselves: the folder says it too
            // from now on, so that a running bzz sees it.
            let _ = hold::mark_moved_away(&paths.data_dir, agent.id, Destination::File);
        }
    }
    corrected
}

/// When and where an agent was moved, for `bzz agent show`.
pub fn moved_line(data_dir: &Path, agent: Uuid) -> Option<String> {
    match hold::read(data_dir, agent)? {
        Hold::MovedAway { at, to } => Some(format!(
            "moved away{}{}",
            at.map(|at| format!(" on {}", manage::when(at)))
                .unwrap_or_default(),
            to.map(|to| format!(", {}", to.as_str()))
                .unwrap_or_default()
        )),
        Hold::Moving { pid } => Some(format!("being moved by bzz process {pid}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FolderGrant, KeyBackend};

    struct Made {
        owner: Keys,
        key: Keys,
        agent: HostedAgentConfig,
        config: Config,
    }

    /// A configuration with one identity, one community and one agent that
    /// has every setting an owner can give it.
    fn made() -> Made {
        let (owner, key) = (Keys::generate(), Keys::generate());
        let auth_tag = buzz_sdk::nip_oa::compute_auth_tag(&owner, &key.public_key(), "").unwrap();
        let mut config = Config::default();
        let identity = Uuid::new_v4();
        config.identities.push(IdentityConfig {
            id: identity,
            label: "ann".into(),
            pubkey: owner.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "ann.key".into(),
        });
        let community = config
            .add_community("team".into(), "wss://relay.example".into(), identity, false)
            .unwrap();
        let agent = HostedAgentConfig {
            id: Uuid::new_v4(),
            label: "Scout".into(),
            pubkey: key.public_key().to_hex(),
            community,
            owner_identity: identity,
            auth_tag,
            credential_hash: "c".repeat(64),
            allowed: Vec::new(),
            paused: false,
            local: Some(LocalAgentConfig {
                wake: WakeRule::Allowlist,
                wake_allowlist: vec![Keys::generate().public_key().to_hex()],
                model: Some(AgentModelConfig {
                    provider: "openai-codex".into(),
                    model_id: "gpt-6-luna".into(),
                    thinking_level: Some("low".into()),
                }),
                instructions: "Be brief.".into(),
                folders: vec![FolderGrant {
                    name: "site".into(),
                    path: "/home/ann/private-site".into(),
                    write: true,
                }],
                network: vec!["github.com".into()],
                command: vec!["/home/ann/checkout/host".into()],
                picture: Some("https://example.org/scout.png".into()),
                packages: false,
                ..LocalAgentConfig::new(Harness::PiDurable)
            }),
        };
        config.hosted_agents.push(agent.clone());
        Made {
            owner,
            key,
            agent,
            config,
        }
    }

    fn carried(made: &Made) -> Carried {
        Carried::of(
            &made.agent,
            &made.config.communities[0],
            &made.key.secret_key().to_secret_hex(),
        )
        .unwrap()
    }

    #[test]
    fn what_is_carried_is_what_a_new_home_needs_and_nothing_of_this_machine() {
        let made = made();
        let carried = carried(&made);
        let local = made.agent.local.clone().unwrap();
        let written: Value = serde_json::to_value(&carried).unwrap();
        // Every setting a new home needs, by name.
        assert_eq!(
            written,
            json!({
                "format": 1,
                "made_by": format!("bzz {}", env!("CARGO_PKG_VERSION")),
                "made_at": carried.made_at,
                "id": made.agent.id,
                "name": "Scout",
                "pubkey": made.agent.pubkey,
                "auth_tag": made.agent.auth_tag,
                "relay": "wss://relay.example/",
                "harness": "pi-durable",
                "model": {"provider": "openai-codex", "model_id": "gpt-6-luna", "thinking_level": "low"},
                "instructions": "Be brief.",
                "wake": "allowlist",
                "wake_allowlist": local.wake_allowlist,
                "network": ["github.com"],
                "packages": false,
                "picture": "https://example.org/scout.png",
                "key": made.key.secret_key().to_secret_hex(),
            })
        );
        // Never what belongs to this machine: a granted folder, the launch
        // override, the broker's credential.
        let text = written.to_string();
        for stays in [
            "private-site",
            "folders",
            "checkout",
            "command",
            "credential",
        ] {
            assert!(!text.contains(stays), "{stays} travels: {text}");
        }
        // In and out: the same, and in the new home it is active, with no
        // folder and the default launch.
        let back: Carried = serde_json::from_str(&text).unwrap();
        assert_eq!(back, carried);
        back.check().unwrap();
        assert_eq!(back.owner().unwrap(), made.owner.public_key().to_hex());
        assert_eq!(
            back.settings(),
            LocalAgentConfig {
                folders: Vec::new(),
                command: Vec::new(),
                state: LocalAgentState::Active,
                ..local
            }
        );
        // A paused agent arrives active: its new owner's bzz decides anew.
        let mut paused = made.agent.clone();
        paused.local.as_mut().unwrap().state = LocalAgentState::Paused;
        assert_eq!(
            Carried::of(&paused, &made.config.communities[0], &back.key)
                .unwrap()
                .settings()
                .state,
            LocalAgentState::Active
        );
        // The key is never printed with the rest.
        let shown = format!("{carried:?}");
        assert!(
            !shown.contains(&carried.key) && shown.contains("[secret]"),
            "{shown}"
        );
        // Without the key, as a launch kit's settings file holds it.
        let bare = carried.without_key();
        assert!(bare.key.is_empty());
        assert!(!serde_json::to_string(&bare).unwrap().contains("\"key\""));
        bare.check().unwrap();
    }

    #[test]
    fn the_host_reads_its_settings_from_agent_json_and_no_folder_is_in_it() {
        let made = made();
        assert_eq!(
            carried(&made).agent_json(),
            json!({
                "name": "Scout",
                "model": {"provider": "openai-codex", "modelId": "gpt-6-luna", "thinkingLevel": "low"},
                "instructions": "Be brief.",
                "network": ["github.com"],
                "packages": false,
            })
        );
        // With every default: no model is named, and packages are on.
        let mut plain = made.agent.clone();
        plain.local = Some(LocalAgentConfig::new(Harness::PiDurable));
        assert_eq!(
            Carried::of(&plain, &made.config.communities[0], "")
                .unwrap()
                .agent_json(),
            json!({"name": "Scout", "instructions": "", "network": [], "packages": true})
        );
    }

    #[test]
    fn what_arrives_is_checked_before_any_of_it_is_used() {
        let made = made();
        let good = carried(&made);
        good.check().unwrap();
        let refused = |change: &dyn Fn(&mut Carried)| {
            let mut changed = good.clone();
            change(&mut changed);
            changed.check().unwrap_err().to_string()
        };
        // Another version of what is carried.
        assert!(refused(&|carried| carried.format = 2).contains("update bzz"));
        // A harness that cannot be moved.
        assert!(refused(&|carried| carried.harness = Harness::Grok).contains("cannot be moved"));
        // A key that is not the agent's, and something that is no key.
        let other = Keys::generate();
        assert!(
            refused(&|carried| carried.key = other.secret_key().to_secret_hex())
                .contains("not the key of the agent it names")
        );
        assert!(refused(&|carried| carried.key = "not a key".into()).contains("is not a key"));
        // An authorization made for another agent's key, or by nobody.
        let foreign =
            buzz_sdk::nip_oa::compute_auth_tag(&made.owner, &other.public_key(), "").unwrap();
        assert!(
            refused(&|carried| carried.auth_tag = foreign.clone())
                .contains("was not made for this agent's key")
        );
        assert!(
            refused(&|carried| carried.auth_tag = "[]".into())
                .contains("was not made for this agent's key")
        );
        // Settings bzz itself would not have written.
        assert!(refused(&|carried| carried.name = "two\nlines".into()).contains("1 to 64"));
        assert!(refused(&|carried| carried.wake_allowlist.clear()).contains("allowlist"));
        assert!(
            refused(&|carried| carried.network.push("https://evil.example/x".into()))
                .contains("is not a host")
        );
        assert!(
            refused(&|carried| carried.network.push("GitHub.com".into()))
                .contains("not written as bzz writes a host")
        );
        assert!(
            refused(&|carried| carried.picture = Some("http://example.org/a.png".into()))
                .contains("HTTPS")
        );
        assert!(refused(&|carried| carried.relay = "http://relay.example".into()).contains("wss"));
        assert!(refused(&|carried| carried.pubkey = "abc".into()).contains("public key"));
        // Something it does not know is not read past: a file that names a
        // folder of some machine is no agent of ours.
        let mut with_folder = serde_json::to_value(&good).unwrap();
        with_folder["folders"] = json!([{"name": "site", "path": "/etc", "write": true}]);
        assert!(serde_json::from_value::<Carried>(with_folder).is_err());
    }

    #[test]
    fn an_agent_goes_only_to_its_owner_in_its_own_community() {
        let made = made();
        let arriving = carried(&made);
        // Here it is already, active: not received again.
        let said = plan(&made.config, &arriving).unwrap_err().to_string();
        assert!(
            said.contains("already here and was not moved away"),
            "{said}"
        );
        // Moved away from here: taken back, under the id it has here.
        let mut away = made.config.clone();
        away.hosted_agents[0].local.as_mut().unwrap().state = LocalAgentState::MovedAway;
        let back = plan(&away, &arriving).unwrap();
        assert_eq!((back.id, back.taken_back), (made.agent.id, true));
        // New in another bzz of the owner's: its id travels with it, and the
        // relay is the same however its address was written.
        let mut fresh = made.config.clone();
        fresh.hosted_agents.clear();
        fresh.communities[0].relay_url = "wss://RELAY.example".into();
        let new = plan(&fresh, &arriving).unwrap();
        assert_eq!(
            (new.id, new.taken_back, new.community, new.identity),
            (
                made.agent.id,
                false,
                fresh.communities[0].id,
                fresh.identities[0].id
            )
        );
        // A v0.13 hosted agent with that key is not something to take back.
        let mut hosted = made.config.clone();
        hosted.hosted_agents[0].local = None;
        assert!(plan(&hosted, &arriving).is_err());
        // An id that something else here has: it gets a new one.
        let mut clash = fresh.clone();
        let mut other = made.agent.clone();
        other.label = "Other".into();
        other.pubkey = Keys::generate().public_key().to_hex();
        clash.hosted_agents.push(other);
        let renumbered = plan(&clash, &arriving).unwrap();
        assert_ne!(renumbered.id, made.agent.id);
        // Another owner's identity in that community.
        let mut stranger = fresh.clone();
        stranger.identities[0].pubkey = Keys::generate().public_key().to_hex();
        let said = plan(&stranger, &arriving).unwrap_err().to_string();
        assert!(
            said.contains("owned by npub1") && said.contains("this bzz's identity in team is ann"),
            "{said}"
        );
        // A bzz that does not have the community.
        let mut elsewhere = fresh.clone();
        elsewhere.communities[0].relay_url = "wss://other.example".into();
        let said = plan(&elsewhere, &arriving).unwrap_err().to_string();
        assert!(
            said.contains("belongs to the community at wss://relay.example/"),
            "{said}"
        );
    }

    #[test]
    fn the_settings_follow_what_the_agents_folder_says() {
        let made = made();
        let temporary = tempfile::TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        let mut config = made.config.clone();
        assert!(reconcile(&paths, &mut config).is_empty());
        assert_eq!(hold::read(&paths.data_dir, made.agent.id), None);
        // The folder says moved away, the settings do not: an older copy of
        // the settings was written back.
        hold::mark_moved_away(&paths.data_dir, made.agent.id, Destination::LaunchKit).unwrap();
        config.hosted_agents[0].paused = true;
        assert_eq!(reconcile(&paths, &mut config), ["Scout"]);
        assert_eq!(manage::state_of(&config.hosted_agents[0]), "moved away");
        assert!(
            moved_line(&paths.data_dir, made.agent.id)
                .unwrap()
                .contains("as a launch kit, for a machine with Buzz's buzz-acp")
        );
        // The settings say moved away, the folder does not: from now on the
        // folder says it too, so that a running bzz sees it.
        hold::clear(&paths.data_dir, made.agent.id);
        assert!(reconcile(&paths, &mut config).is_empty());
        assert!(hold::moved_away(&paths.data_dir, made.agent.id));
        // A move at work changes nothing.
        let mut active = made.config.clone();
        hold::clear(&paths.data_dir, made.agent.id);
        let held = hold::take(&paths.data_dir, made.agent.id).unwrap();
        assert!(reconcile(&paths, &mut active).is_empty());
        assert_eq!(manage::state_of(&active.hosted_agents[0]), "active");
        assert!(
            moved_line(&paths.data_dir, made.agent.id)
                .unwrap()
                .contains("being moved by bzz process")
        );
        drop(held);
        assert_eq!(moved_line(&paths.data_dir, made.agent.id), None);
    }
}
