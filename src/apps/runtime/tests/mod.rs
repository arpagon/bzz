//! Tests of the runtime as a whole: the interpreter and what stops it, the
//! runtime driven without a window, the fixtures, the drawing, and the
//! contract against its document.

mod board;
mod contract;
mod differential;
mod drawing;
mod hostile;
mod limits;
mod reach;
mod records;
mod running;
mod sweep;
mod vm;

use serde_json::json;

use super::clock::ManualClock;
use super::headless::{Headless, HeadlessOptions, START_OF_TIME};
use super::limits::Limits;
use super::memory::{MemoryStore, content};
use super::records::Who;

/// The fixtures: two apps written against `api = 1`.
pub const HELLO: &str = include_str!("../../../../tests/fixtures/apps/hello/app.lua");
pub const BOARD: &str = include_str!("../../../../tests/fixtures/apps/board/app.lua");

pub fn ana() -> Who {
    Who::new("a1".repeat(32), "ana")
}

pub fn bo() -> Who {
    Who::new("b0".repeat(32), "bo")
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
