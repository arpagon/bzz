//! The Board of `bzz demo` (plan v0.17.0, A2): the shipped `board`
//! template, installed and approved in the demo's own store, open in
//! `#launch` on cards held in memory.
//!
//! **No second door.** The template is proposed, loaded apart and approved
//! through [`Apps`] in the demo's store (in memory, its files in the demo's
//! temporary folder), and bzz's pane takes it from [`Apps::runnable`] as it
//! takes any app. Only the records differ: they are a [`SharedStore`] in
//! memory instead of the relay's, so the cards the newcomer adds stay until
//! the demo ends and nothing is written anywhere.
//!
//! Two others write on the same cards, so that the newcomer sees that a
//! board is shared:
//!
//! - **A teammate** moves a card now and then ([`DemoBoard::tick`]): the
//!   first time [`FIRST_MOVE_AFTER`] after the Board is first shown, then
//!   never sooner than [`MOVE_EVERY`] after the last, never while the person
//!   has typed in the Board within [`TYPING_QUIET`], and only while the Board
//!   is on screen. A few moves, then the team leaves the board alone.
//! - **Scout**, when asked in words the welcome suggests
//!   (`@Scout add <title>`, `@Scout move <title>`), adds or moves a card and
//!   says so in the thread ([`scout`]). It is scripted, and says what it
//!   can do when asked anything else about the Board.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::apps::runtime::{SharedClock, SystemClock, Who};
use crate::apps::session::memory::content;
use crate::apps::session::{MemoryStore, SharedStore};
use crate::apps::store::{Apps, Proposer, Recorded, Submission, templates};
use crate::error::Error;
use crate::paths::Paths;
use crate::store::Store;

/// The app's name, as the template's.
pub const NAME: &str = "board";

/// The first teammate's move, after the Board is first on screen.
pub const FIRST_MOVE_AFTER: Duration = Duration::from_secs(15);
/// A teammate moves a card at most once in this long.
pub const MOVE_EVERY: Duration = Duration::from_secs(60);
/// No teammate moves a card while the person typed in the Board this
/// recently.
pub const TYPING_QUIET: Duration = Duration::from_secs(10);

/// The Board's columns, as the template writes them, and their names.
const COLUMNS: [(&str, &str); 3] = [("todo", "To do"), ("doing", "Doing"), ("done", "Done")];

fn column_name(column: &str) -> &'static str {
    COLUMNS
        .iter()
        .find(|(id, _)| *id == column)
        .map_or("another column", |(_, name)| name)
}

/// The people who write on the Board: the viewer, three teammates, Scout.
#[derive(Clone)]
pub struct People {
    pub viewer: Who,
    pub ada: Who,
    pub lin: Who,
    pub sam: Who,
    pub scout: Who,
}

/// The cards the Board starts with: Harbor 2.4's last days.
const CARDS: [(&str, &str, Person); 8] = [
    ("Launch tweet", "todo", Person::Lin),
    ("Changelog TODO", "todo", Person::Lin),
    ("On-call handover", "todo", Person::Sam),
    ("Blog post review", "doing", Person::Lin),
    ("Status page migration", "doing", Person::Sam),
    ("2.4-rc2 on staging", "done", Person::Ada),
    ("Load test 4k/min", "done", Person::Sam),
    ("Dark theme passes AA", "done", Person::Lin),
];

/// What teammates do on their own, in order: who moves which card where.
/// A move whose card is no longer where it was is skipped.
const MOVES: [(Person, &str, &str, &str); 3] = [
    (Person::Lin, "Blog post review", "doing", "done"),
    (Person::Sam, "On-call handover", "todo", "doing"),
    (Person::Ada, "Changelog TODO", "todo", "doing"),
];

#[derive(Clone, Copy)]
enum Person {
    Ada,
    Lin,
    Sam,
}

impl People {
    fn of(&self, person: Person) -> &Who {
        match person {
            Person::Ada => &self.ada,
            Person::Lin => &self.lin,
            Person::Sam => &self.sam,
        }
    }
}

/// The cards in memory, seeded, the viewer writing as themself.
pub fn records(people: &People) -> SharedStore {
    SharedStore::new(seeded(people, SystemClock::shared()))
}

/// The Board's first cards, on `clock`.
pub fn seeded(people: &People, clock: SharedClock) -> MemoryStore {
    let mut store = MemoryStore::new(people.viewer.clone(), clock);
    for (order, (title, column, by)) in CARDS.iter().enumerate() {
        store.seed(
            "card",
            content(json!({"title": title, "column": column, "order": order + 1})),
            people.of(*by),
        );
    }
    store.set_name(&people.scout.id, &people.scout.name);
    store
}

