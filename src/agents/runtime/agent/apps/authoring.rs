//! An agent writes and changes an app (v0.16 M5, plans D5, D7, D16, D17):
//! `app_guide`, `apps` with an `app`, `try_app` and `propose_app`.
//!
//! - **Told as Buzz tells it (D17).** Nothing about apps is in the agent's
//!   prompt beforehand. `app_guide` returns the author guide and the
//!   contract, as Buzz desktop's `plugin_authoring_guide` does.
//! - **Nothing an agent sends runs unapproved (D5, rule 16).** A try runs
//!   the script apart, on records in memory: nothing is stored and nothing
//!   reaches the relay. A proposal goes through `Apps::record`, the one way
//!   a version is recorded; it waits for the owner, and only
//!   `Apps::runnable` hands out a script to run. No tool approves, opens,
//!   enables, disables or removes an app, and none writes a file.
//! - **A script that does not load is answered at once (M5).** The proposal
//!   is loaded apart first; one that fails comes back with the error, file
//!   and line, is not recorded and is never put to the owner.
//! - **Who may ask (D7).** Any local agent of the owner, in a turn a person
//!   who may wake it started: the proposal records the agent and that
//!   person, and the approval shows both. An agent has at most
//!   [`AGENT_WAITING_AT_ONCE`] versions waiting, and sends at most
//!   [`PROPOSALS_PER_MINUTE`] proposals and [`TRIES_PER_MINUTE`] tries in a
//!   minute, one try at a time.
//! - **The outcome reaches the agent** at its next turn in the channel it
//!   proposed in, once ([`Agent::app_notices`]): applied; refused, with the
//!   owner's reason word for word; or failed as it was put to run, with the
//!   error. No turn is started for it, so one reply per mention holds.
//!   `apps` with `app` shows it whenever the agent asks.
//!
//! [`AGENT_WAITING_AT_ONCE`]: crate::apps::store::AGENT_WAITING_AT_ONCE

use std::time::{Duration, Instant};

use super::*;
use crate::{
    agents::runtime::app_tools::TryCall,
    apps::{
        dev::{Seed, local_user},
        runtime::{Limits as AppLimits, ManualClock, Who},
        session::{
            MemoryStore,
            headless::{Headless, HeadlessOptions, START_OF_TIME},
        },
        store::{AppVersion, Outcome, Proposer, ProposerKind, Recorded, Submission, examine},
    },
    render::sanitize,
};

/// The author guide (`docs/app-authoring.md`) and the contract
/// (`docs/app-api.md`), as `app_guide` returns them.
pub const GUIDE: &str = include_str!("../../../../../docs/app-authoring.md");
pub const CONTRACT: &str = include_str!("../../../../../docs/app-api.md");

/// Tries one agent may start in any minute; one runs at a time.
pub const TRIES_PER_MINUTE: u32 = 20;
/// Proposals one agent may send in any minute, those that do not load
/// included.
pub const PROPOSALS_PER_MINUTE: u32 = 6;
/// The longest a try may take, all its steps together. A script that is
/// still busy then is given up on, and the answer says after which step.
const TRY_TIME: Duration = Duration::from_secs(30);
/// The app's log lines a try answers with: the last ones.
const TRY_LOG_LINES: usize = 20;
/// The versions of an app this agent proposed that `apps` with `app`
/// shows: the newest ones.
const OWN_VERSIONS_SHOWN: usize = 5;
/// How long a proposal waits for the name of the person who asked.
const NAME_LIMIT: Duration = Duration::from_secs(3);

/// What one agent spends of tries and proposals.
pub(in crate::agents::runtime::agent) struct Authoring {
    tries: tokio::sync::Semaphore,
    try_budget: Mutex<WriteBudget>,
    propose_budget: Mutex<WriteBudget>,
}

impl Default for Authoring {
    fn default() -> Self {
        Self {
            tries: tokio::sync::Semaphore::new(1),
            try_budget: Mutex::new(WriteBudget::new(TRIES_PER_MINUTE)),
            propose_budget: Mutex::new(WriteBudget::new(PROPOSALS_PER_MINUTE)),
        }
    }
}

fn spend(budget: &Mutex<WriteBudget>, clock: Instant) -> std::result::Result<(), u64> {
    lock(budget)
        .spend(clock.elapsed())
        .map_err(|wait| wait.as_secs_f64().ceil().max(1.0) as u64)
}

