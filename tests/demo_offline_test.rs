//! `bzz demo` in a terminal with no network (plan v0.17.0, A1 and rule 22).
//!
//! The binary runs in tmux, inside a network namespace of its own
//! (`unshare -rn`) whose only interface is loopback, the way
//! `docker --network none` gives it. The test sees the full first screen,
//! quits, and checks that no demo folder is left and that the home it ran
//! with is byte for byte what it was. A second run adds a card on the
//! Board of `#launch` and moves it (plan A2), with the same checks.
//!
//! The time limit is 1 s for the binary named by `BZZ_DEMO_BIN` (the release
//! build the proof uses), and 10 s for the debug build `cargo test` makes.
//! Without tmux, `unshare` or `ip`, or where user namespaces are refused,
//! the test says so and passes.
#![cfg(target_os = "linux")]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn works(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Every file under `root`, with its bytes.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.insert(path.strip_prefix(root).unwrap().to_path_buf(), Vec::new());
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap_or_default(),
                );
            }
        }
    }
    files
}

struct Tmux {
    socket: PathBuf,
}

impl Tmux {
    fn run(&self, arguments: &[&str]) -> String {
        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn screen(&self) -> String {
        self.run(&["capture-pane", "-p", "-t", "demo"])
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        self.run(&["kill-server"]);
    }
}

/// The first screen is full: `#team` open, the demo's mark, the newest
/// message with its author, the people's names and the DM.
fn full(screen: &str) -> bool {
    screen.contains("┌ team ")
        && screen.contains("DEMO · generated, offline")
        && screen.contains("Ada ")
        && screen.contains("Three")
        && screen.contains("@Lin")
        && screen.contains("#infra")
}

#[test]
fn the_demo_fills_its_first_screen_offline_and_leaves_nothing() {
    if !works("tmux", &["-V"]) || !works("unshare", &["-rn", "ip", "link", "set", "lo", "up"]) {
        eprintln!("skipped: needs tmux, and unshare -rn with ip (user namespaces)");
        return;
    }
    let (binary, limit) = match std::env::var("BZZ_DEMO_BIN") {
        Ok(binary) => (binary, Duration::from_secs(1)),
        Err(_) => (
            env!("CARGO_BIN_EXE_bzz").to_owned(),
            Duration::from_secs(10),
        ),
    };
    let home = tempfile::TempDir::new().unwrap();
    let temporary = tempfile::TempDir::new().unwrap();
    let sockets = tempfile::TempDir::new().unwrap();
    // A configuration of the person's own, which the demo must not touch.
    let config = home.path().join(".config/bzz");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "# the person's own\n").unwrap();
    let before = tree(home.path());

