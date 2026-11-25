//! The palette (v0.17 C1, C9, C12): one key finds and runs any action,
//! channel, DM, person, agent, app, community or theme, and asks for what an
//! action needs in steps of its own.
//!
//! Pure presentation: it ranks rows of [`crate::ui::action_table::TABLE`]
//! and the things it is handed ([`Sources`]), and says which one the person
//! chose ([`Chosen`]). It holds no signer, store, relay or terminal; `App`
//! runs the choice through the paths a key or a typed `:` command already
//! reach. A row with steps builds a `:` command, shown in the footer while
//! it is built, and `App` runs it as if it had been typed.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Config, Matcher, Utf32String,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::{
    render::sanitize,
    ui::{
        action_table::{ActionSpec, Arg, Ask, Needs, Run, TABLE, Then, Where, command_text},
        actions::{ActionContext, derive},
        keymap::{KeyChord, KeyMap, KeyScope, KeySequence, UiAction},
        state::{FocusSurface, Route},
        theme::{BorderSurface, HighlightGroup, Theme, ThemeEntry},
        typing::truncate_cells,
    },
};

/// Longest query kept. A search line, not a document.
const MAX_QUERY: usize = 128;
/// Longest free-text answer (a status, a section's name, a note).
const MAX_TEXT: usize = 400;
/// Choices of this session the empty palette offers again.
const RECENT_SHOWN: usize = 3;
/// Recent choices kept (this session only: remembering them across
/// sessions would need a migration, a non-goal of v0.17).
const RECENT_KEPT: usize = 8;
/// Rows of the empty state's first group (what the selection allows).
const CONTEXT_SHOWN: usize = 5;
/// Unread channels the empty state offers.
const UNREAD_SHOWN: usize = 5;
/// An exact alias ("q", "status") wins over any fuzzy score.
const ALIAS_BONUS: u32 = 1 << 20;
/// A choice of this session comes before what was not chosen (C12), and
/// after an exact alias.
const RECENT_BONUS: u32 = 1 << 19;

/// The emoji the palette offers after the recent ones, with words that
/// find them. The first eight are the reaction picker's, in its order.
const EMOJI: &[(&str, &str)] = &[
    ("👍", "thumbs up yes agree +1"),
    ("❤️", "heart love"),
    ("😂", "laugh joy funny lol"),
    ("🎉", "party tada celebrate"),
    ("😮", "wow surprised"),
    ("😢", "sad cry"),
    ("🙏", "thanks pray please"),
    ("👀", "eyes looking"),
    ("✅", "done check yes"),
    ("🔥", "fire hot"),
    ("🚀", "rocket launch ship"),
    ("👏", "clap applause"),
    ("💯", "hundred perfect"),
    ("🤔", "thinking hmm"),
    ("😄", "smile happy"),
    ("👋", "wave hello bye"),
];

/// The expiries offered for a status, beside a typed `N[smhdw]`.
const DURATIONS: &[(&str, &str)] = &[
    ("30m", "30 minutes"),
    ("1h", "an hour"),
    ("2h", "2 hours"),
    ("4h", "4 hours"),
    ("1d", "a day"),
    ("1w", "a week"),
];

/// Facts about the moment the palette opened, beyond the selected item's
/// [`ActionContext`]. Read once at open; the executors check again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Facts {
    pub actions: ActionContext,
    pub channel_is_dm: bool,
    pub huddle_running: bool,
    pub in_call: bool,
    pub app_here: bool,
    pub artifact_selected: bool,
    pub communities: usize,
}

/// A channel or DM the palette can open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteChannel {
    pub id: Uuid,
    pub name: String,
    pub dm: bool,
    pub unread: bool,
    pub last_event_at: u64,
}

/// A person bzz has a profile of (C12), or an agent it knows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PalettePerson {
    /// Hex.
    pub pubkey: String,
    pub name: String,
    pub agent: bool,
}

/// A community the palette can switch to: not the open one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteCommunity {
    pub id: Uuid,
    pub name: String,
}

/// What the palette lists beside the table's actions, read as it opens.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    /// Channels and DMs, in the order to list them ([`order_channels`]).
    pub channels: Vec<PaletteChannel>,
    /// People and agents, without me.
    pub people: Vec<PalettePerson>,
    /// Installed apps, by name.
    pub apps: Vec<String>,
    pub communities: Vec<PaletteCommunity>,
    pub themes: Vec<ThemeEntry>,
    /// The theme in use: the theme step starts on it.
    pub theme: String,
    /// Emoji I reacted with, the newest first.
    pub recent_emoji: Vec<String>,
}

/// A choice of this session, ranked first the next time (C12).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Recent {
    Action(&'static str),
    Channel(Uuid),
    Person(String),
    App(String),
    Community(Uuid),
    Theme(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// A row of [`TABLE`], by index.
    Action(usize),
    Channel(Uuid),
    /// Indexes into the [`Sources`] the palette holds.
    Person(usize),
    App(usize),
    Community(usize),
    Theme(usize),
    /// "Search messages for «…»".
    Search,
    /// An answer offered by the open step, by index.
    Option(usize),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Action,
    Channel,
    Dm,
    Person,
    Agent,
    App,
    Community,
    Theme,
    Shell,
    Search,
    /// An emoji offered by a step.
    Emoji,
    /// A word, a duration or a choice offered by a step.
    Word,
}

impl Kind {
    const fn glyph(self) -> &'static str {
        match self {
            Self::Action | Self::Word => "·",
            Self::Channel => "#",
            Self::Dm | Self::Person => "@",
            Self::Agent => "◆",
            Self::App => "▣",
            Self::Community => "⌂",
            Self::Theme => "◑",
            Self::Shell => "$",
            Self::Search => "/",
            Self::Emoji => " ",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Ready,
    /// Listed, dimmed, and Enter runs nothing: the reason why.
    Unavailable(String),
    /// Only the shell can do it: the command, which Enter copies.
    Shell(&'static str),
}

/// One row the palette can show, with its search text prepared once.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub target: Target,
    pub kind: Kind,
    pub title: String,
    /// The `:` form, a short key or a meaning, shown dim when there is room.
    pub detail: String,
    /// The key from the live key map, or empty.
    pub key: String,
    pub status: Status,
    aliases: &'static [&'static str],
    /// Its place among this session's choices, the newest 0.
    recent: Option<u8>,
    title32: Utf32String,
    words32: Utf32String,
}

impl Candidate {
    /// A row with no table entry behind it, for benches and tests.
    #[doc(hidden)]
    pub fn synthetic(title: &str, words: &str) -> Self {
        Self::new(
            Target::Action(usize::MAX),
            Kind::Action,
            title.to_owned(),
            String::new(),
            String::new(),
            Status::Ready,
            &[],
            words,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        target: Target,
        kind: Kind,
        title: String,
        detail: String,
        key: String,
        status: Status,
        aliases: &'static [&'static str],
        words: &str,
    ) -> Self {
        let title32 = Utf32String::from(title.as_str());
        let words32 = Utf32String::from(words);
        Self {
            target,
            kind,
            title,
            detail,
            key,
            status,
            aliases,
            recent: None,
            title32,
            words32,
        }
    }

    /// A row of an object or a step's answer: no key, no alias.
    fn object(target: Target, kind: Kind, title: String, detail: String, words: &str) -> Self {
        Self::new(
            target,
            kind,
            title,
            detail,
            String::new(),
            Status::Ready,
            &[],
            words,
        )
    }

    pub const fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Row {
    Header(String),
    Item(usize),
    Hint(String),
}

/// What the person chose, for `App` to run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Chosen {
    /// A row of [`TABLE`] without steps, by index.
    Action(usize),
    Channel(Uuid),
    /// A `:` command, run as if typed: a row's steps answered, a person's
    /// profile, a DM, an app opened here. `row` is the table row it came
    /// from, `object` what was chosen (both remembered as recent).
    Command {
        text: String,
        row: Option<usize>,
        object: Option<Recent>,
    },
    /// React to the selected message (the reaction picker's path).
    React {
        emoji: String,
        row: usize,
    },
    /// Keep this theme (the theme picker's path).
    Theme {
        id: &'static str,
        row: Option<usize>,
    },
    Community(Uuid),
    /// Open the message search with this text.
    Search(String),
}

impl Chosen {
    /// What this session remembers of the choice, the action first.
    pub fn recent(&self) -> Vec<Recent> {
        let row = |index: &Option<usize>| {
            index
                .and_then(|index| TABLE.get(index))
                .map(|spec| Recent::Action(spec.id))
        };
        match self {
            Self::Action(index) => TABLE
                .get(*index)
                .map(|spec| Recent::Action(spec.id))
                .into_iter()
                .collect(),
            Self::Channel(id) => vec![Recent::Channel(*id)],
            Self::Command {
                row: index, object, ..
            } => row(index).into_iter().chain(object.clone()).collect(),
            Self::React { row: index, .. } => row(&Some(*index)).into_iter().collect(),
            Self::Theme { id, row: index } => row(index)
                .into_iter()
                .chain(Some(Recent::Theme(id)))
                .collect(),
            Self::Community(id) => vec![Recent::Community(*id)],
            Self::Search(_) => Vec::new(),
        }
    }
}

/// What a key in the palette asks of `App`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Nothing outside the palette changes.
    Stay,
    /// Esc: close and give the focus back (and the theme, after a preview).
    Close,
    /// Run the choice.
    Run(Chosen),
    /// Copy this shell command (the row is shell-only).
    Copy(&'static str),
    /// The theme step's selection moved: show this theme behind it.
    Preview(&'static str),
    /// The theme step was left: put the theme back.
    Restore,
}

/// Which kinds a leading `>`, `#`, `@`, `$` or `/` narrows the list to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Narrow {
    All,
    /// `>`: actions, shell commands included.
    Actions,
    /// `#`: channels.
    Channels,
    /// `@`: people, agents and DMs.
    People,
    /// `$`: what only the shell does.
    Shell,
    /// `/`: only "Search messages for «…»".
    Search,
}

impl Narrow {
    /// The narrowing a query asks for, and the query without its prefix.
    pub fn split(query: &str) -> (Self, &str) {
        let query = query.trim_start();
        let narrow = match query.chars().next() {
            Some('>') => Self::Actions,
            Some('#') => Self::Channels,
            Some('@') => Self::People,
            Some('$') => Self::Shell,
            Some('/') => Self::Search,
            _ => return (Self::All, query.trim()),
        };
        (narrow, query[1..].trim())
    }

    const fn admits(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Actions => matches!(kind, Kind::Action | Kind::Shell),
            Self::Channels => matches!(kind, Kind::Channel),
            Self::People => matches!(kind, Kind::Dm | Kind::Person | Kind::Agent),
            Self::Shell => matches!(kind, Kind::Shell),
            Self::Search => matches!(kind, Kind::Search),
        }
    }
}

