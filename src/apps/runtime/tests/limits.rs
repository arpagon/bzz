//! Every limit of `limits.rs`, reached and passed: what a script is told,
//! and that the app goes on.

use serde_json::json;

use super::*;
use crate::apps::runtime::Phase;
use crate::apps::runtime::limits;
use crate::apps::runtime::memory::content;

/// A script whose keys do the things that have a limit. What it was told
/// is the first line of its screen.
const GREEDY: &str = r#"
local app = { api = 1, types = { "note" } }

function app.init(ctx) return { said = "", n = 0, replies = {} } end

function app.view(s, ctx)
  if s.screen == "items" then
    local items = {}
    for i = 1, s.count do items[i] = "item " .. i end
    return ui.list { items = items }
  elseif s.screen == "text" then
    return ui.text { string.rep("x", s.count), wrap = true }
  elseif s.screen == "deep" then
    local inner = ui.text { "bottom" }
    for i = 1, s.count do inner = ui.column { inner } end
    return inner
  elseif s.screen == "parts" then
    local parts = {}
    for i = 1, s.count do parts[i] = ui.text { "p" } end
    return ui.row(parts)
  elseif s.screen == "constructor" then
    local items = {}
    for i = 1, s.count do items[i] = "x" end
    return ui.text(items)
  end
  return ui.text { "said: " .. s.said, "n=" .. s.n, "replies: " .. table.concat(s.replies, " "), wrap = true }
end

local function try(s, f)
  local ok, err = pcall(f)
  s.said = ok and "ok" or err
end

function app.key(s, key, ctx)
  local n = key.name
  s.screen = nil
  if n == "t" then
    try(s, function() for i = 1, s.count do host.after(100 + i, i); s.n = i end end)
  elseif n == "a" then
    try(s, function() host.after(s.count, "x") end)
  elseif n == "l" then
    try(s, function() for i = 1, s.count do data.list { type = "note", tag = i }; s.n = i end end)
  elseif n == "g" then
    for i = 1, s.count do host.log("line " .. i) end
  elseif n == "L" then
    host.log(string.rep("é", 1000), { a = 1, none = data.null }, nil, 2.5, true, data.null, print)
    print("tab", "separated")
  elseif n == "c" then
    try(s, function() s.id = data.create { type = "note", content = { text = string.rep("x", s.count) }, tag = "big" } end)
  elseif n == "T" then
    try(s, function() data.list { type = "note", tag = string.rep("x", s.count) } end)
  elseif n == "m" then
    try(s, function() data.list { type = "note", match = string.rep("x", s.count) } end)
  else
    s.screen = ({ i = "items", x = "text", d = "deep", p = "parts", k = "constructor" })[n]
  end
end

function app.paste(s, text, ctx)
  local n = tonumber(text)
  if n then s.count = n else s.said = "pasted " .. #text end
end

