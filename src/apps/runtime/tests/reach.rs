//! What a script can reach (rule 14): no file, network, process,
//! environment, native code or bytecode. The cases of the prototype's
//! `tests/hostile.py` and `tests/hostile/app.lua`, run through the whole
//! runtime: a script that tries each way out and shows what came of it.

use serde_json::json;

use super::*;
use crate::apps::runtime::memory::content;
use crate::apps::runtime::{Change, Event, Phase, RecordPush};

/// The prototype's hostile app at `api = 1`. Each attempt runs in `pcall`
/// and its outcome is a line of the screen. Keys trigger the attempts that
/// would stop an ordinary script.
const HOSTILE_APP: &str = r#"
local app = {}
app.api = 1
app.types = { "note" }

local function try(label, f)
  local ok, res = pcall(f)
  return label .. ": " .. (ok and ("REACHED " .. tostring(res)) or ("blocked (" .. tostring(res) .. ")"))
end

function app.init(ctx)
  local out = {}
  local function add(l) out[#out + 1] = l end
  add(try("read file io.open", function() return io.open("/etc/hostname"):read("a") end))
  add(try("read file dofile", function() return dofile("/etc/hostname") end))
  add(try("read file loadfile", function() return loadfile("/etc/hostname") end))
  add(try("require module", function() return require("socket") end))
  add(try("load source", function() return load("return 1")() end))
  add(try("load bytecode", function() return load(string.dump(function() return 1 end))() end))
  add(try("dump bytecode", function() return string.dump(function() return 1 end) end))
  add(try("dump through a string", function() return ("x").dump(function() return 1 end) end))
  add(try("native code package.loadlib", function() return package.loadlib("libc.so.6", "getpid") end))
  add(try("environment os.getenv", function() return os.getenv("HOME") end))
  add(try("process os.execute", function() return os.execute("touch MARKER") end))
  add(try("process io.popen", function() return io.popen("id"):read("a") end))
  add(try("exit os.exit", function() return os.exit(3) end))
  add(try("clock os.clock", function() return os.clock() end))
  add(try("debug library", function() return debug.getregistry() end))
  add(try("coroutine", function() return coroutine.running() end))
  add(try("collectgarbage", function() return collectgarbage("count") end))
  add(try("terminal io.write", function() return io.write("\27[2J") end))
  add(try("terminal warn", function() return warn("@on") end))
  local names = {}
  for k in pairs(_G) do names[#names + 1] = k end
  table.sort(names)
  add("globals: " .. table.concat(names, " "))
  for _, lib in ipairs { "host", "data", "ui" } do
    local members = {}
    for k in pairs(_G[lib]) do members[#members + 1] = k end
    table.sort(members)
    add(lib .. ": " .. table.concat(members, " "))
  end
  return { lines = out, pushes = 0, note = "" }
end

function app.view(s, ctx)
  local lines = {}
  for i, l in ipairs(s.lines) do lines[i] = l end
  lines[#lines + 1] = "pushes received: " .. s.pushes
  lines[#lines + 1] = "note: " .. s.note
  return ui.text { wrap = true, table.unpack(lines) }
end

function app.key(s, key, ctx)
  local n = key.name
  s.note = "key " .. n .. " handled"
  if n == "x" then while true do end
  elseif n == "y" then while true do pcall(function() while true do end end) end
  elseif n == "p" then return string.rep("a", 300000):find(".-.-.-.-x")
  elseif n == "m" then
    local t = {}
    while true do t[#t + 1] = string.rep("x", 1000000) .. #t end
  elseif n == "c" then data.create { type = "card", content = { title = "smuggled", column = "todo" } }
  elseif n == "l" then data.list { type = "card" }
  elseif n == "u" then data.update { record_id = s.other or "r0001", rev = "1", content = { title = "hijacked" } }
  elseif n == "d" then data.delete { record_id = s.other or "r0001", rev = "1" }
  elseif n == "s" then
    local function f(i) return 1 + f(i + 1) end
    f(1)
  end
end

function app.push(s, p, ctx)
  s.pushes = s.pushes + 1
end

return app
"#;

/// With time for its calls on a busy machine: these tests are about what
/// a script reaches, not about how long it runs.
fn hostile_app(marker: &std::path::Path) -> Headless {
    hostile_app_with(marker, Limits::patient())
}

fn hostile_app_with(marker: &std::path::Path, limits: Limits) -> Headless {
    let source = HOSTILE_APP.replace("MARKER", &marker.display().to_string());
    Headless::start(
        source,
        HeadlessOptions::new("hostile.lua")
            .size(120, 60)
            .limits(limits),
    )
    .expect("starts")
}

#[test]
fn a_script_reaches_no_file_network_process_environment_native_code_or_bytecode() {
    let dir = tempfile::tempdir().expect("a folder");
    let marker = dir.path().join("pwned");
    let mut app = hostile_app(&marker);
    let screen = app.screen();
    assert_eq!(app.error(), None, "{screen}");
    let attempts: Vec<&str> = screen
        .lines()
        .take_while(|l| !l.starts_with("globals:"))
        .collect();
    assert_eq!(attempts.len(), 19, "{screen}");
    for attempt in attempts {
        assert!(
            attempt.contains(": blocked (") && !attempt.contains("REACHED"),
            "{attempt}"
        );
    }
    assert!(!marker.exists(), "os.execute made a file");
    // Everything a script has, by name: Lua's own that is left, and bzz's.
    let all = screen.replace('\n', " ");
    assert!(
        all.contains(
            "globals: _G _VERSION assert data error getmetatable host ipairs math next pairs \
             pcall print rawequal rawget rawlen rawset select setmetatable string table tonumber \
             tostring type ui utf8 xpcall host: after log quit time data: create delete list \
             null update ui: bar_chart block column edit gauge input layers line line_gauge list \
             paste popup row scrollbar spacer span sparkline table tabs text truncate width \
             pushes received: 0"
        ),
        "{screen}"
    );
}

#[test]
fn a_script_that_misbehaves_is_stopped_and_the_app_goes_on() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut app = hostile_app_with(&dir.path().join("pwned"), Limits::default());
    let wait = "ran longer than 250 ms and was stopped";
    // An endless loop, and one that catches the stop.
    for (key, code) in [("x", "if n == \"x\""), ("y", "elseif n == \"y\"")] {
        app.key(key);
        let error = app.error().unwrap_or_default();
        // It says where the script was: the line of the loop.
        let line = 1 + HOSTILE_APP
            .lines()
            .position(|l| l.trim_start().starts_with(code))
            .expect("the line");
        assert!(
            error.starts_with(&format!("hostile.lua:{line}: app.key")) && error.contains(wait),
            "{key}: {error}"
        );
        assert_eq!(app.phase(), Phase::Running);
        // The app answers the next key, and its error is gone.
        app.key("j");
        assert_eq!(app.error(), None);
        assert!(app.screen().contains("note: key j handled"));
    }
    // A pattern that runs for ever in Lua's own matcher: the prototype
    // could not stop it, and showed "not responding" until bzz quit.
    app.key("p");
    let error = app.error().unwrap_or_default();
    assert!(
        error.contains("a pattern ran past the budget of 250 ms and was stopped"),
        "{error}"
    );
    assert!(app.status().responding);
    // Recursion without end is an error of the script when Lua's stack is
    // full, which takes about half the budget; on a busy machine the
    // budget ends it first. Either way it ends, and the app goes on.
    app.key("s");
    let error = app.error().unwrap_or_default();
    assert!(
        error.contains("stack overflow") || error.contains(wait),
        "{error}"
    );
    assert_eq!(app.phase(), Phase::Running);
}

#[test]
fn memory_past_the_cap_is_an_error_in_the_script_not_an_abort() {
    let dir = tempfile::tempdir().expect("a folder");
    // Time enough to fill the memory: on a busy machine the budget would
    // end the call first, which is another test.
    let mut app = hostile_app_with(&dir.path().join("pwned"), Limits::patient());
    app.key("m");
    assert_eq!(
        app.error().as_deref(),
        Some("app.key ran out of memory: an app may hold 64 MiB")
    );
    assert_eq!(app.phase(), Phase::Running);
    app.key("j");
    assert_eq!(app.error(), None);
    assert!(app.screen().contains("note: key j handled"));
}

#[test]
fn the_fence_holds_a_script_to_its_declared_types() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut app = hostile_app(&dir.path().join("pwned"));
    // Another participant has a record of a type the app did not declare.
    // The session would never hand it over; here the runtime is given it
    // all the same, as if its owner had not looked.
    let (card, _) = app.store().remote_create(
        &bo(),
        "card",
        content(json!({ "title": "secret", "column": "todo" })),
    );
    let (note, _) = app
        .store()
        .remote_create(&bo(), "note", content(json!({ "text": "hi" })));
    for record in [card.clone(), note] {
        app.send(Event::Push(RecordPush {
            change: Change::Create,
            record,
            by: bo(),
        }));
    }
    // Of the two pushes only the note arrived.
    assert!(
        app.screen().contains("pushes received: 1"),
        "{}",
        app.screen()
    );

    for (key, wanted) in [
        (
            "c",
            "data.create: record type \"card\" is not declared in app.types (note)",
        ),
        (
            "l",
            "data.list: record type \"card\" is not declared in app.types (note)",
        ),
        ("u", "data.update: record r0001 is unknown"),
        ("d", "data.delete: record r0001 is unknown"),
    ] {
        app.key(key);
        let error = app.error().unwrap_or_default();
        assert!(error.contains(wanted), "{key}: {error}");
        assert!(error.starts_with("in app.key: hostile.lua:"), "{error}");
    }
    // Nor with the real id of the record it was never given.
    let source = HOSTILE_APP.replace("s.other", &format!("{:?}", card.id));
    app.reload(source).key("u");
    let error = app.error().unwrap_or_default();
    assert!(
        error.contains(&format!("record {} is unknown", card.id)),
        "{error}"
    );
    // The store saw none of it.
    for op in ["create", "update", "delete", "list"] {
        assert_eq!(app.store().count(op), 0, "{op}");
    }
    assert_eq!(
        app.store().get(&card.id).expect("the card").content,
        card.content
    );
}

/// A script that hangs a metatable on everything it hands to bzz. Every
/// metamethod counts its calls: bzz reads a script's tables raw, so the
/// count must stay at zero. (Were one of them a loop, bzz would be caught
/// in it where no budget applies.)
const WATCHED: &str = r#"
local calls = 0
local function hit() calls = calls + 1 return nil end
local watch = {
  __index = hit, __newindex = function(t, k, v) calls = calls + 1 rawset(t, k, v) end,
  __len = function() calls = calls + 1 return 0 end, __pairs = hit, __call = hit, __tostring = hit,
  __eq = hit, __lt = hit, __le = hit, __concat = hit, __close = hit, __name = "watched",
}
local function watched(t) return setmetatable(t, watch) end

-- The handlers are put in before the table is watched: an assignment to a
-- watched table is the script's own call of `__newindex`.
local app = { api = 1, types = watched { "note" } }

function app.init(ctx)
  data.list(watched { type = "note", tag = watched { why = "load" } })
  host.after(100, watched { n = 1 })
  host.log(watched { logged = true })
  return watched { field = watched { value = "ab", cursor = 1 }, notes = watched {}, fired = 0, replies = 0 }
end

function app.view(s, ctx)
  local count = calls
  return watched(ui.column(watched {
    watched(ui.text(watched { "calls " .. count .. " fired " .. rawget(s, "fired") .. " replies " .. rawget(s, "replies") })),
    watched(ui.list(watched { items = watched { watched(ui.line(watched { watched(ui.span(watched { "item" })) })) }, selected = 1, highlight = watched { bold = true } })),
    watched(ui.table(watched { header = watched { "h" }, rows = watched { watched { "cell" } }, widths = watched { "fill" } })),
    watched(ui.row(watched { watched(ui.text(watched { "f: ", size = 3 })), watched(ui.input(watched { value = rawget(rawget(s, "field"), "value"), focused = true })) })),
    watched(ui.block(watched { borders = watched { "top" }, padding = watched { 1, 0 }, title = watched(ui.line(watched { "t" })) })),
    watched(ui.bar_chart(watched { bars = watched { watched { label = "a", value = 1 } } })),
    watched(ui.sparkline(watched { data = watched { 1, 2 } })),
  }))
end

function app.key(s, key, ctx)
  ui.edit(rawget(s, "field"), watched(key))
  ui.paste(rawget(s, "field"), "x")
  data.create(watched { type = "note", content = watched { text = "t", list = watched { 1, 2 }, sub = watched { a = 1 } }, tag = watched { 1 } })
end

function app.timer(s, tag, ctx) rawset(s, "fired", rawget(s, "fired") + 1) end
function app.reply(s, r, ctx) rawset(s, "replies", rawget(s, "replies") + 1) end

return watched(app)
"#;

#[test]
fn bzz_never_runs_a_metamethod_of_a_script() {
    let mut app = Headless::start(
        WATCHED,
        HeadlessOptions::new("watched.lua")
            .size(40, 12)
            .limits(Limits::patient()),
    )
    .expect("starts");
    assert_eq!(app.error(), None);
    assert_eq!(app.types(), ["note"]);
    app.key("z").advance(100);
    let screen = app.screen();
    assert_eq!(app.error(), None, "{screen}");
    // Everything worked: the request was made and answered, the timer
    // fired, the field was edited, the content was stored as it is.
    assert!(
        screen.starts_with("calls 0 fired 1 replies 2\n"),
        "{screen}"
    );
    assert!(
        screen.contains("> item") && screen.contains("cell") && screen.contains("f: azxb"),
        "{screen}"
    );
    assert_eq!(app.logs(), [r#"{"logged":true}"#]);
    let stored = app.store().records()[0].content.clone();
    assert_eq!(
        serde_json::Value::Object(stored),
        json!({ "text": "t", "list": [1, 2], "sub": { "a": 1 } })
    );
    // And across a reload, where the whole state is read.
    app.reload(WATCHED);
    assert_eq!(app.error(), None);
    assert!(
        app.screen().starts_with("calls 0 fired 1 replies 2\n"),
        "{}",
        app.screen()
    );
}
