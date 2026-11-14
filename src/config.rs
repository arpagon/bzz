use std::{fs, io::Write as _, path::Path};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    paths::{Paths, set_private_permissions},
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub default_community: Option<Uuid>,
    #[serde(default)]
    pub identities: Vec<IdentityConfig>,
    #[serde(default)]
    pub communities: Vec<CommunityConfig>,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub media: MediaConfig,
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    /// Agents whose keys bzz holds for the local broker (M9). Never secrets:
    /// keys live NIP-44-sealed to the owner, credentials only as hashes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosted_agents: Vec<HostedAgentConfig>,
    /// What this copy last read from the settings file or wrote to it. A
    /// save writes only what changed since, over what the file holds now
    /// (see [`Config::save`]). Not a setting, and never written.
    #[doc(hidden)]
    #[serde(skip)]
    pub synced: Synced,
}

/// A copy's own record of the settings file as it last saw it. Two copies
/// of the settings are equal whatever each last saw.
#[derive(Default)]
pub struct Synced(std::sync::Mutex<Option<toml::Value>>);

impl Synced {
    fn get(&self) -> Option<toml::Value> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set(&self, value: toml::Value) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(value);
    }
}

impl Clone for Synced {
    fn clone(&self) -> Self {
        Self(std::sync::Mutex::new(self.get()))
    }
}

impl PartialEq for Synced {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Synced {}

impl std::fmt::Debug for Synced {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Synced")
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalJournalMode {
    #[default]
    On,
    Off,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticsConfig {
    pub local_journal: LocalJournalMode,
}

/// Non-secret remote-export configuration. Bearer credentials are never stored
/// here; persistent tokens live in a dedicated OS credential service.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<Uuid>,
    #[serde(default)]
    pub credential_persisted: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyBackend {
    Keychain,
    EncryptedFile,
    /// The key lives in a running Buzz Desktop (local credential provider);
    /// `key_ref` names the keychain entry holding the paired token.
    BuzzDesktop,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityConfig {
    pub id: Uuid,
    pub label: String,
    pub pubkey: String,
    pub backend: KeyBackend,
    pub key_ref: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostedAgentConfig {
    pub id: Uuid,
    pub label: String,
    pub pubkey: String,
    pub community: Uuid,
    /// The human identity that owns the agent and seals its key.
    pub owner_identity: Uuid,
    /// NIP-OA owner `auth` tag (compact JSON), signed once at creation.
    pub auth_tag: String,
    /// Lowercase hex SHA-256 of the bearer credential.
    pub credential_hash: String,
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub paused: bool,
    /// Present when bzz itself starts this agent's process (v0.15, a local
    /// agent). Absent for a v0.13 hosted agent, whose process the user runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalAgentConfig>,
}

impl HostedAgentConfig {
    /// Whether bzz may start this agent's process now.
    pub fn runs_locally(&self) -> bool {
        !self.paused
            && self
                .local
                .as_ref()
                .is_some_and(|local| local.state == LocalAgentState::Active)
    }
}

/// The program that runs a local agent (ADR v0.15, D14).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Harness {
    PiDurable,
    Grok,
    Codex,
    ClaudeCode,
}

impl Harness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PiDurable => "pi-durable",
            Self::Grok => "grok",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
        }
    }
}

/// Who may wake a local agent (D3). DMs are owner only whatever this says.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WakeRule {
    #[default]
    OwnerOnly,
    Allowlist,
    Anyone,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalAgentState {
    #[default]
    Active,
    Paused,
    /// `agent move` took it to another machine; bzz refuses to run it.
    MovedAway,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentModelConfig {
    pub provider: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// Whether real commands of this machine may run in a granted folder
/// (plan v0.15.0, M7). A command has the owner's rights, whatever `write`
/// says: `write` binds the agent's file tools only.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecGrant {
    /// None: the agent has its file tools there and nothing else.
    #[default]
    No,
    /// Each command waits for the owner's answer in the TUI.
    Ask,
    /// Commands run there without the owner being asked.
    WithoutAsking,
}

impl ExecGrant {
    pub const fn is_no(&self) -> bool {
        matches!(self, Self::No)
    }

    /// Whether any command may run in the folder.
    pub const fn granted(self) -> bool {
        !self.is_no()
    }
}

/// A real folder the owner grants to an agent; read-only unless `write`,
/// and no real command runs there unless `exec` says so.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FolderGrant {
    pub name: String,
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub write: bool,
    #[serde(default, skip_serializing_if = "ExecGrant::is_no")]
    pub exec: ExecGrant,
}

impl FolderGrant {
    /// A folder the agent may read, and nothing more.
    pub fn read_only(name: &str, path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            name: name.to_owned(),
            path: path.into(),
            write: false,
            exec: ExecGrant::No,
        }
    }
}

const fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_yes(value: &bool) -> bool {
    *value
}

/// Settings of a local agent. Never secrets: the key stays sealed in
/// `keys/`, and no provider key is stored (D11).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAgentConfig {
    pub harness: Harness,
    #[serde(default)]
    pub wake: WakeRule,
    /// Public keys (hex) that may wake the agent under `wake = "allowlist"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_allowlist: Vec<String>,
    /// Absent: the harness uses its own default model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModelConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<FolderGrant>,
    /// Hosts the agent may reach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network: Vec<String>,
    #[serde(default)]
    pub state: LocalAgentState,
    /// Program and arguments to start instead of the harness's own launch
    /// command, for a harness run from a checkout. Empty: the default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// The picture of the agent's profile and of the owner's record: an
    /// HTTPS address. Kept so that an edit can publish both again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Whether bzz's own host loads the Pi packages named in Pi's settings.
    /// A package's code runs inside the host with the owner's rights, so an
    /// owner can switch them off for one agent. On unless it says `false`.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub packages: bool,
}

impl LocalAgentConfig {
    pub fn new(harness: Harness) -> Self {
        Self {
            harness,
            wake: WakeRule::default(),
            wake_allowlist: Vec::new(),
            model: None,
            instructions: String::new(),
            folders: Vec::new(),
            network: Vec::new(),
            state: LocalAgentState::default(),
            command: Vec::new(),
            picture: None,
            packages: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommunityConfig {
    pub id: Uuid,
    pub label: String,
    pub relay_url: String,
    pub identity_id: Uuid,
    #[serde(default)]
    pub allow_insecure_localhost: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// NIP-FA adapter origin for communities that require federated identity
    /// (ADR v0.13). HTTPS, or loopback HTTP for local testing only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_adapter: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseMode {
    Auto,
    On,
    Off,
}

impl MouseMode {
    pub fn enabled(self) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::Auto => {
                std::io::IsTerminal::is_terminal(&std::io::stdout())
                    && std::env::var("TERM").is_ok_and(|term| term != "dumb")
            }
        }
    }
}

/// Private local ordering for the joined-channel directory. It is intentionally
/// presentation-only: no sort mode changes a subscription, marker, or stored
/// channel record.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelSort {
    #[default]
    Smart,
    Recent,
    Alphabetical,
}

impl ChannelSort {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Smart => "smart",
            Self::Recent => "recent",
            Self::Alphabetical => "A-Z",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Smart => Self::Recent,
            Self::Recent => Self::Alphabetical,
            Self::Alphabetical => Self::Smart,
        }
    }
}

/// OSC 52 is emitted only after an explicit copy action. Disabling it leaves
/// terminal-native selection available without bzz writing clipboard data.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClipboardMode {
    Disabled,
    #[default]
    Osc52,
}