/// Rank `candidates` for `query` (already without its prefix): the best
/// first. Titles score in full, extra words at half; an exact alias wins
/// outright; a choice of this session comes next; a row that cannot run now
/// loses a third, a shell row half. At equal score actions come before
/// channels, then people, then the rest, then the table's order. The search
/// row is never ranked: the palette adds it last.
pub fn rank(
    candidates: &[Candidate],
    query: &str,
    narrow: Narrow,
    matcher: &mut Matcher,
) -> Vec<usize> {
    let query = query.trim();
    let listed =
        |candidate: &Candidate| candidate.kind != Kind::Search && narrow.admits(candidate.kind);
    if query.is_empty() {
        let mut all = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| listed(candidate))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        all.sort_by_key(|index| {
            let candidate = &candidates[*index];
            (candidate.recent.unwrap_or(u8::MAX), candidate.kind, *index)
        });
        return all;
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let lowered = query.to_lowercase();
    let mut scored = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| listed(candidate))
        .filter_map(|(index, candidate)| {
            let title = pattern.score(candidate.title32.slice(..), matcher);
            let words = if candidate.words32.is_empty() {
                None
            } else {
                pattern
                    .score(candidate.words32.slice(..), matcher)
                    .map(|score| score / 2)
            };
            let alias = candidate.aliases.iter().any(|alias| *alias == lowered)
                || candidate.title.eq_ignore_ascii_case(&lowered);
            let raw = match (title, words) {
                (None, None) if !alias => return None,
                (title, words) => title.unwrap_or(0).max(words.unwrap_or(0)),
            };
            let mut score = raw;
            if alias {
                score = score.saturating_add(ALIAS_BONUS);
            }
            if let Some(place) = candidate.recent {
                score = score.saturating_add(RECENT_BONUS - u32::from(place));
            }
            score = match candidate.status {
                Status::Ready => score,
                Status::Unavailable(_) => score.saturating_mul(2) / 3,
                Status::Shell(_) => score / 2,
            };
            Some((score, candidate.kind, index, raw, alias))
        })
        .collect::<Vec<_>>();
    // From three letters on, a match scattered over the words of a row
    // ("thr" in "resize width larger") is noise beside a whole word: rows
    // under two fifths of the best match go.
    if query.chars().count() >= 3 {
        let best = scored.iter().map(|row| row.3).max().unwrap_or(0);
        scored.retain(|row| row.4 || row.3.saturating_mul(5) >= best.saturating_mul(2));
    }
    let mut scored = scored
        .into_iter()
        .map(|(score, kind, index, _, _)| (score, kind, index))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    scored.into_iter().map(|(_, _, index)| index).collect()
}

/// Whether `spec` can run now, and if not, why. The reason for an action
/// of the selected item is the one `actions::derive` gives the `Space a`
/// menu, so the two never disagree.
pub fn status(spec: &ActionSpec, facts: &Facts) -> Status {
    if let Run::Shell(command) = spec.run {
        return Status::Shell(command);
    }
    let inbox = facts.actions.route == Route::Inbox;
    match spec.place {
        Where::Workspace if inbox => {
            return Status::Unavailable("leave the Inbox first (Esc)".into());
        }
        Where::Inbox if !inbox => {
            return Status::Unavailable("open the Inbox first (Space n)".into());
        }
        Where::Writing => return Status::Unavailable("only while writing".into()),
        _ => {}
    }
    // A row with steps is judged as the key it stands for ("React to
    // message" as `r`).
    let action = match spec.run {
        Run::Ui(action) => Some(action),
        Run::Steps(..) => spec.key_of,
        _ => None,
    };
    if let Some(action) = action {
        let mut context = facts.actions;
        if inbox {
            context.focus = FocusSurface::InboxList;
        } else if context.focus != FocusSurface::Context {
            // The palette moves the focus to the timeline before it runs a
            // message action from another pane: judge it from there.
            context.focus = FocusSurface::Timeline;
        }
        if let Some(entry) = derive(context)
            .into_iter()
            .find(|entry| entry.action == action)
            && !entry.enabled
            && let Some(reason) = entry.reason
        {
            return Status::Unavailable(reason.into());
        }
        if let Run::Ui(_) = spec.run
            && derive(context)
                .into_iter()
                .any(|entry| entry.action == action)
        {
            return Status::Ready;
        }
    }
    let met = match spec.needs {
        Needs::Nothing => true,
        Needs::Channel => facts.actions.has_channel,
        Needs::Message => facts.actions.has_selected_event,
        Needs::Connected => facts.actions.can_publish,
        Needs::Thread => facts.actions.context_open,
        Needs::Dm => facts.channel_is_dm,
        Needs::Huddle => facts.huddle_running,
        Needs::InCall => facts.in_call,
        Needs::AppHere => facts.app_here,
        Needs::Artifact => facts.artifact_selected,
        Needs::Communities => facts.communities > 1,
    };
    if met {
        Status::Ready
    } else {
        Status::Unavailable(spec.needs.reason().into())
    }
}

/// The key of `action` in the live key map, written as a person reads it:
/// "Ctrl+K", "Space p", "G", "Enter".
pub fn key_label(keymap: &KeyMap, scope: KeyScope, action: UiAction) -> String {
    keymap
        .effective_bindings(scope)
        .into_iter()
        .find(|binding| binding.action == action)
        .map(|binding| sequence_label(&binding.sequence))
        .unwrap_or_default()
}

pub fn sequence_label(sequence: &KeySequence) -> String {
    sequence
        .chords()
        .iter()
        .map(|chord| chord_label(*chord))
        .collect::<Vec<_>>()
        .join(" ")
}

fn chord_label(chord: KeyChord) -> String {
    let label = chord.label();
    // "Shift+G" is how the key map writes `G`; a person reads "G".
    if let Some(letter) = label.strip_prefix("Shift+")
        && letter.chars().count() == 1
        && letter
            .chars()
            .all(|character| character.is_ascii_uppercase())
    {
        return letter.to_owned();
    }
    // "Ctrl+k" → "Ctrl+K", as menus and the plan write it.
    match label.rsplit_once('+') {
        Some((modifiers, key)) if key.chars().count() == 1 => {
            format!("{modifiers}+{}", key.to_uppercase())
        }
        _ => label,
    }
}

/// The short form of a key: "852b14d9…a120".
fn short_key(pubkey: &str) -> String {
    crate::domain::abbreviated_pubkey(pubkey)
}

/// A hex key as a person types it after `:dm` or `:profile`.
fn npub(pubkey: &str) -> String {
    use nostr::nips::nip19::ToBech32 as _;
    nostr::PublicKey::from_hex(pubkey)
        .ok()
        .and_then(|key| key.to_bech32().ok())
        .unwrap_or_else(|| pubkey.to_owned())
}

/// The candidates: every listed row of the table, then channels and DMs,
/// people and agents, apps, communities, themes, and the search row.
pub fn candidates(keymap: &KeyMap, facts: &Facts, sources: &Sources) -> Vec<Candidate> {
    let scope = if facts.actions.route == Route::Inbox {
        KeyScope::Inbox
    } else {
        KeyScope::Workspace
    };
    let mut all = Vec::with_capacity(
        TABLE.len()
            + sources.channels.len()
            + sources.people.len()
            + sources.apps.len()
            + sources.communities.len()
            + sources.themes.len()
            + 1,
    );
    for (index, spec) in TABLE.iter().enumerate() {
        if !spec.listed() {
            continue;
        }
        let key_scope = match spec.place {
            Where::Inbox => KeyScope::Inbox,
            Where::Workspace => KeyScope::Workspace,
            Where::Anywhere | Where::Writing => scope,
        };
        let key = spec
            .key_action()
            .map(|action| key_label(keymap, key_scope, action))
            .unwrap_or_default();
        let (kind, detail) = match spec.run {
            Run::Shell(command) => (Kind::Shell, command.to_owned()),
            _ => (Kind::Action, spec.command.unwrap_or_default().to_owned()),
        };
        let mut words = String::from(spec.words);
        if let Some(command) = spec.command {
            words.push(' ');
            words.push_str(command.trim_start_matches(':'));
        }
        if let Run::Shell(command) = spec.run {
            words.push(' ');
            words.push_str(command);
        }
        all.push(Candidate::new(
            Target::Action(index),
            kind,
            spec.title.to_owned(),
            detail,
            key,
            status(spec, facts),
            spec.aliases,
            &words,
        ));
    }
    for channel in &sources.channels {
        let name = sanitize::single_line(&channel.name);
        all.push(Candidate::object(
            Target::Channel(channel.id),
            if channel.dm { Kind::Dm } else { Kind::Channel },
            name,
            if channel.unread {
                "unread".into()
            } else {
                String::new()
            },
            "",
        ));
    }
    for (index, person) in sources.people.iter().enumerate() {
        let (kind, detail) = if person.agent {
            (
                Kind::Agent,
                format!("{} · agent", short_key(&person.pubkey)),
            )
        } else {
            (Kind::Person, short_key(&person.pubkey))
        };
        all.push(Candidate::object(
            Target::Person(index),
            kind,
            sanitize::single_line(&person.name),
            detail,
            "",
        ));
    }
    for (index, app) in sources.apps.iter().enumerate() {
        let mut candidate = Candidate::object(
            Target::App(index),
            Kind::App,
            format!("Open {} here", sanitize::single_line(app)),
            "app".into(),
            "app",
        );
        if !facts.actions.has_channel {
            candidate.status = Status::Unavailable(Needs::Channel.reason().into());
        }
        all.push(candidate);
    }
    for (index, community) in sources.communities.iter().enumerate() {
        all.push(Candidate::object(
            Target::Community(index),
            Kind::Community,
            format!("Switch to {}", sanitize::single_line(&community.name)),
            "community".into(),
            "community",
        ));
    }
    for (index, theme) in sources.themes.iter().enumerate() {
        all.push(Candidate::object(
            Target::Theme(index),
            Kind::Theme,
            theme.name.to_owned(),
            format!("theme · {}", theme.id),
            "theme colours colors",
        ));
    }
    all.push(Candidate::object(
        Target::Search,
        Kind::Search,
        "Search messages for".into(),
        ":search".into(),
        "",
    ));
    all
}

/// Mark the candidates chosen in this session, the newest first.
fn mark_recent(candidates: &mut [Candidate], sources: &Sources, recent: &[Recent]) {
    for candidate in candidates.iter_mut() {
        candidate.recent = None;
    }
    for (place, wanted) in recent.iter().enumerate() {
        let place = u8::try_from(place).unwrap_or(u8::MAX);
        let found = candidates
            .iter()
            .position(|candidate| match (wanted, candidate.target) {
                (Recent::Action(id), Target::Action(index)) => {
                    TABLE.get(index).is_some_and(|spec| spec.id == *id)
                }
                (Recent::Channel(id), Target::Channel(channel)) => *id == channel,
                (Recent::Person(pubkey), Target::Person(index)) => sources
                    .people
                    .get(index)
                    .is_some_and(|person| person.pubkey == *pubkey),
                (Recent::App(name), Target::App(index)) => sources.apps.get(index) == Some(name),
                (Recent::Community(id), Target::Community(index)) => sources
                    .communities
                    .get(index)
                    .is_some_and(|community| community.id == *id),
                (Recent::Theme(id), Target::Theme(index)) => sources
                    .themes
                    .get(index)
                    .is_some_and(|theme| theme.id == *id),
                _ => false,
            });
        if let Some(found) = found {
            candidates[found].recent = Some(place);
        }
    }
}

