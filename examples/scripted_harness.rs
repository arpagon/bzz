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
//!   submission and does nothing for it.
//! - `log`: every message received, every tool result and the process's own
//!   arguments and environment, one JSON object per line.
//! - `state`: what survives a restart: which turns were used, which requests
//!   were seen, how often the harness started.
//! - A prompt takes the first unused turn whose `match` is in its text. A
//!   request seen before and not finished takes a turn marked `resume`.
//!
//! Steps: `{"tool": "send", "arguments": {…}, "nowait": false}`,
//! `{"text": "…"}`, `{"update": {…}}`, `{"notify": {"method": …}}`,
//! `{"request": {"method": …}}`, `{"sleep_ms": 50}`, `{"stall": true}`
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
    waiting: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
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
        if let Some(path) = &self.state_file {
            let _ = std::fs::write(path, state.to_string());
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

    /// Sends a request to bzz. With `wait`, returns its whole answer.
    fn request(&self, method: &str, params: Value, wait: bool) -> Option<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id, sender);
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
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
                self.log(json!({"response": response, "to": message["method"]}));
            } else if let Some(millis) = step.get("sleep_ms").and_then(Value::as_u64) {
                stop = pause(Some(Duration::from_millis(millis)), false);
            } else if step.get("stall").is_some() {
                stop = pause(
                    None,
                    step.get("deaf").and_then(Value::as_bool).unwrap_or(false),
                );
            } else if let Some(flood) = step.get("flood") {
                let count = |name: &str| flood.get(name).and_then(Value::as_u64).unwrap_or(0);
                let chunk = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "flood "}}}}).to_string() + "\n";
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
            if let Some(waiting) = id
                .as_ref()
                .and_then(Value::as_u64)
                .and_then(|id| self.waiting.lock().unwrap().remove(&id))
            {
                let _ = waiting.send(message);
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
                if self.own_host() {
                    result["_meta"] = json!({"bzz": {"host": "scripted", "tools": "pipe"}});
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
                answer(json!({"sessionId": session}));
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
                    answer(json!({}));
                } else if let Some(id) = &id {
                    self.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such session"}}));
                }
            }
            "session/prompt" => {
                let (harness, id) = (self.clone(), id.unwrap_or(Value::Null));
                std::thread::spawn(move || harness.prompt(id, params));
            }
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
