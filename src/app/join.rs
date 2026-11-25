//! The join form inside bzz (v0.17 A9): the App's half. `ui::join` is the
//! form; this drives it with the CLI's own functions (`crate::join`,
//! `IdentityManager`, `auth::pairing`), so the interface and the command
//! line make a community the same way.
//!
//! ```text
//!   address ─ checked (NIP-11) ─ who you are ─ identity made or unlocked
//!     ─ invite claimed (when it is one) ─ AUTH asked ─┬─ in: written, opened
//!                                                     ├─ members only: the
//!                                                     │  npub, asked again
//!                                                     │  15 s, 30 s, 60 s…
//!                                                     └─ refused: said, and
//!                                                        what was made here
//!                                                        taken back
//! ```
//!
//! The seam for the first screen and the palette is [`App::open_join`].

use std::time::Instant;

use crossterm::event::KeyEvent;
use nostr::{Keys, ToBech32 as _};
use ratatui::{Frame, layout::Rect};
use secrecy::SecretString;

use super::{App, Background, Runtime};
use crate::{
    auth::{IdentityManager, keychain, pairing, signer::SignerHandle},
    config::{IdentityConfig, KeyBackend},
    error::Result,
    join::{self, Address, Admission, Claimed, RelayFacts},
    ui::{
        join::{Check, Field, JoinForm, JoinStart, Stage, Step, Who},
        state::Overlay,
    },
};

/// What bzz asks of this machine for the form, put aside so a test never
/// reaches the real keychain or a real Buzz desktop.
#[derive(Clone, Copy)]
pub(super) struct Lookups {
    pub keychain: fn() -> bool,
    pub desktop: fn() -> std::result::Result<String, String>,
}

impl Default for Lookups {
    fn default() -> Self {
        Self {
            keychain: keychain::available,
            desktop: || {
                pairing::find_desktop(None)
                    .map(|(instance, _)| instance)
                    .map_err(|error| desktop_unavailable(&error))
            },
        }
    }
}

/// Why pairing cannot be chosen, in the form's words.
fn desktop_unavailable(error: &crate::Error) -> String {
    match error {
        crate::Error::Config(said) if said.starts_with("no running") => {
            "Buzz desktop is not running here: start it, then come back".into()
        }
        crate::Error::Config(said) if said.starts_with("several") => {
            "several Buzz desktops run here: bzz identity pair-desktop --instance <name> picks one"
                .into()
        }
        crate::Error::Config(said) | crate::Error::Unsupported(said) => said.clone(),
        other => other.to_string(),
    }
}

/// The join form and the join it drives.
#[derive(Default)]
pub(super) struct JoinState {
    pub form: Option<JoinForm>,
    /// The signer of the identity being let in, while the relay is asked.
    signer: Option<SignerHandle>,
    /// The invite was claimed already: only AUTH is asked again.
    claimed: bool,
    /// Asked once per process (`keychain::available`).
    keychain: Option<bool>,
    /// A community just joined, until its channels are known: then the
    /// person lands in its first channel, or chooses one to open.
    landing: Option<(uuid::Uuid, Instant)>,
    pub lookups: Lookups,
}

/// A step of the join that finished in the background.
pub(super) enum JoinEvent {
    Checked {
        input: String,
        result: std::result::Result<(Address, RelayFacts), (String, String)>,
    },
    /// The identity exists (made, imported, paired or unlocked).
    Ready {
        identity: IdentityConfig,
        made_here: bool,
        signer: SignerHandle,
    },
    /// The invite's claim, when there was one, and the relay's answer.
    Asked {
        claimed: Option<Claimed>,
        admission: Admission,
    },
    Failed {
        what: String,
        next: String,
    },
}

impl std::fmt::Debug for JoinEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Checked { .. } => "JoinEvent::Checked",
            Self::Ready { .. } => "JoinEvent::Ready",
            Self::Asked { .. } => "JoinEvent::Asked",
            Self::Failed { .. } => "JoinEvent::Failed",
        })
    }
}

impl App {
    /// The seam the first screen, the `:` line and the palette call: opens
    /// the join form, or the join that is waiting for an admin.
    pub(super) fn open_join(&mut self, start: JoinStart) {
        if self.join_in_demo() {
            self.join.form = Some(JoinForm::demo());
            self.presentation.open_overlay(Overlay::Join);
            return;
        }
        let ongoing = self.join.form.as_ref().is_some_and(|form| {
            matches!(form.stage, Stage::Waiting | Stage::Working(_) | Stage::Keep)
        });
        if !ongoing {
            let keychain = *self
                .join
                .keychain
                .get_or_insert_with(self.join.lookups.keychain);
            let mut form = JoinForm::new(&start, keychain);
            if !form.address.is_empty() {
                form.continue_when_known = false;
                self.join.form = Some(form);
                self.spawn_join_check();
            } else {
                self.join.form = Some(form);
            }
        } else if matches!(start, JoinStart::Address(_)) {
            self.status_error =
                Some("a join is waiting for an admin; x on its screen stops it".into());
        }
        self.presentation.open_overlay(Overlay::Join);
    }

