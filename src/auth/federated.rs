//! NIP-FA federated identity adapter client and NIP-FI evidence handling.
//!
//! See `docs/adr-v0.13-federated-identity.md`. Every adapter call uses an HTTP
//! client that follows no redirects. Session tokens, handoff secrets, codes
//! and assertions are never logged, never put in URLs other than the adapter's
//! own login start, and never sent to any origin but the adapter's.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng as _;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use url::Url;

use crate::{
    auth::signer::SignerHandle,
    clock::WallClock,
    error::{Error, Result},
};

pub const HEADER: &str = "Nostr-Federated-Identity";
/// Re-mint an assertion this many seconds before it expires.
pub const REMINT_MARGIN_SECONDS: u64 = 60;
const MAX_ADAPTER_RESPONSE: usize = 64 * 1024;

/// NIP-FA canonical `wss://host[:port]`: lowercase host, default port
/// omitted, no path/query/fragment, IPv6 in brackets. Port 80 is refused.
pub fn canonical_relay_url(relay: &Url) -> Result<String> {
    let host = match relay.host() {
        Some(url::Host::Domain(domain)) => domain.trim_end_matches('.').to_ascii_lowercase(),
        Some(url::Host::Ipv4(address)) => address.to_string(),
        Some(url::Host::Ipv6(address)) => format!("[{address}]"),
        None => return Err(Error::Config("relay URL has no host".into())),
    };
    let port = relay.port_or_known_default();
    if port == Some(80) {
        return Err(Error::Config(
            "NIP-FA does not support relay authorities on port 80".into(),
        ));
    }
    Ok(match port {
        Some(443) | None => format!("wss://{host}"),
        Some(port) => format!("wss://{host}:{port}"),
    })
}

/// Assertion `aud` for a canonical relay URL: `https://` + authority.
pub fn audience(canonical: &str) -> String {
    format!("https://{}", canonical.trim_start_matches("wss://"))
}

/// What NIP-11 says about federated identity on this host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement {
    NotSupported,
    Optional,
    Required,
}

pub fn requirement(info: &Value) -> Requirement {
    if info.get("requires_nip_fi").and_then(Value::as_bool) == Some(true) {
        return Requirement::Required;
    }
    let supported = info
        .pointer("/limitation/federated_identity")
        .and_then(Value::as_bool)
        == Some(true)
        || info
            .pointer("/federated_identity/core")
            .and_then(Value::as_str)
            == Some("client-attached");
    if supported {
        Requirement::Optional
    } else {
        Requirement::NotSupported
    }
}

/// NIP-FA client actions for a denial (Denials table).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Denial {
    /// `session_required` / `session_expired`: clear the session, sign in again.
    SessionEnded,
    /// Keep the session, show the fixed reason, never retry automatically.
    Refused(RefusalReason),
    /// 429, any 5xx, or a network failure: keep the session, back off.
    Retry { after: Option<Duration> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalReason {
    InvalidRequest,
    AuthorizationDenied,
    InvalidProof,
    BindingMismatch,
    RequestTooLarge,
    InvalidResponse,
    Undefined,
}

impl RefusalReason {
    pub const fn notice(self) -> &'static str {
        match self {
            Self::InvalidRequest => "federated sign-in: the adapter rejected the request",
            Self::AuthorizationDenied => "federated sign-in: access to this community was denied",
            Self::InvalidProof => "federated sign-in: the adapter rejected the key proof",
            Self::BindingMismatch => {
                "federated sign-in: this account is bound to a different Nostr key"
            }
            Self::RequestTooLarge => "federated sign-in: request too large",
            Self::InvalidResponse => "federated sign-in: the adapter sent an invalid response",
            Self::Undefined => "federated sign-in: the adapter refused the request",
        }
    }
}

impl std::fmt::Display for Denial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionEnded => {
                formatter.write_str("federated sign-in: session ended; sign in again")
            }
            Self::Refused(reason) => formatter.write_str(reason.notice()),
            Self::Retry { .. } => {
                formatter.write_str("federated sign-in: adapter unavailable; retrying")
            }
        }
    }
}

