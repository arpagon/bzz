//! Joining a community (v0.17 A9): what `bzz community add` does and what
//! the join form in the interface does, in one place. One path, two doors:
//! the CLI and `ui::join` call these functions and nothing else touches the
//! configuration of a new community.
//!
//! The order is the same at both doors: read what answers at the address
//! (its NIP-11 document), then make or choose the identity, then ask the
//! relay whether that identity may come in, and only when it may is the
//! community written. Nothing that cannot connect is left behind.

use std::{fmt, time::Duration};

use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::{federated::Requirement, signer::SignerHandle},
    config::{Config, RelayEndpoint, validate_relay_url},
    error::{Error, Result},
    paths::Paths,
    render::sanitize,
    store::Store,
};

/// How long the address check and the invite claim wait for an answer.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(15);

/// A community address as a person types it, made into the relay's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayAddress {
    /// The relay's WebSocket origin, as the configuration keeps it.
    pub url: String,
    /// `ws://` on this machine (a test relay): no TLS, loopback only.
    pub insecure: bool,
}

impl RelayAddress {
    /// The relay's WebSocket and HTTP origins.
    pub fn endpoint(&self) -> Result<RelayEndpoint> {
        validate_relay_url(&self.url, self.insecure)
    }

    /// The host (and port) the person reads: `my-team.example`.
    pub fn host(&self) -> String {
        url::Url::parse(&self.url)
            .ok()
            .and_then(|url| {
                url.host_str().map(|host| match url.port() {
                    Some(port) => format!("{host}:{port}"),
                    None => host.to_owned(),
                })
            })
            .unwrap_or_else(|| self.url.clone())
    }
}

/// An invite code: a bearer of an admin's permission to join. It is never
/// printed, logged or put in a diagnostic; only the relay it came for sees
/// it, in the body of the claim.
#[derive(Clone)]
pub struct InviteCode(SecretString);

impl InviteCode {
    pub fn new(code: &str) -> Self {
        Self(SecretString::from(code.to_owned()))
    }

    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for InviteCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InviteCode(…)")
    }
}

impl PartialEq for InviteCode {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

/// What a person typed: a community's address, or an invite to one.
#[derive(Clone, Debug, PartialEq)]
pub enum Address {
    Relay(RelayAddress),
    /// An invite link, as Buzz makes them at the pinned revision:
    /// `https://<relay>/invite/<code>` or `buzz://join?relay=…&code=…`.
    Invite {
        relay: RelayAddress,
        code: InviteCode,
    },
}

impl Address {
    pub fn relay(&self) -> &RelayAddress {
        match self {
            Self::Relay(relay) | Self::Invite { relay, .. } => relay,
        }
    }

    pub fn invite(&self) -> Option<&InviteCode> {
        match self {
            Self::Relay(_) => None,
            Self::Invite { code, .. } => Some(code),
        }
    }
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// The relay address of `url`'s origin; `ws://` only on this machine.
fn relay_of(url: &url::Url, secure: bool) -> std::result::Result<RelayAddress, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "that address has no host".to_owned())?;
    if url.username() != "" || url.password().is_some() {
        return Err("an address with a user name or password in it is refused".into());
    }
    let insecure = !secure;
    if insecure && !is_loopback(host) {
        return Err("ws:// and http:// work only on this machine; use wss://".into());
    }
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    let candidate = format!("{}://{authority}/", if secure { "wss" } else { "ws" });
    let endpoint = validate_relay_url(&candidate, insecure).map_err(|error| error.to_string())?;
    Ok(RelayAddress {
        url: endpoint.websocket.to_string(),
        insecure,
    })
}

