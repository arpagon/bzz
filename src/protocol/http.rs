use std::time::Duration;

use futures_util::StreamExt as _;
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
        let bytes = read_bounded_response(response, 1024 * 1024).await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| Error::Protocol(format!("invalid NIP-11 response: {error}")))
    }

    pub async fn query(&self, filters: &[QueryFilter]) -> Result<Vec<Event>> {
        if filters.len() > 16 {
            return Err(Error::Config(
                "a query may contain at most 16 filters".into(),
            ));
        }
        for filter in filters {
            filter.validate()?;
        }
        let body =
            serde_json::to_vec(filters).map_err(|error| Error::Serialization(error.to_string()))?;
        let bytes = self.post("query", body).await?;
        let response: QueryResponse = serde_json::from_slice(&bytes)
            .map_err(|error| Error::Protocol(format!("invalid query response: {error}")))?;
        let events = response.into_events();
        let maximum_events = filters
            .iter()
            .try_fold(0_usize, |total, filter| {
                total.checked_add(filter.limit.unwrap_or(500) as usize)
            })
            .ok_or_else(|| Error::Protocol("query response count overflow".into()))?;
        if events.len() > maximum_events {
            return Err(Error::Protocol(
                "relay returned more events than the query limit".into(),
            ));
        }
        Ok(events)
    }

    pub async fn event(&self, event: &Event) -> Result<Value> {
        let body = serde_json::to_vec(&json!({ "event": event }))
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let bytes = self.post("events", body).await?;
        serde_json::from_slice(&bytes).map_err(|error| Error::Protocol(error.to_string()))
    }

    /// The base URL this client is bound to.
    pub fn base(&self) -> &Url {
        &self.base
    }

    /// One NIP-98-authenticated POST with no retry, returning the HTTP status,
    /// the bounded body and the ID of the NIP-98 event that authorized it.
    /// Request-bound projections (NIP-CL 39008, NIP-RS snapshots) bind their
    /// response to that ID, so each attempt must sign a fresh event.
    pub async fn post_once_bound(
        &self,
        path: &str,
        body: Vec<u8>,
        maximum: usize,
    ) -> Result<(u16, Vec<u8>, String)> {
        let url = self
            .base
            .join(path)
            .map_err(|error| Error::Config(error.to_string()))?;
        let auth = self
            .signer
            .nip98_header("POST", url.as_str(), Some(&body))
            .await?;
        let request_id = nip98_event_id(&auth)?;
        let response = self
            .client
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| Error::Network(error.to_string()))?;
        let status = response.status().as_u16();
        let bytes = read_bounded_response(response, maximum).await?;
        Ok((status, bytes, request_id))
    }

    /// Fetch and verify this viewer's NIP-CL compact membership snapshot.
    /// `Ok(None)` means the relay does not advertise the capability.
    pub async fn membership_snapshot(
        &self,
        info: &Value,
        pinned_relay_pubkey: &str,
        viewer: &str,
    ) -> Result<Option<crate::protocol::membership_snapshot::VerifiedSnapshot>> {
        use crate::protocol::membership_snapshot as snapshot;
        let Some(descriptor) = snapshot::descriptor(info) else {
            return Ok(None);
        };
        if descriptor.relay_pubkey != pinned_relay_pubkey {
            return Err(Error::Protocol(
                "membership snapshot descriptor names an unpinned relay key".into(),
            ));
        }
        let url = self
            .base
            .join("query")
            .map_err(|error| Error::Config(error.to_string()))?;
        let host = snapshot::canonical_host(&url)
            .ok_or_else(|| Error::Config("relay URL has no host".into()))?;
        let body = snapshot::request_body(viewer, &snapshot::ALL_TYPES);
        let (status, bytes, request_id) = self
            .post_once_bound("query", body, descriptor.max_response_bytes)
            .await?;
        if status != 200 {
            return Err(Error::Network(format!(
                "membership snapshot returned HTTP {status}"
            )));
        }
        snapshot::verify(
            &bytes,
            &snapshot::Expected {
                relay_pubkey: pinned_relay_pubkey,
                community_id: descriptor.community_id,
                host: &host,
                viewer,
                request_id: &request_id,
                types: &snapshot::ALL_TYPES,
            },
        )
        .map(Some)
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
            let bytes = read_bounded_response(response, 32 * 1024 * 1024).await?;
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

async fn read_bounded_response(response: reqwest::Response, maximum: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(Error::Protocol(
            "relay response exceeds the size limit".into(),
        ));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| Error::Network(error.to_string()))?;
        let next = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| Error::Protocol("relay response size overflow".into()))?;
        if next > maximum {
            return Err(Error::Protocol(
                "relay response exceeds the size limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The event ID inside an `Authorization: Nostr <base64>` NIP-98 header.
fn nip98_event_id(header: &str) -> Result<String> {
    use base64::Engine as _;
    use nostr::JsonUtil as _;
    let encoded = header
        .strip_prefix("Nostr ")
        .ok_or_else(|| Error::Protocol("NIP-98 header has no Nostr scheme".into()))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::Protocol("NIP-98 header is not base64".into()))?;
    let event = nostr::Event::from_json(decoded)
        .map_err(|_| Error::Protocol("NIP-98 header is not an event".into()))?;
    Ok(event.id.to_hex())
}
