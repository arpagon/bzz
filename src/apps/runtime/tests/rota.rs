//! The Rota fixture, driven without a window on the in-memory store,
//! through the fourteen cases of its spec (`tests/apps-bench/spec/rota.md`,
//! `lib/cases_rota.py`). What a case checks about the process and its
//! terminal has no meaning here; each case says what it leaves out.

use serde_json::{Value, json};

use super::*;
use crate::apps::runtime::Phase;

const WEEK: [(&str, Option<&str>); 7] = [
    ("Mon", Some("bo")),
    ("Tue", None),
    ("Wed", Some("cy")),
    ("Thu", None),
    ("Fri", Some("bo")),
    ("Sat", None),
    ("Sun", None),
];

fn week(store: &mut MemoryStore) {
    for (order, (day, holder)) in WEEK.into_iter().enumerate() {
        store.seed(
            "day",
            content(json!({ "day": day, "holder": holder, "order": order + 1 })),
            &bo(),
        );
    }
}

/// What a person sees of the rota.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    rows: Vec<(String, Option<String>)>,
    selected: Option<String>,
    days: Option<String>,
    show: Option<String>,
}

fn read(screen: &str) -> Seen {
    let mut seen = Seen {
        rows: vec![],
        selected: None,
        days: None,
        show: None,
    };
    for row in above_status(screen) {
        if let Some(days) = row.strip_prefix("Days: ") {
            seen.days = Some(days.to_string());
        } else if let Some(show) = row.strip_prefix("Show: ") {
            seen.show = Some(show.to_string());
        } else if let Some((selected, rest)) = marked(row) {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if let [day, holder] = words[..]
                && WEEK.iter().any(|(known, _)| *known == day)
            {
                if selected {
                    assert_eq!(seen.selected, None, "two days are marked:\n{screen}");
                    seen.selected = Some(day.to_string());
                }
                let holder = (holder != "free").then(|| holder.to_string());
                seen.rows.push((day.to_string(), holder));
            }
        }
    }
    seen
}

/// The week with some days held by someone else than at the start.
fn held(changes: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    WEEK.iter()
        .map(|(day, holder)| {
            let holder = changes
                .iter()
                .find(|(changed, _)| changed == day)
                .map_or(*holder, |(_, now)| *now);
            (day.to_string(), holder.map(str::to_string))
        })
        .collect()
}

fn only(days: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    days.iter()
        .map(|(day, holder)| (day.to_string(), holder.map(str::to_string)))
        .collect()
}

#[track_caller]
fn want(app: &mut Headless, rows: &[(String, Option<String>)], selected: &str) {
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, rows, "{screen}");
    assert_eq!(seen.selected.as_deref(), Some(selected), "{screen}");
}

fn holder(app: &mut Headless, day: &str) -> Value {
    app.store()
        .find_by("day", day)
        .expect("the day")
        .content
        .get("holder")
        .cloned()
        .unwrap_or(Value::Null)
}

fn start_rota(size: (u16, u16)) -> Headless {
    let mut app = start_on("rota.lua", ROTA, size, week);
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, held(&[]), "{size:?}\n{screen}");
    assert_eq!(seen.selected.as_deref(), Some("Mon"), "{screen}");
    assert_eq!(seen.days.as_deref(), Some("bo 2, cy 1"), "{screen}");
    assert_eq!(seen.show.as_deref(), Some("all"), "{screen}");
    assert_eq!(last_row(&screen), "Loaded 7 days.", "{screen}");
    assert_eq!(app.error(), None);
    app
}

#[test]
fn case_01_first_draw() {
    // Left out: "the program is still running 0.5 s later".
    for size in [(40, 16), (80, 24), (120, 40)] {
        let mut app = start_rota(size);
        fits(&app.screen(), size);
        assert_eq!(app.phase(), Phase::Running);
    }
}

