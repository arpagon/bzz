//! Records as a script sees them, on the in-memory store: who, order, the
//! error codes, a second participant, a quota, a lost connection, and a set
//! that had to be fetched again.

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::apps::runtime::memory::content;
use crate::apps::runtime::records::RecordError;
use crate::apps::runtime::{Event, RecordReply};

/// A script that shows every record it holds and everything it was told.
const LEDGER: &str = r#"
local app = { api = 1, types = { "note" } }

local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id
end

function app.init(ctx)
  data.list { type = "note", tag = "load" }
  return { notes = {}, told = {} }
end

local function say(s, text) s.told[#s.told + 1] = text end

function app.view(s, ctx)
  local lines = {}
  table.sort(s.notes, by_order)
  for i, n in ipairs(s.notes) do
    lines[i] = tostring(n.content.text) .. " by " .. n.by.name .. "/" .. n.by.id:sub(1, 2)
  end
  lines[#lines + 1] = "--"
  for _, t in ipairs(s.told) do lines[#lines + 1] = t end
  return ui.text(lines)
end

local function find(s, id)
  for i, n in ipairs(s.notes) do if n.id == id then return i end end
end

function app.key(s, key, ctx)
  local n, first = key.name, s.notes[1]
  if n == "c" then
    data.create { type = "note", content = { text = "mine", order = host.time(), holder = data.null, n = 3, f = 2.5, list = { 1, data.null, 3 } }, tag = "create" }
  elseif n == "u" then
    data.update { record_id = first.id, rev = first.rev, content = { text = "edited", order = first.content.order }, tag = "update" }
  elseif n == "d" then
    data.delete { record_id = first.id, rev = first.rev, tag = "delete" }
  elseif n == "r" then
    data.list { type = "note", tag = "load" }
  elseif n == "b" then
    local ok, err = pcall(data.update, { record_id = first.id, rev = 7, content = {} })
    say(s, tostring(ok) .. " " .. tostring(err))
  elseif n == "x" then
    s.told = {}
  end
end

function app.reply(s, r, ctx)
  if not r.ok then
    local e = r.error
    say(s, r.op .. " " .. e.code .. ": " .. e.message
      .. (e.retry_after and (" [retry_after " .. e.retry_after .. "]") or "")
      .. (r.current and (" [current " .. r.current.content.text .. " by " .. r.current.by.name .. "]") or ""))
    return
  end
  if r.op == "list" then
    s.notes = r.records
    say(s, "list " .. #r.records)
  elseif r.op == "delete" then
    local i = find(s, r.record.id)
    if i then table.remove(s.notes, i) end
    say(s, "deleted " .. r.record.content.text)
  else
    local i = find(s, r.record.id)
    if i then s.notes[i] = r.record else s.notes[#s.notes + 1] = r.record end
    local c = r.record.content
    say(s, r.op .. " ok rev " .. type(r.record.rev) .. " holder " .. tostring(c.holder)
      .. " n " .. math.type(c.n or 0) .. " f " .. tostring(c.f) .. " list " .. tostring(c.list and c.list[2]))
  end
end

function app.push(s, p, ctx)
  local i = find(s, p.record.id)
  if p.change == "delete" then
    if i then table.remove(s.notes, i) end
  elseif i then s.notes[i] = p.record
  else s.notes[#s.notes + 1] = p.record end
  say(s, "push " .. p.change .. " " .. tostring(p.record.content.text) .. " by " .. p.by.name)
end

return app
"#;

fn three_notes(store: &mut MemoryStore) {
    for (order, text) in ["one", "two", "three"].into_iter().enumerate() {
        store.seed(
            "note",
            content(json!({ "text": text, "order": order + 1 })),
            &bo(),
        );
    }
}

fn ledger() -> Headless {
    start_on("ledger.lua", LEDGER, (160, 24), three_notes)
}

/// What the script was told, after its records.
fn told(app: &mut Headless) -> Vec<String> {
    app.screen()
        .lines()
        .skip_while(|l| *l != "--")
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn notes(app: &mut Headless) -> Vec<String> {
    app.screen()
        .lines()
        .take_while(|l| *l != "--")
        .map(str::to_string)
        .collect()
}

#[test]
fn a_list_has_no_order_and_the_script_keeps_its_own() {
    let mut app = ledger();
    // The store hands the three records out by id, which says nothing of
    // when they were made.
    let handed: Vec<String> = {
        let mut records = app.store().records().to_vec();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        records
            .iter()
            .map(|r| r.content["text"].as_str().unwrap_or("").to_string())
            .collect()
    };
    assert_ne!(
        handed,
        ["one", "two", "three"],
        "the ids happen to sort by creation"
    );
    // The script sorts by the order field it keeps in the record.
    assert_eq!(
        notes(&mut app),
        ["one by bo/b0", "two by bo/b0", "three by bo/b0"]
    );
    assert_eq!(told(&mut app), ["list 3"]);
}

#[test]
fn a_record_says_who_signed_its_last_revision() {
    let mut app = ledger();
    // The app's own write is signed by the local participant, and replaces
    // who the record was by: any member can revise any record.
    app.key("u");
    assert_eq!(notes(&mut app)[0], "edited by ana/a1");
    // Its own write does not come back as a push.
    assert_eq!(told(&mut app).len(), 2);
    let id = app
        .store()
        .records()
        .iter()
        .find(|r| r.content["text"] == "two")
        .map(|r| r.id.clone());
    let pushes = app.store().remote_update(
        &Who::new("c0".repeat(32), "cy"),
        &id.expect("two"),
        content(json!({ "text": "two!", "order": 2 })),
    );
    app.deliver(pushes);
    assert_eq!(notes(&mut app)[1], "two! by cy/c0");
    assert_eq!(
        told(&mut app).last().map(String::as_str),
        Some("push update two! by cy")
    );
}

#[test]
fn null_and_numbers_cross_as_the_contract_says() {
    let mut app = ledger();
    app.key("c");
    // Sent: data.null is null, also in a list; an integer stays one.
    let stored = app
        .store()
        .find_by("text", "mine")
        .expect("the record")
        .content
        .clone();
    assert_eq!(
        serde_json::Value::Object(stored),
        json!({ "text": "mine", "order": START_OF_TIME as f64, "holder": null, "n": 3, "f": 2.5, "list": [1, null, 3] })
    );
    // Received: null is nil, never data.null; a revision is a string.
    assert_eq!(
        told(&mut app).last().map(String::as_str),
        Some("create ok rev string holder nil n integer f 2.5 list nil")
    );
    // A revision is opaque: only the string a record came with is taken.
    app.key("b");
    let last = told(&mut app).pop().unwrap_or_default();
    assert!(
        last.starts_with("false ledger.lua:")
            && last.ends_with(
                "data.update: 'rev' must be the string the record came with, got integer"
            ),
        "{last}"
    );
}

#[test]
fn every_error_code_reaches_the_script() {
    let mut app = ledger();
    let first = app.store().find_by("text", "one").expect("one").id.clone();

    // conflict, with the record as it is now.
    app.store().arm_conflict(
        &first,
        &bo(),
        content(json!({ "text": "bo's", "order": 1 })),
    );
    app.key("u");
    assert_eq!(
        told(&mut app)[1..],
        [
            "push update bo's by bo",
            "update conflict: the record was changed by someone else [current bo's by bo]"
        ]
    );
    // not_found: bo deleted it while the app was not told.
    app.key("x");
    app.store().lose_connection();
    let nothing = app.store().remote_delete(&bo(), &first);
    assert!(nothing.is_empty());
    app.store().set_available(true);
    app.key("d");
    assert_eq!(told(&mut app), ["delete not_found: no such record"]);
    // unavailable.
    app.key("x").key("r");
    app.store().lose_connection();
    app.key("u");
    assert_eq!(
        told(&mut app)[1],
        "update unavailable: no connection to the relay"
    );
    app.store().set_available(true);
    // rate_limited, with the seconds to wait.
    app.key("x");
    app.store().set_quota(2, Duration::from_secs(60));
    app.key("c")
        .advance(20_000)
        .key("c")
        .advance(10_500)
        .key("c");
    assert_eq!(
        told(&mut app)[2],
        "create rate_limited: too many writes; try again in 30 s [retry_after 30]"
    );
    // refused, with the relay's reason. Reading still works.
    app.key("x").advance(60_000);
    app.store()
        .set_refusal(Some("restricted: guests cannot write to this channel"));
    app.key("c").key("r");
    assert_eq!(
        told(&mut app),
        [
            "create refused: restricted: guests cannot write to this channel",
            "list 4"
        ]
    );
    app.store().set_refusal(None);
    // limit: the app has as many records as it may.
    app.key("x");
    app.store().live_limit = 4;
    app.key("c");
    assert_eq!(
        told(&mut app),
        ["create limit: the app has 4 records, the most it may have"]
    );
    // bad_request and too_large from the store itself, as a relay service
    // would send them.
    app.key("x");
    app.store().fail_next(
        "list",
        RecordError::BadRequest {
            message: "no such channel".into(),
        },
    );
    app.store().content_limit = 10;
    app.store().live_limit = 1000;
    app.key("r").key("c");
    let told = told(&mut app);
    assert_eq!(told[0], "list bad_request: no such channel");
    assert!(
        told[1].starts_with("create too_large: the content is "),
        "{told:?}"
    );
}

#[test]
fn a_resync_is_not_a_change() {
    let mut app = ledger();
    let before = app.screen();
    // bzz had to fetch the set again, and nothing had changed: the script
    // is told nothing at all.
    app.store().lose_connection();
    let pushes = app.store().resync();
    assert!(pushes.is_empty());
    app.deliver(pushes);
    assert_eq!(app.screen(), before);

    // This time the others went on writing while the app could not hear.
    app.store().lose_connection();
    let two = app.store().find_by("text", "two").expect("two").id.clone();
    let three = app
        .store()
        .find_by("text", "three")
        .expect("three")
        .id
        .clone();
    assert!(
        app.store()
            .remote_update(
                &bo(),
                &two,
                content(json!({ "text": "two v2", "order": 2 }))
            )
            .is_empty()
    );
    assert!(app.store().remote_delete(&bo(), &three).is_empty());
    assert!(
        app.store()
            .remote_create(
                &bo(),
                "note",
                content(json!({ "text": "four", "order": 4 }))
            )
            .1
            .is_empty()
    );
    // And a change that was undone again before the app could see it.
    let (gone, _) = app.store().remote_create(
        &bo(),
        "note",
        content(json!({ "text": "never seen", "order": 5 })),
    );
    app.store().remote_delete(&bo(), &gone.id);
    assert_eq!(app.screen(), before, "nothing reached the app yet");

    let pushes = app.store().resync();
    app.deliver(pushes);
    // Only the real differences, as pushes. The record that did not change
    // is not deleted and created again.
    let mut seen = told(&mut app);
    seen.remove(0);
    seen.sort();
    assert_eq!(
        seen,
        [
            "push create four by bo",
            "push delete three by bo",
            "push update two v2 by bo"
        ]
    );
    assert_eq!(
        notes(&mut app),
        ["one by bo/b0", "two v2 by bo/b0", "four by bo/b0"]
    );
    // The app can change what it learned of in the resync.
    app.key("x").key("d");
    assert_eq!(told(&mut app), ["deleted one"]);
}

#[test]
fn a_reply_nobody_waits_for_is_ignored() {
    let mut app = ledger();
    let before = app.screen();
    app.send(Event::Reply(RecordReply {
        id: 4242,
        result: Err(RecordError::NotFound),
    }));
    assert_eq!(app.screen(), before);
    assert_eq!(app.error(), None);
}
