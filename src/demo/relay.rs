//! The demo's relay: NIP-01 and NIP-42 over a WebSocket, Buzz's HTTP
//! `POST /query` and `POST /events`, NIP-11 and the media of the world,
//! held in memory on a loopback port the system picks.
//!
//! Every address but the media's starts with a random token
//! (`/<token>/`), and media is served only to a read authorization signed
//! by the viewer's key, which exists only in this process: another program
//! on the machine that finds the port gets nothing. The relay never opens a
//! connection of its own.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, EventId, JsonUtil as _, PublicKey};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::broadcast,
};
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{ErrorResponse, Request, Response},
    http::StatusCode,
};

use super::world::{Script, World, first, values};

/// Filter fields the relay understands (`search_mode` and `page` of
/// Buzz's search are taken as given: one page, words anywhere). A filter
/// with any other field (a window, a snapshot, an artifact view) matches
/// nothing, and the client takes its plain path.
const KNOWN_FIELDS: [&str; 9] = [
    "ids",
    "authors",
    "kinds",
    "since",
    "until",
    "limit",
    "search",
    "search_mode",
    "page",
];
const DEFAULT_LIMIT: usize = 500;

struct State {
    events: Vec<Event>,
    /// The channel of an event that names its target only by `e`
    /// (reactions, deletions), so that `#h` finds it.
    channel_of: HashMap<EventId, String>,
    script: Script,
}

struct Inner {
    state: Mutex<State>,
    live: broadcast::Sender<Event>,
    media: HashMap<String, Vec<u8>>,
    token: String,
    viewer: PublicKey,
    relay_pubkey: String,
}

#[derive(Clone)]
pub struct Relay {
    inner: Arc<Inner>,
}

