//! Approving and managing apps inside bzz (v0.16 M4): the approval of a
//! version on v0.15's overlay, and the commands beside `:app open|close|
//! zoom` that the command line has as `bzz app …`.
//!
//! **One implementation of each rule.** Every answer here is a call of
//! `apps::store::Apps`, the same the command line makes: approve of the
//! hash shown, refuse with a reason, roll back, enable, disable, remove,
//! propose a folder. What an answer changes in an open app is done by the
//! pane when it next asks the store (`App::sync_apps`, at once here and
//! every second for an answer given at the shell).
//!
//! ```text
//!   :approvals, Ctrl+Y      the version that has waited longest (after an
//!                           agent's command, which has a time limit)
//!   :app pending [name]     the same, or that app's
//!   :app approve <name>     shows it: the approval is given there, of the
//!                           hash shown, never by a command alone
//!   :app refuse <name> [reason…]
//!   :app rollback <name> [version]
//!   :app enable|disable|remove <name>
//!   :app add <folder>       proposes the folder as yours, then shows it
//!   :app new <name> [template]   writes ./<name> from a template
//!   :app dev <folder>       the folder in this channel's pane, unapproved
//! ```

use crossterm::event::KeyEvent;
#[cfg(feature = "apps")]
use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::error::Result;
#[cfg(feature = "apps")]
use crate::ui::state::Overlay;

/// How the commands are written, for a mistake.
pub const USAGE: &str = "usage: :app open <name> | close | zoom | pending [name] | approve <name> | refuse <name> [reason] | rollback <name> [version] | enable <name> | disable <name> | remove <name> | add <folder> | new <name> [template] | dev <folder>";

#[cfg(not(feature = "apps"))]
impl App {
    pub(super) async fn app_review_key(&mut self, _key: KeyEvent) -> Result<bool> {
        Ok(false)
    }

    pub(super) async fn open_app_review(&mut self, _name: Option<&str>) -> Result<()> {
        self.status_error = Some("this bzz was built without apps".into());
        Ok(())
    }

    pub(super) async fn app_review_answer_key(&mut self, _key: KeyEvent) -> Result<()> {
        self.presentation.close_overlay();
        Ok(())
    }

    pub(super) async fn manage_command(&mut self, _verb: &str, _rest: &str) -> Result<bool> {
        self.status_error = Some("this bzz was built without apps".into());
        Ok(true)
    }
}

#[cfg(feature = "apps")]
impl App {
    /// `Ctrl+Y` when no agent's command waits and a version of an app does.
    /// `true`: the key was taken.
    pub(super) async fn app_review_key(&mut self, key: KeyEvent) -> Result<bool> {
        let asks = key.code == KeyCode::Char('y')
            && key.modifiers == KeyModifiers::CONTROL
            && self.approvals.is_empty()
            && !self.apps.waiting.is_empty()
            && !matches!(
                self.presentation.overlay,
                Some(Overlay::Approval | Overlay::AppApproval)
            );
        if asks {
            self.open_app_review(None).await?;
        }
        Ok(asks)
    }

    /// Opens the approval on the version of `name` that has waited
    /// longest, or on the one of any app. It is opened by the owner only.
    pub(super) async fn open_app_review(&mut self, name: Option<&str>) -> Result<()> {
        self.sync_apps().await?;
        let first = self
            .apps
            .waiting
            .iter()
            .find(|version| name.is_none_or(|name| version.name == name))
            .map(|version| (version.name.clone(), version.version));
        let Some((name, version)) = first else {
            self.status_error = Some(match name {
                Some(name) => format!("no version of {name} waits for your answer"),
                None => "no version of an app waits for your answer".into(),
            });
            return Ok(());
        };
        self.show_version(&name, version).await?;
        self.presentation.open_overlay(Overlay::AppApproval);
        Ok(())
    }

    /// Reads what the owner is shown of a version: `Apps::review`.
    async fn show_version(&mut self, name: &str, version: u32) -> Result<()> {
        let Some(community) = self.active_community_id() else {
            return Ok(());
        };
        let root = self.paths.apps_dir();
        let wanted = name.to_owned();
        let review = self
            .store
            .call(move |store| {
                Ok(crate::apps::store::Apps::at(store, root)
                    .review(community, &wanted, version)
                    .map_err(|error| error.to_string()))
            })
            .await?;
        self.apps.review.show(name, version, review);
        Ok(())
    }

