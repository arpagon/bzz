//! A harness for tests: it speaks ACP on its standard streams and does what
//! a script tells it. bzz's agent runtime is tested against it, so no test
//! needs Node or a model (`docs/agent-host-protocol.md` is the contract).
//!
//! `scripted_harness <script.json> [ignored…]`. The script:
//!
//! ```json
//! {"own_host": true, "load_session": false, "ignore_eof": false,
//!  "initialize": "stall" | "exit" | "version",
//!  "log": "<file>", "state": "<file>",
//!  "turns": [{"match": "<text in the prompt>", "resume": false, "steps": [ … ]}]}
//! ```
//!
//! - `own_host`: answer `initialize` as bzz's own host does and ask for
//!   tools over the pipe (`_bzz/tool`). Otherwise start the MCP server
//!   listed in `session/new` and call tools through it, as a coding harness
//!   does. An own host also treats a request id it has finished as the same
//!   submission and does nothing for it. Its requests carry string ids
//!   (`h1`, `h2`, …), as the real host's do.
//! - `run`, `packages`: what an own host reports in `_meta.bzz` of its
//!   answer to `initialize`.
//! - As a coding harness (not `own_host`), what stands in for the owner's
//!   own program:
//!   - `modes`: the modes each session offers. Absent: `default`,
//!     `workspace-write` and `auto`. A session starts in `auto`, as the
//!     owner's own settings would have it; `session/set_mode` to a mode on
//!     the list succeeds. `[]`: no modes, and `session/set_mode` is a
//!     method it does not know. `refuse_mode: true`: it refuses every mode.
//!   - `models`, `thinking`: the values of the `model` and `thought_level`
//!     options of `configOptions`. `session/set_config_option` succeeds for
//!     a value on its list.
//!   - `capabilities`: `agentCapabilities.sessionCapabilities`.
//!   - `load_lists: false`: `session/load` answers without modes or
//!     options, as a harness may.
//! - `replay`: a transcript recorded from a real program
//!   (`tests/fixtures/agents/transcripts/`). The harness then expects each
//!   message bzz sent to that program and sends what the program sent, in
//!   the transcript's order, and calls bzz's tools where it did. `folders`
//!   maps the names of granted folders to their paths in this run, and
//!   `places` any other name a transcript holds (`<not-granted>`). A
//!   message of bzz that differs is logged as a mismatch and ends the
//!   harness with status 4.
//! - `log`: every message received, every tool result and the process's own
//!   arguments and environment, one JSON object per line.
//! - `state`: what survives a restart: which turns were used, which requests
//!   were seen, how often the harness started.
//! - A prompt takes the first unused turn whose `match` is in its text. A
//!   request seen before and not finished takes a turn marked `resume`.
//!
//! Steps: `{"tool": "send", "arguments": {…}, "nowait": false}`,
//! `{"text": "…"}` (the model's text), `{"notice": "…"}` (text with no
//! message id, as Codex's adapter writes its own warnings),
//! `{"update": {…}}`, `{"notify": {"method": …}}`,
//! `{"request": {"method": …}}` (`"$session"` as its `sessionId` is the
//! turn's session; with `"then_touch": "<file>"` the file is written when
//! the answer is `{"allow": true}`, as a command that starts, or selects
//! the option `"when_selected"` names, as an edit that is made),
//! `{"requests": [{"method": …}, …]}` (all sent at once, then all awaited;
//! the log has `answered`, the order in which the answers came),
//! `{"sleep_ms": 50}`, `{"stall": true}`
//! (`"deaf": true` ignores a cancel), `{"flood": {"chunks": 1000, "garbage":
//! 10, "long_line_bytes": 5000000}}`, `{"exit": 1}`, `{"touch": "<file>"}`,
//! `{"wait_for": "<file>"}`, `{"spawn": ["sleep", "600"], "pid_file":
//! "<file>"}`, `{"stop": "refusal"}`, `{"error": {"code": -32000, "message":
//! "…"}}`.

use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader, Write as _},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use serde_json::{Value, json};

