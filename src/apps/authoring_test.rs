//! v0.16 M5: the author guide's code loads on the runtime.
//!
//! `docs/app-authoring.md` is what an agent writes apps from (`app_guide`
//! returns it with the contract). Every Lua block in it is a whole script:
//! each must load apart and draw at the pane's size, and the first, the
//! guide's example app, must do what the guide says it does, at 40×16 and
//! at a narrow and a wide size.

use super::dev::{Trial, parse_seed, try_script};
use super::runtime::Limits;
use super::session::headless::Step;
use super::store::load_apart;

const GUIDE: &str = include_str!("../../docs/app-authoring.md");

/// The Lua blocks of the guide, in their order.
fn lua_blocks() -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in GUIDE.lines() {
        match &mut current {
            None if line.trim_start() == "```lua" => current = Some(String::new()),
            Some(block) if line.trim_start() == "```" => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
            None => {}
        }
    }
    assert!(current.is_none(), "a Lua block of the guide is not closed");
    blocks
}

fn steps(written: &[&str]) -> Vec<Step> {
    written
        .iter()
        .map(|step| Step::parse(step).unwrap())
        .collect()
}

#[test]
fn every_lua_block_of_the_guide_loads_and_draws() {
    let blocks = lua_blocks();
    assert!(blocks.len() >= 2, "the example and the reload: {blocks:?}");
    for (index, block) in blocks.iter().enumerate() {
        let loaded = load_apart(
            block.as_bytes(),
            &format!("guide-{}.lua", index + 1),
            &Limits::patient(),
        )
        .unwrap_or_else(|error| panic!("block {} of the guide: {error}", index + 1));
        assert!(!loaded.types.is_empty(), "block {}", index + 1);
        assert!(
            !loaded.screen.trim().is_empty(),
            "block {} draws nothing",
            index + 1
        );
    }
}

#[test]
fn the_guides_example_counts_and_fits_the_pane() {
    let tally = lua_blocks().remove(0);
    let seeds = parse_seed(
        r#"[{"type":"counter","content":{"title":"coffee","count":3,"order":1},"by":"bo"},
            {"type":"counter","content":{"title":"tea","count":1,"order":2}}]"#,
    )
    .unwrap();
    let trial = |width, height, keys: &[&str]| {
        try_script(Trial {
            script: tally.clone().into_bytes(),
            script_name: "tally.lua".into(),
            width,
            height,
            steps: steps(keys),
            seeds: seeds.clone(),
        })
        .unwrap()
        .tried
    };
    // At the pane's size: the two counters in their order, a new one, and
    // counting on the selected one.
    let tried = trial(40, 16, &["j", "+", "+", "n", "type:milk", "enter", "j"]);
    assert_eq!(tried.error, None, "{}", tried.screen);
    assert_eq!(tried.types, ["counter"]);
    let lines: Vec<&str> = tried.screen.lines().collect();
    assert_eq!(lines.len(), 16);
    assert!(lines.iter().all(|line| line.chars().count() <= 40));
    assert_eq!(lines[0].trim_end(), "Tally");
    let row = |index: usize| lines[index].trim_end();
    assert!(
        row(1).contains("coffee") && row(1).ends_with("   3"),
        "{lines:?}"
    );
    assert!(
        row(2).contains("tea") && row(2).ends_with("   3"),
        "{lines:?}"
    );
    assert!(
        row(3).starts_with("> milk") && row(3).ends_with("   0"),
        "{lines:?}"
    );
    assert!(
        tried.screen.contains("3 counters  n new  + -  q"),
        "{}",
        tried.screen
    );
    // Who made a counter is stored by id and shown by host.name.
    assert!(tried.screen.contains("made by "), "{}", tried.screen);
    // Narrow and wide: nothing is cut but the titles, nothing fails.
    for (width, height) in [(20, 8), (100, 30)] {
        let tried = trial(
            width,
            height,
            &["+", "n", "paste:a long title for a counter", "enter"],
        );
        assert_eq!(tried.error, None, "{width}x{height}: {}", tried.screen);
        assert_eq!(tried.screen.lines().count(), usize::from(height));
        assert!(tried.screen.contains("coffee"), "{}", tried.screen);
    }
}