/// Reads what a person pasted or typed: `wss://host`, a bare `host`, an
/// `https://host` web address, or an invite link. A bare host on this
/// machine (`localhost:3030`) is a test relay without TLS.
pub fn parse_address(input: &str) -> std::result::Result<Address, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("type the community's address".into());
    }
    if input.chars().any(char::is_whitespace) {
        return Err("an address has no spaces".into());
    }
    if let Some(rest) = input.strip_prefix("buzz://") {
        let url = url::Url::parse(&format!("buzz://{rest}"))
            .map_err(|_| "that invite link is not complete".to_owned())?;
        if url.host_str() != Some("join") {
            return Err("bzz reads buzz://join links only".into());
        }
        let mut relay = None;
        let mut code = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "relay" => relay = Some(value.into_owned()),
                "code" => code = Some(value.into_owned()),
                _ => {}
            }
        }
        let (Some(relay), Some(code)) = (relay, code) else {
            return Err("that invite link has no relay or no code".into());
        };
        let relay = url::Url::parse(&relay).map_err(|_| "the invite's relay is not an address")?;
        let secure = match relay.scheme() {
            "wss" => true,
            "ws" => false,
            _ => return Err("the invite's relay is not a ws:// or wss:// address".into()),
        };
        if relay.fragment().is_some() || code.is_empty() {
            return Err("that invite link is not complete".into());
        }
        return Ok(Address::Invite {
            relay: relay_of(&relay, secure)?,
            code: InviteCode::new(&code),
        });
    }
    if input.contains("://") {
        let url = url::Url::parse(input).map_err(|_| "that is not an address bzz can read")?;
        let secure = match url.scheme() {
            "wss" | "https" => true,
            "ws" | "http" => false,
            other => return Err(format!("bzz cannot join a {other}:// address")),
        };
        if url.fragment().is_some() {
            return Err("an address with a # part is refused".into());
        }
        let path = url.path().trim_end_matches('/');
        if matches!(url.scheme(), "https" | "http")
            && let Some(code) = path.strip_prefix("/invite/")
            && !code.is_empty()
            && !code.contains('/')
        {
            let code = percent_decode(code);
            return Ok(Address::Invite {
                relay: relay_of(&url, secure)?,
                code: InviteCode::new(&code),
            });
        }
        if !path.is_empty() || url.query().is_some() {
            return Err(
                "a community's address is its host alone (wss://host), with no path".into(),
            );
        }
        return Ok(Address::Relay(relay_of(&url, secure)?));
    }
    if input.contains('/') {
        return Err("a community's address is its host alone, with no path".into());
    }
    if input.starts_with(buzz_core::invite::V2_PREFIX) {
        return Err(
            "that is an invite code alone; paste the whole invite link your admin sent".into(),
        );
    }
    let url = url::Url::parse(&format!("wss://{input}"))
        .map_err(|_| "that is not an address bzz can read")?;
    let secure = !url.host_str().is_some_and(is_loopback);
    Ok(Address::Relay(relay_of(&url, secure)?))
}

fn percent_decode(text: &str) -> String {
    url::form_urlencoded::parse(format!("c={text}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| text.to_owned())
}

/// What answers at an address, read from its NIP-11 document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayFacts {
    /// The community's name and description, made safe to print.
    pub name: Option<String>,
    pub description: Option<String>,
    /// The relay's signing key, pinned when the community is written.
    pub relay_pubkey: String,
    /// The relay lets in only its members (it advertises NIP-43).
    pub members_only: bool,
    /// What it says about sign-in through an identity provider (NIP-FI).
    pub federated: Requirement,
    /// NIPs bzz needs that it does not list.
    pub missing_nips: Vec<u64>,
}

impl RelayFacts {
    /// Reads a NIP-11 document; `None` when it is not a Buzz relay's.
    pub fn read(info: &Value) -> Option<Self> {
        let relay_pubkey = crate::protocol::http::relay_signing_pubkey(info)?.to_owned();
        let text = |key: &str| {
            info.get(key)
                .and_then(Value::as_str)
                .map(|value| sanitize::single_line(value).trim().to_owned())
                .filter(|value| !value.is_empty())
                .map(|value| value.chars().take(120).collect::<String>())
        };
        let members_only = info
            .get("supported_nips")
            .and_then(Value::as_array)
            .is_some_and(|nips| nips.iter().any(|nip| nip.as_u64() == Some(43)));
        Some(Self {
            name: text("name"),
            description: text("description"),
            relay_pubkey,
            members_only,
            federated: crate::auth::federated::requirement(info),
            missing_nips: crate::protocol::http::missing_required_nips(info),
        })
    }

    /// The label the community and its new identity get: the relay's
    /// name in a form fit for a label, else its host.
    pub fn label(&self, relay: &RelayAddress) -> String {
        let name = self
            .name
            .as_deref()
            .map(|name| name.chars().take(40).collect::<String>())
            .filter(|name| !name.trim().is_empty());
        name.unwrap_or_else(|| relay.host())
    }
}

/// Why nothing could be read at an address. Each says what happened and
/// what to try, in the person's words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeFailure {
    /// Nothing answered: no host, no route, refused, or too slow.
    NoAnswer,
    /// A Buzz relay answered, with no community at this address.
    NoCommunity,
    /// Something answered that is not a relay bzz speaks to.
    NotBuzz,
}

