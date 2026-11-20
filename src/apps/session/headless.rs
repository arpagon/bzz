//! An app without a window: load a script on the record store in memory,
//! feed it keys and pastes, and read the screen it draws as text, with any
//! error. `bzz app try` and the agents' "try" tool are this.
//!
//! Between the script and the store stands an [`AppSession`], the one
//! bzz's window uses on the relay: a list is answered, a write is counted
//! against the app's budget and a change becomes a push by the same code.
//!
//! Time does not pass by itself here. The clock is a [`ManualClock`];
//! [`Headless::advance`] moves it and fires the timers that fall due, so a
//! run of the same steps gives the same screen every time.

use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

use super::backend::Told;
use super::memory::{MemoryStore, SharedStore};
use super::{AppSession, SessionOptions, SessionStatus};
use crate::apps::runtime::clock::ManualClock;
use crate::apps::runtime::draw::text_of;
use crate::apps::runtime::limits::Limits;
use crate::apps::runtime::records::Who;
use crate::apps::runtime::ui::AppTheme;
use crate::apps::runtime::{Event, Key, Output, Phase, Runtime, Signal, Status};
use crate::protocol::record::valid_name;

/// The clock a headless app starts at: 2026-11-18 00:00:00 UTC.
pub const START_OF_TIME: u64 = 1_794_960_000;

pub struct HeadlessOptions {
    /// The script's name in error messages.
    pub name: String,
    /// The app's name, which with a channel says whose records these are.
    /// None: the script's name without `.lua` if that is an app's name,
    /// else `app`.
    pub app: Option<String>,
    pub user: Who,
    pub width: u16,
    pub height: u16,
    pub limits: Limits,
    pub theme: AppTheme,
}

impl HeadlessOptions {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            app: None,
            user: Who::new(
                "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                "ana",
            ),
            width: 40,
            height: 16,
            limits: Limits::default(),
            theme: AppTheme::default(),
        }
    }

    pub fn size(mut self, width: u16, height: u16) -> Self {
        (self.width, self.height) = (width, height);
        self
    }

    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn user(mut self, user: Who) -> Self {
        self.user = user;
        self
    }

    pub fn app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// The name the app's records go by.
    fn app_name(&self) -> String {
        self.app.clone().unwrap_or_else(|| {
            let stem = self.name.strip_suffix(".lua").unwrap_or(&self.name);
            if valid_name(stem) {
                stem.to_string()
            } else {
                "app".to_string()
            }
        })
    }
}

/// One thing done to an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(Key),
    /// Text typed one key at a time.
    Type(String),
    /// Text pasted as one piece.
    Paste(String),
    Resize(u16, u16),
    /// Time passes: this many milliseconds.
    Advance(u64),
}

impl Step {
    /// A step as a person writes it: a key (`j`, `enter`, `ctrl+u`),
    /// `type:some text`, `paste:some text`, `resize:40x16`, `wait:500`.
    pub fn parse(written: &str) -> Result<Self, String> {
        if let Some(text) = written.strip_prefix("type:") {
            return Ok(Step::Type(text.to_string()));
        }
        if let Some(text) = written.strip_prefix("paste:") {
            return Ok(Step::Paste(text.to_string()));
        }
        if let Some(size) = written.strip_prefix("resize:") {
            let parsed = size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
            return match parsed {
                Some((w, h)) => Ok(Step::Resize(w, h)),
                None => Err(format!("{written}: a size is written 40x16")),
            };
        }
        if let Some(ms) = written.strip_prefix("wait:") {
            return ms
                .parse()
                .map(Step::Advance)
                .map_err(|_| format!("{written}: the wait is in milliseconds"));
        }
        Key::parse(written).map(Step::Key).ok_or_else(|| {
            format!("{written}: not a key (j, enter, esc, ctrl+u, shift+tab, f1...)")
        })
    }
}

