//! The app runtime: one Lua script, run on a thread of its own, drawn by
//! whoever owns the runtime.
//!
//! ```text
//!   owner (bzz's loop, a test)                 the script's thread
//!   ─────────────────────────                  ───────────────────
//!   Runtime::send(Event)   ── events ───────▶  handler calls, then view
//!   Runtime::poll()        ◀── reports ─────   screen, requests, log lines
//!   Runtime::draw(area, buffer, theme)         (never touches a terminal)
//! ```
//!
//! **Nothing in this exchange blocks the owner.** `send` puts an event in a
//! queue, `poll` takes what is there, `draw` draws the last screen the
//! script produced. A script that is stuck costs the owner a stale screen
//! and nothing else. The owner is woken through the [`Waker`] it gave.
//!
//! The runtime has no terminal and no socket. Records cross a pair of plain
//! types ([`records`]): requests go out in [`Output::Request`], replies and
//! pushes come in as [`Event::Reply`] and [`Event::Push`].
//!
//! What a script is held to is in [`limits`]; how it is stopped is in
//! [`vm`]; what it can call is in [`api`], and `docs/app-api.md` is the
//! contract it is written against.

pub mod api;
mod bounded;
pub mod clock;
pub mod draw;
pub mod headless;
mod layout;
pub mod limits;
pub mod memory;
mod pattern;
pub mod records;
pub mod ui;
mod value;
pub mod vm;
mod worker;

#[cfg(test)]
mod tests;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

pub use clock::{Clock, ManualClock, SharedClock, SystemClock};
pub use limits::Limits;
pub use records::{
    Change, Record, RecordAnswer, RecordError, RecordOp, RecordPush, RecordReply, RecordRequest,
    Who,
};
pub use ui::AppTheme;
pub use worker::{API_VERSION, HANDLERS};

use ui::Node;
use worker::{Report, Setup, Shared};

/// How the owner is told there is something new: the script's thread calls
/// it after it has put reports in the queue. It must not block and may be
/// called often. For bzz's loop it sends a message on the channel the loop
/// already waits on; for a plain test, [`Signal`] is one.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A waker that does nothing, for an owner that polls anyway.
pub fn no_waker() -> Waker {
    Arc::new(|| {})
}

/// A [`Waker`] one thread can wait on.
#[derive(Clone, Default)]
pub struct Signal {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl Signal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn waker(&self) -> Waker {
        let inner = self.inner.clone();
        Arc::new(move || {
            let (flag, condition) = &*inner;
            *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
            condition.notify_all();
        })
    }

    /// Waits until the waker was called, or `timeout`. Answers whether it
    /// was called.
    pub fn wait(&self, timeout: Duration) -> bool {
        let (flag, condition) = &*self.inner;
        let guard = flag.lock().unwrap_or_else(|e| e.into_inner());
        let (mut guard, _) = condition
            .wait_timeout_while(guard, timeout, |woken| !*woken)
            .unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    }
}

/// A key press, as the contract's `Key`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// The character itself for a printable key (`"j"`, `"J"`, `"/"`),
    /// otherwise one of [`KEY_NAMES`].
    pub name: String,
    /// The text the key types; none for a control key, or with Ctrl or Alt.
    pub text: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The names of the keys that are not a character.
pub const KEY_NAMES: &[&str] = &[
    "space",
    "enter",
    "esc",
    "backspace",
    "tab",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "delete",
    "insert",
];

impl Key {
    /// A key without modifiers: a name of [`KEY_NAMES`], `f1` to `f12`, or
    /// one character.
    pub fn named(name: &str) -> Self {
        let mut chars = name.chars();
        let text = match (name, chars.next(), chars.next()) {
            ("space", ..) => Some(" ".to_string()),
            (_, Some(c), None) => Some(c.to_string()),
            _ => None,
        };
        let shift = text
            .as_deref()
            .is_some_and(|t| t.chars().any(char::is_uppercase));
        Self {
            name: name.to_string(),
            text,
            ctrl: false,
            alt: false,
            shift,
        }
    }

