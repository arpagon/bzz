//! The hostile scripts of M0, question 2 (`hostile.lua`): each tries to run
//! past the time budget or the memory cap of its interpreter. Loops in the
//! script's own code, single calls of a library function, and memory.
//!
//! A script passes when its call ends within the budget and a margin, by
//! an error, a stop or a return, and the interpreter can be used again or
//! replaced afterwards. Each runs on a thread of its own, as an app does:
//! one that is not stopped is left behind, and the test fails without
//! waiting for it.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use mlua::Value;

use crate::apps::runtime::vm::{Failure, Vm};

const HOSTILE: &str = include_str!("hostile.lua");

/// The budget and the cap the scripts were written for (M0).
pub const BUDGET: Duration = Duration::from_millis(200);
pub const MEMORY: usize = 64 << 20;

/// A call must end within the budget and this. M0 allowed 100 ms and saw
/// one of eighty scripts miss it on a loaded machine; this is a debug build
/// besides. What it must still catch is a call that the budget does not
/// end at all: the shortest of those ran 1.4 s over, and the rest for ever.
pub const MARGIN: Duration = Duration::from_millis(800);

/// A script still running after this is not going to end.
const GIVE_UP: Duration = Duration::from_secs(8);

/// A script that ended late is tried again this many times: a late end
/// that does not repeat was the machine.
const TRIES: usize = 3;

pub struct Case {
    pub name: String,
    pub note: String,
    pub source: String,
}

/// A hostile file is a list of scripts, each after a line
/// `-- @case name [engine] : what it tries`. Scripts marked for another
/// engine than Lua 5.4 are left out.
pub fn cases() -> Vec<Case> {
    let mut all: Vec<(Case, bool)> = Vec::new();
    for line in HOSTILE.lines() {
        if let Some(head) = line.strip_prefix("-- @case ") {
            let (head, note) = head.split_once(':').unwrap_or((head, ""));
            let mut words = head.split_whitespace();
            let name = words.next().unwrap_or("?").to_string();
            let ours = words.next().is_none_or(|engine| engine == "[lua54]");
            all.push((
                Case {
                    name,
                    note: note.trim().to_string(),
                    source: String::new(),
                },
                ours,
            ));
        } else if let Some((case, _)) = all.last_mut() {
            case.source.push_str(line);
            case.source.push('\n');
        }
    }
    all.into_iter()
        .filter_map(|(case, ours)| ours.then_some(case))
        .collect()
}

/// What a script must still be able to do after a hostile call.
pub const USABLE: &str = r##"
local t = {}
for w in string.gmatch("one two three", "%a+") do table.insert(t, w) end
table.sort(t)
return table.concat(t, ",") .. ":" .. (string.gsub("a-b", "%-", "+")) .. ":" .. select("#", string.find("hello", "(l)(l)"))
"##;
pub const USABLE_ANSWER: &str = "one,three,two:a+b:4";

pub fn usable(vm: &Vm) -> bool {
    matches!(
        vm.run("usable.lua", USABLE.as_bytes()).map(|mut v| v.pop_front()),
        Ok(Some(Value::String(s))) if s.to_string_lossy() == USABLE_ANSWER
    )
}

#[derive(Debug)]
pub struct Ended {
    /// `returned`, `stopped`, `memory` or `error`.
    pub how: &'static str,
    pub message: String,
    pub call: Duration,
    pub usable: bool,
    pub usable_in: Duration,
    pub replaced: bool,
    pub replaced_in: Duration,
}

/// Runs one script on a thread of its own. None: it was still running
/// after [`GIVE_UP`], and its thread is left behind.
pub fn run_apart(name: &str, source: &str, make: fn() -> Vm, budget: Duration) -> Option<Ended> {
    let (tx, rx) = mpsc::channel();
    let (name, source) = (name.to_string(), source.to_string());
    std::thread::Builder::new()
        .name(format!("hostile {name}"))
        .spawn(move || {
            let vm = make();
            let started = Instant::now();
            let outcome = vm.run(&name, source.as_bytes());
            let call = started.elapsed();
            let (how, message) = match outcome {
                Ok(_) => ("returned", String::new()),
                Err(Failure::Stopped { at }) => ("stopped", at.unwrap_or_default()),
                Err(Failure::Memory) => ("memory", String::new()),
                Err(Failure::Script(m)) => ("error", m),
            };
            // Is the same interpreter usable afterwards?
            vm.collect();
            let started = Instant::now();
            let usable = usable(&vm);
            let usable_in = started.elapsed();
            // Can it be replaced cleanly: dropped, and a new one started?
            let started = Instant::now();
            drop(vm);
            let fresh = make();
            let replaced = self::usable(&fresh);
            let replaced_in = started.elapsed();
            let _ = tx.send(Ended {
                how,
                message,
                call,
                usable,
                usable_in,
                replaced,
                replaced_in,
            });
        })
        .expect("a thread");
    rx.recv_timeout(GIVE_UP + budget).ok()
}

