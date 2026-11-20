//! The script thread. It owns the interpreter, turns the owner's events
//! into calls of the app's handlers, and reports what came of them. The
//! owner never waits for it.
//!
//! One round: take every event that is waiting, call the handler of each,
//! then call `app.view` once and report the screen. Every call has its own
//! time budget.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mlua::{IntoLuaMulti, Table, Value};

use super::api::{self, Gate, SharedGate};
use super::clock::SharedClock;
use super::limits::Limits;
use super::records::{Change, RecordAnswer, RecordPush, RecordReply, Who};
use super::ui::{self, Node};
use super::value::{self, Copier, CopyFailed};
use super::vm::{Failure, Vm};
use super::{Event, Key, Output, Phase, Waker};

/// The handlers a script may define, besides `view`, which it must.
pub const HANDLERS: &[&str] = &[
    "init", "key", "paste", "reply", "push", "resize", "timer", "reload",
];

/// The number of the contract this runtime serves (rule 18).
pub const API_VERSION: i64 = 1;

/// What the handle and the worker both see, without a lock.
pub struct Shared {
    /// Events the handle has sent.
    pub sent: AtomicU64,
    /// Events the worker has finished, their screen reported.
    pub handled: AtomicU64,
    /// When the running call began, in milliseconds since `epoch` plus one;
    /// zero while no script code runs.
    pub call_started: AtomicU64,
    pub epoch: Instant,
    /// The owner let go of the runtime.
    pub stop: AtomicBool,
    /// Timers that wait to fire: an owner need not tick an app without.
    pub timers: AtomicUsize,
    /// Ids `host.name` had no name for, for the owner to look up.
    pub asked: Mutex<Vec<String>>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            // Starting counts as the first event.
            sent: AtomicU64::new(1),
            handled: AtomicU64::new(0),
            call_started: AtomicU64::new(0),
            epoch: Instant::now(),
            stop: AtomicBool::new(false),
            timers: AtomicUsize::new(0),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn begin(&self) {
        let now = self.epoch.elapsed().as_millis() as u64 + 1;
        self.call_started.store(now, Ordering::SeqCst);
    }

    fn end(&self) {
        self.call_started.store(0, Ordering::SeqCst);
    }
}

/// What the worker tells the handle.
pub enum Report {
    /// After a round. `screen: None` keeps the screen that was there.
    Frame {
        screen: Option<Box<Node>>,
        error: Option<String>,
        phase: Phase,
    },
    Out(Output),
}

pub struct Setup {
    pub name: String,
    pub user: Who,
    pub size: (u16, u16),
    pub limits: Limits,
    pub clock: SharedClock,
    pub waker: Waker,
    pub source: Vec<u8>,
    /// The names the owner knows, by id, for `host.name`.
    pub names: HashMap<String, String>,
}

/// One version of the script, loaded and checked.
struct Loaded {
    vm: Vm,
    app: Table,
    ctx: Table,
    user: Table,
    state: Value,
    types: Vec<String>,
}

struct Worker {
    name: String,
    user: Who,
    size: (u16, u16),
    limits: Limits,
    gate: SharedGate,
    loaded: Option<Loaded>,
    phase: Phase,
    /// What the app's region says is wrong, and whether it stays until a
    /// reload (a failed load) or only until the next key or paste.
    error: Option<(String, bool)>,
    /// Stops in a row, by handler.
    stops: HashMap<&'static str, u32>,
    /// A handler ran in this round: the screen must be asked for again.
    dirty: bool,
    /// Events taken whose round is not yet reported as handled, because a
    /// reply the runtime gives by itself still follows from them.
    owed: u64,
    out: Sender<Report>,
    shared: Arc<Shared>,
    waker: Waker,
}

pub fn run(setup: Setup, events: Receiver<Event>, out: Sender<Report>, shared: Arc<Shared>) {
    let waker = setup.waker.clone();
    let crashed = out.clone();
    let done = shared.clone();
    // A panic here would be a fault of bzz, never something a script may
    // cause. It must still not take the owner's screen with it unseen.
    let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let Setup {
            name,
            user,
            size,
            limits,
            clock,
            waker,
            source,
            names,
        } = setup;
        let gate = Rc::new(RefCell::new(Gate::new(limits.clone(), clock)));
        gate.borrow_mut().names = names;
        let mut worker = Worker {
            name,
            user,
            size,
            limits,
            gate,
            loaded: None,
            phase: Phase::Starting,
            error: None,
            stops: HashMap::new(),
            dirty: true,
            owed: 0,
            out,
            shared,
            waker,
        };
        worker.serve(&source, &events);
    }));
    if served.is_err() {
        let _ = crashed.send(Report::Frame {
            screen: None,
            error: Some("the app's runtime failed: a fault in bzz, not in the script".into()),
            phase: Phase::Crashed,
        });
    }
    done.end();
    // Whatever was sent and not handled never will be.
    done.handled
        .store(done.sent.load(Ordering::SeqCst), Ordering::SeqCst);
    waker();
}

