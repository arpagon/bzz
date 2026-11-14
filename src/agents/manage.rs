//! Creating and managing a local agent (plan v0.15.0, M4): the questions an
//! owner answers, the records Buzz needs to see the agent as an agent, and
//! the acts of its life (add to a channel, edit, pause, delete).
//!
//! What Buzz 1.0 does, bzz does with the same records (ADR v0.15 local
//! agents, decision 11):
//!
//! - the **agent's profile** (kind 0), signed by the agent: its name,
//!   `"bot": true`, a picture when it has one, and the owner's tag;
//! - the **owner's record** (kind 30177), signed by the owner: `d` is the
//!   agent's key, `runtime` is `bzz/<harness>`, and the content holds the
//!   name, the picture, `parallelism`, and who may wake it in Buzz's own
//!   words (`respond_to`);
//! - **adding to a channel** (kind 9000), by the owner: a member, with the
//!   `bot` role only in a Solo chat (D13).
//!
//! Creation is all or nothing. Used by `bzz agent …` and by the TUI.

use std::{path::Path, sync::Arc, time::Duration};

use async_trait::async_trait;
use nostr::{Event, EventBuilder, Kind, PublicKey, Tag};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    agents::{
        capability,
        runtime::{
            admission::{ChannelFacts, PlaceKind},
            agent::Environment,
            install::{self, ModelCheck},
            process,
            relay::{self, Published},
        },
    },
    auth::{federated::EvidenceSlot, signer::SignerHandle},
    broker::custody,
    config::{
        AgentModelConfig, CommunityConfig, Config, ExecGrant, FolderGrant, Harness,
        HostedAgentConfig, IdentityConfig, KeyBackend, LocalAgentConfig, LocalAgentState, WakeRule,
        validate_relay_url,
    },
    diagnostics::DiagnosticHandle,
    error::{Error, Result},
    paths::Paths,
    protocol::{events, http::HttpClient},
    realtime::supervisor::SupervisorHandle,
    store::{agent_runtime::RecentTurn, writer::StoreHandle},
};

/// The owner's record of an agent (NIP-AP managed agent).
pub const KIND_OWNER_RECORD: u16 = 30_177;
/// The most characters of an agent's name, as its label in bzz.
pub const MAX_NAME_CHARS: usize = 64;
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(15);

/// The `runtime` tag of the owner's record: `bzz/<harness>`. Buzz 1.0 reads
/// it only when it matches `^[a-z0-9][a-z0-9._-]*/[a-z0-9][a-z0-9._-]*$`.
pub fn runtime_tag(harness: Harness) -> String {
    format!("bzz/{}", harness.as_str())
}

/// Who may wake the agent, in the words of Buzz's `respond_to`.
pub const fn respond_to(wake: WakeRule) -> &'static str {
    match wake {
        WakeRule::OwnerOnly => "owner-only",
        WakeRule::Allowlist => "allowlist",
        WakeRule::Anyone => "anyone",
    }
}

// ---- what the owner is asked ------------------------------------------------

/// The answers to the questions of `agent new`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAgent {
    pub name: String,
    pub harness: Harness,
    pub model: Option<AgentModelConfig>,
    pub instructions: String,
    pub wake: WakeRule,
    pub wake_allowlist: Vec<String>,
    pub folders: Vec<FolderGrant>,
    pub network: Vec<String>,
    pub picture: Option<String>,
    /// Whether its host loads the Pi packages of Pi's settings.
    pub packages: bool,
}

impl NewAgent {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.trim().to_owned(),
            harness: Harness::PiDurable,
            model: None,
            instructions: String::new(),
            wake: WakeRule::OwnerOnly,
            wake_allowlist: Vec::new(),
            folders: Vec::new(),
            network: Vec::new(),
            picture: None,
            packages: true,
        }
    }

    fn settings(&self) -> LocalAgentConfig {
        LocalAgentConfig {
            harness: self.harness,
            wake: self.wake,
            wake_allowlist: self.wake_allowlist.clone(),
            model: self.model.clone(),
            instructions: self.instructions.clone(),
            folders: self.folders.clone(),
            network: self.network.clone(),
            state: LocalAgentState::Active,
            command: Vec::new(),
            picture: self.picture.clone(),
            packages: self.packages,
        }
    }
}

/// The answers as an owner writes them, before they are read: from the
/// flags and questions of `bzz agent new`, or from the form of `:agent new`.
/// Both ask the same things and read them the same way.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Answers {
    pub name: String,
    /// Empty: `pi-durable`.
    pub harness: String,
    /// `provider/model`. Empty: the harness's own default.
    pub model: String,
    pub thinking: Option<String>,
    pub instructions: String,
    /// Empty: the owner only, or the list when people are named.
    pub wake: String,
    pub allow: Vec<String>,
    pub folders: Vec<String>,
    pub hosts: Vec<String>,
    /// Empty: none.
    pub picture: String,
    /// `on` or `off`. Empty: on.
    pub packages: String,
}

/// The questions, in the order they are asked, each with what to answer.
/// The community comes first and is not in the list: a command asks for it,
/// and the TUI creates an agent in the community that is open.
pub const QUESTIONS: [(&str, &str); 10] = [
    ("Name", "as Buzz will show it"),
    (
        "Harness",
        "pi-durable (grok, codex and claude-code are not available in this version yet)",
    ),
    (
        "Model",
        "provider/model as Pi names them; empty: Pi's own default",
    ),
    (
        "Instructions",
        "one line; a longer text: `bzz agent edit <name> --instructions-file <file>`",
    ),
    (
        "Who may wake it",
        "owner-only, allowlist or anyone; empty: owner-only",
    ),
    (
        "People who may wake it",
        "npub or hex, with commas; only for an allowlist",
    ),
    (
        "Folders it may read",
        "name=/path; add :rw to let it write, :exec to let it run real commands there, each asked of you first (:exec-without-asking: never asked); with commas; empty: none",
    ),
    (
        "Hosts it may reach",
        "for example github.com; with commas; empty: none",
    ),
    ("Picture", "an HTTPS address; empty: none"),
    (
        "Pi packages",
        "on or off: a package named in Pi's settings runs inside the agent's host, with your rights; empty: on",
    ),
];

impl Answers {
    /// Reads the answers. Every refusal says what to write instead.
    pub fn read(&self) -> Result<NewAgent> {
        let mut new = NewAgent::named(&self.name);
        new.harness = parse_harness(if self.harness.trim().is_empty() {
            "pi-durable"
        } else {
            &self.harness
        })?;
        new.model = parse_model(&self.model, self.thinking.as_deref())?;
        new.instructions = self.instructions.trim().to_owned();
        new.wake = if self.wake.trim().is_empty() && !self.allow.is_empty() {
            WakeRule::Allowlist
        } else {
            parse_wake(&self.wake)?
        };
        new.wake_allowlist = self
            .allow
            .iter()
            .map(|key| parse_pubkey(key))
            .collect::<Result<Vec<_>>>()?;
        new.wake_allowlist.sort();
        new.wake_allowlist.dedup();
        new.folders = self
            .folders
            .iter()
            .map(|folder| parse_folder(folder))
            .collect::<Result<Vec<_>>>()?;
        new.network = self
            .hosts
            .iter()
            .map(|host| parse_host(host))
            .collect::<Result<Vec<_>>>()?;
        new.network.sort();
        new.network.dedup();
        new.picture = parse_picture(&self.picture)?;
        new.packages = parse_switch(&self.packages)?.unwrap_or(true);
        validate_settings(&new.name, new.wake, &new.wake_allowlist, &new.folders)?;
        Ok(new)
    }
}

pub fn parse_harness(text: &str) -> Result<Harness> {
    match text.trim().to_ascii_lowercase().as_str() {
        "pi-durable" | "pi" => Ok(Harness::PiDurable),
        "grok" => Ok(Harness::Grok),
        "codex" => Ok(Harness::Codex),
        "claude-code" | "claude" => Ok(Harness::ClaudeCode),
        other => Err(Error::Config(format!(
            "unknown harness {other:?}: choose pi-durable, grok, codex or claude-code"
        ))),
    }
}

pub fn parse_wake(text: &str) -> Result<WakeRule> {
    match text.trim().to_ascii_lowercase().as_str() {
        "owner-only" | "owner" | "" => Ok(WakeRule::OwnerOnly),
        "allowlist" | "list" => Ok(WakeRule::Allowlist),
        "anyone" => Ok(WakeRule::Anyone),
        other => Err(Error::Config(format!(
            "unknown rule {other:?}: choose owner-only, allowlist or anyone"
        ))),
    }
}

/// `on` or `off`. Empty: no answer.
pub fn parse_switch(text: &str) -> Result<Option<bool>> {
    match text.trim().to_ascii_lowercase().as_str() {
        "" => Ok(None),
        "on" | "yes" | "true" => Ok(Some(true)),
        "off" | "no" | "false" => Ok(Some(false)),
        other => Err(Error::Config(format!("{other:?} is neither on nor off"))),
    }
}

/// `provider/model`, as Pi names them. The model's own name may hold `/`.
/// Empty: the harness's own default.
pub fn parse_model(text: &str, thinking: Option<&str>) -> Result<Option<AgentModelConfig>> {
    let text = text.trim();
    let thinking = thinking
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .map(str::to_owned);
    if text.is_empty() {
        return match thinking {
            None => Ok(None),
            Some(_) => Err(Error::Config(
                "a thinking level needs a model: give --model provider/model too".into(),
            )),
        };
    }
    let plain = |value: &str| {
        !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_whitespace)
    };
    match text.split_once('/') {
        Some((provider, model)) if plain(provider) && plain(model) => Ok(Some(AgentModelConfig {
            provider: provider.to_owned(),
            model_id: model.to_owned(),
            thinking_level: thinking,
        })),
        _ => Err(Error::Config(format!(
            "a model is written provider/model, as Pi names them (for example openai-codex/gpt-6-luna); got {text:?}"
        ))),
    }
}

/// One person: an `npub…` or 64 hex characters. Returns hex.
pub fn parse_pubkey(text: &str) -> Result<String> {
    PublicKey::parse(text.trim())
        .map(|key| key.to_hex())
        .map_err(|_| {
            Error::Config(format!(
                "{:?} is not a public key: give an npub or 64 hex characters",
                text.trim()
            ))
        })
}