/// Installs the shipped Board in the demo's store and opens it in
/// `channel`: proposed as the owner's, loaded apart, approved by its hash,
/// as `bzz app add` and `bzz app approve` do. The store is the demo's own
/// (in memory), and the files go under `paths`, the demo's folder.
pub fn install(
    store: &mut Store,
    paths: &Paths,
    community: Uuid,
    channel: Uuid,
) -> Result<(), Error> {
    let template = templates::find(NAME)
        .ok_or_else(|| Error::Config("the board template is missing".into()))?;
    let mut apps = Apps::new(store, paths);
    let submission = Submission::new(
        NAME,
        template.script.as_bytes(),
        template.brief.as_bytes(),
        Proposer::owner(),
    )
    .titled(template.title);
    let (version, hash) = match apps.propose(community, submission)? {
        Recorded::Waiting { version, hash, .. } => (version, hash),
        Recorded::Failed { error, .. } => {
            return Err(Error::Config(format!("the demo's Board: {error}")));
        }
        Recorded::Running { .. } => return Ok(()),
    };
    apps.approve(community, NAME, version, &hash)?;
    apps.open_in(community, NAME, channel)?;
    Ok(())
}

/// The demo's Board in bzz's window: its records, and the teammates'
/// moves.
pub struct DemoBoard {
    store: SharedStore,
    people: People,
    /// When the Board was first on screen.
    first_shown: Option<Instant>,
    last_move: Option<Instant>,
    last_key: Option<Instant>,
    /// The next of [`MOVES`] to try.
    next: usize,
    /// Says in the channel what a teammate did.
    announce: Option<Box<dyn Fn(&Who, &str) + Send + Sync>>,
}

impl DemoBoard {
    pub fn new(store: SharedStore, people: People) -> Self {
        Self {
            store,
            people,
            first_shown: None,
            last_move: None,
            last_key: None,
            next: 0,
            announce: None,
        }
    }

    /// A teammate who moves a card says so with `say`, as a message.
    pub fn announcing(mut self, say: impl Fn(&Who, &str) + Send + Sync + 'static) -> Self {
        self.announce = Some(Box::new(say));
        self
    }

    /// The records the Board's session runs on.
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// The person pressed a key in the Board, or pasted into it.
    pub fn typed(&mut self, at: Instant) {
        self.last_key = Some(at);
    }

    /// Time for the teammates. `shown`: the Board is on screen. Answers
    /// whether a teammate moved a card; the session hears of it when it is
    /// next polled.
    pub fn tick(&mut self, shown: bool, now: Instant) -> bool {
        if !shown || self.next >= MOVES.len() {
            return false;
        }
        let first = *self.first_shown.get_or_insert(now);
        let due = match self.last_move {
            None => now.saturating_duration_since(first) >= FIRST_MOVE_AFTER,
            Some(last) => now.saturating_duration_since(last) >= MOVE_EVERY,
        };
        let typing = self
            .last_key
            .is_some_and(|key| now.saturating_duration_since(key) < TYPING_QUIET);
        if !due || typing {
            return false;
        }
        while let Some((who, title, from, to)) = MOVES.get(self.next).copied() {
            self.next += 1;
            let who = self.people.of(who);
            if move_card(&self.store, who, title, Some(from), to).is_some() {
                self.last_move = Some(now);
                if let Some(say) = &self.announce {
                    say(
                        who,
                        &format!("Moved \"{title}\" to {} on the Board.", column_name(to)),
                    );
                }
                return true;
            }
        }
        false
    }
}

/// Moves the card titled `title` (from `from`, when given) to `to`, as
/// `who`: only the column changes, as the Board's brief says. Answers the
/// column it was in.
fn move_card(
    store: &SharedStore,
    who: &Who,
    title: &str,
    from: Option<&str>,
    to: &str,
) -> Option<String> {
    let mut held = store.lock();
    let card = held.find_by("title", title)?;
    let was = card
        .content
        .get("column")
        .and_then(Value::as_str)?
        .to_owned();
    if from.is_some_and(|from| from != was) || was == to {
        return None;
    }
    let mut moved = card.content.clone();
    moved.insert("column".into(), to.into());
    let told = held.remote_update(who, &card.id, moved);
    drop(held);
    store.tell(told);
    Some(was)
}

/// What Scout was asked to do on the Board.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ask {
    /// `@Scout add <title>`.
    Add(String),
    /// `@Scout move <title> [to <column>]`.
    Move { title: String, to: Option<String> },
    /// Something about the Board that is neither.
    Other,
}

/// The longest title Scout writes on a card.
const TITLE_CHARS: usize = 40;

