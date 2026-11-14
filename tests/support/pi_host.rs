//! For tests that run bzz's real Pi Durable host (`agent-host/`): the Node
//! of this machine, and a stand-in for `node` that switches on the host's
//! scripted model.
//!
//! The host adds its stand-in model only when its environment names one
//! (`BZZ_AGENT_HOST_SCRIPTED_MODEL`), and bzz builds that environment from
//! nothing. So a test cannot switch it on through bzz: it gives bzz, as
//! `BZZ_NODE`, a small program that sets the variable and then runs the real
//! Node. bzz itself never sets it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use bzz::agents::runtime::install::{self, Node};

/// The model the host's stand-in plays (`agent-host/test/support`).
pub const MODEL: &str = "scripted/scripted";

fn real_environment(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The Node these tests run on, or `None` with a message when this machine
/// has none that is new enough. `BZZ_TEST_REQUIRE_NODE=1` (the relay job in
/// CI) turns the skip into a failure.
pub async fn node_or_skip(test: &str) -> Option<Node> {
    match install::find_node(real_environment).await {
        Ok(node) => Some(node),
        Err(problem) => {
            let message = format!(
                "SKIPPED {test}: it runs bzz's real agent host, which needs Node.js {}.{} or newer ({problem})",
                install::MIN_NODE.0,
                install::MIN_NODE.1
            );
            assert!(
                std::env::var_os("BZZ_TEST_REQUIRE_NODE").is_none(),
                "{message}"
            );
            // Written to the terminal itself: the test harness hides what a
            // passing test prints, and a skip must be seen.
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "{message}");
            None
        }
    }
}

/// Writes the stand-in for `node` into `folder` and returns its path.
#[cfg(unix)]
pub fn scripted_node(folder: &Path, node: &Node) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(folder).unwrap();
    let model =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("agent-host/test/support/scripted-model.mjs");
    assert!(model.is_file(), "{} is missing", model.display());
    let path = folder.join("node");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nBZZ_AGENT_HOST_SCRIPTED_MODEL='{}'\nexport BZZ_AGENT_HOST_SCRIPTED_MODEL\nexec '{}' \"$@\"\n",
            model.display(),
            node.program.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Where npm keeps what it downloaded, shared by every test run so that
/// only the first one needs the registry. Never the owner's own cache.
pub fn npm_cache() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("npm-cache")
}

/// What a test adds to the environment of the bzz it starts, or reads its
/// own through: `BZZ_NODE` is the stand-in, Pi's folder is an empty one of
/// the test's, and npm's cache is the shared one.
pub fn variables(node: &Path, pi_dir: &Path) -> Vec<(String, String)> {
    vec![
        ("BZZ_NODE".into(), node.display().to_string()),
        ("PI_CODING_AGENT_DIR".into(), pi_dir.display().to_string()),
        ("npm_config_cache".into(), npm_cache().display().to_string()),
    ]
}

/// bzz's environment as an in-process test gives it.
pub fn environment(
    node: &Path,
    pi_dir: &Path,
) -> impl Fn(&str) -> Option<String> + Clone + Send + Sync + use<> {
    let variables = variables(node, pi_dir);
    move |name: &str| {
        variables
            .iter()
            .find(|(variable, _)| variable == name)
            .map(|(_, value)| value.clone())
            .or_else(|| real_environment(name))
    }
}

/// A private git remote for a test: the host's own test server
/// (`agent-host/test/support/git-server.mjs`), which serves a bare
/// repository through the real `git http-backend` on this machine and
/// answers 401 without exactly `username` and `token`. It ends when this is
/// dropped.
pub struct GitRemote {
    _server: std::process::Child,
    /// The URL to clone.
    pub origin: String,
    /// As an allow-list names it: `127.0.0.1:<port>`.
    pub host: String,
}

/// Why the git tests cannot run on this machine, or `None`.
pub fn git_missing() -> Option<String> {
    let exec = std::process::Command::new("git")
        .arg("--exec-path")
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let folder = PathBuf::from(String::from_utf8_lossy(&exec.stdout).trim());
    (!folder.join("git-http-backend").is_file()).then(|| "this git has no http-backend".to_owned())
}

/// Starts the private remote under `root`, or says why it cannot and
/// returns `None` (a failure under `BZZ_TEST_REQUIRE_NODE`).
pub fn git_remote(
    test: &str,
    node: &Node,
    root: &Path,
    username: &str,
    token: &str,
) -> Option<GitRemote> {
    use std::io::{BufRead as _, Write as _};
    let missing = match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => git_missing(),
        _ => Some("git is not installed".to_owned()),
    };
    if let Some(missing) = missing {
        let message = format!(
            "SKIPPED {test}: it needs a real git remote that asks for credentials ({missing})"
        );
        assert!(
            std::env::var_os("BZZ_TEST_REQUIRE_NODE").is_none(),
            "{message}"
        );
        let _ = writeln!(std::io::stderr(), "{message}");
        return None;
    }
    std::fs::create_dir_all(root).unwrap();
    let support =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("agent-host/test/support/git-server.mjs");
    let launcher = root.join("serve.mjs");
    // The server is written for the host's own tests, which hand it the
    // test to clean up after; here it runs until its input ends.
    std::fs::write(
        &launcher,
        format!(
            "import {{ startGitServer }} from {support:?};\n\
             const [root, username, password] = process.argv.slice(2);\n\
             const server = await startGitServer({{ after() {{}} }}, root, {{ auth: {{ username, password }} }});\n\
             console.log(JSON.stringify({{ origin: server.origin, host: server.host }}));\n\
             process.stdin.resume();\n\
             process.stdin.on('end', () => process.exit(0));\n",
            support = support.display().to_string()
        ),
    )
    .unwrap();
    let mut server = std::process::Command::new(&node.program)
        .arg(&launcher)
        .arg(root.join("served"))
        .arg(username)
        .arg(token)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let said = serde_json::from_str::<serde_json::Value>(&line)
        .unwrap_or_else(|_| panic!("the git server did not start: {line:?}"));
    Some(GitRemote {
        origin: said["origin"].as_str().unwrap().to_owned(),
        host: said["host"].as_str().unwrap().to_owned(),
        _server: server,
    })
}

impl Drop for GitRemote {
    fn drop(&mut self) {
        let _ = self._server.kill();
        let _ = self._server.wait();
    }
}

/// Every form a user name and a token travel or could be kept in, as the
/// host's own tests search for them.
pub fn secret_forms(username: &str, token: &str) -> Vec<(String, String)> {
    use base64::Engine as _;
    let base64 = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
    vec![
        ("the token".to_owned(), token.to_owned()),
        (
            "the Basic header".to_owned(),
            base64(format!("{username}:{token}")),
        ),
        (
            "the token alone, as Basic".to_owned(),
            base64(format!(":{token}")),
        ),
        ("the token in base64".to_owned(), base64(token.to_owned())),
    ]
}

/// Every file under `root`, by path.
pub fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                folders.push(path);
            } else if path.is_file() {
                found.push(path);
            }
        }
    }
    found
}

/// Whether `needle` is anywhere in the file's bytes.
pub fn file_holds(path: &Path, needle: &str) -> bool {
    std::fs::read(path).is_ok_and(|bytes| {
        !needle.is_empty()
            && bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
    })
}
