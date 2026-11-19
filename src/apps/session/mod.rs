//! An app session: one open app in one channel. It joins the two halves of
//! an app, the script ([`runtime`](super::runtime)) and its records
//! (`service::records`), and is what the app's pane owns.
//!
//! ```text
//!   owner (bzz's loop, a test)            session                  records
//!   ─────────────────────────   ─────────────────────────   ──────────────────
//!   key, paste, resize, tick ─▶  the script's thread
//!   poll()  (when woken)         requests ── fence, budget ─▶  the relay's service,
//!   draw(area, buffer, theme)    replies, pushes ◀── names ──  or a store in memory
//!   status()                     the set, the sync state
//! ```
//!
//! **No call of the owner waits** for the script or for the relay. The
//! owner gives a [`Waker`], is woken when there is something new, calls
//! [`AppSession::poll`], and draws.
//!
//! The rules of `docs/app-api.md` that are neither the script's nor the
//! relay's are here, once, for the relay and for the store in memory alike:
//!
//! - **Every request gets exactly one reply.**
//! - **The fence** (rule 17): only the types the running version declared,
//!   and only records the session holds. The service checks again.
//! - **A list is answered from the session's set**, with `match`, at once.
//!   Only an app that holds no record yet waits for the first complete set.
//! - **A change is a push; a resync is not a change.** The session's set
//!   follows what the records tell it, and the script hears of each real
//!   difference once, for the types it has listed: a push says that what
//!   a script was given has changed. The app's own writes are answered,
//!   never pushed.
//! - **Who** is `{ id, name }`: the id is the public key, the name comes
//!   from the owner's profiles ([`AppSession::set_names`]).
//! - **The app's budget of writes**: half of the person's at the relay.
//!   Past it the script gets `rate_limited` with the wait and no frame is
//!   sent, so an app cannot keep its owner from sending a message.
//! - **What the pane says by itself** ([`SessionStatus::notice`]): the wait
//!   at the quota, too many records, an archived channel, no access.

pub mod backend;
pub mod headless;
pub mod memory;
pub mod relay;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use uuid::Uuid;

pub use backend::{Listing, Records, Told, Write};
pub use memory::{MemoryStore, SharedStore};
pub use relay::RelayRecords;

use super::runtime::limits::{self, Limits};
use super::runtime::{
    AppTheme, Change as Pushed, Event, Key, Options, Output, Phase, Record as Shown, RecordAnswer,
    RecordError as ScriptError, RecordOp, RecordPush, RecordReply, RecordRequest, Runtime,
    SharedClock, SystemClock, Waker, Who, no_waker,
};
use crate::protocol::record::{
    self, Change, MAX_LIVE_RECORDS, Record, RecordError, Refusal, WriteBudget, valid_name,
};
use crate::service::records::{AppRecords, RecordsService, SharedBudget, SyncState};

/// What a session is started with.
pub struct SessionOptions {
    /// The app's name, `[a-z][a-z0-9_-]{0,31}`: with the channel it says
    /// whose records these are. One instance of an app per channel.
    pub app: String,
    /// The script's name in error messages: `board.lua`.
    pub script: String,
    /// The app's region, in columns and rows.
    pub width: u16,
    pub height: u16,
    pub limits: Limits,
    pub clock: SharedClock,
    /// Called when there is something for [`AppSession::poll`]. It must not
    /// block; it is called from the script's thread and from Tokio's.
    pub waker: Waker,
    /// The budget of writes this app shares with the owner's other apps.
    /// None: the records service's own on the relay, a new one in memory.
    pub budget: Option<SharedBudget>,
    /// The names bzz knows, by public key.
    pub names: HashMap<String, String>,
    /// The channel is archived: writes are refused before they are sent.
    pub archived: bool,
}

impl SessionOptions {
    /// For the app `app`, whose script is named `<app>.lua` in messages.
    pub fn new(app: impl Into<String>) -> Self {
        let app = app.into();
        Self {
            script: format!("{app}.lua"),
            app,
            width: 40,
            height: 16,
            limits: Limits::default(),
            clock: SystemClock::shared(),
            waker: no_waker(),
            budget: None,
            names: HashMap::new(),
            archived: false,
        }
    }

    pub fn script(mut self, name: impl Into<String>) -> Self {
        self.script = name.into();
        self
    }

    pub fn size(mut self, width: u16, height: u16) -> Self {
        (self.width, self.height) = (width, height);
        self
    }

    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn clock(mut self, clock: SharedClock) -> Self {
        self.clock = clock;
        self
    }