function app.reply(s, r, ctx)
  s.replies[#s.replies + 1] = r.ok and "ok" or r.error.code
  if r.tag == "big" then
    s.said = (r.ok and "stored" or r.error.code .. ": " .. r.error.message)
      .. " / echo content " .. type(r.request.content) .. " / id " .. tostring(r.id == s.id)
  end
end

return app
"#;

fn greedy() -> Headless {
    Headless::start(GREEDY, HeadlessOptions::new("greedy.lua").size(100, 16)).expect("starts")
}

/// Sets the script's `count` (by a paste) and presses `key`.
fn with(app: &mut Headless, count: usize, key: &str) -> String {
    app.paste(&count.to_string()).key(key);
    app.screen()
}

fn said(screen: &str) -> String {
    screen
        .split("n=")
        .next()
        .unwrap_or("")
        .replace('\n', " ")
        .trim()
        .to_string()
}

#[test]
fn a_script_larger_than_the_limit_is_not_loaded() {
    let padded = |bytes: usize| {
        let mut source = String::from(HELLO);
        source.push_str("\n--");
        while source.len() < bytes {
            source.push('x');
        }
        source
    };
    let app = start("hello.lua", &padded(limits::SCRIPT_BYTES));
    assert_eq!(app.error(), None);
    assert_eq!(app.phase(), Phase::Running);
    let app = start("hello.lua", &padded(limits::SCRIPT_BYTES + 1));
    assert_eq!(
        app.error().as_deref(),
        Some("hello.lua: the script is 131073 bytes; an app's script may be 131072")
    );
    assert_eq!(app.phase(), Phase::NotLoaded);
}

#[test]
fn a_screen_past_its_limits_is_refused_and_the_last_one_stays() {
    let mut app = greedy();
    let good = app.screen();
    for (count, key, wanted) in [
        // Every item of a list is an element, with the list and the screen.
        (limits::SCREEN_ELEMENTS - 2, "i", None),
        (
            limits::SCREEN_ELEMENTS,
            "i",
            Some(
                "ui.list: the screen has more than 10000 elements (every element, line, span and cell counts)",
            ),
        ),
        (limits::SCREEN_TEXT_BYTES, "x", None),
        (
            limits::SCREEN_TEXT_BYTES + 1,
            "x",
            Some("ui.text: item [1]: the screen has more than 131072 bytes of text"),
        ),
        (limits::SCREEN_DEPTH, "d", None),
        (
            limits::SCREEN_DEPTH + 1,
            "d",
            Some("elements nest deeper than 32 levels"),
        ),
        // A row of thousands of parts is only a screen with many elements.
        (limits::SCREEN_ELEMENTS / 2 - 1, "p", None),
        (
            limits::SCREEN_ELEMENTS / 2,
            "p",
            Some("the screen has more than 10000 elements"),
        ),
        (limits::SCREEN_ELEMENTS - 2, "k", None),
        (
            limits::SCREEN_ELEMENTS + 1,
            "k",
            Some("ui.text has more than 10000 items; a screen cannot hold them"),
        ),
    ] {
        app.key("z");
        assert_eq!(app.error(), None);
        let screen = with(&mut app, count, key);
        match wanted {
            None => {
                assert_eq!(app.error(), None, "{count} {key}");
                assert_ne!(screen, good);
            }
            Some(wanted) => {
                let error = app.error().unwrap_or_default();
                assert!(error.contains(wanted), "{count} {key}: {error}");
                assert!(error.contains("greedy.lua:"), "{error}");
                assert_eq!(app.phase(), Phase::Running);
            }
        }
    }
}

#[test]
fn timers_have_a_number_and_a_least_interval() {
    let mut app = greedy();
    let screen = with(&mut app, limits::TIMERS, "t");
    assert_eq!(said(&screen), "said: ok", "{screen}");
    let screen = with(&mut app, 1, "t");
    assert!(
        said(&screen).ends_with(
            "host.after: 32 timers are waiting already; no more are taken until one fires"
        ),
        "{screen}"
    );
    // When they have fired there is room again.
    app.advance(1000);
    assert_eq!(said(&with(&mut app, 1, "t")), "said: ok");
    for (ms, ok) in [
        (99, false),
        (100, true),
        (86_400_000, true),
        (86_400_001, false),
        (0, false),
    ] {
        app.advance(90_000_000);
        let screen = with(&mut app, ms, "a");
        if ok {
            assert_eq!(said(&screen), "said: ok", "{ms}");
        } else {
            assert!(
                said(&screen).ends_with(&format!(
                    "host.after: the delay must be from 100 to 86400000 ms, got {ms}"
                )),
                "{ms}: {screen}"
            );
        }
    }
}

#[test]
fn requests_that_wait_and_requests_in_a_minute_have_a_number() {
    let mut app = greedy();
    // Sixty-four replies are held back: the sixty-fifth request is refused.
    for _ in 0..limits::PENDING_REQUESTS {
        app.store().hold_next("list");
    }
    let screen = with(&mut app, limits::PENDING_REQUESTS + 1, "l");
    assert!(
        said(&screen).ends_with(
            "data.list: 64 requests are still waiting for their reply; no more are taken until one arrives"
        ),
        "{screen}"
    );
    assert!(screen.contains("n=64"), "{screen}");
    assert_eq!(app.store().count("list"), 64);
    let replies = app.store().release();
    assert_eq!(replies.len(), 64);
    app.deliver(replies);
    // 600 in a minute, each answered: the next is refused until the
    // minute has passed.
    for _ in 0..8 {
        assert_eq!(said(&with(&mut app, 64, "l")), "said: ok");
    }
    let screen = with(&mut app, 30, "l");
    assert!(
        said(&screen).ends_with("data.list: more than 600 requests in a minute"),
        "{screen}"
    );
    assert!(screen.contains("n=24"), "{screen}");
    assert_eq!(
        app.store().count("list"),
        limits::REQUESTS_PER_MINUTE as usize
    );
    app.advance(60_000);
    assert_eq!(said(&with(&mut app, 30, "l")), "said: ok");
}

#[test]
fn the_log_has_a_volume_and_a_line_has_a_length() {
    let mut app = greedy();
    with(&mut app, 200, "g");
    assert_eq!(app.logs().len(), limits::LOG_LINES_PER_MINUTE as usize);
    assert_eq!(app.logs()[119], "line 120");
    // The next line that is taken says what was lost.
    app.advance(60_000);
    app.key("L");
    let logs = app.logs();
    assert_eq!(
        logs[120],
        "(80 lines were dropped: more than 120 in a minute)"
    );
    // A line is cut on a character; values are written without asking the
    // script anything.
    assert_eq!(logs[121].len(), limits::LOG_LINE_BYTES);
    assert!(logs[121].chars().all(|c| c == 'é'));
    assert_eq!(logs[122], "tab\tseparated");
    app.advance(60_000);
    let source = GREEDY.replace("string.rep(\"é\", 1000), ", "");
    app.reload(source).key("L");
    assert!(
        app.logs()
            .contains(&r#"{"a":1,"none":null} nil 2.5 true data.null function"#.to_string()),
        "{:?}",
        app.logs()
    );
}

#[test]
fn content_that_is_too_large_is_answered_without_being_sent() {
    let mut app = greedy();
    // {"text":"xxx…"} : eleven bytes around the text.
    let screen = with(&mut app, limits::CONTENT_BYTES - 11, "c");
    assert_eq!(
        said(&screen),
        "said: stored / echo content table / id true",
        "{screen}"
    );
    assert_eq!(app.store().count("create"), 1);
    let screen = with(&mut app, limits::CONTENT_BYTES - 10, "c");
    assert_eq!(
        said(&screen),
        "said: too_large: the content is larger than 16384 bytes as JSON / echo content nil / id true",
        "{screen}"
    );
    assert_eq!(app.store().count("create"), 1, "nothing was sent");
    assert_eq!(app.error(), None);
    // A tag and a match are bzz's to keep, and have their own sizes.
    assert_eq!(
        said(&with(&mut app, limits::TAG_BYTES - 2, "T")),
        "said: ok"
    );
    assert!(
        said(&with(&mut app, limits::TAG_BYTES - 1, "T"))
            .ends_with("data.list: 'tag' is larger than 4096 bytes as JSON")
    );
    assert_eq!(said(&with(&mut app, limits::MATCH_BYTES, "m")), "said: ok");
    assert!(
        said(&with(&mut app, limits::MATCH_BYTES + 1, "m"))
            .ends_with("data.list: 'match' is longer than 256 bytes")
    );
}

#[test]
fn a_paste_that_is_too_long_is_not_given_to_the_script() {
    let mut app = greedy();
    app.paste(&"p".repeat(limits::PASTE_BYTES));
    assert_eq!(said(&app.screen()), "said: pasted 65536");
    app.paste(&"p".repeat(limits::PASTE_BYTES + 1));
    assert_eq!(
        app.error().as_deref(),
        Some("a paste of 65537 bytes was not given to the app: more than 65536")
    );
    assert_eq!(app.screen().lines().next(), Some("said: pasted 65536"));
}

/// A script that lists its records and says how many it holds.
const HOLDER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) data.list { type = "note" } return { n = "none yet", lists = 0 } end
function app.key(s, key, ctx) data.list { type = "note" } end
function app.reply(s, r, ctx)
  if r.ok then s.records = r.records; s.n = #r.records; s.lists = s.lists + 1 end
end
function app.view(s, ctx) return "holds " .. s.n .. " after " .. s.lists .. " lists" end
return app
"#;

#[test]
fn the_published_limits_fit_in_the_memory_of_one_app() {
    // The fullest set an app may have, as text: a thousand records of
    // sixteen kibibytes. The app must be able to hold it, and to list it
    // again while it still holds the first answer.
    let text = "x".repeat(limits::CONTENT_BYTES - 11);
    let mut app = Headless::start_on(
        HOLDER,
        HeadlessOptions::new("holder.lua"),
        {
            let clock = ManualClock::new(START_OF_TIME);
            let mut store = MemoryStore::new(ana(), clock);
            for _ in 0..limits::LIVE_RECORDS {
                store.seed("note", content(json!({ "text": text })), &bo());
            }
            store
        },
        ManualClock::new(START_OF_TIME),
    )
    .expect("starts");
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().trim_end(), "holds 1000 after 1 lists");
    app.key("r").key("r");
    assert_eq!(app.error(), None);
    assert_eq!(app.screen().trim_end(), "holds 1000 after 3 lists");
    // And the store takes no more than the limit.
    assert_eq!(app.store().records().len(), limits::LIVE_RECORDS);
}

