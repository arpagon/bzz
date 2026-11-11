//! Generated demo community for release videos and site captures, on a
//! *fresh* isolated pinned harness relay (`ws://localhost:3030`). Every
//! person, message and image is generated; never point it at a real
//! community.
//!
//! Usage: `demo_fixture <empty fixture dir> <pinned Buzz checkout>`
//!
//! Three generated people (kind 0 names), `#team` with history, a thread,
//! reactions, an inline custom emoji and a task artifact, `#release-news`
//! where the viewer joins as a guest, and the viewer's own system channel.
//! Writes the viewer's isolated `config/`, `data/`, `cache/` and its
//! passphrase (`passphrase`, mode 0600).

use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bzz::{
    auth::{IdentityManager, signer::SignerHandle},
    config::{Config, KeyBackend},
    paths::Paths,
    realtime::session::{self, SessionEvent, SessionHandle},
};
use nostr::{EventBuilder, Keys, Kind, Metadata, Tag};
use secrecy::SecretString;
use uuid::Uuid;

const RELAY_WS: &str = "ws://localhost:3030/";
/// A generated 64×64 image committed in this repository.
const EMOJI_URL: &str =
    "https://raw.githubusercontent.com/arpagon/bzz/v0.13.0/tests/fixtures/custom-emoji/bzzwave.png";
static CLOCK: AtomicU64 = AtomicU64::new(0);

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
}

async fn person(keys: &Keys, name: &str, about: &str) -> Person {
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
    let person = Person { signer, session };
    let metadata = Metadata::new().name(name).display_name(name).about(about);
    person.publish(EventBuilder::metadata(&metadata)).await;
    person
}

