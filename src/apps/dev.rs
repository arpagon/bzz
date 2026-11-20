//! An app outside bzz's window: tried once (`bzz app try`), or run from the
//! owner's own folder and reloaded on every save (`bzz app dev`, plan D5).
//!
//! Both run the script on a record store in memory, which can be seeded
//! from a file. No relay, no channel and no key is involved, and nothing
//! is approved: `try` draws and ends, and `dev` runs the bytes of a folder
//! only the owner can have named, in a terminal he is sitting at.
//!
//! [`Dev`] is the app with its store and its file; it has no terminal, so
//! that a test can save the file and read the screen. [`run`] puts it in
//! one.
//!
//! The pane of the TUI does not exist yet. When it does, `dev` is to run
//! the folder there, on a channel's records: see `docs/apps.md`, "Writing
//! an app", for what that needs.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event as TerminalEvent, KeyCode, KeyEventKind,
    KeyModifiers,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use serde_json::{Map, Value};

use super::runtime::records::valid_type_name;
use super::runtime::{
    AppTheme, Event, Key, Limits, ManualClock, Output, Phase, Signal, Status, SystemClock, Who,
    limits,
};
use super::session::headless::{Headless, HeadlessOptions, START_OF_TIME, Step, Tried};
use super::session::{AppSession, MemoryStore, SessionOptions, SharedStore};
use super::store::folder::read_bounded;
use super::store::manifest::{SCRIPT_BYTES, SCRIPT_FILE, sha256, visible};

/// The largest seed file `try` and `dev` read.
pub const SEED_BYTES: usize = 4 * 1024 * 1024;

/// How often `dev` looks at the script's file.
pub const WATCH_EVERY: Duration = Duration::from_millis(200);

/// Log lines `dev` keeps to show.
const LOG_LINES: usize = 200;

/// The local participant of an app run outside bzz's window. No key of
/// anyone is involved: the id is a made-up one.
pub fn local_user() -> Who {
    Who::new("0".repeat(64), "you")
}

/// A participant made up from a name, for a seeded record written "by"
/// someone else. The same name gives the same id.
fn named(name: &str) -> Who {
    Who::new(sha256(name.as_bytes()), name)
}

/// One record to start with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    pub record_type: String,
    pub content: Map<String, Value>,
    /// Who wrote it; the local participant when none is named.
    pub by: Option<String>,
}

/// Reads a seed file: a JSON list of `{ "type": "card", "content": { ... },
/// "by": "bo" }`, where `by` may be left out. The error says which record
/// is wrong.
pub fn parse_seed(text: &str) -> Result<Vec<Seed>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("the seed is not JSON: {error}"))?;
    let Value::Array(items) = value else {
        return Err(
            "the seed is a JSON list of records: [{ \"type\": ..., \"content\": { ... } }]".into(),
        );
    };
    if items.len() > limits::LIVE_RECORDS {
        return Err(format!(
            "the seed has {} records; an app may have {}",
            items.len(),
            limits::LIVE_RECORDS
        ));
    }
    let mut seeds = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let number = index + 1;
        let Value::Object(mut item) = item else {
            return Err(format!("seed record {number} is not an object"));
        };
        let record_type = match item.remove("type") {
            Some(Value::String(name)) if valid_type_name(&name) => name,
            _ => {
                return Err(format!(
                    "seed record {number} needs \"type\": a record type's name"
                ));
            }
        };
        let content = match item.remove("content") {
            Some(Value::Object(content)) => content,
            _ => {
                return Err(format!(
                    "seed record {number} needs \"content\": an object with the record's fields"
                ));
            }
        };
        let by = match item.remove("by") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) if !name.trim().is_empty() => Some(name),
            Some(_) => return Err(format!("seed record {number}: \"by\" is a name")),
        };
        if let Some(unknown) = item.keys().next() {
            return Err(format!(
                "seed record {number} has {unknown:?}; a seed record has \"type\", \"content\" and \"by\""
            ));
        }
        seeds.push(Seed {
            record_type,
            content,
            by,
        });
    }
    Ok(seeds)
}

/// Reads and parses a seed file.
pub fn read_seed(path: &Path) -> Result<Vec<Seed>, String> {
    let bytes = read_bounded(path, SEED_BYTES)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("{}: the seed is not UTF-8 text", path.display()))?;
    parse_seed(text).map_err(|error| format!("{}: {error}", path.display()))
}

