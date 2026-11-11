//! Caller side of the Buzz local credential provider, version 1
//! (`docs/local-credential-provider.md` upstream). A running Buzz Desktop
//! holds the key; bzz sends complete unsigned events and asks for NIP-44,
//! NIP-FI evidence and identity description over loopback.
//!
//! Version 1 has no Windows transport: on Windows every entry point refuses.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
};
use url::Url;

use crate::error::{Error, Result};

pub const VERSION: u32 = 1;
pub const ENV: &str = "BUZZ_CREDENTIAL_PROVIDER";
const MAX_RESPONSE: usize = 12_352 * 1024;
/// `sign_event`, `nip44_decrypt` and `pair` can be held 120 s for approval.
const HELD_TIMEOUT: Duration = Duration::from_secs(150);
const QUICK_TIMEOUT: Duration = Duration::from_secs(15);

pub const fn supported_platform() -> bool {
    !cfg!(windows)
}

fn unsupported() -> Error {
    Error::Unsupported(
        "the Buzz Desktop credential provider has no Windows transport in version 1".into(),
    )
}

/// Provider error codes and their caller behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    ApprovalRequired,
    Denied,
    RateLimited { retry_after: Duration },
    Revoked,
    Unavailable,
    UnsupportedVersion,
    UnknownMethod,
    InvalidRequest,
    Transport,
}

impl ProviderError {
    fn from_code(code: &str, retry_after: Option<u64>) -> Self {
        match code {
            "approval_required" => Self::ApprovalRequired,
            "denied" => Self::Denied,
            "rate_limited" => Self::RateLimited {
                retry_after: Duration::from_secs(retry_after.unwrap_or(30).min(300)),
            },
            "revoked" => Self::Revoked,
            "unavailable" => Self::Unavailable,
            "unsupported_version" => Self::UnsupportedVersion,
            "unknown_method" => Self::UnknownMethod,
            "invalid_request" => Self::InvalidRequest,
            // Callers MUST treat an unrecognized code as `denied`.
            _ => Self::Denied,
        }
    }

    pub const fn notice(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "Buzz Desktop: approve the request in Buzz, then retry",
            Self::Denied => "Buzz Desktop refused the request",
            Self::RateLimited { .. } => "Buzz Desktop is rate limiting requests",
            Self::Revoked => "Buzz Desktop revoked this pairing; pair again",
            Self::Unavailable => "Buzz Desktop cannot act now (signed out or session lapsed)",
            Self::UnsupportedVersion => "Buzz Desktop does not support this provider version",
            Self::UnknownMethod => "Buzz Desktop does not implement this method",
            Self::InvalidRequest => "Buzz Desktop rejected a malformed request",
            Self::Transport => "Buzz Desktop is not reachable",
        }
    }
}

impl From<ProviderError> for Error {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Transport | ProviderError::RateLimited { .. } => {
                Self::Network(error.notice().into())
            }
            ProviderError::Revoked | ProviderError::Unavailable => {
                Self::Locked(error.notice().into())
            }
            _ => Self::Auth(error.notice().into()),
        }
    }
}

/// Reject anything but `http://127.0.0.1:<port>/<path>` without userinfo.
pub fn validate_endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::Config("provider URL is invalid".into()))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_none()
    {
        return Err(Error::Config(
            "provider URL must be http://127.0.0.1:<port>/ without user information".into(),
        ));
    }
    Ok(url)
}

/// Relay URL normalization for bound-relay comparison: `http(s)` reads as
/// `ws(s)`, lowercase host, no default port, `/` for an empty path.
pub fn normalize_relay(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return None,
    };
    url.set_scheme(scheme).ok()?;
    if url.port() == Some(if scheme == "wss" { 443 } else { 80 }) {
        url.set_port(None).ok()?;
    }
    url.set_fragment(None);
    let host = url.host_str()?.to_ascii_lowercase();
    url.set_host(Some(&host)).ok()?;
    Some(url.to_string())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Description {
    pub instance: String,
    pub pubkey: String,
    pub role: String,
    #[serde(default)]
    pub owner_pubkey: Option<String>,
    #[serde(default)]
    pub relays: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
}

