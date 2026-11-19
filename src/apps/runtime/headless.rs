//! An app without a window: load a script on the in-memory record store,
//! feed it keys and pastes, and read the screen it draws as text, with any
//! error. `bzz app try` and the agents' "try" tool are this.
//!
//! Time does not pass by itself here. The clock is a [`ManualClock`];
//! [`Headless::advance`] moves it and fires the timers that fall due, so a
//! run of the same steps gives the same screen every time.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

use super::clock::ManualClock;
use super::draw::text_of;
use super::limits::Limits;
use super::memory::MemoryStore;
use super::records::Who;
use super::ui::AppTheme;
use super::{Event, Key, Options, Output, Phase, Runtime, Signal, Status};

/// The clock a headless app starts at: 2026-11-18 00:00:00 UTC.
pub const START_OF_TIME: u64 = 1_794_960_000;

pub struct HeadlessOptions {
    /// The script's name in error messages.
    pub name: String,
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
    runtime: Runtime,
    store: MemoryStore,
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
        let runtime = Runtime::start(
            source,
            Options::new(options.name, options.user)
                .size(options.width, options.height)
                .limits(options.limits)
                .clock(clock.clone())
                .waker(signal.waker()),
        )?;
        let mut app = Self {
            runtime,
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
    pub fn store(&mut self) -> &mut MemoryStore {
        &mut self.store
    }

    /// The runtime itself: to draw it somewhere else than
    /// [`Headless::buffer`] does.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// The clock the app sees.
    pub fn clock(&self) -> &Arc<ManualClock> {
        &self.clock
    }

    /// Gives the app what the store produced, and waits for it to rest.
    pub fn deliver(&mut self, events: Vec<Event>) -> &mut Self {
        for event in events {
            self.runtime.send(event);
        }
        self.settle();
        self
    }

    /// Sends one event and waits for the app to rest.
    pub fn send(&mut self, event: Event) -> &mut Self {
        self.runtime.send(event);
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
            self.runtime.send(Event::Key(key));
        }
        self.settle();
        self
    }

    pub fn paste(&mut self, text: &str) -> &mut Self {
        self.send(Event::Paste(text.to_string()))
    }

    pub fn resize(&mut self, width: u16, height: u16) -> &mut Self {
        self.size = (width, height);
        self.send(Event::Resize { width, height })
    }

    /// Lets time pass and fires the timers that fall due.
    pub fn advance(&mut self, ms: u64) -> &mut Self {
        self.clock.advance(Duration::from_millis(ms));
        self.send(Event::Tick)
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

    /// Serves the app's record requests from the store until the app has
    /// nothing left to do. False when it did not come to rest in time: the
    /// time a call may take, a few times over.
    pub fn settle(&mut self) -> bool {
        self.wait(self.patience)
    }

    /// The same, with the time to wait given.
    pub fn wait(&mut self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        loop {
            let mut asked = false;
            for output in self.runtime.poll() {
                match output {
                    Output::Request(request) => {
                        asked = true;
                        for event in self.store.serve(request) {
                            self.runtime.send(event);
                        }
                    }
                    Output::Log(line) => self.logs.push(line),
                    Output::Loaded { types } => self.types = types,
                    Output::Close => self.closed = true,
                    Output::Redraw => {}
                }
            }
            if !asked && self.runtime.is_idle() {
                // What was reported between the poll and the look at the
                // counter is still in the queue.
                let late = self.runtime.poll();
                if late.is_empty() {
                    self.settled = true;
                    return true;
                }
                for output in late {
                    match output {
                        Output::Request(request) => {
                            for event in self.store.serve(request) {
                                self.runtime.send(event);
                            }
                        }
                        Output::Log(line) => self.logs.push(line),
                        Output::Loaded { types } => self.types = types,
                        Output::Close => self.closed = true,
                        Output::Redraw => {}
                    }
                }
                continue;
            }
            if Instant::now() > deadline {
                self.settled = false;
                return false;
            }
            self.signal.wait(Duration::from_millis(20));
        }
    }

    /// The app's region as Ratatui drew it.
    pub fn buffer(&mut self) -> Buffer {
        let area = Rect::new(0, 0, self.size.0, self.size.1);
        let mut buffer = Buffer::empty(area);
        self.cursor = self.runtime.draw(area, &mut buffer, &self.theme);
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
        self.runtime.status()
    }

    /// What is wrong with the app, if anything.
    pub fn error(&self) -> Option<String> {
        self.runtime.status().error
    }

    pub fn phase(&self) -> Phase {
        self.runtime.status().phase
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
            responding: self.settled && self.runtime.status().responding,
        }
    }
}
