//! bzz does its work on a thread whose stack it sets itself (8 MiB), not on
//! the program's first thread, which Windows gives 1 MiB. Seen in CI on
//! Windows (v0.15.0): a debug build of `bzz broker list` overflowed its
//! stack once the commands for agents were added.

/// The same on Unix, where the first thread's stack can be made as small as
/// Windows makes it: with 1 MiB, bzz still answers.
#[cfg(unix)]
#[test]
fn bzz_answers_with_the_first_threads_stack_of_windows() {
    let temporary = tempfile::TempDir::new().unwrap();
    for arguments in ["--version", "broker list"] {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("ulimit -s 1024 && exec \"$BZZ\" {arguments}"))
            .env("BZZ", env!("CARGO_BIN_EXE_bzz"))
            .env("BZZ_CONFIG_DIR", temporary.path().join("c"))
            .env("BZZ_DATA_DIR", temporary.path().join("d"))
            .env("BZZ_CACHE_DIR", temporary.path().join("k"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bzz {arguments}: {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
