//! `bzz agent-tools`: an MCP server over standard input and output with the
//! tools `send`, `read` and `react` (`docs/agent-host-protocol.md`, section
//! 6). A harness starts it because bzz listed it in `session/new`.
//!
//! It holds no key and decides nothing. Each call goes to bzz on the
//! loopback address with the session's credential, both read from the
//! `BZZ_AGENT_TOOLS` variable of its own environment; bzz treats the call
//! exactly as one that came over the ACP pipe.

use std::{future::Future, net::SocketAddr};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};

use crate::{
    agents::runtime::{
        acp::{Frame, RpcError, frame_line, parse_frame},
        endpoint::TOOL_PATH,
        tools::{self, ToolReply},
    },
    error::{Error, Result},
};

/// `<address>#<credential>`.
pub const TARGET_VARIABLE: &str = "BZZ_AGENT_TOOLS";
/// MCP revisions this server answers as asked; any other gets the newest.
const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
/// A line longer than this is not a tool call.
const MAX_LINE_BYTES: usize = 1024 * 1024;

pub fn parse_target(value: &str) -> Option<(SocketAddr, String)> {
    let (address, credential) = value.split_once('#')?;
    let address = address.parse::<SocketAddr>().ok()?;
    // Only bzz on this machine is ever called.
    (address.ip().is_loopback() && !credential.is_empty()).then(|| (address, credential.to_owned()))
}

fn answer(method: &str, params: &Value) -> Option<std::result::Result<Value, RpcError>> {
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|version| PROTOCOL_VERSIONS.contains(version))
                .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "buzz", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::definitions()})),
        "tools/call" => return None,
        other => Err(RpcError::method_not_found(other)),
    })
}

/// Serves MCP until the input ends. `call` performs one tool call.
pub async fn serve<R, W, F, Fut>(reader: R, mut writer: W, call: F) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: Fn(String, Value) -> Fut,
    Fut: Future<Output = ToolReply>,
{
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        // Notifications (`notifications/initialized`) need no answer.
        let Some(Frame::Request { id, method, params }) = parse_frame(&line) else {
            continue;
        };
        let result = match answer(&method, &params) {
            Some(result) => result,
            None => match params.get("name").and_then(Value::as_str) {
                Some(name) => {
                    let reply = call(
                        name.to_owned(),
                        params.get("arguments").cloned().unwrap_or(Value::Null),
                    )
                    .await;
                    Ok(json!({
                        "content": [{"type": "text", "text": reply.text}],
                        "isError": reply.is_error,
                    }))
                }
                None => Err(RpcError::new(
                    crate::agents::runtime::acp::INVALID_PARAMS,
                    "tools/call needs a tool name",
                )),
            },
        };
        writer
            .write_all(frame_line(&Frame::Response { id, result }).as_bytes())
            .await?;
        writer.flush().await?;
    }
    Ok(())
}

/// One call to bzz. Whatever goes wrong becomes a refusal the model reads.
async fn forward(
    client: &reqwest::Client,
    address: SocketAddr,
    credential: &str,
    name: String,
    arguments: Value,
) -> ToolReply {
    const UNREACHABLE: &str = "bzz could not be reached, so nothing was done.";
    let response = client
        .post(format!("http://{address}{TOOL_PATH}"))
        .bearer_auth(credential)
        .json(&json!({"name": name, "arguments": arguments}))
        .send()
        .await;
    let Ok(response) = response else {
        return ToolReply::refused(UNREACHABLE);
    };
    response
        .json::<Value>()
        .await
        .ok()
        .and_then(|body| ToolReply::from_json(&body))
        .unwrap_or_else(|| ToolReply::refused(UNREACHABLE))
}

/// The hidden command: serves this process's standard streams.
pub async fn run_stdio() -> Result<()> {
    let target = std::env::var(TARGET_VARIABLE)
        .ok()
        .and_then(|value| parse_target(&value))
        .ok_or_else(|| {
            Error::Config(format!(
                "`bzz agent-tools` is started by an agent's harness with {TARGET_VARIABLE} set; it is not run by hand"
            ))
        })?;
    let client = reqwest::Client::builder()
        // Loopback only: no proxy of the harness's environment is used.
        .no_proxy()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|error| Error::Network(error.to_string()))?;
    let (address, credential) = target;
    serve(
        tokio::io::stdin(),
        tokio::io::stdout(),
        |name, arguments| forward(&client, address, &credential, name, arguments),
    )
    .await
    .map_err(|error| Error::io("standard streams", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt as _;

    #[test]
    fn the_target_is_a_loopback_address_and_a_credential() {
        assert_eq!(
            parse_target("127.0.0.1:4821#bzzt_abc"),
            Some(("127.0.0.1:4821".parse().unwrap(), "bzzt_abc".into()))
        );
        assert!(parse_target("[::1]:9#x").is_some());
        for wrong in [
            "",
            "127.0.0.1:4821",
            "127.0.0.1:4821#",
            "example.com:80#x",
            "10.0.0.1:80#x",
        ] {
            assert_eq!(parse_target(wrong), None, "{wrong}");
        }
    }

    #[tokio::test]
    async fn it_lists_the_three_tools_and_passes_calls_on() {
        let input = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "send", "arguments": {"text": "hi", "final": true}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "send", "arguments": {"msg": "hi"}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}),
        ]
        .iter()
        .map(|message| message.to_string() + "\n")
        .collect::<String>()
            + "not json\n";
        let (mut output, writer) = tokio::io::duplex(64 * 1024);
        let calls = std::sync::Mutex::new(Vec::new());
        serve(input.as_bytes(), writer, |name, arguments| {
            calls
                .lock()
                .unwrap()
                .push((name.clone(), arguments.clone()));
            async move {
                match tools::parse(&name, &arguments) {
                    Ok(_) => ToolReply::final_reply("sent as abc"),
                    Err(text) => ToolReply::refused(text),
                }
            }
        })
        .await
        .unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).await.unwrap();
        let answers = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            answers.len(),
            7,
            "one answer per request, none for the rest"
        );
        assert_eq!(answers[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(answers[0]["result"]["serverInfo"]["name"], "buzz");
        let listed = answers[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|tool| tool["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["send", "read", "react"]
        );
        assert_eq!(
            answers[2]["result"],
            json!({"content": [{"type": "text", "text": "sent as abc"}], "isError": false})
        );
        assert_eq!(answers[3]["result"]["isError"], true);
        assert!(
            answers[3]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no argument `msg`")
        );
        assert_eq!(answers[4]["error"]["code"], -32601);
        assert_eq!(answers[5]["error"]["code"], -32602);
        assert_eq!(answers[6]["result"], json!({}));
        assert_eq!(calls.lock().unwrap().len(), 2);
    }
}
