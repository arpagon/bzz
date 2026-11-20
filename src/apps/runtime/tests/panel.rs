//! The Panel fixture, driven without a window on the in-memory store,
//! through the fifteen cases of its spec (`tests/apps-bench/spec/panel.md`,
//! `lib/cases_panel.py`): the app with a table, tabs, a sparkline and a
//! gauge. The sparkline and the gauge are read as the bench reads them, as
//! properties of the text and not as pictures (`lib/panel_props.py`).

use serde_json::{Value, json};

use super::*;
use crate::apps::runtime::Phase;

const NAMES: [&str; 6] = ["CPU", "Disk", "Latency", "Memory", "Queue", "Network"];

fn history(name: &str) -> Vec<u64> {
    match name {
        "CPU" => vec![35, 42, 50, 61, 70, 88, 93],
        "Disk" => vec![40, 41, 41, 42, 43, 43, 44],
        "Latency" => vec![120, 180, 260, 310, 420, 450, 480],
        "Memory" => vec![6, 7, 7, 8, 9, 8, 8],
        "Queue" => vec![300, 280, 350, 400, 380, 420, 390],
        _ => vec![],
    }
}

fn maximum(name: &str) -> u64 {
    match name {
        "CPU" | "Disk" => 100,
        "Latency" => 500,
        "Memory" => 16,
        _ => 1000,
    }
}

fn five_metrics(store: &mut MemoryStore) {
    let seed = [
        ("CPU", "%", "alert", None, None),
        ("Disk", "%", "ok", None, None),
        ("Latency", "ms", "alert", None, None),
        ("Memory", "GiB", "ok", None, None),
        ("Queue", "jobs", "acked", Some("bo"), Some("Known batch")),
    ];
    for (name, unit, status, acked_by, note) in seed {
        store.seed(
            "metric",
            content(json!({
                "name": name, "unit": unit, "status": status, "max": maximum(name),
                "history": history(name), "acked_by": acked_by, "note": note,
            })),
            &bo(),
        );
    }
}

type Row = (String, u64, String);

/// What a person sees of the panel.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    rows: Vec<Row>,
    selected: Option<String>,
    /// The active tab, the count of All and the count of Alerts.
    tabs: Option<(String, u64, u64)>,
    sort: Option<String>,
}

fn count_in(text: &str, label: &str) -> Option<u64> {
    let after = &text[text.find(label)? + label.len()..];
    after.split(')').next()?.parse().ok()
}

fn read(screen: &str) -> Seen {
    let mut seen = Seen {
        rows: vec![],
        selected: None,
        tabs: None,
        sort: None,
    };
    for row in above_status(screen) {
        if let (Some(all), Some(alerts)) = (count_in(row, "All ("), count_in(row, "Alerts (")) {
            let active = match (row.contains("*All ("), row.contains("*Alerts (")) {
                (true, false) => "All",
                (false, true) => "Alerts",
                _ => "?",
            };
            seen.tabs = Some((active.to_string(), all, alerts));
        } else if let Some(sort) = row.strip_prefix("Sort: ") {
            seen.sort = Some(sort.to_string());
        } else if let Some((selected, rest)) = marked(row) {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if let [name, value, _unit, status] = words[..]
                && NAMES.contains(&name)
                && let Ok(value) = value.parse()
            {
                if selected {
                    assert_eq!(seen.selected, None, "two rows are marked:\n{screen}");
                    seen.selected = Some(name.to_string());
                }
                seen.rows
                    .push((name.to_string(), value, status.to_string()));
            }
        }
    }
    seen
}

fn row(name: &str) -> Row {
    let (value, status) = match name {
        "CPU" => (93, "alert"),
        "Disk" => (44, "ok"),
        "Latency" => (480, "alert"),
        "Memory" => (8, "ok"),
        "Queue" => (390, "acked"),
        _ => panic!("no such metric at the start: {name}"),
    };
    (name.to_string(), value, status.to_string())
}

fn rows_of(names: &[&str]) -> Vec<Row> {
    names.iter().map(|n| row(n)).collect()
}

const BY_NAME: [&str; 5] = ["CPU", "Disk", "Latency", "Memory", "Queue"];

fn init() -> Vec<Row> {
    rows_of(&BY_NAME)
}

/// The five metrics by name, with one row replaced.
fn replaced(with: Row) -> Vec<Row> {
    init()
        .into_iter()
        .map(|r| if r.0 == with.0 { with.clone() } else { r })
        .collect()
}

fn tabs(active: &str, all: u64, alerts: u64) -> Option<(String, u64, u64)> {
    Some((active.to_string(), all, alerts))
}

