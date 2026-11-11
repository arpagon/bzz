//! Loopback HTTP binding of the broker: `POST /v1/action` on `127.0.0.1`.
//! The bearer credential, not the network position, is the authority; the
//! listener exists only while bzz is unlocked and is dropped on lock.

use std::{net::SocketAddr, path::Path};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

use crate::{
    broker::host::{Host, MAX_BODY_BYTES},
    error::{Error, Result},
};

const MAX_HEAD_BYTES: usize = 16 * 1024;

pub struct BrokerServer {
    pub address: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for BrokerServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Binds `127.0.0.1:0` and writes the port to `port_file` (owner-only).
pub async fn serve(host: Host, port_file: Option<&Path>) -> Result<BrokerServer> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| Error::io("broker listener", error))?;
    let address = listener
        .local_addr()
        .map_err(|error| Error::io("broker listener", error))?;
    if let Some(path) = port_file {
        write_owner_only(path, &format!("{}\n", address.port()))?;
    }
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let host = host.clone();
            tokio::spawn(async move {
                let _ = connection(stream, host).await;
            });
        }
    });
    Ok(BrokerServer { address, task })
}

fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| Error::io(path, error))?;
    std::io::Write::write_all(&mut file, contents.as_bytes())
        .map_err(|error| Error::io(path, error))
}

async fn respond(stream: &mut TcpStream, status: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

async fn connection(mut stream: TcpStream, host: Host) -> std::io::Result<()> {
    let mut buffer = Vec::with_capacity(4096);
    let head_end = loop {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut chunk))
                .await
                .map_err(|_| std::io::ErrorKind::TimedOut)??;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return respond(&mut stream, "431 Request Header Fields Too Large", b"{}").await;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut authorization = None;
    let mut length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "authorization" => authorization = Some(value.trim().to_owned()),
                "content-length" => length = value.trim().parse::<usize>().ok(),
                _ => {}
            }
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next(), parts.next());
    if path != Some(buzz_sdk::broker::BROKER_ACTION_PATH) {
        return respond(&mut stream, "404 Not Found", b"{}").await;
    }
    if method != Some("POST") {
        return respond(&mut stream, "405 Method Not Allowed", b"{}").await;
    }
    let Some(length) = length.filter(|length| *length <= MAX_BODY_BYTES) else {
        return respond(&mut stream, "413 Content Too Large", b"{}").await;
    };
    let mut body = buffer[head_end..].to_vec();
    while body.len() < length {
        let mut chunk = vec![0_u8; (length - body.len()).min(64 * 1024)];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut chunk))
                .await
                .map_err(|_| std::io::ErrorKind::TimedOut)??;
        if read == 0 {
            return Ok(());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    let response = host.handle(authorization.as_deref(), &body).await;
    let bytes = serde_json::to_vec(&response).unwrap_or_else(|_| b"{}".to_vec());
    // Every host verdict is HTTP 200; the envelope carries the result.
    respond(&mut stream, "200 OK", &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::host::{ExecError, Executor, HostedAgent, credential_hash, new_credential};
    use async_trait::async_trait;
    use buzz_sdk::broker::{
        ActionArgs, ActionOutcome, BrokerClient, BrokerClientExt, BrokerErrorCode, BrokerFuture,
        BrokerRequest, BrokerTransportError, Dispatch, EventPublished, MessagePostArgs,
        PreparedRequest,
    };
    use std::sync::Arc;
    use uuid::Uuid;

    struct Echo;

    #[async_trait]
    impl Executor for Echo {
        async fn execute(
            &self,
            _agent: &HostedAgent,
            _args: &ActionArgs,
        ) -> std::result::Result<ActionOutcome, ExecError> {
            Ok(ActionOutcome::MessagePost(EventPublished {
                event_id: "c".repeat(64),
                kind: 9,
                created_at: 2,
            }))
        }
        fn target_label(&self, _args: &ActionArgs) -> Option<String> {
            None
        }
    }

    /// An HTTP `BrokerClient`: send the frozen bytes, parse any envelope.
    struct Http(reqwest::Client, String, String);

    impl BrokerClient for Http {
        fn send<'a>(
            &'a self,
            request: &'a PreparedRequest,
            _dispatch: Dispatch,
        ) -> BrokerFuture<'a> {
            Box::pin(async move {
                let response = self
                    .0
                    .post(&self.1)
                    .header("authorization", format!("Bearer {}", self.2))
                    .header("content-type", "application/json")
                    .body(request.body().to_vec())
                    .send()
                    .await
                    .map_err(|error| BrokerTransportError::Unreachable(error.to_string()))?;
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|error| BrokerTransportError::Unreachable(error.to_string()))?;
                serde_json::from_slice(&bytes)
                    .map_err(|error| BrokerTransportError::Unreachable(error.to_string()))
            })
        }
    }

    #[tokio::test]
    async fn loopback_binding_serves_the_contract_and_writes_an_owner_only_port_file() {
        let credential = new_credential();
        let agent = HostedAgent {
            id: Uuid::new_v4(),
            label: "helper".into(),
            pubkey: "d".repeat(64),
            credential_hash: credential_hash(&credential),
            allowed: ["message.post".to_owned()].into(),
            paused: false,
        };
        let temporary = tempfile::TempDir::new().unwrap();
        let port_file = temporary.path().join("broker.port");
        let server = serve(Host::new(Arc::new(Echo), vec![agent]), Some(&port_file))
            .await
            .unwrap();
        assert!(server.address.ip().is_loopback());
        assert_eq!(
            std::fs::read_to_string(&port_file).unwrap().trim(),
            server.address.port().to_string()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&port_file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let url = format!("http://{}/v1/action", server.address);
        let request = BrokerRequest::new(
            "h1",
            ActionArgs::MessagePost(MessagePostArgs {
                channel_id: Uuid::new_v4().to_string(),
                content: "hi".into(),
                mentions: Vec::new(),
            }),
        )
        .unwrap()
        .prepare()
        .unwrap();
        let client = Http(reqwest::Client::new(), url.clone(), credential);
        let response = client.execute(&request).await.unwrap();
        assert!(response.result().outcome().is_some());
        let stranger = Http(reqwest::Client::new(), url.clone(), new_credential());
        let denied = stranger.execute(&request).await.unwrap();
        assert_eq!(
            denied.result().error().map(|error| error.code),
            Some(BrokerErrorCode::Unauthenticated)
        );
        let other = reqwest::Client::new()
            .get(format!("http://{}/v1/other", server.address))
            .send()
            .await
            .unwrap();
        assert_eq!(other.status(), 404);
        drop(server);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            reqwest::Client::new()
                .post(&url)
                .body("{}")
                .send()
                .await
                .is_err(),
            "nothing is served after the server is dropped (lock)"
        );
    }
}
