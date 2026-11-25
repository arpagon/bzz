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
    #[cfg(not(feature = "apps"))]
    for command in [":apps", ":app open board"] {
        assert_eq!(super::refuses(&parse(command)), Some(super::NO_APPS_YET));
    }
    // The Board works as in a real community; nothing else of apps does.
    #[cfg(feature = "apps")]
    {
        for command in [
            ":apps",
            ":app open board",
            ":app close",
            ":app zoom",
            ":app brief",
        ] {
            assert_eq!(super::refuses(&parse(command)), None, "{command}");
        }
        for command in [
            ":app add ./mine",
            ":app new mine",
            ":app dev ./mine",
            ":app share",
            ":app unshare board",
            ":app pending",
            ":app approve board",
            ":app refuse board no",
            ":app rollback board",
            ":app disable board",
            ":app enable board",
            ":app remove board",
            ":app",
        ] {
            assert_eq!(
                super::refuses(&parse(command)),
                Some(super::APPS_NEED_A_COMMUNITY),
                "{command}"
            );
        }
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
    let (voices, people) = super::huddle();
    assert_eq!(voices.peers(), [1, 2, 3], "no line is the viewer's");
    assert_eq!(people.len(), 3);
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

/// A panic in the demo unwinds through its folder's guard; what a task
/// writes afterwards is removed once main's runtime has stopped.
#[test]
fn the_folder_goes_on_a_panic_and_after_the_runtime_stops() {
    let mut made = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let scratch = super::Scratch::create().unwrap();
        std::fs::create_dir_all(scratch.paths().cache_dir).unwrap();
        made = Some(scratch.0.clone());
        panic!("a generated failure");
    }));
    assert!(outcome.is_err());
    let made = made.unwrap();
    assert!(!made.exists(), "removed by the unwinding guard");

    // A late write recreates it; main's clean-up removes it again.
    std::fs::create_dir_all(made.join("cache")).unwrap();
    super::clean_up_after_exit();
    assert!(!made.exists());
}

/// Every notice of the demo is shown whole in the status line of an
/// 80-column terminal: the mode, the notice, the demo's mark and the hint
/// on one row.
#[test]
fn every_notice_of_the_demo_fits_an_80_column_status_line() {
    use unicode_width::UnicodeWidthStr as _;
    let notices = [
        super::NEEDS_A_COMMUNITY,
        super::HUDDLE_IS_A_RECORDING,
        super::NO_APPS_YET,
        #[cfg(feature = "apps")]
        super::APPS_NEED_A_COMMUNITY,
    ];
    for notice in notices {
        assert!(notice.width() <= super::NOTICE_COLUMNS, "{notice}");
        assert!(notice.starts_with("Demo"), "{notice}");
    }
    // What the row holds at 80 columns besides the notice today.
    let mode = " NORMAL ".width();
    let mark = format!(" {} ", super::STATUS_MARK).width();
    let media = "  ".width();
    let hint = " ? help · q quit ".width();
    assert!(mode + 1 + super::NOTICE_COLUMNS + mark + media + hint <= 80);
}

/// An answer is dated after what it answers, whatever the second of
/// either: the timeline orders by the second, then by id.
#[test]
fn an_answer_is_dated_after_what_it_answers() {
    let mut world = world::build(ORIGIN);
    let now = nostr::Timestamp::now().as_secs();
    for ahead in [0, 1, 5] {
        let said = buzz_sdk::build_message(world.team, "hello", None, &[], false, &[], &[])
            .unwrap()
            .custom_created_at(nostr::Timestamp::from(now + ahead))
            .sign_with_keys(&world.viewer)
            .unwrap();
        for (_, reply) in world.script.answer(&said) {
            let answer = reply.sign().unwrap();
            assert!(answer.created_at > said.created_at, "{ahead}");
        }
    }
    // A teammate waits long enough that the answer can be dated a second
    // before it is sent and still after what it answers: what the person
    // writes after reading it then comes after it too.
    let scout = world::build(ORIGIN)
        .events
        .iter()
        .find(|event| event.kind.as_u16() == 10_100)
        .unwrap()
        .pubkey
        .to_hex();
    let mut world = world::build(ORIGIN);
    for (text, mentions) in [("hi", vec![]), ("@Scout hi", vec![scout.as_str()])] {
        let said = viewer_message(&world, text, &mentions);
        let delay = world.script.answer(&said).last().unwrap().0;
        assert!(delay >= Duration::from_secs(2), "{text}");
    }
}

#[cfg(feature = "apps")]
mod board {
    use std::time::{Duration, Instant};

    use serde_json::Value;

    use super::super::board::{self, Ask, DemoBoard, People};
    use super::{ORIGIN, viewer_message, world};
    use crate::apps::runtime::Who;
    use crate::apps::runtime::headless::{Headless, HeadlessOptions};
    use crate::apps::store::{Apps, templates};

