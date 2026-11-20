//! The app runtime alone in a terminal, on the simulated data service of
//! the apps bench (`tests/apps-bench/`). It is how the bench judges bzz's
//! own runtime with real apps, and a way to work on a script by hand:
//!
//! ```sh
//! tests/apps-bench/data-service --app board      # prints its addresses
//! MINIAPP_DATA_ADDR=127.0.0.1:PORT MINIAPP_USER=ana \
//!     cargo run --example app_host -- tests/fixtures/apps/board
//! ```
//!
//! Usage: `app_host <app.lua | folder with app.lua>`. The app is drawn in
//! the whole terminal. `Ctrl+C` leaves. **Saving the file reloads the
//! script in place**; a version that fails to load leaves the old one
//! running. `APP_HOST_LOG=<file>` keeps the app's log, with every error in
//! full.
//!
//! bzz itself never speaks the bench's data protocol
//! (`tests/apps-bench/spec/data-protocol.md`); **this file is the only
//! place that knows it**. [`Bridge`] turns the service's answers into the
//! records of the contract `api = 1` (`docs/app-api.md`). The two differ
//! here, and the bench's cases were written for the service:
//!
//! | The simulated service | What the script is given |
//! |---|---|
//! | `rev` is a number that grows by one | an opaque string; the host keeps which number it stands for |
//! | ids `r0001`, `r0002`: they sort by creation | ids that say nothing; the host keeps the service's id of each |
//! | `list` returns records in creation order | in the order of those ids: no order a script may rely on |
//! | `list` without a `type` returns every record | never asked: the contract's `data.list` names one type |
//! | every reply and push carries `seq`, a change counter | nothing: the contract has no `seq`. The host uses it for the next row |
//! | a subscriber is pushed every change from the moment it subscribes | pushes of a type begin with the script's first list of it: before the reply to that list a push of the type is dropped, or, while the list is on its way, held and given after the reply if its `seq` is newer than the list's |
//! | `list` is answered when the service answers, late in some scenarios (`slow-load`, `late-search`) and failing in one (`load-error`) | the same, passed on: bzz answers a list at once from the records it holds, and only waits (up to five seconds) for an app that holds none yet. The scenarios stand for that wait and for a relay that did not answer |
//! | a participant is a name (`"by": "bo"`, `MINIAPP_USER`) | `{ id, name }`; the id is made from the name, so two people of one name are one |
//! | a record does not say who wrote it | `record.by` is who the host last saw write it (a push, the app's own write), and a participant without a name for a record it has only listed |
//! | a type is any string, stored as given | the app's short name, unchanged: no `bzz.app.<app>.<record>` is made |
//! | `match` looks at `content.title`, and at nothing when there is none | passed on as it is; the contract matches the type's name when a record has no title |
//! | errors: `unavailable`, `conflict`, `not_found`, `bad_request` | the same four. `rate_limited`, `limit` and `refused` never happen here; `too_large` is the runtime's own check |
//! | no limit of records, and none of writes | none here either: the 1000 records of an app and the 30 writes a minute of a person's apps are bzz's session's (`src/apps/session/`) |
//! | content of any size | the runtime refuses content past 16,384 bytes with `too_large` before it is sent, as it does in bzz |
//! | names in record fields (`holder: "bo"`) | passed on as they are. The Rota stores names; the Poll and the Approvals, which are templates, store ids (`voter`, `author`, `decided_by`), and the bench's cases write the id of a name as this host makes it |
//! | the service knows no profiles | `host.name(id)` knows the people named in `MINIAPP_PEOPLE` (`ana,bo,cy`), the local participant, and whoever it sees write; for any other id it gives the short form |
//! | a connection that closes is closed for good | every later request is answered `unavailable`; there is no resync |
//!
//! The process and its terminal are the host's, not the runtime's: raw
//! mode, the alternate screen and bracketed paste on the way in, all of it
//! undone on the way out, with status 0.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use bzz::apps::runtime::{
    AppTheme, Change, Event, Key, Options, Output, Phase, Record, RecordAnswer, RecordError,
    RecordOp, RecordPush, RecordReply, RecordRequest, Runtime, Waker, Who,
};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// How often the script's file is looked at.
const WATCH_EVERY: Duration = Duration::from_millis(50);
/// A tick, while the app has timers waiting.
const TICK: Duration = Duration::from_millis(100);

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("app_host: {error}");
            2
        }
    };
    // The reader threads sit in a read that never returns: leave them.
    std::process::exit(code);
}

