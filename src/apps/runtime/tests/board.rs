//! The Board fixture, driven without a window on the in-memory store,
//! through the twelve cases the experiment's bench runs against a terminal
//! program (`bench/spec/board.md` of the experiment, `cases_board.py`).
//!
//! Each case runs at 40 columns by 16 rows, the size of the app pane, where
//! the three columns are stacked, and at 80 by 24, where they stand side by
//! side. What a case of the bench checks about the process and its terminal
//! (the exit status, echo and the alternate screen restored, no child left)
//! has no meaning here; each case says what it leaves out.

use serde_json::json;
use unicode_width::UnicodeWidthChar;

use super::*;
use crate::apps::runtime::Phase;
use crate::apps::runtime::memory::content;
use crate::apps::runtime::records::RecordError;

const SIZES: [(u16, u16); 2] = [(40, 16), (80, 24)];
const HEADERS: [&str; 3] = ["To do (", "Doing (", "Done ("];

/// What a person sees of the board: the cards under each header, and which
/// is marked as selected.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    todo: Vec<String>,
    doing: Vec<String>,
    done: Vec<String>,
    selected: Option<String>,
}

/// The text of a row from display column `from` up to `to`.
fn columns(row: &str, from: usize, to: usize) -> String {
    let mut at = 0;
    let mut out = String::new();
    for c in row.chars() {
        if at >= from && at < to {
            out.push(c);
        }
        at += c.width().unwrap_or(0);
    }
    out
}

fn column_of(row: &str, needle: &str) -> Option<usize> {
    let byte = row.find(needle)?;
    Some(row[..byte].chars().map(|c| c.width().unwrap_or(0)).sum())
}

/// Reads a board as the bench does: a card belongs to the header it stands
/// under, and a card row is a marker (`> ` or two spaces) and a title.
fn read(screen: &str) -> Seen {
    let rows: Vec<&str> = screen.lines().collect();
    let mut seen = Seen {
        todo: vec![],
        doing: vec![],
        done: vec![],
        selected: None,
    };
    for (which, header) in HEADERS.iter().enumerate() {
        let Some((y, x)) = rows
            .iter()
            .enumerate()
            .find_map(|(y, row)| column_of(row, header).map(|x| (y, x)))
        else {
            panic!("no header {header:?} on the screen:\n{screen}");
        };
        // The header's count is what the app says it shows.
        let said: usize = columns(rows[y], x, usize::MAX)
            .split(['(', ')'])
            .nth(1)
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no count in {:?}", rows[y]));
        // Its column ends where the next header on the same row begins.
        let end = HEADERS
            .iter()
            .filter_map(|other| column_of(rows[y], other))
            .filter(|other| *other > x)
            .min()
            .unwrap_or(usize::MAX);
        let mut cards = Vec::new();
        for row in &rows[y + 1..] {
            let cell = columns(row, x, end);
            let (marker, title) = (columns(&cell, 0, 2), columns(&cell, 2, usize::MAX));
            let title = title.trim_end().to_string();
            let is_card =
                (marker == "> " || marker == "  ") && !title.is_empty() && !title.starts_with(' ');
            if !is_card {
                break;
            }
            if marker == "> " {
                assert_eq!(seen.selected, None, "two cards are marked:\n{screen}");
                seen.selected = Some(title.clone());
            }
            cards.push(title);
        }
        assert!(
            cards.len() <= said,
            "{header}{said}) shows {} cards:\n{screen}",
            cards.len()
        );
        match which {
            0 => seen.todo = cards,
            1 => seen.doing = cards,
            _ => seen.done = cards,
        }
    }
    seen
}

fn board(todo: &[&str], doing: &[&str], done: &[&str], selected: Option<&str>) -> Seen {
    let own = |titles: &[&str]| titles.iter().map(|t| t.to_string()).collect();
    Seen {
        todo: own(todo),
        doing: own(doing),
        done: own(done),
        selected: selected.map(str::to_string),
    }
}

/// The board every case starts from, with `selected` marked.
fn initial(selected: &str) -> Seen {
    board(
        &["Alpha", "Bravo", "Charlie"],
        &["Delta", "Echo"],
        &["Foxtrot"],
        Some(selected),
    )
}

