//! Generated community and live "director" for the bzz promotional film, on a
//! *fresh* isolated pinned harness relay (`ws://localhost:3030`). Every
//! person, message and image is generated or a bzz-owned asset; never point
//! it at a real community.
//!
//! ```text
//! promo_fixture seed    <empty dir> <pinned Buzz checkout> <avatars dir>  # people (with
//!                        avatars <name>.png), channels, DM, profile
//! promo_fixture bot     <dir> <agent pubkey>        # viewer gives the agent the bot role in #team
//! promo_fixture history <dir> <image.png>           # backdated history, after every system row
//! promo_fixture drive <dir> live                  # Lin and Sam post while a shot records
//! promo_fixture drive <dir> agent-profile <png>   # the agent sets its name and avatar via the broker
//! promo_fixture drive <dir> agent-reply           # answers the viewer's @mention through the broker
//! promo_fixture drive <dir> huddle <seconds>      # Ada and Sam hold a huddle with live audio
//! ```
//!
//! `seed` writes the viewer's isolated `config/`, `data/`, `cache/`, its
//! passphrase and `state.json` (generated keys and ids), all owner-only.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bzz::{
    auth::{IdentityManager, signer::SignerHandle},
    config::{Config, KeyBackend},
    media::client::MediaClient,
    paths::Paths,
    protocol::http::HttpClient,
    realtime::session::{self, SessionEvent, SessionHandle},
};
use nostr::{EventBuilder, Keys, Kind, Metadata, Tag};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

const RELAY_WS: &str = "ws://localhost:3030/";
const RELAY_HTTP: &str = "http://localhost:3030/";
/// A generated 64×64 image committed in this repository.
const EMOJI_URL: &str =
    "https://raw.githubusercontent.com/arpagon/bzz/v0.13.0/tests/fixtures/custom-emoji/bzzwave.png";
const AGENT_NAME: &str = "Scout";
static CLOCK: AtomicU64 = AtomicU64::new(0);

fn owner_only(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn seed_member(source: &str, pubkey: &str) {
    let compose = format!("{source}/docker-compose.harness.yml");
    let sql = format!(
        "INSERT INTO relay_members (community_id,pubkey,role,added_by) SELECT id,'{pubkey}','member',NULL FROM communities WHERE lower(host)='localhost:3030' ON CONFLICT (community_id,pubkey) DO NOTHING;"
    );
    let status = Command::new("docker")
        .args([
            "compose",
            "-p",
            "buzz-harness",
            "-f",
            &compose,
            "exec",
            "-T",
            "postgres",
        ])
        .args([
            "psql",
            "-U",
            "buzz",
            "-d",
            "buzz",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
            &sql,
        ])
        .status()
        .expect("docker compose");
    assert!(status.success(), "seeding a harness member failed");
}

struct Person {
    signer: SignerHandle,
    session: SessionHandle,
    /// Backdate history (seed) or publish at the real time (live shots).
    backdate: bool,
}

async fn connect(keys: &Keys, backdate: bool) -> Person {
    let signer = SignerHandle::spawn(keys.clone());
    let (session, mut events) =
        session::connect(url::Url::parse(RELAY_WS).unwrap(), signer.clone())
            .await
            .expect("harness relay reachable");
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    Person {
        signer,
        session,
        backdate,
    }
}

impl Person {
    async fn publish(&self, builder: EventBuilder) -> nostr::Event {
        let builder = if self.backdate {
            let at = CLOCK.fetch_add(1, Ordering::Relaxed);
            builder.custom_created_at(nostr::Timestamp::from(at))
        } else {
            builder
        };
        let event = self.signer.sign(builder).await.unwrap();
        let ack = self.session.publish(event.clone()).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        event
    }

    async fn ack_message(&self, builder: EventBuilder) -> String {
        let event = self.signer.sign(builder).await.unwrap();
        let ack = self.session.publish(event).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        ack.message
    }

    async fn named(&self, name: &str, about: &str, picture: Option<&str>) {
        let mut metadata = Metadata::new().name(name).display_name(name).about(about);
        if let Some(picture) = picture {
            metadata = metadata.picture(url::Url::parse(picture).unwrap());
        }
        self.publish(EventBuilder::metadata(&metadata)).await;
    }

    async fn say(&self, channel: Uuid, text: &str) -> nostr::Event {
        self.publish(buzz_sdk::build_message(channel, text, None, &[], false, &[], &[]).unwrap())
            .await
    }

    async fn reply(&self, channel: Uuid, root: &nostr::Event, text: &str) -> nostr::Event {
        let thread = buzz_sdk::ThreadRef {
            root_event_id: root.id,
            parent_event_id: root.id,
        };
        self.publish(
            buzz_sdk::build_message(channel, text, Some(&thread), &[], false, &[], &[]).unwrap(),
        )
        .await
    }

    async fn react(&self, target: &nostr::Event, emoji: &str) {
        self.publish(buzz_sdk::build_reaction(target.id, emoji).unwrap())
            .await;
    }

    async fn channel(&self, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.publish(
            buzz_sdk::build_create_channel(
                id,
                name,
                Some(buzz_sdk::Visibility::Open),
                Some(buzz_sdk::ChannelKind::Stream),
                None,
                None,
                false,
            )
            .unwrap(),
        )
        .await;
        id
    }

    async fn join(&self, channel: Uuid) {
        self.publish(buzz_sdk::build_join(channel).unwrap()).await;
    }
}

/// Upload a generated avatar to the relay's Blossom store; returns its
/// canonical same-origin `/media/<sha256>.png` URL (bzz's relay-avatar path).
async fn upload_avatar(person: &Person, path: PathBuf) -> String {
    let media = MediaClient::new(
        url::Url::parse(RELAY_HTTP).unwrap(),
        "localhost:3030".into(),
        person.signer.clone(),
        2,
    )
    .unwrap();
    media
        .upload(&path, "image/png", None)
        .await
        .unwrap_or_else(|error| panic!("uploading {}: {error}", path.display()))
        .url
}

fn state_path(root: &Path) -> PathBuf {
    root.join("state.json")
}

fn load(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(state_path(root)).unwrap()).unwrap()
}