struct McpServer {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl McpServer {
    fn start(server: &Value) -> Option<Self> {
        let mut command = Command::new(server.get("command")?.as_str()?);
        for argument in server.get("args")?.as_array()? {
            command.arg(argument.as_str()?);
        }
        for variable in server
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            command.env(
                variable.get("name")?.as_str()?,
                variable.get("value")?.as_str()?,
            );
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok()?;
        let mut server = Self {
            stdin: child.stdin.take()?,
            stdout: BufReader::new(child.stdout.take()?),
            _child: child,
            next: 1,
        };
        server.call(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "scripted", "version": "0"}}),
        )?;
        writeln!(
            server.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .ok()?;
        Some(server)
    }

    fn call(&mut self, method: &str, params: Value) -> Option<Value> {
        let id = self.next;
        self.next += 1;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .ok()?;
        self.stdin.flush().ok()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).ok()?;
        serde_json::from_str::<Value>(&line).ok()
    }
}

struct Harness {
    script: Value,
    out: Mutex<std::io::Stdout>,
    log: Mutex<Option<std::fs::File>>,
    state_file: Option<PathBuf>,
    state: Mutex<Value>,
    next_id: AtomicU64,
    waiting: Mutex<HashMap<String, mpsc::Sender<Value>>>,
    /// The ids of answered requests, in the order the answers came.
    answered: Mutex<Vec<Value>>,
    servers: Mutex<HashMap<String, Arc<Mutex<McpServer>>>>,
    cancelled: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Harness {
    fn write(&self, message: &Value) {
        let mut out = self.out.lock().unwrap();
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn raw(&self, bytes: &[u8]) {
        let mut out = self.out.lock().unwrap();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }

    fn log(&self, entry: Value) {
        if let Some(file) = self.log.lock().unwrap().as_mut() {
            let _ = writeln!(file, "{entry}");
            let _ = file.flush();
        }
    }

    fn save(&self, change: impl FnOnce(&mut Value)) {
        let mut state = self.state.lock().unwrap();
        change(&mut state);
        // By rename: the harness may be killed at any moment, and half a
        // state file would make the next start forget what it did.
        if let Some(path) = &self.state_file {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, state.to_string()).is_ok() {
                let _ = std::fs::rename(&temporary, path);
            }
        }
    }

    fn own_host(&self) -> bool {
        self.script
            .get("own_host")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn notify(&self, method: &str, params: Value) {
        self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Sends a request to bzz and returns its id and where its answer
    /// arrives. An own host names its requests `h1`, `h2`, …, as bzz's real
    /// host does; another harness numbers them.
    fn send_request(&self, method: &str, params: Value) -> (Value, mpsc::Receiver<Value>) {
        let number = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = if self.own_host() {
            json!(format!("h{number}"))
        } else {
            json!(number)
        };
        let (sender, receiver) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id.to_string(), sender);
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        (id, receiver)
    }

    /// Sends a request to bzz. With `wait`, returns its whole answer.
    fn request(&self, method: &str, params: Value, wait: bool) -> Option<Value> {
        let (_, receiver) = self.send_request(method, params);
        wait.then(|| receiver.recv().ok()).flatten()
    }

    fn tool(&self, session: &str, name: &str, arguments: Value, wait: bool) -> Option<Value> {
        if self.own_host() {
            let answer = self.request(
                "_bzz/tool",
                json!({"sessionId": session, "name": name, "arguments": arguments}),
                wait,
            )?;
            return answer.get("result").cloned();
        }
        let server = self.servers.lock().unwrap().get(session).cloned()?;
        let answer = server
            .lock()
            .unwrap()
            .call("tools/call", json!({"name": name, "arguments": arguments}))?;
        let result = answer.get("result")?;
        Some(json!({
            "text": result.pointer("/content/0/text").cloned().unwrap_or_default(),
            "isError": result.get("isError").cloned().unwrap_or(json!(false)),
        }))
    }

    /// The steps for one prompt, by the rules in the file's header.
    fn steps_for(&self, text: &str, request: Option<&str>) -> Option<Vec<Value>> {
        let seen = request.and_then(|id| {
            self.state.lock().unwrap()["seen"][id]
                .as_str()
                .map(str::to_owned)
        });
        if self.own_host() && seen.as_deref() == Some("done") {
            return None;
        }
        let resumed = seen.is_some();
        let turns = self
            .script
            .get("turns")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let used = self.state.lock().unwrap()["used"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let pick = |resume: bool| {
            turns.iter().enumerate().find(|(index, turn)| {
                !used.contains(&json!(index))
                    && turn.get("resume").and_then(Value::as_bool).unwrap_or(false) == resume
                    && turn
                        .get("match")
                        .and_then(Value::as_str)
                        .is_none_or(|part| text.contains(part))
            })
        };
        let chosen = if resumed {
            pick(true).or_else(|| pick(false))
        } else {
            pick(false)
        };
        let steps = chosen.map(|(index, turn)| {
            self.save(|state| state["used"].as_array_mut().unwrap().push(json!(index)));
            turn.get("steps")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        });
        if let Some(id) = request {
            self.save(|state| state["seen"][id] = json!("open"));
        }
        Some(steps.unwrap_or_default())
    }

    fn prompt(self: &Arc<Self>, id: Value, params: Value) {
        let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
        let text = params
            .pointer("/prompt/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let request = params
            .pointer("/_meta/bzz~1request")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let cancelled = self
            .cancelled
            .lock()
            .unwrap()
            .entry(session.clone())
            .or_default()
            .clone();
        cancelled.store(false, Ordering::Release);
        let mut answer = json!({"result": {"stopReason": "end_turn"}});
        let pause = |limit: Option<Duration>, deaf: bool| {
            let started = std::time::Instant::now();
            while limit.is_none_or(|limit| started.elapsed() < limit) {
                if !deaf && cancelled.load(Ordering::Acquire) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            false
        };
        for step in self
            .steps_for(&text, request.as_deref())
            .unwrap_or_default()
        {
            let mut stop = false;
            if let Some(name) = step.get("tool").and_then(Value::as_str) {
                let wait = !step.get("nowait").and_then(Value::as_bool).unwrap_or(false);
                let result = self.tool(
                    &session,
                    name,
                    step.get("arguments").cloned().unwrap_or(Value::Null),
                    wait,
                );
                self.log(json!({"tool": name, "session": session, "result": result}));
            } else if let Some(chunk) = step.get("text").and_then(Value::as_str) {
                // The model's text, with the id of its message as the
                // adapters of Codex and Claude Code give it.
                self.notify(
                    "session/update",
                    json!({"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "messageId": "msg_scripted", "content": {"type": "text", "text": chunk}}}),
                );
            } else if let Some(chunk) = step.get("notice").and_then(Value::as_str) {
                // What Codex's adapter writes of its own: message text
                // with no message id.
                self.notify(
                    "session/update",
                    json!({"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": chunk}}}),
                );
            } else if let Some(update) = step.get("update") {
                self.notify(
                    "session/update",
                    json!({"sessionId": session, "update": update}),
                );
            } else if let Some(message) = step.get("notify") {
                self.notify(
                    message["method"].as_str().unwrap_or("x"),
                    message.get("params").cloned().unwrap_or(Value::Null),
                );
            } else if let Some(message) = step.get("request") {
                let mut params = message.get("params").cloned().unwrap_or(Value::Null);
                if params.get("sessionId") == Some(&json!("$session")) {
                    params["sessionId"] = json!(session);
                }
                let response =
                    self.request(message["method"].as_str().unwrap_or("x"), params, true);
                // As a command that starts only on a yes.
                let answered = |pointer: &str| {
                    response
                        .as_ref()
                        .and_then(|response| response.pointer(pointer))
                        .cloned()
                };
                if let Some(path) = step.get("then_touch").and_then(Value::as_str)
                    && (answered("/result/allow") == Some(json!(true))
                        || step.get("when_selected").is_some_and(|option| {
                            Some(option) == answered("/result/outcome/optionId").as_ref()
                        }))
                {
                    let _ = std::fs::write(path, "started\n");
                }
                self.log(json!({"response": response, "to": message["method"]}));
            } else if let Some(messages) = step.get("requests").and_then(Value::as_array) {
                // All sent before any answer is read: several are open at
                // once, and bzz answers them in whatever order they end.
                let open = messages
                    .iter()
                    .map(|message| {
                        let mut params = message.get("params").cloned().unwrap_or(Value::Null);
                        if params.get("sessionId") == Some(&json!("$session")) {
                            params["sessionId"] = json!(session);
                        }
                        let method = message["method"].as_str().unwrap_or("x").to_owned();
                        let (id, receiver) = self.send_request(&method, params);
                        (id, method, receiver)
                    })
                    .collect::<Vec<_>>();
                let ids = open.iter().map(|(id, ..)| id.clone()).collect::<Vec<_>>();
                let responses = open
                    .into_iter()
                    .map(|(id, method, receiver)| {
                        json!({"id": id, "to": method, "response": receiver.recv().ok()})
                    })
                    .collect::<Vec<_>>();
                let answered = self
                    .answered
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|id| ids.contains(id))
                    .cloned()
                    .collect::<Vec<_>>();
                self.log(json!({"responses": responses, "sent": ids, "answered": answered}));
            } else if let Some(millis) = step.get("sleep_ms").and_then(Value::as_u64) {
                stop = pause(Some(Duration::from_millis(millis)), false);
            } else if step.get("stall").is_some() {
                stop = pause(
                    None,
                    step.get("deaf").and_then(Value::as_bool).unwrap_or(false),
                );
            } else if let Some(flood) = step.get("flood") {
                let count = |name: &str| flood.get(name).and_then(Value::as_u64).unwrap_or(0);
                let chunk = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "messageId": "msg_scripted", "content": {"type": "text", "text": "flood "}}}}).to_string() + "\n";
                for _ in 0..count("chunks") {
                    self.raw(chunk.as_bytes());
                }
                for index in 0..count("garbage") {
                    self.raw(format!("npm warn {index} {{not json\n").as_bytes());
                }
                let long = usize::try_from(count("long_line_bytes")).unwrap_or(0);
                if long > 0 {
                    let mut line = vec![b'x'; long];
                    line.push(b'\n');
                    self.raw(&line);
                }
            } else if let Some(code) = step.get("exit").and_then(Value::as_i64) {
                self.log(json!({"exit": code}));
                std::process::exit(i32::try_from(code).unwrap_or(1));
            } else if let Some(path) = step.get("touch").and_then(Value::as_str) {
                let _ = std::fs::write(path, "reached\n");
            } else if let Some(path) = step.get("wait_for").and_then(Value::as_str) {
                while !std::path::Path::new(path).exists() && !cancelled.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            } else if let Some(command) = step.get("spawn").and_then(Value::as_array) {
                let mut arguments = command.iter().filter_map(Value::as_str);
                if let Some(program) = arguments.next()
                    && let Ok(child) = Command::new(program)
                        .args(arguments)
                        .stdin(Stdio::null())
                        .spawn()
                    && let Some(path) = step.get("pid_file").and_then(Value::as_str)
                {
                    let _ = std::fs::write(path, format!("{}\n", child.id()));
                }
            } else if let Some(reason) = step.get("stop").and_then(Value::as_str) {
                answer = json!({"result": {"stopReason": reason}});
                break;
            } else if let Some(error) = step.get("error") {
                answer = json!({"error": error});
                break;
            }
            if stop || cancelled.load(Ordering::Acquire) {
                answer = json!({"result": {"stopReason": "cancelled"}});
                break;
            }
        }
        if let Some(request) = &request {
            self.save(|state| state["seen"][request] = json!("done"));
        }
        answer["jsonrpc"] = json!("2.0");
        answer["id"] = id;
        self.write(&answer);
    }