    /// Seam for `bzz demo` (agent `demo`'s `App::demo`): the form explains
    /// that it needs a real community and writes nothing (plan rule 22).
    /// When the two branches are joined this returns `self.demo`.
    fn join_in_demo(&self) -> bool {
        false
    }

    pub(super) fn render_join(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(form) = &self.join.form else {
            return;
        };
        let width = area.width.min(100);
        let height = area.height.min(30);
        let popup = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        crate::ui::join::render(frame, popup, form, &self.theme);
    }

    pub(super) async fn join_key(&mut self, key: KeyEvent) -> Result<()> {
        let Some(form) = &mut self.join.form else {
            self.presentation.close_overlay();
            return Ok(());
        };
        match form.key(key) {
            Step::Nothing => {}
            Step::Close => self.leave_join(),
            Step::Check => self.spawn_join_check(),
            Step::Start => self.start_join(),
            Step::Copy(npub) => {
                let said = match crate::ui::terminal::copy_osc52(&npub) {
                    Ok(_) => "your id is copied; paste it to an admin".to_owned(),
                    Err(error) => format!("copy unavailable here ({error}); select the id above"),
                };
                if let Some(form) = &mut self.join.form {
                    form.notice = Some(said);
                }
            }
            Step::RetryNow => self.spawn_admission(),
            Step::Abandon { keep, leave } => self.abandon_join(keep, leave).await,
        }
        Ok(())
    }

    /// The screen closes; a join at work or waiting goes on.
    fn leave_join(&mut self) {
        self.presentation.close_overlay();
        let Some(form) = &mut self.join.form else {
            return;
        };
        match form.stage {
            Stage::Waiting | Stage::Working(_) => {
                self.status_error = Some("the join goes on; :join shows it again".into());
            }
            Stage::Keep => form.stage = Stage::Waiting,
            _ => {
                form.wipe();
                self.join.form = None;
            }
        }
    }

    /// Stops the join: what it made is taken back unless `keep`.
    async fn abandon_join(&mut self, keep: bool, leave: bool) {
        let Some(form) = &mut self.join.form else {
            return;
        };
        let made = form.made_here.then(|| form.identity.clone()).flatten();
        form.back_to_address();
        if let Some(signer) = self.join.signer.take() {
            // Not the open community's signer: that one stays unlocked.
            if self
                .runtime
                .as_ref()
                .is_none_or(|runtime| Some(runtime.identity_id) != made.as_ref().map(|i| i.id))
            {
                signer.lock().await;
            }
        }
        self.join.claimed = false;
        if let Some(identity) = made {
            if keep {
                self.status_error = Some(format!(
                    "the identity {} stays; choose it when you join again",
                    crate::render::sanitize::single_line(&identity.label)
                ));
            } else if let Err(error) =
                IdentityManager::new(&self.paths).forget(&mut self.config, &identity)
            {
                self.status_error = Some(format!("the identity made for it stays: {error}"));
            }
        }
        if leave {
            self.join.form = None;
            self.presentation.close_overlay();
        }
    }

    /// Every 100 ms: the address after a pause, asking again while an
    /// admin has not let you in, and the landing in a community just
    /// joined. True when the screen changed.
    pub(super) async fn join_tick(&mut self) -> Result<bool> {
        if self.join.landing.is_some() {
            return self.land().await;
        }
        let now = Instant::now();
        let Some(form) = &self.join.form else {
            return Ok(false);
        };
        if form.check_due(now) {
            self.spawn_join_check();
            return Ok(true);
        }
        if form.stage == Stage::Waiting && !form.asking && form.next_try.is_some_and(|at| at <= now)
        {
            self.spawn_admission();
            return Ok(true);
        }
        Ok(false)
    }

    /// Once a joined community's channels are known: its first channel is
    /// opened, or, when the person is in none yet, the channel finder
    /// lists the ones they may open. Nothing is joined for them.
    async fn land(&mut self) -> Result<bool> {
        let Some((community, since)) = self.join.landing else {
            return Ok(false);
        };
        if self.active_community_id() != Some(community)
            || since.elapsed() > std::time::Duration::from_secs(30)
        {
            self.join.landing = None;
            return Ok(false);
        }
        if self.channels.is_empty()
            || self.connection != crate::domain::ConnectionState::Online
            || self.presentation.overlay.is_some()
        {
            return Ok(false);
        }
        self.join.landing = None;
        let first = self
            .ordered_channel_indexes()
            .into_iter()
            .find(|&index| self.channels[index].is_member);
        match first {
            Some(index) => {
                self.select_channel_index(index);
                self.showing_open_channel = false;
                self.load_selected_channel().await?;
                self.presentation
                    .set_workspace_focus(crate::ui::state::FocusSurface::Timeline);
            }
            None => {
                self.finder.clear();
                self.presentation.open_overlay(Overlay::Finder);
                self.status_error =
                    Some("you are in no channel yet: choose one to open, Enter".into());
            }
        }
        Ok(true)
    }

