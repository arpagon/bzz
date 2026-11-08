//! Fake Buzz Desktop credential provider (v0.13 T5), protocol version 1.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use nostr::{
    JsonUtil as _, Keys,
    nips::nip44::{self, Version},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

#[derive(Clone)]
pub struct Behavior {
    /// Bound relays (`describe` and policy checks).
    pub relays: Vec<String>,
    /// `federated_identity` answer; empty string means "none".
    pub assertion: String,
    /// Return a signature over different content.
    pub tamper: bool,
    pub revoked: bool,
}

pub struct FakeProvider {
    pub url: url::Url,
    pub token: String,
    pub keys: Keys,
    pub behavior: Arc<Mutex<Behavior>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeProvider {
    pub async fn start(relays: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url =
            url::Url::parse(&format!("http://{}/rpc", listener.local_addr().unwrap())).unwrap();
        let keys = Keys::generate();
        let token = bzz::sync::read_state::random_id(32);
        let behavior = Arc::new(Mutex::new(Behavior {
            relays: relays.iter().map(|relay| (*relay).to_owned()).collect(),
            assertion: String::new(),
            tamper: false,
            revoked: false,
        }));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (server_keys, server_token, server_behavior, server_calls) =
            (keys.clone(), token.clone(), behavior.clone(), calls.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let keys = server_keys.clone();
                let token = server_token.clone();
                let behavior = server_behavior.lock().unwrap().clone();
                let calls = server_calls.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0_u8; 8192];
                    let (head_end, length) = loop {
                        let read = stream.read(&mut chunk).await.unwrap_or(0);
                        if read == 0 {
                            return;
                        }
                        buffer.extend_from_slice(&chunk[..read]);
                        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head =
                                String::from_utf8_lossy(&buffer[..position]).to_ascii_lowercase();
                            let length = head
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length:"))
                                .and_then(|value| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (position + 4, length);
                        }
                    };
                    while buffer.len() < head_end + length {
                        let read = stream.read(&mut chunk).await.unwrap_or(0);
                        if read == 0 {
                            break;
                        }
                        buffer.extend_from_slice(&chunk[..read]);
                    }
                    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
                    let request: Value =
                        serde_json::from_slice(&buffer[head_end..head_end + length])
                            .unwrap_or(Value::Null);
                    let reply = answer(&head, &request, &keys, &token, &behavior);
                    calls
                        .lock()
                        .unwrap()
                        .push(request["method"].as_str().unwrap_or("").to_owned());
                    let body = reply.to_string();
                    let _ = stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        Self {
            url,
            token,
            keys,
            behavior,
            calls,
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

fn failure(id: &Value, code: &str) -> Value {
    json!({"id": id, "result": "", "error": code})
}

fn answer(head: &str, request: &Value, keys: &Keys, token: &str, behavior: &Behavior) -> Value {
    let id = &request["id"];
    if !head.contains("Buzz-Credential-Provider: 1") {
        return json!({"id": id, "result": "", "error": "unsupported_version", "versions": [1]});
    }
    let method = request["method"].as_str().unwrap_or_default();
    if method != "pair"
        && (!head.contains(&format!("Authorization: Bearer {token}")) || behavior.revoked)
    {
        return failure(id, "revoked");
    }
    let params = request["params"].as_array().cloned().unwrap_or_default();
    let param = |index: usize| {
        params
            .get(index)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match method {
        "get_public_key" => json!({"id": id, "result": keys.public_key().to_hex()}),
        "describe" => json!({"id": id, "result": json!({
            "instance": "dev.test.buzz",
            "pubkey": keys.public_key().to_hex(),
            "role": "owner",
            "relays": behavior.relays,
            "methods": ["get_public_key","sign_event","nip44_encrypt","nip44_decrypt","describe","federated_identity"],
        }).to_string()}),
        "federated_identity" => json!({"id": id, "result": behavior.assertion}),
        "pair" => json!({"id": id, "result": json!({
            "token": token, "instance": "dev.test.buzz",
            "pubkey": keys.public_key().to_hex(), "relays": [param(1)],
        }).to_string()}),
        "nip44_encrypt" => {
            let peer = nostr::PublicKey::from_hex(&param(0)).unwrap();
            json!({"id": id, "result": nip44::encrypt(keys.secret_key(), &peer, param(1), Version::V2).unwrap()})
        }
        "nip44_decrypt" => {
            let peer = nostr::PublicKey::from_hex(&param(0)).unwrap();
            match nip44::decrypt(keys.secret_key(), &peer, param(1)) {
                Ok(plaintext) => json!({"id": id, "result": plaintext}),
                Err(_) => failure(id, "invalid_request"),
            }
        }
        "sign_event" => {
            let unsigned: Value = serde_json::from_str(&param(0)).unwrap_or(Value::Null);
            if unsigned["pubkey"]
                .as_str()
                .is_some_and(|pubkey| pubkey != keys.public_key().to_hex())
            {
                return failure(id, "denied");
            }
            let kind = unsigned["kind"].as_u64().unwrap_or(u64::MAX);
            // Sensitive (profile) needs approval; unknown kinds are denied.
            match kind {
                0 => return failure(id, "approval_required"),
                9 | 7 | 22_242 | 27_235 | 24_242 | 30_078 | 5 => {}
                _ => return failure(id, "denied"),
            }
            let tags: Vec<nostr::Tag> = serde_json::from_value(unsigned["tags"].clone()).unwrap();
            if kind == 27_235 {
                let u = tags
                    .iter()
                    .find(|tag| tag.as_slice()[0] == "u")
                    .map(|tag| tag.as_slice()[1].clone());
                let bound = u
                    .as_deref()
                    .and_then(|u| url::Url::parse(u).ok())
                    .is_some_and(|u| {
                        behavior.relays.iter().any(|relay| {
                            let relay = url::Url::parse(relay).unwrap();
                            relay.host_str() == u.host_str()
                                && relay.port_or_known_default() == u.port_or_known_default()
                        })
                    });
                if !bound {
                    return failure(id, "denied");
                }
            }
            let content = if behavior.tamper {
                format!(
                    "{} (tampered)",
                    unsigned["content"].as_str().unwrap_or_default()
                )
            } else {
                unsigned["content"].as_str().unwrap_or_default().to_owned()
            };
            let event = nostr::EventBuilder::new(nostr::Kind::Custom(kind as u16), content)
                .tags(tags)
                .custom_created_at(nostr::Timestamp::from(
                    unsigned["created_at"].as_u64().unwrap(),
                ))
                .allow_self_tagging()
                .sign_with_keys(keys)
                .unwrap();
            json!({"id": id, "result": event.as_json()})
        }
        _ => failure(id, "unknown_method"),
    }
}
