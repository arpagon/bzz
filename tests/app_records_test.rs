//! The records service of apps against a scripted relay (v0.16 M2, service
//! half): what the caller gets when the relay says each of the things the
//! pinned relay was measured to say (M0 question 3). Real sockets, real
//! supervisor, real store; only the relay's words are scripted.
#![cfg(feature = "apps")]
mod support;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bzz::{
    auth::signer::SignerHandle,
    protocol::{
        http::HttpClient,
        record::{Change, Fence, MAX_CONTENT_BYTES, MAX_LIVE_RECORDS, RecordError, Refusal},
    },
    realtime::supervisor::{SupervisorEvent, SupervisorHandle},
    service::records::{
        CONFLICT_SYNC_WAIT, Cause, RESYNC_DELAY, RecordEvent, RecordsService, SyncState, Writer,
    },
    store::{Store, writer::StoreHandle},
};
use nostr::{Event, EventBuilder, JsonUtil as _, Keys};
use serde_json::{Map, json};
use support::records_world::*;
use support::scripted_relay::{Script, ScriptedRelay, relay_signed};
use uuid::Uuid;

#[tokio::test]
async fn a_conflict_returns_the_current_record_from_the_sync_or_over_http() {
    let world = world().await;
    let other = Keys::generate();
    let first = card(&world.fence, &other, "Buy milk");
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![first.clone()]));
    let (app, mut events) = world.open().await;
    let (initial, cause) = changes(&mut events).await;
    assert_eq!((initial.len(), cause), (1, Cause::Sync));
    until_state(&mut events, &SyncState::Live).await;
    let held = app.records().pop().unwrap();
    assert_eq!(
        (title(&held), held.revision.as_str()),
        ("Buy milk", first.id.to_hex().as_str())
    );
    assert_eq!(held.author, other.public_key().to_hex());

    // 1. Someone revises the card first. The relay refuses ours without the
    // head; the app's own sync delivers the winner right after.
    let second = sign(
        world
            .fence
            .update(&head(&first), &object(json!({"title": "Buy oat milk"})))
            .unwrap(),
        &other,
    );
    let subscription = world.script().subscription.clone().unwrap();
    world.script().answers.push_back(Answer {
        accepted: false,
        message: "conflict: artifact head changed",
        then: vec![json!(["EVENT", subscription, second])],
    });
    let error = app
        .update(
            held.id,
            &held.revision,
            object(json!({"title": "Buy soy milk"})),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "conflict");
    let RecordError::Conflict { current } = error else {
        panic!("{error:?}")
    };
    assert_eq!(
        (
            title(&current),
            current.revision.as_str(),
            current.author.as_str()
        ),
        (
            "Buy oat milk",
            second.id.to_hex().as_str(),
            other.public_key().to_hex().as_str()
        )
    );
    let queries = |world: &World| {
        world
            .relay
            .http_requests()
            .into_iter()
            .filter(|request| request.path == "/query")
            .collect::<Vec<_>>()
    };
    assert!(queries(&world).is_empty(), "the sync had the head");
    // The winner is also a change by another participant, told once.
    let (told, cause) = changes(&mut events).await;
    assert_eq!(told, [Change::Upsert((*current).clone())]);
    assert_eq!(cause, Cause::Live);

    // The retry on the current record is stored, built on the winner.
    let stored = app
        .update(
            held.id,
            &current.revision,
            object(json!({"title": "Buy soy milk"})),
        )
        .await
        .unwrap();
    let sent = world.writes();
    assert_eq!(sent.len(), 2);
    assert_eq!(first_tag(&sent[1], "prev"), Some(second.id.to_hex()));
    assert_eq!(first_tag(&sent[1], "op").as_deref(), Some("update"));
    assert_eq!(stored.revision, sent[1]["id"].as_str().unwrap());
    assert_eq!(stored.author, world.keys.public_key().to_hex());
    assert_eq!(app.record(held.id).unwrap(), stored);

    // 2. The sync does not deliver the winner: after a short wait bzz reads
    // the head over HTTP.
    let ours = Event::from_json(sent[1].to_string()).unwrap();
    let third = sign(
        world
            .fence
            .update(&head(&ours), &object(json!({"title": "Buy almond milk"})))
            .unwrap(),
        &other,
    );
    world.script().heads.push(third.clone());
    world
        .script()
        .answers
        .push_back(Answer::refuse("conflict: artifact head changed"));
    let started = Instant::now();
    let error = app
        .update(
            held.id,
            &stored.revision,
            object(json!({"title": "Buy rice milk"})),
        )
        .await
        .unwrap_err();
    let RecordError::Conflict { current } = error else {
        panic!("{error:?}")
    };
    assert_eq!(title(&current), "Buy almond milk");
    assert_eq!(current.revision, third.id.to_hex());
    assert!(started.elapsed() >= CONFLICT_SYNC_WAIT);
    let asked = queries(&world);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].json(),
        json!([{"artifact": "head", "#d": [held.id.to_string()]}])
    );
    assert_eq!(
        changes(&mut events).await.0,
        [Change::Upsert((*current).clone())]
    );

    // 3. A revision the caller no longer holds is a conflict bzz knows
    // without asking: no frame, no quota.
    let error = app
        .update(held.id, &stored.revision, object(json!({"title": "late"})))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RecordError::Conflict {
            current: current.clone()
        }
    );
    assert_eq!(world.writes().len(), 3);

    // 4. The record was deleted meanwhile: the head view shows the deletion,
    // and the answer is that it is gone.
    let deletion = sign(world.fence.delete(&head(&third)).unwrap(), &other);
    world.script().heads.push(deletion);
    world
        .script()
        .answers
        .push_back(Answer::refuse("conflict: artifact head changed"));
    let error = app
        .update(
            held.id,
            &current.revision,
            object(json!({"title": "too late"})),
        )
        .await
        .unwrap_err();
    assert_eq!(error, RecordError::NotFound);
    assert_eq!(
        changes(&mut events).await.0,
        [Change::Removed {
            record: (*current).clone(),
            by: Some(other.public_key().to_hex())
        }]
    );
    assert!(app.records().is_empty());

    // The two other refusals that begin with `conflict:` are not a reason to
    // read the head and try again.
    let again = card(&world.fence, &other, "Another");
    world.live(&again);
    changes(&mut events).await;
    let asked_before = queries(&world).len();
    world
        .script()
        .answers
        .push_back(Answer::refuse("conflict: artifact head unavailable"));
    assert_eq!(
        app.delete(again.id_of(), &again.id.to_hex())
            .await
            .unwrap_err(),
        RecordError::NotFound
    );
    world
        .script()
        .answers
        .push_back(Answer::refuse("conflict: artifact identity is taken"));
    assert!(matches!(
        app.create("card", object(json!({"title": "x"}))).await,
        Err(RecordError::Refused {
            refusal: Refusal::Other,
            ..
        })
    ));
    assert_eq!(queries(&world).len(), asked_before);
    world.stop().await;
}

