//! The launch kit: a folder from which a machine without bzz runs an agent
//! under Buzz's own `buzz-acp` (plan v0.15.0, M5; ADR v0.15 local agents,
//! decision 12).
//!
//! ```text
//! <kit>/
//!   README.md            the steps on the other machine
//!   buzz-acp.env   0600  THE AGENT'S KEY, the owner's authorization, the
//!                        relay, and how buzz-acp starts the host
//!   start.sh             loads that file and runs buzz-acp from this folder
//!   bzz-agent.service    a service unit, as an example
//!   bzz-agent.json 0600  the agent's settings, for `bzz agent receive`
//!   agent/         0700  memory.sqlite, files.sqlite, agent.json
//!   host/                bzz's Pi Durable host and its lock file
//! ```
//!
//! This is the one place where an agent's key leaves bzz in the clear, by
//! the owner's explicit act: under `buzz-acp` the key sits beside the agent,
//! which is Buzz's rule for a remote agent (decision D8). The key is in
//! `buzz-acp.env` and in no other file of the kit.
//!
//! Every path in the kit is relative to the kit, so it runs wherever it is
//! copied to: `buzz-acp` is started in the kit's folder and starts
//! `node host/main.mjs agent` there.

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::{
    agents::{
        moving::Carried,
        runtime::{install, process, settle},
    },
    config::WakeRule,
    error::{Error, Result},
};

pub const AGENT_FOLDER: &str = "agent";
pub const HOST_FOLDER: &str = "host";
pub const ENV_FILE: &str = "buzz-acp.env";
pub const SETTINGS_FILE: &str = "bzz-agent.json";
/// The revision of `block/buzz` this bzz is built and tested against: the
/// `buzz-acp` and `buzz` a kit is run with (`Cargo.toml` pins the same).
pub const BUZZ_PIN: &str = "bf3bdd0eaefe39289bf9620594f254c7f68897f5";
const KEY_VARIABLE: &str = "BUZZ_PRIVATE_KEY";

/// One line of an environment file, readable alike by a shell and by
/// systemd: the value in single quotes, which nothing inside may end.
fn assignment(name: &str, value: &str) -> Result<String> {
    if value.contains(['\'', '\n', '\r', '\0']) {
        return Err(Error::Config(format!(
            "{name} holds a character that cannot be written to an environment file"
        )));
    }
    Ok(format!("{name}='{value}'\n"))
}

/// The environment `buzz-acp` needs for this agent. The first value is the
/// agent's key.
pub fn environment_file(carried: &Carried) -> Result<Zeroizing<String>> {
    let mut text = Zeroizing::new(String::from(
        "# The environment of Buzz's buzz-acp for one agent, written by `bzz agent launch-kit`.\n\
         # THE NEXT LINE IS THE AGENT'S KEY, IN THE CLEAR. Whoever reads this file can act\n\
         # as the agent. Keep it readable by its user only (mode 600), and delete it when\n\
         # the agent leaves this machine.\n",
    ));
    text.push_str(&assignment(KEY_VARIABLE, &carried.key)?);
    text.push_str("# The owner's authorization of the agent (NIP-OA).\n");
    text.push_str(&assignment("BUZZ_AUTH_TAG", &carried.auth_tag)?);
    text.push_str(&assignment(
        "BUZZ_RELAY_URL",
        carried.relay.trim_end_matches('/'),
    )?);
    text.push_str(
        "# How buzz-acp starts the agent's host. Both paths are relative to this folder:\n\
         # start buzz-acp here (start.sh does, and the service unit's WorkingDirectory).\n",
    );
    text.push_str(&assignment("BUZZ_ACP_AGENT_COMMAND", "node")?);
    text.push_str(&assignment(
        "BUZZ_ACP_AGENT_ARGS",
        &format!("{HOST_FOLDER}/main.mjs,{AGENT_FOLDER}"),
    )?);
    text.push_str("# One conversation per thread, and one turn per message, as under bzz.\n");
    text.push_str(&assignment("BUZZ_ACP_SESSION_POLICY", "thread")?);
    text.push_str(&assignment("BUZZ_ACP_MULTIPLE_EVENT_HANDLING", "queue")?);
    text.push_str(
        "# Buzz's own orientation for agents and its relay-side memory are off: they tell\n\
         # an agent to work with the `buzz` command in a shell, and this agent has its own\n\
         # tools, its own disk and its own memory.\n",
    );
    text.push_str(&assignment("BUZZ_ACP_NO_BASE_PROMPT", "true")?);
    text.push_str(&assignment("BUZZ_ACP_NO_MEMORY", "true")?);
    text.push_str("# Who may wake it, as you set it in bzz.\n");
    text.push_str(&assignment(
        "BUZZ_ACP_RESPOND_TO",
        crate::agents::manage::respond_to(carried.wake),
    )?);
    if carried.wake == WakeRule::Allowlist {
        text.push_str(&assignment(
            "BUZZ_ACP_RESPOND_TO_ALLOWLIST",
            &carried.wake_allowlist.join(","),
        )?);
    }
    Ok(text)
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn start_script() -> &'static str {
    "#!/bin/sh\n\
     # Runs the agent under Buzz's buzz-acp, from this folder, until it is stopped.\n\
     # `node`, `buzz-acp` and `buzz` must be on the PATH.\n\
     set -eu\n\
     cd \"$(dirname \"$0\")\"\n\
     set -a\n\
     . ./buzz-acp.env\n\
     set +a\n\
     exec buzz-acp \"$@\"\n"
}

