//! An agent and the records of the apps open in its conversation's channel
//! (v0.16 M7, plan D8). The app's script is not involved: the agent reads
//! and writes the records the app shares, without a screen.
//!
//! - **The fence (rule 17), for agents**, checked here whatever the harness
//!   sends: only an app that the owner installed, that is enabled, whose
//!   approved version bzz can hand out, and that is open in the channel of
//!   the conversation the agent was woken in; only the record types that
//!   version declares. The channel is the live turn's, never an argument.
//!   Below this, `service::records` holds every record to the same fence.
//! - **Signed by bzz with the agent's key.** The records service used here
//!   is the agent's own ([`AgentRelay::records`]): its key, its connection,
//!   its HTTP client. The agent's process holds none of them (rule 10), and
//!   the record's author is the agent.
//! - **The agent's own quota.** Its writes are counted by the relay against
//!   the agent, not its owner. bzz sends at most
//!   [`AGENT_RECORD_WRITES_PER_MINUTE`] of them in any minute and answers
//!   `rate_limited` with the wait past that, without sending a frame. While
//!   the relay's own wait lasts, the agent's connection sends nothing of it
//!   (`realtime::admission`), and the answer is the same.
//! - **As `send`, not as a command.** A record write needs no answer of the
//!   owner: the channel already lets the agent write there, and the fence
//!   is narrow. bzz asks the relay's records whether the agent may still
//!   write in the channel before each write, as before a message.
//! - **The owner sees it** in the agent's activity (`bzz agent show`), one
//!   line per write with the app, the change, the type and the record.
//!
//! The tools of M5, an app's script, are in [`authoring`].

mod authoring;

use std::time::Instant;

use serde_json::json;
use tokio::sync::watch;

use super::*;
use crate::{
    agents::runtime::app_tools::AppCall,
    apps::store::{AppError, Apps},
    protocol::record::{self as record_rules, Record, RecordError, WriteBudget},
    service::records::{AppRecords, RecordEvent, SyncState},
};

/// What one agent's record writes may spend of its quota at the relay in
/// any minute, all its apps and turns together. The pinned relay gives an
/// agent 120 writes a minute (M0 question 3), which its replies, messages,
/// reactions and typing indicators spend too. Half, as an owner's apps get
/// half of his (`APP_WRITES_PER_MINUTE`): an agent that fills a board
/// cannot leave itself unable to answer the mention that asked for it.
pub const AGENT_RECORD_WRITES_PER_MINUTE: u32 = 60;
/// Apps whose records one agent keeps open at once; the least recently
/// used is closed first.
const OPEN_AT_ONCE: usize = 8;
/// An app's records not used by the agent for this long are closed.
const IDLE: Duration = Duration::from_secs(600);
/// How long a call waits for the relay's first complete set of an app's
/// records, before it answers from what bzz holds.
const FIRST_SET: Duration = Duration::from_secs(10);
/// How long a reopened app waits for its earlier sync to end.
const REOPEN: Duration = Duration::from_secs(3);
/// The most text one answer of `records` holds.
const MAX_ANSWER_BYTES: usize = 64 * 1024;

/// One app's records, open on the agent's own connection.
struct Opened {
    channel: Uuid,
    app: String,
    types: Vec<String>,
    records: AppRecords,
    state: watch::Receiver<SyncState>,
    used: Instant,
}

/// What one agent holds of apps: the records it has open, its budget, and
/// what it spends of tries and proposals.
pub(super) struct AgentApps {
    open: tokio::sync::Mutex<Vec<Opened>>,
    budget: Mutex<WriteBudget>,
    clock: Instant,
    authoring: authoring::Authoring,
}