    fn open_session(&self, params: &Value, session: &str) {
        if self.own_host() {
            return;
        }
        if let Some(server) = params.pointer("/mcpServers/0").and_then(McpServer::start) {
            let server = Arc::new(Mutex::new(server));
            let tools = server.lock().unwrap().call("tools/list", json!({}));
            self.log(json!({"mcp": "tools/list", "session": session, "answer": tools}));
            self.servers
                .lock()
                .unwrap()
                .insert(session.to_owned(), server);
        } else {
            self.log(json!({"mcp": "not started", "session": session}));
        }
    }

    fn list(&self, name: &str) -> Option<Vec<Value>> {
        self.script.get(name).and_then(Value::as_array).cloned()
    }

    /// The modes a coding harness offers: those of the script, or the ones
    /// the two adapters need and the permissive one a session starts in.
    fn modes(&self) -> Vec<Value> {
        self.list("modes")
            .unwrap_or_else(|| vec![json!("default"), json!("workspace-write"), json!("auto")])
    }

    /// What a coding harness answers when a session opens, beside its id.
    fn session_answer(&self, mut answer: Value) -> Value {
        if self.own_host() {
            return answer;
        }
        let modes = self.modes();
        if !modes.is_empty() {
            answer["modes"] = json!({
                "currentModeId": "auto",
                "availableModes": modes.iter().map(|mode| json!({"id": mode, "name": mode})).collect::<Vec<_>>(),
            });
        }
        let option = |id: &str, category: &str, values: Vec<Value>| {
            json!({
                "id": id, "name": id, "category": category, "type": "select",
                "currentValue": values.first().cloned().unwrap_or(Value::Null),
                "options": values.iter().map(|value| json!({"value": value, "name": value})).collect::<Vec<_>>(),
            })
        };
        let mut options = Vec::new();
        if let Some(models) = self.list("models") {
            options.push(option("model", "model", models));
        }
        if let Some(levels) = self.list("thinking") {
            options.push(option("effort", "thought_level", levels));
        }
        if !options.is_empty() {
            answer["configOptions"] = json!(options);
        }
        answer
    }