/// Map an adapter response to its NIP-FA client action. Raw adapter text is
/// read only for the fixed `error` code and then discarded.
pub fn classify(status: u16, body: &[u8], retry_after: Option<Duration>) -> Denial {
    if status == 429 || (500..600).contains(&status) {
        return Denial::Retry { after: retry_after };
    }
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    match (status, code.as_deref()) {
        (401, Some("session_required" | "session_expired")) => Denial::SessionEnded,
        (400, Some("invalid_request")) => Denial::Refused(RefusalReason::InvalidRequest),
        (403, Some("authorization_denied")) => Denial::Refused(RefusalReason::AuthorizationDenied),
        (403, Some("invalid_proof")) => Denial::Refused(RefusalReason::InvalidProof),
        (403, Some("binding_mismatch")) => Denial::Refused(RefusalReason::BindingMismatch),
        (413, Some("request_too_large")) => Denial::Refused(RefusalReason::RequestTooLarge),
        _ => Denial::Refused(RefusalReason::Undefined),
    }
}

/// An adapter session. The token never appears in `Debug`.
#[derive(Clone)]
pub struct Session {
    pub token: SecretString,
    pub expires_at: OffsetDateTime,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// A relay assertion, held in memory only.
#[derive(Clone)]
pub struct Assertion {
    pub jws: SecretString,
    pub expires_at: u64,
}

impl std::fmt::Debug for Assertion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Assertion")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Assertion {
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.jws.expose_secret())
    }

    pub fn needs_remint(&self, clock: &dyn WallClock) -> bool {
        clock.now_unix().saturating_add(REMINT_MARGIN_SECONDS) >= self.expires_at
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum AdapterError {
    Denied(Denial),
    Other(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(denial) => denial.fmt(formatter),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl From<AdapterError> for Error {
    fn from(error: AdapterError) -> Self {
        match error {
            AdapterError::Denied(Denial::Refused(RefusalReason::AuthorizationDenied)) => {
                Self::Access(error.to_string())
            }
            AdapterError::Denied(Denial::Retry { .. }) => Self::Network(error.to_string()),
            other => Self::Auth(other.to_string()),
        }
    }
}

type AdapterResult<T> = std::result::Result<T, AdapterError>;

#[derive(Clone)]
pub struct AdapterClient {
    base: Url,
    client: Client,
}

#[derive(Deserialize)]
struct SessionResponse {
    #[serde(default)]
    session_token: Option<String>,
    expires_at: String,
}

#[derive(Deserialize)]
struct AssertionResponse {
    assertion: String,
    nostr_pubkey: String,
    expires_at: u64,
}

/// A browser login waiting for its loopback callback.
pub struct PendingLogin {
    /// Open this in the user's browser (or print it).
    pub url: Url,
    listener: TcpListener,
    handoff_secret: SecretString,
}

impl AdapterClient {
    /// `base` must be HTTPS, or HTTP on a loopback IP for local testing.
    pub fn new(base: Url) -> Result<Self> {
        let loopback = matches!(base.host(), Some(url::Host::Ipv4(address)) if address.is_loopback())
            || matches!(base.host(), Some(url::Host::Ipv6(address)) if address.is_loopback())
            || base.host_str() == Some("localhost");
        if base.scheme() != "https" && !(base.scheme() == "http" && loopback) {
            return Err(Error::Config(
                "identity_adapter must be an https:// URL".into(),
            ));
        }
        if !base.username().is_empty() || base.password().is_some() || base.query().is_some() {
            return Err(Error::Config(
                "identity_adapter must be a plain origin".into(),
            ));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("bzz/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Network(error.to_string()))?;
        Ok(Self { base, client })
    }

    pub fn origin(&self) -> String {
        self.base.origin().ascii_serialization()
    }

    fn endpoint(&self, path: &str) -> AdapterResult<Url> {
        self.base
            .join(path)
            .map_err(|error| AdapterError::Other(error.to_string()))
    }

    /// Start a browser login: S256 handoff challenge and a loopback callback
    /// on `127.0.0.1`, any port.
    pub async fn begin_login(&self) -> Result<PendingLogin> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| Error::Network(error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| Error::Network(error.to_string()))?
            .port();
        let mut secret = [0_u8; 32];
        rand::rng().fill(&mut secret);
        let handoff_secret = URL_SAFE_NO_PAD.encode(secret);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(handoff_secret.as_bytes()));
        let mut url = self
            .endpoint("v1/login/start")
            .map_err(|error| Error::Config(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("return_to", &format!("http://127.0.0.1:{port}/callback"))
            .append_pair("handoff_challenge", &challenge)
            .append_pair("handoff_challenge_method", "S256");
        Ok(PendingLogin {
            url,
            listener,
            handoff_secret: SecretString::from(handoff_secret),
        })
    }

    /// Wait for the callback, exchange the code, then verify the session.
    pub async fn complete_login(
        &self,
        pending: PendingLogin,
        timeout: Duration,
    ) -> Result<Session> {
        let code = tokio::time::timeout(timeout, accept_callback(&pending.listener))
            .await
            .map_err(|_| Error::Timeout("federated sign-in callback".into()))??;
        let exchanged = self
            .exchange(&code, pending.handoff_secret.expose_secret())
            .await?;
        let checked = self.session(&exchanged.token).await?;
        // The exchange and the session check must describe one fixed session.
        if checked != exchanged.expires_at {
            return Err(Error::Auth(
                "federated sign-in: session check did not match the exchanged session".into(),
            ));
        }
        Ok(exchanged)
    }

    async fn exchange(&self, code: &str, handoff_secret: &str) -> AdapterResult<Session> {
        let response = self
            .client
            .post(self.endpoint("v1/login/exchange")?)
            .json(&json!({"code": code, "handoff_secret": handoff_secret}))
            .send()
            .await
            .map_err(|error| AdapterError::Denied(network(&error)))?;
        let parsed: SessionResponse = success_json(response).await?;
        let token = parsed
            .session_token
            .filter(|token| !token.is_empty())
            .ok_or(AdapterError::Denied(Denial::Refused(
                RefusalReason::InvalidResponse,
            )))?;
        Ok(Session {
            token: SecretString::from(token),
            expires_at: parse_instant(&parsed.expires_at)?,
        })
    }

    /// `GET /v1/session`; returns the session's fixed expiry.
    pub async fn session(&self, token: &SecretString) -> AdapterResult<OffsetDateTime> {
        let response = self
            .client
            .get(self.endpoint("v1/session")?)
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|error| AdapterError::Denied(network(&error)))?;
        let parsed: SessionResponse = success_json(response).await?;
        parse_instant(&parsed.expires_at)
    }

    /// `POST /v1/identity/assertions` with the session and a fresh NIP-98
    /// proof over the exact body, signed by the key bzz connects with.
    pub async fn assertion(
        &self,
        session: &Session,
        relay: &Url,
        signer: &SignerHandle,
    ) -> AdapterResult<Assertion> {
        let pubkey = signer.public_key().to_hex();
        let canonical =
            canonical_relay_url(relay).map_err(|error| AdapterError::Other(error.to_string()))?;
        let body = serde_json::to_vec(&json!({"relay_url": canonical, "nostr_pubkey": pubkey}))
            .map_err(|error| AdapterError::Other(error.to_string()))?;
        let url = self.endpoint("v1/identity/assertions")?;
        let proof = signer
            .nip98_header("POST", url.as_str(), Some(&body))
            .await
            .map_err(|error| AdapterError::Other(error.to_string()))?;
        let response = self
            .client
            .post(url)
            .bearer_auth(session.token.expose_secret())
            .header("Nostr-Authorization", proof)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| AdapterError::Denied(network(&error)))?;
        let parsed: AssertionResponse = success_json(response).await?;
        if parsed.nostr_pubkey != pubkey || parsed.assertion.is_empty() {
            return Err(AdapterError::Denied(Denial::Refused(
                RefusalReason::InvalidResponse,
            )));
        }
        Ok(Assertion {
            jws: SecretString::from(parsed.assertion),
            expires_at: parsed.expires_at,
        })
    }
}

fn network(_error: &reqwest::Error) -> Denial {
    Denial::Retry { after: None }
}

fn parse_instant(value: &str) -> AdapterResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| AdapterError::Denied(Denial::Refused(RefusalReason::InvalidResponse)))
}

async fn success_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> AdapterResult<T> {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.min(300)));
    if status.is_redirection() {
        return Err(AdapterError::Denied(Denial::Refused(
            RefusalReason::InvalidResponse,
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| AdapterError::Denied(Denial::Retry { after: None }))?;
    if bytes.len() > MAX_ADAPTER_RESPONSE {
        return Err(AdapterError::Denied(Denial::Refused(
            RefusalReason::InvalidResponse,
        )));
    }
    if status != StatusCode::OK {
        return Err(AdapterError::Denied(classify(
            status.as_u16(),
            &bytes,
            retry_after,
        )));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| AdapterError::Denied(Denial::Refused(RefusalReason::InvalidResponse)))
}

/// Accept one loopback `GET /callback?code=...` (or `?error=...`).
async fn accept_callback(listener: &TcpListener) -> Result<String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| Error::Network(error.to_string()))?;
        let mut buffer = vec![0_u8; 8192];
        let read = stream.read(&mut buffer).await.unwrap_or(0);
        let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let target = head
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or_default()
            .to_owned();
        let url = Url::parse(&format!("http://127.0.0.1{target}")).ok();
        let pairs = url
            .as_ref()
            .filter(|url| url.path() == "/callback")
            .map(|url| url.query_pairs().into_owned().collect::<HashMap<_, _>>());
        let Some(pairs) = pairs else {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let page =
            "<!doctype html><title>bzz</title><p>You can close this tab and return to bzz.</p>";
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                )
                .as_bytes(),
            )
            .await;
        if let Some(code) = pairs.get("code").filter(|code| !code.is_empty()) {
            return Ok(code.clone());
        }
        return Err(Error::Auth(
            "federated sign-in was cancelled or refused in the browser".into(),
        ));
    }
}

