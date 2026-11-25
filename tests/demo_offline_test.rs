//! `bzz demo` in a terminal with no network (plan v0.17.0, A1 and rule 22).
//!
//! The binary runs in tmux, inside a network namespace of its own
//! (`unshare -rn`) whose only interface is loopback, the way
//! `docker --network none` gives it. The test sees the full first screen,
//! quits, and checks that no demo folder is left and that the home it ran
//! with is byte for byte what it was.
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
