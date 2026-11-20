//! A scripted relay that keeps apps' records as the pinned relay was
//! measured to (v0.16 M0 question 3), for several identities at once: a
//! head per record, `prev` checked against it, the refusals worded as the
//! relay words them, and every stored revision sent to every connection
//! that holds the app's sync. The agent tests of M7 run an agent, its
//! owner's bzz and another profile on it. Real sockets; only the relay's
//! words are scripted.
#![allow(dead_code, clippy::result_large_err)]

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex},
};

use bzz::{
    auth::signer::SignerHandle,
    protocol::{http::HttpClient, record::Fence},
    realtime::supervisor::SupervisorHandle,
    service::records::{RecordsService, Writer},
    store::writer::StoreHandle,
};
use nostr::{Event, JsonUtil as _, Keys};
use serde_json::{Value, json};
use uuid::Uuid;

use super::scripted_relay::{HttpReply, Script, ScriptedRelay, WsAction};

#[derive(Default)]
pub struct Kept {
    /// The head of every record, by its id.
    pub heads: BTreeMap<String, Event>,
    /// Identities whose writes the relay refuses as a guest's.
    pub guests: HashSet<String>,
    /// Writes each identity may still make before it is told to wait.
    pub quota: HashMap<String, usize>,
    /// Every record write received, accepted or not, with its answer.
    pub writes: Vec<(Event, bool, String)>,
}

pub struct ArtifactRelay {
    pub relay: ScriptedRelay,
    pub kept: Arc<Mutex<Kept>>,
    pub keys: Keys,
}

fn tag(event: &Event, name: &str) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        let values = tag.as_slice();
        (values.first().map(String::as_str) == Some(name)).then(|| values.get(1).cloned())?
    })
}

fn listed(filter: &Value, name: &str, value: Option<String>) -> bool {
    filter
        .get(name)
        .and_then(Value::as_array)
        .is_none_or(|wanted| {
            value.is_some_and(|value| wanted.iter().any(|one| one == &json!(value)))
        })
}

/// The app's sync on which a revision is told: as every bzz names it.
fn subscription_of(event: &Event) -> Option<String> {
    let kind = tag(event, "type")?;
    let app = kind.strip_prefix("bzz.app.")?.split('.').next()?.to_owned();
    let channel = Uuid::parse_str(&tag(event, "h")?).ok()?;
    Some(Fence::subscription_of(&app, channel))
}

fn write(kept: &mut Kept, event: &Event) -> (bool, String) {
    let author = event.pubkey.to_hex();
    if let Some(left) = kept.quota.get_mut(&author) {
        // A refused frame counts as an accepted one does.
        if *left == 0 {
            return (false, "rate-limited: quota exceeded; retry in 19s".into());
        }
        *left -= 1;
    }
    if kept.guests.contains(&author) {
        return (
            false,
            "restricted: only members can post in this channel".into(),
        );
    }
    let Some(id) = tag(event, "d") else {
        return (false, "invalid: artifact needs a d tag".into());
    };
    let head = kept.heads.get(&id);
    match tag(event, "op").as_deref() {
        Some("create") if head.is_some() => {
            return (false, "conflict: artifact identity is taken".into());
        }
        Some("create") => {}
        Some(_) => match head {
            None => return (false, "conflict: artifact head unavailable".into()),
            Some(head) if tag(event, "prev") != Some(head.id.to_hex()) => {
                return (false, "conflict: artifact head changed".into());
            }
            Some(_) => {}
        },
        None => return (false, "invalid: artifact needs an op".into()),
    }
    kept.heads.insert(id, event.clone());
    (true, String::new())
}

