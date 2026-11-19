//! The bounded functions against Lua's own, on random arguments
//! (`diff.lua`, from M0 question 2): patterns of up to seven items from 42
//! pieces, subjects of up to 14 bytes, tables with holes and with
//! metatables, bad arguments. Results and error messages must be the same,
//! but for the function's name and the line.
//!
//! The bounded library is code bzz must keep true to Lua. This is the test
//! that says whether it is.

use std::time::Duration;

use mlua::Value;

use crate::apps::runtime::vm::Vm;

const DIFF: &str = include_str!("diff.lua");

/// Comparisons for each seed. `BZZ_APPS_DIFF=1000000` runs what M0 ran.
fn comparisons() -> i64 {
    std::env::var("BZZ_APPS_DIFF")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(60_000)
}

fn compare(seed: i64, n: i64) -> (i64, i64, String) {
    let vm = Vm::with_stock(256 << 20, Duration::from_secs(1200)).expect("Lua starts");
    let globals = vm.lua().globals();
    globals.raw_set("N", n).expect("N");
    globals.raw_set("SEED", seed).expect("SEED");
    let mut results = vm
        .run("diff.lua", DIFF.as_bytes())
        .expect("the comparison runs");
    let ran = match results.pop_front() {
        Some(Value::Integer(ran)) => ran,
        other => panic!("{other:?}"),
    };
    let different = match results.pop_front() {
        Some(Value::Integer(different)) => different,
        other => panic!("{other:?}"),
    };
    let report = match results.pop_front() {
        Some(Value::String(report)) => report.to_string_lossy(),
        other => panic!("{other:?}"),
    };
    (ran, different, report)
}

#[test]
fn the_bounded_functions_answer_as_lua_does() {
    let n = comparisons();
    let mut total = 0;
    for seed in [1, 2, 3] {
        let (ran, different, report) = compare(seed, n);
        assert_eq!(ran, n);
        assert_eq!(
            different, 0,
            "seed {seed}: {different} of {ran} calls differ from Lua's own:\n{report}"
        );
        total += ran;
    }
    println!("{total} comparisons, 0 differences");
}