fn service_unit(carried: &Carried, kit_name: &str) -> String {
    // `%` starts a specifier in a unit file.
    let name = one_line(&carried.name).replace('%', "%%");
    let kit = kit_name.replace('%', "%%");
    format!(
        "# An example of a user service for this agent (systemd). Change the three paths\n\
         # to where this folder, buzz-acp, node and buzz are on this machine, then:\n\
         #   cp bzz-agent.service ~/.config/systemd/user/\n\
         #   systemctl --user daemon-reload && systemctl --user enable --now bzz-agent\n\
         [Unit]\n\
         Description=bzz agent {name}, under Buzz's buzz-acp\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         WorkingDirectory=%h/{kit}\n\
         EnvironmentFile=%h/{kit}/{ENV_FILE}\n\
         # node and buzz are found on this PATH.\n\
         Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin\n\
         ExecStart=%h/.local/bin/buzz-acp\n\
         # The owner's `!shutdown` ends buzz-acp with code 0, and a stop that was\n\
         # meant stays a stop: only a failure starts it again.\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn readme(carried: &Carried) -> String {
    let name = one_line(&carried.name);
    let model = carried.model.as_ref().map_or_else(
        || "Pi's own default model on that machine".to_owned(),
        |model| format!("`{}/{}`", model.provider, model.model_id),
    );
    let hosts = if carried.network.is_empty() {
        "no host".to_owned()
    } else {
        carried.network.join(", ")
    };
    let command_name = if name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        name.clone()
    } else {
        format!("\"{}\"", name.replace('"', "\\\""))
    };
    // What the owner wrote goes in last, so that nothing in it is read as
    // a place to fill.
    include_str!("launch_kit_readme.md")
        .replace("@MADE_BY@", &carried.made_by)
        .replace("@ENV_FILE@", ENV_FILE)
        .replace("@HOST_FOLDER@", HOST_FOLDER)
        .replace("@AGENT_FOLDER@", AGENT_FOLDER)
        .replace("@BUZZ_PIN@", BUZZ_PIN)
        .replace(
            "@NODE@",
            &format!("{}.{}", install::MIN_NODE.0, install::MIN_NODE.1),
        )
        .replace("@PACKAGES@", if carried.packages { "on" } else { "off" })
        .replace("@WAKE@", crate::agents::manage::respond_to(carried.wake))
        .replace("@HOSTS@", &hosts)
        .replace("@MODEL@", &one_line(&model))
        .replace("@COMMAND_NAME@", &command_name)
        .replace("@NAME@", &name)
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(path).map_err(|error| Error::io(path, error))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|error| Error::io(path, error))?;
    file.sync_all().map_err(|error| Error::io(path, error))
}