#[test]
fn case_02_confirm_cancel() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.key("j").key("t");
        let took = held(&[("Tue", Some("ana"))]);
        want(&mut app, &took, "Tue");
        assert_eq!(last_row(&app.screen()), "Took Tue.");
        app.key("g");
        let screen = app.screen();
        assert!(
            screen.contains("Give up Tue?") && screen.contains("Enter confirm  Esc cancel"),
            "{screen}"
        );
        // Only Enter and Esc act on the confirmation.
        app.key("q").key("j");
        assert!(!app.closed());
        assert!(app.screen().contains("Give up Tue?"));
        app.key("esc");
        assert!(!app.screen().contains("Give up Tue?"));
        want(&mut app, &took, "Tue");
        assert_eq!(app.store().count("update"), 1);
        assert_eq!(holder(&mut app, "Tue"), json!("ana"));
        app.key("j");
        want(&mut app, &took, "Wed");
    });
}

#[test]
fn case_03_save_once() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        // The write is slow: its reply is held back while t is pressed again.
        app.store().hold_next("update");
        app.key("j").key("t").key("t");
        assert_eq!(last_row(&app.screen()), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one key, one write");
        app.deliver(replies);
        let took = held(&[("Tue", Some("ana"))]);
        want(&mut app, &took, "Tue");
        assert_eq!(last_row(&app.screen()), "Took Tue.");
        assert_eq!(app.store().count("update"), 1);
        app.key("g");
        assert!(app.screen().contains("Give up Tue?"));
        app.store().hold_next("update");
        app.key("enter").key("enter");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one write");
        app.deliver(replies);
        want(&mut app, &held(&[]), "Tue");
        assert_eq!(last_row(&app.screen()), "Gave up Tue.");
        assert_eq!(app.store().count("update"), 2);
        // Free is a null in the record, and every day kept its place: an
        // update replaces the whole content.
        assert_eq!(holder(&mut app, "Tue"), Value::Null);
        let tue = app.store().find_by("day", "Tue").expect("Tue").clone();
        assert_eq!(
            Value::Object(tue.content),
            json!({ "day": "Tue", "holder": null, "order": 2 })
        );
    });
}

#[test]
fn case_04_filter() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.key("j").key("t");
        let took = held(&[("Tue", Some("ana"))]);
        want(&mut app, &took, "Tue");
        app.key("j").key("j");
        want(&mut app, &took, "Thu");
        app.key("f");
        let free = only(&[("Thu", None), ("Sat", None), ("Sun", None)]);
        want(&mut app, &free, "Thu");
        assert_eq!(read(&app.screen()).show.as_deref(), Some("free"));
        app.key("j");
        want(&mut app, &free, "Sat");
        // Thu is not ana's: under "mine" the first shown day is selected.
        app.key("k").key("f");
        want(&mut app, &only(&[("Tue", Some("ana"))]), "Tue");
        assert_eq!(read(&app.screen()).show.as_deref(), Some("mine"));
        // Back to all, the selected day stays selected.
        app.key("f");
        want(&mut app, &took, "Tue");
        assert_eq!(read(&app.screen()).show.as_deref(), Some("all"));
        app.key("j");
        want(&mut app, &took, "Wed");
        assert_eq!(app.store().count("list"), 1, "a filter asks for nothing");
    });
}

#[test]
fn case_05_late_reload() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.store().hold_next("list");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loading...");
        app.key("j").key("t");
        let took = held(&[("Tue", Some("ana"))]);
        want(&mut app, &took, "Tue");
        assert_eq!(last_row(&app.screen()), "Took Tue.");
        // The reload's reply predates the take: it must not undo it.
        let late = app.store().release();
        assert_eq!(late.len(), 1);
        app.deliver(late);
        want(&mut app, &took, "Tue");
        assert_eq!(
            read(&app.screen()).days.as_deref(),
            Some("ana 1, bo 2, cy 1")
        );
    });
}