fn plant(store: &mut MemoryStore, seeds: &[Seed], user: &Who) {
    for seed in seeds {
        let by = seed.by.as_deref().map_or_else(|| user.clone(), named);
        store.seed(&seed.record_type, seed.content.clone(), &by);
    }
}

/// The seeds whose type the script does not declare: it would never see
/// them, and whoever wrote the seed should know.
fn undeclared(seeds: &[Seed], types: &[String]) -> Option<String> {
    let mut strangers: Vec<&str> = seeds
        .iter()
        .map(|seed| seed.record_type.as_str())
        .filter(|name| !types.iter().any(|declared| declared == name))
        .collect();
    strangers.sort_unstable();
    strangers.dedup();
    (!strangers.is_empty()).then(|| {
        format!(
            "the seed has records of type {}, which the script does not declare (it declares {}): it will never see them",
            strangers.join(", "),
            types.join(", ")
        )
    })
}

/// What `bzz app try` is given.
pub struct Trial {
    pub script: Vec<u8>,
    /// The script's name in error messages.
    pub script_name: String,
    pub width: u16,
    pub height: u16,
    pub steps: Vec<Step>,
    pub seeds: Vec<Seed>,
}

/// What a trial gave: what the script drew, and what to tell about the
/// seed.
pub struct Trying {
    pub tried: Tried,
    pub seed_warning: Option<String>,
}

/// Loads a script apart on a seeded store in memory, does the steps to it,
/// and tells what it drew. Time passes only by a `wait:` step, so the same
/// trial gives the same screen.
pub fn try_script(trial: Trial) -> std::io::Result<Trying> {
    let user = local_user();
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(user.clone(), clock.clone());
    plant(&mut store, &trial.seeds, &user);
    let options = HeadlessOptions::new(trial.script_name)
        .size(trial.width, trial.height)
        .user(user);
    let mut app = Headless::start_on(trial.script, options, store, clock)?;
    let seed_warning = (app.phase() == Phase::Running)
        .then(|| undeclared(&trial.seeds, app.types()))
        .flatten();
    app.run(&trial.steps);
    Ok(Trying {
        tried: app.tried(),
        seed_warning,
    })
}

/// A size written `40x16`.
pub fn parse_size(written: &str) -> Result<(u16, u16), String> {
    written
        .split_once('x')
        .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
        .filter(|(width, height)| *width > 0 && *height > 0)
        .ok_or_else(|| format!("{written}: a size is written 40x16"))
}

/// What a look at the script's file found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Saved {
    /// The file holds what is running.
    Same,
    /// The file changed, and its bytes were given to the running app.
    Reloaded,
    /// The file cannot be read as a script right now: it is being written,
    /// was removed, or is too large. What runs goes on.
    Unreadable(String),
}

/// An app run from a file, on a record store in memory, reloaded when the
/// file changes. Between the script and the store stands an [`AppSession`],
/// the one bzz's window uses on the relay.
pub struct Dev {
    script: PathBuf,
    script_name: String,
    session: AppSession,
    store: SharedStore,
    signal: Signal,
    /// The bytes that were last given to the runtime.
    running: Vec<u8>,
    /// The size the app was asked to have, and the size it has: smaller
    /// when the terminal has no room for the first.
    wanted: (u16, u16),
    size: (u16, u16),
    theme: AppTheme,
    logs: VecDeque<String>,
    types: Vec<String>,
    reloads: u32,
    closed: bool,
    /// Why the file could not be read at the last look, if it could not.
    unreadable: Option<String>,
    seed_warning: Option<String>,
    seeds: Vec<Seed>,
}