impl ProbeFailure {
    pub fn explain(&self, relay: &RelayAddress) -> (String, &'static str) {
        let host = relay.host();
        match self {
            Self::NoAnswer => (
                format!("nothing answers at {host}"),
                "check the address with the person who gave it to you, and this machine's network",
            ),
            Self::NoCommunity => (
                format!("{host} is a Buzz relay with no community at this address"),
                "check the address: a community has its own host name",
            ),
            Self::NotBuzz => (
                format!("something answers at {host}, but it is not a Buzz community"),
                "bzz speaks to Buzz relays only; check the address",
            ),
        }
    }
}

/// Reads the NIP-11 document at `relay`. The CLI's `community add` and the
/// join form both ask this before anything is made.
pub async fn probe(relay: &RelayAddress) -> std::result::Result<RelayFacts, ProbeFailure> {
    let endpoint = relay.endpoint().map_err(|_| ProbeFailure::NotBuzz)?;
    let info = nip11(&endpoint).await?;
    RelayFacts::read(&info).ok_or(ProbeFailure::NotBuzz)
}

/// The relay's NIP-11 document, unauthenticated, following no redirect.
pub async fn nip11(endpoint: &RelayEndpoint) -> std::result::Result<Value, ProbeFailure> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(ANSWER_TIMEOUT)
        .user_agent(crate::realtime::session::USER_AGENT)
        .build()
        .map_err(|_| ProbeFailure::NoAnswer)?;
    let response = client
        .get(endpoint.http_base.clone())
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .map_err(|_| ProbeFailure::NoAnswer)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ProbeFailure::NoCommunity);
    }
    if !response.status().is_success() {
        return Err(ProbeFailure::NotBuzz);
    }
    let bytes = response.bytes().await.map_err(|_| ProbeFailure::NoAnswer)?;
    if bytes.len() > 1024 * 1024 {
        return Err(ProbeFailure::NotBuzz);
    }
    serde_json::from_slice(&bytes).map_err(|_| ProbeFailure::NotBuzz)
}

/// Whether an identity may come in, as the relay answered its AUTH.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Admission {
    /// Authenticated: the community can be written and opened.
    In,
    /// The relay lets in members only and this identity is not one yet.
    NotMember,
    /// The relay refused this identity for another reason (a ban, a
    /// restriction): its words, made safe to print.
    Refused(String),
    /// The community asks for sign-in through its identity provider first.
    SignInFirst,
    /// No answer this time: the network, or the relay is busy.
    NoAnswer(String),
}

/// One AUTH with `signer` at `relay`, and the session closed at once.
pub async fn admission(relay: &RelayAddress, signer: &SignerHandle) -> Admission {
    let endpoint = match relay.endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return Admission::Refused(error.to_string()),
    };
    match crate::realtime::session::connect(endpoint.websocket, signer.clone()).await {
        Ok((session, _events)) => {
            session.shutdown().await;
            Admission::In
        }
        Err(error) => classify_admission(&error),
    }
}

/// What a failed AUTH means for the person joining.
pub fn classify_admission(error: &Error) -> Admission {
    let said = |message: &str| sanitize::single_line(message).chars().take(160).collect();
    match error {
        Error::Access(message) if message.to_ascii_lowercase().contains("not a relay member") => {
            Admission::NotMember
        }
        Error::Auth(message)
            if message == crate::auth::federated::RelayDenial::EvidenceRequired.notice() =>
        {
            Admission::SignInFirst
        }
        Error::Access(message) | Error::Auth(message) => Admission::Refused(said(message)),
        Error::Network(message) | Error::Timeout(message) => Admission::NoAnswer(said(message)),
        other => Admission::NoAnswer(said(&other.to_string())),
    }
}

/// What claiming an invite came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Claimed {
    /// The identity is a member now (or was already).
    Member,
    /// The relay refused the invite: its reason, in the person's words.
    Refused(String),
    NoAnswer,
}