fn keys(state: &Value, who: &str) -> Keys {
    Keys::parse(state["keys"][who].as_str().unwrap()).unwrap()
}

fn uuid(state: &Value, name: &str) -> Uuid {
    Uuid::parse_str(state["channels"][name].as_str().unwrap()).unwrap()
}

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&arguments[1]);
    match arguments[0].as_str() {
        "seed" => seed(&root, &arguments[2], Path::new(&arguments[3])).await,
        "history" => history(&root, Path::new(&arguments[2])).await,
        "bot" => bot(&root, &arguments[2]).await,
        "drive" => drive(&root, &arguments[2], &arguments[3..]).await,
        other => panic!("unknown command {other}"),
    }
}

async fn seed(root: &Path, source: &str, avatars: &Path) {
    assert!(
        !root.exists() || std::fs::read_dir(root).unwrap().next().is_none(),
        "use an empty fixture directory"
    );
    std::fs::create_dir_all(root).unwrap();
    let all: Vec<(&str, Keys)> = ["ada", "lin", "sam", "viewer"]
        .into_iter()
        .map(|name| (name, Keys::generate()))
        .collect();
    for (_, key) in &all {
        seed_member(source, &key.public_key().to_hex());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let key = |name: &str| all.iter().find(|(n, _)| *n == name).unwrap().1.clone();
    let ada = connect(&key("ada"), false).await;
    let lin = connect(&key("lin"), false).await;
    let sam = connect(&key("sam"), false).await;
    let viewer = connect(&key("viewer"), false).await;
    let avatar = |who: &str| upload_avatar(&viewer, avatars.join(format!("{who}.png")));
    let (ada_pic, lin_pic, sam_pic, you_pic) = (
        avatar("ada").await,
        avatar("lin").await,
        avatar("sam").await,
        avatar("you").await,
    );
    ada.named("Ada", "Generated persona · backend", Some(&ada_pic))
        .await;
    lin.named("Lin", "Generated persona · design", Some(&lin_pic))
        .await;
    sam.named("Sam", "Generated persona · infra", Some(&sam_pic))
        .await;
    viewer
        .named("You", "Generated viewer", Some(&you_pic))
        .await;

    // The viewer owns #team, so it can give its own agent the bot role there.
    let team = viewer.channel("team").await;
    let design = lin.channel("design").await;
    let infra = sam.channel("infra").await;
    let launch = ada.channel("launch").await;
    for (who, channels) in [
        (&ada, vec![team, design, infra]),
        (&lin, vec![team, infra, launch]),
        (&sam, vec![team, design, launch]),
        (&viewer, vec![design, infra, launch]),
    ] {
        for channel in channels {
            who.join(channel).await;
        }
    }
    let opened = viewer
        .ack_message(buzz_sdk::build_dm_open(&[key("lin").public_key().to_hex().as_str()]).unwrap())
        .await;
    let response: Value =
        serde_json::from_str(opened.strip_prefix("response:").unwrap_or(&opened)).unwrap();
    let dm = Uuid::parse_str(response["channel_id"].as_str().unwrap()).unwrap();
    for who in [ada, lin, sam, viewer] {
        who.session.shutdown().await;
    }
    let state = json!({
        "keys": all.iter().map(|(n, k)| (n.to_string(), json!(k.secret_key().to_secret_hex()))).collect::<serde_json::Map<_, _>>(),
        "channels": {"team": team, "design": design, "infra": infra, "launch": launch, "dm": dm},
        "seeded_at": nostr::Timestamp::now().as_secs(),
    });
    owner_only(
        &state_path(root),
        &serde_json::to_string_pretty(&state).unwrap(),
    );
    write_profile(root, key("viewer"));
}

/// History is backdated, but always after the relay-signed system rows from
/// `seed` and `bot` (joins, the agent's bot role), so those sit above it.
async fn history(root: &Path, image: &Path) {
    let state = load(root);
    let seeded_at = state["seeded_at"].as_u64().unwrap();
    let wait = (seeded_at + 75).saturating_sub(nostr::Timestamp::now().as_secs());
    tokio::time::sleep(Duration::from_secs(wait)).await;
    CLOCK.store(nostr::Timestamp::now().as_secs() - 45, Ordering::Relaxed);
    let ada = connect(&keys(&state, "ada"), true).await;
    let lin = connect(&keys(&state, "lin"), true).await;
    let sam = connect(&keys(&state, "sam"), true).await;
    let (team, design, infra, launch, dm) = (
        uuid(&state, "team"),
        uuid(&state, "design"),
        uuid(&state, "infra"),
        uuid(&state, "launch"),
        uuid(&state, "dm"),
    );
    sam.say(infra, "Relay metrics are calm after the upgrade.")
        .await;
    ada.say(infra, "Nice. Backups ran on schedule too.").await;
    lin.say(design, "New onboarding copy is ready for review.")
        .await;
    sam.say(design, "Reads well. Ship it.").await;
    ada.say(launch, "Launch is Thursday. Checklist lives in #team.")
        .await;

    ada.say(team, "Morning! Staging is green.").await;
    lin.say(team, "I'm polishing the launch post today.").await;
    sam.say(team, "Infra is ready. Autoscaling tested twice.")
        .await;
    let root_message = ada
        .say(team, "Launch checklist: anything still blocking?")
        .await;
    lin.reply(team, &root_message, "Copy is final.").await;
    sam.reply(team, &root_message, "Load test passed.").await;
    ada.react(&root_message, "🚀").await;
    lin.react(&root_message, "🚀").await;
    sam.react(&root_message, "👍").await;

    // A real image through the relay's Blossom endpoint, shown inline.
    let media = MediaClient::new(
        url::Url::parse(RELAY_HTTP).unwrap(),
        "localhost:3030".into(),
        lin.signer.clone(),
        2,
    )
    .unwrap();
    let attachment = media
        .upload(image, "image/png", Some("launch-card.png".into()))
        .await
        .unwrap();
    lin.publish(
        buzz_sdk::build_message(
            team,
            &format!(
                "Draft social card for launch day\n{}",
                attachment.markdown_line()
            ),
            None,
            &[],
            false,
            &[attachment.imeta_tag()],
            &[],
        )
        .unwrap(),
    )
    .await;
    ada.publish(
        buzz_sdk::build_custom_emoji_set(&[buzz_sdk::CustomEmoji {
            shortcode: "bzzwave".into(),
            url: EMOJI_URL.into(),
        }])
        .unwrap(),
    )
    .await;
    sam.publish(
        buzz_sdk::build_message(
            team,
            "Looks great :bzzwave:",
            None,
            &[],
            false,
            &[],
            &[vec!["emoji".into(), "bzzwave".into(), EMOJI_URL.into()]],
        )
        .unwrap(),
    )
    .await;

    lin.say(dm, "Got a minute after standup?").await;

    let task = EventBuilder::new(Kind::Custom(45_010), r#"{"status":"open"}"#).tags(
        [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &team.to_string()],
            ["type", "buzz.task"],
            ["title", "Final smoke test on macOS"],
            ["op", "create"],
            ["assignee", &keys(&state, "viewer").public_key().to_hex()],
        ]
        .map(|parts| Tag::parse(parts).unwrap()),
    );
    ada.publish(task).await;
    for who in [ada, lin, sam] {
        who.session.shutdown().await;
    }

    println!("history written");
}