/// A list written with commas or spaces.
pub fn split_list(text: &str) -> Vec<&str> {
    text.split([',', ' ', '\n', '\t'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

/// A list written with commas only, for values that may hold a space.
pub fn split_commas(text: &str) -> Vec<String> {
    text.split([',', '\n'])
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

fn simple_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

/// A folder grant: `name=/path`, or a path alone, whose last part is then
/// the name. After it, any of `:rw` (the agent's file tools may write
/// there; `:ro` says they may not), `:exec` (real commands may run there,
/// each asked of the owner first) and `:exec-without-asking` (they run
/// without the owner being asked). The agent sees the folder as
/// `/mnt/<name>`.
pub fn parse_folder(text: &str) -> Result<FolderGrant> {
    let text = text.trim();
    let (mut rest, mut write, mut exec) = (text, false, ExecGrant::No);
    // The words are read from the end, so a path may hold a colon.
    while let Some((before, word)) = rest.rsplit_once(':') {
        match word {
            "rw" => write = true,
            "ro" => write = false,
            "exec" => exec = ExecGrant::Ask,
            "exec-without-asking" => exec = ExecGrant::WithoutAsking,
            _ => break,
        }
        rest = before;
    }
    let (name, path) = match rest.split_once('=') {
        Some((name, path)) => (name.trim().to_owned(), path.trim()),
        None => (
            Path::new(rest)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            rest,
        ),
    };
    let path = Path::new(path);
    if !simple_name(&name) {
        return Err(Error::Config(format!(
            "the folder {text:?} needs a simple name (letters, digits, - and _, starting with a letter or a digit): write name=/path"
        )));
    }
    if !path.is_absolute() {
        return Err(Error::Config(format!(
            "the folder {name:?} needs an absolute path, got {:?}",
            path.display()
        )));
    }
    if !path.is_dir() {
        return Err(Error::Config(format!(
            "the folder {} does not exist on this machine",
            path.display()
        )));
    }
    Ok(FolderGrant {
        name,
        path: path.to_path_buf(),
        write,
        exec,
    })
}

/// A folder grant as the owner would write it, for `--folder`.
pub fn folder_text(folder: &FolderGrant) -> String {
    format!(
        "{}={}{}{}",
        folder.name,
        folder.path.display(),
        if folder.write { ":rw" } else { "" },
        match folder.exec {
            ExecGrant::No => "",
            ExecGrant::Ask => ":exec",
            ExecGrant::WithoutAsking => ":exec-without-asking",
        }
    )
}

/// What a grant lets the agent do in the folder, in the owner's words.
pub fn folder_rights(folder: &FolderGrant) -> String {
    format!(
        "{}{}",
        if folder.write {
            "read and write"
        } else {
            "read-only"
        },
        match folder.exec {
            ExecGrant::No => "",
            ExecGrant::Ask => "; real commands, each asked of you first",
            ExecGrant::WithoutAsking => "; real commands, WITHOUT asking you",
        }
    )
}

/// A host the agent may reach: `host` or `host:port`. Never a URL or a
/// pattern: the host's allow-list is exact.
pub fn parse_host(text: &str) -> Result<String> {
    let text = text.trim().to_ascii_lowercase();
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (text.as_str(), None),
    };
    let label = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    let host_ok = !host.is_empty() && host.len() <= 253 && host.split('.').all(label);
    let port_ok = port.is_none_or(|port| port.parse::<u16>().is_ok_and(|port| port > 0));
    if host_ok && port_ok {
        Ok(text)
    } else {
        Err(Error::Config(format!(
            "{text:?} is not a host: write a name such as github.com, or host:port; not a URL and not a pattern"
        )))
    }
}

/// A picture as Buzz's record carries it: an HTTPS URL.
pub fn parse_picture(text: &str) -> Result<Option<String>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match url::Url::parse(text) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some() && text.len() <= 2048 => {
            Ok(Some(url.to_string()))
        }
        _ => Err(Error::Config(
            "the picture must be an HTTPS address of an image".into(),
        )),
    }
}

fn valid_name(name: &str) -> Result<()> {
    let count = name.chars().count();
    if name.trim() != name
        || count == 0
        || count > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(Error::Config(format!(
            "an agent's name is 1 to {MAX_NAME_CHARS} characters on one line"
        )));
    }
    Ok(())
}

/// Checks the settings of an agent, whatever asked for them.
pub fn validate_settings(
    name: &str,
    wake: WakeRule,
    wake_allowlist: &[String],
    folders: &[FolderGrant],
) -> Result<()> {
    valid_name(name)?;
    match (wake, wake_allowlist.is_empty()) {
        (WakeRule::Allowlist, true) => {
            return Err(Error::Config(
                "an allowlist needs at least one person: name who may wake the agent".into(),
            ));
        }
        (WakeRule::OwnerOnly | WakeRule::Anyone, false) => {
            return Err(Error::Config(
                "people were listed, but the rule is not `allowlist`".into(),
            ));
        }
        _ => {}
    }
    if wake_allowlist.len() > 256 {
        return Err(Error::Config(
            "an allowlist holds 256 people at most".into(),
        ));
    }
    let mut names = std::collections::HashSet::new();
    for folder in folders {
        if !names.insert(folder.name.as_str()) {
            return Err(Error::Config(format!(
                "two folders are both named {:?}",
                folder.name
            )));
        }
        // Whoever may wake the agent makes it run commands, as its owner.
        if wake == WakeRule::Anyone && folder.exec == ExecGrant::WithoutAsking {
            return Err(Error::Config(format!(
                "the folder {:?} is allowed for commands without asking, and anyone may wake this agent: anyone in its channels could then run commands as you. Grant the folder with `:exec` (you are asked each time), or let only you or listed people wake the agent",
                folder.name
            )));
        }
    }
    Ok(())
}

/// Whether another agent of the configuration already has this name.
pub fn name_taken(config: &Config, name: &str, except: Option<Uuid>) -> bool {
    config
        .hosted_agents
        .iter()
        .any(|agent| Some(agent.id) != except && agent.label.eq_ignore_ascii_case(name.trim()))
}

/// The local agent called `name`.
pub fn find<'a>(config: &'a Config, name: &str) -> Result<&'a HostedAgentConfig> {
    let wanted = name.trim();
    let mut matches = config
        .hosted_agents
        .iter()
        .filter(|agent| agent.label.eq_ignore_ascii_case(wanted) || agent.pubkey == wanted);
    match (matches.next(), matches.next()) {
        (Some(agent), None) if agent.local.is_some() => Ok(agent),
        (Some(_), None) => Err(Error::Config(format!(
            "{wanted} is an agent whose process you run yourself: use `bzz broker`"
        ))),
        (Some(_), Some(_)) => Err(Error::Config(format!(
            "more than one agent is called {wanted}: give its public key"
        ))),
        (None, _) => Err(Error::Config(format!(
            "no local agent is called {wanted}; `bzz agent list` shows them"
        ))),
    }
}

// ---- the records ------------------------------------------------------------

/// The agent's profile: its name, marked as a bot, as Buzz 1.0 writes it.
/// The caller adds the owner's tag and signs with the agent's key.
pub fn profile_builder(name: &str, picture: Option<&str>) -> EventBuilder {
    let mut content = json!({"name": name, "bot": true});
    if let Some(picture) = picture {
        content["picture"] = json!(picture);
    }
    EventBuilder::new(Kind::Metadata, content.to_string())
}

fn tag(parts: &[&str]) -> Tag {
    Tag::parse(parts.iter().copied()).expect("a tag of plain strings")
}

/// The owner's record of the agent, replacing `prior` when there is one. As
/// Buzz 1.0 does, it keeps every setting and tag of the record it replaces
/// that is not its own to set, and always carries `parallelism` and
/// `respond_to`, which Buzz's reader requires.
pub fn record_builder(
    agent: &HostedAgentConfig,
    settings: &LocalAgentConfig,
    prior: Option<&Event>,
) -> EventBuilder {
    let mut content = Map::new();
    content.insert("parallelism".into(), json!(1));
    if let Some(Value::Object(kept)) =
        prior.and_then(|event| serde_json::from_str::<Value>(&event.content).ok())
    {
        for (field, value) in kept {
            content.insert(field, value);
        }
    }
    // What bzz itself decides, whatever the record before said.
    content.remove("picture");
    content.remove("respond_to_allowlist");
    content.insert("respond_to".into(), json!(respond_to(settings.wake)));
    if settings.wake == WakeRule::Allowlist {
        content.insert(
            "respond_to_allowlist".into(),
            json!(settings.wake_allowlist),
        );
    }
    content.insert("name".into(), json!(agent.label));
    if let Some(picture) = &settings.picture {
        content.insert("picture".into(), json!(picture));
    }
    let mut tags = vec![
        tag(&["d", &agent.pubkey]),
        tag(&["runtime", &runtime_tag(settings.harness)]),
    ];
    if let Some(prior) = prior {
        tags.extend(
            prior
                .tags
                .iter()
                .filter(|tag| {
                    !matches!(
                        tag.as_slice().first().map(String::as_str),
                        Some("d" | "runtime" | "client-id") | None
                    )
                })
                .cloned(),
        );
    }
    EventBuilder::new(
        Kind::Custom(KIND_OWNER_RECORD),
        Value::Object(content).to_string(),
    )
    .tags(tags)
}

/// The owner's removal of his record of the agent (NIP-09 by coordinate),
/// as Buzz 1.0 removes it.
pub fn record_removal_builder(owner: &str, agent: &str) -> EventBuilder {
    EventBuilder::new(Kind::Custom(5), "")
        .tags([tag(&["a", &format!("{KIND_OWNER_RECORD}:{owner}:{agent}")])])
}

/// The removal of one event by its author (NIP-09).
pub fn event_removal_builder(event: &Event) -> EventBuilder {
    EventBuilder::new(Kind::Custom(5), "").tags([tag(&["e", &event.id.to_hex()])])
}

// ---- the relay, as the owner ------------------------------------------------