enum Msg {
    Term(event::Event),
    Net(Value),
    NetEnd,
    /// The script's file changed: its new bytes.
    File(Vec<u8>),
    /// The script's thread has something new.
    Wake,
}

fn run() -> Result<(), String> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("usage: app_host <app.lua | folder with app.lua>")?;
    let mut path = PathBuf::from(&arg);
    if path.is_dir() {
        path = path.join("app.lua");
    }
    let source = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = display_name(&path);
    let log = Log::open(std::env::var_os("APP_HOST_LOG").map(PathBuf::from));
    log.write(&format!("start {name}"));
    let user = std::env::var("MINIAPP_USER").unwrap_or_else(|_| "you".into());
    let user = person(&user);
    // What bzz would know from profiles: the bench names its people.
    let mut people: HashMap<String, String> = std::env::var("MINIAPP_PEOPLE")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            let who = person(name);
            (who.id, who.name)
        })
        .collect();
    people.insert(user.id.clone(), user.name.clone());

    let (tx, rx) = mpsc::channel::<Msg>();
    let mut net = connect(&tx, &log);
    watch(path.clone(), source.clone(), tx.clone());

    setup_terminal().map_err(|e| e.to_string())?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| e.to_string())?;
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = event::read() {
                if tx.send(Msg::Term(event)).is_err() {
                    break;
                }
            }
        });
    }

    let waker: Waker = {
        let tx = tx.clone();
        Arc::new(move || {
            let _ = tx.send(Msg::Wake);
        })
    };
    let start = |source: Vec<u8>| -> Result<Runtime, String> {
        let (width, height) = terminal::size().unwrap_or((80, 24));
        Runtime::start(
            source,
            Options::new(name.clone(), user.clone())
                .size(width, height)
                .waker(waker.clone())
                .names(people.clone()),
        )
        .map_err(|e| format!("cannot start the app's thread: {e}"))
    };

    let mut host = Host {
        runtime: start(source)?,
        bridge: Bridge::new(user.clone()),
        people: people.clone(),
        theme: AppTheme::default(),
        log,
        responding: true,
        said: None,
    };
    let result = host.event_loop(&rx, &mut net, &mut terminal, &start);
    restore_terminal();
    host.log.write("exit");
    result
}

struct Host {
    runtime: Runtime,
    bridge: Bridge,
    /// Names by id, for `host.name`: what the runtime has been told.
    people: HashMap<String, String>,
    theme: AppTheme,
    log: Log,
    responding: bool,
    /// The error last written to the log, so that it is written once.
    said: Option<String>,
}

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

