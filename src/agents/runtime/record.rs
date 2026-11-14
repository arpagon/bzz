//! A transcript of one harness process: every message bzz sent it and every
//! message it sent, in the order bzz saw them, with each call of bzz's chat
//! tools between them. The tests replay such transcripts against bzz's real
//! runtime with only the pipe replaced, so that bzz is tested against what
//! Grok, Codex and Claude Code really send where no sign-in exists
//! (`tests/fixtures/agents/transcripts/`, plan v0.15.0 M6).
//!
//! Written only when `BZZ_AGENT_RECORD` names a folder. A transcript holds
//! the text of the conversation it records, so it is for a disposable
//! conversation. What it never holds:
//!
//! - the session credential of `bzz agent-tools`;
//! - what a harness says outside the protocol's own messages. Grok lists
//!   the owner's own MCP servers in a notification, with whatever their
//!   settings hold: every notification but `session/update` is kept by its
//!   name alone;
//! - what an answer holds beyond what bzz reads: account, usage, the
//!   machine's name.
//!
//! One line is one JSON object: `{"transcript": …}` first, then
//! `{"out": <message>}` for what bzz sent, `{"in": <message>}` for what the
//! harness sent, and `{"tool": {"session", "name", "arguments"}}` for a
//! call of a chat tool. Paths of the machine are written as `<workspace>`,
//! `<folder:NAME>`, `<data>` and `<home>`; what changes from one run to the
//! next (a prompt's text, an event id) as `<any>`.

use std::{
    collections::HashMap,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};

/// The variable of bzz's environment that names the folder transcripts are
/// written in.
pub const VARIABLE: &str = "BZZ_AGENT_RECORD";

const ANY: &str = "<any>";

pub struct Recorder {
    file: Mutex<std::fs::File>,
    /// Text of this machine and what is written in its place, the longest
    /// first.
    places: Vec<(String, String)>,
    /// The method of each request bzz has open, by its id.
    open: Mutex<HashMap<String, String>>,
}

/// A path as it stands inside a JSON string.
fn in_json(path: &Path) -> String {
    let quoted = serde_json::to_string(&path.display().to_string()).unwrap_or_default();
    quoted.trim_matches('"').to_owned()
}

/// Writes `name` for every `path` in `line` that ends there: the folder
/// itself or something inside it, never a longer name that starts alike.
fn replace_path(line: &str, path: &str, name: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(path) {
        let after = &rest[at + path.len()..];
        let ends = after
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || "-_.".contains(next)));
        out.push_str(&rest[..at]);
        out.push_str(if ends { name } else { path });
        rest = after;
    }
    out.push_str(rest);
    out
}

fn set(value: &mut Value, pointer: &str, with: Value) {
    if let Some(slot) = value.pointer_mut(pointer) {
        *slot = with;
    }
}

/// What bzz sent, without what changes from run to run and without the
/// session credential.
fn scrub_out(message: &mut Value) {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match method.as_str() {
        "initialize" => {
            set(message, "/params/clientInfo/version", json!(ANY));
            set(message, "/params/_meta", json!(ANY));
        }
        "session/new" | "session/load" => {
            for server in message
                .pointer_mut("/params/mcpServers")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                set(server, "/command", json!("<bzz>"));
                for variable in server
                    .get_mut("env")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    set(variable, "/value", json!("<credential>"));
                }
            }
            for block in message
                .pointer_mut("/params/systemPrompt/content")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                set(block, "/text", json!(ANY));
            }
            set(message, "/params/_meta/rules", json!(ANY));
            set(message, "/params/_meta/bzz~1place", json!(ANY));
        }
        "session/prompt" => {
            for block in message
                .pointer_mut("/params/prompt")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                set(block, "/text", json!(ANY));
            }
            set(message, "/params/_meta/bzz~1request", json!(ANY));
        }
        _ => {}
    }
}

/// Keeps of an object only the members named.
fn keep(value: &mut Value, names: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|name, _| names.contains(&name.as_str()));
    }
}