/// One answer a step offers: what it writes, and what the breadcrumb shows.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Answer {
    text: String,
    shown: String,
}

/// A row with steps, while its steps are asked.
#[derive(Clone, Debug)]
struct Flow {
    row: usize,
    answers: Vec<Answer>,
    /// The query at each earlier level, the root's first: Backspace on an
    /// empty field goes back to it.
    queries: Vec<String>,
}

/// The palette's state while it is open.
pub struct Palette {
    query: String,
    candidates: Vec<Candidate>,
    sources: Sources,
    empty: Vec<Row>,
    rows: Vec<Row>,
    selected: usize,
    notice: Option<String>,
    matcher: Matcher,
    /// Whether a DM can be opened now (Ctrl+D on a person).
    can_publish: bool,
    flow: Option<Flow>,
    /// The open step's answers as rows; the first is the typed answer's
    /// slot (a duration or an emoji typed in full), listed when valid.
    options: Vec<Candidate>,
    answers: Vec<Answer>,
    /// The theme shown behind the theme step.
    previewing: Option<&'static str>,
    /// This session's choices: a step lists the recent ones first.
    recent: Vec<Recent>,
    /// Where the focus was; Esc puts it back.
    pub return_to: (Route, FocusSurface),
    /// What the candidates were built from (the App's channel count, unread
    /// count and people): when it changes while the palette is open, the
    /// App hands it new candidates (`replace_candidates`).
    pub source: (usize, usize, usize),
    offset: Cell<usize>,
    page: Cell<usize>,
}

impl Palette {
    /// Open over `candidates` (built from `sources`), with the empty state:
    /// what the selected item allows (`derive`), this session's recent
    /// choices, unread channels.
    pub fn open(
        mut candidates: Vec<Candidate>,
        sources: Sources,
        facts: &Facts,
        recent: &[Recent],
        return_to: (Route, FocusSurface),
    ) -> Self {
        mark_recent(&mut candidates, &sources, recent);
        let empty = empty_rows(&candidates, facts);
        let mut palette = Self {
            query: String::new(),
            candidates,
            sources,
            rows: Vec::new(),
            empty,
            selected: 0,
            notice: None,
            matcher: Matcher::new(Config::DEFAULT),
            can_publish: facts.actions.can_publish,
            flow: None,
            options: Vec::new(),
            answers: Vec::new(),
            previewing: None,
            recent: recent.to_vec(),
            return_to,
            source: (0, 0, 0),
            offset: Cell::new(0),
            page: Cell::new(10),
        };
        palette.refilter();
        palette
    }

