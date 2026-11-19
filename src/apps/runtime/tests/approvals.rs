//! The Approvals fixture, driven without a window on the in-memory store,
//! through the fifteen cases of its spec
//! (`tests/apps-bench/spec/approvals.md`, `lib/cases_approvals.py`). What a
//! case checks about the process and its terminal has no meaning here; each
//! case says what it leaves out.

use serde_json::{Value, json};

use super::*;
use crate::apps::runtime::Phase;

const TITLES: [&str; 9] = [
    "New laptop",
    "Team lunch",
    "Conference trip",
    "Standing desk",
    "Extra monitor",
    "Gym pass",
    "Desk lamp",
    "Café 日本",
    "one two q",
];

fn request(
    title: &str,
    author: &str,
    state: &str,
    by: Option<&str>,
    reason: Option<&str>,
) -> Value {
    json!({ "title": title, "author": author, "state": state, "decided_by": by, "reason": reason })
}

fn five_requests(store: &mut MemoryStore) {
    let seed = [
        request("New laptop", "bo", "pending", None, None),
        request("Team lunch", "ana", "pending", None, None),
        request("Conference trip", "cy", "approved", Some("bo"), None),
        request(
            "Standing desk",
            "di",
            "rejected",
            Some("bo"),
            Some("No budget"),
        ),
        request("Extra monitor", "cy", "pending", None, None),
    ];
    for (order, mut value) in seed.into_iter().enumerate() {
        value["order"] = json!(order + 1);
        store.seed("request", content(value), &bo());
    }
}

/// What a person sees of the list: each request's state and title, which
/// is marked, and the filter.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    rows: Vec<(String, String)>,
    selected: Option<String>,
    filter: Option<String>,
}

fn read(screen: &str) -> Seen {
    let mut seen = Seen {
        rows: vec![],
        selected: None,
        filter: None,
    };
    for row in above_status(screen) {
        if let Some(filter) = row.strip_prefix("Filter: ") {
            let filter = filter.split_whitespace().next().unwrap_or("");
            seen.filter = Some(filter.to_string());
            continue;
        }
        let Some((selected, rest)) = marked(row) else {
            continue;
        };
        let Some((state, rest)) = rest.split_once(' ') else {
            continue;
        };
        if !["pending", "approved", "rejected"].contains(&state) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(title) = TITLES.iter().find(|t| rest.starts_with(**t)) else {
            continue;
        };
        if selected {
            assert_eq!(seen.selected, None, "two requests are marked:\n{screen}");
            seen.selected = Some(title.to_string());
        }
        seen.rows.push((state.to_string(), title.to_string()));
    }
    seen
}

fn rows(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(state, title)| (state.to_string(), title.to_string()))
        .collect()
}

const INIT: [(&str, &str); 5] = [
    ("pending", "New laptop"),
    ("pending", "Team lunch"),
    ("approved", "Conference trip"),
    ("rejected", "Standing desk"),
    ("pending", "Extra monitor"),
];

/// The five requests, with one of them in another state.
fn with_state(title: &str, state: &str) -> Vec<(String, String)> {
    INIT.iter()
        .map(|(was, t)| {
            (
                if *t == title { state } else { was }.to_string(),
                t.to_string(),
            )
        })
        .collect()
}

#[track_caller]
fn want(app: &mut Headless, list: &[(String, String)], selected: &str) {
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, list, "{screen}");
    assert_eq!(seen.selected.as_deref(), Some(selected), "{screen}");
}

fn stored(app: &mut Headless, title: &str) -> Value {
    Value::Object(
        app.store()
            .find_by("title", title)
            .expect("the request")
            .content
            .clone(),
    )
}

fn writes(app: &mut Headless) -> usize {
    let store = app.store();
    store.count("create") + store.count("update")
}

fn start_approvals(size: (u16, u16)) -> Headless {
    let mut app = start_on("approvals.lua", APPROVALS, size, five_requests);
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, rows(&INIT), "{size:?}\n{screen}");
    assert_eq!(seen.selected.as_deref(), Some("New laptop"), "{screen}");
    assert_eq!(seen.filter.as_deref(), Some("all"), "{screen}");
    assert_eq!(last_row(&screen), "Loaded 5 requests.", "{screen}");
    assert_eq!(app.error(), None);
    app
}