impl Description {
    pub fn binds(&self, relay: &str) -> bool {
        let Some(relay) = normalize_relay(relay) else {
            return false;
        };
        self.relays
            .iter()
            .filter_map(|bound| normalize_relay(bound))
            .any(|bound| bound == relay)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pairing {
    pub token: String,
    pub instance: String,
    pub pubkey: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// How the provider endpoint was found. Advertised endpoints get the
/// same-user check before any token is sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Endpoint {
    Inherited(Url),
    Advertised(Url),
}

impl Endpoint {
    fn url(&self) -> &Url {
        match self {
            Self::Inherited(url) | Self::Advertised(url) => url,
        }
    }
}

pub struct ProviderClient {
    endpoint: Endpoint,
    token: Option<SecretString>,
    evidence_cache: Mutex<HashMap<String, (String, u64)>>,
}

impl std::fmt::Debug for ProviderClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderClient")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl ProviderClient {
    pub fn new(endpoint: Endpoint, token: Option<SecretString>) -> Result<Self> {
        if !supported_platform() {
            return Err(unsupported());
        }
        validate_endpoint(endpoint.url().as_str())?;
        Ok(Self {
            endpoint,
            token,
            evidence_cache: Mutex::new(HashMap::new()),
        })
    }

    /// `BUZZ_CREDENTIAL_PROVIDER={"url":...,"token":...}` from the environment.
    pub fn inherited() -> Result<Option<Self>> {
        match std::env::var(ENV) {
            Ok(value) => Self::from_inherited_value(&value).map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Parse a `BUZZ_CREDENTIAL_PROVIDER` value.
    pub fn from_inherited_value(value: &str) -> Result<Self> {
        let parsed: Value = serde_json::from_str(value)
            .map_err(|_| Error::Config(format!("{ENV} is not valid JSON")))?;
        let url = parsed
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Config(format!("{ENV} has no url")))?;
        let token = parsed
            .get("token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| Error::Config(format!("{ENV} has no token")))?;
        Self::new(
            Endpoint::Inherited(validate_endpoint(url)?),
            Some(SecretString::from(token.to_owned())),
        )
    }

    pub fn is_inherited(&self) -> bool {
        matches!(self.endpoint, Endpoint::Inherited(_))
    }

    /// One request on one fresh connection, with the same-user check first
    /// for both inherited and advertised endpoints.
    pub async fn call(
        &self,
        method: &str,
        params: &[String],
    ) -> std::result::Result<String, ProviderError> {
        let id = crate::sync::read_state::random_id(8);
        let body = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
            .map_err(|_| ProviderError::InvalidRequest)?;
        let timeout = match method {
            "sign_event" | "nip44_decrypt" | "pair" => HELD_TIMEOUT,
            _ => QUICK_TIMEOUT,
        };
        let token = if method == "pair" {
            None
        } else {
            Some(self.token.as_ref().ok_or(ProviderError::Revoked)?)
        };
        let url = self.endpoint.url();
        let response = tokio::time::timeout(timeout, request(url, token, &body))
            .await
            .map_err(|_| ProviderError::Transport)??;
        let value: Value =
            serde_json::from_slice(&response).map_err(|_| ProviderError::InvalidRequest)?;
        if value.get("id").and_then(Value::as_str) != Some(id.as_str()) {
            return Err(ProviderError::InvalidRequest);
        }
        if let Some(code) = value.get("error").and_then(Value::as_str) {
            return Err(ProviderError::from_code(
                code,
                value.get("retry_after").and_then(Value::as_u64),
            ));
        }
        value
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(ProviderError::InvalidRequest)
    }

    pub async fn describe(&self) -> std::result::Result<Description, ProviderError> {
        let result = self.call("describe", &[]).await?;
        serde_json::from_str(&result).map_err(|_| ProviderError::InvalidRequest)
    }

    pub async fn pair(
        &self,
        client_name: &str,
        relay: &str,
    ) -> std::result::Result<Pairing, ProviderError> {
        let result = self
            .call("pair", &[client_name.to_owned(), relay.to_owned()])
            .await?;
        serde_json::from_str(&result).map_err(|_| ProviderError::InvalidRequest)
    }

    /// NIP-FI evidence for `relay`, cached until shortly before `exp`.
    /// `Ok(None)`: the relay needs none, or the provider has no such method.
    pub async fn federated_identity(
        &self,
        relay: &str,
        now: u64,
    ) -> std::result::Result<Option<String>, ProviderError> {
        if let Some((assertion, expires)) = self
            .evidence_cache
            .lock()
            .expect("cache")
            .get(relay)
            .cloned()
            && now + crate::auth::federated::REMINT_MARGIN_SECONDS < expires
        {
            return Ok(Some(assertion));
        }
        let assertion = match self.call("federated_identity", &[relay.to_owned()]).await {
            Ok(assertion) => assertion,
            Err(ProviderError::UnknownMethod) => return Ok(None),
            Err(error) => return Err(error),
        };
        if assertion.is_empty() {
            return Ok(None);
        }
        let expires = jws_expiry(&assertion).unwrap_or(now + 60);
        self.evidence_cache
            .lock()
            .expect("cache")
            .insert(relay.to_owned(), (assertion.clone(), expires));
        Ok(Some(assertion))
    }

    pub fn forget_evidence(&self, relay: &str) {
        self.evidence_cache.lock().expect("cache").remove(relay);
    }
}

/// `exp` from a compact JWS payload. Display-only: the relay verifies it.
fn jws_expiry(jws: &str) -> Option<u64> {
    use base64::Engine as _;
    let payload = jws.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_u64()
}

async fn request(
    url: &Url,
    token: Option<&SecretString>,
    body: &[u8],
) -> std::result::Result<Vec<u8>, ProviderError> {
    let port = url.port().ok_or(ProviderError::Transport)?;
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|_| ProviderError::Transport)?;
    // Same-user check before any byte, token or not, leaves this socket.
    let peer_port = stream
        .local_addr()
        .map_err(|_| ProviderError::Transport)?
        .port();
    let confirmed = tokio::time::timeout(
        Duration::from_secs(1),
        tokio::task::spawn_blocking(move || same_user_holds(port, peer_port)),
    )
    .await
    .ok()
    .and_then(std::result::Result::ok)
    .unwrap_or(false);
    if !confirmed {
        return Err(ProviderError::Transport);
    }
    let mut head = format!(
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nBuzz-Credential-Provider: {VERSION}\r\nContent-Length: {}\r\nConnection: close\r\n",
        url.path(),
        body.len()
    );
    if let Some(token) = token {
        head.push_str(&format!(
            "Authorization: Bearer {}\r\n",
            token.expose_secret()
        ));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|_| ProviderError::Transport)?;
    stream
        .write_all(body)
        .await
        .map_err(|_| ProviderError::Transport)?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| ProviderError::Transport)?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
        if response.len() > MAX_RESPONSE {
            return Err(ProviderError::InvalidRequest);
        }
    }
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(ProviderError::InvalidRequest)?;
    Ok(response[split + 4..].to_vec())
}