    fn people() -> People {
        let cast = world::Cast::default();
        let who = |keys: &nostr::Keys, name: &str| Who::new(keys.public_key().to_hex(), name);
        People {
            viewer: Who::new("f".repeat(64), "You"),
            ada: who(&cast.ada, "Ada"),
            lin: who(&cast.lin, "Lin"),
            sam: who(&cast.sam, "Sam"),
            scout: who(&cast.scout, "Scout"),
        }
    }

    fn column_of(store: &crate::apps::session::SharedStore, title: &str) -> Option<String> {
        store
            .lock()
            .find_by("title", title)
            .and_then(|card| card.content.get("column")?.as_str().map(str::to_owned))
    }

    /// The Board is the shipped template, proposed and approved in the
    /// demo's own store, and handed out by `Apps::runnable` as any app.
    #[test]
    fn the_board_is_the_approved_template_from_the_one_door() {
        let directory = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths {
            config_dir: directory.path().join("config"),
            data_dir: directory.path().join("data"),
            cache_dir: directory.path().join("cache"),
        };
        let mut config = crate::config::Config::default();
        let identity = crate::config::IdentityConfig {
            id: uuid::Uuid::new_v4(),
            label: "viewer".into(),
            pubkey: "f".repeat(64),
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
        let launch = uuid::Uuid::new_v4();
        let mut store = crate::store::Store::open_memory().unwrap();
        store.sync_config(&config).unwrap();
        board::install(&mut store, &paths, community, launch).unwrap();
        let apps = Apps::new(&mut store, &paths);
        let runnable = apps.runnable(community, board::NAME).unwrap();
        let template = templates::find("board").unwrap();
        assert_eq!(runnable.script(), template.script.as_bytes());
        assert_eq!(runnable.title(), "Board");
        assert_eq!(runnable.types(), ["card"]);
        assert_eq!(apps.open_in_channel(community, launch).unwrap(), ["board"]);
        // Its files are in the demo's folder, nowhere else.
        assert!(
            apps.app_dir(community, board::NAME)
                .starts_with(directory.path())
        );
    }

    /// On the seeded cards: `n`, a title, Enter, `]`, and the new card is
    /// in Doing; the cards of the story are there.
    #[test]
    fn a_card_added_and_moved_is_in_the_next_column() {
        let people = people();
        let clock = crate::apps::runtime::ManualClock::new(1_800_000_000);
        let store = board::seeded(&people, clock.clone());
        let template = templates::find("board").unwrap();
        let mut app = Headless::start_on(
            template.script,
            HeadlessOptions::new("board.lua").user(people.viewer.clone()),
            store,
            clock,
        )
        .unwrap();
        let screen = app.screen();
        for title in ["Launch tweet", "Blog post review", "2.4-rc2 on staging"] {
            assert!(screen.contains(title), "{screen}");
        }
        assert!(screen.contains("To do (3)"), "{screen}");
        app.key("n").type_text("Press kit").key("enter");
        assert!(app.screen().contains("To do (4)"), "{}", app.screen());
        app.key("]");
        let screen = app.screen();
        assert!(screen.contains("Doing (3)"), "{screen}");
        let card = app.store().find_by("title", "Press kit").unwrap();
        assert_eq!(card.content.get("column"), Some(&Value::from("doing")));
        assert_eq!(card.by.id, people.viewer.id);
    }

    /// A teammate moves a card while the Board is on screen: the first a
    /// while after it is first shown, then at most once a minute, never
    /// while the person types in it, and says so.
    #[test]
    fn a_teammate_moves_a_card_calmly_and_not_while_the_person_types() {
        let people = people();
        let store = board::records(&people);
        let said = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let heard = said.clone();
        let mut demo =
            DemoBoard::new(store.clone(), people.clone()).announcing(move |who, text| {
                heard
                    .lock()
                    .unwrap()
                    .push((who.name.clone(), text.to_owned()));
            });
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        // Not on screen: nothing, however long.
        assert!(!demo.tick(false, at(0)));
        assert!(!demo.tick(false, at(600)));
        // First shown at 600; the first move 15 s later.
        assert!(!demo.tick(true, at(600)));
        assert!(!demo.tick(true, at(614)));
        assert!(demo.tick(true, at(615)));
        assert_eq!(
            column_of(&store, "Blog post review").as_deref(),
            Some("done")
        );
        assert_eq!(
            said.lock().unwrap().as_slice(),
            [(
                "Lin".to_owned(),
                "Moved \"Blog post review\" to Done on the Board.".to_owned()
            )]
        );
        // Not again within a minute.
        for second in 616..675 {
            assert!(!demo.tick(true, at(second)), "{second}");
        }
        // The person types at 670: nothing until ten seconds after.
        demo.typed(at(670));
        assert!(!demo.tick(true, at(675)));
        assert!(!demo.tick(true, at(679)));
        assert!(demo.tick(true, at(680)));
        assert_eq!(
            column_of(&store, "On-call handover").as_deref(),
            Some("doing")
        );
        // A move whose card the person moved already is skipped.
        {
            let held = store.lock();
            let card = held.find_by("title", "Changelog TODO").unwrap();
            drop(held);
            let mut content = card.content.clone();
            content.insert("column".into(), "done".into());
            let told = store
                .lock()
                .remote_update(&people.viewer, &card.id, content);
            store.tell(told);
        }
        assert!(!demo.tick(true, at(800)));
        assert_eq!(column_of(&store, "Changelog TODO").as_deref(), Some("done"));
        assert_eq!(said.lock().unwrap().len(), 2);
        // Then the team leaves the board alone.
        assert!(!demo.tick(true, at(2000)));
    }

    #[test]
    fn scout_reads_add_and_move_and_nothing_else() {
        assert_eq!(
            board::ask("@Scout add Ship party"),
            Some(Ask::Add("Ship party".into()))
        );
        assert_eq!(
            board::ask("@Scout please add a card: Order cake."),
            Some(Ask::Add("Order cake".into()))
        );
        assert_eq!(
            board::ask("nostr:npub1xyz move launch tweet to Done"),
            Some(Ask::Move {
                title: "launch tweet".into(),
                to: Some("done".into())
            })
        );
        assert_eq!(
            board::ask("@Scout move the blog post"),
            Some(Ask::Move {
                title: "blog post".into(),
                to: None
            })
        );
        assert_eq!(board::ask("@Scout what is on the board?"), Some(Ask::Other));
        assert_eq!(board::ask("@Scout what do you do?"), None);
        // A verb that is not the first word is not an order.
        assert_eq!(board::ask("@Scout can you add more tests?"), None);
    }

    #[test]
    fn scout_adds_and_moves_cards_and_says_what_it_did() {
        let people = people();
        let store = board::records(&people);
        let said = board::scout(&store, &people.scout, &Ask::Add("Ship party".into()));
        assert_eq!(
            said,
            "Done: I added \"Ship party\" to To do on the Board in #launch."
        );
        let card = store.lock().find_by("title", "Ship party").unwrap();
        assert_eq!(card.by.name, "Scout");
        assert_eq!(column_of(&store, "Ship party").as_deref(), Some("todo"));
        let said = board::scout(
            &store,
            &people.scout,
            &Ask::Move {
                title: "launch tweet".into(),
                to: None,
            },
        );
        assert_eq!(
            said,
            "Done: I moved \"Launch tweet\" from To do to Doing on the Board in #launch."
        );
        let said = board::scout(
            &store,
            &people.scout,
            &Ask::Move {
                title: "Dark theme".into(),
                to: None,
            },
        );
        assert!(said.contains("in Done already"), "{said}");
        let said = board::scout(
            &store,
            &people.scout,
            &Ask::Move {
                title: "nonsense".into(),
                to: Some("done".into()),
            },
        );
        assert!(
            said.starts_with("I found no card called \"nonsense\""),
            "{said}"
        );
        assert!(said.contains(board::SCOUT_CAN), "{said}");
        assert_eq!(
            board::scout(&store, &people.scout, &Ask::Other),
            board::SCOUT_CAN
        );
        let said = board::scout(&store, &people.scout, &Ask::Add(String::new()));
        assert!(said.starts_with("Which card?"), "{said}");
    }

    /// Through the relay's script: a mention that asks Scout to add a card
    /// adds it when Scout answers, in the thread of the mention.
    #[test]
    fn a_mention_that_asks_scout_writes_on_the_board_when_it_answers() {
        let people = people();
        let store = board::records(&people);
        let mut world = world::build(ORIGIN);
        world.script.set_board(store.clone(), people.scout.clone());
        let asked = viewer_message(&world, "@Scout add Order cake", &[]);
        let mut answers = world.script.answer(&asked);
        assert_eq!(answers.len(), 3);
        // Nothing is written before Scout answers.
        assert!(store.lock().find_by("title", "Order cake").is_none());
        let answer = answers.pop().unwrap().1.sign().unwrap();
        assert_eq!(
            answer.content,
            "Done: I added \"Order cake\" to To do on the Board in #launch."
        );
        assert_eq!(column_of(&store, "Order cake").as_deref(), Some("todo"));
        // Anything else gets Scout's usual answer, which says what it can do here.
        let other = viewer_message(&world, "@Scout what do you do?", &[]);
        let answer = world.script.answer(&other).pop().unwrap().1.sign().unwrap();
        assert_eq!(answer.content, world::SCOUT_ANSWER);
        assert!(answer.content.contains("@Scout add"));
    }

    /// Ada's welcome names the Board and its keys, and the words to ask
    /// Scout; the Board's keys are the template's.
    #[test]
    fn the_welcome_names_the_board_its_keys_and_the_words_for_scout() {
        for said in [
            "Board in #launch",
            "`5` opens it",
            "`n` adds a card",
            "`]` moves",
            "@Scout add",
        ] {
            assert!(world::WELCOME.contains(said), "{said}");
        }
        let brief = templates::find("board").unwrap().brief;
        assert!(brief.contains("`n` opens a form"));
        assert!(brief.contains("`]` and `[`"));
        assert!(board::ask("@Scout add Ship party").is_some());
    }
}