#[test]
fn case_06_another_size() {
    at_each_size(|size| {
        let other = other_size(size);
        let mut app = start_rota(size);
        app.key("j");
        app.resize(other.0, other.1);
        let screen = app.screen();
        fits(&screen, other);
        let seen = read(&screen);
        assert_eq!(seen.rows, held(&[]), "{screen}");
        assert_eq!(seen.selected.as_deref(), Some("Tue"));
        assert_eq!(seen.days.as_deref(), Some("bo 2, cy 1"));
        assert_eq!(last_row(&screen), "Loaded 7 days.");
        app.key("t");
        let took = held(&[("Tue", Some("ana"))]);
        want(&mut app, &took, "Tue");
        assert_eq!(last_row(&app.screen()), "Took Tue.");
        app.key("f");
        want(
            &mut app,
            &only(&[("Thu", None), ("Sat", None), ("Sun", None)]),
            "Thu",
        );
        app.key("f").key("f");
        assert_eq!(read(&app.screen()).rows, took);
        fits(&app.screen(), other);
        app.resize(size.0, size.1);
        let screen = app.screen();
        fits(&screen, size);
        assert_eq!(read(&screen).rows, took, "{screen}");
        assert_eq!(read(&screen).show.as_deref(), Some("all"));
    });
}

#[test]
fn case_07_paste() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        // The rota has no text field: a paste is one piece of text, never keys.
        app.paste("j\nt\nf\nq\n");
        assert!(!app.closed(), "the pasted q closed the app");
        want(&mut app, &held(&[]), "Mon");
        assert_eq!(app.store().count("update"), 0);
        app.key("j").key("t").key("g");
        assert!(app.screen().contains("Give up Tue?"));
        app.paste("x\r\nq");
        assert!(!app.closed());
        assert!(app.screen().contains("Give up Tue?"));
        assert_eq!(app.store().count("update"), 1);
        app.key("esc");
        assert!(!app.screen().contains("Give up Tue?"));
    });
}

#[test]
fn case_08_load_error() {
    at_each_size(|size| {
        let mut app = start_on("rota.lua", ROTA, size, |store| {
            week(store);
            store.fail_next("list", unavailable());
        });
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        app.key("r");
        want(&mut app, &held(&[]), "Mon");
        assert_eq!(last_row(&app.screen()), "Loaded 7 days.");
        app.key("j");
        app.store().fail_next("list", unavailable());
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        // Everything shown before the failed load is kept.
        want(&mut app, &held(&[]), "Tue");
        assert_eq!(read(&app.screen()).days.as_deref(), Some("bo 2, cy 1"));
        app.key("r");
        want(&mut app, &held(&[]), "Tue");
        assert_eq!(last_row(&app.screen()), "Loaded 7 days.");
    });
}

#[test]
fn case_09_quit_while_a_write_is_pending() {
    // Bench: `q` ends the process within 2 s with status 0. Here: the app
    // asks to close at once, with its write unanswered.
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.store().hold_next("update");
        app.key("j").key("t");
        assert_eq!(last_row(&app.screen()), "Saving...");
        app.key("q");
        assert!(app.closed());
        assert_eq!(app.phase(), Phase::Closed);
    });
}

#[test]
fn case_10_repeatable() {
    at_each_size(|size| {
        let mut screens = Vec::new();
        for _ in 0..3 {
            let mut app = start_rota(size);
            app.key("j").key("t");
            assert_eq!(last_row(&app.screen()), "Took Tue.");
            app.key("down").key("down").key("t");
            assert_eq!(last_row(&app.screen()), "Took Thu.");
            app.key("f");
            want(&mut app, &only(&[("Sat", None), ("Sun", None)]), "Sat");
            screens.push(app.screen());
        }
        assert_eq!(screens[0], screens[1]);
        assert_eq!(screens[0], screens[2]);
    });
}

