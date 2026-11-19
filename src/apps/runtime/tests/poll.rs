//! The Poll fixture, driven without a window on the in-memory store,
//! through the twelve cases of its spec (`tests/apps-bench/spec/poll.md`,
//! `lib/cases_poll.py`). Nobody had written the Poll in Lua: this fixture
//! was written from that spec. It is the one app with two record types, so
//! a load is two lists.

use serde_json::{Value, json};

use super::*;
use crate::apps::runtime::Phase;

const QUESTION: &str = "Lunch on Friday?";

fn lunch(store: &mut MemoryStore) {
    store.seed(
        "poll",
        content(json!({ "question": QUESTION, "options": ["Tacos", "Sushi", "Pizza", "Salad"] })),
        &bo(),
    );
    for (voter, option) in [("bo", "Sushi"), ("cy", "Sushi"), ("di", "Pizza")] {
        store.seed(
            "vote",
            content(json!({ "voter": voter, "option": option })),
            &bo(),
        );
    }
}

/// What a person sees of the poll: each option with its tally, which is
/// marked, which holds the vote of the person looking, and the total.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    rows: Vec<(String, u64)>,
    selected: Option<String>,
    mine: Option<String>,
    total: Option<u64>,
}

/// `3 votes`, `1 vote`: the number, if the word agrees with it.
fn votes(text: &str) -> Option<u64> {
    let (number, word) = text.split_once(' ')?;
    let n: u64 = number.parse().ok()?;
    (word == if n == 1 { "vote" } else { "votes" }).then_some(n)
}

fn read(screen: &str) -> Seen {
    let mut seen = Seen {
        rows: vec![],
        selected: None,
        mine: None,
        total: None,
    };
    for row in above_status(screen) {
        if let Some(total) = row.strip_prefix("Total: ") {
            seen.total = votes(total);
            continue;
        }
        let Some((selected, rest)) = marked(row) else {
            continue;
        };
        let (mine, rest) = match (rest.strip_prefix("[x] "), rest.strip_prefix("[ ] ")) {
            (Some(rest), _) => (true, rest),
            (_, Some(rest)) => (false, rest),
            _ => continue,
        };
        // The label, then at least two spaces, then the tally.
        let Some((label, tally)) = rest.rsplit_once("  ") else {
            continue;
        };
        let Some(n) = votes(tally.trim()) else {
            continue;
        };
        let label = label.trim_end().to_string();
        if selected {
            assert_eq!(seen.selected, None, "two options are marked:\n{screen}");
            seen.selected = Some(label.clone());
        }
        if mine {
            assert_eq!(seen.mine, None, "two options hold the vote:\n{screen}");
            seen.mine = Some(label.clone());
        }
        seen.rows.push((label, n));
    }
    seen
}

fn rows(list: &[(&str, u64)]) -> Vec<(String, u64)> {
    list.iter()
        .map(|(label, n)| (label.to_string(), *n))
        .collect()
}

const INIT: [(&str, u64); 4] = [("Tacos", 0), ("Sushi", 2), ("Pizza", 1), ("Salad", 0)];

#[track_caller]
fn want(app: &mut Headless, list: &[(&str, u64)], selected: &str, mine: Option<&str>, total: u64) {
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, rows(list), "{screen}");
    assert_eq!(seen.selected.as_deref(), Some(selected), "{screen}");
    assert_eq!(seen.mine.as_deref(), mine, "{screen}");
    assert_eq!(seen.total, Some(total), "{screen}");
}

fn options(app: &mut Headless) -> Value {
    let poll = app
        .store()
        .records()
        .into_iter()
        .find(|r| r.record_type == "poll")
        .expect("the poll");
    poll.content.get("options").cloned().unwrap_or(Value::Null)
}

fn poll_id(app: &mut Headless) -> String {
    let poll = app
        .store()
        .records()
        .into_iter()
        .find(|r| r.record_type == "poll")
        .expect("the poll");
    poll.id
}

fn writes(app: &mut Headless) -> usize {
    let store = app.store();
    store.count("create") + store.count("update")
}

fn start_poll(size: (u16, u16)) -> Headless {
    let mut app = start_on("poll.lua", POLL, size, lunch);
    let screen = app.screen();
    want(&mut app, &INIT, "Tacos", None, 3);
    assert!(screen.contains(QUESTION), "{screen}");
    assert_eq!(last_row(&screen), "Loaded 4 options.", "{screen}");
    assert_eq!(app.error(), None);
    // A list names one type: a load is two of them.
    assert_eq!(app.store().count("list"), 2);
    app
}

