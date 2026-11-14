//! v0.15 M4: creating an agent is all or nothing (`bzz::agents::manage`).
//! A recorded relay stands in for the real one and fails at each step in
//! turn; whatever fails, no key, no folder, no settings and nothing on the
//! relay is left. Nothing here needs Node, a model or a network.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use bzz::{
    agents::{
        manage::{self, NewAgent, OwnerContext, Publisher},
        runtime::{process, relay::Published},
    },
    auth::signer::SignerHandle,
    broker::custody,
    config::{
        Config, FolderGrant, Harness, HostedAgentConfig, IdentityConfig, KeyBackend,
        LocalAgentState, WakeRule,
    },
    paths::Paths,
};
use nostr::{Event, Keys};
use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

/// How the recorded relay treats one publish.
#[derive(Clone, Debug)]
enum Answer {
    Accept,
    Refuse,
    /// No answer, and the relay does not have the event.
    Lost,
    /// No answer, but the relay has the event: the worst case for an undo.
    Silent,
}

/// The owner's and the agent's side of a relay, recorded. It keeps what was
/// published and honours a removal (kind 5) as a relay does.
#[derive(Default)]
struct RecordedRelay {
    held: Mutex<Vec<Event>>,
    /// Every publish, in order: who published, and what.
    calls: Mutex<Vec<(&'static str, Event)>>,
    /// The answers to the next publishes; when empty, every one is accepted.
    answers: Mutex<VecDeque<Answer>>,
}

impl RecordedRelay {
    fn answering(answers: &[Answer]) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.iter().cloned().collect()),
            ..Self::default()
        })
    }

    fn tag<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
        event.tags.iter().find_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some(name))
                .then(|| values.get(1).map(String::as_str))
                .flatten()
        })
    }

    fn land(&self, event: &Event) {
        let mut held = self.held.lock().unwrap();
        if event.kind.as_u16() != 5 {
            held.push(event.clone());
            return;
        }
        let author = event.pubkey;
        if let Some(id) = Self::tag(event, "e") {
            held.retain(|other| !(other.pubkey == author && other.id.to_hex() == id));
        }
        if let Some(coordinate) = Self::tag(event, "a") {
            held.retain(|other| {
                let own = format!(
                    "{}:{}:{}",
                    other.kind.as_u16(),
                    other.pubkey.to_hex(),
                    Self::tag(other, "d").unwrap_or_default()
                );
                !(other.pubkey == author && own == coordinate)
            });
        }
    }

    fn publish(&self, who: &'static str, event: Event) -> Published {
        self.calls.lock().unwrap().push((who, event.clone()));
        match self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Answer::Accept)
        {
            Answer::Accept => {
                self.land(&event);
                Published::Accepted
            }
            Answer::Refuse => Published::Refused("blocked: not today".into()),
            Answer::Lost => Published::Unknown,
            Answer::Silent => {
                self.land(&event);
                Published::Unknown
            }
        }
    }

    fn held(&self) -> Vec<Event> {
        self.held.lock().unwrap().clone()
    }

    /// `(who, kind)` of every publish.
    fn calls(&self) -> Vec<(&'static str, u16)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(who, event)| (*who, event.kind.as_u16()))
            .collect()
    }
}

#[async_trait]
impl Publisher for RecordedRelay {
    async fn as_owner(&self, event: Event) -> Published {
        self.publish("owner", event)
    }

    async fn as_agent(
        &self,
        agent: &HostedAgentConfig,
        signer: &SignerHandle,
        event: Event,
    ) -> Published {
        // The agent publishes with its own key, never the owner's.
        assert_eq!(signer.public_key().to_hex(), agent.pubkey);
        assert_eq!(event.pubkey.to_hex(), agent.pubkey);
        self.publish("agent", event)
    }
}

struct World {
    _temporary: TempDir,
    paths: Paths,
    owner_keys: Keys,
    owner: SignerHandle,
    identity: Uuid,
    community: Uuid,
}

impl World {
    /// The world, and the owner's settings as bzz holds them in memory.
    fn new() -> (Self, Config) {
        let temporary = TempDir::new().unwrap();
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        let owner_keys = Keys::generate();
        let identity = Uuid::new_v4();
        let mut config = Config::default();
        config.identities.push(IdentityConfig {
            id: identity,
            label: "owner".into(),
            pubkey: owner_keys.public_key().to_hex(),
            backend: KeyBackend::EncryptedFile,
            key_ref: "owner-key".into(),
        });
        let community = config
            .add_community("test".into(), "ws://localhost:3030".into(), identity, true)
            .unwrap();
        config.save(&paths).unwrap();
        (
            Self {
                _temporary: temporary,
                paths,
                owner: SignerHandle::spawn(owner_keys.clone()),
                owner_keys,
                identity,
                community,
            },
            config,
        )
    }