fn write_profile(root: &Path, viewer: Keys) {
    let paths = Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        cache_dir: root.join("cache"),
    };
    paths.ensure().unwrap();
    let passphrase = hex::encode(rand::random::<[u8; 16]>());
    owner_only(&root.join("passphrase"), &format!("{passphrase}\n"));
    let mut config = Config::default();
    let identity = IdentityManager::new(&paths)
        .import_keys(
            &mut config,
            "demo".into(),
            KeyBackend::EncryptedFile,
            viewer,
            Some(&SecretString::from(passphrase)),
        )
        .unwrap();
    config
        .add_community(
            "Studio".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    config.media.protocol = bzz::config::MediaProtocol::Sixel;
    config.save(&paths).unwrap();
    println!("promo fixture ready: #team (owned by the viewer), #design, #infra, #launch, a DM");
}

async fn bot(root: &Path, agent: &str) {
    let state = load(root);
    let viewer = connect(&keys(&state, "viewer"), false).await;
    viewer
        .publish(
            buzz_sdk::build_add_member(
                uuid(&state, "team"),
                agent,
                Some(buzz_sdk::MemberRole::Bot),
            )
            .unwrap(),
        )
        .await;
    viewer.session.shutdown().await;
    println!("agent {agent} has the bot role in #team");
}