#[track_caller]
fn want(app: &mut Headless, rows: &[Row], selected: &str) {
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, rows, "{screen}");
    assert_eq!(seen.selected.as_deref(), Some(selected), "{screen}");
}

const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The heights, in eighths of a cell, of the columns of the sparkline under
/// `History: <name>`, from the left, with the column each stands in.
fn sparkline(screen: &str, name: &str) -> Vec<(usize, usize)> {
    let rows: Vec<&str> = above_status(screen);
    let label = format!("History: {name}");
    let Some(at) = rows.iter().position(|r| r.trim_end() == label) else {
        panic!("no row {label:?}:\n{screen}");
    };
    let mut heights: Vec<(usize, usize)> = Vec::new();
    for row in &rows[at + 1..] {
        let cells: Vec<(usize, usize)> = row
            .chars()
            .enumerate()
            .filter_map(|(x, c)| Some((x, SPARK.iter().position(|g| *g == c)? + 1)))
            .collect();
        if cells.is_empty() {
            break;
        }
        for (x, level) in cells {
            match heights.iter_mut().find(|(column, _)| *column == x) {
                Some((_, height)) => *height += level,
                None => heights.push((x, level)),
            }
        }
    }
    heights.sort();
    heights
}

/// The spec's rules for a sparkline: one column for each value, side by
/// side; a higher value never drawn lower; the largest the tallest.
#[track_caller]
fn check_sparkline(app: &mut Headless, name: &str, values: &[u64]) {
    let screen = app.screen();
    let drawn = sparkline(&screen, name);
    let heights: Vec<usize> = drawn.iter().map(|(_, h)| *h).collect();
    assert_eq!(
        heights.len(),
        values.len(),
        "{heights:?} for {values:?}\n{screen}"
    );
    for pair in drawn.windows(2) {
        assert_eq!(
            pair[1].0,
            pair[0].0 + 1,
            "a gap in the sparkline:\n{screen}"
        );
    }
    for (i, a) in values.iter().enumerate() {
        for (j, b) in values.iter().enumerate() {
            assert!(
                !(a > b && heights[i] < heights[j]),
                "{heights:?} for {values:?}"
            );
        }
    }
    let (largest, smallest) = (
        values.iter().max().expect("values"),
        values.iter().min().expect("values"),
    );
    let top = values.iter().position(|v| v == largest).expect("it");
    let low = values.iter().position(|v| v == smallest).expect("it");
    assert_eq!(
        heights[top],
        *heights.iter().max().expect("heights"),
        "{heights:?}"
    );
    if smallest * 4 <= largest * 3 {
        assert!(heights[top] > heights[low], "{heights:?} for {values:?}");
    }
}

/// The gauge: its percentage and how many cells of it are filled.
fn gauge(screen: &str) -> Option<(u64, usize)> {
    above_status(screen).into_iter().find_map(|row| {
        let at = row.find('%')?;
        let digits: String = row[..at]
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let percent = digits.parse().ok()?;
        let filled = row.chars().filter(|c| *c == '█').count();
        (filled > 0 || percent < 10).then_some((percent, filled))
    })
}

#[track_caller]
fn check_gauge(app: &mut Headless, value: u64, maximum: u64) -> usize {
    let screen = app.screen();
    let wanted = (value * 100 + maximum / 2) / maximum;
    let Some((percent, filled)) = gauge(&screen) else {
        panic!("no gauge with {wanted}%:\n{screen}");
    };
    assert_eq!(percent, wanted, "{screen}");
    filled
}

fn stored(app: &mut Headless, name: &str) -> Value {
    Value::Object(
        app.store()
            .find_by("name", name)
            .expect("the metric")
            .content
            .clone(),
    )
}

fn start_panel(size: (u16, u16)) -> Headless {
    let mut app = start_on("panel.lua", PANEL, size, five_metrics);
    let screen = app.screen();
    let seen = read(&screen);
    assert_eq!(seen.rows, init(), "{size:?}\n{screen}");
    assert_eq!(seen.selected.as_deref(), Some("CPU"), "{screen}");
    assert_eq!(seen.tabs, tabs("All", 5, 2), "{screen}");
    assert_eq!(seen.sort.as_deref(), Some("Name asc"), "{screen}");
    assert_eq!(last_row(&screen), "Loaded 5 metrics.", "{screen}");
    assert_eq!(app.error(), None);
    app
}