/// What a script drew after the steps it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tried {
    /// The screen as text, one line for each row.
    pub screen: String,
    /// What is wrong with the app, if anything: with file and line.
    pub error: Option<String>,
    /// The app's log: its own lines and the runtime's about it.
    pub logs: Vec<String>,
    /// The record types the script declares; empty if it did not load.
    pub types: Vec<String>,
    /// The app asked to close.
    pub closed: bool,
    /// False when the script was still inside a call when it was given up
    /// on.
    pub responding: bool,
    /// What the app's pane would say by itself, whatever the script draws:
    /// the wait at the quota, too many records, an archived channel.
    pub notice: Option<String>,
}

/// Loads `source`, does `steps` to it, and tells what it drew.
pub fn try_app(
    source: impl Into<Vec<u8>>,
    options: HeadlessOptions,
    steps: &[Step],
) -> std::io::Result<Tried> {
    let mut app = Headless::start(source, options)?;
    app.run(steps);
    Ok(app.tried())
}

pub struct Headless {
    session: AppSession,
    store: SharedStore,
    clock: Arc<ManualClock>,
    signal: Signal,
    size: (u16, u16),
    theme: AppTheme,
    logs: Vec<String>,
    types: Vec<String>,
    closed: bool,
    settled: bool,
    cursor: Option<Position>,
    /// How long to wait for a script to come to rest.
    patience: Duration,
}

impl Headless {
    /// Starts the app on an empty store and waits until it has drawn.
    pub fn start(source: impl Into<Vec<u8>>, options: HeadlessOptions) -> std::io::Result<Self> {
        let clock = ManualClock::new(START_OF_TIME);
        let store = MemoryStore::new(options.user.clone(), clock.clone());
        Self::start_on(source, options, store, clock)
    }

    /// Starts the app on a store that already holds records.
    pub fn start_on(
        source: impl Into<Vec<u8>>,
        options: HeadlessOptions,
        store: MemoryStore,
        clock: Arc<ManualClock>,
    ) -> std::io::Result<Self> {
        let signal = Signal::new();
        // Long enough for a call to run out its budget and be given up on.
        let patience = options.limits.call_time * 4 + options.limits.not_responding_after;
        let store = SharedStore::new(store);
        let session = AppSession::in_memory(
            source,
            SessionOptions::new(options.app_name())
                .script(options.name)
                .size(options.width, options.height)
                .limits(options.limits)
                .clock(clock.clone())
                .waker(signal.waker()),
            &store,
        )?;
        let mut app = Self {
            session,
            store,
            clock,
            signal,
            size: (options.width, options.height),
            theme: options.theme,
            logs: Vec::new(),
            types: Vec::new(),
            closed: false,
            settled: true,
            cursor: None,
            patience,
        };
        app.settle();
        Ok(app)
    }