/// Controls whether bzz fetches a Nostr kind-0 `picture`. Trusted fetching is
/// credential-free for external URLs; a canonical image path on the active
/// community relay receives narrowly scoped media-read authorization.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileAvatars {
    Off,
    #[default]
    Trusted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub sidebar_width: u16,
    pub thread_width: u16,
    /// Maximum readable message measure on a wide conversation surface.
    /// The pane may be narrower; this never changes stored content.
    pub message_width: u16,
    pub channel_sort: ChannelSort,
    pub clipboard: ClipboardMode,
    pub profile_avatars: ProfileAvatars,
    pub theme: String,
    pub mouse: MouseMode,
    /// Publish your own online/away presence (kind 20001). Off by default:
    /// bzz publishes nothing about activity unless you opt in.
    pub publish_presence: bool,
    /// Owner-signed kind 20003 presence for hosted agents while the broker
    /// serves them. The only automatic owner-signed event; off by default and
    /// shown in `:broker`. Requires `publish_presence`.
    #[serde(default)]
    pub broker_agent_presence: bool,
    /// Huddle microphone and speaker by name (default devices when unset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub huddle_input_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub huddle_output_device: Option<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 28,
            thread_width: 44,
            message_width: 110,
            channel_sort: ChannelSort::Smart,
            clipboard: ClipboardMode::Osc52,
            profile_avatars: ProfileAvatars::Trusted,
            theme: crate::ui::theme::DEFAULT_THEME_ID.into(),
            mouse: MouseMode::Auto,
            publish_presence: false,
            broker_agent_presence: false,
            huddle_input_device: None,
            huddle_output_device: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaProtocol {
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Halfblocks,
    Off,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaAutoload {
    Visible,
    Preview,
    Off,
}

/// Native clipboard reads occur only for an explicit composer paste. This is
/// separate from `ui.clipboard`, which controls explicit OSC-52 writes.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClipboardImportMode {
    Off,
    #[default]
    Explicit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConfig {
    pub enabled: bool,
    pub protocol: MediaProtocol,
    pub autoload: MediaAutoload,
    pub clipboard_import: ClipboardImportMode,
    pub max_inline_rows: u16,
    pub auto_download_bytes: u64,
    pub memory_cache_bytes: u64,
    pub disk_cache_bytes: u64,
    pub download_concurrency: usize,
    pub decode_concurrency: usize,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            protocol: MediaProtocol::Auto,
            autoload: MediaAutoload::Visible,
            clipboard_import: ClipboardImportMode::Explicit,
            max_inline_rows: 12,
            auto_download_bytes: 25 * 1024 * 1024,
            memory_cache_bytes: 64 * 1024 * 1024,
            disk_cache_bytes: 512 * 1024 * 1024,
            download_concurrency: 4,
            decode_concurrency: 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayEndpoint {
    pub websocket: Url,
    pub http_base: Url,
    pub authority: String,
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.config_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|error| Error::io(&path, error))?;
        let (config, removed_local_agents) = parse_config(&text, &path)?;
        config.validate()?;
        config.synced.set(as_value(&config)?);
        if removed_local_agents {
            config.save(paths)?;
        }
        Ok(config)
    }

    /// Writes the settings, **without writing over what another bzz saved
    /// meanwhile**. A TUI and a command each hold a copy of the settings;
    /// whichever saves last used to win whole, so a command's edit could be
    /// lost to a TUI that was open.
    ///
    /// A save takes the settings lock, reads the file as it is now, and
    /// writes that with only what this copy changed since it last read or
    /// wrote the file:
    ///
    /// - a setting this copy did not change keeps the file's value;
    /// - a setting only this copy changed gets this copy's value;
    /// - identities, communities and agents are matched by their id, and
    ///   one that either side removed stays removed;
    /// - a setting both changed gets this copy's value: it is the later act.
    ///
    /// This copy itself is not changed: a bzz that is running keeps using
    /// the settings it started with. A copy that never read the file writes
    /// itself whole, and so does any copy when the file cannot be read.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.validate()?;
        paths.ensure()?;
        let path = paths.config_file();
        let _lock = SettingsLock::take(&path)?;
        let mine = as_value(self)?;
        let on_disk = fs::read_to_string(&path)
            .ok()
            .and_then(|text| parse_config(&text, &path).ok())
            .and_then(|(config, _)| as_value(&config).ok());
        let merged = match (self.synced.get(), on_disk) {
            (Some(base), Some(theirs)) if theirs != base && theirs != mine => {
                let merged = merge(&base, &mine, &theirs)
                    .try_into::<Self>()
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                // What the two changes make together must still hold.
                merged.validate().map_err(|error| {
                    Error::Config(format!(
                        "the settings were changed by another bzz meanwhile, and the two changes do not fit together ({error}); nothing was saved"
                    ))
                })?;
                Some(merged)
            }
            _ => None,
        };
        let text = toml::to_string_pretty(merged.as_ref().unwrap_or(self))
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let temporary = path.with_extension("toml.tmp");
        let mut file =
            fs::File::create(&temporary).map_err(|error| Error::io(&temporary, error))?;
        file.write_all(text.as_bytes())
            .map_err(|error| Error::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
        set_private_permissions(&temporary)?;
        replace_file(&temporary, &path)?;
        // From here, what this copy changes is measured against itself.
        self.synced.set(mine);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut identity_ids = std::collections::HashSet::new();
        for identity in &self.identities {
            if !identity_ids.insert(identity.id) {
                return Err(Error::Config(format!(
                    "duplicate identity id {}",
                    identity.id
                )));
            }
            if identity.label.trim().is_empty() {
                return Err(Error::Config("identity label cannot be empty".into()));
            }
            let pubkey_is_hex = identity.pubkey.len() == 64
                && identity.pubkey.bytes().all(|byte| byte.is_ascii_hexdigit());
            if !pubkey_is_hex {
                return Err(Error::Config(format!(
                    "identity {} has an invalid public key",
                    identity.id
                )));
            }
            if identity.key_ref.trim().is_empty() {
                return Err(Error::Config(format!(
                    "identity {} has an empty key reference",
                    identity.id
                )));
            }
        }
        let mut community_ids = std::collections::HashSet::new();
        let mut urls = std::collections::HashSet::new();
        for community in &self.communities {
            if !community_ids.insert(community.id) {
                return Err(Error::Config(format!(
                    "duplicate community id {}",
                    community.id
                )));
            }
            if !identity_ids.contains(&community.identity_id) {
                return Err(Error::Config(format!(
                    "community {} refers to missing identity {}",
                    community.id, community.identity_id
                )));
            }
            if community.label.trim().is_empty() {
                return Err(Error::Config("community label cannot be empty".into()));
            }
            let endpoint =
                validate_relay_url(&community.relay_url, community.allow_insecure_localhost)?;
            if let Some(adapter) = &community.identity_adapter {
                let url = url::Url::parse(adapter).map_err(|_| {
                    Error::Config(format!(
                        "community {} identity_adapter is not a URL",
                        community.id
                    ))
                })?;
                crate::auth::federated::AdapterClient::new(url)?;
            }
            if !urls.insert(endpoint.websocket.to_string()) {
                return Err(Error::Config(format!(
                    "duplicate relay URL {}",
                    endpoint.websocket
                )));
            }
        }
        if let Some(default) = self.default_community
            && !community_ids.contains(&default)
        {
            return Err(Error::Config(format!(
                "default community {default} does not exist"
            )));
        }
        for agent in &self.hosted_agents {
            let Some(local) = &agent.local else { continue };
            let is_key = |value: &String| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            };
            if !local.wake_allowlist.iter().all(is_key) {
                return Err(Error::Config(format!(
                    "agent {} has an invalid public key in wake_allowlist",
                    agent.label
                )));
            }
            // A folder appears to the agent as /mnt/<name>.
            let mut names = std::collections::HashSet::new();
            for folder in &local.folders {
                let simple = !folder.name.is_empty()
                    && folder.name.len() <= 64
                    && folder
                        .name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                    && folder.name.as_bytes()[0].is_ascii_alphanumeric();
                if !simple || !names.insert(folder.name.as_str()) || !folder.path.is_absolute() {
                    return Err(Error::Config(format!(
                        "agent {} has an invalid folder grant {:?}",
                        agent.label, folder.name
                    )));
                }
            }
        }
        validate_theme_name(&self.ui.theme, "ui.theme")?;
        for community in &self.communities {
            if let Some(theme) = &community.theme {
                validate_theme_name(theme, "community theme")?;
            }
        }
        if !(18..=60).contains(&self.ui.sidebar_width) {
            return Err(Error::Config(
                "ui.sidebar_width must be between 18 and 60".into(),
            ));
        }
        if !(30..=80).contains(&self.ui.thread_width) {
            return Err(Error::Config(
                "ui.thread_width must be between 30 and 80".into(),
            ));
        }
        if !(48..=200).contains(&self.ui.message_width) {
            return Err(Error::Config(
                "ui.message_width must be between 48 and 200".into(),
            ));
        }
        if !(2..=40).contains(&self.media.max_inline_rows) {
            return Err(Error::Config(
                "media.max_inline_rows must be between 2 and 40".into(),
            ));
        }
        if self.media.auto_download_bytes > 50 * 1024 * 1024 {
            return Err(Error::Config(
                "media.auto_download_bytes cannot exceed 50 MiB".into(),
            ));
        }
        if self.media.memory_cache_bytes > 1024 * 1024 * 1024
            || self.media.disk_cache_bytes > 20 * 1024 * 1024 * 1024
        {
            return Err(Error::Config(
                "media cache limit exceeds its safety cap".into(),
            ));
        }
        if !(1..=16).contains(&self.media.download_concurrency)
            || !(1..=8).contains(&self.media.decode_concurrency)
        {
            return Err(Error::Config(
                "media worker concurrency is outside its safety range".into(),
            ));
        }
        match (
            self.telemetry.endpoint.as_deref(),
            self.telemetry.endpoint_digest.as_deref(),
            self.telemetry.installation_id,
        ) {
            (None, None, None)
                if !self.telemetry.enabled && !self.telemetry.credential_persisted => {}
            (Some(endpoint), Some(digest), Some(_)) => {
                validate_telemetry_endpoint(endpoint)?;
                if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(Error::Config(
                        "telemetry endpoint binding is invalid; configure telemetry again".into(),
                    ));
                }
            }
            _ => {
                return Err(Error::Config(
                    "telemetry configuration is incomplete; configure or forget it".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn add_community(
        &mut self,
        label: String,
        relay_url: String,
        identity_id: Uuid,
        insecure: bool,
    ) -> Result<Uuid> {
        let endpoint = validate_relay_url(&relay_url, insecure)?;
        if !self
            .identities
            .iter()
            .any(|identity| identity.id == identity_id)
        {
            return Err(Error::Config(format!(
                "identity {identity_id} does not exist"
            )));
        }
        if self.communities.iter().any(|entry| {
            validate_relay_url(&entry.relay_url, entry.allow_insecure_localhost)
                .is_ok_and(|existing| existing.websocket == endpoint.websocket)
        }) {
            return Err(Error::Config("that relay is already configured".into()));
        }
        let id = Uuid::new_v4();
        self.communities.push(CommunityConfig {
            id,
            label,
            relay_url: endpoint.websocket.to_string(),
            identity_id,
            allow_insecure_localhost: insecure,
            theme: None,
            identity_adapter: None,
        });
        self.default_community.get_or_insert(id);
        self.validate()?;
        Ok(id)
    }

    pub fn resolved_theme(&self, community_index: usize) -> &str {
        self.communities
            .get(community_index)
            .and_then(|community| community.theme.as_deref())
            .unwrap_or(&self.ui.theme)
    }

    pub fn remove_community(&mut self, id: Uuid) -> bool {
        let original = self.communities.len();
        self.communities.retain(|community| community.id != id);
        if self.default_community == Some(id) {
            self.default_community = self.communities.first().map(|entry| entry.id);
        }
        original != self.communities.len()
    }
}

fn as_value(config: &Config) -> Result<toml::Value> {
    toml::Value::try_from(config).map_err(|error| Error::Serialization(error.to_string()))
}

/// The lock a save holds while it reads the settings file and writes it:
/// an exclusive lock on `config.toml.lock`, released when this is dropped
/// or the process ends, however it ends.
struct SettingsLock(#[allow(dead_code)] fs::File);

impl SettingsLock {
    /// How long a save waits for another bzz that is saving.
    const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

    fn take(config_file: &Path) -> Result<Self> {
        let path = config_file.with_extension("toml.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| Error::io(&path, error))?;
        let _ = set_private_permissions(&path);
        let started = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self(file)),
                Err(fs::TryLockError::WouldBlock) if started.elapsed() < Self::WAIT => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(Error::Config(format!(
                        "another bzz has been writing the settings for {} seconds; nothing was saved",
                        Self::WAIT.as_secs()
                    )));
                }
                Err(fs::TryLockError::Error(error)) => return Err(Error::io(&path, error)),
            }
        }
    }
}

/// Whether `value` is a list whose entries are told apart by an `id`:
/// identities, communities and agents.
fn keyed(value: &toml::Value) -> bool {
    value.as_array().is_some_and(|entries| {
        entries
            .iter()
            .all(|entry| entry.get("id").is_some_and(toml::Value::is_str))
    })
}

/// What `mine` changed since `base`, applied to `theirs`. See
/// [`Config::save`] for the rule.
fn merge(base: &toml::Value, mine: &toml::Value, theirs: &toml::Value) -> toml::Value {
    use toml::Value;
    if mine == base {
        return theirs.clone();
    }
    if theirs == base || mine == theirs {
        return mine.clone();
    }
    match (base, mine, theirs) {
        (Value::Table(base), Value::Table(mine), Value::Table(theirs)) => {
            let mut merged = toml::Table::new();
            // The file's order, then what only this copy has.
            for key in theirs.keys().chain(mine.keys()) {
                if merged.contains_key(key) {
                    continue;
                }
                let (base, mine, theirs) = (base.get(key), mine.get(key), theirs.get(key));
                let empty = Value::Array(Vec::new());
                let lists = [base, mine, theirs]
                    .iter()
                    .all(|value| value.is_none_or(keyed));
                let value = if lists {
                    // A list that is empty is not written at all.
                    Some(merge_keyed(
                        base.unwrap_or(&empty),
                        mine.unwrap_or(&empty),
                        theirs.unwrap_or(&empty),
                    ))
                } else if mine == base {
                    theirs.cloned()
                } else if theirs == base {
                    mine.cloned()
                } else {
                    match (base, mine, theirs) {
                        (Some(base), Some(mine), Some(theirs)) => Some(merge(base, mine, theirs)),
                        // Both changed it and at least one of the three
                        // has none: this copy's is the later act.
                        _ => mine.cloned(),
                    }
                };
                if let Some(value) = value {
                    merged.insert(key.clone(), value);
                }
            }
            Value::Table(merged)
        }
        // Both changed one value: this copy's is the later act.
        _ => mine.clone(),
    }
}

fn merge_keyed(base: &toml::Value, mine: &toml::Value, theirs: &toml::Value) -> toml::Value {
    let entries = |value: &toml::Value| value.as_array().cloned().unwrap_or_default();
    let id = |entry: &toml::Value| {
        entry
            .get("id")
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    let find = |list: &[toml::Value], wanted: &str| {
        list.iter()
            .find(|entry| id(entry).as_deref() == Some(wanted))
            .cloned()
    };
    let (base, mine, theirs) = (entries(base), entries(mine), entries(theirs));
    let mut ids = Vec::new();
    for entry in theirs.iter().chain(&mine) {
        if let Some(id) = id(entry)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    let mut merged = Vec::new();
    for id in ids {
        let (was, here, there) = (find(&base, &id), find(&mine, &id), find(&theirs, &id));
        let entry = match (was, here, there) {
            // Removed by either side since: it stays removed, whatever the
            // other changed in it. Its key may be gone already.
            (Some(_), None, _) | (Some(_), _, None) => None,
            (Some(was), Some(here), Some(there)) => Some(merge(&was, &here, &there)),
            // New on one side.
            (None, Some(here), None) => Some(here),
            (None, None, Some(there)) => Some(there),
            // New on both, with one id: this copy's is the later act.
            (None, Some(here), Some(_)) => Some(here),
            (None, None, None) => None,
        };
        merged.extend(entry);
    }
    toml::Value::Array(merged)
}

pub fn validate_relay_url(input: &str, allow_insecure_localhost: bool) -> Result<RelayEndpoint> {
    let mut url =
        Url::parse(input).map_err(|error| Error::Config(format!("invalid relay URL: {error}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(Error::Config("relay URL credentials are forbidden".into()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(Error::Config(
            "relay URL query/fragment is forbidden".into(),
        ));
    }
    if url.path() != "" && url.path() != "/" {
        return Err(Error::Config("relay URL must use the origin root".into()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Config("relay URL needs a host".into()))?;
    match url.scheme() {
        "wss" => {}
        "ws" if allow_insecure_localhost && is_loopback(host) => {}
        "ws" => {
            return Err(Error::Config(
                "ws:// is allowed only for loopback with explicit acknowledgement".into(),
            ));
        }
        _ => return Err(Error::Config("relay URL scheme must be wss://".into())),
    }
    url.set_path("/");
    let mut http = url.clone();
    http.set_scheme(if url.scheme() == "wss" {
        "https"
    } else {
        "http"
    })
    .map_err(|()| Error::Config("cannot map relay URL to HTTP".into()))?;
    let authority = buzz_core::tenant::relay_url_authority(url.as_str());
    Ok(RelayEndpoint {
        websocket: url,
        http_base: http,
        authority,
    })
}

pub fn validate_telemetry_endpoint(input: &str) -> Result<Url> {
    let url = Url::parse(input)
        .map_err(|_| Error::Config("telemetry endpoint must be a canonical HTTPS URL".into()))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(Error::Config(
            "telemetry endpoint must use HTTPS and include a host".into(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Config(
            "telemetry endpoint credentials, query, and fragment are forbidden".into(),
        ));
    }
    if url.path() != "/v1/logs" {
        return Err(Error::Config(
            "telemetry endpoint path must be exactly /v1/logs".into(),
        ));
    }
    if url.as_str() != input {
        return Err(Error::Config(
            "telemetry endpoint must use its canonical URL form".into(),
        ));
    }
    Ok(url)
}

fn validate_theme_name(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > 80
        || value.chars().any(|character| character.is_control())
    {
        return Err(Error::Config(format!("{field} is invalid")));
    }
    Ok(())
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    fs::rename(temporary, destination).map_err(|error| Error::io(destination, error))
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    let backup = destination.with_extension("toml.bak");
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| Error::io(&backup, error))?;
    }
    if destination.exists() {
        fs::rename(destination, &backup).map_err(|error| Error::io(destination, error))?;
    }
    if let Err(error) = fs::rename(temporary, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        return Err(Error::io(destination, error));
    }
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| Error::io(&backup, error))?;
    }
    Ok(())
}

fn parse_config(text: &str, path: &Path) -> Result<(Config, bool)> {
    let mut value = toml::from_str::<toml::Value>(text).map_err(|_| {
        Error::Config(format!(
            "{} contains invalid or unknown settings",
            path.display()
        ))
    })?;
    // The v0.3-v0.9 one-shot Codex drafter was retired. Tolerate and remove
    // only its former top-level key so upgrades remain bootable; every other
    // unknown setting still fails closed during typed deserialization.
    let removed_local_agents = value
        .as_table_mut()
        .and_then(|table| table.remove("local_agents"))
        .is_some();
    let config = value.try_into().map_err(|_| {
        Error::Config(format!(
            "{} contains invalid or unknown settings",
            path.display()
        ))
    })?;
    Ok((config, removed_local_agents))
}

pub fn load_from(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path).map_err(|error| Error::io(path, error))?;
    parse_config(&text, path).map(|(config, _)| config)
}