impl Dev {
    /// Starts the script of `script` and waits until it has drawn, or
    /// until the time a stuck script is given has passed.
    pub fn start(
        script: &Path,
        width: u16,
        height: u16,
        seeds: Vec<Seed>,
        limits: Limits,
    ) -> Result<Self, String> {
        let source = read_bounded(script, SCRIPT_BYTES)?;
        let script_name = script
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| SCRIPT_FILE.to_owned());
        let user = local_user();
        let clock = SystemClock::shared();
        let mut store = MemoryStore::new(user.clone(), clock.clone());
        plant(&mut store, &seeds, &user);
        let store = SharedStore::new(store);
        let signal = Signal::new();
        let patience = limits.call_time * 4 + limits.not_responding_after;
        let session = AppSession::in_memory(
            source.clone(),
            SessionOptions::new(app_name(&script_name))
                .script(script_name.clone())
                .size(width, height)
                .limits(limits)
                .clock(clock)
                .waker(signal.waker()),
            &store,
        )
        .map_err(|error| format!("no thread could be started for the script: {error}"))?;
        let mut dev = Self {
            script: script.to_owned(),
            script_name,
            session,
            store,
            signal,
            running: source,
            wanted: (width, height),
            size: (width, height),
            theme: AppTheme::default(),
            logs: VecDeque::new(),
            types: Vec::new(),
            reloads: 0,
            closed: false,
            unreadable: None,
            seed_warning: None,
            seeds,
        };
        dev.settle(patience);
        Ok(dev)
    }

    /// Takes what the script and the store produced; the session carries
    /// requests and replies between them. Answers whether there is
    /// something new to draw.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        loop {
            let outputs = self.session.poll();
            if outputs.is_empty() {
                return changed;
            }
            for output in outputs {
                changed = true;
                match output {
                    Output::Request(_) => {}
                    Output::Log(line) => {
                        if self.logs.len() == LOG_LINES {
                            self.logs.pop_front();
                        }
                        self.logs.push_back(line);
                    }
                    Output::Loaded { types } => {
                        self.seed_warning = undeclared(&self.seeds, &types);
                        self.types = types;
                    }
                    Output::Close => self.closed = true,
                    Output::Redraw => {}
                }
            }
        }
    }

    /// Pumps until the script has nothing left to do, or `patience` has
    /// passed. Answers whether it came to rest.
    pub fn settle(&mut self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        loop {
            let changed = self.pump();
            if !changed && self.session.is_idle() && !self.pump() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            self.signal.wait(Duration::from_millis(20));
        }
    }

    /// Waits until the script has something new, or `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        self.signal.wait(timeout)
    }

    /// Looks at the script's file. When its bytes are not the ones that
    /// run, they are given to the running app: a version that loads takes
    /// the old one's place and keeps its state, and one that does not
    /// leaves the old one running, with the error.
    pub fn look(&mut self) -> Saved {
        let bytes = match read_bounded(&self.script, SCRIPT_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                let changed = self.unreadable.as_deref() != Some(error.as_str());
                self.unreadable = Some(error.clone());
                return if changed {
                    Saved::Unreadable(error)
                } else {
                    Saved::Same
                };
            }
        };
        self.unreadable = None;
        // An editor may empty a file before it writes it: wait for bytes.
        if bytes == self.running || bytes.is_empty() {
            return Saved::Same;
        }
        self.running.clone_from(&bytes);
        self.reloads += 1;
        self.session.send(Event::Reload(bytes));
        Saved::Reloaded
    }

    pub fn key(&mut self, key: Key) {
        self.session.key(key);
    }

    pub fn paste(&mut self, text: String) {
        self.session.paste(text);
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        if self.size != (width, height) {
            self.size = (width, height);
            self.session.resize(width, height);
        }
    }

    /// Lets the app's timers fire, if it has any waiting, and the session
    /// answer what waited for time. True when the status changed with it.
    pub fn tick(&mut self) -> bool {
        self.session.tick()
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn status(&self) -> Status {
        self.session.runtime().status()
    }

    /// The app called `host.quit()`.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// How many times the file was given to the running app again.
    pub fn reloads(&self) -> u32 {
        self.reloads
    }

    pub fn types(&self) -> &[String] {
        &self.types
    }

    pub fn logs(&self) -> impl Iterator<Item = &str> {
        self.logs.iter().map(String::as_str)
    }

    /// How many records the store in memory holds.
    pub fn records(&self) -> usize {
        self.store.lock().records().len()
    }

    /// The app's own region as text.
    pub fn screen(&mut self) -> String {
        let area = Rect::new(0, 0, self.size.0, self.size.1);
        let mut buffer = Buffer::empty(area);
        self.session.draw(area, &mut buffer, &self.theme);
        super::runtime::draw::text_of(&buffer)
    }

    /// The lines under the app: what runs, and what is wrong.
    pub fn status_lines(&self) -> Vec<String> {
        let status = self.session.runtime().status();
        let mut lines = vec![format!(
            "{} · {} · {} · {} in memory",
            self.script.display(),
            match status.phase {
                Phase::Starting => "starting".to_owned(),
                Phase::Running => "running".to_owned(),
                Phase::NotLoaded => "not loaded".to_owned(),
                Phase::GivenUp => "stopped: it ran too long three times".to_owned(),
                Phase::Closed => "closed".to_owned(),
                Phase::Crashed => "the runtime failed".to_owned(),
            },
            match self.reloads {
                0 => "not reloaded yet".to_owned(),
                1 => "reloaded once".to_owned(),
                n => format!("reloaded {n} times"),
            },
            match self.records() {
                1 => "1 record".to_owned(),
                n => format!("{n} records"),
            },
        )];
        if let Some(error) = &self.unreadable {
            lines.push(format!("cannot read the file: {error}"));
        }
        if let Some(error) = &status.error {
            lines.push(visible(error.lines().next().unwrap_or_default()));
        }
        if !status.responding {
            lines.push("the app does not respond".into());
        }
        if let Some(notice) = self.session.status().notice {
            lines.push(notice);
        }
        if let Some(warning) = &self.seed_warning {
            lines.push(warning.clone());
        }
        lines.push(
            "Ctrl+C leaves. Every other key is the app's. Saving the file reloads it.".into(),
        );
        lines
    }

    /// Draws the app in a border at the top left of `area`, and the status
    /// lines under it. The app is given as much of its size as fits.
    /// Answers where the app wants the cursor.
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer) -> Option<Position> {
        let lines = self.status_lines();
        let wanted = (
            self.wanted.0.saturating_add(2),
            self.wanted.1.saturating_add(2),
        );
        let room = area.height.saturating_sub(lines.len() as u16);
        let frame = Rect::new(
            area.x,
            area.y,
            wanted.0.min(area.width),
            wanted.1.min(room.max(3).min(area.height)),
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border)
            .title(format!(" {} · dev ", self.script_name));
        let inner = block.inner(frame);
        block.render(frame, buffer);
        // The app is told the size it really has; it draws for it at its
        // next round.
        self.resize(inner.width, inner.height);
        let cursor = self.session.draw(inner, buffer, &self.theme);
        let under = Rect::new(
            area.x,
            frame.bottom(),
            area.width,
            area.bottom().saturating_sub(frame.bottom()),
        );
        Paragraph::new(lines.into_iter().map(Line::raw).collect::<Vec<_>>())
            .style(self.theme.muted)
            .render(under, buffer);
        cursor
    }
}