#[test]
fn case_11_remote_change() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        let tue = app.store().find_by("day", "Tue").expect("Tue").id.clone();
        let wed = app.store().find_by("day", "Wed").expect("Wed").id.clone();
        let taken = app.store().remote_update(
            &bo(),
            &tue,
            content(json!({ "day": "Tue", "holder": "bo", "order": 2 })),
        );
        app.deliver(taken);
        let given = app.store().remote_update(
            &cy(),
            &wed,
            content(json!({ "day": "Wed", "holder": null, "order": 3 })),
        );
        app.deliver(given);
        // With no key pressed, and no reload.
        want(
            &mut app,
            &held(&[("Tue", Some("bo")), ("Wed", None)]),
            "Mon",
        );
        assert_eq!(read(&app.screen()).days.as_deref(), Some("bo 3"));
        assert_eq!(app.store().count("list"), 1);
    });
}

#[test]
fn case_12_conflict() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        let tue = app.store().find_by("day", "Tue").expect("Tue").id.clone();
        // While ana takes Tue, bo takes it.
        app.store().arm_conflict(
            &tue,
            &bo(),
            content(json!({ "day": "Tue", "holder": "bo", "order": 2 })),
        );
        app.key("j").key("t");
        want(&mut app, &held(&[("Tue", Some("bo"))]), "Tue");
        assert_eq!(last_row(&app.screen()), "Tue was taken by bo.");
        // A rota never overwrites: no second update.
        assert_eq!(holder(&mut app, "Tue"), json!("bo"));
        assert_eq!(app.store().count("update"), 1);
        assert_eq!(app.store().get(&tue).expect("Tue").by, bo());
    });
}

#[test]
fn case_13_held_day() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.key("t");
        assert_eq!(last_row(&app.screen()), "Held by bo.");
        app.key("g");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Not your day.");
        assert!(!screen.contains("Give up"), "{screen}");
        want(&mut app, &held(&[]), "Mon");
        assert_eq!(app.store().count("update"), 0);
        assert_eq!(holder(&mut app, "Mon"), json!("bo"));
    });
}

#[test]
fn case_14_counts_and_mine() {
    at_each_size(|size| {
        let mut app = start_rota(size);
        app.key("j").key("t");
        want(&mut app, &held(&[("Tue", Some("ana"))]), "Tue");
        assert_eq!(
            read(&app.screen()).days.as_deref(),
            Some("ana 1, bo 2, cy 1")
        );
        app.key("j").key("j").key("t");
        want(
            &mut app,
            &held(&[("Tue", Some("ana")), ("Thu", Some("ana"))]),
            "Thu",
        );
        assert_eq!(
            read(&app.screen()).days.as_deref(),
            Some("ana 2, bo 2, cy 1")
        );
        // Under "free" Thu is not shown and Sat is selected; under "mine"
        // Sat is not shown, so the first shown day is.
        app.key("f").key("f");
        let mine = only(&[("Tue", Some("ana")), ("Thu", Some("ana"))]);
        want(&mut app, &mine, "Tue");
        let seen = read(&app.screen());
        assert_eq!(seen.show.as_deref(), Some("mine"));
        // The count is of every day, whatever the filter.
        assert_eq!(seen.days.as_deref(), Some("ana 2, bo 2, cy 1"));
        app.key("j");
        want(&mut app, &mine, "Thu");
        app.key("g");
        assert!(app.screen().contains("Give up Thu?"));
        app.key("enter");
        // Given up, it leaves "mine".
        want(&mut app, &only(&[("Tue", Some("ana"))]), "Tue");
        assert_eq!(
            read(&app.screen()).days.as_deref(),
            Some("ana 1, bo 2, cy 1")
        );
        assert_eq!(last_row(&app.screen()), "Gave up Thu.");
    });
}

#[test]
fn a_take_past_the_quota_says_how_long_to_wait() {
    // Not a case of the bench: `rate_limited` is the contract's.
    let mut app = start_rota((40, 16));
    app.store().set_quota(1, std::time::Duration::from_secs(60));
    app.key("j").key("t");
    assert_eq!(last_row(&app.screen()), "Took Tue.");
    app.advance(18_000);
    app.key("j").key("j").key("t");
    assert_eq!(last_row(&app.screen()), "Too many changes. Retry in 42 s.");
    want(&mut app, &held(&[("Tue", Some("ana"))]), "Thu");
}