/// `app_guide`'s answer: the guide, then the contract.
pub fn guide_text() -> String {
    format!(
        "{GUIDE}\n\n---\n\nThe contract follows: docs/app-api.md, the whole of `api = 1`.\n\n{CONTRACT}"
    )
}

/// What became of a version, in words for the agent that proposed it.
/// `None` for an outcome there is nothing to tell of.
pub fn outcome_words(version: &AppVersion, outcome: &Outcome) -> Option<String> {
    let what = format!(
        "your version {} of the app {}",
        version.version, version.name
    );
    match outcome {
        Outcome::Waiting | Outcome::Superseded => None,
        Outcome::Applied => Some(format!(
            "{what}: applied. The owner approved it, and it runs where the app is open."
        )),
        Outcome::Refused {
            reason: Some(reason),
        } => Some(format!(
            "{what}: refused by the owner, who said: \"{reason}\". Nothing of it runs."
        )),
        Outcome::Refused { reason: None } => Some(format!(
            "{what}: refused by the owner, who gave no reason. Nothing of it runs."
        )),
        Outcome::Failed { error } => Some(format!(
            "{what}: approved, and failed as it was put to run; the version before it still runs. The error: {error}"
        )),
    }
}

fn state_words(version: &AppVersion) -> Value {
    let outcome = Outcome::of(version);
    json!({
        "version": version.version,
        "state": version.state.as_str(),
        "detail": version.detail,
        "said": outcome_words(version, &outcome),
    })
}

/// The seeds whose type the script does not declare, as `bzz app try` warns.
fn undeclared(types_seeded: &[String], declared: &[String]) -> Option<String> {
    let mut strangers: Vec<&str> = types_seeded
        .iter()
        .map(String::as_str)
        .filter(|name| !declared.iter().any(|declared| declared == name))
        .collect();
    strangers.sort_unstable();
    strangers.dedup();
    (!strangers.is_empty()).then(|| {
        format!(
            "records of type {} were given, which the script does not declare (it declares {}): it never sees them",
            strangers.join(", "),
            declared.join(", ")
        )
    })
}

/// One record to plant in a try's store, with who wrote it.
struct Planted {
    record_type: String,
    content: serde_json::Map<String, Value>,
    by: Who,
}

/// What a try found.
struct TryOutcome {
    screen: String,
    error: Option<String>,
    logs: Vec<String>,
    types: Vec<String>,
    closed: bool,
    responding: bool,
    notice: Option<String>,
    steps_done: usize,
    stopped: bool,
}

/// Runs a try on a thread of its own: the script loaded apart on a store
/// in memory, the steps done one by one until [`TRY_TIME`] has passed.
fn run_try(
    call: &TryCall,
    script_name: String,
    planted: Vec<Planted>,
) -> std::io::Result<TryOutcome> {
    let user = local_user();
    let clock = ManualClock::new(START_OF_TIME);
    let mut store = MemoryStore::new(user.clone(), clock.clone());
    for record in planted {
        store.seed(&record.record_type, record.content, &record.by);
    }
    let mut options = HeadlessOptions::new(script_name)
        .size(call.width, call.height)
        .user(user)
        .limits(AppLimits::default());
    if let Some(app) = &call.from_app {
        options = options.app(app.clone());
    }
    let started = Instant::now();
    let mut app = Headless::start_on(call.script.clone().into_bytes(), options, store, clock)?;
    let mut steps_done = 0;
    let mut stopped = false;
    for step in &call.steps {
        if started.elapsed() > TRY_TIME {
            stopped = true;
            break;
        }
        app.run(std::slice::from_ref(step));
        steps_done += 1;
    }
    let tried = app.tried();
    Ok(TryOutcome {
        screen: tried
            .screen
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n"),
        error: tried.error,
        logs: tried
            .logs
            .iter()
            .rev()
            .take(TRY_LOG_LINES)
            .rev()
            .map(|line| sanitize::visible(line))
            .collect(),
        types: tried.types,
        closed: tried.closed,
        responding: tried.responding,
        notice: tried.notice,
        steps_done,
        stopped,
    })
}