#[test]
fn case_01_first_draw() {
    // Left out: "the program is still running 0.5 s later".
    for size in [(40, 12), (40, 16), (80, 24), (120, 40)] {
        let mut app = start_poll(size);
        fits(&app.screen(), size);
        assert_eq!(app.phase(), Phase::Running);
    }
}

#[test]
fn case_02_form_cancel() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        app.key("j");
        want(&mut app, &INIT, "Sushi", None, 3);
        app.key("a");
        let screen = app.screen();
        assert!(
            screen.contains("New option") && screen.contains("Option:"),
            "{screen}"
        );
        app.type_text("Café 日本");
        assert!(
            app.screen().contains("Option: Café 日本"),
            "{}",
            app.screen()
        );
        app.key("esc");
        assert!(!app.screen().contains("New option"));
        want(&mut app, &INIT, "Sushi", None, 3);
        assert_eq!(writes(&mut app), 0);
        assert_eq!(
            options(&mut app),
            json!(["Tacos", "Sushi", "Pizza", "Salad"])
        );
        app.key("j");
        want(&mut app, &INIT, "Pizza", None, 3);
    });
}

#[test]
fn case_03_save_once() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        app.key("a").key("enter");
        assert!(
            app.screen().contains("Option is required."),
            "{}",
            app.screen()
        );
        app.type_text("sushi").key("enter");
        assert!(
            app.screen().contains("Option already exists."),
            "{}",
            app.screen()
        );
        assert_eq!(writes(&mut app), 0);
        for _ in 0..5 {
            app.key("backspace");
        }
        // The write is slow: its reply is held back while Enter is pressed
        // a second time.
        app.store().hold_next("update");
        app.type_text("Ramen").key("enter").key("enter");
        let screen = app.screen();
        assert!(
            screen.contains("New option"),
            "the form stays open:\n{screen}"
        );
        assert_eq!(last_row(&screen), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one option");
        app.deliver(replies);
        let with_ramen = [
            ("Tacos", 0),
            ("Sushi", 2),
            ("Pizza", 1),
            ("Salad", 0),
            ("Ramen", 0),
        ];
        want(&mut app, &with_ramen, "Tacos", None, 3);
        assert_eq!(last_row(&app.screen()), "Option added.");
        assert_eq!(
            options(&mut app),
            json!(["Tacos", "Sushi", "Pizza", "Salad", "Ramen"])
        );
        // A vote, with Enter pressed twice.
        app.store().hold_next("create");
        app.key("enter").key("enter");
        assert_eq!(last_row(&app.screen()), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one vote");
        app.deliver(replies);
        let voted = [
            ("Tacos", 1),
            ("Sushi", 2),
            ("Pizza", 1),
            ("Salad", 0),
            ("Ramen", 0),
        ];
        want(&mut app, &voted, "Tacos", Some("Tacos"), 4);
        assert_eq!(last_row(&app.screen()), "Voted for Tacos.");
        let mine = app
            .store()
            .records()
            .iter()
            .filter(|r| r.content.get("voter") == Some(&json!("ana")))
            .count();
        assert_eq!(mine, 1);
    });
}

#[test]
fn case_04_search_clear() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        app.key("j");
        want(&mut app, &INIT, "Sushi", None, 3);
        app.key("/");
        assert!(app.screen().contains("Search:"));
        app.type_text("sa");
        // The total counts every option, filtered or not.
        want(&mut app, &[("Salad", 0)], "Salad", None, 3);
        app.key("enter");
        assert!(app.screen().contains("Search: sa"));
        app.key("esc");
        assert!(!app.screen().contains("Search: sa"));
        // Clearing the filter keeps the selected option selected.
        want(&mut app, &INIT, "Salad", None, 3);
        app.key("k");
        want(&mut app, &INIT, "Pizza", None, 3);
        assert_eq!(app.store().count("list"), 2, "a search asks for nothing");
        app.key("/").type_text("xq");
        let screen = app.screen();
        assert!(screen.contains("No options match."), "{screen}");
    });
}