impl Relay {
    pub fn new(world: World, token: String) -> Self {
        let mut channel_of = HashMap::new();
        for event in &world.events {
            if let Some(channel) = first(event, "h") {
                channel_of.insert(event.id, channel);
            }
        }
        let (live, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    events: world.events,
                    channel_of,
                    script: world.script,
                }),
                live,
                media: world.media,
                token,
                viewer: world.viewer.public_key(),
                relay_pubkey: world.relay_keys.public_key().to_hex(),
            }),
        }
    }

    /// Serves `listener` until the runtime stops.
    pub fn serve(self, listener: TcpListener) {
        tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                // Bound to 127.0.0.1; checked again, as the demo promises.
                if !peer.ip().is_loopback() {
                    continue;
                }
                tokio::spawn(self.clone().connection(stream));
            }
        });
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn prefix(&self) -> String {
        format!("/{}/", self.inner.token)
    }

    async fn connection(self, stream: TcpStream) {
        let mut peek = [0_u8; 2048];
        let Ok(read) = stream.peek(&mut peek).await else {
            return;
        };
        let head = String::from_utf8_lossy(&peek[..read]).to_ascii_lowercase();
        if head.contains("upgrade: websocket") {
            self.websocket(stream).await;
        } else {
            self.http(stream).await;
        }
    }

    // ------------------------------------------------------------- events

    /// Stores an event (or replaces the one it replaces) and sends it to
    /// every subscription. Ephemeral kinds are only sent.
    fn accept(&self, event: Event) {
        let kind = event.kind.as_u16();
        if !(20_000..30_000).contains(&kind) {
            let mut state = self.state();
            let replaceable = matches!(kind, 0 | 3) || (10_000..20_000).contains(&kind);
            let addressable = (30_000..40_000).contains(&kind);
            if replaceable || addressable {
                let d = first(&event, "d");
                state.events.retain(|old| {
                    !(old.kind == event.kind
                        && old.pubkey == event.pubkey
                        && (!addressable || first(old, "d") == d))
                });
            }
            let channel = first(&event, "h").or_else(|| {
                first(&event, "e")
                    .and_then(|target| EventId::from_hex(&target).ok())
                    .and_then(|target| state.channel_of.get(&target).cloned())
            });
            if let Some(channel) = channel {
                state.channel_of.insert(event.id, channel);
            }
            state.events.push(event.clone());
        }
        let _ = self.inner.live.send(event);
    }

    /// The viewer wrote: the script answers, each answer when it is due.
    fn answer(&self, event: &Event) {
        let replies = self.state().script.answer(event);
        for (delay, reply) in replies {
            let relay = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                if let Some(event) = reply.sign() {
                    relay.accept(event);
                }
            });
        }
    }

    fn matches(state: &State, filter: &Value, event: &Event, live: bool) -> bool {
        let Some(object) = filter.as_object() else {
            return false;
        };
        let listed = |field: &str, value: &str| {
            object
                .get(field)
                .and_then(Value::as_array)
                .is_none_or(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .any(|wanted| wanted.eq_ignore_ascii_case(value))
                })
        };
        if !listed("ids", &event.id.to_hex()) || !listed("authors", &event.pubkey.to_hex()) {
            return false;
        }
        if let Some(kinds) = object.get("kinds").and_then(Value::as_array)
            && !kinds
                .iter()
                .filter_map(Value::as_u64)
                .any(|kind| kind == u64::from(event.kind.as_u16()))
        {
            return false;
        }
        // A live event is new: `since`, `until` and `limit` were about the
        // stored ones.
        let at = event.created_at.as_secs();
        if !live
            && (object
                .get("since")
                .and_then(Value::as_u64)
                .is_some_and(|since| at < since)
                || object
                    .get("until")
                    .and_then(Value::as_u64)
                    .is_some_and(|until| at > until))
        {
            return false;
        }
        for (field, wanted) in object {
            let Some(name) = field.strip_prefix('#') else {
                continue;
            };
            let wanted = wanted
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            let mut present = values(event, name);
            if name == "h"
                && present.is_empty()
                && let Some(channel) = state.channel_of.get(&event.id)
            {
                present.push(channel.clone());
            }
            if !present.iter().any(|value| wanted.contains(&value.as_str())) {
                return false;
            }
        }
        if let Some(search) = object.get("search").and_then(Value::as_str) {
            let content = event.content.to_lowercase();
            if !search
                .to_lowercase()
                .split_whitespace()
                .all(|word| content.contains(word))
            {
                return false;
            }
        }
        true
    }

    fn understood(filter: &Value) -> bool {
        filter.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| key.starts_with('#') || KNOWN_FIELDS.contains(&key.as_str()))
        })
    }

    /// The stored events for `filters`, newest first, within each limit.
    fn query(&self, filters: &[Value]) -> Vec<Event> {
        let state = self.state();
        let mut found: Vec<Event> = Vec::new();
        for filter in filters.iter().filter(|filter| Self::understood(filter)) {
            let mut hits = state
                .events
                .iter()
                .filter(|event| Self::matches(&state, filter, event, false))
                .collect::<Vec<_>>();
            hits.sort_by_key(|event| std::cmp::Reverse(event.created_at));
            let limit = filter
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|limit| usize::try_from(limit).ok())
                .unwrap_or(DEFAULT_LIMIT);
            for event in hits.into_iter().take(limit) {
                if !found.iter().any(|known| known.id == event.id) {
                    found.push(event.clone());
                }
            }
        }
        found
    }

    // ---------------------------------------------------------- websocket

    async fn websocket(self, stream: TcpStream) {
        let token = self.inner.token.clone();
        // The handshake's callback type is tungstenite's own.
        #[allow(clippy::result_large_err)]
        let guard = move |request: &Request, response: Response| {
            let path = request.uri().path();
            if path.trim_end_matches('/') == format!("/{token}") {
                Ok(response)
            } else {
                let mut refused = ErrorResponse::new(None);
                *refused.status_mut() = StatusCode::NOT_FOUND;
                Err(refused)
            }
        };
        let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, guard).await else {
            return;
        };
        let mut live = self.inner.live.subscribe();
        let mut subscriptions: HashMap<String, Vec<Value>> = HashMap::new();
        let challenge = json!(["AUTH", format!("bzz-demo-{}", self.inner.token)]);
        if socket
            .send(Message::Text(challenge.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
        loop {
            let frames = tokio::select! {
                pushed = live.recv() => {
                    let event = match pushed {
                        Ok(event) => event,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return,
                    };
                    let state = self.state();
                    subscriptions
                        .iter()
                        .filter(|(_, filters)| {
                            filters
                                .iter()
                                .any(|filter| Self::matches(&state, filter, &event, true))
                        })
                        .map(|(id, _)| json!(["EVENT", id, event]))
                        .collect::<Vec<_>>()
                }
                message = socket.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            self.frame(&text, &mut subscriptions)
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            let _ = socket.send(Message::Pong(payload)).await;
                            continue;
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(_)) | None => return,
                    }
                }
            };
            for frame in frames {
                if socket
                    .send(Message::Text(frame.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    /// What the relay answers to one frame from the client.
    fn frame(&self, text: &str, subscriptions: &mut HashMap<String, Vec<Value>>) -> Vec<Value> {
        let Ok(Value::Array(frame)) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        match frame.first().and_then(Value::as_str) {
            Some("AUTH") => frame
                .get(1)
                .and_then(|event| Event::from_json(event.to_string()).ok())
                .map(|event| {
                    let ok = event.verify().is_ok() && event.pubkey == self.inner.viewer;
                    vec![json!([
                        "OK",
                        event.id.to_hex(),
                        ok,
                        if ok {
                            ""
                        } else {
                            "auth-required: not the demo's viewer"
                        }
                    ])]
                })
                .unwrap_or_default(),
            Some("EVENT") => frame
                .get(1)
                .and_then(|event| Event::from_json(event.to_string()).ok())
                .map(|event| vec![self.publish(event)])
                .unwrap_or_default(),
            Some("REQ") => {
                let Some(id) = frame.get(1).and_then(Value::as_str) else {
                    return Vec::new();
                };
                let filters = frame.get(2..).unwrap_or_default().to_vec();
                let mut replies = self
                    .query(&filters)
                    .into_iter()
                    .rev()
                    .map(|event| json!(["EVENT", id, event]))
                    .collect::<Vec<_>>();
                replies.push(json!(["EOSE", id]));
                subscriptions.insert(
                    id.to_owned(),
                    filters.into_iter().filter(Self::understood).collect(),
                );
                replies
            }
            Some("CLOSE") => {
                let Some(id) = frame.get(1).and_then(Value::as_str) else {
                    return Vec::new();
                };
                subscriptions.remove(id);
                vec![json!(["CLOSED", id, ""])]
            }
            _ => Vec::new(),
        }
    }

    /// An event the viewer sends, over either transport.
    fn publish(&self, event: Event) -> Value {
        let id = event.id.to_hex();
        if event.verify().is_err() {
            return json!(["OK", id, false, "invalid: bad signature"]);
        }
        if event.pubkey != self.inner.viewer {
            return json!([
                "OK",
                id,
                false,
                "restricted: only the demo's viewer writes here"
            ]);
        }
        self.accept(event.clone());
        self.answer(&event);
        json!(["OK", id, true, ""])
    }

    // --------------------------------------------------------------- http

    async fn http(self, mut stream: TcpStream) {
        while let Some(request) = read_request(&mut stream).await {
            let (status, kind, body) = self.route(&request);
            let head = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\r\n",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Error"),
                body.len()
            );
            if stream.write_all(head.as_bytes()).await.is_err()
                || stream.write_all(&body).await.is_err()
            {
                return;
            }
        }
    }

    fn route(&self, request: &HttpRequest) -> (StatusCode, &'static str, Vec<u8>) {
        const JSON: &str = "application/json";
        let not_found = (StatusCode::NOT_FOUND, "text/plain", b"not found\n".to_vec());
        if let Some(file) = request.path.strip_prefix("/media/") {
            if request.method != "GET" || !self.viewer_may_read(request) {
                return (StatusCode::UNAUTHORIZED, "text/plain", b"no\n".to_vec());
            }
            return match self.inner.media.get(file) {
                Some(bytes) => (StatusCode::OK, "image/jpeg", bytes.clone()),
                None => not_found,
            };
        }
        let Some(rest) = request.path.strip_prefix(&self.prefix()) else {
            return not_found;
        };
        match (request.method.as_str(), rest) {
            ("GET", "") => {
                let info = json!({
                    "name": "bzz demo",
                    "description": "A generated community. Nothing here is real, and nothing leaves this machine.",
                    "pubkey": self.inner.relay_pubkey,
                    "self": self.inner.relay_pubkey,
                    "supported_nips": [1, 11, 25, 29, 42, 50],
                    "software": "bzz demo",
                });
                (
                    StatusCode::OK,
                    "application/nostr+json",
                    info.to_string().into_bytes(),
                )
            }
            ("POST", "query") => {
                let filters = match serde_json::from_slice::<Value>(&request.body) {
                    Ok(Value::Array(filters)) => filters,
                    Ok(filter) => vec![filter],
                    Err(_) => Vec::new(),
                };
                let events = self.query(&filters);
                (
                    StatusCode::OK,
                    JSON,
                    serde_json::to_vec(&events).unwrap_or_default(),
                )
            }
            ("POST", "events") => {
                let event = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|body| Event::from_json(body["event"].to_string()).ok());
                let Some(event) = event else {
                    return (
                        StatusCode::BAD_REQUEST,
                        JSON,
                        br#"{"error":"invalid event"}"#.to_vec(),
                    );
                };
                let answer = self.publish(event);
                let accepted = answer[2].as_bool().unwrap_or(false);
                let body = json!({"accepted": accepted, "id": answer[1], "message": answer[3]});
                let status = if accepted {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                };
                (status, JSON, body.to_string().into_bytes())
            }
            _ => not_found,
        }
    }

    /// A Blossom read authorization (kind 24242) signed by the viewer.
    fn viewer_may_read(&self, request: &HttpRequest) -> bool {
        request
            .authorization
            .as_deref()
            .and_then(|value| value.strip_prefix("Nostr "))
            .and_then(|encoded| URL_SAFE_NO_PAD.decode(encoded.trim()).ok())
            .and_then(|json| Event::from_json(json).ok())
            .is_some_and(|event| {
                event.kind.as_u16() == 24_242
                    && event.pubkey == self.inner.viewer
                    && event.verify().is_ok()
            })
    }
}

struct HttpRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

const MAX_REQUEST: usize = 1 << 20;

async fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut buffer = Vec::new();
    let end = loop {
        let mut chunk = [0_u8; 8192];
        let read = tokio::time::timeout(Duration::from_secs(60), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > MAX_REQUEST {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first_line = lines.next()?.split(' ');
    let method = first_line.next()?.to_owned();
    let path = first_line.next()?.split('?').next()?.to_owned();
    let mut length = 0_usize;
    let mut authorization = None;
    for (name, value) in lines.filter_map(|line| line.split_once(':')) {
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().ok()?;
        } else if name.trim().eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_owned());
        }
    }
    if length > MAX_REQUEST {
        return None;
    }
    let mut body = buffer[end..].to_vec();
    while body.len() < length {
        let mut chunk = vec![0_u8; length - body.len()];
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Some(HttpRequest {
        method,
        path,
        authorization,
        body,
    })
}
