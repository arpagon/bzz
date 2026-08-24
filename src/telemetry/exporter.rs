use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    diagnostics::event::{DiagnosticRecord, ErrorClass},
    error::{Error, Result},
    paths::set_private_permissions,
    telemetry::otlp::{self, RemoteRecord},
};

pub const QUEUE_RECORDS: usize = 256;
pub const QUEUE_BYTES: usize = 512 * 1024;
pub const BATCH_RECORDS: usize = 64;
pub const BATCH_BYTES: usize = 128 * 1024;
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(1);
const RECORD_TTL: Duration = Duration::from_secs(5 * 60);
const ROUTINE_QUEUE_LIMIT: usize = QUEUE_RECORDS - BATCH_RECORDS;

struct QueuedRecord {
    record: RemoteRecord,
    estimate: usize,
    queued_at: tokio::time::Instant,
}

enum ExportCommand {
    Record(QueuedRecord),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportHealth {
    pub last_success_unix_ms: Option<u64>,
    pub last_error_class: Option<ErrorClass>,
    pub dropped: u64,
    pub queued: usize,
    pub stopped_for_run: bool,
}

#[derive(Clone)]
pub struct TelemetryHandle {
    sender: mpsc::Sender<ExportCommand>,
    queued_bytes: Arc<AtomicUsize>,
    queued_records: Arc<AtomicUsize>,
    dropped: Arc<AtomicU64>,
    health: Arc<Mutex<ExportHealth>>,
}

impl TelemetryHandle {
    pub fn start(endpoint: Url, token: Zeroizing<String>, health_path: PathBuf) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| Error::Network("telemetry HTTP client could not be created".into()))?;
        let (sender, receiver) = mpsc::channel(QUEUE_RECORDS);
        let mut initial_health = load_health(&health_path);
        initial_health.queued = 0;
        initial_health.stopped_for_run = false;
        let _ = save_health(&health_path, &initial_health);
        let handle = Self {
            sender,
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            queued_records: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            health: Arc::new(Mutex::new(initial_health)),
        };
        tokio::spawn(run(
            endpoint,
            token,
            client,
            receiver,
            handle.clone(),
            health_path,
        ));
        Ok(handle)
    }

    pub fn try_emit(&self, record: &DiagnosticRecord) {
        let priority = record_priority(&record.event);
        let Some(remote) = RemoteRecord::from_diagnostic(record) else {
            return;
        };
        let estimate = otlp::encode(std::slice::from_ref(&remote)).len();
        if estimate > BATCH_BYTES || !self.reserve(estimate, priority) {
            self.note_drop();
            return;
        }
        let queued = QueuedRecord {
            record: remote,
            estimate,
            queued_at: tokio::time::Instant::now(),
        };
        if self.sender.try_send(ExportCommand::Record(queued)).is_err() {
            self.release(estimate);
            self.note_drop();
        }
    }

    fn reserve(&self, estimate: usize, priority: RecordPriority) -> bool {
        let queued = self.queued_records.load(Ordering::Relaxed);
        if queued >= QUEUE_RECORDS
            || (priority == RecordPriority::Routine && queued >= ROUTINE_QUEUE_LIMIT)
        {
            return false;
        }
        let mut current = self.queued_bytes.load(Ordering::Relaxed);
        loop {
            let Some(next) = current.checked_add(estimate) else {
                return false;
            };
            if next > QUEUE_BYTES {
                return false;
            }
            match self.queued_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.queued_records.fetch_add(1, Ordering::Relaxed);
                    return true;
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, estimate: usize) {
        self.queued_bytes.fetch_sub(estimate, Ordering::Relaxed);
        self.queued_records.fetch_sub(1, Ordering::Relaxed);
    }

    fn note_drop(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn health(&self) -> ExportHealth {
        let mut health = self.health.lock().expect("telemetry health lock").clone();
        health.dropped = health
            .dropped
            .saturating_add(self.dropped.load(Ordering::Relaxed));
        health.queued = self.queued_records.load(Ordering::Relaxed);
        health
    }

    pub async fn shutdown(&self) {
        let sender = self.sender.clone();
        let _ = tokio::time::timeout(SHUTDOWN_BUDGET, async move {
            let (done_tx, done_rx) = oneshot::channel();
            sender
                .send(ExportCommand::Shutdown(done_tx))
                .await
                .map_err(|_| ())?;
            done_rx.await.map_err(|_| ())
        })
        .await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordPriority {
    Routine,
    Outcome,
}

fn record_priority(event: &crate::diagnostics::event::DiagnosticEvent) -> RecordPriority {
    use crate::diagnostics::event::DiagnosticEvent;
    match event {
        DiagnosticEvent::ConnectFailed { .. }
        | DiagnosticEvent::Disconnected { .. }
        | DiagnosticEvent::HeartbeatTimeout { .. }
        | DiagnosticEvent::PublishAcknowledged { .. }
        | DiagnosticEvent::PublishUncertain { .. }
        | DiagnosticEvent::OutboxStateChanged { .. }
        | DiagnosticEvent::ReconcileObserved { .. }
        | DiagnosticEvent::ReconcileRepublished { .. }
        | DiagnosticEvent::ReconcileFinished { .. } => RecordPriority::Outcome,
        _ => RecordPriority::Routine,
    }
}

async fn run(
    endpoint: Url,
    token: Zeroizing<String>,
    client: Client,
    mut receiver: mpsc::Receiver<ExportCommand>,
    handle: TelemetryHandle,
    health_path: PathBuf,
) {
    let mut pending: Option<QueuedRecord> = None;
    let mut stopped = false;
    loop {
        let command = match pending.take() {
            Some(record) => ExportCommand::Record(record),
            None => match receiver.recv().await {
                Some(command) => command,
                None => break,
            },
        };
        match command {
            ExportCommand::Shutdown(done) => {
                let _ = done.send(());
                break;
            }
            ExportCommand::Record(first) => {
                let mut batch = vec![first];
                let mut bytes = batch[0].estimate;
                let deadline = tokio::time::Instant::now() + FLUSH_INTERVAL;
                while batch.len() < BATCH_RECORDS {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match tokio::time::timeout(remaining, receiver.recv()).await {
                        Ok(Some(ExportCommand::Record(record))) => {
                            if bytes.saturating_add(record.estimate) > BATCH_BYTES {
                                pending = Some(record);
                                break;
                            }
                            bytes += record.estimate;
                            batch.push(record);
                        }
                        Ok(Some(ExportCommand::Shutdown(done))) => {
                            let _ = done.send(());
                            return;
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
                for record in &batch {
                    handle.release(record.estimate);
                }
                let now = tokio::time::Instant::now();
                batch.retain(|record| now.duration_since(record.queued_at) <= RECORD_TTL);
                if batch.is_empty() || stopped {
                    if stopped {
                        handle
                            .dropped
                            .fetch_add(batch.len() as u64, Ordering::Relaxed);
                    }
                    continue;
                }
                let records = batch
                    .into_iter()
                    .map(|queued| queued.record)
                    .collect::<Vec<_>>();
                let outcome = send_with_retries(&client, &endpoint, &token, &records).await;
                if outcome == SendOutcome::TooLarge && records.len() > 1 {
                    let middle = records.len() / 2;
                    let left = send_once(&client, &endpoint, &token, &records[..middle]).await;
                    let right = send_once(&client, &endpoint, &token, &records[middle..]).await;
                    update_health(
                        &handle,
                        &health_path,
                        combine_split_outcomes(left, right),
                        records.len() as u64,
                    );
                } else {
                    if outcome == SendOutcome::Stop {
                        stopped = true;
                    }
                    update_health(&handle, &health_path, outcome, records.len() as u64);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendOutcome {
    Delivered,
    Permanent(ErrorClass),
    Retryable(ErrorClass, Option<Duration>),
    TooLarge,
    Stop,
}

async fn send_with_retries(
    client: &Client,
    endpoint: &Url,
    token: &str,
    records: &[RemoteRecord],
) -> SendOutcome {
    let mut attempt = 0_u32;
    loop {
        attempt += 1;
        let outcome = send_once(client, endpoint, token, records).await;
        match outcome {
            SendOutcome::Retryable(_, retry_after) if attempt < 3 => {
                let base = Duration::from_millis(250_u64.saturating_mul(1 << (attempt - 1)));
                let jitter = Duration::from_millis(rand::random_range(0..=100));
                let delay = retry_after
                    .unwrap_or(base.saturating_add(jitter))
                    .min(Duration::from_secs(20));
                tokio::time::sleep(delay).await;
            }
            SendOutcome::Retryable(class, _) => return SendOutcome::Permanent(class),
            other => return other,
        }
    }
}

async fn send_once(
    client: &Client,
    endpoint: &Url,
    token: &str,
    records: &[RemoteRecord],
) -> SendOutcome {
    let payload = otlp::encode(records);
    let response = client
        .post(endpoint.clone())
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .bearer_auth(token)
        .body(payload)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_timeout() => {
            return SendOutcome::Retryable(ErrorClass::Connect, None);
        }
        Err(_) => return SendOutcome::Retryable(ErrorClass::Connect, None),
    };
    let status = response.status();
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|duration| *duration <= Duration::from_secs(20));
    // Never persist or parse the response body. Dropping the bounded response
    // is sufficient because reqwest owns no application state.
    if status.is_success() {
        SendOutcome::Delivered
    } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        SendOutcome::Stop
    } else if status == StatusCode::PAYLOAD_TOO_LARGE {
        SendOutcome::TooLarge
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        SendOutcome::Retryable(ErrorClass::RateLimited, retry_after)
    } else if status.is_server_error() {
        SendOutcome::Retryable(ErrorClass::Http5xx, None)
    } else {
        SendOutcome::Permanent(ErrorClass::Http4xx)
    }
}

fn combine_split_outcomes(left: SendOutcome, right: SendOutcome) -> SendOutcome {
    if left == SendOutcome::Delivered && right == SendOutcome::Delivered {
        SendOutcome::Delivered
    } else if left == SendOutcome::Stop || right == SendOutcome::Stop {
        SendOutcome::Stop
    } else {
        SendOutcome::Permanent(ErrorClass::Http4xx)
    }
}

fn update_health(handle: &TelemetryHandle, path: &Path, outcome: SendOutcome, batch_len: u64) {
    let mut health = handle.health.lock().expect("telemetry health lock");
    match outcome {
        SendOutcome::Delivered => {
            health.last_success_unix_ms = Some(crate::diagnostics::event::unix_millis());
            health.last_error_class = None;
        }
        SendOutcome::Stop => {
            health.last_error_class = Some(ErrorClass::AccessDenied);
            health.stopped_for_run = true;
            health.dropped = health.dropped.saturating_add(batch_len);
        }
        SendOutcome::Permanent(class) | SendOutcome::Retryable(class, _) => {
            health.last_error_class = Some(class);
            health.dropped = health.dropped.saturating_add(batch_len);
        }
        SendOutcome::TooLarge => {
            health.last_error_class = Some(ErrorClass::Http4xx);
            health.dropped = health.dropped.saturating_add(batch_len);
        }
    }
    health.dropped = health
        .dropped
        .saturating_add(handle.dropped.swap(0, Ordering::Relaxed));
    health.queued = handle.queued_records.load(Ordering::Relaxed);
    let snapshot = health.clone();
    drop(health);
    let _ = save_health(path, &snapshot);
}

fn load_health(path: &Path) -> ExportHealth {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn read_health(path: &Path) -> ExportHealth {
    load_health(path)
}

pub fn record_start_failure(path: &Path, error_class: ErrorClass) {
    let mut health = load_health(path);
    health.last_error_class = Some(error_class);
    health.stopped_for_run = true;
    let _ = save_health(path, &health);
}

fn save_health(path: &Path, health: &ExportHealth) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
        set_private_permissions(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec(health).map_err(|error| Error::Serialization(error.to_string()))?;
    std::fs::write(&temporary, bytes).map_err(|error| Error::io(&temporary, error))?;
    set_private_permissions(&temporary)?;
    replace_health_file(&temporary, path)
}

#[cfg(not(windows))]
fn replace_health_file(temporary: &Path, destination: &Path) -> Result<()> {
    std::fs::rename(temporary, destination).map_err(|error| Error::io(destination, error))
}

#[cfg(windows)]
fn replace_health_file(temporary: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        std::fs::remove_file(destination).map_err(|error| Error::io(destination, error))?;
    }
    std::fs::rename(temporary, destination).map_err(|error| Error::io(destination, error))
}

pub async fn test_export(endpoint: Url, token: Zeroizing<String>) -> Result<()> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| Error::Network("telemetry HTTP client could not be created".into()))?;
    let diagnostic = DiagnosticRecord::new(
        uuid::Uuid::new_v4().simple().to_string(),
        crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
    );
    let remote = RemoteRecord::from_diagnostic(&diagnostic)
        .ok_or_else(|| Error::Protocol("telemetry test record was not eligible".into()))?;
    match send_once(&client, &endpoint, &token, &[remote]).await {
        SendOutcome::Delivered => Ok(()),
        SendOutcome::Stop => Err(Error::Access("telemetry credential was rejected".into())),
        SendOutcome::TooLarge => Err(Error::Protocol("telemetry test was too large".into())),
        SendOutcome::Permanent(class) | SendOutcome::Retryable(class, _) => Err(Error::Network(
            format!("telemetry test failed: {}", class.as_str()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message as _;

    async fn fake_receiver(
        status: &str,
        extra_headers: &str,
    ) -> (Url, tokio::sync::oneshot::Receiver<Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (captured_tx, captured_rx) = tokio::sync::oneshot::channel();
        let status = status.to_owned();
        let extra_headers = extra_headers.to_owned();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let expected = loop {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break request.len();
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let header_end = header_end + 4;
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    break header_end + length;
                }
            };
            while request.len() < expected {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let _ = captured_tx.send(request);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\n{extra_headers}Connection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (
            Url::parse(&format!("http://{address}/v1/logs")).unwrap(),
            captured_rx,
        )
    }

    #[tokio::test]
    async fn fake_otlp_receiver_gets_exact_protobuf_and_auth_boundary() {
        let (endpoint, captured) = fake_receiver("200 OK", "").await;
        let diagnostic = DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
        );
        let remote = RemoteRecord::from_diagnostic(&diagnostic).unwrap();
        assert_eq!(
            send_once(&Client::new(), &endpoint, "test-only-token", &[remote]).await,
            SendOutcome::Delivered
        );
        let request = captured.await.unwrap();
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/logs http/1.1"));
        assert!(headers.contains("content-type: application/x-protobuf"));
        assert!(headers.contains("authorization: bearer test-only-token"));
        assert!(
            !request[header_end..]
                .windows(15)
                .any(|window| window == b"test-only-token")
        );
        let decoded =
            opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest::decode(
                &request[header_end..],
            )
            .unwrap();
        assert_eq!(
            decoded.resource_logs[0].scope_logs[0].log_records[0].event_name,
            "telemetry.test"
        );
    }

    #[tokio::test]
    async fn otlp_statuses_have_bounded_normalized_outcomes() {
        let cases = [
            (
                "400 Bad Request",
                "",
                SendOutcome::Permanent(ErrorClass::Http4xx),
            ),
            ("401 Unauthorized", "", SendOutcome::Stop),
            ("403 Forbidden", "", SendOutcome::Stop),
            ("413 Payload Too Large", "", SendOutcome::TooLarge),
            (
                "429 Too Many Requests",
                "Retry-After: 2\r\n",
                SendOutcome::Retryable(ErrorClass::RateLimited, Some(Duration::from_secs(2))),
            ),
            (
                "500 Internal Server Error",
                "",
                SendOutcome::Retryable(ErrorClass::Http5xx, None),
            ),
        ];
        for (status, headers, expected) in cases {
            let (endpoint, captured) = fake_receiver(status, headers).await;
            let diagnostic = DiagnosticRecord::new(
                "a".repeat(32),
                crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
            );
            let remote = RemoteRecord::from_diagnostic(&diagnostic).unwrap();
            assert_eq!(
                send_once(&Client::new(), &endpoint, "status-token", &[remote]).await,
                expected
            );
            let _ = captured.await.unwrap();
        }
    }

    #[tokio::test]
    async fn unreachable_endpoint_is_normalized_without_response_content() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = Url::parse(&format!("http://{address}/v1/logs")).unwrap();
        let diagnostic = DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
        );
        let remote = RemoteRecord::from_diagnostic(&diagnostic).unwrap();
        assert_eq!(
            send_once(&Client::new(), &endpoint, "connect-token", &[remote]).await,
            SendOutcome::Retryable(ErrorClass::Connect, None)
        );
    }

    #[tokio::test]
    async fn slow_receiver_is_cancelled_by_the_request_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let endpoint = Url::parse(&format!("http://{address}/v1/logs")).unwrap();
        let client = Client::builder()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        let diagnostic = DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
        );
        let remote = RemoteRecord::from_diagnostic(&diagnostic).unwrap();
        assert_eq!(
            send_once(&client, &endpoint, "timeout-token", &[remote]).await,
            SendOutcome::Retryable(ErrorClass::Connect, None)
        );
    }

    #[tokio::test]
    async fn redirects_are_refused_without_forwarding_credentials() {
        let (endpoint, captured) = fake_receiver(
            "307 Temporary Redirect",
            "Location: http://127.0.0.1:9/stolen\r\n",
        )
        .await;
        let diagnostic = DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
        );
        let remote = RemoteRecord::from_diagnostic(&diagnostic).unwrap();
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        assert_eq!(
            send_once(&client, &endpoint, "redirect-secret", &[remote]).await,
            SendOutcome::Permanent(ErrorClass::Http4xx)
        );
        let _ = captured.await.unwrap();
    }

    #[tokio::test]
    async fn queue_is_hard_bounded_without_waiting() {
        let (sender, _receiver) = mpsc::channel(QUEUE_RECORDS);
        let handle = TelemetryHandle {
            sender,
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            queued_records: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            health: Arc::new(Mutex::new(ExportHealth::default())),
        };
        let record = DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::TelemetryTest,
        );
        for _ in 0..(QUEUE_RECORDS + 20) {
            handle.try_emit(&record);
        }
        assert!(handle.queued_records.load(Ordering::Relaxed) <= ROUTINE_QUEUE_LIMIT);
        assert!(handle.queued_bytes.load(Ordering::Relaxed) <= QUEUE_BYTES);
        assert!(handle.dropped.load(Ordering::Relaxed) > 0);

        handle.try_emit(&DiagnosticRecord::new(
            "a".repeat(32),
            crate::diagnostics::event::DiagnosticEvent::PublishUncertain {
                event_id: "a".repeat(64),
                error_class: ErrorClass::AckTimeout,
                duration_ms: 25_000,
            },
        ));
        assert_eq!(
            handle.queued_records.load(Ordering::Relaxed),
            ROUTINE_QUEUE_LIMIT + 1
        );
    }
}