    /// The store behind the app: to look at its records, or to make it
    /// misbehave. What a call on it returns is given to [`Headless::deliver`].
    pub fn store(&mut self) -> MutexGuard<'_, MemoryStore> {
        self.store.lock()
    }

    /// The session between the script and the store.
    pub fn session(&mut self) -> &mut AppSession {
        &mut self.session
    }

    /// The runtime itself: to draw it somewhere else than
    /// [`Headless::buffer`] does.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.session.runtime_mut()
    }

    /// The clock the app sees.
    pub fn clock(&self) -> &Arc<ManualClock> {
        &self.clock
    }

    /// Gives the app what the store produced, and waits for it to rest.
    pub fn deliver(&mut self, told: Vec<Told>) -> &mut Self {
        self.store.tell(told);
        self.settle();
        self
    }

    /// Sends one event and waits for the app to rest.
    pub fn send(&mut self, event: Event) -> &mut Self {
        self.session.send(event);
        self.settle();
        self
    }

    /// A key, written as [`Key::parse`] reads it. A name that is no key is
    /// a mistake of the caller and is ignored.
    pub fn key(&mut self, written: &str) -> &mut Self {
        match Key::parse(written) {
            Some(key) => self.send(Event::Key(key)),
            None => self,
        }
    }

    /// Types text, one key for each character.
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        for c in text.chars() {
            let key = if c == ' ' {
                Key::named("space")
            } else {
                Key::named(&c.to_string())
            };
            self.session.send(Event::Key(key));
        }
        self.settle();
        self
    }

    pub fn paste(&mut self, text: &str) -> &mut Self {
        self.send(Event::Paste(text.to_string()))
    }

    pub fn resize(&mut self, width: u16, height: u16) -> &mut Self {
        self.size = (width, height);
        self.session.resize(width, height);
        self.settle();
        self
    }

    /// Lets time pass and fires the timers that fall due.
    pub fn advance(&mut self, ms: u64) -> &mut Self {
        self.clock.advance(Duration::from_millis(ms));
        self.session.tick();
        self.settle();
        self
    }

    /// Replaces the script while the app runs.
    pub fn reload(&mut self, source: impl Into<Vec<u8>>) -> &mut Self {
        self.send(Event::Reload(source.into()))
    }

    pub fn run(&mut self, steps: &[Step]) -> &mut Self {
        for step in steps {
            match step {
                Step::Key(key) => self.send(Event::Key(key.clone())),
                Step::Type(text) => self.type_text(text),
                Step::Paste(text) => self.paste(text),
                Step::Resize(w, h) => self.resize(*w, *h),
                Step::Advance(ms) => self.advance(*ms),
            };
        }
        self
    }

    /// Lets the session carry requests, replies and changes between the
    /// script and the store until neither has anything left to do. False
    /// when the app did not come to rest in time: the time a call may
    /// take, a few times over.
    pub fn settle(&mut self) -> bool {
        self.wait(self.patience)
    }

    /// The same, with the time to wait given.
    pub fn wait(&mut self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        loop {
            // Whoever wrote in the store has a name there.
            let names = self.store.lock().names().clone();
            self.session.set_names(names);
            let quiet = self.take() == 0;
            if quiet && self.session.is_idle() {
                // What was reported between the look and the look at the
                // counter is still in the queue.
                if self.take() == 0 {
                    self.settled = true;
                    return true;
                }
                continue;
            }
            if Instant::now() > deadline {
                self.settled = false;
                return false;
            }
            if quiet {
                self.signal.wait(Duration::from_millis(20));
            }
        }
    }

    /// One look at the session. How many things it had to tell.
    fn take(&mut self) -> usize {
        let outputs = self.session.poll();
        let told = outputs.len();
        for output in outputs {
            match output {
                Output::Log(line) => self.logs.push(line),
                Output::Loaded { types } => self.types = types,
                Output::Close => self.closed = true,
                Output::Redraw | Output::Request(_) => {}
            }
        }
        told
    }

    /// The app's region as Ratatui drew it.
    pub fn buffer(&mut self) -> Buffer {
        let area = Rect::new(0, 0, self.size.0, self.size.1);
        let mut buffer = Buffer::empty(area);
        self.cursor = self.session.draw(area, &mut buffer, &self.theme);
        buffer
    }

    /// The screen as text, one line for each row.
    pub fn screen(&mut self) -> String {
        text_of(&self.buffer())
    }

    /// Where the app wants the cursor, after the last [`Headless::screen`].
    pub fn cursor(&self) -> Option<Position> {
        self.cursor
    }

    pub fn status(&self) -> Status {
        self.session.runtime().status()
    }

    /// The status of the session: the script's, and its records'.
    pub fn session_status(&self) -> SessionStatus {
        self.session.status()
    }

    /// What the app's pane would say by itself, if anything.
    pub fn notice(&self) -> Option<String> {
        self.session.status().notice
    }

    /// What is wrong with the app, if anything.
    pub fn error(&self) -> Option<String> {
        self.session.runtime().status().error
    }

    pub fn phase(&self) -> Phase {
        self.session.runtime().status().phase
    }

    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    /// The record types the running version declares.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// The app asked to close.
    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn tried(&mut self) -> Tried {
        Tried {
            screen: self.screen(),
            error: self.error(),
            logs: self.logs.clone(),
            types: self.types.clone(),
            closed: self.closed,
            responding: self.settled && self.session.runtime().status().responding,
            notice: self.notice(),
        }
    }
}
