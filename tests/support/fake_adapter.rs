//! Fake NIP-FA adapter and NIP-FI issuer (v0.13 T3).
//!
//! Implements browser login (S256 handoff, single-use codes), code exchange,
//! session check and relay assertions. Assertions are real ES256 compact JWS
//! with protected `typ` `nip-fi+jwt`, verifiable by the pinned Buzz relay via
//! the JWKS this adapter serves at `/.well-known/jwks.json`. Every request is
//! recorded so tests can assert what bzz sent; a scripted `override` makes any
//! route return any denial.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use nostr::JsonUtil as _;
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::scripted_relay::{HttpReply, HttpRequest, Script, ScriptedRelay};

pub const SUBJECT: &str = "employee-0001";

#[derive(Default)]
struct State {
    challenges: HashMap<String, String>,
    codes: HashMap<String, String>,
    sessions: HashMap<String, u64>,
    used_proofs: HashSet<String>,
    /// route -> (status, body) forced for the next calls.
    overrides: HashMap<String, (u16, Value)>,
    issued: Vec<Value>,
    /// session token -> the owner key it proved on the assertion route.
    session_owner: HashMap<String, String>,
    /// agent grant -> (agent key, owner key).
    grants: HashMap<String, (String, String)>,
    /// agent key -> owner key, for good.
    enrolled: HashMap<String, String>,
    revoked: HashSet<String>,
}

pub struct FakeAdapter {
    pub server: ScriptedRelay,
    pub issuer: String,
    state: Arc<Mutex<State>>,
}

pub struct Config {
    /// Lifetime of a minted assertion in seconds.
    pub assertion_ttl: u64,
    /// Adapter session lifetime in seconds.
    pub session_ttl: u64,
    /// Report a different session expiry from `/v1/session`.
    pub mismatched_session_check: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            assertion_ttl: 600,
            session_ttl: 3600,
            mismatched_session_check: false,
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn rfc3339(unix: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(unix).unwrap())
        .unwrap()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn error(status: u16, code: &str) -> HttpReply {
    HttpReply::json(status, &json!({"error": code})).with_header("Cache-Control", "no-store")
}

/// Verify a `Nostr <base64>` NIP-98 proof for `url`/POST over `body`.
fn verify_proof(header: Option<&str>, url: &str, body: &[u8]) -> Option<nostr::Event> {
    let encoded = header?.strip_prefix("Nostr ")?;
    let event = nostr::Event::from_json(STANDARD.decode(encoded).ok()?).ok()?;
    event.verify().ok()?;
    let tag = |name: &str| bzz::protocol::events::first_tag(&event, name);
    let fresh = now().saturating_sub(event.created_at.as_secs()) <= 60;
    (event.kind.as_u16() == 27_235
        && tag("u").as_deref() == Some(url)
        && tag("method").as_deref() == Some("POST")
        && tag("payload").as_deref() == Some(&hex::encode(Sha256::digest(body)))
        && fresh)
        .then_some(event)
}

impl FakeAdapter {
    pub async fn start(config: Config) -> Self {
        let key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
        Self::start_with("127.0.0.1:0", None, key, config).await
    }

    /// Fixed address, public origin (issuer) and key, for a long-lived issuer
    /// the pinned relay trusts.
    pub async fn start_with(
        address: &str,
        public_origin: Option<&str>,
        key: SigningKey,
        config: Config,
    ) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let public = p256::PublicKey::from(key.verifying_key());
        let jwk: Value = serde_json::from_str(&public.to_jwk_string()).unwrap();
        let base = Arc::new(Mutex::new(String::new()));
        let handler_state = state.clone();
        let handler_base = base.clone();
        let server = ScriptedRelay::start_on(
            address,
            Script::default().http(move |request| {
                let base = handler_base.lock().unwrap().clone();
                handle(&handler_state, &key, &jwk, &base, &config, request)
            }),
        )
        .await;
        let origin = public_origin.map_or_else(
            || server.http_url.as_str().trim_end_matches('/').to_owned(),
            |origin| origin.trim_end_matches('/').to_owned(),
        );
        *base.lock().unwrap() = origin.clone();
        Self {
            server,
            issuer: origin,
            state,
        }
    }

