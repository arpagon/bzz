//! The session's own rules, on the store in memory: the same session bzz's
//! window runs on the relay (`tests/app_session_test.rs` drives it there).
//! These run on every platform.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::headless::{Headless, HeadlessOptions, START_OF_TIME};
use super::memory::{MemoryStore, content};
use super::*;
use crate::apps::runtime::clock::ManualClock;
use crate::apps::runtime::{RecordOp, RecordRequest};
use crate::protocol::record::{APP_WRITES_PER_MINUTE, MAX_TITLE_BYTES};

const BOARD: &str = include_str!("../../../../tests/fixtures/apps/board/app.lua");

/// A script that shows who it runs for, every record it holds and
/// everything it was told. Its keys ask for what a test wants asked.
const LEDGER: &str = r#"
local app = { api = 1, types = { "note", "tag" } }

local function by_order(a, b)
  local x, y = tonumber(a.content.order) or 0, tonumber(b.content.order) or 0
  if x ~= y then return x < y end
  return a.id < b.id
end

function app.init(ctx)
  data.list { type = "note", tag = "load" }
  return { notes = {}, told = {}, query = "" }
end

local function say(s, text) s.told[#s.told + 1] = text end

function app.view(s, ctx)
  local lines = { "me: " .. ctx.user.name .. "/" .. ctx.user.id:sub(1, 2) }
  table.sort(s.notes, by_order)
  for _, n in ipairs(s.notes) do
    lines[#lines + 1] = tostring(n.content.title or n.content.text) .. " by " .. n.by.name
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
    s.made = (s.made or 0) + 1
    data.create { type = "note", content = { title = "mine " .. s.made, order = 100 + s.made }, tag = "create" }
  elseif n == "u" then
    data.update { record_id = first.id, rev = first.rev, content = { title = "edited", order = first.content.order }, tag = "update" }
  elseif n == "d" then
    data.delete { record_id = first.id, rev = first.rev, tag = "delete" }
  elseif n == "r" then
    data.list { type = "note", tag = "load" }
  elseif n == "m" then
    data.list { type = "note", match = s.query, tag = "match" }
  elseif n == "t" then
    data.list { type = "tag", tag = "tags" }
  elseif n == "x" then
    s.told = {}
  end
end

function app.paste(s, text, ctx) s.query = text end

function app.reply(s, r, ctx)
  if not r.ok then
    local e = r.error
    say(s, r.op .. " " .. e.code .. ": " .. e.message
      .. (e.retry_after and (" [retry_after " .. e.retry_after .. "]") or ""))
    return
  end
  if r.tag == "match" or r.tag == "tags" then
    local titles = {}
    for i, n in ipairs(r.records) do titles[i] = tostring(n.content.title or n.content.text):gsub("%c", "_") end
    table.sort(titles)
    say(s, r.tag .. " " .. #r.records .. ": " .. table.concat(titles, ", "))
  elseif r.op == "list" then
    s.notes = r.records
    say(s, "list " .. #r.records)
  elseif r.op == "delete" then
    local i = find(s, r.record.id)
    if i then table.remove(s.notes, i) end
    say(s, "deleted " .. tostring(r.record.content.title))
  else
    local i = find(s, r.record.id)
    if i then s.notes[i] = r.record else s.notes[#s.notes + 1] = r.record end
    say(s, r.op .. " ok " .. tostring(r.record.content.title))
  end
end

function app.push(s, p, ctx)
  if p.record.type == "note" then
    local i = find(s, p.record.id)
    if p.change == "delete" then
      if i then table.remove(s.notes, i) end
    elseif i then s.notes[i] = p.record
    else s.notes[#s.notes + 1] = p.record end
  end
  say(s, "push " .. p.change .. " " .. tostring(p.record.content.title or p.record.content.text) .. " by " .. p.by.name)
end

return app
"#;

/// A script that counts what it is told.
const COUNTER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) data.list { type = "note" } return { pushes = 0, listed = -1 } end
function app.view(s, ctx) return ui.text { "listed " .. s.listed, "pushes " .. s.pushes } end
function app.reply(s, r, ctx) if r.ok then s.listed = #r.records end end
function app.push(s, p, ctx) s.pushes = s.pushes + 1 end
return app
"#;

fn ana() -> Who {
    Who::new("a1".repeat(32), "ana")
}

fn bo() -> Who {
    Who::new("b0".repeat(32), "bo")
}

fn cy() -> Who {
    Who::new("c0".repeat(32), "cy")
}

fn three_notes(store: &mut MemoryStore) {
    for (order, title) in ["one", "two", "three"].into_iter().enumerate() {
        store.seed(
            "note",
            content(json!({ "title": title, "order": order + 1 })),
            &bo(),
        );
    }
}

fn start_on(name: &str, source: &str, seed: impl FnOnce(&mut MemoryStore)) -> Headless {
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(ana(), clock.clone());
    seed(&mut store);
    Headless::start_on(
        source,
        HeadlessOptions::new(name)
            .size(100, 40)
            .limits(Limits::patient()),
        store,
        clock,
    )
    .expect("starts")
}

fn ledger() -> Headless {
    start_on("ledger.lua", LEDGER, three_notes)
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
        .skip(1)
        .take_while(|l| *l != "--")
        .map(str::to_string)
        .collect()
}

fn id_of(app: &mut Headless, title: &str) -> String {
    app.store().find_by("title", title).expect(title).id.clone()
}

/// How many requests of these kinds reached the store.
fn reached(app: &mut Headless, ops: &[&str]) -> usize {
    let store = app.store();
    ops.iter().map(|op| store.count(op)).sum()
}

fn stored(app: &mut Headless) -> usize {
    app.store().records().len()
}

fn last_row(screen: &str) -> &str {
    screen.lines().last().unwrap_or("")
}

#[test]
fn an_undeclared_type_is_refused_at_the_session() {
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        // A record of a type the app did not declare, in the same store.
        store.seed("card", content(json!({ "title": "secret" })), &bo());
    });
    let card = id_of(&mut app, "secret");
    let note = id_of(&mut app, "one");
    let rev = app.store().get(&note).expect("one").rev;
    // The runtime refuses these at the line that asks. The session is
    // asked here as if the runtime had not.
    let asked = [
        RecordOp::Create {
            record_type: "card".into(),
            content: content(json!({ "title": "x" })),
        },
        RecordOp::List {
            record_type: "card".into(),
            matching: None,
        },
        // The long form is not a declared name either.
        RecordOp::Create {
            record_type: "bzz.app.ledger.note".into(),
            content: content(json!({})),
        },
        RecordOp::Update {
            record_id: card.clone(),
            record_type: "card".into(),
            rev: rev.clone(),
            content: content(json!({})),
        },
        // A declared type named for a record of another type.
        RecordOp::Delete {
            record_id: card.clone(),
            record_type: "note".into(),
            rev: rev.clone(),
        },
        // A real record of the app, named with another declared type.
        RecordOp::Update {
            record_id: note.clone(),
            record_type: "tag".into(),
            rev,
            content: content(json!({})),
        },
        RecordOp::Delete {
            record_id: "not-an-id".into(),
            record_type: "note".into(),
            rev: "0".repeat(64),
        },
    ];
    let wanted = [
        "bad_request",
        "bad_request",
        "bad_request",
        "bad_request",
        "not_found",
        "not_found",
        "not_found",
    ];
    let before = (reached(&mut app, &["list"]), stored(&mut app));
    for (n, (op, code)) in asked.into_iter().zip(wanted).enumerate() {
        let id = 9000 + n as u64;
        let session = app.session();
        session.serve(RecordRequest { id, op });
        session.serve_waiting();
        // Exactly one reply, at once, and it is the refusal.
        let replies: Vec<_> = session
            .outbox
            .iter()
            .filter_map(|event| match event {
                Event::Reply(reply) if reply.id == id => Some(reply.result.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(replies.len(), 1, "request {n}");
        let error = replies[0].clone().expect_err("refused");
        assert_eq!(error.code(), code, "request {n}: {error:?}");
        app.settle();
    }
    // Nothing of it reached the records.
    for op in ["create", "update", "delete"] {
        assert_eq!(app.store().count(op), 0, "{op}");
    }
    assert_eq!((reached(&mut app, &["list"]), stored(&mut app)), before);
    assert!(
        app.session().records().all(|r| r.record_type == "note"),
        "the session holds only the declared types"
    );
    assert_eq!(app.session().records().count(), 3);
}

#[test]
fn a_list_is_answered_from_the_sessions_set_with_match() {
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        for (order, title) in [
            json!("Write the Brief"),
            json!("  brief\tnotes\n "),
            json!("Ship"),
            // No title to show: other clients list it as "ledger note".
            json!(null),
            json!(7),
            json!("   "),
        ]
        .into_iter()
        .enumerate()
        {
            store.seed(
                "note",
                content(
                    json!({ "title": title, "order": order, "text": format!("untitled {order}") }),
                ),
                &bo(),
            );
        }
        // Long titles: `match` looks at the title bzz gives the relay,
        // which is cut at 512 bytes.
        let long = format!("{}needle", "a".repeat(MAX_TITLE_BYTES));
        store.seed("note", content(json!({ "title": long, "order": 9 })), &bo());
        store.seed("tag", content(json!({ "title": "brief tag" })), &bo());
    });
    assert_eq!(told(&mut app), ["list 7"]);
    // Ignoring case; control characters in a title are spaces; another
    // type is not searched.
    app.paste("BRIEF").key("m");
    assert_eq!(
        told(&mut app)[1],
        "match 2:   brief_notes_ , Write the Brief"
    );
    app.key("x").paste("brief notes").key("m");
    assert_eq!(told(&mut app), ["match 1:   brief_notes_"]);
    // A record without a title is found by the name it is listed under.
    app.key("x").paste("ledger NOTE").key("m");
    assert_eq!(told(&mut app), ["match 3:    , 7, untitled 3"]);
    // Past the cut there is nothing to find.
    app.key("x").paste("needle").key("m");
    assert_eq!(told(&mut app), ["match 0:"]);
    app.key("x").paste("nothing like it").key("m");
    assert_eq!(told(&mut app), ["match 0:"]);
    // Each list reached the records once, to be counted, and none was
    // answered by them.
    assert_eq!(app.store().count("list"), 6);
    assert_eq!(
        app.store().list_matches(),
        [
            "",
            "BRIEF",
            "brief notes",
            "ledger NOTE",
            "needle",
            "nothing like it"
        ]
    );
    // What another participant changes is in the next list.
    let ship = id_of(&mut app, "Ship");
    let changed = app.store().remote_update(
        &bo(),
        &ship,
        content(json!({ "title": "Ship the brief", "order": 2 })),
    );
    app.deliver(changed);
    app.key("x").paste("brief").key("m");
    assert_eq!(
        told(&mut app),
        ["match 3:   brief_notes_ , Ship the brief, Write the Brief"]
    );
}

#[test]
fn a_script_hears_of_a_type_from_its_first_list_of_it() {
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.seed("tag", content(json!({ "title": "urgent" })), &bo());
    });
    // The records the app starts with are no changes: one list, no push.
    assert_eq!(told(&mut app), ["list 3"]);
    // It has not listed its tags: a change of one is not told.
    let (_, change) = app
        .store()
        .remote_create(&bo(), "tag", content(json!({ "title": "later" })));
    app.deliver(change);
    let (_, change) = app.store().remote_create(
        &bo(),
        "note",
        content(json!({ "title": "four", "order": 4 })),
    );
    app.deliver(change);
    assert_eq!(told(&mut app), ["list 3", "push create four by bo"]);
    // The list has both tags, the one made meanwhile too; from then on a
    // change of a tag is a push.
    app.key("x").key("t");
    assert_eq!(told(&mut app), ["tags 2: later, urgent"]);
    let (_, change) = app
        .store()
        .remote_create(&bo(), "tag", content(json!({ "title": "third" })));
    app.deliver(change);
    assert_eq!(told(&mut app)[1], "push create third by bo");
}

/// A script that stores people by id in a field, as the templates do, and
/// shows them with `host.name`.
const NAMES: &str = r#"
local app = { api = 1, types = { "vote" } }
function app.init(ctx)
  data.list { type = "vote", tag = "load" }
  return { votes = {}, views = 0 }
end
function app.view(s, ctx)
  s.views = s.views + 1
  local lines = { "me: " .. host.name(ctx.user.id) }
  for _, v in ipairs(s.votes) do lines[#lines + 1] = "voter: " .. host.name(v.content.voter) end
  local ok, err = pcall(host.name, 7)
  lines[#lines + 1] = "seven: " .. tostring(err)
  lines[#lines + 1] = "odd: " .. host.name("éééééééééééééé")
  lines[#lines + 1] = "pushes: " .. (s.pushes or 0)
  return ui.text(lines)
end
function app.reply(s, r, ctx) if r.ok then s.votes = r.records end end
function app.push(s, p, ctx) s.pushes = (s.pushes or 0) + 1 end
return app
"#;

#[test]
fn host_name_is_the_name_bzz_knows_and_a_name_learnt_later_draws_again() {
    let mut app = start_on("names.lua", NAMES, |store| {
        // bo signed the vote that names cy; bzz has no name for cy, and cy
        // signed nothing the app holds.
        store.seed("vote", content(json!({ "voter": bo().id })), &bo());
        store.seed("vote", content(json!({ "voter": cy().id })), &bo());
    });
    let screen = app.screen();
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    assert_eq!(rows[0], "me: ana");
    let mut voters = vec![rows[1], rows[2]];
    voters.sort_unstable();
    let short = short_name(&cy().id);
    assert_eq!(voters, ["voter: bo".to_string(), format!("voter: {short}")]);
    assert!(
        rows[3].starts_with("seven: names.lua:")
            && rows[3].contains("host.name: the id must be a string, got integer"),
        "{screen}"
    );
    // Whatever a script gives, the short form is cut on characters.
    assert_eq!(rows[4], "odd: éééééééé…éééé");
    // bzz is asked to look cy up, and the odd id too: whoever answers
    // looks up only what can be a key.
    let unnamed = app.session().unnamed();
    assert!(unnamed.contains(&cy().id), "{unnamed:?}");
    assert!(!unnamed.contains(&bo().id), "{unnamed:?}");

    // bzz learns cy's name: the app is drawn again with it, and is told
    // of no change, because no record changed.
    app.session().set_names([(cy().id, "cy".to_string())]);
    app.settle();
    let screen = app.screen();
    assert!(screen.contains("voter: cy"), "{screen}");
    assert!(screen.contains("pushes: 0"), "{screen}");
    assert!(!app.session().unnamed().contains(&cy().id));
}

#[test]
fn who_is_an_id_and_the_name_bzz_knows() {
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.seed(
            "note",
            content(json!({ "title": "cy's", "order": 9 })),
            &cy(),
        );
        // bzz has no profile of cy yet.
        store.forget_name(&cy().id);
    });
    let short = short_name(&cy().id);
    assert_eq!(short, "c0c0c0c0…c0c0");
    assert_eq!(app.screen().lines().next(), Some("me: ana/a1"));
    assert_eq!(notes(&mut app)[3], format!("cy's by {short}"));
    assert_eq!(app.session().unnamed(), [cy().id]);
    let rev = |app: &mut Headless| app.store().find_by("title", "cy's").expect("cy's").rev;
    let before = rev(&mut app);

    // The name becomes known: the record is handed over again, as an
    // update with the revision the app has. Nobody else's is.
    app.store().set_name(&cy().id, "cy");
    app.settle();
    assert_eq!(notes(&mut app)[3], "cy's by cy");
    assert_eq!(told(&mut app), ["list 4", "push update cy's by cy"]);
    assert_eq!(rev(&mut app), before);
    assert!(app.session().unnamed().is_empty());
    // The same name again is nothing new.
    app.session().set_names([(cy().id, "cy".to_string())]);
    app.settle();
    assert_eq!(told(&mut app).len(), 2);

    // A name changes: bo's three records are told again, cy's is not.
    app.key("x");
    app.store().set_name(&bo().id, "Bo B.");
    app.settle();
    let mut again = told(&mut app);
    again.sort();
    assert_eq!(
        again,
        [
            "push update one by Bo B.",
            "push update three by Bo B.",
            "push update two by Bo B."
        ]
    );
    assert_eq!(notes(&mut app)[0], "one by Bo B.");
    // The local participant's own name reaches `ctx.user`; the id stays.
    app.store().set_name(&ana().id, "Ana");
    app.settle();
    assert_eq!(app.screen().lines().next(), Some("me: Ana/a1"));
    // A record the app writes is by the local participant, under the name
    // bzz knows now.
    app.key("x").key("c");
    assert_eq!(told(&mut app), ["create ok mine 1"]);
    assert_eq!(
        notes(&mut app).last().map(String::as_str),
        Some("mine 1 by Ana")
    );
}

#[test]
fn an_apps_writes_stop_at_half_the_persons_budget() {
    let mut app = ledger();
    let cap = APP_WRITES_PER_MINUTE as usize;
    assert_eq!(cap, 30);
    // Thirty writes in a burst are sent.
    for _ in 0..cap {
        app.key("c");
    }
    assert_eq!(app.store().count("create"), cap);
    assert_eq!(app.store().records().len(), 3 + cap);
    assert_eq!(app.notice(), None);
    // The next one is answered with the wait, and no request is made.
    app.key("x").key("c");
    assert_eq!(
        told(&mut app),
        ["create rate_limited: too many writes; try again in 60 s [retry_after 60]"]
    );
    assert_eq!(app.store().count("create"), cap, "nothing was sent");
    // The pane says the wait by itself, and counts it down.
    assert_eq!(
        app.notice().as_deref(),
        Some("Too many changes: this app can write again in 60 s.")
    );
    assert_eq!(app.session_status().wait, Some(60));
    app.clock().advance(Duration::from_millis(20_500));
    assert!(app.session().tick(), "the wait shown has changed");
    assert!(!app.session().tick(), "and not again within the second");
    assert_eq!(
        app.notice().as_deref(),
        Some("Too many changes: this app can write again in 40 s.")
    );
    // An update and a delete count as a create does.
    app.key("x").key("u").key("d");
    let seen = told(&mut app);
    assert_eq!(seen.len(), 2);
    assert!(
        seen[0].starts_with("update rate_limited:") && seen[1].starts_with("delete rate_limited:"),
        "{seen:?}"
    );
    assert_eq!(reached(&mut app, &["update", "delete"]), 0);
    // A list is not a write.
    app.key("x").key("r");
    assert_eq!(told(&mut app), [format!("list {}", 3 + cap)]);
    // When the minute of the first writes has passed, it writes again.
    app.advance(39_500);
    assert_eq!(app.notice(), None);
    assert_eq!(app.session_status().wait, None);
    app.key("x").key("c");
    assert_eq!(told(&mut app), [format!("create ok mine {}", cap + 2)]);
    assert_eq!(app.store().count("create"), cap + 1);
}

#[test]
fn the_apps_of_one_person_share_one_budget() {
    // Two apps of the same person, in two channels: two stores here.
    let budget: SharedBudget = Arc::new(Mutex::new(WriteBudget::default()));
    let clock = ManualClock::new(START_OF_TIME);
    let open = |name: &str| {
        let store = SharedStore::new(MemoryStore::new(ana(), clock.clone()));
        let session = AppSession::in_memory(
            LEDGER,
            SessionOptions::new(name)
                .limits(Limits::patient())
                .clock(clock.clone())
                .budget(budget.clone()),
            &store,
        )
        .expect("starts");
        (session, store)
    };
    let (mut first, first_store) = open("ledger");
    let (mut second, second_store) = open("minutes");
    let rest = |session: &mut AppSession| {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let quiet = session.poll().is_empty();
            if quiet && session.is_idle() && session.poll().is_empty() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the app did not rest");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    rest(&mut first);
    rest(&mut second);
    for n in 0..40 {
        let session = if n % 2 == 0 { &mut first } else { &mut second };
        session.key(Key::named("c"));
        rest(session);
    }
    let sent = first_store.lock().count("create") + second_store.lock().count("create");
    assert_eq!(sent, 30, "both apps together");
    assert!(first.status().wait.is_some() && second.status().wait.is_some());
}

#[test]
fn the_relays_quota_is_a_wait_the_pane_shows() {
    let mut app = ledger();
    // The person's chat has used the relay's budget but for two writes.
    app.store().set_quota(2, Duration::from_secs(60));
    app.key("c")
        .advance(15_000)
        .key("c")
        .advance(5_000)
        .key("x");
    app.key("c");
    assert_eq!(
        told(&mut app),
        ["create rate_limited: too many writes; try again in 40 s [retry_after 40]"]
    );
    assert_eq!(
        app.store().count("create"),
        3,
        "the relay refused the third"
    );
    assert_eq!(
        app.notice().as_deref(),
        Some("Too many changes: this app can write again in 40 s.")
    );
    // While the relay's wait lasts nothing more is sent: a refused frame
    // costs the person's quota as an accepted one does.
    app.advance(10_000).key("x").key("c").key("u");
    assert_eq!(
        told(&mut app),
        [
            "create rate_limited: too many writes; try again in 30 s [retry_after 30]",
            "update rate_limited: too many writes; try again in 30 s [retry_after 30]"
        ]
    );
    assert_eq!(reached(&mut app, &["create", "update"]), 3);
    app.advance(30_000);
    assert_eq!(app.notice(), None);
    app.key("x").key("c");
    assert_eq!(told(&mut app), ["create ok mine 5"]);
}

#[test]
fn an_app_that_holds_nothing_waits_for_its_first_set() {
    // Opened for the first time: bzz kept nothing, the relay has three.
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.start_loading(|_| false);
    });
    assert_eq!(told(&mut app), [] as [&str; 0], "the list waits");
    assert_eq!(app.session_status().sync_words, "loading");
    assert_eq!(app.notice(), None);
    // The relay's set arrives: one reply with all of it, and no push.
    let first = app.store().resync();
    app.deliver(first);
    assert_eq!(told(&mut app), ["list 3"]);
    assert_eq!(app.session_status().sync_words, "live");
    assert_eq!(notes(&mut app).len(), 3);

    // The relay does not answer: after five seconds the list is
    // `unavailable`. When the set comes after all, it is no push either:
    // the script was given nothing that could have changed.
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.start_loading(|_| false);
    });
    app.advance(4_900);
    assert!(told(&mut app).is_empty());
    app.advance(200);
    assert_eq!(
        told(&mut app),
        ["list unavailable: the relay has not sent this app's records yet"]
    );
    let late = app.store().resync();
    app.deliver(late);
    assert_eq!(told(&mut app).len(), 1);
    app.key("r");
    assert_eq!(told(&mut app)[1], "list 3");
}

#[test]
fn a_restart_shows_what_bzz_kept_and_then_only_the_differences() {
    // bzz kept two of the three; the third was made while it was closed.
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.start_loading(|record| record.content["title"] != "three");
    });
    // Before the relay answers, the screen has what was kept.
    assert_eq!(told(&mut app), ["list 2"]);
    assert_eq!(notes(&mut app), ["one by bo", "two by bo"]);
    let before = app.screen();
    // A record of the kept set can be changed before the relay answers.
    app.key("u");
    assert_eq!(told(&mut app)[1], "update ok edited");
    let found = app.store().resync();
    app.deliver(found);
    assert_eq!(
        told(&mut app)[2..],
        ["push create three by bo"],
        "only what differs"
    );
    assert_eq!(
        notes(&mut app),
        ["edited by ana", "two by bo", "three by bo"]
    );

    // Nothing differs: the screen after the relay's answer is the screen
    // before it.
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.start_loading(|_| true);
    });
    let before_answer = app.screen();
    assert_ne!(before_answer, before);
    let nothing = app.store().resync();
    assert!(nothing.is_empty());
    app.deliver(nothing);
    assert_eq!(app.screen(), before_answer);
    assert_eq!(app.session_status().sync_words, "live");
}