/// The record id of an artifact event.
trait IdOf {
    fn id_of(&self) -> Uuid;
}

impl IdOf for Event {
    fn id_of(&self) -> Uuid {
        head(self).id
    }
}

#[tokio::test]
async fn the_relays_quota_becomes_a_wait_in_seconds() {
    let world = world().await;
    let (app, mut events) = world.open().await;
    until_state(&mut events, &SyncState::Live).await;
    let mut relay_events = world.supervisor.subscribe_events();
    world
        .script()
        .answers
        .push_back(Answer::refuse("rate-limited: quota exceeded; retry in 40s"));
    let error = app
        .create("card", object(json!({"title": "One too many"})))
        .await
        .unwrap_err();
    assert_eq!(error, RecordError::RateLimited { seconds: 40 });
    assert_eq!(error.code(), "rate_limited");
    assert_eq!(world.writes().len(), 1);

    // bzz knows the gate is closed: the next writes are answered at once
    // with what is left of the wait, and no frame is spent on them.
    wait_for("the supervisor to close its gate", || {
        matches!(
            relay_events.try_recv(),
            Ok(SupervisorEvent::RateLimited { retry_after }) if retry_after == Duration::from_secs(40)
        )
    })
    .await;
    let started = Instant::now();
    for _ in 0..3 {
        let error = app
            .create("card", object(json!({"title": "Still too many"})))
            .await
            .unwrap_err();
        let RecordError::RateLimited { seconds } = error else {
            panic!("{error:?}")
        };
        assert!((35..=40).contains(&seconds), "{seconds}");
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "not held in a queue"
    );
    assert_eq!(world.writes().len(), 1);
    assert!(app.records().is_empty());
    world.stop().await;
}