/// The broker's loopback HTTP binding, driven by the SDK's validating client.
struct BrokerHttp(String, String);

impl buzz_sdk::broker::BrokerClient for BrokerHttp {
    fn send<'a>(
        &'a self,
        request: &'a buzz_sdk::broker::PreparedRequest,
        _dispatch: buzz_sdk::broker::Dispatch,
    ) -> buzz_sdk::broker::BrokerFuture<'a> {
        Box::pin(async move {
            let unreachable = |error: reqwest::Error| {
                buzz_sdk::broker::BrokerTransportError::Unreachable(error.to_string())
            };
            let bytes = reqwest::Client::new()
                .post(&self.0)
                .header("authorization", format!("Bearer {}", self.1))
                .header("content-type", "application/json")
                .body(request.body().to_vec())
                .send()
                .await
                .map_err(unreachable)?
                .bytes()
                .await
                .map_err(unreachable)?;
            serde_json::from_slice(&bytes).map_err(|error| {
                buzz_sdk::broker::BrokerTransportError::Unreachable(error.to_string())
            })
        })
    }
}

fn broker(root: &Path) -> BrokerHttp {
    let port = std::fs::read_to_string(root.join("data/broker.port"))
        .expect("bzz is running and unlocked")
        .trim()
        .to_owned();
    let credential = std::fs::read_to_string(root.join("agent.credential"))
        .unwrap()
        .trim()
        .to_owned();
    BrokerHttp(format!("http://127.0.0.1:{port}/v1/action"), credential)
}

