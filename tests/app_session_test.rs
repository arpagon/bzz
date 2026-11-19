//! An app session on a scripted relay (v0.16 M2, the join): a script, the
//! runtime, the session, the real records service, supervisor and store,
//! against a relay that says what the pinned relay was measured to say.
//! Driven by keys and read as screens, as the app's pane will. Real
//! sockets, so it runs on every platform; the pinned relay has its own
//! tests in `tests/relay_integration.rs`.
#![cfg(feature = "apps")]
mod support;

use std::time::{Duration, Instant};

use bzz::apps::runtime::Phase;
use bzz::protocol::record::{APP_WRITES_PER_MINUTE, Fence};
use bzz::service::records::SyncState;
use bzz::store::Store;
use nostr::{Event, Keys};
use serde_json::{Value, json};
use support::app_pane::{Pane, last_row};
use support::records_world::*;
use uuid::Uuid;

const BOARD: &str = include_str!("fixtures/apps/board/app.lua");

/// A script that writes as many records at once as it is pasted a number,
/// and counts what it is answered.
const BURST: &str = r#"
local app = { api = 1, types = { "card" } }
function app.init(ctx) return { ok = 0, waits = 0, other = 0, wait = "" } end
function app.view(s, ctx)
  return ui.text { "stored " .. s.ok, "told to wait " .. s.waits .. s.wait, "other " .. s.other }
end
function app.paste(s, text, ctx)
  for i = 1, tonumber(text) do data.create { type = "card", content = { title = "card " .. i } } end
end
function app.reply(s, r, ctx)
  if r.ok then s.ok = s.ok + 1
  elseif r.error.code == "rate_limited" then
    s.waits = s.waits + 1
    s.wait = " (" .. (r.error.retry_after > 0 and r.error.retry_after <= 60 and "1 to 60 s" or "?") .. ")"
  else s.other = s.other + 1 end
end
return app
"#;

/// A card of the Board, written by `keys`.
fn board_card(fence: &Fence, keys: &Keys, title: &str, column: &str, order: u32) -> Event {
    sign(
        fence
            .create(
                "card",
                Uuid::new_v4(),
                &object(json!({"title": title, "column": column, "order": order})),
            )
            .unwrap(),
        keys,
    )
}

fn revised(fence: &Fence, keys: &Keys, of: &Event, content: Value) -> Event {
    sign(fence.update(&head(of), &object(content)).unwrap(), keys)
}

fn three_cards(world: &World, other: &Keys) -> Vec<Event> {
    vec![
        board_card(&world.fence, other, "Alpha", "todo", 1),
        board_card(&world.fence, other, "Bravo", "todo", 2),
        board_card(&world.fence, other, "Delta", "doing", 3),
    ]
}

#[tokio::test]
async fn a_resync_changes_nothing_on_the_screen() {
    let world = world().await;
    let other = Keys::generate();
    let cards = three_cards(&world, &other);
    world.script().syncs.push_back(Sync::Set(cards.clone()));
    let mut pane = Pane::open(&world.service, world.channel, "board", BOARD);
    let first = pane.settled("Loaded 3 cards.").await;
    assert!(
        first.contains("To do (2)") && first.contains("> Alpha") && first.contains("Doing (1)"),
        "{first}"
    );
    assert_eq!(pane.status().sync, SyncState::Live);
    assert_eq!(pane.status().notice, None);
    // The selection moves, so that a list sent again would show.
    pane.key("j");
    let before = pane
        .until("the selection on Bravo", |screen, _| {
            screen.contains("> Bravo")
        })
        .await;

    // 1. The relay ends the sync, as a deleted chat message makes it, and
    // sends the same set again. Every screen drawn meanwhile is the same
    // screen, and the pane says nothing.
    world.script().syncs.push_back(Sync::Set(cards.clone()));
    world.push(|subscription| json!(["CLOSED", subscription, "resync: redaction"]));
    let mut seen = Vec::new();
    pane.until("the records to be live again", |screen, status| {
        assert_eq!(screen, before, "the screen during a resync");
        assert_eq!(status.notice, None);
        if seen.last() != Some(&status.sync_words) {
            seen.push(status.sync_words.clone());
        }
        seen.len() == 3
    })
    .await;
    assert_eq!(seen, ["live", "syncing again", "live"]);
    assert_eq!(world.requests().len(), 2, "bzz asked again once");
    // Quiet afterwards too.
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(pane.screen(), before);
    }

    // 2. This time the set differs: Alpha was moved, Bravo is gone, Echo
    // is new, Delta is as it was. The screen shows exactly that.
    let alpha = revised(
        &world.fence,
        &other,
        &cards[0],
        json!({"title": "Alpha", "column": "done", "order": 1}),
    );
    let echo = board_card(&world.fence, &other, "Echo", "todo", 5);
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![alpha, cards[2].clone(), echo]));
    world.push(|subscription| json!(["CLOSED", subscription, "resync: channel access changed"]));
    let after = pane
        .until("the differences", |screen, status| {
            status.sync == SyncState::Live && screen.contains("Done (1)")
        })
        .await;
    for shown in [
        "To do (1)",
        "> Echo",
        "Doing (1)",
        "Delta",
        "Done (1)",
        "Alpha",
    ] {
        assert!(after.contains(shown), "{shown}:\n{after}");
    }
    assert!(!after.contains("Bravo"), "{after}");
    // The script was not made to load again: its own status line stands.
    assert_eq!(last_row(&after), "Loaded 3 cards.");
    assert!(world.writes().is_empty());
    drop(pane);
    world.stop().await;
}