/// Persisted adapter session, NIP-44-encrypted to the identity's own key in an
/// owner-only file under the data directory.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    v: u8,
    origin: String,
    token: String,
    expires_at: String,
}

pub struct SessionStore {
    directory: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            directory: data_dir.join("federated"),
        }
    }

    fn path(&self, identity: uuid::Uuid, origin: &str) -> PathBuf {
        let digest = hex::encode(Sha256::digest(origin.as_bytes()));
        self.directory
            .join(format!("{identity}-{}.session", &digest[..16]))
    }

    pub async fn save(
        &self,
        identity: uuid::Uuid,
        origin: &str,
        session: &Session,
        signer: &SignerHandle,
    ) -> Result<()> {
        let plaintext = serde_json::to_string(&StoredSession {
            v: 1,
            origin: origin.to_owned(),
            token: session.token.expose_secret().to_owned(),
            expires_at: session
                .expires_at
                .format(&Rfc3339)
                .map_err(|error| Error::Serialization(error.to_string()))?,
        })
        .map_err(|error| Error::Serialization(error.to_string()))?;
        let ciphertext = signer.encrypt_self(plaintext).await?;
        std::fs::create_dir_all(&self.directory).map_err(|source| Error::Io {
            path: self.directory.clone(),
            source,
        })?;
        crate::paths::set_private_permissions(&self.directory)?;
        let path = self.path(identity, origin);
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, ciphertext).map_err(|source| Error::Io {
            path: temporary.clone(),
            source,
        })?;
        crate::paths::set_private_permissions(&temporary)?;
        std::fs::rename(&temporary, &path).map_err(|source| Error::Io { path, source })
    }

    /// `Ok(None)` when absent, expired, unreadable, or for another origin.
    pub async fn load(
        &self,
        identity: uuid::Uuid,
        origin: &str,
        signer: &SignerHandle,
        clock: &dyn WallClock,
    ) -> Result<Option<Session>> {
        let path = self.path(identity, origin);
        let Ok(ciphertext) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        let Ok(plaintext) = signer.decrypt_self(ciphertext).await else {
            return Ok(None);
        };
        let Ok(stored) = serde_json::from_str::<StoredSession>(&plaintext) else {
            return Ok(None);
        };
        let Ok(expires_at) = OffsetDateTime::parse(&stored.expires_at, &Rfc3339) else {
            return Ok(None);
        };
        if stored.v != 1
            || stored.origin != origin
            || u64::try_from(expires_at.unix_timestamp()).unwrap_or(0) <= clock.now_unix()
        {
            return Ok(None);
        }
        Ok(Some(Session {
            token: SecretString::from(stored.token),
            expires_at,
        }))
    }

    pub fn clear(&self, identity: uuid::Uuid, origin: &str) -> Result<()> {
        let path = self.path(identity, origin);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Io { path, source }),
        }
    }
}