    /// New candidates while open (channels arrived after it opened): the
    /// query stays, and the selection stays on the same row if it is still
    /// listed. An open step keeps its answers.
    pub fn replace_candidates(
        &mut self,
        mut candidates: Vec<Candidate>,
        sources: Sources,
        facts: &Facts,
        recent: &[Recent],
    ) {
        mark_recent(&mut candidates, &sources, recent);
        self.recent = recent.to_vec();
        self.empty = empty_rows(&candidates, facts);
        self.candidates = candidates;
        self.sources = sources;
        self.can_publish = facts.actions.can_publish;
        if self.flow.is_some() {
            return;
        }
        let selected = self.selected_candidate().map(|candidate| candidate.target);
        let notice = self.notice.take();
        self.refilter();
        self.notice = notice;
        if let Some(target) = selected
            && let Some(row) = self.rows.iter().position(
                |row| matches!(row, Row::Item(index) if self.candidates[*index].target == target),
            )
        {
            self.selected = row;
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The installed apps it was opened with.
    pub fn apps(&self) -> Vec<String> {
        self.sources.apps.clone()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The candidate a row points at: one of the root's, or of the open
    /// step's answers.
    pub fn candidate(&self, index: usize) -> Option<&Candidate> {
        if self.flow.is_some() {
            self.options.get(index)
        } else {
            self.candidates.get(index)
        }
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        match self.rows.get(self.selected) {
            Some(Row::Item(index)) => self.candidate(*index),
            _ => None,
        }
    }

    /// The titles of the listed items, in order (tests and captures).
    pub fn item_titles(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Item(index) => self.candidate(*index).map(|row| row.title.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Replace the query (tests, and a later "open with this text").
    pub fn set_query(&mut self, query: &str) {
        self.query = query.chars().take(self.query_limit()).collect();
        self.refilter();
    }

    /// The breadcrumb of an open step: "Start a DM › person", with the
    /// answers so far between ("Set my status › In a meeting › expires").
    pub fn breadcrumb(&self) -> Option<String> {
        let flow = self.flow.as_ref()?;
        let (spec, arg) = self.step()?;
        let mut parts = vec![spec.title.trim_end_matches('…').to_owned()];
        parts.extend(
            flow.answers
                .iter()
                .filter(|answer| !answer.shown.is_empty())
                .map(|answer| sanitize::single_line(&answer.shown)),
        );
        parts.push(arg.label.to_owned());
        Some(parts.join(" › "))
    }

    /// While a step is open, the `:` line its answers build so far, the
    /// selected or typed answer of this step included: what the palette
    /// teaches. `None` at the root, and for a row with no `:` form.
    pub fn building(&self) -> Option<String> {
        let flow = self.flow.as_ref()?;
        let (spec, arg) = self.step()?;
        let Run::Steps(then, args) = spec.run else {
            return None;
        };
        let tentative = self.tentative();
        match then {
            Then::Command(command) => {
                let mut answers = flow
                    .answers
                    .iter()
                    .map(|answer| answer.text.clone())
                    .collect::<Vec<_>>();
                answers.push(match tentative {
                    Some(answer) => answer.text,
                    None if arg.skip.is_some() => String::new(),
                    None => format!("<{}>", arg.label),
                });
                let answers = answers.iter().map(String::as_str).collect::<Vec<_>>();
                Some(format!(":{}", command_text(command, args, &answers)))
            }
            Then::React => Some(match tentative {
                Some(answer) => format!("reacts with {} to the selected message", answer.text),
                None => "pick the emoji to react with".into(),
            }),
            Then::Theme => Some(match tentative {
                Some(answer) => format!("theme {} · Enter keeps · Esc puts back", answer.shown),
                None => "Enter keeps · Esc puts back".into(),
            }),
        }
    }

    /// The row and argument of the open step.
    fn step(&self) -> Option<(&'static ActionSpec, &'static Arg)> {
        let flow = self.flow.as_ref()?;
        let spec = TABLE.get(flow.row)?;
        let Run::Steps(_, args) = spec.run else {
            return None;
        };
        args.get(flow.answers.len()).map(|arg| (spec, arg))
    }

    /// The answer Enter would give the open step now.
    fn tentative(&self) -> Option<Answer> {
        let (_, arg) = self.step()?;
        if arg.ask == Ask::Text {
            let text = self.query.trim();
            return (!text.is_empty()).then(|| Answer {
                text: text.to_owned(),
                shown: text.to_owned(),
            });
        }
        let Some(Row::Item(index)) = self.rows.get(self.selected) else {
            return None;
        };
        let mut answer = self.answers.get(*index)?.clone();
        if arg.ask == Ask::Person && !answer.text.is_empty() {
            answer.text = npub(&answer.text);
        }
        Some(answer)
    }

    fn query_limit(&self) -> usize {
        match self.step() {
            Some((_, arg)) if arg.ask == Ask::Text => MAX_TEXT,
            _ => MAX_QUERY,
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        let outcome = self.handle(key);
        if outcome != Outcome::Stay {
            return outcome;
        }
        // The theme step shows the selected theme behind the palette.
        match self.step() {
            Some((_, arg)) if arg.ask == Ask::Theme => {
                let selected = self.selected_theme();
                if selected.is_some() && selected != self.previewing {
                    self.previewing = selected;
                    return selected.map_or(Outcome::Stay, Outcome::Preview);
                }
                Outcome::Stay
            }
            _ => Outcome::Stay,
        }
    }

    fn selected_theme(&self) -> Option<&'static str> {
        let (_, arg) = self.step()?;
        if arg.ask != Ask::Theme {
            return None;
        }
        let Some(Row::Item(index)) = self.rows.get(self.selected) else {
            return None;
        };
        let text = &self.answers.get(*index)?.text;
        self.sources
            .themes
            .iter()
            .find(|theme| theme.id == text)
            .map(|theme| theme.id)
    }

    fn handle(&mut self, key: KeyEvent) -> Outcome {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => return self.choose(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p') if control => self.move_by(-1),
            KeyCode::Char('n') if control => self.move_by(1),
            KeyCode::Char('d') if control => return self.direct_message(),
            KeyCode::PageUp => self.move_by(-isize::try_from(self.page()).unwrap_or(1)),
            KeyCode::PageDown => self.move_by(isize::try_from(self.page()).unwrap_or(1)),
            KeyCode::Backspace if self.query.is_empty() && self.flow.is_some() => {
                return self.back();
            }
            KeyCode::Backspace if self.query.pop().is_some() => self.refilter(),
            KeyCode::Char('u') if control => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Char('w') if control => {
                let trimmed = self.query.trim_end().len();
                self.query.truncate(trimmed);
                let start = self
                    .query
                    .rfind(char::is_whitespace)
                    .map_or(0, |index| index + 1);
                self.query.truncate(start);
                self.refilter();
            }
            KeyCode::Char(character)
                if !control
                    && !alt
                    && !character.is_control()
                    && self.query.chars().count() < self.query_limit() =>
            {
                self.query.push(character);
                self.refilter();
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn page(&self) -> usize {
        self.page.get().max(1)
    }

    /// Ctrl+D on a person or an agent: the DM with them (C12).
    fn direct_message(&mut self) -> Outcome {
        if self.flow.is_some() {
            return Outcome::Stay;
        }
        let Some(Target::Person(index)) = self.selected_candidate().map(|row| row.target) else {
            return Outcome::Stay;
        };
        let Some(person) = self.sources.people.get(index) else {
            return Outcome::Stay;
        };
        if !self.can_publish {
            self.notice = Some(format!("can't now: {}", Needs::Connected.reason()));
            return Outcome::Stay;
        }
        Outcome::Run(Chosen::Command {
            text: format!("dm {}", npub(&person.pubkey)),
            row: crate::ui::action_table::by_id("dm.new").map(|(index, _)| index),
            object: Some(Recent::Person(person.pubkey.clone())),
        })
    }

    fn choose(&mut self) -> Outcome {
        if self.flow.is_some() {
            return self.answer();
        }
        let Some(candidate) = self.selected_candidate() else {
            return Outcome::Stay;
        };
        match &candidate.status {
            Status::Ready => {}
            Status::Unavailable(reason) => {
                self.notice = Some(format!("can't now: {reason}"));
                return Outcome::Stay;
            }
            Status::Shell(command) => return Outcome::Copy(command),
        }
        let chosen = match candidate.target {
            Target::Action(index) => {
                if let Some(ActionSpec {
                    run: Run::Steps(..),
                    ..
                }) = TABLE.get(index)
                {
                    self.begin(index);
                    return self.key_preview();
                }
                Chosen::Action(index)
            }
            Target::Channel(id) => Chosen::Channel(id),
            Target::Person(index) => {
                let Some(person) = self.sources.people.get(index) else {
                    return Outcome::Stay;
                };
                Chosen::Command {
                    text: format!("profile {}", npub(&person.pubkey)),
                    row: crate::ui::action_table::by_id("person.profile").map(|(index, _)| index),
                    object: Some(Recent::Person(person.pubkey.clone())),
                }
            }
            Target::App(index) => {
                let Some(app) = self.sources.apps.get(index) else {
                    return Outcome::Stay;
                };
                Chosen::Command {
                    text: format!("app open {app}"),
                    row: crate::ui::action_table::by_id("app.open").map(|(index, _)| index),
                    object: Some(Recent::App(app.clone())),
                }
            }
            Target::Community(index) => {
                let Some(community) = self.sources.communities.get(index) else {
                    return Outcome::Stay;
                };
                Chosen::Community(community.id)
            }
            Target::Theme(index) => {
                let Some(theme) = self.sources.themes.get(index) else {
                    return Outcome::Stay;
                };
                Chosen::Theme {
                    id: theme.id,
                    row: None,
                }
            }
            Target::Search => Chosen::Search(Narrow::split(&self.query).1.to_owned()),
            Target::Option(_) => return Outcome::Stay,
        };
        Outcome::Run(chosen)
    }

    /// After a step opens: the theme step previews its first selection.
    fn key_preview(&mut self) -> Outcome {
        let selected = self.selected_theme();
        if selected.is_some() && selected != self.previewing {
            self.previewing = selected;
            return selected.map_or(Outcome::Stay, Outcome::Preview);
        }
        Outcome::Stay
    }

    /// Open the first step of `row`.
    fn begin(&mut self, row: usize) {
        let root = std::mem::take(&mut self.query);
        self.flow = Some(Flow {
            row,
            answers: Vec::new(),
            queries: vec![root],
        });
        self.open_step();
    }

    /// Enter in a step: its answer, then the next step or the end.
    fn answer(&mut self) -> Outcome {
        let Some((spec, arg)) = self.step() else {
            return Outcome::Stay;
        };
        let answer = match self.tentative() {
            Some(answer) => answer,
            None if arg.ask == Ask::Text && arg.skip.is_some() => Answer {
                text: String::new(),
                shown: String::new(),
            },
            None => {
                self.notice = Some(match arg.ask {
                    Ask::Text => format!("type the {} first", arg.label),
                    Ask::Duration if !self.query.trim().is_empty() => {
                        format!("«{}» is not a duration", self.query.trim())
                    }
                    _ => format!("choose the {} first", arg.label),
                });
                return Outcome::Stay;
            }
        };
        let Run::Steps(then, args) = spec.run else {
            return Outcome::Stay;
        };
        let Some(flow) = self.flow.as_mut() else {
            return Outcome::Stay;
        };
        flow.answers.push(answer);
        if flow.answers.len() < args.len() {
            let query = std::mem::take(&mut self.query);
            if let Some(flow) = self.flow.as_mut() {
                flow.queries.push(query);
            }
            let leaving_theme = arg.ask == Ask::Theme;
            self.open_step();
            if leaving_theme {
                self.previewing = None;
            }
            return self.key_preview();
        }
        let row = flow.row;
        let texts = flow
            .answers
            .iter()
            .map(|answer| answer.text.as_str())
            .collect::<Vec<_>>();
        let object = args
            .iter()
            .zip(&flow.answers)
            .find_map(|(arg, answer)| match arg.ask {
                Ask::Person => nostr::PublicKey::parse(&answer.text)
                    .ok()
                    .map(|key| Recent::Person(key.to_hex())),
                Ask::Channel => self
                    .sources
                    .channels
                    .iter()
                    .find(|channel| format!("#{}", channel.name) == answer.text)
                    .map(|channel| Recent::Channel(channel.id)),
                Ask::App => (!answer.text.is_empty()).then(|| Recent::App(answer.text.clone())),
                _ => None,
            });
        let chosen = match then {
            Then::Command(command) => Chosen::Command {
                text: command_text(command, args, &texts),
                row: Some(row),
                object,
            },
            Then::React => Chosen::React {
                emoji: texts.last().copied().unwrap_or_default().to_owned(),
                row,
            },
            Then::Theme => {
                let Some(id) = self
                    .sources
                    .themes
                    .iter()
                    .find(|theme| Some(theme.id) == texts.last().copied())
                    .map(|theme| theme.id)
                else {
                    return Outcome::Stay;
                };
                Chosen::Theme { id, row: Some(row) }
            }
        };
        Outcome::Run(chosen)
    }

    /// Backspace on an empty field: one step back, or out of the steps.
    fn back(&mut self) -> Outcome {
        let leaving_theme = self.step().is_some_and(|(_, arg)| arg.ask == Ask::Theme);
        let Some(flow) = self.flow.as_mut() else {
            return Outcome::Stay;
        };
        let query = flow.queries.pop().unwrap_or_default();
        if flow.answers.pop().is_none() {
            self.flow = None;
            self.options.clear();
            self.answers.clear();
            self.query = query;
            self.refilter();
        } else {
            self.query = query;
            self.open_step_keeping_query();
        }
        if leaving_theme {
            self.previewing = None;
            return Outcome::Restore;
        }
        self.key_preview()
    }

    fn open_step(&mut self) {
        self.query.clear();
        self.open_step_keeping_query();
    }

    /// Build the open step's answers and list them for the query.
    fn open_step_keeping_query(&mut self) {
        let Some((_, arg)) = self.step() else {
            return;
        };
        let (mut options, answers) = step_options(arg, &self.sources);
        // A person, an app or a theme chosen in this session comes first.
        for (option, answer) in options.iter_mut().zip(&answers).skip(1) {
            option.recent = self
                .recent
                .iter()
                .position(|recent| match (recent, arg.ask) {
                    (Recent::Person(pubkey), Ask::Person) => *pubkey == answer.text,
                    (Recent::App(name), Ask::App) => *name == answer.text,
                    (Recent::Theme(id), Ask::Theme) => *id == answer.text,
                    (Recent::Channel(id), Ask::Channel) => {
                        self.sources.channels.iter().any(|channel| {
                            channel.id == *id && format!("#{}", channel.name) == answer.text
                        })
                    }
                    _ => false,
                })
                .and_then(|place| u8::try_from(place).ok());
        }
        self.options = options;
        self.answers = answers;
        self.refilter();
        if arg.ask == Ask::Theme && self.query.is_empty() {
            // Start on the theme in use, as the theme picker does.
            let current = self.sources.theme.clone();
            if let Some(row) = self.rows.iter().position(|row| {
                matches!(row, Row::Item(index)
                    if self.answers.get(*index).is_some_and(|answer| answer.text == current))
            }) {
                self.selected = row;
            }
            self.previewing = Some(
                self.sources
                    .themes
                    .iter()
                    .find(|theme| theme.id == current)
                    .map_or("", |theme| theme.id),
            )
            .filter(|id| !id.is_empty());
        }
    }

    fn move_by(&mut self, delta: isize) {
        let items = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Item(_)))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        let current = items
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0);
        let last = items.len() - 1;
        let next = if delta.unsigned_abs() == 1 {
            // One step wraps, as the `Space a` menu does.
            if delta < 0 {
                current.checked_sub(1).unwrap_or(last)
            } else if current == last {
                0
            } else {
                current + 1
            }
        } else {
            current.saturating_add_signed(delta).min(last)
        };
        self.selected = items[next];
        self.notice = None;
    }

    fn refilter(&mut self) {
        self.notice = None;
        self.rows = match self.step() {
            Some((spec, arg)) => self.step_rows(spec, arg),
            None => self.root_rows(),
        };
        if !self.rows.iter().any(|row| matches!(row, Row::Item(_)))
            && !self.rows.iter().any(|row| matches!(row, Row::Hint(_)))
        {
            self.rows.push(Row::Hint("Nothing matches".into()));
        }
        self.selected = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Item(_)))
            .unwrap_or(0);
        self.offset.set(0);
    }

    fn root_rows(&mut self) -> Vec<Row> {
        let (narrow, text) = Narrow::split(&self.query);
        if self.query.trim().is_empty() {
            return self.empty.clone();
        }
        let mut rows = rank(&self.candidates, text, narrow, &mut self.matcher)
            .into_iter()
            .map(Row::Item)
            .collect::<Vec<_>>();
        // Messages are not listed: the last row searches them (C12).
        if !text.is_empty()
            && narrow.admits(Kind::Search)
            && let Some(search) = self
                .candidates
                .iter()
                .rposition(|candidate| candidate.kind == Kind::Search)
        {
            rows.push(Row::Item(search));
        }
        if rows.is_empty() {
            rows.push(Row::Hint("Nothing matches".into()));
        }
        rows
    }

    fn step_rows(&mut self, spec: &ActionSpec, arg: &Arg) -> Vec<Row> {
        let query = self.query.trim();
        let mut rows = Vec::new();
        let usage = spec.command.map(|usage| format!("usage: {usage}"));
        if arg.ask == Ask::Text {
            rows.push(Row::Hint(match arg.skip {
                Some(skip) => format!("Type the {}, or Enter for {skip}", arg.label),
                None => format!("Type the {}, then Enter", arg.label),
            }));
            if let Some(usage) = usage {
                rows.push(Row::Hint(usage));
            }
            rows.push(Row::Hint("⌫ on an empty field goes back a step".into()));
            return rows;
        }
        rows.push(Row::Header(step_header(arg).into()));
        // The typed answer (a duration or an emoji written in full) first,
        // when it is one and no offered answer is the same.
        let typed = match arg.ask {
            Ask::Duration if !query.is_empty() => {
                if query.bytes().all(|byte| byte.is_ascii_digit()) {
                    rows.push(Row::Hint(format!(
                        "{query} what? Add s, m, h, d or w: {query}m, {query}h, {query}d"
                    )));
                    None
                } else if crate::protocol::presence::parse_expiry(query, 0).is_some() {
                    Some(query.to_owned())
                } else {
                    rows.push(Row::Hint(format!(
                        "«{query}» is not a duration: a number and s, m, h, d or w (30m, 2h, 1d)"
                    )));
                    None
                }
            }
            Ask::Emoji if !query.is_ascii() => Some(query.to_owned()),
            _ => None,
        }
        .filter(|typed| !self.answers.iter().any(|answer| answer.text == *typed));
        if let Some(typed) = typed {
            self.options[0] = Candidate::object(
                Target::Option(0),
                Kind::Word,
                typed.clone(),
                "as typed".into(),
                "",
            );
            self.answers[0] = Answer {
                text: typed.clone(),
                shown: typed,
            };
            rows.push(Row::Item(0));
        }
        let text = match arg.ask {
            Ask::Person | Ask::Channel => query.trim_start_matches(['@', '#']),
            _ => query,
        };
        let listed = rank(&self.options[1..], text, Narrow::All, &mut self.matcher);
        // With nothing typed, a step lists its answers in the order given
        // (the skip first, recent emoji first).
        let mut listed = listed
            .into_iter()
            .map(|index| index + 1)
            .collect::<Vec<_>>();
        if text.is_empty() {
            listed.sort_by_key(|index| (self.options[*index].recent.unwrap_or(u8::MAX), *index));
        }
        if listed.is_empty() && rows.iter().all(|row| !matches!(row, Row::Item(_))) {
            rows.push(Row::Hint(match arg.ask {
                Ask::App if self.sources.apps.is_empty() => {
                    "No app is installed: :app add <folder> or :app new <name>".into()
                }
                Ask::Person if self.sources.people.is_empty() => {
                    "No people yet: bzz knows a person once it has seen them".into()
                }
                _ => "Nothing matches".into(),
            }));
        }
        rows.extend(listed.into_iter().map(Row::Item));
        rows
    }

    /// The number of rows that can be chosen.
    pub fn item_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, Row::Item(_)))
            .count()
    }
}