#[tokio::test]
async fn too_many_restricted_and_archived_each_show_their_sentence() {
    // 1. More records than one sync returns.
    let world = world().await;
    let other = Keys::generate();
    world.script().syncs.push_back(Sync::Closed(
        three_cards(&world, &other),
        "error: sync set exceeds max_events",
    ));
    let mut pane = Pane::open(&world.service, world.channel, "board", BOARD);
    pane.until("too many", |_, status| {
        matches!(status.sync, SyncState::TooMany { .. })
    })
    .await;
    assert_eq!(
        pane.status().notice.as_deref(),
        Some("This app has more than 4096 records here, too many to load. None can be added.")
    );
    // The part of the set that arrived is not shown as if it were the set.
    let screen = pane.settled("Loaded 0 cards.").await;
    assert!(screen.contains("To do (0)"), "{screen}");
    // A new card is refused by bzz, in the app's own words, without a frame.
    pane.key("n");
    pane.type_text("One more");
    pane.key("enter");
    pane.settled("The board is full.").await;
    assert!(world.writes().is_empty(), "no frame");
    assert_eq!(
        world.requests().len(),
        1,
        "bzz does not ask again by itself"
    );
    drop(pane);
    world.stop().await;

    // 2. No access to the channel's records.
    let world = support::records_world::world().await;
    world
        .script()
        .syncs
        .push_back(Sync::Closed(Vec::new(), "restricted: not a channel member"));
    let mut pane = Pane::open(&world.service, world.channel, "board", BOARD);
    pane.until("no access", |_, status| {
        status.sync == SyncState::Restricted
    })
    .await;
    assert_eq!(
        pane.status().notice.as_deref(),
        Some("You cannot read this channel's records: this app cannot be used here.")
    );
    pane.settled("Load failed. Press r to retry.").await;
    pane.key("n");
    pane.type_text("Mine");
    pane.key("enter");
    let screen = pane
        .until("the refusal", |screen, _| {
            last_row(screen).starts_with("Refused: you cannot read this channel")
        })
        .await;
    assert!(screen.contains("New card"), "the form stays:\n{screen}");
    assert!(world.writes().is_empty(), "no frame");
    drop(pane);
    world.stop().await;

    // 3. An archived channel. bzz did not know: the relay refuses the
    // write, and from then on the pane says so.
    let world = support::records_world::world().await;
    let other = Keys::generate();
    world
        .script()
        .syncs
        .push_back(Sync::Set(three_cards(&world, &other)));
    world
        .script()
        .answers
        .push_back(Answer::refuse("invalid: channel is archived"));
    let mut pane = Pane::open(&world.service, world.channel, "board", BOARD);
    pane.settled("Loaded 3 cards.").await;
    assert_eq!(pane.status().notice, None);
    pane.key("]");
    let screen = pane
        .until("the relay's refusal", |screen, _| {
            last_row(screen).starts_with("Refused: this channel is archived")
        })
        .await;
    // The card is back in its column.
    assert!(
        screen.contains("To do (2)") && screen.contains("> Alpha"),
        "{screen}"
    );
    assert_eq!(
        pane.status().notice.as_deref(),
        Some("This channel is archived: its records can be read, not changed.")
    );
    assert_eq!(world.writes().len(), 1);
    // bzz knows now (its owner is told by the channel's own state): the
    // next write is refused without a frame.
    pane.session.set_archived(true);
    pane.key("j");
    pane.key("]");
    pane.until("the second refusal", |screen, _| {
        screen.contains("> Bravo")
            && last_row(screen).starts_with("Refused: this channel is archived")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(world.writes().len(), 1, "no second frame");
    // Reading goes on.
    pane.key("r");
    pane.settled("Loaded 3 cards.").await;
    assert_eq!(
        pane.status().notice.as_deref(),
        Some("This channel is archived: its records can be read, not changed.")
    );
    drop(pane);
    world.stop().await;
}

#[tokio::test]
async fn a_message_of_the_person_is_not_delayed_by_an_apps_writes() {
    let message = |world: &World, text: &str| {
        buzz_sdk::build_message(world.channel, text, None, &[], false, &[], &[])
            .unwrap()
            .sign_with_keys(&world.keys)
            .unwrap()
    };
    let cap = APP_WRITES_PER_MINUTE as usize;

    // 1. A script asks for forty writes at once. Thirty are sent, ten are
    // answered `rate_limited` by bzz: the relay's budget for the person is
    // sixty, and the other thirty stay for the person's chat. A message
    // sent while the app's writes wait goes out before nearly all of them.
    let world = world().await;
    let mut pane = Pane::open(&world.service, world.channel, "burst", BURST);
    pane.until("the app", |screen, status| {
        screen.starts_with("stored 0") && status.sync == SyncState::Live
    })
    .await;
    assert!(pane.session.paste("40"));
    pane.until("the first write on the wire", |_, _| {
        !world.writes().is_empty()
    })
    .await;
    let first = message(&world, "first");
    let ack = world.supervisor.publish(first.clone()).await.unwrap();
    assert!(ack.accepted);
    let frames = world.relay.frames_of("EVENT");
    let at = frames
        .iter()
        .position(|frame| frame.value[1]["id"] == first.id.to_hex())
        .expect("the message reached the relay");
    let before = frames[..at]
        .iter()
        .filter(|frame| frame.value[1]["kind"] == 45_010)
        .count();
    assert!(
        before <= 3,
        "{before} of the app's writes went out before the person's message"
    );
    let screen = pane
        .until("every answer", |screen, _| {
            screen.starts_with(&format!("stored {cap}\ntold to wait 10 (1 to 60 s)"))
        })
        .await;
    assert!(screen.contains("other 0"), "{screen}");
    assert_eq!(
        world.writes().len(),
        cap,
        "frames of the app in this minute"
    );
    let status = pane.status();
    assert!(
        status
            .wait
            .is_some_and(|seconds| (1..=60).contains(&seconds)),
        "{status:?}"
    );
    let notice = status.notice.expect("the pane says the wait");
    assert!(
        notice.starts_with("Too many changes: this app can write again in "),
        "{notice}"
    );
    // More asked for is more refused, and still nothing is sent.
    assert!(pane.session.paste("5"));
    pane.until("five more refusals", |screen, _| {
        screen.contains("told to wait 15")
    })
    .await;
    assert_eq!(world.writes().len(), cap);
    // The person's next messages are sent as if no app were open.
    for n in 0..5 {
        let started = Instant::now();
        let ack = world
            .supervisor
            .publish(message(&world, &format!("after {n}")))
            .await
            .unwrap();
        assert!(ack.accepted);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    drop(pane);
    world.stop().await;

    // 2. The relay stops acknowledging the app's writes: one is on the
    // wire without an answer and four wait behind it. The person's message
    // is sent and acknowledged at once.
    let world = support::records_world::world().await;
    world.script().mute_records = true;
    let mut pane = Pane::open(&world.service, world.channel, "burst", BURST);
    pane.until("the app", |screen, status| {
        screen.starts_with("stored 0") && status.sync == SyncState::Live
    })
    .await;
    assert!(pane.session.paste("5"));
    pane.until("an unanswered write on the wire", |_, _| {
        world.writes().len() == 1
    })
    .await;
    let started = Instant::now();
    let ack = tokio::time::timeout(
        Duration::from_secs(3),
        world.supervisor.publish(message(&world, "second")),
    )
    .await
    .expect("the message did not wait for the app's write")
    .unwrap();
    assert!(ack.accepted);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        world.writes().len(),
        1,
        "the app's other writes wait for its own"
    );
    // And the app's pane still answers: its owner was never made to wait.
    assert!(pane.screen().starts_with("stored 0"));
    assert_eq!(pane.status().phase, Phase::Running);
    drop(pane);
    world.stop().await;
}

/// The resident memory of this process, in bytes. Linux only.
fn resident() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// Time and memory of a sync of 1000 records through a session: a
/// measurement, with bounds wide enough for a loaded machine. Run it alone
/// and with `--nocapture` to read the numbers:
/// `cargo test --test app_session_test a_sync_of_a_thousand -- --nocapture`.
#[tokio::test]
async fn a_sync_of_a_thousand_records_through_a_session() {
    let keys = Keys::generate();
    let relay_keys = Keys::generate();
    let (config, community) = config(&keys);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("records.sqlite3");
    let open_store = || {
        let mut store = Store::open(&path).unwrap();
        store.sync_config(&config).unwrap();
        store
    };
    let other = Keys::generate();
    let world = world_on(
        keys.clone(),
        relay_keys.clone(),
        community,
        open_store(),
        Scripted::default(),
    )
    .await;
    let columns = ["todo", "doing", "done"];
    let cards: Vec<Event> = (0..1000)
        .map(|n| {
            board_card(
                &world.fence,
                &other,
                &format!("Card {n:04} of a thousand"),
                columns[n % 3],
                n as u32,
            )
        })
        .collect();
    let wide = |world: &World, source: &str| {
        let mut pane = Pane::open(&world.service, world.channel, "board", source);
        pane.size = (80, 24);
        pane.session.resize(80, 24);
        pane
    };

    // 1. The first time: bzz kept nothing. One list reply with a thousand
    // records, and no push.
    world.script().syncs.push_back(Sync::Set(cards.clone()));
    let before = resident();
    let started = Instant::now();
    let mut pane = wide(&world, BOARD);
    let screen = pane.settled("Loaded 1000 cards.").await;
    let first = started.elapsed();
    assert!(screen.contains("To do (334)"), "{screen}");
    let after = resident();
    assert_eq!(pane.session.records().count(), 1000);

    // 2. Every record was revised while the sync was closed: a thousand
    // differences, each a push the script takes one at a time.
    let again: Vec<Event> = cards
        .iter()
        .enumerate()
        .map(|(n, card)| {
            revised(
                &world.fence,
                &other,
                card,
                json!({"title": format!("Card {n:04} revised"), "column": columns[(n + 1) % 3], "order": n}),
            )
        })
        .collect();
    world.script().syncs.push_back(Sync::Set(again.clone()));
    let started = Instant::now();
    world.push(|subscription| json!(["CLOSED", subscription, "resync: redaction"]));
    let screen = pane
        .until("a thousand pushes", |screen, status| {
            status.sync == SyncState::Live && screen.contains("To do (333)")
        })
        .await;
    let pushes = started.elapsed();
    assert!(screen.contains("revised"), "{screen}");
    assert!(pane.session.status().responding);
    let after_pushes = resident();
    let channel = world.channel;
    drop(pane);
    world.stop().await;

    // 3. A restart: the thousand are in the store. The screen has them
    // before the relay says anything, and the relay's same set changes
    // nothing.
    let mut script = Scripted::default();
    script.syncs.push_back(Sync::Silent);
    let mut world = world_on(keys, relay_keys, community, open_store(), script).await;
    world.channel = channel;
    let started = Instant::now();
    let mut pane = wide(&world, BOARD);
    let screen = pane.settled("Loaded 1000 cards.").await;
    let restart = started.elapsed();
    assert!(screen.contains("To do (333)"), "{screen}");
    assert_eq!(
        pane.status().sync,
        SyncState::Loading,
        "the relay is silent"
    );
    drop(pane);
    world.stop().await;

    let mib = |bytes: Option<u64>| bytes.map_or(f64::NAN, |b| b as f64 / 1_048_576.0);
    eprintln!(
        "a sync of 1000 records through a session ({}):\n  first open, to the screen with all of them: {first:?}\n  a resync in which all 1000 differ, to the screen: {pushes:?}\n  a restart, to the screen from the store: {restart:?}\n  resident memory: {:.1} MiB before, {:.1} MiB after the first sync, {:.1} MiB after the thousand pushes",
        if cfg!(debug_assertions) {
            "debug build"
        } else {
            "release build"
        },
        mib(before),
        mib(after),
        mib(after_pushes),
    );
    for (what, took) in [
        ("first open", first),
        ("the pushes", pushes),
        ("a restart", restart),
    ] {
        assert!(took < Duration::from_secs(60), "{what} took {took:?}");
    }
}