/// Where the records of an agent are published. Tests replace it.
#[async_trait]
pub trait Publisher: Send + Sync {
    /// On the owner's connection.
    async fn as_owner(&self, event: Event) -> Published;
    /// On a connection the agent itself authenticates, which is also how the
    /// relay learns who owns it.
    async fn as_agent(
        &self,
        agent: &HostedAgentConfig,
        signer: &SignerHandle,
        event: Event,
    ) -> Published;
}

/// The owner's connection to a community's relay, for the acts that create
/// and manage an agent.
pub struct OwnerRelay {
    pub community: Uuid,
    pub owner_identity: Uuid,
    pub signer: SignerHandle,
    pub http: HttpClient,
    pub supervisor: SupervisorHandle,
    websocket: url::Url,
    diagnostics: DiagnosticHandle,
    /// The community's NIP-FA adapter, when the owner's connection carries
    /// federated-identity evidence.
    federated: Option<url::Url>,
    data_dir: std::path::PathBuf,
    /// Whether `close` ends the owner's connection (not when it is the
    /// TUI's own).
    owned: bool,
    agents: tokio::sync::Mutex<Vec<(Uuid, SupervisorHandle)>>,
}

impl OwnerRelay {
    /// Connects as the owner, for a command.
    pub async fn connect(
        paths: &Paths,
        community: &CommunityConfig,
        identity: &IdentityConfig,
        signer: SignerHandle,
    ) -> Result<Self> {
        let endpoint =
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
        let evidence = EvidenceSlot::default();
        if identity.backend == KeyBackend::BuzzDesktop {
            evidence.set(crate::auth::provider::ProviderEvidence::new(
                Arc::new(crate::auth::desktop_client(identity)?),
                crate::clock::system(),
            ));
        }
        let adapter = community
            .identity_adapter
            .as_deref()
            .map(url::Url::parse)
            .transpose()
            .map_err(|_| Error::Config("identity_adapter is not a URL".into()))?;
        let http = HttpClient::new(endpoint.http_base.clone(), signer.clone())?
            .with_evidence(evidence.clone());
        let mut federated = None;
        if let Some(adapter) = &adapter {
            let info = http.nip11().await?;
            match crate::auth::federated::install_from_store(
                &info,
                Some(adapter),
                &paths.data_dir,
                identity.id,
                &signer,
                &evidence,
            )
            .await
            {
                crate::auth::federated::Startup::Ready => federated = Some(adapter.clone()),
                crate::auth::federated::Startup::NeedsSignIn => {
                    return Err(Error::Auth(
                        "this community needs a sign-in first: open bzz and run :signin".into(),
                    ));
                }
                _ => {}
            }
        }
        let diagnostics = DiagnosticHandle::disabled();
        let supervisor = SupervisorHandle::spawn_with_evidence(
            endpoint.websocket.clone(),
            signer.clone(),
            diagnostics.clone(),
            evidence,
        );
        Ok(Self {
            community: community.id,
            owner_identity: identity.id,
            signer,
            http,
            supervisor,
            websocket: endpoint.websocket,
            diagnostics,
            federated,
            data_dir: paths.data_dir.clone(),
            owned: true,
            agents: tokio::sync::Mutex::new(Vec::new()),
        })
    }

    /// The owner's connection the TUI already holds.
    #[allow(clippy::too_many_arguments)]
    pub fn from_runtime(
        paths: &Paths,
        community: &CommunityConfig,
        owner_identity: Uuid,
        signer: SignerHandle,
        http: HttpClient,
        supervisor: SupervisorHandle,
        federated: Option<url::Url>,
        diagnostics: DiagnosticHandle,
    ) -> Result<Self> {
        let endpoint =
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
        Ok(Self {
            community: community.id,
            owner_identity,
            signer,
            http,
            supervisor,
            websocket: endpoint.websocket,
            diagnostics,
            federated,
            data_dir: paths.data_dir.clone(),
            owned: false,
            agents: tokio::sync::Mutex::new(Vec::new()),
        })
    }

    /// Ends every connection this opened.
    pub async fn close(&self) {
        for (_, supervisor) in self.agents.lock().await.drain(..) {
            supervisor.shutdown().await;
        }
        if self.owned {
            self.supervisor.shutdown().await;
        }
    }

    async fn relay_pubkey(&self, store: &StoreHandle) -> Result<String> {
        crate::agents::runtime::service::relay_pubkey(store, self.community, &self.http).await
    }

    /// A channel by the relay's own signed records, with those records.
    pub async fn channel(
        &self,
        store: &StoreHandle,
        channel: Uuid,
    ) -> Result<(ChannelFacts, Vec<Event>)> {
        let relay_pubkey = self.relay_pubkey(store).await?;
        let id = channel.to_string();
        let records = self
            .http
            .query_raw(&[
                json!({"kinds": [39_000], "#d": [id], "limit": 2}),
                json!({"kinds": [39_002], "#d": [id], "limit": 2}),
            ])
            .await?;
        let facts = relay::channel_facts(channel, &relay_pubkey, &records).ok_or_else(|| {
            Error::Access(format!(
                "there is no channel {channel} that you can see in this community"
            ))
        })?;
        Ok((facts, records))
    }

    /// The channel the owner means: its id, or its name as bzz or the relay
    /// knows it.
    pub async fn find_channel(&self, store: &StoreHandle, wanted: &str) -> Result<Uuid> {
        let wanted = wanted.trim().trim_start_matches('#').to_owned();
        if let Ok(id) = Uuid::parse_str(&wanted) {
            return Ok(id);
        }
        let community = self.community;
        let name = wanted.clone();
        let mut ids = store
            .call(move |store| store.channels_named(community, &name))
            .await?;
        if ids.is_empty() {
            // A profile that never opened the TUI knows no channel yet.
            let relay_pubkey = self.relay_pubkey(store).await?;
            let records = self
                .http
                .query_raw(&[json!({"kinds": [39_000], "limit": 1000})])
                .await?;
            ids = records
                .iter()
                .filter(|event| {
                    event.pubkey.to_hex() == relay_pubkey
                        && events::verify(event).is_ok()
                        && events::first_tag(event, "name")
                            .is_some_and(|name| name.eq_ignore_ascii_case(&wanted))
                })
                .filter_map(|event| events::first_tag(event, "d"))
                .filter_map(|id| Uuid::parse_str(&id).ok())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        match ids.as_slice() {
            [id] => Ok(*id),
            [] => Err(Error::Config(format!(
                "no channel is called {wanted}: give its name as the sidebar shows it, or its id"
            ))),
            _ => Err(Error::Config(format!(
                "more than one channel is called {wanted}: give its id"
            ))),
        }
    }

    /// The owner's current record of `agent` on the relay, if any.
    pub async fn owner_record(&self, agent: &str) -> Result<Option<Event>> {
        let owner = self.signer.public_key().to_hex();
        let records = self
            .http
            .query_raw(&[json!({
                "kinds": [KIND_OWNER_RECORD], "authors": [owner], "#d": [agent], "limit": 4,
            })])
            .await?;
        Ok(records
            .into_iter()
            .filter(|event| {
                event.kind.as_u16() == KIND_OWNER_RECORD
                    && event.pubkey.to_hex() == owner
                    && events::first_tag(event, "d").as_deref() == Some(agent)
                    && events::verify(event).is_ok()
            })
            .max_by_key(|event| (event.created_at, event.id.to_hex())))
    }

    /// Every channel the relay lists `agent` in, that the owner can see.
    pub async fn channels_of(&self, agent: &str) -> Result<Vec<Uuid>> {
        let lists = self
            .http
            .query_raw(&[json!({"kinds": [39_002], "#p": [agent], "limit": 500})])
            .await?;
        Ok(lists
            .iter()
            .filter(|event| {
                events::tag_values(event, "p")
                    .iter()
                    .any(|member| member.eq_ignore_ascii_case(agent))
            })
            .filter_map(|event| events::first_tag(event, "d"))
            .filter_map(|id| Uuid::parse_str(&id).ok())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    async fn agent_connection(
        &self,
        agent: &HostedAgentConfig,
        signer: &SignerHandle,
    ) -> Result<SupervisorHandle> {
        let mut agents = self.agents.lock().await;
        if let Some((_, supervisor)) = agents.iter().find(|(id, _)| *id == agent.id) {
            return Ok(supervisor.clone());
        }
        // NIP-FI communities: the agent proves itself with its own
        // owner-authorized grant, never with the owner's session.
        let evidence = EvidenceSlot::default();
        if let Some(adapter) = &self.federated {
            match crate::auth::federated::agent_evidence(
                adapter,
                &self.data_dir,
                self.owner_identity,
                &self.signer,
                signer,
                &agent.auth_tag,
            )
            .await?
            {
                Some(source) => evidence.set(source),
                None => {
                    return Err(Error::Auth(
                        "sign in (:signin) to enroll agents with the identity adapter".into(),
                    ));
                }
            }
        }
        let supervisor = SupervisorHandle::spawn_with_evidence(
            self.websocket.clone(),
            signer.clone(),
            self.diagnostics.clone(),
            evidence,
        );
        agents.push((agent.id, supervisor.clone()));
        Ok(supervisor)
    }
}

#[async_trait]
impl Publisher for OwnerRelay {
    async fn as_owner(&self, event: Event) -> Published {
        relay::publish_on(&self.supervisor, event, PUBLISH_TIMEOUT).await
    }

    async fn as_agent(
        &self,
        agent: &HostedAgentConfig,
        signer: &SignerHandle,
        event: Event,
    ) -> Published {
        match self.agent_connection(agent, signer).await {
            Ok(supervisor) => relay::publish_on(&supervisor, event, PUBLISH_TIMEOUT).await,
            Err(error) => Published::Refused(error.to_string()),
        }
    }
}

// ---- before anything is made ------------------------------------------------

/// What the checks before a creation found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Preflight {
    /// Lines for the owner: the Node in use, the model that will answer.
    pub notes: Vec<String>,
}

/// The checks that can fail before anything exists: whether the harness is
/// offered, and for Pi Durable Node, the install of the host (made now when
/// this is its first use) and the model when one is named.
pub async fn preflight(
    data_dir: &Path,
    new: &NewAgent,
    pi_dir: Option<&Path>,
    environment: &Environment,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Preflight> {
    let record = capability::record(new.harness);
    if !record.available {
        return Err(Error::Unsupported(format!(
            "the {} harness is not available in this version yet; choose pi-durable",
            new.harness.as_str()
        )));
    }
    let mut notes = Vec::new();
    let lookup = |name: &str| environment(name);
    let node = install::find_node(lookup)
        .await
        .map_err(|problem| Error::Config(problem.to_string()))?;
    notes.push(format!(
        "Node.js {} at {}",
        node.version_text(),
        node.program.display()
    ));
    if !install::is_installed(data_dir) {
        progress("installing the Pi Durable host (first use; this needs the npm registry)…");
    }
    let host = install::ensure_installed(data_dir, &node, lookup)
        .await
        .map_err(|problem| Error::Config(problem.to_string()))?;
    if let Some(model) = &new.model {
        let Some(pi_dir) = pi_dir else {
            return Err(Error::Config(
                "Pi's folder could not be found, so the model cannot be checked".into(),
            ));
        };
        match install::check_model(&node, &host, pi_dir, Some(model), new.packages, lookup).await {
            ModelCheck::Available {
                provider,
                model_id,
                thinking_level,
            } => notes.push(format!(
                "model {provider}/{model_id}{}, with Pi's own sign-in",
                thinking_level
                    .map(|level| format!(" (thinking: {level})"))
                    .unwrap_or_default()
            )),
            ModelCheck::Unavailable {
                kind,
                message,
                package,
            } => {
                return Err(Error::Config(format!(
                    "the model {}/{} cannot be used ({kind}): {message}{}",
                    model.provider,
                    model.model_id,
                    missing_provider_note(&kind, package.as_deref(), new.packages)
                )));
            }
            ModelCheck::Unknown(why) => {
                return Err(Error::Config(format!(
                    "the model {}/{} could not be checked: {why}",
                    model.provider, model.model_id
                )));
            }
        }
    } else {
        notes.push("model: Pi's own default, read when the agent is first asked".into());
    }
    // What the answers give the agent beyond its own disk, said before the
    // owner confirms: where it may run real commands and whether he is
    // asked, and whether Pi packages are loaded.
    notes.extend(capability::record_for(&new.settings()).notes);
    Ok(Preflight { notes })
}

/// What an owner can do about a provider that is missing: the host says
/// whether a Pi package of Pi's settings names it without a Pi Durable
/// entry (`package`), and bzz says what that means.
pub fn missing_provider_note(kind: &str, package: Option<&str>, packages: bool) -> String {
    if kind != "provider_unavailable" {
        return String::new();
    }
    match (package, packages) {
        (Some(package), _) => format!(
            " bzz found the Pi package {package} in Pi's settings, which names this provider but has no Pi Durable entry (\"pi\": {{\"durable\": [...]}} in its package.json): the `pi` command can use it, bzz's host cannot, until the package adds one."
        ),
        (None, true) => " bzz found no Pi package in Pi's settings that names this provider. A provider that comes from a Pi extension needs a Pi Durable entry in that package.".into(),
        (None, false) => " Pi packages are switched off for this agent, so no package's provider was looked for.".into(),
    }
}

// ---- creation ---------------------------------------------------------------

/// What a creation or an edit needs of the owner and the machine.
pub struct OwnerContext<'a> {
    pub paths: &'a Paths,
    pub owner: &'a SignerHandle,
    pub owner_identity: Uuid,
    pub community: Uuid,
    /// The community's NIP-FA adapter, when agents enroll with one.
    pub federated: Option<&'a url::Url>,
}

#[derive(Clone, Debug)]
pub struct Created {
    pub agent: HostedAgentConfig,
    pub profile: Event,
    pub record: Event,
}

/// A creation that did not happen: why, and anything it could not undo.
#[derive(Debug)]
pub struct CreateFailure {
    pub error: Error,
    /// Empty when nothing of the attempt is left anywhere.
    pub left: Vec<String>,
}

impl std::fmt::Display for CreateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "the agent was not created: {}", self.error)?;
        if self.left.is_empty() {
            write!(
                formatter,
                ". Nothing was left: no key, no folder, no settings, nothing on the relay"
            )
        } else {
            write!(
                formatter,
                ". Its key, folder and settings were removed, but this could not be: {}",
                self.left.join("; ")
            )
        }
    }
}