    pub fn waker(mut self, waker: Waker) -> Self {
        self.waker = waker;
        self
    }

    pub fn budget(mut self, budget: SharedBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn names(mut self, names: HashMap<String, String>) -> Self {
        self.names = names;
        self
    }

    pub fn archived(mut self, archived: bool) -> Self {
        self.archived = archived;
        self
    }
}

/// What the owner of a session reads to show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStatus {
    /// Where the script is in its life.
    pub phase: Phase,
    /// False when the script's thread has been inside one call for much
    /// longer than a call may take. The app can be closed and opened again.
    pub responding: bool,
    /// What is wrong with the script, in full. Its region shows the first
    /// line or two.
    pub error: Option<String>,
    /// Where the app's records stand.
    pub sync: SyncState,
    /// The same in a word or two, for a status line: `live`, `loading`.
    pub sync_words: String,
    /// What the pane says by itself, whatever the script draws: one
    /// sentence, or nothing. The wait at the quota, too many records, an
    /// archived channel, no access to the channel's records.
    pub notice: Option<String>,
    /// The seconds until the app may write again, while it may not.
    pub wait: Option<u64>,
    /// The record types the running version declares.
    pub types: Vec<String>,
}

/// What an identity is called until bzz knows its name: the ends of its
/// public key, as bzz writes an author it has no profile of.
pub fn short_name(id: &str) -> String {
    crate::domain::abbreviated_pubkey(id)
}

/// Where the records are in being opened for the running version's types.
enum Door {
    /// No version has loaded, so no types are known.
    Shut,
    Opening,
    Open,
    /// They could not be opened: every request is answered with this.
    Failed(RecordError),
}

/// A write the records have and have not answered.
struct Sent {
    deletes: bool,
}

/// A list that waits for the first complete set.
struct Waiting {
    request: u64,
    record_type: String,
    matching: Option<String>,
    since: Duration,
}

pub struct AppSession {
    runtime: Runtime,
    records: Box<dyn Records>,
    app: String,
    clock: SharedClock,
    limits: Limits,
    /// The local participant's id.
    user: String,
    names: HashMap<String, String>,
    /// The types the running version declared: the fence.
    types: Vec<String>,
    door: Door,
    sync: SyncState,
    /// The app's records as the script has been told of them, or is about
    /// to be: every push and reply in `outbox` is already in it.
    set: BTreeMap<Uuid, Record>,
    /// For the script, in order: replies and pushes. Sent a part at a time.
    outbox: VecDeque<Event>,
    /// Requests that arrived before the records were open.
    early: VecDeque<RecordRequest>,
    waiting: Vec<Waiting>,
    /// The types the script was given a list of. It hears of changes to
    /// these: before its first list of a type there is nothing of that
    /// type it holds that could have changed, and the records an app
    /// starts with would arrive as one push each, and then as the list.
    listed: Vec<String>,
    /// Replies to lists the records keep back.
    kept: Vec<RecordReply>,
    sent: HashMap<u64, Sent>,
    budget: SharedBudget,
    /// Until when no write is sent, on the clock's `elapsed`: the relay's
    /// quota or the app's own budget said to wait.
    wait_until: Option<Duration>,
    /// The seconds of the wait last shown, to say when it changed.
    wait_shown: Option<u64>,
    /// The owner said the channel is archived.
    archived: bool,
    /// The relay said so, by refusing a write.
    archived_seen: bool,
    size: (u16, u16),
    closed: bool,
}