#[test]
fn case_01_first_draw() {
    // Left out: "the program is still running 0.5 s later".
    for size in [(40, 16), (80, 24), (120, 40)] {
        let mut app = start_panel(size);
        fits(&app.screen(), size);
        check_sparkline(&mut app, "CPU", &history("CPU"));
        check_gauge(&mut app, 93, 100);
        assert_eq!(app.phase(), Phase::Running);
    }
}

#[test]
fn case_02_form_cancel() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.key("a");
        let screen = app.screen();
        assert!(
            screen.contains("Acknowledge CPU") && screen.contains("Note:"),
            "{screen}"
        );
        app.type_text("Café 日本");
        assert!(app.screen().contains("Note: Café 日本"), "{}", app.screen());
        app.key("esc");
        assert!(!app.screen().contains("Acknowledge CPU"));
        want(&mut app, &init(), "CPU");
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 2));
        assert_eq!(app.store().count("update"), 0);
        app.key("j");
        want(&mut app, &init(), "Disk");
    });
}

#[test]
fn case_03_save_once() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.key("a").key("enter");
        assert!(
            app.screen().contains("Note is required."),
            "{}",
            app.screen()
        );
        assert_eq!(app.store().count("update"), 0);
        app.store().hold_next("update");
        app.type_text("On it").key("enter").key("enter");
        assert_eq!(last_row(&app.screen()), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one write");
        app.deliver(replies);
        want(
            &mut app,
            &replaced(("CPU".into(), 93, "acked".into())),
            "CPU",
        );
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 1));
        assert_eq!(last_row(&app.screen()), "Acknowledged.");
        assert_eq!(app.store().count("update"), 1);
        // The acknowledgement on top of the metric: its history is kept.
        assert_eq!(
            stored(&mut app, "CPU"),
            json!({
                "name": "CPU", "unit": "%", "status": "acked", "max": 100,
                "history": history("CPU"), "acked_by": "ana", "note": "On it",
            })
        );
    });
}

#[test]
fn case_04_tabs() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.key("j").key("j");
        want(&mut app, &init(), "Latency");
        app.key("tab");
        let alerts = rows_of(&["CPU", "Latency"]);
        want(&mut app, &alerts, "Latency");
        assert_eq!(read(&app.screen()).tabs, tabs("Alerts", 5, 2));
        // Back to All, the selected metric stays selected.
        app.key("tab");
        want(&mut app, &init(), "Latency");
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 2));
        app.key("k");
        want(&mut app, &init(), "Disk");
        // Disk is not in alert: on Alerts the first row is selected.
        app.key("tab");
        want(&mut app, &alerts, "CPU");
        assert_eq!(app.store().count("list"), 1, "a tab asks for nothing");
    });
}

#[test]
fn case_05_late_reload() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.store().hold_next("list");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loading...");
        app.key("a").type_text("x").key("enter");
        let acked = replaced(("CPU".into(), 93, "acked".into()));
        want(&mut app, &acked, "CPU");
        assert_eq!(last_row(&app.screen()), "Acknowledged.");
        // The reload's reply predates the acknowledgement.
        let late = app.store().release();
        assert_eq!(late.len(), 1);
        app.deliver(late);
        want(&mut app, &acked, "CPU");
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 1));
    });
}

#[test]
fn case_06_another_size() {
    at_each_size(|size| {
        let other = other_size(size);
        let mut app = start_panel(size);
        app.resize(other.0, other.1);
        let screen = app.screen();
        fits(&screen, other);
        let seen = read(&screen);
        assert_eq!(seen.rows, init(), "{screen}");
        assert_eq!(seen.tabs, tabs("All", 5, 2));
        assert_eq!(seen.sort.as_deref(), Some("Name asc"));
        assert_eq!(last_row(&screen), "Loaded 5 metrics.");
        check_sparkline(&mut app, "CPU", &history("CPU"));
        check_gauge(&mut app, 93, 100);
        app.key("j");
        want(&mut app, &init(), "Disk");
        check_gauge(&mut app, 44, 100);
        app.key("s");
        want(
            &mut app,
            &rows_of(&["Memory", "Disk", "CPU", "Queue", "Latency"]),
            "Disk",
        );
        assert_eq!(read(&app.screen()).sort.as_deref(), Some("Value asc"));
        app.key("tab");
        want(&mut app, &rows_of(&["CPU", "Latency"]), "CPU");
        fits(&app.screen(), other);
        app.resize(size.0, size.1);
        let screen = app.screen();
        fits(&screen, size);
        let seen = read(&screen);
        assert_eq!(seen.rows, rows_of(&["CPU", "Latency"]), "{screen}");
        assert_eq!(seen.tabs, tabs("Alerts", 5, 2));
        assert_eq!(seen.sort.as_deref(), Some("Value asc"));
    });
}