impl From<Error> for CreateFailure {
    fn from(error: Error) -> Self {
        Self {
            error,
            left: Vec::new(),
        }
    }
}

/// What a creation has made so far, to undo it.
struct Undo<'a> {
    context: &'a OwnerContext<'a>,
    publisher: &'a dyn Publisher,
    agent: HostedAgentConfig,
    signer: Option<SignerHandle>,
    in_config: bool,
    /// The profile reached the relay, or may have.
    profile: Option<Event>,
    /// The owner's record may have reached the relay.
    record: bool,
}

impl Undo<'_> {
    /// Takes back everything, the published records first while their keys
    /// are still here. Returns what could not be taken back.
    async fn run(self, config: &mut Config) -> Vec<String> {
        let mut left = Vec::new();
        let owner = self.context.owner.public_key().to_hex();
        if self.record {
            let removed = match self
                .context
                .owner
                .sign(record_removal_builder(&owner, &self.agent.pubkey))
                .await
            {
                Ok(removal) => self.publisher.as_owner(removal).await,
                Err(error) => Published::Refused(error.to_string()),
            };
            if removed != Published::Accepted {
                left.push(format!(
                    "your record of the agent (kind {KIND_OWNER_RECORD}, d={}) may still be on the relay",
                    self.agent.pubkey
                ));
            }
        }
        if let Some(profile) = &self.profile {
            let removed = match (&self.signer, auth_tag(&self.agent)) {
                (Some(signer), Ok(tag)) => {
                    match signer.sign(event_removal_builder(profile).tag(tag)).await {
                        Ok(removal) => self.publisher.as_agent(&self.agent, signer, removal).await,
                        Err(error) => Published::Refused(error.to_string()),
                    }
                }
                _ => Published::Refused("the agent's key is not open".into()),
            };
            if removed != Published::Accepted {
                left.push(format!(
                    "the agent's profile (event {}) may still be on the relay",
                    profile.id.to_hex()
                ));
            }
        }
        if let Some(signer) = &self.signer {
            signer.lock().await;
        }
        if let Some(adapter) = self.context.federated
            && (self.profile.is_some() || self.record)
            && crate::auth::federated::revoke_agent(
                adapter,
                &self.context.paths.data_dir,
                self.context.owner_identity,
                self.context.owner,
                &self.agent.pubkey,
            )
            .await
            .is_err()
        {
            left.push("the agent's grant with the identity adapter".into());
        }
        if self.in_config {
            config
                .hosted_agents
                .retain(|entry| entry.id != self.agent.id);
            if config.save(self.context.paths).is_err() {
                left.push(format!(
                    "the entry of {} in {}",
                    self.agent.label,
                    self.context.paths.config_file().display()
                ));
            }
        }
        let folder = process::agent_dir(&self.context.paths.data_dir, self.agent.id);
        if folder.exists() && std::fs::remove_dir_all(&folder).is_err() {
            left.push(format!("the folder {}", folder.display()));
        }
        if custody::delete(&self.context.paths.keys_dir(), &self.agent).is_err() {
            left.push("the agent's sealed key".into());
        }
        let _ = crate::agents::git_credentials::remove_all(
            &self.context.paths.keys_dir(),
            self.agent.id,
        );
        left
    }
}

fn auth_tag(agent: &HostedAgentConfig) -> Result<Tag> {
    crate::broker::relay::auth_tag(&agent.auth_tag)
}

/// Creates an agent, all or nothing, in this order: the key and the owner's
/// authorization, the agent's folder, its entry in the settings, its profile
/// marked as a bot, and the owner's record of it. A failure at any step
/// takes back the steps before it. The checks of [`preflight`] and the
/// owner's confirmation come before this.
pub async fn create(
    context: &OwnerContext<'_>,
    config: &mut Config,
    new: &NewAgent,
    publisher: &dyn Publisher,
) -> std::result::Result<Created, CreateFailure> {
    validate_settings(&new.name, new.wake, &new.wake_allowlist, &new.folders)?;
    if name_taken(config, &new.name, None) {
        return Err(Error::Config(format!("an agent named {} exists", new.name)).into());
    }
    if !capability::record(new.harness).available {
        return Err(Error::Unsupported(format!(
            "the {} harness is not available in this version yet",
            new.harness.as_str()
        ))
        .into());
    }
    let keys_dir = context.paths.keys_dir();
    // The key, sealed to the owner, and the owner's authorization for it.
    let (mut agent, _credential) = custody::create(
        &keys_dir,
        context.owner,
        context.owner_identity,
        context.community,
        &new.name,
    )
    .await?;
    // bzz itself runs this agent: it gets its tools over the pipe and never
    // uses the broker or its credential.
    agent.allowed.clear();
    agent.local = Some(new.settings());
    let settings = new.settings();
    let mut undo = Undo {
        context,
        publisher,
        agent: agent.clone(),
        signer: None,
        in_config: false,
        profile: None,
        record: false,
    };
    macro_rules! fail {
        ($error:expr) => {{
            let error = $error;
            let left = undo.run(config).await;
            return Err(CreateFailure { error, left });
        }};
    }
    // Its folder.
    let folder = process::agent_dir(&context.paths.data_dir, agent.id);
    if let Err(error) = process::create_private_dir(&folder) {
        fail!(error);
    }
    // Its entry in the settings, before anything is published: bzz holds
    // the key of every agent the relay has heard of.
    config.hosted_agents.push(agent.clone());
    if let Err(error) = config.save(context.paths) {
        config.hosted_agents.retain(|entry| entry.id != agent.id);
        fail!(error);
    }
    undo.in_config = true;
    // Its profile, signed by the agent on its own connection.
    let signer = match custody::unlock(&keys_dir, context.owner, &agent).await {
        Ok(signer) => signer,
        Err(error) => fail!(error),
    };
    undo.signer = Some(signer.clone());
    let profile = match auth_tag(&agent) {
        Ok(tag) => {
            signer
                .sign(profile_builder(&agent.label, settings.picture.as_deref()).tag(tag))
                .await
        }
        Err(error) => Err(error),
    };
    let profile = match profile {
        Ok(profile) => profile,
        Err(error) => fail!(error),
    };
    match publisher.as_agent(&agent, &signer, profile.clone()).await {
        Published::Accepted => undo.profile = Some(profile.clone()),
        Published::Refused(why) => {
            fail!(Error::Protocol(format!(
                "the relay refused the agent's profile: {why}"
            )));
        }
        Published::Unknown => {
            undo.profile = Some(profile.clone());
            fail!(Error::Timeout(
                "the relay did not confirm the agent's profile".into()
            ));
        }
    }
    // The owner's record, which is what Buzz lists an agent from.
    let record = match context
        .owner
        .sign(record_builder(&agent, &settings, None))
        .await
    {
        Ok(record) => record,
        Err(error) => fail!(error),
    };
    match publisher.as_owner(record.clone()).await {
        Published::Accepted => {}
        Published::Refused(why) => {
            fail!(Error::Protocol(format!(
                "the relay refused your record of the agent: {why}"
            )));
        }
        Published::Unknown => {
            undo.record = true;
            fail!(Error::Timeout(
                "the relay did not confirm your record of the agent".into()
            ));
        }
    }
    signer.lock().await;
    Ok(Created {
        agent,
        profile,
        record,
    })
}

