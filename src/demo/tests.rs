use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{EventBuilder, JsonUtil as _, Kind, Tag};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
};
use tokio_tungstenite::tungstenite::Message;

use super::{relay::Relay, world};
use crate::ui::command::parse;

const ORIGIN: &str = "http://127.0.0.1:4242";

/// Every address of the world is the demo relay's own loopback origin, and
/// nothing of this machine or its owner is in it.
#[test]
fn the_world_names_no_address_outside_loopback() {
    let world = world::build(ORIGIN);
    let text = world
        .events
        .iter()
        .map(|event| event.as_json())
        .collect::<Vec<_>>()
        .join("\n");
    let mut addresses = 0;
    for (at, _) in text.match_indices("://") {
        let start = text[..at]
            .rfind(|character: char| !character.is_ascii_alphanumeric())
            .map_or(0, |index| index + 1);
        let address = &text[start..];
        assert!(
            address.starts_with(&format!("{ORIGIN}/media/")),
            "{}",
            &address[..address.len().min(60)]
        );
        addresses += 1;
    }
    assert_eq!(addresses, 5, "one picture for each person");
    // No dotted address but loopback, and no host name.
    for word in
        text.split(|character: char| !(character.is_ascii_alphanumeric() || character == '.'))
    {
        let parts = word.split('.').collect::<Vec<_>>();
        if parts.len() == 4 && parts.iter().all(|part| part.parse::<u8>().is_ok()) {
            assert_eq!(word, "127.0.0.1");
        }
        for ending in [".com", ".dev", ".xyz", ".org", ".net", ".io", ".local"] {
            assert!(!word.ends_with(ending), "{word}");
        }
    }
    for name in ["USER", "LOGNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(name)
            && value.len() > 3
        {
            assert!(!text.contains(&value), "{name}");
        }
    }
    if let Ok(host) = std::fs::read_to_string("/etc/hostname") {
        let host = host.trim();
        if host.len() > 3 {
            assert!(!text.contains(host));
        }
    }
}

