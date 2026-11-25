//! Joining a community (v0.17 A9) against a scripted relay: what the join
//! form and `bzz community add` read at an address, how a relay's answer
//! to AUTH is told apart, and how an invite is claimed. Each failure the
//! brief names (no answer, a relay bzz does not speak to, an identity
//! refused) is one case here.
mod support;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use base64::Engine as _;
use bzz::{
    auth::signer::SignerHandle,
    join::{self, Address, Admission, Claimed, ProbeFailure, RelayAddress},
};
use nostr::{JsonUtil as _, Keys};
use serde_json::json;
use support::scripted_relay::{
    HttpReply, Script, ScriptedRelay, WsAction, default_frame, default_http,
};

fn address_of(relay: &ScriptedRelay) -> RelayAddress {
    let port = relay.ws_url.port().unwrap();
    match join::parse_address(&format!("127.0.0.1:{port}")).unwrap() {
        Address::Relay(relay) => relay,
        other => panic!("{other:?}"),
    }
}

fn nip11(members_only: bool) -> serde_json::Value {
    let mut nips = vec![1, 11, 29, 42];
    if members_only {
        nips.push(43);
    }
    json!({"name": "My team", "description": "We ship.", "self": "ab".repeat(32), "supported_nips": nips})
}

#[tokio::test]
async fn the_address_says_what_answers_there() {
    let open = ScriptedRelay::start(Script::default().nip11(nip11(false))).await;
    let facts = join::probe(&address_of(&open)).await.unwrap();
    assert_eq!(facts.name.as_deref(), Some("My team"));
    assert_eq!(facts.description.as_deref(), Some("We ship."));
    assert!(!facts.members_only);
    assert_eq!(facts.relay_pubkey, "ab".repeat(32));
    open.stop();

    let closed = ScriptedRelay::start(Script::default().nip11(nip11(true))).await;
    assert!(
        join::probe(&address_of(&closed))
            .await
            .unwrap()
            .members_only
    );
    closed.stop();
}

#[tokio::test]
async fn each_failure_at_the_address_is_told_apart() {
    // A relay with no community at this host answers 404.
    let empty = ScriptedRelay::start(
        Script::default().http(|_| HttpReply::text(404, "relay: no community\n")),
    )
    .await;
    assert_eq!(
        join::probe(&address_of(&empty)).await,
        Err(ProbeFailure::NoCommunity)
    );
    empty.stop();
    // A relay bzz does not speak to: no signing key in its document.
    let plain = ScriptedRelay::start(
        Script::default().nip11(json!({"name": "a plain relay", "supported_nips": [1, 11]})),
    )
    .await;
    let failure = join::probe(&address_of(&plain)).await.unwrap_err();
    assert_eq!(failure, ProbeFailure::NotBuzz);
    let (what, next) = failure.explain(&address_of(&plain));
    assert!(what.contains("not a Buzz community"), "{what}");
    assert!(next.contains("Buzz relays only"), "{next}");
    plain.stop();
    // A web server, not a relay.
    let web = ScriptedRelay::start(
        Script::default().http(|_| HttpReply::text(200, "<html>hello</html>")),
    )
    .await;
    assert_eq!(
        join::probe(&address_of(&web)).await,
        Err(ProbeFailure::NotBuzz)
    );
    web.stop();
    // Nothing listens there.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let Address::Relay(nobody) = join::parse_address(&format!("localhost:{port}")).unwrap() else {
        panic!("a relay address");
    };
    let failure = join::probe(&nobody).await.unwrap_err();
    assert_eq!(failure, ProbeFailure::NoAnswer);
    assert!(
        failure
            .explain(&nobody)
            .0
            .starts_with("nothing answers at localhost:")
    );
}

