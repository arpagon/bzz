//! The runtime at work: loading, reloading, failing, being stopped and
//! given up, keys, timers, and what a stuck script costs its owner.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;

use super::*;
use crate::apps::runtime::draw::text_of;
use crate::apps::runtime::memory::content;
use crate::apps::runtime::{
    AppTheme, ERROR_ROWS, Event, Key, Options, Output, Phase, Runtime, Signal,
};

/// A script that shows its state and does what a key asks.
const PROBE: &str = r#"
local app = {}
app.api = 1
app.types = { "note" }

function app.init(ctx)
  return { keys = {}, starts = 1, fired = {} }
end

function app.view(s, ctx)
  if s.spin_in_view then while true do end end
  if s.bad_view then return ui.list { items = { "a" }, selected = "first" } end
  return ui.column {
    ui.text { "keys: " .. table.concat(s.keys, " ") },
    ui.text { "fired: " .. table.concat(s.fired, " ") },
    ui.text { "a=" .. tostring(s.a) .. " b=" .. tostring(s.b) .. " saved=" .. tostring(s.saved) },
    ui.text { "user: " .. ctx.user.name .. " " .. ctx.user.id:sub(1, 4) .. " " .. ctx.width .. "x" .. ctx.height },
  }
end

function app.key(s, key, ctx)
  s.keys[#s.keys + 1] = (key.ctrl and "ctrl+" or "") .. key.name
  local n = key.name
  if n == "x" then while true do end
  elseif n == "e" then error("boom")
  elseif n == "t" then return ui.text { "x", colour = "red" }
  elseif n == "v" then s.bad_view = true
  elseif n == "V" then s.bad_view = nil
  elseif n == "w" then s.spin_in_view = true
  elseif n == "q" then host.quit()
  elseif n == "h" then
    s.a = 1
    data.create { type = "note", content = { text = "asked before the loop" }, tag = "half" }
    while true do end
  elseif n == "1" then host.after(100, "a"); host.after(300, { n = 2, none = data.null })
  elseif n == "2" then host.after(100, "again")
  end
end

function app.timer(s, tag, ctx)
  if type(tag) == "table" then
    s.fired[#s.fired + 1] = "n" .. tag.n .. (tag.none == nil and "+nil" or "+null")
  else
    s.fired[#s.fired + 1] = tag
    if tag == "again" and #s.fired < 3 then host.after(100, "again") end
  end
end

function app.reply(s, r, ctx)
  if r.tag == "half" then s.saved = r.ok end
end

return app
"#;

fn probe() -> Headless {
    start_strict("probe.lua", PROBE)
}

/// The line of `PROBE` that starts with `code`.
fn line(code: &str) -> usize {
    1 + PROBE
        .lines()
        .position(|l| l.trim_start().starts_with(code))
        .unwrap_or_else(|| panic!("no line {code:?}"))
}

// ------------------------------------------------------------------ reload

#[test]
fn state_survives_a_reload() {
    let mut app = start("hello.lua", HELLO);
    app.key("n").type_text("first").key("enter").advance(1000);
    app.key("n").type_text("second").key("enter").key("k");
    // A form is open with text half typed, and the selection is not where
    // a fresh start would put it.
    app.key("n").type_text("half");
    let before = app.screen();
    assert!(
        before.contains("Notes (2)") && before.contains("> ana: first"),
        "{before}"
    );

    let next = HELLO
        .replace("\"Notes (\"", "\"Shared notes (\"")
        .replace("\"Saved.\"", "\"Stored.\"");
    assert_ne!(next, HELLO);
    app.reload(next.clone());
    let after = app.screen();
    assert_eq!(app.error(), None);
    // The new version draws, with everything the old one held.
    assert_eq!(
        after,
        before.replace("Notes (2)", "Shared notes (2)"),
        "{after}"
    );
    assert!(
        app.logs().iter().any(|l| l == "reloaded; state kept"),
        "{:?}",
        app.logs()
    );
    // `init` was not called again: the app did not ask for the list twice.
    assert_eq!(app.store().count("list"), 1);

    // The form goes on where it was, and a request that was waiting when
    // the script changed is answered to the new version.
    app.store().hold_next("create");
    app.type_text(" typed").key("enter");
    assert_eq!(last_row(&app.screen()), "Saving...");
    app.reload(next.replace("Shared notes", "Our notes"));
    let replies = app.store().release();
    app.deliver(replies);
    let screen = app.screen();
    assert!(screen.contains("Our notes (3)"), "{screen}");
    assert!(screen.contains("> ana: half typed"), "{screen}");
    assert_eq!(last_row(&screen), "Stored.");
}

#[test]
fn a_broken_reload_leaves_the_old_version_running() {
    let mut app = start("hello.lua", HELLO);
    app.key("n").type_text("first").key("enter").advance(1000);
    app.key("n").type_text("second").key("enter");
    let good = app.screen();
    assert!(good.contains("> ana: second"), "{good}");

    let broken = HELLO.replacen("local app = {}", "local app = = {}", 1);
    app.reload(broken);
    let error = app.error().expect("an error");
    let at = 1 + HELLO
        .lines()
        .position(|l| l == "local app = {}")
        .expect("the line");
    assert_eq!(
        error,
        format!(
            "hello.lua:{at}: unexpected symbol near '='\n(reload failed: the previous version keeps running)"
        )
    );
    assert_eq!(app.phase(), Phase::Running);
    // It says so on the last rows of its own region, two at most, and the
    // rest of the screen is the old version's.
    let screen = app.screen();
    let kept = good.lines().count() - ERROR_ROWS as usize;
    assert_eq!(
        screen.lines().take(kept).collect::<Vec<_>>(),
        good.lines().take(kept).collect::<Vec<_>>(),
        "{screen}"
    );
    assert!(
        screen
            .lines()
            .skip(kept)
            .collect::<String>()
            .contains("unexpected symbol near '='"),
        "{screen}"
    );
    // The old version answers a key and another participant's change.
    app.key("k");
    assert!(app.screen().contains("> ana: first"), "{}", app.screen());
    let (_, pushes) = app.store().remote_create(
        &bo(),
        "note",
        content(json!({ "text": "from bo", "author": "bo", "order": 9e9 })),
    );
    app.deliver(pushes);
    let screen = app.screen();
    assert!(
        screen.contains("Notes (3)") && screen.contains("bo: from bo"),
        "{screen}"
    );
    // The error stays until a version loads.
    assert!(app.error().is_some());

    // Every other way a version can fail to load.
    for (bad, wanted) in [
        (
            HELLO.replace("return app", "return 42"),
            "must end with `return app`",
        ),
        (
            HELLO.replace("app.api = 1", "app.api = 2"),
            "written for api 2",
        ),
        (
            HELLO.replace("function app.view(", "local function view("),
            "app.view is missing",
        ),
        (
            HELLO.replace("app.types = { \"note\" }", ""),
            "app.types is missing",
        ),
        (HELLO.replace("return app", "error('at load')"), "at load"),
        (
            HELLO.replace("return app", "data.list { type = 'note' } return app"),
            "while the script is loaded",
        ),
    ] {
        assert_ne!(bad, HELLO);
        app.reload(bad);
        let error = app.error().unwrap_or_default();
        assert!(error.contains(wanted), "{wanted}: {error}");
        assert!(
            error.ends_with("(reload failed: the previous version keeps running)"),
            "{error}"
        );
        assert_eq!(app.phase(), Phase::Running);
        assert!(app.screen().contains("Notes (3)"));
    }
    // A version refused at load asked for nothing.
    assert_eq!(app.store().count("list"), 1);

    app.reload(HELLO.replace("\"Notes (\"", "\"Fixed notes (\""));
    assert_eq!(app.error(), None);
    let screen = app.screen();
    assert!(
        screen.contains("Fixed notes (3)") && screen.contains("> ana: first"),
        "{screen}"
    );
}

#[test]
fn a_version_that_never_finishes_loading_is_stopped_and_refused() {
    let mut app = start_strict("hello.lua", HELLO);
    app.key("n").type_text("kept").key("enter");
    app.reload(HELLO.replace("return app", "while true do end"));
    let at = HELLO.lines().count();
    assert_eq!(
        app.error(),
        Some(format!(
            "hello.lua:{at}: the script ran longer than 250 ms while it was loaded, and was stopped\n(reload failed: the previous version keeps running)"
        ))
    );
    assert_eq!(app.phase(), Phase::Running);
    assert!(app.screen().contains("> ana: kept"));
}

#[test]
fn a_reload_drops_what_cannot_move_and_lets_the_script_migrate() {
    let first = r#"
        local app = { api = 1, types = { "note" } }
        function app.init(ctx)
          return { count = 3, tab = 2, f = function() end, m = setmetatable({ k = 1 }, { __index = function() return 7 end }) }
        end
        function app.view(s, ctx) return "v1 count=" .. s.count .. " tab=" .. s.tab .. " m.other=" .. tostring(s.m.other) end
        return app
    "#;
    let second = r#"
        local app = { api = 1, types = { "note" } }
        function app.init(ctx) return { count = 0, tab = "all" } end
        function app.reload(s, ctx)
          -- In the new version a tab is a name, not a number.
          s.tab = ({ "all", "alerts" })[s.tab] or "all"
          s.reloaded = type(s.f) .. "/" .. tostring(getmetatable(s.m)) .. "/" .. s.m.k .. "/" .. tostring(s.m.other)
        end
        function app.view(s, ctx) return "v2 count=" .. s.count .. " tab=" .. s.tab .. " " .. tostring(s.reloaded) end
        return app
    "#;
    let mut app = start("app.lua", first);
    assert_eq!(app.screen().trim_end(), "v1 count=3 tab=2 m.other=7");
    app.reload(second);
    assert_eq!(app.error(), None);
    assert_eq!(
        app.screen().trim_end(),
        "v2 count=3 tab=alerts nil/nil/1/nil"
    );
    assert!(
        app.logs().iter().any(|l| l
            == "reload: dropped values that cannot move to the new version: state.f (function)"),
        "{:?}",
        app.logs()
    );
    // `app.reload` may also return a table to replace the state.
    app.reload(second.replace("s.reloaded = ", "return app.init(ctx) --"));
    assert_eq!(app.screen().trim_end(), "v2 count=0 tab=all nil");
}

#[test]
fn a_state_too_deep_to_carry_starts_the_app_again() {
    // A state nested a hundred thousand tables deep: copying it would walk
    // that deep on bzz's own stack. The app starts with init instead.
    let source = r#"
        local app = { api = 1, types = { "note" } }
        function app.init(ctx) return { deep = false } end
        function app.key(s, key, ctx)
          local t = {}
          s.deep = t
          for i = 1, 100000 do t.next = {}; t = t.next end
        end
        function app.view(s, ctx) return "deep=" .. tostring(s.deep ~= false) end
        return app
    "#;
    let mut app = start("app.lua", source);
    app.key("d");
    assert_eq!(app.screen().trim_end(), "deep=true");
    app.reload(source);
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().trim_end(), "deep=false");
    assert!(
        app.logs()
            .iter()
            .any(|l| l.contains("the state was not kept (it is nested too deep)")),
        "{:?}",
        app.logs()
    );
}

// ----------------------------------------------------------------- loading

#[test]
fn an_api_other_than_1_is_refused_with_a_message_that_names_1() {
    let script = |api: &str| {
        format!(
            "local app = {{ types = {{ \"note\" }} }}\n{api}\nfunction app.view() return \"drawn\" end\nreturn app"
        )
    };
    for (api, wanted) in [
        (
            "app.api = 2",
            "app.lua: the script is written for api 2, and this bzz knows only api 1",
        ),
        (
            "app.api = 0",
            "app.lua: the script is written for api 0, and this bzz knows only api 1",
        ),
        (
            "app.api = 1.5",
            "app.lua: the script is written for api 1.5, and this bzz knows only api 1",
        ),
        (
            "app.api = \"1\"",
            "app.lua: app.api must be the number 1, the only api this bzz knows; it is a string",
        ),
        (
            "",
            "app.lua: app.api is missing. This bzz knows api 1: write `app.api = 1`",
        ),
    ] {
        let mut app = start("app.lua", &script(api));
        assert_eq!(app.error().as_deref(), Some(wanted), "{api}");
        assert_eq!(app.phase(), Phase::NotLoaded);
        // Nothing of it runs: no handler was called, no type is declared.
        assert!(!app.screen().contains("drawn"));
        assert!(app.types().is_empty());
    }
    let mut app = start("app.lua", &script("app.api = 1"));
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().trim_end(), "drawn");
    assert_eq!(app.types(), ["note"]);
}

#[test]
fn a_script_that_does_not_load_waits_for_one_that_does() {
    let mut app = start(
        "hello.lua",
        &HELLO.replace("function app.key(", "function app.key(("),
    );
    assert_eq!(app.phase(), Phase::NotLoaded);
    let error = app.error().expect("an error");
    assert!(
        error.starts_with("hello.lua:") && error.contains("expected near '('"),
        "{error}"
    );
    // With no screen to keep, the error has the whole region.
    assert!(
        app.screen()
            .replace('\n', " ")
            .contains("expected near '('"),
        "{}",
        app.screen()
    );
    // Keys go nowhere.
    app.key("n").key("q");
    assert!(!app.closed());
    app.reload(HELLO);
    assert_eq!(app.phase(), Phase::Running);
    assert_eq!(app.error(), None);
    assert!(app.screen().contains("Notes (0)"));
    assert!(
        app.logs()
            .iter()
            .any(|l| l == "loaded; the app starts with init")
    );
}

#[test]
fn the_types_a_script_declares_are_checked_and_reported() {
    for (types, wanted) in [
        ("{}", "app.types is empty"),
        (
            "{ \"Card\" }",
            "app.types[1] = \"Card\" is not a record type's name",
        ),
        ("{ \"bzz.app.board.card\" }", "is not a record type's name"),
        ("{ \"card\", \"card\" }", "app.types names \"card\" twice"),
        ("{ 1 }", "app.types[1] must be a string"),
        ("\"card\"", "app.types is missing"),
        (
            "{ 'a','b','c','d','e','f','g','h','i','j','k','l','m','n','o','p','q' }",
            "app.types has 17 types; an app may declare 16",
        ),
    ] {
        let source = format!(
            "local app = {{ api = 1, types = {types} }}\nfunction app.view() return '' end\nreturn app"
        );
        let app = start("app.lua", &source);
        let error = app.error().unwrap_or_default();
        assert!(error.contains(wanted), "{types}: {error}");
    }
    let app = start(
        "app.lua",
        "local app = { api = 1, types = { 'card', 'vote-2', 'day_off' } }\nfunction app.view() return '' end\nreturn app",
    );
    assert_eq!(app.types(), ["card", "vote-2", "day_off"]);
}

// ------------------------------------------------------------------ errors

#[test]
fn an_error_carries_the_file_and_the_line_and_the_app_goes_on() {
    let mut app = probe();
    let good = app.screen();
    app.key("e");
    assert_eq!(
        app.error(),
        Some(format!(
            "in app.key: probe.lua:{}: boom",
            line("elseif n == \"e\"")
        ))
    );
    // A property that does not exist fails at the line that wrote it.
    app.key("t");
    let error = app.error().unwrap_or_default();
    assert!(
        error.starts_with(&format!(
            "in app.key: probe.lua:{}: ui.text: unknown property 'colour'. Known: align, wrap, scroll, fg, bg",
            line("elseif n == \"t\"")
        )),
        "{error}"
    );
    assert!(
        app.logs()
            .iter()
            .any(|l| l.starts_with("error: in app.key: probe.lua:"))
    );
    // The error covers the last two rows of the app's region, no more,
    // and goes with the next key.
    let screen = app.screen();
    let rows: Vec<&str> = screen.lines().collect();
    assert_eq!(
        rows[..4],
        good.lines().collect::<Vec<_>>()[..4]
            .iter()
            .map(|l| l.replace("keys:", "keys: e t"))
            .collect::<Vec<_>>()[..],
        "{screen}"
    );
    assert_eq!(
        rows[14],
        format!(
            "in app.key: probe.lua:{}: ui.text:",
            line("elseif n == \"t\"")
        ),
        "{screen}"
    );
    assert_eq!(
        rows[15], "unknown property 'colour'. Known: align,",
        "{screen}"
    );
    app.key("j");
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().lines().count(), good.lines().count());

    // A wrong value in a screen is found when the screen is read, and is
    // told with the line of the element. The last good screen stays.
    app.key("v");
    assert_eq!(
        app.error(),
        Some(format!(
            "app.view returned a bad screen: probe.lua:{}: ui.list: 'selected' must be an integer, got \"first\"",
            line("if s.bad_view")
        ))
    );
    assert!(
        app.screen().starts_with("keys: e t j\n"),
        "{}",
        app.screen()
    );
    app.key("V");
    assert_eq!(app.error(), None);
    assert!(app.screen().starts_with("keys: e t j v V\n"));
}