async fn drive(root: &Path, scene: &str, rest: &[String]) {
    use buzz_sdk::broker::{ActionArgs, BrokerClientExt as _, BrokerRequest};
    let state = load(root);
    let team = uuid(&state, "team");
    match scene {
        "live" => {
            let lin = connect(&keys(&state, "lin"), false).await;
            let sam = connect(&keys(&state, "sam"), false).await;
            tokio::time::sleep(Duration::from_millis(1500)).await;
            lin.say(team, "Launch post is scheduled for 9am.").await;
            tokio::time::sleep(Duration::from_millis(1600)).await;
            sam.say(team, "Status page is ready too.").await;
        }
        "agent-profile" => {
            let viewer = connect(&keys(&state, "viewer"), false).await;
            let picture = match rest.first() {
                Some(path) => Some(upload_avatar(&viewer, PathBuf::from(path)).await),
                None => None,
            };
            viewer.session.shutdown().await;
            let request = BrokerRequest::new(
                "profile-1",
                ActionArgs::ProfileSet(buzz_sdk::broker::ProfileSetArgs {
                    display_name: Some(AGENT_NAME.into()),
                    about: Some("Generated demo agent".into()),
                    picture,
                }),
            )
            .unwrap()
            .prepare()
            .unwrap();
            let response = broker(root).execute(&request).await.unwrap();
            assert!(response.result().error().is_none(), "{response:?}");
            println!("agent profile set");
        }
        "agent-reply" => {
            let agent = std::fs::read_to_string(root.join("agent.pubkey"))
                .unwrap()
                .trim()
                .to_owned();
            let viewer = keys(&state, "viewer");
            let http = HttpClient::new(
                url::Url::parse(RELAY_HTTP).unwrap(),
                SignerHandle::spawn(viewer.clone()),
            )
            .unwrap();
            let since = nostr::Timestamp::now().as_secs() - 5;
            let mention = loop {
                let events = http
                    .query_raw(&[json!({"kinds": [9], "#h": [team.to_string()], "#p": [agent], "since": since})])
                    .await
                    .unwrap();
                if let Some(event) = events.into_iter().find(|e| e.pubkey == viewer.public_key()) {
                    break event;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            };
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let request = BrokerRequest::new(
                "reply-1",
                ActionArgs::MessageReply(buzz_sdk::broker::MessageReplyArgs {
                    channel_id: team.to_string(),
                    reply_to_event_id: mention.id.to_hex(),
                    content: "Checked all five items: copy, load test, infra and status page pass. One left: the macOS smoke test.".into(),
                    mentions: Vec::new(),
                }),
            )
            .unwrap()
            .prepare()
            .unwrap();
            let response = broker(root).execute(&request).await.unwrap();
            assert!(response.result().error().is_none(), "{response:?}");
            println!("agent replied");
        }
        "huddle" => {
            use bzz::{huddle::audio, protocol::huddle};
            let seconds: u64 = rest.first().map_or(20, |s| s.parse().unwrap());
            let ada_keys = keys(&state, "ada");
            let sam_keys = keys(&state, "sam");
            let ada = connect(&ada_keys, false).await;
            let backing = Uuid::new_v4();
            ada.publish(huddle::backing_channel(backing).unwrap()).await;
            ada.publish(huddle::started(team, backing).unwrap()).await;
            let ws = url::Url::parse(RELAY_WS).unwrap();
            let mut calls = Vec::new();
            let mut feeds = Vec::new();
            for who in [&ada_keys, &sam_keys] {
                let signer = SignerHandle::spawn(who.clone());
                let joined = audio::join(&ws, backing, team, &signer).await.unwrap();
                let (capture, capture_rx) = tokio::sync::mpsc::channel(64);
                let (playback, mut playback_rx) = tokio::sync::mpsc::channel(256);
                tokio::spawn(async move { while playback_rx.recv().await.is_some() {} });
                calls.push(audio::start(joined, capture_rx, playback).unwrap());
                feeds.push(capture);
            }
            // Ada speaks: a quiet tone, so the huddle carries real audio.
            calls[0].set_muted(false);
            let frames = seconds * 50;
            for frame in 0..frames as usize {
                let chunk: Vec<f32> = (0..960)
                    .map(|i| {
                        let t = (frame * 960 + i) as f32 / 48_000.0;
                        0.15 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    })
                    .collect();
                let _ = feeds[0].send(chunk).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            for call in calls {
                call.stop().await;
            }
            ada.publish(huddle::ended(team, backing).unwrap()).await;
            ada.session.shutdown().await;
        }
        other => panic!("unknown scene {other}"),
    }
}