#[test]
fn case_07_paste() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.paste("a\ns\nd\tq\n");
        assert!(!app.closed(), "the pasted q closed the app");
        let screen = app.screen();
        assert!(!screen.contains("Acknowledge"), "{screen}");
        want(&mut app, &init(), "CPU");
        assert_eq!(read(&screen).sort.as_deref(), Some("Name asc"));
        assert_eq!(app.store().count("update"), 0);
        app.key("a").paste("one\ntwo\r\nq");
        let screen = app.screen();
        assert!(screen.contains("Note: one two q"), "{screen}");
        assert!(screen.contains("Acknowledge CPU"));
        assert_eq!(app.store().count("update"), 0);
        app.key("esc");
        assert!(!app.screen().contains("Acknowledge CPU"));
    });
}

#[test]
fn case_08_load_error() {
    at_each_size(|size| {
        let mut app = start_on("panel.lua", PANEL, size, |store| {
            five_metrics(store);
            store.fail_next("list", unavailable());
        });
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        app.key("r");
        want(&mut app, &init(), "CPU");
        assert_eq!(last_row(&app.screen()), "Loaded 5 metrics.");
        app.key("j");
        app.store().fail_next("list", unavailable());
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        want(&mut app, &init(), "Disk");
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 2));
        app.key("r");
        want(&mut app, &init(), "Disk");
        assert_eq!(last_row(&app.screen()), "Loaded 5 metrics.");
    });
}

#[test]
fn case_09_quit_while_a_load_is_pending() {
    // Bench: `q` ends the process within 2 s with status 0. Here: the app
    // asks to close at once, with a request unanswered.
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.store().hold_next("list");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loading...");
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
            let mut app = start_panel(size);
            app.key("s").key("down").key("tab");
            want(&mut app, &rows_of(&["CPU", "Latency"]), "CPU");
            assert_eq!(read(&app.screen()).sort.as_deref(), Some("Value asc"));
            app.key("down").key("a");
            assert!(app.screen().contains("Acknowledge Latency"));
            app.type_text("ok").key("enter");
            want(&mut app, &rows_of(&["CPU"]), "CPU");
            assert_eq!(read(&app.screen()).tabs, tabs("Alerts", 5, 1));
            assert_eq!(last_row(&app.screen()), "Acknowledged.");
            screens.push(app.screen());
        }
        assert_eq!(screens[0], screens[1]);
        assert_eq!(screens[0], screens[2]);
    });
}

/// The content of a metric with some fields changed.
fn changed(
    app: &mut Headless,
    name: &str,
    change: Value,
) -> (String, serde_json::Map<String, Value>) {
    let record = app
        .store()
        .find_by("name", name)
        .expect("the metric")
        .clone();
    let mut content = record.content;
    for (key, value) in change.as_object().expect("an object") {
        content.insert(key.clone(), value.clone());
    }
    (record.id, content)
}

#[test]
fn case_11_remote_change() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        let before = check_gauge(&mut app, 93, 100);
        let mut more = history("CPU");
        more.push(100);
        let (cpu, content_now) = changed(&mut app, "CPU", json!({ "history": more }));
        let pushed = app.store().remote_update(&bo(), &cpu, content_now);
        app.deliver(pushed);
        let mut memory = history("Memory");
        memory.push(15);
        let (mem, content_now) = changed(
            &mut app,
            "Memory",
            json!({ "status": "alert", "history": memory }),
        );
        let pushed = app.store().remote_update(&cy(), &mem, content_now);
        app.deliver(pushed);
        let (_, pushed) = app.store().remote_create(
            &bo(),
            "metric",
            content(json!({
                "name": "Network", "unit": "Mbps", "status": "ok", "max": 1000,
                "history": [200, 250, 300], "acked_by": null, "note": null,
            })),
        );
        app.deliver(pushed);
        let disk = app
            .store()
            .find_by("name", "Disk")
            .expect("Disk")
            .id
            .clone();
        let pushed = app.store().remote_delete(&bo(), &disk);
        app.deliver(pushed);
        // With no key pressed, and no reload.
        let after = vec![
            ("CPU".to_string(), 100, "alert".to_string()),
            row("Latency"),
            ("Memory".to_string(), 15, "alert".to_string()),
            ("Network".to_string(), 300, "ok".to_string()),
            row("Queue"),
        ];
        want(&mut app, &after, "CPU");
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 3));
        check_sparkline(&mut app, "CPU", &more);
        let now = check_gauge(&mut app, 100, 100);
        assert!(
            now > before,
            "the gauge grows from 93% to 100%: {before} to {now} cells"
        );
        assert_eq!(app.store().count("list"), 1);
    });
}