#[test]
fn every_error_of_the_api_is_a_string_a_script_can_catch() {
    let source = r#"
        local app = { api = 1, types = { "note" } }
        local attempts = {
          function() ui.text { bogus = 1 } end,
          function() ui.list { "positional" } end,
          function() ui.block(true) end,
          function() ui.text { [1.5] = "x" } end,
          function() ui.width({}) end,
          function() ui.width(string.rep("x", 70000)) end,
          function() ui.truncate("x", -1) end,
          function() ui.truncate() end,
          function() ui.edit(1, 2) end,
          function() ui.edit({ value = {} }, { name = "a" }) end,
          function() ui.edit({ value = "x", cursor = "end" }, { name = "a" }) end,
          function() ui.paste({}, {}) end,
          function() data.list() end,
          function() data.list { type = "other" } end,
          function() data.list { type = "note", match = 1 } end,
          function() data.list { type = "note", matching = "x" } end,
          function() data.create { type = "note" } end,
          function() data.create { type = "note", content = 1 } end,
          function() data.create { type = "note", content = { f = print } } end,
          function() data.create { type = "note", content = { n = 0/0 } } end,
          function() data.create { type = "note", content = {}, tag = string.rep("x", 5000) } end,
          function() data.update { record_id = "x", rev = "r", content = {} } end,
          function() data.update { rev = "r", content = {} } end,
          function() data.delete {} end,
          function() host.after(1) end,
          function() host.after("soon") end,
          function() host.after(100, print) end,
          function() setmetatable({}, { __gc = print }) end,
          function() string.rep("x", -1, {}) end,
        }
        function app.init(ctx)
          local s = { lines = {} }
          for i, attempt in ipairs(attempts) do
            local ok, err = pcall(attempt)
            local line = i .. " " .. tostring(ok) .. " " .. type(err)
            if type(err) == "string" then line = line .. " " .. ("failed: " .. err) end
            s.lines[i] = line
          end
          return s
        end
        function app.view(s, ctx) return ui.text(s.lines) end
        return app
    "#;
    let mut app = Headless::start(
        source,
        HeadlessOptions::new("app.lua")
            .size(200, 40)
            .limits(Limits::patient()),
    )
    .expect("starts");
    assert_eq!(app.error(), None);
    let screen = app.screen();
    let lines: Vec<&str> = screen.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 29, "{screen}");
    for (i, line) in lines.iter().enumerate() {
        // Each failed, with a string that says where: the attempt's line.
        assert!(
            line.starts_with(&format!(
                "{} false string failed: app.lua:{}: ",
                i + 1,
                i + 4
            )),
            "{line}"
        );
    }
    assert!(lines[0].ends_with("ui.text: unknown property 'bogus'. Known: align, wrap, scroll, fg, bg, bold, dim, italic, underline, reversed, crossed_out, size"), "{}", lines[0]);
    assert!(
        lines[13].contains("data.list: record type \"other\" is not declared in app.types (note)")
    );
    assert!(lines[24].contains("host.after: the delay must be from 100 to 86400000 ms, got 1"));
    // Nothing was asked of the store by a call that was refused.
    assert_eq!(app.store().records().len(), 0);
}