#[tokio::test]
async fn a_resync_tells_only_real_differences_and_never_empties_the_set() {
    let world = world().await;
    let other = Keys::generate();
    let (a, b, c) = (
        card(&world.fence, &other, "a"),
        card(&world.fence, &other, "b"),
        card(&world.fence, &other, "c"),
    );
    let set = vec![a.clone(), b.clone(), c.clone()];
    world.script().syncs.push_back(Sync::Set(set.clone()));
    let (app, mut events) = world.open().await;
    assert_eq!(changes(&mut events).await.0.len(), 3);
    until_state(&mut events, &SyncState::Live).await;
    let held = app.records();
    assert_eq!(held.len(), 3);

    // The least number of records the caller could have read at any moment.
    let least = Arc::new(Mutex::new(usize::MAX));
    let watcher = tokio::spawn({
        let (app, least) = (app.clone(), least.clone());
        async move {
            loop {
                let now = app.records().len();
                {
                    let mut least = least.lock().unwrap();
                    *least = (*least).min(now);
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    });

    // 1. A chat message is deleted: the relay ends the sync. The next
    // request gets the same set: the caller is told of the state and of no
    // change at all.
    world.script().syncs.push_back(Sync::Set(set.clone()));
    let closed = tokio::time::Instant::now();
    world.push(|subscription| json!(["CLOSED", subscription, "resync: redaction"]));
    until_state(&mut events, &SyncState::Resyncing).await;
    assert_eq!(
        app.records(),
        held,
        "nothing is emptied while bzz asks again"
    );
    until_state(&mut events, &SyncState::Live).await;
    let requests = world.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].at.duration_since(closed) >= RESYNC_DELAY,
        "asked again only after a short delay"
    );
    assert_eq!(
        requests[0].value[2], requests[1].value[2],
        "the same request"
    );
    assert_eq!(
        requests[1].value[2],
        json!({"artifact": "current", "#h": [world.channel.to_string()],
               "#type": ["bzz.app.board.card", "bzz.app.board.column"]})
    );
    assert_eq!(app.records(), held);

    // 2. The topic changes. This time the relay ends the new sync as well,
    // after its heads and before EOSE (seen on the pinned relay in 2 of 30
    // rounds): nothing of the unfinished set is used. The third request gets
    // a set that really differs: `a` as it was, `b` revised, `c` gone, `d` new.
    let b2 = sign(
        world
            .fence
            .update(&head(&b), &object(json!({"title": "b2"})))
            .unwrap(),
        &other,
    );
    let d = card(&world.fence, &other, "d");
    world
        .script()
        .syncs
        .push_back(Sync::Closed(vec![a.clone()], "resync: redaction"));
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![a.clone(), b2.clone(), d.clone()]));
    world.push(|subscription| json!(["CLOSED", subscription, "resync: channel access changed"]));
    until_state(&mut events, &SyncState::Resyncing).await;
    let (told, cause) = changes(&mut events).await;
    assert_eq!(cause, Cause::Sync);
    let record = |event: &Event| world.fence.record(&head(event)).unwrap();
    let mut expected = vec![
        Change::Upsert(record(&b2)),
        Change::Removed {
            record: record(&c),
            by: None,
        },
        Change::Upsert(record(&d)),
    ];
    expected.sort_by_key(|change| change.record().id);
    assert_eq!(told, expected, "`a` did not change and is not mentioned");
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(world.requests().len(), 4);
    silence(&mut events, Duration::from_millis(300)).await;

    watcher.abort();
    assert_eq!(
        *least.lock().unwrap(),
        3,
        "the caller's set never held fewer records than the smallest real set"
    );

    // A dropped connection is the same to the caller: the supervisor sends
    // the request again, and an unchanged set is no change.
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![a.clone(), b2.clone(), d.clone()]));
    world.supervisor.reconnect().await;
    until_state(&mut events, &SyncState::Resyncing).await;
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.records().len(), 3);
    silence(&mut events, Duration::from_millis(200)).await;
    world.stop().await;
}

#[tokio::test]
async fn frames_missed_inside_bzz_are_made_up_by_a_whole_new_set() {
    let world = world().await;
    let other = Keys::generate();
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|title| card(&world.fence, &other, title));
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![a.clone(), b.clone()]));
    let (app, mut events) = world.open().await;
    assert_eq!(changes(&mut events).await.0.len(), 2);
    until_state(&mut events, &SyncState::Live).await;

    // bzz's store is busy. The task that follows this app's sync takes one
    // live head and waits for the store with it...
    let (release, held) = std::sync::mpsc::channel::<()>();
    let busy = world.store.clone();
    let blocked = tokio::spawn(async move {
        busy.call(move |_| {
            let _ = held.recv_timeout(Duration::from_secs(30));
            Ok(())
        })
        .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut heard = world.supervisor.subscribe_events();
    world.live(&c);
    // ...while the relay goes on: another head, and then more frames than
    // bzz keeps for a listener that is not listening (2048). The head is
    // among the ones that are dropped.
    world.live(&d);
    const NOISE: usize = 2600;
    for n in 0..NOISE {
        assert_eq!(world.relay.push(json!(["NOTICE", format!("noise {n}")])), 1);
        if n % 256 == 255 {
            tokio::task::yield_now().await;
        }
    }
    let last = format!("noise {}", NOISE - 1);
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match heard.recv().await {
                Ok(SupervisorEvent::Session(bzz::realtime::session::SessionEvent::Notice(
                    notice,
                ))) if notice == last => break,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .expect("every frame was heard by bzz");
    // The relay will answer the next request with the whole set.
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![a, b, c.clone(), d.clone()]));
    release.send(()).unwrap();
    blocked.await.unwrap().unwrap();

    // The head it had in hand is told as the live change it was. Then bzz
    // finds that it missed frames, asks for the whole set again, and tells
    // exactly what it had missed: nothing is taken on trust, nothing is
    // told twice, and the set was never emptied.
    let told = |event: &Event| vec![Change::Upsert(world.fence.record(&head(event)).unwrap())];
    assert_eq!(
        next(&mut events).await,
        RecordEvent::Changes {
            changes: told(&c),
            cause: Cause::Live
        }
    );
    assert_eq!(
        next(&mut events).await,
        RecordEvent::State(SyncState::Resyncing)
    );
    assert!(app.records().len() >= 3);
    assert_eq!(
        next(&mut events).await,
        RecordEvent::Changes {
            changes: told(&d),
            cause: Cause::Sync
        }
    );
    assert_eq!(next(&mut events).await, RecordEvent::State(SyncState::Live));
    assert_eq!(app.records().len(), 4);
    assert_eq!(world.requests().len(), 2, "one request for the new set");
    silence(&mut events, Duration::from_millis(300)).await;
    world.stop().await;
}