/// Whether a process of this user holds the server side of the loopback
/// connection `127.0.0.1:client_port -> 127.0.0.1:server_port`.
fn same_user_holds(server_port: u16, client_port: u16) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some(uid) = own_uid() else { return false };
        let local = format!("0100007F:{server_port:04X}");
        let remote = format!("0100007F:{client_port:04X}");
        std::fs::read_to_string("/proc/net/tcp").is_ok_and(|table| {
            table.lines().skip(1).any(|line| {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                fields.len() > 7
                    && fields[1] == local
                    && fields[2] == remote
                    && fields[7].parse::<u32>().ok() == Some(uid)
            })
        })
    }
    #[cfg(target_os = "macos")]
    {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|value| value.trim().parse::<u32>().ok());
        let Some(uid) = uid else { return false };
        let output = std::process::Command::new("lsof")
            .args(["-nP", "-iTCP", "-sTCP:ESTABLISHED", "-Fun"])
            .output();
        let Ok(output) = output else { return false };
        let text = String::from_utf8_lossy(&output.stdout);
        let wanted = format!("n127.0.0.1:{server_port}->127.0.0.1:{client_port}");
        let mut current_uid = None;
        text.lines().any(|line| {
            if let Some(value) = line.strip_prefix('u') {
                current_uid = value.parse::<u32>().ok();
                false
            } else {
                line == wanted && current_uid == Some(uid)
            }
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (server_port, client_port);
        false
    }
}

#[cfg(target_os = "linux")]
fn own_uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// The provider advertisement directory.
pub fn advertisement_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|base| {
        base.home_dir()
            .join(".buzz-foundation/credential-providers")
    })
}

/// Advertised endpoint for `instance`, after the ownership and permission
/// checks the spec requires. Files that fail them are ignored.
pub fn advertised(directory: &Path, instance: &str) -> Option<Url> {
    advertisements(directory)
        .into_iter()
        .find(|(name, _)| name == instance)
        .map(|(_, url)| url)
}