#[test]
fn the_top_level_of_a_script_can_only_compute() {
    for (call, name) in [
        ("data.list { type = 'note' }", "data.list"),
        ("data.create { type = 'note', content = {} }", "data.create"),
        ("host.after(100, 'x')", "host.after"),
        ("host.quit()", "host.quit"),
    ] {
        let source = format!(
            "local app = {{ api = 1, types = {{ 'note' }} }}\n{call}\nfunction app.view() return '' end\nreturn app"
        );
        let mut app = start("app.lua", &source);
        assert_eq!(
            app.error(),
            Some(format!(
                "app.lua:2: {name} cannot be called while the script is loaded; call it from app.init or another handler"
            ))
        );
        assert!(!app.closed());
        assert_eq!(app.store().count("list") + app.store().count("create"), 0);
    }
    // It may compute, build elements, log and read the clock.
    let mut app = start(
        "app.lua",
        "local app = { api = 1, types = { 'note' } }\nlocal title = ui.text { 'at ' .. math.floor(host.time()) }\nhost.log('loading')\nfunction app.view() return title end\nreturn app",
    );
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().trim_end(), format!("at {START_OF_TIME}"));
    assert_eq!(app.logs(), ["loading"]);
}

// ----------------------------------------------------- stopped and given up

#[test]
fn a_stopped_call_leaves_its_work_half_done_and_the_app_goes_on() {
    let mut app = probe();
    app.key("h");
    let error = app.error().unwrap_or_default();
    assert!(
        error.contains("app.key ran longer than 250 ms and was stopped"),
        "{error}"
    );
    assert_eq!(app.phase(), Phase::Running);
    // The error stays through what happens by itself (a reply arrived
    // here), until the person at the app presses a key.
    app.key("j");
    assert_eq!(app.error(), None);
    // What the call changed before it was stopped stays changed, what it
    // had not reached did not happen, and what it had asked for was done:
    // the reply arrived like any other.
    let screen = app.screen();
    assert!(screen.contains("a=1 b=nil saved=true"), "{screen}");
    assert_eq!(app.store().count("create"), 1);
    assert_eq!(app.store().records().len(), 1);
}

