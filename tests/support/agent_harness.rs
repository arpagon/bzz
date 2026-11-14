//! Shared by the tests of the local agent runtime: where the scripted
//! harness is (`examples/scripted_harness.rs`), how its script is written and
//! its log read, and a recorded relay that stands in for the agent's own
//! connection.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use bzz::agents::runtime::{
    admission::ChannelFacts,
    relay::{AgentRelay, Published},
};
use nostr::{Event, EventBuilder, Keys, Tag};
use serde_json::{Value, json};
use uuid::Uuid;

/// The scripted harness, built by `cargo test` with the other examples.
pub fn program() -> PathBuf {
    let path = Path::new(env!("CARGO_BIN_EXE_bzz"))
        .parent()
        .expect("the bzz binary has a folder")
        .join("examples")
        .join(format!("scripted_harness{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.exists(),
        "{} is missing: run `cargo build --example scripted_harness` (a plain `cargo test` builds it)",
        path.display()
    );
    // `cargo test --test <name>` does not rebuild examples: a harness older
    // than its source would test yesterday's script language.
    let modified = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|file| file.modified())
            .ok()
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scripted_harness.rs");
    assert!(
        modified(&source) <= modified(&path),
        "{} is older than its source: run `cargo build --example scripted_harness`",
        path.display()
    );
    path
}

/// A script and the files the harness writes beside it.
pub struct Script {
    pub directory: PathBuf,
}

impl Script {
    pub fn new(directory: &Path) -> Self {
        std::fs::create_dir_all(directory).unwrap();
        Self {
            directory: directory.to_path_buf(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join("script.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.directory.join("harness-log.jsonl")
    }

    pub fn state_path(&self) -> PathBuf {
        self.directory.join("harness-state.json")
    }

    /// A file a step can `touch` or `wait_for`.
    pub fn marker(&self, name: &str) -> PathBuf {
        self.directory.join(format!("marker-{name}"))
    }

    /// Writes the script: `settings` (`own_host`, `initialize`, …) and the
    /// turns. The harness reads it at each start.
    pub fn write(&self, mut settings: Value, turns: Value) {
        settings["log"] = json!(self.log_path());
        settings["state"] = json!(self.state_path());
        settings["turns"] = turns;
        std::fs::write(self.path(), settings.to_string()).unwrap();
    }

    /// The command that starts the harness on this script.
    pub fn command(&self) -> Vec<String> {
        vec![
            program().display().to_string(),
            self.path().display().to_string(),
        ]
    }

    /// Everything the harness logged, oldest first.
    pub fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The parameters of every `method` the harness received.
    pub fn received(&self, method: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|entry| entry["got"] == method)
            .map(|entry| entry["params"].clone())
            .collect()
    }

    /// The result of every call of the tool `name`.
    pub fn tool_results(&self, name: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|entry| entry["tool"] == name)
            .map(|entry| entry["result"].clone())
            .collect()
    }

    /// Process ids of every start of the harness.
    pub fn starts(&self) -> Vec<u32> {
        self.log()
            .into_iter()
            .filter_map(|entry| entry["start"]["pid"].as_u64())
            .map(|pid| pid as u32)
            .collect()
    }
}

/// Polls `check` until it yields, or panics naming `what` after `limit`.
pub async fn until<T>(what: &str, limit: Duration, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Whether a process exists and is not a zombie.
#[cfg(unix)]
pub fn running(pid: u32) -> bool {
    bzz::agents::runtime::process::identity_of(pid).is_some()
}

/// SIGKILL to one process, as `kill -9` does it.
#[cfg(unix)]
pub fn kill(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill {pid}");
}

/// How the recorded relay treats the next publishes of messages.
#[derive(Clone, Debug, Default)]
pub enum Mode {
    #[default]
    Accept,
    Refuse(String),
    /// No answer, and the relay does not have the event.
    Lost,
    /// No answer, but the relay has the event.
    Silent,
    /// Waits for `release`, then accepts.
    Held,
}

/// The agent's side of a relay, recorded. It signs with a real agent key and
/// a real owner tag, keeps what was published, and answers queries from it.
pub struct RecordedRelay {
    pub agent: Keys,
    auth_tag: Tag,
    /// What the relay holds: seeded events and accepted publishes, once each.
    stored: Mutex<Vec<Event>>,
    /// Every publish call, in order, typing indicators included.
    calls: Mutex<Vec<Event>>,
    channels: Mutex<HashMap<Uuid, ChannelFacts>>,
    mode: Mutex<Mode>,
    release: tokio::sync::Notify,
}

impl RecordedRelay {
    pub fn new(agent: Keys, auth_tag: Tag) -> Arc<Self> {
        Arc::new(Self {
            agent,
            auth_tag,
            stored: Mutex::default(),
            calls: Mutex::default(),
            channels: Mutex::default(),
            mode: Mutex::default(),
            release: tokio::sync::Notify::new(),
        })
    }

    pub fn set_mode(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    /// Lets every held publish through.
    pub fn release(&self) {
        self.release.notify_waiters();
    }

    pub fn seed(&self, event: &Event) {
        let mut stored = self.stored.lock().unwrap();
        if !stored.iter().any(|existing| existing.id == event.id) {
            stored.push(event.clone());
        }
    }

    pub fn set_channel(&self, facts: ChannelFacts) {
        self.channels.lock().unwrap().insert(facts.id, facts);
    }

    /// What the relay holds of `kind`, written by the agent.
    pub fn by_agent(&self, kind: u16) -> Vec<Event> {
        self.stored
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.kind.as_u16() == kind && event.pubkey == self.agent.public_key())
            .cloned()
            .collect()
    }

    /// The agent's messages the relay holds that answer `request`.
    pub fn replies_to(&self, request: &Event) -> Vec<Event> {
        let id = request.id.to_hex();
        self.by_agent(9)
            .into_iter()
            .filter(|event| {
                event.tags.iter().any(|tag| {
                    let values = tag.as_slice();
                    values.first().map(String::as_str) == Some("e")
                        && values.get(1) == Some(&id)
                        && values.get(3).map(String::as_str) == Some("reply")
                })
            })
            .collect()
    }

    /// Every publish call of `kind`, repeats included.
    pub fn calls(&self, kind: u16) -> Vec<Event> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.kind.as_u16() == kind)
            .cloned()
            .collect()
    }
}

