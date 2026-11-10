//! Publish a generated kind 0 profile for a disposable identity, as the
//! manual E2E plan's section 3 asks before member search, mentions and DMs.
//! bzz itself never publishes a kind 0 for a human identity.
//!
//! Usage (paths from BZZ_CONFIG_DIR / BZZ_DATA_DIR / BZZ_CACHE_DIR; the
//! passphrase of an encrypted-file identity through BZZ_PASSPHRASE_FD):
//! `publish_test_profile <identity label> <display name>`

use bzz::{
    auth::{IdentityManager, read_passphrase, signer::SignerHandle},
    config::{Config, KeyBackend},
    paths::Paths,
    realtime::session::{self, SessionEvent},
};
use nostr::{EventBuilder, Metadata};

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let label = arguments.next().expect("identity label");
    let display = arguments.next().expect("display name");
    let paths = Paths::discover().expect("paths from BZZ_* variables");
    let config = Config::load(&paths).expect("config");
    let identity = config
        .identities
        .iter()
        .find(|identity| identity.label == label)
        .expect("identity with that label")
        .clone();
    let community = config
        .communities
        .iter()
        .find(|community| community.identity_id == identity.id)
        .expect("a community for that identity");
    let passphrase = matches!(identity.backend, KeyBackend::EncryptedFile)
        .then(|| read_passphrase("Identity passphrase: ", false).expect("passphrase"));
    let keys = IdentityManager::new(&paths)
        .unlock(&identity, passphrase.as_ref())
        .expect("unlock");
    let signer = SignerHandle::spawn(keys);
    let relay = url::Url::parse(&community.relay_url).expect("relay URL");
    let (session, mut events) = session::connect(relay, signer.clone())
        .await
        .expect("relay reachable");
    assert!(matches!(
        events.recv().await,
        Some(SessionEvent::Authenticated)
    ));
    let metadata = Metadata::new()
        .name(display.to_ascii_lowercase().replace(' ', "-"))
        .display_name(&display);
    let event = signer
        .sign(EventBuilder::metadata(&metadata))
        .await
        .expect("sign");
    let ack = session.publish(event).await.expect("publish");
    assert!(ack.accepted, "{}", ack.message);
    session.shutdown().await;
    signer.lock().await;
    println!("published a generated kind 0 profile for {label}");
}