#[tokio::test]
async fn the_relays_answer_to_auth_decides_what_comes_next() {
    let member = Arc::new(AtomicBool::new(false));
    let refused = Arc::new(AtomicBool::new(false));
    let script = {
        let (member, refused) = (member.clone(), refused.clone());
        Script::default().nip11(nip11(true)).frames(move |frame| {
            if frame.value[0] == "AUTH" {
                let event = nostr::Event::from_json(frame.value[1].to_string()).unwrap();
                let said = if refused.load(Ordering::SeqCst) {
                    "blocked: this pubkey is banned"
                } else {
                    "restricted: not a relay member"
                };
                if !member.load(Ordering::SeqCst) {
                    return vec![WsAction::Send(json!([
                        "OK",
                        event.id.to_hex(),
                        false,
                        said
                    ]))];
                }
            }
            default_frame(frame)
        })
    };
    let relay = ScriptedRelay::start(script).await;
    let address = address_of(&relay);
    let signer = SignerHandle::spawn(Keys::generate());
    assert_eq!(
        join::admission(&address, &signer).await,
        Admission::NotMember
    );
    // An admin adds the person: the next ask gets in, with no restart.
    member.store(true, Ordering::SeqCst);
    assert_eq!(join::admission(&address, &signer).await, Admission::In);
    // A refusal that is not about membership is said as the relay said it.
    member.store(false, Ordering::SeqCst);
    refused.store(true, Ordering::SeqCst);
    assert!(matches!(
        join::admission(&address, &signer).await,
        Admission::Refused(said) if said.contains("banned")
    ));
    assert_eq!(relay.frames_of("AUTH").len(), 3);
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn an_invite_is_claimed_by_the_joining_key_with_the_code_in_the_body_only() {
    let relay = ScriptedRelay::start(Script::default().http(|request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/api/invites/claim") if request.json()["code"] == "v2.good" => {
                HttpReply::json(200, &json!({"status": "joined", "role": "member"}))
            }
            ("POST", "/api/invites/claim") => {
                HttpReply::json(403, &json!({"error": "invite_expired"}))
            }
            _ => default_http(request, &nip11(true)),
        }
    }))
    .await;
    let port = relay.http_url.port().unwrap();
    let Address::Invite {
        relay: address,
        code,
    } = join::parse_address(&format!("http://localhost:{port}/invite/v2.good")).unwrap()
    else {
        panic!("an invite");
    };
    let keys = Keys::generate();
    let signer = SignerHandle::spawn(keys.clone());
    assert_eq!(
        join::claim_invite(&address, &signer, &code).await,
        Claimed::Member
    );
    let expired = join::InviteCode::new("v2.old");
    assert!(matches!(
        join::claim_invite(&address, &signer, &expired).await,
        Claimed::Refused(reason) if reason.contains("expired")
    ));
    let claims = relay
        .http_requests()
        .into_iter()
        .filter(|request| request.path == "/api/invites/claim")
        .collect::<Vec<_>>();
    assert_eq!(claims.len(), 2);
    assert!(claims[0].query.is_empty(), "the code is never in the URL");
    let auth = claims[0].header("authorization").unwrap();
    let event = nostr::Event::from_json(
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(auth.strip_prefix("Nostr ").unwrap())
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    event.verify().unwrap();
    assert_eq!(event.pubkey, keys.public_key(), "signed by the joining key");
    assert!(!auth.contains("v2.good"));
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn a_community_is_written_with_its_relay_key_pinned_and_taken_back_on_failure() {
    let temporary = tempfile::TempDir::new().unwrap();
    let paths = bzz::paths::Paths {
        config_dir: temporary.path().join("c"),
        data_dir: temporary.path().join("d"),
        cache_dir: temporary.path().join("k"),
    };
    paths.ensure().unwrap();
    let mut config = bzz::config::Config::default();
    let secret = secrecy::SecretString::from("a join test passphrase".to_owned());
    let manager = bzz::auth::IdentityManager::new(&paths);
    let identity = manager
        .create(
            &mut config,
            "my-team".into(),
            bzz::config::KeyBackend::EncryptedFile,
            Some(&secret),
        )
        .unwrap();
    manager.save_new(&mut config, &identity).unwrap();
    let Address::Relay(relay) = join::parse_address("localhost:3999").unwrap() else {
        panic!("a relay");
    };
    let id = join::add_community(
        &paths,
        &mut config,
        "My team".into(),
        &relay,
        identity.id,
        &"cd".repeat(32),
    )
    .unwrap();
    let saved = bzz::config::Config::load(&paths).unwrap();
    assert_eq!(saved.communities.len(), 1);
    assert_eq!(saved.communities[0].relay_url, "ws://localhost:3999/");
    assert!(saved.communities[0].allow_insecure_localhost);
    assert_eq!(saved.default_community, Some(id));
    assert_eq!(
        join::already_joined(&saved, &relay).as_deref(),
        Some("My team")
    );
    // The same relay a second time is refused, and nothing changes.
    assert!(
        join::add_community(
            &paths,
            &mut config,
            "again".into(),
            &relay,
            identity.id,
            "x"
        )
        .is_err()
    );
    assert_eq!(
        bzz::config::Config::load(&paths).unwrap().communities.len(),
        1
    );
    // An identity made for a join that is dropped goes with its key file.
    let mut config = bzz::config::Config::load(&paths).unwrap();
    let spare = manager
        .create(
            &mut config,
            "spare".into(),
            bzz::config::KeyBackend::EncryptedFile,
            Some(&secret),
        )
        .unwrap();
    manager.save_new(&mut config, &spare).unwrap();
    let keys = std::fs::read_dir(paths.keys_dir()).unwrap().count();
    manager.forget(&mut config, &spare).unwrap();
    assert_eq!(
        std::fs::read_dir(paths.keys_dir()).unwrap().count(),
        keys - 1
    );
    assert_eq!(
        bzz::config::Config::load(&paths).unwrap().identities.len(),
        1
    );
    // One that a community uses is not dropped.
    assert!(manager.forget(&mut config, &identity).is_err());
}