/// Claims `code` at `relay` as `signer` (`POST /api/invites/claim`, NIP-98
/// signed by the joining key, as Buzz desktop does). The code goes in the
/// body only, to that relay only.
pub async fn claim_invite(
    relay: &RelayAddress,
    signer: &SignerHandle,
    code: &InviteCode,
) -> Claimed {
    let Ok(endpoint) = relay.endpoint() else {
        return Claimed::NoAnswer;
    };
    let Ok(http) = crate::protocol::http::HttpClient::new(endpoint.http_base, signer.clone())
    else {
        return Claimed::NoAnswer;
    };
    let body = match serde_json::to_vec(&serde_json::json!({ "code": code.expose() })) {
        Ok(body) => zeroize::Zeroizing::new(body),
        Err(_) => return Claimed::NoAnswer,
    };
    let answer = tokio::time::timeout(
        ANSWER_TIMEOUT,
        http.post_once_bound("api/invites/claim", body.to_vec(), 64 * 1024),
    )
    .await;
    let Ok(Ok((status, bytes, _))) = answer else {
        return Claimed::NoAnswer;
    };
    if (200..300).contains(&status) {
        return Claimed::Member;
    }
    let error = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    Claimed::Refused(invite_refusal(status, &error))
}

/// Buzz desktop's words for an invite the relay refused.
pub fn invite_refusal(status: u16, error: &str) -> String {
    match error {
        "invite_expired" => "this invite has expired; ask for a new one".into(),
        "invite_exhausted" => "this invite has reached its use limit; ask for a new one".into(),
        "invite_invalid" => "the relay does not know this invite; check the link".into(),
        "invite_restricted" => {
            "this identity is restricted in this community; ask one of its admins".into()
        }
        "join_policy_required" => {
            "this community asks you to accept its terms first; join it once in Buzz desktop or on the web"
                .into()
        }
        _ if status == 429 => "too many tries; wait a minute and try again".into(),
        "" => format!("the relay refused the invite (HTTP {status})"),
        other => format!(
            "the relay refused the invite: {}",
            sanitize::single_line(other).chars().take(120).collect::<String>()
        ),
    }
}

/// Writes a community whose relay answered: the configuration, the store's
/// row, and the relay key pinned. If any step fails, what was written is
/// taken back. The CLI's `community add` and the join form both end here.
pub fn add_community(
    paths: &Paths,
    config: &mut Config,
    label: String,
    relay: &RelayAddress,
    identity: Uuid,
    relay_pubkey: &str,
) -> Result<Uuid> {
    let before = config.default_community;
    let id = config.add_community(label, relay.url.clone(), identity, relay.insecure)?;
    let undo = |config: &mut Config| {
        config.communities.retain(|community| community.id != id);
        config.default_community = before;
    };
    if let Err(error) = config.save(paths) {
        undo(config);
        return Err(error);
    }
    let pinned = Store::open(paths.database_file()).and_then(|mut store| {
        store.sync_config(config)?;
        store.pin_relay_pubkey(id, relay_pubkey)
    });
    if let Err(error) = pinned {
        undo(config);
        let _ = config.save(paths);
        return Err(error);
    }
    Ok(id)
}