    fn context(&self) -> OwnerContext<'_> {
        OwnerContext {
            paths: &self.paths,
            owner: &self.owner,
            owner_identity: self.identity,
            community: self.community,
            federated: None,
        }
    }

    /// What a creation leaves on this machine: sealed keys, agent folders,
    /// and agents in the settings as saved.
    fn on_this_machine(&self) -> (usize, usize, usize) {
        let count = |folder: std::path::PathBuf, wanted: &dyn Fn(&str) -> bool| {
            std::fs::read_dir(folder)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| wanted(&entry.file_name().to_string_lossy()))
                        .count()
                })
                .unwrap_or(0)
        };
        (
            count(self.paths.keys_dir(), &|name| name.starts_with("agent-")),
            count(self.paths.data_dir.join("agents"), &|name| {
                Uuid::parse_str(name).is_ok()
            }),
            Config::load(&self.paths).unwrap().hosted_agents.len(),
        )
    }
}

fn content(event: &Event) -> Value {
    serde_json::from_str(&event.content).unwrap()
}

#[tokio::test]
async fn a_created_agent_has_its_key_folder_settings_profile_and_owners_record() {
    let (world, mut config) = World::new();
    let relay = RecordedRelay::answering(&[]);
    let granted = world.paths.data_dir.clone();
    std::fs::create_dir_all(&granted).unwrap();
    let friend = Keys::generate().public_key().to_hex();
    let mut new = NewAgent::named(" Scout ");
    new.instructions = "Be brief.".into();
    new.wake = WakeRule::Allowlist;
    new.wake_allowlist = vec![friend.clone()];
    new.folders = vec![FolderGrant {
        name: "site".into(),
        path: granted.clone(),
        write: false,
        exec: bzz::config::ExecGrant::No,
    }];
    new.network = vec!["github.com".into()];
    new.picture = Some("https://example.org/scout.png".into());
    let context = world.context();
    let created = manage::create(&context, &mut config, &new, relay.as_ref())
        .await
        .unwrap();
    let agent = &created.agent;
    assert_eq!(agent.label, "Scout");
    assert_eq!(
        (agent.community, agent.owner_identity),
        (world.community, world.identity)
    );
    // The agent's profile first, then the owner's record of it.
    assert_eq!(relay.calls(), [("agent", 0), ("owner", 30_177)]);
    let held = relay.held();
    let profile = &held[0];
    assert_eq!(profile.pubkey.to_hex(), agent.pubkey);
    assert_eq!(
        content(profile),
        json!({"name": "Scout", "bot": true, "picture": "https://example.org/scout.png"})
    );
    let record = &held[1];
    assert_eq!(record.pubkey, world.owner_keys.public_key());
    assert_eq!(
        record
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<_>>(),
        [
            vec!["d".to_owned(), agent.pubkey.clone()],
            vec!["runtime".to_owned(), "bzz/pi-durable".to_owned()],
        ]
    );
    assert_eq!(
        content(record),
        json!({
            "parallelism": 1, "respond_to": "allowlist", "respond_to_allowlist": [friend],
            "name": "Scout", "picture": "https://example.org/scout.png",
        })
    );
    // bzz's own reader (v0.11) takes the two as a verified agent of this
    // owner: the profile carries the owner's authorization.
    let verified = bzz::agents::protocol::verify_public_agent(profile, None, Some(record)).unwrap();
    assert_eq!(
        verified.owner_pubkey,
        world.owner_keys.public_key().to_hex()
    );
    assert_eq!(verified.name, "Scout");

    // On this machine: the sealed key, the folder, and the settings, saved.
    assert_eq!(world.on_this_machine(), (1, 1, 1));
    assert!(custody::has_key(&world.paths.keys_dir(), agent));
    assert!(process::agent_dir(&world.paths.data_dir, agent.id).is_dir());
    let saved = Config::load(&world.paths).unwrap();
    assert_eq!(saved, config);
    let entry = &saved.hosted_agents[0];
    assert_eq!(entry.pubkey, agent.pubkey);
    assert!(
        entry.allowed.is_empty(),
        "bzz runs it: the v0.13 broker serves it nothing"
    );
    let local = entry.local.as_ref().unwrap();
    assert_eq!(
        (local.harness, local.wake, local.state),
        (
            Harness::PiDurable,
            WakeRule::Allowlist,
            LocalAgentState::Active
        )
    );
    assert_eq!(local.instructions, "Be brief.");
    assert_eq!(local.folders[0].path, granted);
    assert_eq!(local.network, ["github.com"]);
    assert!(entry.runs_locally());
    // The key opens only with its owner, and is the profile's author.
    let signer = custody::unlock(&world.paths.keys_dir(), &world.owner, entry)
        .await
        .unwrap();
    assert_eq!(signer.public_key().to_hex(), agent.pubkey);
    // Neither the settings nor anything published holds a key.
    let settings = std::fs::read_to_string(world.paths.config_file()).unwrap();
    let secret = world.owner_keys.secret_key().to_secret_hex();
    assert!(!settings.contains(&secret));

    // A second agent of the same name is refused before anything is made.
    let again = manage::create(&context, &mut config, &new, relay.as_ref())
        .await
        .unwrap_err();
    assert!(again.to_string().contains("exists"), "{again}");
    assert_eq!(world.on_this_machine(), (1, 1, 1));
    assert_eq!(relay.calls().len(), 2, "nothing more was published");
}

