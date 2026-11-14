//! The Agent Client Protocol wire: JSON-RPC 2.0, one message per line, over a
//! harness's standard input and output (`docs/agent-host-protocol.md`).
//!
//! bzz is the client. Nothing here trusts the harness: a line that is not a
//! message is dropped, a line longer than [`MAX_LINE_BYTES`] is skipped
//! without being held in memory, and both directions are bounded, so a
//! harness that floods or stops reading slows only its own pipe.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader},
    sync::{mpsc, oneshot},
};

/// The longest line read from a harness. A longer one is discarded whole.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Messages from the harness waiting for the runtime. When full, the reader
/// stops reading and the harness blocks on its own output.
pub const INCOMING_CAPACITY: usize = 256;
/// Lines waiting to be written. When full, the harness has stopped reading.
pub const OUTGOING_CAPACITY: usize = 256;

pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// ACP: the agent needs the user to sign in.
pub const AUTH_REQUIRED: i64 = -32000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("method not found: {method}"))
    }
}

/// One line of the protocol.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Result<Value, RpcError>,
    },
}

/// Reads one line. `None`: it is not a JSON-RPC 2.0 message (a banner, a
/// warning printed to standard output, damaged JSON) and is to be ignored.
pub fn parse_frame(line: &str) -> Option<Frame> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    let object = value.as_object()?;
    let id = object.get("id").filter(|id| !id.is_null());
    if let Some(method) = object.get("method") {
        let method = method.as_str()?.to_owned();
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        return Some(match id {
            Some(id) if id.is_string() || id.is_number() => Frame::Request {
                id: id.clone(),
                method,
                params,
            },
            Some(_) => return None,
            None => Frame::Notification { method, params },
        });
    }
    let id = id?.clone();
    if let Some(error) = object.get("error") {
        return Some(Frame::Response {
            id,
            result: Err(RpcError {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(-32603),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
        });
    }
    Some(Frame::Response {
        id,
        result: Ok(object.get("result").cloned()?),
    })
}

/// Serializes one message as a single line (JSON escapes any newline).
pub fn frame_line(frame: &Frame) -> String {
    let value = match frame {
        Frame::Request { id, method, params } => {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        }
        Frame::Notification { method, params } => {
            json!({"jsonrpc": "2.0", "method": method, "params": params})
        }
        Frame::Response {
            id,
            result: Ok(result),
        } => {
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        Frame::Response {
            id,
            result: Err(error),
        } => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": error.code, "message": error.message},
        }),
    };
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// What the harness sent that is not an answer to a request of ours.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// The harness closed its output: it exited or was stopped.
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallError {
    /// The harness answered with a JSON-RPC error.
    Rpc(RpcError),
    /// The connection ended before an answer came.
    Closed,
    /// The harness is not reading its input: the outgoing queue is full.
    Stalled,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rpc(error) => write!(formatter, "harness error {}", error.code),
            Self::Closed => formatter.write_str("the harness closed the connection"),
            Self::Stalled => formatter.write_str("the harness is not reading"),
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>>;

/// Lines the reader dropped, for the status line and the tests.
#[derive(Debug, Default)]
pub struct Dropped {
    pub not_a_message: AtomicU64,
    pub too_long: AtomicU64,
}

/// bzz's end of one harness connection. Cheap to clone.
#[derive(Clone)]
pub struct Connection {
    outgoing: mpsc::Sender<String>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    dropped: Arc<Dropped>,
    close_input: Arc<tokio::sync::Notify>,
}

fn lock(
    pending: &Pending,
) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>> {
    pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Connection {
    /// Starts the reader and writer tasks over a harness's output and input.
    /// Dropping every clone closes the harness's input.
    pub fn start<R, W>(reader: R, writer: W) -> (Self, mpsc::Receiver<Incoming>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing, mut lines) = mpsc::channel::<String>(OUTGOING_CAPACITY);
        let (incoming, received) = mpsc::channel(INCOMING_CAPACITY);
        let pending = Pending::default();
        let closed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(Dropped::default());
        let close_input = Arc::new(tokio::sync::Notify::new());
        let close = close_input.clone();
        tokio::spawn(async move {
            let mut writer = writer;
            loop {
                let line = tokio::select! {
                    biased;
                    () = close.notified() => break,
                    line = lines.recv() => line,
                };
                let Some(line) = line else { break };
                let written = tokio::select! {
                    biased;
                    () = close.notified() => break,
                    written = async {
                        writer.write_all(line.as_bytes()).await?;
                        writer.flush().await
                    } => written,
                };
                if written.is_err() {
                    break;
                }
            }
            // Leaving drops the writer: the harness reads end of input.
            let _ = tokio::time::timeout(std::time::Duration::from_millis(200), writer.shutdown())
                .await;
        });
        tokio::spawn(read(
            reader,
            incoming,
            pending.clone(),
            closed.clone(),
            dropped.clone(),
        ));
        (
            Self {
                outgoing,
                pending,
                next_id: Arc::new(AtomicU64::new(1)),
                closed,
                dropped,
                close_input,
            },
            received,
        )
    }

    /// Closes the harness's input now, whoever else still holds a clone:
    /// the first step of stopping a harness.
    pub fn close_input(&self) {
        self.close_input.notify_one();
    }

    pub fn dropped(&self) -> &Dropped {
        &self.dropped
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn write(&self, frame: &Frame) -> Result<(), CallError> {
        if self.is_closed() {
            return Err(CallError::Closed);
        }
        self.outgoing
            .try_send(frame_line(frame))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => CallError::Stalled,
                mpsc::error::TrySendError::Closed(_) => CallError::Closed,
            })
    }

    /// Sends a request and waits for its answer. It never waits on a harness
    /// that does not read; a harness that never answers is the caller's to
    /// time out.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        lock(&self.pending).insert(id, sender);
        if let Err(error) = self.write(&Frame::Request {
            id: id.into(),
            method: method.to_owned(),
            params,
        }) {
            lock(&self.pending).remove(&id);
            return Err(error);
        }
        // The reader may have ended between the check and the insert.
        if self.is_closed() {
            lock(&self.pending).remove(&id);
            return Err(CallError::Closed);
        }
        match receiver.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(CallError::Rpc(error)),
            Err(_) => Err(CallError::Closed),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), CallError> {
        self.write(&Frame::Notification {
            method: method.to_owned(),
            params,
        })
    }

    pub fn respond(&self, id: Value, result: Result<Value, RpcError>) -> Result<(), CallError> {
        self.write(&Frame::Response { id, result })
    }
}

