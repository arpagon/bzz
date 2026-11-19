//! The Board with priority and filter, driven without a window on the
//! in-memory store: the Board's twelve cases on cards that carry
//! priorities, then the five cases of the change
//! (`tests/apps-bench/spec/board-priority.md`,
//! `lib/cases_board_priority.py`), as the bench runs them. This is the
//! owner's example: an agent adds a priority and a filter to the channel's
//! board, and the board changes in place.

use serde_json::{Value, json};

use super::board::{self, Kit, Seen, read_whole, with_kit};
use super::*;

/// The Board's six cards, four of them with a priority.
fn six_cards_with_priorities(store: &mut MemoryStore) {
    let seed = [
        ("Alpha", "todo", Some(1)),
        ("Bravo", "todo", None),
        ("Charlie", "todo", Some(2)),
        ("Delta", "doing", Some(1)),
        ("Echo", "doing", Some(3)),
        ("Foxtrot", "done", None),
    ];
    for (order, (title, column, priority)) in seed.into_iter().enumerate() {
        let mut card = json!({ "title": title, "column": column, "order": order + 1 });
        if let Some(priority) = priority {
            card["priority"] = json!(priority);
        }
        store.seed("card", content(card), &bo());
    }
}

const KIT: Kit = Kit {
    name: "board-priority.lua",
    source: BOARD_PRIORITY,
    seed: six_cards_with_priorities,
    priorities: true,
};

/// The Board's own cases, on this board.
macro_rules! the_boards_cases {
    ($($case:ident),* $(,)?) => {
        $(
            #[test]
            fn $case() {
                with_kit(KIT, board::$case);
            }
        )*
    };
}

the_boards_cases!(
    case_01_first_draw,
    case_02_form_cancel,
    case_03_save_once,
    case_04_search_clear,
    case_05_late_search,
    case_06_small_screen,
    case_07_paste,
    case_08_load_error,
    case_09_quit_while_a_load_is_pending,
    case_10_repeatable,
    case_11_remote_change,
    case_12_conflict,
);

fn board_of(todo: &[&str], doing: &[&str], done: &[&str], selected: &str) -> Seen {
    board::board(todo, doing, done, Some(selected))
}

/// The board as it starts, with its tags, and `selected` marked.
fn initial(selected: &str) -> Seen {
    board_of(
        &["Alpha P1", "Bravo", "Charlie P2"],
        &["Delta P1", "Echo P3"],
        &["Foxtrot"],
        selected,
    )
}

#[track_caller]
fn want(app: &mut Headless, wanted: Seen, filter: &str) {
    let screen = app.screen();
    assert_eq!(read_whole(&screen), wanted, "{screen}");
    assert!(screen.contains(&format!("Priority: {filter}")), "{screen}");
}

fn stored(app: &mut Headless, title: &str) -> Value {
    Value::Object(
        app.store()
            .find_by("title", title)
            .expect("the card")
            .content
            .clone(),
    )
}

fn start_board(size: (u16, u16)) -> Headless {
    let mut app = start_on(KIT.name, KIT.source, size, KIT.seed);
    want(&mut app, initial("Alpha P1"), "all");
    assert_eq!(last_row(&app.screen()), "Loaded 6 cards.");
    assert_eq!(app.error(), None);
    app
}

#[test]
fn case_13_set_and_remove_a_priority() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("j").key("2");
        want(
            &mut app,
            board_of(
                &["Alpha P1", "Bravo P2", "Charlie P2"],
                &["Delta P1", "Echo P3"],
                &["Foxtrot"],
                "Bravo P2",
            ),
            "all",
        );
        assert_eq!(last_row(&app.screen()), "Saved.");
        // The rest of the card is kept: an update replaces the content.
        assert_eq!(
            stored(&mut app, "Bravo"),
            json!({ "title": "Bravo", "column": "todo", "order": 2, "priority": 2 })
        );
        // The priority the card already has: nothing is sent.
        app.key("2");
        assert_eq!(app.store().count("update"), 1);
        app.key("0");
        want(&mut app, initial("Bravo"), "all");
        assert_eq!(last_row(&app.screen()), "Saved.");
        assert_eq!(
            stored(&mut app, "Bravo"),
            json!({ "title": "Bravo", "column": "todo", "order": 2 })
        );
        assert_eq!(app.store().count("update"), 2);
    });
}