#[tokio::test]
async fn too_many_records_and_no_access_reach_the_caller_as_states() {
    // More live records of the app's types than one sync returns.
    let world = world().await;
    let other = Keys::generate();
    let known = card(&world.fence, &other, "known");
    world.script().syncs.push_back(Sync::Closed(
        vec![known.clone()],
        "error: sync set exceeds max_events",
    ));
    let (app, mut events) = world.open().await;
    until_state(&mut events, &SyncState::TooMany { max: 4096 }).await;
    assert_eq!(app.state(), SyncState::TooMany { max: 4096 });
    assert!(app.state().is_final());
    assert!(
        app.records().is_empty(),
        "a part of a set is not shown as the set"
    );
    // Final: bzz does not ask again on a timer.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(world.requests().len(), 1);
    // Nothing can be created; the answer is the limit, and no frame is sent.
    assert_eq!(
        app.create("card", object(json!({"title": "one more"})))
            .await
            .unwrap_err(),
        RecordError::Limit {
            limit: MAX_LIVE_RECORDS
        }
    );
    assert!(world.writes().is_empty());
    // After records were deleted, the caller asks again.
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![known.clone()]));
    app.retry();
    assert_eq!(changes(&mut events).await.0.len(), 1);
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(world.requests().len(), 2);
    world.stop().await;

    // The identity cannot read the channel.
    let world = self::world().await;
    world
        .script()
        .syncs
        .push_back(Sync::Closed(Vec::new(), "restricted: channel not readable"));
    let (app, mut events) = world.open().await;
    until_state(&mut events, &SyncState::Restricted).await;
    assert!(app.state().is_final());
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(world.requests().len(), 1);
    let error = app
        .create("card", object(json!({"title": "no"})))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            RecordError::Refused {
                refusal: Refusal::Unreadable,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(world.writes().is_empty());
    world.stop().await;

    // A relay without the sync is a state too, and no request is sent.
    let keys = Keys::generate();
    let (config, community) = config(&keys);
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    let relay = ScriptedRelay::start(Script::default().nip11(json!({"name": "plain"}))).await;
    let signer = SignerHandle::spawn(keys);
    let supervisor = SupervisorHandle::spawn(relay.ws_url.clone(), signer.clone());
    let service = RecordsService::new(
        community,
        HttpClient::new(relay.http_url.clone(), signer.clone()).unwrap(),
        StoreHandle::spawn(store).unwrap(),
        Writer::new(signer.clone(), supervisor.clone()),
    );
    let (app, mut events) = service.open(Uuid::new_v4(), "board", &TYPES).await.unwrap();
    let RecordEvent::State(SyncState::Unavailable { reason }) = next(&mut events).await else {
        panic!("expected the state")
    };
    assert_eq!(reason, "this relay has no artifact sync");
    assert!(matches!(
        app.create("card", Map::new()).await,
        Err(RecordError::Unavailable { .. })
    ));
    assert!(relay.frames_of("REQ").is_empty() && relay.frames_of("EVENT").is_empty());
    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn an_apps_writes_do_not_delay_a_persons_message() {
    let world = world().await;
    let (app, mut events) = world.open().await;
    until_state(&mut events, &SyncState::Live).await;
    let message = |text: &str| {
        buzz_sdk::build_message(world.channel, text, None, &[], false, &[], &[])
            .unwrap()
            .sign_with_keys(&world.keys)
            .unwrap()
    };
    let position = |world: &World, id: &str| {
        let frames = world.relay.frames_of("EVENT");
        let at = frames
            .iter()
            .position(|frame| frame.value[1]["id"] == id)
            .expect("the message reached the relay");
        frames[..at]
            .iter()
            .filter(|frame| frame.value[1]["kind"] == 45_010)
            .count()
    };

    // 1. Twenty writes of an app are waiting when the person sends a
    // message. The message is the next frame out, not the twenty-first.
    let writes = (0..20)
        .map(|index| {
            let app = app.clone();
            tokio::spawn(async move {
                app.create("card", object(json!({"title": format!("card {index}")})))
                    .await
            })
        })
        .collect::<Vec<_>>();
    wait_for("the first write of the app on the wire", || {
        !world.writes().is_empty()
    })
    .await;
    let first = message("first");
    let ack = world.supervisor.publish(first.clone()).await.unwrap();
    assert!(ack.accepted);
    let before = position(&world, &first.id.to_hex());
    assert!(
        before <= 3,
        "{before} of the app's 20 writes went out before the person's message"
    );
    for write in writes {
        write.await.unwrap().unwrap();
    }
    assert_eq!(
        world.writes().len(),
        20,
        "every write of the app was sent, after"
    );
    assert_eq!(app.records().len(), 20);

    // 2. The relay stops acknowledging the app's writes. One is on the wire
    // without an answer and more are waiting; the person's message is still
    // sent and acknowledged at once.
    world.script().mute_records = true;
    let stuck = (0..5)
        .map(|index| {
            let app = app.clone();
            tokio::spawn(async move {
                app.create("card", object(json!({"title": format!("stuck {index}")})))
                    .await
            })
        })
        .collect::<Vec<_>>();
    wait_for("an unanswered write of the app on the wire", || {
        world.writes().len() == 21
    })
    .await;
    let second = message("second");
    let started = Instant::now();
    let ack = tokio::time::timeout(
        Duration::from_secs(3),
        world.supervisor.publish(second.clone()),
    )
    .await
    .expect("the message did not wait for the app's write")
    .unwrap();
    assert!(ack.accepted);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        world.writes().len(),
        21,
        "the app's other writes wait for its own"
    );
    for write in stuck {
        write.abort();
    }
    world.stop().await;
}

