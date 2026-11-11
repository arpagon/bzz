//! Test-only fake NIP-FA adapter and NIP-FI issuer for the FI-enforcing
//! pinned relay profile (`scripts/test-relay.sh --nip-fi`). Never ship this.
//!
//! Usage: `fake_nip_fa <bind address> <public origin> <64-hex P-256 signing key>`
#[path = "../tests/support/fake_adapter.rs"]
mod fake_adapter;
#[path = "../tests/support/scripted_relay.rs"]
mod scripted_relay;

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let address = arguments.next().unwrap_or_else(|| "127.0.0.1:3093".into());
    let origin = arguments
        .next()
        .unwrap_or_else(|| "http://localhost:3093".into());
    let key = arguments.next().expect("signing key hex");
    let key = p256::ecdsa::SigningKey::from_slice(&hex::decode(key).expect("hex key"))
        .expect("P-256 key");
    let adapter = fake_adapter::FakeAdapter::start_with(
        &address,
        Some(&origin),
        key,
        fake_adapter::Config {
            assertion_ttl: 120,
            ..fake_adapter::Config::default()
        },
    )
    .await;
    println!(
        "fake NIP-FA adapter at {} (issuer {})",
        adapter.base(),
        adapter.issuer
    );
    tokio::signal::ctrl_c().await.ok();
}
