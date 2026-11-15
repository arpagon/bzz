//! A coding harness under bzz's guard (plan v0.15.0 M6; ADR v0.15 local
//! agents, decision 8). Grok, Codex and Claude Code are the owner's own
//! programs, and in the mode his own settings name all three write anywhere
//! without asking. So bzz, and not those settings, decides:
//!
//! - **Before a session is prompted**, new or loaded, the mode bzz sets for
//!   that harness is set on it, and the model the owner chose. A session
//!   that does not take the mode is never prompted.
//! - **Each thing the harness asks leave for** is answered by the rule of
//!   [`permission_rule`]: bzz's chat tools, yes; a file edit, only inside
//!   the agent's folders; a shell command, by the folders granted for
//!   commands and the owner's answer, as a Pi Durable agent's `run`;
//!   anything else, no.
//!
//! What the owner took away since this bzz started holds at once: the
//! settings file is read again for every answer.

use std::path::Path;

use super::*;
use crate::{
    agents::runtime::{
        coding::Pick,
        permission::{Roots, Wanted},
        record,
    },
    config::FolderGrant,
};

/// The weaker of two grants for commands.
fn weaker(one: ExecGrant, other: ExecGrant) -> ExecGrant {
    match (one, other) {
        (ExecGrant::No, _) | (_, ExecGrant::No) => ExecGrant::No,
        (ExecGrant::Ask, _) | (_, ExecGrant::Ask) => ExecGrant::Ask,
        _ => ExecGrant::WithoutAsking,
    }
}