#[test]
fn case_01_first_draw() {
    // Left out: "the program is still running 0.5 s later".
    for size in [(40, 16), (80, 24), (120, 40)] {
        let mut app = start_approvals(size);
        fits(&app.screen(), size);
        assert_eq!(app.phase(), Phase::Running);
    }
}

#[test]
fn case_02_form_cancel() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.key("j");
        want(&mut app, &rows(&INIT), "Team lunch");
        app.key("n");
        let screen = app.screen();
        assert!(
            screen.contains("New request") && screen.contains("Title:"),
            "{screen}"
        );
        app.type_text("Café 日本");
        assert!(
            app.screen().contains("Title: Café 日本"),
            "{}",
            app.screen()
        );
        app.key("esc");
        assert!(!app.screen().contains("New request"));
        want(&mut app, &rows(&INIT), "Team lunch");
        assert_eq!(writes(&mut app), 0);
        app.key("j");
        want(&mut app, &rows(&INIT), "Conference trip");
    });
}

#[test]
fn case_03_save_once() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.key("n").key("enter");
        assert!(
            app.screen().contains("Title is required."),
            "{}",
            app.screen()
        );
        assert_eq!(writes(&mut app), 0);
        app.store().hold_next("create");
        app.type_text("Gym pass").key("enter").key("enter");
        assert_eq!(last_row(&app.screen()), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one request");
        app.deliver(replies);
        let mut after = rows(&INIT);
        after.push(("pending".into(), "Gym pass".into()));
        want(&mut app, &after, "Gym pass");
        assert_eq!(last_row(&app.screen()), "Request filed.");
        assert!(!app.screen().contains("New request"));
        assert_eq!(app.store().count("create"), 1);
        // The nulls are in the record, and it has a place after the others.
        let mut content = stored(&mut app, "Gym pass");
        let order = content.as_object_mut().and_then(|c| c.remove("order"));
        assert_eq!(content, request("Gym pass", "ana", "pending", None, None));
        assert!(order.and_then(|o| o.as_f64()).is_some_and(|o| o > 5.0));
    });
}

#[test]
fn case_04_filter() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.key("j").key("j").key("j").key("j");
        want(&mut app, &rows(&INIT), "Extra monitor");
        app.key("f");
        let pending: Vec<_> = rows(&INIT)
            .into_iter()
            .filter(|(s, _)| s == "pending")
            .collect();
        want(&mut app, &pending, "Extra monitor");
        assert_eq!(read(&app.screen()).filter.as_deref(), Some("pending"));
        app.key("f");
        want(
            &mut app,
            &rows(&[("approved", "Conference trip")]),
            "Conference trip",
        );
        app.key("f");
        want(
            &mut app,
            &rows(&[("rejected", "Standing desk")]),
            "Standing desk",
        );
        // Back to all, the selected request stays selected.
        app.key("f");
        want(&mut app, &rows(&INIT), "Standing desk");
        assert_eq!(read(&app.screen()).filter.as_deref(), Some("all"));
        app.key("k");
        want(&mut app, &rows(&INIT), "Conference trip");
        assert_eq!(app.store().count("list"), 1, "a filter asks for nothing");
    });
}

#[test]
fn case_05_late_reload() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.store().hold_next("list");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loading...");
        app.key("a");
        let approved = with_state("New laptop", "approved");
        want(&mut app, &approved, "New laptop");
        assert_eq!(last_row(&app.screen()), "Approved.");
        // The reload's reply predates the approval: it must not undo it.
        let late = app.store().release();
        assert_eq!(late.len(), 1);
        app.deliver(late);
        want(&mut app, &approved, "New laptop");
    });
}