/// What a harness sent, without what bzz does not read.
fn scrub_in(message: &mut Value, answered: Option<&str>) {
    match message.get("method").and_then(Value::as_str) {
        Some("session/update") => {
            // Usage is the owner's account; the commands on offer are the
            // owner's own skills and settings.
            if let Some(update) = message.pointer_mut("/params/update")
                && matches!(
                    update.get("sessionUpdate").and_then(Value::as_str),
                    Some("usage_update" | "available_commands_update")
                )
            {
                keep(update, &["sessionUpdate"]);
            }
        }
        Some("session/request_permission") => {}
        // Anything else is kept by its name alone.
        Some(_) => set(message, "/params", json!({})),
        None => {
            let Some(result) = message.get_mut("result") else {
                return;
            };
            match answered {
                Some("initialize") => {
                    keep(
                        result,
                        &[
                            "protocolVersion",
                            "agentCapabilities",
                            "agentInfo",
                            "authMethods",
                        ],
                    );
                    if let Some(capabilities) = result.get_mut("agentCapabilities")
                        && let Some(object) = capabilities.as_object_mut()
                    {
                        object.remove("_meta");
                    }
                    for method in result
                        .get_mut("authMethods")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                    {
                        keep(method, &["id"]);
                    }
                }
                Some("session/new" | "session/load") => {
                    keep(result, &["sessionId", "modes", "configOptions"]);
                }
                Some("session/prompt") => keep(result, &["stopReason"]),
                _ => {}
            }
        }
    }
}

