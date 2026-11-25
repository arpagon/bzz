//! The palette (v0.17 C1): one key finds and runs any action, channel or DM.
//!
//! Pure presentation: it ranks rows of [`crate::ui::action_table::TABLE`]
//! and the channels it is handed, and says which one the person chose. It
//! holds no signer, store, relay or terminal; `App` runs the choice through
//! the paths a key or a typed `:` command already reach.

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
    widgets::{Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::{
    render::sanitize,
    ui::{
        action_table::{ActionSpec, Needs, Run, TABLE, Where},
        actions::{ActionContext, derive},
        keymap::{KeyChord, KeyMap, KeyScope, KeySequence, UiAction},
        state::{FocusSurface, Route},
        theme::{BorderSurface, HighlightGroup, Theme},
        typing::truncate_cells,
    },
};

/// Longest query kept. A search line, not a document.
const MAX_QUERY: usize = 128;
/// Actions of this session the empty palette offers again.
const RECENT_SHOWN: usize = 3;
/// Recent actions kept (this session only: remembering them across
/// sessions would need a migration, a non-goal of v0.17).
const RECENT_KEPT: usize = 8;
/// Rows of the empty state's first group (what the selection allows).
const CONTEXT_SHOWN: usize = 5;
/// Unread channels the empty state offers.
const UNREAD_SHOWN: usize = 5;
/// An exact alias ("q", "status") wins over any fuzzy score.
const ALIAS_BONUS: u32 = 1 << 20;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// A row of [`TABLE`], by index.
    Action(usize),
    Channel(Uuid),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Action,
    Channel,
    Dm,
    Shell,
}

impl Kind {
    const fn glyph(self) -> &'static str {
        match self {
            Self::Action => "·",
            Self::Channel => "#",
            Self::Dm => "@",
            Self::Shell => "$",
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
    /// The `:` form, shown dim when there is room.
    pub detail: String,
    /// The key from the live key map, or empty.
    pub key: String,
    pub status: Status,
    aliases: &'static [&'static str],
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
            title32,
            words32,
        }
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

/// What a key in the palette asks of `App`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Nothing outside the palette changes.
    Stay,
    /// Esc: close and give the focus back.
    Close,
    /// Run the chosen row.
    Run(Target),
    /// Copy this shell command (the row is shell-only).
    Copy(&'static str),
}

/// Which kinds a leading `>`, `#` or `@` narrows the list to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Narrow {
    All,
    Actions,
    Channels,
    Dms,
}

impl Narrow {
    /// The narrowing a query asks for, and the query without its prefix.
    pub fn split(query: &str) -> (Self, &str) {
        let query = query.trim_start();
        match query.chars().next() {
            Some('>') => (Self::Actions, query[1..].trim()),
            Some('#') => (Self::Channels, query[1..].trim()),
            Some('@') => (Self::Dms, query[1..].trim()),
            _ => (Self::All, query.trim()),
        }
    }

    const fn admits(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Actions => matches!(kind, Kind::Action | Kind::Shell),
            Self::Channels => matches!(kind, Kind::Channel),
            Self::Dms => matches!(kind, Kind::Dm),
        }
    }
}