#[test]
fn a_record_set_too_big_for_the_memory_is_an_error_in_the_app() {
    // The same thousand records, each a list of small numbers: the same
    // bytes of JSON, eight times the memory once they are Lua values. Any
    // member of a channel can write such records. The app that lists them
    // is told it ran out of memory; bzz is not harmed.
    let numbers: Vec<u8> = vec![1; (limits::CONTENT_BYTES - 12) / 2];
    let mut app = Headless::start_on(
        HOLDER,
        HeadlessOptions::new("holder.lua"),
        {
            let clock = ManualClock::new(START_OF_TIME);
            let mut store = MemoryStore::new(ana(), clock);
            for _ in 0..limits::LIVE_RECORDS {
                store.seed("note", content(json!({ "list": numbers })), &bo());
            }
            store
        },
        ManualClock::new(START_OF_TIME),
    )
    .expect("starts");
    // Eight million values take a debug build a while to hand over.
    assert!(app.wait(std::time::Duration::from_secs(300)));
    assert_eq!(
        app.error().as_deref(),
        Some("app.reply ran out of memory: an app may hold 64 MiB")
    );
    assert_eq!(app.phase(), Phase::Running);
    assert_eq!(
        app.screen().lines().next(),
        Some("holds none yet after 0 lists")
    );
}