#[tokio::test]
async fn the_fence_refuses_at_the_service_whoever_asks() {
    let world = world().await;
    let other = Keys::generate();
    let (community, channel) = (world.community, world.channel);
    let elsewhere = Uuid::new_v4();
    // What the local store already holds from other syncs: a task in this
    // channel, a vote of another app in this channel, and a card of the
    // same app in another channel.
    let foreign = |home: Uuid, long: &str| {
        let tags = [
            ["ar", "1"],
            ["d", &Uuid::new_v4().to_string()],
            ["h", &home.to_string()],
            ["type", long],
            ["title", "Not this app's"],
            ["op", "create"],
        ]
        .map(|parts| nostr::Tag::parse(parts).unwrap());
        sign(
            EventBuilder::new(
                nostr::Kind::Custom(45_010),
                "{\"title\":\"Not this app's\"}",
            )
            .tags(tags),
            &other,
        )
    };
    let task = foreign(channel, "buzz.task");
    let vote = foreign(channel, "bzz.app.poll.vote");
    let far = foreign(elsewhere, "bzz.app.board.card");
    for event in [&task, &vote, &far] {
        let head = head(event);
        world
            .store
            .call(move |store| store.apply_artifact_head(community, &head))
            .await
            .unwrap();
    }
    // A relay that sends what was not asked for: the same three in the set.
    let mine = card(&world.fence, &other, "mine");
    world.script().syncs.push_back(Sync::Set(vec![
        task.clone(),
        mine.clone(),
        vote.clone(),
        far.clone(),
    ]));
    let (app, mut events) = world.open().await;
    let (told, _) = changes(&mut events).await;
    assert_eq!(told.len(), 1, "{told:?}");
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.records().len(), 1);
    assert_eq!(app.records()[0].id, mine.id_of());

    // And live.
    for event in [&task, &vote, &far] {
        world.live(event);
    }
    world.live(&foreign(channel, "bzz.app.board.cards"));
    silence(&mut events, Duration::from_millis(300)).await;
    assert_eq!(app.records().len(), 1);

    // A type that was not declared: refused before anything is signed.
    let agent = Writer::new(
        SignerHandle::spawn(Keys::generate()),
        world.supervisor.clone(),
    );
    for outside in ["vote", "bzz.app.board.card", "buzz.task", "Card", ""] {
        let content = object(json!({"title": "x"}));
        for error in [
            app.create(outside, content.clone()).await.unwrap_err(),
            app.create_as(&agent, outside, content.clone())
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code(), "fenced", "{outside}: {error:?}");
        }
    }
    // A record of another type or another channel, known to this bzz: it
    // is not this app's to revise or delete, whoever asks.
    for event in [&task, &vote, &far] {
        let (id, revision) = (event.id_of(), event.id.to_hex());
        let content = object(json!({"title": "taken over"}));
        for error in [
            app.update(id, &revision, content.clone())
                .await
                .unwrap_err(),
            app.delete(id, &revision).await.unwrap_err(),
            app.update_as(&agent, id, &revision, content.clone())
                .await
                .unwrap_err(),
            app.delete_as(&agent, id, &revision).await.unwrap_err(),
        ] {
            assert_eq!(error, RecordError::NotFound);
        }
    }
    assert!(
        world.writes().is_empty(),
        "nothing outside the fence was sent"
    );
    // The rows of the other types and channels are as they were.
    for event in [&task, &vote, &far] {
        let id = event.id_of();
        let stored = world
            .store
            .call(move |store| store.artifact(community, id))
            .await
            .unwrap();
        assert!(stored.is_some());
    }

    // A record that leaves: moved to another channel by someone, or removed
    // by the relay itself. Only the relay's pinned key is believed.
    let removal = |keys: &Keys, id: Uuid| {
        relay_signed(
            keys,
            45_011,
            "",
            vec![
                vec!["ar".into(), "1".into()],
                vec!["d".into(), id.to_string()],
                vec!["h".into(), channel.to_string()],
            ],
        )
    };
    world.live(&removal(&other, mine.id_of()));
    silence(&mut events, Duration::from_millis(200)).await;
    assert_eq!(app.records().len(), 1);
    world.live(&removal(&world.relay_keys, mine.id_of()));
    let (told, _) = changes(&mut events).await;
    assert!(
        matches!(&told[..], [Change::Removed { by: None, .. }]),
        "{told:?}"
    );
    assert!(app.records().is_empty());

    // One instance of an app per channel.
    assert!(matches!(
        world.service.open(channel, "board", &TYPES).await,
        Err(RecordError::Invalid { .. })
    ));
    drop((app, events));
    wait_for("the closed app to leave the open set", || {
        world.relay.frames_of("CLOSE").len() == 1
    })
    .await;
    let reopened = world.service.open(channel, "board", &["card"]).await;
    assert!(reopened.is_ok());
    world.stop().await;
}