const fn step_header(arg: &Arg) -> &'static str {
    match arg.ask {
        Ask::Person => "People",
        Ask::Channel => "Channels",
        Ask::Emoji => "Emoji · recent first",
        Ask::Duration => "Expires in",
        Ask::Text => "",
        Ask::Choice(_) => "Choose one",
        Ask::Theme => "Themes · moving previews, Enter keeps, Esc puts back",
        Ask::App => "Installed apps",
    }
}

/// The answers a step offers, as rows and as what they write. The first of
/// each is the typed answer's slot.
fn step_options(arg: &Arg, sources: &Sources) -> (Vec<Candidate>, Vec<Answer>) {
    let mut options = vec![Candidate::object(
        Target::Option(0),
        Kind::Word,
        String::new(),
        String::new(),
        "",
    )];
    let mut answers = vec![Answer {
        text: String::new(),
        shown: String::new(),
    }];
    let mut offer = |kind: Kind, title: String, detail: String, words: &str, answer: Answer| {
        options.push(Candidate::object(
            Target::Option(options.len()),
            kind,
            title,
            detail,
            words,
        ));
        answers.push(answer);
    };
    let plain = |text: &str| Answer {
        text: text.to_owned(),
        shown: text.to_owned(),
    };
    if let Some(skip) = arg.skip {
        offer(
            Kind::Word,
            format!("— {skip}"),
            String::new(),
            "none skip",
            Answer {
                text: String::new(),
                shown: String::new(),
            },
        );
    }
    match arg.ask {
        Ask::Person => {
            for person in &sources.people {
                let name = sanitize::single_line(&person.name);
                let detail = if person.agent {
                    format!("{} · agent", short_key(&person.pubkey))
                } else {
                    short_key(&person.pubkey)
                };
                offer(
                    if person.agent {
                        Kind::Agent
                    } else {
                        Kind::Person
                    },
                    name.clone(),
                    detail,
                    "",
                    Answer {
                        text: person.pubkey.clone(),
                        shown: name,
                    },
                );
            }
        }
        Ask::Channel => {
            for channel in sources.channels.iter().filter(|channel| !channel.dm) {
                let name = sanitize::single_line(&channel.name);
                offer(
                    Kind::Channel,
                    name.clone(),
                    String::new(),
                    "",
                    Answer {
                        text: format!("#{}", channel.name),
                        shown: format!("#{name}"),
                    },
                );
            }
        }
        Ask::Emoji => {
            for emoji in &sources.recent_emoji {
                let words = EMOJI
                    .iter()
                    .find(|(known, _)| known == emoji)
                    .map_or("", |(_, words)| words);
                offer(
                    Kind::Emoji,
                    format!("{emoji}  {words}"),
                    "recent".into(),
                    "recent",
                    plain(emoji),
                );
            }
            for (emoji, words) in EMOJI {
                if sources.recent_emoji.iter().any(|recent| recent == emoji) {
                    continue;
                }
                offer(
                    Kind::Emoji,
                    format!("{emoji}  {words}"),
                    String::new(),
                    "",
                    plain(emoji),
                );
            }
        }
        Ask::Duration => {
            for (duration, meaning) in DURATIONS {
                offer(
                    Kind::Word,
                    (*duration).to_owned(),
                    (*meaning).to_owned(),
                    meaning,
                    plain(duration),
                );
            }
        }
        Ask::Text => {}
        Ask::Choice(choices) => {
            for (word, meaning) in choices {
                let (title, detail) = if word.is_empty() {
                    ((*meaning).to_owned(), String::new())
                } else {
                    ((*word).to_owned(), (*meaning).to_owned())
                };
                offer(
                    Kind::Word,
                    title,
                    detail,
                    meaning,
                    Answer {
                        text: (*word).to_owned(),
                        shown: if word.is_empty() {
                            (*meaning).to_owned()
                        } else {
                            (*word).to_owned()
                        },
                    },
                );
            }
        }
        Ask::Theme => {
            for theme in &sources.themes {
                offer(
                    Kind::Theme,
                    theme.name.to_owned(),
                    theme.id.to_owned(),
                    theme.id,
                    Answer {
                        text: theme.id.to_owned(),
                        shown: theme.name.to_owned(),
                    },
                );
            }
        }
        Ask::App => {
            for app in &sources.apps {
                let name = sanitize::single_line(app);
                offer(
                    Kind::App,
                    name.clone(),
                    String::new(),
                    "",
                    Answer {
                        text: app.clone(),
                        shown: name,
                    },
                );
            }
        }
    }
    (options, answers)
}