fn shown(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Agent {
    /// Whether `run` is one of the three coding harnesses, which get their
    /// tools from `bzz agent-tools`.
    pub(super) fn is_coding(&self, run: &Run) -> bool {
        !run.pipe_tools && coding::spec(self.spec.local.harness).is_some()
    }

    /// The working folder of a coding harness.
    pub(super) fn workspace(&self) -> PathBuf {
        process::agent_dir(&self.shared.data_dir, self.spec.id).join("workspace")
    }

    /// What every message tells a harness that is not bzz's own host: how
    /// to answer, and the agent's instructions unless the harness took
    /// them as its system prompt. `None` for bzz's own host, which was told
    /// at its start.
    pub(super) fn prompt_instructions(&self, run: &Run) -> Option<&str> {
        if run.pipe_tools {
            None
        } else if self.is_coding(run)
            && coding::takes_system_prompt(self.spec.local.harness, run.offer)
        {
            Some("")
        } else {
            Some(self.spec.local.instructions.as_str())
        }
    }

    /// A transcript of the process about to start, when
    /// `BZZ_AGENT_RECORD` names a folder for them. Only of a coding
    /// harness.
    pub(super) fn recorder(&self, launch: &Launch) -> Option<Arc<Recorder>> {
        coding::spec(self.spec.local.harness)?;
        let directory =
            (self.shared.environment)(record::VARIABLE).filter(|value| !value.trim().is_empty())?;
        let mut places = vec![(launch.cwd.clone(), "<workspace>".to_owned())];
        places.extend(
            self.spec
                .local
                .folders
                .iter()
                .map(|folder| (folder.path.clone(), format!("<folder:{}>", folder.name))),
        );
        places.push((self.shared.data_dir.clone(), "<data>".into()));
        if let Some(dirs) = directories::BaseDirs::new() {
            places.push((dirs.home_dir().to_path_buf(), "<home>".into()));
        }
        // A program may name a folder as it really is.
        let real = places
            .iter()
            .filter_map(|(path, name)| Some((std::fs::canonicalize(path).ok()?, name.clone())))
            .filter(|place| !places.contains(place))
            .collect::<Vec<_>>();
        places.extend(real);
        Recorder::create(
            Path::new(&directory),
            self.spec.local.harness.as_str(),
            &json!({
                "harness": self.spec.local.harness.as_str(),
                "bzz": env!("CARGO_PKG_VERSION"),
            }),
            &places,
        )
    }

    /// Notes a call of a chat tool in the transcript of the process `run`.
    pub(super) async fn record_tool(&self, run: u64, session: &str, name: &str, arguments: &Value) {
        // Never waited for: a process that is starting holds this.
        let Ok(current) = self.run.try_lock() else {
            return;
        };
        if let Some(recorder) = current
            .as_ref()
            .filter(|current| current.id == run)
            .and_then(|current| current.recorder.as_ref())
        {
            recorder.tool(session, name, arguments);
        }
    }

    /// Sets on a session that was just opened or loaded what bzz decides
    /// for it, before anything is asked of it: the mode of the harness, and
    /// the model and thinking level its owner chose. An error means the
    /// session must not be prompted.
    pub(super) async fn guard_session(
        &self,
        run: &Run,
        session: &str,
        opened: &Value,
    ) -> std::result::Result<(), CallError> {
        if !self.is_coding(run) {
            return Ok(());
        }
        let harness = self.spec.local.harness;
        let limit = self.limits().session;
        let call = |method: &'static str, params: Value| async move {
            tokio::time::timeout(limit, run.connection.request(method, params))
                .await
                .unwrap_or(Err(CallError::Stalled))
        };
        let refuse = |kind: &str, why: String| {
            self.state().problem = Some(why.clone());
            CallError::Rpc(RpcError::new(acp::REFUSED, why).with_kind(kind))
        };
        if let Some(mode) = coding::required_mode(harness) {
            // A session that lists its modes without this one cannot take
            // it. One that lists none (a loaded session need not) says so
            // by its answer.
            if coding::offers_mode(opened, mode) == Some(false) {
                return Err(refuse(
                    "not_guarded",
                    format!(
                        "its program does not offer the mode `{mode}`, which bzz sets on every session: nothing was sent to it"
                    ),
                ));
            }
            match call(
                "session/set_mode",
                json!({"sessionId": session, "modeId": mode}),
            )
            .await
            {
                Ok(_) => {}
                Err(CallError::Rpc(_)) => {
                    return Err(refuse(
                        "not_guarded",
                        format!(
                            "its program refused the mode `{mode}`, which bzz sets on every session: nothing was sent to it"
                        ),
                    ));
                }
                Err(other) => return Err(other),
            }
        }
        let Some(model) = &self.spec.local.model else {
            return Ok(());
        };
        let unavailable = |offered: &[String]| {
            refuse(
                "model_unavailable",
                format!(
                    "its program does not offer the model `{}`{}",
                    crate::protocol::artifact::display(&model.model_id, 80),
                    if offered.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "; it offers: {}",
                            crate::protocol::artifact::display(&offered.join(", "), 400)
                        )
                    }
                ),
            )
        };
        let mut options = opened.get("configOptions").cloned();
        let (config_id, value) =
            match coding::pick_option(options.as_ref(), "model", &model.model_id) {
                Pick::Set { config_id, value } => (config_id, value),
                Pick::NotOffered { offered } => return Err(unavailable(&offered)),
                // A session that lists no options: asked for, and the program
                // says whether it has it.
                Pick::NoOption => ("model".to_owned(), model.model_id.clone()),
            };
        match call(
            "session/set_config_option",
            json!({"sessionId": session, "configId": config_id, "value": value}),
        )
        .await
        {
            // What a model offers may differ from the one before it.
            Ok(answer) => {
                if let Some(now) = answer.get("configOptions") {
                    options = Some(now.clone());
                }
            }
            Err(CallError::Rpc(_)) => return Err(unavailable(&[])),
            Err(other) => return Err(other),
        }
        if let Some(level) = &model.thinking_level {
            let set = match coding::pick_option(options.as_ref(), "thought_level", level) {
                Pick::Set { config_id, value } => call(
                    "session/set_config_option",
                    json!({"sessionId": session, "configId": config_id, "value": value}),
                )
                .await
                .is_ok(),
                Pick::NotOffered { .. } | Pick::NoOption => false,
            };
            // Not a reason to stay silent: the model answers at the level
            // its program gives it.
            if !set {
                self.state().problem = Some(format!(
                    "its program does not offer the thinking level `{}` for its model; it uses its own",
                    crate::protocol::artifact::display(level, 40)
                ));
            }
        }
        Ok(())
    }

    /// The folders this agent has now: those bzz started it with that the
    /// settings file still grants, each with what both allow. And whether
    /// anyone may wake it, by either. `Err`: the settings cannot be read,
    /// and nothing is allowed.
    fn folders_now(&self) -> std::result::Result<(Vec<FolderGrant>, bool), &'static str> {
        let started = &self.spec.local;
        let Some(now) = self.settings_now()? else {
            return Ok((started.folders.clone(), started.wake == WakeRule::Anyone));
        };
        let folders = started
            .folders
            .iter()
            .filter_map(|folder| {
                let still = now
                    .folders
                    .iter()
                    .find(|granted| granted.name == folder.name && granted.path == folder.path)?;
                Some(FolderGrant {
                    name: folder.name.clone(),
                    path: folder.path.clone(),
                    write: folder.write && still.write,
                    exec: weaker(folder.exec, still.exec),
                })
            })
            .collect();
        Ok((
            folders,
            started.wake == WakeRule::Anyone || now.wake == WakeRule::Anyone,
        ))
    }

    /// The turn a request of `session` belongs to, when it is running in
    /// the process `run` and was not stopped: its channel, its place, and
    /// who it answers.
    fn live_turn(&self, run: u64, session: &str) -> Option<(Uuid, String, String)> {
        if self.halted() {
            return None;
        }
        let state = self.state();
        state
            .turns
            .get(session)
            .filter(|turn| turn.run == run && !turn.stopped)
            .map(|turn| {
                (
                    turn.row.channel_id,
                    turn.row.place.clone(),
                    turn.row.author.clone(),
                )
            })
    }

    /// Answers `session/request_permission` of a coding harness, by rule.
    /// The answer allows once or refuses once; a request bzz cannot read is
    /// refused.
    pub(super) async fn permission_request(
        self: &Arc<Self>,
        run: &Arc<Run>,
        params: &Value,
    ) -> Value {
        let session = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // What the harness reported of the call it asks leave for: a
        // request may name only the call's id.
        let reported = params
            .pointer("/toolCall/toolCallId")
            .and_then(Value::as_str)
            .and_then(|id| {
                let state = self.state();
                state
                    .turns
                    .get(&session)
                    .filter(|turn| turn.run == run.id)
                    .and_then(|turn| turn.calls.get(id).cloned())
            });
        let wanted = permission_rule::read(params, reported.as_ref());
        // bzz's own chat tools: what they may do is decided where the call
        // arrives.
        if wanted == Wanted::ChatTool {
            return permission_rule::answer(params, true);
        }
        let refused = |asked: String, why: &str| {
            self.record(&activity::Entry::request(&asked, format!("refused: {why}")));
            false
        };
        let folders = |agent: &Self| {
            agent
                .folders_now()
                .map(|(folders, _)| Roots::new(&agent.workspace(), &folders))
        };
        let allow = match &wanted {
            Wanted::ChatTool => true,
            Wanted::Execute { command, cwd } => {
                self.coding_command(run.id, &session, command, cwd.as_deref())
                    .await
            }
            Wanted::Unclear(what) => refused(what.clone(), "bzz has no rule that allows it"),
            Wanted::Edit { paths } | Wanted::Read { paths } => {
                let edit = matches!(wanted, Wanted::Edit { .. });
                let asked = format!(
                    "{}: {}",
                    if edit { "a file edit" } else { "a read" },
                    shown(paths)
                );
                match folders(self) {
                    _ if self.live_turn(run.id, &session).is_none() => {
                        refused(asked, "no turn is running for it")
                    }
                    Err(why) => refused(asked, why),
                    Ok(roots) if edit && roots.may_write(paths) => true,
                    Ok(roots) if !edit && roots.may_read(paths) => true,
                    Ok(_) if edit => refused(
                        asked,
                        "outside its working folder and the folders granted for writing",
                    ),
                    Ok(_) => refused(asked, "outside its working folder and its granted folders"),
                }
            }
        };
        if !allow
            && let Some(turn) = self
                .state()
                .turns
                .get_mut(&session)
                .filter(|turn| turn.run == run.id)
        {
            turn.refused = true;
        }
        permission_rule::answer(params, allow)
    }

    /// Whether a shell command a coding harness asks leave for may run: by
    /// the folders granted for commands, in a turn that is live, and,
    /// unless every such folder is allowed without asking, only after the
    /// owner said so in the TUI. The rule of [`Agent::approve`], for a
    /// program that starts its commands itself.
    ///
    /// bzz answers the program's request and starts nothing: what the
    /// owner is shown is the command as the program reports it.
    async fn coding_command(
        self: &Arc<Self>,
        run: u64,
        session: &str,
        command: &str,
        cwd: Option<&Path>,
    ) -> bool {
        let workspace = self.workspace();
        let at = cwd.map_or_else(|| workspace.clone(), Path::to_path_buf);
        let note = |place: &Path, answer: String| {
            self.record(&activity::Entry::command_at(place, command, answer));
        };
        if command.is_empty()
            || command.contains('\0')
            || command.len() > approval::MAX_COMMAND_BYTES
        {
            note(&at, "refused: not a command bzz can put to you".into());
            return false;
        }
        let (folders, anyone) = match self.folders_now() {
            Ok(now) => now,
            Err(why) => {
                note(&at, format!("refused: {why}"));
                return false;
            }
        };
        let place = match permission_rule::command_place(&folders, &workspace, cwd) {
            Ok(place) => place,
            Err(why) => {
                note(&at, format!("refused: {why}"));
                return false;
            }
        };
        // "Without asking" holds only while the people who may wake the
        // agent are ones its owner named.
        let asks = place.asks || anyone;
        let Some((channel, conversation, requester)) = self.live_turn(run, session) else {
            note(&place.path, "refused: no turn is running for it".into());
            return false;
        };
        if !asks {
            note(&place.path, "allowed without asking".into());
            return true;
        }
        let limit = self.limits().approval;
        let asked = Asked {
            id: 0,
            agent: self.spec.id,
            agent_name: self.spec.name.clone(),
            channel,
            place: conversation,
            requester,
            folder: place.folders.join(", "),
            path: place.path.clone(),
            command: command.to_owned(),
            coding: true,
            asked_at: unix_now(),
            expires: Instant::now() + limit,
        };
        let (id, outcome) = match self.shared.approvals.ask(asked, session) {
            Ok(opened) => opened,
            Err(NotAsked::NobodyToAsk) => {
                note(&place.path, "refused: nobody was at bzz to ask".into());
                return false;
            }
            Err(NotAsked::OneAtATime) => {
                note(
                    &place.path,
                    "refused: another command of this conversation waits for your answer".into(),
                );
                return false;
            }
        };
        note(&place.path, "asked of you".into());
        let first_line =
            crate::render::sanitize::visible(command.split('\n').next().unwrap_or_default());
        if let Some(turn) = self.state().turns.get_mut(session) {
            turn.activity = Some(
                format!(
                    "waiting for your answer: run in {}: {first_line}",
                    place.path.display()
                )
                .chars()
                .take(120)
                .collect(),
            );
        }
        // The turn may have ended between the look and the question.
        if self.live_turn(run, session).is_none() {
            self.shared.approvals.close(id);
            note(&place.path, "void: the turn ended".into());
            return false;
        }
        tokio::pin!(outcome);
        let ended = tokio::select! {
            biased;
            ended = &mut outcome => ended.ok(),
            () = tokio::time::sleep(limit) => {
                if self.shared.approvals.close(id) {
                    None
                } else {
                    // Answered at that very moment: the answer counts.
                    (&mut outcome).await.ok()
                }
            }
        };
        if let Some(turn) = self.state().turns.get_mut(session) {
            turn.activity = Some(first_line.chars().take(80).collect());
        }
        match ended {
            // A yes for a turn that has gone meanwhile allows nothing.
            Some(Outcome::Allowed) if self.live_turn(run, session).is_some() => {
                note(&place.path, "allowed by you".into());
                true
            }
            Some(Outcome::Allowed) => {
                note(
                    &place.path,
                    "void: the turn ended before your answer".into(),
                );
                false
            }
            Some(Outcome::Refused(reason)) => {
                note(
                    &place.path,
                    match reason {
                        Some(reason) => format!("refused by you: {reason}"),
                        None => "refused by you".to_owned(),
                    },
                );
                false
            }
            Some(Outcome::Void(why)) => {
                note(&place.path, format!("void: {why}"));
                false
            }
            None => {
                note(
                    &place.path,
                    format!("refused: no answer in {} s", limit.as_secs()),
                );
                false
            }
        }
    }
}
