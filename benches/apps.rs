//! One app open (v0.16): what the Board costs bzz's thread when it is in
//! the pane. The Board fixture (`tests/fixtures/apps/board/app.lua`, the
//! template `bzz app new` writes) on 200 cards in a store in memory, through
//! the same `AppSession` the pane uses, at the size of a docked app (40 by
//! 16) and of a zoomed one (120 by 40).
//!
//! - **open**: the interpreter started, the script loaded, its first list
//!   answered and its first screen drawn;
//! - **redraw**: drawing the screen the script last returned into the
//!   pane's area, which bzz does for every frame it draws while the app is
//!   shown, whether or not the app changed;
//! - **key to screen**: a key sent, the script's `app.key` and `app.view`
//!   run on its own thread, the new screen drawn.
//!
//! Before the benchmarks it prints the resident memory one open Board
//! adds to the process (Linux: `/proc/self/statm`), and that of ten.

use bzz::apps::runtime::{Limits, ManualClock, records::Who};
use bzz::apps::session::{
    headless::{Headless, HeadlessOptions, START_OF_TIME},
    memory::{MemoryStore, content},
};
use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use std::hint::black_box;

const BOARD: &str = include_str!("../tests/fixtures/apps/board/app.lua");
const CARDS: usize = 200;

fn person(name: &str) -> Who {
    let id = format!("{:0>64}", hex(name));
    Who::new(id, name)
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// The Board on `CARDS` cards in three columns, at `width` by `height`.
fn board(width: u16, height: u16) -> Headless {
    let clock = ManualClock::new(START_OF_TIME);
    let ana = person("ana");
    let bo = person("bo");
    let mut store = MemoryStore::new(ana.clone(), clock.clone());
    for order in 0..CARDS {
        let column = ["todo", "doing", "done"][order % 3];
        store.seed(
            "card",
            content(json!({ "title": format!("Card {order:03}"), "column": column, "order": order })),
            &bo,
        );
    }
    store.set_name(&bo.id, &bo.name);
    let options = HeadlessOptions::new("board.lua")
        .app("board")
        .user(ana)
        .size(width, height)
        .limits(Limits::patient());
    let mut app = Headless::start_on(BOARD, options, store, clock).expect("the Board starts");
    assert!(app.settle(), "the Board comes to rest");
    let screen = app.screen();
    assert!(screen.contains("Card"), "the Board shows its cards:\n{screen}");
    app
}

/// Resident memory of this process, in bytes, where the system says it.
fn resident() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

fn report_memory() {
    let Some(before) = resident() else {
        println!("apps: resident memory not measured on this system");
        return;
    };
    let one = board(40, 16);
    let with_one = resident().unwrap_or(before);
    let mut more = (0..9).map(|_| board(40, 16)).collect::<Vec<_>>();
    let with_ten = resident().unwrap_or(before);
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    println!(
        "apps: resident memory before {:.1} MiB; one Board on {CARDS} cards open +{:.1} MiB; ten open +{:.1} MiB",
        mib(before),
        mib(with_one.saturating_sub(before)),
        mib(with_ten.saturating_sub(before)),
    );
    more.clear();
    drop(one);
}

fn bench_apps(c: &mut Criterion) {
    report_memory();
    c.bench_function("app: open the Board on 200 cards, first screen at 40x16", |bench| {
        bench.iter(|| black_box(board(40, 16)))
    });
    for (width, height) in [(40, 16), (120, 40)] {
        let mut app = board(width, height);
        c.bench_function(
            &format!("app: redraw the Board, 200 cards, at {width}x{height}"),
            |bench| bench.iter(|| black_box(app.buffer())),
        );
        let mut down = true;
        c.bench_function(
            &format!("app: key to screen on the Board, 200 cards, at {width}x{height}"),
            |bench| {
                bench.iter(|| {
                    // Down and up in turn, so that every key changes the
                    // selection and the screen.
                    app.key(if down { "j" } else { "k" });
                    down = !down;
                    black_box(app.buffer())
                })
            },
        );
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_apps
}
criterion_main!(benches);