/// One way a creation fails: what it is, the relay's answers to the profile
/// and to the owner's record, what the error says, and every publish made,
/// the undo's included.
type Case = (
    &'static str,
    &'static [Answer],
    &'static str,
    &'static [(&'static str, u16)],
);

/// Whatever step fails, nothing of the attempt is left: no key, no folder,
/// no settings, and nothing on the relay that names a key bzz no longer
/// holds.
#[tokio::test]
async fn a_creation_that_fails_at_any_step_leaves_nothing() {
    use Answer::{Accept, Lost, Refuse, Silent};
    let cases: [Case; 6] = [
        (
            "the profile is refused",
            &[Refuse],
            "refused the agent's profile",
            &[("agent", 0)],
        ),
        (
            "the profile is lost",
            &[Lost],
            "did not confirm the agent's profile",
            &[("agent", 0), ("agent", 5)],
        ),
        (
            "the profile lands and is not confirmed",
            &[Silent],
            "did not confirm the agent's profile",
            &[("agent", 0), ("agent", 5)],
        ),
        (
            "the owner's record is refused",
            &[Accept, Refuse],
            "refused your record of the agent",
            &[("agent", 0), ("owner", 30_177), ("agent", 5)],
        ),
        (
            "the owner's record is lost",
            &[Accept, Lost],
            "did not confirm your record of the agent",
            &[("agent", 0), ("owner", 30_177), ("owner", 5), ("agent", 5)],
        ),
        (
            "the owner's record lands and is not confirmed",
            &[Accept, Silent],
            "did not confirm your record of the agent",
            &[("agent", 0), ("owner", 30_177), ("owner", 5), ("agent", 5)],
        ),
    ];
    for (what, answers, error, calls) in cases {
        let (world, mut config) = World::new();
        let relay = RecordedRelay::answering(answers);
        let context = world.context();
        let failure = manage::create(
            &context,
            &mut config,
            &NewAgent::named("Scout"),
            relay.as_ref(),
        )
        .await
        .expect_err(what);
        let text = failure.to_string();
        assert!(text.contains(error), "{what}: {text}");
        assert!(failure.left.is_empty(), "{what}: {:?}", failure.left);
        assert!(text.contains("Nothing was left"), "{what}: {text}");
        assert_eq!(relay.calls(), calls, "{what}");
        assert_eq!(
            relay.held(),
            [],
            "{what}: the relay holds nothing of the agent"
        );
        assert_eq!(world.on_this_machine(), (0, 0, 0), "{what}");
        assert!(config.hosted_agents.is_empty(), "{what}");
        // The removal of the profile is the agent's own, with the owner's
        // authorization, made while bzz still held its key.
        if let Some((_, removal)) = relay
            .calls
            .lock()
            .unwrap()
            .iter()
            .find(|(who, event)| *who == "agent" && event.kind.as_u16() == 5)
        {
            assert!(RecordedRelay::tag(removal, "auth").is_some(), "{what}");
        }
    }
}