impl Host {
    fn event_loop(
        &mut self,
        rx: &Receiver<Msg>,
        net: &mut Option<TcpStream>,
        terminal: &mut Tui,
        start: &dyn Fn(Vec<u8>) -> Result<Runtime, String>,
    ) -> Result<(), String> {
        let mut last_tick = Instant::now();
        let mut redraw = true;
        loop {
            let wait = if self.runtime.timers() > 0 {
                TICK
            } else {
                Duration::from_millis(250)
            };
            let mut next = match rx.recv_timeout(wait) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Err("event channel closed".into()),
            };
            while let Some(msg) = next {
                if self.handle(msg, net, &mut redraw, start)? {
                    return Ok(());
                }
                next = rx.try_recv().ok();
            }
            if self.runtime.timers() > 0 && last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                self.runtime.send(Event::Tick);
            }
            for output in self.runtime.poll() {
                match output {
                    Output::Request(request) => self.ask(request, net),
                    Output::Log(line) => self.log.write(&line),
                    Output::Loaded { types } => {
                        self.log
                            .write(&format!("loaded; types: {}", types.join(", ")));
                    }
                    Output::Close => return Ok(()),
                    Output::Redraw => redraw = true,
                }
            }
            let status = self.runtime.status();
            if status.responding != self.responding {
                self.responding = status.responding;
                if !status.responding {
                    self.log.write("the app is not responding");
                }
                redraw = true;
            }
            if status.error != self.said {
                if let Some(error) = &status.error {
                    self.log.write(&format!("error: {error}"));
                }
                self.said = status.error;
            }
            if redraw {
                redraw = false;
                let (runtime, theme) = (&mut self.runtime, &self.theme);
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        if let Some(at) = runtime.draw(area, frame.buffer_mut(), theme) {
                            frame.set_cursor_position(at);
                        }
                    })
                    .map_err(|e| format!("draw failed: {e}"))?;
            }
        }
    }

    /// Returns true to leave.
    fn handle(
        &mut self,
        msg: Msg,
        net: &mut Option<TcpStream>,
        redraw: &mut bool,
        start: &dyn Fn(Vec<u8>) -> Result<Runtime, String>,
    ) -> Result<bool, String> {
        match msg {
            Msg::Term(event::Event::Key(key)) => {
                if let Some(key) = Key::from_crossterm(key) {
                    // The one key the owner of a runtime keeps.
                    if key.is_reserved() {
                        return Ok(true);
                    }
                    self.runtime.send(Event::Key(key));
                }
            }
            Msg::Term(event::Event::Paste(text)) => {
                self.runtime.send(Event::Paste(text));
            }
            Msg::Term(event::Event::Resize(width, height)) => {
                self.runtime.send(Event::Resize { width, height });
                *redraw = true;
            }
            Msg::Term(_) => {}
            Msg::Net(message) => {
                if message.get("push").is_some() {
                    if let Some(push) = self.bridge.pushed(message) {
                        self.met(&push.by);
                        self.runtime.send(Event::Push(push));
                    }
                } else if let Some((reply, pushes)) = self.bridge.reply(&message) {
                    self.runtime.send(Event::Reply(reply));
                    for push in pushes {
                        self.met(&push.by);
                        self.runtime.send(Event::Push(push));
                    }
                }
            }
            Msg::NetEnd => {
                *net = None;
                self.log.write("the data service closed the connection");
                // What was asked and will never be answered.
                for reply in self.bridge.abandon() {
                    self.runtime.send(Event::Reply(reply));
                }
            }
            Msg::File(source) => {
                let status = self.runtime.status();
                if !status.responding || status.phase == Phase::Crashed {
                    // Its thread cannot take a reload: a new one, and the
                    // state is gone.
                    self.log
                        .write("saved while the app did not respond: started again");
                    self.runtime = start(source)?;
                    self.responding = true;
                } else {
                    self.log.write("saved: reload");
                    self.runtime.send(Event::Reload(source));
                }
                *redraw = true;
            }
            Msg::Wake => {}
        }
        Ok(false)
    }

    /// Someone was seen to write: `host.name` knows them from now on.
    fn met(&mut self, who: &Who) {
        if !who.name.is_empty() && self.people.get(&who.id) != Some(&who.name) {
            self.people.insert(who.id.clone(), who.name.clone());
            self.runtime
                .send(Event::Names(vec![(who.id.clone(), who.name.clone())]));
        }
    }

    /// Sends a request of the script to the service, or answers it at once
    /// when that cannot be done.
    fn ask(&mut self, request: RecordRequest, net: &mut Option<TcpStream>) {
        let id = request.id;
        let line = match self.bridge.request(request) {
            Ok(line) => line,
            Err(error) => {
                self.runtime.send(Event::Reply(RecordReply {
                    id,
                    result: Err(error),
                }));
                return;
            }
        };
        let sent = net
            .as_mut()
            .is_some_and(|stream| stream.write_all(format!("{line}\n").as_bytes()).is_ok());
        if !sent {
            *net = None;
            self.bridge.forget(id);
            self.runtime.send(Event::Reply(RecordReply {
                id,
                result: Err(RecordError::Unavailable {
                    message: "no connection to the data service".into(),
                }),
            }));
        }
    }
}

// ------------------------------------------------- the bench's data protocol