fn start_board(size: (u16, u16)) -> Headless {
    let mut app = start_on("board.lua", BOARD, size, six_cards);
    let screen = app.screen();
    assert_eq!(read(&screen), initial("Alpha"), "{size:?}\n{screen}");
    assert_eq!(last_row(&screen), "Loaded 6 cards.", "{screen}");
    assert_eq!(app.error(), None);
    app
}

fn at_each_size(case: impl Fn((u16, u16))) {
    for size in SIZES {
        case(size);
    }
}

#[test]
fn case_01_first_draw() {
    // Bench: the board is right at two sizes and nothing is written outside
    // the screen. Here: three sizes, and that drawing stays inside its
    // area is `drawing::nothing_is_drawn_outside_the_area`.
    // Left out: "the program is still running 0.5 s later".
    for size in [(40, 16), (80, 24), (120, 40)] {
        let mut app = start_board(size);
        let screen = app.screen();
        assert_eq!(screen.lines().count(), size.1 as usize);
        assert!(
            screen.lines().all(
                |row| row.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>() <= size.0 as usize
            ),
            "{screen}"
        );
        assert_eq!(app.phase(), Phase::Running);
    }
}

#[test]
fn case_02_form_cancel() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("j");
        assert_eq!(read(&app.screen()), initial("Bravo"));
        app.key("n");
        let screen = app.screen();
        assert!(
            screen.contains("New card") && screen.contains("Title:"),
            "{screen}"
        );
        app.type_text("Café 日本");
        assert!(
            app.screen().contains("Title: Café 日本"),
            "{}",
            app.screen()
        );
        app.key("esc");
        let screen = app.screen();
        assert!(!screen.contains("New card"), "{screen}");
        // Nothing saved, the selection kept.
        assert_eq!(read(&screen), initial("Bravo"));
        assert_eq!(app.store().count("create"), 0);
        assert!(app.store().find_by("title", "Café 日本").is_none());
        // Keys are back on the board.
        app.key("j");
        assert_eq!(read(&app.screen()), initial("Charlie"));
    });
}

#[test]
fn case_03_save_once() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("n").key("enter");
        assert!(
            app.screen().contains("Title is required."),
            "{}",
            app.screen()
        );
        assert_eq!(app.store().count("create"), 0);
        // The write is slow: its reply is held back while Enter is pressed
        // a second time.
        app.store().hold_next("create");
        app.type_text("Write tests").key("enter").key("enter");
        let screen = app.screen();
        assert!(
            screen.contains("New card"),
            "the form stays open:\n{screen}"
        );
        assert_eq!(last_row(&screen), "Saving...");
        let replies = app.store().release();
        assert_eq!(replies.len(), 1, "one confirmation, one card");
        app.deliver(replies);
        let screen = app.screen();
        let after = board(
            &["Alpha", "Bravo", "Charlie", "Write tests"],
            &["Delta", "Echo"],
            &["Foxtrot"],
            Some("Write tests"),
        );
        assert_eq!(read(&screen), after, "{screen}");
        assert_eq!(last_row(&screen), "Saved.");
        let written = app
            .store()
            .records()
            .iter()
            .filter(|r| r.content.get("title") == Some(&json!("Write tests")))
            .count();
        assert_eq!(written, 1);
        assert_eq!(app.store().count("create"), 1);
    });
}

#[test]
fn case_04_search_clear() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("j");
        assert_eq!(read(&app.screen()), initial("Bravo"));
        app.key("/");
        assert!(app.screen().contains("Search:"));
        app.type_text("echo");
        let only_echo = board(&[], &["Echo"], &[], Some("Echo"));
        assert_eq!(read(&app.screen()), only_echo, "{}", app.screen());
        app.key("enter");
        let screen = app.screen();
        assert!(screen.contains("Search: echo"), "{screen}");
        assert_eq!(read(&screen), only_echo);
        app.key("esc");
        let screen = app.screen();
        assert!(!screen.contains("Search: echo"), "{screen}");
        // Clearing the filter keeps the selected card selected.
        assert_eq!(read(&screen), initial("Echo"));
        app.key("k");
        assert_eq!(read(&app.screen()), initial("Delta"));
    });
}