fn tag_matches(event: &Event, name: &str, wanted: &Value) -> bool {
    let Some(wanted) = wanted.as_array() else {
        return true;
    };
    event.tags.iter().any(|tag| {
        let values = tag.as_slice();
        values.first().map(String::as_str) == Some(name)
            && values
                .get(1)
                .is_some_and(|value| wanted.iter().any(|wanted| wanted == value))
    })
}

fn filter_matches(event: &Event, filter: &Value) -> bool {
    let listed = |name: &str, value: String| {
        filter
            .get(name)
            .and_then(Value::as_array)
            .is_none_or(|all| all.iter().any(|one| one.as_str() == Some(value.as_str())))
    };
    listed("ids", event.id.to_hex())
        && listed("authors", event.pubkey.to_hex())
        && filter
            .get("kinds")
            .and_then(Value::as_array)
            .is_none_or(|kinds| kinds.contains(&json!(event.kind.as_u16())))
        && ["h", "e", "p", "d"].iter().all(|name| {
            filter
                .get(format!("#{name}"))
                .is_none_or(|wanted| tag_matches(event, name, wanted))
        })
        && filter
            .get("until")
            .and_then(Value::as_u64)
            .is_none_or(|until| event.created_at.as_secs() <= until)
}

#[async_trait]
impl AgentRelay for RecordedRelay {
    async fn sign(&self, builder: EventBuilder) -> bzz::Result<Event> {
        builder
            .tag(self.auth_tag.clone())
            .sign_with_keys(&self.agent)
            .map_err(|error| bzz::Error::Auth(error.to_string()))
    }

    async fn publish(&self, event: Event) -> Published {
        self.calls.lock().unwrap().push(event.clone());
        // A typing indicator is ephemeral: no mode applies to it.
        let mode = if event.kind.as_u16() == 20_002 {
            Mode::Accept
        } else {
            self.mode.lock().unwrap().clone()
        };
        match mode {
            Mode::Accept => {
                self.seed(&event);
                Published::Accepted
            }
            Mode::Refuse(why) => Published::Refused(why),
            Mode::Lost => Published::Unknown,
            Mode::Silent => {
                self.seed(&event);
                Published::Unknown
            }
            Mode::Held => {
                self.release.notified().await;
                self.seed(&event);
                Published::Accepted
            }
        }
    }

    async fn query(&self, filters: Vec<Value>) -> bzz::Result<Vec<Event>> {
        Ok(self
            .stored
            .lock()
            .unwrap()
            .iter()
            .filter(|event| filters.iter().any(|filter| filter_matches(event, filter)))
            .cloned()
            .collect())
    }

    async fn channel(&self, channel: Uuid) -> bzz::Result<ChannelFacts> {
        self.channels
            .lock()
            .unwrap()
            .get(&channel)
            .cloned()
            .ok_or_else(|| bzz::Error::Access("no such channel".into()))
    }
}