#[test]
fn an_app_is_given_up_after_three_stops_of_one_handler_in_a_row() {
    let mut app = probe();
    // Stops that are not in a row do not add up: a key that works in
    // between shows the handler is not stuck.
    for _ in 0..4 {
        app.key("x");
        assert!(
            app.error()
                .unwrap_or_default()
                .contains("and was stopped (an endless loop?)")
        );
        app.key("j");
        assert_eq!(app.error(), None);
        assert_eq!(app.phase(), Phase::Running);
    }
    app.key("x").key("x");
    assert_eq!(app.phase(), Phase::Running);
    app.key("x");
    assert_eq!(app.phase(), Phase::GivenUp);
    assert_eq!(
        app.error(),
        Some(format!(
            "probe.lua:{}: app.key ran longer than 250 ms 3 times in a row. The app is stopped until it is reloaded",
            line("if n == \"x\"")
        ))
    );
    // It is not called any more, and says so in its region.
    let logged = app.logs().len();
    app.key("j").key("q");
    assert!(!app.closed());
    assert_eq!(app.logs().len(), logged);
    assert!(
        last_row(&app.screen()).contains("times in a row"),
        "{}",
        app.screen()
    );
    // A reload starts it again, fresh: the state went with the stops.
    app.reload(PROBE);
    assert_eq!(app.phase(), Phase::Running);
    assert_eq!(app.error(), None);
    assert!(app.screen().starts_with("keys:\n"), "{}", app.screen());
}

