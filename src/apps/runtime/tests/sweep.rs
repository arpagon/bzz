//! The sweep of M0, question 2: every function a script can reach, called
//! with every list of up to three arguments from a pool of awkward values,
//! and every list of four from a smaller pool. It answers "is there one
//! more call that cannot be stopped", which reading the source alone does
//! not. Here the functions of the app API are swept with Lua's.
//!
//! A call that does not come back leaves its thread behind and fails the
//! test at once, with the function and its arguments.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{Function, Table, Value};

use crate::apps::runtime::api::{self, Gate};
use crate::apps::runtime::clock::SystemClock;
use crate::apps::runtime::limits::Limits;
use crate::apps::runtime::vm::Vm;

const BUDGET: Duration = Duration::from_millis(100);
const MEMORY: usize = 64 << 20;
/// As in the hostile suite: room for a loaded machine and a debug build.
const MARGIN: Duration = Duration::from_millis(800);
/// A worker that reports nothing for this long is inside a call that will
/// not end.
const SILENCE: Duration = Duration::from_secs(10);

/// The pool. Each entry is a name and a Lua expression, evaluated anew for
/// every call so that one call cannot spoil the next.
const POOL: &[(&str, &str)] = &[
    ("\"\"", "\"\""),
    ("\"a\"", "\"a\""),
    ("BIG(1 MiB of a)", "BIG"),
    ("\".-.-.-.-.-x\"", "\".-.-.-.-.-x\""),
    ("0", "0"),
    ("1", "1"),
    ("-1", "-1"),
    ("MAXINT", "MAXI"),
    ("MININT", "MINI"),
    ("2^31-1", "2147483647"),
    ("1e308", "1e308"),
    ("{}", "{}"),
    (
        "LEN_HUGE{__len=MAXINT}",
        "setmetatable({}, {__len = function() return MAXI end})",
    ),
    (
        "PROXY{__index=C,__newindex=C,__len=2^30}",
        "setmetatable({}, {__index = CIDX, __newindex = rawequal, __len = function() return 1073741824 end})",
    ),
    ("SPARSE{1,2,4,..}", "sparse()"),
    ("LUAFN", "function(...) return true end"),
    ("CFN(rawequal)", "rawequal"),
    ("BIGARR(200k)", "BIGARR"),
    // For the app API: a table every metamethod of which loops, and a
    // request that is right but for its size.
    (
        "EVIL{every metamethod loops}",
        "setmetatable({ value = \"x\", cursor = 0, name = \"a\", text = \"a\", type = \"note\", content = {} }, EVIL)",
    ),
    (
        "REQUEST{type, content of 200k}",
        "{ type = \"note\", content = { list = BIGARR }, tag = BIGARR, items = BIGARR, rows = BIGARR }",
    ),
];
/// Indices into `POOL` for lists of four arguments.
const SMALL: &[usize] = &[0, 2, 3, 5, 7, 11, 12, 14];

const SETUP: &str = r#"
MAXI = math.maxinteger
MINI = math.mininteger
BIG = string.rep("a", 1048576)
CIDX = rawlen
BIGARR = {}
for i = 1, 200000 do BIGARR[i] = 200000 - i end
local function spin() while true do end end
EVIL = { __index = spin, __newindex = spin, __len = spin, __pairs = spin, __tostring = spin,
         __call = spin, __eq = spin, __lt = spin, __le = spin, __concat = spin, __close = spin,
         __add = spin, __name = "evil" }
"#;

/// A table with no metatable whose length Lua 5.4 reports as 2^61: 62 keys,
/// all in the hash part because the constructor sizes it at once.
fn sparse_source() -> String {
    let keys: Vec<String> = (0..62).map(|k| format!("[{}] = 1", 1u64 << k)).collect();
    format!("function sparse() return {{ {} }} end", keys.join(", "))
}

fn mk_source() -> String {
    let mut s = String::from("function mk(i)\n");
    for (i, (_, expr)) in POOL.iter().enumerate() {
        s.push_str(&format!("  if i == {} then return {} end\n", i + 1, expr));
    }
    s.push_str("end\n");
    s
}

/// Which argument lists are tried. The whole sweep of M0 is every list of
/// up to three values of the pool and every list of four of the small one;
/// it takes minutes in a debug build. Without `BZZ_APPS_SWEEP=full` lists
/// of three come from the small pool too.
struct Lists {
    full: bool,
}

