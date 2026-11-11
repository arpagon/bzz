//! Disposable fixture for Herdr scenarios and captures on a *fresh* isolated
//! pinned harness relay (`ws://localhost:3030`; channel names are fixed).
//! Never point it at a real community.
//!
//! Usage: `harness_fixture <empty fixture dir> <pinned Buzz checkout>`
//!
//! Writes `config/`, `data/` and `cache/` for a fresh viewer identity
//! (encrypted file; its passphrase in `passphrase`, mode 0600) and fills the
//! relay with generated content: `t9-general` (history, a thread, a reaction,
//! a task artifact and custom emoji), `t9-announcements` where the viewer is a
//! guest, and the viewer's own `t9-agent-config` system channel.

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
use nostr::{EventBuilder, Keys, Kind, Tag};
use secrecy::SecretString;
use uuid::Uuid;

const RELAY_WS: &str = "ws://localhost:3030/";
/// One second apart per event, so history has a deterministic order.
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

async fn connect(signer: &SignerHandle) -> SessionHandle {
    let (session, mut events) =
        session::connect(url::Url::parse(RELAY_WS).unwrap(), signer.clone())
            .await
            .expect("harness relay reachable");
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    session
}

async fn publish(
    session: &SessionHandle,
    signer: &SignerHandle,
    builder: EventBuilder,
) -> nostr::Event {
    let at = CLOCK.fetch_add(1, Ordering::Relaxed);
    let event = signer
        .sign(builder.custom_created_at(nostr::Timestamp::from(at)))
        .await
        .unwrap();
    let ack = session.publish(event.clone()).await.unwrap();
    assert!(ack.accepted, "{}", ack.message);
    event
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
    // BZZ_FIXTURE_HISTORY: generated messages in #t9-general (default 24;
    // more than 100 spans several channel-window pages for scroll captures).
    let history = std::env::var("BZZ_FIXTURE_HISTORY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(24)
        .clamp(1, 2_000);
    CLOCK.store(
        nostr::Timestamp::now().as_secs() - history - 60,
        Ordering::Relaxed,
    );
    let owner = Keys::generate();
    let viewer = Keys::generate();
    seed_member(&source, &owner.public_key().to_hex());
    seed_member(&source, &viewer.public_key().to_hex());
    tokio::time::sleep(Duration::from_secs(2)).await;
    // BZZ_FIXTURE_IDENTITY_ADAPTER: an FI-enforcing relay (`test-relay.sh
    // --nip-fi`) admits nothing without evidence, so write only the viewer's
    // profile with this adapter and no relay content.
    let adapter = std::env::var("BZZ_FIXTURE_IDENTITY_ADAPTER").ok();
    if adapter.is_none() {
        publish_content(&owner, &viewer, history).await;
    }
    write_profile(&root, viewer, adapter);
}

async fn publish_content(owner: &Keys, viewer: &Keys, history: u64) {
    let owner_signer = SignerHandle::spawn(owner.clone());
    let viewer_signer = SignerHandle::spawn(viewer.clone());
    let owner_session = connect(&owner_signer).await;
    let viewer_session = connect(&viewer_signer).await;
    use buzz_sdk::{ChannelKind, Visibility};

    // general: open stream with generated history, a thread and a reaction.
    let general = Uuid::new_v4();
    publish(
        &owner_session,
        &owner_signer,
        create(general, "t9-general", Visibility::Open, ChannelKind::Stream),
    )
    .await;
    publish(
        &viewer_session,
        &viewer_signer,
        buzz_sdk::build_join(general).unwrap(),
    )
    .await;
    let mut last = None;
    for index in 1..=history {
        let text = format!(
            "Generated fixture message {index:03}: the harness relay keeps this channel busy."
        );
        last = Some(
            publish(
                &owner_session,
                &owner_signer,
                buzz_sdk::build_message(general, &text, None, &[], false, &[], &[]).unwrap(),
            )
            .await,
        );
    }
    let thread_root = last.unwrap();
    let thread = buzz_sdk::ThreadRef {
        root_event_id: thread_root.id,
        parent_event_id: thread_root.id,
    };
    for index in 1..=3 {
        publish(
            &owner_session,
            &owner_signer,
            buzz_sdk::build_message(
                general,
                &format!("Generated thread reply {index}"),
                Some(&thread),
                &[],
                false,
                &[],
                &[],
            )
            .unwrap(),
        )
        .await;
    }
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_reaction(thread_root.id, "👍").unwrap(),
    )
    .await;
    // Custom emoji. The default image URL is deliberately unreachable (text
    // fallback); graphics captures set BZZ_FIXTURE_EMOJI_URL to a public
    // bzz-owned image.
    let emoji_url = std::env::var("BZZ_FIXTURE_EMOJI_URL")
        .unwrap_or_else(|_| "https://example.invalid/bzz/wave.png".into());
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_custom_emoji_set(&[buzz_sdk::CustomEmoji {
            shortcode: "bzzwave".into(),
            url: emoji_url.clone(),
        }])
        .unwrap(),
    )
    .await;
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_message(
            general,
            "Generated custom emoji :bzzwave: fixture",
            None,
            &[],
            false,
            &[],
            &[vec!["emoji".into(), "bzzwave".into(), emoji_url.clone()]],
        )
        .unwrap(),
    )
    .await;
    // A task artifact assigned to the viewer.
    let task = EventBuilder::new(Kind::Custom(45_010), r#"{"status":"open"}"#).tags(
        [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &general.to_string()],
            ["type", "buzz.task"],
            ["title", "Generated fixture task"],
            ["op", "create"],
            ["assignee", &viewer.public_key().to_hex()],
        ]
        .map(|parts| Tag::parse(parts).unwrap()),
    );
    publish(&owner_session, &owner_signer, task).await;

    // announcements: joiners become guests (read-only).
    let announcements = Uuid::new_v4();
    publish(
        &owner_session,
        &owner_signer,
        create(
            announcements,
            "t9-announcements",
            Visibility::Open,
            ChannelKind::Stream,
        ),
    )
    .await;
    publish(
        &owner_session,
        &owner_signer,
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
    publish(
        &owner_session,
        &owner_signer,
        buzz_sdk::build_message(
            announcements,
            "Generated announcement: guests read only.",
            None,
            &[],
            false,
            &[],
            &[],
        )
        .unwrap(),
    )
    .await;
    publish(
        &viewer_session,
        &viewer_signer,
        buzz_sdk::build_join(announcements).unwrap(),
    )
    .await;

    // The viewer's own system channel.
    publish(
        &viewer_session,
        &viewer_signer,
        create(
            Uuid::new_v4(),
            "t9-agent-config",
            Visibility::Private,
            ChannelKind::System,
        ),
    )
    .await;
    owner_session.shutdown().await;
    viewer_session.shutdown().await;
}

/// The viewer's isolated bzz profile.
fn write_profile(root: &std::path::Path, viewer: Keys, adapter: Option<String>) {
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
            "harness-viewer".into(),
            KeyBackend::EncryptedFile,
            viewer,
            Some(&SecretString::from(passphrase)),
        )
        .unwrap();
    let community = config
        .add_community(
            "harness".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    let fi = adapter.is_some();
    if let Some(adapter) = adapter {
        config
            .communities
            .iter_mut()
            .find(|entry| entry.id == community)
            .unwrap()
            .identity_adapter = Some(adapter);
    }
    config.save(&paths).unwrap();
    if fi {
        println!("fixture ready: FI profile (identity_adapter set, no relay content)");
    } else {
        println!(
            "fixture ready: #t9-general, #t9-announcements (guest), #t9-agent-config (system)"
        );
    }
}