impl Worker {
    fn serve(&mut self, source: &[u8], events: &Receiver<Event>) {
        // Loading the script is the first thing the owner asked for.
        self.start(source);
        self.finish_round(1);
        while !self.closing() {
            // A reply the runtime gives by itself comes before the owner's
            // next event, and is not one the owner counted.
            let own = self.gate.borrow_mut().local_replies.pop_front();
            let mut counted = 0;
            match own {
                Some(reply) => self.handle(Event::Reply(reply)),
                None => match events.recv() {
                    Ok(event) => {
                        counted += 1;
                        self.handle(event);
                    }
                    Err(_) => return,
                },
            }
            while !self.closing() {
                match events.try_recv() {
                    Ok(event) => {
                        counted += 1;
                        self.handle(event);
                    }
                    Err(_) => break,
                }
            }
            self.finish_round(counted);
        }
    }

    fn closing(&self) -> bool {
        self.phase == Phase::Closed || self.shared.stop.load(Ordering::SeqCst)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Stop => self.shared.stop.store(true, Ordering::SeqCst),
            Event::Reload(source) => self.reload(&source),
            Event::User(who) => {
                self.user = who;
                self.dirty = true;
            }
            Event::Names(names) => {
                // A name the app was given a short form for: its screen
                // is asked for again, so that it shows the name.
                if self.gate.borrow_mut().learn_names(names) {
                    self.dirty = true;
                }
            }
            Event::Resize { width, height } => {
                self.size = (width, height);
                self.dirty = true;
                self.call("resize", |l| Ok((l.state.clone(), l.ctx.clone())));
            }
            _ if self.phase != Phase::Running => {}
            Event::Key(key) => {
                self.seen();
                self.key(key);
            }
            Event::Paste(text) => {
                self.seen();
                if text.len() > self.limits.paste_bytes {
                    self.fail(
                        format!(
                            "a paste of {} bytes was not given to the app: more than {}",
                            text.len(),
                            self.limits.paste_bytes
                        ),
                        false,
                    );
                    return;
                }
                self.call("paste", |l| {
                    Ok((
                        l.state.clone(),
                        l.vm.lua().create_string(&text)?,
                        l.ctx.clone(),
                    ))
                });
            }
            Event::Tick => self.tick(),
            Event::Reply(reply) => self.reply(reply),
            Event::Push(push) => self.push(push),
        }
    }

    // ---------------------------------------------------------------- output

    fn send(&self, output: Output) {
        let _ = self.out.send(Report::Out(output));
    }

    /// Hands what the last call asked for to the owner.
    fn flush(&mut self) {
        let (logs, requests, timers, asked) = {
            let mut gate = self.gate.borrow_mut();
            (
                std::mem::take(&mut gate.logs),
                std::mem::take(&mut gate.requests),
                gate.timers.len(),
                std::mem::take(&mut gate.asked),
            )
        };
        self.shared.timers.store(timers, Ordering::SeqCst);
        let any = !logs.is_empty() || !requests.is_empty() || !asked.is_empty();
        if !asked.is_empty() {
            self.shared
                .asked
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend(asked);
        }
        for line in logs {
            self.send(Output::Log(line));
        }
        for request in requests {
            self.send(Output::Request(request));
        }
        if any {
            (self.waker)();
        }
    }

    /// The runtime's own line about the app, in the log.
    fn note(&mut self, line: &str) {
        self.gate.borrow_mut().log(line, false);
    }

    /// The person at the app did something: an error of an earlier call
    /// has been on the screen and goes. One that says the app's version is
    /// not the one that was asked for stays.
    fn seen(&mut self) {
        if matches!(self.error, Some((_, false))) {
            self.error = None;
            self.dirty = true;
        }
    }

    fn fail(&mut self, message: String, sticky: bool) {
        self.note(&format!("error: {message}"));
        self.dirty = true;
        if !matches!(self.error, Some((_, true))) || sticky {
            self.error = Some((message, sticky));
        }
    }

    fn finish_round(&mut self, counted: u64) {
        let screen = if self.dirty { self.view() } else { None };
        if self.gate.borrow().quit && self.phase == Phase::Running {
            self.phase = Phase::Closed;
            self.flush();
            self.send(Output::Close);
        }
        self.flush();
        if self.dirty {
            let _ = self.out.send(Report::Frame {
                screen,
                error: self.error.as_ref().map(|(message, _)| message.clone()),
                phase: self.phase,
            });
        }
        self.dirty = false;
        self.owed += counted;
        if self.gate.borrow().local_replies.is_empty() || self.closing() {
            let owed = std::mem::take(&mut self.owed);
            self.shared.handled.fetch_add(owed, Ordering::SeqCst);
        }
        (self.waker)();
    }

    // --------------------------------------------------------------- loading

    fn start(&mut self, source: &[u8]) {
        match self.load(source) {
            Ok(loaded) => self.adopt(loaded, None),
            Err(message) => {
                self.phase = Phase::NotLoaded;
                self.fail(message, true);
            }
        }
    }

    /// Loads one version: compiles it, runs its top level, and checks what
    /// it returned against the contract. Nothing of the running version is
    /// touched.
    fn load(&mut self, source: &[u8]) -> Result<Loaded, String> {
        let name = self.name.clone();
        if source.len() > self.limits.script_bytes {
            return Err(format!(
                "{name}: the script is {} bytes; an app's script may be {}",
                source.len(),
                self.limits.script_bytes
            ));
        }
        let vm = Vm::new(self.limits.memory, self.limits.call_time)?;
        api::install(vm.lua(), &self.gate)
            .map_err(|e| format!("cannot set up the app API: {e}"))?;
        let chunk = vm.compile(&name, source).map_err(|f| f.to_string())?;
        // While the top level runs, the script can only compute: a version
        // that is then refused must not have asked for anything.
        self.gate.borrow_mut().loading = true;
        self.shared.begin();
        let returned = vm.call(&chunk, ());
        self.shared.end();
        self.gate.borrow_mut().loading = false;
        self.flush();
        let app = match returned {
            Ok(mut values) => values.pop_front().unwrap_or(Value::Nil),
            Err(Failure::Stopped { at }) => {
                return Err(format!(
                    "{}the script ran longer than {} ms while it was loaded, and was stopped",
                    at.map(|at| format!("{at}: ")).unwrap_or_default(),
                    self.limits.call_time.as_millis()
                ));
            }
            Err(Failure::Memory) => {
                return Err(format!(
                    "{name}: the script ran out of memory while it was loaded (an app may hold {} MiB)",
                    self.limits.memory >> 20
                ));
            }
            Err(Failure::Script(message)) => return Err(message),
        };
        let Value::Table(app) = app else {
            return Err(format!(
                "{name}: the script must end with `return app` (a table of handlers); it returned {}",
                app.type_name()
            ));
        };
        // Rule 18: the contract has a number.
        match app.raw_get::<Value>("api") {
            Ok(Value::Integer(API_VERSION)) => {}
            Ok(Value::Number(n)) if n == API_VERSION as f64 => {}
            Ok(Value::Nil) => {
                return Err(format!(
                    "{name}: app.api is missing. This bzz knows api {API_VERSION}: write `app.api = {API_VERSION}`"
                ));
            }
            Ok(Value::Integer(n)) => {
                return Err(format!(
                    "{name}: the script is written for api {n}, and this bzz knows only api {API_VERSION}"
                ));
            }
            Ok(Value::Number(n)) => {
                return Err(format!(
                    "{name}: the script is written for api {n}, and this bzz knows only api {API_VERSION}"
                ));
            }
            Ok(other) => {
                return Err(format!(
                    "{name}: app.api must be the number {API_VERSION}, the only api this bzz knows; it is a {}",
                    other.type_name()
                ));
            }
            Err(e) => return Err(e.to_string()),
        }
        // The fence: the record types the app may touch.
        let types = match app.raw_get::<Value>("types") {
            Ok(Value::Table(t)) => {
                let n = t.raw_len();
                if n > self.limits.types {
                    return Err(format!(
                        "{name}: app.types has {n} types; an app may declare {}",
                        self.limits.types
                    ));
                }
                let mut types = Vec::with_capacity(n);
                for i in 1..=n {
                    match t.raw_get::<Value>(i) {
                        Ok(Value::String(s)) => types.push(s.to_string_lossy()),
                        _ => {
                            return Err(format!(
                                "{name}: app.types[{i}] must be a string (a record type)"
                            ));
                        }
                    }
                }
                types
            }
            _ => {
                return Err(format!(
                    "{name}: app.types is missing; declare the record types the app uses, e.g. app.types = {{ \"note\" }}"
                ));
            }
        };
        api::check_types(&types, &self.limits).map_err(|e| format!("{name}: {e}"))?;
        match app.raw_get::<Value>("view") {
            Ok(Value::Function(_)) => {}
            _ => {
                return Err(format!(
                    "{name}: app.view is missing; it must be a function(state, ctx) that returns ui elements"
                ));
            }
        }
        for handler in HANDLERS {
            match app.raw_get::<Value>(*handler) {
                Ok(Value::Nil | Value::Function(_)) => {}
                Ok(other) => {
                    return Err(format!(
                        "{name}: app.{handler} must be a function, got {}",
                        other.type_name()
                    ));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        let made = |e: mlua::Error| format!("{name}: {e}");
        let ctx = vm.lua().create_table().map_err(made)?;
        let user = vm.lua().create_table().map_err(made)?;
        Ok(Loaded {
            vm,
            app,
            ctx,
            user,
            state: Value::Nil,
            types,
        })
    }

    /// Makes `loaded` the running version. With `state`, the app goes on
    /// from it (a reload); without, it starts with `init`.
    fn adopt(&mut self, mut loaded: Loaded, state: Option<Value>) {
        {
            let mut gate = self.gate.borrow_mut();
            gate.types = loaded.types.clone();
            let types = gate.types.clone();
            gate.known.retain(|_, t| types.contains(t));
            if state.is_none() {
                // A fresh start owes nothing to what ran before.
                gate.pending.clear();
                gate.local_replies.clear();
                gate.timers.clear();
                gate.known.clear();
                gate.quit = false;
            }
        }
        self.error = None;
        self.stops.clear();
        self.phase = Phase::Running;
        self.dirty = true;
        self.send(Output::Loaded {
            types: loaded.types.clone(),
        });
        match state {
            Some(state) => {
                loaded.state = state;
                self.loaded = Some(loaded);
                let migrated = self.call("reload", |l| Ok((l.state.clone(), l.ctx.clone())));
                if let (Some(state @ Value::Table(_)), Some(loaded)) = (migrated, &mut self.loaded)
                {
                    loaded.state = state;
                }
            }
            None => {
                loaded.state = match loaded.vm.lua().create_table() {
                    Ok(t) => Value::Table(t),
                    Err(_) => Value::Nil,
                };
                self.loaded = Some(loaded);
                let made = self.call("init", |l| Ok(l.ctx.clone()));
                if let (Some(state @ Value::Table(_)), Some(loaded)) = (made, &mut self.loaded) {
                    loaded.state = state;
                }
            }
        }
    }

    fn reload(&mut self, source: &[u8]) {
        let running = self.phase == Phase::Running && self.loaded.is_some();
        let new = match self.load(source) {
            Ok(new) => new,
            Err(message) if running => {
                // The old version keeps running; the error stays until a
                // version loads.
                self.fail(
                    format!("{message}\n(reload failed: the previous version keeps running)"),
                    true,
                );
                return;
            }
            Err(message) => {
                self.phase = Phase::NotLoaded;
                self.error = None;
                self.fail(message, true);
                return;
            }
        };
        let old = if running { self.loaded.take() } else { None };
        self.loaded = None;
        let Some(old) = old else {
            self.note("loaded; the app starts with init");
            self.adopt(new, None);
            return;
        };
        // The state moves into the new interpreter: plain data only.
        self.shared.begin();
        let deadline = Instant::now() + self.limits.call_time * 4;
        let mut copier = Copier::new(new.vm.lua(), deadline);
        let copied = copier.copy(&old.state, "state", 0);
        let dropped = std::mem::take(&mut copier.dropped);
        drop(copier);
        drop(old);
        self.shared.end();
        match copied {
            Ok(state @ Value::Table(_)) => {
                if !dropped.is_empty() {
                    self.note(&format!(
                        "reload: dropped values that cannot move to the new version: {}",
                        dropped.join(", ")
                    ));
                }
                self.note("reloaded; state kept");
                self.adopt(new, Some(state));
            }
            other => {
                let why = match other {
                    Err(CopyFailed::TookTooLong) => "it took too long to copy",
                    Err(CopyFailed::TooDeep) => "it is nested too deep",
                    Err(CopyFailed::NoRoom) => "it did not fit in the new version's memory",
                    Ok(_) => "it was not a table",
                };
                self.note(&format!(
                    "reload: the state was not kept ({why}); the app starts with init"
                ));
                new.vm.collect();
                self.adopt(new, None);
            }
        }
    }

    // ----------------------------------------------------------------- calls

    /// Calls `app.<name>` if the app defines it, and returns its first
    /// result. The one place a handler is called from.
    fn call<A: IntoLuaMulti>(
        &mut self,
        name: &'static str,
        args: impl FnOnce(&Loaded) -> mlua::Result<A>,
    ) -> Option<Value> {
        if self.phase != Phase::Running {
            return None;
        }
        let loaded = self.loaded.as_ref()?;
        let handler = match loaded.app.raw_get::<Value>(name) {
            Ok(Value::Function(f)) => f,
            _ => return None,
        };
        self.dirty = true;
        let prepared = (|| {
            loaded.ctx.raw_set("width", self.size.0)?;
            loaded.ctx.raw_set("height", self.size.1)?;
            loaded.user.raw_set("id", self.user.id.as_str())?;
            loaded.user.raw_set("name", self.user.name.as_str())?;
            loaded.ctx.raw_set("user", loaded.user.clone())?;
            args(loaded)
        })();
        let result = match prepared {
            Ok(args) => {
                self.shared.begin();
                let result = loaded.vm.call(&handler, args);
                self.shared.end();
                result
            }
            Err(_) => Err(Failure::Memory),
        };
        self.flush();
        match result {
            Ok(mut values) => {
                self.stops.remove(name);
                Some(values.pop_front().unwrap_or(Value::Nil))
            }
            Err(failure) => {
                self.failed(name, failure);
                None
            }
        }
    }

    fn failed(&mut self, name: &'static str, failure: Failure) {
        if let Some(loaded) = &self.loaded {
            // What the failed call left behind, its abandoned coroutine
            // included, is garbage.
            loaded.vm.collect();
        }
        match failure {
            Failure::Script(message) => {
                self.stops.remove(name);
                self.fail(format!("in app.{name}: {message}"), false);
            }
            Failure::Memory => {
                self.stops.remove(name);
                self.fail(
                    format!(
                        "app.{name} ran out of memory: an app may hold {} MiB",
                        self.limits.memory >> 20
                    ),
                    false,
                );
            }
            Failure::Stopped { at } => {
                let at = at.map(|at| format!("{at}: ")).unwrap_or_default();
                let budget = self.limits.call_time.as_millis();
                let stops = self.stops.entry(name).or_insert(0);
                *stops += 1;
                if *stops >= self.limits.stops_before_giving_up {
                    let stops = *stops;
                    self.give_up(format!(
                        "{at}app.{name} ran longer than {budget} ms {stops} times in a row. The app is stopped until it is reloaded"
                    ));
                } else {
                    self.fail(
                        format!(
                            "{at}app.{name} ran longer than {budget} ms and was stopped (an endless loop?)"
                        ),
                        false,
                    );
                }
            }
        }
    }

    /// The app is not called any more until a version is loaded again. Its
    /// interpreter is dropped, and with it the state.
    fn give_up(&mut self, why: String) {
        self.phase = Phase::GivenUp;
        self.loaded = None;
        {
            let mut gate = self.gate.borrow_mut();
            gate.pending.clear();
            gate.local_replies.clear();
            gate.timers.clear();
            gate.known.clear();
        }
        self.error = None;
        self.fail(why, true);
    }

    /// Asks the app for its screen.
    fn view(&mut self) -> Option<Box<Node>> {
        let returned = self.call("view", |l| Ok((l.state.clone(), l.ctx.clone())))?;
        // Reading the tree is bzz's own work on the script's tables: it
        // counts as running too, for whoever watches.
        self.shared.begin();
        let parsed = ui::parse(&returned, &self.limits);
        self.shared.end();
        match parsed {
            Ok(node) => Some(Box::new(node)),
            Err(message) => {
                self.fail(format!("app.view returned a bad screen: {message}"), false);
                None
            }
        }
    }

    fn key(&mut self, key: Key) {
        // Ctrl+C is bzz's own: it leaves the app. No script ever sees it.
        if key.is_reserved() {
            return;
        }
        self.call("key", |l| {
            let t = l.vm.lua().create_table_with_capacity(0, 5)?;
            t.raw_set("name", key.name.as_str())?;
            t.raw_set("text", key.text.as_deref())?;
            t.raw_set("ctrl", key.ctrl)?;
            t.raw_set("alt", key.alt)?;
            t.raw_set("shift", key.shift)?;
            Ok((l.state.clone(), t, l.ctx.clone()))
        });
    }

    /// Fires the timers that are due, in the order they are due.
    fn tick(&mut self) {
        let due = {
            let mut gate = self.gate.borrow_mut();
            let now = gate.clock.elapsed();
            let (mut due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut gate.timers)
                .into_iter()
                .partition(|t| t.due <= now);
            gate.timers = later;
            due.sort_by_key(|t| (t.due, t.order));
            due
        };
        for timer in due {
            self.call("timer", |l| {
                let tag = value::to_lua(l.vm.lua(), &timer.tag)?;
                Ok((l.state.clone(), tag, l.ctx.clone()))
            });
        }
        self.flush();
    }

    // ---------------------------------------------------------------- records

    fn reply(&mut self, reply: RecordReply) {
        let Some(pending) = self.gate.borrow_mut().pending.remove(&reply.id) else {
            return;
        };
        {
            let mut gate = self.gate.borrow_mut();
            match &reply.result {
                Ok(RecordAnswer::Records(records)) => {
                    for record in records {
                        gate.learn(&record.id, &record.record_type);
                    }
                }
                Ok(RecordAnswer::Record(record)) if pending.op == "delete" => {
                    gate.forget(&record.id);
                }
                Ok(RecordAnswer::Record(record)) => {
                    gate.learn(&record.id, &record.record_type);
                }
                Err(super::records::RecordError::Conflict { current }) => {
                    gate.learn(&current.id, &current.record_type);
                }
                Err(_) => {}
            }
        }
        let declared = self.gate.borrow().types.clone();
        self.call("reply", |l| {
            let lua = l.vm.lua();
            let t = lua.create_table_with_capacity(0, 8)?;
            t.raw_set("id", reply.id)?;
            t.raw_set("op", pending.op)?;
            t.raw_set("tag", value::to_lua(lua, &pending.tag)?)?;
            t.raw_set("request", value::to_lua(lua, &pending.request)?)?;
            match &reply.result {
                Ok(answer) => {
                    t.raw_set("ok", true)?;
                    match answer {
                        RecordAnswer::Records(records) => {
                            let list = lua.create_table_with_capacity(records.len(), 0)?;
                            let mut n = 0;
                            // The fence, once more: only declared types.
                            for record in records {
                                if declared.contains(&record.record_type) {
                                    n += 1;
                                    list.raw_set(n, value::record_to_lua(lua, record)?)?;
                                }
                            }
                            t.raw_set("records", list)?;
                        }
                        RecordAnswer::Record(record) => {
                            t.raw_set("record", value::record_to_lua(lua, record)?)?;
                        }
                    }
                }
                Err(error) => {
                    t.raw_set("ok", false)?;
                    let e = lua.create_table_with_capacity(0, 3)?;
                    e.raw_set("code", error.code())?;
                    e.raw_set("message", error.message())?;
                    match error {
                        super::records::RecordError::RateLimited { retry_after_secs } => {
                            e.raw_set("retry_after", (*retry_after_secs).max(1))?;
                        }
                        super::records::RecordError::Conflict { current } => {
                            t.raw_set("current", value::record_to_lua(lua, current)?)?;
                        }
                        _ => {}
                    }
                    t.raw_set("error", e)?;
                }
            }
            Ok((l.state.clone(), t, l.ctx.clone()))
        });
    }

    fn push(&mut self, push: RecordPush) {
        {
            let mut gate = self.gate.borrow_mut();
            if !gate.declares(&push.record.record_type) {
                return; // the fence: other types never reach the script
            }
            match push.change {
                Change::Delete => gate.forget(&push.record.id),
                _ => gate.learn(&push.record.id, &push.record.record_type),
            }
        }
        self.call("push", |l| {
            let lua = l.vm.lua();
            let t = lua.create_table_with_capacity(0, 3)?;
            t.raw_set("change", push.change.name())?;
            t.raw_set("record", value::record_to_lua(lua, &push.record)?)?;
            t.raw_set("by", value::who_to_lua(lua, &push.by)?)?;
            Ok((l.state.clone(), t, l.ctx.clone()))
        });
    }
}
