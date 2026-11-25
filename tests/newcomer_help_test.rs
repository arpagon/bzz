//! What a newcomer reads before bzz has anything of theirs: `bzz help`
//! names the demo first, `bzz demo --help` says what it is, that nothing is
//! saved and how to leave, and every shell's completion offers `demo`.

use std::process::Command;

fn bzz(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_bzz"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "bzz {arguments:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn help_lists_the_demo_first() {
    for arguments in [&["help"][..], &["--help"]] {
        let help = bzz(arguments);
        let first = help
            .split_once("Commands:\n")
            .and_then(|(_, commands)| commands.lines().next())
            .unwrap_or_default();
        assert!(
            first.trim_start().starts_with("demo "),
            "{arguments:?}: {help}"
        );
        assert!(first.contains("offline"), "{first}");
    }
}

#[test]
fn the_demos_help_says_what_it_is_that_nothing_is_saved_and_how_to_leave() {
    let help = bzz(&["demo", "--help"]);
    let about = help.split_once("\n\nUsage:").map_or("", |(about, _)| about);
    for said in ["generated team", "offline", "read or saved", "q, then y"] {
        assert!(about.contains(said), "{said:?} in:\n{help}");
    }
    // Two lines of prose before the usage, each fitting 80 columns.
    let lines = about.lines().filter(|line| !line.is_empty()).count();
    assert!(lines <= 3, "{about}");
    assert!(
        about.lines().all(|line| line.chars().count() <= 80),
        "{about}"
    );
}

#[test]
fn every_shells_completion_offers_the_demo() {
    for shell in ["bash", "elvish", "fish", "power-shell", "zsh"] {
        let script = bzz(&["completions", shell]);
        assert!(script.contains("demo"), "{shell}");
    }
    // Where the shell says what a command is, the demo's words are there.
    for shell in ["fish", "zsh"] {
        let script = bzz(&["completions", shell]);
        assert!(script.contains("generated team"), "{shell}");
    }
}