    /// A key as a person writes it: `j`, `enter`, `ctrl+u`, `shift+tab`,
    /// `alt+x`. None when the name is not a key.
    pub fn parse(written: &str) -> Option<Self> {
        let mut parts: Vec<&str> = written.split('+').collect();
        // "+" itself, and "ctrl++".
        if written == "+" || written.ends_with("++") {
            parts.retain(|p| !p.is_empty());
            parts.push("+");
        }
        let name = parts.pop()?;
        let known = KEY_NAMES.contains(&name)
            || name.chars().count() == 1
            || name
                .strip_prefix('f')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n));
        if !known {
            return None;
        }
        let mut key = Self::named(name);
        for modifier in parts {
            match modifier {
                "ctrl" => key.ctrl = true,
                "alt" => key.alt = true,
                "shift" => key.shift = true,
                _ => return None,
            }
        }
        if key.ctrl || key.alt {
            key.text = None;
        }
        Some(key)
    }

    /// A key of the terminal. None for what the contract has no name for,
    /// and for a key's release.
    pub fn from_crossterm(event: KeyEvent) -> Option<Self> {
        if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return None;
        }
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = event.modifiers.contains(KeyModifiers::ALT);
        let mut shift = event.modifiers.contains(KeyModifiers::SHIFT);
        let (name, text) = match event.code {
            KeyCode::Char(' ') => ("space".to_string(), Some(" ".to_string())),
            KeyCode::Char(c) => {
                if c.is_uppercase() {
                    shift = true;
                }
                (c.to_string(), Some(c.to_string()))
            }
            KeyCode::Enter => ("enter".into(), None),
            KeyCode::Esc => ("esc".into(), None),
            KeyCode::Backspace => ("backspace".into(), None),
            KeyCode::Tab => ("tab".into(), None),
            KeyCode::BackTab => {
                shift = true;
                ("tab".into(), None)
            }
            KeyCode::Up => ("up".into(), None),
            KeyCode::Down => ("down".into(), None),
            KeyCode::Left => ("left".into(), None),
            KeyCode::Right => ("right".into(), None),
            KeyCode::Home => ("home".into(), None),
            KeyCode::End => ("end".into(), None),
            KeyCode::PageUp => ("pageup".into(), None),
            KeyCode::PageDown => ("pagedown".into(), None),
            KeyCode::Delete => ("delete".into(), None),
            KeyCode::Insert => ("insert".into(), None),
            KeyCode::F(n) if (1..=12).contains(&n) => (format!("f{n}"), None),
            _ => return None,
        };
        Some(Self {
            name,
            text: text.filter(|_| !ctrl && !alt),
            ctrl,
            alt,
            shift,
        })
    }

    /// Ctrl+C: the one key bzz keeps. It leaves the app, and the runtime
    /// gives it to no script even when its owner sends it.
    pub fn is_reserved(&self) -> bool {
        self.ctrl && !self.alt && self.name.eq_ignore_ascii_case("c")
    }
}

/// What the owner sends.
#[derive(Clone, Debug)]
pub enum Event {
    Key(Key),
    /// A bracketed paste, as one piece of text.
    Paste(String),
    /// The app's region changed size.
    Resize {
        width: u16,
        height: u16,
    },
    /// Time has passed: timers that are due fire. An owner sends it at its
    /// own pace (bzz: every 100 ms), and need not while
    /// [`Runtime::timers`] is zero.
    Tick,
    /// The answer to a request.
    Reply(RecordReply),
    /// A change by another participant.
    Push(RecordPush),
    /// A new version of the script. It replaces the running one in place if
    /// it loads; if not, the running one goes on.
    Reload(Vec<u8>),
    /// The local participant's name or id changed.
    User(Who),
    /// The owner is done with the app.
    Stop,
}

/// What the script's thread produced, for the owner to act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// A version of the script was loaded, at start or by a reload. These
    /// are the record types it declares.
    Loaded { types: Vec<String> },
    /// The script asked for records. Answer with [`Event::Reply`].
    Request(RecordRequest),
    /// A line for the app's log: the script's own, or the runtime's about
    /// it (an error with its file and line).
    Log(String),
    /// The script called `host.quit()`: the app asked to be closed. Its
    /// thread has ended.
    Close,
    /// There is a new screen or a new error to draw.
    Redraw,
}