impl Agent {
    /// Serves one of the tools of an app's script for the turn whose
    /// request is `row`.
    pub(super) async fn author_tool(&self, row: &InboxRow, call: AppCall) -> ToolReply {
        match call {
            AppCall::Guide => ToolReply::ok(guide_text()),
            AppCall::Read { app } => self.read_app(row, &app).await,
            AppCall::Try(call) => self.try_script(row, call).await,
            AppCall::Propose {
                app,
                script,
                brief,
                title,
            } => self.propose(row, app, script, brief, title).await,
            _ => unreachable!("not a tool of an app's script"),
        }
    }

    /// `apps` with `app`: an app open in this channel, whole; or of an app
    /// not open here, only what became of this agent's versions of it.
    async fn read_app(&self, row: &InboxRow, name: &str) -> ToolReply {
        let (community, root, channel) = (
            self.spec.community,
            self.shared.data_dir.join("apps"),
            row.channel_id,
        );
        let (me, wanted) = (self.spec.pubkey.clone(), name.to_owned());
        let read = self
            .shared
            .store
            .call(move |store| {
                let apps = Apps::at(store, root);
                let listed = match apps.get(community, &wanted) {
                    Ok(listed) => listed,
                    Err(AppError::Store(error)) => return Err(error),
                    Err(AppError::NotInstalled(_)) => {
                        return Ok(Err(format!(
                            "No app named {wanted} is installed. `apps` lists the apps open in this channel; `propose_app` proposes a new one."
                        )));
                    }
                    Err(error) => return Ok(Err(error.to_string())),
                };
                let mine = apps
                    .versions(community, &wanted)
                    .map_err(Error::from)?
                    .into_iter()
                    .filter(|version| {
                        version.proposer.kind == ProposerKind::Agent
                            && version.proposer.id.as_deref() == Some(me.as_str())
                    })
                    .take(OWN_VERSIONS_SHOWN)
                    .map(|version| state_words(&version))
                    .collect::<Vec<_>>();
                let open_here = listed.channels.contains(&channel);
                if !open_here {
                    if mine.is_empty() {
                        return Ok(Err(format!(
                            "The app {wanted} is not open in this channel. `apps` lists the apps open here."
                        )));
                    }
                    return Ok(Ok(json!({
                        "app": wanted,
                        "open_here": false,
                        "your_versions": mine,
                        "note": "The app is not open in this channel: only what became of your versions of it is shown.",
                    })));
                }
                let waiting = listed.waiting.len();
                let mut answer = json!({
                    "app": wanted,
                    "title": listed.title(),
                    "open_here": true,
                    "enabled": listed.app.enabled,
                    "versions_waiting": waiting,
                    "your_versions": mine,
                });
                match listed.app.current {
                    Some(current) => match apps.bundle(community, &wanted, current) {
                        Ok(bundle) => {
                            answer["version"] = json!(current);
                            answer["types"] = json!(bundle.manifest.types);
                            answer["manifest"] = json!(bundle.manifest_text);
                            answer["brief"] = json!(bundle.brief);
                            answer["script"] =
                                json!(String::from_utf8_lossy(&bundle.script).into_owned());
                        }
                        Err(AppError::Store(error)) => return Err(error),
                        Err(error) => {
                            answer["version"] = json!(current);
                            answer["unreadable"] = json!(error.to_string());
                        }
                    },
                    None => {
                        answer["version"] = Value::Null;
                        answer["note"] = json!("No version of this app was approved yet.");
                    }
                }
                Ok(Ok(answer))
            })
            .await;
        match read {
            Ok(Ok(answer)) => ToolReply::ok(answer.to_string()),
            Ok(Err(why)) => ToolReply::refused(why),
            Err(_) => {
                ToolReply::refused("bzz could not read its apps just now. Try again shortly.")
            }
        }
    }

