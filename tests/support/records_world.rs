//! The world of the tests of app records (v0.16 M2): the real records
//! service, supervisor and store against a scripted relay that says what
//! the pinned relay was measured to say (M0 question 3). Real sockets;
//! only the relay's words are scripted. `tests/app_records_test.rs` drives
//! the service on it, `tests/app_session_test.rs` an app session.
#![allow(dead_code, clippy::result_large_err)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    protocol::{
        artifact::{self, Artifact},
        http::HttpClient,
        record::{Change, Fence, Record},
    },
    realtime::supervisor::SupervisorHandle,
    service::records::{AppRecords, Cause, RecordEvent, RecordsService, SyncState, Writer},
    store::{Store, writer::StoreHandle},
};
use nostr::{Event, EventBuilder, Keys};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

use super::scripted_relay::{self, HttpReply, Script, ScriptedRelay, WsAction};

pub const TYPES: [&str; 2] = ["card", "column"];

/// What the relay answers to one sync request.
pub enum Sync {
    /// The heads, then `EOSE`.
    Set(Vec<Event>),
    /// The heads, then `CLOSED` with this reason and no `EOSE`.
    Closed(Vec<Event>, &'static str),
    /// Nothing at all.
    Silent,
}

/// What the relay answers to one record write.
pub struct Answer {
    pub accepted: bool,
    pub message: &'static str,
    /// Frames sent after the `OK`, such as the head that won.
    pub then: Vec<Value>,
}

impl Answer {
    pub fn refuse(message: &'static str) -> Self {
        Self {
            accepted: false,
            message,
            then: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Scripted {
    /// Answers to the sync requests, in turn. None left: an empty set.
    pub syncs: VecDeque<Sync>,
    /// Answers to record writes, in turn. None left: accepted.
    pub answers: VecDeque<Answer>,
    /// Record writes get no answer at all.
    pub mute_records: bool,
    /// An accepted record write is echoed on the sync before its `OK`.
    pub echo_first: bool,
    /// Heads the HTTP head view knows, newest last.
    pub heads: Vec<Event>,
    /// The subscription of the last sync request.
    pub subscription: Option<String>,
}

pub struct World {
    pub relay: ScriptedRelay,
    pub script: Arc<Mutex<Scripted>>,
    pub relay_keys: Keys,
    pub keys: Keys,
    pub signer: SignerHandle,
    pub supervisor: SupervisorHandle,
    pub store: StoreHandle,
    pub service: RecordsService,
    pub community: Uuid,
    pub channel: Uuid,
    pub fence: Fence,
}

pub fn config(keys: &Keys) -> (Config, Uuid) {
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "records".into(),
        pubkey: keys.public_key().to_hex(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "records".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "records".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    (config, community)
}

pub fn first_tag(event: &Value, name: &str) -> Option<String> {
    event["tags"]
        .as_array()?
        .iter()
        .find_map(|tag| (tag[0] == name).then(|| tag[1].as_str().unwrap_or_default().to_owned()))
}

pub async fn world() -> World {
    let keys = Keys::generate();
    let (config, community) = config(&keys);
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    world_on(
        keys,
        Keys::generate(),
        community,
        store,
        Scripted::default(),
    )
    .await
}

pub async fn world_on(
    keys: Keys,
    relay_keys: Keys,
    community: Uuid,
    store: Store,
    scripted: Scripted,
) -> World {
    let script = Arc::new(Mutex::new(scripted));
    let info = json!({
        "self": relay_keys.public_key().to_hex(),
        "artifacts": {"modes": ["current", "history", "head"], "sync": {"version": 1, "max_events": 4096}},
    });
    let (http_script, frame_script) = (script.clone(), script.clone());
    let relay = ScriptedRelay::start(
        Script::default()
            .http(
                move |request| match (request.method.as_str(), request.path.as_str()) {
                    ("GET", "/") => HttpReply::json(200, &info),
                    ("POST", "/query") => {
                        let wanted = request.json()[0]["#d"][0].as_str().map(str::to_owned);
                        let script = http_script.lock().unwrap();
                        let found = script
                            .heads
                            .iter()
                            .rev()
                            .map(|event| serde_json::to_value(event).unwrap())
                            .find(|event| first_tag(event, "d") == wanted);
                        HttpReply::json(200, &Value::Array(found.into_iter().collect()))
                    }
                    ("POST", "/count") => HttpReply::json(200, &json!({"count": 3600})),
                    _ => HttpReply::text(404, "not found\n"),
                },
            )
            .frames(move |frame| {
                let value = &frame.value;
                let mut script = frame_script.lock().unwrap();
                match value[0].as_str().unwrap_or_default() {
                    "AUTH" => vec![WsAction::Send(json!(["OK", value[1]["id"], true, ""]))],
                    "REQ" => {
                        let id = value[1].clone();
                        script.subscription = id.as_str().map(str::to_owned);
                        let events = |heads: &[Event]| {
                            heads
                                .iter()
                                .map(|event| WsAction::Send(json!(["EVENT", id, event])))
                                .collect::<Vec<_>>()
                        };
                        match script.syncs.pop_front() {
                            Some(Sync::Set(heads)) => {
                                let mut actions = events(&heads);
                                actions.push(WsAction::Send(json!(["EOSE", id])));
                                actions
                            }
                            Some(Sync::Closed(heads, reason)) => {
                                let mut actions = events(&heads);
                                actions.push(WsAction::Send(json!(["CLOSED", id, reason])));
                                actions
                            }
                            Some(Sync::Silent) => Vec::new(),
                            None => vec![WsAction::Send(json!(["EOSE", id]))],
                        }
                    }
                    "EVENT" => {
                        let event_id = value[1]["id"].clone();
                        if value[1]["kind"] != 45_010 {
                            return vec![WsAction::Send(json!(["OK", event_id, true, ""]))];
                        }
                        if script.mute_records {
                            return Vec::new();
                        }
                        let echo = script
                            .subscription
                            .clone()
                            .filter(|_| script.echo_first)
                            .map(|subscription| {
                                WsAction::Send(json!(["EVENT", subscription, value[1]]))
                            });
                        match script.answers.pop_front() {
                            Some(answer) => {
                                let mut actions = vec![WsAction::Send(json!([
                                    "OK",
                                    event_id,
                                    answer.accepted,
                                    answer.message
                                ]))];
                                actions.extend(answer.then.into_iter().map(WsAction::Send));
                                actions
                            }
                            None => echo
                                .into_iter()
                                .chain([WsAction::Send(json!(["OK", event_id, true, ""]))])
                                .collect(),
                        }
                    }
                    "CLOSE" => vec![WsAction::Send(json!(["CLOSED", value[1], ""]))],
                    _ => Vec::new(),
                }
            }),
    )
    .await;
    store
        .pin_relay_pubkey(community, &relay_keys.public_key().to_hex())
        .unwrap();
    let store = StoreHandle::spawn(store).unwrap();
    let signer = SignerHandle::spawn(keys.clone());
    let supervisor = SupervisorHandle::spawn(relay.ws_url.clone(), signer.clone());
    let http = HttpClient::new(relay.http_url.clone(), signer.clone()).unwrap();
    let service = RecordsService::new(
        community,
        http,
        store.clone(),
        Writer::new(signer.clone(), supervisor.clone()),
    );
    let channel = Uuid::new_v4();
    World {
        relay,
        script,
        relay_keys,
        keys,
        signer,
        supervisor,
        store,
        service,
        community,
        channel,
        fence: Fence::new("board", channel, &TYPES).unwrap(),
    }
}

impl World {
    pub async fn open(&self) -> (AppRecords, UnboundedReceiver<RecordEvent>) {
        self.service
            .open(self.channel, "board", &TYPES)
            .await
            .unwrap()
    }

    pub fn script(&self) -> std::sync::MutexGuard<'_, Scripted> {
        self.script.lock().unwrap()
    }

    /// Record writes the relay received, as sent.
    pub fn writes(&self) -> Vec<Value> {
        self.relay
            .frames_of("EVENT")
            .into_iter()
            .map(|frame| frame.value[1].clone())
            .filter(|event| event["kind"] == 45_010)
            .collect()
    }

    /// Sync requests for the app's records the relay received.
    pub fn requests(&self) -> Vec<scripted_relay::WsFrame> {
        self.relay
            .frames_of("REQ")
            .into_iter()
            .filter(|frame| {
                frame.value[1]
                    .as_str()
                    .is_some_and(|id| id.starts_with("apprec-"))
            })
            .collect()
    }

    /// A frame of the relay's own on the app's sync.
    pub fn push(&self, frame: impl FnOnce(&str) -> Value) {
        let subscription = self.script().subscription.clone().expect("a sync is open");
        assert_eq!(self.relay.push(frame(&subscription)), 1);
    }

    pub fn live(&self, event: &Event) {
        self.push(|subscription| json!(["EVENT", subscription, event]));
    }

    pub async fn stop(self) {
        self.supervisor.shutdown().await;
        self.signer.lock().await;
        self.relay.stop();
    }
}

pub fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

pub fn sign(builder: EventBuilder, keys: &Keys) -> Event {
    builder.sign_with_keys(keys).unwrap()
}

pub fn head(event: &Event) -> Artifact {
    artifact::parse(event).unwrap()
}

/// A card another participant made.
pub fn card(fence: &Fence, keys: &Keys, title: &str) -> Event {
    sign(
        fence
            .create("card", Uuid::new_v4(), &object(json!({"title": title})))
            .unwrap(),
        keys,
    )
}

pub async fn next(events: &mut UnboundedReceiver<RecordEvent>) -> RecordEvent {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("an event of the records in time")
        .expect("the records are open")
}

/// The next batch of changes, past any change of state.
pub async fn changes(events: &mut UnboundedReceiver<RecordEvent>) -> (Vec<Change>, Cause) {
    loop {
        if let RecordEvent::Changes { changes, cause } = next(events).await {
            return (changes, cause);
        }
    }
}

pub async fn until_state(events: &mut UnboundedReceiver<RecordEvent>, wanted: &SyncState) {
    loop {
        match next(events).await {
            RecordEvent::State(state) if &state == wanted => return,
            RecordEvent::State(_) => {}
            other @ RecordEvent::Changes { .. } => panic!("unexpected {other:?}"),
        }
    }
}

/// Nothing is told for `quiet`.
pub async fn silence(events: &mut UnboundedReceiver<RecordEvent>, quiet: Duration) {
    if let Ok(event) = tokio::time::timeout(quiet, events.recv()).await {
        panic!("unexpected {event:?}");
    }
}

pub fn title(record: &Record) -> &str {
    record.content["title"].as_str().unwrap()
}

pub async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