/// Where an app is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Its script is being loaded.
    Starting,
    Running,
    /// No version loaded. It waits for a reload with one that does.
    NotLoaded,
    /// The same handler was stopped [`limits::STOPS_BEFORE_GIVING_UP`]
    /// times in a row. It is not called again until it is reloaded, and
    /// then it starts fresh.
    GivenUp,
    /// It asked to close, or its owner stopped it.
    Closed,
    /// The runtime itself failed. A fault in bzz.
    Crashed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub phase: Phase,
    /// The script's thread has been inside one call for much longer than
    /// the budget allows. The app can be closed and started again; the
    /// thread cannot be killed, and holds a processor core until bzz quits.
    pub responding: bool,
    /// What is wrong, in full: with file, line and every line of the
    /// message. The app's region shows the first line or two of it.
    pub error: Option<String>,
}

/// What a runtime is started with.
pub struct Options {
    /// The script's name in error messages: `board.lua`.
    pub name: String,
    pub user: Who,
    /// The app's region, in columns and rows.
    pub width: u16,
    pub height: u16,
    pub limits: Limits,
    pub clock: SharedClock,
    pub waker: Waker,
}

impl Options {
    pub fn new(name: impl Into<String>, user: Who) -> Self {
        Self {
            name: name.into(),
            user,
            width: 40,
            height: 16,
            limits: Limits::default(),
            clock: SystemClock::shared(),
            waker: no_waker(),
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

    pub fn clock(mut self, clock: SharedClock) -> Self {
        self.clock = clock;
        self
    }

    pub fn waker(mut self, waker: Waker) -> Self {
        self.waker = waker;
        self
    }
}

/// One app: the owner's end of it.
pub struct Runtime {
    events: Sender<Event>,
    reports: Receiver<Report>,
    shared: Arc<Shared>,
    limits: Limits,
    screen: Option<Box<Node>>,
    error: Option<String>,
    phase: Phase,
    states: draw::States,
    /// More events were waiting than the script's thread may be owed.
    overrun: bool,
}

impl Runtime {
    /// Starts a thread for the script and returns at once. The script is
    /// loaded on that thread; a script that does not load is an app in
    /// [`Phase::NotLoaded`], with its error to show.
    pub fn start(source: impl Into<Vec<u8>>, options: Options) -> std::io::Result<Self> {
        let (events, events_rx) = mpsc::channel();
        let (reports_tx, reports) = mpsc::channel();
        let shared = Arc::new(Shared::new());
        let limits = options.limits.clone();
        let setup = Setup {
            name: options.name.clone(),
            user: options.user,
            size: (options.width, options.height),
            limits: options.limits,
            clock: options.clock,
            waker: options.waker,
            source: source.into(),
        };
        let for_thread = shared.clone();
        std::thread::Builder::new()
            .name(format!("bzz-app {}", options.name))
            .spawn(move || worker::run(setup, events_rx, reports_tx, for_thread))?;
        Ok(Self {
            events,
            reports,
            shared,
            limits,
            screen: None,
            error: None,
            phase: Phase::Starting,
            states: draw::States::default(),
            overrun: false,
        })
    }

    /// Sends an event. Never blocks. Answers false when the event was not
    /// taken: the app has ended, or more than [`Limits::event_backlog`]
    /// events already wait for a script that does not take them.
    pub fn send(&mut self, event: Event) -> bool {
        let waiting = self
            .shared
            .sent
            .load(Ordering::SeqCst)
            .saturating_sub(self.shared.handled.load(Ordering::SeqCst));
        if waiting >= self.limits.event_backlog as u64 && !matches!(event, Event::Stop) {
            self.overrun = true;
            return false;
        }
        self.shared.sent.fetch_add(1, Ordering::SeqCst);
        if self.events.send(event).is_err() {
            self.shared.sent.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Takes what the script's thread has reported since the last time.
    /// Never blocks.
    pub fn poll(&mut self) -> Vec<Output> {
        let mut outputs = Vec::new();
        let mut redraw = false;
        while let Ok(report) = self.reports.try_recv() {
            match report {
                Report::Frame {
                    screen,
                    error,
                    phase,
                } => {
                    if screen.is_some() {
                        self.screen = screen;
                    }
                    self.error = error;
                    self.phase = phase;
                    redraw = true;
                }
                Report::Out(output) => outputs.push(output),
            }
        }
        if redraw {
            outputs.push(Output::Redraw);
        }
        outputs
    }

    /// Has the script's thread finished everything it was sent? When it
    /// has, [`Runtime::poll`] gives the last of what it produced.
    pub fn is_idle(&self) -> bool {
        self.shared.handled.load(Ordering::SeqCst) >= self.shared.sent.load(Ordering::SeqCst)
    }

    /// Timers the app has waiting. While it is zero a tick does nothing.
    pub fn timers(&self) -> usize {
        self.shared.timers.load(Ordering::SeqCst)
    }

    fn responding(&self) -> bool {
        if self.overrun {
            return false;
        }
        let started = self.shared.call_started.load(Ordering::SeqCst);
        if started == 0 {
            return true;
        }
        let now = self.shared.epoch.elapsed().as_millis() as u64 + 1;
        let allowed = self.limits.call_time + self.limits.not_responding_after;
        now.saturating_sub(started) <= allowed.as_millis() as u64
    }

    pub fn status(&self) -> Status {
        Status {
            phase: self.phase,
            responding: self.responding(),
            error: self.error.clone(),
        }
    }

    /// Lets go of the app. Its thread ends after the call it is in, if it
    /// is in one. Never blocks; a thread cannot be killed.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.events.send(Event::Stop);
    }

    /// Draws the app's last screen into `area` of `buffer`, and never
    /// outside it. What is wrong with the app is said on the last row or
    /// two of its own region. Returns where a focused input wants the
    /// terminal's cursor.
    ///
    /// Call [`Runtime::poll`] first: this draws what `poll` last took.
    ///
    /// The screen is drawn on a buffer of its own, the size of the region,
    /// and copied in. A widget cannot then write past the region whatever
    /// it does: one of Ratatui's does, by a cell, when it is narrower than
    /// its selection symbol.
    pub fn draw(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) -> Option<Position> {
        let area = area.intersection(buffer.area);
        if area.is_empty() {
            return None;
        }
        let region = Rect::new(0, 0, area.width, area.height);
        let mut own = Buffer::empty(region);
        let cursor = self.draw_region(region, &mut own, theme);
        for y in 0..region.height {
            for x in 0..region.width {
                buffer[(area.x + x, area.y + y)] = own[(x, y)].clone();
            }
        }
        cursor.map(|at| Position::new(area.x + at.x, area.y + at.y))
    }

    fn draw_region(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        theme: &AppTheme,
    ) -> Option<Position> {
        match &self.screen {
            Some(screen) => draw::draw(screen, area, buffer, theme, &mut self.states),
            None => {
                self.states.cursor = None;
                buffer.set_style(area, theme.normal);
            }
        }
        let responding = self.responding();
        let trouble = match (&self.error, responding) {
            (_, false) => {
                Some("The app is not responding. Close it; it can be opened again.".to_string())
            }
            (Some(error), true) => Some(error.clone()),
            (None, true) if self.screen.is_none() && self.phase == Phase::Starting => {
                Some("Starting...".to_string())
            }
            (None, true) => None,
        };
        if let Some(trouble) = trouble {
            let plain = self.error.is_none() && responding;
            // With a screen under it the trouble takes one row, two if it
            // needs them. With none, it may take the whole region.
            let first = trouble.lines().next().unwrap_or("");
            let rows = if self.screen.is_some() {
                let cells = unicode_width::UnicodeWidthStr::width(first).max(1);
                (cells
                    .div_ceil(area.width as usize)
                    .clamp(1, ERROR_ROWS as usize) as u16)
                    .min(area.height)
            } else {
                area.height
            };
            let band = Rect::new(area.x, area.bottom() - rows, area.width, rows);
            Clear.render(band, buffer);
            buffer.set_style(band, theme.normal);
            let text: Vec<Line> = if self.screen.is_some() {
                vec![Line::raw(first.to_string())]
            } else {
                trouble.lines().map(|l| Line::raw(l.to_string())).collect()
            };
            Paragraph::new(text)
                .style(if plain { theme.muted } else { theme.error })
                .wrap(Wrap { trim: false })
                .render(band, buffer);
            if self.states.cursor.is_some_and(|c| band.contains(c)) {
                self.states.cursor = None;
            }
        }
        self.states.cursor
    }
}

/// How many rows of an app's region its error may cover.
pub const ERROR_ROWS: u16 = 2;

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}