impl ArtifactRelay {
    pub async fn start() -> Self {
        let keys = Keys::generate();
        let kept = Arc::new(Mutex::new(Kept::default()));
        let info = json!({
            "self": keys.public_key().to_hex(),
            "artifacts": {"modes": ["current", "history", "head"], "sync": {"version": 1, "max_events": 4096}},
        });
        let (http_kept, frame_kept) = (kept.clone(), kept.clone());
        let relay = ScriptedRelay::start(
            Script::default()
                .http(
                    move |request| match (request.method.as_str(), request.path.as_str()) {
                        ("GET", "/") => HttpReply::json(200, &info),
                        ("POST", "/query") => {
                            let filters = request.json();
                            let kept = http_kept.lock().unwrap();
                            let found = filters
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|filter| filter["#d"][0].as_str())
                                .filter_map(|id| kept.heads.get(id))
                                .map(|event| serde_json::to_value(event).unwrap())
                                .collect::<Vec<_>>();
                            HttpReply::json(200, &Value::Array(found))
                        }
                        _ => HttpReply::text(404, "not found\n"),
                    },
                )
                .frames(move |frame| {
                    let value = &frame.value;
                    match value[0].as_str().unwrap_or_default() {
                        "AUTH" => vec![WsAction::Send(json!(["OK", value[1]["id"], true, ""]))],
                        "REQ" => {
                            let id = value[1].clone();
                            let filter = &value[2];
                            let mut actions = Vec::new();
                            if filter["artifact"] == "current" {
                                let kept = frame_kept.lock().unwrap();
                                for head in kept.heads.values().filter(|head| {
                                    tag(head, "op").as_deref() != Some("delete")
                                        && listed(filter, "#h", tag(head, "h"))
                                        && listed(filter, "#type", tag(head, "type"))
                                }) {
                                    actions.push(WsAction::Send(json!(["EVENT", id, head])));
                                }
                            }
                            actions.push(WsAction::Send(json!(["EOSE", id])));
                            actions
                        }
                        "EVENT" => {
                            let Ok(event) = Event::from_json(value[1].to_string()) else {
                                return vec![WsAction::Send(json!([
                                    "OK",
                                    value[1]["id"],
                                    false,
                                    "invalid: not an event"
                                ]))];
                            };
                            let event_id = event.id.to_hex();
                            if event.verify().is_err() {
                                return vec![WsAction::Send(json!([
                                    "OK",
                                    event_id,
                                    false,
                                    "invalid: bad signature"
                                ]))];
                            }
                            if event.kind.as_u16() != 45_010 {
                                return vec![WsAction::Send(json!(["OK", event_id, true, ""]))];
                            }
                            let (accepted, message) = {
                                let mut kept = frame_kept.lock().unwrap();
                                let answer = write(&mut kept, &event);
                                kept.writes
                                    .push((event.clone(), answer.0, answer.1.clone()));
                                answer
                            };
                            let mut actions =
                                vec![WsAction::Send(json!(["OK", event_id, accepted, message]))];
                            if accepted && let Some(subscription) = subscription_of(&event) {
                                actions.push(WsAction::Broadcast(json!([
                                    "EVENT",
                                    subscription,
                                    event
                                ])));
                            }
                            actions
                        }
                        "CLOSE" => vec![WsAction::Send(json!(["CLOSED", value[1], ""]))],
                        _ => Vec::new(),
                    }
                }),
        )
        .await;
        Self { relay, kept, keys }
    }

    pub fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap()
    }

    /// A head the relay holds from the start, as if someone wrote it before.
    pub fn seed(&self, event: &Event) {
        let id = tag(event, "d").unwrap();
        self.kept().heads.insert(id, event.clone());
    }

    /// Record writes received from `pubkey`, accepted or not.
    pub fn writes_by(&self, pubkey: &str) -> Vec<(Event, bool, String)> {
        self.kept()
            .writes
            .iter()
            .filter(|(event, ..)| event.pubkey.to_hex() == pubkey)
            .cloned()
            .collect()
    }

    /// A records service for `keys`, with its own connection to this relay,
    /// and the supervisor and signer to stop at the end.
    pub fn service(
        &self,
        keys: &Keys,
        community: Uuid,
        store: StoreHandle,
    ) -> (RecordsService, SupervisorHandle, SignerHandle) {
        let signer = SignerHandle::spawn(keys.clone());
        let supervisor = SupervisorHandle::spawn(self.relay.ws_url.clone(), signer.clone());
        let http = HttpClient::new(self.relay.http_url.clone(), signer.clone()).unwrap();
        (
            RecordsService::new(
                community,
                http,
                store,
                Writer::new(signer.clone(), supervisor.clone()),
            ),
            supervisor,
            signer,
        )
    }
}
