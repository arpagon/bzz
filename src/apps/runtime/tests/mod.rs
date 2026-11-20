//! Tests of the runtime as a whole: the interpreter and what stops it, the
//! runtime driven without a window, the fixtures, the drawing, and the
//! contract against its document.

mod approvals;
mod board;
mod board_priority;
mod contract;
mod differential;
mod drawing;
mod hostile;
mod limits;
mod panel;
mod poll;
mod reach;
mod records;
mod rota;
mod running;
mod sweep;
mod vm;

use serde_json::json;

use super::clock::ManualClock;
use super::limits::Limits;
use super::records::Who;
use crate::apps::session::headless::{Headless, HeadlessOptions, START_OF_TIME};
use crate::apps::session::memory::{MemoryStore, content};

/// The fixtures: apps written against `api = 1`. The six of the experiment
/// (Board, Poll, Rota, Approvals, Panel, the Board with priority) each have
/// a file of cases here, the ones their spec in `tests/apps-bench/spec/`
/// fixes and the in-memory store can express; the bench runs the same
/// cases against a terminal, on Linux.
pub const HELLO: &str = include_str!("../../../../tests/fixtures/apps/hello/app.lua");
pub const BOARD: &str = include_str!("../../../../tests/fixtures/apps/board/app.lua");
pub const BOARD_PRIORITY: &str =
    include_str!("../../../../tests/fixtures/apps/board-priority/app.lua");
pub const POLL: &str = include_str!("../../../../tests/fixtures/apps/poll/app.lua");
pub const ROTA: &str = include_str!("../../../../tests/fixtures/apps/rota/app.lua");
pub const APPROVALS: &str = include_str!("../../../../tests/fixtures/apps/approvals/app.lua");
pub const PANEL: &str = include_str!("../../../../tests/fixtures/apps/panel/app.lua");

/// The pane of bzz, and the bench's own size: every case runs at both.
pub const SIZES: [(u16, u16); 2] = [(40, 16), (80, 24)];

pub fn at_each_size(case: impl Fn((u16, u16))) {
    for size in SIZES {
        case(size);
    }
}

/// The other of the two sizes: what a case resizes to and comes back from.
pub fn other_size(size: (u16, u16)) -> (u16, u16) {
    if size == SIZES[0] { SIZES[1] } else { SIZES[0] }
}

pub fn cy() -> Who {
    Who::new("c7".repeat(32), "cy")
}

/// "The relay did not answer": what a failed load is made of.
pub fn unavailable() -> crate::protocol::record::RecordError {
    crate::protocol::record::RecordError::Unavailable {
        reason: "no connection".into(),
    }
}

/// The rows of a screen above its status line.
pub fn above_status(screen: &str) -> Vec<&str> {
    let rows: Vec<&str> = screen.lines().collect();
    rows[..rows.len().saturating_sub(1)].to_vec()
}

/// A row as a marker (`>` when it is the selected one) and what follows.
pub fn marked(row: &str) -> Option<(bool, &str)> {
    if let Some(rest) = row.strip_prefix("> ") {
        Some((true, rest))
    } else {
        row.strip_prefix("  ").map(|rest| (false, rest))
    }
}

/// No row of the screen is wider than the region, and there are as many
/// rows as the region has.
pub fn fits(screen: &str, size: (u16, u16)) {
    use unicode_width::UnicodeWidthStr;
    assert_eq!(screen.lines().count(), size.1 as usize, "{screen}");
    assert!(
        screen.lines().all(|row| row.width() <= size.0 as usize),
        "{screen}"
    );
}

pub fn ana() -> Who {
    Who::new("a1".repeat(32), "ana")
}

pub fn bo() -> Who {
    Who::new("b0".repeat(32), "bo")
}

pub fn di() -> Who {
    Who::new("d1".repeat(32), "di")
}

pub fn eve() -> Who {
    Who::new("e5".repeat(32), "eve")
}

/// The id of one of the cases' people, by name: what a template stores
/// in a field where the bench's spec once had the name.
pub fn id_of(name: &str) -> String {
    match name {
        "ana" => ana(),
        "bo" => bo(),
        "cy" => cy(),
        "di" => di(),
        "eve" => eve(),
        other => panic!("no person {other} in the cases"),
    }
    .id
}

/// bzz knows the names of every person of the cases, as it knows the
/// profiles of a channel's members.
pub fn everyone_named(store: &mut MemoryStore) {
    for who in [ana(), bo(), cy(), di(), eve()] {
        store.set_name(&who.id, &who.name);
    }
}

/// A script on an empty store, with time for its calls on a busy machine.
pub fn start(name: &str, source: &str) -> Headless {
    Headless::start(source, HeadlessOptions::new(name).limits(Limits::patient())).expect("starts")
}

/// A script with the published limits: for what the limits themselves do.
pub fn start_strict(name: &str, source: &str) -> Headless {
    Headless::start(source, HeadlessOptions::new(name)).expect("starts")
}

/// A script on a store that `seed` has filled.
pub fn start_on(
    name: &str,
    source: &str,
    size: (u16, u16),
    seed: impl FnOnce(&mut MemoryStore),
) -> Headless {
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(ana(), clock.clone());
    seed(&mut store);
    Headless::start_on(
        source,
        HeadlessOptions::new(name)
            .size(size.0, size.1)
            .limits(Limits::patient()),
        store,
        clock,
    )
    .expect("starts")
}

/// The six cards every case of the Board starts from.
pub fn six_cards(store: &mut MemoryStore) {
    for (order, (title, column)) in [
        ("Alpha", "todo"),
        ("Bravo", "todo"),
        ("Charlie", "todo"),
        ("Delta", "doing"),
        ("Echo", "doing"),
        ("Foxtrot", "done"),
    ]
    .into_iter()
    .enumerate()
    {
        store.seed(
            "card",
            content(json!({ "title": title, "column": column, "order": order + 1 })),
            &bo(),
        );
    }
}

/// The last row of a screen that has text on it.
pub fn last_row(screen: &str) -> &str {
    screen.lines().last().unwrap_or("")
}