    let tmux = Tmux {
        socket: sockets.path().join("tmux"),
    };
    let home_dir = home.path().display().to_string();
    let inner = format!(
        "ip link set lo up && exec env -u DBUS_SESSION_BUS_ADDRESS HOME={home_dir} \
         XDG_CONFIG_HOME={home_dir}/.config XDG_DATA_HOME={home_dir}/.local/share \
         XDG_CACHE_HOME={home_dir}/.cache XDG_STATE_HOME={home_dir}/.local/state \
         TMPDIR={} {binary} demo",
        temporary.path().display()
    );
    let command = format!("unshare -rn sh -c '{inner}'; echo DEMO-ENDED $?; sleep 30");
    let started = Instant::now();
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "demo",
        "-x",
        "80",
        "-y",
        "24",
        &command,
    ]);
    let mut screen = String::new();
    let mut seen = None;
    while started.elapsed() < Duration::from_secs(20) {
        screen = tmux.screen();
        if full(&screen) {
            seen = Some(started.elapsed());
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let seen = seen.unwrap_or_else(|| panic!("the first screen never filled:\n{screen}"));
    eprintln!("full first screen after {seen:?} (limit {limit:?})");
    assert!(seen <= limit, "{seen:?} > {limit:?}:\n{screen}");
    let folders = std::fs::read_dir(temporary.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("bzz-demo-"))
        .count();
    assert_eq!(folders, 1, "the demo works in one folder of its own");

    // Leave: `q`, then `y`.
    std::thread::sleep(Duration::from_millis(300));
    tmux.run(&["send-keys", "-t", "demo", "q"]);
    let asked = Instant::now();
    while !tmux.screen().contains("quit bzz?") && asked.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    tmux.run(&["send-keys", "-t", "demo", "y"]);
    let left = Instant::now();
    while !tmux.screen().contains("DEMO-ENDED") && left.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(tmux.screen().contains("DEMO-ENDED 0"), "{}", tmux.screen());
    let leftovers = std::fs::read_dir(temporary.path()).unwrap().count();
    assert_eq!(leftovers, 0, "no demo folder is left");
    assert_eq!(tree(home.path()), before, "the home is untouched");
}

/// Waits until `wanted` holds of the screen, or panics with the screen.
fn wait_for(tmux: &Tmux, what: &str, wanted: impl Fn(&str) -> bool) -> String {
    let started = Instant::now();
    loop {
        let screen = tmux.screen();
        if wanted(&screen) {
            return screen;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{what} never showed:\n{screen}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The Board (v0.17 A2): in `#launch`, offline, `5` focuses it, `n`, a
/// title and Enter add a card, `]` moves it to Doing. Leaving takes the
/// card with the demo: no folder is left, the home is untouched.
#[test]
fn the_board_takes_a_card_offline_and_the_demo_leaves_nothing() {
    if !works("tmux", &["-V"]) || !works("unshare", &["-rn", "ip", "link", "set", "lo", "up"]) {
        eprintln!("skipped: needs tmux, and unshare -rn with ip (user namespaces)");
        return;
    }
    let binary =
        std::env::var("BZZ_DEMO_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_bzz").to_owned());
    let home = tempfile::TempDir::new().unwrap();
    let temporary = tempfile::TempDir::new().unwrap();
    let sockets = tempfile::TempDir::new().unwrap();
    let config = home.path().join(".config/bzz");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "# the person's own\n").unwrap();
    let before = tree(home.path());
    let tmux = Tmux {
        socket: sockets.path().join("tmux"),
    };
    let home_dir = home.path().display().to_string();
    let inner = format!(
        "ip link set lo up && exec env -u DBUS_SESSION_BUS_ADDRESS HOME={home_dir} \
         XDG_CONFIG_HOME={home_dir}/.config XDG_DATA_HOME={home_dir}/.local/share \
         XDG_CACHE_HOME={home_dir}/.cache XDG_STATE_HOME={home_dir}/.local/state \
         TMPDIR={} {binary} demo",
        temporary.path().display()
    );
    let command = format!("unshare -rn sh -c '{inner}'; echo DEMO-ENDED $?; sleep 30");
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "demo",
        "-x",
        "80",
        "-y",
        "24",
        &command,
    ]);
    let keys = |keys: &[&str]| {
        let mut arguments = vec!["send-keys", "-t", "demo"];
        arguments.extend(keys);
        tmux.run(&arguments);
    };
    wait_for(&tmux, "the first screen", full);
    std::thread::sleep(Duration::from_millis(300));
    keys(&["Space", "Space"]);
    wait_for(&tmux, "the channel finder", |screen| {
        screen.contains("channel finder")
    });
    keys(&["-l", "launch"]);
    keys(&["Enter"]);
    wait_for(&tmux, "the Board in #launch", |screen| {
        screen.contains("┌ launch ")
            && screen.contains("Board · 5 focus")
            && screen.contains("To do (3)")
    });
    keys(&["5"]);
    wait_for(&tmux, "the Board's focus", |screen| {
        screen.contains("Board · Ctrl-c leave")
    });
    keys(&["n"]);
    wait_for(&tmux, "the form", |screen| screen.contains("New card"));
    keys(&["-l", "Press kit"]);
    keys(&["Enter"]);
    wait_for(&tmux, "the new card", |screen| {
        screen.contains("To do (4)") && screen.contains("> Press kit")
    });
    keys(&["]"]);
    let screen = wait_for(&tmux, "the card in Doing", |screen| {
        screen.contains("Doing (3)") && screen.contains("To do (3)")
    });
    // In the next column: after "Doing" and before "Done".
    let doing = screen.find("Doing (3)").unwrap();
    let card = screen.find("> Press kit").unwrap();
    let done = screen.find("Done (3)").unwrap();
    assert!(doing < card && card < done, "{screen}");

    keys(&["C-c"]);
    std::thread::sleep(Duration::from_millis(300));
    keys(&["q"]);
    wait_for(&tmux, "the question", |screen| screen.contains("quit bzz?"));
    keys(&["y"]);
    let screen = wait_for(&tmux, "the end", |screen| screen.contains("DEMO-ENDED"));
    assert!(screen.contains("DEMO-ENDED 0"), "{screen}");
    let leftovers = std::fs::read_dir(temporary.path()).unwrap().count();
    assert_eq!(leftovers, 0, "no demo folder is left");
    assert_eq!(tree(home.path()), before, "the home is untouched");
}
