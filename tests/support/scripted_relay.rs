//! Programmable relay for protocol tests (v0.13 T2).
//!
//! One loopback port serves both HTTP (NIP-11, `POST /query`, `/events`,
//! `/media/*`) and the NIP-01 WebSocket. Behavior comes from closures, so each
//! test states exactly what the relay says, including forged or mismatched
//! signed responses and NIP-FI upgrade denials. Every HTTP request, upgrade and
//! inbound WebSocket frame is recorded with its virtual (`tokio::time`)
//! arrival time.
#![allow(dead_code, clippy::result_large_err)]

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, JsonUtil as _};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::Instant,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
        http::{HeaderValue, StatusCode},
    },
};

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub at: Instant,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

#[derive(Clone, Debug)]
pub struct HttpReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpReply {
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: value.to_string().into_bytes(),
        }
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// What the relay does with a WebSocket upgrade.
#[derive(Clone, Debug)]
pub enum Upgrade {
    Accept,
    Deny(HttpReply),
}

/// Inbound WebSocket frame as the relay saw it.
#[derive(Clone, Debug)]
pub struct WsFrame {
    pub connection: usize,
    pub value: Value,
    pub at: Instant,
}

/// Outbound action for one inbound frame.
#[derive(Clone, Debug)]
pub enum WsAction {
    Send(Value),
    /// Close with a WebSocket close code and reason (e.g. 1008 policy).
    Close(u16, String),
}

type HttpHandler = dyn Fn(&HttpRequest) -> HttpReply + Send + Sync;
type UpgradeHandler = dyn Fn(&HttpRequest) -> Upgrade + Send + Sync;
type FrameHandler = dyn Fn(&WsFrame) -> Vec<WsAction> + Send + Sync;

pub struct Script {
    http: Box<HttpHandler>,
    upgrade: Box<UpgradeHandler>,
    frame: Box<FrameHandler>,
    auth_challenge: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            http: Box::new(|request| default_http(request, &json!({}))),
            upgrade: Box::new(|_| Upgrade::Accept),
            frame: Box::new(default_frame),
            auth_challenge: true,
        }
    }
}

impl Script {
    /// Serve `info` as NIP-11 and an empty array for `/query`.
    pub fn nip11(mut self, info: Value) -> Self {
        self.http = Box::new(move |request| default_http(request, &info));
        self
    }

    pub fn http(
        mut self,
        handler: impl Fn(&HttpRequest) -> HttpReply + Send + Sync + 'static,
    ) -> Self {
        self.http = Box::new(handler);
        self
    }

    pub fn upgrade(
        mut self,
        handler: impl Fn(&HttpRequest) -> Upgrade + Send + Sync + 'static,
    ) -> Self {
        self.upgrade = Box::new(handler);
        self
    }

    /// Replace frame handling. `default_frame` remains available to delegate to.
    pub fn frames(
        mut self,
        handler: impl Fn(&WsFrame) -> Vec<WsAction> + Send + Sync + 'static,
    ) -> Self {
        self.frame = Box::new(handler);
        self
    }

    pub fn without_auth_challenge(mut self) -> Self {
        self.auth_challenge = false;
        self
    }
}

/// NIP-11 on `GET /` with `Accept: application/nostr+json`, `[]` for every
/// `POST /query`, and 404 otherwise.
pub fn default_http(request: &HttpRequest, info: &Value) -> HttpReply {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => HttpReply::json(200, info).with_header("Cache-Control", "no-store"),
        ("POST", "/query") => HttpReply::json(200, &json!([])),
        _ => HttpReply::text(404, "not found\n"),
    }
}

/// AUTH → OK, REQ → EOSE, EVENT → OK accepted, CLOSE → CLOSED.
pub fn default_frame(frame: &WsFrame) -> Vec<WsAction> {
    let value = &frame.value;
    match value[0].as_str().unwrap_or_default() {
        "AUTH" | "EVENT" => {
            let event = Event::from_json(value[1].to_string()).expect("signed event");
            event.verify().expect("valid signature");
            vec![WsAction::Send(json!(["OK", event.id.to_hex(), true, ""]))]
        }
        "REQ" => vec![WsAction::Send(json!(["EOSE", value[1]]))],
        "CLOSE" => vec![WsAction::Send(json!(["CLOSED", value[1], ""]))],
        _ => Vec::new(),
    }
}

#[derive(Default)]
struct Log {
    http: Vec<HttpRequest>,
    upgrades: Vec<HttpRequest>,
    frames: Vec<WsFrame>,
    connections: usize,
}