fn empty_rows(candidates: &[Candidate], facts: &Facts) -> Vec<Row> {
    let mut rows = Vec::new();
    let action_row = |action: UiAction| {
        candidates.iter().position(|candidate| {
            let Target::Action(index) = candidate.target else {
                return false;
            };
            TABLE.get(index).is_some_and(|spec| {
                (spec.run == Run::Ui(action)
                    || (matches!(spec.run, Run::Steps(..)) && spec.key_of == Some(action)))
                    && candidate.is_ready()
                    && !(facts.actions.route == Route::Inbox && spec.place == Where::Workspace)
            })
        })
    };
    let context = derive(facts.actions)
        .into_iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| action_row(entry.action))
        .fold(Vec::new(), |mut found, index| {
            if !found.contains(&index) {
                found.push(index);
            }
            found
        });
    if !context.is_empty() {
        let title = match (facts.actions.route, facts.actions.focus) {
            (Route::Inbox, _) => "For the selected Inbox item",
            (_, FocusSurface::Channels) => "For the selected channel",
            (_, FocusSurface::Communities) => "For the selected community",
            _ if facts.actions.has_selected_event => "For the selected message",
            _ => "For this channel",
        };
        rows.push(Row::Header(title.into()));
        rows.extend(context.into_iter().take(CONTEXT_SHOWN).map(Row::Item));
    }
    let mut recent = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| candidate.recent.map(|place| (place, index)))
        .filter(|(_, index)| !rows.contains(&Row::Item(*index)))
        .collect::<Vec<_>>();
    recent.sort_unstable();
    if !recent.is_empty() {
        rows.push(Row::Header("Recent".into()));
        rows.extend(
            recent
                .into_iter()
                .take(RECENT_SHOWN)
                .map(|(_, index)| Row::Item(index)),
        );
    }
    let unread = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            matches!(candidate.kind, Kind::Channel | Kind::Dm) && candidate.detail == "unread"
        })
        .map(|(index, _)| index)
        .filter(|index| !rows.contains(&Row::Item(*index)))
        .take(UNREAD_SHOWN)
        .collect::<Vec<_>>();
    if !unread.is_empty() {
        rows.push(Row::Header("Go to · unread".into()));
        rows.extend(unread.into_iter().map(Row::Item));
    }
    let count = |kinds: &[Kind]| {
        candidates
            .iter()
            .filter(|candidate| kinds.contains(&candidate.kind))
            .count()
    };
    let counted = |kinds: &[Kind], one: &str, many: &str| {
        let count = count(kinds);
        (count > 0).then(|| format!("{count} {}", if count == 1 { one } else { many }))
    };
    let parts = [
        counted(&[Kind::Action], "action", "actions"),
        counted(&[Kind::Channel], "channel", "channels"),
        counted(&[Kind::Dm], "DM", "DMs"),
        counted(&[Kind::Person, Kind::Agent], "person", "people"),
        counted(&[Kind::App], "app", "apps"),
        counted(&[Kind::Theme], "theme", "themes"),
        counted(&[Kind::Shell], "shell command", "shell commands"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    rows.push(Row::Hint(format!("Type to search {}", parts.join(", "))));
    rows
}

/// Remember `recent` as the newest choices of this session.
pub fn remember(kept: &mut Vec<Recent>, recent: Vec<Recent>) {
    for item in recent.into_iter().rev() {
        kept.retain(|existing| *existing != item);
        kept.insert(0, item);
    }
    kept.truncate(RECENT_KEPT);
}

/// Sort `channels` as the palette lists them: unread first, then the most
/// recent.
pub fn order_channels(channels: &mut [PaletteChannel]) {
    channels.sort_by(|left, right| {
        right
            .unread
            .cmp(&left.unread)
            .then(right.last_event_at.cmp(&left.last_event_at))
            .then(left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
}

/// The panel's place: the whole width less a margin up to 99 columns, then
/// three fifths of the window between 70 and 100 columns; up to 26 rows,
/// a quarter of the way down; the status line stays visible.
pub fn area(screen: Rect, rows: usize) -> Rect {
    let width = if screen.width < 100 {
        screen.width.saturating_sub(2)
    } else {
        (screen.width.saturating_mul(3) / 5).clamp(70, 100)
    };
    let room = screen.height.saturating_sub(1);
    // As tall as its rows (a frame, the search line and its rule around
    // them), never under 12 rows so the panel does not jump at each key.
    let wanted = u16::try_from(rows.saturating_add(4))
        .unwrap_or(u16::MAX)
        .clamp(12, 26);
    let height = wanted.min(room.saturating_sub(2)).max(6.min(room));
    let x = screen.x + screen.width.saturating_sub(width) / 2;
    let y = screen.y + room.saturating_sub(height) / 4;
    Rect::new(x, y, width, height)
}

pub fn render(frame: &mut Frame<'_>, screen: Rect, palette: &Palette, theme: &Theme) {
    let popup = area(screen, palette.rows.len());
    if popup.width < 20 || popup.height < 5 {
        return;
    }
    frame.render_widget(Clear, popup);
    let inner_width = usize::from(popup.width.saturating_sub(2));
    let footer = footer(palette, theme, inner_width.saturating_sub(2));
    let title = palette
        .breadcrumb()
        .map_or_else(|| " Actions ".to_owned(), |crumb| format!(" {crumb} "));
    let block = Block::bordered()
        .border_type(theme.border_type(BorderSurface::Modal))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(truncate_cells(&title, inner_width.saturating_sub(6)))
        .title_top(
            Line::from(Span::styled(" Esc ", theme.style(HighlightGroup::Muted))).right_aligned(),
        )
        .title_bottom(footer);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::with_capacity(usize::from(inner.height));
    lines.push(query_line(palette, theme, inner_width));
    lines.push(Line::from(Span::styled(
        "─".repeat(inner_width),
        theme.style(HighlightGroup::Muted),
    )));
    let visible = usize::from(inner.height).saturating_sub(2);
    palette.page.set(visible.max(1));
    let mut offset = palette.offset.get();
    if palette.selected < offset {
        offset = palette.selected;
    } else if palette.selected >= offset.saturating_add(visible) {
        offset = palette.selected.saturating_add(1).saturating_sub(visible);
    }
    // Keep a header above the first item of a group when there is room.
    if offset > 0
        && palette.selected == offset
        && matches!(palette.rows.get(offset - 1), Some(Row::Header(_)))
    {
        offset -= 1;
    }
    palette.offset.set(offset);
    let text = if palette.flow.is_some() {
        palette.query.trim().trim_start_matches(['@', '#'])
    } else {
        Narrow::split(&palette.query).1
    };
    let mut matcher = palette.matcher.clone();
    let pattern = (!text.is_empty())
        .then(|| Pattern::parse(text, CaseMatching::Ignore, Normalization::Smart));
    for (index, row) in palette.rows.iter().enumerate().skip(offset).take(visible) {
        lines.push(match row {
            Row::Header(title) => Line::from(Span::styled(
                truncate_cells(title, inner_width.saturating_sub(1)),
                theme
                    .style(HighlightGroup::Title)
                    .add_modifier(Modifier::BOLD),
            )),
            Row::Hint(text) => Line::from(Span::styled(
                truncate_cells(text, inner_width.saturating_sub(1)),
                theme.style(HighlightGroup::Muted),
            )),
            Row::Item(candidate) => {
                let Some(candidate) = palette.candidate(*candidate) else {
                    continue;
                };
                item_line(
                    candidate,
                    text,
                    index == palette.selected,
                    pattern.as_ref(),
                    &mut matcher,
                    theme,
                    inner_width,
                )
            }
        });
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme.style(HighlightGroup::Normal)),
        inner,
    );
}

fn query_line<'a>(palette: &Palette, theme: &Theme, width: usize) -> Line<'a> {
    let prompt = Span::styled("❯ ", theme.style(HighlightGroup::SelectionMarker));
    if palette.query.is_empty() {
        let placeholder = match palette.step() {
            None => "Search actions, channels, people, apps, themes…".to_owned(),
            Some((_, arg)) => match arg.ask {
                Ask::Person => "Search people…".to_owned(),
                Ask::Channel => "Search channels…".to_owned(),
                Ask::Emoji => "Search emoji, or type one…".to_owned(),
                Ask::Duration => "30m, 2h, 1d…".to_owned(),
                Ask::Text => format!("Type the {}…", arg.label),
                Ask::Choice(_) => format!("Choose the {}…", arg.label),
                Ask::Theme => "Search themes…".to_owned(),
                Ask::App => "Search apps…".to_owned(),
            },
        };
        return Line::from(vec![
            prompt,
            Span::styled("▏", theme.style(HighlightGroup::SelectionMarker)),
            Span::styled(
                truncate_cells(&placeholder, width.saturating_sub(4)),
                theme.style(HighlightGroup::Muted),
            ),
        ]);
    }
    let shown = sanitize::single_line(&palette.query);
    // The end of a long query stays in view: that is where the cursor is.
    let room = width.saturating_sub(4);
    let shown = if shown.width() > room {
        let mut tail = String::new();
        for character in shown.chars().rev() {
            if tail.width() + 2 > room {
                break;
            }
            tail.insert(0, character);
        }
        format!("…{tail}")
    } else {
        shown
    };
    Line::from(vec![
        prompt,
        Span::styled(shown, theme.style(HighlightGroup::Normal)),
        Span::styled("▏", theme.style(HighlightGroup::SelectionMarker)),
    ])
}

fn item_line<'a>(
    candidate: &Candidate,
    query: &str,
    selected: bool,
    pattern: Option<&Pattern>,
    matcher: &mut Matcher,
    theme: &Theme,
    width: usize,
) -> Line<'a> {
    let ready = candidate.is_ready();
    let base = if ready {
        theme.style(HighlightGroup::Normal)
    } else {
        theme.style(HighlightGroup::Muted)
    };
    let row_style = if selected {
        theme.style(HighlightGroup::SelectedRow)
    } else {
        Style::default()
    };
    // The marker is a glyph, so the selection reads without colour.
    let mut spans = vec![
        Span::styled(
            if selected { "▌" } else { " " },
            theme.style(HighlightGroup::SelectionMarker),
        ),
        Span::styled(
            format!("{} ", candidate.kind.glyph()),
            theme.style(HighlightGroup::Muted),
        ),
    ];
    let used_left = 3;
    // Right: the key, the reason a row cannot run, or the shell command.
    let right = match &candidate.status {
        Status::Ready => candidate.key.clone(),
        Status::Unavailable(reason) if width >= 90 => format!("✕ {reason}"),
        Status::Unavailable(_) => "✕".into(),
        Status::Shell(_) => "✕ shell only".into(),
    };
    let right_room = (width * 45 / 100).max(1);
    let right = truncate_cells(&right, right_room);
    let right_width = right.width();
    let title_room = width
        .saturating_sub(used_left)
        .saturating_sub(right_width)
        .saturating_sub(2);
    let title = if candidate.kind == Kind::Search {
        format!(
            "Search messages for «{}»",
            sanitize::single_line(query.trim())
        )
    } else {
        candidate.title.clone()
    };
    let title = truncate_cells(&title, title_room);
    let title_width = title.width();
    let mut highlighted = Vec::new();
    if let Some(pattern) = pattern
        && candidate.kind != Kind::Search
    {
        let mut buffer = Vec::new();
        let haystack = nucleo_matcher::Utf32Str::new(&title, &mut buffer);
        let mut indices = Vec::new();
        if pattern.indices(haystack, matcher, &mut indices).is_some() {
            indices.sort_unstable();
            indices.dedup();
            highlighted = indices;
        }
    }
    if highlighted.is_empty() {
        spans.push(Span::styled(title, base));
    } else {
        let hit = base
            .patch(theme.style(HighlightGroup::Emphasis))
            .add_modifier(Modifier::BOLD);
        let mut run = String::new();
        let mut run_hit = false;
        for (position, character) in title.chars().enumerate() {
            let is_hit = u32::try_from(position)
                .is_ok_and(|position| highlighted.binary_search(&position).is_ok());
            if is_hit != run_hit && !run.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut run),
                    if run_hit { hit } else { base },
                ));
            }
            run_hit = is_hit;
            run.push(character);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, if run_hit { hit } else { base }));
        }
    }
    let mut used = used_left + title_width;
    // The `:` form, a short key or a meaning, dim, when the whole of it fits.
    let detail = &candidate.detail;
    if !detail.is_empty()
        && candidate.kind != Kind::Shell
        && used + 2 + detail.width() + 2 + right_width <= width
    {
        spans.push(Span::styled(
            format!("  {detail}"),
            theme.style(HighlightGroup::Muted),
        ));
        used += 2 + detail.width();
    }
    let gap = width
        .saturating_sub(used)
        .saturating_sub(right_width)
        .saturating_sub(1);
    spans.push(Span::raw(" ".repeat(gap)));
    let right_style = if ready {
        theme
            .style(HighlightGroup::Emphasis)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.style(HighlightGroup::Muted)
    };
    spans.push(Span::styled(right, right_style));
    Line::from(spans).style(row_style)
}

/// The footer's text and colour: a notice, the reason a row cannot run, the
/// `:` line a step builds, or the keys.
pub fn footer_text(palette: &Palette) -> (String, HighlightGroup) {
    if let Some(notice) = palette.notice() {
        return (notice.to_owned(), HighlightGroup::Error);
    }
    if let Some(building) = palette.building() {
        return (building, HighlightGroup::Normal);
    }
    match palette.selected_candidate() {
        Some(candidate) => match &candidate.status {
            Status::Unavailable(reason) => (format!("can't now: {reason}"), HighlightGroup::Error),
            Status::Shell(command) => (
                format!("only in the shell: {command} · Enter copies it"),
                HighlightGroup::Warning,
            ),
            Status::Ready => (keys_hint(palette, candidate), HighlightGroup::Muted),
        },
        None => (
            format!(
                "↵ run · ↑↓ move · Esc close · > # @ $ / narrow · {}",
                palette.item_count()
            ),
            HighlightGroup::Muted,
        ),
    }
}

fn footer<'a>(palette: &Palette, theme: &Theme, width: usize) -> Line<'a> {
    let (text, group) = footer_text(palette);
    Line::from(Span::styled(
        format!(" {} ", truncate_cells(&text, width)),
        theme.style(group),
    ))
}