#[test]
fn a_view_that_never_returns_gives_the_app_up_too() {
    let mut app = probe();
    let good = app.screen();
    app.key("w");
    // The key was handled; its screen was asked for and stopped. The last
    // good screen stays, with the error on its last row.
    assert!(
        app.error()
            .unwrap_or_default()
            .contains("app.view ran longer than 250 ms")
    );
    assert_eq!(app.phase(), Phase::Running);
    assert_eq!(app.screen().lines().next(), good.lines().next());
    app.key("j");
    assert_eq!(app.phase(), Phase::Running);
    app.key("j");
    assert_eq!(app.phase(), Phase::GivenUp);
}

// ------------------------------------------------- the owner is not delayed

/// The owner's three calls, timed while a script is stuck.
fn owner_calls(runtime: &mut Runtime, buffer: &mut Buffer, during: Duration) -> (Duration, u32) {
    let theme = AppTheme::default();
    let area = buffer.area;
    let until = Instant::now() + during;
    let (mut worst, mut rounds) = (Duration::ZERO, 0);
    while Instant::now() < until {
        let started = Instant::now();
        runtime.send(Event::Tick);
        runtime.poll();
        runtime.draw(area, buffer, &theme);
        worst = worst.max(started.elapsed());
        rounds += 1;
        std::thread::sleep(Duration::from_millis(1));
    }
    (worst, rounds)
}