#[test]
fn each_state_of_the_sync_has_its_sentence_and_its_answers() {
    // Too many records to sync: said by the pane; what is held can be
    // listed, changed and deleted, which is the way out; none is created.
    let mut app = ledger();
    app.store().set_sync(SyncState::TooMany { max: 4096 });
    app.settle();
    assert_eq!(
        app.notice().as_deref(),
        Some("This app has more than 4096 records here, too many to load. None can be added.")
    );
    assert_eq!(app.session_status().sync_words, "too many records");
    app.key("x").key("c").key("r").key("d");
    assert_eq!(
        told(&mut app),
        [
            "create limit: this app already holds 1000 records here",
            "list 3",
            "deleted one"
        ]
    );
    assert_eq!(app.store().count("create"), 0, "no frame for the create");
    // Asking again once some are gone.
    app.session().retry();
    app.settle();
    assert_eq!(app.notice(), None);

    // No access to the channel's records: nothing is listed, nothing sent.
    let mut app = ledger();
    app.store().set_sync(SyncState::Restricted);
    app.settle();
    assert_eq!(
        app.notice().as_deref(),
        Some("You cannot read this channel's records: this app cannot be used here.")
    );
    app.key("x").key("r").key("c").key("u");
    assert_eq!(
        told(&mut app),
        [
            "list refused: you cannot read this channel's records",
            "create refused: you cannot read this channel's records",
            "update refused: you cannot read this channel's records"
        ]
    );
    assert_eq!(reached(&mut app, &["create", "update"]), 0);

    // A relay that cannot sync records: the reason is shown; what bzz
    // holds is still listed.
    let mut app = ledger();
    app.store().set_sync(SyncState::Unavailable {
        reason: "this relay has no artifact sync".into(),
    });
    app.settle();
    assert_eq!(
        app.notice().as_deref(),
        Some("This app's records are not available: this relay has no artifact sync.")
    );
    app.key("x").key("r").key("c");
    assert_eq!(
        told(&mut app),
        [
            "list 3",
            "create unavailable: this relay has no artifact sync"
        ]
    );
    assert_eq!(app.store().count("create"), 0);

    // While bzz fetches the records again the pane says nothing, and the
    // app lists what it held.
    let mut app = ledger();
    app.store().lose_connection();
    app.settle();
    assert_eq!(app.notice(), None);
    assert_eq!(app.session_status().sync_words, "syncing again");
    app.key("x").key("r");
    assert_eq!(told(&mut app), ["list 3"]);
}

