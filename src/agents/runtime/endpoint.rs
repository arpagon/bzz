//! The loopback endpoint behind `bzz agent-tools`: `POST /v1/tool` on
//! `127.0.0.1`, a port chosen at start (`docs/agent-host-protocol.md`,
//! section 6). The bearer credential alone says which session calls; a call
//! with a credential bzz did not make, or one whose process has exited, is
//! refused before its body is read.

use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc};

use serde_json::{Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

use crate::{
    agents::runtime::tools::ToolReply,
    broker::http::{read_body, read_head, respond},
    error::{Error, Result},
};

pub const TOOL_PATH: &str = "/v1/tool";
/// The largest call body read: a message is at most 64 KiB.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// A call the endpoint cannot hand on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Denied {
    /// The credential is unknown or revoked.
    Credential,
    /// The credential's session is not open, or its agent is gone.
    NotReady,
}

pub type Handled = Pin<Box<dyn Future<Output = std::result::Result<ToolReply, Denied>> + Send>>;

/// Checks the credential. It runs before the body is read.
pub type Authorize = Arc<dyn Fn(Option<&str>) -> std::result::Result<(), Denied> + Send + Sync>;
/// Serves one call: `(authorization, tool name, arguments)`.
pub type Handle = Arc<dyn Fn(Option<String>, String, Value) -> Handled + Send + Sync>;

pub struct ToolsServer {
    pub address: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for ToolsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn serve(authorize: Authorize, handle: Handle) -> Result<ToolsServer> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| Error::io("agent tools listener", error))?;
    let address = listener
        .local_addr()
        .map_err(|error| Error::io("agent tools listener", error))?;
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (authorize, handle) = (authorize.clone(), handle.clone());
            tokio::spawn(async move {
                let _ = connection(stream, authorize, handle).await;
            });
        }
    });
    Ok(ToolsServer { address, task })
}

fn refusal(denied: Denied) -> (&'static str, Vec<u8>) {
    let (status, text) = match denied {
        Denied::Credential => (
            "401 Unauthorized",
            "bzz does not know this credential, or the agent's process has exited. Nothing was done.",
        ),
        Denied::NotReady => (
            "409 Conflict",
            "This session is not open in bzz. Nothing was done.",
        ),
    };
    (
        status,
        json!({"text": text, "isError": true})
            .to_string()
            .into_bytes(),
    )
}

async fn connection(
    mut stream: TcpStream,
    authorize: Authorize,
    handle: Handle,
) -> std::io::Result<()> {
    let Some(head) = read_head(&mut stream).await? else {
        return Ok(());
    };
    if head.path.as_deref() != Some(TOOL_PATH) {
        return respond(&mut stream, "404 Not Found", b"{}").await;
    }
    if head.method.as_deref() != Some("POST") {
        return respond(&mut stream, "405 Method Not Allowed", b"{}").await;
    }
    if let Err(denied) = authorize(head.authorization.as_deref()) {
        let (status, body) = refusal(denied);
        return respond(&mut stream, status, &body).await;
    }
    let Some(length) = head.length.filter(|length| *length <= MAX_BODY_BYTES) else {
        return respond(&mut stream, "413 Content Too Large", b"{}").await;
    };
    let authorization = head.authorization.clone();
    let Some(body) = read_body(&mut stream, head, length).await? else {
        return Ok(());
    };
    let call = serde_json::from_slice::<Value>(&body).ok();
    let Some(name) = call
        .as_ref()
        .and_then(|call| call.get("name"))
        .and_then(Value::as_str)
    else {
        return respond(&mut stream, "400 Bad Request", b"{}").await;
    };
    let arguments = call
        .as_ref()
        .and_then(|call| call.get("arguments"))
        .cloned()
        .unwrap_or(Value::Null);
    match handle(authorization, name.to_owned(), arguments).await {
        Ok(reply) => {
            respond(
                &mut stream,
                "200 OK",
                reply.to_json().to_string().as_bytes(),
            )
            .await
        }
        Err(denied) => {
            let (status, body) = refusal(denied);
            respond(&mut stream, status, &body).await
        }
    }
}