impl Default for AgentApps {
    fn default() -> Self {
        Self {
            open: tokio::sync::Mutex::new(Vec::new()),
            budget: Mutex::new(WriteBudget::new(AGENT_RECORD_WRITES_PER_MINUTE)),
            clock: Instant::now(),
            authoring: authoring::Authoring::default(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AgentApps {
    /// Takes one write out of the budget, or answers the wait in seconds.
    /// Closes every app's records the agent holds open.
    pub(super) async fn close(&self) {
        self.open.lock().await.clear();
    }

    fn spend(&self) -> std::result::Result<(), u64> {
        lock(&self.budget)
            .spend(self.clock.elapsed())
            .map_err(|wait| wait.as_secs_f64().ceil().max(1.0) as u64)
    }
}

/// An app open in the channel, as an agent may use it.
#[derive(Clone, Debug)]
struct Usable {
    name: String,
    title: String,
    types: Vec<String>,
    brief: String,
    version: u32,
}

/// The apps open in a channel: those an agent may use, and those it may
/// not, each with the reason.
#[derive(Debug, Default)]
struct OpenHere {
    usable: Vec<Usable>,
    unusable: Vec<(String, String)>,
}

impl OpenHere {
    fn find(&self, app: &str) -> std::result::Result<&Usable, String> {
        if let Some(found) = self.usable.iter().find(|usable| usable.name == app) {
            return Ok(found);
        }
        if let Some((_, why)) = self.unusable.iter().find(|(name, _)| name == app) {
            return Err(format!(
                "The app {app} is open in this channel but cannot be used now: {why}."
            ));
        }
        let names = self
            .usable
            .iter()
            .map(|usable| usable.name.as_str())
            .collect::<Vec<_>>();
        Err(if names.is_empty() {
            format!("No app is open in this channel, so there is no app {app} here.")
        } else {
            format!(
                "The app {app} is not open in this channel. The apps open here: {}. `apps` lists them with their record types.",
                names.join(", ")
            )
        })
    }
}

/// Why an app open in a channel cannot be used, in words for the model.
fn unusable(error: &AppError) -> String {
    match error {
        AppError::Disabled(_) => "its owner disabled it".into(),
        AppError::NothingApproved { .. } => "no version of it is approved".into(),
        AppError::Changed { .. } | AppError::Missing { .. } => {
            "its approved script is not on this machine as it was approved".into()
        }
        _ => "bzz cannot hand it out".into(),
    }
}

/// A participant as an app's contract names one: `{ id, name }`, with a
/// short form of the key while no name is known.
fn who(pubkey: &str, names: &HashMap<String, String>) -> Value {
    let name = names.get(pubkey).cloned().unwrap_or_else(|| {
        if pubkey.len() > 12
            && pubkey.is_char_boundary(8)
            && pubkey.is_char_boundary(pubkey.len() - 4)
        {
            format!("{}…{}", &pubkey[..8], &pubkey[pubkey.len() - 4..])
        } else {
            pubkey.to_owned()
        }
    });
    json!({"id": pubkey, "name": name})
}

/// A record as an agent is shown it: what a script is given
/// (`docs/app-api.md`, "Records"), `{ id, type, rev, content, by }`.
fn view(record: &Record, names: &HashMap<String, String>) -> Value {
    json!({
        "id": record.id.to_string(),
        "type": record.record_type,
        "rev": record.revision,
        "content": Value::Object(record.content.clone()),
        "by": who(&record.author, names),
    })
}

/// What a failed record call answers: the code and the reason a script
/// gets, a sentence that says what to do, and the same as JSON.
fn failed(
    error: &RecordError,
    types: &[String],
    names: &HashMap<String, String>,
    nothing: &str,
) -> ToolReply {
    let code = error.code();
    let message = error.to_string();
    let mut detail = json!({"error": {"code": code, "message": message}});
    let advice = match error {
        RecordError::Conflict { current } => {
            detail["current"] = view(current, names);
            "Someone changed the record since the rev you gave. Its current state is in `current`: decide again from it, as the app's brief says, and send the whole content with its rev.".to_owned()
        }
        RecordError::RateLimited { seconds } => {
            detail["error"]["retry_after"] = json!(seconds);
            format!("Try again in {seconds} s; until then every write is answered the same way.")
        }
        RecordError::TooLarge { .. } => "Write less into the record.".to_owned(),
        RecordError::Limit { .. } => {
            "Records can still be changed and deleted; none can be added.".to_owned()
        }
        RecordError::NotFound => {
            "It may have been deleted meanwhile: `records` shows what is there now.".to_owned()
        }
        RecordError::Unavailable { .. } => {
            "If a write was sent it may still have been stored: `records` shows what is there. Try again later.".to_owned()
        }
        RecordError::Fenced { .. } => format!(
            "The app's record types are {}.",
            types.join(", ")
        ),
        RecordError::Refused { .. } | RecordError::Invalid { .. } => {
            "Do not try the same again.".to_owned()
        }
    };
    ToolReply::refused(format!("{code}: {message}. {nothing}{advice}\n{detail}"))
}

impl Agent {
    /// The apps open in `channel`, read from the owner's store of apps.
    async fn open_here(&self, channel: Uuid) -> Result<OpenHere> {
        let (community, root) = (self.spec.community, self.shared.data_dir.join("apps"));
        self.shared
            .store
            .call(move |store| {
                let apps = Apps::at(store, root);
                let mut here = OpenHere::default();
                for name in apps.open_in_channel(community, channel)? {
                    let found = apps.runnable(community, &name).and_then(|runnable| {
                        let bundle = apps.bundle(community, &name, runnable.version())?;
                        Ok(Usable {
                            name: name.clone(),
                            title: runnable.title().to_owned(),
                            types: runnable.types().to_vec(),
                            brief: bundle.brief,
                            version: runnable.version(),
                        })
                    });
                    match found {
                        Ok(usable) => here.usable.push(usable),
                        Err(AppError::Store(error)) => return Err(error),
                        Err(error) => here.unusable.push((name, unusable(&error))),
                    }
                }
                Ok(here)
            })
            .await
    }

    /// The records of `app` in `channel` on the agent's own connection,
    /// opened now or kept from an earlier call, for exactly `types`.
    async fn app_records(
        &self,
        service: &crate::service::records::RecordsService,
        channel: Uuid,
        app: &Usable,
    ) -> std::result::Result<(AppRecords, watch::Receiver<SyncState>), RecordError> {
        let mut open = self.apps.open.lock().await;
        open.retain(|opened| opened.used.elapsed() < IDLE);
        if let Some(opened) = open.iter_mut().find(|opened| {
            opened.channel == channel && opened.app == app.name && opened.types == app.types
        }) {
            opened.used = Instant::now();
            return Ok((opened.records.clone(), opened.state.clone()));
        }
        // A new version with other types, or the first use: the old sync
        // ends with its last handle.
        open.retain(|opened| !(opened.channel == channel && opened.app == app.name));
        if open.len() >= OPEN_AT_ONCE
            && let Some(oldest) = open
                .iter()
                .enumerate()
                .min_by_key(|(_, opened)| opened.used)
                .map(|(index, _)| index)
        {
            open.remove(oldest);
        }
        let deadline = Instant::now() + REOPEN;
        while service.is_open(channel, &app.name) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (records, mut events) = service.open(channel, &app.name, &app.types).await?;
        let (state_tx, state) = watch::channel(records.state());
        // What the records tell is not for the agent until it asks: only
        // the state is kept. The task ends with the last handle.
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let RecordEvent::State(next) = event {
                    let _ = state_tx.send(next);
                }
            }
        });
        open.push(Opened {
            channel,
            app: app.name.clone(),
            types: app.types.clone(),
            records: records.clone(),
            state: state.clone(),
            used: Instant::now(),
        });
        Ok((records, state))
    }

    /// Serves one call of an app tool for the turn whose request is `row`.
    pub(super) async fn app_tool(&self, row: &InboxRow, call: AppCall) -> ToolReply {
        const NOTHING: &str = "Nothing was changed. ";
        // An app's script: the store of apps, not its records.
        if call.authors() {
            return self.author_tool(row, call).await;
        }
        let nothing = if call.writes() { NOTHING } else { "" };
        let Some(service) = self.relay.records() else {
            return ToolReply::refused(format!(
                "Apps' records cannot be reached for you here. {nothing}"
            ));
        };
        let channel = row.channel_id;
        let here = match self.open_here(channel).await {
            Ok(here) => here,
            Err(_) => {
                return ToolReply::refused(format!(
                    "bzz could not read which apps are open in this channel. {nothing}Try again shortly."
                ));
            }
        };
        let app = match call.app() {
            None => {
                let apps = here
                    .usable
                    .iter()
                    .map(|app| {
                        json!({"name": app.name, "title": app.title, "version": app.version, "types": app.types, "brief": app.brief})
                    })
                    .collect::<Vec<_>>();
                let mut listed = json!({"apps": apps});
                if !here.unusable.is_empty() {
                    listed["unusable"] = here
                        .unusable
                        .iter()
                        .map(|(name, why)| json!({"name": name, "why": why}))
                        .collect();
                }
                return ToolReply::ok(listed.to_string());
            }
            Some(app) => match here.find(app) {
                Ok(app) => app.clone(),
                Err(why) => {
                    return ToolReply::refused(format!("{why} {nothing}").trim_end().to_owned());
                }
            },
        };
        let (records, mut state) = match self.app_records(&service, channel, &app).await {
            Ok(opened) => opened,
            Err(error) => return failed(&error, &app.types, &HashMap::new(), nothing),
        };
        // The relay's first complete set, when it comes in time.
        let _ = tokio::time::timeout(
            FIRST_SET,
            state.wait_for(|state| *state != SyncState::Loading),
        )
        .await;
        match call {
            AppCall::Apps => unreachable!("answered above"),
            AppCall::Records {
                record_type,
                matching,
                limit,
                ..
            } => {
                self.list_records(&records, &app, &record_type, matching.as_deref(), limit)
                    .await
            }
            write => self.write_record(row, &records, &app, write).await,
        }
    }

    async fn names_of(&self, authors: impl Iterator<Item = &str>) -> HashMap<String, String> {
        let mut keys = authors
            .filter(|author| *author != self.spec.pubkey)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        let mut names = relay::names(self.relay.as_ref(), &keys).await;
        names.insert(self.spec.pubkey.clone(), self.spec.name.clone());
        names
    }

    async fn list_records(
        &self,
        records: &AppRecords,
        app: &Usable,
        record_type: &str,
        matching: Option<&str>,
        limit: usize,
    ) -> ToolReply {
        // The fence first, as for a script: an undeclared type is no list.
        if let Err(error) = records.fence().long_type(record_type) {
            return failed(&error, &app.types, &HashMap::new(), "");
        }
        match records.state() {
            SyncState::Restricted => {
                return failed(
                    &RecordError::Refused {
                        refusal: record_rules::Refusal::Unreadable,
                        reason: "you cannot read this channel's records".into(),
                    },
                    &app.types,
                    &HashMap::new(),
                    "",
                );
            }
            SyncState::Loading if records.records().is_empty() => {
                return failed(
                    &RecordError::Unavailable {
                        reason: "the relay has not sent this app's records yet".into(),
                    },
                    &app.types,
                    &HashMap::new(),
                    "",
                );
            }
            SyncState::Unavailable { reason } if records.records().is_empty() => {
                return failed(
                    &RecordError::Unavailable { reason },
                    &app.types,
                    &HashMap::new(),
                    "",
                );
            }
            _ => {}
        }
        // `match` over the title rule, as a script's `data.list`.
        let wanted = matching.map(str::to_lowercase);
        let found = records
            .records()
            .into_iter()
            .filter(|record| record.record_type == record_type)
            .filter(|record| {
                wanted.as_ref().is_none_or(|wanted| {
                    record_rules::title(&app.name, record_type, &record.content)
                        .to_lowercase()
                        .contains(wanted)
                })
            })
            .collect::<Vec<_>>();
        let names = self
            .names_of(found.iter().map(|record| record.author.as_str()))
            .await;
        let mut shown = Vec::new();
        let mut bytes = 0;
        for record in found.iter().take(limit) {
            let one = view(record, &names);
            bytes += one.to_string().len();
            if bytes > MAX_ANSWER_BYTES && !shown.is_empty() {
                break;
            }
            shown.push(one);
        }
        let mut answer = json!({
            "app": app.name,
            "type": record_type,
            "count": found.len(),
            "records": shown,
        });
        if shown.len() < found.len() {
            answer["more"] = json!(found.len() - shown.len());
            answer["note"] =
                json!("Not every record is shown: narrow it with `match`, or raise `limit`.");
        }
        let notice = match records.state() {
            SyncState::TooMany { max } => Some(format!(
                "This app has more than {max} records here, too many to load: these are the ones bzz holds."
            )),
            SyncState::Unavailable { reason } => Some(format!(
                "The relay cannot be asked just now ({reason}): these are the records bzz holds."
            )),
            _ => None,
        };
        if let Some(notice) = notice {
            answer["notice"] = json!(notice);
        }
        ToolReply::ok(answer.to_string())
    }

    async fn write_record(
        &self,
        row: &InboxRow,
        records: &AppRecords,
        app: &Usable,
        call: AppCall,
    ) -> ToolReply {
        const NOTHING: &str = "Nothing was changed. ";
        let (change, record_type, id) = match &call {
            AppCall::Create { record_type, .. } => ("create", record_type.clone(), None),
            AppCall::Update { id, .. } | AppCall::Delete { id, .. } => (
                if matches!(call, AppCall::Update { .. }) {
                    "update"
                } else {
                    "delete"
                },
                records
                    .record(*id)
                    .map_or_else(|| "record".to_owned(), |held| held.record_type),
                Some(id.to_string()),
            ),
            _ => unreachable!("not a write"),
        };
        let note = |answer: &str, id: Option<String>| {
            self.record(&activity::Entry::record(
                &app.name,
                change,
                &record_type,
                id,
                answer,
            ));
        };
        // Checked before anything costs quota: the type, then whether the
        // agent may still write here, then the budget.
        if let AppCall::Create { record_type, .. } = &call
            && let Err(error) = records.fence().long_type(record_type)
        {
            note(&format!("refused: {}", error.code()), None);
            return failed(&error, &app.types, &HashMap::new(), NOTHING);
        }
        if let Some(refusal) = self.may_post(row, NOTHING.trim_end()).await {
            note("refused: the agent may not write in this channel", id);
            return refusal;
        }
        if let Err(seconds) = self.apps.spend() {
            note("refused: rate_limited", id);
            return failed(
                &RecordError::RateLimited { seconds },
                &app.types,
                &HashMap::new(),
                NOTHING,
            );
        }
        // Before the write, so that a crash can only overcount: a turn
        // that may have written is not handed to a harness again.
        if !self.count_effect(row).await {
            return ToolReply::refused(format!("bzz could not record the change. {NOTHING}"));
        }
        let result = match call {
            AppCall::Create {
                record_type,
                content,
                ..
            } => records.create(&record_type, content).await,
            AppCall::Update {
                id, rev, content, ..
            } => records.update(id, &rev, content).await,
            AppCall::Delete { id, rev, .. } => records.delete(id, &rev).await,
            _ => unreachable!("not a write"),
        };
        match result {
            Ok(stored) => {
                note(
                    match change {
                        "create" => "created",
                        "update" => "updated",
                        _ => "deleted",
                    },
                    Some(stored.id.to_string()),
                );
                let names = self.names_of(std::iter::once(stored.author.as_str())).await;
                let mut answer = json!({"record": view(&stored, &names)});
                if change == "delete" {
                    answer["deleted"] = json!(true);
                }
                ToolReply::ok(answer.to_string())
            }
            Err(error) => {
                note(&format!("refused: {}", error.code()), id);
                let names = match &error {
                    RecordError::Conflict { current } => {
                        self.names_of(std::iter::once(current.author.as_str()))
                            .await
                    }
                    _ => HashMap::new(),
                };
                failed(&error, &app.types, &names, NOTHING)
            }
        }
    }
}

/// The content a model sends, as a record's content: for the tests.
#[cfg(test)]
fn object(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_says_its_code_what_to_do_and_the_same_as_json() {
        let current = Record {
            id: Uuid::new_v4(),
            record_type: "day".into(),
            revision: "b".repeat(64),
            content: object(json!({"day": "Tue", "holder": "bo", "order": 2})),
            author: "c".repeat(64),
            signed_at: 1,
        };
        let names = HashMap::from([("c".repeat(64), "bo".to_owned())]);
        let reply = failed(
            &RecordError::Conflict {
                current: Box::new(current.clone()),
            },
            &["day".into()],
            &names,
            "Nothing was changed. ",
        );
        assert!(reply.is_error && !reply.is_final);
        let (sentence, detail) = reply.text.split_once('\n').unwrap();
        assert!(sentence.starts_with("conflict: "), "{sentence}");
        assert!(sentence.contains("Nothing was changed."), "{sentence}");
        let detail: Value = serde_json::from_str(detail).unwrap();
        assert_eq!(detail["error"]["code"], "conflict");
        assert_eq!(detail["current"]["rev"], "b".repeat(64));
        assert_eq!(detail["current"]["content"]["holder"], "bo");
        assert_eq!(
            detail["current"]["by"],
            json!({"id": "c".repeat(64), "name": "bo"})
        );
        let waited = failed(
            &RecordError::RateLimited { seconds: 23 },
            &["day".into()],
            &HashMap::new(),
            "",
        );
        let detail: Value = serde_json::from_str(waited.text.split_once('\n').unwrap().1).unwrap();
        assert_eq!(detail["error"]["retry_after"], 23);
        assert!(waited.text.contains("Try again in 23 s"), "{}", waited.text);
        let fenced = failed(
            &RecordError::Fenced {
                reason: "the app rota did not declare the record type card".into(),
            },
            &["day".into()],
            &HashMap::new(),
            "",
        );
        assert!(fenced.text.starts_with("fenced: "), "{}", fenced.text);
        assert!(
            fenced.text.contains("record types are day"),
            "{}",
            fenced.text
        );
    }

    #[test]
    fn an_unknown_author_is_shown_by_a_short_form_of_the_key() {
        let key = "0123456789abcdef".repeat(4);
        assert_eq!(
            who(&key, &HashMap::new()),
            json!({"id": key, "name": "01234567…cdef"})
        );
    }

    #[test]
    fn an_agent_writes_at_most_its_budget_in_a_minute() {
        let apps = AgentApps::default();
        for _ in 0..AGENT_RECORD_WRITES_PER_MINUTE {
            assert_eq!(apps.spend(), Ok(()));
        }
        let wait = apps.spend().unwrap_err();
        assert!((59..=60).contains(&wait), "{wait}");
        // Half of what the pinned relay gives an agent.
        assert_eq!(AGENT_RECORD_WRITES_PER_MINUTE * 2, 120);
    }

    #[test]
    fn an_app_not_open_here_is_named_with_those_that_are() {
        let here = OpenHere {
            usable: vec![Usable {
                name: "rota".into(),
                title: "Rota".into(),
                types: vec!["day".into()],
                brief: String::new(),
                version: 1,
            }],
            unusable: vec![("board".into(), "its owner disabled it".into())],
        };
        assert_eq!(here.find("rota").unwrap().types, ["day"]);
        let disabled = here.find("board").unwrap_err();
        assert!(disabled.contains("its owner disabled it"), "{disabled}");
        let elsewhere = here.find("poll").unwrap_err();
        assert!(
            elsewhere.contains("not open in this channel") && elsewhere.contains("rota"),
            "{elsewhere}"
        );
        assert!(
            OpenHere::default()
                .find("rota")
                .unwrap_err()
                .contains("No app is open")
        );
    }
}
