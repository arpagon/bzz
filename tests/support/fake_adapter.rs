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
            });
            let jws = sign_jws(key, &claims);
            state.lock().unwrap().issued.push(claims.clone());
            HttpReply::json(
                200,
                &json!({"assertion": jws, "nostr_pubkey": pubkey, "expires_at": issued_at + config.assertion_ttl}),
            )
            .with_header("Cache-Control", "no-store")
        }
        _ => error(404, "not_found"),
    }
}

fn bearer(request: &HttpRequest) -> Option<&str> {
    request.header("authorization")?.strip_prefix("Bearer ")
}

pub fn sign_jws(key: &SigningKey, claims: &Value) -> String {
    let header = json!({"alg": "ES256", "typ": "nip-fi+jwt", "kid": "bzz-test-1"});
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