#[tokio::test]
async fn nothing_is_sent_that_bzz_knows_would_be_refused() {
    let world = world().await;
    let other = Keys::generate();
    // The app already holds its thousand records, one of them written by
    // some other client with content that is no object: a record with no
    // fields, which counts, and which the app can delete.
    let mut set = (0..MAX_LIVE_RECORDS - 1)
        .map(|index| card(&world.fence, &other, &format!("card {index}")))
        .collect::<Vec<_>>();
    let odd = sign(
        EventBuilder::new(nostr::Kind::Custom(45_010), "not json").tags(
            [
                ["ar", "1"],
                ["d", &Uuid::new_v4().to_string()],
                ["h", &world.channel.to_string()],
                ["type", "bzz.app.board.card"],
                ["title", "Odd"],
                ["op", "create"],
            ]
            .map(|parts| nostr::Tag::parse(parts).unwrap()),
        ),
        &other,
    );
    let odd_id = head(&odd).id;
    set.push(odd);
    let one = set[0].clone();
    world.script().syncs.push_back(Sync::Set(set));
    let (app, mut events) = world.open().await;
    assert_eq!(changes(&mut events).await.0.len(), MAX_LIVE_RECORDS);
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.records().len(), MAX_LIVE_RECORDS);
    let odd = app.record(odd_id).expect("the odd one is a record");
    assert!(odd.content.is_empty());

    // Over the app's limit.
    let error = app
        .create("card", object(json!({"title": "one too many"})))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RecordError::Limit {
            limit: MAX_LIVE_RECORDS
        }
    );
    assert_eq!(error.code(), "limit");
    // Too large, for a create and for a revision.
    let huge = object(json!({"title": "x", "body": "y".repeat(MAX_CONTENT_BYTES)}));
    let revision = one.id.to_hex();
    for error in [
        app.create("column", huge.clone()).await.unwrap_err(),
        app.update(one.id_of(), &revision, huge.clone())
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error.code(), "too_large", "{error:?}");
    }
    // A record that is not there, and a revision that is not the current one.
    assert_eq!(
        app.delete(Uuid::new_v4(), &revision).await.unwrap_err(),
        RecordError::NotFound
    );
    assert_eq!(
        app.delete(one.id_of(), &"0".repeat(64))
            .await
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert!(world.writes().is_empty(), "none of the six cost a frame");

    // Deleting one makes room; the relay's refusals of a channel are told
    // as they are.
    let last = app.delete(one.id_of(), &revision).await.unwrap();
    assert_eq!(title(&last), "card 0");
    for (message, refusal) in [
        ("invalid: channel is archived", Refusal::Archived),
        (
            "restricted: only members can post in this channel",
            Refusal::Guest,
        ),
        ("restricted: not a channel member", Refusal::NotMember),
    ] {
        world.script().answers.push_back(Answer::refuse(message));
        let error = app
            .create("card", object(json!({"title": "refused"})))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "refused");
        assert!(
            matches!(&error, RecordError::Refused { refusal: told, reason } if *told == refusal && !reason.is_empty()),
            "{error:?}"
        );
    }
    let sent = world.writes();
    assert_eq!(sent.len(), 4);
    // The deletion as the relay wants it: no title, no content.
    assert_eq!(first_tag(&sent[0], "op").as_deref(), Some("delete"));
    assert_eq!(first_tag(&sent[0], "title"), None);
    assert_eq!(sent[0]["content"], "");
    assert_eq!(app.records().len(), MAX_LIVE_RECORDS - 1);
    // What no contract wrote can be deleted like any record.
    let gone = app.delete(odd_id, &odd.revision).await.unwrap();
    assert!(gone.content.is_empty());
    assert_eq!(app.records().len(), MAX_LIVE_RECORDS - 2);
    let sent = world.writes();
    assert_eq!(first_tag(&sent[4], "d"), Some(odd_id.to_string()));
    assert_eq!(first_tag(&sent[4], "op").as_deref(), Some("delete"));
    // The relay's count of the whole channel, for the warning.
    assert_eq!(
        world.service.channel_total(world.channel).await.unwrap(),
        3600
    );
    world.stop().await;
}