// ---- its life ---------------------------------------------------------------

/// What an edit changes. `None`: left as it is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Edit {
    pub name: Option<String>,
    pub model: Option<Option<AgentModelConfig>>,
    pub instructions: Option<String>,
    pub wake: Option<WakeRule>,
    pub wake_allowlist: Option<Vec<String>>,
    pub folders: Option<Vec<FolderGrant>>,
    pub network: Option<Vec<String>>,
    pub picture: Option<Option<String>>,
    pub packages: Option<bool>,
}

/// Which records an edit must publish again.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Republish {
    pub profile: bool,
    pub record: bool,
}

impl Republish {
    pub const BOTH: Self = Self {
        profile: true,
        record: true,
    };

    pub const fn any(self) -> bool {
        self.profile || self.record
    }
}

/// Applies `edit` to the agent's settings in `config`, without saving, and
/// says which records now differ from what the relay holds.
pub fn apply_edit(config: &mut Config, id: Uuid, edit: &Edit) -> Result<Republish> {
    if let Some(name) = &edit.name
        && name_taken(config, name, Some(id))
    {
        return Err(Error::Config(format!("an agent named {name} exists")));
    }
    let agent = config
        .hosted_agents
        .iter_mut()
        .find(|agent| agent.id == id)
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    let mut local = agent
        .local
        .clone()
        .ok_or_else(|| Error::Config("this agent is not run by bzz".into()))?;
    let mut changed = Republish::default();
    let mut label = agent.label.clone();
    if let Some(name) = &edit.name
        && *name != label
    {
        label = name.clone();
        changed = Republish::BOTH;
    }
    if let Some(picture) = &edit.picture
        && *picture != local.picture
    {
        local.picture = picture.clone();
        changed = Republish::BOTH;
    }
    if let Some(wake) = edit.wake
        && wake != local.wake
    {
        local.wake = wake;
        changed.record = true;
        // A list belongs to the rule that reads it.
        if wake != WakeRule::Allowlist && edit.wake_allowlist.is_none() {
            local.wake_allowlist.clear();
        }
    }
    if let Some(allowlist) = &edit.wake_allowlist
        && *allowlist != local.wake_allowlist
    {
        local.wake_allowlist = allowlist.clone();
        changed.record = true;
    }
    if let Some(model) = &edit.model {
        local.model = model.clone();
    }
    if let Some(instructions) = &edit.instructions {
        local.instructions = instructions.clone();
    }
    if let Some(folders) = &edit.folders {
        local.folders = folders.clone();
    }
    if let Some(network) = &edit.network {
        local.network = network.clone();
    }
    if let Some(packages) = edit.packages {
        local.packages = packages;
    }
    validate_settings(&label, local.wake, &local.wake_allowlist, &local.folders)?;
    agent.label = label;
    agent.local = Some(local);
    Ok(changed)
}

/// Publishes again the agent's profile, the owner's record, or both, from
/// the settings as they are now. The record keeps what the one before it
/// held that is not bzz's to set.
pub async fn republish(
    context: &OwnerContext<'_>,
    agent: &HostedAgentConfig,
    what: Republish,
    relay: &OwnerRelay,
) -> Result<()> {
    let settings = agent
        .local
        .as_ref()
        .ok_or_else(|| Error::Config("this agent is not run by bzz".into()))?;
    let refused = |what: &str, published: Published| match published {
        Published::Accepted => Ok(()),
        Published::Refused(why) => Err(Error::Protocol(format!("the relay refused {what}: {why}"))),
        Published::Unknown => Err(Error::Timeout(format!("the relay did not confirm {what}"))),
    };
    if what.profile {
        let signer = custody::unlock(&context.paths.keys_dir(), context.owner, agent).await?;
        let published = match signer
            .sign(profile_builder(&agent.label, settings.picture.as_deref()).tag(auth_tag(agent)?))
            .await
        {
            Ok(profile) => relay.as_agent(agent, &signer, profile).await,
            Err(error) => Published::Refused(error.to_string()),
        };
        signer.lock().await;
        refused("the agent's profile", published)?;
    }
    if what.record {
        let prior = relay.owner_record(&agent.pubkey).await?;
        let record = context
            .owner
            .sign(record_builder(agent, settings, prior.as_ref()))
            .await?;
        refused("your record of the agent", relay.as_owner(record).await)?;
    }
    Ok(())
}

/// Pauses or resumes an agent in `config`, without saving. A paused agent
/// has no process and does not queue what it misses.
pub fn set_paused(config: &mut Config, id: Uuid, paused: bool) -> Result<()> {
    let agent = config
        .hosted_agents
        .iter_mut()
        .find(|agent| agent.id == id)
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    let local = agent
        .local
        .as_mut()
        .ok_or_else(|| Error::Config("this agent is not run by bzz".into()))?;
    if local.state == LocalAgentState::MovedAway {
        return Err(Error::Config(format!(
            "{} was moved away and runs elsewhere; bzz does not run it",
            agent.label
        )));
    }
    local.state = if paused {
        LocalAgentState::Paused
    } else {
        LocalAgentState::Active
    };
    // The v0.13 flag is cleared on resume, so that one act always resumes.
    agent.paused = false;
    Ok(())
}

/// How an agent was added to a channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Added {
    pub channel: Uuid,
    pub channel_name: String,
    pub kind: PlaceKind,
    /// The role asked for: `bot` in a Solo chat, none elsewhere (D13).
    pub role: Option<&'static str>,
    /// It was a member before; nothing was published.
    pub already: bool,
}

/// The owner's act that adds an agent to a channel, as Buzz 1.0 writes it:
/// a member, with the `bot` role only in a Solo chat.
pub fn add_member_builder(channel: Uuid, agent: &str, kind: PlaceKind) -> Result<EventBuilder> {
    let role = (kind == PlaceKind::Solo).then_some(buzz_sdk::MemberRole::Bot);
    buzz_sdk::build_add_member(channel, agent, role)
        .map_err(|error| Error::Protocol(error.to_string()))
}