/// The relay an address names is already one of this person's communities.
pub fn already_joined(config: &Config, relay: &RelayAddress) -> Option<String> {
    let wanted = relay.endpoint().ok()?.websocket;
    config
        .communities
        .iter()
        .find(|community| {
            validate_relay_url(&community.relay_url, community.allow_insecure_localhost)
                .is_ok_and(|existing| existing.websocket == wanted)
        })
        .map(|community| community.label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay(input: &str) -> RelayAddress {
        match parse_address(input).unwrap() {
            Address::Relay(relay) => relay,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_address_is_read_as_people_paste_it() {
        assert_eq!(relay("my-team.example").url, "wss://my-team.example/");
        assert_eq!(relay("wss://my-team.example").url, "wss://my-team.example/");
        assert_eq!(
            relay("wss://my-team.example/").url,
            "wss://my-team.example/"
        );
        assert_eq!(
            relay("https://my-team.example").url,
            "wss://my-team.example/"
        );
        assert_eq!(
            relay("MY-TEAM.example:8443").url,
            "wss://my-team.example:8443/"
        );
        let local = relay("localhost:3030");
        assert_eq!(
            (local.url.as_str(), local.insecure),
            ("ws://localhost:3030/", true)
        );
        let local = relay("ws://127.0.0.1:3030");
        assert_eq!(
            (local.url.as_str(), local.insecure),
            ("ws://127.0.0.1:3030/", true)
        );
        assert_eq!(relay("my-team.example").host(), "my-team.example");
        assert_eq!(local.host(), "127.0.0.1:3030");
    }

    #[test]
    fn what_is_not_an_address_says_why() {
        for (input, said) in [
            ("", "type the community's address"),
            ("my team", "no spaces"),
            ("ws://my-team.example", "only on this machine"),
            ("http://my-team.example", "only on this machine"),
            ("wss://my-team.example/channels", "no path"),
            ("my-team.example/x", "no path"),
            ("wss://me:pw@my-team.example", "password"),
            ("ftp://my-team.example", "ftp://"),
            ("v2.abcdef", "invite code alone"),
        ] {
            let error = parse_address(input).unwrap_err();
            assert!(error.contains(said), "{input:?}: {error}");
        }
    }

    #[test]
    fn invite_links_of_the_pinned_buzz_are_read() {
        let Address::Invite { relay, code } =
            parse_address("https://my-team.example/invite/v2.Zm9v").unwrap()
        else {
            panic!("an invite");
        };
        assert_eq!(relay.url, "wss://my-team.example/");
        assert_eq!(code.expose(), "v2.Zm9v");
        let Address::Invite { relay, code } =
            parse_address("buzz://join?relay=wss%3A%2F%2Fmy-team.example&code=v2.Zm9v").unwrap()
        else {
            panic!("an invite");
        };
        assert_eq!(relay.url, "wss://my-team.example/");
        assert_eq!(code.expose(), "v2.Zm9v");
        let Address::Invite { relay, .. } =
            parse_address("http://localhost:3030/invite/abc").unwrap()
        else {
            panic!("an invite");
        };
        assert!(relay.insecure);
        // The code is a bearer: its Debug never shows it.
        assert!(!format!("{code:?}").contains("Zm9v"));
        assert!(parse_address("buzz://join?relay=wss://x.example").is_err());
        assert!(parse_address("buzz://open?relay=wss://x.example&code=a").is_err());
    }

    #[test]
    fn nip11_says_what_answers() {
        let key = "ab".repeat(32);
        let open = RelayFacts::read(&serde_json::json!({
            "name": "My team\u{1b}[31m", "description": "We ship", "self": key,
            "supported_nips": [1, 11, 29, 42]
        }))
        .unwrap();
        // Relay text is inert: no escape reaches the screen.
        let name = open.name.as_deref().unwrap();
        assert!(
            name.starts_with("My team") && !name.contains('\u{1b}'),
            "{name}"
        );
        assert!(!open.members_only);
        assert!(open.missing_nips.is_empty());
        assert_eq!(open.federated, Requirement::NotSupported);
        let closed = RelayFacts::read(&serde_json::json!({
            "self": key, "supported_nips": [29, 42, 43], "requires_nip_fi": true
        }))
        .unwrap();
        assert!(closed.members_only);
        assert_eq!(closed.federated, Requirement::Required);
        assert_eq!(closed.label(&relay("x.example")), "x.example");
        assert!(RelayFacts::read(&serde_json::json!({"name": "a plain nostr relay"})).is_none());
    }

    #[test]
    fn a_refused_auth_is_told_apart() {
        assert_eq!(
            classify_admission(&Error::Access("restricted: not a relay member".into())),
            Admission::NotMember
        );
        assert!(matches!(
            classify_admission(&Error::Access("blocked: you are banned".into())),
            Admission::Refused(said) if said.contains("banned")
        ));
        assert!(matches!(
            classify_admission(&Error::Network("connection refused".into())),
            Admission::NoAnswer(_)
        ));
        assert_eq!(
            classify_admission(&Error::Auth(
                crate::auth::federated::RelayDenial::EvidenceRequired
                    .notice()
                    .into()
            )),
            Admission::SignInFirst
        );
    }

    #[test]
    fn invite_refusals_are_said_as_buzz_desktop_says_them() {
        assert!(invite_refusal(403, "invite_expired").contains("expired"));
        assert!(invite_refusal(403, "invite_exhausted").contains("use limit"));
        assert!(invite_refusal(429, "x").contains("wait a minute"));
        assert!(invite_refusal(500, "").contains("HTTP 500"));
    }
}