impl Lists {
    fn total(&self) -> usize {
        let n = POOL.len();
        let m = SMALL.len();
        let three = if self.full { n * n * n } else { m * m * m };
        1 + n + n * n + three + m.pow(4)
    }

    fn decode(&self, mut index: usize) -> Vec<usize> {
        let n = POOL.len();
        let m = SMALL.len();
        if index == 0 {
            return vec![];
        }
        index -= 1;
        for arity in 1..=2u32 {
            let count = n.pow(arity);
            if index < count {
                return (0..arity).map(|k| (index / n.pow(k)) % n).collect();
            }
            index -= count;
        }
        if self.full {
            if index < n.pow(3) {
                return (0..3u32).map(|k| (index / n.pow(k)) % n).collect();
            }
            index -= n.pow(3);
        } else {
            if index < m.pow(3) {
                return (0..3u32).map(|k| SMALL[(index / m.pow(k)) % m]).collect();
            }
            index -= m.pow(3);
        }
        (0..4u32).map(|k| SMALL[(index / m.pow(k)) % m]).collect()
    }

    fn describe(&self, index: usize) -> String {
        let names: Vec<&str> = self.decode(index).iter().map(|&i| POOL[i].0).collect();
        format!("({})", names.join(", "))
    }
}

/// An interpreter as an app has it: the bounded library and the app API.
fn app_vm() -> Vm {
    let vm = Vm::new(MEMORY, BUDGET).expect("Lua starts");
    let mut gate = Gate::new(Limits::default(), SystemClock::shared());
    gate.types = vec!["note".into()];
    api::install(vm.lua(), &Rc::new(RefCell::new(gate))).expect("the API");
    vm
}

