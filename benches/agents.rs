//! What agents cost a profile (v0.15.0 release acceptance): the agent
//! directory in a community of 5,000 members, and the start of the agent
//! runtime for a profile whose agents are all paused, so that none runs.
//!
//! The part between `v0.15-only` marks uses what v0.14.0 does not have. With
//! that part cut out the file compiles in a v0.14.0 checkout, which is how
//! the directory numbers of the two releases were compared
//! (`docs/validation-v0.15.0.md`, "Benchmarks against v0.14.0").

use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    store::Store,
};
use criterion::{Criterion, criterion_group, criterion_main};
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::hint::black_box;
use uuid::Uuid;

const CHANNELS: usize = 50;
const MEMBERS_PER_CHANNEL: usize = 100;
/// Agents in the community: half with the `bot` role, half plain members
/// with their owner's proof on their profile (D13).
const AGENTS: usize = 50;

struct Directory {
    store: Store,
    community: Uuid,
    viewer: String,
    channel: Uuid,
    agent: String,
}

/// A community of `CHANNELS` channels of `MEMBERS_PER_CHANNEL` people each,
/// with `AGENTS` agents among them. `every_agent_a_bot`: each agent has the
/// `bot` role where it is a member, so v0.14.0 lists as many agents as
/// v0.15.0 does; otherwise half of them are plain members, which only
/// v0.15.0 lists.
fn directory(every_agent_a_bot: bool) -> Directory {
    let mut store = Store::open_memory().unwrap();
    let viewer = Keys::generate();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "bench".into(),
        pubkey: viewer.public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "bench".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "bench".into(),
            "wss://bench.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    store.sync_config(&config).unwrap();
    let relay = Keys::generate();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    let owner = Keys::generate();
    let agents = (0..AGENTS).map(|_| Keys::generate()).collect::<Vec<_>>();
    let mut first_channel = None;
    for index in 0..CHANNELS {
        let channel = Uuid::new_v4();
        first_channel.get_or_insert(channel);
        let metadata = EventBuilder::new(Kind::Custom(39_000), "")
            .tags([
                Tag::parse(["d", &channel.to_string()]).unwrap(),
                Tag::parse(["name", &format!("bench-{index}")]).unwrap(),
                Tag::parse(["t", "stream"]).unwrap(),
                Tag::parse(["public"]).unwrap(),
            ])
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &metadata).unwrap();
        let mut tags = vec![
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &identity.pubkey]).unwrap(),
            Tag::parse(["p", &owner.public_key().to_hex()]).unwrap(),
        ];
        for member in 0..MEMBERS_PER_CHANNEL {
            let person = format!("{:064x}", index * MEMBERS_PER_CHANNEL + member + 1);
            tags.push(Tag::parse(["p", &person]).unwrap());
        }
        // One agent in each channel: with the `bot` role in the even ones.
        let agent = agents[index % AGENTS].public_key().to_hex();
        tags.push(if every_agent_a_bot || index % 2 == 0 {
            Tag::parse(["p", &agent, "", "bot"]).unwrap()
        } else {
            Tag::parse(["p", &agent]).unwrap()
        });
        let roster = EventBuilder::new(Kind::Custom(39_002), "")
            .tags(tags)
            .sign_with_keys(&relay)
            .unwrap();
        store.apply_event(community, &roster).unwrap();
    }
    for agent in &agents {
        let auth = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap();
        let auth: Vec<String> = serde_json::from_str(&auth).unwrap();
        let profile = EventBuilder::new(Kind::Metadata, r#"{"name":"Worker","bot":true}"#)
            .tags([Tag::parse(auth).unwrap()])
            .sign_with_keys(agent)
            .unwrap();
        let policy = EventBuilder::new(
            Kind::Custom(30_177),
            r#"{"name":"Worker","parallelism":1,"respond_to":"anyone"}"#,
        )
        .tags([Tag::parse(["d", &agent.public_key().to_hex()]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
        store.apply_event(community, &profile).unwrap();
        store.apply_event(community, &policy).unwrap();
    }
    // Profiles of other people, for the size of the events table. The
    // members above have made-up keys and so no profile: a refresh asks for
    // theirs.
    for index in 0..CHANNELS * MEMBERS_PER_CHANNEL / 10 {
        let person = Keys::generate();
        let profile = EventBuilder::new(Kind::Metadata, format!(r#"{{"name":"P{index}"}}"#))
            .sign_with_keys(&person)
            .unwrap();
        store.apply_event(community, &profile).unwrap();
    }
    store.reconcile_remote_agents(community).unwrap();
    Directory {
        store,
        community,
        viewer: identity.pubkey,
        channel: first_channel.unwrap(),
        agent: agents[0].public_key().to_hex(),
    }
}

/// The mention that is timed is one that passes: the whole check runs.
fn checked(world: &Directory) {
    world
        .store
        .validate_agent_mentions(
            world.community,
            world.channel,
            &world.viewer,
            std::slice::from_ref(&world.agent),
        )
        .expect("the first agent may be mentioned in the first channel");
}

/// The same community with every agent a `bot` member: both releases list
/// all fifty, so the numbers compare like with like.
fn bench_directory_of_bots(c: &mut Criterion) {
    let mut world = directory(true);
    let listed = world
        .store
        .list_remote_agents(world.community, &world.viewer)
        .unwrap()
        .len();
    eprintln!("agent directory, every agent a bot: {listed} agent(s) listed of {AGENTS}");
    checked(&world);
    c.bench_function("agent directory, 50 bots: reconcile over 5k members", |b| {
        b.iter(|| {
            world
                .store
                .reconcile_remote_agents(black_box(world.community))
                .unwrap()
        })
    });
    c.bench_function(
        "agent directory, 50 bots: list agents among 5k members",
        |b| {
            b.iter(|| {
                world
                    .store
                    .list_remote_agents(black_box(world.community), &world.viewer)
                    .unwrap()
            })
        },
    );
    c.bench_function(
        "agent directory, 50 bots: validate one agent mention",
        |b| {
            b.iter(|| {
                world.store.validate_agent_mentions(
                    world.community,
                    world.channel,
                    &world.viewer,
                    std::slice::from_ref(&world.agent),
                )
            })
        },
    );
}

fn bench_directory(c: &mut Criterion) {
    let mut world = directory(false);
    let listed = world
        .store
        .list_remote_agents(world.community, &world.viewer)
        .unwrap()
        .len();
    eprintln!("agent directory: {listed} agent(s) listed of {AGENTS}");
    checked(&world);
    c.bench_function("agent directory: reconcile over 5k members", |b| {
        b.iter(|| {
            world
                .store
                .reconcile_remote_agents(black_box(world.community))
                .unwrap()
        })
    });
    c.bench_function("agent directory: list agents among 5k members", |b| {
        b.iter(|| {
            world
                .store
                .list_remote_agents(black_box(world.community), &world.viewer)
                .unwrap()
        })
    });
    c.bench_function("agent directory: candidates among 5k members", |b| {
        b.iter(|| {
            world
                .store
                .remote_agent_candidate_pubkeys(black_box(world.community))
                .unwrap()
        })
    });
    c.bench_function(
        "agent directory: mention candidates, 100-member channel",
        |b| {
            b.iter(|| {
                world
                    .store
                    .mention_candidates(world.community, world.channel, &world.viewer, "Wor")
                    .unwrap()
            })
        },
    );
    c.bench_function("agent directory: validate one agent mention", |b| {
        b.iter(|| {
            world.store.validate_agent_mentions(
                world.community,
                world.channel,
                &world.viewer,
                std::slice::from_ref(&world.agent),
            )
        })
    });
    // v0.15-only begin
    let members = world
        .store
        .remote_agent_member_pubkeys(world.community)
        .unwrap()
        .len();
    eprintln!("agent directory: {members} member(s) whose profile a refresh asks for");
    c.bench_function(
        "agent directory: members a refresh asks profiles for (5k members)",
        |b| {
            b.iter(|| {
                world
                    .store
                    .remote_agent_member_pubkeys(black_box(world.community))
                    .unwrap()
            })
        },
    );
    // v0.15-only end
}

// v0.15-only begin
/// The start of the agent runtime for a profile with `count` local agents,
/// all paused: what bzz does at start-up when it has agents and runs none.
fn bench_runtime_start(c: &mut Criterion) {
    use bzz::{
        agents::runtime::host::{AgentHost, HostOptions},
        config::{Harness, HostedAgentConfig, LocalAgentConfig, LocalAgentState},
        store::writer::StoreHandle,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    for count in [0_usize, 20] {
        let temporary = tempfile::TempDir::new().unwrap();
        let data = temporary.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let store = runtime.block_on(async {
            StoreHandle::spawn(Store::open(data.join("bzz.db")).unwrap()).unwrap()
        });
        let agents = (0..count)
            .map(|index| {
                let mut local = LocalAgentConfig::new(Harness::PiDurable);
                local.state = LocalAgentState::Paused;
                let id = Uuid::new_v4();
                std::fs::create_dir_all(data.join("agents").join(id.to_string())).unwrap();
                HostedAgentConfig {
                    id,
                    label: format!("agent-{index}"),
                    pubkey: Keys::generate().public_key().to_hex(),
                    community: Uuid::nil(),
                    owner_identity: Uuid::nil(),
                    auth_tag: String::new(),
                    credential_hash: String::new(),
                    allowed: Vec::new(),
                    paused: true,
                    local: Some(local),
                }
            })
            .collect::<Vec<_>>();
        c.bench_function(
            &format!("agent runtime: start with {count} paused agents"),
            |b| {
                b.iter(|| {
                    runtime.block_on(async {
                        let host = AgentHost::start(HostOptions::new(data.clone(), store.clone()))
                            .await
                            .unwrap();
                        let runnable = agents.iter().filter(|agent| host.runnable(agent)).count();
                        assert_eq!(runnable, 0);
                        host.shutdown().await;
                    })
                })
            },
        );
    }
}
// v0.15-only end

criterion_group!(
    benches,
    bench_directory,
    bench_directory_of_bots,
    // v0.15-only begin
    bench_runtime_start,
    // v0.15-only end
);
criterion_main!(benches);