    fn handle(self: &Arc<Self>, message: Value) {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = message.get("id").cloned();
        if method.is_empty() {
            // An answer to a request of ours.
            let key = id.as_ref().map(Value::to_string).unwrap_or_default();
            self.answered
                .lock()
                .unwrap()
                .push(id.clone().unwrap_or(Value::Null));
            match self.waiting.lock().unwrap().remove(&key) {
                Some(waiting) if waiting.send(message.clone()).is_ok() => {}
                // Nobody waits for it any more.
                _ => self.log(json!({"unclaimed": message})),
            }
            return;
        }
        self.log(json!({"got": method, "params": params}));
        let answer = |result: Value| {
            if let Some(id) = &id {
                self.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
            }
        };
        match method.as_str() {
            "initialize" => {
                let starts = self.state.lock().unwrap()["starts"].as_u64().unwrap_or(0) + 1;
                self.save(|state| state["starts"] = json!(starts));
                match self.script.get("initialize").and_then(Value::as_str) {
                    Some("stall") => return,
                    Some("exit") => std::process::exit(3),
                    Some("version") => {
                        return answer(json!({"protocolVersion": 2, "agentCapabilities": {}}));
                    }
                    _ => {}
                }
                let mut result = json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {"loadSession": self.script.get("load_session").and_then(Value::as_bool).unwrap_or(false)},
                });
                if let Some(capabilities) = self.script.get("capabilities") {
                    result["agentCapabilities"]["sessionCapabilities"] = capabilities.clone();
                }
                if self.own_host() {
                    result["_meta"] = json!({"bzz": {
                        "host": "scripted",
                        "tools": "pipe",
                        "run": self.script.get("run").cloned().unwrap_or(json!([])),
                        "packages": self.script.get("packages").cloned().unwrap_or(json!([])),
                    }});
                }
                answer(result);
            }
            "session/new" => {
                let count = self.state.lock().unwrap()["sessions"]
                    .as_array()
                    .map_or(0, Vec::len);
                let session = format!("s{}-{}", count + 1, std::process::id());
                self.save(|state| {
                    state["sessions"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(session))
                });
                self.open_session(&params, &session);
                answer(self.session_answer(json!({"sessionId": session})));
            }
            "session/set_mode" if !self.own_host() && !self.modes().is_empty() => {
                let wanted = params.get("modeId").cloned().unwrap_or(Value::Null);
                let refuses = self.script.get("refuse_mode") == Some(&json!(true));
                if self.modes().contains(&wanted) && !refuses {
                    answer(json!({}));
                } else if let Some(id) = &id {
                    self.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such mode"}}));
                }
            }
            "session/set_config_option" if !self.own_host() => {
                let offered = match params.get("configId").and_then(Value::as_str) {
                    Some("model") => self.list("models"),
                    Some("effort") => self.list("thinking"),
                    _ => None,
                }
                .unwrap_or_default();
                if params
                    .get("value")
                    .is_some_and(|value| offered.contains(value))
                {
                    answer(self.session_answer(json!({})));
                } else if let Some(id) = &id {
                    self.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such value"}}));
                }
            }
            "session/load" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let known = self.state.lock().unwrap()["sessions"]
                    .as_array()
                    .is_some_and(|all| all.contains(&json!(session)));
                if known
                    && self
                        .script
                        .get("load_session")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                {
                    self.open_session(&params, &session);
                    answer(if self.script.get("load_lists") == Some(&json!(false)) {
                        json!({})
                    } else {
                        self.session_answer(json!({}))
                    });
                } else if let Some(id) = &id {
                    self.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such session"}}));
                }
            }
            "session/prompt" => {
                let (harness, id) = (self.clone(), id.unwrap_or(Value::Null));
                std::thread::spawn(move || harness.prompt(id, params));
            }
            // bzz's own host checkpoints its files; this one only says so.
            "_bzz/checkpoint" if self.own_host() => answer(json!({})),
            "session/cancel" => {
                if let Some(flag) = self
                    .cancelled
                    .lock()
                    .unwrap()
                    .get(params["sessionId"].as_str().unwrap_or_default())
                {
                    flag.store(true, Ordering::Release);
                }
            }
            _ => {
                if let Some(id) = &id {
                    self.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}));
                }
            }
        }
    }
}