impl Person {
    async fn publish(&self, builder: EventBuilder) -> nostr::Event {
        let at = CLOCK.fetch_add(2, Ordering::Relaxed);
        let event = self
            .signer
            .sign(builder.custom_created_at(nostr::Timestamp::from(at)))
            .await
            .unwrap();
        let ack = self.session.publish(event.clone()).await.unwrap();
        assert!(ack.accepted, "{}", ack.message);
        event
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
}

fn create(
    id: Uuid,
    name: &str,
    visibility: buzz_sdk::Visibility,
    kind: buzz_sdk::ChannelKind,
) -> EventBuilder {
    buzz_sdk::build_create_channel(id, name, Some(visibility), Some(kind), None, None, false)
        .unwrap()
}

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let root = std::path::PathBuf::from(arguments.next().expect("fixture directory"));
    let source = arguments.next().expect("pinned Buzz checkout");
    assert!(
        !root.exists() || std::fs::read_dir(&root).unwrap().next().is_none(),
        "use an empty fixture directory"
    );
    CLOCK.store(nostr::Timestamp::now().as_secs() - 100, Ordering::Relaxed);
    let keys = [
        Keys::generate(),
        Keys::generate(),
        Keys::generate(),
        Keys::generate(),
    ];
    for key in &keys {
        seed_member(&source, &key.public_key().to_hex());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let [ada_keys, lin_keys, sam_keys, viewer_keys] = keys;
    let ada = person(&ada_keys, "Ada", "Generated demo persona · maintainer").await;
    let lin = person(&lin_keys, "Lin", "Generated demo persona · design").await;
    let sam = person(&sam_keys, "Sam", "Generated demo persona · infra").await;
    let viewer = person(&viewer_keys, "You", "Generated demo viewer").await;
    use buzz_sdk::{ChannelKind, Visibility};

    let general = Uuid::new_v4();
    ada.publish(create(
        general,
        "team",
        Visibility::Open,
        ChannelKind::Stream,
    ))
    .await;
    for who in [&lin, &sam, &viewer] {
        who.publish(buzz_sdk::build_join(general).unwrap()).await;
    }
    let lines: [(&Person, &str); 18] = [
        (
            &ada,
            "Morning! Release branch is cut, CI is green on all four platforms.",
        ),
        (&lin, "Nice. I'm reviewing the new channel windows today."),
        (
            &sam,
            "Relay upgrade went fine overnight, no reconnect storms.",
        ),
        (
            &ada,
            "Remember: system channels now stay folded in the System group until you ask.",
        ),
        (&lin, "Love that. My sidebar was all agent config before."),
        (
            &sam,
            "Huddle audio checked with two clients, round trip is clean.",
        ),
        (
            &ada,
            "Profile cards show public channels and roles now. Try `Space p`.",
        ),
        (&lin, "Custom emoji render inline in Kitty and Ghostty."),
        (
            &sam,
            "Broker agents only post what their allowlist says. Tested the denials.",
        ),
        (
            &ada,
            "Upgrade from 0.12.1 keeps drafts, unread and search. Backup written first.",
        ),
        (&lin, "Writing the release notes now."),
        (&sam, "Benchmarks: no regressions against 0.12.1."),
        (
            &ada,
            "Windows fetch history in two requests instead of three.",
        ),
        (
            &lin,
            "Guests in announce channels get a clear read-only notice.",
        ),
        (&sam, "Presence is opt-in: `:presence on`."),
        (&ada, "Federated sign-in works with the browser flow."),
        (&lin, "Screenshots for the site are next."),
        (&sam, "Tagging after the last check."),
    ];
    for (who, text) in lines {
        who.say(general, text).await;
    }
    let root_message = ada
        .say(general, "Shipping v0.13 today. Anything blocking?")
        .await;
    ada.react(&root_message, "🚀").await;
    lin.react(&root_message, "🚀").await;
    sam.react(&root_message, "👍").await;
    lin.reply(general, &root_message, "Docs are merged.").await;
    sam.reply(
        general,
        &root_message,
        "CI and the pinned relay suite are green.",
    )
    .await;
    ada.reply(general, &root_message, "Then we ship. 🎉").await;
    ada.publish(
        buzz_sdk::build_custom_emoji_set(&[buzz_sdk::CustomEmoji {
            shortcode: "bzzwave".into(),
            url: EMOJI_URL.into(),
        }])
        .unwrap(),
    )
    .await;
    lin.publish(
        buzz_sdk::build_message(
            general,
            "Thanks everyone :bzzwave:",
            None,
            &[],
            false,
            &[],
            &[vec!["emoji".into(), "bzzwave".into(), EMOJI_URL.into()]],
        )
        .unwrap(),
    )
    .await;
    let task = EventBuilder::new(Kind::Custom(45_010), r#"{"status":"open"}"#).tags(
        [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &general.to_string()],
            ["type", "buzz.task"],
            ["title", "Record the v0.13 demo"],
            ["op", "create"],
            ["assignee", &viewer_keys.public_key().to_hex()],
        ]
        .map(|parts| Tag::parse(parts).unwrap()),
    );
    ada.publish(task).await;

    let announcements = Uuid::new_v4();
    ada.publish(create(
        announcements,
        "release-news",
        Visibility::Open,
        ChannelKind::Stream,
    ))
    .await;
    ada.publish(
        buzz_sdk::build_update_channel(
            announcements,
            None,
            None,
            None,
            None,
            Some(buzz_sdk::ChannelJoinRole::Guest),
            None,
            None,
        )
        .unwrap(),
    )
    .await;
    ada.say(
        announcements,
        "bzz v0.13.0 is out. Release notes on GitHub.",
    )
    .await;
    viewer
        .publish(buzz_sdk::build_join(announcements).unwrap())
        .await;
    viewer
        .publish(create(
            Uuid::new_v4(),
            "my-agent-config",
            Visibility::Private,
            ChannelKind::System,
        ))
        .await;
    for who in [ada, lin, sam, viewer] {
        who.session.shutdown().await;
    }
    write_profile(&root, viewer_keys);
}

fn write_profile(root: &std::path::Path, viewer: Keys) {
    let paths = Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        cache_dir: root.join("cache"),
    };
    paths.ensure().unwrap();
    let passphrase = hex::encode(rand::random::<[u8; 16]>());
    let passphrase_file = root.join("passphrase");
    std::fs::write(&passphrase_file, format!("{passphrase}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&passphrase_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
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
            "demo".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    config.save(&paths).unwrap();
    println!("demo fixture ready: #team, #release-news (guest), #my-agent-config (system)");
}