pub fn advertisements(directory: &Path) -> Vec<(String, Url)> {
    if !supported_platform() || !private_dir(directory) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                && private_file(path)
        })
        .filter_map(|path| {
            let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            let instance = value.get("instance")?.as_str()?.to_owned();
            let url = validate_endpoint(value.get("url")?.as_str()?).ok()?;
            Some((instance, url))
        })
        .collect()
}

#[cfg(unix)]
fn private_dir(path: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir()
            && metadata.permissions().mode() & 0o777 == 0o700
            && Some(metadata.uid()) == current_uid()
    })
}

#[cfg(unix)]
fn private_file(path: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && metadata.permissions().mode() & 0o077 == 0
            && Some(metadata.uid()) == current_uid()
    })
}

#[cfg(unix)]
fn current_uid() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        own_uid()
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::unix::fs::MetadataExt as _;
        directories::BaseDirs::new()
            .and_then(|base| std::fs::metadata(base.home_dir()).ok())
            .map(|metadata| metadata.uid())
    }
}

#[cfg(not(unix))]
fn private_dir(_path: &Path) -> bool {
    false
}

#[cfg(not(unix))]
fn private_file(_path: &Path) -> bool {
    false
}

/// NIP-FI evidence from Buzz Desktop's `federated_identity`.
pub struct ProviderEvidence {
    client: std::sync::Arc<ProviderClient>,
    clock: crate::clock::SharedClock,
}

impl ProviderEvidence {
    pub fn new(
        client: std::sync::Arc<ProviderClient>,
        clock: crate::clock::SharedClock,
    ) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self { client, clock })
    }
}

#[async_trait::async_trait]
impl crate::auth::federated::EvidenceSource for ProviderEvidence {
    async fn evidence(&self, relay: &Url) -> Result<Option<String>> {
        let relay = normalize_relay(relay.as_str())
            .ok_or_else(|| Error::Config("relay URL cannot be normalized".into()))?;
        match self
            .client
            .federated_identity(&relay, self.clock.now_unix())
            .await
        {
            Ok(Some(assertion)) => Ok(Some(format!("Bearer {assertion}"))),
            // The relay needs none, or the desktop has no enterprise login
            // for it: send no evidence (the relay decides).
            Ok(None) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn invalidate(&self, relay: &Url) {
        if let Some(relay) = normalize_relay(relay.as_str()) {
            self.client.forget_evidence(&relay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_be_ipv4_loopback_http_without_userinfo() {
        assert!(validate_endpoint("http://127.0.0.1:4100/rpc").is_ok());
        for bad in [
            "https://127.0.0.1:4100/",
            "http://localhost:4100/",
            "http://[::1]:4100/",
            "http://u:p@127.0.0.1:4100/",
            "http://127.0.0.2:4100/",
            "http://127.0.0.1/",
        ] {
            assert!(validate_endpoint(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bound_relays_compare_after_normalization() {
        let description = Description {
            instance: "x".into(),
            pubkey: "a".repeat(64),
            role: "owner".into(),
            owner_pubkey: None,
            relays: vec![
                "https://Team.Example".into(),
                "wss://other.example/c1".into(),
            ],
            methods: vec![],
        };
        assert!(description.binds("wss://team.example/"));
        assert!(description.binds("wss://team.example:443"));
        assert!(!description.binds("wss://other.example/c2"), "path counts");
        assert!(description.binds("wss://other.example/c1"));
        assert!(!description.binds("ws://team.example"));
    }

    #[test]
    fn unknown_error_codes_are_denied() {
        assert_eq!(
            ProviderError::from_code("brand_new", None),
            ProviderError::Denied
        );
        assert_eq!(
            ProviderError::from_code("rate_limited", Some(7)),
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(7)
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn advertisements_require_private_directory_and_files() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::TempDir::new().unwrap();
        let directory = root.path().join("credential-providers");
        std::fs::create_dir(&directory).unwrap();
        let write = |name: &str, mode: u32| {
            let path = directory.join(name);
            std::fs::write(
                &path,
                serde_json::json!({"instance": name.trim_end_matches(".json"), "url":"http://127.0.0.1:4100/rpc"}).to_string(),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        write("good.json", 0o600);
        write("shared.json", 0o644);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            advertisements(&directory).is_empty(),
            "group-readable directory ignored"
        );
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let found = advertisements(&directory);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "good");
        assert!(advertised(&directory, "good").is_some());
        assert!(advertised(&directory, "shared").is_none());
    }
}