/// Adds the agent to a channel as its owner and waits for the relay's own
/// member list to show it.
pub async fn add_to_channel(
    relay: &OwnerRelay,
    store: &StoreHandle,
    agent: &HostedAgentConfig,
    channel: Uuid,
) -> Result<Added> {
    let (facts, mut records) = relay.channel(store, channel).await?;
    if facts.kind == PlaceKind::Dm {
        return Err(Error::Config(
            "a direct message is not a channel an agent is added to: its owner opens one with it"
                .into(),
        ));
    }
    let is_member = |facts: &ChannelFacts| {
        facts
            .members
            .iter()
            .any(|member| member.eq_ignore_ascii_case(&agent.pubkey))
    };
    let mut added = Added {
        channel,
        channel_name: facts.name.clone(),
        kind: facts.kind,
        role: (facts.kind == PlaceKind::Solo).then_some("bot"),
        already: is_member(&facts),
    };
    if !added.already {
        let event = relay
            .signer
            .sign(add_member_builder(channel, &agent.pubkey, facts.kind)?)
            .await?;
        match relay.as_owner(event).await {
            Published::Accepted => {}
            Published::Refused(why) => {
                return Err(Error::Access(format!(
                    "the relay refused to add {} to {}: {why}",
                    agent.label, facts.name
                )));
            }
            Published::Unknown => {
                return Err(Error::Timeout(
                    "the relay did not confirm the addition; check the channel's members".into(),
                ));
            }
        }
        // One exact read of the member list, as Buzz confirms an addition.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let (now, fresh) = relay.channel(store, channel).await?;
            if is_member(&now) {
                added.channel_name = now.name;
                records = fresh;
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout(format!(
                    "the relay accepted the addition but does not list {} in {} yet",
                    agent.label, facts.name
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    // What `agent show` reads: the relay's own signed records, kept as any
    // other bzz keeps them.
    let community = relay.community;
    let _ = store
        .call(move |store| {
            for record in &records {
                let _ = store.apply_event(community, record);
            }
            Ok(())
        })
        .await;
    Ok(added)
}

/// What a deletion removed, and what it could not.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Removal {
    pub removed: Vec<String>,
    pub left: Vec<String>,
}

/// Whether a harness of this agent is running under some bzz right now.
pub fn running_pid(data_dir: &Path, agent: Uuid) -> Option<u32> {
    process::read_record(&process::run_dir(data_dir), agent)
        .filter(|record| process::is_alive(&record.process))
        .map(|record| record.process.pid)
}

/// Removes what the relay holds for the agent, in the order Buzz 1.0 does:
/// the owner's record of it first, which must go, and then its place in
/// every channel the owner can see it in, as well as can be done. The
/// agent's own messages and profile stay, as with Buzz.
pub async fn remove_from_relay(relay: &OwnerRelay, agent: &HostedAgentConfig) -> Removal {
    let mut removal = Removal::default();
    let owner = relay.signer.public_key().to_hex();
    let published = match relay
        .signer
        .sign(record_removal_builder(&owner, &agent.pubkey))
        .await
    {
        Ok(event) => relay.as_owner(event).await,
        Err(error) => Published::Refused(error.to_string()),
    };
    if published == Published::Accepted {
        removal
            .removed
            .push("your record of it on the relay".into());
    } else {
        removal.left.push(RECORD_LEFT.into());
        // Its channels are left alone: the deletion stops here and can be
        // tried again.
        return removal;
    }
    match relay.channels_of(&agent.pubkey).await {
        Ok(channels) => {
            for channel in channels {
                let published = match buzz_sdk::build_remove_member(channel, &agent.pubkey) {
                    Ok(builder) => match relay.signer.sign(builder).await {
                        Ok(event) => relay.as_owner(event).await,
                        Err(error) => Published::Refused(error.to_string()),
                    },
                    Err(error) => Published::Refused(error.to_string()),
                };
                if published == Published::Accepted {
                    removal
                        .removed
                        .push(format!("its place in channel {channel}"));
                } else {
                    removal.left.push(format!(
                        "its place in channel {channel} (remove it there as the channel's owner or admin)"
                    ));
                }
            }
        }
        Err(error) => removal.left.push(format!(
            "its places in channels (they could not be read: {error})"
        )),
    }
    removal
}

/// What a deletion reports when the owner's record could not be removed.
pub const RECORD_LEFT: &str = "your record of it on the relay (Buzz may still list it)";

/// Removes what this machine holds for the agent: its entry in the
/// settings, its sealed key, its folder with its memory and disk, and its
/// rows in bzz's database.
pub async fn remove_local(
    paths: &Paths,
    config: &mut Config,
    store: &StoreHandle,
    id: Uuid,
) -> Result<Removal> {
    let index = config
        .hosted_agents
        .iter()
        .position(|agent| agent.id == id)
        .ok_or_else(|| Error::Config("the agent no longer exists".into()))?;
    let agent = config.hosted_agents.remove(index);
    if let Err(error) = config.save(paths) {
        config.hosted_agents.insert(index, agent);
        return Err(error);
    }
    let mut removal = Removal::default();
    removal.removed.push("its settings".into());
    match custody::delete(&paths.keys_dir(), &agent) {
        Ok(()) => removal.removed.push("its key".into()),
        Err(error) => removal.left.push(format!("its sealed key ({error})")),
    }
    match crate::agents::git_credentials::remove_all(&paths.keys_dir(), id) {
        Ok(0) => {}
        Ok(count) => removal
            .removed
            .push(format!("its {count} stored git credential(s)")),
        Err(error) => removal
            .left
            .push(format!("its stored git credentials ({error})")),
    }
    let folder = process::agent_dir(&paths.data_dir, id);
    if folder.exists() {
        match std::fs::remove_dir_all(&folder) {
            Ok(()) => removal
                .removed
                .push("its folder, with its memory and its disk".into()),
            Err(error) => removal
                .left
                .push(format!("its folder {} ({error})", folder.display())),
        }
    }
    let _ = std::fs::remove_file(process::run_dir(&paths.data_dir).join(format!("{id}.json")));
    match store.call(move |store| store.agent_forget(id)).await {
        Ok(rows) => removal
            .removed
            .push(format!("its {rows} row(s) in bzz's database")),
        Err(error) => removal
            .left
            .push(format!("its rows in bzz's database ({error})")),
    }
    Ok(removal)
}

// ---- what `agent show` prints ----------------------------------------------

/// One agent's state as its settings say it, before any process is asked.
pub fn state_of(agent: &HostedAgentConfig) -> &'static str {
    match agent.local.as_ref().map(|local| local.state) {
        Some(LocalAgentState::MovedAway) => "moved away",
        Some(LocalAgentState::Paused) => "paused",
        Some(LocalAgentState::Active) if agent.paused => "paused",
        Some(LocalAgentState::Active) => "active",
        None => "run by you",
    }
}

/// A time as `bzz agent show` prints it.
pub fn when(seconds: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(seconds).unwrap_or(0))
        .ok()
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| seconds.to_string())
}

/// How one turn ended, in a few words.
pub fn outcome(turn: &RecentTurn) -> String {
    match (turn.state.as_str(), turn.failure.as_deref()) {
        ("replied", _) => "answered".into(),
        ("ended", _) => "spoke without a final reply".into(),
        ("failed", Some(kind)) => format!("failed: {kind}"),
        ("failed", None) => "failed".into(),
        ("delivered", _) => "answering now".into(),
        ("queued", _) => "waiting".into(),
        (other, _) => other.to_owned(),
    }
}

/// How many lines of an agent's activity `bzz agent show` prints.
pub const SHOWN_ACTIVITY: usize = 20;

/// The folders an agent's settings grant for commands, by name.
pub fn exec_folders(local: &LocalAgentConfig) -> Vec<String> {
    local
        .folders
        .iter()
        .filter(|folder| folder.exec.granted())
        .map(|folder| folder.name.clone())
        .collect()
}

/// The hosts a git credential is stored for, each with whether the agent
/// may reach it: a credential for a host that is not allowed is never given.
pub fn credential_lines(local: &LocalAgentConfig, hosts: &[String]) -> Vec<String> {
    hosts
        .iter()
        .map(|host| {
            if local.network.contains(host) {
                host.clone()
            } else {
                format!(
                    "{host} (not on its list of allowed hosts: never given until you allow the host)"
                )
            }
        })
        .collect()
}

/// Everything `bzz agent show` prints for one agent.
pub struct Details {
    pub running: Option<u32>,
    /// `(id, name, role)` of the channels bzz last saw it in.
    pub channels: Vec<(Uuid, String, String)>,
    pub turns: Vec<RecentTurn>,
    /// For an agent that is being moved or was moved away: when, and where
    /// to.
    pub moved: Option<String>,
    /// For an agent that was moved away, once the relay was asked: whether
    /// it is present there, or why that could not be told.
    pub presence: Option<String>,
    /// What its host said when it last started: where it offers `run`,
    /// and the Pi packages it loaded or left out.
    pub started: Option<install::StartReport>,
    /// The commands it asked to run and the credentials it asked for,
    /// with what it was answered; oldest first.
    pub activity: Vec<crate::agents::activity::Entry>,
    /// The hosts a git credential is stored for. Never the credentials.
    pub credential_hosts: Vec<String>,
}

pub async fn details(
    paths: &Paths,
    store: &StoreHandle,
    agent: &HostedAgentConfig,
) -> Result<Details> {
    let (id, community, pubkey) = (agent.id, agent.community, agent.pubkey.clone());
    let (channels, turns) = store
        .call(move |store| {
            Ok((
                store.agent_channels(community, &pubkey)?,
                store.agent_recent_turns(id, 10)?,
            ))
        })
        .await?;
    Ok(Details {
        running: running_pid(&paths.data_dir, agent.id),
        channels,
        turns,
        moved: crate::agents::moving::moved_line(&paths.data_dir, agent.id),
        presence: None,
        started: install::StartReport::read(&paths.data_dir, agent.id),
        activity: crate::agents::activity::recent(&paths.data_dir, agent.id, SHOWN_ACTIVITY),
        credential_hosts: crate::agents::git_credentials::hosts(&paths.keys_dir(), agent.id),
    })
}

/// An agent's settings as `bzz agent show` and the agents panel print them:
/// harness, model, who may wake it, instructions, folders, hosts, picture.
pub fn settings_lines(local: &LocalAgentConfig) -> Vec<String> {
    let mut lines = vec![
        format!("harness:     {}", local.harness.as_str()),
        format!(
            "model:       {}",
            match &local.model {
                Some(model) => format!(
                    "{}/{}{}",
                    model.provider,
                    model.model_id,
                    model
                        .thinking_level
                        .as_deref()
                        .map(|level| format!(" (thinking: {level})"))
                        .unwrap_or_default()
                ),
                None => "the harness's own default".into(),
            }
        ),
        format!(
            "wakes for:   {}",
            match local.wake {
                WakeRule::OwnerOnly => "its owner only".to_owned(),
                WakeRule::Anyone =>
                    "anyone in its channels (direct messages: its owner only)".into(),
                WakeRule::Allowlist => format!(
                    "its owner and {} listed (direct messages: its owner only)",
                    local.wake_allowlist.len()
                ),
            }
        ),
    ];
    lines.extend(
        local
            .wake_allowlist
            .iter()
            .map(|key| format!("             {key}")),
    );
    lines.push(format!(
        "instructions: {}",
        if local.instructions.trim().is_empty() {
            "none".to_owned()
        } else {
            format!("{} characters", local.instructions.chars().count())
        }
    ));
    if local.folders.is_empty() {
        lines.push("folders:     none of this machine: only its own disk".into());
    }
    for folder in &local.folders {
        lines.push(format!(
            "folder:      /mnt/{} = {} ({})",
            folder.name,
            folder.path.display(),
            folder_rights(folder)
        ));
    }
    lines.push(format!(
        "may reach:   {}",
        if local.network.is_empty() {
            "no host".to_owned()
        } else {
            local.network.join(", ")
        }
    ));
    if let Some(picture) = &local.picture {
        lines.push(format!("picture:     {picture}"));
    }
    if local.harness == Harness::PiDurable {
        lines.push(format!(
            "Pi packages: {}",
            if local.packages {
                "on: a package named in Pi's settings runs inside its host, with your rights"
            } else {
                "off"
            }
        ));
    }
    if !local.command.is_empty() {
        lines.push(format!(
            "launch:      overridden in the settings ({})",
            crate::protocol::artifact::display(&local.command.join(" "), 120)
        ));
    }
    lines
}