    pub fn base(&self) -> url::Url {
        url::Url::parse(&format!("{}/", self.issuer)).unwrap()
    }

    pub fn jwks_uri(&self) -> String {
        format!("{}/.well-known/jwks.json", self.issuer)
    }

    /// Force `route` (e.g. `/v1/identity/assertions`) to answer `status/code`.
    pub fn deny(&self, route: &str, status: u16, code: &str) {
        self.state
            .lock()
            .unwrap()
            .overrides
            .insert(route.into(), (status, json!({"error": code})));
    }

    pub fn clear_denials(&self) {
        self.state.lock().unwrap().overrides.clear();
    }

    pub fn revoke_sessions(&self) {
        self.state.lock().unwrap().sessions.clear();
    }

    /// Claims of every assertion issued so far.
    pub fn issued(&self) -> Vec<Value> {
        self.state.lock().unwrap().issued.clone()
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.server.http_requests()
    }

    /// Play the browser: follow the adapter's login redirect to bzz's
    /// loopback callback, as a user completing sign-in would.
    pub async fn browse(login: &url::Url) {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let response = client.get(login.clone()).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 302, "login start redirects");
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let callback = client.get(location).send().await.unwrap();
        assert_eq!(callback.status().as_u16(), 200);
    }
}

fn handle(
    state: &Mutex<State>,
    key: &SigningKey,
    jwk: &Value,
    base: &str,
    config: &Config,
    request: &HttpRequest,
) -> HttpReply {
    if let Some((status, body)) = state.lock().unwrap().overrides.get(&request.path).cloned() {
        return HttpReply::json(status, &body).with_header("Cache-Control", "no-store");
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/.well-known/jwks.json") => {
            let mut jwk = jwk.clone();
            jwk["kid"] = json!("bzz-test-1");
            jwk["alg"] = json!("ES256");
            jwk["use"] = json!("sig");
            HttpReply::json(200, &json!({"keys": [jwk]}))
        }
        ("GET", "/v1/login/start") => {
            let query = request.query.clone();
            let pairs: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            let (Some(return_to), Some(challenge), Some("S256")) = (
                pairs.get("return_to"),
                pairs.get("handoff_challenge"),
                pairs.get("handoff_challenge_method").map(String::as_str),
            ) else {
                return error(400, "invalid_request");
            };
            let callback = url::Url::parse(return_to).ok();
            if callback.as_ref().and_then(|url| url.host_str()) != Some("127.0.0.1")
                && callback.as_ref().and_then(|url| url.host_str()) != Some("[::1]")
            {
                return error(400, "invalid_request");
            }
            let code = bzz::sync::read_state::random_id(16);
            state
                .lock()
                .unwrap()
                .challenges
                .insert(code.clone(), challenge.clone());
            HttpReply::text(302, "").with_header("Location", &format!("{return_to}?code={code}"))
        }
        ("POST", "/v1/login/exchange") => {
            let body = request.json();
            let (Some(code), Some(secret)) =
                (body["code"].as_str(), body["handoff_secret"].as_str())
            else {
                return error(400, "invalid_request");
            };
            let mut state = state.lock().unwrap();
            let Some(challenge) = state.challenges.remove(code) else {
                return error(400, "invalid_request");
            };
            if URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes())) != challenge {
                return error(400, "invalid_request");
            }
            let token = bzz::sync::read_state::random_id(32);
            let expires = now() + config.session_ttl;
            state.sessions.insert(token.clone(), expires);
            state.codes.insert(code.into(), token.clone());
            HttpReply::json(
                200,
                &json!({"session_token": token, "expires_at": rfc3339(expires), "email": "employee@example.com"}),
            )
            .with_header("Cache-Control", "no-store")
        }
        ("GET", "/v1/session") => {
            let token = bearer(request);
            let expires =
                token.and_then(|token| state.lock().unwrap().sessions.get(token).copied());
            match expires {
                Some(expires) if expires > now() => {
                    let reported = if config.mismatched_session_check {
                        expires + 1
                    } else {
                        expires
                    };
                    HttpReply::json(200, &json!({"expires_at": rfc3339(reported)}))
                }
                Some(_) => error(401, "session_expired"),
                None => error(401, "session_required"),
            }
        }
        ("POST", "/v1/identity/assertions") => {
            let Some(token) = bearer(request) else {
                return error(401, "session_required");
            };
            let expires = state.lock().unwrap().sessions.get(token).copied();
            match expires {
                Some(expires) if expires > now() => {}
                Some(_) => return error(401, "session_expired"),
                None => return error(401, "session_required"),
            }
            if request.body.len() > 4096 {
                return error(413, "request_too_large");
            }
            let url = format!("{base}/v1/identity/assertions");
            let Some(proof) =
                verify_proof(request.header("nostr-authorization"), &url, &request.body)
            else {
                return error(403, "invalid_proof");
            };
            if !state.lock().unwrap().used_proofs.insert(proof.id.to_hex()) {
                return error(400, "invalid_request");
            }
            let body = request.json();
            let (Some(relay), Some(pubkey)) =
                (body["relay_url"].as_str(), body["nostr_pubkey"].as_str())
            else {
                return error(400, "invalid_request");
            };
            if pubkey != proof.pubkey.to_hex()
                || body.as_object().is_some_and(|object| object.len() != 2)
                || !relay.starts_with("wss://")
            {
                return error(400, "invalid_request");
            }
            let issued_at = now();
            let claims = json!({
                "iss": base,
                "sub": SUBJECT,
                "nostr_pubkey": pubkey,
                "aud": format!("https://{}", relay.trim_start_matches("wss://")),
                "iat": issued_at,
                "exp": issued_at + config.assertion_ttl,
                // Operator-authorized public names (NIP-FI verified profiles).
                "nostr_public_profile": {"display_name": "Test Employee", "username": "employee"},
            });
            let jws = sign_jws(key, &claims);
            {
                let mut state = state.lock().unwrap();
                state.issued.push(claims.clone());
                state
                    .session_owner
                    .insert(token.to_owned(), pubkey.to_owned());
            }
            HttpReply::json(
                200,
                &json!({"assertion": jws, "nostr_pubkey": pubkey, "expires_at": issued_at + config.assertion_ttl}),
            )
            .with_header("Cache-Control", "no-store")
        }
        ("POST", "/v1/nip-fi/agent-grant") => {
            if request.header("authorization").is_some() || request.body.len() > 8192 {
                return error(400, "invalid_request");
            }
            let fields = url::form_urlencoded::parse(&request.body)
                .into_owned()
                .collect::<Vec<_>>();
            let field = |name: &str| {
                let values = fields
                    .iter()
                    .filter(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
                    .collect::<Vec<_>>();
                (values.len() == 1).then(|| values[0].clone())
            };
            let (Some(token), Some(agent), Some(attestation)) = (
                field("subject_token"),
                field("agent_pubkey"),
                field("owner_attestation"),
            ) else {
                return error(400, "invalid_request");
            };
            if fields.len() != 6
                || field("grant_type").as_deref()
                    != Some("urn:ietf:params:oauth:grant-type:token-exchange")
                || field("subject_token_type").as_deref()
                    != Some("urn:nostr:params:oauth:token-type:nip-fa-session")
                || field("requested_token_type").as_deref()
                    != Some("urn:nostr:params:oauth:token-type:nip-fi-agent-grant")
            {
                return error(400, "invalid_request");
            }
            let url = format!("{base}/v1/nip-fi/agent-grant");
            let Some(proof) =
                verify_proof(request.header("nostr-authorization"), &url, &request.body)
            else {
                return error(400, "invalid_request");
            };
            let mut state = state.lock().unwrap();
            if proof.pubkey.to_hex() != agent || !state.used_proofs.insert(proof.id.to_hex()) {
                return error(400, "invalid_request");
            }
            if !state
                .sessions
                .get(&token)
                .is_some_and(|expires| *expires > now())
            {
                return error(400, "invalid_request");
            }
            let Ok(agent_key) = nostr::PublicKey::from_hex(&agent) else {
                return error(400, "invalid_request");
            };
            let Ok(owner) = buzz_sdk::nip_oa::verify_auth_tag(&attestation, &agent_key) else {
                return error(400, "invalid_grant");
            };
            let owner = owner.to_hex();
            let bound = state.session_owner.get(&token).cloned();
            if bound.as_deref().is_some_and(|bound| bound != owner)
                || state.revoked.contains(&agent)
                || state
                    .enrolled
                    .get(&agent)
                    .is_some_and(|known| *known != owner)
            {
                return error(400, "invalid_grant");
            }
            state.enrolled.insert(agent.clone(), owner.clone());
            // A new enrollment replaces every earlier grant for this key.
            state.grants.retain(|_, (key, _)| *key != agent);
            let grant = format!("grant-{}", hex::encode(rand::random::<[u8; 16]>()));
            state.grants.insert(grant.clone(), (agent, owner.clone()));
            HttpReply::json(
                200,
                &json!({"access_token": grant, "issued_token_type": "urn:nostr:params:oauth:token-type:nip-fi-agent-grant", "token_type": "N_A", "expires_in": 691200, "nostr_owner_pubkey": owner, "nip_fi_issuer": base}),
            )
            .with_header("Cache-Control", "no-store")
        }
        ("POST", "/v1/nip-fi/agent-assertion") => {
            let Some(grant) = bearer(request) else {
                return error(401, "invalid_grant");
            };
            let Some((agent, owner)) = state.lock().unwrap().grants.get(grant).cloned() else {
                return error(401, "invalid_grant");
            };
            let url = format!("{base}/v1/nip-fi/agent-assertion");
            let Some(proof) =
                verify_proof(request.header("nostr-authorization"), &url, &request.body)
            else {
                return error(403, "invalid_proof");
            };
            if !state.lock().unwrap().used_proofs.insert(proof.id.to_hex()) {
                return error(400, "invalid_request");
            }
            let body = request.json();
            let (Some(relay), Some(pubkey)) =
                (body["relay_url"].as_str(), body["nostr_pubkey"].as_str())
            else {
                return error(400, "invalid_request");
            };
            if pubkey != agent || proof.pubkey.to_hex() != agent {
                return error(400, "invalid_request");
            }
            let issued_at = now();
            let claims = json!({
                "iss": base,
                "sub": format!("agent:{agent}"),
                "nostr_pubkey": agent,
                "nostr_owner_pubkey": owner,
                "aud": format!("https://{}", relay.trim_start_matches("wss://")),
                "iat": issued_at,
                "exp": issued_at + config.assertion_ttl,
            });
            let jws = sign_jws_typed(key, &claims, "nip-fi-agent+jwt");
            state.lock().unwrap().issued.push(claims.clone());
            HttpReply::json(
                200,
                &json!({"assertion": jws, "nostr_pubkey": agent, "expires_at": issued_at + config.assertion_ttl}),
            )
            .with_header("Cache-Control", "no-store")
        }
        ("POST", "/v1/nip-fi/agent-revoke") => {
            let Some(token) = bearer(request) else {
                return error(401, "session_required");
            };
            let fields = url::form_urlencoded::parse(&request.body)
                .into_owned()
                .collect::<Vec<_>>();
            let [(name, agent)] = fields.as_slice() else {
                return error(400, "invalid_request");
            };
            if name != "agent_pubkey" {
                return error(400, "invalid_request");
            }
            let mut state = state.lock().unwrap();
            if !state
                .sessions
                .get(token)
                .is_some_and(|expires| *expires > now())
            {
                return error(401, "session_required");
            }
            let owner = state.session_owner.get(token).cloned();
            if owner.is_none() || state.enrolled.get(agent) != owner.as_ref() {
                return error(404, "agent_not_found");
            }
            state.revoked.insert(agent.clone());
            state.grants.retain(|_, (key, _)| key != agent);
            HttpReply::json(200, &json!({"revoked": true})).with_header("Cache-Control", "no-store")
        }
        _ => error(404, "not_found"),
    }
}

fn bearer(request: &HttpRequest) -> Option<&str> {
    request.header("authorization")?.strip_prefix("Bearer ")
}

pub fn sign_jws(key: &SigningKey, claims: &Value) -> String {
    sign_jws_typed(key, claims, "nip-fi+jwt")
}

pub fn sign_jws_typed(key: &SigningKey, claims: &Value, typ: &str) -> String {
    let header = json!({"alg": "ES256", "typ": typ, "kid": "bzz-test-1"});
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature: Signature = key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}