    /// `try_app`: the script loaded apart, on records in memory.
    async fn try_script(&self, row: &InboxRow, call: TryCall) -> ToolReply {
        const NOTHING: &str = "Nothing was tried.";
        let Ok(_one_at_a_time) = self.apps.authoring.tries.try_acquire() else {
            return ToolReply::refused(format!(
                "Another try of yours is still running: wait for its answer. {NOTHING}"
            ));
        };
        if let Err(seconds) = spend(&self.apps.authoring.try_budget, self.apps.clock) {
            return ToolReply::refused(format!(
                "rate_limited: you tried {TRIES_PER_MINUTE} scripts in the last minute. Try again in {seconds} s. {NOTHING}"
            ));
        }
        let mut planted = call
            .records
            .iter()
            .map(|seed: &Seed| Planted {
                record_type: seed.record_type.clone(),
                content: seed.content.clone(),
                by: seed.by.as_deref().map_or_else(local_user, |name| {
                    Who::new(crate::apps::store::manifest::sha256(name.as_bytes()), name)
                }),
            })
            .collect::<Vec<_>>();
        // A copy of the records of an app open here, read as the agent
        // reads them: nothing the script writes reaches them.
        if let Some(app) = &call.from_app {
            match self.copy_of_records(row, app).await {
                Ok(copied) => planted.extend(copied),
                Err(reply) => return reply,
            }
        }
        let seeded = planted
            .iter()
            .map(|record| record.record_type.clone())
            .collect::<Vec<_>>();
        let script_name = format!("{}.lua", call.from_app.as_deref().unwrap_or("app"));
        let size = format!("{}x{}", call.width, call.height);
        let steps = call.steps.len();
        let ran = tokio::task::spawn_blocking(move || run_try(&call, script_name, planted)).await;
        let tried = match ran {
            Ok(Ok(tried)) => tried,
            _ => {
                return ToolReply::refused(format!(
                    "bzz could not start a thread to try the script. {NOTHING}"
                ));
            }
        };
        let mut answer = json!({
            "size": size,
            "screen": tried.screen,
            "error": tried.error,
            "types": tried.types,
            "responding": tried.responding,
            "steps": steps,
            "log": tried.logs,
        });
        if tried.closed {
            answer["closed"] = json!("the app closed itself (host.quit)");
        }
        if let Some(notice) = tried.notice {
            answer["notice"] = json!(notice);
        }
        let mut notes = Vec::new();
        if tried.stopped {
            notes.push(format!(
                "the try was stopped after {} of {steps} steps: it took more than {} s",
                tried.steps_done,
                TRY_TIME.as_secs()
            ));
        }
        if !tried.responding {
            notes.push("the script did not come to rest: it does not respond".to_owned());
        }
        if !tried.types.is_empty()
            && let Some(warning) = undeclared(&seeded, &tried.types)
        {
            notes.push(warning);
        }
        if !notes.is_empty() {
            answer["notes"] = json!(notes);
        }
        ToolReply::ok(answer.to_string())
    }

    /// The current records of an app open here, as records to plant.
    async fn copy_of_records(
        &self,
        row: &InboxRow,
        app: &str,
    ) -> std::result::Result<Vec<Planted>, ToolReply> {
        const NOTHING: &str = " Nothing was tried.";
        let Some(service) = self.relay.records() else {
            return Err(ToolReply::refused(format!(
                "Apps' records cannot be reached for you here, so `from_app` cannot be used: give records with `records`.{NOTHING}"
            )));
        };
        let here = self.open_here(row.channel_id).await.map_err(|_| {
            ToolReply::refused(format!(
                "bzz could not read which apps are open in this channel.{NOTHING}"
            ))
        })?;
        let usable = here
            .find(app)
            .map_err(|why| ToolReply::refused(format!("{why}{NOTHING}")))?
            .clone();
        let (records, mut state) = self
            .app_records(&service, row.channel_id, &usable)
            .await
            .map_err(|error| failed(&error, &usable.types, &HashMap::new(), NOTHING.trim()))?;
        let _ = tokio::time::timeout(
            FIRST_SET,
            state.wait_for(|state| *state != SyncState::Loading),
        )
        .await;
        let held = records.records();
        let names = self
            .names_of(held.iter().map(|record| record.author.as_str()))
            .await;
        Ok(held
            .into_iter()
            .take(crate::apps::runtime::limits::LIVE_RECORDS)
            .map(|record| {
                let name = who(&record.author, &names)["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                Planted {
                    record_type: record.record_type,
                    content: record.content,
                    by: Who::new(record.author, name),
                }
            })
            .collect())
    }