#[test]
fn an_archived_channel_says_so_and_refuses_before_a_frame() {
    // bzz knows the channel is archived: the session is told by its owner.
    let mut app = ledger();
    app.session().set_archived(true);
    assert_eq!(
        app.notice().as_deref(),
        Some("This channel is archived: its records can be read, not changed.")
    );
    app.key("x").key("c").key("u").key("d").key("r");
    assert_eq!(
        told(&mut app),
        [
            "create refused: this channel is archived; its records can be read, not changed",
            "update refused: this channel is archived; its records can be read, not changed",
            "delete refused: this channel is archived; its records can be read, not changed",
            "list 3"
        ]
    );
    for op in ["create", "update", "delete"] {
        assert_eq!(app.store().count(op), 0, "{op}: no frame");
    }
    app.session().set_archived(false);
    assert_eq!(app.notice(), None);

    // bzz did not know: the relay refuses the write, and from then on the
    // pane says so, until the relay takes a write or the sync starts anew
    // (unarchiving ends every sync of the channel).
    let mut app = ledger();
    app.store().set_archived(true);
    assert_eq!(app.notice(), None);
    app.key("x").key("c");
    assert_eq!(
        told(&mut app),
        ["create refused: this channel is archived; its records can be read, not changed"]
    );
    assert_eq!(app.store().count("create"), 1);
    assert_eq!(
        app.notice().as_deref(),
        Some("This channel is archived: its records can be read, not changed.")
    );
    app.store().set_archived(false);
    app.store().lose_connection();
    let nothing = app.store().resync();
    app.deliver(nothing);
    assert_eq!(app.notice(), None);
    app.key("x").key("c");
    assert_eq!(told(&mut app), ["create ok mine 2"]);
}