/// The simulated service's protocol on one side, the records of `api = 1`
/// on the other. The table at the top of this file lists what it changes.
struct Bridge {
    local: Who,
    /// The service's id of a record, by the id the script is given.
    service_ids: HashMap<String, String>,
    /// The service's revision number, by the revision the script is given.
    revisions: HashMap<String, i64>,
    /// Who the host last saw write a record, by the service's id.
    writers: HashMap<String, Who>,
    /// What was asked and not answered yet.
    waiting: HashMap<u64, RecordOp>,
    /// The types the script has been given a list of: it hears of their
    /// changes from then on.
    heard: HashSet<String>,
    /// Pushes of a type whose first list is on its way, with their `seq`.
    held: HashMap<String, Vec<(i64, Value)>>,
}

fn digest(what: &str) -> String {
    Sha256::digest(what.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A participant of the simulated service, which knows only names.
fn person(name: &str) -> Who {
    Who::new(digest(&format!("participant:{name}")), name)
}

/// An id as the relay gives one: nothing in it says when it was made.
fn opaque_id(service_id: &str) -> String {
    let hex = digest(&format!("record:{service_id}"));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn opaque_rev(service_id: &str, number: i64) -> String {
    digest(&format!("rev:{service_id}:{number}"))
}

impl Bridge {
    fn new(local: Who) -> Self {
        Self {
            local,
            service_ids: HashMap::new(),
            revisions: HashMap::new(),
            writers: HashMap::new(),
            waiting: HashMap::new(),
            heard: HashSet::new(),
            held: HashMap::new(),
        }
    }

    /// Is a list of `record_type` waiting for its reply?
    fn listing(&self, record_type: &str) -> bool {
        self.waiting.values().any(
            |op| matches!(op, RecordOp::List { record_type: listed, .. } if listed == record_type),
        )
    }

    /// A push of the service, as the contract has it: of a type the script
    /// has listed, it is given now; while the first list of its type is on
    /// its way, it is held for the reply; before that, there is nothing the
    /// script holds that it could change.
    fn pushed(&mut self, json: Value) -> Option<RecordPush> {
        let record_type = json
            .get("record")
            .and_then(|r| r.get("type"))
            .and_then(Value::as_str)?
            .to_string();
        if self.heard.contains(&record_type) {
            return self.push(&json);
        }
        if self.listing(&record_type) {
            let seq = json.get("seq").and_then(Value::as_i64).unwrap_or(i64::MAX);
            self.held.entry(record_type).or_default().push((seq, json));
        }
        None
    }

    /// A record of the service as the contract's. `writer` is who is known
    /// to have just written it.
    fn record(&mut self, json: &Value, writer: Option<&Who>) -> Option<Record> {
        let service_id = json.get("id")?.as_str()?;
        let record_type = json.get("type")?.as_str()?;
        let number = json.get("rev")?.as_i64()?;
        let content = match json.get("content") {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        };
        if let Some(writer) = writer {
            self.writers.insert(service_id.to_string(), writer.clone());
        }
        let by = self
            .writers
            .get(service_id)
            .cloned()
            .unwrap_or_else(|| person(""));
        let id = opaque_id(service_id);
        let rev = opaque_rev(service_id, number);
        self.service_ids.insert(id.clone(), service_id.to_string());
        self.revisions.insert(rev.clone(), number);
        Some(Record {
            id,
            record_type: record_type.to_string(),
            rev,
            content,
            by,
        })
    }

    /// The line to send for a request of the script.
    fn request(&mut self, request: RecordRequest) -> Result<String, RecordError> {
        let id = request.id;
        // A revision the host never gave is stale by definition: 0 is the
        // revision of no record, and the service answers with the current.
        let number =
            |revisions: &HashMap<String, i64>, rev: &str| revisions.get(rev).copied().unwrap_or(0);
        let line = match &request.op {
            RecordOp::List {
                record_type,
                matching,
            } => {
                let mut line = json!({ "id": id, "op": "list", "type": record_type });
                if let Some(matching) = matching.as_deref().filter(|m| !m.is_empty()) {
                    line["match"] = json!(matching);
                }
                line
            }
            RecordOp::Create {
                record_type,
                content,
            } => json!({ "id": id, "op": "create", "type": record_type, "content": content }),
            RecordOp::Update {
                record_id,
                rev,
                content,
                ..
            } => {
                let service_id = self
                    .service_ids
                    .get(record_id)
                    .ok_or(RecordError::NotFound)?;
                json!({
                    "id": id, "op": "update", "record_id": service_id,
                    "rev": number(&self.revisions, rev), "content": content,
                })
            }
            RecordOp::Delete { record_id, rev, .. } => {
                let service_id = self
                    .service_ids
                    .get(record_id)
                    .ok_or(RecordError::NotFound)?;
                json!({
                    "id": id, "op": "delete", "record_id": service_id,
                    "rev": number(&self.revisions, rev),
                })
            }
        };
        self.waiting.insert(id, request.op);
        Ok(line.to_string())
    }

    fn forget(&mut self, id: u64) {
        self.waiting.remove(&id);
    }

    /// The connection is gone: every request that waits is `unavailable`.
    fn abandon(&mut self) -> Vec<RecordReply> {
        let mut ids: Vec<u64> = self.waiting.drain().map(|(id, _)| id).collect();
        ids.sort_unstable();
        ids.into_iter()
            .map(|id| RecordReply {
                id,
                result: Err(RecordError::Unavailable {
                    message: "the data service closed the connection".into(),
                }),
            })
            .collect()
    }

    /// A reply of the service as the contract's, and the pushes the script
    /// is to be given after it. None for what is not the answer to a
    /// request of the script (the host's own `subscribe`).
    fn reply(&mut self, json: &Value) -> Option<(RecordReply, Vec<RecordPush>)> {
        let id = json.get("id")?.as_u64()?;
        let op = self.waiting.remove(&id)?;
        let ok = json.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let mut after = Vec::new();
        if let RecordOp::List { record_type, .. } = &op {
            if ok {
                // The list holds every change up to its `seq`: what was
                // held and is newer follows it.
                let seq = json.get("seq").and_then(Value::as_i64).unwrap_or(i64::MIN);
                self.heard.insert(record_type.clone());
                let held = self.held.remove(record_type).unwrap_or_default();
                for (pushed_at, push) in held {
                    if pushed_at > seq
                        && let Some(push) = self.push(&push)
                    {
                        after.push(push);
                    }
                }
            } else if !self.listing(record_type) {
                // The next list will hold what was held.
                self.held.remove(record_type);
            }
        }
        let reply = self.answer(id, &op, ok, json);
        Some((reply, after))
    }

    fn answer(&mut self, id: u64, op: &RecordOp, ok: bool, json: &Value) -> RecordReply {
        let broken = |what: &str| RecordError::Unavailable {
            message: format!("the data service sent no {what}"),
        };
        let result = if ok {
            match op {
                RecordOp::List { .. } => {
                    let mut records: Vec<Record> = json
                        .get("records")
                        .and_then(Value::as_array)
                        .map(|list| list.iter().filter_map(|r| self.record(r, None)).collect())
                        .unwrap_or_default();
                    // The service lists in creation order. The contract
                    // promises none: by id, which says nothing.
                    records.sort_by(|a, b| a.id.cmp(&b.id));
                    Ok(RecordAnswer::Records(records))
                }
                RecordOp::Create { .. } | RecordOp::Update { .. } => {
                    let local = self.local.clone();
                    json.get("record")
                        .and_then(|r| self.record(r, Some(&local)))
                        .map(RecordAnswer::Record)
                        .ok_or_else(|| broken("record"))
                }
                RecordOp::Delete { .. } => {
                    let record = json.get("record").and_then(|r| self.record(r, None));
                    if let Some(record) = &record {
                        self.service_ids.remove(&record.id);
                    }
                    record
                        .map(RecordAnswer::Record)
                        .ok_or_else(|| broken("record"))
                }
            }
        } else {
            let error = json.get("error");
            let code = error
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("unavailable");
            let message = error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("the data service gave no reason")
                .to_string();
            Err(match code {
                "conflict" => match json.get("current").and_then(|r| self.record(r, None)) {
                    Some(current) => RecordError::Conflict {
                        current: Box::new(current),
                    },
                    None => broken("current record with its conflict"),
                },
                "not_found" => RecordError::NotFound,
                "bad_request" => RecordError::BadRequest { message },
                _ => RecordError::Unavailable { message },
            })
        };
        RecordReply { id, result }
    }

    /// A push of the service as the contract's.
    fn push(&mut self, json: &Value) -> Option<RecordPush> {
        let change = match json.get("change")?.as_str()? {
            "create" => Change::Create,
            "update" => Change::Update,
            "delete" => Change::Delete,
            _ => return None,
        };
        let by = person(json.get("by").and_then(Value::as_str).unwrap_or(""));
        let record = match change {
            // A deleted record is given in its last state: who wrote that
            // is not who deleted it.
            Change::Delete => {
                let record = self.record(json.get("record")?, None)?;
                self.service_ids.remove(&record.id);
                record
            }
            _ => self.record(json.get("record")?, Some(&by))?,
        };
        Some(RecordPush { change, record, by })
    }
}

/// Connects to the data service and subscribes before anything else, so
/// that no change falls between the subscription and the app's first list.
fn connect(tx: &Sender<Msg>, log: &Log) -> Option<TcpStream> {
    let addr = match std::env::var("MINIAPP_DATA_ADDR") {
        Ok(addr) => addr,
        Err(_) => {
            log.write("MINIAPP_DATA_ADDR is not set: every request is answered 'unavailable'");
            return None;
        }
    };
    let mut stream = match TcpStream::connect(&addr) {
        Ok(stream) => stream,
        Err(error) => {
            log.write(&format!("cannot connect to {addr}: {error}"));
            return None;
        }
    };
    let _ = stream.set_nodelay(true);
    let reader = stream.try_clone().ok()?;
    stream
        .write_all(b"{\"id\":\"host-subscribe\",\"op\":\"subscribe\"}\n")
        .ok()?;
    let tx = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && tx.send(Msg::Net(message)).is_err()
            {
                return;
            }
        }
        let _ = tx.send(Msg::NetEnd);
    });
    Some(stream)
}