    /// `propose_app`: a version, loaded apart, then put to the owner.
    async fn propose(
        &self,
        row: &InboxRow,
        name: String,
        script: String,
        brief: String,
        title: Option<String>,
    ) -> ToolReply {
        const NOTHING: &str = "Nothing was proposed, and the owner was not asked.";
        let asker_key = row.author.clone();
        let asker = tokio::time::timeout(
            NAME_LIMIT,
            self.names_of(std::iter::once(asker_key.as_str())),
        )
        .await
        .unwrap_or_default();
        let asker_name = who(&asker_key, &asker)["name"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let note = |version: Option<u32>, answer: &str| {
            self.record(&activity::Entry::proposal(
                &name,
                version,
                &asker_name,
                answer,
            ));
        };
        if let Err(seconds) = spend(&self.apps.authoring.propose_budget, self.apps.clock) {
            note(None, "refused: rate_limited");
            return ToolReply::refused(format!(
                "rate_limited: you proposed {PROPOSALS_PER_MINUTE} versions in the last minute. Try again in {seconds} s. {NOTHING}"
            ));
        }
        // Which apps an agent may propose a version of: a new one, one open
        // in this channel, or one of which no version was approved yet.
        let (community, root, channel) = (
            self.spec.community,
            self.shared.data_dir.join("apps"),
            row.channel_id,
        );
        let wanted = name.clone();
        let may = self
            .shared
            .store
            .call(move |store| {
                let apps = Apps::at(store, root);
                Ok(match apps.get(community, &wanted) {
                    Err(AppError::NotInstalled(_)) => Ok(()),
                    Err(AppError::Store(error)) => return Err(error),
                    Err(error) => Err(error.to_string()),
                    Ok(listed) if listed.channels.contains(&channel) => Ok(()),
                    Ok(listed) if listed.app.current.is_none() => Ok(()),
                    Ok(_) => Err(format!(
                        "The app {wanted} is installed, and not open in this channel: a version of it is proposed where it is open. The owner opens it here with `:app open {wanted}`."
                    )),
                })
            })
            .await;
        match may {
            Ok(Ok(())) => {}
            Ok(Err(why)) => {
                note(None, "refused: not open here");
                return ToolReply::refused(format!("{why} {NOTHING}"));
            }
            Err(_) => {
                return ToolReply::refused(format!(
                    "bzz could not read its apps just now. {NOTHING} Try again shortly."
                ));
            }
        }
        let from = Proposer::agent(self.spec.pubkey.clone(), self.spec.name.clone())
            .in_channel(channel)
            .asked_by(asker_key, asker_name.clone());
        let brief_text = brief.clone();
        let mut submission = Submission::new(name.clone(), script, brief, from);
        if let Some(title) = title {
            submission = submission.titled(title);
        }
        // Loaded apart first, off bzz's own threads: a stuck script costs
        // its own thread a few seconds.
        let examined = match tokio::task::spawn_blocking(move || examine(submission)).await {
            Ok(Ok(examined)) => examined,
            Ok(Err(error)) => {
                note(None, "refused: not a name an app can have");
                return ToolReply::refused(format!("{error}. {NOTHING}"));
            }
            Err(_) => {
                return ToolReply::refused(format!(
                    "bzz could not start a thread to load the script. {NOTHING}"
                ));
            }
        };
        // The brief stays true (D16): it names every type the script
        // declares. Checked on the types the script declared as it loaded.
        let untrue = examined.types().and_then(|types| {
            crate::apps::store::manifest::check_brief_names_types(&brief_text, types).err()
        });
        if let Some(error) = examined.error().map(str::to_owned).or(untrue) {
            let error = sanitize::visible(&error);
            note(None, "failed; the owner was not asked");
            return ToolReply::refused(format!(
                "failed: {error}\n{NOTHING} Fix it and propose again; `try_app` shows the screen and any error.\n{}",
                json!({"error": {"code": "failed", "message": error}})
            ));
        }
        let root = self.shared.data_dir.join("apps");
        let recorded = self
            .shared
            .store
            .call(move |store| {
                Ok(match Apps::at(store, root).record(community, examined) {
                    Ok(recorded) => Ok(recorded),
                    Err(AppError::Store(error)) => return Err(error),
                    Err(error) => Err(error.to_string()),
                })
            })
            .await;
        match recorded {
            Ok(Ok(Recorded::Waiting {
                version,
                hash,
                superseded,
            })) => {
                note(Some(version), "waiting for the owner");
                let mut answer = json!({
                    "status": "waiting",
                    "app": name,
                    "version": version,
                    "hash": hash,
                    "message": format!(
                        "Waiting for the owner. Version {version} of {name} is put to the owner, who approves or refuses these exact bytes; nothing of it runs before. You are told what became of it at your next turn in this channel, and `apps` with `app` shows it any time."
                    ),
                });
                if !superseded.is_empty() {
                    answer["replaces"] = json!(superseded);
                    answer["message"] = json!(format!(
                        "{} It takes the place of your version {} that waited.",
                        answer["message"].as_str().unwrap_or_default(),
                        superseded
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                ToolReply::ok(answer.to_string())
            }
            Ok(Ok(Recorded::Running { version })) => {
                note(Some(version), "the version that runs already");
                ToolReply::ok(
                    json!({
                        "status": "running",
                        "app": name,
                        "version": version,
                        "message": format!("This is version {version} of {name}, which runs already: there is nothing to approve."),
                    })
                    .to_string(),
                )
            }
            Ok(Ok(Recorded::Failed { error, .. })) => {
                // Examined as loading; recorded as failed all the same.
                note(None, "failed; the owner was not asked");
                ToolReply::refused(format!("failed: {}\n{NOTHING}", sanitize::visible(&error)))
            }
            Ok(Err(why)) => {
                note(None, "refused: too many waiting");
                ToolReply::refused(format!("{why}. {NOTHING}"))
            }
            Err(_) => ToolReply::refused(format!(
                "bzz could not record the proposal. {NOTHING} Try again shortly."
            )),
        }
    }

    /// What became of the versions this agent proposed in `channel` of
    /// which it was not told yet, one sentence each, and records that it
    /// was told. For the prompt of its next turn there.
    pub(in crate::agents::runtime::agent) async fn app_notices(
        &self,
        channel: Uuid,
    ) -> Vec<String> {
        let (community, root, me) = (
            self.spec.community,
            self.shared.data_dir.join("apps"),
            self.spec.pubkey.clone(),
        );
        self.shared
            .store
            .call(move |store| {
                let mut apps = Apps::at(store, root);
                let untold = match apps.untold(community, &me, channel) {
                    Ok(untold) => untold,
                    Err(AppError::Store(error)) => return Err(error),
                    Err(_) => return Ok(Vec::new()),
                };
                let mut said = Vec::new();
                for (version, outcome) in untold {
                    if let Some(words) = outcome_words(&version, &outcome) {
                        said.push(words);
                    }
                    let _ = apps.told(community, &version.name, version.version);
                }
                Ok(said)
            })
            .await
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::apps::VersionState;

    fn version(state: VersionState, detail: Option<&str>) -> AppVersion {
        AppVersion {
            community: Uuid::nil(),
            name: "board".into(),
            version: 3,
            state,
            detail: detail.map(str::to_owned),
            title: None,
            api: Some(1),
            types: vec!["card".into()],
            script_sha256: "0".repeat(64),
            manifest_sha256: None,
            brief_sha256: "0".repeat(64),
            script_bytes: 1,
            brief_bytes: 1,
            proposer: Proposer::agent("a".repeat(64), "Scout"),
            proposed_at: 1,
            decided_at: Some(2),
            kept: false,
            told_at: None,
        }
    }

    #[test]
    fn an_outcome_is_said_with_the_owners_reason_word_for_word() {
        let said = |state, detail| {
            let version = version(state, detail);
            outcome_words(&version, &Outcome::of(&version))
        };
        assert_eq!(
            said(VersionState::Approved, None).as_deref(),
            Some(
                "your version 3 of the app board: applied. The owner approved it, and it runs where the app is open."
            )
        );
        assert_eq!(
            said(VersionState::Refused, Some("no filter, only the priority")).as_deref(),
            Some(
                "your version 3 of the app board: refused by the owner, who said: \"no filter, only the priority\". Nothing of it runs."
            )
        );
        assert!(
            said(VersionState::Refused, None)
                .unwrap()
                .contains("who gave no reason")
        );
        assert!(
            said(VersionState::Failed, Some("board.lua:7: boom"))
                .unwrap()
                .ends_with("The error: board.lua:7: boom")
        );
        assert_eq!(said(VersionState::Waiting, None), None);
        assert_eq!(said(VersionState::Superseded, Some("replaced")), None);
    }

    #[test]
    fn the_guide_is_the_guide_then_the_contract() {
        let text = guide_text();
        assert!(
            text.starts_with("# Writing an app for bzz"),
            "{}",
            &text[..80]
        );
        assert!(text.contains("# The app contract, `api = 1`"));
        assert!(text.find("Writing an app") < text.find("The app contract"));
    }
}
