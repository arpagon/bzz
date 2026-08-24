use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

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
    pub local_agents: Vec<LocalAgentConfig>,
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
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
pub struct CommunityConfig {
    pub id: Uuid,
    pub label: String,
    pub relay_url: String,
    pub identity_id: Uuid,
    #[serde(default)]
    pub allow_insecure_localhost: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalAgentBackend {
    Codex,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAgentConfig {
    pub id: Uuid,
    pub label: String,
    pub backend: LocalAgentBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<PathBuf>,
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
        let config: Self = toml::from_str(&text).map_err(|_| {
            Error::Config(format!(
                "{} contains invalid or unknown settings",
                path.display()
            ))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.validate()?;
        paths.ensure()?;
        let path = paths.config_file();
        let text = toml::to_string_pretty(self)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let temporary = path.with_extension("toml.tmp");
        let mut file =
            fs::File::create(&temporary).map_err(|error| Error::io(&temporary, error))?;
        file.write_all(text.as_bytes())
            .map_err(|error| Error::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
        set_private_permissions(&temporary)?;
        replace_file(&temporary, &path)
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
        let mut agent_ids = std::collections::HashSet::new();
        let mut agent_labels = std::collections::HashSet::new();
        for agent in &self.local_agents {
            if !agent_ids.insert(agent.id) {
                return Err(Error::Config(format!(
                    "duplicate local agent id {}",
                    agent.id
                )));
            }
            validate_agent_label(&agent.label)?;
            if !agent_labels.insert(agent.label.trim().to_ascii_lowercase()) {
                return Err(Error::Config(format!(
                    "duplicate local agent label {}",
                    agent.label.trim()
                )));
            }
            if let Some(workdir) = &agent.workdir {
                let canonical = fs::canonicalize(workdir).map_err(|_| {
                    Error::Config(format!(
                        "local agent {} has an unavailable working directory",
                        agent.id
                    ))
                })?;
                if canonical != *workdir || !canonical.is_dir() {
                    return Err(Error::Config(format!(
                        "local agent {} working directory must be a canonical directory",
                        agent.id
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

    pub fn add_local_agent(&mut self, label: String, workdir: Option<PathBuf>) -> Result<Uuid> {
        let workdir = workdir
            .map(|path| {
                let canonical = fs::canonicalize(&path).map_err(|_| {
                    Error::Config(format!(
                        "local agent working directory {} is unavailable",
                        path.display()
                    ))
                })?;
                if !canonical.is_dir() {
                    return Err(Error::Config(
                        "local agent working directory is not a directory".into(),
                    ));
                }
                Ok(canonical)
            })
            .transpose()?;
        let agent = LocalAgentConfig {
            id: Uuid::new_v4(),
            label,
            backend: LocalAgentBackend::Codex,
            workdir,
        };
        validate_agent_label(&agent.label)?;
        if self
            .local_agents
            .iter()
            .any(|entry| entry.label.trim().eq_ignore_ascii_case(agent.label.trim()))
        {
            return Err(Error::Config(
                "a local agent already uses that label".into(),
            ));
        }
        let id = agent.id;
        self.local_agents.push(agent);
        if let Err(error) = self.validate() {
            self.local_agents.pop();
            return Err(error);
        }
        Ok(id)
    }

    pub fn remove_local_agent(&mut self, id: Uuid) -> bool {
        let before = self.local_agents.len();
        self.local_agents.retain(|agent| agent.id != id);
        self.local_agents.len() != before
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

fn validate_agent_label(value: &str) -> Result<()> {
    let label = value.trim();
    if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
        return Err(Error::Config(
            "local agent label must be 1-80 visible characters".into(),
        ));
    }
    Ok(())
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

pub fn load_from(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path).map_err(|error| Error::io(path, error))?;
    toml::from_str(&text).map_err(|_| Error::Config("invalid or unknown settings".into()))
}