/// Whether `actual` is what the transcript expects: every member the
/// transcript names is there with its value, `<any>`, `<credential>` and
/// `<bzz>` stand for any text, and the folders are those of this run.
fn expected_matches(expected: &Value, actual: &Value, places: &[(String, String)]) -> bool {
    match (expected, actual) {
        (Value::String(text), _) if text == "<any>" => true,
        (Value::String(text), Value::String(actual)) => {
            let mut text = text.clone();
            for (name, path) in places {
                text = text.replace(name, path);
            }
            // The parts between two wildcards, in their order.
            let mut parts = Vec::new();
            let mut rest = text.as_str();
            loop {
                let next = ["<any>", "<credential>", "<bzz>"]
                    .iter()
                    .filter_map(|wild| rest.find(wild).map(|at| (at, wild.len())))
                    .min();
                match next {
                    Some((at, length)) => {
                        parts.push(&rest[..at]);
                        rest = &rest[at + length..];
                    }
                    None => {
                        parts.push(rest);
                        break;
                    }
                }
            }
            if parts.len() == 1 {
                return parts[0] == actual;
            }
            let (first, last) = (parts[0], parts[parts.len() - 1]);
            if !actual.starts_with(first)
                || !actual.ends_with(last)
                || actual.len() < first.len() + last.len()
            {
                return false;
            }
            let mut at = first.len();
            let end = actual.len() - last.len();
            parts[1..parts.len() - 1].iter().all(|part| {
                actual[at..end].find(part).is_some_and(|found| {
                    at += found + part.len();
                    true
                })
            })
        }
        (Value::Object(expected), Value::Object(actual)) => expected.iter().all(|(name, value)| {
            actual
                .get(name)
                .is_some_and(|actual| expected_matches(value, actual, places))
        }),
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(expected, actual)| expected_matches(expected, actual, places))
        }
        _ => expected == actual,
    }
}