#[tokio::test]
async fn the_callers_own_writes_are_answered_once_and_another_writers_are_told() {
    let world = world().await;
    let (app, mut events) = world.open().await;
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.identity(), world.keys.public_key().to_hex());

    // Create, revise and delete, each on the revision the last one gave.
    let made = app
        .create(
            "card",
            object(json!({"title": "Mine", "order": 1.5, "n": 2})),
        )
        .await
        .unwrap();
    assert_eq!(made.record_type, "card");
    assert_eq!(made.author, app.identity());
    assert_eq!(app.records(), std::slice::from_ref(&made));
    let sent = world.writes();
    assert_eq!(
        first_tag(&sent[0], "type").as_deref(),
        Some("bzz.app.board.card")
    );
    assert_eq!(first_tag(&sent[0], "title").as_deref(), Some("Mine"));
    assert_eq!(
        sent[0]["content"],
        "{\"n\":2,\"order\":1.5,\"title\":\"Mine\"}"
    );
    // The relay echoes an accepted write on the sync, as the pinned one does.
    world.live(&Event::from_json(sent[0].to_string()).unwrap());
    let revised = app
        .update(
            made.id,
            &made.revision,
            object(json!({"title": "Mine, revised"})),
        )
        .await
        .unwrap();
    let sent = world.writes();
    assert_eq!(first_tag(&sent[1], "prev"), Some(made.revision.clone()));
    world.live(&Event::from_json(sent[1].to_string()).unwrap());
    silence(&mut events, Duration::from_millis(300)).await;
    assert_eq!(app.records(), std::slice::from_ref(&revised));

    // The store has it too, at once: it is what a restart shows.
    let (community, channel, types) = (world.community, world.channel, world.fence.long_types());
    let stored = world
        .store
        .call(move |store| store.typed_artifacts(community, channel, &types))
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].head, revised.revision);

    // Another writer of this bzz (M7: a local agent with its own key). The
    // record is signed by that key, and the app is told as of anyone's change.
    let agent_keys = Keys::generate();
    let agent_signer = SignerHandle::spawn(agent_keys.clone());
    let agent = Writer::new(agent_signer.clone(), world.supervisor.clone());
    assert_eq!(agent.pubkey(), agent_keys.public_key().to_hex());
    let theirs = app
        .update_as(
            &agent,
            made.id,
            &revised.revision,
            object(json!({"title": "The agent's"})),
        )
        .await
        .unwrap();
    assert_eq!(theirs.author, agent.pubkey());
    assert_eq!(world.writes()[2]["pubkey"], agent.pubkey());
    let (told, cause) = changes(&mut events).await;
    assert_eq!(
        (told, cause),
        (vec![Change::Upsert(theirs.clone())], Cause::Live)
    );
    world.live(&Event::from_json(world.writes()[2].to_string()).unwrap());
    silence(&mut events, Duration::from_millis(200)).await;

    let last = app.delete(made.id, &theirs.revision).await.unwrap();
    assert_eq!(last, theirs);
    assert!(app.records().is_empty());
    let stored = world
        .store
        .call(move |store| store.artifact(community, made.id))
        .await
        .unwrap();
    assert!(stored.is_none(), "no row is kept for a deleted record");
    silence(&mut events, Duration::from_millis(200)).await;

    // A write accepted while the relay makes bzz fetch the set again: the
    // new set was cut before it, and the caller's new record must not vanish
    // and come back.
    world.script().syncs.push_back(Sync::Silent);
    world.push(|subscription| json!(["CLOSED", subscription, "resync: redaction"]));
    until_state(&mut events, &SyncState::Resyncing).await;
    wait_for("the second request", || world.requests().len() == 2).await;
    let fresh = app
        .create("card", object(json!({"title": "During the resync"})))
        .await
        .unwrap();
    world.push(|subscription| json!(["EOSE", subscription]));
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.records(), std::slice::from_ref(&fresh));
    world.live(&Event::from_json(world.writes()[4].to_string()).unwrap());
    silence(&mut events, Duration::from_millis(300)).await;
    assert_eq!(app.records(), std::slice::from_ref(&fresh));

    // The relay may deliver the echo before the acknowledgement. The
    // caller is still answered once and told nothing.
    world.script().echo_first = true;
    let early = app
        .create("column", object(json!({"title": "Echoed first"})))
        .await
        .unwrap();
    let early = app
        .update(
            early.id,
            &early.revision,
            object(json!({"title": "Echoed first, revised"})),
        )
        .await
        .unwrap();
    assert_eq!(app.record(early.id), Some(early.clone()));
    assert_eq!(app.delete(early.id, &early.revision).await.unwrap(), early);
    assert_eq!(app.records(), std::slice::from_ref(&fresh));
    silence(&mut events, Duration::from_millis(300)).await;
    world.script().echo_first = false;

    agent_signer.lock().await;
    world.stop().await;
}