pub struct ScriptedRelay {
    pub ws_url: url::Url,
    pub http_url: url::Url,
    log: Arc<Mutex<Log>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ScriptedRelay {
    pub async fn start(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let script = Arc::new(script);
        let log = Arc::new(Mutex::new(Log::default()));
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let task_log = log.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        tokio::spawn(serve(stream, script.clone(), task_log.clone()));
                    }
                }
            }
        });
        Self {
            ws_url: url::Url::parse(&format!("ws://{address}/")).unwrap(),
            http_url: url::Url::parse(&format!("http://{address}/")).unwrap(),
            log,
            stop: Some(stop_tx),
        }
    }

    pub fn http_requests(&self) -> Vec<HttpRequest> {
        self.log.lock().unwrap().http.clone()
    }

    pub fn upgrades(&self) -> Vec<HttpRequest> {
        self.log.lock().unwrap().upgrades.clone()
    }

    pub fn frames(&self) -> Vec<WsFrame> {
        self.log.lock().unwrap().frames.clone()
    }

    /// Inbound frames of one verb (`"REQ"`, `"EVENT"`, ...).
    pub fn frames_of(&self, verb: &str) -> Vec<WsFrame> {
        self.frames()
            .into_iter()
            .filter(|frame| frame.value[0] == verb)
            .collect()
    }

    pub fn connections(&self) -> usize {
        self.log.lock().unwrap().connections
    }

    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for ScriptedRelay {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

async fn serve(stream: TcpStream, script: Arc<Script>, log: Arc<Mutex<Log>>) {
    let mut peek = [0_u8; 4096];
    let Ok(read) = stream.peek(&mut peek).await else {
        return;
    };
    let head = String::from_utf8_lossy(&peek[..read]).to_ascii_lowercase();
    if head.contains("upgrade: websocket") {
        serve_websocket(stream, script, log).await;
    } else {
        serve_http(stream, script, log).await;
    }
}

async fn serve_websocket(stream: TcpStream, script: Arc<Script>, log: Arc<Mutex<Log>>) {
    let callback_log = log.clone();
    let callback_script = script.clone();
    let callback =
        move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            let recorded = HttpRequest {
                method: request.method().to_string(),
                path: request.uri().path().to_owned(),
                headers: request
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_ascii_lowercase(),
                            value.to_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect(),
                body: Vec::new(),
                at: Instant::now(),
            };
            callback_log.lock().unwrap().upgrades.push(recorded.clone());
            match (callback_script.upgrade)(&recorded) {
                Upgrade::Accept => Ok(response),
                Upgrade::Deny(reply) => {
                    let mut denial =
                        ErrorResponse::new(Some(String::from_utf8_lossy(&reply.body).into_owned()));
                    *denial.status_mut() = StatusCode::from_u16(reply.status).unwrap();
                    for (name, value) in reply.headers {
                        denial.headers_mut().insert(
                            tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(
                                name.as_bytes(),
                            )
                            .unwrap(),
                            HeaderValue::from_str(&value).unwrap(),
                        );
                    }
                    Err(denial)
                }
            }
        };
    let Ok(socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    let connection = {
        let mut log = log.lock().unwrap();
        log.connections += 1;
        log.connections
    };
    run_socket(socket, connection, script, log).await;
}

async fn run_socket(
    mut socket: WebSocketStream<TcpStream>,
    connection: usize,
    script: Arc<Script>,
    log: Arc<Mutex<Log>>,
) {
    if script.auth_challenge
        && socket
            .send(Message::Text(r#"["AUTH","scripted-challenge"]"#.into()))
            .await
            .is_err()
    {
        return;
    }
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let frame = WsFrame {
                    connection,
                    value,
                    at: Instant::now(),
                };
                log.lock().unwrap().frames.push(frame.clone());
                for action in (script.frame)(&frame) {
                    match action {
                        WsAction::Send(value) => {
                            if socket
                                .send(Message::Text(value.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        WsAction::Close(code, reason) => {
                            let _ = socket
                                .close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                    code: code.into(),
                                    reason: reason.into(),
                                }))
                                .await;
                            return;
                        }
                    }
                }
            }
            Message::Ping(payload) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => return,
            _ => {}
        }
    }
}

async fn serve_http(mut stream: TcpStream, script: Arc<Script>, log: Arc<Mutex<Log>>) {
    loop {
        let Some(request) = read_http_request(&mut stream).await else {
            return;
        };
        log.lock().unwrap().http.push(request.clone());
        let reply = (script.http)(&request);
        let reason = StatusCode::from_u16(reply.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("Status");
        let mut head = format!("HTTP/1.1 {} {reason}\r\n", reply.status);
        for (name, value) in &reply.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", reply.body.len()));
        if stream.write_all(head.as_bytes()).await.is_err()
            || stream.write_all(&reply.body).await.is_err()
        {
            return;
        }
    }
}

async fn read_http_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut buffer = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > 1 << 20 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();
    let path = target.split('?').next().unwrap_or_default().to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end..].to_vec();
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
        headers,
        body,
        at: Instant::now(),
    })
}

/// Sign `event` as the relay would (for relay-authored 39000/39002/39005/...).
pub fn relay_signed(keys: &nostr::Keys, kind: u16, content: &str, tags: Vec<Vec<String>>) -> Event {
    let tags = tags
        .into_iter()
        .map(|tag| nostr::Tag::parse(tag).unwrap())
        .collect::<Vec<_>>();
    nostr::EventBuilder::new(nostr::Kind::Custom(kind), content)
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap()
}