#[test]
fn case_05_late_reload() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        // Both lists of the reload are slow.
        app.store().hold_next("list");
        app.store().hold_next("list");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loading...");
        assert_eq!(app.store().count("list"), 4);
        app.key("enter");
        let voted = [("Tacos", 1), ("Sushi", 2), ("Pizza", 1), ("Salad", 0)];
        want(&mut app, &voted, "Tacos", Some("Tacos"), 4);
        // The reload's replies predate the vote: they must not undo it.
        let late = app.store().release();
        assert_eq!(late.len(), 2);
        app.deliver(late);
        want(&mut app, &voted, "Tacos", Some("Tacos"), 4);
        assert_eq!(last_row(&app.screen()), "Loaded 4 options.");
    });
}

#[test]
fn case_06_small_screen() {
    // The poll stays usable down to 40 by 12.
    at_each_size(|size| {
        let mut app = start_poll(size);
        app.resize(40, 12);
        let screen = app.screen();
        fits(&screen, (40, 12));
        assert!(screen.contains(QUESTION), "{screen}");
        want(&mut app, &INIT, "Tacos", None, 3);
        assert_eq!(last_row(&screen), "Loaded 4 options.");
        app.key("j").key("space");
        let voted = [("Tacos", 0), ("Sushi", 3), ("Pizza", 1), ("Salad", 0)];
        want(&mut app, &voted, "Sushi", Some("Sushi"), 4);
        assert_eq!(last_row(&app.screen()), "Voted for Sushi.");
        fits(&app.screen(), (40, 12));
    });
}

#[test]
fn case_07_paste() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        app.paste("j\n \nq\n");
        assert!(!app.closed(), "the pasted q closed the app");
        want(&mut app, &INIT, "Tacos", None, 3);
        assert_eq!(writes(&mut app), 0);
        app.key("a").paste("one\ntwo\r\nq");
        let screen = app.screen();
        assert!(screen.contains("Option: one two q"), "{screen}");
        assert!(screen.contains("New option"));
        assert_eq!(writes(&mut app), 0);
        app.key("esc");
        assert!(!app.screen().contains("New option"));
    });
}

#[test]
fn case_08_load_error() {
    at_each_size(|size| {
        // One of the two lists fails: the load failed.
        let mut app = start_on("poll.lua", POLL, size, |store| {
            lunch(store);
            store.fail_next("list", unavailable());
        });
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Load failed. Press r to retry.");
        assert_eq!(read(&screen).rows, vec![], "{screen}");
        app.key("r");
        want(&mut app, &INIT, "Tacos", None, 3);
        assert_eq!(last_row(&app.screen()), "Loaded 4 options.");
        app.key("j");
        app.store().fail_next("list", unavailable());
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        // Everything shown before the failed load is kept.
        want(&mut app, &INIT, "Sushi", None, 3);
        app.key("r");
        want(&mut app, &INIT, "Sushi", None, 3);
        assert_eq!(last_row(&app.screen()), "Loaded 4 options.");
    });
}

#[test]
fn case_09_nothing_is_shown_while_the_first_load_is_pending() {
    // Bench: Ctrl+C during the slow first load ends the process; Ctrl+C is
    // the owner's key. Here: what the app shows while one of its two lists
    // has not answered, and that `q` closes it then.
    at_each_size(|size| {
        let mut app = start_on("poll.lua", POLL, size, |store| {
            lunch(store);
            store.hold_next("list");
        });
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Loading...");
        assert!(
            !screen.contains("Tacos") && !screen.contains("Sushi"),
            "{screen}"
        );
        app.key("q");
        assert!(app.closed());
    });
}

#[test]
fn case_10_repeatable() {
    at_each_size(|size| {
        let mut screens = Vec::new();
        for _ in 0..3 {
            let mut app = start_poll(size);
            app.key("down").key("down");
            want(&mut app, &INIT, "Pizza", None, 3);
            app.key("enter");
            assert_eq!(last_row(&app.screen()), "Voted for Pizza.");
            app.key("a").type_text("Ramen").key("enter");
            let end = [
                ("Tacos", 0),
                ("Sushi", 2),
                ("Pizza", 2),
                ("Salad", 0),
                ("Ramen", 0),
            ];
            want(&mut app, &end, "Pizza", Some("Pizza"), 4);
            assert_eq!(last_row(&app.screen()), "Option added.");
            screens.push(app.screen());
        }
        assert_eq!(screens[0], screens[1]);
        assert_eq!(screens[0], screens[2]);
    });
}

