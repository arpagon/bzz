//! The launch kit's README is read on a machine without bzz, by an owner who
//! goes back to bzz to do what it says. Every `bzz agent …` command it names
//! with a flag must be one bzz takes. Seen in the recorded run of v0.15.0:
//! it named `bzz agent edit <name> --no-packages`, which does not exist.

use std::process::Command;

#[test]
fn every_flag_the_launch_kits_readme_names_is_one_bzz_takes() {
    let readme = include_str!("../src/agents/launch_kit_readme.md");
    let mut checked = 0;
    for command in readme
        .split('`')
        .filter(|text| text.starts_with("bzz agent "))
    {
        let words: Vec<&str> = command.split_whitespace().collect();
        let sub = words[2];
        let help = Command::new(env!("CARGO_BIN_EXE_bzz"))
            .args(["agent", sub, "--help"])
            .output()
            .unwrap();
        assert!(help.status.success(), "`bzz agent {sub}` is not a command");
        let help = String::from_utf8_lossy(&help.stdout);
        for flag in words.iter().filter(|word| word.starts_with("--")) {
            assert!(
                help.split(
                    |character: char| !(character.is_ascii_alphanumeric() || character == '-')
                )
                .any(|word| word == *flag),
                "the README says `{command}`, and `bzz agent {sub}` takes no {flag}"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the README names no flag: the test reads nothing"
    );
}