/// The name an app's records go by when it runs from a file: the file's
/// name without `.lua` if that is an app's name, else `app`.
fn app_name(script_name: &str) -> String {
    let stem = script_name.strip_suffix(".lua").unwrap_or(script_name);
    if crate::protocol::record::valid_name(stem) {
        stem.to_owned()
    } else {
        "app".to_owned()
    }
}

/// What `bzz app dev` is given.
pub struct DevOptions {
    pub script: PathBuf,
    pub width: u16,
    pub height: u16,
    pub seeds: Vec<Seed>,
}

/// Runs a script in the terminal until Ctrl+C or until it closes itself,
/// reloading it whenever its file changes. Answers the last lines of the
/// app's log, for printing once the terminal is the shell's again.
pub fn run(options: DevOptions) -> crate::Result<Vec<String>> {
    use crate::error::Error;
    use crate::ui::terminal::TerminalGuard;
    let mut dev = Dev::start(
        &options.script,
        options.width,
        options.height,
        options.seeds,
        Limits::default(),
    )
    .map_err(Error::Config)?;
    let terminal_error = |error: std::io::Error| Error::io("terminal", error);
    let (mut guard, mut terminal) = TerminalGuard::enter(false)?;
    let _ = crossterm::execute!(std::io::stdout(), EnableBracketedPaste);
    let mut looked = Instant::now();
    let mut ticked = Instant::now();
    let mut dirty = true;
    let outcome = loop {
        if dirty {
            let drawn = terminal.draw(|frame| {
                let area = frame.area();
                if let Some(cursor) = dev.render(area, frame.buffer_mut()) {
                    frame.set_cursor_position(cursor);
                }
            });
            if let Err(error) = drawn {
                break Err(terminal_error(error));
            }
            dirty = false;
        }
        match crossterm::event::poll(Duration::from_millis(30)) {
            Ok(true) => match crossterm::event::read() {
                Ok(TerminalEvent::Key(key)) => {
                    let pressed = matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat);
                    if pressed
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c' | 'C'))
                    {
                        break Ok(());
                    }
                    if let Some(key) = Key::from_crossterm(key) {
                        dev.key(key);
                    }
                }
                Ok(TerminalEvent::Paste(text)) => dev.paste(text),
                Ok(TerminalEvent::Resize(..)) => dirty = true,
                Ok(_) => {}
                Err(error) => break Err(terminal_error(error)),
            },
            Ok(false) => {}
            Err(error) => break Err(terminal_error(error)),
        }
        if ticked.elapsed() >= Duration::from_millis(limits::TIMER_MIN_MS) {
            ticked = Instant::now();
            dirty |= dev.tick();
        }
        if looked.elapsed() >= WATCH_EVERY {
            looked = Instant::now();
            dirty |= dev.look() != Saved::Same;
        }
        dirty |= dev.pump();
        if dev.closed() {
            break Ok(());
        }
    };
    let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
    guard.restore();
    outcome?;
    Ok(dev.logs().map(visible).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(says: &str) -> String {
        format!(
            "local app = {{}}\napp.api = 1\napp.types = {{ \"note\" }}\n\
             function app.init(ctx)\n  data.list {{ type = \"note\", tag = \"load\" }}\n  return {{ count = 0, notes = 0 }}\nend\n\
             function app.key(s, key, ctx)\n  if key.name == \"q\" then host.quit() else s.count = s.count + 1 end\nend\n\
             function app.reply(s, r, ctx)\n  if r.ok and r.records then s.notes = #r.records end\nend\n\
             function app.view(s, ctx)\n  return ui.text {{ \"{says} keys=\" .. s.count .. \" notes=\" .. s.notes }}\nend\n\
             return app\n"
        )
    }

    fn rest(dev: &mut Dev) {
        assert!(dev.settle(Duration::from_secs(60)), "the app came to rest");
    }

    /// The owner's folder is reloaded when its script is saved: the new
    /// version draws, with the state the old one had. A save that does not
    /// load leaves the old version running and says why.
    #[test]
    fn a_save_reloads_the_running_app_and_a_broken_save_leaves_it_running() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join(SCRIPT_FILE);
        std::fs::write(&path, script("first")).unwrap();
        let seeds = parse_seed(
            r#"[{"type":"note","content":{"text":"a"}},{"type":"note","content":{"text":"b"},"by":"bo"}]"#,
        )
        .unwrap();
        let mut dev = Dev::start(&path, 40, 16, seeds, Limits::patient()).unwrap();
        assert_eq!(dev.screen().trim_end(), "first keys=0 notes=2");
        assert_eq!(dev.types(), ["note"]);

        // Nothing was saved: a look changes nothing.
        assert_eq!(dev.look(), Saved::Same);
        assert_eq!(dev.reloads(), 0);

        // Some keys, so that there is state to keep.
        dev.key(Key::named("j"));
        dev.key(Key::named("k"));
        rest(&mut dev);
        assert_eq!(dev.screen().trim_end(), "first keys=2 notes=2");

        // A save: the new version draws, and the state is the old one's.
        std::fs::write(&path, script("second")).unwrap();
        assert_eq!(dev.look(), Saved::Reloaded);
        rest(&mut dev);
        assert_eq!(dev.screen().trim_end(), "second keys=2 notes=2");
        assert_eq!(dev.reloads(), 1);
        assert_eq!(dev.status().error, None);
        assert_eq!(
            dev.look(),
            Saved::Same,
            "the same bytes are not given twice"
        );

        // A broken save: the old version goes on, with the error.
        std::fs::write(
            &path,
            script("third").replace("return app\n", "return app(\n"),
        )
        .unwrap();
        assert_eq!(dev.look(), Saved::Reloaded);
        rest(&mut dev);
        let error = dev.status().error.expect("the error of the broken save");
        assert!(error.starts_with("app.lua:"), "{error}");
        assert!(
            dev.screen().starts_with("second keys=2 notes=2"),
            "{}",
            dev.screen()
        );
        dev.key(Key::named("j"));
        rest(&mut dev);
        assert!(
            dev.screen().starts_with("second keys=3 notes=2"),
            "{}",
            dev.screen()
        );
        assert_eq!(dev.status().phase, Phase::Running);

        // The file is gone for a moment, as when an editor replaces it.
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(dev.look(), Saved::Unreadable(_)));
        assert_eq!(dev.look(), Saved::Same, "said once");
        assert!(
            dev.status_lines()
                .iter()
                .any(|line| line.starts_with("cannot read the file")),
            "{:?}",
            dev.status_lines()
        );

        // The fix is saved: it runs, still with the state.
        std::fs::write(&path, script("fixed")).unwrap();
        assert_eq!(dev.look(), Saved::Reloaded);
        rest(&mut dev);
        assert_eq!(dev.screen().trim_end(), "fixed keys=3 notes=2");
        assert_eq!(dev.status().error, None);
        assert_eq!(dev.reloads(), 3);

        // The app closes itself.
        dev.key(Key::named("q"));
        rest(&mut dev);
        assert!(dev.closed());
    }

    #[test]
    fn the_app_is_drawn_in_its_border_with_what_runs_under_it() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join(SCRIPT_FILE);
        std::fs::write(&path, script("hello")).unwrap();
        let mut dev = Dev::start(&path, 40, 16, Vec::new(), Limits::patient()).unwrap();
        let area = Rect::new(0, 0, 100, 30);
        let mut buffer = Buffer::empty(area);
        dev.render(area, &mut buffer);
        let text = super::super::runtime::draw::text_of(&buffer);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("┌ app.lua · dev ─"), "{}", lines[0]);
        assert_eq!(lines[0].chars().count(), 42, "40 columns inside the border");
        assert!(
            lines[1].starts_with("│hello keys=0 notes=0"),
            "{}",
            lines[1]
        );
        assert!(lines[17].starts_with('└'), "16 rows inside: {}", lines[17]);
        assert!(lines[18].contains("app.lua · running · not reloaded yet · 0 records in memory"));
        assert!(lines[19].starts_with("Ctrl+C leaves."), "{}", lines[19]);

        // A terminal smaller than the app: nothing is drawn outside it.
        let small = Rect::new(0, 0, 30, 10);
        let mut buffer = Buffer::empty(small);
        dev.render(small, &mut buffer);
        let text = super::super::runtime::draw::text_of(&buffer);
        assert!(text.lines().next().unwrap().starts_with("┌ app.lua · dev "));
        assert_eq!(dev.size(), (28, 6), "the app is told the room it has");
        dev.render(area, &mut Buffer::empty(area));
        assert_eq!(
            dev.size(),
            (40, 16),
            "and its own size again when there is room"
        );
    }

    #[test]
    fn a_trial_is_seeded_stepped_and_repeatable() {
        let seeds =
            parse_seed(r#"[{"type":"note","content":{"text":"a"}},{"type":"vote","content":{}}]"#)
                .unwrap();
        let trial = || Trial {
            script: script("try").into_bytes(),
            script_name: "note.lua".into(),
            width: 30,
            height: 4,
            steps: vec![Step::parse("j").unwrap(), Step::parse("type:ab").unwrap()],
            seeds: seeds.clone(),
        };
        let first = try_script(trial()).unwrap();
        assert_eq!(
            first.tried.screen.lines().next(),
            Some("try keys=3 notes=1")
        );
        assert_eq!(first.tried.screen.lines().count(), 4);
        assert_eq!(first.tried.error, None);
        assert_eq!(first.tried.types, ["note"]);
        let warning = first.seed_warning.expect("a seed of a type not declared");
        assert!(
            warning.contains("records of type vote") && warning.contains("it declares note"),
            "{warning}"
        );
        assert_eq!(try_script(trial()).unwrap().tried, first.tried);
    }

    #[test]
    fn a_seed_that_is_wrong_says_which_record() {
        for (text, expected) in [
            ("{", "the seed is not JSON"),
            ("{}", "the seed is a JSON list of records"),
            ("[1]", "seed record 1 is not an object"),
            (
                r#"[{"type":"note","content":{}},{"type":"Note","content":{}}]"#,
                "seed record 2 needs \"type\"",
            ),
            (r#"[{"type":"note"}]"#, "seed record 1 needs \"content\""),
            (
                r#"[{"type":"note","content":[]}]"#,
                "seed record 1 needs \"content\"",
            ),
            (
                r#"[{"type":"note","content":{},"by":7}]"#,
                "\"by\" is a name",
            ),
            (r#"[{"type":"note","content":{},"id":"x"}]"#, "has \"id\""),
        ] {
            let error = parse_seed(text).unwrap_err();
            assert!(error.contains(expected), "`{expected}` in: {error}");
        }
        assert_eq!(parse_seed("[]").unwrap(), []);
        assert_eq!(parse_size("40x16"), Ok((40, 16)));
        assert!(parse_size("40").is_err() && parse_size("0x5").is_err());
    }
}