/// Supplies NIP-FI evidence for one relay. Implemented by the NIP-FA adapter
/// path and by the Buzz Desktop credential provider.
#[async_trait::async_trait]
pub trait EvidenceSource: Send + Sync {
    /// A current assertion header value (`Bearer <jws>`), minting if needed.
    async fn evidence(&self, relay: &Url) -> Result<String>;
    /// Drop the cached assertion so the next call mints a fresh one.
    fn invalidate(&self, relay: &Url);
}

/// NIP-FA evidence: an adapter session plus an in-memory assertion cache.
pub struct AdapterEvidence {
    adapter: AdapterClient,
    session: Mutex<Option<Session>>,
    signer: SignerHandle,
    clock: crate::clock::SharedClock,
    cache: Mutex<HashMap<String, Assertion>>,
}

impl AdapterEvidence {
    pub fn new(
        adapter: AdapterClient,
        session: Session,
        signer: SignerHandle,
        clock: crate::clock::SharedClock,
    ) -> Arc<Self> {
        Arc::new(Self {
            adapter,
            session: Mutex::new(Some(session)),
            signer,
            clock,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Whether the session is still held (it is dropped on `SessionEnded`).
    pub fn has_session(&self) -> bool {
        self.session.lock().expect("session lock").is_some()
    }
}

#[async_trait::async_trait]
impl EvidenceSource for AdapterEvidence {
    async fn evidence(&self, relay: &Url) -> Result<String> {
        let key = canonical_relay_url(relay)?;
        if let Some(cached) = self.cache.lock().expect("cache lock").get(&key)
            && !cached.needs_remint(self.clock.as_ref())
        {
            return Ok(cached.header_value());
        }
        let session = self
            .session
            .lock()
            .expect("session lock")
            .clone()
            .ok_or_else(|| Error::Auth(Denial::SessionEnded.to_string()))?;
        match self.adapter.assertion(&session, relay, &self.signer).await {
            Ok(assertion) => {
                let header = assertion.header_value();
                self.cache
                    .lock()
                    .expect("cache lock")
                    .insert(key, assertion);
                Ok(header)
            }
            Err(AdapterError::Denied(Denial::SessionEnded)) => {
                *self.session.lock().expect("session lock") = None;
                Err(AdapterError::Denied(Denial::SessionEnded).into())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn invalidate(&self, relay: &Url) {
        if let Ok(key) = canonical_relay_url(relay) {
            self.cache.lock().expect("cache lock").remove(&key);
        }
    }
}

/// The shared, swappable evidence source for one community runtime. Empty
/// until NIP-11 says the host requires (or offers) federated identity and a
/// source is configured; every relay client consults it per request.
#[derive(Clone, Default)]
pub struct EvidenceSlot(Arc<std::sync::RwLock<Option<Arc<dyn EvidenceSource>>>>);

impl EvidenceSlot {
    pub fn set(&self, source: Arc<dyn EvidenceSource>) {
        *self.0.write().expect("evidence lock") = Some(source);
    }

    pub fn clear(&self) {
        *self.0.write().expect("evidence lock") = None;
    }

    pub fn is_set(&self) -> bool {
        self.0.read().expect("evidence lock").is_some()
    }

    /// `Ok(None)` when no evidence is configured for this runtime.
    pub async fn header(&self, relay: &Url) -> Result<Option<String>> {
        let source = self.0.read().expect("evidence lock").clone();
        match source {
            Some(source) => source.evidence(relay).await.map(Some),
            None => Ok(None),
        }
    }

    pub fn invalidate(&self, relay: &Url) {
        if let Some(source) = self.0.read().expect("evidence lock").clone() {
            source.invalidate(relay);
        }
    }
}

impl std::fmt::Debug for EvidenceSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EvidenceSlot")
            .field("set", &self.is_set())
            .finish()
    }
}

/// Classify an HTTP status on a protected relay route or WebSocket upgrade
/// using NIP-FI's exact denial byte contracts. Other 401/403 responses (for
/// example ordinary membership denials) are not NIP-FI denials.
pub fn relay_denial(
    status: u16,
    www_authenticate: Option<&str>,
    body: &[u8],
) -> Option<RelayDenial> {
    let text = std::str::from_utf8(body).unwrap_or_default().trim();
    match status {
        401 if www_authenticate.is_some_and(|value| value.trim().starts_with("Nostr"))
            && text == "authentication required" =>
        {
            Some(RelayDenial::EvidenceRequired)
        }
        403 if text == "evidence rejected" => Some(RelayDenial::EvidenceRejected),
        _ => None,
    }
}

/// Marker the session uses for a 1008 policy close after AUTH.
pub const POLICY_CLOSE_MARKER: &str = "relay policy close (1008)";

/// A relay denial at the WebSocket upgrade or after NIP-42, typed for UI and
/// diagnostics. Raw relay text never leaves this classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayDenial {
    /// `401` + `WWW-Authenticate: Nostr`: evidence missing.
    EvidenceRequired,
    /// `403 evidence rejected`.
    EvidenceRejected,
    /// `1008` policy close of an established session (deadline, revocation,
    /// community upgrade): reconnect with fresh evidence.
    PolicyClose,
    /// `1008` close during NIP-42: the evidence names another key or the key
    /// is denied. Terminal until the user acts.
    KeyRefused,
}

impl RelayDenial {
    pub const fn notice(self) -> &'static str {
        match self {
            Self::EvidenceRequired => {
                "this community requires federated sign-in; configure identity_adapter or sign in"
            }
            Self::EvidenceRejected => "this community rejected the federated identity evidence",
            Self::PolicyClose => {
                "the relay closed the session by policy; reconnecting with fresh evidence"
            }
            Self::KeyRefused => "the relay refused this key for federated identity",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_relay_urls_follow_nip_fa() {
        let canonical = |value: &str| canonical_relay_url(&Url::parse(value).unwrap());
        assert_eq!(
            canonical("wss://Community.Example.com/").unwrap(),
            "wss://community.example.com"
        );
        assert_eq!(
            canonical("wss://c.example.com:8443/path?x=1").unwrap(),
            "wss://c.example.com:8443"
        );
        assert_eq!(
            canonical("wss://[2001:db8::1]:8443").unwrap(),
            "wss://[2001:db8::1]:8443"
        );
        assert_eq!(
            canonical("ws://localhost:3030").unwrap(),
            "wss://localhost:3030"
        );
        assert!(canonical("ws://relay.example").is_err(), "port 80");
        assert_eq!(
            audience("wss://c.example.com:8443"),
            "https://c.example.com:8443"
        );
    }

    #[test]
    fn denial_table_maps_every_nip_fa_code() {
        let body = |code: &str| serde_json::to_vec(&json!({"error": code})).unwrap();
        assert_eq!(
            classify(401, &body("session_required"), None),
            Denial::SessionEnded
        );
        assert_eq!(
            classify(401, &body("session_expired"), None),
            Denial::SessionEnded
        );
        for (status, code, reason) in [
            (400, "invalid_request", RefusalReason::InvalidRequest),
            (
                403,
                "authorization_denied",
                RefusalReason::AuthorizationDenied,
            ),
            (403, "invalid_proof", RefusalReason::InvalidProof),
            (403, "binding_mismatch", RefusalReason::BindingMismatch),
            (413, "request_too_large", RefusalReason::RequestTooLarge),
            (403, "made_up", RefusalReason::Undefined),
            (400, "session_required", RefusalReason::Undefined),
            (418, "invalid_request", RefusalReason::Undefined),
        ] {
            assert_eq!(
                classify(status, &body(code), None),
                Denial::Refused(reason),
                "{status} {code}"
            );
        }
        assert_eq!(
            classify(429, &body("rate_limited"), Some(Duration::from_secs(5))),
            Denial::Retry {
                after: Some(Duration::from_secs(5))
            }
        );
        assert_eq!(
            classify(503, b"not json", None),
            Denial::Retry { after: None }
        );
        assert_eq!(
            classify(502, &body("invalid_proof"), None),
            Denial::Retry { after: None }
        );
    }

    #[test]
    fn relay_denials_require_the_exact_nip_fi_contracts() {
        assert_eq!(
            relay_denial(401, Some("Nostr"), b"authentication required\n"),
            Some(RelayDenial::EvidenceRequired)
        );
        assert_eq!(relay_denial(401, None, b"authentication required\n"), None);
        assert_eq!(
            relay_denial(403, None, b"evidence rejected\n"),
            Some(RelayDenial::EvidenceRejected)
        );
        assert_eq!(
            relay_denial(403, None, br#"{"error":"not a member"}"#),
            None
        );
        assert_eq!(
            relay_denial(500, Some("Nostr"), b"authentication required"),
            None
        );
    }

    #[test]
    fn requirement_reads_host_bound_nip11() {
        assert_eq!(requirement(&json!({})), Requirement::NotSupported);
        assert_eq!(
            requirement(&json!({"limitation":{"federated_identity":true}})),
            Requirement::Optional
        );
        assert_eq!(
            requirement(&json!({"requires_nip_fi":true})),
            Requirement::Required
        );
        assert_eq!(
            requirement(&json!({"requires_nip_fi":false})),
            Requirement::NotSupported
        );
    }

    #[test]
    fn secrets_never_appear_in_debug() {
        let session = Session {
            token: SecretString::from("session-sentinel-123".to_owned()),
            expires_at: OffsetDateTime::UNIX_EPOCH,
        };
        let assertion = Assertion {
            jws: SecretString::from("assertion-sentinel-456".to_owned()),
            expires_at: 10,
        };
        let rendered = format!("{session:?} {assertion:?}");
        assert!(!rendered.contains("sentinel"), "{rendered}");
    }

    #[test]
    fn adapter_origin_must_be_https_or_loopback() {
        assert!(AdapterClient::new(Url::parse("https://idp.example/").unwrap()).is_ok());
        assert!(AdapterClient::new(Url::parse("http://127.0.0.1:9/").unwrap()).is_ok());
        assert!(AdapterClient::new(Url::parse("http://idp.example/").unwrap()).is_err());
        assert!(AdapterClient::new(Url::parse("https://u:p@idp.example/").unwrap()).is_err());
    }

    #[test]
    fn remint_happens_inside_the_margin() {
        let clock = crate::clock::ManualClock::new(1_000);
        let assertion = Assertion {
            jws: SecretString::from("x".to_owned()),
            expires_at: 1_000 + REMINT_MARGIN_SECONDS + 1,
        };
        assert!(!assertion.needs_remint(clock.as_ref()));
        clock.advance(1);
        assert!(assertion.needs_remint(clock.as_ref()));
    }
}