#[test]
fn case_06_another_size() {
    at_each_size(|size| {
        let other = other_size(size);
        let mut app = start_approvals(size);
        app.resize(other.0, other.1);
        let screen = app.screen();
        fits(&screen, other);
        assert_eq!(read(&screen).rows, rows(&INIT), "{screen}");
        assert_eq!(last_row(&screen), "Loaded 5 requests.");
        app.key("enter");
        let screen = app.screen();
        for line in ["Request: New laptop", "Author: bo", "State: pending"] {
            assert!(screen.contains(line), "{line}\n{screen}");
        }
        // In the pane the detail still leaves five rows of the list.
        assert_eq!(read(&screen).rows.len(), 5, "{screen}");
        app.key("esc").key("x");
        let screen = app.screen();
        assert!(
            screen.contains("Reject request") && screen.contains("Reason:"),
            "{screen}"
        );
        app.type_text("Too old").key("enter");
        let rejected = with_state("New laptop", "rejected");
        want(&mut app, &rejected, "New laptop");
        assert_eq!(last_row(&app.screen()), "Rejected.");
        app.key("enter");
        let screen = app.screen();
        for line in ["Decided by: ana", "Reason: Too old"] {
            assert!(screen.contains(line), "{line}\n{screen}");
        }
        fits(&screen, other);
        app.key("esc");
        app.resize(size.0, size.1);
        let screen = app.screen();
        fits(&screen, size);
        want(&mut app, &rejected, "New laptop");
    });
}

#[test]
fn case_07_paste() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.paste("a\nx\nn\nq\n");
        assert!(!app.closed(), "the pasted q closed the app");
        let screen = app.screen();
        assert!(!screen.contains("New request") && !screen.contains("Reject request"));
        want(&mut app, &rows(&INIT), "New laptop");
        assert_eq!(writes(&mut app), 0);
        app.key("n").paste("one\ntwo\r\nq");
        let screen = app.screen();
        assert!(screen.contains("Title: one two q"), "{screen}");
        assert!(screen.contains("New request"));
        assert_eq!(writes(&mut app), 0);
        app.key("esc");
        assert!(!app.screen().contains("New request"));
    });
}

#[test]
fn case_08_load_error() {
    at_each_size(|size| {
        let mut app = start_on("approvals.lua", APPROVALS, size, |store| {
            five_requests(store);
            store.fail_next("list", unavailable());
        });
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        app.key("r");
        want(&mut app, &rows(&INIT), "New laptop");
        assert_eq!(last_row(&app.screen()), "Loaded 5 requests.");
        app.key("j");
        app.store().fail_next("list", unavailable());
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        want(&mut app, &rows(&INIT), "Team lunch");
        app.key("r");
        want(&mut app, &rows(&INIT), "Team lunch");
        assert_eq!(last_row(&app.screen()), "Loaded 5 requests.");
    });
}

#[test]
fn case_09_nothing_is_shown_while_the_first_load_is_pending() {
    // Bench: Ctrl+C during the slow first load ends the process. Ctrl+C is
    // the owner's key and never reaches a script
    // (`running::ctrl_c_never_reaches_a_script...`). Here: what the app
    // shows meanwhile, and that `q` closes it with the load unanswered.
    at_each_size(|size| {
        let mut app = start_on("approvals.lua", APPROVALS, size, |store| {
            five_requests(store);
            store.hold_next("list");
        });
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Loading...");
        assert_eq!(read(&screen).rows, vec![], "{screen}");
        app.key("q");
        assert!(app.closed());
    });
}

#[test]
fn case_10_repeatable() {
    at_each_size(|size| {
        let mut screens = Vec::new();
        for _ in 0..3 {
            let mut app = start_approvals(size);
            app.key("a");
            assert_eq!(last_row(&app.screen()), "Approved.");
            app.key("down").key("down").key("down").key("down").key("x");
            assert!(app.screen().contains("Reject request"));
            app.type_text("Later").key("enter");
            assert_eq!(last_row(&app.screen()), "Rejected.");
            app.key("f");
            want(&mut app, &rows(&[("pending", "Team lunch")]), "Team lunch");
            screens.push(app.screen());
        }
        assert_eq!(screens[0], screens[1]);
        assert_eq!(screens[0], screens[2]);
    });
}

#[test]
fn case_11_remote_change() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        let laptop = app
            .store()
            .find_by("title", "New laptop")
            .expect("it")
            .clone();
        let desk = app
            .store()
            .find_by("title", "Standing desk")
            .expect("it")
            .id
            .clone();
        let mut lamp = request("Desk lamp", "bo", "pending", None, None);
        lamp["order"] = json!(1001);
        let (_, created) = app.store().remote_create(&bo(), "request", content(lamp));
        app.deliver(created);
        let mut decided = laptop.content.clone();
        decided.insert("state".into(), json!("approved"));
        decided.insert("decided_by".into(), json!("cy"));
        let updated = app.store().remote_update(&cy(), &laptop.id, decided);
        app.deliver(updated);
        let deleted = app.store().remote_delete(&bo(), &desk);
        app.deliver(deleted);
        // With no key pressed, and no reload: new requests last.
        let after = rows(&[
            ("approved", "New laptop"),
            ("pending", "Team lunch"),
            ("approved", "Conference trip"),
            ("pending", "Extra monitor"),
            ("pending", "Desk lamp"),
        ]);
        want(&mut app, &after, "New laptop");
        assert_eq!(app.store().count("list"), 1);
    });
}

