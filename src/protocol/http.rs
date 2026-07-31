use std::time::Duration;

use nostr::Event;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use url::Url;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::types::{QueryFilter, QueryResponse},
};

#[derive(Clone)]
pub struct HttpClient {
    base: Url,
    client: Client,
    signer: SignerHandle,
}

pub fn relay_signing_pubkey(info: &Value) -> Option<&str> {
    info.get("self")
        .and_then(Value::as_str)
        .or_else(|| info.get("pubkey").and_then(Value::as_str))
        .filter(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

impl HttpClient {
    pub fn new(base: Url, signer: SignerHandle) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("bzz/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Network(error.to_string()))?;
        Ok(Self {
            base,
            client,
            signer,
        })
    }

    pub async fn nip11(&self) -> Result<Value> {
        let response = self
            .client
            .get(self.base.clone())
            .header("Accept", "application/nostr+json")
            .send()
            .await
            .map_err(|error| Error::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Network(format!(
                "NIP-11 returned {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|error| Error::Protocol(error.to_string()))
    }

    pub async fn query(&self, filters: &[QueryFilter]) -> Result<Vec<Event>> {
        let body =
            serde_json::to_vec(filters).map_err(|error| Error::Serialization(error.to_string()))?;
        let bytes = self.post("query", body).await?;
        let response: QueryResponse = serde_json::from_slice(&bytes)
            .map_err(|error| Error::Protocol(format!("invalid query response: {error}")))?;
        Ok(response.into_events())
    }

    pub async fn event(&self, event: &Event) -> Result<Value> {
        let body = serde_json::to_vec(&json!({ "event": event }))
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let bytes = self.post("events", body).await?;
        serde_json::from_slice(&bytes).map_err(|error| Error::Protocol(error.to_string()))
    }

    async fn post(&self, path: &str, body: Vec<u8>) -> Result<Vec<u8>> {
        let url = self
            .base
            .join(path)
            .map_err(|error| Error::Config(error.to_string()))?;
        let mut attempt = 0_u32;
        loop {
            let auth = self
                .signer
                .nip98_header("POST", url.as_str(), Some(&body))
                .await?;
            let result = self
                .client
                .post(url.clone())
                .header("Authorization", auth)
                .header("Content-Type", "application/json")
                .body(body.clone())
                .send()
                .await;
            let response = match result {
                Ok(response) => response,
                Err(error) if error.is_connect() && attempt < 2 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                    continue;
                }
                Err(error) => return Err(Error::Network(error.to_string())),
            };
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .map_err(|error| Error::Network(error.to_string()))?;
            if status.is_success() {
                return Ok(bytes.to_vec());
            }
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await;
                continue;
            }
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .or_else(|| value.get("message"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| format!("HTTP {status}"));
            return if matches!(status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) {
                Err(Error::Access(message))
            } else {
                Err(Error::Network(message))
            };
        }
    }
}