#[test]
fn the_same_team_on_every_run_and_a_new_viewer() {
    let one = world::build(ORIGIN);
    let two = world::build(ORIGIN);
    assert_eq!(one.team, two.team);
    assert_ne!(one.viewer.public_key(), two.viewer.public_key());
    let authors = |world: &world::World| {
        world
            .events
            .iter()
            .filter(|event| event.kind.as_u16() == 9)
            .map(|event| (event.pubkey, event.content.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(authors(&one), authors(&two));
    // The newest message of #team says what to try.
    let newest = one
        .events
        .iter()
        .filter(|event| {
            event.kind.as_u16() == 9 && world::first(event, "h") == Some(one.team.to_string())
        })
        .max_by_key(|event| event.created_at)
        .unwrap();
    assert_eq!(newest.content, world::WELCOME);
}

/// Scout is a verified agent of `#team` in the seeded store: it carries
/// the agent mark and may show as typing.
#[test]
fn scout_is_a_verified_agent_in_the_seeded_store() {
    let world = world::build(ORIGIN);
    let directory = tempfile::TempDir::new().unwrap();
    let mut config = crate::config::Config::default();
    let identity = crate::config::IdentityConfig {
        id: uuid::Uuid::new_v4(),
        label: "viewer".into(),
        pubkey: world.viewer.public_key().to_hex(),
        backend: crate::config::KeyBackend::EncryptedFile,
        key_ref: "demo".into(),
    };
    config.identities.push(identity.clone());
    config
        .add_community(
            "DEMO".into(),
            "ws://127.0.0.1:4242/".into(),
            identity.id,
            true,
        )
        .unwrap();
    let community = config.communities[0].id;
    let mut store = crate::store::Store::open(directory.path().join("bzz.db")).unwrap();
    store.sync_config(&config).unwrap();
    store
        .pin_relay_pubkey(community, &world.relay_keys.public_key().to_hex())
        .unwrap();
    for event in &world.events {
        store.apply_event(community, event).unwrap();
    }
    store.reconcile_remote_agents(community).unwrap();
    let agents = store
        .list_remote_agents(community, &identity.pubkey)
        .unwrap();
    assert_eq!(agents.len(), 1, "{agents:?}");
    let scout = &agents[0];
    assert_eq!(scout.name, "Scout");
    assert!(!scout.stale);
    assert!(scout.channel_ids.contains(&world.team));
    assert_eq!(scout.eligibility, crate::agents::Eligibility::Eligible);
    let team = store
        .channels(community)
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == world.team)
        .unwrap();
    assert_eq!(team.name, "team");
}

fn viewer_message(world: &world::World, text: &str, mentions: &[&str]) -> nostr::Event {
    buzz_sdk::build_message(world.team, text, None, mentions, false, &[], &[])
        .unwrap()
        .sign_with_keys(&world.viewer)
        .unwrap()
}

#[test]
fn scout_types_then_answers_in_the_thread_of_its_mention() {
    let mut world = world::build(ORIGIN);
    let scout = world::build(ORIGIN)
        .events
        .iter()
        .find(|event| event.kind.as_u16() == 10_100)
        .unwrap()
        .pubkey
        .to_hex();
    let asked = viewer_message(&world, "@Scout what do you do?", &[scout.as_str()]);
    let answers = world.script.answer(&asked);
    let mut delays = answers.iter().map(|(delay, _)| *delay).collect::<Vec<_>>();
    let events = answers
        .into_iter()
        .map(|(_, reply)| reply.sign().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_u16())
            .collect::<Vec<_>>(),
        [20_002, 20_002, 9]
    );
    assert!(events.iter().all(|event| event.pubkey.to_hex() == scout));
    let answer = &events[2];
    assert_eq!(answer.content, world::SCOUT_ANSWER);
    assert!(answer.tags.iter().any(|tag| {
        let values = tag.as_slice();
        values.first().map(String::as_str) == Some("e") && values.get(1) == Some(&asked.id.to_hex())
    }));
    // It types first, where the viewer looks, and answers after.
    let typing_in_thread =
        crate::agents::typing::parse(&events[1], world.team, nostr::Timestamp::now().as_secs())
            .unwrap();
    assert_eq!(
        typing_in_thread.scope,
        crate::agents::typing::TypingScope::Thread(asked.id.to_hex())
    );
    let typing_in_channel =
        crate::agents::typing::parse(&events[0], world.team, nostr::Timestamp::now().as_secs())
            .unwrap();
    assert_eq!(
        typing_in_channel.scope,
        crate::agents::typing::TypingScope::Channel
    );
    delays.dedup();
    assert!(delays[0] < delays[1]);
}

#[test]
fn teammates_answer_a_few_times_then_go_quiet() {
    let mut world = world::build(ORIGIN);
    let mut answered = 0;
    for turn in 0..8 {
        let said = viewer_message(&world, &format!("hello {turn}"), &[]);
        answered += world.script.answer(&said).len();
    }
    assert_eq!(answered, 4);
    // What others write is never answered.
    let other = buzz_sdk::build_message(world.team, "hi", None, &[], false, &[], &[])
        .unwrap()
        .sign_with_keys(&nostr::Keys::generate())
        .unwrap();
    assert!(world.script.answer(&other).is_empty());
}

#[test]
fn what_needs_a_real_community_says_so() {
    for command in [
        ":community add",
        ":community remove",
        ":signin",
        ":broker",
        ":agent new Mine",
        ":agents local",
    ] {
        assert_eq!(
            super::refuses(&parse(command)),
            Some(super::NEEDS_A_COMMUNITY),
            "{command}"
        );
    }
    for command in [":apps", ":app open board"] {
        assert_eq!(super::refuses(&parse(command)), Some(super::NO_APPS_YET));
    }
    for command in [
        ":huddle join",
        ":huddle start",
        ":huddle devices",
        ":huddle mute",
    ] {
        assert_eq!(
            super::refuses(&parse(command)),
            Some(super::HUDDLE_IS_A_RECORDING),
            "{command}"
        );
    }
    for command in [
        ":huddle",
        ":agents",
        ":inbox",
        ":search",
        ":theme reload",
        ":status lunch",
    ] {
        assert_eq!(super::refuses(&parse(command)), None, "{command}");
    }
    assert!(super::huddle_levels().is_empty());
}

async fn http(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reply = vec![0_u8; 64 * 1024];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut reply))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&reply[..read]).into_owned()
}