fn keys_hint(palette: &Palette, candidate: &Candidate) -> String {
    let count = palette.item_count();
    match candidate.target {
        Target::Person(_) => format!("↵ profile · Ctrl+D DM · ↑↓ move · Esc close · {count}"),
        Target::Action(index) => match TABLE.get(index).map(|spec| spec.run) {
            Some(Run::Steps(_, args)) => format!(
                "↵ asks for the {} · ↑↓ move · Esc close · {count}",
                args.first().map_or("", |arg| arg.label)
            ),
            _ => format!("↵ run · ↑↓ move · Esc close · > # @ $ / narrow · {count}"),
        },
        _ => format!("↵ run · ↑↓ move · Esc close · > # @ $ / narrow · {count}"),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use uuid::Uuid;

    use super::{
        Chosen, Facts, Kind, Narrow, Outcome, Palette, PaletteChannel, PaletteCommunity,
        PalettePerson, Recent, Row, Sources, Status, candidates, footer_text, key_label, remember,
    };
    use crate::ui::{
        action_table::{TABLE, by_id},
        actions::ActionContext,
        keymap::{KeyMap, KeyScope, UiAction},
        state::{FocusSurface, Route},
        theme::{Theme, ThemeRegistry},
    };

    const SCOUT: &str = "852b14d9852b14d9852b14d9852b14d9852b14d9852b14d9852b14d98520a120";

    fn facts() -> Facts {
        Facts {
            actions: ActionContext {
                route: Route::Workspace,
                focus: FocusSurface::Timeline,
                has_inbox_selection: false,
                inbox_has_context: false,
                inbox_can_reply: false,
                inbox_visible_count: 0,
                has_channel: true,
                has_selected_event: true,
                selected_event_is_own: false,
                selected_event_has_media: false,
                context_open: false,
                can_publish: true,
                delete_refused: None,
            },
            channel_is_dm: false,
            huddle_running: false,
            in_call: false,
            app_here: false,
            artifact_selected: true,
            communities: 2,
        }
    }

    fn channels() -> Vec<PaletteChannel> {
        vec![
            PaletteChannel {
                id: Uuid::from_u128(1),
                name: "t9-general".into(),
                dm: false,
                unread: true,
                last_event_at: 10,
            },
            PaletteChannel {
                id: Uuid::from_u128(2),
                name: "t14-names".into(),
                dm: false,
                unread: false,
                last_event_at: 5,
            },
            PaletteChannel {
                id: Uuid::from_u128(3),
                name: "Scout".into(),
                dm: true,
                unread: false,
                last_event_at: 1,
            },
        ]
    }

    fn sources() -> Sources {
        Sources {
            channels: channels(),
            people: vec![
                PalettePerson {
                    pubkey: "a".repeat(64),
                    name: "Ada".into(),
                    agent: false,
                },
                PalettePerson {
                    pubkey: SCOUT.into(),
                    name: "Scout".into(),
                    agent: true,
                },
            ],
            apps: vec!["board".into()],
            communities: vec![PaletteCommunity {
                id: Uuid::from_u128(40),
                name: "harness".into(),
            }],
            themes: ThemeRegistry::entries().collect(),
            theme: "bzz".into(),
            recent_emoji: vec!["🚀".into()],
        }
    }

    fn open(facts: Facts, sources: Sources, recent: &[Recent]) -> Palette {
        let keymap = KeyMap::builtin();
        let all = candidates(&keymap, &facts, &sources);
        Palette::open(
            all,
            sources,
            &facts,
            recent,
            (Route::Workspace, FocusSurface::Timeline),
        )
    }

    fn palette(facts: Facts) -> Palette {
        open(facts, sources(), &[])
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(palette: &mut Palette, text: &str) {
        for character in text.chars() {
            palette.key(key(KeyCode::Char(character)));
        }
    }

    fn npub(hex: &str) -> String {
        super::npub(hex)
    }

    #[test]
    fn thr_puts_open_thread_first() {
        let mut palette = palette(facts());
        typed(&mut palette, "thr");
        assert_eq!(palette.item_titles()[0], "Open thread");
    }

    #[test]
    fn a_match_scattered_over_words_is_not_listed() {
        let mut palette = palette(facts());
        typed(&mut palette, "thr");
        let titles = palette.item_titles();
        assert!(!titles.contains(&"Widen side pane"), "{titles:?}");
        assert!(titles.contains(&"Focus thread"), "{titles:?}");
    }

    #[test]
    fn channels_that_arrive_while_it_is_open_are_listed() {
        let keymap = KeyMap::builtin();
        let facts = facts();
        let mut palette = open(facts, Sources::default(), &[]);
        typed(&mut palette, "#t9");
        assert!(palette.item_titles().is_empty());
        let sources = sources();
        palette.replace_candidates(candidates(&keymap, &facts, &sources), sources, &facts, &[]);
        assert_eq!(palette.query(), "#t9");
        assert_eq!(palette.item_titles(), vec!["t9-general"]);
    }

    #[test]
    fn q_puts_quit_first_and_enter_runs_it() {
        let mut palette = palette(facts());
        typed(&mut palette, "q");
        assert_eq!(palette.item_titles()[0], "Quit bzz");
        let (index, _) = by_id("app.quit").unwrap();
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Action(index))
        );
    }

    #[test]
    fn theme_and_status_find_their_rows_first() {
        let mut palette = palette(facts());
        typed(&mut palette, "theme");
        assert_eq!(palette.item_titles()[0], "Change theme…");
        palette.set_query("status");
        assert_eq!(palette.item_titles()[0], "Set my status…");
    }

    #[test]
    fn delete_on_another_persons_message_says_why_and_stays() {
        let mut palette = palette(facts());
        typed(&mut palette, "delete");
        assert_eq!(palette.item_titles()[0], "Delete my message");
        assert_eq!(palette.key(key(KeyCode::Enter)), Outcome::Stay);
        assert_eq!(
            palette.notice(),
            Some("can't now: only your own message can be deleted")
        );
    }

    #[test]
    fn the_signers_refusal_is_the_reason_when_it_refuses() {
        let mut facts = facts();
        facts.actions.selected_event_is_own = true;
        facts.actions.delete_refused = Some("Buzz does not let other apps do this; do it in Buzz");
        let mut palette = palette(facts);
        typed(&mut palette, "delete");
        palette.key(key(KeyCode::Enter));
        assert_eq!(
            palette.notice(),
            Some("can't now: Buzz does not let other apps do this; do it in Buzz")
        );
    }

    #[test]
    fn prefixes_narrow_the_kinds() {
        let mut palette = palette(facts());
        typed(&mut palette, "#gen");
        let first = palette.selected_candidate().unwrap();
        assert_eq!(first.kind, Kind::Channel);
        assert_eq!(first.target, super::Target::Channel(Uuid::from_u128(1)));
        palette.set_query("@sc");
        // The DM with Scout and Scout the agent.
        assert_eq!(palette.item_titles(), vec!["Scout", "Scout"]);
        palette.set_query(">gen");
        assert!(
            palette
                .rows()
                .iter()
                .all(|row| !matches!(row, Row::Item(index)
                    if !matches!(palette.candidate(*index).unwrap().kind, Kind::Action | Kind::Shell)))
        );
        palette.set_query("$back");
        assert!(
            palette
                .rows()
                .iter()
                .all(|row| !matches!(row, Row::Item(index)
                    if palette.candidate(*index).unwrap().kind != Kind::Shell))
        );
        palette.set_query("/launch");
        assert_eq!(palette.item_titles(), vec!["Search messages for"]);
        assert_eq!(Narrow::split(" # t9"), (Narrow::Channels, "t9"));
    }

    #[test]
    fn channels_and_actions_share_one_list() {
        let mut palette = palette(facts());
        typed(&mut palette, "t9-gen");
        assert_eq!(palette.item_titles()[0], "t9-general");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Channel(Uuid::from_u128(1)))
        );
    }

    #[test]
    fn a_shell_row_asks_to_copy_its_command() {
        let mut palette = palette(facts());
        typed(&mut palette, "community add");
        let first = palette.selected_candidate().unwrap();
        assert_eq!(first.title, "Add a community");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Copy("bzz community add <url>")
        );
    }

    #[test]
    fn start_a_dm_asks_for_a_person_and_builds_the_dm() {
        let mut palette = palette(facts());
        typed(&mut palette, "start a dm");
        assert_eq!(palette.item_titles()[0], "Start a DM");
        assert_eq!(palette.key(key(KeyCode::Enter)), Outcome::Stay);
        assert_eq!(palette.breadcrumb().as_deref(), Some("Start a DM › person"));
        assert_eq!(palette.item_titles(), vec!["Ada", "Scout"]);
        typed(&mut palette, "sco");
        assert_eq!(footer_text(&palette).0, format!(":dm {}", npub(SCOUT)));
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Command {
                text: format!("dm {}", npub(SCOUT)),
                row: by_id("dm.new").map(|(index, _)| index),
                object: Some(Recent::Person(SCOUT.into())),
            })
        );
    }

    #[test]
    fn status_asks_text_then_expiry_and_the_footer_teaches_the_command() {
        let mut palette = palette(facts());
        typed(&mut palette, "status");
        palette.key(key(KeyCode::Enter));
        assert_eq!(
            palette.breadcrumb().as_deref(),
            Some("Set my status › status")
        );
        assert_eq!(footer_text(&palette).0, ":status <status>");
        // The usage of the command is under the field.
        assert!(palette.rows().contains(&Row::Hint(
            "usage: :status [--expires N[smhdw]] <text>".into()
        )));
        // A free-text step does not go on with nothing typed.
        assert_eq!(palette.key(key(KeyCode::Enter)), Outcome::Stay);
        assert_eq!(palette.notice(), Some("type the status first"));
        typed(&mut palette, "In a meeting");
        assert_eq!(footer_text(&palette).0, ":status In a meeting");
        palette.key(key(KeyCode::Enter));
        assert_eq!(
            palette.breadcrumb().as_deref(),
            Some("Set my status › In a meeting › expires")
        );
        // "no expiry" is first: Enter now would set the status alone.
        assert_eq!(palette.item_titles()[0], "— no expiry");
        assert_eq!(footer_text(&palette).0, ":status In a meeting");
        typed(&mut palette, "45x");
        assert!(palette.rows().iter().any(
            |row| matches!(row, Row::Hint(text) if text.starts_with("«45x» is not a duration"))
        ));
        assert_eq!(palette.key(key(KeyCode::Enter)), Outcome::Stay);
        palette.set_query("45m");
        assert_eq!(palette.item_titles()[0], "45m");
        assert_eq!(
            footer_text(&palette).0,
            ":status --expires 45m In a meeting"
        );
        palette.set_query("2h");
        assert_eq!(palette.item_titles()[0], "2h");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Command {
                text: "status --expires 2h In a meeting".into(),
                row: by_id("status.set").map(|(index, _)| index),
                object: None,
            })
        );
    }

    #[test]
    fn backspace_on_an_empty_field_goes_back_a_step() {
        let mut palette = palette(facts());
        typed(&mut palette, "status");
        palette.key(key(KeyCode::Enter));
        typed(&mut palette, "lunch");
        palette.key(key(KeyCode::Enter));
        assert_eq!(palette.key(key(KeyCode::Backspace)), Outcome::Stay);
        assert_eq!(
            palette.breadcrumb().as_deref(),
            Some("Set my status › status")
        );
        assert_eq!(palette.query(), "lunch");
        palette.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        palette.key(key(KeyCode::Backspace));
        assert_eq!(palette.breadcrumb(), None);
        assert_eq!(palette.query(), "status");
        assert_eq!(palette.item_titles()[0], "Set my status…");
    }

    #[test]
    fn a_step_lists_this_sessions_choices_first() {
        let mut palette = open(facts(), sources(), &[Recent::Person(SCOUT.into())]);
        typed(&mut palette, "start a dm");
        palette.key(key(KeyCode::Enter));
        assert_eq!(palette.item_titles(), vec!["Scout", "Ada"]);
    }

    #[test]
    fn the_theme_step_previews_as_it_moves_and_backspace_puts_back() {
        let mut palette = palette(facts());
        typed(&mut palette, "theme");
        // Opening the step starts on the theme in use: nothing to preview.
        assert_eq!(palette.key(key(KeyCode::Enter)), Outcome::Stay);
        assert_eq!(
            palette.breadcrumb().as_deref(),
            Some("Change theme › theme")
        );
        let first = palette.selected_candidate().unwrap().detail.clone();
        assert_eq!(first, "bzz");
        let Outcome::Preview(next) = palette.key(key(KeyCode::Down)) else {
            panic!("moving previews");
        };
        assert_ne!(next, "bzz");
        typed(&mut palette, "nord");
        assert_eq!(palette.selected_candidate().unwrap().detail, "nord");
        palette.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(palette.key(key(KeyCode::Backspace)), Outcome::Restore);
        assert_eq!(palette.breadcrumb(), None);
    }

    #[test]
    fn react_asks_for_an_emoji_with_recent_ones_first() {
        let mut palette = palette(facts());
        typed(&mut palette, "react to");
        palette.key(key(KeyCode::Enter));
        assert_eq!(
            palette.breadcrumb().as_deref(),
            Some("React to message › emoji")
        );
        let titles = palette.item_titles();
        assert!(titles[0].starts_with("🚀"), "{titles:?}");
        // The reaction picker's eight are offered.
        for emoji in crate::ui::reaction_picker::REACTIONS {
            assert!(
                titles.iter().any(|title| title.starts_with(emoji)),
                "{emoji}"
            );
        }
        typed(&mut palette, "🦀");
        assert_eq!(palette.item_titles()[0], "🦀");
        palette.set_query("thumbs");
        let row = by_id("msg.react").unwrap().0;
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::React {
                emoji: "👍".into(),
                row
            })
        );
    }

    #[test]
    fn a_huddle_reaction_and_an_artifact_move_build_their_commands() {
        let mut huddle = facts();
        huddle.huddle_running = true;
        let mut palette = open(huddle, sources(), &[]);
        typed(&mut palette, "react in huddle");
        palette.key(key(KeyCode::Enter));
        typed(&mut palette, "party");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Command {
                text: "huddle react 🎉".into(),
                row: by_id("huddle.react").map(|(index, _)| index),
                object: None,
            })
        );
        let mut palette = self::palette(facts());
        typed(&mut palette, "move artifact");
        palette.key(key(KeyCode::Enter));
        assert_eq!(palette.item_titles(), vec!["t9-general", "t14-names"]);
        typed(&mut palette, "#t14");
        let Outcome::Run(Chosen::Command { text, .. }) = palette.key(key(KeyCode::Enter)) else {
            panic!("a channel answers the step");
        };
        assert_eq!(text, "artifact move #t14-names");
    }

    #[test]
    fn a_choice_and_an_optional_step() {
        let mut palette = palette(facts());
        typed(&mut palette, "channel group sort");
        palette.key(key(KeyCode::Enter));
        assert_eq!(palette.item_titles(), vec!["alpha", "recent", "default"]);
        palette.key(key(KeyCode::Down));
        assert_eq!(footer_text(&palette).0, ":sort recent");
        let mut palette = self::palette(facts());
        typed(&mut palette, "report");
        palette.key(key(KeyCode::Enter));
        assert_eq!(palette.item_titles()[0], "the selected message");
        palette.key(key(KeyCode::Enter));
        typed(&mut palette, "spam");
        palette.key(key(KeyCode::Enter));
        assert_eq!(footer_text(&palette).0, ":report spam");
        let Outcome::Run(Chosen::Command { text, .. }) = palette.key(key(KeyCode::Enter)) else {
            panic!("Enter with no note runs the report");
        };
        assert_eq!(text, "report spam");
    }

    #[test]
    fn people_apps_communities_themes_and_search_are_found() {
        let mut palette = palette(facts());
        typed(&mut palette, "ada");
        assert_eq!(palette.item_titles()[0], "Ada");
        assert!(footer_text(&palette).0.starts_with("↵ profile · Ctrl+D DM"));
        let Outcome::Run(Chosen::Command { text, .. }) =
            palette.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
        else {
            panic!("Ctrl+D starts a DM");
        };
        assert_eq!(text, format!("dm {}", npub(&"a".repeat(64))));
        palette.set_query("board");
        assert_eq!(palette.item_titles()[0], "Open board here");
        let Outcome::Run(Chosen::Command { text, .. }) = palette.key(key(KeyCode::Enter)) else {
            panic!("an app opens here");
        };
        assert_eq!(text, "app open board");
        palette.set_query("harness");
        assert_eq!(palette.item_titles()[0], "Switch to harness");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Community(Uuid::from_u128(40)))
        );
        palette.set_query("nord");
        assert_eq!(palette.item_titles()[0], "Nord");
        assert!(matches!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Chosen::Theme { id: "nord", .. })
        ));
        palette.set_query("scout");
        let kinds = palette
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Item(index) => Some(palette.candidate(*index).unwrap().kind),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(kinds.contains(&Kind::Agent), "{kinds:?}");
        assert_eq!(kinds.last(), Some(&Kind::Search));
    }

    #[test]
    fn recent_choices_rank_first() {
        let mut palette = open(facts(), sources(), &[]);
        typed(&mut palette, "t");
        let before = palette.item_titles()[0].to_owned();
        assert_ne!(before, "t14-names");
        let mut palette = open(facts(), sources(), &[Recent::Channel(Uuid::from_u128(2))]);
        typed(&mut palette, "t");
        assert_eq!(palette.item_titles()[0], "t14-names");
        // And the empty palette offers it under "Recent".
        let palette = open(
            facts(),
            sources(),
            &[
                Recent::Person("a".repeat(64)),
                Recent::Action("huddle.panel"),
            ],
        );
        let rows = palette.rows();
        let recent = rows
            .iter()
            .position(|row| *row == Row::Header("Recent".into()))
            .unwrap();
        let next = |offset: usize| match &rows[recent + offset] {
            Row::Item(index) => palette.candidate(*index).unwrap().title.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(next(1), "Ada");
        assert_eq!(next(2), "Huddle panel");
    }

    #[test]
    fn the_empty_state_offers_the_selection_recent_and_unread() {
        let palette = open(facts(), sources(), &[Recent::Action("huddle.panel")]);
        let headers = palette
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Header(title) => Some(title.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            headers,
            vec!["For the selected message", "Recent", "Go to · unread"]
        );
        let titles = palette.item_titles();
        assert!(titles.contains(&"React to message"));
        assert!(titles.contains(&"Open thread"));
        assert!(titles.contains(&"Huddle panel"));
        assert!(titles.contains(&"t9-general"));
        assert!(!titles.contains(&"t14-names"));
        assert!(!titles.contains(&"Delete my message"));
        assert!(matches!(palette.rows().last(), Some(Row::Hint(_))));
    }

    #[test]
    fn keys_move_and_wrap_and_esc_closes() {
        let mut palette = palette(facts());
        typed(&mut palette, "thr");
        let first = palette.selected_candidate().unwrap().title.clone();
        palette.key(key(KeyCode::Down));
        palette.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        palette.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        palette.key(key(KeyCode::Up));
        assert_eq!(palette.selected_candidate().unwrap().title, first);
        palette.key(key(KeyCode::Up));
        assert_ne!(palette.selected_candidate().unwrap().title, first);
        palette.key(key(KeyCode::PageDown));
        for _ in 0..palette.item_count() {
            palette.key(key(KeyCode::PageUp));
        }
        assert_eq!(palette.selected_candidate().unwrap().title, first);
        assert_eq!(palette.key(key(KeyCode::Esc)), Outcome::Close);
    }

    #[test]
    fn the_shortcut_comes_from_the_live_key_map() {
        let keymap = KeyMap::builtin();
        assert_eq!(
            key_label(&keymap, KeyScope::Workspace, UiAction::OpenPalette),
            "Ctrl+K"
        );
        assert_eq!(
            key_label(&keymap, KeyScope::Workspace, UiAction::JumpBottom),
            "G"
        );
        assert_eq!(
            key_label(&keymap, KeyScope::Workspace, UiAction::ViewProfile),
            "Space p"
        );
        let all = candidates(&keymap, &facts(), &Sources::default());
        let thread = all.iter().find(|row| row.title == "Open thread").unwrap();
        assert_eq!(thread.key, "Enter");
        let quit = all.iter().find(|row| row.title == "Quit bzz").unwrap();
        assert_eq!(quit.key, "q");
        let react = all
            .iter()
            .find(|row| row.title == "React to message")
            .unwrap();
        assert_eq!(react.key, "r");
    }

    #[test]
    fn inbox_rows_wait_for_the_inbox_and_workspace_rows_for_the_workspace() {
        let all = candidates(&KeyMap::builtin(), &facts(), &Sources::default());
        let mark_read = all.iter().find(|row| row.title == "Mark read").unwrap();
        assert_eq!(
            mark_read.status,
            Status::Unavailable("open the Inbox first (Space n)".into())
        );
        let mut inbox = facts();
        inbox.actions.route = Route::Inbox;
        inbox.actions.focus = FocusSurface::InboxList;
        let all = candidates(&KeyMap::builtin(), &inbox, &Sources::default());
        let react = all
            .iter()
            .find(|row| row.title == "React to message")
            .unwrap();
        assert_eq!(
            react.status,
            Status::Unavailable("leave the Inbox first (Esc)".into())
        );
    }

    #[test]
    fn recent_keeps_the_newest_first_without_repeats() {
        let mut recent = Vec::new();
        remember(&mut recent, vec![Recent::Action("a")]);
        remember(&mut recent, vec![Recent::Action("b")]);
        remember(
            &mut recent,
            vec![Recent::Action("a"), Recent::App("x".into())],
        );
        assert_eq!(
            recent,
            vec![
                Recent::Action("a"),
                Recent::App("x".into()),
                Recent::Action("b")
            ]
        );
    }

    #[test]
    fn writing_rows_are_not_listed() {
        let all = candidates(&KeyMap::builtin(), &facts(), &Sources::default());
        let listed = TABLE.iter().filter(|spec| spec.listed()).count();
        // And the search row.
        assert_eq!(all.len(), listed + 1);
        assert!(!all.iter().any(|row| row.title == "Delete to line end"));
    }

    fn screen(palette: &Palette, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| super::render(frame, frame.area(), palette, &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn it_reads_at_80x24_and_200x50() {
        let mut palette = palette(facts());
        let empty = screen(&palette, 80, 24);
        assert!(empty.contains("Actions"), "{empty}");
        assert!(empty.contains("For the selected message"), "{empty}");
        assert!(empty.contains("Type to search"), "{empty}");
        typed(&mut palette, "thr");
        for (width, height) in [(80, 24), (200, 50)] {
            let text = screen(&palette, width, height);
            assert!(text.contains("▌· Open thread"), "{text}");
            assert!(text.contains("Enter"), "{text}");
            assert!(text.contains("↵ run"), "{text}");
        }
        palette.set_query("delete");
        let narrow = screen(&palette, 80, 24);
        assert!(
            narrow.contains("can't now: only your own message"),
            "{narrow}"
        );
        assert!(narrow.contains('✕'), "{narrow}");
        let wide = screen(&palette, 200, 50);
        assert!(
            wide.contains("✕ only your own message can be deleted"),
            "{wide}"
        );
    }

    #[test]
    fn a_step_reads_at_80x24_and_160x45() {
        let mut palette = palette(facts());
        typed(&mut palette, "start a dm");
        palette.key(key(KeyCode::Enter));
        typed(&mut palette, "sc");
        for (width, height) in [(80, 24), (160, 45)] {
            let text = screen(&palette, width, height);
            assert!(text.contains("Start a DM › person"), "{text}");
            assert!(text.contains("People"), "{text}");
            assert!(text.contains("▌◆ Scout"), "{text}");
            assert!(text.contains(":dm npub1"), "{text}");
        }
    }

    #[test]
    fn the_panel_fits_every_size() {
        for (width, height) in [(50, 12), (80, 24), (120, 36), (160, 45), (200, 50)] {
            let area = super::area(ratatui::layout::Rect::new(0, 0, width, height), 500);
            assert!(
                area.right() <= width && area.bottom() < height,
                "{width}x{height}"
            );
            assert!(area.width <= 100);
        }
        let area = super::area(ratatui::layout::Rect::new(0, 0, 200, 50), 500);
        assert_eq!((area.width, area.height), (100, 26));
        let area = super::area(ratatui::layout::Rect::new(0, 0, 200, 50), 2);
        assert_eq!(area.height, 12);
    }
}