#[test]
fn a_change_told_before_the_answer_to_an_own_write_is_not_lost() {
    // The app revises a record; the relay stores it; someone revises the
    // record again; that change reaches bzz before the answer to the
    // app's own write does. The script must end on the later revision.
    let mut app = ledger();
    let one = id_of(&mut app, "one");
    app.store().hold_next("update");
    app.key("x").key("u");
    let later = app.store().remote_update(
        &bo(),
        &one,
        content(json!({ "title": "bo's, later", "order": 1 })),
    );
    app.deliver(later);
    assert_eq!(told(&mut app), ["push update bo's, later by bo"]);
    let answer = app.store().release();
    assert_eq!(answer.len(), 1);
    app.deliver(answer);
    assert_eq!(
        told(&mut app),
        [
            "push update bo's, later by bo",
            // The reply carries the app's own revision, as stored...
            "update ok edited",
            // ...and the record as it is now follows it.
            "push update bo's, later by bo"
        ]
    );
    assert_eq!(notes(&mut app)[0], "bo's, later by bo");

    // The same when the later change is a deletion.
    let mut app = ledger();
    let one = id_of(&mut app, "one");
    app.store().hold_next("update");
    app.key("x").key("u");
    let gone = app.store().remote_delete(&bo(), &one);
    app.deliver(gone);
    let answer = app.store().release();
    app.deliver(answer);
    assert_eq!(
        told(&mut app),
        [
            "push delete one by bo",
            "update ok edited",
            "push delete edited by ana"
        ]
    );
    assert_eq!(notes(&mut app), ["two by bo", "three by bo"]);

    // In the usual order nothing is told twice: the answer, then a change.
    let mut app = ledger();
    let one = id_of(&mut app, "one");
    app.key("x").key("u");
    let later = app.store().remote_update(
        &bo(),
        &one,
        content(json!({ "title": "bo's, later", "order": 1 })),
    );
    app.deliver(later);
    assert_eq!(
        told(&mut app),
        ["update ok edited", "push update bo's, later by bo"]
    );
}

