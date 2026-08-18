use std::{
    env,
    path::{Path, PathBuf},
};

use tokio::{
    io::AsyncReadExt as _,
    process::Command,
    time::{Duration, timeout},
};

const REQUIRED_FLAGS: &[&str] = &[
    "--json",
    "--ephemeral",
    "--ignore-user-config",
    "--ignore-rules",
    "--sandbox",
    "read-only",
    "--skip-git-repo-check",
];
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_OUTPUT_LIMIT: usize = 128 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExecutable(PathBuf);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Doctor {
    Ready,
    Unavailable,
    Unsupported,
}

impl CodexExecutable {
    pub fn resolve() -> Option<Self> {
        find_in_path("codex").map(Self)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn from_test_path(path: PathBuf) -> Self {
        Self(path)
    }

    pub async fn doctor(&self) -> Doctor {
        let mut command = Command::new(&self.0);
        command
            .arg("exec")
            .arg("--help")
            .env_clear()
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        restore_platform_environment(&mut command);
        let Ok(mut child) = command.spawn() else {
            return Doctor::Unavailable;
        };
        let Some(mut stdout) = child.stdout.take() else {
            return Doctor::Unavailable;
        };
        let read = tokio::spawn(async move {
            let mut output = Vec::new();
            let mut bytes = [0_u8; 4096];
            loop {
                let count = stdout.read(&mut bytes).await?;
                if count == 0 {
                    return Ok::<_, std::io::Error>(output);
                }
                if output.len().saturating_add(count) > PROBE_OUTPUT_LIMIT {
                    return Ok(Vec::new());
                }
                output.extend_from_slice(&bytes[..count]);
            }
        });
        let status = match timeout(PROBE_TIMEOUT, child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                read.abort();
                return Doctor::Unavailable;
            }
        };
        let output = read.await.ok().and_then(Result::ok).unwrap_or_default();
        match status {
            Ok(status) if status.success() => {
                let help = String::from_utf8(output).unwrap_or_default();
                if REQUIRED_FLAGS.iter().all(|flag| help.contains(flag)) {
                    Doctor::Ready
                } else {
                    Doctor::Unsupported
                }
            }
            _ => Doctor::Unavailable,
        }
    }
}

pub(super) fn command(executable: &CodexExecutable, cwd: &Path) -> Command {
    let mut command = Command::new(executable.path());
    command
        .args([
            "exec",
            "--json",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
        ])
        .current_dir(cwd)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    restore_platform_environment(&mut command);
    command
}

fn restore_platform_environment(command: &mut Command) {
    for name in [
        "HOME",
        "USERPROFILE",
        "CODEX_HOME",
        "PATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find_map(canonical_executable)
}

fn canonical_executable(path: PathBuf) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    canonical.is_file().then_some(canonical)
}

#[cfg(test)]
mod tests {
    use super::REQUIRED_FLAGS;

    #[test]
    fn capability_probe_requires_every_safety_flag() {
        let help = REQUIRED_FLAGS.join(" ");
        assert!(REQUIRED_FLAGS.iter().all(|flag| help.contains(flag)));
        assert!(!REQUIRED_FLAGS.iter().all(|flag| "--json".contains(flag)));
    }
}