/// Only the path with the token reaches the relay, and media only with the
/// viewer's signature.
#[tokio::test]
async fn the_relay_serves_only_the_token_and_the_viewer() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let world = world::build(&format!("http://127.0.0.1:{port}"));
    let viewer = world.viewer.clone();
    let team = world.team;
    let picture = world.media.keys().next().unwrap().clone();
    Relay::new(world, "secret".into()).serve(listener);

    let info = http(port, "GET /secret/ HTTP/1.1\r\nHost: x\r\n\r\n").await;
    assert!(info.starts_with("HTTP/1.1 200"), "{info}");
    assert!(info.contains("\"supported_nips\""));
    let wrong = http(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").await;
    assert!(wrong.starts_with("HTTP/1.1 404"), "{wrong}");
    let body = json!([{"kinds": [9], "#h": [team.to_string()], "search": "load test"}]).to_string();
    let query = |path: &str| {
        format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    };
    assert!(
        http(port, &query("/query"))
            .await
            .starts_with("HTTP/1.1 404")
    );
    let found = http(port, &query("/secret/query")).await;
    assert!(found.starts_with("HTTP/1.1 200"), "{found}");
    assert!(found.contains("Load test passed"), "{found}");
    assert!(!found.contains("Morning"), "{found}");

    let media = format!("GET /media/{picture} HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(http(port, &media).await.starts_with("HTTP/1.1 401"));
    let authorization = EventBuilder::new(Kind::Custom(24_242), "Get buzz-media")
        .tags([Tag::parse(["t", "get"]).unwrap()])
        .sign_with_keys(&viewer)
        .unwrap();
    let signed = format!(
        "GET /media/{picture} HTTP/1.1\r\nHost: x\r\nAuthorization: Nostr {}\r\n\r\n",
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            authorization.as_json()
        )
    );
    assert!(http(port, &signed).await.starts_with("HTTP/1.1 200"));
    let stranger = EventBuilder::new(Kind::Custom(24_242), "Get buzz-media")
        .sign_with_keys(&nostr::Keys::generate())
        .unwrap();
    let forged = signed.replace(
        signed
            .split("Nostr ")
            .nth(1)
            .unwrap()
            .split("\r\n")
            .next()
            .unwrap(),
        &base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            stranger.as_json(),
        ),
    );
    assert!(http(port, &forged).await.starts_with("HTTP/1.1 401"));

    // The WebSocket, only under the token.
    assert!(
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
            .await
            .is_err()
    );
    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/secret/"))
            .await
            .unwrap();
    let challenge = socket.next().await.unwrap().unwrap();
    assert!(challenge.to_text().unwrap().starts_with("[\"AUTH\""));
    // A message the viewer writes is stored, sent back live, and answered.
    let request = json!(["REQ", "live", {"kinds": [9], "#h": [team.to_string()], "limit": 0}]);
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();
    let said = buzz_sdk::build_message(team, "hello team", None, &[], false, &[], &[])
        .unwrap()
        .sign_with_keys(&viewer)
        .unwrap();
    socket
        .send(Message::Text(json!(["EVENT", said]).to_string().into()))
        .await
        .unwrap();
    let mut seen = Vec::new();
    while seen.len() < 4 {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        seen.push(value);
    }
    assert_eq!(seen[0][0], "EOSE");
    assert!(
        seen.iter()
            .any(|frame| frame[0] == "OK" && frame[2] == true)
    );
    let contents = seen
        .iter()
        .filter(|frame| frame[0] == "EVENT")
        .map(|frame| frame[2]["content"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(contents.len(), 2, "{seen:?}");
    assert_eq!(contents[0], "hello team");
    // A stranger's event is refused.
    let stranger = buzz_sdk::build_message(team, "let me in", None, &[], false, &[], &[])
        .unwrap()
        .sign_with_keys(&nostr::Keys::generate())
        .unwrap();
    socket
        .send(Message::Text(json!(["EVENT", stranger]).to_string().into()))
        .await
        .unwrap();
    let refused = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let refused: Value = serde_json::from_str(refused.to_text().unwrap()).unwrap();
    assert_eq!(refused[0], "OK");
    assert_eq!(refused[2], false);
}

#[test]
fn a_folder_left_by_a_killed_demo_is_removed_and_a_running_one_kept() {
    let directory = tempfile::TempDir::new().unwrap();
    let own = directory
        .path()
        .join(format!("bzz-demo-{}-aaaa", std::process::id()));
    // No process has this id: Linux caps ids far below it.
    let dead = directory.path().join("bzz-demo-2147483600-bbbb");
    let other = directory.path().join("bzz-demo-notes");
    for folder in [&own, &dead, &other] {
        std::fs::create_dir(folder).unwrap();
    }
    std::fs::write(directory.path().join("bzz-demo-file"), "").unwrap();
    let mut found = super::demo_folders(directory.path());
    found.sort();
    assert_eq!(found.len(), 3, "a file is not a demo folder");
    assert!(!super::left_behind(&own));
    #[cfg(unix)]
    {
        assert!(super::left_behind(&dead));
        // A name without a process id is left alone while it is new.
        assert!(!super::left_behind(&other));
    }
}