/// Reads one line of at most `limit` bytes into `line`. `Ok(None)`: end of
/// input. `Ok(Some(false))`: the line was longer and has been skipped.
async fn read_line_capped<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    line: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<Option<bool>> {
    line.clear();
    let mut fits = true;
    let mut seen = false;
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok(seen.then_some(fits));
        }
        seen = true;
        let (take, done) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if fits {
            if line.len() + take > limit {
                fits = false;
                line.clear();
            } else {
                line.extend_from_slice(&buffer[..take]);
            }
        }
        reader.consume(take);
        if done {
            return Ok(Some(fits));
        }
    }
}

async fn read<R: AsyncRead + Unpin>(
    reader: R,
    incoming: mpsc::Sender<Incoming>,
    pending: Pending,
    closed: Arc<AtomicBool>,
    dropped: Arc<Dropped>,
) {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut line = Vec::new();
    loop {
        match read_line_capped(&mut reader, &mut line, MAX_LINE_BYTES).await {
            Ok(Some(true)) => {}
            Ok(Some(false)) => {
                dropped.too_long.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            Ok(None) | Err(_) => break,
        }
        let Some(frame) = std::str::from_utf8(&line).ok().and_then(parse_frame) else {
            if !line.iter().all(u8::is_ascii_whitespace) {
                dropped.not_a_message.fetch_add(1, Ordering::Relaxed);
            }
            continue;
        };
        let message = match frame {
            Frame::Response { id, result } => {
                // An answer to a request we did not make is ignored.
                if let Some(waiting) = id.as_u64().and_then(|id| lock(&pending).remove(&id)) {
                    let _ = waiting.send(result);
                }
                continue;
            }
            Frame::Request { id, method, params } => Incoming::Request { id, method, params },
            Frame::Notification { method, params } => Incoming::Notification { method, params },
        };
        if incoming.send(message).await.is_err() {
            break;
        }
    }
    closed.store(true, Ordering::Release);
    // Every request still waiting ends as `Closed`.
    lock(&pending).clear();
    let _ = incoming.send(Incoming::Closed).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, duplex};

    #[test]
    fn frames_are_told_apart_and_anything_else_is_ignored() {
        assert_eq!(
            parse_frame(r#"{"jsonrpc":"2.0","id":7,"method":"_bzz/tool","params":{"a":1}}"#),
            Some(Frame::Request {
                id: 7.into(),
                method: "_bzz/tool".into(),
                params: json!({"a": 1}),
            })
        );
        assert_eq!(
            parse_frame(r#"{"jsonrpc":"2.0","id":"x","method":"session/request_permission"}"#),
            Some(Frame::Request {
                id: "x".into(),
                method: "session/request_permission".into(),
                params: Value::Null,
            })
        );
        assert_eq!(
            parse_frame(r#" {"jsonrpc":"2.0","method":"session/update","params":{}} "#),
            Some(Frame::Notification {
                method: "session/update".into(),
                params: json!({}),
            })
        );
        // A null id is a notification, as JSON-RPC says.
        assert!(matches!(
            parse_frame(r#"{"id":null,"method":"x"}"#),
            Some(Frame::Notification { .. })
        ));
        assert_eq!(
            parse_frame(r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#),
            Some(Frame::Response {
                id: 3.into(),
                result: Ok(json!({"stopReason": "end_turn"})),
            })
        );
        assert_eq!(
            parse_frame(r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"sign in"}}"#),
            Some(Frame::Response {
                id: 3.into(),
                result: Err(RpcError::new(AUTH_REQUIRED, "sign in")),
            })
        );
        for noise in [
            "",
            "npm warn deprecated",
            "[1,2,3]",
            "\"text\"",
            "{\"id\":3}",
            "{\"method\":5}",
            "{\"id\":{},\"method\":\"x\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"x\"",
        ] {
            assert_eq!(parse_frame(noise), None, "{noise}");
        }
    }

    #[test]
    fn a_message_is_always_one_line() {
        let line = frame_line(&Frame::Request {
            id: 1.into(),
            method: "session/prompt".into(),
            params: json!({"prompt": [{"type": "text", "text": "two\nlines\r\n"}]}),
        });
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        assert!(matches!(parse_frame(&line), Some(Frame::Request { .. })));
        let error = frame_line(&Frame::Response {
            id: "a".into(),
            result: Err(RpcError::method_not_found("fs/read_text_file")),
        });
        assert_eq!(
            parse_frame(&error),
            Some(Frame::Response {
                id: "a".into(),
                result: Err(RpcError::new(
                    METHOD_NOT_FOUND,
                    "method not found: fs/read_text_file"
                )),
            })
        );
    }

    /// The harness's side of a connection, written by hand.
    struct Peer {
        to_client: tokio::io::DuplexStream,
        from_client: BufReader<tokio::io::DuplexStream>,
    }

    fn connect() -> (Connection, mpsc::Receiver<Incoming>, Peer) {
        let (to_client, client_reads) = duplex(1024 * 1024);
        let (client_writes, from_client) = duplex(1024 * 1024);
        let (connection, incoming) = Connection::start(client_reads, client_writes);
        (
            connection,
            incoming,
            Peer {
                to_client,
                from_client: BufReader::new(from_client),
            },
        )
    }

    impl Peer {
        async fn line(&mut self) -> Value {
            let mut line = String::new();
            self.from_client.read_line(&mut line).await.unwrap();
            serde_json::from_str(&line).unwrap()
        }

        async fn send(&mut self, text: &str) {
            self.to_client.write_all(text.as_bytes()).await.unwrap();
        }
    }

    #[tokio::test]
    async fn answers_reach_their_requests_in_any_order() {
        let (connection, mut incoming, mut peer) = connect();
        let first = tokio::spawn({
            let connection = connection.clone();
            async move { connection.request("session/new", json!({"n": 1})).await }
        });
        let one = peer.line().await;
        let second = tokio::spawn({
            let connection = connection.clone();
            async move { connection.request("session/new", json!({"n": 2})).await }
        });
        let two = peer.line().await;
        assert_eq!(
            (one["params"]["n"].clone(), two["params"]["n"].clone()),
            (1.into(), 2.into())
        );
        // Answered in reverse, with a notification, noise, an answer to
        // nothing and a request in between.
        peer.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":-32000,\"message\":\"no\"}}}}\n\
             not json\n\
             {{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{{}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":\"h1\",\"method\":\"_bzz/tool\",\"params\":{{}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"sessionId\":\"s\"}}}}\n",
            two["id"], one["id"]
        ))
        .await;
        assert_eq!(first.await.unwrap(), Ok(json!({"sessionId": "s"})));
        assert_eq!(
            second.await.unwrap(),
            Err(CallError::Rpc(RpcError::new(AUTH_REQUIRED, "no")))
        );
        assert!(matches!(
            incoming.recv().await,
            Some(Incoming::Notification { .. })
        ));
        let Some(Incoming::Request { id, method, .. }) = incoming.recv().await else {
            panic!("expected the harness's request")
        };
        assert_eq!(method, "_bzz/tool");
        connection
            .respond(id, Err(RpcError::method_not_found("x")))
            .unwrap();
        let answer = peer.line().await;
        assert_eq!(
            (answer["id"].clone(), answer["error"]["code"].clone()),
            ("h1".into(), METHOD_NOT_FOUND.into())
        );
        assert_eq!(
            connection.dropped().not_a_message.load(Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn an_oversized_line_is_skipped_and_the_next_one_is_read() {
        let (connection, mut incoming, mut peer) = connect();
        let writer = tokio::spawn(async move {
            // Longer than the limit, sent in pieces, never held whole.
            let piece = vec![b'x'; 256 * 1024];
            let mut sent = 0;
            while sent <= MAX_LINE_BYTES {
                peer.to_client.write_all(&piece).await.unwrap();
                sent += piece.len();
            }
            peer.send("\n{\"jsonrpc\":\"2.0\",\"method\":\"after\"}\n")
                .await;
            peer
        });
        assert_eq!(
            incoming.recv().await,
            Some(Incoming::Notification {
                method: "after".into(),
                params: Value::Null,
            })
        );
        assert_eq!(connection.dropped().too_long.load(Ordering::Relaxed), 1);
        drop(writer.await.unwrap());
        assert_eq!(incoming.recv().await, Some(Incoming::Closed));
    }

    #[tokio::test]
    async fn a_closed_harness_ends_every_waiting_request() {
        let (connection, mut incoming, peer) = connect();
        let waiting = tokio::spawn({
            let connection = connection.clone();
            async move { connection.request("session/prompt", json!({})).await }
        });
        tokio::task::yield_now().await;
        drop(peer);
        assert_eq!(waiting.await.unwrap(), Err(CallError::Closed));
        assert_eq!(incoming.recv().await, Some(Incoming::Closed));
        assert!(connection.is_closed());
        assert_eq!(
            connection.request("x", json!({})).await,
            Err(CallError::Closed)
        );
        assert_eq!(connection.notify("x", json!({})), Err(CallError::Closed));
    }

    #[tokio::test]
    async fn a_harness_that_does_not_read_is_reported_not_waited_for() {
        // A pipe of one byte that nobody reads.
        let (_to_client, client_reads) = duplex(64);
        let (client_writes, _unread) = duplex(1);
        let (connection, _incoming) = Connection::start(client_reads, client_writes);
        let mut outcome = Ok(());
        for _ in 0..OUTGOING_CAPACITY + 8 {
            outcome = connection.notify("session/cancel", json!({"sessionId": "s"}));
            if outcome.is_err() {
                break;
            }
        }
        assert_eq!(outcome, Err(CallError::Stalled));
        let request = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            connection.request("session/prompt", json!({})),
        )
        .await
        .expect("a request to a stalled harness returns at once");
        assert_eq!(request, Err(CallError::Stalled));
    }

    #[tokio::test]
    async fn closing_the_input_ends_it_even_while_a_clone_is_held() {
        let (connection, _incoming, mut peer) = connect();
        let held = connection.clone();
        connection.close_input();
        let mut rest = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            peer.from_client.read_to_string(&mut rest),
        )
        .await
        .expect("end of input reaches the harness")
        .unwrap();
        drop(held);
    }

    #[tokio::test]
    async fn dropping_the_connection_closes_the_harness_input() {
        let (connection, incoming, mut peer) = connect();
        connection.notify("session/cancel", json!({})).unwrap();
        drop(connection);
        drop(incoming);
        let mut rest = String::new();
        peer.from_client.read_to_string(&mut rest).await.unwrap();
        assert!(
            rest.contains("session/cancel"),
            "queued lines are written first"
        );
    }
}