    /// Reads the address and asks what answers there.
    fn spawn_join_check(&mut self) {
        let Some(form) = &mut self.join.form else {
            return;
        };
        form.edited_at = None;
        let input = form.address.trim().to_owned();
        let address = match join::parse_address(&input) {
            Ok(address) => address,
            Err(what) => {
                form.check = Check::Bad {
                    what,
                    next: "a community's address looks like my-team.communities.buzz.xyz".into(),
                };
                form.continue_when_known = false;
                return;
            }
        };
        if let Some(label) = join::already_joined(&self.config, address.relay()) {
            form.check = Check::Bad {
                what: format!(
                    "you are in {} already",
                    crate::render::sanitize::single_line(&label)
                ),
                next: "it is in your communities; switch to it there".into(),
            };
            form.continue_when_known = false;
            return;
        }
        form.check = Check::Checking(input.clone());
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let result = join::probe(address.relay())
                .await
                .map(|facts| (address.clone(), facts))
                .map_err(|failure| {
                    let (what, next) = failure.explain(address.relay());
                    (what, next.to_owned())
                });
            let _ = tx
                .send(Background::Join(Box::new(JoinEvent::Checked {
                    input,
                    result,
                })))
                .await;
        });
    }

    pub(super) async fn join_event(&mut self, event: JoinEvent) -> Result<()> {
        match event {
            JoinEvent::Checked { input, result } => {
                let held = self.config.identities.clone();
                let keychain = self.join.keychain.unwrap_or(false);
                let desktop = self.join.lookups.desktop;
                let Some(form) = &mut self.join.form else {
                    return Ok(());
                };
                if form.address.trim() != input {
                    return Ok(());
                }
                match result {
                    Ok((address, facts)) => {
                        // The usual answer first; an identity already made
                        // for this community (a join left waiting) next.
                        let label = facts.label(address.relay());
                        let mut held = held;
                        held.sort_by_key(|identity| identity.label != label);
                        form.set_choices(crate::ui::join::choices(
                            &facts,
                            &held,
                            keychain,
                            desktop(),
                        ));
                        if form.chosen.is_none()
                            && let Some(index) = form.choices.iter().position(|choice| {
                                matches!(choice.who, Who::Held(id)
                                    if held.iter().any(|i| i.id == id && i.label == label))
                            })
                        {
                            form.selected = index;
                        }
                        form.check = Check::Known {
                            input,
                            address,
                            facts,
                        };
                        if std::mem::take(&mut form.continue_when_known) {
                            form.stage = Stage::Who;
                        }
                    }
                    Err((what, next)) => {
                        form.check = Check::Bad { what, next };
                        form.continue_when_known = false;
                    }
                }
            }
            JoinEvent::Ready {
                identity,
                made_here,
                signer,
            } => {
                if made_here
                    && !self
                        .config
                        .identities
                        .iter()
                        .any(|entry| entry.id == identity.id)
                {
                    // Written by the background step; this copy learns it.
                    self.config.identities.push(identity.clone());
                }
                let Some(form) = &mut self.join.form else {
                    return Ok(());
                };
                form.npub = nostr::PublicKey::from_hex(&identity.pubkey)
                    .ok()
                    .and_then(|key| key.to_bech32().ok());
                form.identity = Some(identity);
                form.made_here = made_here;
                self.join.signer = Some(signer);
                self.join.claimed = false;
                self.spawn_admission();
            }
            JoinEvent::Asked { claimed, admission } => {
                self.join_answered(claimed, admission).await?;
            }
            JoinEvent::Failed { what, next } => {
                if let Some(form) = &mut self.join.form {
                    form.asking = false;
                    form.stage = Stage::Failed { what, next };
                }
            }
        }
        Ok(())
    }

    /// Makes, imports, pairs or unlocks the identity the form chose.
    fn start_join(&mut self) {
        let keychain = self.join.keychain.unwrap_or(false);
        let Some(form) = &mut self.join.form else {
            return;
        };
        let Some((address, facts)) = form.known().map(|(a, f)| (a.clone(), f.clone())) else {
            form.stage = Stage::Address;
            return;
        };
        let Some(who) = form.chosen.clone() else {
            return;
        };
        let label = facts.label(address.relay());
        let tx = self.background_tx.clone();
        let paths = self.paths.clone();
        let mut config = self.config.clone();
        let send = move |event: JoinEvent| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(Background::Join(Box::new(event))).await;
            }
        };
        let failed = |what: String, next: &str| JoinEvent::Failed {
            what,
            next: next.to_owned(),
        };
        match who {
            Who::New | Who::Browser | Who::Import => {
                let keys = if who == Who::Import {
                    match Keys::parse(form.secret().trim()) {
                        Ok(keys) => keys,
                        Err(_) => {
                            form.wipe();
                            form.notice = Some("that is not a Nostr secret".into());
                            form.stage = Stage::Typing(Field::Secret);
                            return;
                        }
                    }
                } else {
                    Keys::generate()
                };
                let backend = if keychain {
                    KeyBackend::Keychain
                } else {
                    KeyBackend::EncryptedFile
                };
                let passphrase =
                    (!keychain).then(|| SecretString::from(form.passphrase().as_str().to_owned()));
                form.wipe();
                if config
                    .identities
                    .iter()
                    .any(|identity| identity.pubkey == keys.public_key().to_hex())
                {
                    form.stage = Stage::Failed {
                        what: "bzz holds this key already".into(),
                        next: "choose it in the list instead of importing it again".into(),
                    };
                    return;
                }
                form.stage = Stage::Working(if keychain {
                    "making your identity; its key goes in this system's keychain".into()
                } else {
                    "making your identity; its key goes in a file locked with your passphrase"
                        .into()
                });
                tokio::spawn(async move {
                    let made = tokio::task::spawn_blocking(move || {
                        let manager = IdentityManager::new(&paths);
                        let identity = manager.import_keys(
                            &mut config,
                            label,
                            backend,
                            keys.clone(),
                            passphrase.as_ref(),
                        )?;
                        manager.save_new(&mut config, &identity)?;
                        Ok::<_, crate::Error>((identity, keys))
                    })
                    .await;
                    send(match made {
                        Ok(Ok((identity, keys))) => JoinEvent::Ready {
                            identity,
                            made_here: true,
                            signer: SignerHandle::spawn(keys),
                        },
                        Ok(Err(error)) => failed(
                            format!("the identity could not be made: {error}"),
                            "nothing was kept; Enter to try again",
                        ),
                        Err(_) => failed(
                            "the identity could not be made".into(),
                            "nothing was kept; Enter to try again",
                        ),
                    })
                    .await;
                });
            }
            Who::Held(id) => {
                let Some(identity) = config.identities.iter().find(|i| i.id == id).cloned() else {
                    form.stage = Stage::Who;
                    return;
                };
                let passphrase = (identity.backend == KeyBackend::EncryptedFile)
                    .then(|| SecretString::from(form.passphrase().as_str().to_owned()));
                form.wipe();
                if let Some(runtime) = self
                    .runtime
                    .as_ref()
                    .filter(|runtime| runtime.identity_id == id)
                {
                    let signer = runtime.signer.clone();
                    form.stage = Stage::Working("asking the community".into());
                    let event = JoinEvent::Ready {
                        identity,
                        made_here: false,
                        signer,
                    };
                    tokio::spawn(async move { send(event).await });
                    return;
                }
                form.stage = Stage::Working(format!(
                    "unlocking {}",
                    crate::render::sanitize::single_line(&identity.label)
                ));
                tokio::spawn(async move {
                    let unlocked = tokio::task::spawn_blocking(move || {
                        IdentityManager::new(&paths)
                            .signer(&identity, passphrase.as_ref())
                            .map(|signer| (identity, signer))
                    })
                    .await;
                    send(match unlocked {
                        Ok(Ok((identity, signer))) => JoinEvent::Ready {
                            identity,
                            made_here: false,
                            signer,
                        },
                        Ok(Err(error)) => failed(
                            format!("that identity could not be unlocked: {error}"),
                            "check its passphrase, or choose another identity",
                        ),
                        Err(_) => failed(
                            "that identity could not be unlocked".into(),
                            "choose another identity",
                        ),
                    })
                    .await;
                });
            }
            Who::Pair => {
                form.stage = Stage::Working(
                    "asking Buzz desktop to pair: approve the request in Buzz".into(),
                );
                let relay = address.relay().url.clone();
                tokio::spawn(async move {
                    let paired = async {
                        let (instance, url) = pairing::find_desktop(None)?;
                        let identity =
                            pairing::pair(&paths, &mut config, label, &relay, &instance, url)
                                .await?;
                        let signer = IdentityManager::new(&paths).signer(&identity, None)?;
                        Ok::<_, crate::Error>((identity, signer))
                    }
                    .await;
                    send(match paired {
                        Ok((identity, signer)) => JoinEvent::Ready {
                            identity,
                            made_here: true,
                            signer,
                        },
                        Err(error) => failed(
                            format!("pairing with Buzz desktop did not work: {error}"),
                            "check that Buzz desktop runs and that you approved the request",
                        ),
                    })
                    .await;
                });
            }
        }
    }

    /// Claims the invite (once), then asks the relay to let the identity in.
    fn spawn_admission(&mut self) {
        let claim = !self.join.claimed;
        let Some(signer) = self.join.signer.clone() else {
            return;
        };
        let Some(form) = &mut self.join.form else {
            return;
        };
        let Some((address, facts)) = form.known().map(|(a, f)| (a.clone(), f.clone())) else {
            return;
        };
        form.asking = true;
        form.next_try = None;
        if form.stage != Stage::Waiting {
            form.stage = Stage::Working(format!(
                "asking {} whether you may come in",
                facts.label(address.relay())
            ));
        }
        let tx = self.background_tx.clone();
        tokio::spawn(async move {
            let claimed = match address.invite() {
                Some(code) if claim => {
                    Some(join::claim_invite(address.relay(), &signer, code).await)
                }
                _ => None,
            };
            let admission = match &claimed {
                Some(Claimed::Refused(_) | Claimed::NoAnswer) => Admission::NotMember,
                _ => join::admission(address.relay(), &signer).await,
            };
            let _ = tx
                .send(Background::Join(Box::new(JoinEvent::Asked {
                    claimed,
                    admission,
                })))
                .await;
        });
    }

    async fn join_answered(
        &mut self,
        claimed: Option<Claimed>,
        admission: Admission,
    ) -> Result<()> {
        let Some(form) = &mut self.join.form else {
            return Ok(());
        };
        form.asking = false;
        let Some((address, facts)) = form.known().map(|(a, f)| (a.clone(), f.clone())) else {
            return Ok(());
        };
        let name = facts.label(address.relay());
        match claimed {
            Some(Claimed::Member) => self.join.claimed = true,
            Some(Claimed::Refused(reason)) => {
                form.stage = Stage::Failed {
                    what: reason,
                    next: "ask an admin for a new invite, or join with the address alone".into(),
                };
                return Ok(());
            }
            Some(Claimed::NoAnswer) => {
                form.stage = Stage::Failed {
                    what: format!("no answer from {} to the invite", address.relay().host()),
                    next: "check this machine's network, then Enter to try again".into(),
                };
                return Ok(());
            }
            None => {}
        }
        let waiting = form.stage == Stage::Waiting;
        match admission {
            Admission::In => return self.finish_join().await,
            Admission::SignInFirst if form.chosen == Some(Who::Browser) => {
                return self.finish_join().await;
            }
            Admission::SignInFirst => {
                form.stage = Stage::Failed {
                    what: format!("{name} signs people in through its identity provider"),
                    next: "choose \"Sign in through the browser\" with the address of its sign-in service"
                        .into(),
                };
            }
            Admission::NotMember => {
                form.attempts += 1;
                form.next_try = Some(Instant::now() + crate::ui::join::retry_after(form.attempts));
                form.stage = Stage::Waiting;
                if !waiting && self.presentation.overlay != Some(Overlay::Join) {
                    self.status_error = Some(format!(
                        "{name} lets in its members only; :join shows your id for its admins"
                    ));
                }
            }
            Admission::Refused(said) => {
                form.stage = Stage::Failed {
                    what: format!("{name} refused this identity ({said})"),
                    next: "ask one of its admins, or choose another identity".into(),
                };
            }
            Admission::NoAnswer(said) if waiting => {
                form.attempts += 1;
                form.next_try = Some(Instant::now() + crate::ui::join::retry_after(form.attempts));
                form.notice = Some(format!("no answer this time ({said}); bzz asks again"));
            }
            Admission::NoAnswer(said) => {
                form.stage = Stage::Failed {
                    what: format!(
                        "no answer from {} while signing in ({said})",
                        address.relay().host()
                    ),
                    next: "check this machine's network, then Enter to try again".into(),
                };
            }
        }
        Ok(())
    }

    /// The relay lets the identity in: the community is written, and opened
    /// when the person is on this screen or has no other one open.
    async fn finish_join(&mut self) -> Result<()> {
        let Some(mut form) = self.join.form.take() else {
            return Ok(());
        };
        let (Some((address, facts)), Some(identity)) = (
            form.known().map(|(a, f)| (a.clone(), f.clone())),
            form.identity.clone(),
        ) else {
            return Ok(());
        };
        let label = facts.label(address.relay());
        let written = join::add_community(
            &self.paths,
            &mut self.config,
            label.clone(),
            address.relay(),
            identity.id,
            &facts.relay_pubkey,
        );
        let id = match written {
            Ok(id) => id,
            Err(error) => {
                form.stage = Stage::Failed {
                    what: format!("{label} let you in, but bzz could not keep it: {error}"),
                    next: "Enter to try again".into(),
                };
                self.join.form = Some(form);
                return Ok(());
            }
        };
        let browser = form.chosen == Some(Who::Browser);
        if browser && let Some(community) = self.config.communities.iter_mut().find(|c| c.id == id)
        {
            community.identity_adapter = Some(form.adapter.trim().to_owned());
            if let Err(error) = self.config.save(&self.paths) {
                self.status_error = Some(error.to_string());
            }
        }
        form.wipe();
        let on_screen = self.presentation.overlay == Some(Overlay::Join);
        if on_screen {
            self.presentation.close_overlay();
        }
        let signer = self.join.signer.take();
        self.join.claimed = false;
        let Some(index) = self.config.communities.iter().position(|c| c.id == id) else {
            return Ok(());
        };
        if !on_screen && self.runtime.is_some() {
            if let Some(signer) = signer
                && self
                    .runtime
                    .as_ref()
                    .is_none_or(|runtime| runtime.identity_id != identity.id)
            {
                signer.lock().await;
            }
            self.status_error = Some(format!(
                "{} let you in: it is in your communities now",
                crate::render::sanitize::single_line(&label)
            ));
            return Ok(());
        }
        let reuse = signer.or_else(|| {
            self.runtime
                .as_ref()
                .filter(|runtime| runtime.identity_id == identity.id)
                .map(|runtime| runtime.signer.clone())
        });
        match Runtime::build(
            &self.config,
            index,
            &self.paths,
            self.store.clone(),
            reuse,
            self.diagnostics.clone(),
        ) {
            Ok(runtime) => {
                self.adopt_runtime(index, runtime).await?;
                self.join.landing = Some((id, Instant::now()));
                self.status_error = Some(format!(
                    "you are in {}",
                    crate::render::sanitize::single_line(&label)
                ));
                if browser {
                    self.begin_federated_sign_in().await;
                }
            }
            Err(error) => self.status_error = Some(error.to_string()),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/support/scripted_relay.rs"]
mod scripted_relay;

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use nostr::JsonUtil as _;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;
    use tempfile::TempDir;

    use super::scripted_relay::{Script, ScriptedRelay, WsAction, default_frame};
    use super::*;
    use crate::{config::Config, paths::Paths, store::Store, store::writer::StoreHandle};

    const PASSPHRASE: &str = "a newcomer's long passphrase";

    async fn empty_app(temporary: &TempDir) -> App {
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let config = Config::default();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = App::new(config, paths, handle).await.unwrap();
        // Never this machine's keychain or its Buzz desktop.
        app.join.lookups = Lookups {
            keychain: || false,
            desktop: || Err("Buzz desktop is not running here: start it, then come back".into()),
        };
        app
    }

    /// A relay that lets in everyone when `member` is set, and answers
    /// "not a relay member" (or a ban, with `banned`) otherwise.
    async fn relay(member: Arc<AtomicBool>, banned: bool) -> ScriptedRelay {
        let info = json!({
            "name": "My team", "description": "We ship.", "self": "ab".repeat(32),
            "supported_nips": [1, 11, 29, 42, 43]
        });
        ScriptedRelay::start(Script::default().nip11(info).frames(move |frame| {
            if frame.value[0] == "AUTH" && !member.load(Ordering::SeqCst) {
                let event = nostr::Event::from_json(frame.value[1].to_string()).unwrap();
                let said = if banned {
                    "blocked: you are banned"
                } else {
                    "restricted: not a relay member"
                };
                return vec![WsAction::Send(json!([
                    "OK",
                    event.id.to_hex(),
                    false,
                    said
                ]))];
            }
            default_frame(frame)
        }))
        .await
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    async fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            app.join_key(key(KeyCode::Char(character))).await.unwrap();
        }
    }

    /// Runs background events until one of the join's is handled.
    async fn next_join_step(app: &mut App) {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(30), app.background_rx.recv())
                .await
                .expect("a join step in time")
                .expect("the lane is open");
            if let Background::Join(event) = event {
                app.join_event(*event).await.unwrap();
                return;
            }
        }
    }

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// From `:join <address>` to the address checked, a new identity with
    /// a passphrase (no keychain here), and the relay asked once.
    async fn join_with_a_new_identity(app: &mut App, relay: &ScriptedRelay) {
        let port = relay.ws_url.port().unwrap();
        app.command = format!("join 127.0.0.1:{port}");
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Join));
        next_join_step(app).await;
        let shown = screen(app, 80, 24);
        assert!(shown.contains("✓ My team · a Buzz community"), "{shown}");
        assert!(shown.contains("it lets in its members only"), "{shown}");
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        let shown = screen(app, 80, 24);
        assert!(shown.contains("A new identity, made here"), "{shown}");
        assert!(shown.contains("no keychain: key in a file"), "{shown}");
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        type_text(app, PASSPHRASE).await;
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        type_text(app, PASSPHRASE).await;
        let shown = screen(app, 80, 24);
        assert!(!shown.contains(PASSPHRASE), "{shown}");
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        assert!(matches!(
            app.join.form.as_ref().unwrap().stage,
            Stage::Working(_)
        ));
        next_join_step(app).await; // the identity
        next_join_step(app).await; // the relay's answer
    }

    /// Every file bzz wrote, as text: no secret may be in any of them.
    fn written(temporary: &TempDir) -> String {
        let mut text = String::new();
        let mut folders = vec![temporary.path().to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    text.push_str(&String::from_utf8_lossy(&std::fs::read(&path).unwrap()));
                }
            }
        }
        text
    }

    #[tokio::test]
    async fn a_newcomer_joins_an_open_community_and_lands_in_it() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let relay = relay(Arc::new(AtomicBool::new(true)), false).await;
        join_with_a_new_identity(&mut app, &relay).await;
        assert!(app.join.form.is_none(), "{:?}", app.join.form);
        assert_eq!(app.presentation.overlay, None);
        assert_eq!(app.status_error.as_deref(), Some("you are in My team"));
        let saved = Config::load(&app.paths).unwrap();
        assert_eq!(saved.communities.len(), 1);
        assert_eq!(saved.identities.len(), 1);
        assert_eq!(saved.identities[0].label, "My team");
        assert_eq!(saved.identities[0].backend, KeyBackend::EncryptedFile);
        assert_eq!(saved.communities[0].identity_id, saved.identities[0].id);
        let runtime = app.runtime.as_ref().expect("the community is open");
        assert_eq!(runtime.community_id, saved.communities[0].id);
        assert!(!written(&temporary).contains(PASSPHRASE));
        app.shutdown().await;
        relay.stop();
    }

    #[tokio::test]
    async fn a_members_only_community_shows_the_npub_and_lets_in_without_a_restart() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let member = Arc::new(AtomicBool::new(false));
        let relay = relay(member.clone(), false).await;
        join_with_a_new_identity(&mut app, &relay).await;
        let form = app.join.form.as_ref().unwrap();
        assert_eq!(form.stage, Stage::Waiting);
        let npub = form.npub.clone().unwrap();
        assert!(npub.starts_with("npub1"));
        let shown = screen(&mut app, 80, 24);
        assert!(shown.contains(&npub), "{shown}");
        assert!(shown.contains("lets in its members only"), "{shown}");
        assert!(shown.contains("c copy your id"), "{shown}");
        // Nothing of the community is written while it waits; the identity
        // is, since its id was given out.
        let saved = Config::load(&app.paths).unwrap();
        assert!(saved.communities.is_empty());
        assert_eq!(saved.identities.len(), 1);
        // The person may leave the screen: bzz goes on asking.
        app.join_key(key(KeyCode::Esc)).await.unwrap();
        assert_eq!(app.presentation.overlay, None);
        assert!(app.join.form.is_some());
        let next = app.join.form.as_ref().unwrap().next_try.unwrap();
        assert!(
            next > Instant::now() + Duration::from_secs(10),
            "a calm pace"
        );
        // Coming back shows the same wait.
        app.open_join(JoinStart::Join);
        assert_eq!(app.join.form.as_ref().unwrap().stage, Stage::Waiting);
        // The admin adds them; `r` asks now, and bzz goes in.
        member.store(true, Ordering::SeqCst);
        app.join_key(key(KeyCode::Char('r'))).await.unwrap();
        next_join_step(&mut app).await;
        assert!(app.join.form.is_none());
        let saved = Config::load(&app.paths).unwrap();
        assert_eq!(saved.communities.len(), 1);
        assert!(app.runtime.is_some());
        assert!(relay.frames_of("AUTH").len() >= 2);
        app.shutdown().await;
        relay.stop();
    }

    #[tokio::test]
    async fn the_wait_asks_again_on_its_own() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let member = Arc::new(AtomicBool::new(false));
        let relay = relay(member.clone(), false).await;
        join_with_a_new_identity(&mut app, &relay).await;
        app.join_key(key(KeyCode::Esc)).await.unwrap();
        // Nothing is asked before its time.
        assert!(!app.join_tick().await.unwrap());
        member.store(true, Ordering::SeqCst);
        app.join.form.as_mut().unwrap().next_try = Some(Instant::now());
        assert!(app.join_tick().await.unwrap());
        next_join_step(&mut app).await;
        // Not on the screen and nothing else open: bzz opens it.
        assert!(app.join.form.is_none());
        assert!(app.runtime.is_some());
        app.shutdown().await;
        relay.stop();
    }

    #[tokio::test]
    async fn stopping_a_wait_keeps_or_drops_the_identity_as_the_person_says() {
        for keep in [true, false] {
            let temporary = TempDir::new().unwrap();
            let mut app = empty_app(&temporary).await;
            let relay = relay(Arc::new(AtomicBool::new(false)), false).await;
            join_with_a_new_identity(&mut app, &relay).await;
            app.join_key(key(KeyCode::Char('x'))).await.unwrap();
            assert_eq!(app.join.form.as_ref().unwrap().stage, Stage::Keep);
            assert!(screen(&mut app, 80, 24).contains("y keep it · n remove it"));
            app.join_key(key(KeyCode::Char(if keep { 'y' } else { 'n' })))
                .await
                .unwrap();
            assert!(app.join.form.is_none());
            assert_eq!(app.presentation.overlay, None);
            let saved = Config::load(&app.paths).unwrap();
            assert!(saved.communities.is_empty());
            assert_eq!(saved.identities.len(), usize::from(keep));
            let files = std::fs::read_dir(app.paths.keys_dir())
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert_eq!(files, usize::from(keep), "the key file goes with it");
            relay.stop();
        }
    }

    #[tokio::test]
    async fn a_refused_identity_is_said_and_nothing_half_made_stays() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let relay = relay(Arc::new(AtomicBool::new(false)), true).await;
        join_with_a_new_identity(&mut app, &relay).await;
        let Stage::Failed { what, .. } = &app.join.form.as_ref().unwrap().stage else {
            panic!("{:?}", app.join.form);
        };
        assert!(
            what.contains("refused this identity") && what.contains("banned"),
            "{what}"
        );
        let shown = screen(&mut app, 80, 24);
        assert!(shown.contains("k keeps it"), "{shown}");
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        assert_eq!(app.join.form.as_ref().unwrap().stage, Stage::Address);
        let saved = Config::load(&app.paths).unwrap();
        assert!(saved.communities.is_empty());
        assert!(saved.identities.is_empty(), "no orphan identity");
        relay.stop();
    }

    #[tokio::test]
    async fn an_address_where_nothing_answers_says_so_in_the_form() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        app.open_join(JoinStart::Address(format!("localhost:{port}")));
        next_join_step(&mut app).await;
        let shown = screen(&mut app, 80, 24);
        assert!(shown.contains("✗ nothing answers at localhost:"), "{shown}");
        assert!(shown.contains("check the address"), "{shown}");
        // Enter does not go on to the identity; it checks again.
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        assert_eq!(app.join.form.as_ref().unwrap().stage, Stage::Address);
        assert!(Config::load(&app.paths).unwrap().identities.is_empty());
    }

    #[tokio::test]
    async fn the_community_add_command_opens_the_form_and_a_bad_address_is_said() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        app.command = "community add".into();
        app.execute_command().await.unwrap();
        assert_eq!(app.presentation.overlay, Some(Overlay::Join));
        let shown = screen(&mut app, 80, 24);
        assert!(shown.contains("Join a community"), "{shown}");
        assert!(
            shown.contains("e.g. my-team.communities.buzz.xyz"),
            "{shown}"
        );
        type_text(&mut app, "ws://far.example").await;
        app.join_key(key(KeyCode::Enter)).await.unwrap();
        let shown = screen(&mut app, 80, 24);
        assert!(shown.contains("only on this machine"), "{shown}");
        // A paste goes into the field as one piece of text.
        app.join_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        app.handle_terminal_paste("my-team.example\n".into())
            .await
            .unwrap();
        assert_eq!(app.join.form.as_ref().unwrap().address, "my-team.example");
        app.join_key(key(KeyCode::Esc)).await.unwrap();
        assert!(app.join.form.is_none());
        assert_eq!(app.presentation.overlay, None);
    }

    #[tokio::test]
    async fn after_joining_bzz_lands_in_the_first_channel_or_offers_the_open_ones() {
        let temporary = TempDir::new().unwrap();
        let mut app = empty_app(&temporary).await;
        let relay = relay(Arc::new(AtomicBool::new(true)), false).await;
        join_with_a_new_identity(&mut app, &relay).await;
        let community = app.active_community_id().unwrap();
        assert_eq!(app.join.landing.map(|(id, _)| id), Some(community));
        // Nothing until the channels are known.
        assert!(!app.join_tick().await.unwrap());
        let channel = |name: &str, is_member: bool| crate::domain::Channel {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            about: String::new(),
            kind: crate::domain::ChannelKind::Stream,
            visibility: crate::domain::Visibility::Public,
            is_member,
            is_hidden: false,
            member_count: 3,
            last_event_at: None,
            facts: Default::default(),
        };
        app.channels = vec![channel("general", false), channel("random", false)];
        app.connection = crate::domain::ConnectionState::Online;
        assert!(app.join_tick().await.unwrap());
        assert_eq!(app.presentation.overlay, Some(Overlay::Finder));
        assert!(
            app.status_error
                .as_deref()
                .unwrap()
                .contains("choose one to open")
        );
        assert!(app.join.landing.is_none(), "once");
        app.shutdown().await;
        relay.stop();
    }
}