#[test]
fn case_11_remote_change() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        let theirs = app
            .store()
            .find_by("voter", "bo")
            .expect("bo's vote")
            .id
            .clone();
        let poll = poll_id(&mut app);
        let pushed = app.store().remote_update(
            &bo(),
            &theirs,
            content(json!({ "voter": "bo", "option": "Tacos" })),
        );
        app.deliver(pushed);
        let (_, pushed) = app.store().remote_create(
            &cy(),
            "vote",
            content(json!({ "voter": "eve", "option": "Salad" })),
        );
        app.deliver(pushed);
        let pushed = app.store().remote_update(
            &bo(),
            &poll,
            content(json!({
                "question": QUESTION,
                "options": ["Tacos", "Sushi", "Pizza", "Salad", "Ramen"],
            })),
        );
        app.deliver(pushed);
        // With no key pressed, and no reload.
        let after = [
            ("Tacos", 1),
            ("Sushi", 1),
            ("Pizza", 1),
            ("Salad", 1),
            ("Ramen", 0),
        ];
        want(&mut app, &after, "Tacos", None, 4);
        assert_eq!(app.store().count("list"), 2);
    });
}

#[test]
fn case_12_conflict() {
    at_each_size(|size| {
        let mut app = start_poll(size);
        let poll = poll_id(&mut app);
        // While ana adds Curry, bo adds Ramen.
        app.store().arm_conflict(
            &poll,
            &bo(),
            content(json!({
                "question": QUESTION,
                "options": ["Tacos", "Sushi", "Pizza", "Salad", "Ramen"],
            })),
        );
        app.key("a").type_text("Curry").key("enter");
        let merged = [
            ("Tacos", 0),
            ("Sushi", 2),
            ("Pizza", 1),
            ("Salad", 0),
            ("Ramen", 0),
            ("Curry", 0),
        ];
        want(&mut app, &merged, "Tacos", None, 3);
        assert_eq!(
            last_row(&app.screen()),
            "Conflict: merged with a newer change."
        );
        // Their option keeps its place and ours comes last.
        assert_eq!(
            options(&mut app),
            json!(["Tacos", "Sushi", "Pizza", "Salad", "Ramen", "Curry"])
        );
        assert_eq!(app.store().count("update"), 2);
    });
}

#[test]
fn a_changed_vote_is_one_record_and_votes_for_no_option_count_nowhere() {
    let mut app = start_on("poll.lua", POLL, (40, 16), |store| {
        lunch(store);
        store.seed(
            "vote",
            content(json!({ "voter": "eve", "option": "Burgers" })),
            &cy(),
        );
    });
    want(&mut app, &INIT, "Tacos", None, 3);
    app.key("enter");
    assert_eq!(last_row(&app.screen()), "Voted for Tacos.");
    // Voting for the option that holds the vote does nothing.
    app.key("enter");
    assert_eq!(writes(&mut app), 1);
    app.key("j").key("space");
    let moved = [("Tacos", 0), ("Sushi", 3), ("Pizza", 1), ("Salad", 0)];
    want(&mut app, &moved, "Sushi", Some("Sushi"), 4);
    assert_eq!(app.store().count("create"), 1);
    assert_eq!(app.store().count("update"), 1);
}

#[test]
fn a_channel_without_a_poll_is_asked_for_its_question() {
    // Not in the spec, which starts from a poll that is there: in a channel
    // somebody has to make it.
    let mut app = start("poll.lua", POLL);
    let screen = app.screen();
    assert!(
        screen.contains("No poll yet. Press a to ask a question."),
        "{screen}"
    );
    assert_eq!(last_row(&screen), "Loaded 0 options.");
    app.key("a");
    let screen = app.screen();
    assert!(
        screen.contains("New poll") && screen.contains("Question:"),
        "{screen}"
    );
    app.key("enter");
    assert!(app.screen().contains("Question is required."));
    app.type_text("Tea or coffee?").key("enter");
    let screen = app.screen();
    assert!(
        screen.contains("Tea or coffee?") && !screen.contains("New poll"),
        "{screen}"
    );
    assert_eq!(last_row(&screen), "Poll created.");
    // An empty table inside content is a list.
    assert_eq!(options(&mut app), json!([]));
    app.key("a").type_text("Tea").key("enter");
    app.key("a").type_text("Coffee").key("enter");
    app.key("j").key("enter");
    want(
        &mut app,
        &[("Tea", 0), ("Coffee", 1)],
        "Coffee",
        Some("Coffee"),
        1,
    );
}
