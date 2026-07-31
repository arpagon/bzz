//! Generate completion files from the installed or freshly built `bzz` binary.
//!
//! Usage:
//! `BZZ_BIN=target/release/bzz rustc scripts/generate-completions.rs -o /tmp/bzz-completions && /tmp/bzz-completions target/completions`

use std::{env, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/completions"));
    let binary = env::var_os("BZZ_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release/bzz"));
    fs::create_dir_all(&output_dir)?;

    for (shell, filename) in [
        ("bash", "bzz.bash"),
        ("elvish", "bzz.elv"),
        ("fish", "bzz.fish"),
        ("power-shell", "_bzz.ps1"),
        ("zsh", "_bzz"),
    ] {
        let output = Command::new(&binary)
            .args(["completions", shell])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "{} completions failed: {}",
                shell,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        fs::write(output_dir.join(filename), output.stdout)?;
    }
    Ok(())
}