#[test]
fn a_stuck_script_does_not_delay_its_owner() {
    // A call may run two seconds here, so that the script is stuck for all
    // of the half second the owner is timed.
    let limits = Limits {
        call_time: Duration::from_secs(2),
        event_backlog: 1_000_000,
        ..Limits::default()
    };
    let signal = Signal::new();
    let started = Instant::now();
    let mut runtime = Runtime::start(
        PROBE,
        Options::new("probe.lua", ana())
            .limits(limits)
            .waker(signal.waker()),
    )
    .expect("starts");
    let to_start = started.elapsed();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 16));
    let theme = AppTheme::default();
    // Wait for the first screen, as an owner does: woken, not asking.
    while !runtime.is_idle() {
        signal.wait(Duration::from_secs(10));
    }
    runtime.poll();
    runtime.draw(buffer.area, &mut buffer, &theme);
    assert!(text_of(&buffer).starts_with("keys:\n"));

    // Idle first, for the comparison; then with the script in a loop.
    let (idle, _) = owner_calls(&mut runtime, &mut buffer, Duration::from_millis(300));
    runtime.send(Event::Key(Key::named("x")));
    std::thread::sleep(Duration::from_millis(50));
    let (stuck, rounds) = owner_calls(&mut runtime, &mut buffer, Duration::from_millis(500));
    // The script was in its loop the whole time: nothing new was drawn,
    // and what the owner sent is still waiting.
    assert!(!runtime.is_idle());
    assert!(
        text_of(&buffer).starts_with("keys:\n"),
        "a stale screen, and no more"
    );
    println!(
        "start {to_start:?}; one send + poll + draw: worst {idle:?} idle, {stuck:?} with the script stuck ({rounds} rounds)"
    );
    // A stuck script costs its owner nothing: far under one frame.
    assert!(to_start < Duration::from_millis(200), "{to_start:?}");
    assert!(stuck < Duration::from_millis(100), "{stuck:?}");

    // Letting go of it does not wait for the loop either.
    let started = Instant::now();
    drop(runtime);
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn an_app_that_takes_no_events_is_shown_as_not_responding() {
    // No call is known that the budget does not end (ADR, decision 6). If
    // one exists, this is what its owner sees. Here the script is only in
    // a loop that a long budget lets run.
    let limits = Limits {
        call_time: Duration::from_secs(3),
        not_responding_after: Duration::ZERO,
        event_backlog: 8,
        ..Limits::default()
    };
    let signal = Signal::new();
    let mut runtime = Runtime::start(
        PROBE,
        Options::new("probe.lua", ana())
            .limits(limits)
            .waker(signal.waker()),
    )
    .expect("starts");
    while !runtime.is_idle() {
        signal.wait(Duration::from_secs(10));
    }
    runtime.poll();
    assert!(runtime.status().responding);
    runtime.send(Event::Key(Key::named("x")));
    // Events pile up behind the loop; past the backlog they are refused.
    let taken = (0..20)
        .filter(|_| runtime.send(Event::Key(Key::named("j"))))
        .count();
    assert_eq!(taken, 7, "the backlog is 8 with the key that loops");
    assert!(!runtime.status().responding);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 16));
    runtime.draw(buffer.area, &mut buffer, &AppTheme::default());
    let screen = text_of(&buffer);
    assert!(screen.starts_with("keys:\n"), "{screen}");
    assert!(screen.contains("The app is not responding."), "{screen}");
    // It can be let go of and started again.
    runtime.stop();
}

#[test]
fn a_call_that_outlives_its_budget_is_seen_from_outside() {
    // The script's thread gives a call two seconds. The owner's end is
    // told the budget is a tenth of a second: to it, the loop below is a
    // call that the budget did not end.
    let limits = Limits {
        call_time: Duration::from_secs(2),
        ..Limits::default()
    };
    let signal = Signal::new();
    let mut runtime = Runtime::start(
        PROBE,
        Options::new("probe.lua", ana())
            .limits(limits)
            .waker(signal.waker()),
    )
    .expect("starts");
    while !runtime.is_idle() {
        signal.wait(Duration::from_secs(10));
    }
    runtime.poll();
    runtime.limits.call_time = Duration::from_millis(100);
    runtime.limits.not_responding_after = Duration::from_millis(100);
    assert!(runtime.status().responding);
    runtime.send(Event::Key(Key::named("x")));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!runtime.status().responding);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 16));
    runtime.draw(buffer.area, &mut buffer, &AppTheme::default());
    assert!(text_of(&buffer).contains("The app is not responding."));
    // The call ends after all: the app is back, with its error.
    while !runtime.is_idle() {
        signal.wait(Duration::from_secs(10));
    }
    runtime.poll();
    let status = runtime.status();
    assert!(status.responding);
    assert!(
        status
            .error
            .unwrap_or_default()
            .contains("app.key ran longer than 2000 ms"),
    );
}

// --------------------------------------------------------------------- keys