#[tokio::test]
async fn a_restart_shows_the_stored_records_before_the_relay_answers() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("bzz.sqlite3");
    let keys = Keys::generate();
    let (config, community) = config(&keys);
    let other = Keys::generate();

    // First run: two records arrive and are kept.
    let mut store = Store::open(&path).unwrap();
    store.sync_config(&config).unwrap();
    let relay_keys = Keys::generate();
    let world = world_on(
        keys.clone(),
        relay_keys.clone(),
        community,
        store,
        Scripted::default(),
    )
    .await;
    let channel = world.channel;
    let (a, b) = (
        card(&world.fence, &other, "a"),
        card(&world.fence, &other, "b"),
    );
    world
        .script()
        .syncs
        .push_back(Sync::Set(vec![a.clone(), b.clone()]));
    let (app, mut events) = world.open().await;
    assert_eq!(app.state(), SyncState::Loading);
    assert_eq!(changes(&mut events).await.0.len(), 2);
    until_state(&mut events, &SyncState::Live).await;
    let mine = app
        .create("column", object(json!({"title": "Doing"})))
        .await
        .unwrap();
    let mut before = app.records();
    drop((app, events));
    world.stop().await;

    // Second run, another process as far as the store knows: the relay does
    // not answer the sync at all.
    let store = Store::open(&path).unwrap();
    let mut world = world_on(
        keys,
        relay_keys,
        community,
        store,
        Scripted {
            syncs: VecDeque::from([Sync::Silent]),
            ..Scripted::default()
        },
    )
    .await;
    world.channel = channel;
    world.fence = Fence::new("board", channel, &TYPES).unwrap();
    let (app, mut events) = world.open().await;
    let mut shown = app.records();
    before.sort_by_key(|record| record.id);
    shown.sort_by_key(|record| record.id);
    assert_eq!(shown, before, "the same records, revisions and authors");
    assert_eq!(shown.len(), 3);
    assert!(shown.iter().any(|record| record == &mine));
    assert_eq!(app.state(), SyncState::Loading);
    wait_for("the sync request", || world.requests().len() == 1).await;
    silence(&mut events, Duration::from_millis(300)).await;

    // When the relay does answer, only what changed while bzz was away is
    // told: `a` was deleted, the rest is as stored.
    world.push(|subscription| json!(["EVENT", subscription, b]));
    let column = world.store.clone();
    let stored_mine = column
        .call(move |store| store.artifact(community, mine.id))
        .await
        .unwrap()
        .unwrap();
    world.push(|subscription| json!(["EVENT", subscription, stored_mine.head.event]));
    world.push(|subscription| json!(["EOSE", subscription]));
    let (told, cause) = changes(&mut events).await;
    assert_eq!(cause, Cause::Sync);
    assert!(
        matches!(&told[..], [Change::Removed { record, by: None }] if record.id == a.id_of()),
        "{told:?}"
    );
    until_state(&mut events, &SyncState::Live).await;
    assert_eq!(app.records().len(), 2);
    world.stop().await;
}