#[test]
fn case_12_conflict() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        // While ana acknowledges CPU, bo adds a value to it.
        let mut more = history("CPU");
        more.push(97);
        let (cpu, theirs) = changed(&mut app, "CPU", json!({ "history": more }));
        app.store().arm_conflict(&cpu, &bo(), theirs);
        app.key("a").type_text("On it").key("enter");
        want(
            &mut app,
            &replaced(("CPU".into(), 97, "acked".into())),
            "CPU",
        );
        assert_eq!(read(&app.screen()).tabs, tabs("All", 5, 1));
        assert_eq!(
            last_row(&app.screen()),
            "Conflict: merged with a newer change."
        );
        // Nothing lost: bo's value, and ana's acknowledgement.
        assert_eq!(
            stored(&mut app, "CPU"),
            json!({
                "name": "CPU", "unit": "%", "status": "acked", "max": 100,
                "history": more, "acked_by": "ana", "note": "On it",
            })
        );
        assert_eq!(app.store().count("update"), 2);
    });
}

#[test]
fn case_13_sort() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        let steps: [(&str, &str, [&str; 5]); 6] = [
            (
                "s",
                "Value asc",
                ["Memory", "Disk", "CPU", "Queue", "Latency"],
            ),
            (
                "d",
                "Value desc",
                ["Latency", "Queue", "CPU", "Disk", "Memory"],
            ),
            (
                "s",
                "Status asc",
                ["Queue", "CPU", "Latency", "Disk", "Memory"],
            ),
            (
                "d",
                "Status desc",
                ["Disk", "Memory", "CPU", "Latency", "Queue"],
            ),
            (
                "s",
                "Name asc",
                ["CPU", "Disk", "Latency", "Memory", "Queue"],
            ),
            (
                "d",
                "Name desc",
                ["Queue", "Memory", "Latency", "Disk", "CPU"],
            ),
        ];
        for (key, sort, names) in steps {
            app.key(key);
            // CPU stays selected through every sort.
            want(&mut app, &rows_of(&names), "CPU");
            assert_eq!(read(&app.screen()).sort.as_deref(), Some(sort));
        }
        app.key("k");
        want(&mut app, &rows_of(&steps[5].2), "Disk");
        assert_eq!(app.store().count("list"), 1, "a sort asks for nothing");
    });
}

#[test]
fn case_14_sparkline_and_gauge_follow_the_selection() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        let cpu = check_gauge(&mut app, 93, 100);
        for name in ["Disk", "Latency", "Memory", "Queue"] {
            app.key("j");
            want(&mut app, &init(), name);
            let values = history(name);
            check_sparkline(&mut app, name, &values);
            let filled = check_gauge(&mut app, values[values.len() - 1], maximum(name));
            if name == "Disk" {
                assert!(filled < cpu, "Disk at 44% fills less than CPU at 93%");
            }
        }
    });
}

#[test]
fn case_15_what_can_be_acknowledged() {
    at_each_size(|size| {
        let mut app = start_panel(size);
        app.key("j").key("a");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Nothing to acknowledge.");
        assert!(!screen.contains("Acknowledge"), "{screen}");
        app.key("j").key("j").key("j");
        want(&mut app, &init(), "Queue");
        app.key("r");
        assert_eq!(last_row(&app.screen()), "Loaded 5 metrics.");
        app.key("a");
        assert_eq!(last_row(&app.screen()), "Nothing to acknowledge.");
        assert_eq!(app.store().count("update"), 0);
        app.key("k").key("k").key("k").key("k");
        want(&mut app, &init(), "CPU");
        // While ana writes her note, bo acknowledges it.
        let (cpu, theirs) = changed(
            &mut app,
            "CPU",
            json!({ "status": "acked", "acked_by": "bo", "note": "Mine" }),
        );
        app.store().arm_conflict(&cpu, &bo(), theirs.clone());
        app.key("a").type_text("On it").key("enter");
        want(
            &mut app,
            &replaced(("CPU".into(), 93, "acked".into())),
            "CPU",
        );
        assert_eq!(last_row(&app.screen()), "Already acknowledged by bo.");
        // Theirs stands: no second update.
        assert_eq!(stored(&mut app, "CPU"), Value::Object(theirs));
        assert_eq!(app.store().count("update"), 1);
    });
}