    /// The version shown was answered elsewhere (at the shell), or replaced:
    /// the overlay says so and takes no answer.
    pub(super) fn review_still_waits(&mut self) {
        let Some((name, version)) = self.apps.review.selected.clone() else {
            return;
        };
        let waits = self
            .apps
            .waiting
            .iter()
            .any(|waiting| waiting.name == name && waiting.version == version);
        if !waits {
            self.apps.review.review = None;
            self.apps.review.reason = None;
        }
    }

    fn close_app_review(&mut self) {
        self.apps.review = crate::ui::app_approval::AppApprovalState::default();
        self.presentation.close_overlay();
    }

    /// A key in the approval: `a` approves the hash shown once all of it
    /// has been on screen, `r` refuses with a reason, `Tab` shows the next
    /// waiting version, `Esc` leaves it waiting.
    pub(super) async fn app_review_answer_key(&mut self, key: KeyEvent) -> Result<()> {
        let control = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let state = &mut self.apps.review;
        if state.reason.is_some() {
            match key.code {
                KeyCode::Esc => state.reason = None,
                KeyCode::Enter => {
                    let reason = state.reason.take().unwrap_or_default();
                    return self.refuse_shown(Some(reason)).await;
                }
                KeyCode::Backspace => {
                    if let Some(reason) = &mut state.reason {
                        reason.pop();
                    }
                }
                KeyCode::Char(character) if !control => {
                    state.type_reason(character.encode_utf8(&mut [0; 4]));
                }
                _ => {}
            }
            return Ok(());
        }
        let page = isize::try_from(state.page.max(1)).unwrap_or(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_app_review(),
            KeyCode::Char('j') | KeyCode::Down => state.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => state.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => state.scroll_by(page),
            KeyCode::PageUp => state.scroll_by(-page),
            KeyCode::Char('G') | KeyCode::End => state.scroll_by(isize::MAX),
            KeyCode::Char('g') | KeyCode::Home => state.scroll_by(isize::MIN),
            KeyCode::Tab => {
                let at = state.selected.clone();
                let waiting: Vec<(String, u32)> = self
                    .apps
                    .waiting
                    .iter()
                    .map(|version| (version.name.clone(), version.version))
                    .collect();
                let next = at
                    .and_then(|at| waiting.iter().position(|shown| *shown == at))
                    .map_or(0, |index| (index + 1) % waiting.len().max(1));
                if let Some((name, version)) = waiting.get(next).cloned() {
                    self.show_version(&name, version).await?;
                }
            }
            KeyCode::Char('a' | 'y') if !control => {
                if state.review.is_none() {
                    state.notice = Some("that version no longer waits; Esc closes this".into());
                } else if !state.seen_end {
                    // What is approved is what was shown.
                    state.notice = Some(
                        "scroll to the end first (j, PgDn): an approval is of all of it".into(),
                    );
                } else {
                    return self.approve_shown().await;
                }
            }
            KeyCode::Char('r' | 'n') if !control && state.review.is_some() => {
                state.reason = Some(String::new());
                state.notice = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Approves the version shown, by the hash shown (`Apps::approve`).
    async fn approve_shown(&mut self) -> Result<()> {
        let Some(review) = self.apps.review.review.clone() else {
            return Ok(());
        };
        let root = self.paths.apps_dir();
        let (community, name, version, hash) = (
            review.community,
            review.name.clone(),
            review.version,
            review.hash.clone(),
        );
        let approved = self
            .store
            .call(move |store| {
                Ok(crate::apps::store::Apps::at(store, root)
                    .approve(community, &name, version, &hash)
                    .map_err(|error| error.to_string()))
            })
            .await?;
        match approved {
            Ok(approved) => {
                let open = self.apps.open.values().any(|open| *open == review.name);
                let then = if open {
                    "where it is open it runs now in place of the version before".to_owned()
                } else {
                    format!(":app open {} opens it here", review.name)
                };
                self.status_error = Some(format!(
                    "{}: version {} approved, the script of hash {}…; {then}",
                    review.name,
                    approved.version,
                    &review.hash[..12.min(review.hash.len())],
                ));
                self.close_app_review();
                // The open app is given the version at once.
                self.sync_apps().await?;
            }
            Err(error) => self.apps.review.notice = Some(error),
        }
        Ok(())
    }

    async fn refuse_shown(&mut self, reason: Option<String>) -> Result<()> {
        let Some((name, version)) = self.apps.review.selected.clone() else {
            return Ok(());
        };
        let refused = self.refuse(&name, version, reason).await?;
        match refused {
            Ok(()) => {
                self.close_app_review();
                self.sync_apps().await?;
            }
            Err(error) => self.apps.review.notice = Some(error),
        }
        Ok(())
    }

    async fn refuse(
        &mut self,
        name: &str,
        version: u32,
        reason: Option<String>,
    ) -> Result<std::result::Result<(), String>> {
        let Some(community) = self.active_community_id() else {
            return Ok(Err("select a community first".into()));
        };
        let root = self.paths.apps_dir();
        let wanted = name.to_owned();
        let refused = self
            .store
            .call(move |store| {
                Ok(crate::apps::store::Apps::at(store, root)
                    .refuse(community, &wanted, version, reason.as_deref())
                    .map_err(|error| error.to_string()))
            })
            .await?;
        Ok(refused.map(|reason| {
            self.status_error = Some(match reason {
                Some(reason) => format!(
                    "{name}: version {version} refused (\"{}\"); nothing of it runs",
                    crate::render::sanitize::single_line(&reason)
                ),
                None => format!("{name}: version {version} refused; nothing of it runs"),
            });
        }))
    }

    /// `:app <verb> …` beyond open, close and zoom. `true`: it was one.
    pub(super) async fn manage_command(&mut self, verb: &str, rest: &str) -> Result<bool> {
        let mut words = rest.split_whitespace();
        let first = words.next();
        let second = words.next();
        match (verb, first) {
            ("pending", name) if second.is_none() => self.open_app_review(name).await?,
            ("approve", Some(name)) if second.is_none() => {
                self.open_app_review(Some(name)).await?;
            }
            ("refuse", Some(name)) => {
                let reason = rest.trim_start().strip_prefix(name).map(str::trim);
                let waiting = self
                    .apps
                    .waiting
                    .iter()
                    .find(|version| version.name == name)
                    .map(|version| version.version);
                self.sync_apps().await?;
                let waiting = waiting.or_else(|| {
                    self.apps
                        .waiting
                        .iter()
                        .find(|version| version.name == name)
                        .map(|version| version.version)
                });
                match waiting {
                    Some(version) => {
                        let reason = reason.filter(|reason| !reason.is_empty());
                        if let Err(error) = self
                            .refuse(name, version, reason.map(str::to_owned))
                            .await?
                        {
                            self.status_error = Some(error);
                        }
                        self.sync_apps().await?;
                    }
                    None => {
                        self.status_error =
                            Some(format!("no version of {name} waits for your answer"));
                    }
                }
            }
            ("rollback", Some(name)) => {
                let to = match second.map(str::parse::<u32>) {
                    None => None,
                    Some(Ok(version)) => Some(version),
                    Some(Err(_)) => {
                        self.status_error = Some("usage: :app rollback <name> [version]".into());
                        return Ok(true);
                    }
                };
                self.with_store(name, move |apps, community, name| {
                    apps.rollback(community, name, to).map(|(from, to)| match from {
                        Some(from) if from != to => format!(
                            "{name}: back to version {to} from {from}; where it is open it changes in place, keeping its state"
                        ),
                        _ => format!("{name}: version {to} runs already"),
                    })
                })
                .await?;
            }
            ("enable", Some(name)) | ("disable", Some(name)) if second.is_none() => {
                let enabled = verb == "enable";
                self.with_store(name, move |apps, community, name| {
                    apps.set_enabled(community, name, enabled)
                        .map(|changed| match (changed, enabled) {
                            (false, true) => format!("{name} is enabled already"),
                            (false, false) => format!("{name} is disabled already"),
                            (true, true) => format!(
                                "{name} is enabled: it runs again where it is open, and :app open {name} opens it"
                            ),
                            (true, false) => format!(
                                "{name} is disabled: it is closed wherever it was open, and runs nowhere until :app enable {name}"
                            ),
                        })
                })
                .await?;
            }
            ("remove", Some(name)) if second.is_none() => {
                self.with_store(name, |apps, community, name| {
                    apps.remove(community, name).map(|removed| {
                        format!(
                            "{name} is uninstalled: every version bzz kept of it is deleted. {} record{} of types {}* that this bzz has stored stay in the channel{}; an app of the same name installed again finds them",
                            removed.records,
                            if removed.records == 1 { "" } else { "s" },
                            removed.type_prefix,
                            match removed.record_channels {
                                0 | 1 => String::new(),
                                n => format!(" ({n} channels)"),
                            }
                        )
                    })
                })
                .await?;
            }
            ("add", Some(_)) => {
                let path = expand(rest.trim());
                self.propose_folder(&path).await?;
            }
            ("new", Some(name)) if words.next().is_none() => {
                let template = second.unwrap_or(crate::apps::store::templates::DEFAULT);
                self.status_error = Some(match crate::apps::store::templates::find(template) {
                    None => format!(
                        "no template {template}; the templates are {}",
                        crate::apps::store::templates::names()
                    ),
                    Some(found) => {
                        let dir = std::path::PathBuf::from(name);
                        match found.write(name, None, &dir) {
                            Ok(_) => format!(
                                "wrote ./{name} from the template {template}: app.lua, app.toml, brief.md. :app dev {name} runs it here; :app add {name} proposes it"
                            ),
                            Err(error) => error,
                        }
                    }
                });
            }
            ("dev", Some(_)) => {
                let path = expand(rest.trim());
                self.open_dev_here(&path).await?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// One call of the store for the app `name` of the community, whose
    /// answer goes on the status line; what it changed reaches the open
    /// apps at once.
    async fn with_store<F>(&mut self, name: &str, call: F) -> Result<()>
    where
        F: FnOnce(
                &mut crate::apps::store::Apps<'_>,
                uuid::Uuid,
                &str,
            ) -> crate::apps::store::AppResult<String>
            + Send
            + 'static,
    {
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("select a community first".into());
            return Ok(());
        };
        let root = self.paths.apps_dir();
        let wanted = name.to_owned();
        let said = self
            .store
            .call(move |store| {
                let mut apps = crate::apps::store::Apps::at(store, root);
                Ok(call(&mut apps, community, &wanted).map_err(|error| error.to_string()))
            })
            .await?;
        // What the command did is said, over what the pane then says of it.
        self.sync_apps().await?;
        self.status_error = Some(said.unwrap_or_else(|error| error));
        Ok(())
    }

    /// `:app add <folder>`: the folder as a version proposed by the owner,
    /// loaded apart first (off this loop's thread), then shown for an
    /// answer. One that does not load is recorded as failed and never
    /// shown.
    async fn propose_folder(&mut self, path: &std::path::Path) -> Result<()> {
        use crate::apps::store::{Proposer, Recorded, examine, folder::Folder};
        let Some(community) = self.active_community_id() else {
            self.status_error = Some("select a community first".into());
            return Ok(());
        };
        let folder = match Folder::read(path) {
            Ok(folder) => folder,
            Err(error) => {
                self.status_error = Some(error);
                return Ok(());
            }
        };
        let submission = folder.submission(Proposer::owner());
        let name = submission.name.clone();
        let examined = tokio::task::spawn_blocking(move || examine(submission))
            .await
            .map_err(|error| crate::error::Error::Config(format!("loading the app: {error}")))?;
        let examined = match examined {
            Ok(examined) => examined,
            Err(error) => {
                self.status_error = Some(error.to_string());
                return Ok(());
            }
        };
        let root = self.paths.apps_dir();
        let recorded = self
            .store
            .call(move |store| {
                Ok(crate::apps::store::Apps::at(store, root)
                    .record(community, examined)
                    .map_err(|error| error.to_string()))
            })
            .await?;
        match recorded {
            Ok(Recorded::Waiting { version, .. }) => {
                self.sync_apps().await?;
                self.show_version(&name, version).await?;
                self.presentation.open_overlay(Overlay::AppApproval);
            }
            Ok(Recorded::Failed { version, error }) => {
                self.status_error = Some(format!(
                    "{name}: it does not load, and nothing was changed (recorded as version {version}, failed): {}",
                    error.lines().next().unwrap_or_default()
                ));
            }
            Ok(Recorded::Running { version }) => {
                self.status_error = Some(format!(
                    "{name}: the folder is version {version}, which runs: there is nothing to approve"
                ));
            }
            Err(error) => self.status_error = Some(error),
        }
        Ok(())
    }
}

/// A path as the owner typed it: `~/` is his home folder.
#[cfg(feature = "apps")]
fn expand(typed: &str) -> std::path::PathBuf {
    match (typed.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(typed),
    }
}