/// Every function a script can reach: the globals and one level below.
fn functions(vm: &Vm) -> Vec<String> {
    let source = r#"
      local out = {}
      for k, v in pairs(_G) do
        if type(v) == "function" then out[#out + 1] = k
        elseif type(v) == "table" and k ~= "_G" then
          for m, f in pairs(v) do if type(f) == "function" then out[#out + 1] = k .. "." .. m end end
        end
      end
      table.sort(out)
      return table.concat(out, " ")
    "#;
    match vm
        .run("functions.lua", source.as_bytes())
        .map(|mut v| v.pop_front())
    {
        Ok(Some(Value::String(s))) => s.to_string_lossy().split(' ').map(str::to_string).collect(),
        other => panic!("{other:?}"),
    }
}

fn resolve(vm: &Vm, path: &str) -> Function {
    let globals = vm.lua().globals();
    match path.split_once('.') {
        Some((library, name)) => globals
            .raw_get::<Table>(library)
            .and_then(|t| t.raw_get::<Function>(name))
            .expect("a function"),
        None => globals.raw_get::<Function>(path).expect("a function"),
    }
}

enum Progress {
    /// About to call `function` with the list `index`.
    At {
        worker: usize,
        function: String,
        index: usize,
    },
    Slow {
        function: String,
        index: usize,
        took: Duration,
    },
    Done {
        function: String,
        calls: usize,
        worst: Duration,
    },
    Finished {
        worker: usize,
    },
}

fn sweep_function(
    worker: usize,
    path: &str,
    lists: &Lists,
    progress: &mpsc::Sender<Progress>,
    make: fn() -> Vm,
) {
    let vm = make();
    // The function is taken before the pool is built, by its own name.
    let f = resolve(&vm, path);
    // The pool is built outside any call: it is the test's, not a script's,
    // and must not meet the budget on a busy machine.
    for setup in [SETUP.to_string(), sparse_source(), mk_source()] {
        vm.lua().load(setup).exec().expect("setup");
    }
    let mk: Function = vm.lua().globals().raw_get("mk").expect("mk");
    let limit = BUDGET + MARGIN;
    let mut worst = Duration::ZERO;
    let total = lists.total();
    for index in 0..total {
        let _ = progress.send(Progress::At {
            worker,
            function: path.to_string(),
            index,
        });
        let mut took = Duration::MAX;
        // A call that ends late is tried again: late once is the machine.
        for _ in 0..3 {
            let mut values = Vec::new();
            for i in lists.decode(index) {
                let made = vm.call(&mk, i + 1).ok().and_then(|mut v| v.pop_front());
                values.push(made.unwrap_or(Value::Nil));
            }
            let started = Instant::now();
            let outcome = vm.call(&f, mlua::MultiValue::from_vec(values));
            took = took.min(started.elapsed());
            if matches!(outcome, Err(crate::apps::runtime::vm::Failure::Memory)) {
                vm.collect();
            }
            if took <= limit {
                break;
            }
        }
        worst = worst.max(took);
        if took > limit {
            let _ = progress.send(Progress::Slow {
                function: path.to_string(),
                index,
                took,
            });
        }
        if index % 32 == 0 {
            vm.collect();
        }
    }
    let _ = progress.send(Progress::Done {
        function: path.to_string(),
        calls: total,
        worst,
    });
}

/// Sweeps every function of an interpreter that `make` makes. Panics with
/// the function and its arguments when a call does not come back.
fn sweep(make: fn() -> Vm, functions: usize) {
    let lists = Arc::new(Lists {
        full: std::env::var("BZZ_APPS_SWEEP").is_ok_and(|v| v == "full"),
    });
    let names = self::functions(&make());
    // What a script can reach, by name: a new function must be looked at
    // before it is added here, as M0 looked at these.
    assert_eq!(names.len(), functions, "{}", names.join(" "));
    let queue = Arc::new(Mutex::new(
        names.clone().into_iter().rev().collect::<Vec<_>>(),
    ));
    let jobs = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 8);
    let (tx, rx) = mpsc::channel();
    for worker in 0..jobs {
        let (queue, tx, lists) = (queue.clone(), tx.clone(), lists.clone());
        std::thread::Builder::new()
            .name(format!("sweep {worker}"))
            .spawn(move || {
                loop {
                    let next = queue.lock().expect("queue").pop();
                    let Some(name) = next else { break };
                    sweep_function(worker, &name, &lists, &tx, make);
                }
                let _ = tx.send(Progress::Finished { worker });
            })
            .expect("a thread");
    }
    drop(tx);
    let mut at: Vec<Option<(String, usize, Instant)>> = vec![None; jobs];
    let mut finished = 0;
    let (mut calls, mut swept) = (0usize, 0usize);
    let mut slow = Vec::new();
    let mut worst = (Duration::ZERO, String::new());
    while finished < jobs {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Progress::At {
                worker,
                function,
                index,
            }) => {
                at[worker] = Some((function, index, Instant::now()));
            }
            Ok(Progress::Slow {
                function,
                index,
                took,
            }) => {
                slow.push(format!("{function}{} took {took:?}", lists.describe(index)));
            }
            Ok(Progress::Done {
                function,
                calls: n,
                worst: w,
            }) => {
                calls += n;
                swept += 1;
                if w > worst.0 {
                    worst = (w, function);
                }
            }
            Ok(Progress::Finished { worker }) => {
                at[worker] = None;
                finished += 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        for (function, index, since) in at.iter().flatten() {
            assert!(
                since.elapsed() < SILENCE,
                "NOT STOPPED: {function}{} has been running for {:?}",
                lists.describe(*index),
                since.elapsed()
            );
        }
    }
    println!(
        "{swept} functions, {calls} calls, {} argument lists each; the slowest call took {:?} in {}",
        lists.total(),
        worst.0,
        worst.1
    );
    assert_eq!(swept, names.len(), "a worker ended early: it panicked");
    assert!(
        slow.is_empty(),
        "{} calls ran past the budget of {BUDGET:?} and {MARGIN:?}:\n{}",
        slow.len(),
        slow.join("\n")
    );
}

#[test]
fn no_function_a_script_can_reach_runs_past_the_budget() {
    // 76 are Lua's (M0 counted 77 with `print`, which is the API's now);
    // the rest are the API's functions, `data.null` being a value.
    sweep(app_vm, 76 + api::FUNCTIONS.len() - 1);
}

/// The same sweep over Lua as it ships, in the prototype's sandbox. To be
/// run by hand, as the proof that the sweep can fail: it ends at the first
/// call that does not come back.
///
/// ```sh
/// cargo test --lib apps::runtime::tests::sweep::the_prototype -- --ignored --nocapture
/// ```
#[test]
#[ignore = "the red run: it leaves a thread spinning until the test process ends"]
fn the_prototype_sandbox_fails_the_sweep() {
    sweep(
        || Vm::plain_for_the_red_run(MEMORY, BUDGET).expect("Lua starts"),
        76,
    );
}