#[test]
fn case_14_the_filter_cycles() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("p");
        want(
            &mut app,
            board_of(&["Alpha P1"], &["Delta P1"], &[], "Alpha P1"),
            "P1",
        );
        app.key("p");
        want(
            &mut app,
            board_of(&["Charlie P2"], &[], &[], "Charlie P2"),
            "P2",
        );
        app.key("p");
        want(&mut app, board_of(&[], &["Echo P3"], &[], "Echo P3"), "P3");
        // Back to all, the selected card stays selected.
        app.key("p");
        want(&mut app, initial("Echo P3"), "all");
        app.key("k");
        want(&mut app, initial("Delta P1"), "all");
        assert_eq!(app.store().count("list"), 1, "the filter asks for nothing");
    });
}

#[test]
fn case_15_the_filter_and_the_search_combine() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("p").key("/").type_text("e");
        // P1 and "e": of Alpha and Delta, only Delta.
        want(
            &mut app,
            board_of(&[], &["Delta P1"], &[], "Delta P1"),
            "P1",
        );
        app.key("enter");
        assert!(app.screen().contains("Search: e"));
        // Cycling the filter keeps the search: P2 and "e" is Charlie.
        app.key("p");
        want(
            &mut app,
            board_of(&["Charlie P2"], &[], &[], "Charlie P2"),
            "P2",
        );
        // Clearing the search keeps the filter.
        app.key("p").key("esc");
        let screen = app.screen();
        assert!(!screen.contains("Search: e"), "{screen}");
        want(&mut app, board_of(&[], &["Echo P3"], &[], "Echo P3"), "P3");
    });
}

#[test]
fn case_16_remote_changes_respect_the_filter() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("p");
        want(
            &mut app,
            board_of(&["Alpha P1"], &["Delta P1"], &[], "Alpha P1"),
            "P1",
        );
        let (_, pushed) = app.store().remote_create(
            &bo(),
            "card",
            content(json!({ "title": "Zulu", "column": "doing", "priority": 1, "order": 1001 })),
        );
        app.deliver(pushed);
        let (_, pushed) = app.store().remote_create(
            &bo(),
            "card",
            content(json!({ "title": "Ship it", "column": "todo", "priority": 3, "order": 1002 })),
        );
        app.deliver(pushed);
        let delta = app
            .store()
            .find_by("title", "Delta")
            .expect("Delta")
            .id
            .clone();
        let pushed = app.store().remote_update(
            &bo(),
            &delta,
            content(json!({ "title": "Delta", "column": "doing", "priority": 2, "order": 4 })),
        );
        app.deliver(pushed);
        // Zulu is P1 and shows; Ship it is P3 and does not; Delta, now P2,
        // leaves the view. The counts follow.
        let screen = app.screen();
        want(
            &mut app,
            board_of(&["Alpha P1"], &["Zulu P1"], &[], "Alpha P1"),
            "P1",
        );
        assert!(!screen.contains("Ship it"), "{screen}");
        assert!(
            screen.contains("To do (1)")
                && screen.contains("Doing (1)")
                && screen.contains("Done (0)")
        );
        assert_eq!(app.store().count("list"), 1);
    });
}

#[test]
fn case_17_a_move_keeps_the_priority() {
    at_each_size(|size| {
        let mut app = start_board(size);
        app.key("]");
        want(
            &mut app,
            board_of(
                &["Bravo", "Charlie P2"],
                &["Alpha P1", "Delta P1", "Echo P3"],
                &["Foxtrot"],
                "Alpha P1",
            ),
            "all",
        );
        assert_eq!(last_row(&app.screen()), "Saved.");
        assert_eq!(
            stored(&mut app, "Alpha"),
            json!({ "title": "Alpha", "column": "doing", "order": 1, "priority": 1 })
        );
    });
}