impl AppSession {
    /// Starts the script on a thread of its own and returns at once. The
    /// records are opened when the script has loaded and said which types
    /// it uses.
    pub fn start(
        source: impl Into<Vec<u8>>,
        options: SessionOptions,
        records: Box<dyn Records>,
    ) -> std::io::Result<Self> {
        if !valid_name(&options.app) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "\"{}\" is not an app's name: a lower-case letter, then up to 31 of a-z, 0-9, _ and -",
                    options.app.chars().take(40).collect::<String>()
                ),
            ));
        }
        let user = records.identity();
        let who = Who::new(
            user.clone(),
            options
                .names
                .get(&user)
                .cloned()
                .unwrap_or_else(|| short_name(&user)),
        );
        let runtime = Runtime::start(
            source,
            Options::new(options.script, who)
                .size(options.width, options.height)
                .limits(options.limits.clone())
                .clock(options.clock.clone())
                .waker(options.waker),
        )?;
        Ok(Self {
            runtime,
            records,
            app: options.app,
            clock: options.clock,
            limits: options.limits,
            user,
            names: options.names,
            types: Vec::new(),
            door: Door::Shut,
            sync: SyncState::Loading,
            set: BTreeMap::new(),
            outbox: VecDeque::new(),
            early: VecDeque::new(),
            waiting: Vec::new(),
            listed: Vec::new(),
            kept: Vec::new(),
            sent: HashMap::new(),
            budget: options
                .budget
                .unwrap_or_else(|| Arc::new(Mutex::new(WriteBudget::default()))),
            wait_until: None,
            wait_shown: None,
            archived: options.archived,
            archived_seen: false,
            size: (options.width, options.height),
            closed: false,
        })
    }

    /// A session on the relay: the records of `options.app` in `channel`,
    /// through `service`. Must be called inside a Tokio runtime. The app's
    /// budget of writes is the service's, shared with the owner's other
    /// apps, unless the options give another.
    pub fn on_relay(
        source: impl Into<Vec<u8>>,
        mut options: SessionOptions,
        service: &RecordsService,
        channel: Uuid,
    ) -> std::io::Result<Self> {
        if options.budget.is_none() {
            options.budget = Some(service.budget());
        }
        let records = RelayRecords::new(
            service.clone(),
            channel,
            options.app.clone(),
            options.waker.clone(),
        )?;
        Self::start(source, options, Box::new(records))
    }

    /// A session on a store in memory: the same session, with nothing of a
    /// relay behind it. The names of whoever wrote in the store are known.
    pub fn in_memory(
        source: impl Into<Vec<u8>>,
        mut options: SessionOptions,
        store: &SharedStore,
    ) -> std::io::Result<Self> {
        for (id, name) in store.lock().names() {
            options
                .names
                .entry(id.clone())
                .or_insert_with(|| name.clone());
        }
        Self::start(source, options, Box::new(store.clone()))
    }

    // ------------------------------------------------------ the owner's calls

    /// Sends the script an event. Never blocks. False when it was not
    /// taken: the app has ended, or does not take its events.
    pub fn send(&mut self, event: Event) -> bool {
        self.runtime.send(event)
    }

    /// A key press. Ctrl+C is bzz's own and reaches no script.
    pub fn key(&mut self, key: Key) -> bool {
        self.send(Event::Key(key))
    }

    /// A bracketed paste, as one piece of text.
    pub fn paste(&mut self, text: impl Into<String>) -> bool {
        self.send(Event::Paste(text.into()))
    }

    /// The app's region changed size. [`AppSession::draw`] says so itself
    /// when it is given an area of another size.
    pub fn resize(&mut self, width: u16, height: u16) {
        if self.size != (width, height) {
            self.size = (width, height);
            self.send(Event::Resize { width, height });
        }
    }

    /// A new version of the script. It replaces the running one in place if
    /// it loads; if not, the running one goes on, and the status says why.
    pub fn reload(&mut self, source: impl Into<Vec<u8>>) -> bool {
        self.send(Event::Reload(source.into()))
    }

    /// Time has passed: call it at the owner's own pace (bzz: every 100 ms).
    /// Timers that are due fire, and a list that has waited too long for
    /// the first set is answered. True when what [`AppSession::status`]
    /// says has changed with the time: the seconds of a wait.
    pub fn tick(&mut self) -> bool {
        if self.runtime.timers() > 0 {
            self.send(Event::Tick);
        }
        self.serve_waiting();
        self.pump();
        let wait = self.wait();
        let changed = wait != self.wait_shown;
        self.wait_shown = wait;
        changed
    }

    /// Takes what the script and the records produced since the last time,
    /// and carries it between them. Never blocks. What it returns is for
    /// the owner: a log line, a new screen to draw, the app's wish to
    /// close. It never returns [`Output::Request`]: the session answers
    /// those.
    pub fn poll(&mut self) -> Vec<Output> {
        let mut outputs = Vec::new();
        for output in self.runtime.poll() {
            match output {
                Output::Request(request) => self.serve(request),
                Output::Loaded { types } => {
                    self.loaded(&types);
                    outputs.push(Output::Loaded { types });
                }
                Output::Close => {
                    // The script's thread has ended: its records are let
                    // go of with it.
                    self.closed = true;
                    self.records.close();
                    outputs.push(Output::Close);
                }
                other => outputs.push(other),
            }
        }
        let mut told_something = false;
        for told in self.records.poll() {
            told_something = true;
            self.told(told);
        }
        self.serve_waiting();
        self.pump();
        if told_something && !outputs.contains(&Output::Redraw) {
            // The status may say something new, whatever the script draws.
            outputs.push(Output::Redraw);
        }
        outputs
    }

    /// Draws the app's last screen into `area` of `buffer`, never outside
    /// it, and tells the script when the area is of another size than the
    /// last. Returns where a focused input wants the terminal's cursor.
    /// Call [`AppSession::poll`] first.
    pub fn draw(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) -> Option<Position> {
        if !area.is_empty() {
            self.resize(area.width, area.height);
        }
        self.runtime.draw(area, buffer, theme)
    }

    /// Lets go of the app: its thread ends after the call it is in, and
    /// its sync ends. Never blocks. Dropping the session does the same.
    pub fn close(&mut self) {
        self.closed = true;
        self.runtime.stop();
        self.records.close();
    }

    /// The app asked to close (`host.quit()`), or its owner closed it.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Asks for the records again after a state that is final: too many
    /// records (once some were deleted), or a relay that refused.
    /// Records that could not be opened at all are opened again.
    pub fn retry(&mut self) {
        if matches!(self.door, Door::Failed(_)) {
            self.door = Door::Opening;
            self.records.open(&self.types);
        } else {
            self.records.retry();
        }
    }

    /// The channel was archived, or is no longer. While it is, a write is
    /// answered `refused` without a frame, and the status says so.
    pub fn set_archived(&mut self, archived: bool) {
        self.archived = archived;
        if !archived {
            self.archived_seen = false;
        }
    }

    /// Names bzz knows, by public key: all of them, or the ones that are
    /// new. A record already handed to the script carries the name known
    /// then; when the name of its signer becomes known or changes, the
    /// record is handed over again, as a push of an update with the same
    /// revision.
    pub fn set_names(&mut self, names: impl IntoIterator<Item = (String, String)>) {
        let mut changed = Vec::new();
        for (id, name) in names {
            if self.names.get(&id) != Some(&name) {
                self.names.insert(id.clone(), name);
                changed.push(id);
            }
        }
        if changed.is_empty() {
            return;
        }
        if changed.contains(&self.user) {
            let who = self.who(&self.user);
            self.send(Event::User(who));
        }
        let again: Vec<Record> = self
            .set
            .values()
            .filter(|record| changed.contains(&record.author) && self.hears(&record.record_type))
            .cloned()
            .collect();
        for record in again {
            self.push(Pushed::Update, &record, &record.author);
        }
        self.pump();
    }

    /// Whose records the session holds without knowing their name: for the
    /// owner to look up. The local participant is among them if unnamed.
    pub fn unnamed(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::iter::once(&self.user)
            .chain(self.set.values().map(|record| &record.author))
            .filter(|id| !self.names.contains_key(*id))
            .cloned()
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn status(&self) -> SessionStatus {
        let script = self.runtime.status();
        let sync = match &self.door {
            Door::Failed(error) => SyncState::Unavailable {
                reason: error.to_string(),
            },
            _ => self.sync.clone(),
        };
        let wait = self.wait();
        let notice = match &sync {
            SyncState::Restricted => Some(
                "You cannot read this channel's records: this app cannot be used here.".to_string(),
            ),
            SyncState::Unavailable { reason } => {
                Some(format!("This app's records are not available: {reason}."))
            }
            SyncState::TooMany { max } => Some(format!(
                "This app has more than {max} records here, too many to load. None can be added."
            )),
            _ => None,
        }
        .or_else(|| {
            (self.archived || self.archived_seen).then(|| {
                "This channel is archived: its records can be read, not changed.".to_string()
            })
        })
        .or_else(|| {
            wait.map(|seconds| {
                format!("Too many changes: this app can write again in {seconds} s.")
            })
        })
        .or_else(|| {
            (self.set.len() >= MAX_LIVE_RECORDS).then(|| {
                format!(
                    "This app holds {MAX_LIVE_RECORDS} records here, the most it may. Delete some to add more."
                )
            })
        });
        SessionStatus {
            phase: script.phase,
            responding: script.responding,
            error: script.error,
            sync_words: sync_words(&sync, matches!(self.door, Door::Open)).to_string(),
            sync,
            notice,
            wait,
            types: self.types.clone(),
        }
    }

    /// The record types the running version declares.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// The records the session holds, in no order.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.set.values()
    }

    /// The relay's records of this app, for a writer that is not the owner
    /// (an agent's tools, M7). None on a store in memory, and until the
    /// records are open.
    pub fn relay(&self) -> Option<AppRecords> {
        self.records.relay()
    }

    /// The script's runtime: its phase, its timers.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// Has everything the owner sent been done, with nothing left to carry?
    /// A write the relay has not answered is not waited for.
    pub fn is_idle(&self) -> bool {
        self.runtime.is_idle() && self.outbox.is_empty()
    }

    // ----------------------------------------------------------------- inside

    fn now(&self) -> Duration {
        self.clock.elapsed()
    }

    /// The seconds until the app may write again, rounded up.
    fn wait(&self) -> Option<u64> {
        let left = self.wait_until?.checked_sub(self.now())?;
        (!left.is_zero()).then(|| (left.as_millis() as u64).div_ceil(1000).max(1))
    }

    /// No write is sent for this long: the relay or the app's own budget
    /// said so. A longer wait already known stays.
    fn wait_for(&mut self, wait: Duration) {
        let until = self.now() + wait;
        if self.wait_until.is_none_or(|known| known < until) {
            self.wait_until = Some(until);
        }
    }

    fn who(&self, id: &str) -> Who {
        Who::new(
            id,
            self.names
                .get(id)
                .cloned()
                .unwrap_or_else(|| short_name(id)),
        )
    }

    /// A record as the script is handed it.
    fn shown(&self, record: &Record) -> Shown {
        Shown {
            id: record.id.to_string(),
            record_type: record.record_type.clone(),
            rev: record.revision.clone(),
            content: record.content.clone(),
            by: self.who(&record.author),
        }
    }

    fn declares(&self, record_type: &str) -> bool {
        self.types.iter().any(|declared| declared == record_type)
    }

    /// What a failure of the records is to a script: one of the contract's
    /// eight codes. What it means for the pane is noted on the way.
    fn failure(&mut self, error: RecordError) -> ScriptError {
        match error {
            RecordError::Conflict { current } => ScriptError::Conflict {
                current: Box::new(self.shown(&current)),
            },
            RecordError::RateLimited { seconds } => ScriptError::RateLimited {
                retry_after_secs: seconds.max(1),
            },
            RecordError::Refused { refusal, reason } => {
                if refusal == Refusal::Archived {
                    self.archived_seen = true;
                }
                ScriptError::Refused { reason }
            }
            RecordError::NotFound => ScriptError::NotFound,
            RecordError::Unavailable { reason } => ScriptError::Unavailable { message: reason },
            // The fence and a wrong request are both "the request itself is
            // wrong" to a script, which cannot do anything else about them.
            RecordError::Fenced { reason } | RecordError::Invalid { reason } => {
                ScriptError::BadRequest { message: reason }
            }
            error @ RecordError::TooLarge { .. } => ScriptError::TooLarge {
                message: error.to_string(),
            },
            error @ RecordError::Limit { .. } => ScriptError::Limit {
                message: error.to_string(),
            },
        }
    }

    fn reply(&mut self, request: u64, result: Result<RecordAnswer, RecordError>) {
        let result = result.map_err(|error| self.failure(error));
        self.outbox.push_back(Event::Reply(RecordReply {
            id: request,
            result,
        }));
    }

    fn push(&mut self, change: Pushed, record: &Record, by: &str) {
        let push = RecordPush {
            change,
            record: self.shown(record),
            by: self.who(by),
        };
        self.outbox.push_back(Event::Push(push));
    }

    /// Sends the script what waits for it, as long as it keeps up. A set
    /// fetched again can be a thousand pushes: they are sent as the script
    /// takes them, so that bzz itself never fills the script's queue.
    fn pump(&mut self) {
        let room = (self.limits.event_backlog / 2).max(1) as u64;
        while !self.outbox.is_empty() && self.runtime.waiting() < room {
            if let Some(event) = self.outbox.pop_front() {
                self.runtime.send(event);
            }
        }
    }

    /// A version of the script loaded: its types are the fence, and the
    /// records are opened for them.
    fn loaded(&mut self, types: &[String]) {
        let same = self.types == types && matches!(self.door, Door::Open | Door::Opening);
        self.types = types.to_vec();
        self.listed.retain(|listed| types.contains(listed));
        if !same {
            self.door = Door::Opening;
            self.records.open(types);
        }
    }

    // --------------------------------------------------------------- requests

    /// One request of the script: exactly one reply follows, now or later.
    /// [`AppSession::poll`] calls it for each request the script made. It
    /// is public for a test that asks what the runtime lets no script ask:
    /// the reply then goes to a script that does not wait for it.
    #[doc(hidden)]
    pub fn serve(&mut self, request: RecordRequest) {
        // The fence, where the request is served (rule 17). The runtime
        // refuses these at the line that asks; the service refuses them
        // again.
        if !self.declares(request.op.record_type()) {
            let reason = format!(
                "the record type \"{}\" is not declared in app.types",
                request
                    .op
                    .record_type()
                    .chars()
                    .take(40)
                    .collect::<String>()
            );
            self.reply(request.id, Err(RecordError::Fenced { reason }));
            return;
        }
        match &self.door {
            Door::Shut | Door::Opening => {
                self.early.push_back(request);
                return;
            }
            Door::Failed(error) => {
                // Not a fault of the request: the records cannot be had.
                let reason = error.to_string();
                self.reply(request.id, Err(RecordError::Unavailable { reason }));
                return;
            }
            Door::Open => {}
        }
        match request.op {
            RecordOp::List {
                record_type,
                matching,
            } => self.list(request.id, record_type, matching),
            RecordOp::Create {
                record_type,
                content,
            } => {
                if let Err(error) = self.may_write(true) {
                    self.reply(request.id, Err(error));
                    return;
                }
                self.write(
                    request.id,
                    Write::Create {
                        record_type,
                        content,
                    },
                );
            }
            RecordOp::Update {
                record_id,
                record_type,
                rev,
                content,
            } => match self.target(&record_id, &record_type, &rev) {
                Ok(id) => self.write(
                    request.id,
                    Write::Update {
                        id,
                        revision: rev,
                        content,
                    },
                ),
                Err(error) => self.reply(request.id, Err(error)),
            },
            RecordOp::Delete {
                record_id,
                record_type,
                rev,
            } => match self.target(&record_id, &record_type, &rev) {
                Ok(id) => self.write(request.id, Write::Delete { id, revision: rev }),
                Err(error) => self.reply(request.id, Err(error)),
            },
        }
    }

    /// What is known before a write is sent, and so costs neither the
    /// relay's quota nor the app's budget.
    fn may_write(&self, creates: bool) -> Result<(), RecordError> {
        match &self.sync {
            SyncState::Restricted => {
                return Err(RecordError::Refused {
                    refusal: Refusal::Unreadable,
                    reason: "you cannot read this channel's records".into(),
                });
            }
            SyncState::Unavailable { reason } => {
                return Err(RecordError::Unavailable {
                    reason: reason.clone(),
                });
            }
            _ => {}
        }
        if self.archived {
            return Err(RecordError::Refused {
                refusal: Refusal::Archived,
                reason: "this channel is archived; its records can be read, not changed".into(),
            });
        }
        if creates
            && (matches!(self.sync, SyncState::TooMany { .. })
                || self.set.len() >= MAX_LIVE_RECORDS)
        {
            return Err(RecordError::Limit {
                limit: MAX_LIVE_RECORDS,
            });
        }
        Ok(())
    }

    /// The record a script wants to revise or delete: one the session
    /// holds, of the type the request names, at the revision it names.
    fn target(&self, record_id: &str, record_type: &str, rev: &str) -> Result<Uuid, RecordError> {
        self.may_write(false)?;
        let held = Uuid::parse_str(record_id)
            .ok()
            .and_then(|id| self.set.get(&id))
            .filter(|record| record.record_type == record_type)
            .ok_or(RecordError::NotFound)?;
        if held.revision != rev {
            return Err(RecordError::Conflict {
                current: Box::new(held.clone()),
            });
        }
        Ok(held.id)
    }

    /// Takes a write out of the app's budget and sends it, or answers with
    /// the wait: the relay's, if it asked for one, or the budget's.
    fn write(&mut self, request: u64, write: Write) {
        if let Some(seconds) = self.wait() {
            self.reply(request, Err(RecordError::RateLimited { seconds }));
            return;
        }
        let spent = self
            .budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .spend(self.now());
        if let Err(wait) = spent {
            self.wait_for(wait);
            let seconds = (wait.as_millis() as u64).div_ceil(1000).max(1);
            self.reply(request, Err(RecordError::RateLimited { seconds }));
            return;
        }
        self.sent.insert(
            request,
            Sent {
                deletes: matches!(write, Write::Delete { .. }),
            },
        );
        self.records.write(request, write);
    }

    fn list(&mut self, request: u64, record_type: String, matching: Option<String>) {
        match self
            .records
            .list(request, &record_type, matching.as_deref())
        {
            Listing::Fail(error) => self.reply(request, Err(error)),
            Listing::Hold => {
                let reply = self.listed(request, &record_type, matching.as_deref());
                self.kept.push(reply);
                self.hear(&record_type);
            }
            // Answered at the end of this look at the script and the
            // records, after whatever the records have to tell: an app
            // that holds no record yet waits for its first complete set.
            Listing::Serve => self.waiting.push(Waiting {
                request,
                record_type,
                matching,
                since: self.now(),
            }),
        }
    }

    /// The reply to a list, from the set as it is now.
    fn listed(&self, request: u64, record_type: &str, matching: Option<&str>) -> RecordReply {
        let wanted = matching.map(str::to_lowercase).filter(|m| !m.is_empty());
        let records = self
            .set
            .values()
            .filter(|held| held.record_type == record_type)
            .filter(|held| {
                wanted.as_ref().is_none_or(|wanted| {
                    record::title(&self.app, &held.record_type, &held.content)
                        .to_lowercase()
                        .contains(wanted)
                })
            })
            .map(|held| self.shown(held))
            .collect();
        RecordReply {
            id: request,
            result: Ok(RecordAnswer::Records(records)),
        }
    }

    /// Whether a list can be answered now, and with what failure if not
    /// with records. `None`: it waits.
    fn listable(&self, since: Duration) -> Option<Result<(), RecordError>> {
        match &self.door {
            Door::Failed(error) => {
                return Some(Err(RecordError::Unavailable {
                    reason: error.to_string(),
                }));
            }
            Door::Shut | Door::Opening => {}
            Door::Open => match &self.sync {
                SyncState::Restricted => {
                    return Some(Err(RecordError::Refused {
                        refusal: Refusal::Unreadable,
                        reason: "you cannot read this channel's records".into(),
                    }));
                }
                SyncState::Unavailable { reason } if self.set.is_empty() => {
                    return Some(Err(RecordError::Unavailable {
                        reason: reason.clone(),
                    }));
                }
                // No record is held and the first complete set has not
                // come: whether the app has none is not known yet.
                SyncState::Loading if self.set.is_empty() => {}
                _ => return Some(Ok(())),
            },
        }
        (self.now().saturating_sub(since) >= limits::FIRST_SET_WAIT).then(|| {
            Err(RecordError::Unavailable {
                reason: "the relay has not sent this app's records yet".into(),
            })
        })
    }

    /// Does the script hear of changes to this type? It does once it was
    /// given a list of it.
    fn hears(&self, record_type: &str) -> bool {
        self.listed.iter().any(|listed| listed == record_type)
    }

    fn hear(&mut self, record_type: &str) {
        if !self.hears(record_type) {
            self.listed.push(record_type.to_string());
        }
    }

    fn serve_waiting(&mut self) {
        if self.waiting.is_empty() {
            return;
        }
        for list in std::mem::take(&mut self.waiting) {
            match self.listable(list.since) {
                None => self.waiting.push(list),
                Some(Err(error)) => self.reply(list.request, Err(error)),
                Some(Ok(())) => {
                    let reply =
                        self.listed(list.request, &list.record_type, list.matching.as_deref());
                    self.outbox.push_back(Event::Reply(reply));
                    // From here on a change of this type is a push.
                    self.hear(&list.record_type);
                }
            }
        }
    }

    // ---------------------------------------------------------- what is told

    fn told(&mut self, told: Told) {
        match told {
            Told::Opened(Ok(())) => {
                self.door = Door::Open;
                self.sync = self.records.state();
                // What the records hold now, against what the script was
                // told under the types of the version before: only the
                // real differences, and only of types it has listed. The
                // records an app starts with are no changes.
                let fresh: BTreeMap<Uuid, Record> = self
                    .records
                    .records()
                    .into_iter()
                    .map(|held| (held.id, held))
                    .collect();
                let changes = record::difference(&self.set, &fresh);
                self.apply(changes);
                for request in std::mem::take(&mut self.early) {
                    self.serve(request);
                }
            }
            Told::Opened(Err(error)) => {
                self.door = Door::Failed(error);
                for request in std::mem::take(&mut self.early) {
                    self.serve(request);
                }
            }
            Told::State(state) => {
                // An archived channel that is no longer ends the sync, as
                // archiving did: what the relay said of it is asked again.
                if state == SyncState::Live && self.sync != SyncState::Live {
                    self.archived_seen = false;
                }
                self.sync = state;
            }
            Told::Changes { changes, cause: _ } => self.apply(changes),
            Told::Answer { request, result } => self.answered(request, result),
            Told::Release { request } => {
                if let Some(at) = self.kept.iter().position(|reply| reply.id == request) {
                    let reply = self.kept.remove(at);
                    self.outbox.push_back(Event::Reply(reply));
                }
            }
        }
    }

    /// Changes by other participants, live or found when the set was
    /// fetched again: each real difference from the session's set is one
    /// push, and what does not differ is none. How a change came about is
    /// not told to a script: a change is a change.
    fn apply(&mut self, changes: Vec<Change>) {
        for change in changes {
            match change {
                Change::Upsert(fresh) => {
                    let known = self.set.get(&fresh.id).map(|held| held.revision.clone());
                    if known.as_deref() == Some(fresh.revision.as_str()) {
                        continue;
                    }
                    let pushed = if known.is_some() {
                        Pushed::Update
                    } else {
                        Pushed::Create
                    };
                    if self.hears(&fresh.record_type) {
                        self.push(pushed, &fresh, &fresh.author);
                    }
                    self.set.insert(fresh.id, fresh);
                }
                Change::Removed { record: last, by } => {
                    let Some(held) = self.set.remove(&last.id) else {
                        continue;
                    };
                    if self.hears(&held.record_type) {
                        // Nobody says who deleted a record that is simply
                        // gone from a set fetched again: then it is whoever
                        // signed it last.
                        let by = by.unwrap_or_else(|| held.author.clone());
                        self.push(Pushed::Delete, &held, &by);
                    }
                }
            }
        }
    }

    /// The answer to a write of the script. Its reply carries the record
    /// as stored. The set follows the records' own, which may already be
    /// further: someone else's revision can be told before this answer.
    fn answered(&mut self, request: u64, result: Result<Record, RecordError>) {
        let Some(sent) = self.sent.remove(&request) else {
            return;
        };
        match result {
            Ok(carried) => {
                // The relay took a write: the channel is not archived.
                self.archived_seen = false;
                let shown = self.shown(&carried);
                self.reply(request, Ok(RecordAnswer::Record(shown)));
                self.settle(
                    &carried,
                    if sent.deletes {
                        Carried::Deleted
                    } else {
                        Carried::Stored
                    },
                );
            }
            Err(RecordError::Conflict { current }) => {
                let carried = (*current).clone();
                self.reply(request, Err(RecordError::Conflict { current }));
                self.settle(&carried, Carried::Current);
            }
            Err(error) => {
                // The relay's own quota: nothing is sent until it has passed.
                if let RecordError::RateLimited { seconds } = &error {
                    self.wait_for(Duration::from_secs((*seconds).max(1)));
                }
                self.reply(request, Err(error));
            }
        }
    }

    /// After a reply that carried a record. The app's own write is not
    /// told as a change, so the set takes it here, if it is what the
    /// records hold. If the records hold something else that the script
    /// was already told (a later change arrived before this answer), the
    /// script is told it again, after the reply, so that it does not end
    /// on what the reply carried.
    fn settle(&mut self, carried: &Record, what: Carried) {
        let truth = self.records.record(carried.id);
        let mine = self.set.get(&carried.id);
        let told = |held: &Record| mine.is_some_and(|mine| mine.revision == held.revision);
        match (what, truth) {
            (Carried::Deleted, None) => {
                self.set.remove(&carried.id);
            }
            // Deleted, and made again by someone since.
            (Carried::Deleted, Some(again)) if told(&again) => {
                self.push(Pushed::Update, &again, &again.author);
            }
            (Carried::Stored, Some(held)) if held.revision == carried.revision => {
                self.set.insert(carried.id, carried.clone());
            }
            // The winner of a conflict is told as the change it is.
            (Carried::Current, Some(held)) if held.revision == carried.revision => {}
            (Carried::Stored | Carried::Current, Some(later)) if told(&later) => {
                self.push(Pushed::Update, &later, &later.author);
            }
            (Carried::Stored | Carried::Current, None) if mine.is_none() => {
                self.push(Pushed::Delete, carried, &carried.author);
            }
            // The session is behind the records here: the change that
            // makes the difference is on its way.
            _ => {}
        }
    }
}

/// What the record in a reply is.
#[derive(Clone, Copy)]
enum Carried {
    /// The app's own create or update, as stored.
    Stored,
    /// The last state of the record the app deleted.
    Deleted,
    /// The record as it is now, in a conflict.
    Current,
}

/// The sync state in a word or two.
fn sync_words(state: &SyncState, open: bool) -> &'static str {
    match state {
        _ if !open && !matches!(state, SyncState::Unavailable { .. }) => "opening",
        SyncState::Loading => "loading",
        SyncState::Live => "live",
        SyncState::Resyncing => "syncing again",
        SyncState::TooMany { .. } => "too many records",
        SyncState::Restricted => "no access",
        SyncState::Unavailable { .. } => "unavailable",
    }
}

impl Drop for AppSession {
    fn drop(&mut self) {
        self.runtime.stop();
    }
}