#[test]
fn a_new_version_with_other_types_gets_other_records() {
    let mut app = start_on("ledger.lua", LEDGER, |store| {
        three_notes(store);
        store.seed("tag", content(json!({ "title": "urgent" })), &bo());
        store.seed("card", content(json!({ "title": "a card" })), &bo());
    });
    assert_eq!(app.session().types(), ["note", "tag"]);
    assert_eq!(app.session().records().count(), 4);
    // A version that declares cards instead of tags, and lists them when
    // it replaces the old one.
    let next = LEDGER
        .replace(r#"types = { "note", "tag" }"#, r#"types = { "note", "card" }"#)
        .replace(
            "function app.paste(s, text, ctx)",
            "function app.reload(s, ctx) data.list { type = \"card\", tag = \"tags\" } end\nfunction app.paste(s, text, ctx)",
        );
    app.key("x").reload(next);
    assert_eq!(app.error(), None);
    assert_eq!(app.session().types(), ["note", "card"]);
    assert_eq!(told(&mut app), ["tags 1: a card"]);
    // The notes are as they were: no push said otherwise. The tag is no
    // longer the app's.
    assert_eq!(notes(&mut app).len(), 3);
    assert!(
        app.session()
            .records()
            .all(|r| r.record_type == "note" || r.record_type == "card")
    );
    // A note still changes as before, a card now does, a tag no more.
    let (_, change) =
        app.store()
            .remote_create(&bo(), "card", content(json!({ "title": "second card" })));
    app.deliver(change);
    let (_, change) =
        app.store()
            .remote_create(&bo(), "tag", content(json!({ "title": "another tag" })));
    app.deliver(change);
    app.key("u");
    assert_eq!(
        told(&mut app)[1..],
        ["push create second card by bo", "update ok edited"]
    );
}

#[test]
fn a_set_fetched_again_never_fills_the_scripts_queue() {
    // An app whose owner may be owed 64 events is told of 500 changes at
    // once: the session sends them as the script takes them.
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(ana(), clock.clone());
    three_notes(&mut store);
    let limits = Limits {
        event_backlog: 64,
        ..Limits::patient()
    };
    let mut app = Headless::start_on(
        COUNTER,
        HeadlessOptions::new("counter.lua")
            .size(60, 20)
            .limits(limits),
        store,
        clock,
    )
    .expect("starts");
    assert_eq!(app.error(), None);
    app.store().lose_connection();
    for n in 0..500 {
        let nothing = app.store().remote_create(
            &bo(),
            "note",
            content(json!({ "title": format!("n{n}"), "order": 10 + n })),
        );
        assert!(nothing.1.is_empty());
    }
    let found = app.store().resync();
    app.deliver(found);
    assert!(app.status().responding, "the queue was never filled");
    assert_eq!(app.screen().trim_end(), "listed 3\npushes 500");
    assert_eq!(app.session().records().count(), 503);
}

#[test]
fn the_board_shows_the_wait_and_the_pane_shows_it_too() {
    // The Board fixture at its docked size, on the app's own budget.
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(ana(), clock.clone());
    store.seed(
        "card",
        content(json!({ "title": "Alpha", "column": "todo", "order": 1 })),
        &bo(),
    );
    let mut app = Headless::start_on(
        BOARD,
        HeadlessOptions::new("board.lua").limits(Limits::patient()),
        store,
        clock,
    )
    .expect("starts");
    assert_eq!(last_row(&app.screen()), "Loaded 1 cards.");
    // Thirty moves, right and left: `]` and `[`.
    for n in 0..30 {
        app.key(if n % 2 == 0 { "]" } else { "[" });
    }
    assert_eq!(last_row(&app.screen()), "Saved.");
    assert_eq!(app.store().count("update"), 30);
    app.key("]");
    assert_eq!(last_row(&app.screen()), "Too many changes. Retry in 60 s.");
    assert_eq!(
        app.notice().as_deref(),
        Some("Too many changes: this app can write again in 60 s.")
    );
    assert_eq!(app.store().count("update"), 30, "no frame was sent");
    // The card is back where it was.
    assert!(app.screen().contains("To do (1)"), "{}", app.screen());
}

#[test]
fn a_session_can_be_owned_by_a_task() {
    // bzz's loop is a task: what it owns across a wait must be `Send`.
    fn sendable<T: Send>() {}
    sendable::<AppSession>();
    sendable::<SessionStatus>();
}