// ------------------------------------------------------------ the script file

/// Looks at the script's file and sends its new bytes when they change.
/// Looking every 50 ms needs no file-watching dependency and works the
/// same on every platform, also for an editor that saves by renaming.
fn watch(path: PathBuf, mut last: Vec<u8>, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let stamp = |p: &Path| {
            std::fs::metadata(p)
                .ok()
                .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
        };
        let mut seen = stamp(&path);
        loop {
            std::thread::sleep(WATCH_EVERY);
            let now = stamp(&path);
            if now == seen {
                continue;
            }
            seen = now;
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes != last {
                last = bytes.clone();
                if tx.send(Msg::File(bytes)).is_err() {
                    return;
                }
            }
        }
    });
}

/// The script's name in error messages: its folder and its file, as
/// `board/app.lua`. An error is shown in a row or two of a region that may
/// be 40 columns wide, and Lua cuts a long name.
fn display_name(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let part = |p: Option<&std::ffi::OsStr>| p.map(|f| f.to_string_lossy().to_string());
    let file = part(absolute.file_name()).unwrap_or_else(|| "app.lua".into());
    match part(absolute.parent().and_then(Path::file_name)) {
        Some(folder) => format!("{folder}/{file}"),
        None => file,
    }
}

// ------------------------------------------------------------------- the log

/// The app's log, when `APP_HOST_LOG` names a file: its own lines and the
/// runtime's about it. Nothing is written beside the script.
struct Log {
    file: Option<std::sync::Mutex<std::fs::File>>,
    started: Instant,
}

impl Log {
    fn open(path: Option<PathBuf>) -> Self {
        let file = path
            .and_then(|p| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok()
            })
            .map(std::sync::Mutex::new);
        Self {
            file,
            started: Instant::now(),
        }
    }

    fn write(&self, line: &str) {
        if let Some(file) = &self.file {
            let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
            let _ = writeln!(
                file,
                "[{:9.3}] {line}",
                self.started.elapsed().as_secs_f64()
            );
        }
    }
}

// -------------------------------------------------------------- the terminal

fn setup_terminal() -> io::Result<()> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));
    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        cursor::Hide
    )
}

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        DisableFocusChange,
        LeaveAlternateScreen,
        cursor::Show
    );
}