fn bounded() -> Vm {
    Vm::new(MEMORY, BUDGET).expect("Lua starts")
}

/// Why a script did not pass, or nothing.
pub fn verdict(ended: &Option<Ended>, budget: Duration) -> Option<String> {
    let limit = budget + MARGIN;
    match ended {
        None => Some(format!("NOT STOPPED: still running after {GIVE_UP:?}")),
        Some(e) if e.call > limit => Some(format!(
            "ended ({}) after {:?}, past {limit:?}",
            e.how, e.call
        )),
        Some(e) => {
            let again = e.usable && e.usable_in <= limit;
            let anew = e.replaced && e.replaced_in <= limit;
            if again || anew {
                None
            } else {
                Some(format!(
                    "the interpreter was neither usable ({}, {:?}) nor replaced ({}, {:?}) in time",
                    e.usable, e.usable_in, e.replaced, e.replaced_in
                ))
            }
        }
    }
}

/// Runs every case, at most `jobs` at a time, and returns the failures.
pub fn run_suite(make: fn() -> Vm, budget: Duration, report: &mut String) -> Vec<String> {
    let all = cases();
    let mut failures = Vec::new();
    let mut second_line = Vec::new();
    for case in &all {
        let mut ended = run_apart(&case.name, &case.source, make, budget);
        let mut why = verdict(&ended, budget);
        // A late end is tried again; a script that never ended is not.
        let mut tries = 1;
        while why.is_some() && ended.is_some() && tries < TRIES {
            ended = run_apart(&case.name, &case.source, make, budget);
            why = verdict(&ended, budget);
            tries += 1;
        }
        let line = match &ended {
            Some(e) => format!(
                "{:<30} {:>7.1} ms  {:<8} usable {:<5} replaced {:<5} {}",
                case.name,
                e.call.as_secs_f64() * 1000.0,
                e.how,
                e.usable,
                e.replaced,
                e.message.chars().take(60).collect::<String>()
            ),
            None => format!("{:<30} never", case.name),
        };
        report.push_str(&line);
        report.push('\n');
        if let Some(e) = &ended
            && e.message.contains("the call ran past its time budget")
        {
            second_line.push(case.name.clone());
        }
        if let Some(why) = why {
            failures.push(format!("{} ({}): {why}", case.name, case.note));
        }
    }
    // The yield must have been enough everywhere: the hook's error is the
    // second line, for a place nobody has found.
    for name in second_line {
        failures.push(format!(
            "{name}: stopped by the hook's error, not by the yield"
        ));
    }
    failures
}

#[test]
fn the_hostile_file_has_its_eighty_scripts() {
    let all = cases();
    assert_eq!(all.len(), 80, "scripts for Lua 5.4");
    let mut names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 80, "every script has its own name");
    assert!(all.iter().all(|c| !c.source.trim().is_empty()));
}

#[test]
fn every_hostile_script_is_stopped_in_budget() {
    let mut report = String::new();
    let failures = run_suite(bounded, BUDGET, &mut report);
    println!("{report}");
    assert!(
        failures.is_empty(),
        "{} of {} hostile scripts were not stopped:\n{}",
        failures.len(),
        cases().len(),
        failures.join("\n")
    );
}

/// The same suite on Lua as it ships, with only the names removed and a
/// hook that raises an error: the prototype's sandbox. It is here to be
/// run by hand, as the proof that the suite can fail:
///
/// ```sh
/// cargo test --lib apps::runtime::tests::hostile::the_prototype -- --ignored --nocapture
/// ```
#[test]
#[ignore = "the red run: it leaves thirty-one threads spinning until the test process ends"]
fn the_prototype_sandbox_fails_the_suite() {
    let mut report = String::new();
    let failures = run_suite(plain, BUDGET, &mut report);
    println!("{report}");
    println!("{} failed:\n{}", failures.len(), failures.join("\n"));
    assert!(
        failures.is_empty(),
        "{} hostile scripts were not stopped",
        failures.len()
    );
}

/// An interpreter as the prototype made it.
fn plain() -> Vm {
    Vm::plain_for_the_red_run(MEMORY, BUDGET).expect("Lua starts")
}