#[test]
fn case_12_conflict() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        let laptop = app
            .store()
            .find_by("title", "New laptop")
            .expect("it")
            .clone();
        // While ana approves it, bo rejects it.
        let mut theirs = laptop.content.clone();
        theirs.insert("state".into(), json!("rejected"));
        theirs.insert("decided_by".into(), json!("bo"));
        theirs.insert("reason".into(), json!("Duplicate"));
        app.store().arm_conflict(&laptop.id, &bo(), theirs.clone());
        app.key("a");
        want(
            &mut app,
            &with_state("New laptop", "rejected"),
            "New laptop",
        );
        assert_eq!(last_row(&app.screen()), "Already rejected by bo.");
        // A decision is never overwritten: no second update.
        assert_eq!(stored(&mut app, "New laptop"), Value::Object(theirs));
        assert_eq!(app.store().count("update"), 1);
    });
}

#[test]
fn case_13_own_request() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.key("j").key("a");
        assert_eq!(
            last_row(&app.screen()),
            "You cannot decide your own request."
        );
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loaded 5 requests.");
        app.key("x");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "You cannot decide your own request.");
        assert!(!screen.contains("Reject request"), "{screen}");
        want(&mut app, &rows(&INIT), "Team lunch");
        assert_eq!(writes(&mut app), 0);
    });
}

#[test]
fn case_14_reject_needs_a_reason() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        let before = stored(&mut app, "New laptop");
        app.key("x");
        let screen = app.screen();
        assert!(
            screen.contains("Reject request") && screen.contains("Reason:"),
            "{screen}"
        );
        app.key("enter");
        assert!(app.screen().contains("Reason is required."));
        app.type_text("  ").key("enter");
        assert!(app.screen().contains("Reason is required."));
        assert_eq!(writes(&mut app), 0);
        app.type_text("Over budget").key("enter");
        want(
            &mut app,
            &with_state("New laptop", "rejected"),
            "New laptop",
        );
        assert_eq!(last_row(&app.screen()), "Rejected.");
        app.key("enter");
        let screen = app.screen();
        for line in [
            "Request: New laptop",
            "State: rejected",
            "Decided by: ana",
            "Reason: Over budget",
        ] {
            assert!(screen.contains(line), "{line}\n{screen}");
        }
        // The decision on top of the record: title, author and order kept.
        let mut after = before;
        after["state"] = json!("rejected");
        after["decided_by"] = json!("ana");
        after["reason"] = json!("Over budget");
        assert_eq!(stored(&mut app, "New laptop"), after);
    });
}

#[test]
fn case_15_already_decided() {
    at_each_size(|size| {
        let mut app = start_approvals(size);
        app.key("j").key("j").key("a");
        assert_eq!(last_row(&app.screen()), "Already approved by bo.");
        app.key("j").key("x");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Already rejected by bo.");
        assert!(!screen.contains("Reject request"), "{screen}");
        app.key("enter");
        let screen = app.screen();
        for line in [
            "Request: Standing desk",
            "Decided by: bo",
            "Reason: No budget",
        ] {
            assert!(screen.contains(line), "{line}\n{screen}");
        }
        app.key("esc");
        want(&mut app, &rows(&INIT), "Standing desk");
        assert_eq!(writes(&mut app), 0);
    });
}

#[test]
fn an_approval_keeps_the_null_reason_in_the_record() {
    // The experiment's host could not write a null (its one failed case):
    // a null arrives as nil, and must go back as `data.null`.
    let mut app = start_approvals((40, 16));
    app.key("a");
    assert_eq!(last_row(&app.screen()), "Approved.");
    let mut wanted = request("New laptop", "bo", "approved", Some("ana"), None);
    wanted["order"] = json!(1);
    assert_eq!(stored(&mut app, "New laptop"), wanted);
}
