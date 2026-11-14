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
            eprintln!("{message}");
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