impl Harness {
    /// Plays a recorded transcript: expects what bzz sent the real program,
    /// in its order, and sends what the program sent.
    fn replay(self: &Arc<Self>, transcript: &str) {
        let workspace = std::env::current_dir().unwrap_or_default();
        let folder = |path: &std::path::Path| path.display().to_string();
        let mut places = vec![("<workspace>".to_owned(), folder(&workspace))];
        for (name, path) in self
            .script
            .get("folders")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            places.push((
                format!("<folder:{name}>"),
                path.as_str().unwrap_or_default().to_owned(),
            ));
        }
        // Any other folder the transcript names: `{"<not-granted>": …}`.
        for (name, path) in self
            .script
            .get("places")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            places.push((name.clone(), path.as_str().unwrap_or_default().to_owned()));
        }
        // `<data>/agents/<id>/workspace`.
        if let Some(data) = workspace.ancestors().nth(3) {
            places.push(("<data>".into(), folder(data)));
        }
        places.push(("<home>".into(), std::env::var("HOME").unwrap_or_default()));
        let (sender, incoming) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(message) = serde_json::from_str::<Value>(&line)
                    && sender.send(message).is_err()
                {
                    break;
                }
            }
        });
        let fail = |line: usize, expected: &Value, got: Value| -> ! {
            self.log(json!({"replay": "mismatch", "line": line, "expected": expected, "got": got}));
            eprintln!("replay: line {line} of the transcript is not what bzz sent");
            std::process::exit(4);
        };
        // The ids of bzz's requests, as recorded and as they are now.
        let mut ids = HashMap::<String, Value>::new();
        // The parameters of a session being opened, until its id is known.
        let mut opening = None::<Value>;
        for (index, line) in transcript.lines().enumerate() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if let Some(expected) = entry.get("out") {
                let got = match incoming.recv_timeout(Duration::from_secs(30)) {
                    Ok(got) => got,
                    Err(_) => fail(index + 1, expected, json!("nothing came in 30 s")),
                };
                let mut wanted = expected.clone();
                let recorded = wanted.as_object_mut().and_then(|object| {
                    object.remove("jsonrpc");
                    object.remove("id")
                });
                if !expected_matches(&wanted, &got, &places) {
                    fail(index + 1, expected, got);
                }
                self.log(json!({"got": got.get("method"), "params": got.get("params"), "replayed": index + 1}));
                let method = got.get("method").and_then(Value::as_str);
                match (method, recorded, got.get("id")) {
                    // A request of bzz: its answer goes back under its id.
                    (Some(_), Some(recorded), Some(id)) => {
                        ids.insert(recorded.to_string(), id.clone());
                    }
                    // An answer of bzz to a request of the program.
                    (None, Some(recorded), id) if id != Some(&recorded) => {
                        fail(index + 1, expected, got.clone());
                    }
                    _ => {}
                }
                match method {
                    Some("session/new") => opening = got.get("params").cloned(),
                    Some("session/load") => {
                        let params = got.get("params").cloned().unwrap_or(Value::Null);
                        let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                        self.open_session(&params, &session);
                    }
                    _ => {}
                }
            } else if let Some(message) = entry.get("in") {
                let mut text = message.to_string();
                for (name, path) in &places {
                    let quoted = serde_json::to_string(path).unwrap_or_default();
                    text = text.replace(name, quoted.trim_matches('"'));
                }
                let mut message = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
                if message.get("method").is_none()
                    && let Some(recorded) = message.get("id")
                {
                    match ids.remove(&recorded.to_string()) {
                        Some(id) => message["id"] = id,
                        None => fail(index + 1, &entry, json!("an answer to no request of bzz")),
                    }
                    // The program has named the session bzz was opening.
                    if let (Some(params), Some(session)) = (
                        opening.as_ref(),
                        message.pointer("/result/sessionId").and_then(Value::as_str),
                    ) {
                        self.open_session(params, session);
                        opening = None;
                    }
                }
                self.write(&message);
            } else if let Some(call) = entry.get("tool") {
                let name = call["name"].as_str().unwrap_or_default();
                let session = call["session"].as_str().unwrap_or_default();
                let result = self.tool(
                    session,
                    name,
                    call.get("arguments").cloned().unwrap_or(Value::Null),
                    true,
                );
                self.log(json!({"tool": name, "session": session, "result": result}));
            }
        }
        self.log(json!({"replay": "complete"}));
        // Until bzz closes the pipe.
        while incoming.recv().is_ok() {}
        self.log(json!({"eof": true}));
    }
}

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    let script = arguments
        .get(1)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| {
            eprintln!("usage: scripted_harness <script.json>");
            std::process::exit(2);
        });
    let path = |name: &str| script.get(name).and_then(Value::as_str).map(PathBuf::from);
    let state_file = path("state");
    let mut state = state_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
    for (name, empty) in [
        ("used", json!([])),
        ("sessions", json!([])),
        ("seen", json!({})),
    ] {
        if state.get(name).is_none() {
            state[name] = empty;
        }
    }
    let log = path("log").and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    });
    let harness = Arc::new(Harness {
        out: Mutex::new(std::io::stdout()),
        log: Mutex::new(log),
        state_file,
        state: Mutex::new(state),
        next_id: AtomicU64::new(1),
        waiting: Mutex::new(HashMap::new()),
        answered: Mutex::new(Vec::new()),
        servers: Mutex::new(HashMap::new()),
        cancelled: Mutex::new(HashMap::new()),
        script,
    });
    // What a harness can see of bzz: its own arguments and environment.
    harness.log(json!({"start": {
        "pid": std::process::id(),
        "argv": arguments,
        "env": std::env::vars().collect::<HashMap<_, _>>(),
        "cwd": std::env::current_dir().ok(),
    }}));
    eprintln!("scripted harness {} started", std::process::id());
    if let Some(transcript) = harness.script.get("replay").and_then(Value::as_str) {
        let transcript = std::fs::read_to_string(transcript).unwrap_or_else(|error| {
            eprintln!("replay: {transcript}: {error}");
            std::process::exit(2);
        });
        harness.replay(&transcript);
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if let Ok(message) = serde_json::from_str::<Value>(&line) {
            harness.handle(message);
        }
    }
    // End of input: bzz is stopping the harness, or is gone.
    harness.log(json!({"eof": true}));
    if harness
        .script
        .get("ignore_eof")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}