/// The lines of `bzz agent show`.
pub fn show_lines(agent: &HostedAgentConfig, community: &str, details: &Details) -> Vec<String> {
    let Some(local) = &agent.local else {
        return vec![format!(
            "{} is an agent whose process you run yourself (`bzz broker`)",
            agent.label
        )];
    };
    let mut lines = vec![
        format!("name:        {}", agent.label),
        format!("public key:  {}", agent.pubkey),
        format!("community:   {community}"),
        format!(
            "state:       {}{}",
            state_of(agent),
            details
                .running
                .map(|pid| format!(" · its process is running ({pid})"))
                .unwrap_or_default()
        ),
    ];
    // Why bzz does not start it, and what is known of it where it runs.
    if let Some(moved) = &details.moved {
        lines.push(format!("             {moved}"));
    }
    if local.state == LocalAgentState::MovedAway {
        lines.push(
            "             this bzz does not start it: it runs elsewhere. `bzz agent receive` takes it back"
                .into(),
        );
        lines.push(format!(
            "on the relay: {}",
            details.presence.as_deref().unwrap_or("not asked")
        ));
    }
    lines.extend(settings_lines(local));
    if details.channels.is_empty() {
        lines.push(
            "channels:    none known here; add it with `bzz agent add <name> <channel>`".into(),
        );
    }
    for (id, name, role) in &details.channels {
        lines.push(format!(
            "channel:     #{} ({role}) {id}",
            crate::protocol::artifact::display(name, 60)
        ));
    }
    if details.turns.is_empty() {
        lines.push("last turns:  none yet".into());
    } else {
        lines.push("last turns:".into());
    }
    for turn in &details.turns {
        lines.push(format!(
            "  {} · {} · attempt {} · request {}",
            when(turn.updated_at),
            outcome(turn),
            turn.attempts.max(1),
            &turn.request_id[..turn.request_id.len().min(12)]
        ));
    }
    if local.harness == Harness::PiDurable {
        if details.credential_hosts.is_empty() {
            lines.push(
                "git credentials: none stored; `bzz agent credential set <name> <host>` stores one"
                    .into(),
            );
        } else {
            lines.push("git credentials stored for (the host only is shown):".into());
        }
        lines.extend(
            credential_lines(local, &details.credential_hosts)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        match &details.started {
            Some(started) => lines.extend(started.lines(&exec_folders(local))),
            None => lines.push("its host: not started yet on this machine".into()),
        }
        if details.activity.is_empty() {
            lines.push("activity:    no command asked for and no credential asked for yet".into());
        } else {
            lines.push(
                "activity (commands it asked to run, credentials it asked for, and the answer):"
                    .into(),
            );
        }
        lines.extend(
            details
                .activity
                .iter()
                .map(|entry| format!("  {}", entry.line())),
        );
    }
    lines.extend(capability::record_for(local).lines());
    lines
}

#[cfg(test)]
mod tests {
    use nostr::Keys;

    use super::*;

    fn agent(keys: &Keys) -> HostedAgentConfig {
        HostedAgentConfig {
            id: Uuid::new_v4(),
            label: "Scout".into(),
            pubkey: keys.public_key().to_hex(),
            community: Uuid::new_v4(),
            owner_identity: Uuid::new_v4(),
            auth_tag: String::new(),
            credential_hash: String::new(),
            allowed: Vec::new(),
            paused: false,
            local: Some(LocalAgentConfig::new(Harness::PiDurable)),
        }
    }

    /// Buzz 1.0's own test of a `runtime` value
    /// (`src/features/agents/service.ts`, `RUNTIME`).
    fn buzz_reads_runtime(value: &str) -> bool {
        let part = |part: &str| {
            let mut bytes = part.bytes();
            bytes
                .next()
                .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
                && bytes.all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
        };
        matches!(value.split_once('/'), Some((plugin, kind)) if part(plugin) && part(kind) && !kind.contains('/'))
    }

    #[test]
    fn the_profile_is_a_bot_with_its_name_and_picture() {
        let keys = Keys::generate();
        let profile = profile_builder("Scout", Some("https://example.org/s.png"))
            .sign_with_keys(&keys)
            .unwrap();
        assert_eq!(profile.kind.as_u16(), 0);
        let content: Value = serde_json::from_str(&profile.content).unwrap();
        assert_eq!(
            content,
            json!({"name": "Scout", "bot": true, "picture": "https://example.org/s.png"})
        );
        let plain: Value = serde_json::from_str(
            &profile_builder("Scout", None)
                .sign_with_keys(&keys)
                .unwrap()
                .content,
        )
        .unwrap();
        assert_eq!(plain, json!({"name": "Scout", "bot": true}));
    }

    #[test]
    fn the_owners_record_is_what_buzz_reads() {
        let (owner, keys) = (Keys::generate(), Keys::generate());
        let mut agent = agent(&keys);
        let mut settings = LocalAgentConfig::new(Harness::PiDurable);
        let record = record_builder(&agent, &settings, None)
            .sign_with_keys(&owner)
            .unwrap();
        assert_eq!(record.kind.as_u16(), 30_177);
        let tags = |event: &Event| {
            event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            tags(&record),
            [
                vec!["d".to_owned(), agent.pubkey.clone()],
                vec!["runtime".to_owned(), "bzz/pi-durable".to_owned()],
            ]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&record.content).unwrap(),
            json!({"parallelism": 1, "respond_to": "owner-only", "name": "Scout"})
        );
        // Every harness's value passes Buzz's own test of a runtime.
        for harness in [
            Harness::PiDurable,
            Harness::Grok,
            Harness::Codex,
            Harness::ClaudeCode,
        ] {
            assert!(buzz_reads_runtime(&runtime_tag(harness)), "{harness:?}");
        }
        assert!(!buzz_reads_runtime("bzz") && !buzz_reads_runtime("Bzz/pi"));
        // bzz's own reader of v0.11 accepts the record with the profile.
        let auth = buzz_sdk::nip_oa::compute_auth_tag(&owner, &keys.public_key(), "").unwrap();
        let profile = profile_builder("Scout", None)
            .tag(Tag::parse(serde_json::from_str::<Vec<String>>(&auth).unwrap()).unwrap())
            .sign_with_keys(&keys)
            .unwrap();
        let verified =
            crate::agents::protocol::verify_public_agent(&profile, None, Some(&record)).unwrap();
        assert_eq!(verified.name, "Scout");
        assert_eq!(verified.owner_pubkey, owner.public_key().to_hex());
        assert_eq!(
            verified.respond_to,
            Some(crate::agents::RespondTo::OwnerOnly)
        );

        // An allowlist travels in Buzz's own words, and an edit keeps what
        // another client put in the record that is not bzz's to set.
        let friend = Keys::generate().public_key().to_hex();
        let prior = EventBuilder::new(
            Kind::Custom(30_177),
            json!({
                "parallelism": 4, "respond_to": "anyone", "name": "Old name",
                "picture": "https://example.org/old.png", "system_prompt": "kept",
                "respond_to_allowlist": ["x"],
            })
            .to_string(),
        )
        .tags([
            tag(&["d", &agent.pubkey]),
            tag(&["runtime", "buzz/pi"]),
            tag(&["client-id", "abc"]),
            tag(&["team", "red"]),
        ])
        .sign_with_keys(&owner)
        .unwrap();
        agent.label = "Scout the second".into();
        settings.wake = WakeRule::Allowlist;
        settings.wake_allowlist = vec![friend.clone()];
        let edited = record_builder(&agent, &settings, Some(&prior))
            .sign_with_keys(&owner)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&edited.content).unwrap(),
            json!({
                "parallelism": 4, "respond_to": "allowlist", "respond_to_allowlist": [friend],
                "name": "Scout the second", "system_prompt": "kept",
            }),
            "the old picture is gone, the rest of the old record is kept"
        );
        assert_eq!(
            tags(&edited),
            [
                vec!["d".to_owned(), agent.pubkey.clone()],
                vec!["runtime".to_owned(), "bzz/pi-durable".to_owned()],
                vec!["team".to_owned(), "red".to_owned()],
            ]
        );
        let verified =
            crate::agents::protocol::verify_public_agent(&profile, None, Some(&edited)).unwrap();
        assert_eq!(
            verified.respond_to,
            Some(crate::agents::RespondTo::Allowlist)
        );
        // The removal names the record by its coordinate, as Buzz does.
        let removal = record_removal_builder(&owner.public_key().to_hex(), &agent.pubkey)
            .sign_with_keys(&owner)
            .unwrap();
        assert_eq!(removal.kind.as_u16(), 5);
        assert_eq!(
            tags(&removal),
            [vec![
                "a".to_owned(),
                format!("30177:{}:{}", owner.public_key().to_hex(), agent.pubkey)
            ]]
        );
    }

    #[test]
    fn an_agent_joins_as_a_member_and_as_a_bot_only_in_a_solo_chat() {
        let agent = Keys::generate().public_key().to_hex();
        let channel = Uuid::new_v4();
        let roles = |kind| {
            add_member_builder(channel, &agent, kind)
                .unwrap()
                .sign_with_keys(&Keys::generate())
                .unwrap()
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect::<Vec<_>>()
        };
        let plain = vec![
            vec!["h".to_owned(), channel.to_string()],
            vec!["p".to_owned(), agent.clone()],
        ];
        assert_eq!(roles(PlaceKind::Channel), plain);
        let mut solo = plain.clone();
        solo.push(vec!["role".to_owned(), "bot".to_owned()]);
        assert_eq!(roles(PlaceKind::Solo), solo);
    }

    #[test]
    fn the_answers_are_read_as_written() {
        assert_eq!(parse_harness("Pi-Durable").unwrap(), Harness::PiDurable);
        assert_eq!(parse_harness("claude").unwrap(), Harness::ClaudeCode);
        assert!(parse_harness("gemini").is_err());
        assert_eq!(parse_wake("").unwrap(), WakeRule::OwnerOnly);
        assert_eq!(parse_wake("anyone").unwrap(), WakeRule::Anyone);
        assert!(parse_wake("everyone").is_err());
        assert_eq!(parse_model("", None).unwrap(), None);
        assert_eq!(
            parse_model("openrouter/anthropic/claude:free", Some("low")).unwrap(),
            Some(AgentModelConfig {
                provider: "openrouter".into(),
                model_id: "anthropic/claude:free".into(),
                thinking_level: Some("low".into()),
            })
        );
        assert!(parse_model("gpt-6", None).is_err());
        assert!(parse_model("", Some("low")).is_err());
        let key = Keys::generate().public_key();
        use nostr::ToBech32 as _;
        assert_eq!(
            parse_pubkey(&key.to_bech32().unwrap()).unwrap(),
            key.to_hex()
        );
        assert_eq!(parse_pubkey(&key.to_hex()).unwrap(), key.to_hex());
        assert!(parse_pubkey("ann").is_err());
        assert_eq!(parse_host("GitHub.com").unwrap(), "github.com");
        assert_eq!(parse_host("example.org:8443").unwrap(), "example.org:8443");
        for bad in [
            "https://github.com",
            "*.github.com",
            "github.com/x",
            "a b",
            "host:0",
            "host:port",
            "",
        ] {
            assert!(parse_host(bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse_picture("https://example.org/a.png").unwrap(),
            Some("https://example.org/a.png".into())
        );
        assert_eq!(parse_picture(" ").unwrap(), None);
        assert!(parse_picture("http://example.org/a.png").is_err());
        assert!(parse_picture("data:image/png;base64,AAAA").is_err());
        assert_eq!(split_list("a, b  c\n"), ["a", "b", "c"]);

        let temporary = tempfile::TempDir::new().unwrap();
        let site = temporary.path().join("site");
        std::fs::create_dir(&site).unwrap();
        let text = site.display().to_string();
        assert_eq!(
            parse_folder(&text).unwrap(),
            FolderGrant::read_only("site", &site)
        );
        assert_eq!(
            parse_folder(&format!("docs={text}:rw")).unwrap(),
            FolderGrant {
                name: "docs".into(),
                path: site.clone(),
                write: true,
                exec: ExecGrant::No,
            }
        );
        assert!(!parse_folder(&format!("docs={text}:ro")).unwrap().write);
        // Real commands are granted by a word of their own, apart from
        // writes, and never by default; asking is the default of the two.
        for (words, write, exec) in [
            ("", false, ExecGrant::No),
            (":exec", false, ExecGrant::Ask),
            (":rw:exec", true, ExecGrant::Ask),
            (":exec:rw", true, ExecGrant::Ask),
            (":ro:exec-without-asking", false, ExecGrant::WithoutAsking),
            (":rw:exec-without-asking", true, ExecGrant::WithoutAsking),
        ] {
            let grant = parse_folder(&format!("docs={text}{words}")).unwrap();
            assert_eq!((grant.write, grant.exec), (write, exec), "{words}");
            assert_eq!(grant.path, site, "{words}");
            // What `agent show` prints can be given back to `--folder`.
            assert_eq!(parse_folder(&folder_text(&grant)).unwrap(), grant);
        }
        assert!(folder_rights(&parse_folder(&format!("{text}:exec")).unwrap()).contains("asked"));
        assert!(
            folder_rights(&parse_folder(&format!("{text}:exec-without-asking")).unwrap())
                .contains("WITHOUT asking")
        );
        // A word bzz does not know is part of the path, which then is not there.
        assert!(parse_folder(&format!("docs={text}:run")).is_err());
        assert!(parse_folder(&format!("-docs={text}")).is_err());
        assert!(parse_folder(&format!("my docs={text}")).is_err());
        assert!(parse_folder("docs=relative/path").is_err());
        assert!(parse_folder(&format!("docs={text}/missing")).is_err());

        // The answers of the command and of the form, read the same way.
        let answers = Answers {
            name: " Scout ".into(),
            model: "scripted/scripted".into(),
            allow: vec![key.to_bech32().unwrap()],
            hosts: vec!["GitHub.com".into(), "github.com".into()],
            folders: vec![format!("docs={text}:rw")],
            picture: "https://example.org/a.png".into(),
            ..Answers::default()
        };
        let new = answers.read().unwrap();
        assert_eq!(new.name, "Scout");
        assert_eq!(new.harness, Harness::PiDurable, "the default");
        assert_eq!(
            (new.wake, new.wake_allowlist.clone()),
            (WakeRule::Allowlist, vec![key.to_hex()]),
            "naming people makes the rule a list"
        );
        assert_eq!(new.network, ["github.com"]);
        assert!(new.folders[0].write);
        assert_eq!(new.picture.as_deref(), Some("https://example.org/a.png"));
        assert_eq!(
            Answers {
                name: "Scout".into(),
                ..Answers::default()
            }
            .read()
            .unwrap(),
            NewAgent::named("Scout"),
            "nothing but a name: every default"
        );
        for wrong in [
            Answers::default(),
            Answers {
                name: "Scout".into(),
                harness: "gemini".into(),
                ..Answers::default()
            },
            Answers {
                name: "Scout".into(),
                wake: "anyone".into(),
                allow: vec![key.to_hex()],
                ..Answers::default()
            },
            Answers {
                name: "Scout".into(),
                wake: "allowlist".into(),
                ..Answers::default()
            },
            Answers {
                name: "Scout".into(),
                hosts: vec!["https://github.com".into()],
                ..Answers::default()
            },
        ] {
            assert!(wrong.read().is_err(), "{wrong:?}");
        }
        assert_eq!(split_commas("a=/x y, b=/z ,"), ["a=/x y", "b=/z"]);
        assert_eq!(QUESTIONS.len(), 10);
        // Pi packages are on unless the owner says off.
        let packages = |answer: &str| {
            Answers {
                name: "Scout".into(),
                packages: answer.into(),
                ..Answers::default()
            }
            .read()
            .map(|new| new.packages)
        };
        assert!(packages("").unwrap() && packages("on").unwrap());
        assert!(!packages("off").unwrap() && !packages("No").unwrap());
        assert!(packages("maybe").is_err());

        // Settings that contradict each other are refused.
        let friend = vec![key.to_hex()];
        assert!(validate_settings("Scout", WakeRule::Allowlist, &[], &[]).is_err());
        assert!(validate_settings("Scout", WakeRule::Anyone, &friend, &[]).is_err());
        assert!(validate_settings("Scout", WakeRule::Allowlist, &friend, &[]).is_ok());
        assert!(validate_settings("", WakeRule::OwnerOnly, &[], &[]).is_err());
        assert!(validate_settings(&"x".repeat(65), WakeRule::OwnerOnly, &[], &[]).is_err());
        assert!(validate_settings("two\nlines", WakeRule::OwnerOnly, &[], &[]).is_err());
        let grant = parse_folder(&text).unwrap();
        assert!(
            validate_settings("Scout", WakeRule::OwnerOnly, &[], &[grant.clone(), grant]).is_err()
        );
        // A folder allowed without asking, with an agent that anyone may
        // wake, would let anyone run commands as the owner: refused. With
        // the owner alone or listed people it is the owner's choice, and a
        // folder that asks is always one.
        let unasked = parse_folder(&format!("ops={text}:exec-without-asking")).unwrap();
        let asked = parse_folder(&format!("work={text}:exec")).unwrap();
        let refused = validate_settings(
            "Scout",
            WakeRule::Anyone,
            &[],
            std::slice::from_ref(&unasked),
        )
        .unwrap_err()
        .to_string();
        assert!(
            refused.contains("anyone in its channels could then run commands as you"),
            "{refused}"
        );
        assert!(
            validate_settings("Scout", WakeRule::Anyone, &[], std::slice::from_ref(&asked)).is_ok()
        );
        assert!(
            validate_settings(
                "Scout",
                WakeRule::OwnerOnly,
                &[],
                std::slice::from_ref(&unasked)
            )
            .is_ok()
        );
        assert!(validate_settings("Scout", WakeRule::Allowlist, &friend, &[unasked]).is_ok());
    }

    #[test]
    fn an_edit_says_which_records_changed() {
        let keys = Keys::generate();
        let agent = agent(&keys);
        let id = agent.id;
        let mut config = Config {
            hosted_agents: vec![agent],
            ..Config::default()
        };
        // Model, instructions, folders and hosts are bzz's own: nothing to
        // publish.
        let quiet = apply_edit(
            &mut config,
            id,
            &Edit {
                instructions: Some("Be brief.".into()),
                model: Some(parse_model("scripted/scripted", None).unwrap()),
                network: Some(vec!["github.com".into()]),
                ..Edit::default()
            },
        )
        .unwrap();
        assert_eq!(quiet, Republish::default());
        let local = config.hosted_agents[0].local.clone().unwrap();
        assert_eq!(local.instructions, "Be brief.");
        assert_eq!(local.network, ["github.com"]);
        // So are a folder granted for commands and the switch for Pi
        // packages.
        let granted = FolderGrant {
            name: "site".into(),
            path: std::env::temp_dir(),
            write: false,
            exec: ExecGrant::Ask,
        };
        let hands = apply_edit(
            &mut config,
            id,
            &Edit {
                folders: Some(vec![granted.clone()]),
                packages: Some(false),
                ..Edit::default()
            },
        )
        .unwrap();
        assert_eq!(hands, Republish::default());
        let local = config.hosted_agents[0].local.clone().unwrap();
        assert_eq!(
            (local.folders.clone(), local.packages),
            (vec![granted], false)
        );
        let shown = settings_lines(&local).join("\n");
        assert!(
            shown.contains("(read-only; real commands, each asked of you first)")
                && shown.contains("Pi packages: off"),
            "{shown}"
        );
        // A name is in both records; who may wake it only in the owner's.
        let renamed = apply_edit(
            &mut config,
            id,
            &Edit {
                name: Some("Ranger".into()),
                ..Edit::default()
            },
        )
        .unwrap();
        assert_eq!(renamed, Republish::BOTH);
        assert_eq!(config.hosted_agents[0].label, "Ranger");
        let opened = apply_edit(
            &mut config,
            id,
            &Edit {
                wake: Some(WakeRule::Anyone),
                ..Edit::default()
            },
        )
        .unwrap();
        assert_eq!(
            opened,
            Republish {
                profile: false,
                record: true
            }
        );
        // A contradiction is refused and changes nothing.
        let before = config.clone();
        assert!(
            apply_edit(
                &mut config,
                id,
                &Edit {
                    wake: Some(WakeRule::Allowlist),
                    ..Edit::default()
                },
            )
            .is_err()
        );
        assert_eq!(config, before);
        // Pause and resume; a moved agent is not resumed.
        set_paused(&mut config, id, true).unwrap();
        assert_eq!(state_of(&config.hosted_agents[0]), "paused");
        assert!(!config.hosted_agents[0].runs_locally());
        config.hosted_agents[0].paused = true;
        set_paused(&mut config, id, false).unwrap();
        assert!(config.hosted_agents[0].runs_locally());
        config.hosted_agents[0].local.as_mut().unwrap().state = LocalAgentState::MovedAway;
        assert!(set_paused(&mut config, id, false).is_err());
    }
}