/// Rank `candidates` for `query` (already without its prefix): the best
/// first. Titles score in full, extra words at half; an exact alias wins
/// outright; a row that cannot run now loses a third, a shell row half. At
/// equal score actions come before channels, then the table's order.
pub fn rank(
    candidates: &[Candidate],
    query: &str,
    narrow: Narrow,
    matcher: &mut Matcher,
) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        let mut all = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| narrow.admits(candidate.kind))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        all.sort_by_key(|index| (candidates[*index].kind, *index));
        return all;
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let lowered = query.to_lowercase();
    let mut scored = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| narrow.admits(candidate.kind))
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
    if let Run::Ui(action) = spec.run {
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
        {
            return match entry.reason {
                Some(reason) if !entry.enabled => Status::Unavailable(reason.into()),
                _ => Status::Ready,
            };
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

fn sequence_label(sequence: &KeySequence) -> String {
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

/// The candidates: every listed row of the table, then channels and DMs.
pub fn candidates(keymap: &KeyMap, facts: &Facts, channels: &[PaletteChannel]) -> Vec<Candidate> {
    let scope = if facts.actions.route == Route::Inbox {
        KeyScope::Inbox
    } else {
        KeyScope::Workspace
    };
    let mut all = Vec::with_capacity(TABLE.len() + channels.len());
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
    for channel in channels {
        let name = sanitize::single_line(&channel.name);
        all.push(Candidate::new(
            Target::Channel(channel.id),
            if channel.dm { Kind::Dm } else { Kind::Channel },
            name,
            if channel.unread {
                "unread".into()
            } else {
                String::new()
            },
            String::new(),
            Status::Ready,
            &[],
            "",
        ));
    }
    all
}

/// The palette's state while it is open.
pub struct Palette {
    query: String,
    candidates: Vec<Candidate>,
    empty: Vec<Row>,
    rows: Vec<Row>,
    selected: usize,
    notice: Option<String>,
    matcher: Matcher,
    /// Where the focus was; Esc puts it back.
    pub return_to: (Route, FocusSurface),
    /// What the candidates were built from (the App's channel count and
    /// unread count): when it changes while the palette is open, the App
    /// hands it new candidates (`replace_candidates`).
    pub source: (usize, usize),
    offset: Cell<usize>,
    page: Cell<usize>,
}

impl Palette {
    /// Open over `candidates`, with the empty state: what the selected item
    /// allows (`derive`), this session's recent actions, unread channels.
    pub fn open(
        candidates: Vec<Candidate>,
        facts: &Facts,
        recent: &[&'static str],
        return_to: (Route, FocusSurface),
    ) -> Self {
        let empty = empty_rows(&candidates, facts, recent);
        let mut palette = Self {
            query: String::new(),
            candidates,
            rows: Vec::new(),
            empty,
            selected: 0,
            notice: None,
            matcher: Matcher::new(Config::DEFAULT),
            return_to,
            source: (0, 0),
            offset: Cell::new(0),
            page: Cell::new(10),
        };
        palette.refilter();
        palette
    }

    /// New candidates while open (channels arrived after it opened): the
    /// query stays, and the selection stays on the same row if it is still
    /// listed.
    pub fn replace_candidates(
        &mut self,
        candidates: Vec<Candidate>,
        facts: &Facts,
        recent: &[&'static str],
    ) {
        let selected = self.selected_candidate().map(|candidate| candidate.target);
        self.empty = empty_rows(&candidates, facts, recent);
        self.candidates = candidates;
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

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn candidate(&self, index: usize) -> Option<&Candidate> {
        self.candidates.get(index)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        match self.rows.get(self.selected) {
            Some(Row::Item(index)) => self.candidates.get(*index),
            _ => None,
        }
    }

    /// The titles of the listed items, in order (tests and captures).
    pub fn item_titles(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Item(index) => Some(self.candidates[*index].title.as_str()),
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
        self.query = query.chars().take(MAX_QUERY).collect();
        self.refilter();
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => return self.choose(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p') if control => self.move_by(-1),
            KeyCode::Char('n') if control => self.move_by(1),
            KeyCode::PageUp => self.move_by(-isize::try_from(self.page()).unwrap_or(1)),
            KeyCode::PageDown => self.move_by(isize::try_from(self.page()).unwrap_or(1)),
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
                    && self.query.chars().count() < MAX_QUERY =>
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

    fn choose(&mut self) -> Outcome {
        let Some(candidate) = self.selected_candidate() else {
            return Outcome::Stay;
        };
        match &candidate.status {
            Status::Ready => Outcome::Run(candidate.target),
            Status::Unavailable(reason) => {
                self.notice = Some(format!("can't now: {reason}"));
                Outcome::Stay
            }
            Status::Shell(command) => Outcome::Copy(command),
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
        let (narrow, text) = Narrow::split(&self.query);
        self.rows = if self.query.trim().is_empty() {
            self.empty.clone()
        } else {
            rank(&self.candidates, text, narrow, &mut self.matcher)
                .into_iter()
                .map(Row::Item)
                .collect()
        };
        if self.rows.is_empty() {
            self.rows.push(Row::Hint("Nothing matches".into()));
        }
        self.selected = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Item(_)))
            .unwrap_or(0);
        self.offset.set(0);
    }

    /// The number of rows that can be chosen.
    pub fn item_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, Row::Item(_)))
            .count()
    }
}

fn empty_rows(candidates: &[Candidate], facts: &Facts, recent: &[&'static str]) -> Vec<Row> {
    let mut rows = Vec::new();
    let action_row = |action: UiAction| {
        candidates.iter().position(|candidate| {
            let Target::Action(index) = candidate.target else {
                return false;
            };
            TABLE.get(index).is_some_and(|spec| {
                spec.run == Run::Ui(action)
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
    let recent = recent
        .iter()
        .filter_map(|id| {
            candidates.iter().position(|candidate| {
                matches!(candidate.target, Target::Action(index)
                    if TABLE.get(index).is_some_and(|spec| spec.id == *id))
            })
        })
        .filter(|index| !rows.contains(&Row::Item(*index)))
        .take(RECENT_SHOWN)
        .collect::<Vec<_>>();
    if !recent.is_empty() {
        rows.push(Row::Header("Recent".into()));
        rows.extend(recent.into_iter().map(Row::Item));
    }
    let unread = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            matches!(candidate.kind, Kind::Channel | Kind::Dm) && candidate.detail == "unread"
        })
        .map(|(index, _)| index)
        .take(UNREAD_SHOWN)
        .collect::<Vec<_>>();
    if !unread.is_empty() {
        rows.push(Row::Header("Go to · unread".into()));
        rows.extend(unread.into_iter().map(Row::Item));
    }
    let count = |kind: Kind| {
        candidates
            .iter()
            .filter(|candidate| candidate.kind == kind)
            .count()
    };
    let counted = |kind: Kind, one: &str, many: &str| {
        let count = count(kind);
        format!("{count} {}", if count == 1 { one } else { many })
    };
    rows.push(Row::Hint(format!(
        "Type to search {}, {}, {}, {}",
        counted(Kind::Action, "action", "actions"),
        counted(Kind::Channel, "channel", "channels"),
        counted(Kind::Dm, "DM", "DMs"),
        counted(Kind::Shell, "shell command", "shell commands")
    )));
    rows
}

/// Remember `id` as the newest action of this session.
pub fn remember(recent: &mut Vec<&'static str>, id: &'static str) {
    recent.retain(|existing| *existing != id);
    recent.insert(0, id);
    recent.truncate(RECENT_KEPT);
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
    let block = crate::ui::frame::block(theme, BorderSurface::Modal, "Actions", true)
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
    let (_, text) = Narrow::split(&palette.query);
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
            Row::Item(candidate) => item_line(
                &palette.candidates[*candidate],
                index == palette.selected,
                pattern.as_ref(),
                &mut matcher,
                theme,
                inner_width,
            ),
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
        return Line::from(vec![
            prompt,
            Span::styled("▏", theme.style(HighlightGroup::SelectionMarker)),
            Span::styled(
                truncate_cells("Search actions, channels, DMs…", width.saturating_sub(4)),
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
    let title = truncate_cells(&candidate.title, title_room);
    let title_width = title.width();
    let mut highlighted = Vec::new();
    if let Some(pattern) = pattern {
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
    // The `:` form, dim, when the whole of it fits.
    let detail = &candidate.detail;
    if !detail.is_empty()
        && matches!(candidate.kind, Kind::Action | Kind::Channel | Kind::Dm)
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

fn footer<'a>(palette: &Palette, theme: &Theme, width: usize) -> Line<'a> {
    let (text, group) = match (palette.selected_candidate(), palette.notice()) {
        (_, Some(notice)) => (notice.to_owned(), HighlightGroup::Error),
        (Some(candidate), None) => match &candidate.status {
            Status::Unavailable(reason) => (format!("can't now: {reason}"), HighlightGroup::Error),
            Status::Shell(command) => (
                format!("only in the shell: {command} · Enter copies it"),
                HighlightGroup::Warning,
            ),
            Status::Ready => return keys_footer(palette, theme, width),
        },
        (None, None) => return keys_footer(palette, theme, width),
    };
    Line::from(Span::styled(
        format!(" {} ", truncate_cells(&text, width)),
        theme.style(group),
    ))
}

/// The palette's keys, coloured as every footer's (`frame::hints`), with
/// the count of rows after them; cut to `width` when it does not fit.
fn keys_footer<'a>(palette: &Palette, theme: &Theme, width: usize) -> Line<'a> {
    let text = keys_hint(palette);
    if text.width() > width {
        return Line::from(Span::styled(
            format!(" {} ", truncate_cells(&text, width)),
            theme.style(HighlightGroup::Muted),
        ));
    }
    let mut line = crate::ui::frame::hints(
        theme,
        &[
            ("↵", "run"),
            ("↑↓", "move"),
            ("Esc", "close"),
            ("> # @", "narrow"),
        ],
    );
    line.spans.pop();
    line.spans
        .push(Span::styled(" · ", theme.style(HighlightGroup::Muted)));
    line.spans.push(Span::styled(
        format!("{} ", palette.item_count()),
        theme.style(HighlightGroup::HintText),
    ));
    line
}

fn keys_hint(palette: &Palette) -> String {
    format!(
        "↵ run · ↑↓ move · Esc close · > # @ narrow · {}",
        palette.item_count()
    )
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use uuid::Uuid;

    use super::{
        Facts, Kind, Narrow, Outcome, Palette, PaletteChannel, Row, Status, Target, candidates,
        key_label, remember,
    };
    use crate::ui::{
        action_table::{TABLE, by_id},
        actions::ActionContext,
        keymap::{KeyMap, KeyScope, UiAction},
        state::{FocusSurface, Route},
        theme::Theme,
    };

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
            artifact_selected: false,
            communities: 1,
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

    fn palette(facts: Facts) -> Palette {
        let keymap = KeyMap::builtin();
        Palette::open(
            candidates(&keymap, &facts, &channels()),
            &facts,
            &[],
            (Route::Workspace, FocusSurface::Timeline),
        )
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(palette: &mut Palette, text: &str) {
        for character in text.chars() {
            palette.key(key(KeyCode::Char(character)));
        }
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
        let mut palette = Palette::open(
            candidates(&keymap, &facts, &[]),
            &facts,
            &[],
            (Route::Workspace, FocusSurface::Timeline),
        );
        typed(&mut palette, "#t9");
        assert!(palette.item_titles().is_empty());
        palette.replace_candidates(candidates(&keymap, &facts, &channels()), &facts, &[]);
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
            Outcome::Run(Target::Action(index))
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
        assert_eq!(first.target, Target::Channel(Uuid::from_u128(1)));
        palette.set_query("@sc");
        assert_eq!(palette.item_titles(), vec!["Scout"]);
        palette.set_query(">gen");
        assert!(
            palette
                .rows()
                .iter()
                .all(|row| !matches!(row, Row::Item(index)
                    if matches!(palette.candidate(*index).unwrap().kind, Kind::Channel | Kind::Dm)))
        );
        assert_eq!(Narrow::split(" # t9"), (Narrow::Channels, "t9"));
    }

    #[test]
    fn channels_and_actions_share_one_list() {
        let mut palette = palette(facts());
        typed(&mut palette, "t9-gen");
        assert_eq!(palette.item_titles()[0], "t9-general");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Target::Channel(Uuid::from_u128(1)))
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
    fn an_argument_row_runs_as_a_prefill() {
        let (index, spec) = by_id("status.set").unwrap();
        assert_eq!(spec.run, crate::ui::action_table::Run::Prefill("status "));
        let mut palette = palette(facts());
        typed(&mut palette, "status");
        assert_eq!(
            palette.key(key(KeyCode::Enter)),
            Outcome::Run(Target::Action(index))
        );
    }

    #[test]
    fn the_empty_state_offers_the_selection_recent_and_unread() {
        let keymap = KeyMap::builtin();
        let facts = facts();
        let palette = Palette::open(
            candidates(&keymap, &facts, &channels()),
            &facts,
            &["huddle.panel"],
            (Route::Workspace, FocusSurface::Timeline),
        );
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
        palette.key(key(KeyCode::PageUp));
        palette.key(key(KeyCode::PageUp));
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
        let all = candidates(&keymap, &facts(), &[]);
        let thread = all.iter().find(|row| row.title == "Open thread").unwrap();
        assert_eq!(thread.key, "Enter");
        let quit = all.iter().find(|row| row.title == "Quit bzz").unwrap();
        assert_eq!(quit.key, "q");
    }

    #[test]
    fn inbox_rows_wait_for_the_inbox_and_workspace_rows_for_the_workspace() {
        let all = candidates(&KeyMap::builtin(), &facts(), &[]);
        let mark_read = all.iter().find(|row| row.title == "Mark read").unwrap();
        assert_eq!(
            mark_read.status,
            Status::Unavailable("open the Inbox first (Space n)".into())
        );
        let mut inbox = facts();
        inbox.actions.route = Route::Inbox;
        inbox.actions.focus = FocusSurface::InboxList;
        let all = candidates(&KeyMap::builtin(), &inbox, &[]);
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
        remember(&mut recent, "a");
        remember(&mut recent, "b");
        remember(&mut recent, "a");
        assert_eq!(recent, vec!["a", "b"]);
    }

    #[test]
    fn writing_rows_are_not_listed() {
        let all = candidates(&KeyMap::builtin(), &facts(), &[]);
        let listed = TABLE.iter().filter(|spec| spec.listed()).count();
        assert_eq!(all.len(), listed);
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