impl Recorder {
    /// Starts a transcript in `directory`. `places` are folders of this
    /// machine and the names written in their place; `about` says what was
    /// recorded.
    pub fn create(
        directory: &Path,
        name: &str,
        about: &Value,
        places: &[(PathBuf, String)],
    ) -> Option<Arc<Self>> {
        std::fs::create_dir_all(directory).ok()?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(format!("{name}-{stamp}.ndjson")))
            .ok()?;
        let mut places = places
            .iter()
            .map(|(path, name)| (in_json(path), name.clone()))
            .filter(|(path, _)| path.len() > 1)
            .collect::<Vec<_>>();
        places.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        writeln!(file, "{}", json!({"transcript": about})).ok()?;
        Some(Arc::new(Self {
            file: Mutex::new(file),
            places,
            open: Mutex::new(HashMap::new()),
        }))
    }

    fn write(&self, entry: &Value) {
        let mut line = entry.to_string();
        for (path, name) in &self.places {
            line = replace_path(&line, path, name);
        }
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }

    /// One line of the protocol, as it was written or read.
    pub fn line(&self, outgoing: bool, line: &str) {
        let Ok(mut message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let id = message.get("id").filter(|id| !id.is_null()).cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if outgoing {
            if let (Some(id), Some(method)) = (&id, &method) {
                open.insert(id.to_string(), method.clone());
            }
            drop(open);
            scrub_out(&mut message);
            self.write(&json!({"out": message}));
        } else {
            let answered = match (&id, &method) {
                (Some(id), None) => open.remove(&id.to_string()),
                _ => None,
            };
            drop(open);
            scrub_in(&mut message, answered.as_deref());
            self.write(&json!({"in": message}));
        }
    }

    /// A call of one of bzz's chat tools, as it reached bzz.
    pub fn tool(&self, session: &str, name: &str, arguments: &Value) {
        self.write(&json!({"tool": {"session": session, "name": name, "arguments": arguments}}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcript_holds_no_credential_no_path_and_nothing_bzz_does_not_read() {
        let temporary = tempfile::TempDir::new().unwrap();
        let recorder = Recorder::create(
            temporary.path(),
            "grok",
            &json!({"harness": "grok"}),
            &[
                ("/d".into(), "<data>".into()),
                ("/d/agents/a/workspace".into(), "<workspace>".into()),
                ("/home/ann".into(), "<home>".into()),
                ("/srv/work".into(), "<folder:work>".into()),
            ],
        )
        .unwrap();
        let out = |message: Value| recorder.line(true, &message.to_string());
        let received = |message: Value| recorder.line(false, &message.to_string());
        out(
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": 1, "clientInfo": {"name": "bzz", "version": "0.15.0"},
            "_meta": {"bzz": {"agent": {"pubkey": "abc"}}}}}),
        );
        received(json!({"jsonrpc": "2.0", "id": 1, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {"loadSession": true, "_meta": {"x": 1}},
            "authMethods": [{"id": "cached_token", "description": "Cached token from ~/.grok/auth.json"}],
            "_meta": {"hostname": "ann-laptop.local", "agentVersion": "1.0.77"}}}));
        out(
            json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {
            "cwd": "/d/agents/a/workspace",
            "additionalDirectories": ["/srv/work", "/srv/work-2", "/dev/null"],
            "mcpServers": [{"name": "buzz", "command": "/usr/bin/bzz", "args": ["agent-tools"],
                "env": [{"name": "BZZ_AGENT_TOOLS", "value": "127.0.0.1:4#bzzs_secret"}]}],
            "systemPrompt": {"mode": "append", "content": [{"type": "text", "text": "You are Scout"}]},
            "_meta": {"bzz/place": "chan/evt", "rules": "You are Scout"}}}),
        );
        received(json!({"jsonrpc": "2.0", "id": 2, "result": {
            "sessionId": "s1", "modes": {"currentModeId": "auto"},
            "models": {"currentModelId": "m"}, "_meta": {"account": "ann@example.org"}}}));
        received(
            json!({"jsonrpc": "2.0", "method": "_x.ai/mcp/servers_updated", "params": {
            "mcpServers": [{"name": "postgres", "env": [{"name": "DATABASE_URI", "value": "postgresql://u:hunter2@db"}]}]}}),
        );
        received(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1",
            "update": {"sessionUpdate": "usage_update", "cost": {"amount": 3}}}}),
        );
        out(
            json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {
            "sessionId": "s1", "prompt": [{"type": "text", "text": "From: ann…"}],
            "_meta": {"bzz/request": "evt"}}}),
        );
        received(
            json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {
            "toolCall": {"kind": "edit", "locations": [{"path": "/home/ann/.ssh/config"}]}, "options": []}}),
        );
        recorder.tool("s1", "send", &json!({"text": "hi", "final": true}));
        received(json!({"jsonrpc": "2.0", "id": 3, "result": {
            "stopReason": "end_turn", "usage": {"totalTokens": 9}, "_meta": {"quota": {}}}}));

        let file = std::fs::read_dir(temporary.path())
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        let text = std::fs::read_to_string(&file).unwrap();
        for absent in [
            "bzzs_secret",
            "hunter2",
            "postgres",
            "ann-laptop",
            "ann@example.org",
            "/usr/bin/bzz",
            "/d/agents",
            "/home/ann",
            "\"/srv/work\"",
            "You are Scout",
            "From: ann",
            "chan/evt",
            "totalTokens",
            "quota",
            "cost",
            "auth.json",
            "abc",
        ] {
            assert!(!text.contains(absent), "`{absent}` in:\n{text}");
        }
        let lines = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(lines[0], json!({"transcript": {"harness": "grok"}}));
        assert_eq!(
            lines[2]["in"]["result"]["agentCapabilities"],
            json!({"loadSession": true})
        );
        assert_eq!(
            lines[2]["in"]["result"]["authMethods"],
            json!([{"id": "cached_token"}])
        );
        let opened = &lines[3]["out"]["params"];
        assert_eq!(opened["cwd"], "<workspace>");
        // A longer name that starts like a folder's is another folder.
        assert_eq!(
            opened["additionalDirectories"],
            json!(["<folder:work>", "/srv/work-2", "/dev/null"])
        );
        assert_eq!(opened["mcpServers"][0]["command"], "<bzz>");
        assert_eq!(opened["mcpServers"][0]["env"][0]["value"], "<credential>");
        assert_eq!(opened["systemPrompt"]["content"][0]["text"], "<any>");
        assert_eq!(
            lines[4]["in"]["result"],
            json!({"sessionId": "s1", "modes": {"currentModeId": "auto"}})
        );
        assert_eq!(
            lines[5]["in"],
            json!({"jsonrpc": "2.0", "method": "_x.ai/mcp/servers_updated", "params": {}})
        );
        assert_eq!(
            lines[6]["in"]["params"]["update"],
            json!({"sessionUpdate": "usage_update"})
        );
        assert_eq!(lines[7]["out"]["params"]["prompt"][0]["text"], "<any>");
        assert_eq!(
            lines[8]["in"]["params"]["toolCall"]["locations"][0]["path"],
            "<home>/.ssh/config"
        );
        assert_eq!(
            lines[9],
            json!({"tool": {"session": "s1", "name": "send", "arguments": {"text": "hi", "final": true}}})
        );
        assert_eq!(lines[10]["in"]["result"], json!({"stopReason": "end_turn"}));
    }
}