fn pretty(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Writes the kit into the new folder `to`, from the agent's folder with
/// its two whole files. Returns the one file that holds the key. The caller
/// removes `to` when this fails.
pub fn write(to: &Path, carried: &Carried, agent_folder: &Path) -> Result<PathBuf> {
    std::fs::create_dir(to).map_err(|error| Error::io(to, error))?;
    crate::paths::set_private_permissions(to)?;
    let kit_name = to.file_name().map_or_else(
        || "launch-kit".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    // The host, as this bzz carries it.
    let host = to.join(HOST_FOLDER);
    for (path, content) in install::HOST_FILES {
        let file = host.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
        }
        write_new(&file, content.as_bytes(), 0o644)?;
    }
    // The agent: its two files and what the host reads beside them.
    let agent = to.join(AGENT_FOLDER);
    process::create_private_dir(&agent)?;
    for name in settle::FILES {
        let (from, into) = (agent_folder.join(name), agent.join(name));
        let mut source = std::fs::File::open(&from).map_err(|error| Error::io(&from, error))?;
        write_new(&into, b"", 0o600)?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .open(&into)
            .map_err(|error| Error::io(&into, error))?;
        std::io::copy(&mut source, &mut target).map_err(|error| Error::io(&into, error))?;
        target.sync_all().map_err(|error| Error::io(&into, error))?;
    }
    write_new(
        &agent.join("agent.json"),
        &pretty(&carried.agent_json())?,
        0o600,
    )?;
    write_new(
        &to.join(SETTINGS_FILE),
        &pretty(&carried.without_key())?,
        0o600,
    )?;
    write_new(&to.join("README.md"), readme(carried).as_bytes(), 0o644)?;
    write_new(&to.join("start.sh"), start_script().as_bytes(), 0o755)?;
    write_new(
        &to.join("bzz-agent.service"),
        service_unit(carried, &kit_name).as_bytes(),
        0o644,
    )?;
    // The key last, into its one file.
    let key_file = to.join(ENV_FILE);
    write_new(&key_file, environment_file(carried)?.as_bytes(), 0o600)?;
    Ok(key_file)
}

/// The value of `name` in an environment file as [`environment_file`]
/// writes one.
fn value_of(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix(name)?.strip_prefix('=')?.trim();
        Some(
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
                .or_else(|| {
                    value
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                })
                .unwrap_or(value)
                .to_owned(),
        )
    })
}

/// Reads a kit back: the agent's settings and its key.
pub fn read(folder: &Path) -> Result<(Carried, Zeroizing<String>)> {
    let not_a_kit = |what: &str| {
        Error::Config(format!(
            "{} is not a launch kit made by `bzz agent launch-kit` ({what}). Nothing was received",
            folder.display()
        ))
    };
    let settings = std::fs::read(folder.join(SETTINGS_FILE))
        .map_err(|_| not_a_kit(&format!("it has no {SETTINGS_FILE}")))?;
    let carried: Carried = serde_json::from_slice(&settings)
        .map_err(|_| not_a_kit(&format!("its {SETTINGS_FILE} cannot be read")))?;
    let environment = Zeroizing::new(
        std::fs::read_to_string(folder.join(ENV_FILE))
            .map_err(|_| not_a_kit(&format!("it has no {ENV_FILE}")))?,
    );
    let key = Zeroizing::new(
        value_of(&environment, KEY_VARIABLE)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| not_a_kit(&format!("its {ENV_FILE} holds no key")))?,
    );
    Ok((carried, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pin is written in `Cargo.toml` too: the kit must name the Buzz
    /// this bzz is built against.
    #[test]
    fn the_kit_names_the_buzz_revision_bzz_is_built_against() {
        let manifest = include_str!("../../Cargo.toml");
        assert!(
            manifest.contains(&format!("rev = \"{BUZZ_PIN}\"")),
            "Cargo.toml pins another Buzz revision than {BUZZ_PIN}"
        );
    }

    #[test]
    fn a_value_is_written_so_that_a_shell_and_systemd_read_the_same() {
        assert_eq!(assignment("A", "x y").unwrap(), "A='x y'\n");
        assert_eq!(
            assignment("TAG", r#"["auth","ab","","cd"]"#).unwrap(),
            "TAG='[\"auth\",\"ab\",\"\",\"cd\"]'\n"
        );
        for bad in ["it's", "two\nlines", "nul\0"] {
            assert!(assignment("A", bad).is_err(), "{bad:?}");
        }
        let text = "# a comment\nOTHER='1'\nBUZZ_PRIVATE_KEY='abc'\nBUZZ_PRIVATE_KEY_NOT='zzz'\n";
        assert_eq!(value_of(text, "BUZZ_PRIVATE_KEY").as_deref(), Some("abc"));
        assert_eq!(value_of(text, "MISSING"), None);
        assert_eq!(value_of("A=plain\n", "A").as_deref(), Some("plain"));
    }
}