#[test]
fn ctrl_c_never_reaches_a_script_and_host_quit_closes_the_app() {
    let mut app = probe();
    // However its owner sends it.
    for reserved in [
        Key::parse("ctrl+c").expect("a key"),
        Key::parse("ctrl+C").expect("a key"),
        Key::from_crossterm(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .expect("a key"),
        Key::from_crossterm(KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))
        .expect("a key"),
    ] {
        assert!(reserved.is_reserved(), "{reserved:?}");
        app.send(Event::Key(reserved));
    }
    // Every other key is the app's, Ctrl+D and a plain c among them.
    app.key("ctrl+d")
        .key("c")
        .key("alt+c")
        .key("esc")
        .key("tab")
        .key("f5");
    assert!(
        app.screen().starts_with("keys: ctrl+d c c esc tab f5\n"),
        "{}",
        app.screen()
    );
    assert!(!app.closed());
    app.key("q");
    assert!(app.closed());
    assert_eq!(app.phase(), Phase::Closed);
}

#[test]
fn a_key_is_named_as_the_contract_names_it() {
    let key = |code, modifiers| Key::from_crossterm(KeyEvent::new(code, modifiers)).expect("a key");
    let none = KeyModifiers::NONE;
    let j = key(KeyCode::Char('j'), none);
    assert_eq!(
        (j.name.as_str(), j.text.as_deref(), j.shift),
        ("j", Some("j"), false)
    );
    let upper = key(KeyCode::Char('J'), KeyModifiers::SHIFT);
    assert_eq!(
        (upper.name.as_str(), upper.text.as_deref(), upper.shift),
        ("J", Some("J"), true)
    );
    let space = key(KeyCode::Char(' '), none);
    assert_eq!(
        (space.name.as_str(), space.text.as_deref()),
        ("space", Some(" "))
    );
    let wide = key(KeyCode::Char('日'), none);
    assert_eq!(
        (wide.name.as_str(), wide.text.as_deref()),
        ("日", Some("日"))
    );
    let back_tab = key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(
        (back_tab.name.as_str(), back_tab.shift, back_tab.text),
        ("tab", true, None)
    );
    let ctrl_u = key(KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(
        (ctrl_u.name.as_str(), ctrl_u.ctrl, ctrl_u.text),
        ("u", true, None)
    );
    for (code, name) in [
        (KeyCode::Enter, "enter"),
        (KeyCode::Esc, "esc"),
        (KeyCode::Backspace, "backspace"),
        (KeyCode::Tab, "tab"),
        (KeyCode::Up, "up"),
        (KeyCode::Down, "down"),
        (KeyCode::Left, "left"),
        (KeyCode::Right, "right"),
        (KeyCode::Home, "home"),
        (KeyCode::End, "end"),
        (KeyCode::PageUp, "pageup"),
        (KeyCode::PageDown, "pagedown"),
        (KeyCode::Delete, "delete"),
        (KeyCode::Insert, "insert"),
        (KeyCode::F(1), "f1"),
        (KeyCode::F(12), "f12"),
    ] {
        let named = key(code, none);
        assert_eq!((named.name.as_str(), named.text.clone()), (name, None));
        // And a key written by a person reads back the same.
        assert_eq!(Key::parse(name), Some(named));
    }
    // What the contract has no name for is not a key of an app.
    assert_eq!(
        Key::from_crossterm(KeyEvent::new(KeyCode::F(13), none)),
        None
    );
    assert_eq!(
        Key::from_crossterm(KeyEvent::new(KeyCode::CapsLock, none)),
        None
    );
    assert_eq!(Key::parse("hyper+x"), None);
    assert_eq!(Key::parse("return"), None);
    assert_eq!(Key::parse("+").map(|k| k.name), Some("+".into()));
    assert_eq!(
        Key::parse("ctrl++").map(|k| (k.name, k.ctrl)),
        Some(("+".into(), true))
    );
}

// ------------------------------------------------------------------- timers

#[test]
fn a_timer_fires_once_when_its_time_has_come() {
    let mut app = probe();
    app.key("1");
    // Time does not pass by itself: a tick before the time does nothing.
    app.advance(99);
    assert!(app.screen().contains("fired:\n"), "{}", app.screen());
    app.advance(1);
    assert!(app.screen().contains("fired: a\n"), "{}", app.screen());
    app.advance(100);
    assert!(app.screen().contains("fired: a\n"));
    // A tag is plain data, and comes back as data does: null as nil.
    app.advance(100);
    assert!(
        app.screen().contains("fired: a n2+nil\n"),
        "{}",
        app.screen()
    );
    app.advance(10_000);
    assert!(app.screen().contains("fired: a n2+nil\n"));
    // A timer may set the next one.
    app.key("2")
        .advance(100)
        .advance(100)
        .advance(100)
        .advance(100);
    assert!(
        app.screen().contains("fired: a n2+nil again\n"),
        "{}",
        app.screen()
    );
}

#[test]
fn timers_that_are_due_together_fire_in_their_order() {
    let source = r#"
        local app = { api = 1, types = { "note" } }
        function app.init(ctx)
          host.after(300, "c"); host.after(100, "a1"); host.after(200, "b"); host.after(100, "a2")
          return { fired = {} }
        end
        function app.timer(s, tag) s.fired[#s.fired + 1] = tag end
        function app.view(s) return table.concat(s.fired, " ") end
        return app
    "#;
    let mut app = start("app.lua", source);
    app.advance(1000);
    assert_eq!(app.screen().trim_end(), "a1 a2 b c");
}

// ---------------------------------------------------------------------- who

#[test]
fn a_script_is_told_who_it_runs_for() {
    let mut app = probe();
    assert!(
        app.screen().contains("user: ana a1a1 40x16"),
        "{}",
        app.screen()
    );
    // A name can change; the id does not.
    app.send(Event::User(Who::new("a1".repeat(32), "Ana María")));
    assert!(
        app.screen().contains("user: Ana María a1a1 40x16"),
        "{}",
        app.screen()
    );
    app.resize(50, 20);
    assert!(
        app.screen().contains("user: Ana María a1a1 50x20"),
        "{}",
        app.screen()
    );
}

// ------------------------------------------------------------------- output

#[test]
fn the_owner_is_told_what_a_version_declares() {
    let signal = Signal::new();
    let mut runtime = Runtime::start(
        HELLO,
        Options::new("hello.lua", ana())
            .limits(Limits::patient())
            .waker(signal.waker()),
    )
    .expect("starts");
    let mut outputs = Vec::new();
    while !runtime.is_idle() {
        signal.wait(Duration::from_secs(10));
        outputs.extend(runtime.poll());
    }
    outputs.extend(runtime.poll());
    assert_eq!(
        outputs[0],
        Output::Loaded {
            types: vec!["note".into()]
        }
    );
    // The request `init` made, with the script's short type name.
    assert!(
        matches!(&outputs[1], Output::Request(request)
            if request.op.name() == "list" && request.op.record_type() == "note"),
        "{outputs:?}"
    );
    assert_eq!(outputs.last(), Some(&Output::Redraw));
    assert_eq!(runtime.timers(), 0);
}

// ------------------------------------------------------------ the driver

#[test]
fn a_script_is_tried_without_a_window() {
    use crate::apps::runtime::headless::{Step, try_app};
    // What `bzz app try` and the agents' tool will do: load a script, do
    // steps to it, and read the screen as text with any error.
    let steps: Vec<Step> = ["n", "type:buy milk", "enter", "resize:30x6", "wait:500"]
        .iter()
        .map(|written| Step::parse(written).expect("a step"))
        .collect();
    let tried = try_app(HELLO, HeadlessOptions::new("hello.lua"), &steps).expect("runs");
    assert_eq!(tried.screen, "Notes (1)\n> ana: buy milk\n\n\n\nSaved.\n");
    assert_eq!(tried.error, None);
    assert_eq!(tried.types, ["note"]);
    assert!(tried.responding && !tried.closed);

    // A script with a fault comes back with its file and line, and with
    // the screen it had.
    let faulty = HELLO.replace(
        "s.form = { value = \"\" }",
        "s.form = { value = nil .. \"\" }",
    );
    let tried = try_app(
        faulty,
        HeadlessOptions::new("hello.lua"),
        &[Step::parse("n").expect("a step")],
    )
    .expect("runs");
    let error = tried.error.expect("an error");
    assert!(
        error.starts_with("in app.key: hello.lua:")
            && error.contains("attempt to concatenate a nil value"),
        "{error}"
    );
    assert!(tried.screen.starts_with("Notes (0)\n"), "{}", tried.screen);
    assert!(
        tried
            .logs
            .iter()
            .any(|l| l.starts_with("error: in app.key: hello.lua:"))
    );

    // One that does not load, and one that never comes back.
    let tried = try_app("return 1", HeadlessOptions::new("x.lua"), &[]).expect("runs");
    assert!(
        tried
            .error
            .unwrap_or_default()
            .contains("must end with `return app`")
    );
    assert!(tried.types.is_empty());
    let tried = try_app(
        PROBE,
        HeadlessOptions::new("probe.lua"),
        &[
            Step::parse("x").expect("a step"),
            Step::parse("x").expect("a step"),
            Step::parse("x").expect("a step"),
            Step::parse("q").expect("a step"),
        ],
    )
    .expect("runs");
    assert!(tried.error.unwrap_or_default().contains("3 times in a row"));
    assert!(tried.responding && !tried.closed);

    // The steps as a person writes them.
    assert_eq!(
        Step::parse("ctrl+u"),
        Ok(Step::Key(Key::parse("ctrl+u").expect("a key")))
    );
    assert_eq!(Step::parse("type:a b"), Ok(Step::Type("a b".into())));
    assert_eq!(
        Step::parse("paste:two\nlines"),
        Ok(Step::Paste("two\nlines".into()))
    );
    assert_eq!(Step::parse("resize:40x16"), Ok(Step::Resize(40, 16)));
    assert_eq!(Step::parse("wait:250"), Ok(Step::Advance(250)));
    assert!(Step::parse("resize:wide").is_err());
    assert!(Step::parse("wait:soon").is_err());
    assert!(Step::parse("return").unwrap_err().contains("not a key"));
}