#[test]
fn case_05_late_search() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("/");
        // The reply to the first search comes late: after the second's.
        app.store().hold_next("list");
        app.type_text("e");
        app.type_text("c");
        let only_echo = board(&[], &["Echo"], &[], Some("Echo"));
        let screen = app.screen();
        assert!(screen.contains("Search: ec"), "{screen}");
        assert_eq!(read(&screen), only_echo, "{screen}");
        let late = app.store().release();
        assert_eq!(late.len(), 1);
        app.deliver(late);
        // The late reply to "e" would add Charlie and Delta.
        assert_eq!(read(&app.screen()), only_echo, "{}", app.screen());
        assert_eq!(app.store().list_matches(), ["", "e", "ec"]);
    });
}

#[test]
fn case_06_small_screen() {
    // Bench: 40x12 is too small for a board that needs 60x16. The fixture
    // needs 40x16, so one row or one column less is too small here.
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("j");
        for small in [(39, 16), (40, 15), (20, 5)] {
            app.resize(small.0, small.1);
            let screen = app.screen();
            let said: String = "Terminal too small. Need 40x16."
                .chars()
                .take(small.0 as usize)
                .collect();
            assert_eq!(screen.lines().next(), Some(said.trim_end()), "{screen}");
            assert!(
                !screen.contains("Alpha") && !screen.contains("Bravo"),
                "{screen}"
            );
            assert_eq!(screen.lines().count(), small.1 as usize);
            // Back at its size the board returns with the same selection.
            app.resize(size.0, size.1);
            assert_eq!(read(&app.screen()), initial("Bravo"));
        }
        // `q` still closes from the small-screen state.
        app.resize(39, 16).key("q");
        assert!(app.closed());
    });
}

#[test]
fn case_07_paste() {
    at_each_size(|size| {
        let mut app = start_board(size);
        // A paste on the board is ignored: one piece of text, never keys.
        app.paste("n\nq\n");
        assert!(!app.closed(), "the pasted q closed the app");
        let screen = app.screen();
        assert!(!screen.contains("New card"), "{screen}");
        assert_eq!(read(&screen), initial("Alpha"));
        app.key("n").paste("line one\nline two\r\nq");
        let screen = app.screen();
        assert!(screen.contains("Title: line one line two q"), "{screen}");
        assert!(screen.contains("New card"));
        assert_eq!(app.store().count("create"), 0);
        app.key("esc");
        assert!(!app.screen().contains("New card"));
    });
}

#[test]
fn case_08_load_error() {
    at_each_size(|size| {
        let mut app = start_on("board.lua", BOARD, size, |store| {
            six_cards(store);
            store.fail_next(
                "list",
                RecordError::Unavailable {
                    message: "no connection".into(),
                },
            );
        });
        assert_eq!(last_row(&app.screen()), "Load failed. Press r to retry.");
        app.key("r");
        let screen = app.screen();
        assert_eq!(read(&screen), initial("Alpha"));
        assert_eq!(last_row(&screen), "Loaded 6 cards.");
        app.key("j");
        app.store().fail_next(
            "list",
            RecordError::Unavailable {
                message: "no connection".into(),
            },
        );
        app.key("r");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Load failed. Press r to retry.");
        // Every card that was shown before the failed load is kept.
        assert_eq!(read(&screen), initial("Bravo"));
        app.key("r");
        let screen = app.screen();
        assert_eq!(last_row(&screen), "Loaded 6 cards.");
        assert_eq!(read(&screen), initial("Bravo"));
    });
}

#[test]
fn case_09_quit_while_a_load_is_pending() {
    // Bench: `q` ends the process within 2 s with status 0, no child left
    // and the terminal restored. Here: the app asks to close, at once, with
    // a request still unanswered. The process and the terminal are bzz's.
    at_each_size(|size| {
        let mut app = start_board(size);
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
            let mut app = start_board(size);
            app.key("right");
            assert_eq!(read(&app.screen()), initial("Delta"));
            app.key("down");
            assert_eq!(read(&app.screen()), initial("Echo"));
            app.key("]");
            let moved = board(
                &["Alpha", "Bravo", "Charlie"],
                &["Delta"],
                &["Echo", "Foxtrot"],
                Some("Echo"),
            );
            let screen = app.screen();
            assert_eq!(read(&screen), moved, "{screen}");
            assert_eq!(last_row(&screen), "Saved.");
            app.key("n").type_text("Ship it").key("enter");
            let screen = app.screen();
            let end = board(
                &["Alpha", "Bravo", "Charlie", "Ship it"],
                &["Delta"],
                &["Echo", "Foxtrot"],
                Some("Ship it"),
            );
            assert_eq!(read(&screen), end, "{screen}");
            assert_eq!(last_row(&screen), "Saved.");
            screens.push(screen);
        }
        assert_eq!(screens[0], screens[1]);
        assert_eq!(screens[0], screens[2]);
    });
}