#[tokio::test]
async fn a_creation_that_fails_on_this_machine_publishes_nothing() {
    // The agent's folder cannot be made: a file is where `agents/` should be.
    let (world, mut config) = World::new();
    std::fs::create_dir_all(&world.paths.data_dir).unwrap();
    std::fs::write(world.paths.data_dir.join("agents"), "in the way").unwrap();
    let relay = RecordedRelay::answering(&[]);
    let context = world.context();
    let new = NewAgent::named("Scout");
    let failure = manage::create(&context, &mut config, &new, relay.as_ref())
        .await
        .unwrap_err();
    assert!(failure.left.is_empty(), "{failure}");
    assert_eq!(relay.calls(), []);
    assert_eq!(world.on_this_machine(), (0, 0, 0));

    // The settings cannot be saved: a folder is where their file is written.
    let (world, mut config) = World::new();
    std::fs::create_dir_all(world.paths.config_file().with_extension("toml.tmp")).unwrap();
    let context = world.context();
    let failure = manage::create(&context, &mut config, &new, relay.as_ref())
        .await
        .unwrap_err();
    assert_eq!(relay.calls(), [], "{failure}");
    assert!(failure.left.is_empty(), "{failure}");
    std::fs::remove_dir_all(world.paths.config_file().with_extension("toml.tmp")).unwrap();
    assert_eq!(world.on_this_machine(), (0, 0, 0), "{failure}");
    assert!(config.hosted_agents.is_empty());

    // Refused before anything is made: a harness of another task, a name
    // that is no name, and an allowlist with nobody in it.
    let (world, mut config) = World::new();
    let context = world.context();
    for harness in [Harness::Grok, Harness::Codex, Harness::ClaudeCode] {
        let mut coding = NewAgent::named("Scout");
        coding.harness = harness;
        let failure = manage::create(&context, &mut config, &coding, relay.as_ref())
            .await
            .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("not available in this version yet"),
            "{harness:?}: {failure}"
        );
    }
    let mut listed = NewAgent::named("Scout");
    listed.wake = WakeRule::Allowlist;
    for wrong in [NewAgent::named(""), NewAgent::named("two\nlines"), listed] {
        assert!(
            manage::create(&context, &mut config, &wrong, relay.as_ref())
                .await
                .is_err()
        );
    }
    assert_eq!(relay.calls(), []);
    assert_eq!(world.on_this_machine(), (0, 0, 0));
}

/// When the relay will not take back what it holds, the key, the folder and
/// the settings still go, and the failure names what could not be removed.
#[tokio::test]
async fn what_an_undo_cannot_take_back_is_named() {
    use Answer::{Accept, Refuse, Silent};
    let (world, mut config) = World::new();
    // Profile accepted, record lands unconfirmed, both removals refused.
    let relay = RecordedRelay::answering(&[Accept, Silent, Refuse, Refuse]);
    let context = world.context();
    let failure = manage::create(
        &context,
        &mut config,
        &NewAgent::named("Scout"),
        relay.as_ref(),
    )
    .await
    .unwrap_err();
    let held = relay.held();
    assert_eq!(held.len(), 2, "the relay kept both");
    let text = failure.to_string();
    assert_eq!(failure.left.len(), 2, "{text}");
    assert!(
        text.contains(&held[0].id.to_hex()) && text.contains("kind 30177"),
        "{text}"
    );
    assert!(!text.contains("Nothing was left"), "{text}");
    assert_eq!(world.on_this_machine(), (0, 0, 0));
}

/// The checks before anything exists: a harness that is not offered, and a
/// machine without Node, each with what to do.
#[tokio::test]
async fn the_checks_before_a_creation_say_what_to_do() {
    let temporary = TempDir::new().unwrap();
    let empty = temporary.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let path = empty.display().to_string();
    let environment: bzz::agents::runtime::agent::Environment =
        Arc::new(move |name| (name == "PATH").then(|| path.clone()));
    let quiet = |_: &str| {};
    let data = temporary.path().join("data");
    let missing = manage::preflight(&data, &NewAgent::named("Scout"), None, &environment, &quiet)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        missing.contains("Node.js 22.19") && missing.contains("BZZ_NODE"),
        "{missing}"
    );
    for harness in [Harness::Grok, Harness::Codex, Harness::ClaudeCode] {
        let mut coding = NewAgent::named("Scout");
        coding.harness = harness;
        let refused = manage::preflight(&data, &coding, None, &environment, &quiet)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("not available in this version yet"),
            "{harness:?}: {refused}"
        );
    }
    assert!(!data.exists(), "nothing was installed or made");
}