/// Reads a message to Scout: `add` or `move` as its first word (after the
/// mention, and a "please"), or a message that names the Board. None: the
/// message is not about the Board.
pub fn ask(text: &str) -> Option<Ask> {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.starts_with('@') && !word.starts_with("nostr:"))
        .collect();
    let mut rest = words.as_slice();
    if rest
        .first()
        .is_some_and(|word| trimmed(word).eq_ignore_ascii_case("please"))
    {
        rest = &rest[1..];
    }
    let verb = rest.first().map(|word| trimmed(word).to_lowercase());
    let tail = rest.get(1..).unwrap_or_default().join(" ");
    match verb.as_deref() {
        Some("add") => {
            // "add a card called: X", "add card X", "add X".
            let mut title = tail.as_str();
            loop {
                let rest = title.trim_start_matches([' ', ':']);
                let first = rest.split_whitespace().next().unwrap_or_default();
                let filler = ["a", "an", "new", "card", "card:", "called", "called:"]
                    .contains(&first.to_lowercase().as_str());
                if !filler || rest.len() == first.len() {
                    title = rest;
                    break;
                }
                title = &rest[first.len()..];
            }
            let title = title
                .trim()
                .trim_matches(|c: char| c == '"' || c == '\'' || c == ':' || c == '.')
                .trim();
            let title: String = title
                .chars()
                .filter(|c| !c.is_control())
                .take(TITLE_CHARS)
                .collect();
            Some(Ask::Add(title.trim().to_owned()))
        }
        Some("move") => {
            let lower = tail.to_lowercase();
            let (title, to) = match lower.rfind(" to ") {
                Some(at) => {
                    let column = lower[at + 4..].trim().trim_end_matches(['.', '!']);
                    let column = match column.trim_start_matches("the ") {
                        "to do" | "todo" => Some("todo"),
                        "doing" => Some("doing"),
                        "done" => Some("done"),
                        _ => None,
                    };
                    match column {
                        Some(column) => (tail[..at].to_owned(), Some(column.to_owned())),
                        None => (tail.clone(), None),
                    }
                }
                None => (tail.clone(), None),
            };
            let title = title
                .trim()
                .trim_start_matches("the ")
                .trim_start_matches("card ")
                .trim_matches(|c: char| c == '"' || c == '\'' || c == '.')
                .trim()
                .to_owned();
            Some(Ask::Move { title, to })
        }
        _ if words
            .iter()
            .any(|word| trimmed(word).eq_ignore_ascii_case("board")) =>
        {
            Some(Ask::Other)
        }
        _ => None,
    }
}

fn trimmed(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric())
}

/// What Scout can do here, in its own words.
pub const SCOUT_CAN: &str = "On the Board in #launch I can add a card or move one: \"@Scout add Ship party\", \"@Scout move Launch tweet to Doing\". That is all I am scripted for here.";

/// Scout does what it was asked on the Board, as `scout`, and answers what
/// it says in the thread.
pub fn scout(store: &SharedStore, scout: &Who, asked: &Ask) -> String {
    match asked {
        Ask::Add(title) if title.is_empty() => {
            format!("Which card? Give me its title: \"@Scout add Ship party\". {SCOUT_CAN}")
        }
        Ask::Add(title) => {
            let order = store
                .lock()
                .records()
                .iter()
                .filter_map(|card| card.content.get("order").and_then(Value::as_f64))
                .fold(0.0_f64, f64::max)
                + 1.0;
            let told = store
                .lock()
                .remote_create(
                    scout,
                    "card",
                    content(json!({"title": title, "column": "todo", "order": order})),
                )
                .1;
            store.tell(told);
            format!("Done: I added \"{title}\" to To do on the Board in #launch.")
        }
        Ask::Move { title, to } => {
            let Some(found) = find(store, title) else {
                return format!(
                    "I found no card called \"{title}\" on the Board. Its cards: {}. {SCOUT_CAN}",
                    titles(store)
                );
            };
            let (found_title, column) = found;
            let to = match to {
                Some(to) => to.clone(),
                None => match COLUMNS.iter().position(|(id, _)| *id == column) {
                    Some(at) if at + 1 < COLUMNS.len() => COLUMNS[at + 1].0.to_owned(),
                    _ => {
                        return format!("\"{found_title}\" is in Done already: nothing to move.");
                    }
                },
            };
            match move_card(store, scout, &found_title, None, &to) {
                Some(was) => format!(
                    "Done: I moved \"{found_title}\" from {} to {} on the Board in #launch.",
                    column_name(&was),
                    column_name(&to)
                ),
                None => format!(
                    "\"{found_title}\" is in {} already: nothing to move.",
                    column_name(&to)
                ),
            }
        }
        Ask::Other => SCOUT_CAN.to_owned(),
    }
}

/// The card whose title is `wanted`, or contains it, or else shares the
/// most words with it: its title and its column.
fn find(store: &SharedStore, wanted: &str) -> Option<(String, String)> {
    let wanted = wanted.to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    let cards: Vec<(String, String)> = store
        .lock()
        .records()
        .into_iter()
        .filter_map(|card| {
            let title = card.content.get("title")?.as_str()?.to_owned();
            let column = card.content.get("column")?.as_str()?.to_owned();
            Some((title, column))
        })
        .collect();
    let score = |title: &str| {
        let lower = title.to_lowercase();
        if lower == wanted {
            return 1000;
        }
        if lower.contains(&wanted) {
            return 500;
        }
        wanted
            .split_whitespace()
            .filter(|word| word.len() > 2 && lower.contains(word))
            .count()
    };
    cards
        .into_iter()
        .map(|card| (score(&card.0), card))
        .filter(|(score, _)| *score > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.0.cmp(&a.1.0)))
        .map(|(_, card)| card)
}

fn titles(store: &SharedStore) -> String {
    let mut titles: Vec<String> = store
        .lock()
        .records()
        .into_iter()
        .filter_map(|card| Some(card.content.get("title")?.as_str()?.to_owned()))
        .collect();
    titles.sort();
    titles.join(", ")
}