#[test]
fn a_priority_set_while_another_renames_the_card_is_merged() {
    // The change's own conflict: like a move, the priority goes on top of
    // what the other participant wrote.
    let mut app = start_board((40, 16));
    let bravo = app
        .store()
        .find_by("title", "Bravo")
        .expect("Bravo")
        .id
        .clone();
    app.store().arm_conflict(
        &bravo,
        &bo(),
        content(json!({ "title": "Bravo (bo)", "column": "doing", "order": 2 })),
    );
    app.key("j").key("3");
    assert_eq!(
        last_row(&app.screen()),
        "Conflict: merged with a newer change."
    );
    assert_eq!(
        Value::Object(app.store().get(&bravo).expect("the card").content.clone()),
        json!({ "title": "Bravo (bo)", "column": "doing", "order": 2, "priority": 3 })
    );
}

#[test]
fn a_card_that_leaves_the_filter_gives_the_selection_to_the_first_shown() {
    let mut app = start_board((40, 16));
    app.key("p");
    want(
        &mut app,
        board_of(&["Alpha P1"], &["Delta P1"], &[], "Alpha P1"),
        "P1",
    );
    // Alpha becomes P2 under the filter P1: it leaves the view.
    app.key("2");
    want(
        &mut app,
        board_of(&[], &["Delta P1"], &[], "Delta P1"),
        "P1",
    );
    assert_eq!(last_row(&app.screen()), "Saved.");
}

#[test]
fn the_running_board_becomes_the_board_with_priority() {
    // The owner's guiding example, without a window: the script of the
    // open Board is replaced, and the board changes in place, with the
    // selection and the search still there.
    at_each_size(|size| {
        let mut app = start_on("board.lua", BOARD, size, six_cards_with_priorities);
        app.key("j").key("/").type_text("r").key("enter").key("j");
        let screen = app.screen();
        assert!(
            screen.contains("Search: r") && !screen.contains("Priority:"),
            "{screen}"
        );
        // The Board knows no priorities and shows none.
        assert_eq!(
            read_whole(&screen),
            board_of(&["Bravo", "Charlie"], &[], &["Foxtrot"], "Charlie"),
            "{screen}"
        );
        app.reload(BOARD_PRIORITY);
        assert_eq!(app.error(), None);
        let screen = app.screen();
        assert!(screen.contains("Search: r"), "{screen}");
        // The same cards, the same selection, and what the new version
        // knows of them.
        want(
            &mut app,
            board_of(&["Bravo", "Charlie P2"], &[], &["Foxtrot"], "Charlie P2"),
            "all",
        );
        // One list at the start and one for the search: the reload asked for
        // nothing.
        assert_eq!(app.store().count("list"), 2);
        // Its keys work at once.
        app.key("3");
        want(
            &mut app,
            board_of(&["Bravo", "Charlie P3"], &[], &["Foxtrot"], "Charlie P3"),
            "all",
        );
        app.key("p").key("p").key("p");
        want(
            &mut app,
            board_of(&["Charlie P3"], &[], &[], "Charlie P3"),
            "P3",
        );
        assert_eq!(
            stored(&mut app, "Charlie"),
            json!({ "title": "Charlie", "column": "todo", "order": 3, "priority": 3 })
        );
    });
}

#[test]
fn the_board_without_priorities_keeps_a_priority_it_does_not_know() {
    // Two versions of an app can be open in one channel at once. The older
    // one copies the content it writes, so a move does not lose the field.
    let mut app = start_on("board.lua", BOARD, (40, 16), six_cards_with_priorities);
    app.key("]");
    assert_eq!(last_row(&app.screen()), "Saved.");
    assert_eq!(
        stored(&mut app, "Alpha"),
        json!({ "title": "Alpha", "column": "doing", "order": 1, "priority": 1 })
    );
}