#[test]
fn case_11_remote_change() {
    at_each_size(|size| {
        let mut app = start_board(size);
        let lists = app.store().count("list");
        let alpha = app
            .store()
            .find_by("title", "Alpha")
            .expect("Alpha")
            .id
            .clone();
        let charlie = app
            .store()
            .find_by("title", "Charlie")
            .expect("Charlie")
            .id
            .clone();
        let renamed = app.store().remote_update(
            &bo(),
            &alpha,
            content(json!({ "title": "Alpha v2", "column": "todo", "order": 1 })),
        );
        app.deliver(renamed);
        let (_, created) = app.store().remote_create(
            &bo(),
            "card",
            content(json!({ "title": "Zulu", "column": "doing", "order": 7 })),
        );
        app.deliver(created);
        let deleted = app.store().remote_delete(&bo(), &charlie);
        app.deliver(deleted);
        // With no key pressed, and no reload.
        let screen = app.screen();
        let after = board(
            &["Alpha v2", "Bravo"],
            &["Delta", "Echo", "Zulu"],
            &["Foxtrot"],
            Some("Alpha v2"),
        );
        assert_eq!(read(&screen), after, "{screen}");
        assert_eq!(app.store().count("list"), lists);
    });
}

#[test]
fn case_12_conflict() {
    at_each_size(|size| {
        let mut app = start_board(size);
        let alpha = app
            .store()
            .find_by("title", "Alpha")
            .expect("Alpha")
            .id
            .clone();
        // While the app moves Alpha, bo renames it.
        app.store().arm_conflict(
            &alpha,
            &bo(),
            content(json!({ "title": "Alpha (bo)", "column": "todo", "order": 1 })),
        );
        app.key("]");
        let screen = app.screen();
        let merged = board(
            &["Bravo", "Charlie"],
            &["Alpha (bo)", "Delta", "Echo"],
            &["Foxtrot"],
            Some("Alpha (bo)"),
        );
        assert_eq!(read(&screen), merged, "{screen}");
        assert_eq!(last_row(&screen), "Conflict: merged with a newer change.");
        // Nothing lost: bo's title and the app's column.
        let stored = app.store().get(&alpha).expect("the card").clone();
        assert_eq!(
            serde_json::Value::Object(stored.content),
            json!({ "title": "Alpha (bo)", "column": "doing", "order": 1 })
        );
        assert_eq!(stored.by, ana());
        assert_eq!(app.store().count("update"), 2);
    });
}

#[test]
fn a_failed_move_puts_the_card_back() {
    // Not a case of the bench: the experiment found that past the relay's
    // quota a board said only "Save failed." (E4, 2.5). With `api = 1` the
    // reply says how long to wait, and the pane can say it.
    let mut app = start_board((40, 16));
    app.store().set_quota(1, std::time::Duration::from_secs(60));
    app.key("]");
    assert_eq!(last_row(&app.screen()), "Saved.");
    app.advance(18_000);
    app.key("]");
    let screen = app.screen();
    assert_eq!(last_row(&screen), "Too many changes. Retry in 42 s.");
    // The card is where the store has it.
    let moved = board(
        &["Bravo", "Charlie"],
        &["Alpha", "Delta", "Echo"],
        &["Foxtrot"],
        Some("Alpha"),
    );
    assert_eq!(read(&screen), moved, "{screen}");
    // An archived channel refuses, and says so.
    app.advance(60_000);
    app.store().set_refusal(Some("the channel is archived"));
    app.key("]");
    let screen = app.screen();
    assert_eq!(last_row(&screen), "Refused: the channel is archived");
    assert_eq!(read(&screen), moved, "{screen}");
}
